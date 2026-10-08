//! Visible wild food follows saved habitat forage, with stable clump positions.
use crate::{GameEntity, Session, VoxelWorld, terrain::Geometry};
use bevy::{light::NotShadowCaster, prelude::*};
use rubblekin_core::{
    forage::{self, PlantKind},
    wildlife::HabitatSnapshot,
    world::World,
};
use std::collections::HashMap;

#[derive(Component)]
pub(crate) struct ForagePatch;
#[derive(Default)]
pub(crate) struct Scene {
    patches: HashMap<u32, Patch>,
    material: Option<Handle<StandardMaterial>>,
}
struct Patch {
    entity: Entity,
    mesh: Handle<Mesh>,
    stage: u8,
}
/// Actual supported plant cuboids, shared by rendering and aimed inspection.
pub(crate) fn parts(world: &World, habitat: &HabitatSnapshot) -> Vec<(Vec3, Vec3, [f32; 4])> {
    let mut out = Vec::new();
    for plant in forage::plants(world, habitat.id, habitat.position, habitat.forage) {
        let base = Vec3::from_array(plant.position());
        let berries = plant.kind == PlantKind::Berries;
        let mut part = |p, s, c| out.push((base + Vec3::from_array(p), Vec3::from_array(s), c));
        if berries {
            let green = [0.25, 0.40, 0.20, 1.];
            part([0., 0.24, 0.], [0.06, 0.48, 0.06], [0.39, 0.29, 0.17, 1.]);
            part([-0.12, 0.35, 0.], [0.38, 0.29, 0.34], green);
            part(
                [0.12, 0.52, 0.03],
                [0.32, 0.27, 0.31],
                [0.31, 0.47, 0.23, 1.],
            );
            for p in [[-0.24, 0.43, -0.12], [0.22, 0.58, -0.12], [0., 0.36, -0.18]] {
                part(p, [0.085, 0.085, 0.085], [0.62, 0.19, 0.21, 1.]);
            }
        } else {
            let dry = plant.kind == PlantKind::Herbs;
            let green = if dry {
                [0.46, 0.49, 0.29, 1.]
            } else {
                [0.29, 0.48, 0.20, 1.]
            };
            part([0., 0.13, 0.], [0.035, 0.26, 0.035], green);
            for p in [[-0.11, 0.08, 0.], [0.09, 0.12, 0.06], [0., 0.15, -0.10]] {
                part(p, [0.18, 0.035, 0.16], green);
            }
            let flower = if dry {
                [0.56, 0.52, 0.73, 1.]
            } else {
                [0.88, 0.76, 0.80, 1.]
            };
            part([0., 0.28, 0.], [0.11, 0.08, 0.11], flower);
        }
    }
    out
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn update(
    mut commands: Commands,
    world: Res<VoxelWorld>,
    session: Res<Session>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    patches: Query<(), With<ForagePatch>>,
    mut scene: Local<Scene>,
) {
    let eye = session
        .observer
        .as_ref()
        .map_or(Vec3::from_array(session.body.position), |c| c.position);
    let visible: Vec<_> = session
        .habitats
        .iter()
        .filter(|h| Vec2::new(h.position[0] - eye.x, h.position[2] - eye.z).length() < 160.)
        .collect();
    scene.patches.retain(|id, p| {
        if patches.get(p.entity).is_err() {
            false
        } else if visible.iter().any(|h| h.id == *id) {
            true
        } else {
            commands.entity(p.entity).despawn();
            false
        }
    });
    let material = scene
        .material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                perceptual_roughness: 1.,
                ..default()
            })
        })
        .clone();
    for h in visible {
        let band = forage::density(h.forage);
        if scene.patches.get(&h.id).is_some_and(|p| p.stage == band) && !world.is_changed() {
            continue;
        }
        let mut geometry = Geometry::default();
        let base = Vec3::from_array(h.position);
        let plants = parts(&world.0, h);
        if plants.is_empty() {
            if let Some(p) = scene.patches.remove(&h.id) {
                commands.entity(p.entity).despawn();
            }
            continue;
        }
        for (p, size, color) in plants {
            geometry.cuboid(p - base, size, color);
        }
        let mesh = geometry.into_mesh();
        if let Some(p) = scene.patches.get_mut(&h.id) {
            meshes.insert(p.mesh.id(), mesh).unwrap();
            p.stage = band;
        } else {
            let mesh = meshes.add(mesh);
            let entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(base),
                    ForagePatch,
                    GameEntity,
                    NotShadowCaster,
                ))
                .id();
            scene.patches.insert(
                h.id,
                Patch {
                    entity,
                    mesh,
                    stage: band,
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::world::{Block, BlockPos, CELL_SIZE};
    #[test]
    fn food_thins_regrows_at_stable_places_and_edits_remove_actual_plants() {
        let mut world = World::new(42);
        let mut h = HabitatSnapshot {
            id: 0,
            position: world.spawn_position(),
            forage: 100.,
            rabbits: 3,
            wolves: 0,
        };
        let full = parts(&world, &h);
        assert!(!full.is_empty());
        h.forage = 40.;
        let eaten = parts(&world, &h);
        assert!(eaten.len() < full.len());
        assert!(eaten.iter().all(|p| full.contains(p)));
        h.forage = 100.;
        assert_eq!(parts(&world, &h), full);
        let first = full[0].0;
        let x = (first.x / CELL_SIZE).floor() as i32;
        let z = (first.z / CELL_SIZE).floor() as i32;
        let y = world.height_at(x, z);
        world.set_block(BlockPos::new(x, y, z), Block::Air).unwrap();
        assert!(parts(&world, &h).len() < full.len());
        world
            .set_block(BlockPos::new(x, y, z), Block::Grass)
            .unwrap();
        world
            .set_block(BlockPos::new(x, y + 1, z), Block::Brick)
            .unwrap();
        assert!(parts(&world, &h).len() < full.len());
    }
}
