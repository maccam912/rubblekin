//! Small shared meshes; movement and rabbit hops follow authoritative animals.
use crate::{GameEntity, Session, VoxelWorld, terrain::Geometry};
use bevy::{light::NotShadowCaster, prelude::*};
use rubblekin_core::wildlife::{Species, WildlifeAction, WildlifeSnapshot};
use std::collections::HashMap;

#[derive(Component)]
pub(crate) struct Wildlife {
    pub id: u64,
}
#[derive(Component)]
pub(crate) struct Leg {
    phase: f32,
}
#[derive(Component)]
pub(crate) struct Head;
#[derive(Component)]
pub(crate) struct WildlifeShadow;
#[derive(Default)]
pub(crate) struct Scene {
    entities: HashMap<u64, Entity>,
    assets: Option<CachedAssets>,
}
#[derive(Clone)]
struct CachedAssets {
    bodies: [Handle<Mesh>; 2],
    heads: [Handle<Mesh>; 2],
    legs: [Handle<Mesh>; 2],
    material: Handle<StandardMaterial>,
    shadow_mesh: Handle<Mesh>,
    shadow_material: Handle<StandardMaterial>,
}

fn body_mesh(species: Species) -> Mesh {
    let mut g = Geometry::default();
    let mut part = |p, s, c| g.cuboid(Vec3::from_array(p), Vec3::from_array(s), c);
    if species == Species::Rabbit {
        let fur = [0.61, 0.51, 0.38, 1.];
        let cream = [0.84, 0.79, 0.66, 1.];
        part([0., 0.23, 0.04], [0.37, 0.32, 0.46], fur);
        part([0., 0.28, 0.30], [0.13, 0.13, 0.12], cream);
    } else {
        let fur = [0.39, 0.42, 0.43, 1.];
        part([0., 0.53, 0.], [0.40, 0.40, 0.68], fur);
        part([0., 0.57, -0.23], [0.44, 0.46, 0.29], fur);
        part([0., 0.45, 0.45], [0.14, 0.17, 0.30], fur);
        part([0., 0.37, 0.60], [0.12, 0.18, 0.12], fur);
    }
    g.into_mesh()
}
fn head_pivot(species: Species) -> Vec3 {
    match species {
        Species::Rabbit => Vec3::new(0., 0.37, -0.23),
        Species::Wolf => Vec3::new(0., 0.74, -0.38),
    }
}
fn head_mesh(species: Species) -> Mesh {
    let mut g = Geometry::default();
    let pivot = head_pivot(species);
    let mut part = |p, s, c| g.cuboid(Vec3::from_array(p) - pivot, Vec3::from_array(s), c);
    if species == Species::Rabbit {
        let fur = [0.61, 0.51, 0.38, 1.];
        let cream = [0.84, 0.79, 0.66, 1.];
        part([0., 0.37, -0.23], [0.27, 0.24, 0.26], fur);
        part([0., 0.32, -0.34], [0.19, 0.12, 0.08], cream);
        for x in [-0.085, 0.085] {
            part([x, 0.59, -0.20], [0.075, 0.28, 0.10], fur);
            part(
                [x, 0.59, -0.253],
                [0.036, 0.19, 0.015],
                [0.70, 0.49, 0.43, 1.],
            );
            part(
                [x * 1.6, 0.41, -0.29],
                [0.025, 0.035, 0.04],
                [0.06, 0.045, 0.035, 1.],
            );
        }
        part(
            [0., 0.34, -0.39],
            [0.055, 0.04, 0.025],
            [0.44, 0.29, 0.24, 1.],
        );
    } else {
        let fur = [0.39, 0.42, 0.43, 1.];
        let pale = [0.63, 0.64, 0.59, 1.];
        part([0., 0.74, -0.38], [0.34, 0.31, 0.31], fur);
        part([0., 0.67, -0.56], [0.22, 0.17, 0.20], pale);
        part(
            [0., 0.69, -0.665],
            [0.15, 0.09, 0.055],
            [0.08, 0.09, 0.09, 1.],
        );
        for x in [-0.12, 0.12] {
            part([x, 0.94, -0.35], [0.10, 0.19, 0.13], fur);
            part([x, 1.02, -0.35], [0.06, 0.05, 0.08], fur);
            part(
                [x * 1.47, 0.79, -0.47],
                [0.025, 0.04, 0.06],
                [0.84, 0.67, 0.26, 1.],
            );
        }
    }
    g.into_mesh()
}
fn leg_mesh(species: Species) -> Mesh {
    let mut g = Geometry::default();
    let (size, color) = if species == Species::Rabbit {
        ([0.115, 0.14, 0.19], [0.55, 0.45, 0.33, 1.])
    } else {
        ([0.12, 0.37, 0.15], [0.32, 0.35, 0.36, 1.])
    };
    g.cuboid(
        Vec3::new(0., -size[1] * 0.5, 0.),
        Vec3::from_array(size),
        color,
    );
    g.into_mesh()
}
pub(crate) fn rendered_size(species: Species) -> Vec3 {
    match species {
        Species::Rabbit => Vec3::new(0.48, 0.74, 1.),
        Species::Wolf => Vec3::new(1.38, 1.06, 1.38),
    }
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn update(
    mut commands: Commands,
    session: Res<Session>,
    world: Res<VoxelWorld>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut scene: Local<Scene>,
    mut roots: Query<
        (&Wildlife, &mut Transform),
        (Without<Leg>, Without<Head>, Without<WildlifeShadow>),
    >,
    mut legs: Query<
        (&Leg, &ChildOf, &mut Transform),
        (Without<Wildlife>, Without<Head>, Without<WildlifeShadow>),
    >,
    mut heads: Query<
        (&ChildOf, &mut Transform),
        (
            With<Head>,
            Without<Wildlife>,
            Without<Leg>,
            Without<WildlifeShadow>,
        ),
    >,
    mut shadows: Query<
        (&ChildOf, &mut Transform, &mut Visibility),
        (
            With<WildlifeShadow>,
            Without<Wildlife>,
            Without<Leg>,
            Without<Head>,
        ),
    >,
) {
    if scene.entities.values().any(|e| roots.get(*e).is_err()) {
        scene.entities.retain(|_, e| roots.get(*e).is_ok());
    }
    let assets = scene
        .assets
        .get_or_insert_with(|| CachedAssets {
            bodies: [
                meshes.add(body_mesh(Species::Rabbit)),
                meshes.add(body_mesh(Species::Wolf)),
            ],
            heads: [
                meshes.add(head_mesh(Species::Rabbit)),
                meshes.add(head_mesh(Species::Wolf)),
            ],
            legs: [
                meshes.add(leg_mesh(Species::Rabbit)),
                meshes.add(leg_mesh(Species::Wolf)),
            ],
            material: materials.add(StandardMaterial {
                perceptual_roughness: 1.,
                ..default()
            }),
            shadow_mesh: meshes.add(Circle::new(0.5)),
            shadow_material: materials.add(StandardMaterial {
                base_color: Color::srgba(0.04, 0.07, 0.06, 0.26),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            }),
        })
        .clone();
    let eye = session
        .observer
        .as_ref()
        .map_or(Vec3::from_array(session.body.position), |c| c.position);
    let visible: Vec<&WildlifeSnapshot> = session
        .wildlife
        .iter()
        .filter(|a| Vec3::from_array(a.position).distance_squared(eye) < 180_f32.powi(2))
        .collect();
    scene.entities.retain(|id, e| {
        if visible.iter().any(|a| a.id == *id) {
            true
        } else {
            commands.entity(*e).despawn();
            false
        }
    });
    for a in &visible {
        let index = usize::from(a.species == Species::Wolf);
        let entity = *scene.entities.entry(a.id).or_insert_with(|| {
            commands
                .spawn((
                    Mesh3d(assets.bodies[index].clone()),
                    MeshMaterial3d(assets.material.clone()),
                    Transform::from_translation(Vec3::from_array(a.position)),
                    Wildlife { id: a.id },
                    GameEntity,
                ))
                .with_children(|parent| {
                    parent.spawn((
                        Head,
                        Mesh3d(assets.heads[index].clone()),
                        MeshMaterial3d(assets.material.clone()),
                        Transform::from_translation(head_pivot(a.species)),
                    ));
                    parent.spawn((
                        WildlifeShadow,
                        Mesh3d(assets.shadow_mesh.clone()),
                        MeshMaterial3d(assets.shadow_material.clone()),
                        Transform::from_rotation(Quat::from_rotation_x(
                            -std::f32::consts::FRAC_PI_2,
                        )),
                        Visibility::Hidden,
                        NotShadowCaster,
                    ));
                    let (x, z, y) = if a.species == Species::Rabbit {
                        (0.14, 0.16, 0.14)
                    } else {
                        (0.145, 0.23, 0.37)
                    };
                    for (i, (sx, sz)) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)]
                        .into_iter()
                        .enumerate()
                    {
                        parent.spawn((
                            Mesh3d(assets.legs[index].clone()),
                            MeshMaterial3d(assets.material.clone()),
                            Transform::from_xyz(sx * x, y, sz * z),
                            Leg {
                                phase: if i == 0 || i == 3 {
                                    0.
                                } else {
                                    std::f32::consts::PI
                                },
                            },
                        ));
                    }
                })
                .id()
        });
        if let Ok((_, mut pose)) = roots.get_mut(entity) {
            let desired = Vec3::from_array(a.position);
            if pose.translation.distance_squared(desired) > 9. {
                pose.translation = desired;
            } else {
                pose.translation = pose
                    .translation
                    .lerp(desired, 1. - (-time.delta_secs() * 18.).exp());
            }
            let v = Vec2::new(a.velocity[0], a.velocity[2]);
            if v.length_squared() > 0.04 {
                pose.rotation = pose.rotation.slerp(
                    Quat::from_rotation_y((-v.x).atan2(-v.y)),
                    1. - (-time.delta_secs() * 12.).exp(),
                );
            }
        }
    }
    for (leg, parent, mut pose) in &mut legs {
        let Ok((root, _)) = roots.get(parent.parent()) else {
            continue;
        };
        let Some(a) = session.wildlife.iter().find(|a| a.id == root.id) else {
            continue;
        };
        let moving = a.velocity[0].hypot(a.velocity[2]) > 0.15;
        pose.rotation = Quat::from_rotation_x(if moving {
            (time.elapsed_secs() * if a.species == Species::Wolf { 11. } else { 8. }
                + leg.phase
                + a.id as f32)
                .sin()
                * 0.55
        } else if a.action == WildlifeAction::Grazing {
            0.08
        } else {
            0.
        });
    }
    for (parent, mut pose) in &mut heads {
        let Ok((animal, _)) = roots.get(parent.parent()) else {
            continue;
        };
        let Some(a) = visible.iter().find(|a| a.id == animal.id) else {
            continue;
        };
        let phase = time.elapsed_secs() * 3.5 + (a.id % 1024) as f32;
        let grazing = a.species == Species::Rabbit && a.action == WildlifeAction::Grazing;
        let mut target = head_pivot(a.species);
        if grazing {
            target.y -= 0.07;
        }
        let pitch = if grazing {
            -0.65 + phase.sin() * 0.09
        } else if a.action == WildlifeAction::Hunting {
            -0.10
        } else {
            0.
        };
        let blend = 1. - (-time.delta_secs() * 8.).exp();
        pose.translation = pose.translation.lerp(target, blend);
        pose.rotation = pose.rotation.slerp(Quat::from_rotation_x(pitch), blend);
    }
    for (parent, mut shadow, mut visibility) in &mut shadows {
        let Ok((animal, pose)) = roots.get(parent.parent()) else {
            continue;
        };
        let Some(a) = visible.iter().find(|a| a.id == animal.id) else {
            continue;
        };
        let p = pose.translation;
        let ground = world.0.raycast([p.x, p.y + 0.05, p.z], [0., -1., 0.], 1.25);
        if let Some(hit) = ground.filter(|_| !session.graphics.shadows()) {
            let y = p.y + 0.05 - hit.distance + 0.012;
            shadow.translation = Vec3::new(0., y - p.y, 0.);
            let size = if a.species == Species::Rabbit {
                Vec3::new(0.64, 0.8, 1.)
            } else {
                Vec3::new(0.85, 1.3, 1.)
            };
            shadow.scale = size / (1. + (p.y - y).max(0.) * 0.6);
            *visibility = Visibility::Inherited;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{graphics::GraphicsQuality, join};
    use rubblekin_core::{
        protocol::SessionMode,
        world::{Block, BlockPos, CELL_SIZE},
    };
    #[test]
    fn shadows_stay_on_actual_ground_through_hops_edits_quality_changes_and_rejoins() {
        let (world, mut session) = join::session_from_welcome(
            join::tests::welcome(SessionMode::Player),
            "wildlife".into(),
            GraphicsQuality::Low,
            0.,
            SessionMode::Player,
        )
        .unwrap();
        let spawn = world.spawn_position();
        let x = ((spawn[0] + 6.) / CELL_SIZE).floor() as i32;
        let z = (spawn[2] / CELL_SIZE).floor() as i32;
        let p = Vec3::new(
            (x as f32 + 0.5) * CELL_SIZE,
            (world.height_at(x, z) + 1) as f32 * CELL_SIZE,
            (z as f32 + 0.5) * CELL_SIZE,
        );
        session.wildlife = vec![WildlifeSnapshot {
            id: 1,
            species: Species::Rabbit,
            position: p.to_array(),
            velocity: [0.; 3],
            action: WildlifeAction::Resting,
            hunger: 20.,
            habitat: 0,
        }];
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(VoxelWorld(world))
            .init_resource::<Time>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, update);
        app.update();
        app.update();
        let root = app
            .world_mut()
            .query::<(Entity, &Wildlife)>()
            .single(app.world())
            .unwrap()
            .0;
        let shadow = app
            .world_mut()
            .query_filtered::<Entity, With<WildlifeShadow>>()
            .single(app.world())
            .unwrap();
        assert_eq!(
            app.world().get::<Visibility>(shadow),
            Some(&Visibility::Inherited)
        );
        assert!(app.world().get::<NotShadowCaster>(root).is_none());
        let initial_scale = app.world().get::<Transform>(shadow).unwrap().scale;
        let head = app
            .world_mut()
            .query_filtered::<Entity, With<Head>>()
            .single(app.world())
            .unwrap();
        // Feeding changes the head without tilting the authoritative body or
        // allocating new meshes. Leaving that activity restores its idle pose.
        app.world_mut().resource_mut::<Session>().wildlife[0].action = WildlifeAction::Grazing;
        for _ in 0..5 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_millis(100));
            app.update();
        }
        let pose = app.world().get::<Transform>(head).unwrap();
        assert!(pose.rotation.to_euler(EulerRot::XYZ).0 < -0.3);
        assert!(pose.translation.y < head_pivot(Species::Rabbit).y - 0.05);
        assert_eq!(
            app.world().get::<Transform>(root).unwrap().rotation,
            Quat::IDENTITY
        );
        app.world_mut().resource_mut::<Session>().wildlife[0].action = WildlifeAction::Resting;
        for _ in 0..10 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_millis(100));
            app.update();
        }
        let pose = app.world().get::<Transform>(head).unwrap();
        assert!(pose.rotation.angle_between(Quat::IDENTITY) < 0.001);
        assert!(pose.translation.distance(head_pivot(Species::Rabbit)) < 0.001);
        // The body hops while its inexpensive shadow stays on the terrain.
        app.world_mut().resource_mut::<Session>().wildlife[0].position[1] += 0.4;
        app.world_mut()
            .get_mut::<Transform>(root)
            .unwrap()
            .translation
            .y += 0.4;
        app.update();
        let body = app.world().get::<Transform>(root).unwrap();
        let contact = app.world().get::<Transform>(shadow).unwrap();
        assert!((body.translation.y + contact.translation.y - p.y - 0.012).abs() < 0.001);
        assert!(contact.scale.x < initial_scale.x);
        // A player-built raised floor is the contact surface, not baseline height.
        let floor = BlockPos::new(x, (p.y / CELL_SIZE) as i32 + 1, z);
        app.world_mut()
            .resource_mut::<VoxelWorld>()
            .0
            .set_block(floor, Block::Wood)
            .unwrap();
        app.world_mut().resource_mut::<Session>().wildlife[0].position[1] = p.y + 1.;
        app.world_mut()
            .get_mut::<Transform>(root)
            .unwrap()
            .translation
            .y = p.y + 1.;
        app.update();
        let body = app.world().get::<Transform>(root).unwrap();
        let contact = app.world().get::<Transform>(shadow).unwrap();
        assert!((body.translation.y + contact.translation.y - p.y - 1.012).abs() < 0.001);
        app.world_mut().resource_mut::<Session>().graphics = GraphicsQuality::Balanced;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(shadow),
            Some(&Visibility::Hidden)
        );
        app.world_mut().resource_mut::<Session>().graphics = GraphicsQuality::Low;
        app.world_mut().resource_mut::<Session>().wildlife[0].position[1] += 3.;
        app.world_mut()
            .get_mut::<Transform>(root)
            .unwrap()
            .translation
            .y += 3.;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(shadow),
            Some(&Visibility::Hidden)
        );
        app.world_mut().despawn(root);
        app.update();
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&Wildlife>()
                .iter(app.world())
                .count(),
            1
        );
        assert_eq!(
            app.world_mut()
                .query::<&WildlifeShadow>()
                .iter(app.world())
                .count(),
            1
        );
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 7);
        assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 2);
    }
}
