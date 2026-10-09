//! Multi-touch input stays separate from mouse buttons: Android can synthesize a
//! mouse click for a finger, which must never capture the cursor or edit terrain.
use std::collections::HashMap;

use bevy::{
    input::touch::{TouchInput, TouchPhase},
    prelude::*,
    window::{AppLifecycle, PrimaryWindow, WindowFocused},
};
use rubblekin_core::physics::MoveInput;

use crate::{GameEntity, Session, network::Connection};

#[derive(Resource, Default)]
pub struct TouchControls {
    pub enabled: bool,
    pub leave: bool,
    pub menu_open: bool,
    pub(crate) movement: Vec2,
    pub(crate) look: Vec2,
    pub(crate) jump: bool,
    pub(crate) sprint: bool,
    pub(crate) vertical: f32,
    pub(crate) dig: bool,
    pub(crate) build: bool,
    pub(crate) flight: bool,
    pub(crate) inspect: bool,
    pub(crate) activity: bool,
    pub(crate) activity_return: bool,
    pub(crate) activity_hint: bool,
    pub(crate) talk: bool,
    pub(crate) market: bool,
    pub(crate) help: bool,
    pub(crate) selected: Option<usize>,
    pub(crate) palette_page: bool,
    pub(crate) zoom: f32,
    pub(crate) return_spawn: bool,
    pub(crate) next_village: bool,
    contacts: HashMap<u64, Contact>,
    pub(crate) suspended: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Move,
    Look,
    Jump,
    Sprint,
    Rise,
    Fall,
    Dig,
    Build,
    Flight,
    Inspect,
    Activity,
    ActivityReturn,
    ActivityHint,
    Talk,
    Market,
    Menu,
    Material(usize),
    PalettePage,
    ZoomIn,
    ZoomOut,
    Blocked,
}

struct Contact {
    action: Action,
    position: Vec2,
}

#[derive(Clone)]
struct Region {
    action: Action,
    rect: Rect,
    label: String,
}

/// Drawing and hit testing use the same logical-pixel rectangles. Winit already
/// converts touch positions to logical pixels, including Android display density.
struct Layout {
    regions: Vec<Region>,
    stick: Rect,
    scale: f32,
    size: Vec2,
}

impl Layout {
    fn for_session(size: Vec2, session: &Session, menu_open: bool) -> Self {
        let mut layout = Self::new(size, session.observer.is_some(), session.flying, menu_open);
        layout.set_hotbar(&session.hotbar);
        if session.gliding {
            for region in &mut layout.regions {
                match region.action {
                    Action::Sprint => region.label = "Dive".into(),
                    Action::Jump => region.label = "Brake".into(),
                    _ => {}
                }
            }
        }
        if session.ride.is_some() {
            layout.regions.retain(|region| {
                matches!(
                    region.action,
                    Action::Talk
                        | Action::Market
                        | Action::Sprint
                        | Action::Jump
                        | Action::Menu
                        | Action::Inspect
                        | Action::ZoomIn
                        | Action::ZoomOut
                )
            });
        }
        if !menu_open && crate::activities::touch_opportunity(session) && !session.gliding {
            let s = layout.scale;
            for (index, action, label) in [
                (0., Action::Activity, "Use"),
                (1., Action::ActivityReturn, "Return"),
                (2., Action::ActivityHint, "Hint"),
            ] {
                layout.regions.push(Region {
                    action,
                    rect: Rect::from_corners(
                        Vec2::new(size.x - (328. - index * 80.) * s, 70. * s),
                        Vec2::new(size.x - (256. - index * 80.) * s, 118. * s),
                    ),
                    label: label.into(),
                });
            }
        }
        layout
    }

    fn set_hotbar(&mut self, slots: &[rubblekin_core::world::Block; crate::palette::QUICK_SLOTS]) {
        for region in &mut self.regions {
            if let Action::Material(slot) = region.action {
                region.label = slots[slot].name().into();
            } else if region.action == Action::PalettePage {
                region.label = "Inventory".into();
            }
        }
    }

    fn new(size: Vec2, observing: bool, flying: bool, menu_open: bool) -> Self {
        let scale = (size.x / 840.0).min(size.y / 400.0).clamp(0.5, 1.0);
        let s = |v: f32| v * scale;
        let stick = Rect::from_corners(
            Vec2::new(s(20.0), size.y - s(138.0)),
            Vec2::new(s(132.0), size.y - s(26.0)),
        );
        let mut result = Self {
            regions: Vec::new(),
            stick,
            scale,
            size,
        };
        let mut add = |action, x, y, width, height, label: String| {
            result.regions.push(Region {
                action,
                rect: Rect::from_corners(Vec2::new(x, y), Vec2::new(x + s(width), y + s(height))),
                label,
            });
        };
        add(
            Action::Menu,
            size.x - s(88.0),
            s(14.0),
            72.0,
            48.0,
            "Menu".into(),
        );
        if menu_open {
            // The shared pause panel owns all menu controls and hit testing.
            result.regions.clear();
            return result;
        }
        add(
            Action::Inspect,
            size.x - s(168.0),
            s(14.0),
            72.0,
            48.0,
            "Inspect".into(),
        );
        if !observing {
            add(
                Action::Market,
                size.x - s(88.0),
                s(70.0),
                72.0,
                48.0,
                "Cargo".into(),
            );
            add(
                Action::Talk,
                size.x - s(328.0),
                s(14.0),
                72.0,
                48.0,
                "Travel".into(),
            );
            add(
                Action::Flight,
                size.x - s(248.0),
                s(14.0),
                72.0,
                48.0,
                if flying { "Walk" } else { "Fly" }.into(),
            );
        }
        add(
            Action::Sprint,
            s(20.0),
            size.y - s(202.0),
            112.0,
            48.0,
            if observing { "Boost" } else { "Sprint" }.into(),
        );
        add(
            Action::ZoomIn,
            size.x - s(160.0),
            size.y - s(206.0),
            64.0,
            48.0,
            if observing { "Faster" } else { "Zoom +" }.into(),
        );
        add(
            Action::ZoomOut,
            size.x - s(88.0),
            size.y - s(206.0),
            72.0,
            48.0,
            if observing { "Slower" } else { "Zoom −" }.into(),
        );
        if flying || observing {
            add(
                Action::Rise,
                size.x - s(160.0),
                size.y - s(142.0),
                64.0,
                54.0,
                "Rise".into(),
            );
            add(
                Action::Fall,
                size.x - s(88.0),
                size.y - s(142.0),
                72.0,
                54.0,
                "Fall".into(),
            );
        }
        if !observing {
            for (index, (action, label)) in [
                (Action::Dig, "Dig"),
                (Action::Build, "Build"),
                (Action::Jump, "Jump"),
            ]
            .into_iter()
            .enumerate()
            {
                add(
                    action,
                    size.x - s(232.0 - index as f32 * 72.0),
                    size.y - s(72.0),
                    64.0,
                    54.0,
                    label.into(),
                );
            }
            add(
                Action::PalettePage,
                (size.x - s(144.0)) * 0.5,
                size.y - s(132.0),
                144.0,
                44.0,
                "Inventory".into(),
            );
            let left = (size.x - s(301.0)) * 0.5;
            for (index, block) in crate::palette::DEFAULT_HOTBAR.iter().enumerate() {
                add(
                    Action::Material(index),
                    left + s(index as f32 * 51.0),
                    size.y - s(72.0),
                    46.0,
                    54.0,
                    block.name().into(),
                );
            }
        }
        result
    }

    fn action(&self, position: Vec2, menu_open: bool) -> Action {
        if let Some(region) = self.regions.iter().find(|r| r.rect.contains(position)) {
            return region.action;
        }
        if menu_open {
            return Action::Blocked;
        }
        if self.stick.contains(position) {
            Action::Move
        } else if position.x >= self.size.x * 0.5 {
            Action::Look
        } else {
            Action::Blocked
        }
    }
}

impl TouchControls {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..default()
        }
    }

    pub fn reset(&mut self) {
        self.contacts.clear();
        self.menu_open = false;
        self.leave = false;
        self.clear_frame();
    }

    fn clear_frame(&mut self) {
        self.movement = Vec2::ZERO;
        self.look = Vec2::ZERO;
        self.jump = false;
        self.sprint = false;
        self.vertical = 0.0;
        self.dig = false;
        self.build = false;
        self.flight = false;
        self.inspect = false;
        self.activity = false;
        self.activity_return = false;
        self.activity_hint = false;
        self.talk = false;
        self.market = false;
        self.help = false;
        self.selected = None;
        self.palette_page = false;
        self.zoom = 0.0;
        self.return_spawn = false;
        self.next_village = false;
    }

    fn press(&mut self, action: Action) {
        match action {
            Action::Jump => self.jump = true,
            Action::Dig => self.dig = true,
            Action::Build => self.build = true,
            Action::Flight => self.flight = true,
            Action::Activity => self.activity = true,
            Action::ActivityReturn => self.activity_return = true,
            Action::ActivityHint => self.activity_hint = true,
            Action::Talk => self.talk = true,
            Action::Market => self.market = true,
            Action::Inspect => {
                self.inspect = true;
                self.menu_open = false;
                self.contacts.clear();
            }
            Action::Material(index) => self.selected = Some(index),
            Action::PalettePage => self.palette_page = true,
            Action::Menu => {
                self.menu_open = !self.menu_open;
                self.contacts.clear();
                // A menu tap cancels any gameplay input gathered earlier in the frame.
                self.look = Vec2::ZERO;
                self.jump = false;
                self.dig = false;
                self.build = false;
                self.selected = None;
                self.palette_page = false;
                self.activity = false;
                self.activity_return = false;
                self.activity_hint = false;
                self.flight = false;
            }
            _ => {}
        }
    }

    fn route(&mut self, event: &TouchInput, layout: &Layout) {
        if !event.position.is_finite()
            && matches!(event.phase, TouchPhase::Started | TouchPhase::Moved)
        {
            return;
        }
        match event.phase {
            TouchPhase::Started => {
                let mut action = layout.action(event.position, self.menu_open);
                // One movement owner and one look owner. Extra fingers still
                // operate buttons, but never replace an active joystick/look.
                if matches!(action, Action::Move | Action::Look)
                    && self
                        .contacts
                        .values()
                        .any(|contact| contact.action == action)
                {
                    action = Action::Blocked;
                }
                self.press(action);
                self.contacts.insert(
                    event.id,
                    Contact {
                        action,
                        position: event.position,
                    },
                );
            }
            TouchPhase::Moved => {
                if let Some(contact) = self.contacts.get_mut(&event.id) {
                    if contact.action == Action::Look && !self.menu_open {
                        // Logical pixels are stable across Android densities.
                        self.look += event.position - contact.position;
                    }
                    contact.position = event.position;
                }
            }
            TouchPhase::Ended => {
                self.contacts.remove(&event.id);
            }
            TouchPhase::Canceled => {
                if let Some(contact) = self.contacts.remove(&event.id)
                    && !self
                        .contacts
                        .values()
                        .any(|other| other.action == contact.action)
                {
                    match contact.action {
                        Action::Activity => self.activity = false,
                        Action::ActivityReturn => self.activity_return = false,
                        Action::ActivityHint => self.activity_hint = false,
                        Action::Jump => self.jump = false,
                        Action::Dig => self.dig = false,
                        Action::Build => self.build = false,
                        Action::PalettePage => self.palette_page = false,
                        Action::Look => self.look = Vec2::ZERO,
                        _ => {}
                    }
                }
            }
        }
    }

    fn held(&mut self, layout: &Layout) {
        if self.menu_open || self.leave {
            return;
        }
        for contact in self.contacts.values() {
            if contact.action != Action::Move
                && !layout
                    .regions
                    .iter()
                    .any(|region| region.action == contact.action)
            {
                continue;
            }
            match contact.action {
                Action::Move => {
                    let delta =
                        (contact.position - layout.stick.center()) / (layout.stick.width() * 0.36);
                    let length = delta.length();
                    if length > 0.12 {
                        self.movement = delta.normalize() * ((length - 0.12) / 0.88).min(1.0);
                        self.movement.y = -self.movement.y;
                    }
                }
                Action::Jump => self.jump = true,
                Action::Sprint => self.sprint = true,
                Action::Rise => self.vertical += 1.0,
                Action::Fall => self.vertical -= 1.0,
                Action::Dig => self.dig = true,
                Action::Build => self.build = true,
                Action::ZoomIn => self.zoom += 1.0,
                Action::ZoomOut => self.zoom -= 1.0,
                _ => {}
            }
        }
        self.vertical = self.vertical.clamp(-1.0, 1.0);
        self.zoom = self.zoom.clamp(-1.0, 1.0);
    }

    pub(crate) fn movement_input(&self, yaw: f32, flying: bool) -> MoveInput {
        let forward = Vec2::new(yaw.sin(), -yaw.cos());
        let right = Vec2::new(yaw.cos(), yaw.sin());
        MoveInput {
            direction: (forward * self.movement.y + right * self.movement.x).to_array(),
            jump: self.jump,
            sprint: self.sprint,
            vertical: self.vertical,
            fly: flying,
            glide_direction: None,
        }
    }
}

/// Drain events even on the join screen, preventing touches from an earlier
/// session from acquiring gameplay controls after joining.
#[allow(clippy::too_many_arguments)]
pub fn read(
    mut events: MessageReader<TouchInput>,
    mut focus: MessageReader<WindowFocused>,
    mut lifecycle: MessageReader<AppLifecycle>,
    pause: Option<Res<crate::pause::PauseMenu>>,
    conversation: Option<Res<crate::airships::PilotConversation>>,
    console: Option<Res<crate::admin_console::AdminConsole>>,
    map: Option<Res<crate::world_map::WorldMap>>,
    market: Option<Res<crate::market::MarketPanel>>,
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<(Entity, &Window), With<PrimaryWindow>>,
    session: Option<Res<Session>>,
    connection: Option<Res<Connection>>,
    mut controls: ResMut<TouchControls>,
) {
    controls.clear_frame();
    let (window_entity, window) = *window;
    let lost_focus = focus
        .read()
        .any(|event| event.window == window_entity && !event.focused);
    let mut lifecycle_reset = false;
    for event in lifecycle.read() {
        if matches!(
            event,
            AppLifecycle::WillSuspend | AppLifecycle::Suspended | AppLifecycle::Idle
        ) {
            controls.suspended = true;
            lifecycle_reset = true;
        } else {
            controls.suspended = false;
        }
    }
    if !controls.enabled
        || session.is_none()
        || !window.focused
        || lost_focus
        || lifecycle_reset
        || controls.suspended
        || connection.is_none_or(|c| c.error.is_some())
    {
        controls.reset();
        events.clear();
        return;
    }
    if pause.is_some_and(|pause| pause.open) {
        controls.contacts.clear();
        controls.menu_open = true;
        events.clear();
        return;
    }
    if session.as_ref().is_some_and(|s| s.inventory.input_blocked)
        || conversation.is_some_and(|dialog| dialog.open() || dialog.input_blocked)
        || console.is_some_and(|console| console.input_blocked)
        || map.is_some_and(|map| map.open || map.input_blocked)
        || market.is_some_and(|market| market.open || market.input_blocked)
    {
        controls.reset();
        events.clear();
        return;
    }
    let session = session.unwrap();
    let size = Vec2::new(window.width(), window.height());
    // Rebuild after each event: a Menu press changes which controls are active
    // immediately, including additional fingers delivered in this same frame.
    let mut received_touch = false;
    for event in events.read().filter(|event| event.window == window_entity) {
        received_touch = true;
        let layout = Layout::for_session(size, &session, controls.menu_open);
        if event.phase == TouchPhase::Started
            && !controls.menu_open
            && crate::ui::touch_panel_at(event.position, size, &session)
        {
            controls.contacts.insert(
                event.id,
                Contact {
                    action: Action::Blocked,
                    position: event.position,
                },
            );
        } else {
            controls.route(event, &layout);
        }
    }
    if !cfg!(target_os = "android")
        && !received_touch
        && controls.contacts.keys().all(|id| *id == u64::MAX)
    {
        let id = u64::MAX;
        if let Some(position) = window.cursor_position() {
            let phase = if mouse.just_pressed(MouseButton::Left) {
                Some(TouchPhase::Started)
            } else if mouse.just_released(MouseButton::Left) {
                Some(TouchPhase::Ended)
            } else if mouse.pressed(MouseButton::Left) && controls.contacts.contains_key(&id) {
                Some(TouchPhase::Moved)
            } else {
                None
            };
            if let Some(phase) = phase {
                let layout = Layout::for_session(size, &session, controls.menu_open);
                if phase == TouchPhase::Started
                    && !controls.menu_open
                    && crate::ui::touch_panel_at(position, size, &session)
                {
                    controls.contacts.insert(
                        id,
                        Contact {
                            action: Action::Blocked,
                            position,
                        },
                    );
                } else {
                    controls.route(
                        &TouchInput {
                            id,
                            phase,
                            position,
                            window: window_entity,
                            force: None,
                        },
                        &layout,
                    );
                }
            }
        } else {
            controls.contacts.remove(&id);
        }
    }
    let layout = Layout::for_session(size, &session, controls.menu_open);
    controls.held(&layout);
}

#[derive(Component)]
pub(crate) struct TouchButton(Action);
#[derive(Component)]
pub(crate) struct MaterialIcon(usize);
#[derive(Component)]
pub(crate) struct StickBase;
#[derive(Component)]
pub(crate) struct StickKnob;

pub fn setup(
    mut commands: Commands,
    controls: Res<TouchControls>,
    mut fonts: ResMut<Assets<Font>>,
) {
    if !controls.enabled {
        return;
    }
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    commands.spawn((
        GameEntity,
        StickBase,
        Node {
            position_type: PositionType::Absolute,
            border_radius: BorderRadius::MAX,
            border: UiRect::all(px(2)),
            ..default()
        },
        GlobalZIndex(21),
        BackgroundColor(Color::srgba(0.05, 0.13, 0.13, 0.55)),
        BorderColor::all(Color::srgba(0.83, 0.92, 0.82, 0.45)),
    ));
    commands.spawn((
        GameEntity,
        StickKnob,
        Node {
            position_type: PositionType::Absolute,
            border_radius: BorderRadius::MAX,
            ..default()
        },
        GlobalZIndex(22),
        BackgroundColor(Color::srgba(0.83, 0.92, 0.82, 0.75)),
    ));
    let actions = [
        Action::Jump,
        Action::Sprint,
        Action::Rise,
        Action::Fall,
        Action::Dig,
        Action::Build,
        Action::Flight,
        Action::Inspect,
        Action::Activity,
        Action::ActivityReturn,
        Action::ActivityHint,
        Action::Talk,
        Action::Market,
        Action::Menu,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::PalettePage,
    ];
    for action in actions
        .into_iter()
        .chain((0..crate::palette::QUICK_SLOTS).map(Action::Material))
    {
        commands
            .spawn((
                GameEntity,
                TouchButton(action),
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
                GlobalZIndex(23),
                BackgroundColor(Color::srgba(0.055, 0.10, 0.10, 0.78)),
                BorderColor::all(Color::srgba(0.83, 0.92, 0.82, 0.55)),
            ))
            .with_children(|button| {
                if let Action::Material(slot) = action {
                    button.spawn((
                        ImageNode::default(),
                        MaterialIcon(slot),
                        Node {
                            width: px(26),
                            height: px(26),
                            ..default()
                        },
                    ));
                }
                button.spawn((
                    Text::new(""),
                    TextFont::from_font_size(14.).with_font(font.clone()),
                    TextColor(Color::srgb(0.89, 0.92, 0.85)),
                ));
            });
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn refresh(
    window: Single<&Window, With<PrimaryWindow>>,
    time: Res<Time>,
    controls: Res<TouchControls>,
    session: Res<Session>,
    world: Res<crate::VoxelWorld>,
    conversation: Option<Res<crate::airships::PilotConversation>>,
    map: Option<Res<crate::world_map::WorldMap>>,
    market: Option<Res<crate::market::MarketPanel>>,
    mut nodes: Query<
        (
            &mut Node,
            Option<&TouchButton>,
            Option<&StickBase>,
            Option<&StickKnob>,
            Option<&Children>,
            Option<&mut BackgroundColor>,
        ),
        Or<(With<TouchButton>, With<StickBase>, With<StickKnob>)>,
    >,
    mut labels: Query<(&mut Text, &mut TextFont)>,
    icons: Option<Res<crate::block_textures::BlockIcons>>,
    mut material_icons: Query<(&MaterialIcon, &mut ImageNode)>,
) {
    if let Some(icons) = &icons {
        for (slot, mut img) in &mut material_icons {
            img.image = icons.0[session.hotbar[slot.0].catalog_index().unwrap()].clone();
        }
    }
    let layout = Layout::for_session(
        Vec2::new(window.width(), window.height()),
        &session,
        controls.menu_open,
    );
    for (mut node, button, base, knob, children, background) in &mut nodes {
        node.display = Display::None;
        if !controls.enabled
            || controls.menu_open
            || session.inventory.input_blocked
            || conversation.as_ref().is_some_and(|dialog| dialog.open())
            || map.as_ref().is_some_and(|map| map.open)
            || market.as_ref().is_some_and(|market| market.open)
        {
            continue;
        }
        if base.is_some() || knob.is_some() {
            if !controls.menu_open {
                let center = layout.stick.center()
                    + if knob.is_some() {
                        Vec2::new(controls.movement.x, -controls.movement.y) * 40.0 * layout.scale
                    } else {
                        Vec2::ZERO
                    };
                let size = if knob.is_some() {
                    Vec2::splat(44.0 * layout.scale)
                } else {
                    layout.stick.size()
                };
                place(&mut node, Rect::from_center_size(center, size));
            }
            continue;
        }
        if let Some(button) = button
            && let Some(region) = layout.regions.iter().find(|r| r.action == button.0)
        {
            place(&mut node, region.rect);
            if let Some(children) = children {
                for child in children.iter() {
                    if let Ok((mut text, mut font)) = labels.get_mut(child) {
                        if let Action::Material(slot) = button.0 {
                            text.0 = (slot + 1).to_string();
                        } else if button.0 == Action::Market
                            && market.as_ref().is_some_and(|panel| {
                                panel
                                    .nearby_work(&session, time.elapsed_secs_f64())
                                    .is_some()
                            })
                        {
                            text.0 = "Work".into();
                        } else if button.0 == Action::Market
                            && crate::market::nearby_market(&session, &world).is_some()
                        {
                            text.0 = "Market".into();
                        } else {
                            text.0.clone_from(&region.label);
                        }
                        font.font_size = bevy::text::FontSize::Px(14.0 * layout.scale);
                    }
                }
            }
            if let Some(mut background) = background {
                let active = controls.contacts.values().any(|c| c.action == button.0)
                    || matches!(button.0, Action::Material(index) if index == session.selected);
                background.0 = if active {
                    Color::srgba(0.34, 0.40, 0.22, 0.90)
                } else {
                    Color::srgba(0.055, 0.10, 0.10, 0.78)
                };
            }
        }
    }
}

fn place(node: &mut Node, rect: Rect) {
    node.display = Display::Flex;
    node.left = px(rect.min.x);
    node.top = px(rect.min.y);
    node.width = px(rect.width());
    node.height = px(rect.height());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(Vec2::new(840.0, 400.0), false, false, false)
    }
    fn event(id: u64, phase: TouchPhase, position: Vec2) -> TouchInput {
        TouchInput {
            id,
            phase,
            position,
            window: Entity::PLACEHOLDER,
            force: None,
        }
    }
    fn action_position(layout: &Layout, action: Action) -> Vec2 {
        layout
            .regions
            .iter()
            .find(|r| r.action == action)
            .unwrap()
            .rect
            .center()
    }

    #[test]
    fn independent_fingers_move_look_jump_and_edit_without_changing_owners() {
        let layout = layout();
        let mut input = TouchControls::default();
        let center = layout.stick.center();
        input.route(&event(1, TouchPhase::Started, center), &layout);
        input.route(
            &event(1, TouchPhase::Moved, center + Vec2::new(0.0, -40.0)),
            &layout,
        );
        input.route(
            &event(2, TouchPhase::Started, Vec2::new(500.0, 180.0)),
            &layout,
        );
        input.route(
            &event(2, TouchPhase::Moved, Vec2::new(520.0, 190.0)),
            &layout,
        );
        input.route(
            &event(
                3,
                TouchPhase::Started,
                action_position(&layout, Action::Jump),
            ),
            &layout,
        );
        input.route(
            &event(
                4,
                TouchPhase::Started,
                action_position(&layout, Action::Dig),
            ),
            &layout,
        );
        input.held(&layout);
        assert!(input.movement.y > 0.95);
        assert_eq!(input.look, Vec2::new(20.0, 10.0));
        assert!(input.jump && input.dig && !input.build);
        // Moving a held action over the world never turns that finger into look.
        input.route(
            &event(4, TouchPhase::Moved, Vec2::new(600.0, 100.0)),
            &layout,
        );
        assert_eq!(input.look, Vec2::new(20.0, 10.0));
        let movement = input.movement_input(std::f32::consts::FRAC_PI_2, false);
        assert!(movement.direction[0] > 0.95 && movement.direction[1].abs() < 0.001);
        assert!(movement.jump);
        input.clear_frame();
        input.route(&event(1, TouchPhase::Ended, center), &layout);
        input.route(&event(3, TouchPhase::Canceled, Vec2::ZERO), &layout);
        input.held(&layout);
        assert_eq!(input.movement, Vec2::ZERO);
        assert!(!input.jump && input.dig);
    }

    #[test]
    fn brief_jump_dig_and_build_taps_survive_one_frame_and_canceled_taps_do_not_edit() {
        let layout = layout();
        for action in [
            Action::Jump,
            Action::Dig,
            Action::Build,
            Action::PalettePage,
        ] {
            let mut input = TouchControls::default();
            let position = action_position(&layout, action);
            input.route(&event(1, TouchPhase::Started, position), &layout);
            input.route(&event(1, TouchPhase::Ended, position), &layout);
            input.held(&layout);
            assert_eq!(input.jump, action == Action::Jump);
            assert_eq!(input.dig, action == Action::Dig);
            assert_eq!(input.build, action == Action::Build);
            assert_eq!(input.palette_page, action == Action::PalettePage);
            input.clear_frame();
            input.held(&layout);
            assert!(!input.jump && !input.dig && !input.build && !input.palette_page);
            input.route(&event(1, TouchPhase::Started, position), &layout);
            input.route(
                &event(1, TouchPhase::Canceled, Vec2::splat(f32::NAN)),
                &layout,
            );
            input.held(&layout);
            assert!(!input.jump && !input.dig && !input.build && !input.palette_page);
        }
    }

    #[test]
    fn menu_and_lifecycle_reset_cancel_held_input_and_ignore_unowned_moves() {
        let layout = layout();
        let mut input = TouchControls {
            enabled: true,
            ..default()
        };
        input.route(
            &event(
                1,
                TouchPhase::Started,
                action_position(&layout, Action::Build),
            ),
            &layout,
        );
        input.route(&event(2, TouchPhase::Started, layout.stick.min), &layout);
        input.route(
            &event(
                3,
                TouchPhase::Started,
                action_position(&layout, Action::Menu),
            ),
            &layout,
        );
        input.held(&layout);
        assert!(input.menu_open);
        assert!(!input.build && input.movement == Vec2::ZERO);
        input.reset();
        input.route(
            &event(1, TouchPhase::Moved, Vec2::new(500.0, 150.0)),
            &layout,
        );
        input.held(&layout);
        assert!(!input.menu_open && input.look == Vec2::ZERO && !input.build);
        assert!(input.enabled);
    }

    #[test]
    fn flight_and_observer_layouts_have_vertical_controls_and_observer_has_no_editing() {
        for (observing, flying) in [(false, true), (true, false)] {
            let layout = Layout::new(Vec2::new(840.0, 400.0), observing, flying, false);
            let mut input = TouchControls::default();
            input.route(
                &event(
                    1,
                    TouchPhase::Started,
                    action_position(&layout, Action::Rise),
                ),
                &layout,
            );
            input.held(&layout);
            assert_eq!(input.movement_input(0.0, flying).vertical, 1.0);
            assert_eq!(input.movement_input(0.0, flying).fly, flying);
            if observing {
                assert!(!layout.regions.iter().any(|r| matches!(
                    r.action,
                    Action::Dig
                        | Action::Build
                        | Action::Jump
                        | Action::Flight
                        | Action::Material(_)
                        | Action::PalettePage
                )));
            }
        }
    }

    #[test]
    fn layouts_fit_landscape_phones_and_hit_regions_do_not_overlap() {
        for size in [
            Vec2::new(840.0, 400.0),
            Vec2::new(640.0, 360.0),
            Vec2::new(1280.0, 720.0),
        ] {
            for (menu, custom) in [(false, false), (false, true), (true, false), (true, true)] {
                let mut layout = Layout::new(size, false, true, menu);
                let mut slots = crate::palette::DEFAULT_HOTBAR;
                if custom {
                    slots[5] = rubblekin_core::world::Block::PurpleWool;
                }
                layout.set_hotbar(&slots);
                assert!(!layout.regions.iter().any(|region| matches!(region.action, Action::Material(index) if index >= crate::palette::QUICK_SLOTS)));
                if !menu {
                    assert_eq!(
                        layout
                            .regions
                            .iter()
                            .filter(|region| matches!(region.action, Action::Material(_)))
                            .count(),
                        6
                    );
                }
                for (index, a) in layout.regions.iter().enumerate() {
                    assert!(a.rect.min.cmpge(Vec2::ZERO).all() && a.rect.max.cmple(size).all());
                    assert_eq!(layout.action(a.rect.center(), menu), a.action);
                    for b in layout.regions.iter().skip(index + 1) {
                        assert!(
                            a.rect.intersect(b.rect).is_empty(),
                            "overlap: {:?} {:?}",
                            a.action,
                            b.action
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn actual_systems_send_touch_movement_and_edits_and_reset_after_focus_pause_map_disconnect() {
        use bevy::{
            diagnostic::DiagnosticsStore,
            gizmos::{AppGizmoBuilder, config::DefaultGizmoConfigGroup},
            input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
            light::DirectionalLightShadowMap,
            window::CursorOptions,
        };
        use rubblekin_core::{
            protocol::{ClientMessage, ServerMessage, SessionMode},
            world::Block,
        };
        use std::{
            io::{BufRead, BufReader, Write},
            net::TcpListener,
            time::Duration,
        };

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let client = std::thread::spawn(move || {
            Connection::connect(&address, "Touch tester".into(), SessionMode::Player).unwrap()
        });
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut peer = BufReader::new(socket);
        let mut hello = String::new();
        peer.read_line(&mut hello).unwrap();
        assert!(matches!(
            serde_json::from_str::<ClientMessage>(&hello).unwrap(),
            ClientMessage::Hello { .. }
        ));
        let welcome: ServerMessage = crate::join::tests::welcome(SessionMode::Player);
        serde_json::to_writer(peer.get_mut(), &welcome).unwrap();
        peer.get_mut().write_all(b"\n").unwrap();
        let (connection, welcome) = client.join().unwrap();
        let (world, mut session) = crate::join::session_from_welcome(
            welcome,
            "touch peer".into(),
            crate::graphics::GraphicsQuality::Low,
            0.0,
            SessionMode::Player,
        )
        .unwrap();
        session.inspector = false;
        session.help = false;
        session.yaw = 0.0;
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(25));
        let mut app = App::new();
        app.insert_resource(crate::VoxelWorld(world))
            .insert_resource(crate::graphics::GraphicsSettings::new(
                crate::graphics::GraphicsQuality::Low,
            ))
            .init_resource::<crate::pause::PauseMenu>()
            .init_resource::<crate::world_map::WorldMap>()
            .init_resource::<crate::market::MarketPanel>()
            .insert_resource(session)
            .insert_resource(connection)
            .insert_resource(time)
            .insert_resource(TouchControls::new(true))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<DirectionalLightShadowMap>()
            .init_resource::<DiagnosticsStore>()
            .init_gizmo_group::<DefaultGizmoConfigGroup>()
            .add_message::<TouchInput>()
            .add_message::<WindowFocused>()
            .add_message::<AppLifecycle>()
            .add_systems(
                Update,
                (
                    read,
                    (crate::controls, crate::edit_blocks)
                        .chain()
                        .run_if(resource_exists::<Session>),
                )
                    .chain(),
            );
        let window = app
            .world_mut()
            .spawn((
                PrimaryWindow,
                Window {
                    focused: true,
                    resolution: bevy::window::WindowResolution::new(840, 400),
                    ..default()
                },
                CursorOptions::default(),
            ))
            .id();
        app.world_mut().spawn((
            crate::GameCamera,
            Transform::from_xyz(0.25, 6.0, 0.25).looking_to(Vec3::NEG_Y, Vec3::Z),
        ));
        for button in [MouseButton::Left, MouseButton::Right] {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(button);
        }
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::splat(500.0);
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. } if input.direction == [0.0; 2])
        );
        assert_eq!(app.world().resource::<Session>().yaw, 0.0);
        assert_eq!(app.world().resource::<Session>().next_request, 1);
        assert_eq!(
            app.world().get::<CursorOptions>(window).unwrap().grab_mode,
            bevy::window::CursorGrabMode::None
        );

        let layout = layout();
        let dig = action_position(&layout, Action::Dig);
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            app.world_mut().write_message(TouchInput {
                window,
                ..event(1, phase, dig)
            });
        }
        app.world_mut().run_schedule(Update);
        assert!(matches!(
            read_message(&mut peer),
            ClientMessage::Input { .. }
        ));
        assert!(matches!(
            read_message(&mut peer),
            ClientMessage::Edit {
                request_id: 1,
                block: Block::Air,
                ..
            }
        ));
        app.world_mut().resource_mut::<Session>().edit_clock = 0.0;
        let build = action_position(&layout, Action::Build);
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            app.world_mut().write_message(TouchInput {
                window,
                ..event(2, phase, build)
            });
        }
        app.world_mut().run_schedule(Update);
        assert!(matches!(
            read_message(&mut peer),
            ClientMessage::Input { .. }
        ));
        assert!(matches!(
            read_message(&mut peer),
            ClientMessage::Edit {
                request_id: 2,
                block: Block::Wood,
                ..
            }
        ));

        // A custom hotbar item survives a short touch tap and reaches the actual Edit message.
        app.world_mut().resource_mut::<Session>().hotbar[3] = Block::PurpleWool;
        for (id, action) in [(81, Action::Material(3)), (82, Action::Build)] {
            let layout = Layout::for_session(
                Vec2::new(840., 400.),
                app.world().resource::<Session>(),
                false,
            );
            let position = action_position(&layout, action);
            app.world_mut().resource_mut::<Session>().edit_clock = 0.;
            for phase in [TouchPhase::Started, TouchPhase::Ended] {
                app.world_mut().write_message(TouchInput {
                    window,
                    ..event(id, phase, position)
                });
            }
            app.world_mut().run_schedule(Update);
            assert!(matches!(
                read_message(&mut peer),
                ClientMessage::Input { .. }
            ));
            assert_eq!(app.world().resource::<Session>().selected, 3);
        }
        assert!(matches!(
            read_message(&mut peer),
            ClientMessage::Edit {
                request_id: 3,
                block: Block::PurpleWool,
                ..
            }
        ));
        // Number keys select the editable slots; held keys do not change them again.
        for (key, selected) in [
            (KeyCode::Digit6, 5),
            (KeyCode::Digit1, 0),
            (KeyCode::Digit4, 3),
        ] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.world_mut().run_schedule(Update);
            assert!(matches!(
                read_message(&mut peer),
                ClientMessage::Input { .. }
            ));
            assert_eq!(app.world().resource::<Session>().selected, selected);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
            app.world_mut().run_schedule(Update);
            assert!(matches!(
                read_message(&mut peer),
                ClientMessage::Input { .. }
            ));
            assert_eq!(app.world().resource::<Session>().selected, selected);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();

        let movement = layout.stick.center() - Vec2::Y * 40.0;
        app.world_mut().write_message(TouchInput {
            window,
            ..event(3, TouchPhase::Started, movement)
        });
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. } if input.direction[1] < -0.95)
        );
        // Opening the shared menu cancels held contacts and blocks newly started edits.
        app.world_mut()
            .resource_mut::<crate::pause::PauseMenu>()
            .open = true;
        for (id, phase, position) in [
            (3, TouchPhase::Moved, movement),
            (7, TouchPhase::Started, build),
        ] {
            app.world_mut().write_message(TouchInput {
                window,
                ..event(id, phase, position)
            });
        }
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. }
            if input.direction == [0.0; 2] && !input.jump && input.vertical == 0.0)
        );
        assert!(app.world().resource::<TouchControls>().contacts.is_empty());
        app.world_mut()
            .resource_mut::<Connection>()
            .send(ClientMessage::Ping);
        assert!(matches!(read_message(&mut peer), ClientMessage::Ping));
        app.world_mut()
            .resource_mut::<crate::pause::PauseMenu>()
            .open = false;
        app.world_mut().resource_mut::<TouchControls>().menu_open = false;
        app.world_mut().write_message(TouchInput {
            window,
            ..event(3, TouchPhase::Moved, movement)
        });
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. }
            if input.direction == [0.0; 2])
        );
        // Opening either panel drops contacts; closing cannot reuse held fingers.
        for (panel_kind, open) in [
            (0, true),
            (0, false),
            (1, true),
            (1, false),
            (2, true),
            (2, false),
        ] {
            *app.world_mut().resource_mut::<crate::world_map::WorldMap>() = default();
            app.world_mut()
                .resource_mut::<crate::market::MarketPanel>()
                .clear();
            app.world_mut().resource_mut::<Session>().inventory = default();
            if panel_kind == 2 {
                let inv = &mut app.world_mut().resource_mut::<Session>().inventory;
                inv.open = open;
                inv.input_blocked = true;
            } else if panel_kind == 1 {
                let mut market = app.world_mut().resource_mut::<crate::market::MarketPanel>();
                market.open = open;
                market.input_blocked = true;
            } else {
                let mut map = app.world_mut().resource_mut::<crate::world_map::WorldMap>();
                map.open = open;
                map.input_blocked = true;
            }
            for key in [KeyCode::KeyC, KeyCode::Digit1] {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(key);
            }
            for (id, position) in [(3, movement), (7, build)] {
                app.world_mut().write_message(TouchInput {
                    window,
                    ..event(id, TouchPhase::Started, position)
                });
            }
            app.world_mut().run_schedule(Update);
            assert!(
                matches!(read_message(&mut peer), ClientMessage::Input { input, .. }
                if input.direction == [0.0; 2] && !input.jump && input.vertical == 0.0)
            );
            assert_eq!(
                app.world().resource::<Session>().selected,
                3,
                "Opening or closing a modal must not also change materials"
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            assert!(app.world().resource::<TouchControls>().contacts.is_empty());
            app.world_mut()
                .resource_mut::<Connection>()
                .send(ClientMessage::Ping);
            assert!(matches!(read_message(&mut peer), ClientMessage::Ping));
        }
        app.world_mut().resource_mut::<Session>().inventory = default();
        *app.world_mut().resource_mut::<crate::world_map::WorldMap>() = default();
        app.world_mut()
            .resource_mut::<crate::market::MarketPanel>()
            .clear();
        app.world_mut().write_message(TouchInput {
            window,
            ..event(3, TouchPhase::Moved, movement)
        });
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. }
            if input.direction == [0.0; 2])
        );
        app.world_mut().write_message(TouchInput {
            window,
            ..event(3, TouchPhase::Started, movement)
        });
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. }
            if input.direction[1] < -0.95)
        );
        app.world_mut().write_message(WindowFocused {
            window,
            focused: false,
        });
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. } if input.direction == [0.0; 2])
        );
        app.world_mut().write_message(TouchInput {
            window,
            ..event(3, TouchPhase::Moved, movement)
        });
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. } if input.direction == [0.0; 2])
        );

        app.world_mut().write_message(TouchInput {
            window,
            ..event(4, TouchPhase::Started, movement)
        });
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. } if input.direction[1] < -0.95)
        );
        app.world_mut().write_message(AppLifecycle::WillSuspend);
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. } if input.direction == [0.0; 2])
        );
        app.world_mut().write_message(AppLifecycle::Running);
        app.world_mut().write_message(TouchInput {
            window,
            ..event(4, TouchPhase::Moved, movement)
        });
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. } if input.direction == [0.0; 2])
        );
        app.world_mut().write_message(TouchInput {
            window,
            ..event(5, TouchPhase::Started, movement)
        });
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. } if input.direction[1] < -0.95)
        );
        // Physical boarding is acquired by ordinary movement contact. The
        // same touch Jump still sends a movement input and lifts the passenger
        // above the deck instead of selecting an exit action.
        let world = rubblekin_core::world::World::from_generation_edits(
            42,
            rubblekin_core::world::WorldGeneration::GeographyV3,
            &[],
        )
        .unwrap();
        let network = rubblekin_core::airships::AirshipNetwork::new(&world);
        let ship = network
            .ships(0.0)
            .into_iter()
            .find(|ship| ship.docked_at.is_some())
            .unwrap();
        app.world_mut().insert_resource(crate::VoxelWorld(world));
        app.world_mut().resource_mut::<TouchControls>().reset();
        {
            let mut session = app.world_mut().resource_mut::<Session>();
            session.body = rubblekin_core::physics::Body::new(
                rubblekin_core::airships::deck_position(&ship, [0.0, 0.0, 0.0]),
            );
            session.airships = network;
            session.airship_clock.time = 0.0;
            session.ride = None;
            session.deck_position = None;
            session.flying = false;
        }
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. }
            if !input.jump && !input.fly)
        );
        assert_eq!(
            app.world().resource::<Session>().ride.unwrap().ship_id,
            ship.id
        );
        let deck_height = app.world().resource::<Session>().body.position[1];
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            app.world_mut().write_message(TouchInput {
                window,
                ..event(8, phase, action_position(&layout, Action::Jump))
            });
        }
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. }
            if input.jump && !input.fly)
        );
        let aboard = app.world().resource::<Session>();
        assert!(aboard.ride.is_some());
        assert!(!aboard.body.on_ground && aboard.body.position[1] > deck_height);
        app.world_mut()
            .resource_mut::<Connection>()
            .fail("test disconnection".into());
        app.world_mut().run_schedule(Update);
        assert_eq!(app.world().resource::<TouchControls>().movement, Vec2::ZERO);
        assert!(app.world().resource::<TouchControls>().contacts.is_empty());
        app.world_mut().remove_resource::<Session>();
        app.world_mut().write_message(TouchInput {
            window,
            ..event(6, TouchPhase::Started, movement)
        });
        app.world_mut().run_schedule(Update);
        assert!(app.world().resource::<TouchControls>().contacts.is_empty());

        fn read_message(peer: &mut BufReader<std::net::TcpStream>) -> ClientMessage {
            let mut line = String::new();
            peer.read_line(&mut line).unwrap();
            serde_json::from_str(&line).unwrap()
        }
    }
}

#[cfg(test)]
mod activity_touch_tests {
    use super::*;
    #[test]
    fn activity_buttons_use_the_shared_layout_and_cancel_with_the_contact_or_menu() {
        let (world, mut session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(rubblekin_core::protocol::SessionMode::Player),
            "touch activity".into(),
            crate::graphics::GraphicsQuality::Low,
            0.,
            rubblekin_core::protocol::SessionMode::Player,
        )
        .unwrap();
        let plan = rubblekin_core::activities::review_plans(&world)[0].clone();
        session.body.position = plan.objects[0];
        session.activities = vec![rubblekin_core::activities::ActivitySnapshot {
            plan,
            revision: 0,
            props: [rubblekin_core::activities::PropState::Home; 3],
            faces: [0; 3],
            complete: false,
            available: true,
        }];
        let layout = Layout::for_session(Vec2::new(840., 400.), &session, false);
        let region = layout
            .regions
            .iter()
            .find(|r| r.action == Action::Activity)
            .unwrap();
        assert!(region.rect.width() >= 48. && region.rect.height() >= 48.);
        assert_eq!(layout.action(region.rect.center(), false), Action::Activity);
        let mut input = TouchControls::default();
        let event = TouchInput {
            phase: TouchPhase::Started,
            position: region.rect.center(),
            force: None,
            id: 4,
            window: Entity::PLACEHOLDER,
        };
        input.route(&event, &layout);
        assert!(input.activity);
        input.route(
            &TouchInput {
                phase: TouchPhase::Canceled,
                ..event
            },
            &layout,
        );
        assert!(!input.activity);
        input.press(Action::Activity);
        input.press(Action::ActivityReturn);
        input.press(Action::ActivityHint);
        input.press(Action::Menu);
        assert!(!input.activity && !input.activity_return && !input.activity_hint);
        assert!(
            !Layout::for_session(Vec2::new(840., 400.), &session, true)
                .regions
                .iter()
                .any(|r| r.action == Action::Activity)
        );
    }
}
