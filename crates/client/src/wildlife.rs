//! Small shared meshes; movement and rabbit hops follow authoritative animals.
use crate::{GameEntity, Session, terrain::Geometry};
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
#[derive(Default)]
pub(crate) struct Scene {
    entities: HashMap<u64, Entity>,
    assets: Option<CachedAssets>,
}
#[derive(Clone)]
struct CachedAssets {
    bodies: [Handle<Mesh>; 2],
    legs: [Handle<Mesh>; 2],
    material: Handle<StandardMaterial>,
}

fn body_mesh(species: Species) -> Mesh {
    let mut g = Geometry::default();
    let mut part = |p, s, c| g.cuboid(Vec3::from_array(p), Vec3::from_array(s), c);
    if species == Species::Rabbit {
        let fur = [0.61, 0.51, 0.38, 1.];
        let cream = [0.84, 0.79, 0.66, 1.];
        part([0., 0.23, 0.04], [0.37, 0.32, 0.46], fur);
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
        part([0., 0.28, 0.30], [0.13, 0.13, 0.12], cream);
        part(
            [0., 0.34, -0.39],
            [0.055, 0.04, 0.025],
            [0.44, 0.29, 0.24, 1.],
        );
    } else {
        let fur = [0.39, 0.42, 0.43, 1.];
        let pale = [0.63, 0.64, 0.59, 1.];
        part([0., 0.53, 0.], [0.40, 0.40, 0.68], fur);
        part([0., 0.57, -0.23], [0.44, 0.46, 0.29], fur);
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
        part([0., 0.45, 0.45], [0.14, 0.17, 0.30], fur);
        part([0., 0.37, 0.60], [0.12, 0.18, 0.12], fur);
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
        Species::Rabbit => Vec3::new(0.48, 0.74, 0.82),
        Species::Wolf => Vec3::new(1.38, 1.06, 1.38),
    }
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn update(
    mut commands: Commands,
    session: Res<Session>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut scene: Local<Scene>,
    mut roots: Query<(&Wildlife, &mut Transform), Without<Leg>>,
    mut legs: Query<(&Leg, &ChildOf, &mut Transform), Without<Wildlife>>,
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
            legs: [
                meshes.add(leg_mesh(Species::Rabbit)),
                meshes.add(leg_mesh(Species::Wolf)),
            ],
            material: materials.add(StandardMaterial {
                perceptual_roughness: 1.,
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
                    NotShadowCaster,
                ))
                .with_children(|parent| {
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
                            NotShadowCaster,
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
}
