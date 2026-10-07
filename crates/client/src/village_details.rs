//! Small surface details for existing workbenches and grain bins.
//! They share the nearby terrain mesh and never create inventory or collision.
use bevy::prelude::*;
use rubblekin_core::{
    settlement::{BuildingPlot, VillageKind},
    village_assets::{self, BuildingKind},
    world::{Block, BlockPos, CELL_SIZE, World},
};

use crate::terrain::Geometry;

/// Append details owned by this half-open cell rectangle: min x/z, max x/z.
/// At most eleven cuboids per village, with no per-frame entities or materials.
pub(crate) fn append(mesh: &mut Geometry, world: &World, bounds: [i32; 4]) {
    let Some(plan) = world.settlements() else {
        return;
    };
    for village in &plan.villages {
        for building in &village.buildings {
            append_building(mesh, world, building, village.kind, bounds);
        }
    }
}

fn append_building(
    mesh: &mut Geometry,
    world: &World,
    building: &BuildingPlot,
    region: VillageKind,
    bounds: [i32; 4],
) {
    let [width, _, depth] = building.dimensions();
    if building.origin.x >= bounds[2]
        || building.origin.x + width <= bounds[0]
        || building.origin.z >= bounds[3]
        || building.origin.z + depth <= bounds[1]
    {
        return;
    }
    let (sites, support): (&[[i32; 2]], Block) = match building.kind {
        BuildingKind::Workshop => (&[[14, 5]], Block::Stone),
        BuildingKind::Storehouse => (&[[3, 6], [12, 6], [3, 14], [12, 14]], Block::Sand),
        _ => return,
    };
    for &[x, z] in sites {
        let cell = surface_cell(building, x, z);
        if cell.x < bounds[0]
            || cell.x >= bounds[2]
            || cell.z < bounds[1]
            || cell.z >= bounds[3]
            || world.block(cell) != support
            || world.block(BlockPos::new(cell.x, cell.y - 1, cell.z)) != Block::Wood
            || world.block(BlockPos::new(cell.x, cell.y + 1, cell.z)) != Block::Air
        {
            continue;
        }
        let base = Vec3::new(
            (cell.x as f32 + 0.5) * CELL_SIZE,
            (cell.y + 1) as f32 * CELL_SIZE,
            (cell.z as f32 + 0.5) * CELL_SIZE,
        );
        let mut part = |offset: Vec3, size: Vec3, color| {
            let offset = match building.rotation % 4 {
                0 => offset,
                1 => Vec3::new(-offset.z, offset.y, offset.x),
                2 => Vec3::new(-offset.x, offset.y, -offset.z),
                _ => Vec3::new(offset.z, offset.y, -offset.x),
            };
            let size = if building.rotation.is_multiple_of(2) {
                size
            } else {
                Vec3::new(size.z, size.y, size.x)
            };
            mesh.cuboid(base + offset, size, color);
        };
        if building.kind == BuildingKind::Workshop {
            let head = match region {
                VillageKind::Mining | VillageKind::Quarry => [0.52, 0.54, 0.53, 1.],
                _ => [0.54, 0.37, 0.20, 1.],
            };
            part(
                Vec3::new(-0.06, 0.026, 0.015),
                Vec3::new(0.042, 0.05, 0.32),
                [0.66, 0.49, 0.29, 1.],
            );
            part(
                Vec3::new(-0.06, 0.055, -0.10),
                Vec3::new(0.21, 0.11, 0.105),
                head,
            );
            part(
                Vec3::new(0.12, 0.020, 0.01),
                Vec3::new(0.046, 0.04, 0.26),
                [0.66, 0.67, 0.62, 1.],
            );
        } else {
            // Tied bands describe the existing solid grain sack, not live stock.
            part(
                Vec3::new(0., 0.015, 0.),
                Vec3::new(0.48, 0.030, 0.045),
                [0.45, 0.32, 0.19, 1.],
            );
            part(
                Vec3::new(0., 0.018, 0.),
                Vec3::new(0.045, 0.036, 0.48),
                [0.45, 0.32, 0.19, 1.],
            );
        }
    }
}

fn surface_cell(building: &BuildingPlot, x: i32, z: i32) -> BlockPos {
    let [width, _, depth] = village_assets::dimensions(building.kind);
    let [x, z] = match building.rotation % 4 {
        0 => [x, z],
        1 => [depth - 1 - z, x],
        2 => [width - 1 - x, depth - 1 - z],
        _ => [z, width - 1 - x],
    };
    BlockPos::new(
        building.origin.x + x,
        building.origin.y + 2,
        building.origin.z + z,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;
    use rubblekin_core::world::WorldGeneration;

    fn positions(geometry: Geometry) -> Vec<[f32; 3]> {
        let mesh = geometry.into_mesh();
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            VertexAttributeValues::Float32x3(values) => values.clone(),
            _ => panic!("expected positions"),
        }
    }

    #[test]
    fn real_village_fixtures_have_bounded_details_without_changing_the_world() {
        for generation in [WorldGeneration::GeographyV3, WorldGeneration::GeographyV6] {
            let world = World::generate(42, generation);
            let before = world.edits();
            for village in &world.settlements().unwrap().villages {
                let mut geometry = Geometry::default();
                for building in &village.buildings {
                    append_building(
                        &mut geometry,
                        &world,
                        building,
                        village.kind,
                        [-32768, -32768, 32768, 32768],
                    );
                }
                assert_eq!(
                    positions(geometry).len(),
                    11 * 24,
                    "{} has one tool set and four grain ties",
                    village.name
                );
            }
            assert_eq!(world.edits(), before);
        }
    }

    #[test]
    fn rotated_fixtures_require_original_support_and_empty_space_and_have_one_chunk_owner() {
        let mut world = World::new(42);
        for kind in [BuildingKind::Workshop, BuildingKind::Storehouse] {
            for rotation in 0..4 {
                let building = BuildingPlot {
                    kind,
                    origin: BlockPos::new(-7, world.max_y() - 12, -9),
                    rotation,
                };
                let sites: &[[i32; 2]] = if kind == BuildingKind::Workshop {
                    &[[14, 5]]
                } else {
                    &[[3, 6], [12, 6], [3, 14], [12, 14]]
                };
                let material = if kind == BuildingKind::Workshop {
                    Block::Stone
                } else {
                    Block::Sand
                };
                for &[x, z] in sites {
                    let cell = surface_cell(&building, x, z);
                    assert_eq!(building.local_cell(cell.x, cell.z), Some([x, z]));
                    world.set_block(cell, material).unwrap();
                    world
                        .set_block(BlockPos::new(cell.x, cell.y - 1, cell.z), Block::Wood)
                        .unwrap();
                    world
                        .set_block(BlockPos::new(cell.x, cell.y + 1, cell.z), Block::Air)
                        .unwrap();
                }
                let geometry = |world: &World, bounds| {
                    let mut mesh = Geometry::default();
                    append_building(&mut mesh, world, &building, VillageKind::Quarry, bounds);
                    positions(mesh)
                };
                let full = geometry(&world, [-32, -32, 32, 32]);
                let count: usize = (-2..2)
                    .flat_map(|cx| (-2..2).map(move |cz| (cx, cz)))
                    .map(|(cx, cz)| {
                        geometry(&world, [cx * 16, cz * 16, (cx + 1) * 16, (cz + 1) * 16]).len()
                    })
                    .sum();
                assert_eq!(
                    full.len(),
                    count,
                    "each rotated fixture is owned by one chunk"
                );
                let expected = if kind == BuildingKind::Workshop {
                    3 * 24
                } else {
                    8 * 24
                };
                assert_eq!(full.len(), expected);
                for &[x, z] in sites {
                    let cell = surface_cell(&building, x, z);
                    world
                        .set_block(BlockPos::new(cell.x, cell.y + 1, cell.z), Block::Brick)
                        .unwrap();
                }
                assert!(geometry(&world, [-32, -32, 32, 32]).is_empty());
                for &[x, z] in sites {
                    let cell = surface_cell(&building, x, z);
                    world
                        .set_block(BlockPos::new(cell.x, cell.y + 1, cell.z), Block::Air)
                        .unwrap();
                    world.set_block(cell, Block::Air).unwrap();
                }
                assert!(geometry(&world, [-32, -32, 32, 32]).is_empty());
            }
        }
    }
}
