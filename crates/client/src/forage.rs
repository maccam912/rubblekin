//! Visible wild food follows saved habitat forage, with stable clump positions.
use crate::{GameEntity, Session, VoxelWorld, terrain::Geometry};
use bevy::{light::NotShadowCaster, prelude::*};
use rubblekin_core::{
    geography::Biome,
    wildlife::HabitatSnapshot,
    world::{Block, BlockPos, CELL_SIZE, World},
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
fn stage(forage: f32) -> u8 {
    (forage.clamp(0., 100.) / 5.).floor() as u8
}
fn hash(seed: u32, id: u32, index: u32) -> u32 {
    let mut h = seed ^ id.wrapping_mul(0x9e3779b9) ^ index.wrapping_mul(0x85ebca6b);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb352d);
    h ^= h >> 15;
    h
}
/// Actual supported plant cuboids, shared by rendering and aimed inspection.
pub(crate) fn parts(world: &World, habitat: &HabitatSnapshot) -> Vec<(Vec3, Vec3, [f32; 4])> {
    let mut out = Vec::new();
    let count = usize::from(stage(habitat.forage)) * 2;
    for index in 0..count {
        let h = hash(world.seed, habitat.id, index as u32);
        let angle = (h & 65535) as f32 / 65536. * std::f32::consts::TAU;
        let radius = 4. + ((h >> 16) & 65535) as f32 / 65536. * 38.;
        let x = habitat.position[0] + angle.cos() * radius;
        let z = habitat.position[2] + angle.sin() * radius;
        let cx = (x / CELL_SIZE).floor() as i32;
        let cz = (z / CELL_SIZE).floor() as i32;
        let cy = world.height_at(cx, cz);
        let biome = world.geography().map(|g| g.sample(x, z).biome);
        let berries = matches!(
            biome,
            Some(Biome::Forest | Biome::Rainforest | Biome::PineForest)
        );
        if world.block(BlockPos::new(cx, cy, cz)) != Block::Grass
            || !(1..=if berries { 2 } else { 1 })
                .all(|dy| world.block(BlockPos::new(cx, cy + dy, cz)) == Block::Air)
        {
            continue;
        }
        let base = Vec3::new(
            (cx as f32 + 0.5) * CELL_SIZE,
            (cy + 1) as f32 * CELL_SIZE,
            (cz as f32 + 0.5) * CELL_SIZE,
        );
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
            let dry = matches!(biome, Some(Biome::Shrubland));
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
        let band = stage(h.forage);
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
