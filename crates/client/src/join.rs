//! A guest join screen. DNS, connection, and local-server startup run off the render thread.
use crate::{
    Avatars, GameEntity, Session, VoxelWorld, graphics::GraphicsQuality, network::Connection,
    observer::ObserverCamera, prediction::Prediction, terrain::TerrainScene,
};
use bevy::{
    input::keyboard::{Key, KeyboardInput},
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, EditableTextFilter, TextCursorStyle, TextEdit},
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
    winit::{RawWinitWindowEvent, converters::convert_keyboard_input},
};
use rubblekin_core::{
    physics::Body,
    protocol::{PROTOCOL_VERSION, ServerMessage, SessionMode},
    world::World as GameWorld,
};
use rubblekin_server::{ServerConfig, ServerHandle, spawn};
use std::{net::Ipv6Addr, thread::JoinHandle};
use winit::{event::WindowEvent as NativeWindowEvent, keyboard::ModifiersState};

#[derive(Resource)]
pub struct JoinScreen {
    address: String,
    name: String,
    status: String,
    config: ServerConfig,
    graphics: GraphicsQuality,
    mode: SessionMode,
    pending: Option<JoinHandle<Result<Joined, String>>>,
    next_action: Option<Action>,
    local_server: Option<ServerHandle>,
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
        self.status = if local {
            "Opening your local world and shaping its geography…".into()
        } else {
            format!("Connecting to {address}…")
        };
        let config = self.config.clone();
        let mode = self.mode;
        let graphics = self.graphics;
        self.pending = Some(std::thread::spawn(move || {
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
                Connection::connect(&address, name, mode).map_err(|error| error.to_string())?;
            // Global geography is prepared on this worker, keeping the menu
            // responsive even on a first join to a new seed.
            let (world, session) = session_from_welcome(welcome, address, graphics, 0.0, mode)?;
            Ok(Joined {
                connection,
                world,
                session,
                server,
            })
        }));
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
    Mode(SessionMode),
}

fn ink() -> Color {
    Color::srgb(0.89, 0.92, 0.85)
}
fn accent() -> Color {
    Color::srgb(0.90, 0.73, 0.42)
}

pub fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>, menu: Res<JoinScreen>) {
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    let camera = commands.spawn((Camera2d, MenuCamera)).id();
    commands.spawn((
        MenuRoot,
        UiTargetCamera(camera),
        Node { width: percent(100), height: percent(100), align_items: AlignItems::Center, justify_content: JustifyContent::Center, padding: UiRect::all(px(24)), ..default() },
        BackgroundColor(Color::srgb(0.055, 0.10, 0.10)),
    )).with_children(|root| {
        root.spawn((
            Node { width: px(560), max_width: percent(100), padding: UiRect::all(px(32)), flex_direction: FlexDirection::Column, row_gap: px(16), border_radius: BorderRadius::all(px(12)), ..default() },
            BackgroundColor(Color::srgb(0.08, 0.15, 0.15)),
        )).with_children(|panel| {
            panel.spawn((Text::new("R U B B L E K I N"), TextFont::from_font_size(32.0).with_font(font.clone()), TextColor(ink())));
            panel.spawn((Text::new("A LIVING WORLD  /  EARLY PROTOTYPE"), TextFont::from_font_size(14.0).with_font(font.clone()), TextColor(accent())));
            panel.spawn((Text::new("Choose your server"), TextFont::from_font_size(24.0).with_font(font.clone()), TextColor(ink()), Node { margin: UiRect::top(px(12)), ..default() }));
            for (field, label, value, max) in [(Field::Address, "SERVER ADDRESS", menu.address.as_str(), 260), (Field::Name, "DISPLAY NAME", menu.name.as_str(), 24)] {
                panel.spawn((Text::new(label), TextFont::from_font_size(14.0).with_font(font.clone()), TextColor(accent())));
                panel.spawn((
                    field,
                    Interaction::default(),
                    Node { width: percent(100), padding: UiRect::all(px(12)), border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(5)), overflow: Overflow::clip_x(), ..default() },
                    EditableText { max_characters: Some(max), ..EditableText::new(value) },
                    EditableTextFilter::new(|ch| !ch.is_control()),
                    TextLayout::no_wrap(),
                    TextCursorStyle { color: ink(), selection_color: Color::srgb(0.23, 0.43, 0.42), unfocused_selection_color: Color::NONE, ..default() },
                    TextFont::from_font_size(21.0).with_font(font.clone()), TextColor(ink()),
                    BackgroundColor(Color::srgb(0.04, 0.08, 0.08)), BorderColor::all(Color::srgb(0.20, 0.32, 0.30)),
                ));
            }
            panel.spawn((Node { column_gap: px(12), ..default() },)).with_children(|row| {
                for (mode, label) in [(SessionMode::Player, "Play as explorer"), (SessionMode::Observer, "Observe as admin")] {
                    row.spawn((Button, Action::Mode(mode), Node { padding: UiRect::axes(px(15), px(11)), border_radius: BorderRadius::all(px(6)), ..default() }, BackgroundColor(Color::srgb(0.19, 0.34, 0.31))))
                        .with_child((Text::new(label), TextFont::from_font_size(18.0).with_font(font.clone()), TextColor(ink())));
                }
            });
            panel.spawn((Text::new("Observer camera is read-only; server admin controls must be enabled."), TextFont::from_font_size(14.0).with_font(font.clone()), TextColor(Color::srgb(0.62, 0.74, 0.69))));
            panel.spawn((Node { column_gap: px(12), margin: UiRect::top(px(4)), ..default() },)).with_children(|row| {
                for (action, label) in [(Action::Join, "Join server"), (Action::Local, "Local world")] {
                    row.spawn((Button, action, Node { padding: UiRect::axes(px(22), px(13)), border_radius: BorderRadius::all(px(6)), ..default() }, BackgroundColor(Color::srgb(0.19, 0.34, 0.31))))
                        .with_child((Text::new(label), TextFont::from_font_size(19.0).with_font(font.clone()), TextColor(ink())));
                }
            });
            panel.spawn((Text::new(menu.status.clone()), TextFont::from_font_size(17.0).with_font(font.clone()), TextColor(ink()), MenuStatus, Node { min_height: px(46), ..default() }));
            panel.spawn((Text::new("Guest access · no account or password\nTab switches fields · Enter joins · F10 leaves a world"), TextFont::from_font_size(14.0).with_font(font.clone()), TextColor(Color::srgb(0.62, 0.74, 0.69))));
        });
    });
}

// macOS can attach modifier flags to synthesized key events without emitting
// separate modifier key presses. Bevy's ButtonInput does not expose those flags.
#[derive(Message)]
pub(super) struct MenuKey {
    input: KeyboardInput,
    modifiers: ModifiersState,
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
    mut focus: ResMut<InputFocus>,
    mut fields: Query<(Entity, &Field, &Interaction, &mut EditableText)>,
    actions: Query<(&Action, &Interaction), Changed<Interaction>>,
) {
    if session.is_some() || menu.pending.is_some() {
        keyboard.clear();
        return;
    }
    // Text edits commit in PostUpdate. Submit the previous frame's action only
    // after reading those committed values (including paste followed by Enter).
    let requested = menu.next_action.take();
    for (entity, _, interaction, _) in &fields {
        if *interaction == Interaction::Pressed {
            focus.set(entity, FocusCause::Navigated);
        }
    }
    if focus.get().is_none() || keys.just_pressed(KeyCode::Tab) {
        let old = focus.get();
        if let Some((entity, _, _, _)) =
            fields.iter().find(|(entity, _, _, _)| Some(*entity) != old)
        {
            focus.set(entity, FocusCause::Navigated);
        }
    }
    // Each event retains the modifier flags it had when Winit received it,
    // including complete press/release chords arriving in a single frame.
    for key in keyboard.read() {
        let event = &key.input;
        if !event.state.is_pressed() {
            continue;
        }
        let shortcut = key.modifiers.control_key() || key.modifiers.super_key();
        let shift = key.modifiers.shift_key();
        let Some(edit) = text_edit(event, shortcut, shift) else {
            continue;
        };
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
    let action = actions
        .iter()
        .find(|(_, interaction)| **interaction == Interaction::Pressed)
        .map(|(action, _)| *action)
        .or_else(|| keys.just_pressed(KeyCode::Enter).then_some(Action::Join));
    if let Some(Action::Mode(mode)) = action {
        menu.mode = mode;
    } else {
        menu.next_action = action;
    }
    if let Some(action) = requested {
        menu.next_action = None;
        menu.start(matches!(action, Action::Local));
    }
}

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
) {
    if !menu.pending.as_ref().is_some_and(JoinHandle::is_finished) {
        return;
    }
    let result = menu
        .pending
        .take()
        .unwrap()
        .join()
        .unwrap_or_else(|_| Err("Connection worker stopped unexpectedly.".into()));
    match result {
        Ok(mut joined) => {
            joined.session.status_until = time.elapsed_secs_f64() + 12.0;
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

fn session_from_welcome(
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
            help: true,
            graphics,
            npc,
            residents,
            villages,
            players,
            world_time,
            status: status.into(),
            status_until: now + 12.0,
            target: None,
            fps: 0.0,
            edits: edits.len(),
            connected_to: address,
            prediction: Prediction::default(),
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
) {
    if !keys.just_pressed(KeyCode::F10) && connection.error.is_none() {
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
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<Connection>();
    commands.remove_resource::<Session>();
    commands.remove_resource::<VoxelWorld>();
    commands.remove_resource::<TerrainScene>();
    commands.insert_resource(Avatars::default());
    menu.local_server.take();
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
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
    mut buttons: Query<(&Action, &Interaction, &mut BackgroundColor)>,
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
    for (action, interaction, mut background) in &mut buttons {
        background.0 = if menu.pending.is_some() {
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
mod tests {
    use super::*;
    use rubblekin_core::protocol::{NpcAction, NpcSnapshot, PlayerSnapshot};

    pub(super) fn welcome(mode: SessionMode) -> ServerMessage {
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
        menu.pending = Some(std::thread::spawn(|| Err("Connection refused".into())));
        while !menu.pending.as_ref().unwrap().is_finished() {
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
            .init_resource::<InputFocus>()
            .init_resource::<Time>()
            .init_resource::<Assets<Font>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Visits>()
            .add_message::<MenuKey>()
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
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F10);
        app.update();
        assert!(!app.world().contains_resource::<Session>());
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
        app.world_mut()
            .resource_mut::<Connection>()
            .fail("test disconnect".into());
        app.update();
        let menu = app.world().resource::<JoinScreen>();
        assert!(menu.status.contains("test disconnect"));
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
