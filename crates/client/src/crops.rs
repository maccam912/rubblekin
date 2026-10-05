//! Small, batched, noncolliding crop visuals from authoritative growth.
use crate::{GameEntity, Session, VoxelWorld, terrain::Geometry};
use bevy::prelude::*;
use rubblekin_core::{settlement::Village, world::CELL_SIZE};

#[derive(Component)]
pub struct CropField {
    village: u32,
    stage: u8,
    mesh: Handle<Mesh>,
}

pub fn update_crops(
    mut commands: Commands,
    world: Res<VoxelWorld>,
    session: Res<Session>,
    mut fields: Query<&mut CropField>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(plan) = world.0.settlements() else {
        return;
    };
    for village in &plan.villages {
        let growth = session
            .villages
            .iter()
            .find(|v| v.id == village.id)
            .map_or(0.0, |v| v.crop_growth);
        let stage = growth_stage(growth);
        if let Some(mut field) = fields.iter_mut().find(|f| f.village == village.id) {
            // Terrain edits can remove a plant's soil. Rebuild once per coarse
            // stage; plants are decorative and never obstruct the walk routes.
            if field.stage != stage || world.is_changed() {
                if let Some(mut mesh) = meshes.get_mut(&field.mesh) {
                    *mesh = crop_geometry(village, &world.0, stage).into_mesh();
                }
                field.stage = stage;
            }
        } else {
            let mesh = meshes.add(crop_geometry(village, &world.0, stage).into_mesh());
            let material = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 1.0,
                reflectance: 0.0,
                ..default()
            });
            commands.spawn((
                GameEntity,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                CropField {
                    village: village.id,
                    stage,
                    mesh,
                },
            ));
        }
    }
}

fn crop_geometry(village: &Village, world: &rubblekin_core::world::World, stage: u8) -> Geometry {
    let mut geometry = Geometry::default();
    let color = if stage >= 4 {
        [0.78, 0.64, 0.25, 1.0]
    } else {
        [0.35, 0.56, 0.20, 1.0]
    };
    for field in &village.fields {
        for position in field.plant_positions() {
            let soil = world.block(position);
            if !matches!(
                soil,
                rubblekin_core::world::Block::Dirt | rubblekin_core::world::Block::Grass
            ) {
                continue;
            }
            for (center, size) in plant_parts(position, stage) {
                geometry.cuboid(center, size, color);
            }
        }
    }
    geometry
}

pub(crate) fn growth_stage(growth: f32) -> u8 {
    (growth.clamp(0.0, 1.0) * 5.0) as u8
}

/// Rendering and inspection use the same decorative plant shape.
pub(crate) fn plant_parts(
    position: rubblekin_core::world::BlockPos,
    stage: u8,
) -> impl Iterator<Item = (Vec3, Vec3)> {
    let height = 0.12 + f32::from(stage) * 0.14;
    let p = Vec3::new(
        (position.x as f32 + 0.5) * CELL_SIZE,
        (position.y as f32 + 1.0) * CELL_SIZE,
        (position.z as f32 + 0.5) * CELL_SIZE,
    );
    [
        Some((p + Vec3::Y * height * 0.5, Vec3::new(0.10, height, 0.10))),
        (stage >= 3).then_some((p + Vec3::Y * (height - 0.10), Vec3::new(0.22, 0.16, 0.16))),
    ]
    .into_iter()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;
    use rubblekin_core::world::{Block, BlockPos, World, WorldGeneration};

    #[test]
    fn crop_visuals_follow_growth_and_removed_soil() {
        let mut world = World::generate(42, WorldGeneration::GeographyV3);
        let village = world.settlements().unwrap().villages[0].clone();
        let young = crop_geometry(&village, &world, 0).into_mesh();
        let mature = crop_geometry(&village, &world, 5).into_mesh();
        let positions = |mesh: &Mesh| match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            VertexAttributeValues::Float32x3(values) => values.clone(),
            _ => panic!("crop positions must have three components"),
        };
        let young_positions = positions(&young);
        let mature_positions = positions(&mature);
        assert!(!young_positions.is_empty());
        assert!(mature_positions.len() > young_positions.len());
        let max_y = |points: &Vec<[f32; 3]>| {
            points
                .iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max)
        };
        assert!(max_y(&mature_positions) > max_y(&young_positions) + 0.5);
        let field = &village.fields[0];
        world
            .set_block(
                BlockPos::new(field.origin.x + 1, field.origin.y, field.origin.z + 1),
                Block::Air,
            )
            .unwrap();
        let edited = crop_geometry(&village, &world, 5).into_mesh();
        assert!(positions(&edited).len() < mature_positions.len());
    }
}
