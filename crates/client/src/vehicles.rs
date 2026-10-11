//! Bounded vehicle art, model-derived menu pictures and shared weather cues.
use crate::{GameEntity, Session, VoxelWorld, terrain::Geometry};
use bevy::prelude::*;
use rubblekin_core::{environment, vehicles::VehicleKind};
use std::collections::HashMap;
type Part = (Vec3, Vec3, [f32; 4]);
#[derive(Resource)]
pub(crate) struct Pictures(pub [Handle<Image>; 3]);
impl FromWorld for Pictures {
    fn from_world(world: &mut World) -> Self {
        let pictures = VehicleKind::ALL.map(|kind| {
            let mut model = parts(kind);
            if kind == VehicleKind::Sailboat {
                model.extend(
                    sail_parts()
                        .into_iter()
                        .map(|(p, s, c)| (p + Vec3::new(0., 0., -0.5), s, c)),
                );
            }
            let projected: Vec<_> = model
                .into_iter()
                .map(|(p, s, c)| {
                    (
                        Vec3::new(p.x * 0.5 + p.z * 0.85, p.y, 0.),
                        Vec3::new(s.x * 0.5 + s.z * 0.85, s.y, 0.05),
                        c,
                    )
                })
                .collect();
            let min = projected
                .iter()
                .fold(Vec3::splat(f32::INFINITY), |v, (p, s, _)| {
                    v.min(*p - *s / 2.)
                });
            let max = projected
                .iter()
                .fold(Vec3::splat(f32::NEG_INFINITY), |v, (p, s, _)| {
                    v.max(*p + *s / 2.)
                });
            let scale = (1.28 / (max.x - min.x)).min(0.98 / (max.y - min.y));
            let model: Vec<_> = projected
                .into_iter()
                .map(|(p, s, c)| {
                    (
                        Vec3::new(
                            (p.x - (min.x + max.x) / 2.) * scale,
                            (p.y - min.y) * scale + 0.04,
                            0.,
                        ),
                        s * scale,
                        c,
                    )
                })
                .collect();
            crate::activities::picture(&model)
        });
        let mut images = world.resource_mut::<Assets<Image>>();
        Self(pictures.map(|image| images.add(image)))
    }
}
fn index(kind: VehicleKind) -> usize {
    match kind {
        VehicleKind::Bike => 0,
        VehicleKind::Kayak => 1,
        VehicleKind::Sailboat => 2,
    }
}
fn parts(kind: VehicleKind) -> Vec<Part> {
    let mut model = frame_parts(kind);
    match kind {
        VehicleKind::Bike => {
            for centre in wheel_centres() {
                model.extend(
                    wheel_parts()
                        .into_iter()
                        .map(|(p, s, c)| (p + centre, s, c)),
                );
            }
            for side in [-1., 1.] {
                let pedal = bike_pedal(side, 0.);
                model.push((pedal, Vec3::new(0.18, 0.06, 0.24), [0.86, 0.64, 0.22, 1.]));
                model.push((
                    (pedal + Vec3::new(pedal.x, 0.34, 0.)) * 0.5,
                    Vec3::new(0.045, 0.045, 0.12),
                    [0.86, 0.64, 0.22, 1.],
                ));
            }
        }
        VehicleKind::Kayak => model.extend(
            paddle_parts()
                .into_iter()
                .map(|(p, s, c)| (p + resting_paddle_position(), s, c)),
        ),
        VehicleKind::Sailboat => {}
    }
    model
}
fn wheel_centres() -> [Vec3; 2] {
    [Vec3::new(0., 0.34, -0.65), Vec3::new(0., 0.34, 0.65)]
}
fn wheel_parts() -> Vec<Part> {
    let mut model: Vec<_> = (0..20)
        .map(|i| {
            let a = i as f32 * std::f32::consts::TAU / 20.;
            (
                Vec3::new(0., a.sin() * 0.30, a.cos() * 0.30),
                Vec3::new(0.13, 0.10, 0.10),
                [0.10, 0.15, 0.14, 1.],
            )
        })
        .collect();
    for size in [Vec3::new(0.15, 0.07, 0.50), Vec3::new(0.15, 0.50, 0.07)] {
        model.push((Vec3::ZERO, size, [0.86, 0.64, 0.22, 1.]));
    }
    model
}
fn paddle_parts() -> Vec<Part> {
    vec![
        (
            Vec3::ZERO,
            Vec3::new(1.8, 0.045, 0.045),
            [0.42, 0.27, 0.12, 1.],
        ),
        (
            Vec3::new(-0.94, 0., 0.),
            Vec3::new(0.35, 0.06, 0.19),
            [0.86, 0.64, 0.22, 1.],
        ),
        (
            Vec3::new(0.94, 0., 0.),
            Vec3::new(0.35, 0.06, 0.19),
            [0.86, 0.64, 0.22, 1.],
        ),
    ]
}
fn resting_paddle_position() -> Vec3 {
    seat_offset(VehicleKind::Kayak) + Vec3::Y + Quat::from_rotation_x(0.5) * Vec3::NEG_Y * 0.26
}
pub(crate) fn bike_grip(side: f32) -> Vec3 {
    Vec3::new(side * 0.26, 0.94, -0.4)
}
pub(crate) fn bike_pedal(side: f32, phase: f32) -> Vec3 {
    let phase = phase + if side > 0. { std::f32::consts::PI } else { 0. };
    Vec3::new(side * 0.225, 0.34 + phase.sin() * 0.12, phase.cos() * 0.12)
}
fn pedal_transform(side: f32, phase: f32, crank: bool) -> Transform {
    let pedal = bike_pedal(side, phase);
    if crank {
        let pivot = Vec3::new(pedal.x, 0.34, 0.);
        let arm = pedal - pivot;
        Transform::from_translation((pedal + pivot) * 0.5)
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, arm.normalize()))
            .with_scale(Vec3::new(0.045, arm.length(), 0.045))
    } else {
        Transform::from_translation(pedal).with_scale(Vec3::new(0.18, 0.06, 0.24))
    }
}
fn frame_parts(kind: VehicleKind) -> Vec<Part> {
    let wood = [0.42, 0.27, 0.12, 1.];
    let gold = [0.86, 0.64, 0.22, 1.];
    let green = [0.19, 0.39, 0.29, 1.];
    let dark = [0.10, 0.15, 0.14, 1.];
    let mut p = Vec::new();
    let mut add = |x, y, z, w, h, l, c| p.push((Vec3::new(x, y, z), Vec3::new(w, h, l), c));
    match kind {
        VehicleKind::Bike => {
            for (from, to) in [
                (Vec3::new(0., 0.34, -0.65), Vec3::new(0., 0.78, -0.4)),
                (Vec3::new(0., 0.78, -0.4), Vec3::new(0., 0.34, 0.)),
                (Vec3::new(0., 0.34, 0.), Vec3::new(0., 0.76, 0.22)),
                (Vec3::new(0., 0.76, 0.22), Vec3::new(0., 0.34, 0.65)),
                (Vec3::new(0., 0.34, 0.), Vec3::new(0., 0.34, 0.65)),
                (Vec3::new(0., 0.76, 0.22), Vec3::new(0., 0.78, -0.4)),
            ] {
                for i in 0..9 {
                    let v = from.lerp(to, i as f32 / 8.);
                    add(v.x, v.y, v.z, 0.085, 0.085, 0.10, green);
                }
            }
            add(0., 0.85, 0.20, 0.25, 0.08, 0.30, dark);
            let grip = bike_grip(1.);
            add(0., grip.y, grip.z, 0.62, 0.06, 0.10, gold);
            add(0., 0.34, 0., 0.45, 0.08, 0.12, gold);
        }
        VehicleKind::Kayak => {
            add(0., -0.08, 0., 0.74, 0.22, 2.1, gold);
            for z in [-1.15, 1.15] {
                add(0., -0.06, z, 0.48, 0.19, 0.40, gold);
                add(0., -0.04, z.signum() * 1.4, 0.20, 0.15, 0.20, green);
            }
            add(-0.37, 0.09, 0., 0.08, 0.20, 1.3, green);
            add(0.37, 0.09, 0., 0.08, 0.20, 1.3, green);
            add(0., 0.08, 0.12, 0.48, 0.08, 0.50, dark);
            add(0., 0.37, 0.39, 0.45, 0.42, 0.10, dark);
        }
        VehicleKind::Sailboat => {
            add(0., -0.12, 0., 1.6, 0.30, 3.0, wood);
            add(0., -0.07, -1.7, 0.9, 0.20, 0.50, wood);
            add(0., -0.04, -1.95, 0.35, 0.14, 0.12, gold);
            for x in [-0.8, 0.8] {
                add(x, 0.14, 0., 0.10, 0.24, 3.0, green);
            }
            add(0., -0.19, 0., 0.12, 0.30, 1.3, dark);
            add(0., 1.66, -0.5, 0.09, 3.3, 0.09, wood);
            add(0., 0.34, 0.65, 1.3, 0.16, 0.38, gold);
        }
    }
    if kind == VehicleKind::Kayak {
        // The shallow collision draft also sets the visible hull waterline.
        for (position, _, _) in &mut p {
            position.y += 0.08;
        }
    }
    p
}
fn sail_parts() -> Vec<Part> {
    (0..18)
        .map(|i| {
            let height = i as f32 * 0.14;
            let length = 1.9 * (1. - i as f32 / 18.);
            (
                Vec3::new(0., 0.65 + height, length / 2.),
                Vec3::new(0.04, 0.145, length),
                [0.90, 0.91, 0.69, 1.],
            )
        })
        .collect()
}
fn mesh(parts: &[Part]) -> Mesh {
    let mut g = Geometry::default();
    for (p, s, c) in parts {
        g.cuboid(*p, *s, *c);
    }
    g.into_mesh()
}
#[derive(Resource)]
pub(crate) struct Scene {
    crafts: [Handle<Mesh>; 3],
    wheel: Handle<Mesh>,
    paddle: Handle<Mesh>,
    sail: Handle<Mesh>,
    cube: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    foam: Handle<StandardMaterial>,
    gold: Handle<StandardMaterial>,
    riders: HashMap<u64, (VehicleKind, Entity)>,
}
#[derive(Component)]
pub(crate) struct Craft {
    wheel_angle: f32,
    movement_epoch: u64,
}
#[derive(Component)]
pub(crate) struct Wheel;
#[derive(Component)]
pub(crate) struct PedalPart {
    pub(crate) rider: u64,
    pub(crate) side: f32,
    pub(crate) crank: bool,
}
#[derive(Component)]
pub(crate) struct Paddle {
    rider: u64,
}
#[derive(Component)]
pub(crate) struct Sail;
#[derive(Component)]
pub(crate) struct Telltale;
#[derive(Component)]
pub(crate) struct FlowMark {
    x: i32,
    z: i32,
}
#[derive(Component)]
pub(crate) struct Hud;

pub(crate) fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut fonts: ResMut<Assets<Font>>,
) {
    let scene = Scene {
        crafts: VehicleKind::ALL.map(|kind| meshes.add(mesh(&frame_parts(kind)))),
        wheel: meshes.add(mesh(&wheel_parts())),
        paddle: meshes.add(mesh(&paddle_parts())),
        sail: meshes.add(mesh(&sail_parts())),
        cube: meshes.add(Cuboid::default()),
        material: materials.add(Color::WHITE),
        foam: materials.add(StandardMaterial {
            base_color: Color::srgb(0.63, 0.86, 0.87),
            unlit: true,
            ..default()
        }),
        gold: materials.add(StandardMaterial {
            base_color: Color::srgb(0.96, 0.78, 0.30),
            unlit: true,
            ..default()
        }),
        riders: HashMap::new(),
    };
    for z in -4..=4 {
        for x in -4..=4 {
            commands.spawn((
                GameEntity,
                FlowMark { x, z },
                Mesh3d(scene.cube.clone()),
                MeshMaterial3d(scene.foam.clone()),
                Transform::default(),
                Visibility::Hidden,
            ));
        }
    }
    let font = fonts.add(Font::from_bytes(
        include_bytes!("../../../assets/fonts/AtkinsonHyperlegible-Regular.ttf").to_vec(),
    ));
    commands
        .spawn((
            GameEntity,
            Node {
                position_type: PositionType::Absolute,
                right: px(16),
                top: px(130),
                max_width: px(235),
                padding: UiRect::all(px(8)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.13, 0.10, 0.88)),
            Visibility::Hidden,
        ))
        .with_child((
            Hud,
            Text::new(""),
            TextFont::from_font_size(16.).with_font(font),
            TextColor(Color::srgb(0.92, 0.91, 0.75)),
        ));
    commands.insert_resource(scene);
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn update(
    mut commands: Commands,
    session: Res<Session>,
    world: Res<VoxelWorld>,
    mut scene: ResMut<Scene>,
    avatars: Res<crate::Avatars>,
    avatar_transforms: Query<&Transform, (With<crate::Avatar>, Without<Craft>)>,
    mut craft_transforms: Query<(&mut Transform, &mut Craft)>,
    mut sails: Query<
        (&ChildOf, &mut Transform),
        (
            With<Sail>,
            Without<Craft>,
            Without<Telltale>,
            Without<FlowMark>,
            Without<crate::Avatar>,
        ),
    >,
    mut flags: Query<
        (&ChildOf, &mut Transform),
        (
            With<Telltale>,
            Without<Craft>,
            Without<Sail>,
            Without<FlowMark>,
            Without<crate::Avatar>,
        ),
    >,
    mut flow: Query<
        (&FlowMark, &mut Transform, &mut Visibility),
        (
            Without<Craft>,
            Without<Sail>,
            Without<Telltale>,
            Without<crate::Avatar>,
        ),
    >,
    mut hud: Query<(&mut Text, &ChildOf), With<Hud>>,
    mut hud_visibility: Query<&mut Visibility, Without<FlowMark>>,
    modals: (
        Res<crate::pause::PauseMenu>,
        Res<crate::world_map::WorldMap>,
        Res<crate::admin_console::AdminConsole>,
        Res<crate::airships::PilotConversation>,
        Res<crate::market::MarketPanel>,
        Res<crate::touch::TouchControls>,
    ),
) {
    let (pause, map, console, conversation, market, touch) = modals;
    let show_hud = session.vehicle.is_some()
        && !session.inventory.open
        && !pause.open
        && !map.open
        && !console.open
        && !conversation.open()
        && !market.open
        && !touch.menu_open;
    let wind = environment::wind(world.0.seed, session.airship_clock.time);
    let mut live = Vec::new();
    for player in &session.players {
        let craft = if player.id == session.id {
            session.vehicle
        } else {
            player.vehicle
        };
        let Some(craft) = craft else { continue };
        let position = if player.id == session.id {
            session.body.position
        } else {
            // The avatar already smooths network snapshots. Reuse that pose
            // so a second interpolation cannot detach the hull from its rider.
            avatars
                .players
                .get(&player.id)
                .and_then(|entity| avatar_transforms.get(*entity).ok())
                .map_or(player.body.position, |avatar| {
                    (avatar.translation - avatar.rotation * seat_offset(craft.kind)).to_array()
                })
        };
        live.push(player.id);
        let needs = scene
            .riders
            .get(&player.id)
            .is_none_or(|(kind, _)| *kind != craft.kind);
        if needs {
            if let Some((_, old)) = scene.riders.remove(&player.id) {
                commands.entity(old).despawn();
            }
            let entity = commands
                .spawn((
                    GameEntity,
                    Craft {
                        wheel_angle: 0.,
                        movement_epoch: player.movement_epoch,
                    },
                    Mesh3d(scene.crafts[index(craft.kind)].clone()),
                    MeshMaterial3d(scene.material.clone()),
                    Transform::from_translation(Vec3::from_array(position))
                        .with_rotation(rotation(&world.0, position, craft)),
                    Visibility::default(),
                ))
                .with_children(|p| {
                    if craft.kind == VehicleKind::Bike {
                        for centre in wheel_centres() {
                            p.spawn((
                                Wheel,
                                Mesh3d(scene.wheel.clone()),
                                MeshMaterial3d(scene.material.clone()),
                                Transform::from_translation(centre),
                            ));
                        }
                        let phase = avatars
                            .player_pedal_phase
                            .get(&player.id)
                            .copied()
                            .unwrap_or(0.);
                        for side in [-1., 1.] {
                            for crank in [false, true] {
                                p.spawn((
                                    PedalPart {
                                        rider: player.id,
                                        side,
                                        crank,
                                    },
                                    Mesh3d(scene.cube.clone()),
                                    MeshMaterial3d(scene.gold.clone()),
                                    pedal_transform(side, phase, crank),
                                ));
                            }
                        }
                    }
                    if craft.kind == VehicleKind::Kayak {
                        p.spawn((
                            Paddle { rider: player.id },
                            Mesh3d(scene.paddle.clone()),
                            MeshMaterial3d(scene.material.clone()),
                            Transform::from_translation(resting_paddle_position()),
                        ));
                    }
                    if craft.kind == VehicleKind::Sailboat {
                        p.spawn((
                            Sail,
                            Mesh3d(scene.sail.clone()),
                            MeshMaterial3d(scene.material.clone()),
                            Transform::from_xyz(0., 0., -0.5),
                        ));
                        p.spawn((
                            Telltale,
                            Mesh3d(scene.cube.clone()),
                            MeshMaterial3d(scene.gold.clone()),
                            Transform::default(),
                        ));
                    }
                })
                .id();
            scene.riders.insert(player.id, (craft.kind, entity));
        }
        let entity = scene.riders[&player.id].1;
        if let Ok((mut t, mut visual)) = craft_transforms.get_mut(entity) {
            let delta = Vec3::from_array(position) - t.translation;
            if craft.kind == VehicleKind::Bike
                && visual.movement_epoch == player.movement_epoch
                && delta.length_squared() < 16.
            {
                // Signed travel rolls the wheels even while coasting; a stop,
                // sideways slide or teleport does not pedal them in place.
                visual.wheel_angle = (visual.wheel_angle + delta.dot(t.rotation * Vec3::Z) / 0.30)
                    .rem_euclid(std::f32::consts::TAU);
            }
            visual.movement_epoch = player.movement_epoch;
            t.translation = Vec3::from_array(position);
            t.rotation = rotation(&world.0, position, craft);
        }
        let relative = (wind[0].atan2(-wind[1]) - craft.heading + std::f32::consts::PI)
            .rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        for (parent, mut t) in &mut sails {
            if parent.parent() == entity {
                t.rotation = Quat::from_rotation_y(
                    -relative.signum() * (relative.abs() * 0.5).clamp(0.15, 1.15),
                );
            }
        }
        for (parent, mut t) in &mut flags {
            if parent.parent() == entity {
                let v = if player.id == session.id {
                    session.body.velocity
                } else {
                    player.body.velocity
                };
                let apparent = Vec2::new(wind[0] - v[0], wind[1] - v[2]);
                let a = apparent.x.atan2(-apparent.y) - craft.heading;
                t.translation = Vec3::new(a.sin() * 0.36, 3.42, -0.5 - a.cos() * 0.36);
                t.rotation = Quat::from_rotation_y(-a);
                t.scale = Vec3::new(0.08, 0.12, 0.72);
            }
        }
    }
    scene.riders.retain(|id, (_, e)| {
        if live.contains(id) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
    let focus = Vec3::from_array(session.body.position);
    for (mark, mut t, mut visibility) in &mut flow {
        let base = [
            (focus.x / 4.).floor() * 4. + mark.x as f32 * 4.,
            (focus.z / 4.).floor() * 4. + mark.z as f32 * 4.,
        ];
        let current = environment::current(&world.0, base[0], base[1]);
        let speed = current[0].hypot(current[1]);
        let offset = (session.airship_clock.time % 1.5) as f32;
        let p = [base[0] + current[0] * offset, base[1] + current[1] * offset];
        if speed > 0.05
            && let Some(level) = environment::water_surface(&world.0, p[0], p[1])
        {
            *visibility = Visibility::Visible;
            t.translation = Vec3::new(p[0], level + 0.045, p[1]);
            t.rotation = Quat::from_rotation_y(-current[0].atan2(-current[1]));
            t.scale = Vec3::new(0.055, 0.012, (speed * 0.20).clamp(0.22, 0.9));
        } else {
            *visibility = Visibility::Hidden;
        }
    }
    for (mut text, parent) in &mut hud {
        if let Ok(mut v) = hud_visibility.get_mut(parent.parent()) {
            *v = if show_hud {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
        if let Some(vehicle) = session.vehicle {
            let speed = session.body.velocity[0].hypot(session.body.velocity[2]);
            let flow = environment::current(&world.0, focus.x, focus.z);
            let f = [vehicle.heading.sin(), -vehicle.heading.cos()];
            let message = if vehicle.kind == VehicleKind::Sailboat {
                format!(
                    "Wind {:.1} m/s · {}",
                    wind[0].hypot(wind[1]),
                    if rubblekin_core::vehicles::sail_drive(f, wind) < 0.01 {
                        "Tack"
                    } else {
                        "Sailing"
                    }
                )
            } else if vehicle.kind == VehicleKind::Kayak {
                format!("Current {:.1} m/s", flow[0].hypot(flow[1]))
            } else {
                if touch.enabled {
                    "Brake slows · hold Pedal hard"
                } else {
                    "Space brakes · Shift pedals"
                }
                .into()
            };
            let next = format!("{} · {:.1} m/s\n{}", vehicle.kind.name(), speed, message);
            if text.0 != next {
                text.0 = next;
            }
        }
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn animate_parts(
    avatars: Res<crate::Avatars>,
    avatar_children: Query<&Children, With<crate::Avatar>>,
    crafts: Query<&Craft>,
    limbs: Query<(&crate::Limb, &Transform), (Without<Wheel>, Without<Paddle>, Without<PedalPart>)>,
    mut wheels: Query<
        (&ChildOf, &mut Transform),
        (With<Wheel>, Without<Paddle>, Without<PedalPart>),
    >,
    mut paddles: Query<
        (&Paddle, &mut Transform),
        (Without<Wheel>, Without<crate::Limb>, Without<PedalPart>),
    >,
    mut pedals: Query<
        (&PedalPart, &mut Transform),
        (Without<Wheel>, Without<Paddle>, Without<crate::Limb>),
    >,
) {
    for (pedal, mut transform) in &mut pedals {
        let phase = avatars
            .player_pedal_phase
            .get(&pedal.rider)
            .copied()
            .unwrap_or(0.);
        *transform = pedal_transform(pedal.side, phase, pedal.crank);
    }
    for (parent, mut transform) in &mut wheels {
        if let Ok(craft) = crafts.get(parent.parent()) {
            transform.rotation = Quat::from_rotation_x(craft.wheel_angle);
        }
    }
    for (paddle, mut transform) in &mut paddles {
        let Some(children) = avatars
            .players
            .get(&paddle.rider)
            .and_then(|rider| avatar_children.get(*rider).ok())
        else {
            continue;
        };
        let mut hands = [None; 2];
        for child in children.iter() {
            if let Ok((limb, arm)) = limbs.get(child)
                && limb.arm
            {
                hands[usize::from(arm.translation.x > 0.)] =
                    Some(arm.transform_point(Vec3::NEG_Y * 0.5));
            }
        }
        if let [Some(left), Some(right)] = hands {
            transform.translation = seat_offset(VehicleKind::Kayak) + (left + right) * 0.5;
            transform.rotation = Quat::from_rotation_arc(Vec3::X, (right - left).normalize());
        }
    }
}

pub(crate) fn seat_offset(kind: VehicleKind) -> Vec3 {
    // The torso's bottom is 0.70 m above its root. Place it on the actual
    // saddle/cushion/bench, including the bench's position along the hull.
    match kind {
        VehicleKind::Bike => Vec3::new(0., 0.19, 0.20),
        VehicleKind::Kayak => Vec3::new(0., -0.50, 0.12),
        VehicleKind::Sailboat => Vec3::new(0., -0.28, 0.65),
    }
}

pub(crate) fn rotation(
    world: &rubblekin_core::world::World,
    position: [f32; 3],
    craft: rubblekin_core::vehicles::Vehicle,
) -> Quat {
    Quat::from_rotation_y(-craft.heading)
        * Quat::from_rotation_x(rubblekin_core::vehicles::pitch(world, position, craft))
}

pub(crate) fn weather(
    world: Res<VoxelWorld>,
    session: Res<Session>,
    mut materials: ResMut<Assets<crate::terrain_material::TerrainMaterial>>,
) {
    let wind = environment::wind(world.0.seed, session.airship_clock.time);
    let weather = Vec4::new(
        wind[0],
        wind[1],
        (session.airship_clock.time * 2.1).rem_euclid(std::f64::consts::TAU) as f32,
        0.,
    );
    for (_, material) in materials.iter_mut() {
        material.extension.weather = weather;
    }
}
