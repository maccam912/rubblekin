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
    let wood = [0.42, 0.27, 0.12, 1.];
    let gold = [0.86, 0.64, 0.22, 1.];
    let green = [0.19, 0.39, 0.29, 1.];
    let dark = [0.10, 0.15, 0.14, 1.];
    let mut p = Vec::new();
    let mut add = |x, y, z, w, h, l, c| p.push((Vec3::new(x, y, z), Vec3::new(w, h, l), c));
    match kind {
        VehicleKind::Bike => {
            for z in [-0.65, 0.65] {
                for i in 0..20 {
                    let a = i as f32 * std::f32::consts::TAU / 20.;
                    add(
                        0.,
                        0.34 + a.sin() * 0.30,
                        z + a.cos() * 0.30,
                        0.13,
                        0.10,
                        0.10,
                        dark,
                    );
                }
                add(0., 0.34, z, 0.15, 0.07, 0.50, gold);
                add(0., 0.34, z, 0.15, 0.50, 0.07, gold);
            }
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
            add(0., 0.94, -0.4, 0.62, 0.06, 0.10, gold);
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
            add(0., 0.46, -0.38, 1.8, 0.045, 0.045, wood);
            add(-0.94, 0.46, -0.38, 0.35, 0.06, 0.19, gold);
            add(0.94, 0.46, -0.38, 0.35, 0.06, 0.19, gold);
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
    sail: Handle<Mesh>,
    cube: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    foam: Handle<StandardMaterial>,
    gold: Handle<StandardMaterial>,
    riders: HashMap<u64, (VehicleKind, Entity)>,
}
#[derive(Component)]
pub(crate) struct Craft;
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
        crafts: VehicleKind::ALL.map(|kind| meshes.add(mesh(&parts(kind)))),
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
    mut craft_transforms: Query<&mut Transform, With<Craft>>,
    mut sails: Query<
        (&ChildOf, &mut Transform),
        (
            With<Sail>,
            Without<Craft>,
            Without<Telltale>,
            Without<FlowMark>,
        ),
    >,
    mut flags: Query<
        (&ChildOf, &mut Transform),
        (
            With<Telltale>,
            Without<Craft>,
            Without<Sail>,
            Without<FlowMark>,
        ),
    >,
    mut flow: Query<
        (&FlowMark, &mut Transform, &mut Visibility),
        (Without<Craft>, Without<Sail>, Without<Telltale>),
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
    time: Res<Time>,
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
                    Craft,
                    Mesh3d(scene.crafts[index(craft.kind)].clone()),
                    MeshMaterial3d(scene.material.clone()),
                    Transform::default(),
                    Visibility::default(),
                ))
                .with_children(|p| {
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
        let position = if player.id == session.id {
            session.body.position
        } else {
            player.body.position
        };
        if let Ok(mut t) = craft_transforms.get_mut(entity) {
            let target = Vec3::from_array(position);
            t.translation = if player.id == session.id || t.translation.distance(target) > 4. {
                target
            } else {
                t.translation
                    .lerp(target, 1. - (-12. * time.delta_secs()).exp())
            };
            let f = [craft.heading.sin(), -craft.heading.cos()];
            let length = craft.kind.dimensions()[2];
            let surface = |sign: f32| {
                let x = position[0] + f[0] * length * sign;
                let z = position[2] + f[1] * length * sign;
                if craft.kind.watercraft() {
                    environment::water_surface(&world.0, x, z).unwrap_or(position[1])
                } else {
                    world.0.surface_height(x, z)
                }
            };
            let pitch = ((surface(1.) - surface(-1.)) / (length * 2.))
                .atan()
                .clamp(-0.45, 0.45);
            t.rotation = Quat::from_rotation_y(-craft.heading) * Quat::from_rotation_x(pitch);
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
                let v = player.body.velocity;
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
                "Jump brakes · Sprint pedals".into()
            };
            let next = format!("{} · {:.1} m/s\n{}", vehicle.kind.name(), speed, message);
            if text.0 != next {
                text.0 = next;
            }
        }
    }
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
