//! Multi-touch input stays separate from mouse buttons: Android can synthesize a
//! mouse click for a finger, which must never capture the cursor or edit terrain.
use std::collections::HashMap;

use bevy::{
    input::touch::{TouchInput, TouchPhase},
    prelude::*,
    window::{AppLifecycle, PrimaryWindow, WindowFocused},
};
use rubblekin_core::physics::MoveInput;

use crate::{GameEntity, PALETTE, Session, network::Connection};

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
    pub(crate) help: bool,
    pub(crate) graphics: bool,
    pub(crate) selected: Option<usize>,
    pub(crate) zoom: f32,
    pub(crate) return_spawn: bool,
    pub(crate) next_village: bool,
    contacts: HashMap<u64, Contact>,
    suspended: bool,
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
    Menu,
    Material(usize),
    ZoomIn,
    ZoomOut,
    Help,
    Graphics,
    Return,
    NextVillage,
    Leave,
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
            if menu_open { "Close" } else { "Menu" }.into(),
        );
        if menu_open {
            let width = s(364.0);
            let left = (size.x - width) * 0.5;
            let top = (size.y - s(228.0)) * 0.5;
            let entries = [
                (Action::Inspect, "Inspect"),
                (Action::Help, "Controls"),
                (Action::Graphics, "Graphics"),
                (Action::Return, "Return to spawn"),
                (Action::NextVillage, "Next village"),
                (Action::Leave, "Leave world"),
            ];
            for (index, (action, label)) in entries.into_iter().enumerate() {
                if !observing && matches!(action, Action::Return | Action::NextVillage) {
                    continue;
                }
                add(
                    action,
                    left + s((index % 2) as f32 * 186.0),
                    top + s((index / 2) as f32 * 76.0),
                    178.0,
                    64.0,
                    label.into(),
                );
            }
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
            let left = (size.x - s(301.0)) * 0.5;
            for (index, block) in PALETTE.iter().enumerate() {
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
        self.help = false;
        self.graphics = false;
        self.selected = None;
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
            Action::Inspect => {
                self.inspect = true;
                self.menu_open = false;
                self.contacts.clear();
            }
            Action::Help => {
                self.help = true;
                self.menu_open = false;
                self.contacts.clear();
            }
            Action::Graphics => self.graphics = true,
            Action::Material(index) => self.selected = Some(index),
            Action::Menu => {
                self.menu_open = !self.menu_open;
                self.contacts.clear();
                // A menu tap cancels any gameplay input gathered earlier in the frame.
                self.look = Vec2::ZERO;
                self.jump = false;
                self.dig = false;
                self.build = false;
                self.selected = None;
                self.flight = false;
            }
            Action::Leave => {
                self.leave = true;
                self.contacts.clear();
            }
            Action::Return => self.return_spawn = true,
            Action::NextVillage => self.next_village = true,
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
                        Action::Jump => self.jump = false,
                        Action::Dig => self.dig = false,
                        Action::Build => self.build = false,
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
    keys: Res<ButtonInput<KeyCode>>,
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
    let session = session.unwrap();
    if keys.just_pressed(KeyCode::Escape) {
        controls.press(Action::Menu);
    }
    let size = Vec2::new(window.width(), window.height());
    // Rebuild after each event: a Menu press changes which controls are active
    // immediately, including additional fingers delivered in this same frame.
    let mut received_touch = false;
    for event in events.read().filter(|event| event.window == window_entity) {
        received_touch = true;
        let layout = Layout::new(
            size,
            session.observer.is_some(),
            session.flying,
            controls.menu_open,
        );
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
                let layout = Layout::new(
                    size,
                    session.observer.is_some(),
                    session.flying,
                    controls.menu_open,
                );
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
    let layout = Layout::new(
        size,
        session.observer.is_some(),
        session.flying,
        controls.menu_open,
    );
    controls.held(&layout);
}

#[derive(Component)]
pub(crate) struct TouchButton(Action);
#[derive(Component)]
pub(crate) struct StickBase;
#[derive(Component)]
pub(crate) struct StickKnob;
#[derive(Component)]
pub(crate) struct TouchMenuBackdrop;

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
        TouchMenuBackdrop,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            display: Display::None,
            ..default()
        },
        GlobalZIndex(20),
        BackgroundColor(Color::srgba(0.02, 0.05, 0.05, 0.86)),
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
        Action::Menu,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::Help,
        Action::Graphics,
        Action::Return,
        Action::NextVillage,
        Action::Leave,
    ];
    for action in actions
        .into_iter()
        .chain((0..PALETTE.len()).map(Action::Material))
    {
        commands.spawn((
            GameEntity,
            TouchButton(action),
            Node {
                position_type: PositionType::Absolute,
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            GlobalZIndex(23),
            BackgroundColor(Color::srgba(0.055, 0.10, 0.10, 0.78)),
            BorderColor::all(Color::srgba(0.83, 0.92, 0.82, 0.55)),
            children![(
                Text::new(""),
                TextFont::from_font_size(14.0).with_font(font.clone()),
                TextColor(Color::srgb(0.89, 0.92, 0.85))
            )],
        ));
    }
}

#[allow(clippy::type_complexity)]
pub fn refresh(
    window: Single<&Window, With<PrimaryWindow>>,
    controls: Res<TouchControls>,
    session: Res<Session>,
    mut nodes: Query<
        (
            &mut Node,
            Option<&TouchButton>,
            Option<&StickBase>,
            Option<&StickKnob>,
            Option<&TouchMenuBackdrop>,
            Option<&Children>,
            Option<&mut BackgroundColor>,
        ),
        Or<(
            With<TouchButton>,
            With<StickBase>,
            With<StickKnob>,
            With<TouchMenuBackdrop>,
        )>,
    >,
    mut labels: Query<(&mut Text, &mut TextFont)>,
) {
    let layout = Layout::new(
        Vec2::new(window.width(), window.height()),
        session.observer.is_some(),
        session.flying,
        controls.menu_open,
    );
    for (mut node, button, base, knob, backdrop, children, background) in &mut nodes {
        node.display = Display::None;
        if !controls.enabled {
            continue;
        }
        if backdrop.is_some() {
            if controls.menu_open {
                node.display = Display::Flex;
            }
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
                        text.0.clone_from(&region.label);
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
        for action in [Action::Jump, Action::Dig, Action::Build] {
            let mut input = TouchControls::default();
            let position = action_position(&layout, action);
            input.route(&event(1, TouchPhase::Started, position), &layout);
            input.route(&event(1, TouchPhase::Ended, position), &layout);
            input.held(&layout);
            assert_eq!(input.jump, action == Action::Jump);
            assert_eq!(input.dig, action == Action::Dig);
            assert_eq!(input.build, action == Action::Build);
            input.clear_frame();
            input.held(&layout);
            assert!(!input.jump && !input.dig && !input.build);
            input.route(&event(1, TouchPhase::Started, position), &layout);
            input.route(
                &event(1, TouchPhase::Canceled, Vec2::splat(f32::NAN)),
                &layout,
            );
            input.held(&layout);
            assert!(!input.jump && !input.dig && !input.build);
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
            for menu in [false, true] {
                let layout = Layout::new(size, false, true, menu);
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
    fn actual_systems_send_touch_movement_and_edits_and_reset_after_focus_pause_disconnect() {
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

        let movement = layout.stick.center() - Vec2::Y * 40.0;
        app.world_mut().write_message(TouchInput {
            window,
            ..event(3, TouchPhase::Started, movement)
        });
        app.world_mut().run_schedule(Update);
        assert!(
            matches!(read_message(&mut peer), ClientMessage::Input { input, .. } if input.direction[1] < -0.95)
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
