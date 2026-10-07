//! Small, batched, noncolliding crop visuals from authoritative growth.
use crate::{GameEntity, Session, VoxelWorld, terrain::Geometry};
use bevy::prelude::*;
use rubblekin_core::{
    settlement::Village,
    world::{Block, BlockPos, CELL_SIZE, World, WorldGeneration},
};

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
    for (index, field) in village.fields.iter().enumerate() {
        let kind = crop_kind(world, village, index);
        for position in field.plant_positions() {
            for (center, size, color) in visible_plant_parts(world, position, stage, kind) {
                geometry.cuboid(center, size, color);
            }
        }
    }
    geometry
}

pub(crate) fn growth_stage(growth: f32) -> u8 {
    if growth > 0.0 {
        (growth.clamp(0.0, 1.0) * 5.0) as u8 + 1
    } else {
        0 // Harvested/awaiting planting fields have no decorative seedlings.
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CropKind {
    Grain,
    Leafy,
    Roots,
}

impl CropKind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Grain => "Grain",
            Self::Leafy => "Leafy greens",
            Self::Roots => "Root vegetables",
        }
    }
}

/// V6 fields mix familiar food plants. These share the same authoritative Food
/// crop cycle; this changes appearance, never crop prices, soil or yields.
pub(crate) fn crop_kind(world: &World, village: &Village, field: usize) -> CropKind {
    if world.generation() != WorldGeneration::GeographyV6 {
        return CropKind::Grain;
    }
    match (village.id as usize + field) % 3 {
        0 => CropKind::Grain,
        1 => CropKind::Leafy,
        _ => CropKind::Roots,
    }
}

/// Both the mesh and aimed inspection use these actual supported, uncovered
/// shapes. Covering the top of a tall plant removes the whole plant.
pub(crate) fn visible_plant_parts(
    world: &World,
    position: BlockPos,
    stage: u8,
    kind: CropKind,
) -> impl Iterator<Item = (Vec3, Vec3, [f32; 4])> {
    let phase = stage.saturating_sub(1).min(5);
    let height = 0.12 + f32::from(phase) * 0.14;
    let headroom = if kind == CropKind::Grain && height > CELL_SIZE {
        2
    } else {
        1
    };
    let visible = stage > 0
        && matches!(world.block(position), Block::Dirt | Block::Grass)
        && (1..=headroom).all(|dy| {
            world.block(BlockPos::new(position.x, position.y + dy, position.z)) == Block::Air
        });
    let p = Vec3::new(
        (position.x as f32 + 0.5) * CELL_SIZE,
        (position.y as f32 + 1.0) * CELL_SIZE,
        (position.z as f32 + 0.5) * CELL_SIZE,
    );
    let green = [0.35, 0.56, 0.20, 1.0];
    let mature = phase >= 4;
    let parts = match kind {
        CropKind::Grain => {
            let color = if mature {
                [0.78, 0.64, 0.25, 1.0]
            } else {
                green
            };
            [
                Some((
                    p + Vec3::Y * height * 0.5,
                    Vec3::new(0.10, height, 0.10),
                    color,
                )),
                (phase >= 3).then_some((
                    p + Vec3::Y * (height - 0.10),
                    Vec3::new(0.22, 0.16, 0.16),
                    color,
                )),
                None,
            ]
        }
        CropKind::Leafy => {
            let spread = 0.10 + f32::from(phase) * 0.045;
            let h = 0.08 + f32::from(phase) * 0.035;
            [
                Some((
                    p + Vec3::Y * h * 0.5,
                    Vec3::new(spread, h, spread),
                    [0.28, 0.47, 0.19, 1.0],
                )),
                (phase >= 3).then_some((
                    p + Vec3::Y * (h + 0.025),
                    Vec3::new(spread * 0.65, 0.05, spread * 0.65),
                    [0.43, 0.61, 0.25, 1.0],
                )),
                None,
            ]
        }
        CropKind::Roots => {
            let h = 0.08 + f32::from(phase) * 0.025;
            let spread = 0.08 + f32::from(phase) * 0.026;
            [
                Some((p + Vec3::Y * h * 0.5, Vec3::new(0.065, h, 0.065), green)),
                (phase >= 3).then_some((
                    p + Vec3::Y * (h - 0.015),
                    Vec3::new(spread, 0.065, 0.09),
                    [0.30, 0.49, 0.18, 1.0],
                )),
                (phase >= 3).then_some((
                    p + Vec3::Y * 0.035,
                    Vec3::new(0.105, 0.07, 0.105),
                    [0.78, 0.39, 0.18, 1.0],
                )),
            ]
        }
    };
    parts.into_iter().flatten().filter(move |_| visible)
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
        let young = crop_geometry(&village, &world, 1).into_mesh();
        let mature = crop_geometry(&village, &world, 6).into_mesh();
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
        let edited = crop_geometry(&village, &world, 6).into_mesh();
        assert!(positions(&edited).len() < mature_positions.len());
    }

    #[test]
    fn real_crop_sites_share_headroom_rules_and_empty_fields_have_no_plants() {
        let mut world = World::generate(42, WorldGeneration::GeographyV6);
        let village = world.settlements().unwrap().villages[0].clone();
        let soil = village.fields[0].plant_positions().next().unwrap();
        let lower = BlockPos::new(soil.x, soil.y + 1, soil.z);
        let upper = BlockPos::new(soil.x, soil.y + 2, soil.z);
        assert_eq!(growth_stage(0.0), 0);
        assert_eq!(
            crop_geometry(&village, &world, 0)
                .into_mesh()
                .count_vertices(),
            0
        );
        assert_eq!(growth_stage(0.01), 1);
        for kind in [CropKind::Grain, CropKind::Leafy, CropKind::Roots] {
            assert!(visible_plant_parts(&world, soil, 6, kind).count() >= 2);
            world.set_block(lower, Block::Glass).unwrap();
            assert_eq!(visible_plant_parts(&world, soil, 6, kind).count(), 0);
            world.set_block(lower, Block::Air).unwrap();
        }
        world.set_block(upper, Block::Wood).unwrap();
        assert_eq!(
            visible_plant_parts(&world, soil, 6, CropKind::Grain).count(),
            0
        );
        assert_eq!(
            visible_plant_parts(&world, soil, 1, CropKind::Grain).count(),
            1
        );
        assert!(visible_plant_parts(&world, soil, 6, CropKind::Leafy).count() > 0);
        world.set_block(upper, Block::Air).unwrap();
        world.set_block(soil, Block::Stone).unwrap();
        assert_eq!(
            visible_plant_parts(&world, soil, 6, CropKind::Roots).count(),
            0
        );
    }

    #[test]
    fn mixed_food_shapes_remain_inside_their_soil_column_and_bounded_mesh() {
        let world = World::generate(42, WorldGeneration::GeographyV6);
        let village = &world.settlements().unwrap().villages[0];
        let soil = village.fields[0].plant_positions().next().unwrap();
        let base = Vec3::new(
            (soil.x as f32 + 0.5) * CELL_SIZE,
            (soil.y as f32 + 1.0) * CELL_SIZE,
            (soil.z as f32 + 0.5) * CELL_SIZE,
        );
        let mut appearances = Vec::new();
        for kind in [CropKind::Grain, CropKind::Leafy, CropKind::Roots] {
            let mut geometry = Geometry::default();
            for stage in 1..=6 {
                let parts: Vec<_> = visible_plant_parts(&world, soil, stage, kind).collect();
                assert!(parts.len() <= 3);
                for (center, size, color) in parts {
                    let relative = center - base;
                    assert!(center.is_finite() && size.is_finite());
                    assert!(relative.x.abs() + size.x * 0.5 <= CELL_SIZE * 0.5);
                    assert!(relative.z.abs() + size.z * 0.5 <= CELL_SIZE * 0.5);
                    assert!(relative.y - size.y * 0.5 >= -0.0001);
                    assert!(relative.y + size.y * 0.5 <= 0.83);
                    if stage == 6 {
                        geometry.cuboid(center, size, color);
                    }
                }
            }
            let mesh = geometry.into_mesh();
            assert!(mesh.count_vertices() <= 72);
            appearances.push(mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().clone());
        }
        assert_ne!(appearances[0], appearances[1]);
        assert_ne!(appearances[1], appearances[2]);
        let old = World::generate(42, WorldGeneration::GeographyV5);
        assert_eq!(
            crop_kind(&old, &old.settlements().unwrap().villages[0], 1),
            CropKind::Grain
        );
    }
}
