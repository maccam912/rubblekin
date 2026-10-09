//! One pause menu for mouse, keyboard, and touch. The shared world keeps running.
use bevy::{
    input::{
        keyboard::Key,
        mouse::AccumulatedMouseScroll,
        touch::{TouchInput, TouchPhase},
    },
    picking::hover::Hovered,
    prelude::*,
    ui::Pressed,
    ui_widgets::{ActivateOnPress, Button},
    window::{PrimaryWindow, WindowFocused},
};

use crate::{
    GameEntity, Session,
    graphics::{
        DISTANCE_STEP, GraphicsQuality, GraphicsSettings, MAX_NEAR_DISTANCE, MAX_SHADOW_DISTANCE,
        MAX_TREE_DISTANCE, MIN_NEAR_DISTANCE, MIN_SHADOW_DISTANCE, MIN_TREE_DISTANCE,
        TREE_DISTANCE_STEP,
    },
    join::MenuKey,
    touch::TouchControls,
};

#[derive(Resource, Default)]
pub struct PauseMenu {
    pub open: bool,
    pub leave: bool,
    /// Includes the closing frame, so Resume cannot also dig, jump, or look.
    pub input_blocked: bool,
    pub just_closed: bool,
    focused: Option<Action>,
    scroll_finger: Option<(u64, Vec2)>,
    distance_drag: Option<DistanceDrag>,
    keyboard_distance: Option<(Distance, f32)>,
}

#[derive(Component)]
pub(super) struct PauseRoot;
#[derive(Component)]
pub(super) struct PauseContent;
#[derive(Component)]
pub(super) struct PauseActions;
#[derive(Component)]
pub(super) struct PausePanel;
#[derive(Component)]
pub(super) struct CompactNote;
#[derive(Component)]
pub(super) struct NearText;
#[derive(Component)]
pub(super) struct ShadowText;
#[derive(Component)]
pub(super) struct TreeText;
#[derive(Component)]
pub(super) struct QualityNote;

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Distance {
    Near,
    Trees,
    Shadows,
}

#[derive(Component)]
pub(super) struct DistanceFill(Distance);

#[derive(Clone, Copy)]
struct DistanceDrag {
    distance: Distance,
    finger: Option<u64>,
    fraction: f32,
}

impl Distance {
    fn bounds(self) -> (f32, f32) {
        match self {
            Self::Near => (MIN_NEAR_DISTANCE, MAX_NEAR_DISTANCE),
            Self::Trees => (MIN_TREE_DISTANCE, MAX_TREE_DISTANCE),
            Self::Shadows => (MIN_SHADOW_DISTANCE, MAX_SHADOW_DISTANCE),
        }
    }

    fn value(self, graphics: &GraphicsSettings) -> f32 {
        match self {
            Self::Near => graphics.near_distance,
            Self::Trees => graphics.tree_distance,
            Self::Shadows => graphics.shadow_distance,
        }
    }

    fn enabled(self, graphics: &GraphicsSettings) -> bool {
        self != Self::Shadows || graphics.quality.shadows()
    }

    fn set_fraction(self, fraction: f32, graphics: &mut GraphicsSettings) {
        if !fraction.is_finite() || !self.enabled(graphics) {
            return;
        }
        self.set_value(self.value_at_fraction(fraction), graphics);
    }

    fn set_value(self, value: f32, graphics: &mut GraphicsSettings) {
        if !value.is_finite() || !self.enabled(graphics) {
            return;
        }
        match self {
            Self::Near => graphics.adjust_near_distance(value - graphics.near_distance),
            Self::Trees => graphics.adjust_tree_distance(value.round() - graphics.tree_distance),
            Self::Shadows => {
                graphics.adjust_shadow_distance(value.round() - graphics.shadow_distance)
            }
        }
    }

    fn value_at_fraction(self, fraction: f32) -> f32 {
        let (min, max) = self.bounds();
        let value = min + fraction.clamp(0., 1.) * (max - min);
        let step = if self == Self::Near {
            DISTANCE_STEP
        } else {
            1.
        };
        ((value / step).round() * step).clamp(min, max)
    }

    fn displayed_value(self, pause: &PauseMenu, graphics: &GraphicsSettings) -> f32 {
        if let Some((distance, value)) = pause.keyboard_distance
            && distance == self
        {
            return value;
        }
        pause
            .distance_drag
            .filter(|drag| drag.distance == self)
            .map_or_else(
                || self.value(graphics),
                |drag| self.value_at_fraction(drag.fraction),
            )
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Resume,
    Map,
    Quality(GraphicsQuality),
    Distance(Distance),
    NearLess,
    NearMore,
    TreeLess,
    TreeMore,
    ShadowLess,
    ShadowMore,
    ResetDistances,
    Inspect,
    Controls,
    Tutorials,
    ReturnSpawn,
    NextVillage,
    Leave,
}

fn ink() -> Color {
    Color::srgb(0.89, 0.92, 0.85)
}

fn accent() -> Color {
    Color::srgb(0.90, 0.73, 0.42)
}

fn button(action: Action, width: Val) -> impl Bundle {
    (
        (Button, ActivateOnPress, Hovered::default()),
        action,
        Node {
            min_height: px(44),
            width,
            padding: UiRect::axes(px(12), px(8)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.19, 0.34, 0.31)),
        BorderColor::all(Color::NONE),
    )
}

fn label(value: &str, font: &Handle<Font>, size: f32) -> impl Bundle {
    (
        Text::new(value),
        TextFont::from_font_size(size).with_font(font.clone()),
        TextColor(ink()),
    )
}

pub fn setup(
    mut commands: Commands,
    mut fonts: ResMut<Assets<Font>>,
    session: Res<Session>,
    touch: Res<TouchControls>,
    mut pause: ResMut<PauseMenu>,
) {
    *pause = PauseMenu::default();
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    commands
        .spawn((
            GameEntity,
            PauseRoot,
            GlobalZIndex(100),
            Node {
                display: Display::None,
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(16)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
            BackgroundColor(Color::srgba(0.02, 0.04, 0.04, 0.80)),
        ))
        .with_children(|root| {
            root.spawn((
                PausePanel,
                Node {
                    width: px(800),
                    max_width: percent(100),
                    flex_shrink: 0.,
                    padding: UiRect::all(px(16)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    border_radius: BorderRadius::all(px(12)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.08, 0.15, 0.15)),
            ))
            .with_children(|panel| {
                panel.spawn(label("Paused", &font, 26.));
                panel.spawn((
                    Text::new("Your controls are paused. The shared world keeps running."),
                    TextFont::from_font_size(14.).with_font(font.clone()),
                    TextColor(Color::srgb(0.62, 0.74, 0.69)),
                ));
                panel.spawn((
                    PauseContent,
                    Node {
                        column_gap: px(24),
                        row_gap: px(16),
                        ..default()
                    },
                )).with_children(|content| {
                    content.spawn((Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.,
                        min_width: px(0),
                        row_gap: px(8),
                        ..default()
                    },)).with_children(|graphics| {
                        graphics.spawn((Text::new("GRAPHICS QUALITY"), TextFont::from_font_size(14.).with_font(font.clone()), TextColor(accent())));
                        graphics.spawn((Node { column_gap: px(6), ..default() },)).with_children(|row| {
                            for (quality, name) in [(GraphicsQuality::Low, "Low"), (GraphicsQuality::Balanced, "Balanced"), (GraphicsQuality::High, "High")] {
                                row.spawn(button(Action::Quality(quality), percent(33.33))).with_child(label(name, &font, 16.));
                            }
                        });
                        graphics.spawn((QualityNote, CompactNote, label("", &font, 14.)));
                        for (title, distance, less, more) in [
                            ("NEAR DETAIL DISTANCE", Distance::Near, Action::NearLess, Action::NearMore),
                            ("MEDIUM TREE DISTANCE", Distance::Trees, Action::TreeLess, Action::TreeMore),
                            ("SHADOW DISTANCE", Distance::Shadows, Action::ShadowLess, Action::ShadowMore),
                        ] {
                            graphics.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(4), ..default() },)).with_children(|setting| {
                                setting.spawn((Text::new(title), TextFont::from_font_size(14.).with_font(font.clone()), TextColor(accent())));
                                setting.spawn((Node { align_items: AlignItems::Center, column_gap: px(8), ..default() },)).with_children(|row| {
                                    row.spawn(button(less, px(48))).with_child(label("−", &font, 23.));
                                    let value = row.spawn((label("", &font, 20.), Node { min_width: px(76), ..default() })).id();
                                    match less {
                                        Action::NearLess => { row.commands().entity(value).insert(NearText); }
                                        Action::TreeLess => { row.commands().entity(value).insert(TreeText); }
                                        _ => { row.commands().entity(value).insert(ShadowText); }
                                    }
                                    row.spawn((distance, Action::Distance(distance), Hovered::default(), Node { height: px(44), flex_grow: 1., min_width: px(40), align_items: AlignItems::Center, padding: UiRect::horizontal(px(4)), border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(6)), ..default() }, BackgroundColor(Color::NONE), BorderColor::all(Color::NONE))).with_children(|track| {
                                        track.spawn((Node { width: percent(100), height: px(8), border_radius: BorderRadius::all(px(4)), ..default() }, BackgroundColor(Color::srgb(0.16, 0.27, 0.25)))).with_child((DistanceFill(distance), Node { width: percent(0), height: percent(100), border_radius: BorderRadius::all(px(4)), ..default() }, BackgroundColor(accent())));
                                    });
                                    row.spawn(button(more, px(48))).with_child(label("+", &font, 23.));
                                });
                            });
                        }
                        graphics.spawn(button(Action::ResetDistances, percent(100))).with_child(label("Reset distances", &font, 16.));
                        graphics.spawn((CompactNote, Text::new("Drag a bar to adjust; − / + for step changes.\nLonger distances use more memory and drawing work.\nReset uses 48 m detail, 128 m trees and this preset's shadows."), TextFont::from_font_size(14.).with_font(font.clone()), TextColor(Color::srgb(0.62, 0.74, 0.69))));
                    });
                    content.spawn((PauseActions, Node { width: px(220), flex_shrink: 0., flex_direction: FlexDirection::Column, row_gap: px(8), ..default() },)).with_children(|actions| {
                        actions.spawn(button(Action::Resume, percent(100))).with_child(label("Resume", &font, 18.));
                        actions.spawn(button(Action::Map, percent(100))).with_child(label("Map", &font, 18.));
                        actions.spawn((Node { column_gap: px(8), ..default() },)).with_children(|row| {
                            row.spawn(button(Action::Inspect, percent(50))).with_child(label("Inspect", &font, 16.));
                            row.spawn(button(Action::Controls, percent(50))).with_child(label("Controls", &font, 16.));
                        });
                        if session.observer.is_none() {
                            actions.spawn(button(Action::Tutorials, percent(100))).with_child(label("Teach me again", &font, 16.));
                        }
                        if session.observer.is_some() {
                            actions.spawn(button(Action::ReturnSpawn, percent(100))).with_child(label("Return to spawn", &font, 16.));
                            actions.spawn(button(Action::NextVillage, percent(100))).with_child(label("Next village", &font, 16.));
                        }
                        actions.spawn(button(Action::Leave, percent(100))).with_child(label("Leave world", &font, 18.));
                        actions.spawn((Text::new(if touch.enabled { "Tap to choose\nSwipe blank space to scroll" } else { "Esc  resume\nTab / Shift+Tab  select\nEnter  activate\nSelected bar: Left / Right\nHome / End  minimum / maximum\nRelease keys to apply\nScroll if needed" }), TextFont::from_font_size(14.).with_font(font.clone()), TextColor(Color::srgb(0.62, 0.74, 0.69))));
                    });
                });
            });
        });
}

fn enabled(action: Action, graphics: &GraphicsSettings) -> bool {
    match action {
        Action::Distance(distance) => distance.enabled(graphics),
        Action::NearLess => graphics.near_distance > MIN_NEAR_DISTANCE,
        Action::NearMore => graphics.near_distance < MAX_NEAR_DISTANCE,
        Action::TreeLess => graphics.tree_distance > MIN_TREE_DISTANCE,
        Action::TreeMore => graphics.tree_distance < MAX_TREE_DISTANCE,
        Action::ShadowLess => {
            graphics.quality.shadows() && graphics.shadow_distance > MIN_SHADOW_DISTANCE
        }
        Action::ShadowMore => {
            graphics.quality.shadows() && graphics.shadow_distance < MAX_SHADOW_DISTANCE
        }
        _ => true,
    }
}

fn activate(
    action: Action,
    pause: &mut PauseMenu,
    graphics: &mut GraphicsSettings,
    touch: &mut TouchControls,
    map: Option<&mut crate::world_map::WorldMap>,
) {
    if !enabled(action, graphics) {
        return;
    }
    match action {
        Action::Distance(_) => pause.focused = Some(action),
        Action::Quality(quality) => graphics.set_quality(quality),
        Action::NearLess => graphics.adjust_near_distance(-DISTANCE_STEP),
        Action::NearMore => graphics.adjust_near_distance(DISTANCE_STEP),
        Action::TreeLess => graphics.adjust_tree_distance(-TREE_DISTANCE_STEP),
        Action::TreeMore => graphics.adjust_tree_distance(TREE_DISTANCE_STEP),
        Action::ShadowLess => graphics.adjust_shadow_distance(-DISTANCE_STEP),
        Action::ShadowMore => graphics.adjust_shadow_distance(DISTANCE_STEP),
        Action::ResetDistances => graphics.reset_distances(),
        _ => {
            pause.open = false;
            pause.just_closed = true;
            touch.reset();
            match action {
                Action::Map => {
                    if let Some(map) = map {
                        map.requested = true;
                    }
                }
                Action::Inspect => touch.inspect = true,
                Action::Controls => touch.help = true,
                Action::ReturnSpawn => touch.return_spawn = true,
                Action::NextVillage => touch.next_village = true,
                Action::Leave => pause.leave = true,
                _ => {}
            }
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn read(
    mut pause: ResMut<PauseMenu>,
    mut graphics: ResMut<GraphicsSettings>,
    mut touch: ResMut<TouchControls>,
    session: Option<Res<Session>>,
    modals: (
        Option<Res<crate::airships::PilotConversation>>,
        Option<Res<crate::admin_console::AdminConsole>>,
        Option<ResMut<crate::world_map::WorldMap>>,
        Option<Res<crate::market::MarketPanel>>,
    ),
    input: (
        Res<ButtonInput<KeyCode>>,
        Res<ButtonInput<MouseButton>>,
        Res<AccumulatedMouseScroll>,
    ),
    mut native: MessageReader<MenuKey>,
    mut fingers: MessageReader<TouchInput>,
    mut focus_events: MessageReader<WindowFocused>,
    actions: Query<&Action, Changed<crate::ui::Activated>>,
    targets: Query<(
        Entity,
        &Action,
        &ComputedNode,
        &UiGlobalTransform,
        &Node,
        Option<&InheritedVisibility>,
    )>,
    distances: Query<(
        Entity,
        &Distance,
        &ComputedNode,
        &UiGlobalTransform,
        &Node,
        Option<&InheritedVisibility>,
    )>,
    windows: Query<&Window, With<PrimaryWindow>>,
    clipping: Query<&CalculatedClip>,
    mut roots: Query<(&ComputedNode, &mut ScrollPosition), With<PauseRoot>>,
    mut tutorials: Option<ResMut<crate::tutorials::Tutorials>>,
) {
    let (keys, mouse, wheel) = input;
    let (conversation, console, mut map, market) = modals;
    pause.just_closed = false;
    // Drain the full frame: short-circuiting would replay later Back events on
    // the next frame and could reopen the menu immediately after closing it.
    let mut back = false;
    let adjustment_keys = [
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::Home,
        KeyCode::End,
    ];
    let mut adjustments = Vec::new();
    for key in native.read() {
        back |= key.input.state.is_pressed()
            && !key.input.repeat
            && key.input.logical_key == Key::BrowserBack;
        if key.input.state.is_pressed() && adjustment_keys.contains(&key.input.key_code) {
            adjustments.push((key.input.key_code, key.input.repeat));
        }
    }
    // ButtonInput supplies initial presses in headless tests and synthetic
    // input; native events additionally retain desktop held-key repeats.
    for key in adjustment_keys {
        if keys.just_pressed(key) && !adjustments.iter().any(|(candidate, _)| *candidate == key) {
            adjustments.push((key, false));
        }
    }
    let lost_focus = focus_events
        .read()
        .any(|event| !event.focused && windows.get(event.window).is_ok());
    if session.is_none() {
        *pause = PauseMenu::default();
        fingers.clear();
        return;
    }
    let was_open = pause.open;
    if session.as_ref().is_some_and(|s| s.inventory.input_blocked)
        || console.is_some_and(|console| console.input_blocked)
        || market.is_some_and(|market| market.open || market.input_blocked)
        || map
            .as_ref()
            .is_some_and(|map| map.open || map.input_blocked)
    {
        pause.input_blocked = false;
        pause.scroll_finger = None;
        pause.distance_drag = None;
        pause.keyboard_distance = None;
        touch.reset();
        fingers.clear();
        return;
    }
    if lost_focus || touch.suspended || !windows.iter().any(|window| window.focused) {
        pause.input_blocked = true;
        pause.scroll_finger = None;
        pause.distance_drag = None;
        pause.keyboard_distance = None;
        touch.menu_open = pause.open;
        fingers.clear();
        return;
    }
    if touch.menu_open && !pause.open {
        pause.open = true;
    }
    let toggle = (keys.just_pressed(KeyCode::Escape) || back)
        && !conversation
            .as_ref()
            .is_some_and(|dialog| dialog.open() || dialog.just_closed);
    if toggle {
        pause.open = !pause.open;
        pause.just_closed = was_open && !pause.open;
    }
    pause.input_blocked = was_open || pause.open || toggle;
    if was_open != pause.open {
        pause.focused = Some(Action::Resume);
        pause.scroll_finger = None;
        pause.distance_drag = None;
        pause.keyboard_distance = None;
        touch.reset();
    }
    if !pause.open || !was_open {
        fingers.clear();
        touch.menu_open = pause.open;
        return;
    }
    let mut chosen = None;
    let mut scroll_delta = -wheel.delta.y * 28.;
    let distance_at = |point: Vec2| {
        distances
            .iter()
            .find(
                |(entity, distance, computed, transform, node, visibility)| {
                    distance.enabled(&graphics)
                        && node.display != Display::None
                        && visibility.is_none_or(|visible| visible.get())
                        && computed.contains_point(**transform, point)
                        && clipping
                            .get(*entity)
                            .map_or(true, |clip| clip.contains_point(point))
                },
            )
            .map(|(_, distance, computed, transform, _, _)| {
                (*distance, track_fraction(point, computed, transform))
            })
    };
    let drag_fraction = |distance: Distance, point: Vec2| {
        distances
            .iter()
            .find(|(_, candidate, _, _, _, _)| **candidate == distance)
            .map(|(_, _, computed, transform, _, _)| track_fraction(point, computed, transform))
    };
    let mut finished_drag = None;
    // Bevy may synthesize a button activation from a touch while preferring
    // a stale desktop mouse position. Raw touches own their actual targets,
    // including their release frame and frames between movement events.
    let mut native_touch = pause.scroll_finger.is_some()
        || pause
            .distance_drag
            .is_some_and(|drag| drag.finger.is_some());
    for finger in fingers.read() {
        let Ok(window) = windows.get(finger.window) else {
            continue;
        };
        native_touch = true;
        match finger.phase {
            TouchPhase::Started => {
                // One contact owns a drag until it ends. A second finger
                // must not replace its preview, scroll or activate a button.
                if pause.distance_drag.is_some() {
                    continue;
                }
                let point = finger.position * window.scale_factor();
                if let Some((distance, fraction)) = distance_at(point) {
                    pause.focused = Some(Action::Distance(distance));
                    pause.keyboard_distance = None;
                    pause.distance_drag = Some(DistanceDrag {
                        distance,
                        finger: Some(finger.id),
                        fraction,
                    });
                    pause.scroll_finger = None;
                } else if let Some((_, action, _, _, _, _)) =
                    targets
                        .iter()
                        .find(|(entity, _, computed, transform, node, visibility)| {
                            node.display != Display::None
                                && visibility.is_none_or(|visible| visible.get())
                                && computed.contains_point(**transform, point)
                                && clipping
                                    .get(*entity)
                                    .map_or(true, |clip| clip.contains_point(point))
                        })
                {
                    chosen = Some(*action);
                    pause.focused = Some(*action);
                } else {
                    pause.scroll_finger = Some((finger.id, finger.position));
                }
            }
            TouchPhase::Moved => {
                if let Some(drag) = pause.distance_drag.as_mut()
                    && drag.finger == Some(finger.id)
                    && let Some(fraction) =
                        drag_fraction(drag.distance, finger.position * window.scale_factor())
                {
                    drag.fraction = fraction;
                }
                if let Some((id, previous)) = pause.scroll_finger
                    && id == finger.id
                {
                    scroll_delta += previous.y - finger.position.y;
                    pause.scroll_finger = Some((id, finger.position));
                }
            }
            TouchPhase::Ended | TouchPhase::Canceled => {
                if pause
                    .distance_drag
                    .is_some_and(|drag| drag.finger == Some(finger.id))
                {
                    let mut drag = pause.distance_drag.take().unwrap();
                    if finger.phase == TouchPhase::Ended {
                        if let Some(fraction) =
                            drag_fraction(drag.distance, finger.position * window.scale_factor())
                        {
                            drag.fraction = fraction;
                        }
                        finished_drag = Some(drag);
                    }
                }
                if pause.scroll_finger.is_some_and(|(id, _)| id == finger.id) {
                    pause.scroll_finger = None;
                }
            }
        }
    }
    if !cfg!(target_os = "android") && !native_touch {
        let cursor = windows
            .single()
            .ok()
            .and_then(|window| window.physical_cursor_position());
        if mouse.just_pressed(MouseButton::Left)
            && pause.distance_drag.is_none()
            && let Some((distance, fraction)) = cursor.and_then(distance_at)
        {
            pause.focused = Some(Action::Distance(distance));
            pause.keyboard_distance = None;
            pause.distance_drag = Some(DistanceDrag {
                distance,
                finger: None,
                fraction,
            });
        }
        if let Some(drag) = pause.distance_drag.as_mut()
            && drag.finger.is_none()
        {
            if let Some(fraction) = cursor.and_then(|point| drag_fraction(drag.distance, point)) {
                drag.fraction = fraction;
            }
            if !mouse.pressed(MouseButton::Left) {
                finished_drag = pause.distance_drag.take();
            }
        }
    }
    // Preview while dragging; apply once on release so a slider does not queue
    // a landscape rebuild and synchronous preferences save on every frame.
    if let Some(drag) = finished_drag {
        drag.distance.set_fraction(drag.fraction, &mut graphics);
    }
    if !cfg!(target_os = "android") && !native_touch && chosen.is_none() {
        chosen = actions
            .iter()
            .find(|action| !matches!(action, Action::Distance(_)))
            .copied();
    }
    let navigating = keys.just_pressed(KeyCode::Tab)
        || keys.just_pressed(KeyCode::ArrowDown)
        || keys.just_pressed(KeyCode::ArrowUp);
    if navigating {
        pause.keyboard_distance = None;
        let choices: Vec<_> = [
            Action::Resume,
            Action::Map,
            Action::Quality(GraphicsQuality::Low),
            Action::Quality(GraphicsQuality::Balanced),
            Action::Quality(GraphicsQuality::High),
            Action::NearLess,
            Action::Distance(Distance::Near),
            Action::NearMore,
            Action::TreeLess,
            Action::Distance(Distance::Trees),
            Action::TreeMore,
            Action::ShadowLess,
            Action::Distance(Distance::Shadows),
            Action::ShadowMore,
            Action::ResetDistances,
            Action::Inspect,
            Action::Controls,
            Action::Tutorials,
            Action::ReturnSpawn,
            Action::NextVillage,
            Action::Leave,
        ]
        .into_iter()
        .filter(|action| {
            enabled(*action, &graphics)
                && targets
                    .iter()
                    .any(|(_, candidate, _, _, _, _)| candidate == action)
        })
        .collect();
        if !choices.is_empty() {
            let index = choices
                .iter()
                .position(|action| Some(*action) == pause.focused)
                .unwrap_or(0);
            let backwards = keys.just_pressed(KeyCode::ArrowUp)
                || (keys.just_pressed(KeyCode::Tab)
                    && (keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)));
            let next = if backwards {
                (index + choices.len() - 1) % choices.len()
            } else {
                (index + 1) % choices.len()
            };
            pause.focused = Some(choices[next]);
        }
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
        chosen = pause.focused.or(Some(Action::Resume));
    }
    if !navigating
        && chosen.is_none()
        && pause.distance_drag.is_none()
        && let Some(Action::Distance(distance)) = pause.focused
        && distance.enabled(&graphics)
    {
        let (min, max) = distance.bounds();
        let step = if distance == Distance::Near {
            DISTANCE_STEP
        } else {
            1.
        };
        for (key, repeat) in adjustments {
            // A held key cannot begin changing a newly selected bar after a
            // focus change, canceled preview or window reactivation.
            if repeat && pause.keyboard_distance.is_none() {
                continue;
            }
            let value = distance.displayed_value(&pause, &graphics);
            let value = match key {
                KeyCode::ArrowLeft => value - step,
                KeyCode::ArrowRight => value + step,
                KeyCode::Home => min,
                KeyCode::End => max,
                _ => unreachable!(),
            };
            pause.keyboard_distance = Some((distance, value.clamp(min, max)));
        }
        if !keys.any_pressed(adjustment_keys)
            && let Some((distance, value)) = pause.keyboard_distance.take()
        {
            distance.set_value(value, &mut graphics);
        }
    }
    for (computed, mut scroll) in &mut roots {
        let max = (computed.content_size.y - computed.size.y) * computed.inverse_scale_factor;
        scroll.0.y = (scroll.0.y + scroll_delta).clamp(0., max.max(0.));
    }
    if let Some(action) = chosen {
        if action == Action::Tutorials
            && let Some(t) = tutorials.as_deref_mut()
        {
            t.reset();
        }
        pause.distance_drag = None;
        pause.keyboard_distance = None;
        pause.focused = Some(action);
        activate(
            action,
            &mut pause,
            &mut graphics,
            &mut touch,
            map.as_deref_mut(),
        );
    }
    touch.menu_open = pause.open;
}

fn track_fraction(point: Vec2, computed: &ComputedNode, transform: &UiGlobalTransform) -> f32 {
    let local = transform.affine().inverse().transform_point2(point);
    (local.x / computed.size.x.max(1.) + 0.5).clamp(0., 1.)
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn refresh(
    pause: Res<PauseMenu>,
    graphics: Res<GraphicsSettings>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut roots: Query<(&mut Node, &ComputedNode, &mut ScrollPosition), With<PauseRoot>>,
    mut layout: Query<
        (
            &mut Node,
            Option<&PauseContent>,
            Option<&PauseActions>,
            Option<&PausePanel>,
            Option<&CompactNote>,
        ),
        (Without<PauseRoot>, Without<DistanceFill>),
    >,
    mut texts: Query<(
        &mut Text,
        Option<&NearText>,
        Option<&ShadowText>,
        Option<&TreeText>,
        Option<&QualityNote>,
    )>,
    mut buttons: Query<(
        &Action,
        Has<Pressed>,
        &Hovered,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut fills: Query<
        (&DistanceFill, &mut Node, &mut BackgroundColor),
        (Without<Action>, Without<PauseRoot>),
    >,
) {
    let wide = window.width() >= 620.;
    let compact = window.height() < 500.;
    for (mut root, computed, mut scroll) in &mut roots {
        root.padding = UiRect::all(px(if window.height() < 500. { 8. } else { 16. }));
        root.display = if pause.open {
            Display::Flex
        } else {
            Display::None
        };
        root.justify_content = if window.height() < 500. {
            JustifyContent::FlexStart
        } else {
            JustifyContent::Center
        };
        let max = (computed.content_size.y - computed.size.y) * computed.inverse_scale_factor;
        scroll.0.y = scroll.0.y.clamp(0., max.max(0.));
    }
    for (mut node, content, actions, panel, note) in &mut layout {
        if panel.is_some() {
            node.padding = UiRect::all(px(if compact { 8. } else { 16. }));
        }
        if note.is_some() {
            node.display = if compact {
                Display::None
            } else {
                Display::Flex
            };
        }
        if content.is_some() {
            node.flex_direction = if wide {
                FlexDirection::Row
            } else {
                FlexDirection::Column
            };
        }
        if actions.is_some() {
            node.width = if wide { px(220) } else { percent(100) };
        }
    }
    for (mut text, near, shadow, tree, note) in &mut texts {
        let value = if near.is_some() {
            format!("{:.0} m", Distance::Near.displayed_value(&pause, &graphics))
        } else if shadow.is_some() {
            if graphics.quality.shadows() {
                format!(
                    "{:.0} m",
                    Distance::Shadows.displayed_value(&pause, &graphics)
                )
            } else {
                "Off in Low".into()
            }
        } else if tree.is_some() {
            format!(
                "{:.0} m",
                Distance::Trees.displayed_value(&pause, &graphics)
            )
        } else if note.is_some() {
            match graphics.quality {
                GraphicsQuality::Low => "Shaded terrain · dynamic shadows off",
                GraphicsQuality::Balanced => "Nearby sun shadows · no antialiasing",
                GraphicsQuality::High => "Sharper shadows · 4× antialiasing",
            }
            .into()
        } else {
            continue;
        };
        crate::ui::set_text(&mut text, value);
    }
    for (DistanceFill(distance), mut node, mut background) in &mut fills {
        let (min, max) = distance.bounds();
        node.width = percent(
            100. * ((distance.displayed_value(&pause, &graphics) - min) / (max - min))
                .clamp(0., 1.),
        );
        background.0 = if distance.enabled(&graphics) {
            accent()
        } else {
            Color::srgb(0.24, 0.30, 0.27)
        };
    }
    for (action, pressed, hovered, mut background, mut border) in &mut buttons {
        let selected = matches!(action, Action::Quality(quality) if *quality == graphics.quality);
        let enabled = enabled(*action, &graphics);
        background.0 = if matches!(action, Action::Distance(_)) {
            Color::NONE
        } else if !enabled {
            Color::srgb(0.12, 0.20, 0.19)
        } else if pressed {
            Color::srgb(0.34, 0.44, 0.28)
        } else if selected || hovered.0 {
            Color::srgb(0.27, 0.43, 0.36)
        } else {
            Color::srgb(0.19, 0.34, 0.31)
        };
        border.set_all(if enabled && pause.focused == Some(*action) {
            accent()
        } else {
            Color::NONE
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::{protocol::SessionMode, world::CELL_SIZE};

    fn menu_app() -> (App, Entity) {
        let (_, session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(SessionMode::Player),
            "pause test".into(),
            GraphicsQuality::Balanced,
            0.,
            SessionMode::Player,
        )
        .unwrap();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(GraphicsSettings::new(GraphicsQuality::Balanced))
            .insert_resource(TouchControls::new(true))
            .insert_resource(PauseMenu {
                open: true,
                focused: Some(Action::Resume),
                ..default()
            })
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseScroll>()
            .add_message::<MenuKey>()
            .add_message::<TouchInput>()
            .add_message::<WindowFocused>()
            .add_systems(Update, read);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        (app, window)
    }

    fn target(app: &mut App, action: Action) -> Entity {
        app.world_mut()
            .spawn((
                action,
                Hovered::default(),
                Node::default(),
                InheritedVisibility::VISIBLE,
                ComputedNode {
                    size: Vec2::splat(64.),
                    ..default()
                },
                UiGlobalTransform::from_xy(100., 100.),
            ))
            .id()
    }

    fn tap(app: &mut App, window: Entity) {
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            app.world_mut().write_message(TouchInput {
                id: 7,
                phase,
                position: Vec2::splat(100.),
                window,
                force: None,
            });
        }
        app.update();
    }

    #[test]
    fn a_short_touch_changes_distance_once_without_a_mouse_press() {
        let (mut app, window) = menu_app();
        target(&mut app, Action::NearMore);
        for (before, after) in [(48., 56.), (96., 104.), (992., 1000.), (1000., 1000.)] {
            app.world_mut()
                .resource_mut::<GraphicsSettings>()
                .near_distance = before;
            tap(&mut app, window);
            assert_eq!(
                app.world().resource::<GraphicsSettings>().near_distance,
                after
            );
        }
        let pause = app.world().resource::<PauseMenu>();
        assert!(pause.open && pause.input_blocked);
    }

    #[test]
    fn tree_touch_control_reaches_5000_blocks_and_presets_retain_distance() {
        let (mut app, window) = menu_app();
        app.world_mut()
            .resource_mut::<GraphicsSettings>()
            .set_quality(GraphicsQuality::Low);
        let more = target(&mut app, Action::TreeMore);
        assert!(!enabled(
            Action::TreeLess,
            app.world().resource::<GraphicsSettings>()
        ));
        for _ in 0..19 {
            tap(&mut app, window);
        }
        let graphics = app.world().resource::<GraphicsSettings>();
        assert_eq!(graphics.tree_distance / CELL_SIZE, 5000.0);
        assert!(!enabled(Action::TreeMore, graphics));
        tap(&mut app, window);
        assert_eq!(
            app.world().resource::<GraphicsSettings>().tree_distance,
            MAX_TREE_DISTANCE
        );
        app.world_mut()
            .resource_mut::<GraphicsSettings>()
            .set_quality(GraphicsQuality::High);
        assert_eq!(
            app.world().resource::<GraphicsSettings>().tree_distance,
            MAX_TREE_DISTANCE
        );
        app.world_mut().despawn(more);
        target(&mut app, Action::TreeLess);
        tap(&mut app, window);
        assert_eq!(
            app.world().resource::<GraphicsSettings>().tree_distance,
            MAX_TREE_DISTANCE - TREE_DISTANCE_STEP
        );
        assert!(app.world().resource::<PauseMenu>().open);
    }

    #[test]
    fn android_back_drains_the_frame_and_ignores_held_key_repeats() {
        use bevy::input::{ButtonState, keyboard::KeyboardInput};
        use winit::keyboard::ModifiersState;
        let (mut app, window) = menu_app();
        let send = |app: &mut App, state, repeat| {
            app.world_mut().write_message(MenuKey {
                input: KeyboardInput {
                    key_code: KeyCode::Escape,
                    logical_key: Key::BrowserBack,
                    text: None,
                    state,
                    repeat,
                    window,
                },
                modifiers: ModifiersState::empty(),
            });
        };
        for (state, repeat) in [
            (ButtonState::Pressed, false),
            (ButtonState::Pressed, false),
            (ButtonState::Pressed, true),
            (ButtonState::Released, false),
        ] {
            send(&mut app, state, repeat);
        }
        app.update();
        let pause = app.world().resource::<PauseMenu>();
        assert!(!pause.open && pause.just_closed && pause.input_blocked);
        app.update();
        let pause = app.world().resource::<PauseMenu>();
        assert!(
            !pause.open && !pause.just_closed && !pause.input_blocked,
            "queued Back events must not reopen pause next frame"
        );
        send(&mut app, ButtonState::Pressed, true);
        app.update();
        assert!(!app.world().resource::<PauseMenu>().open);
        send(&mut app, ButtonState::Pressed, false);
        app.update();
        assert!(app.world().resource::<PauseMenu>().open);
        send(&mut app, ButtonState::Pressed, true);
        app.update();
        assert!(app.world().resource::<PauseMenu>().open);
    }

    #[test]
    fn keyboard_navigation_skips_disabled_tree_decrease_and_increases_distance() {
        let (mut app, _) = menu_app();
        target(&mut app, Action::Resume);
        target(&mut app, Action::TreeLess);
        target(&mut app, Action::TreeMore);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Tab);
        app.update();
        assert_eq!(
            app.world().resource::<PauseMenu>().focused,
            Some(Action::TreeMore)
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.press(KeyCode::Enter);
        }
        app.update();
        assert_eq!(
            app.world().resource::<GraphicsSettings>().tree_distance,
            MIN_TREE_DISTANCE + TREE_DISTANCE_STEP
        );
        assert!(app.world().resource::<PauseMenu>().open);
    }

    #[test]
    fn mouse_tree_control_applies_a_changed_press_once() {
        let (mut app, _) = menu_app();
        let more = target(&mut app, Action::TreeMore);
        app.world_mut()
            .entity_mut(more)
            .insert(crate::ui::Activated);
        app.update();
        assert_eq!(
            app.world().resource::<GraphicsSettings>().tree_distance,
            MIN_TREE_DISTANCE + TREE_DISTANCE_STEP
        );
        app.update();
        assert_eq!(
            app.world().resource::<GraphicsSettings>().tree_distance,
            MIN_TREE_DISTANCE + TREE_DISTANCE_STEP
        );
    }

    #[test]
    fn tree_distance_label_uses_meters_through_the_cap() {
        let (mut app, _) = menu_app();
        app.add_systems(Update, refresh);
        let value = app.world_mut().spawn((Text::new(""), TreeText)).id();
        app.update();
        assert_eq!(app.world().get::<Text>(value).unwrap().0, "128 m");
        app.world_mut()
            .resource_mut::<GraphicsSettings>()
            .adjust_tree_distance(10000.0);
        app.update();
        assert_eq!(app.world().get::<Text>(value).unwrap().0, "2500 m");
    }

    fn distance_target(app: &mut App, distance: Distance) -> Entity {
        app.world_mut()
            .spawn((
                distance,
                Action::Distance(distance),
                Hovered::default(),
                BackgroundColor(Color::NONE),
                BorderColor::all(Color::NONE),
                Node::default(),
                InheritedVisibility::VISIBLE,
                ComputedNode {
                    size: Vec2::new(200., 44.),
                    ..default()
                },
                UiGlobalTransform::from_xy(200., 100.),
            ))
            .id()
    }

    fn native_adjustment(app: &mut App, window: Entity, key: KeyCode, repeat: bool) {
        use bevy::input::{ButtonState, keyboard::KeyboardInput};
        use winit::keyboard::ModifiersState;
        app.world_mut().write_message(MenuKey {
            input: KeyboardInput {
                key_code: key,
                logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
                text: None,
                state: ButtonState::Pressed,
                repeat,
                window,
            },
            modifiers: ModifiersState::empty(),
        });
    }

    #[test]
    fn selected_distance_keys_preview_fine_steps_and_apply_only_on_release() {
        for (distance, before, step) in [
            (Distance::Near, 48., 8.),
            (Distance::Trees, 128., 1.),
            (Distance::Shadows, 32., 1.),
        ] {
            let (mut app, window) = menu_app();
            target(&mut app, Action::Resume);
            let bar = distance_target(&mut app, distance);
            app.add_systems(Update, refresh.after(read));
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Tab);
            app.update();
            assert_eq!(
                app.world().resource::<PauseMenu>().focused,
                Some(Action::Distance(distance))
            );
            assert_eq!(app.world().get::<BorderColor>(bar).unwrap().top, accent());
            {
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.clear();
                keys.press(KeyCode::ArrowRight);
            }
            // The native initial press and ButtonInput describe one step.
            native_adjustment(&mut app, window, KeyCode::ArrowRight, false);
            app.update();
            for repeats in 0..=2 {
                if repeats > 0 {
                    app.world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>()
                        .clear();
                    native_adjustment(&mut app, window, KeyCode::ArrowRight, true);
                    app.update();
                }
                assert_eq!(
                    distance.value(app.world().resource::<GraphicsSettings>()),
                    before
                );
                assert_eq!(
                    distance.displayed_value(
                        app.world().resource::<PauseMenu>(),
                        app.world().resource::<GraphicsSettings>()
                    ),
                    before + step * (repeats + 1) as f32
                );
            }
            {
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.clear();
                keys.release(KeyCode::ArrowRight);
            }
            app.update();
            assert_eq!(
                distance.value(app.world().resource::<GraphicsSettings>()),
                before + 3. * step
            );
            assert!(
                app.world()
                    .resource::<PauseMenu>()
                    .keyboard_distance
                    .is_none()
            );
            app.update();
            assert_eq!(
                distance.value(app.world().resource::<GraphicsSettings>()),
                before + 3. * step
            );
        }
    }

    #[test]
    fn selected_distance_home_end_and_arrows_respect_bounds_and_disabled_shadows() {
        for distance in [Distance::Near, Distance::Trees, Distance::Shadows] {
            let (mut app, _) = menu_app();
            distance_target(&mut app, distance);
            app.world_mut().resource_mut::<PauseMenu>().focused = Some(Action::Distance(distance));
            let (min, max) = distance.bounds();
            for (key, expected) in [
                (KeyCode::End, max),
                (KeyCode::ArrowRight, max),
                (KeyCode::Home, min),
                (KeyCode::ArrowLeft, min),
            ] {
                {
                    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                    keys.clear();
                    keys.press(key);
                }
                app.update();
                {
                    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                    keys.clear();
                    keys.release(key);
                }
                app.update();
                assert_eq!(
                    distance.value(app.world().resource::<GraphicsSettings>()),
                    expected
                );
            }
            app.world_mut()
                .resource_mut::<GraphicsSettings>()
                .set_quality(GraphicsQuality::Low);
            app.world_mut().resource_mut::<PauseMenu>().focused =
                Some(Action::Distance(Distance::Shadows));
            let before = app.world().resource::<GraphicsSettings>().shadow_distance;
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::End);
            app.update();
            assert!(
                app.world()
                    .resource::<PauseMenu>()
                    .keyboard_distance
                    .is_none()
            );
            assert_eq!(
                app.world().resource::<GraphicsSettings>().shadow_distance,
                before
            );
        }
    }

    #[test]
    fn unfinished_keyboard_distance_is_discarded_on_focus_modal_close_or_navigation() {
        for interruption in ["focus", "suspend", "map", "console", "escape", "tab"] {
            let (mut app, window) = menu_app();
            distance_target(&mut app, Distance::Trees);
            target(&mut app, Action::Resume);
            app.world_mut().resource_mut::<PauseMenu>().focused =
                Some(Action::Distance(Distance::Trees));
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::End);
            app.update();
            assert!(
                app.world()
                    .resource::<PauseMenu>()
                    .keyboard_distance
                    .is_some()
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
            match interruption {
                "focus" => {
                    app.world_mut().write_message(WindowFocused {
                        window,
                        focused: false,
                    });
                }
                "suspend" => app.world_mut().resource_mut::<TouchControls>().suspended = true,
                "map" => {
                    let mut map = crate::world_map::WorldMap::default();
                    map.open = true;
                    app.insert_resource(map);
                }
                "console" => {
                    let mut console = crate::admin_console::AdminConsole::default();
                    console.input_blocked = true;
                    app.insert_resource(console);
                }
                "escape" => app
                    .world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::Escape),
                "tab" => app
                    .world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::Tab),
                _ => unreachable!(),
            }
            app.update();
            assert!(
                app.world()
                    .resource::<PauseMenu>()
                    .keyboard_distance
                    .is_none(),
                "{interruption}"
            );
            assert_eq!(
                app.world().resource::<GraphicsSettings>().tree_distance,
                MIN_TREE_DISTANCE,
                "{interruption}"
            );
            if interruption == "escape" {
                let pause = app.world().resource::<PauseMenu>();
                assert!(pause.just_closed && pause.input_blocked && !pause.open);
            }
        }
    }

    #[test]
    fn held_keys_cannot_begin_an_adjustment_after_another_bar_is_selected() {
        let (mut app, window) = menu_app();
        distance_target(&mut app, Distance::Near);
        distance_target(&mut app, Distance::Trees);
        app.world_mut().resource_mut::<PauseMenu>().focused =
            Some(Action::Distance(Distance::Near));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowRight);
        app.update();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.press(KeyCode::Tab);
        }
        app.update();
        assert_eq!(
            app.world().resource::<PauseMenu>().focused,
            Some(Action::Distance(Distance::Trees))
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        native_adjustment(&mut app, window, KeyCode::ArrowRight, true);
        app.update();
        assert!(
            app.world()
                .resource::<PauseMenu>()
                .keyboard_distance
                .is_none()
        );
        let graphics = app.world().resource::<GraphicsSettings>();
        assert_eq!(graphics.near_distance, 48.);
        assert_eq!(graphics.tree_distance, MIN_TREE_DISTANCE);
    }

    fn drag_touch(app: &mut App, window: Entity, phase: TouchPhase, x: f32) {
        app.world_mut().write_message(TouchInput {
            id: 19,
            phase,
            position: Vec2::new(x, 100.),
            window,
            force: None,
        });
        app.update();
    }

    #[test]
    fn touch_distance_drag_previews_then_applies_once_even_outside_the_bar() {
        let (mut app, window) = menu_app();
        distance_target(&mut app, Distance::Near);
        drag_touch(&mut app, window, TouchPhase::Started, 200.);
        assert_eq!(
            app.world().resource::<GraphicsSettings>().near_distance,
            48.
        );
        assert_eq!(
            Distance::Near.displayed_value(
                app.world().resource::<PauseMenu>(),
                app.world().resource::<GraphicsSettings>()
            ),
            512.
        );
        drag_touch(&mut app, window, TouchPhase::Moved, 450.);
        assert_eq!(
            app.world().resource::<GraphicsSettings>().near_distance,
            48.
        );
        drag_touch(&mut app, window, TouchPhase::Ended, 450.);
        assert_eq!(
            app.world().resource::<GraphicsSettings>().near_distance,
            MAX_NEAR_DISTANCE
        );
        let pause = app.world().resource::<PauseMenu>();
        assert!(pause.open && pause.input_blocked && pause.distance_drag.is_none());
    }

    #[test]
    fn canceled_touch_or_focus_loss_discards_unapplied_distance() {
        for lost_focus in [false, true] {
            let (mut app, window) = menu_app();
            distance_target(&mut app, Distance::Trees);
            drag_touch(&mut app, window, TouchPhase::Started, 290.);
            if lost_focus {
                app.world_mut().write_message(WindowFocused {
                    window,
                    focused: false,
                });
                app.update();
            } else {
                drag_touch(&mut app, window, TouchPhase::Canceled, 290.);
            }
            assert!(app.world().resource::<PauseMenu>().distance_drag.is_none());
            assert_eq!(
                app.world().resource::<GraphicsSettings>().tree_distance,
                MIN_TREE_DISTANCE
            );
        }
    }

    #[test]
    fn a_second_finger_cannot_steal_a_slider_or_trigger_another_action() {
        let (mut app, window) = menu_app();
        distance_target(&mut app, Distance::Near);
        distance_target(&mut app, Distance::Trees);
        target(&mut app, Action::Resume);
        drag_touch(&mut app, window, TouchPhase::Started, 250.);
        let drag = app.world().resource::<PauseMenu>().distance_drag.unwrap();
        for x in [200., 100.] {
            for phase in [TouchPhase::Started, TouchPhase::Ended] {
                app.world_mut().write_message(TouchInput {
                    id: 20,
                    phase,
                    position: Vec2::new(x, 100.),
                    window,
                    force: None,
                });
            }
            app.update();
            let pause = app.world().resource::<PauseMenu>();
            assert!(pause.open);
            assert_eq!(pause.distance_drag.unwrap().finger, Some(19));
            assert_eq!(pause.distance_drag.unwrap().distance, drag.distance);
            assert!(pause.scroll_finger.is_none());
        }
        drag_touch(&mut app, window, TouchPhase::Ended, 300.);
        let graphics = app.world().resource::<GraphicsSettings>();
        assert_eq!(drag.distance.value(graphics), drag.distance.bounds().1);
    }

    #[test]
    fn native_touch_owns_slider_and_release_despite_stale_mouse_interactions() {
        let (mut app, window) = menu_app();
        distance_target(&mut app, Distance::Near);
        let resume = target(&mut app, Action::Resume);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_physical_cursor_position(Some(bevy::math::DVec2::splat(100.)));
        app.world_mut()
            .entity_mut(resume)
            .insert(crate::ui::Activated);
        drag_touch(&mut app, window, TouchPhase::Started, 250.);
        assert!(app.world().resource::<PauseMenu>().open);
        assert_eq!(
            app.world()
                .resource::<PauseMenu>()
                .distance_drag
                .unwrap()
                .finger,
            Some(19)
        );
        // A held contact still owns input on a frame with no raw movement.
        app.world_mut()
            .entity_mut(resume)
            .insert(crate::ui::Activated);
        app.update();
        assert!(app.world().resource::<PauseMenu>().distance_drag.is_some());
        app.world_mut()
            .entity_mut(resume)
            .insert(crate::ui::Activated);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_physical_cursor_position(Some(bevy::math::DVec2::new(200., 100.)));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        drag_touch(&mut app, window, TouchPhase::Ended, 300.);
        assert!(app.world().resource::<PauseMenu>().open);
        assert!(app.world().resource::<PauseMenu>().distance_drag.is_none());
        assert_eq!(
            app.world().resource::<GraphicsSettings>().near_distance,
            MAX_NEAR_DISTANCE
        );
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
        // Even a short blank-space tap must not click the old mouse target.
        app.world_mut()
            .entity_mut(resume)
            .insert(crate::ui::Activated);
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            app.world_mut().write_message(TouchInput {
                id: 22,
                phase,
                position: Vec2::new(400., 200.),
                window,
                force: None,
            });
        }
        app.update();
        assert!(app.world().resource::<PauseMenu>().open);
        assert!(app.world().resource::<PauseMenu>().scroll_finger.is_none());
        // A subsequent real mouse press remains usable.
        app.world_mut()
            .entity_mut(resume)
            .insert(crate::ui::Activated);
        app.update();
        assert!(!app.world().resource::<PauseMenu>().open);
    }

    #[test]
    fn mouse_distance_drag_releases_at_the_last_value_and_low_disables_shadows() {
        let (mut app, window) = menu_app();
        distance_target(&mut app, Distance::Trees);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_physical_cursor_position(Some(bevy::math::DVec2::new(200., 100.)));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert_eq!(
            app.world().resource::<GraphicsSettings>().tree_distance,
            MIN_TREE_DISTANCE
        );
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        assert_eq!(
            app.world().resource::<GraphicsSettings>().tree_distance,
            1314.
        );
        let mut graphics = app.world_mut().resource_mut::<GraphicsSettings>();
        graphics.set_quality(GraphicsQuality::Low);
        let before = graphics.shadow_distance;
        Distance::Shadows.set_fraction(1., &mut graphics);
        assert_eq!(graphics.shadow_distance, before);
    }

    #[test]
    fn resetting_distances_keeps_quality_and_the_menu_open() {
        let (mut app, window) = menu_app();
        target(&mut app, Action::ResetDistances);
        {
            let mut graphics = app.world_mut().resource_mut::<GraphicsSettings>();
            graphics.set_quality(GraphicsQuality::High);
            graphics.near_distance = MAX_NEAR_DISTANCE;
            graphics.tree_distance = MAX_TREE_DISTANCE;
            graphics.shadow_distance = MAX_SHADOW_DISTANCE;
        }
        tap(&mut app, window);
        let graphics = app.world().resource::<GraphicsSettings>();
        assert_eq!(graphics.quality, GraphicsQuality::High);
        assert_eq!(graphics.near_distance, 48.);
        assert_eq!(graphics.tree_distance, 128.);
        assert_eq!(
            graphics.shadow_distance,
            GraphicsSettings::new(GraphicsQuality::High).shadow_distance
        );
        assert!(app.world().resource::<PauseMenu>().open);
    }

    #[test]
    fn resume_clears_gameplay_input_and_blocks_the_closing_frame() {
        let (mut app, window) = menu_app();
        target(&mut app, Action::Resume);
        {
            let mut touch = app.world_mut().resource_mut::<TouchControls>();
            touch.movement = Vec2::ONE;
            touch.look = Vec2::ONE;
            touch.dig = true;
        }
        tap(&mut app, window);
        let pause = app.world().resource::<PauseMenu>();
        assert!(!pause.open && pause.input_blocked && pause.just_closed);
        let touch = app.world().resource::<TouchControls>();
        assert_eq!(touch.movement, Vec2::ZERO);
        assert_eq!(touch.look, Vec2::ZERO);
        assert!(!touch.dig && !touch.menu_open);
        app.update();
        assert!(!app.world().resource::<PauseMenu>().input_blocked);
        assert!(!app.world().resource::<PauseMenu>().just_closed);
    }

    #[test]
    fn map_button_requests_map_and_cancels_pause_input_for_touch_and_keyboard() {
        for keyboard in [false, true] {
            let (mut app, window) = menu_app();
            app.init_resource::<crate::world_map::WorldMap>();
            target(&mut app, Action::Map);
            {
                let mut touch = app.world_mut().resource_mut::<TouchControls>();
                touch.movement = Vec2::ONE;
                touch.dig = true;
            }
            if keyboard {
                app.world_mut().resource_mut::<PauseMenu>().focused = Some(Action::Map);
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::Enter);
                app.update();
            } else {
                tap(&mut app, window);
            }
            assert!(
                app.world()
                    .resource::<crate::world_map::WorldMap>()
                    .requested
            );
            let pause = app.world().resource::<PauseMenu>();
            assert!(!pause.open && pause.input_blocked && pause.just_closed);
            let touch = app.world().resource::<TouchControls>();
            assert!(!touch.menu_open && !touch.dig);
            assert_eq!(touch.movement, Vec2::ZERO);
        }
    }

    #[test]
    fn escape_does_not_open_pause_while_map_is_open_or_closing() {
        for open in [false, true] {
            let (mut app, _) = menu_app();
            app.world_mut().resource_mut::<PauseMenu>().open = false;
            let mut map = crate::world_map::WorldMap::default();
            map.open = open;
            map.input_blocked = true;
            app.insert_resource(map);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
            app.update();
            assert!(!app.world().resource::<PauseMenu>().open);
        }
    }

    #[test]
    fn touch_lifecycle_reset_does_not_close_the_common_menu() {
        let (mut app, _) = menu_app();
        app.world_mut().resource_mut::<TouchControls>().reset();
        app.update();
        assert!(app.world().resource::<PauseMenu>().open);
        assert!(app.world().resource::<TouchControls>().menu_open);
    }

    #[test]
    fn focus_loss_and_suspension_drop_scroll_ownership_and_menu_actions() {
        for suspended in [false, true] {
            let (mut app, window) = menu_app();
            target(&mut app, Action::NearMore);
            app.world_mut().resource_mut::<PauseMenu>().scroll_finger = Some((7, Vec2::ZERO));
            if suspended {
                app.world_mut().resource_mut::<TouchControls>().suspended = true;
            } else {
                app.world_mut().write_message(WindowFocused {
                    window,
                    focused: false,
                });
            }
            tap(&mut app, window);
            let pause = app.world().resource::<PauseMenu>();
            assert!(pause.open && pause.input_blocked);
            assert!(!pause.just_closed);
            assert!(pause.scroll_finger.is_none());
            assert_eq!(
                app.world().resource::<GraphicsSettings>().near_distance,
                48.
            );
        }
    }

    #[test]
    fn low_quality_disables_shadow_adjustment_but_keeps_near_detail_control() {
        let (mut app, window) = menu_app();
        app.world_mut()
            .resource_mut::<GraphicsSettings>()
            .set_quality(GraphicsQuality::Low);
        target(&mut app, Action::ShadowMore);
        let before = app.world().resource::<GraphicsSettings>().shadow_distance;
        tap(&mut app, window);
        assert_eq!(
            app.world().resource::<GraphicsSettings>().shadow_distance,
            before
        );
    }

    #[test]
    fn keyboard_navigation_selects_and_activates_a_quality_button() {
        let (mut app, _) = menu_app();
        target(&mut app, Action::Resume);
        target(&mut app, Action::Quality(GraphicsQuality::Low));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Tab);
        app.update();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.press(KeyCode::Enter);
        }
        app.update();
        assert_eq!(
            app.world().resource::<GraphicsSettings>().quality,
            GraphicsQuality::Low
        );
        assert!(app.world().resource::<PauseMenu>().open);
    }
}
