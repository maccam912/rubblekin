//! A guest join screen. DNS, connection, and local-server startup run off the render thread.
use crate::{
    Avatars, GameEntity, Session, VoxelWorld,
    graphics::GraphicsQuality,
    network::{Connection, ConnectionStage},
    observer::ObserverCamera,
    prediction::Prediction,
    terrain::TerrainScene,
};
use bevy::{
    input::{
        keyboard::{Key, KeyboardInput},
        touch::{TouchInput, TouchPhase},
    },
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, EditableTextFilter, TextCursorStyle, TextEdit},
    window::{CursorGrabMode, CursorOptions, Ime, PrimaryWindow},
    winit::{RawWinitWindowEvent, converters::convert_keyboard_input},
};
use rubblekin_core::{
    physics::Body,
    protocol::{PROTOCOL_VERSION, ServerMessage, SessionMode},
    world::World as GameWorld,
};
use rubblekin_server::{ServerConfig, ServerHandle, spawn};
use std::{
    net::Ipv6Addr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Instant,
};
use winit::{event::WindowEvent as NativeWindowEvent, keyboard::ModifiersState};

#[derive(Resource)]
pub struct JoinScreen {
    address: String,
    name: String,
    status: String,
    config: ServerConfig,
    graphics: GraphicsQuality,
    mode: SessionMode,
    pending: Option<PendingJoin>,
    next_action: Option<Action>,
    local_server: Option<ServerHandle>,
}

#[derive(Clone, Copy)]
enum JoinStage {
    OpeningLocalWorld,
    Network(ConnectionStage),
    PreparingLandscape,
}

impl JoinStage {
    fn label(self) -> &'static str {
        match self {
            Self::OpeningLocalWorld => "Opening your local world…",
            Self::Network(ConnectionStage::ResolvingAddress) => "Looking up the server…",
            Self::Network(ConnectionStage::Connecting) => "Connecting to the server…",
            Self::Network(ConnectionStage::AwaitingWelcome) => "Waiting for the server's world…",
            Self::PreparingLandscape => "Preparing the landscape…",
        }
    }
}

struct PendingJoin {
    worker: JoinHandle<Result<Joined, String>>,
    stage: Arc<Mutex<JoinStage>>,
    cancelled: Arc<AtomicBool>,
    started: Instant,
}

impl PendingJoin {
    fn status(&self) -> String {
        let label = if self.cancelled.load(Ordering::Relaxed) {
            "Cancelling… waiting for the current step to finish."
        } else {
            self.stage
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .label()
        };
        format!("{label}\n{} s elapsed", self.started.elapsed().as_secs())
    }
}

struct Joined {
    connection: Connection,
    world: GameWorld,
    session: Session,
    server: Option<ServerHandle>,
}

impl JoinScreen {
    pub fn new(
        address: String,
        name: String,
        config: ServerConfig,
        graphics: GraphicsQuality,
        mode: SessionMode,
    ) -> Self {
        Self {
            address,
            name,
            config,
            graphics,
            mode,
            status: "Join a shared world, or explore your local island.".into(),
            pending: None,
            next_action: None,
            local_server: None,
        }
    }

    pub fn start(&mut self, local: bool) {
        if self.pending.is_some() {
            return;
        }
        let name = match validate_name(&self.name) {
            Ok(name) => name,
            Err(error) => {
                self.status = error.into();
                return;
            }
        };
        let address = if local {
            String::new()
        } else {
            match validate_address(&self.address) {
                Ok(address) => address,
                Err(error) => {
                    self.status = error.into();
                    return;
                }
            }
        };
        let stage = Arc::new(Mutex::new(if local {
            JoinStage::OpeningLocalWorld
        } else {
            JoinStage::Network(ConnectionStage::ResolvingAddress)
        }));
        self.status = stage.lock().unwrap().label().into();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_stage = stage.clone();
        let worker_cancelled = cancelled.clone();
        let config = self.config.clone();
        let mode = self.mode;
        let graphics = self.graphics;
        let worker = std::thread::spawn(move || {
            let report = |stage| {
                if worker_cancelled.load(Ordering::Relaxed) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "Join cancelled",
                    ));
                }
                *worker_stage
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = stage;
                Ok(())
            };
            report(if local {
                JoinStage::OpeningLocalWorld
            } else {
                JoinStage::Network(ConnectionStage::ResolvingAddress)
            })
            .map_err(|error| error.to_string())?;
            let server = if local {
                Some(spawn(config).map_err(|error| error.to_string())?)
            } else {
                None
            };
            let address = if let Some(server) = &server {
                let mut address = server.addr;
                if address.ip().is_unspecified() {
                    address.set_ip(if address.is_ipv6() {
                        std::net::Ipv6Addr::LOCALHOST.into()
                    } else {
                        std::net::Ipv4Addr::LOCALHOST.into()
                    });
                }
                address.to_string()
            } else {
                address
            };
            let (connection, welcome) =
                Connection::connect_with_progress(&address, name, mode, |stage| {
                    report(JoinStage::Network(stage))
                })
                .map_err(|error| error.to_string())?;
            // Global geography is prepared on this worker, keeping the menu
            // responsive even on a first join to a new seed.
            report(JoinStage::PreparingLandscape).map_err(|error| error.to_string())?;
            let (world, session) = session_from_welcome(welcome, address, graphics, 0.0, mode)?;
            // Cancelled local servers and sockets are dropped on this worker,
            // preserving final-save cleanup without blocking menu input.
            report(JoinStage::PreparingLandscape).map_err(|error| error.to_string())?;
            Ok(Joined {
                connection,
                world,
                session,
                server,
            })
        });
        self.pending = Some(PendingJoin {
            worker,
            stage,
            cancelled,
            started: Instant::now(),
        });
    }

    fn cancel(&mut self) {
        if let Some(pending) = &self.pending {
            pending.cancelled.store(true, Ordering::Relaxed);
            self.status = pending.status();
            self.next_action = None;
        }
    }
}

fn validate_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 24 || name.chars().any(char::is_control) {
        return Err("Enter a display name of 1–24 characters.");
    }
    Ok(name.into())
}

fn validate_address(address: &str) -> Result<String, &'static str> {
    let address = address.trim();
    let help = "Enter a server as host:port, for example 127.0.0.1:7878.";
    if address.len() > 260 || address.chars().any(char::is_whitespace) || address.contains('/') {
        return Err(help);
    }
    let (host, port) = address.rsplit_once(':').ok_or(help)?;
    if host.is_empty() || port.parse::<u16>().ok().is_none_or(|port| port == 0) {
        return Err(help);
    }
    if host.contains(':') || host.contains(['[', ']']) {
        host.strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .and_then(|host| host.parse::<Ipv6Addr>().ok())
            .ok_or("Use [IPv6-address]:port for IPv6 servers.")?;
    }
    Ok(address.into())
}

#[derive(Component)]
pub(super) struct MenuRoot;
#[derive(Component)]
pub(super) struct MenuCamera;
#[derive(Component)]
pub(super) struct MenuStatus;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    Address,
    Name,
}
#[derive(Component, Clone, Copy)]
pub(super) enum Action {
    Join,
    Local,
    Cancel,
    Mode(SessionMode),
    DismissKeyboard,
}

fn ink() -> Color {
    Color::srgb(0.89, 0.92, 0.85)
}
fn accent() -> Color {
    Color::srgb(0.90, 0.73, 0.42)
}

#[derive(Component)]
pub(super) struct MenuPanel;
#[derive(Component)]
pub(super) struct MenuContent;
#[derive(Component)]
pub(super) struct MenuColumn;
#[derive(Component)]
pub(super) struct KeyboardDone;

pub fn setup(
    mut commands: Commands,
    mut fonts: ResMut<Assets<Font>>,
    menu: Res<JoinScreen>,
    touch: Option<Res<crate::touch::TouchControls>>,
) {
    let touch = touch.is_some_and(|touch| touch.enabled);
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    let camera = commands.spawn((Camera2d, MenuCamera)).id();
    commands.spawn((
        MenuRoot,
        UiTargetCamera(camera),
        Node {
            width: percent(100), height: percent(100),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(if touch { 16 } else { 24 })),
            overflow: Overflow::scroll_y(), ..default()
        },
        ScrollPosition::default(),
        BackgroundColor(Color::srgb(0.055, 0.10, 0.10)),
    )).with_children(|root| {
        root.spawn((
            MenuPanel,
            Node {
                width: px(560), max_width: percent(100), flex_shrink: 0.,
                padding: UiRect::all(px(if touch { 16 } else { 32 })),
                flex_direction: FlexDirection::Column, row_gap: px(if touch { 10 } else { 16 }),
                border_radius: BorderRadius::all(px(12)), ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.15, 0.15)),
        )).with_children(|panel| {
            panel.spawn((Node { align_items: AlignItems::Center, justify_content: JustifyContent::SpaceBetween, column_gap: px(8), ..default() },)).with_children(|header| {
                header.spawn((Text::new("R U B B L E K I N"), TextFont::from_font_size(if touch { 24. } else { 32. }).with_font(font.clone()), TextColor(ink())));
                header.spawn((Button, Action::DismissKeyboard, KeyboardDone, Node { display: Display::None, min_height: px(44), padding: UiRect::axes(px(14), px(8)), border_radius: BorderRadius::all(px(6)), ..default() }, BackgroundColor(Color::srgb(0.19, 0.34, 0.31))))
                    .with_child((Text::new("Done typing"), TextFont::from_font_size(16.).with_font(font.clone()), TextColor(ink())));
            });
            panel.spawn((Text::new("A LIVING WORLD  /  EARLY PROTOTYPE"), TextFont::from_font_size(14.).with_font(font.clone()), TextColor(accent())));
            panel.spawn((MenuContent, Node { flex_direction: FlexDirection::Column, row_gap: px(16), column_gap: px(24), ..default() },)).with_children(|content| {
                content.spawn((MenuColumn, Node { flex_direction: FlexDirection::Column, row_gap: px(if touch { 8 } else { 12 }), min_width: px(0), ..default() },)).with_children(|fields| {
                    fields.spawn((Text::new("Choose your server"), TextFont::from_font_size(if touch { 20. } else { 24. }).with_font(font.clone()), TextColor(ink())));
                    for (field, label, value, max) in [(Field::Address, "SERVER ADDRESS", menu.address.as_str(), 260), (Field::Name, "DISPLAY NAME", menu.name.as_str(), 24)] {
                        fields.spawn((Text::new(label), TextFont::from_font_size(14.).with_font(font.clone()), TextColor(accent())));
                        fields.spawn((
                            field, Interaction::default(),
                            Node { width: percent(100), min_height: px(48), padding: UiRect::all(px(10)), border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(5)), overflow: Overflow::clip_x(), ..default() },
                            EditableText { max_characters: Some(max), ..EditableText::new(value) },
                            EditableTextFilter::new(|ch| !ch.is_control()), TextLayout::no_wrap(),
                            TextCursorStyle { color: ink(), selection_color: Color::srgb(0.23, 0.43, 0.42), unfocused_selection_color: Color::NONE, ..default() },
                            TextFont::from_font_size(21.).with_font(font.clone()), TextColor(ink()),
                            BackgroundColor(Color::srgb(0.04, 0.08, 0.08)), BorderColor::all(Color::srgb(0.20, 0.32, 0.30)),
                        ));
                    }
                });
                content.spawn((MenuColumn, Node { flex_direction: FlexDirection::Column, row_gap: px(if touch { 8 } else { 12 }), min_width: px(0), ..default() },)).with_children(|actions| {
                    actions.spawn((Node { column_gap: px(8), flex_wrap: FlexWrap::Wrap, row_gap: px(8), ..default() },)).with_children(|row| {
                        for (mode, label) in [(SessionMode::Player, "Play as explorer"), (SessionMode::Observer, "Observe as admin")] {
                            row.spawn((Button, Action::Mode(mode), Node { min_height: px(48), padding: UiRect::axes(px(12), px(10)), align_items: AlignItems::Center, border_radius: BorderRadius::all(px(6)), ..default() }, BackgroundColor(Color::srgb(0.19, 0.34, 0.31))))
                                .with_child((Text::new(label), TextFont::from_font_size(if touch { 16. } else { 18. }).with_font(font.clone()), TextColor(ink())));
                        }
                    });
                    actions.spawn((Text::new("Observer camera is read-only; the server must allow observers."), TextFont::from_font_size(14.).with_font(font.clone()), TextColor(Color::srgb(0.62, 0.74, 0.69))));
                    actions.spawn((Node { column_gap: px(8), flex_wrap: FlexWrap::Wrap, row_gap: px(8), ..default() },)).with_children(|row| {
                        for (action, label) in [(Action::Join, "Join server"), (Action::Local, "Local world"), (Action::Cancel, "Cancel")] {
                            row.spawn((Button, action, Node { display: if matches!(action, Action::Cancel) { Display::None } else { Display::Flex }, min_height: px(48), padding: UiRect::axes(px(20), px(11)), align_items: AlignItems::Center, border_radius: BorderRadius::all(px(6)), ..default() }, BackgroundColor(Color::srgb(0.19, 0.34, 0.31))))
                                .with_child((Text::new(label), TextFont::from_font_size(19.).with_font(font.clone()), TextColor(ink())));
                        }
                    });
                    actions.spawn((Text::new(menu.status.clone()), TextFont::from_font_size(17.).with_font(font.clone()), TextColor(ink()), MenuStatus, Node { min_height: px(42), ..default() }));
                    actions.spawn((Text::new(if touch { "Tap a field to type · swipe to scroll
Guest access · no account or password" } else { "Guest access · no account or password\nTab switches fields · Enter joins · F10 leaves a world" }), TextFont::from_font_size(14.).with_font(font.clone()), TextColor(Color::srgb(0.62, 0.74, 0.69))));
                });
            });
        });
    });
}

// macOS can attach modifier flags to synthesized key events without emitting
// separate modifier key presses. Bevy's ButtonInput does not expose those flags.
#[derive(Message)]
pub(super) struct MenuKey {
    pub(super) input: KeyboardInput,
    pub(super) modifiers: ModifiersState,
}

pub fn native_input(
    mut events: MessageReader<RawWinitWindowEvent>,
    mut modifiers: Local<ModifiersState>,
    mut keys: MessageWriter<MenuKey>,
    window: Single<Entity, With<PrimaryWindow>>,
) {
    for event in events.read() {
        match &event.event {
            NativeWindowEvent::ModifiersChanged(value) => *modifiers = value.state(),
            NativeWindowEvent::Focused(false) => *modifiers = ModifiersState::empty(),
            NativeWindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => {
                keys.write(MenuKey {
                    input: convert_keyboard_input(event, *window),
                    modifiers: *modifiers,
                });
            }
            _ => {}
        }
    }
}

// The existing Bevy text editor owns selection, Unicode editing, and clipboard
// operations. This maps the two fields' input without a second UI framework.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn interact(
    mut menu: ResMut<JoinScreen>,
    session: Option<Res<Session>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard: MessageReader<MenuKey>,
    mut ime: MessageReader<Ime>,
    mut composing: Local<bool>,
    touch: Option<Res<crate::touch::TouchControls>>,
    mut focus: ResMut<InputFocus>,
    mut fields: Query<(Entity, &Field, &Interaction, &mut EditableText)>,
    actions: Query<(&Action, &Interaction), Changed<Interaction>>,
    mut fingers: MessageReader<TouchInput>,
    targets: Query<(
        Entity,
        &ComputedNode,
        &UiGlobalTransform,
        &Node,
        Option<&InheritedVisibility>,
        Option<&Field>,
        Option<&Action>,
    )>,
    windows: Query<&Window, With<PrimaryWindow>>,
    clipping: Query<(&ComputedNode, &UiGlobalTransform, &Node)>,
    parents: Query<&ChildOf, Without<bevy::ui::OverrideClip>>,
) {
    if session.is_some() {
        keyboard.clear();
        ime.clear();
        fingers.clear();
        return;
    }
    // Text edits commit in PostUpdate. Submit the previous frame's action only
    // after reading those committed values (including paste followed by Enter).
    let requested = menu.next_action.take();
    let mut touched_action = None;
    // Resolve original Started events directly: brief Android taps must work
    // independently of the UI cursor/Interaction timing at low frame rates.
    // Rendered node geometry and inherited clipping remain the hit boundaries.
    for finger in fingers
        .read()
        .filter(|event| event.phase == TouchPhase::Started)
    {
        let Ok(window) = windows.get(finger.window) else {
            continue;
        };
        if finger.position.y < 0. || finger.position.y > visible_height(window) {
            continue;
        }
        let point = finger.position * window.scale_factor();
        for (entity, computed, transform, node, visibility, field, action) in &targets {
            if node.display == Display::None
                || visibility.is_some_and(|visible| !visible.get())
                || !computed.contains_point(*transform, point)
                || !bevy::ui::clip_check_recursive(point, entity, &clipping, &parents)
            {
                continue;
            }
            if field.is_some() && menu.pending.is_none() {
                focus.set(entity, FocusCause::Navigated);
                #[cfg(target_os = "android")]
                if let Some(app) = bevy::android::ANDROID_APP.get() {
                    app.show_soft_input(false);
                }
                break;
            }
            if let Some(action) = action {
                touched_action = Some(*action);
                break;
            }
        }
    }
    if menu.pending.is_some() {
        let back = keyboard.read().any(|key| {
            key.input.state.is_pressed()
                && matches!(key.input.logical_key, Key::Escape | Key::BrowserBack)
        });
        ime.clear();
        if back
            || keys.just_pressed(KeyCode::Escape)
            || matches!(touched_action, Some(Action::Cancel))
            || (!cfg!(target_os = "android")
                && actions.iter().any(|(action, interaction)| {
                    matches!(action, Action::Cancel) && *interaction == Interaction::Pressed
                }))
        {
            menu.cancel();
        }
        return;
    }
    for (entity, _, interaction, _) in &fields {
        if !cfg!(target_os = "android") && *interaction == Interaction::Pressed {
            focus.set(entity, FocusCause::Navigated);
        }
    }
    let touch = touch.is_some_and(|touch| touch.enabled);
    if (!touch && focus.get().is_none()) || keys.just_pressed(KeyCode::Tab) {
        let old = focus.get();
        if let Some((entity, _, _, _)) =
            fields.iter().find(|(entity, _, _, _)| Some(*entity) != old)
        {
            focus.set(entity, FocusCause::Navigated);
        }
    }
    let mut ime_handled_text = *composing;
    for event in ime.read() {
        let edit = match event {
            Ime::Preedit { value, cursor, .. } => {
                *composing = !value.is_empty();
                ime_handled_text |= *composing;
                Some(TextEdit::ImeSetCompose {
                    value: value.as_str().into(),
                    cursor: cursor
                        .map(|(anchor, focus)| bevy::text::PreeditCursor { anchor, focus }),
                })
            }
            Ime::Commit { value, .. } => {
                *composing = false;
                ime_handled_text = true;
                Some(TextEdit::ImeCommit {
                    value: value.as_str().into(),
                })
            }
            Ime::Disabled { .. } => {
                *composing = false;
                Some(TextEdit::clear_ime_compose())
            }
            Ime::Enabled { .. } => None,
        };
        if let Some(edit) = edit
            && let Some(entity) = focus.get()
            && let Ok((_, _, _, mut input)) = fields.get_mut(entity)
        {
            input.queue_edit(edit);
        }
    }
    let mut dismiss_keyboard = false;
    // Each event retains the modifier flags it had when Winit received it,
    // including complete press/release chords arriving in a single frame.
    for key in keyboard.read() {
        let event = &key.input;
        if !event.state.is_pressed() {
            continue;
        }
        if event.logical_key == Key::BrowserBack && touch {
            dismiss_keyboard = true;
            continue;
        }
        // Android GameTextInput owns the complete soft-keyboard buffer. Applying
        // its accompanying key events here would insert/delete text twice.
        if cfg!(target_os = "android") {
            if event.logical_key == Key::Enter {
                dismiss_keyboard = true;
            }
            continue;
        }
        let shortcut = key.modifiers.control_key() || key.modifiers.super_key();
        let shift = key.modifiers.shift_key();
        let Some(edit) = text_edit(event, shortcut, shift) else {
            continue;
        };
        if ime_handled_text && matches!(edit, TextEdit::Insert(_)) && !shortcut {
            continue;
        }
        if let Some(entity) = focus.get()
            && let Ok((_, _, _, mut input)) = fields.get_mut(entity)
        {
            input.queue_edit(edit);
        }
    }
    for (_, field, _, input) in &fields {
        match field {
            Field::Address => menu.address = input.value().to_string(),
            Field::Name => menu.name = input.value().to_string(),
        }
    }
    let action = touched_action
        .or_else(|| {
            // Android menus use the event path exclusively, so a synthesized mouse
            // Interaction cannot toggle a mode or submit the form a second time.
            (!cfg!(target_os = "android"))
                .then(|| {
                    actions
                        .iter()
                        .find(|(_, interaction)| **interaction == Interaction::Pressed)
                        .map(|(action, _)| *action)
                })
                .flatten()
        })
        .or_else(|| {
            (!cfg!(target_os = "android") && !*composing && keys.just_pressed(KeyCode::Enter))
                .then_some(Action::Join)
        })
        .or_else(|| dismiss_keyboard.then_some(Action::DismissKeyboard));
    if let Some(Action::Mode(mode)) = action {
        menu.mode = mode;
    } else {
        menu.next_action = action;
    }
    if let Some(action) = requested {
        menu.next_action = None;
        focus.clear();
        if matches!(action, Action::Join | Action::Local) {
            menu.start(matches!(action, Action::Local));
        }
    }
}

#[derive(Default)]
pub(super) struct MenuScroll {
    finger: Option<(u64, f32, f32)>,
    focused: Option<Entity>,
    height: f32,
    settle: u8,
}

// Winit does not forward Android content-rect/keyboard insets, so query the
// activity's visible rectangle as well as the current window dimensions.
fn visible_height(window: &Window) -> f32 {
    #[cfg(target_os = "android")]
    if let Some(app) = bevy::android::ANDROID_APP.get() {
        let rect = app.content_rect();
        if rect.bottom > rect.top {
            return window
                .height()
                .min(rect.bottom as f32 / window.scale_factor());
        }
    }
    window.height()
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn layout(
    menu: Res<JoinScreen>,
    session: Option<Res<Session>>,
    touch: Option<Res<crate::touch::TouchControls>>,
    touches: Option<Res<Touches>>,
    wheel: Option<Res<bevy::input::mouse::AccumulatedMouseScroll>>,
    focus: Res<InputFocus>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut scroll_state: Local<MenuScroll>,
    fields: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<Field>>,
    mut nodes: ParamSet<(
        Query<(&mut Node, &mut ScrollPosition, &ComputedNode), With<MenuRoot>>,
        Query<&mut Node, With<MenuPanel>>,
        Query<&mut Node, With<MenuContent>>,
        Query<&mut Node, With<MenuColumn>>,
        Query<&mut Node, With<KeyboardDone>>,
    )>,
) {
    let touch = touch.is_some_and(|touch| touch.enabled);
    let editing = session.is_none()
        && menu.pending.is_none()
        && focus.get().is_some_and(|entity| fields.contains(entity));
    window.ime_enabled = editing;
    if session.is_some() {
        return;
    }
    let height = visible_height(&window);
    let wide = touch && window.width() >= 650.;
    for mut panel in &mut nodes.p1() {
        panel.width = px(if wide {
            (window.width() - 32.).min(900.)
        } else {
            560.
        });
    }
    for mut content in &mut nodes.p2() {
        content.flex_direction = if wide {
            FlexDirection::Row
        } else {
            FlexDirection::Column
        };
    }
    for mut column in &mut nodes.p3() {
        column.flex_grow = if wide { 1. } else { 0. };
        column.flex_basis = if wide { percent(50) } else { Val::Auto };
    }
    for mut done in &mut nodes.p4() {
        done.display = if touch && editing {
            Display::Flex
        } else {
            Display::None
        };
    }
    if scroll_state.focused != focus.get() || (scroll_state.height - height).abs() > 1. {
        scroll_state.focused = focus.get();
        scroll_state.height = height;
        scroll_state.settle = 2;
    }
    for (mut root, mut scroll, computed) in &mut nodes.p0() {
        root.height = px(height);
        root.justify_content = if height < 500. || touch {
            JustifyContent::FlexStart
        } else {
            JustifyContent::Center
        };
        let max_scroll =
            (computed.content_size.y - computed.size.y) * computed.inverse_scale_factor;
        if let Some(wheel) = &wheel {
            scroll.0.y -= wheel.delta.y * 28.;
        }
        if let Some(touches) = &touches {
            if scroll_state.finger.is_none()
                && let Some(finger) = touches.iter_just_pressed().next()
            {
                scroll_state.finger = Some((finger.id(), finger.position().y, finger.position().y));
            }
            if let Some((id, start, previous)) = scroll_state.finger {
                if let Some(finger) = touches.get_pressed(id) {
                    let y = finger.position().y;
                    if (y - start).abs() > 12. {
                        scroll.0.y += previous - y;
                    }
                    scroll_state.finger = Some((id, start, y));
                } else {
                    scroll_state.finger = None;
                }
            }
        }
        // Re-layout twice before bringing a focused field above the keyboard.
        if scroll_state.settle > 0 {
            scroll_state.settle -= 1;
            if scroll_state.settle == 0
                && editing
                && let Some(entity) = focus.get()
                && let Ok((_, field, transform)) = fields.get(entity)
            {
                let center = transform.translation.y * field.inverse_scale_factor;
                let half = field.size.y * field.inverse_scale_factor * 0.5;
                if center - half < 12. {
                    scroll.0.y += center - half - 12.;
                }
                if center + half > height - 12. {
                    scroll.0.y += center + half - height + 12.;
                }
            }
        }
        scroll.0.y = scroll.0.y.clamp(0., max_scroll.max(0.));
    }
}

#[cfg(target_os = "android")]
#[derive(Default)]
pub(super) struct AndroidEditor {
    focused: Option<Entity>,
    text: String,
    selection: (usize, usize),
    pending: Option<PendingNativeEdit>,
}

#[cfg(any(test, target_os = "android"))]
struct PendingNativeEdit {
    previous_text: String,
    previous_selection: (usize, usize),
    sent: std::time::Instant,
}

#[cfg(any(test, target_os = "android"))]
impl PendingNativeEdit {
    fn is_stale(
        &self,
        incoming: &str,
        selection: (usize, usize),
        expected: &str,
        expected_selection: (usize, usize),
        now: std::time::Instant,
    ) -> bool {
        now.saturating_duration_since(self.sent) < std::time::Duration::from_millis(250)
            && incoming == self.previous_text
            && selection == self.previous_selection
            && (incoming != expected || selection != expected_selection)
    }
}

#[cfg(target_os = "android")]
fn push_android_text(
    app: &winit::platform::android::activity::AndroidApp,
    state: &mut AndroidEditor,
    text: String,
    start: usize,
    end: usize,
) {
    use winit::platform::android::activity::input::{TextInputState, TextSpan};
    let previous = app.text_input_state();
    state.pending = Some(PendingNativeEdit {
        previous_text: previous.text,
        previous_selection: (previous.selection.start, previous.selection.end),
        sent: std::time::Instant::now(),
    });
    state.text.clone_from(&text);
    state.selection = (start, end);
    app.set_text_input_state(TextInputState {
        text,
        selection: TextSpan { start, end },
        compose_region: None,
    });
}

#[cfg(any(test, target_os = "android"))]
fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units >= offset {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

#[cfg(any(test, target_os = "android"))]
fn clean_android_text(text: &str, limit: usize) -> String {
    text.chars()
        .filter(|ch| !ch.is_control())
        .take(limit)
        .collect()
}

// Winit 0.30's Android backend discards GameActivity TextEvent, including
// composition and deletion. Read the retained GameTextInput buffer directly.
// It contains the entire field; treating it as an insertion duplicates text.
#[cfg(target_os = "android")]
pub fn android_text_input(
    menu: Res<JoinScreen>,
    session: Option<Res<Session>>,
    focus: Res<InputFocus>,
    mut fields: Query<(&Interaction, &mut EditableText), With<Field>>,
    mut state: Local<AndroidEditor>,
    mut font: ResMut<bevy::text::FontCx>,
    mut layout: ResMut<bevy::text::LayoutCx>,
) {
    use winit::platform::android::activity::input::{ImeOptions, InputType, TextInputAction};
    let Some(app) = bevy::android::ANDROID_APP.get() else {
        return;
    };
    let focused = focus
        .get()
        .filter(|entity| session.is_none() && menu.pending.is_none() && fields.contains(*entity));
    if focused.is_none() {
        if state.focused.take().is_some() {
            app.hide_soft_input(false);
        }
        return;
    }
    let entity = focused.unwrap();
    let Ok((interaction, mut input)) = fields.get_mut(entity) else {
        return;
    };
    let value = input.value().to_string();
    if state.focused != Some(entity) {
        state.focused = Some(entity);
        input.pending_edits.clear();
        input
            .editor_mut()
            .driver(&mut font, &mut layout)
            .select_byte_range(0, value.len());
        state.text.clone_from(&value);
        let end = value.encode_utf16().count();
        state.selection = (0, end);
        app.set_ime_editor_info(
            InputType::TYPE_CLASS_TEXT | InputType::TYPE_TEXT_FLAG_NO_SUGGESTIONS,
            TextInputAction::Done,
            ImeOptions::IME_FLAG_NO_FULLSCREEN | ImeOptions::IMG_FLAG_NO_EXTRACT_UI,
        );
        // Selecting the existing value makes replacement possible with only
        // the soft keyboard; there is no native visible EditText context menu.
        push_android_text(app, &mut state, value, 0, end);
        app.show_soft_input(false);
        return;
    }
    if *interaction == Interaction::Pressed {
        app.show_soft_input(false);
    }
    let incoming = app.text_input_state();
    let selection = (incoming.selection.start, incoming.selection.end);
    // Sending a new field/selection crosses the Java event queue. Until its
    // echo arrives, the retained native buffer can still describe the old field.
    if state.pending.as_ref().is_some_and(|pending| {
        pending.is_stale(
            &incoming.text,
            selection,
            &state.text,
            state.selection,
            std::time::Instant::now(),
        )
    }) {
        return;
    }
    state.pending = None;
    if incoming.text != state.text || selection != state.selection {
        let text = clean_android_text(&incoming.text, input.max_characters.unwrap_or(260));
        let start = utf16_to_byte(&text, selection.0);
        let end = utf16_to_byte(&text, selection.1);
        input.pending_edits.clear();
        input.editor_mut().set_text(&text);
        input
            .editor_mut()
            .driver(&mut font, &mut layout)
            .select_byte_range(start, end);
        if text != incoming.text {
            let start = text[..start].encode_utf16().count();
            let end = text[..end].encode_utf16().count();
            push_android_text(app, &mut state, text.clone(), start, end);
        } else {
            state.selection = selection;
        }
        state.text = text;
    } else {
        // Forward selection changes made by tapping the Bevy text field.
        let selection = input.editor().raw_selection();
        let start = value[..selection.anchor().index().min(value.len())]
            .encode_utf16()
            .count();
        let end = value[..selection.focus().index().min(value.len())]
            .encode_utf16()
            .count();
        if value != state.text || (start, end) != state.selection {
            push_android_text(app, &mut state, value, start, end);
        }
    }
}

#[cfg(not(target_os = "android"))]
pub fn android_text_input() {}

fn text_edit(event: &KeyboardInput, shortcut: bool, shift: bool) -> Option<TextEdit> {
    if shortcut {
        return match event.key_code {
            KeyCode::KeyA => Some(TextEdit::SelectAll),
            KeyCode::KeyC => Some(TextEdit::Copy),
            KeyCode::KeyX => Some(TextEdit::Cut),
            KeyCode::KeyV => Some(TextEdit::Paste),
            KeyCode::ArrowLeft => Some(TextEdit::WordLeft(shift)),
            KeyCode::ArrowRight => Some(TextEdit::WordRight(shift)),
            KeyCode::Backspace => Some(TextEdit::BackspaceWord),
            KeyCode::Delete => Some(TextEdit::DeleteWord),
            _ => None,
        };
    }
    match event.logical_key {
        Key::Backspace => Some(TextEdit::Backspace),
        Key::Delete => Some(TextEdit::Delete),
        Key::ArrowLeft => Some(TextEdit::Left(shift)),
        Key::ArrowRight => Some(TextEdit::Right(shift)),
        Key::Home => Some(TextEdit::TextStart(shift)),
        Key::End => Some(TextEdit::TextEnd(shift)),
        _ => event
            .text
            .as_ref()
            .filter(|text| !text.chars().any(char::is_control))
            .map(|text| TextEdit::Insert(text.clone())),
    }
}

pub fn poll_connection(
    mut commands: Commands,
    mut menu: ResMut<JoinScreen>,
    mut focus: ResMut<InputFocus>,
    time: Res<Time>,
    touch: Option<Res<crate::touch::TouchControls>>,
) {
    let Some(pending) = &menu.pending else {
        return;
    };
    if !pending.worker.is_finished() {
        menu.status = pending.status();
        return;
    }
    let pending = menu.pending.take().unwrap();
    let result = pending
        .worker
        .join()
        .unwrap_or_else(|_| Err("Connection worker stopped unexpectedly.".into()));
    if pending.cancelled.load(Ordering::Relaxed) {
        if let Ok(joined) = result {
            // Cancel can win after the worker's final cancellation check. Keep
            // retry disabled until its completed local server has shut down,
            // and keep that potentially blocking save off the render thread.
            menu.pending = Some(PendingJoin {
                worker: std::thread::spawn(move || {
                    drop(joined);
                    Err("Join cancelled".into())
                }),
                ..pending
            });
        } else {
            menu.status = "Join cancelled. Choose a server or local world to try again.".into();
        }
        return;
    }
    match result {
        Ok(mut joined) => {
            joined.session.status_until = time.elapsed_secs_f64() + 12.0;
            joined.session.airship_clock = crate::airships::AirshipClock::new(
                joined.session.world_time,
                time.elapsed_secs_f64(),
            );
            if touch.is_some_and(|touch| touch.enabled) {
                joined.session.help = false;
                joined.session.inspector = false;
                joined.session.status = if joined.session.observer.is_some() {
                    "Swipe to look · use the stick to fly · Menu returns to servers".into()
                } else {
                    "Swipe to look · use the stick to explore · changes save automatically".into()
                };
            }
            menu.local_server = joined.server;
            commands.insert_resource(joined.connection);
            commands.insert_resource(VoxelWorld(joined.world));
            commands.insert_resource(joined.session);
            focus.clear();
        }
        Err(error) => {
            menu.status = format!("Could not join: {error}\nCheck the address and try again.")
        }
    }
}

pub(crate) fn session_from_welcome(
    welcome: ServerMessage,
    address: String,
    graphics: GraphicsQuality,
    now: f64,
    requested_mode: SessionMode,
) -> Result<(GameWorld, Session), String> {
    let ServerMessage::Welcome {
        version,
        session_id,
        mode,
        seed,
        generation,
        edits,
        players,
        npc,
        residents,
        villages,
        world_time,
        can_admin,
    } = welcome
    else {
        return Err("Server did not send a welcome message".into());
    };
    if version != PROTOCOL_VERSION {
        return Err("Server protocol version does not match this client".into());
    }
    if mode != requested_mode {
        return Err("Server returned a different session mode than requested".into());
    }
    let world = GameWorld::from_generation_edits(seed, generation, &edits)?;
    let mut own_players = players.iter().filter(|player| player.id == session_id);
    let own_player = own_players.next();
    let prediction = own_player.map_or_else(Prediction::default, Prediction::from_snapshot);
    let ride = own_player.and_then(|player| player.ride);
    let deck_position = own_player.and_then(|player| player.deck_position);
    let airships = rubblekin_core::airships::AirshipNetwork::new(&world);
    let (body, observer, status) = match mode {
        SessionMode::Player => {
            let player = own_player.ok_or("Server did not provide your player avatar")?;
            if own_players.next().is_some() {
                return Err("Server provided multiple avatars for your session".into());
            }
            (
                player.body.clone(),
                None,
                "Welcome to the world. Click to explore; F10 returns to the server screen.",
            )
        }
        SessionMode::Observer => {
            if own_player.is_some() || can_admin {
                return Err("Server returned an invalid read-only observer session".into());
            }
            let spawn = world.spawn_position();
            (
                Body::new(spawn),
                Some(ObserverCamera::new(spawn)),
                "Observing the world without an avatar. Click to fly; F10 returns to the server screen.",
            )
        }
    };
    Ok((
        world,
        Session {
            id: session_id,
            body,
            observer,
            yaw: -0.45,
            pitch: 0.12,
            camera_distance: 6.5,
            selected: 3,
            flying: false,
            captured: false,
            can_admin,
            inspector: true,
            inspected: None,
            inspect_requested: true,
            help: true,
            graphics,
            npc,
            residents,
            villages,
            players,
            world_time,
            airships,
            ride,
            deck_position,
            airship_clock: crate::airships::AirshipClock::new(world_time, now),
            status: status.into(),
            status_until: now + 12.0,
            target: None,
            fps: 0.0,
            edits: edits.len(),
            connected_to: address,
            prediction,
            edit_clock: 0.0,
            next_request: 1,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn leave_world(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    connection: Res<Connection>,
    session: Res<Session>,
    entities: Query<Entity, With<GameEntity>>,
    mut menu: ResMut<JoinScreen>,
    mut cursor: Single<&mut CursorOptions>,
    mut touch: Option<ResMut<crate::touch::TouchControls>>,
    mut pause: Option<ResMut<crate::pause::PauseMenu>>,
    mut console: Option<ResMut<crate::admin_console::AdminConsole>>,
    mut map: Option<ResMut<crate::world_map::WorldMap>>,
) {
    let menu_leave = pause.as_ref().is_some_and(|pause| pause.leave);
    let leave = touch.as_mut().is_some_and(|touch| {
        let leave = touch.leave;
        touch.leave = false;
        if leave {
            touch.menu_open = false;
        }
        leave
    });
    let keyboard_leave = keys.just_pressed(KeyCode::F10)
        && !console
            .as_ref()
            .is_some_and(|console| console.input_blocked);
    if !keyboard_leave && !leave && !menu_leave && connection.error.is_none() {
        return;
    }
    menu.status = connection.error.as_ref().map_or_else(
        || {
            if session.observer.is_some() {
                "You stopped observing the world.".into()
            } else {
                "You left the world. Accepted changes have been saved.".into()
            }
        },
        |error| format!("Disconnected: {error}"),
    );
    menu.graphics = session.graphics;
    if let Some(pause) = pause.as_mut() {
        **pause = crate::pause::PauseMenu::default();
    }
    if let Some(console) = console.as_mut() {
        **console = crate::admin_console::AdminConsole::default();
    }
    if let Some(map) = map.as_mut() {
        **map = crate::world_map::WorldMap::default();
    }
    if let Some(touch) = touch.as_mut() {
        touch.reset();
    }
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<Connection>();
    commands.remove_resource::<Session>();
    commands.remove_resource::<VoxelWorld>();
    commands.remove_resource::<TerrainScene>();
    commands.insert_resource(Avatars::default());
    menu.local_server.take();
    if !cfg!(target_os = "android") {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn refresh(
    menu: Res<JoinScreen>,
    session: Option<Res<Session>>,
    focus: Res<InputFocus>,
    mut root: Single<&mut Node, With<MenuRoot>>,
    mut camera: Single<&mut Camera, With<MenuCamera>>,
    mut status: Single<&mut Text, With<MenuStatus>>,
    mut fields: Query<(Entity, &mut BorderColor), With<Field>>,
    mut buttons: Query<(&Action, &Interaction, &mut BackgroundColor, &mut Node), Without<MenuRoot>>,
) {
    root.display = if session.is_some() {
        Display::None
    } else {
        Display::Flex
    };
    camera.is_active = session.is_none();
    if status.0 != menu.status {
        status.0.clone_from(&menu.status);
    }
    for (entity, mut border) in &mut fields {
        border.set_all(if focus.get() == Some(entity) && menu.pending.is_none() {
            accent()
        } else {
            Color::srgb(0.20, 0.32, 0.30)
        });
    }
    for (action, interaction, mut background, mut node) in &mut buttons {
        let cancel = matches!(action, Action::Cancel);
        if cancel {
            node.display = if menu
                .pending
                .as_ref()
                .is_some_and(|pending| !pending.cancelled.load(Ordering::Relaxed))
            {
                Display::Flex
            } else {
                Display::None
            };
        }
        background.0 = if menu.pending.is_some() && !cancel {
            Color::srgb(0.12, 0.22, 0.20)
        } else if matches!(action, Action::Mode(mode) if *mode == menu.mode) {
            Color::srgb(0.39, 0.43, 0.24)
        } else if *interaction == Interaction::Hovered {
            Color::srgb(0.27, 0.45, 0.39)
        } else {
            Color::srgb(0.19, 0.34, 0.31)
        };
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rubblekin_core::protocol::{NpcAction, NpcSnapshot, PlayerSnapshot};

    pub(crate) fn welcome(mode: SessionMode) -> ServerMessage {
        ServerMessage::Welcome {
            version: PROTOCOL_VERSION,
            session_id: 17,
            mode,
            seed: 42,
            generation: rubblekin_core::world::WorldGeneration::ValleyV1,
            edits: Vec::new(),
            players: if mode == SessionMode::Player {
                vec![PlayerSnapshot {
                    id: 17,
                    name: "Tester".into(),
                    body: Body::new([0.25, 2.52, 0.25]),
                    yaw: 0.0,
                    last_input_sequence: 0,
                    movement_epoch: 0,
                    ride: None,
                    deck_position: None,
                }]
            } else {
                Vec::new()
            },
            npc: NpcSnapshot {
                name: "Moss".into(),
                position: [3.0, 2.5, 0.0],
                hunger: 60.0,
                energy: 80.0,
                action: NpcAction::Forage,
                reason: "Looking for berries".into(),
                berries: 0,
                forced: false,
                target: None,
            },
            residents: Vec::new(),
            villages: Vec::new(),
            world_time: 0.0,
            can_admin: false,
        }
    }

    fn join(
        welcome: ServerMessage,
        requested: SessionMode,
    ) -> Result<(GameWorld, Session), String> {
        session_from_welcome(
            welcome,
            "test.example:7878".into(),
            GraphicsQuality::default(),
            0.0,
            requested,
        )
    }

    #[test]
    fn session_requires_the_requested_mode_and_exactly_the_right_avatar() {
        for mode in [SessionMode::Player, SessionMode::Observer] {
            let (_, session) = join(welcome(mode), mode).unwrap();
            assert_eq!(session.observer.is_some(), mode == SessionMode::Observer);
            assert_eq!(
                session.players.len(),
                usize::from(mode == SessionMode::Player)
            );
            assert_eq!(session.id, 17);
            let other_mode = if mode == SessionMode::Player {
                SessionMode::Observer
            } else {
                SessionMode::Player
            };
            assert!(
                join(welcome(mode), other_mode)
                    .err()
                    .unwrap()
                    .contains("different session mode")
            );
        }

        let mut missing = welcome(SessionMode::Player);
        if let ServerMessage::Welcome { players, .. } = &mut missing {
            players.clear();
        }
        assert!(
            join(missing, SessionMode::Player)
                .err()
                .unwrap()
                .contains("player avatar")
        );

        let mut duplicate = welcome(SessionMode::Player);
        if let ServerMessage::Welcome { players, .. } = &mut duplicate {
            players.push(players[0].clone());
        }
        assert!(
            join(duplicate, SessionMode::Player)
                .err()
                .unwrap()
                .contains("multiple avatars")
        );

        let mut embodied_observer = welcome(SessionMode::Player);
        if let ServerMessage::Welcome { mode, .. } = &mut embodied_observer {
            *mode = SessionMode::Observer;
        }
        assert!(
            join(embodied_observer, SessionMode::Observer)
                .err()
                .unwrap()
                .contains("invalid read-only")
        );

        let mut writable_observer = welcome(SessionMode::Observer);
        if let ServerMessage::Welcome { can_admin, .. } = &mut writable_observer {
            *can_admin = true;
        }
        assert!(
            join(writable_observer, SessionMode::Observer)
                .err()
                .unwrap()
                .contains("invalid read-only")
        );
    }

    #[test]
    fn server_addresses_require_a_host_and_real_port() {
        for address in [
            "127.0.0.1:7878",
            "test.example:30078",
            "[::1]:7878",
            " [2001:db8::1]:7878 ",
        ] {
            assert_eq!(validate_address(address).unwrap(), address.trim());
        }
        for address in [
            "",
            "test.example",
            ":7878",
            "host:0",
            "host:65536",
            "host:notaport",
            "https://host:7878",
            "host :7878",
            "::1:7878",
            "[invalid]:7878",
        ] {
            assert!(validate_address(address).is_err(), "{address}");
        }
    }

    #[test]
    fn display_names_preserve_unicode_and_match_the_server_limit() {
        assert_eq!(validate_name("  Violet 雪  ").unwrap(), "Violet 雪");
        assert!(validate_name(&"雪".repeat(24)).is_ok());
        for name in ["", "   ", "hello\nworld", &"x".repeat(25)] {
            assert!(validate_name(name).is_err());
        }
    }

    #[test]
    fn failed_validation_allows_another_attempt_without_starting_a_worker() {
        let mut menu = JoinScreen::new(
            "no port".into(),
            "Wayfarer".into(),
            ServerConfig::default(),
            GraphicsQuality::default(),
            SessionMode::Player,
        );
        menu.start(false);
        assert!(menu.pending.is_none());
        assert!(menu.status.contains("host:port"));
        menu.address = "localhost:7878".into();
        menu.name = " ".into();
        menu.start(false);
        assert!(menu.pending.is_none());
        assert!(menu.status.contains("display name"));
    }

    #[test]
    fn a_worker_failure_returns_to_the_same_join_form() {
        let mut app = App::new();
        let mut menu = JoinScreen::new(
            "test.example:7878".into(),
            "Wayfarer".into(),
            ServerConfig::default(),
            GraphicsQuality::default(),
            SessionMode::Player,
        );
        menu.pending = Some(PendingJoin {
            worker: std::thread::spawn(|| Err("Connection refused".into())),
            stage: Arc::new(Mutex::new(JoinStage::Network(ConnectionStage::Connecting))),
            cancelled: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
        });
        while !menu.pending.as_ref().unwrap().worker.is_finished() {
            std::thread::yield_now();
        }
        app.insert_resource(menu)
            .init_resource::<InputFocus>()
            .init_resource::<Time>()
            .add_systems(Update, poll_connection);
        app.update();
        let menu = app.world().resource::<JoinScreen>();
        assert!(menu.pending.is_none());
        assert!(menu.status.contains("Connection refused"));
        assert_eq!(menu.address, "test.example:7878");
        assert_eq!(menu.name, "Wayfarer");
        assert!(!app.world().contains_resource::<Session>());
    }

    #[test]
    fn progress_updates_while_waiting_and_cancel_keeps_retry_disabled_until_cleanup() {
        let (finish, finished) = std::sync::mpsc::channel();
        let stage = Arc::new(Mutex::new(JoinStage::OpeningLocalWorld));
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut menu = JoinScreen::new(
            "test.example:7878".into(),
            "Wayfarer".into(),
            ServerConfig::default(),
            GraphicsQuality::default(),
            SessionMode::Player,
        );
        menu.pending = Some(PendingJoin {
            worker: std::thread::spawn(move || {
                finished
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                Err("Join cancelled".into())
            }),
            stage: stage.clone(),
            cancelled: cancelled.clone(),
            started: Instant::now() - std::time::Duration::from_secs(3),
        });
        let mut app = App::new();
        app.insert_resource(menu)
            .init_resource::<InputFocus>()
            .init_resource::<Time>()
            .add_systems(Update, poll_connection);
        app.update();
        assert!(
            app.world()
                .resource::<JoinScreen>()
                .status
                .contains("Opening your local world")
        );
        *stage.lock().unwrap() = JoinStage::Network(ConnectionStage::AwaitingWelcome);
        app.update();
        assert!(
            app.world()
                .resource::<JoinScreen>()
                .status
                .contains("Waiting for the server's world")
        );
        assert!(
            app.world()
                .resource::<JoinScreen>()
                .status
                .contains("s elapsed")
        );
        app.world_mut().resource_mut::<JoinScreen>().cancel();
        assert!(cancelled.load(Ordering::Relaxed));
        app.world_mut().resource_mut::<JoinScreen>().start(false);
        assert!(Arc::ptr_eq(
            &app.world()
                .resource::<JoinScreen>()
                .pending
                .as_ref()
                .unwrap()
                .cancelled,
            &cancelled
        ));
        app.update();
        assert!(
            app.world()
                .resource::<JoinScreen>()
                .status
                .contains("Cancelling")
        );
        finish.send(()).unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while app.world().resource::<JoinScreen>().pending.is_some() && Instant::now() < deadline {
            app.update();
            std::thread::yield_now();
        }
        let menu = app.world().resource::<JoinScreen>();
        assert!(menu.pending.is_none());
        assert!(menu.status.contains("Join cancelled"));
        assert_eq!(menu.address, "test.example:7878");
        assert_eq!(menu.name, "Wayfarer");
    }

    #[test]
    fn cancelling_a_completed_local_join_never_enters_world_and_allows_retry() {
        let path = std::env::temp_dir().join(format!(
            "rubblekin-cancel-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut menu = JoinScreen::new(
            "remote.example:7878".into(),
            "Tester".into(),
            ServerConfig {
                bind_addr: "127.0.0.1:0".into(),
                save_path: path.clone(),
                seed: 42,
                generation: rubblekin_core::world::WorldGeneration::ValleyV1,
                allow_admin: true,
            },
            GraphicsQuality::default(),
            SessionMode::Player,
        );
        menu.start(true);
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while !menu.pending.as_ref().unwrap().worker.is_finished() && Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(menu.pending.as_ref().unwrap().worker.is_finished());
        // The user cancels after success is produced but before the render
        // thread accepts it. This late result must never create a session.
        menu.cancel();
        let mut app = App::new();
        app.insert_resource(menu)
            .init_resource::<InputFocus>()
            .init_resource::<Time>()
            .add_systems(Update, poll_connection);
        app.update();
        assert!(!app.world().contains_resource::<Session>());
        assert!(app.world().resource::<JoinScreen>().pending.is_some());
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while app.world().resource::<JoinScreen>().pending.is_some() && Instant::now() < deadline {
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(app.world().resource::<JoinScreen>().pending.is_none());
        assert!(!app.world().contains_resource::<Session>());
        assert!(!app.world().contains_resource::<Connection>());
        assert!(app.world().resource::<JoinScreen>().local_server.is_none());
        // Reopening the same save exercises server shutdown and writer-lock
        // release, not merely hiding the cancelled connection in the UI.
        app.world_mut().resource_mut::<JoinScreen>().start(true);
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while app.world().resource::<JoinScreen>().pending.is_some() && Instant::now() < deadline {
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            app.world().contains_resource::<Session>(),
            "{}",
            app.world().resource::<JoinScreen>().status
        );
        drop(app);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json.lock"));
    }

    #[test]
    fn short_touch_taps_survive_down_and_up_in_the_same_input_frame() {
        let mut controls = crate::touch::TouchControls::default();
        controls.enabled = true;
        let mut app = App::new();
        app.add_plugins(bevy::input::InputPlugin)
            .insert_resource(controls)
            .insert_resource(JoinScreen::new(
                "127.0.0.1:7878".into(),
                "Tester".into(),
                ServerConfig::default(),
                GraphicsQuality::default(),
                SessionMode::Player,
            ))
            .init_resource::<InputFocus>()
            .add_message::<MenuKey>()
            .add_message::<Ime>()
            .add_systems(Update, interact);
        let window = app
            .world_mut()
            .spawn((
                PrimaryWindow,
                Window {
                    resolution: bevy::window::WindowResolution::new(800, 480)
                        .with_scale_factor_override(2.),
                    ..default()
                },
            ))
            .id();
        // Computed UI coordinates are physical pixels, touch events logical.
        let field = app
            .world_mut()
            .spawn((
                Field::Address,
                Interaction::None,
                EditableText::new("127.0.0.1:7878"),
                Node::default(),
                ComputedNode {
                    size: Vec2::new(260., 96.),
                    inverse_scale_factor: 0.5,
                    ..default()
                },
                UiGlobalTransform::from_xy(200., 100.),
                InheritedVisibility::VISIBLE,
            ))
            .id();
        app.world_mut().spawn((
            Action::Mode(SessionMode::Observer),
            Interaction::None,
            Node::default(),
            ComputedNode {
                size: Vec2::new(160., 96.),
                inverse_scale_factor: 0.5,
                ..default()
            },
            UiGlobalTransform::from_xy(500., 100.),
            InheritedVisibility::VISIBLE,
        ));
        app.update();
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            app.world_mut().write_message(TouchInput {
                phase,
                position: Vec2::new(100., 50.),
                window,
                force: None,
                id: 1,
            });
        }
        app.update();
        assert_eq!(
            app.world().resource::<Touches>().iter().count(),
            0,
            "both phases reached InputPlugin in the same frame"
        );
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
        // The same raw event route covers mode and other menu buttons.
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            app.world_mut().write_message(TouchInput {
                phase,
                position: Vec2::new(250., 50.),
                window,
                force: None,
                id: 2,
            });
        }
        app.update();
        assert_eq!(
            app.world().resource::<JoinScreen>().mode,
            SessionMode::Observer
        );
        // A tap outside the visible window does not reach scrolled/offscreen UI.
        app.world_mut().resource_mut::<InputFocus>().clear();
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            app.world_mut().write_message(TouchInput {
                phase,
                position: Vec2::new(100., 350.),
                window,
                force: None,
                id: 3,
            });
        }
        app.update();
        assert!(app.world().resource::<InputFocus>().get().is_none());
    }

    #[test]
    fn android_field_switch_waits_for_the_new_native_buffer_echo() {
        let now = std::time::Instant::now();
        let pending = PendingNativeEdit {
            previous_text: "old field".into(),
            previous_selection: (9, 9),
            sent: now,
        };
        assert!(pending.is_stale("old field", (9, 9), "Violet", (6, 6), now));
        assert!(!pending.is_stale("Violet", (6, 6), "Violet", (6, 6), now));
        // A new user edit is accepted even before the exact echo is observed.
        assert!(!pending.is_stale("Violet雪", (7, 7), "Violet", (6, 6), now));
        // The guard is bounded: deleting back to the previous text cannot stall.
        assert!(!pending.is_stale(
            "old field",
            (9, 9),
            "Violet",
            (6, 6),
            now + std::time::Duration::from_millis(251)
        ));
    }

    #[test]
    fn android_whole_field_text_respects_limits_and_utf16_cursor_boundaries() {
        let text = "A😀雪";
        assert_eq!(utf16_to_byte(text, 0), 0);
        assert_eq!(utf16_to_byte(text, 1), 1);
        assert_eq!(utf16_to_byte(text, 3), 5);
        assert_eq!(utf16_to_byte(text, 4), text.len());
        assert_eq!(utf16_to_byte(text, 999), text.len());
        assert_eq!(clean_android_text("Violet\n雪\t😀", 8), "Violet雪😀");
        assert_eq!(
            clean_android_text("雪".repeat(25).as_str(), 24),
            "雪".repeat(24)
        );
        assert_eq!(clean_android_text("", 24), "");
    }

    #[test]
    fn ime_composition_commits_unicode_once_and_touch_fields_wait_for_a_tap() {
        let mut touch = crate::touch::TouchControls::default();
        touch.enabled = true;
        let mut app = App::new();
        app.insert_resource(JoinScreen::new(
            "127.0.0.1:7878".into(),
            "Tester".into(),
            ServerConfig::default(),
            GraphicsQuality::default(),
            SessionMode::Player,
        ))
        .insert_resource(touch)
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<bevy::text::FontCx>()
        .init_resource::<bevy::text::LayoutCx>()
        .init_resource::<bevy::clipboard::Clipboard>()
        .add_message::<MenuKey>()
        .add_message::<Ime>()
        .add_message::<TouchInput>()
        .add_systems(Update, interact)
        .add_systems(PostUpdate, bevy::text::apply_text_edits);
        let font = Font::from_bytes(
            include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
        );
        let mut fonts = app.world_mut().resource_mut::<bevy::text::FontCx>();
        let family = fonts.collection.register_fonts(font.data, None)[0].0;
        let family_name = fonts.collection.family_name(family).unwrap().to_owned();
        fonts.set_sans_serif_family(&family_name).unwrap();
        let field = app
            .world_mut()
            .spawn((Field::Name, Interaction::None, EditableText::new("")))
            .id();
        app.update();
        assert!(
            app.world().resource::<InputFocus>().get().is_none(),
            "touch join must not raise the keyboard before a tap"
        );
        *app.world_mut().get_mut::<Interaction>(field).unwrap() = Interaction::Pressed;
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
        *app.world_mut().get_mut::<Interaction>(field).unwrap() = Interaction::None;
        app.world_mut().write_message(Ime::Preedit {
            window: Entity::PLACEHOLDER,
            value: "雪".into(),
            cursor: Some((3, 3)),
        });
        app.update();
        assert_eq!(
            app.world()
                .get::<EditableText>(field)
                .unwrap()
                .value()
                .to_string(),
            ""
        );
        assert!(
            app.world()
                .get::<EditableText>(field)
                .unwrap()
                .is_composing()
        );
        app.world_mut().write_message(Ime::Commit {
            window: Entity::PLACEHOLDER,
            value: "雪".into(),
        });
        // Some backends also attach text to a key event in the commit frame.
        app.world_mut().write_message(MenuKey {
            input: KeyboardInput {
                key_code: KeyCode::KeyA,
                logical_key: Key::Character("雪".into()),
                state: bevy::input::ButtonState::Pressed,
                text: Some("雪".into()),
                repeat: false,
                window: Entity::PLACEHOLDER,
            },
            modifiers: ModifiersState::empty(),
        });
        app.update();
        assert_eq!(
            app.world()
                .get::<EditableText>(field)
                .unwrap()
                .value()
                .to_string(),
            "雪"
        );
        assert!(
            !app.world()
                .get::<EditableText>(field)
                .unwrap()
                .is_composing()
        );
    }

    #[test]
    fn modifier_flags_typing_and_enter_in_one_frame_submit_committed_text() {
        let mut app = App::new();
        app.insert_resource(JoinScreen::new(
            "127.0.0.1:7878".into(),
            "Tester".into(),
            ServerConfig::default(),
            GraphicsQuality::default(),
            SessionMode::Player,
        ))
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<bevy::text::FontCx>()
        .init_resource::<bevy::text::LayoutCx>()
        .init_resource::<bevy::clipboard::Clipboard>()
        .add_message::<MenuKey>()
        .add_message::<Ime>()
        .add_message::<TouchInput>()
        .add_systems(Update, interact)
        .add_systems(PostUpdate, bevy::text::apply_text_edits);
        // Parley selection uses shaped layout, so the headless test needs the
        // same real font as the rendered menu, not an empty font collection.
        let font = Font::from_bytes(
            include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
        );
        let mut fonts = app.world_mut().resource_mut::<bevy::text::FontCx>();
        let family = fonts.collection.register_fonts(font.data, None)[0].0;
        let family_name = fonts.collection.family_name(family).unwrap().to_owned();
        fonts.set_sans_serif_family(&family_name).unwrap();
        fonts.set_serif_family(&family_name).unwrap();
        let editable = EditableText::new("127.0.0.1:7878");
        let field = app
            .world_mut()
            .spawn((Field::Address, Interaction::None, editable))
            .id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Navigated);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        // Native macOS input can provide Command flags with no separate
        // Command-key event (as reproduced during the actual window test).
        for (key_code, text, modifiers) in [
            (KeyCode::KeyA, "a", ModifiersState::SUPER),
            (KeyCode::KeyN, "no-port", ModifiersState::empty()),
        ] {
            app.world_mut().write_message(MenuKey {
                input: KeyboardInput {
                    key_code,
                    logical_key: Key::Character(text.into()),
                    state: bevy::input::ButtonState::Pressed,
                    text: Some(text.into()),
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                },
                modifiers,
            });
        }
        app.update();
        assert!(app.world().resource::<JoinScreen>().pending.is_none());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        let menu = app.world().resource::<JoinScreen>();
        assert!(menu.pending.is_none());
        assert_eq!(menu.address, "no-port");
        assert!(menu.status.contains("host:port"));
    }

    #[test]
    fn joining_leaving_and_rejoining_clean_up_the_world_and_keep_the_form() {
        #[derive(Resource, Default)]
        struct Visits(u32);
        fn setup_world(mut commands: Commands, mut visits: ResMut<Visits>) {
            visits.0 += 1;
            commands.spawn(GameEntity).with_child(GameEntity);
        }
        let path = std::env::temp_dir().join(format!(
            "rubblekin-join-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut menu = JoinScreen::new(
            "remote.example:7878".into(),
            "Tester".into(),
            ServerConfig {
                bind_addr: "127.0.0.1:0".into(),
                save_path: path.clone(),
                seed: 42,
                generation: rubblekin_core::world::WorldGeneration::ValleyV1,
                allow_admin: true,
            },
            GraphicsQuality::default(),
            SessionMode::Observer,
        );
        menu.start(true);
        let mut app = App::new();
        app.insert_resource(menu)
            .init_resource::<crate::pause::PauseMenu>()
            .init_resource::<crate::world_map::WorldMap>()
            .init_resource::<InputFocus>()
            .init_resource::<Time>()
            .init_resource::<Assets<Font>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Visits>()
            .add_message::<MenuKey>()
            .add_message::<Ime>()
            .add_message::<TouchInput>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    interact,
                    poll_connection,
                    setup_world.run_if(resource_added::<Session>),
                    leave_world.run_if(resource_exists::<Session>),
                    refresh,
                )
                    .chain(),
            );
        app.world_mut()
            .spawn((Window::default(), CursorOptions::default()));
        let wait_for_join = |app: &mut App| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !app.world().contains_resource::<Session>()
                && std::time::Instant::now() < deadline
            {
                app.update();
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(
                app.world().contains_resource::<Session>(),
                "{}",
                app.world().resource::<JoinScreen>().status
            );
        };
        wait_for_join(&mut app);
        let session = app.world().resource::<Session>();
        assert!(session.observer.is_some());
        assert!(!session.can_admin);
        assert!(session.players.is_empty());
        assert_eq!(app.world().resource::<Visits>().0, 1);
        assert!(
            app.world_mut()
                .query_filtered::<&Node, With<MenuRoot>>()
                .single(app.world())
                .unwrap()
                .display
                == Display::None
        );
        app.world_mut()
            .resource_mut::<crate::pause::PauseMenu>()
            .open = true;
        app.world_mut()
            .resource_mut::<crate::world_map::WorldMap>()
            .open = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F10);
        app.update();
        assert!(!app.world().contains_resource::<Session>());
        assert!(!app.world().resource::<crate::pause::PauseMenu>().open);
        assert!(!app.world().resource::<crate::world_map::WorldMap>().open);
        assert!(!app.world().contains_resource::<Connection>());
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<GameEntity>>()
                .iter(app.world())
                .count(),
            0
        );
        assert!(app.world().resource::<JoinScreen>().local_server.is_none());
        assert_eq!(
            app.world().resource::<JoinScreen>().mode,
            SessionMode::Observer
        );
        assert!(path.exists());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        // Switching the visible mode controls applies to the next local join.
        let player_mode_button = app
            .world_mut()
            .query::<(Entity, &Action)>()
            .iter(app.world())
            .find_map(|(entity, action)| {
                matches!(action, Action::Mode(SessionMode::Player)).then_some(entity)
            })
            .unwrap();
        *app.world_mut()
            .get_mut::<Interaction>(player_mode_button)
            .unwrap() = Interaction::Pressed;
        app.update();
        assert_eq!(
            app.world().resource::<JoinScreen>().mode,
            SessionMode::Player
        );
        *app.world_mut()
            .get_mut::<Interaction>(player_mode_button)
            .unwrap() = Interaction::None;
        app.world_mut().resource_mut::<JoinScreen>().start(true);
        wait_for_join(&mut app);
        let session = app.world().resource::<Session>();
        assert!(session.observer.is_none());
        assert!(session.can_admin);
        assert!(session.players.iter().any(|player| player.id == session.id));
        assert_eq!(app.world().resource::<Visits>().0, 2);
        assert!(!app.world().resource::<crate::pause::PauseMenu>().open);
        app.world_mut()
            .resource_mut::<crate::pause::PauseMenu>()
            .open = true;
        app.world_mut()
            .resource_mut::<Connection>()
            .fail("test disconnect".into());
        app.update();
        let menu = app.world().resource::<JoinScreen>();
        assert!(menu.status.contains("test disconnect"));
        assert!(!app.world().resource::<crate::pause::PauseMenu>().open);
        assert_eq!(menu.address, "remote.example:7878");
        assert_eq!(menu.name, "Tester");
        assert!(!app.world().contains_resource::<Session>());
        assert!(
            app.world_mut()
                .query_filtered::<&Camera, With<MenuCamera>>()
                .single(app.world())
                .unwrap()
                .is_active
        );
        drop(app);
        let _ = std::fs::remove_file(path.with_extension("lock"));
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)]
#[path = "observer_tests.rs"]
mod observer_controls_tests;
