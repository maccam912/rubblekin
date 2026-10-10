//! Whip stations, four-seat carriages, personal canopies and the travel panel.
use crate::{
    GameEntity, Session, airships::PilotConversation, network::Connection, touch::TouchControls,
};
use bevy::{
    input::{
        mouse::AccumulatedMouseScroll,
        touch::{TouchInput, TouchPhase},
    },
    prelude::*,
    ui_widgets::Button,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use rubblekin_core::{gliders::*, protocol::ClientMessage};
use std::collections::HashMap;

#[derive(Resource)]
pub(crate) struct Scene {
    cube: Handle<Mesh>,
    wood: Handle<StandardMaterial>,
    metal: Handle<StandardMaterial>,
    canvas: Handle<StandardMaterial>,
    font: Handle<Font>,
    gold: Handle<StandardMaterial>,
    carriages: HashMap<u64, Entity>,
    canopies: HashMap<u64, Entity>,
    signature: String,
    scroll_finger: Option<(u64, f32, f32)>,
    scroll_dragged: bool,
    selected: usize,
    personal: HashMap<u32, Entity>,
}
#[derive(Component)]
pub(crate) struct Panel;
#[derive(Component)]
pub(crate) struct Rows;
#[derive(Component)]
pub(crate) struct Title;
#[derive(Component)]
pub(crate) struct TravelHelp;
#[derive(Component)]
pub(crate) struct Hint;
#[derive(Component)]
pub(crate) struct Segment {
    station: u32,
    index: usize,
}
#[derive(Component)]
struct SeatMark;
#[derive(Component, Clone, Copy)]
pub(crate) enum Action {
    Board(u32, GliderDestination),
    Launch,
    Leave,
    Close,
}

fn parts(parent: &mut ChildSpawnerCommands, scene: &Scene, items: &[(Vec3, Vec3, u8)]) {
    for (position, scale, material) in items {
        parent.spawn((
            Mesh3d(scene.cube.clone()),
            MeshMaterial3d(match material {
                0 => scene.wood.clone(),
                1 => scene.metal.clone(),
                2 => scene.canvas.clone(),
                _ => scene.gold.clone(),
            }),
            Transform::from_translation(*position).with_scale(*scale),
        ));
    }
}
fn craft(commands: &mut Commands, scene: &Scene, glider: bool) -> Entity {
    commands
        .spawn((GameEntity, Transform::default(), Visibility::default()))
        .with_children(|parent| {
            parts(
                parent,
                scene,
                &[
                    (Vec3::new(0.0, -0.22, 0.0), Vec3::new(2.8, 0.35, 4.0), 0),
                    (Vec3::new(0.0, 0.25, -1.9), Vec3::new(2.8, 0.75, 0.18), 0),
                    (Vec3::new(-1.5, 0.15, 0.0), Vec3::new(0.18, 0.65, 4.0), 0),
                    (Vec3::new(1.5, 0.15, 0.0), Vec3::new(0.18, 0.65, 4.0), 0),
                    (Vec3::new(0.0, 0.25, 1.9), Vec3::new(2.8, 0.75, 0.18), 0),
                    (Vec3::new(0.0, 2.5, 0.0), Vec3::new(14.0, 0.13, 2.7), 2),
                    (Vec3::new(0.0, 2.38, 0.0), Vec3::new(14.3, 0.12, 0.18), 0),
                    (Vec3::new(-1.3, 1.2, 0.0), Vec3::new(0.1, 2.5, 0.1), 1),
                    (Vec3::new(1.3, 1.2, 0.0), Vec3::new(0.1, 2.5, 0.1), 1),
                    (Vec3::new(0.0, 1.1, -3.5), Vec3::new(3.8, 0.12, 1.0), 2),
                    (Vec3::new(0.0, 0.0, -2.5), Vec3::new(0.2, 0.2, 3.0), 0),
                ],
            );
            if glider {
                for seat in 0..4 {
                    let local = seat_position(
                        GliderPose {
                            position: [0.0; 3],
                            yaw: 0.0,
                            landed: false,
                            launching: false,
                        },
                        seat,
                    );
                    parent.spawn((
                        SeatMark,
                        Mesh3d(scene.cube.clone()),
                        MeshMaterial3d(scene.gold.clone()),
                        Transform::from_xyz(local[0], 0.02, local[2])
                            .with_scale(Vec3::new(0.62, 0.08, 0.62)),
                    ));
                }
            }
        })
        .id()
}

pub(crate) fn setup(
    mut commands: Commands,
    session: Res<Session>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut fonts: ResMut<Assets<Font>>,
) {
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    let scene = Scene {
        font: font.clone(),
        cube: meshes.add(Cuboid::default()),
        wood: materials.add(Color::srgb(0.34, 0.21, 0.10)),
        metal: materials.add(Color::srgb(0.13, 0.20, 0.19)),
        canvas: materials.add(StandardMaterial {
            base_color: Color::srgb(0.83, 0.86, 0.63),
            double_sided: true,
            cull_mode: None,
            ..default()
        }),
        gold: materials.add(Color::srgb(0.88, 0.63, 0.22)),
        carriages: HashMap::new(),
        canopies: HashMap::new(),
        signature: String::new(),
        scroll_finger: None,
        scroll_dragged: false,
        selected: 0,
        personal: HashMap::new(),
    };
    for station in &session.whip_stations {
        commands
            .spawn((
                GameEntity,
                Transform::from_translation(Vec3::from_array(station.position)),
                Visibility::default(),
            ))
            .with_children(|parent| {
                parts(
                    parent,
                    &scene,
                    &[
                        (Vec3::new(6.0, 0.2, 0.0), Vec3::new(4.0, 0.4, 5.0), 0),
                        (Vec3::new(6.0, 6.0, 0.0), Vec3::new(1.0, 12.0, 1.0), 0),
                        (Vec3::new(6.0, 2.0, 0.0), Vec3::new(3.0, 3.0, 3.0), 1),
                        (Vec3::new(6.0, 2.0, 0.0), Vec3::new(3.3, 0.25, 3.3), 3),
                        (Vec3::new(0.0, 0.0, 0.0), Vec3::new(3.5, 0.08, 4.7), 3),
                    ],
                );
            });
        for index in 0..18 {
            commands.spawn((
                GameEntity,
                Segment {
                    station: station.village_id,
                    index,
                },
                Mesh3d(scene.cube.clone()),
                MeshMaterial3d(if index % 3 == 0 {
                    scene.gold.clone()
                } else {
                    scene.metal.clone()
                }),
                Transform::default(),
            ));
        }
        let e = craft(&mut commands, &scene, true);
        commands.entity(e).insert(Idle {
            station: station.village_id,
        });
    }
    commands.spawn((
        GameEntity,
        Hint,
        GlobalZIndex(24),
        Node {
            position_type: PositionType::Absolute,
            left: px(16),
            bottom: px(88),
            max_width: percent(75),
            padding: UiRect::all(px(10)),
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgba(0.055, 0.10, 0.10, 0.9)),
        children![(
            Text::new(""),
            TextFont::from_font_size(17.0).with_font(font.clone()),
            TextColor(Color::srgb(0.95, 0.88, 0.66))
        )],
    ));
    commands.spawn((
        GameEntity,
        Panel,
        GlobalZIndex(90),
        Node {
            display: Display::None,
            width: percent(100),
            height: percent(100),
            padding: UiRect::all(px(10)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.02, 0.05, 0.05, 0.55)),
    )).with_children(|root| {
        root.spawn((
            Node {
                width: px(560),
                max_width: percent(100),
                height: percent(100),
                max_height: px(620),
                padding: UiRect::all(px(16)),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.15, 0.14)),
        )).with_children(|panel| {
            crate::tutorials::panel(panel, &font, crate::tutorials::Context::Travel);
            panel.spawn((
                Title,
                Text::new("Whip station"),
                TextFont::from_font_size(22.0).with_font(font.clone()),
                TextColor(Color::srgb(0.95, 0.83, 0.56)),
            ));
            panel.spawn((
                TravelHelp,
                Text::new("Choose a town or an explorer within 16 km.\nFour seats · solo trips welcome · Jump to glide away\nExplorer targets use their location at launch."),
                TextFont::from_font_size(16.0).with_font(font.clone()),
                TextColor(Color::srgb(0.88, 0.92, 0.83)),
            ));
            panel.spawn((
                Rows,
                Node {
                    overflow: Overflow::scroll_y(),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    min_height: px(48),
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    flex_basis: px(0),
                    ..default()
                },
                ScrollPosition::default(),
            ));
            panel.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(10),
                ..default()
            }).with_children(|row| {
                button(row, &font, "Launch [L]", Action::Launch, None);
                button(row, &font, "Leave [Jump]", Action::Leave, None);
                button(row, &font, "Close [G]", Action::Close, None);
            });
        });
    });
    commands.insert_resource(scene);
}
#[derive(Component)]
pub(crate) struct Idle {
    station: u32,
}
fn button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    action: Action,
    emblem: Option<Handle<Image>>,
) {
    parent
        .spawn((
            action,
            Button,
            Node {
                min_height: px(44),
                padding: UiRect::axes(px(12), px(8)),
                align_items: AlignItems::Center,
                column_gap: px(8),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(Color::srgb(0.17, 0.29, 0.24)),
        ))
        .with_children(|row| {
            if let Some(image) = emblem {
                row.spawn((
                    ImageNode::new(image),
                    Node {
                        width: px(36),
                        height: px(36),
                        flex_shrink: 0.,
                        ..default()
                    },
                ));
            }
            row.spawn((
                Text::new(label),
                TextFont::from_font_size(17.0).with_font(font.clone()),
                TextColor(Color::srgb(0.97, 0.93, 0.76)),
            ));
        });
}
fn targets(session: &Session, station: &WhipStation) -> Vec<(String, GliderDestination)> {
    if let Some(waiting) = session
        .gliders
        .iter()
        .find(|f| f.station_id == station.village_id && f.started_at.is_none())
    {
        let aboard = session
            .players
            .iter()
            .filter(|p| p.glider_ride.is_some_and(|r| r.carriage_id == waiting.id))
            .count();
        return vec![(
            format!("Join {} · {aboard} / 4 aboard", waiting.destination_name),
            waiting.destination,
        )];
    }
    let mut list: Vec<_> = session
        .players
        .iter()
        .filter(|p| p.id != session.id && reachable(station.position, p.body.position))
        .map(|p| {
            (
                format!(
                    "Meet {} · {:.1} km",
                    p.name,
                    horizontal_distance(station.position, p.body.position) / 1000.0
                ),
                GliderDestination::Player(p.id),
            )
        })
        .collect();
    list.extend(
        session
            .whip_stations
            .iter()
            .filter(|s| {
                s.village_id != station.village_id
                    && reachable(station.position, s.landing_position)
            })
            .map(|s| {
                (
                    format!(
                        "{} · {:.1} km",
                        s.name,
                        horizontal_distance(station.position, s.landing_position) / 1000.0
                    ),
                    GliderDestination::Village(s.village_id),
                )
            }),
    );
    list
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn read(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut dialog: ResMut<PilotConversation>,
    mut scene: ResMut<Scene>,
    mut session: ResMut<Session>,
    mut connection: ResMut<Connection>,
    mut touch: ResMut<TouchControls>,
    mut cursor: Single<&mut CursorOptions>,
    buttons: Query<&Action, Changed<crate::ui::Activated>>,
    modals: (
        Res<crate::pause::PauseMenu>,
        Res<crate::world_map::WorldMap>,
        Res<crate::admin_console::AdminConsole>,
        Res<crate::market::MarketPanel>,
    ),
    wheel: Res<AccumulatedMouseScroll>,
    mut fingers: MessageReader<TouchInput>,
    mut scroll: Query<(&ComputedNode, &mut ScrollPosition), With<Rows>>,
) {
    let was_open = dialog.open();
    dialog.just_closed = false;
    dialog.input_blocked = was_open;
    let requested = keys.just_pressed(KeyCode::KeyG) || touch.talk;
    touch.talk = false;
    let mut action = if scene.scroll_dragged {
        None
    } else {
        buttons.iter().next().copied()
    };
    if !window.focused || touch.suspended {
        dialog.close();
        scene.scroll_finger = None;
        return;
    }
    let (pause, map, console, market) = modals;
    if !was_open
        && requested
        && !pause.open
        && !map.open
        && !console.open
        && !market.open
        && !session.inventory.open
        && session.observer.is_none()
    {
        dialog.station_id = session
            .glider_ride
            .and_then(|r| {
                session
                    .gliders
                    .iter()
                    .find(|f| f.id == r.carriage_id)
                    .map(|f| f.station_id)
            })
            .or_else(|| {
                session
                    .whip_stations
                    .iter()
                    .filter(|s| {
                        rubblekin_core::gliders::distance(s.position, session.body.position)
                            <= STATION_REACH
                    })
                    .min_by(|a, b| {
                        horizontal_distance(a.position, session.body.position)
                            .total_cmp(&horizontal_distance(b.position, session.body.position))
                    })
                    .map(|s| s.village_id)
            });
        if dialog.station_id.is_some() {
            session.captured = false;
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
            scene.signature.clear();
            scene.selected = 0;
            dialog.input_blocked = true;
        } else {
            session.status =
                "Find a village whip station to board. Open the map to see towns and explorers."
                    .into();
            session.status_until = time.elapsed_secs_f64() + 5.0;
        }
    } else if was_open {
        if requested || keys.just_pressed(KeyCode::Escape) {
            action = Some(Action::Close);
        }
        if keys.just_pressed(KeyCode::KeyL) {
            action = Some(Action::Launch);
        }
        if keys.just_pressed(KeyCode::Space) {
            action = Some(Action::Leave);
        }
        if let Some(station) = dialog
            .station_id
            .and_then(|id| session.whip_stations.iter().find(|s| s.village_id == id))
        {
            let list = targets(&session, station);
            if keys.just_pressed(KeyCode::ArrowDown) {
                scene.selected = (scene.selected + 1).min(list.len().saturating_sub(1));
                scene.signature.clear();
            }
            if keys.just_pressed(KeyCode::ArrowUp) {
                scene.selected = scene.selected.saturating_sub(1);
                scene.signature.clear();
            }
            if keys.just_pressed(KeyCode::Enter)
                && let Some((_, destination)) = list.get(scene.selected)
            {
                action = Some(Action::Board(station.village_id, *destination));
            }
        }
        let mut delta = -wheel.delta.y * 35.0;
        for finger in fingers.read() {
            match finger.phase {
                TouchPhase::Started => {
                    scene.scroll_dragged = false;
                    scene.scroll_finger = Some((finger.id, finger.position.y, finger.position.y));
                }
                TouchPhase::Moved => {
                    if let Some((id, y, origin)) = scene.scroll_finger
                        && id == finger.id
                    {
                        delta += y - finger.position.y;
                        scene.scroll_dragged |= (origin - finger.position.y).abs() > 10.0;
                        scene.scroll_finger = Some((id, finger.position.y, origin));
                    }
                }
                TouchPhase::Ended | TouchPhase::Canceled => scene.scroll_finger = None,
            }
        }
        if scene.scroll_dragged && buttons.iter().next().is_some() {
            action = None;
        }
        if let Ok((node, mut position)) = scroll.single_mut() {
            let max = ((node.content_size.y - node.size.y) * node.inverse_scale_factor).max(0.0);
            position.y = (position.y + delta).clamp(0.0, max);
        }
    }
    if let Some(action) = action {
        match action {
            Action::Board(station_id, destination) => connection.send(ClientMessage::Glider {
                action: GliderAction::Board {
                    station_id,
                    destination,
                },
            }),
            Action::Launch => {
                connection.send(ClientMessage::Glider {
                    action: GliderAction::Launch,
                });
                dialog.close();
            }
            Action::Leave => {
                connection.send(ClientMessage::Glider {
                    action: GliderAction::Leave,
                });
                dialog.close();
            }
            Action::Close => dialog.close(),
        }
    }
    if dialog.just_closed {
        session.captured = true;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
        scene.scroll_finger = None;
        touch.reset();
    }
}

pub(crate) fn carry(mut session: ResMut<Session>) {
    if let Some(ride) = session.glider_ride
        && let Some(f) = session.gliders.iter().find(|f| f.id == ride.carriage_id)
    {
        session.body.position = seat_position(f.pose(session.airship_clock.time), ride.seat);
    }
}

pub(crate) fn update_scene(
    mut commands: Commands,
    time: Res<Time>,
    session: Res<Session>,
    mut scene: ResMut<Scene>,
    mut transforms: Query<&mut Transform, Without<Idle>>,
    mut idle: Query<(&Idle, &mut Transform, &mut Visibility)>,
) {
    let now = session.airship_clock.time;
    let stations: Vec<_> = session
        .whip_stations
        .iter()
        .filter(|s| s.temporary)
        .collect();
    let stale: Vec<_> = scene
        .personal
        .keys()
        .copied()
        .filter(|id| !stations.iter().any(|s| s.village_id == *id))
        .collect();
    for id in stale {
        commands
            .entity(scene.personal.remove(&id).unwrap())
            .despawn();
    }
    for station in stations {
        if scene.personal.contains_key(&station.village_id) {
            continue;
        }
        let entity = commands
            .spawn((
                GameEntity,
                Transform::from_translation(Vec3::from_array(station.position)),
                Visibility::default(),
            ))
            .with_children(|parent| {
                parts(
                    parent,
                    &scene,
                    &[
                        (Vec3::new(0., 0.05, 0.), Vec3::new(3.5, 0.1, 4.), 3),
                        (Vec3::new(2., 1.25, 0.), Vec3::new(0.16, 2.5, 0.16), 0),
                        (Vec3::new(1., 2.5, 0.), Vec3::new(2., 0.12, 0.12), 3),
                        (Vec3::new(2., 0.5, 0.), Vec3::new(0.7, 0.7, 0.7), 1),
                    ],
                )
            })
            .id();
        scene.personal.insert(station.village_id, entity);
    }
    for (mark, mut transform, mut visibility) in &mut idle {
        if let Some(station) = session
            .whip_stations
            .iter()
            .find(|s| s.village_id == mark.station)
        {
            transform.translation = Vec3::from_array(station.position) + Vec3::Y * 0.65;
        }
        *visibility = if session
            .gliders
            .iter()
            .any(|f| f.station_id == mark.station && f.started_at.is_none())
        {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }
    let stale: Vec<_> = scene
        .carriages
        .keys()
        .filter(|id| !session.gliders.iter().any(|f| f.id == **id))
        .copied()
        .collect();
    for id in stale {
        commands
            .entity(scene.carriages.remove(&id).unwrap())
            .despawn();
    }
    for f in &session.gliders {
        let entity = if let Some(e) = scene.carriages.get(&f.id) {
            *e
        } else {
            let e = craft(&mut commands, &scene, true);
            let pose = f.pose(now);
            commands.entity(e).insert(
                Transform::from_translation(Vec3::from_array(pose.position))
                    .with_rotation(Quat::from_rotation_y(pose.yaw)),
            );
            scene.carriages.insert(f.id, e);
            e
        };
        if let Ok(mut t) = transforms.get_mut(entity) {
            let pose = f.pose(now);
            t.translation = Vec3::from_array(pose.position);
            t.rotation = Quat::from_rotation_y(pose.yaw);
        }
    }
    let gliding: Vec<_> = session
        .players
        .iter()
        .filter_map(|p| {
            let active = if p.id == session.id {
                session.gliding
            } else {
                p.gliding
            };
            active.then_some((
                p.id,
                if p.id == session.id {
                    session.body.position
                } else {
                    p.body.position
                },
                if p.id == session.id {
                    session.body.velocity
                } else {
                    p.body.velocity
                },
            ))
        })
        .collect();
    let stale: Vec<_> = scene
        .canopies
        .keys()
        .filter(|id| !gliding.iter().any(|p| p.0 == **id))
        .copied()
        .collect();
    for id in stale {
        commands
            .entity(scene.canopies.remove(&id).unwrap())
            .despawn();
    }
    for (id, position, velocity) in gliding {
        let e = if let Some(e) = scene.canopies.get(&id) {
            *e
        } else {
            let e = commands
                .spawn((
                    GameEntity,
                    Transform::from_translation(Vec3::from_array(position))
                        .with_rotation(canopy_rotation(velocity, 0.0)),
                    Visibility::default(),
                ))
                .with_children(|parent| {
                    parts(
                        parent,
                        &scene,
                        &[
                            (Vec3::new(0.0, 3.4, 0.0), Vec3::new(5.8, 0.18, 2.0), 2),
                            (Vec3::new(-0.65, 2.3, 0.0), Vec3::new(0.04, 2.3, 0.04), 1),
                            (Vec3::new(0.65, 2.3, 0.0), Vec3::new(0.04, 2.3, 0.04), 1),
                        ],
                    );
                })
                .id();
            scene.canopies.insert(id, e);
            e
        };
        if let Ok(mut t) = transforms.get_mut(e) {
            t.translation = Vec3::from_array(position);
            let (old_yaw, _, old_bank) = t.rotation.to_euler(EulerRot::YXZ);
            let yaw = velocity[0].atan2(-velocity[2]);
            let delta = yaw + old_yaw;
            let dt = time.delta_secs().clamp(0.001, 0.25);
            let rate = delta.sin().atan2(delta.cos()) / dt;
            let bank = old_bank
                + ((-rate * 0.25).clamp(-0.65, 0.65) - old_bank) * (1.0 - (-6.0 * dt).exp());
            t.rotation = canopy_rotation(velocity, bank);
        }
    }
}

fn canopy_rotation(velocity: [f32; 3], bank: f32) -> Quat {
    let yaw = velocity[0].atan2(-velocity[2]);
    let pitch = (-velocity[1]).atan2(velocity[0].hypot(velocity[2]));
    Quat::from_rotation_y(-yaw) * Quat::from_rotation_x(-pitch) * Quat::from_rotation_z(bank)
}

pub(crate) fn animate_whips(
    session: Res<Session>,
    mut segments: Query<(&Segment, &mut Transform)>,
) {
    for (segment, mut t) in &mut segments {
        let Some(station) = session
            .whip_stations
            .iter()
            .find(|s| s.village_id == segment.station)
        else {
            continue;
        };
        let now = session.airship_clock.time;
        let flight = session.gliders.iter().find(|f| {
            f.station_id == station.village_id
                && f.started_at
                    .is_some_and(|start| now - start < LAUNCH_SECONDS)
        });
        let phase = flight
            .and_then(|f| f.started_at)
            .map_or(0.0, |start| ((now - start) / LAUNCH_SECONDS) as f32);
        let tip = flight.map_or(Vec3::from_array(station.position) + Vec3::Y * 0.65, |f| {
            Vec3::from_array(f.pose(now.min(f.started_at.unwrap() + 5.0)).position)
        });
        let base = Vec3::from_array(station.position) + Vec3::new(6.0, 10.0, 0.0);
        let point = |u: f32| {
            base.lerp(tip, u)
                + Vec3::new(
                    (u * std::f32::consts::PI).sin() * 9.0,
                    (u * std::f32::consts::PI).sin()
                        * (18.0 + (u * 9.0 - phase * 18.0).sin() * phase.sin() * 14.0),
                    0.0,
                )
        };
        let a = point(segment.index as f32 / 18.0);
        let b = point((segment.index + 1) as f32 / 18.0);
        t.translation = (a + b) * 0.5;
        t.rotation = Quat::from_rotation_arc(Vec3::Y, (b - a).normalize_or_zero());
        t.scale = Vec3::new(0.25, (b - a).length() + 0.1, 0.25);
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn refresh(
    mut commands: Commands,
    session: Res<Session>,
    dialog: Res<PilotConversation>,
    mut scene: ResMut<Scene>,
    mut panel: Query<&mut Node, With<Panel>>,
    rows: Query<Entity, With<Rows>>,
    help: Query<Entity, With<TravelHelp>>,
    mut title: Query<&mut Text, With<Title>>,
    mut hints: Query<(&mut Node, &Children), (With<Hint>, Without<Panel>)>,
    mut texts: Query<&mut Text, Without<Title>>,
    pictures: Option<Res<crate::parcels::Pictures>>,
) {
    if let Ok(mut node) = panel.single_mut() {
        node.display = if dialog.station_id.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    if let Some(station) = dialog
        .station_id
        .and_then(|id| session.whip_stations.iter().find(|s| s.village_id == id))
    {
        if let Ok(entity) = help.single()
            && let Ok(mut text) = texts.get_mut(entity)
        {
            let label = if station.temporary {
                "Your one trip to the nearest town with a safe route.\nLaunch uses up this station · Leave puts it away.\nJump during the flight to glide away."
            } else {
                "Choose a town or an explorer within 16 km.\nFour seats · solo trips welcome · Jump to glide away\nExplorer targets use their location at launch."
            };
            if text.0 != label {
                text.0 = label.into();
            }
        }
        let flight = session
            .glider_ride
            .and_then(|r| session.gliders.iter().find(|f| f.id == r.carriage_id));
        let label = flight.map_or_else(
            || format!("{} · whip station", station.name),
            |f| {
                if station.temporary {
                    return format!("Personal return · {}", f.destination_name);
                }
                format!(
                    "{} · {} / 4 aboard",
                    f.destination_name,
                    session
                        .players
                        .iter()
                        .filter(|p| p.glider_ride.is_some_and(|r| r.carriage_id == f.id))
                        .count()
                )
            },
        );
        if let Ok(mut text) = title.single_mut()
            && text.0 != label
        {
            *text = Text::new(label);
        }
        let list = targets(&session, station);
        let signature = format!("{:?}:{:?}:{}", list, flight.map(|f| f.id), scene.selected);
        if signature != scene.signature
            && let Ok(entity) = rows.single()
        {
            commands.entity(entity).despawn_children();
            // In-memory font handle from the title, rather than a new asset per refresh.
            let font = scene.font.clone();
            commands.entity(entity).with_children(|parent| {
                if let Some(flight) = flight {
                    button(
                        parent,
                        &font,
                        if flight.started_at.is_none() {
                            if station.temporary {
                                "Launch your personal return trip"
                            } else {
                                "Friends can join at the station. Launch when ready."
                            }
                        } else {
                            "Leave the carriage and glide to somewhere you see."
                        },
                        if flight.started_at.is_none() {
                            Action::Launch
                        } else {
                            Action::Leave
                        },
                        None,
                    );
                } else {
                    for (index, (label, destination)) in list.iter().enumerate() {
                        button(
                            parent,
                            &font,
                            &format!(
                                "{} {}",
                                if index == scene.selected { "›" } else { "" },
                                label
                            ),
                            Action::Board(station.village_id, *destination),
                            match destination {
                                GliderDestination::Village(id) => {
                                    pictures.as_ref().and_then(|p| p.emblem(*id))
                                }
                                GliderDestination::Player(_) => None,
                            },
                        );
                    }
                }
            });
            scene.signature = signature;
        }
    }
    let hint = if let Some(ride) = session.glider_ride {
        session
            .gliders
            .iter()
            .find(|f| f.id == ride.carriage_id)
            .map_or(String::new(), |f| {
                if let Some(started_at) = f.started_at {
                    format!(
                        "To {} · {:.0}s · Jump to explore",
                        f.destination_name,
                        (started_at + f.duration - session.airship_clock.time).max(0.0)
                    )
                } else {
                    format!(
                        "{} · aboard · G / Travel to launch · Jump to leave",
                        f.destination_name
                    )
                }
            })
    } else if session.gliding {
        format!(
            "{} · {:.0} m/s · look to steer and pitch · Jump brake · Sprint dive",
            if session.body.glide_stalled {
                "Stall · nose lowering"
            } else {
                "Gliding"
            },
            rubblekin_core::physics::flight_speed(session.body.velocity),
        )
    } else if session.whip_stations.iter().any(|s| {
        rubblekin_core::gliders::distance(s.position, session.body.position) <= STATION_REACH
    }) {
        "Whip station nearby · G / Travel · four seats, solo welcome".into()
    } else {
        String::new()
    };
    for (mut node, children) in &mut hints {
        node.display = if hint.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        for child in children {
            if let Ok(mut text) = texts.get_mut(*child)
                && text.0 != hint
            {
                *text = Text::new(hint.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canopy_nose_matches_flight_direction_in_dive_climb_and_bank() {
        for velocity in [
            [20.0, -30.0, -10.0],
            [-10.0, 12.0, 20.0],
            [0.0, -2.0, -18.0],
        ] {
            let direction = Vec3::from_array(velocity).normalize();
            for bank in [-0.6, 0.0, 0.6] {
                let nose = canopy_rotation(velocity, bank) * Vec3::NEG_Z;
                assert!(nose.dot(direction) > 0.9999);
            }
        }
    }
    #[test]
    fn waiting_carriage_shows_its_existing_destination_before_joining() {
        let (_, mut session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(rubblekin_core::protocol::SessionMode::Player),
            "test".into(),
            crate::GraphicsQuality::Low,
            0.0,
            rubblekin_core::protocol::SessionMode::Player,
        )
        .unwrap();
        let station = WhipStation {
            temporary: false,
            village_id: 1,
            name: "Test".into(),
            position: session.body.position,
            landing_position: session.body.position,
        };
        session.gliders.push(GliderFlight {
            emergency: false,
            id: 2,
            station_id: 1,
            destination: GliderDestination::Village(3),
            destination_name: "Pinevale".into(),
            from: station.position,
            to: station.position,
            apex: 500.0,
            duration: 30.0,
            created_at: 0.0,
            started_at: None,
        });
        session.players[0].glider_ride = Some(GliderRide {
            carriage_id: 2,
            seat: 0,
        });
        assert_eq!(
            targets(&session, &station),
            vec![(
                "Join Pinevale · 1 / 4 aboard".into(),
                GliderDestination::Village(3)
            )]
        );
    }
    #[test]
    fn destinations_include_only_other_active_players_within_reach() {
        let (world, mut session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(rubblekin_core::protocol::SessionMode::Player),
            "test".into(),
            crate::GraphicsQuality::Low,
            0.0,
            rubblekin_core::protocol::SessionMode::Player,
        )
        .unwrap();
        let station = WhipStation {
            temporary: false,
            village_id: 1,
            name: "Test".into(),
            position: world.spawn_position(),
            landing_position: world.spawn_position(),
        };
        let mut friend = session.players[0].clone();
        friend.id = 2;
        friend.name = "Friend".into();
        friend.body.position = station.position;
        session.players.push(friend.clone());
        friend.id = 3;
        friend.body.position[0] += LAUNCH_RANGE + 1.0;
        session.players.push(friend);
        let list = targets(&session, &station);
        assert!(list.iter().any(|(_, d)| *d == GliderDestination::Player(2)));
        assert!(
            !list
                .iter()
                .any(|(_, d)| matches!(d, GliderDestination::Player(17 | 3)))
        );
    }
}
