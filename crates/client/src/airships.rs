//! Scheduled vehicles share the server's deterministic clock. Dialogue uses the
//! same direct desktop/touch controls as the pause menu.
use std::collections::HashMap;

use bevy::{
    input::{
        keyboard::Key,
        mouse::AccumulatedMouseScroll,
        touch::{TouchInput, TouchPhase},
    },
    prelude::*,
    window::PrimaryWindow,
};
use rubblekin_core::{
    airships::{AirshipSnapshot, deck_position, initial_deck_position, pilot_position},
    protocol::ClientMessage,
};

use crate::{
    GameEntity, Session, VoxelWorld, join::MenuKey, network::Connection, pause::PauseMenu,
    touch::TouchControls,
};

pub(crate) const TALK_REACH: f32 = 14.0;

#[derive(Resource, Default)]
pub(crate) struct PilotConversation {
    pub ship_id: Option<u64>,
    pub text: String,
    pub input_blocked: bool,
    pub just_closed: bool,
    answered: bool,
    scroll_finger: Option<(u64, Vec2)>,
}
impl PilotConversation {
    pub fn open(&self) -> bool {
        self.ship_id.is_some()
    }
    pub fn reply(&mut self, ship_id: u64, text: String) {
        if self.ship_id == Some(ship_id) {
            self.text = text;
            self.answered = true;
        }
    }
    fn close(&mut self) {
        self.ship_id = None;
        self.just_closed = true;
        self.input_blocked = true;
        self.scroll_finger = None;
    }
}

#[derive(Resource)]
pub(super) struct Scene {
    ships: HashMap<u64, Entity>,
    cube: Handle<Mesh>,
    balloon: Handle<Mesh>,
    wood: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    cream: Handle<StandardMaterial>,
    gold: Handle<StandardMaterial>,
}
#[derive(Component)]
pub(super) struct Ship;
#[derive(Component)]
pub(super) struct DialogRoot;
#[derive(Component)]
pub(super) struct DialogText;
#[derive(Component)]
pub(super) struct ScheduleText;
#[derive(Component)]
pub(super) struct TravelHint;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    NextPilot,
    Close,
}

pub(super) fn setup(
    mut commands: Commands,
    session: Res<Session>,
    world: Res<VoxelWorld>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut fonts: ResMut<Assets<Font>>,
    mut conversation: ResMut<PilotConversation>,
) {
    *conversation = PilotConversation::default();
    let scene = Scene {
        ships: HashMap::new(),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        balloon: meshes.add(crate::airship_mesh::mesh()),
        wood: materials.add(Color::srgb(0.51, 0.31, 0.14)),
        dark: materials.add(Color::srgb(0.22, 0.25, 0.22)),
        cream: materials.add(Color::srgb(0.88, 0.82, 0.61)),
        gold: materials.add(Color::srgb(0.79, 0.53, 0.18)),
    };
    for ramp in session.airships.ramps() {
        let from = Vec3::from_array(ramp.from);
        let to = Vec3::from_array(ramp.to);
        let direction = (to - from).normalize_or_zero();
        let right = direction.cross(Vec3::Y).normalize_or_zero();
        let normal = right.cross(direction).normalize_or_zero();
        if right.length_squared() > 0.5 {
            commands.spawn((
                GameEntity,
                Mesh3d(scene.cube.clone()),
                MeshMaterial3d(scene.wood.clone()),
                Transform {
                    translation: (from + to) * 0.5 - normal * 0.09,
                    rotation: Quat::from_mat3(&Mat3::from_cols(right, normal, -direction)),
                    scale: Vec3::new(ramp.width, 0.18, from.distance(to)),
                },
            ));
        }
    }
    for port in session.airships.ports() {
        for dock in session.airships.dock_positions(port.village_id) {
            let ground = world.0.surface_height(dock[0], dock[2]);
            let height = (dock[1] - ground + 2.2).max(2.0);
            commands.spawn((
                GameEntity,
                Mesh3d(scene.cube.clone()),
                MeshMaterial3d(scene.wood.clone()),
                Transform::from_xyz(dock[0] + 5.0, ground + height * 0.5, dock[2])
                    .with_scale(Vec3::new(0.4, height, 0.4)),
            ));
            commands.spawn((
                GameEntity,
                Mesh3d(scene.cube.clone()),
                MeshMaterial3d(scene.gold.clone()),
                Transform::from_xyz(dock[0] + 5.0, dock[1] + 1.2, dock[2])
                    .with_scale(Vec3::new(0.12, 2.0, 2.5)),
            ));
        }
        commands
            .spawn((
                GameEntity,
                Transform::from_translation(Vec3::from_array(port.position)),
                Visibility::default(),
            ))
            .with_children(|parent| {
                // A small airship silhouette and boarding arrow identify the
                // port even before a full-sized service reaches its berth.
                parent.spawn((
                    Mesh3d(scene.balloon.clone()),
                    MeshMaterial3d(scene.cream.clone()),
                    port_symbol_transform(),
                ));
                parent.spawn((
                    Mesh3d(scene.cube.clone()),
                    MeshMaterial3d(scene.gold.clone()),
                    Transform::from_xyz(0.0, 4.9, -7.0).with_scale(Vec3::new(1.6, 0.18, 0.4)),
                ));
                parent.spawn((
                    Mesh3d(scene.cube.clone()),
                    MeshMaterial3d(scene.cream.clone()),
                    Transform::from_xyz(0.0, 3.8, -6.75).with_scale(Vec3::new(0.14, 1.15, 0.16)),
                ));
                for (x, rotation) in [
                    (-0.19, -std::f32::consts::FRAC_PI_4),
                    (0.19, std::f32::consts::FRAC_PI_4),
                ] {
                    parent.spawn((
                        Mesh3d(scene.cube.clone()),
                        MeshMaterial3d(scene.cream.clone()),
                        Transform::from_xyz(x, 3.28, -6.75)
                            .with_rotation(Quat::from_rotation_z(rotation))
                            .with_scale(Vec3::new(0.14, 0.6, 0.16)),
                    ));
                }
                for (at, scale, material) in [
                    (
                        Vec3::new(0.0, -0.18, 0.0),
                        Vec3::new(5.0, 0.3, 8.0),
                        scene.wood.clone(),
                    ),
                    (
                        Vec3::new(-4.4, 2.5, -7.0),
                        Vec3::new(0.4, 5.0, 0.4),
                        scene.dark.clone(),
                    ),
                    (
                        Vec3::new(4.4, 2.5, -7.0),
                        Vec3::new(0.4, 5.0, 0.4),
                        scene.dark.clone(),
                    ),
                    (
                        Vec3::new(0.0, 4.7, -7.0),
                        Vec3::new(9.2, 0.35, 0.4),
                        scene.wood.clone(),
                    ),
                    (
                        Vec3::new(-4.4, 4.4, -5.9),
                        Vec3::new(0.1, 1.3, 2.0),
                        scene.gold.clone(),
                    ),
                    (
                        Vec3::new(4.4, 4.4, -5.9),
                        Vec3::new(0.1, 1.3, 2.0),
                        scene.gold.clone(),
                    ),
                ] {
                    parent.spawn((
                        Mesh3d(scene.cube.clone()),
                        MeshMaterial3d(material),
                        Transform::from_translation(at).with_scale(scale),
                    ));
                }
            });
    }
    commands.insert_resource(scene);
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    commands.spawn((
        GameEntity,
        TravelHint,
        GlobalZIndex(24),
        Node {
            position_type: PositionType::Absolute,
            left: px(20),
            bottom: px(88),
            max_width: px(620),
            padding: UiRect::axes(px(12), px(8)),
            border_radius: BorderRadius::all(px(6)),
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgba(0.055, 0.10, 0.10, 0.88)),
        children![(
            Text::new(""),
            TextFont::from_font_size(17.).with_font(font.clone()),
            TextColor(Color::srgb(0.95, 0.88, 0.66))
        )],
    ));
    commands
        .spawn((
            GameEntity,
            DialogRoot,
            GlobalZIndex(90),
            Node {
                display: Display::None,
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(12)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.05, 0.05, 0.38)),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(540),
                    max_width: percent(100),
                    padding: UiRect::all(px(20)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(12),
                    flex_shrink: 0.,
                    border_radius: BorderRadius::all(px(9)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.08, 0.15, 0.14)),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Airship pilot"),
                    TextFont::from_font_size(24.).with_font(font.clone()),
                    TextColor(Color::srgb(0.95, 0.83, 0.56)),
                ));
                panel.spawn((
                    DialogText,
                    Text::new(""),
                    TextFont::from_font_size(18.).with_font(font.clone()),
                    TextColor(Color::srgb(0.89, 0.92, 0.85)),
                ));
                panel.spawn((
                    ScheduleText,
                    Text::new(""),
                    TextFont::from_font_size(16.).with_font(font.clone()),
                    TextColor(Color::srgb(0.76, 0.83, 0.77)),
                ));
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(10),
                        row_gap: px(8),
                        ..default()
                    })
                    .with_children(|row| {
                        for (action, label) in [
                            (Action::NextPilot, "Next pilot [N]"),
                            (Action::Close, "Thanks [Enter / Esc]"),
                        ] {
                            row.spawn((
                                Button,
                                action,
                                Node {
                                    min_height: px(44),
                                    padding: UiRect::axes(px(14), px(9)),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    border_radius: BorderRadius::all(px(6)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.20, 0.35, 0.30)),
                            ))
                            .with_child((
                                Text::new(label),
                                TextFont::from_font_size(16.).with_font(font.clone()),
                                TextColor(Color::srgb(0.93, 0.94, 0.86)),
                            ));
                        }
                    });
            });
        });
}

fn port_symbol_transform() -> Transform {
    // The full envelope is about 22m long and centered 9.4m above its
    // deck origin. Scale and center its silhouette independently of the ship.
    Transform::from_xyz(0.0, 5.8 - 9.4 * 0.15, -7.0)
        .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
        .with_scale(Vec3::new(0.17, 0.15, 0.14))
}

pub(crate) fn talk_target(session: &Session) -> Option<u64> {
    if session.observer.is_some() {
        return None;
    }
    talk_candidates(session).first().copied()
}

fn talk_candidates(session: &Session) -> Vec<u64> {
    let position = Vec3::from_array(session.body.position);
    let mut ships: Vec<_> = session
        .airships
        .ships(session.airship_time)
        .into_iter()
        .filter_map(|ship| {
            let distance = position.distance_squared(Vec3::from_array(pilot_position(&ship)));
            (distance <= TALK_REACH * TALK_REACH).then_some((ship.id, distance))
        })
        .collect();
    ships.sort_unstable_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    ships.into_iter().map(|(id, _)| id).collect()
}

fn nearby_port(session: &Session) -> Option<u32> {
    let p = Vec3::from_array(session.body.position);
    session
        .airships
        .ports()
        .iter()
        .filter_map(|port| {
            let distance = p.distance_squared(Vec3::from_array(port.position));
            (distance <= TALK_REACH * TALK_REACH).then_some((port.village_id, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn read(
    mut conversation: ResMut<PilotConversation>,
    mut session: ResMut<Session>,
    mut connection: ResMut<Connection>,
    mut touch: ResMut<TouchControls>,
    pause: Option<Res<PauseMenu>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    wheel: Res<AccumulatedMouseScroll>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut native: MessageReader<MenuKey>,
    mut fingers: MessageReader<TouchInput>,
    actions: Query<(&Action, &Interaction), Changed<Interaction>>,
    targets: Query<(
        Entity,
        &Action,
        &ComputedNode,
        &UiGlobalTransform,
        &Node,
        Option<&InheritedVisibility>,
    )>,
    clipping: Query<(&ComputedNode, &UiGlobalTransform, &Node)>,
    parents: Query<&ChildOf, Without<bevy::ui::OverrideClip>>,
    mut roots: Query<(&ComputedNode, &mut ScrollPosition), With<DialogRoot>>,
) {
    let was_open = conversation.open();
    conversation.just_closed = false;
    conversation.input_blocked = was_open;
    let back = native
        .read()
        .any(|key| key.input.state.is_pressed() && key.input.logical_key == Key::BrowserBack);
    if pause.is_some_and(|menu| menu.open || menu.input_blocked)
        || !windows.iter().any(|window| window.focused)
    {
        fingers.clear();
        return;
    }
    if !was_open && (keys.just_pressed(KeyCode::KeyG) || touch.talk) {
        if let Some(ship_id) = talk_target(&session) {
            conversation.ship_id = Some(ship_id);
            conversation.text = "Where are you headed?".into();
            conversation.answered = false;
            conversation.input_blocked = true;
            session.help = false;
            session.inspector = false;
            touch.reset();
            connection.send(ClientMessage::TalkToPilot { ship_id });
        } else {
            session.status = "Move closer to an airship pilot to ask where they're going.".into();
            session.status_until = time.elapsed_secs_f64() + 4.0;
        }
    }
    if !was_open {
        fingers.clear();
        return;
    }
    let mut chosen = None;
    if keys.just_pressed(KeyCode::Escape)
        || back
        || keys.just_pressed(KeyCode::KeyG)
        || keys.just_pressed(KeyCode::Enter)
    {
        chosen = Some(Action::Close);
    } else if keys.just_pressed(KeyCode::KeyN) || keys.just_pressed(KeyCode::ArrowRight) {
        chosen = Some(Action::NextPilot);
    }
    let mut scroll_delta = -wheel.delta.y * 28.0;
    for finger in fingers.read() {
        if let Ok(window) = windows.get(finger.window) {
            match finger.phase {
                TouchPhase::Started => {
                    let point = finger.position * window.scale_factor();
                    if let Some((_, action, _, _, _, _)) =
                        targets
                            .iter()
                            .find(|(entity, _, computed, transform, node, visibility)| {
                                node.display != Display::None
                                    && visibility.is_none_or(|v| v.get())
                                    && computed.contains_point(**transform, point)
                                    && bevy::ui::clip_check_recursive(
                                        point, *entity, &clipping, &parents,
                                    )
                            })
                    {
                        chosen = Some(*action);
                    } else {
                        conversation.scroll_finger = Some((finger.id, finger.position));
                    }
                }
                TouchPhase::Moved => {
                    if let Some((id, previous)) = conversation.scroll_finger
                        && id == finger.id
                    {
                        scroll_delta += previous.y - finger.position.y;
                        conversation.scroll_finger = Some((id, finger.position));
                    }
                }
                TouchPhase::Ended | TouchPhase::Canceled => {
                    if conversation
                        .scroll_finger
                        .is_some_and(|(id, _)| id == finger.id)
                    {
                        conversation.scroll_finger = None;
                    }
                }
            }
        }
    }
    for (computed, mut scroll) in &mut roots {
        let max = (computed.content_size.y - computed.size.y) * computed.inverse_scale_factor;
        scroll.0.y = (scroll.0.y + scroll_delta).clamp(0.0, max.max(0.0));
    }
    if !cfg!(target_os = "android") && chosen.is_none() {
        chosen = actions
            .iter()
            .find(|(_, i)| **i == Interaction::Pressed)
            .map(|(a, _)| *a);
    }
    if let Some(action) = chosen {
        match action {
            Action::NextPilot => {
                let choices = talk_candidates(&session);
                if !choices.is_empty() {
                    let current = choices
                        .iter()
                        .position(|id| Some(*id) == conversation.ship_id)
                        .unwrap_or(0);
                    let ship_id = choices[(current + 1) % choices.len()];
                    conversation.ship_id = Some(ship_id);
                    conversation.text = "Where are you headed?".into();
                    conversation.answered = false;
                    connection.send(ClientMessage::TalkToPilot { ship_id });
                }
            }
            Action::Close => conversation.close(),
        }
        touch.reset();
    }
}

pub(super) fn advance_clock(mut session: ResMut<Session>, time: Res<Time>) {
    session.airship_time += time.delta_secs_f64();
    if let Some(ride) = session.ride
        && let Some(ship) = session.airships.ship(ride.ship_id, session.airship_time)
    {
        let local = session
            .deck_position
            .unwrap_or(initial_deck_position(ride.seat));
        session.body.position = deck_position(&ship, local);
    }
}

fn village_name(world: &VoxelWorld, id: u32) -> String {
    world
        .0
        .settlements()
        .and_then(|plan| plan.villages.iter().find(|v| v.id == id))
        .map_or_else(|| format!("Village {id}"), |v| v.name.clone())
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn refresh(
    session: Res<Session>,
    world: Res<VoxelWorld>,
    conversation: Res<PilotConversation>,
    touch: Res<TouchControls>,
    pause: Res<PauseMenu>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut roots: Query<&mut Node, (With<DialogRoot>, Without<TravelHint>)>,
    mut buttons: Query<(&Action, &mut Node), (Without<DialogRoot>, Without<TravelHint>)>,
    mut texts: ParamSet<(
        Query<&mut Text, With<DialogText>>,
        Query<&mut Text, With<ScheduleText>>,
        Query<(&mut Node, &Children), (With<TravelHint>, Without<DialogRoot>)>,
        Query<&mut Text, Without<DialogText>>,
    )>,
) {
    let open = conversation.open() && !pause.open;
    for mut node in &mut roots {
        node.display = if open { Display::Flex } else { Display::None };
        node.justify_content = if windows.iter().any(|w| w.height() < 500.) {
            JustifyContent::FlexStart
        } else {
            JustifyContent::Center
        };
    }
    let ship = conversation
        .ship_id
        .and_then(|id| session.airships.ship(id, session.airship_time));
    for (action, mut node) in &mut buttons {
        let show = match action {
            Action::NextPilot => talk_candidates(&session).len() > 1,
            Action::Close => true,
        };
        node.display = if show { Display::Flex } else { Display::None };
    }
    for mut text in &mut texts.p0() {
        text.0.clone_from(&conversation.text);
    }
    let schedule = if !conversation.answered {
        "Asking the pilot…".into()
    } else {
        ship.as_ref().map_or_else(
            || "The airship has moved on. Another service will arrive soon.".into(),
            |ship| {
                let destination = village_name(&world, ship.next_village);
                if ship.docked_at.is_some() {
                    format!(
                        "Next stop: {destination} · departs in {:.0} seconds",
                        ship.departure_in.ceil()
                    )
                } else {
                    format!(
                        "Next stop: {destination} · arrives in {:.0} seconds",
                        ship.arrival_in.ceil()
                    )
                }
            },
        )
    };
    for mut text in &mut texts.p1() {
        text.0.clone_from(&schedule);
    }
    let hint = travel_hint(&session, &world, touch.enabled);
    let mut labels = Vec::new();
    for (mut node, children) in &mut texts.p2() {
        node.top = if touch.enabled { px(112) } else { Val::Auto };
        node.bottom = if touch.enabled {
            Val::Auto
        } else {
            px(if session.help { 280 } else { 88 })
        };
        node.max_width = if touch.enabled { percent(58) } else { px(620) };
        node.display = if hint.is_some()
            && !open
            && !pause.open
            && !(touch.enabled && (session.help || session.inspector))
        {
            Display::Flex
        } else {
            Display::None
        };
        labels.extend(children.iter());
    }
    for entity in labels {
        if let Ok(mut text) = texts.p3().get_mut(entity) {
            text.0 = hint.clone().unwrap_or_default();
        }
    }
}

fn travel_hint(session: &Session, world: &VoxelWorld, touch: bool) -> Option<String> {
    if session.observer.is_some() {
        return None;
    }
    if let Some(ride) = session.ride {
        return session
            .airships
            .ship(ride.ship_id, session.airship_time)
            .map(|ship| {
                let action = if touch {
                    "Move · Jump · Pilot to talk"
                } else {
                    "WASD to walk · Space to jump · G to talk"
                };
                format!(
                    "Aboard · {} · {} in {:.0}s · {action}",
                    village_name(world, ship.next_village),
                    if ship.docked_at.is_some() {
                        "departs"
                    } else {
                        "arrives"
                    },
                    if ship.docked_at.is_some() {
                        ship.departure_in.ceil()
                    } else {
                        ship.arrival_in.ceil()
                    }
                )
            });
    }
    if talk_target(session).is_some() {
        return Some(if touch {
            "Airship pilot nearby · tap Pilot".into()
        } else {
            "Airship pilot nearby · G to ask where they're going".into()
        });
    }
    let port = nearby_port(session)?;
    let wait = session
        .airships
        .routes()
        .iter()
        .filter_map(|route| {
            let destination = if route.from == port {
                route.to
            } else if route.to == port {
                route.from
            } else {
                return None;
            };
            session
                .airships
                .next_leg(port, destination, session.airship_time)
        })
        .min_by(|a, b| a.departure_in.total_cmp(&b.departure_in));
    Some(wait.map_or_else(
        || "Airship port · service currently unavailable".into(),
        |leg| {
            let landed = session
                .airships
                .ship(leg.ship_id, session.airship_time)
                .is_some_and(|ship| ship.docked_at == Some(port));
            let landing = session
                .airships
                .landing_path(leg.ship_id, port)
                .and_then(|path| path.last())
                .map(|position| {
                    let delta =
                        Vec3::from_array(*position) - Vec3::from_array(session.body.position);
                    let distance = delta.xz().length();
                    let direction = ((delta.x.atan2(-delta.z) / std::f32::consts::FRAC_PI_4).round()
                        as i32)
                        .rem_euclid(8) as usize;
                    format!(
                        " · landing {:.0}m {}",
                        distance.ceil(),
                        ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][direction]
                    )
                })
                .unwrap_or_default();
            format!(
                "To {} · {} in {:.0}s{landing}",
                village_name(world, leg.destination),
                if landed { "departs" } else { "next departure" },
                leg.departure_in.ceil()
            )
        },
    ))
}

/// Camera obstruction follows the deck and stepped envelope of the visible ship. The
/// regular terrain ray remains responsible for nearby ground and buildings.
pub(crate) fn camera_position(ship: &AirshipSnapshot, eye: Vec3, desired: Vec3) -> Vec3 {
    let rotation = Quat::from_rotation_y(-ship.yaw);
    let origin = rotation * (eye - Vec3::from_array(ship.position));
    let offset = rotation * (desired - eye);
    let deck = segment_box(
        origin,
        offset,
        Vec3::new(-4.0, -0.35, -8.5),
        Vec3::new(4.0, 0.0, 8.5),
    );
    let balloon = crate::airship_mesh::camera_boxes()
        .iter()
        .filter_map(|(min, max)| segment_box(origin, offset, *min, *max))
        .min_by(f32::total_cmp);
    let hit = deck.into_iter().chain(balloon).min_by(f32::total_cmp);
    hit.map_or(desired, |t| {
        eye + (desired - eye) * (t - 0.2 / (desired - eye).length().max(0.001)).max(0.0)
    })
}

fn segment_box(origin: Vec3, offset: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let mut enter = 0.0_f32;
    let mut exit = 1.0_f32;
    for axis in 0..3 {
        if offset[axis].abs() < 0.000001 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
        } else {
            let a = (min[axis] - origin[axis]) / offset[axis];
            let b = (max[axis] - origin[axis]) / offset[axis];
            enter = enter.max(a.min(b));
            exit = exit.min(a.max(b));
            if enter > exit {
                return None;
            }
        }
    }
    Some(enter)
}

pub(super) fn update_scene(
    mut commands: Commands,
    session: Res<Session>,
    mut scene: ResMut<Scene>,
    mut transforms: Query<&mut Transform, With<Ship>>,
) {
    let ships = session.airships.ships(session.airship_time);
    let live: Vec<_> = ships.iter().map(|ship| ship.id).collect();
    scene.ships.retain(|id, entity| {
        if live.contains(id) {
            true
        } else {
            commands.entity(*entity).despawn();
            false
        }
    });
    for ship in ships {
        let transform = Transform::from_translation(Vec3::from_array(ship.position))
            .with_rotation(Quat::from_rotation_y(ship.yaw));
        if let Some(entity) = scene.ships.get(&ship.id) {
            if let Ok(mut pose) = transforms.get_mut(*entity) {
                *pose = transform;
            }
        } else {
            let entity = spawn_ship(&mut commands, &scene, &ship, transform);
            scene.ships.insert(ship.id, entity);
        }
    }
}

fn spawn_ship(
    commands: &mut Commands,
    scene: &Scene,
    ship: &AirshipSnapshot,
    transform: Transform,
) -> Entity {
    let entity = commands
        .spawn((GameEntity, Ship, transform, Visibility::default()))
        .id();
    commands.entity(entity).with_children(|parent| {
        parent.spawn((
            Mesh3d(scene.balloon.clone()),
            MeshMaterial3d(scene.cream.clone()),
            Transform::default(),
        ));
        parent.spawn((
            Mesh3d(scene.cube.clone()),
            MeshMaterial3d(scene.wood.clone()),
            Transform::from_xyz(0.0, -0.175, 0.0).with_scale(Vec3::new(8.0, 0.35, 17.0)),
        ));
        // Thin suspension ropes remain outside the open walking surface.
        // There are no decorative solid rails or benches to walk through.
        for x in [-4.4, 4.4] {
            for z in [-6.5, 6.5] {
                parent.spawn((
                    Mesh3d(scene.cube.clone()),
                    MeshMaterial3d(scene.dark.clone()),
                    Transform::from_xyz(x, 4.5, z).with_scale(Vec3::new(0.065, 9.0, 0.065)),
                ));
            }
        }
    });
    let p = Vec3::from_array(pilot_position(ship)) - Vec3::from_array(ship.position);
    let p = Quat::from_rotation_y(-ship.yaw) * p;
    commands.entity(entity).with_children(|parent| {
        for (at, scale, material) in [
            (
                p + Vec3::new(0.0, 0.95, 0.0),
                Vec3::new(0.5, 0.65, 0.32),
                scene.dark.clone(),
            ),
            (
                p + Vec3::new(0.0, 1.50, 0.0),
                Vec3::splat(0.38),
                scene.cream.clone(),
            ),
            (
                p + Vec3::new(0.0, 1.76, 0.0),
                Vec3::new(0.52, 0.12, 0.46),
                scene.gold.clone(),
            ),
            (
                p + Vec3::new(-0.15, 0.35, 0.0),
                Vec3::new(0.18, 0.7, 0.24),
                scene.dark.clone(),
            ),
            (
                p + Vec3::new(0.15, 0.35, 0.0),
                Vec3::new(0.18, 0.7, 0.24),
                scene.dark.clone(),
            ),
        ] {
            parent.spawn((
                Mesh3d(scene.cube.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(at).with_scale(scale),
            ));
        }
    });
    entity
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{graphics::GraphicsQuality, join::session_from_welcome};
    use rubblekin_core::{
        airships::AirshipRide,
        protocol::{ServerMessage, SessionMode},
        world::WorldGeneration,
    };
    use std::time::Duration;

    fn fixture() -> (VoxelWorld, Session) {
        let mut welcome = crate::join::tests::welcome(SessionMode::Player);
        if let ServerMessage::Welcome { generation, .. } = &mut welcome {
            *generation = WorldGeneration::GeographyV3;
        }
        let (world, session) = session_from_welcome(
            welcome,
            "test".into(),
            GraphicsQuality::Low,
            0.0,
            SessionMode::Player,
        )
        .unwrap();
        (VoxelWorld(world), session)
    }

    #[test]
    fn pilot_replies_cannot_replace_a_newer_conversation() {
        let mut conversation = PilotConversation {
            ship_id: Some(2),
            ..default()
        };
        conversation.reply(1, "Old pilot".into());
        assert!(!conversation.answered);
        conversation.reply(2, "I am heading to the neighboring village".into());
        assert!(conversation.answered);
        conversation.close();
        conversation.reply(2, "Late reply".into());
        assert!(!conversation.open());
        assert!(conversation.input_blocked && conversation.just_closed);
    }

    #[test]
    fn passenger_animation_tracks_deck_walks_instead_of_ship_carry_or_jumps() {
        let mut motion = crate::DeckMotion::default();
        let ride = AirshipRide {
            ship_id: 1,
            seat: u8::MAX,
        };
        assert!(!motion.walking(ride, [0.0; 3], 1.0));
        assert!(!motion.walking(ride, [0.0, 0.6, 0.0], 1.1));
        assert!(motion.walking(ride, [0.2, 0.6, 0.0], 1.2));
        assert!(motion.walking(ride, [0.2, 0.7, 0.0], 1.25));
        assert!(!motion.walking(ride, [0.2, 0.7, 0.0], 1.4));
        assert!(!motion.walking(AirshipRide { ship_id: 2, ..ride }, [1.0; 3], 1.5));
    }

    #[test]
    fn passenger_camera_stops_before_deck_or_balloon_in_every_ship_orientation() {
        let (_, session) = fixture();
        let mut ship = session.airships.ships(0.0)[0].clone();
        for yaw in [0.0, 0.8, 2.7] {
            ship.yaw = yaw;
            let origin = Vec3::from_array(ship.position);
            let rotate = Quat::from_rotation_y(yaw);
            let eye = origin + Vec3::Y * 1.62;
            let down = origin + rotate * Vec3::new(0.8, -3.0, 3.0);
            let adjusted = camera_position(&ship, eye, down);
            assert!(adjusted.y > origin.y + 0.1);
            let up = origin + rotate * Vec3::new(0.8, 9.0, 3.0);
            let adjusted = camera_position(&ship, eye, up);
            assert!(adjusted.distance(eye) < up.distance(eye));
            let local = Quat::from_rotation_y(-yaw) * (adjusted - origin);
            assert!(
                !crate::airship_mesh::camera_boxes()
                    .iter()
                    .any(|(min, max)| local.cmpge(*min).all() && local.cmple(*max).all())
            );
            let unobstructed = origin + rotate * Vec3::new(0.8, 2.0, 6.5);
            assert!(camera_position(&ship, eye, unobstructed).distance(unobstructed) < 0.0001);
        }
    }

    #[test]
    fn port_hint_names_the_route_and_landing_without_requiring_pilot_dialogue() {
        let (world, mut session) = fixture();
        let ports = session.airships.ports().to_vec();
        let mut found_wait = false;
        let mut found_landed = false;
        for second in 0..600 {
            session.airship_time = second as f64;
            for port in &ports {
                session.body.position = port.position;
                let candidates = talk_candidates(&session);
                if candidates.is_empty() {
                    let hint = travel_hint(&session, &world, false).unwrap();
                    assert!(hint.starts_with("To "));
                    assert!(hint.contains("landing "));
                    assert!(!hint.contains("pilot"));
                    found_wait |= hint.contains("next departure");
                    found_landed |= hint.contains(" · departs");
                }
            }
            if found_wait && found_landed {
                break;
            }
        }
        assert!(found_wait && found_landed);
    }

    #[test]
    fn port_airship_symbol_is_centered_above_the_sign_at_a_compact_scale() {
        let transform = port_symbol_transform();
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for (a, b) in crate::airship_mesh::camera_boxes() {
            for corner in [*a, *b] {
                let point = transform.transform_point(corner);
                min = min.min(point);
                max = max.max(point);
            }
        }
        let size = max - min;
        assert!(size.x <= 3.5 && size.y <= 1.3 && size.z <= 2.0);
        assert!(((min + max) * 0.5).distance(Vec3::new(0.0, 5.8, -7.0)) < 0.15);
    }

    #[test]
    fn pilot_selection_uses_actual_pilot_distance_on_landed_and_flying_ships() {
        let (_, mut session) = fixture();
        for second in [0.0, 60.0] {
            session.airship_time = second;
            for ship in session.airships.ships(second) {
                session.body.position = pilot_position(&ship);
                assert_eq!(talk_target(&session), Some(ship.id));
                let position = Vec3::from_array(session.body.position);
                let candidates = talk_candidates(&session);
                assert!(candidates.contains(&ship.id));
                assert!(candidates.iter().all(|id| {
                    let nearby = session.airships.ship(*id, second).unwrap();
                    position.distance(Vec3::from_array(pilot_position(&nearby))) <= TALK_REACH
                }));
                session.body.position[1] += TALK_REACH + 1.0;
                assert!(!talk_candidates(&session).contains(&ship.id));
            }
        }
    }

    #[test]
    fn ship_scene_and_dialog_updates_keep_passengers_on_the_shared_deck_pose() {
        let (world, mut session) = fixture();
        let ship = session
            .airships
            .ships(0.0)
            .into_iter()
            .find(|ship| ship.docked_at.is_some())
            .unwrap();
        session.ride = Some(AirshipRide {
            ship_id: ship.id,
            seat: 5,
        });
        session.body.on_ground = true;
        let mut time = Time::<()>::default();
        time.advance_by(Duration::from_millis(16));
        let mut app = App::new();
        app.insert_resource(world)
            .insert_resource(session)
            .insert_resource(time)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Font>>()
            .init_resource::<PilotConversation>()
            .init_resource::<PauseMenu>()
            .init_resource::<TouchControls>()
            .add_systems(Startup, setup)
            .add_systems(Update, (advance_clock, update_scene, refresh).chain());
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app.update();
        let current = app.world().resource::<Session>();
        let pose = current
            .airships
            .ship(ship.id, current.airship_time)
            .unwrap();
        assert_eq!(
            current.body.position,
            deck_position(&pose, initial_deck_position(5))
        );
        assert!(current.body.on_ground);
        assert_eq!(
            app.world().resource::<Scene>().ships.len(),
            current.airships.ships(current.airship_time).len()
        );
        {
            let mut conversation = app.world_mut().resource_mut::<PilotConversation>();
            conversation.ship_id = Some(ship.id);
            conversation.reply(ship.id, "Welcome aboard".into());
        }
        app.update();
        let mut roots = app.world_mut().query_filtered::<&Node, With<DialogRoot>>();
        assert_eq!(roots.single(app.world()).unwrap().display, Display::Flex);
        let mut buttons = app.world_mut().query::<(&Action, &Node)>();
        assert!(
            buttons
                .iter(app.world())
                .any(|(action, node)| *action == Action::Close && node.display == Display::Flex)
        );
    }
}
