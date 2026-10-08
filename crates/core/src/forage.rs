//! Stable wild plant locations shared by clients and authoritative gathering.
use crate::{
    geography::Biome,
    world::{Block, BlockPos, CELL_SIZE, World},
};

pub const FORAGE_RADIUS: f32 = 42.;
pub const GATHER_COST: f32 = 5.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlantKind {
    Berries,
    Herbs,
    Clover,
    Mushrooms,
    Strawberries,
}
impl PlantKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Berries => "Wild berry bush",
            Self::Herbs => "Wild herbs",
            Self::Clover => "Flowering clover",
            Self::Mushrooms => "Woodland mushrooms",
            Self::Strawberries => "Wild strawberries",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WildPlant {
    pub index: u32,
    pub ground: BlockPos,
    pub kind: PlantKind,
}
impl WildPlant {
    pub fn position(self) -> [f32; 3] {
        [
            (self.ground.x as f32 + 0.5) * CELL_SIZE,
            (self.ground.y + 1) as f32 * CELL_SIZE,
            (self.ground.z as f32 + 0.5) * CELL_SIZE,
        ]
    }
}
pub fn density(forage: f32) -> u8 {
    (forage.clamp(0., 100.) / GATHER_COST).floor() as u8
}
fn hash(seed: u32, id: u32, index: u32) -> u32 {
    let mut h = seed ^ id.wrapping_mul(0x9e3779b9) ^ index.wrapping_mul(0x85ebca6b);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb352d);
    h ^= h >> 15;
    h
}
pub fn plants(world: &World, id: u32, position: [f32; 3], forage: f32) -> Vec<WildPlant> {
    let mut out = Vec::new();
    for index in 0..u32::from(density(forage)) * 2 {
        let h = hash(world.seed, id, index);
        let angle = (h & 65535) as f32 / 65536. * std::f32::consts::TAU;
        let radius = 4. + ((h >> 16) & 65535) as f32 / 65536. * (FORAGE_RADIUS - 4.);
        let x = position[0] + angle.cos() * radius;
        let z = position[2] + angle.sin() * radius;
        let cx = (x / CELL_SIZE).floor() as i32;
        let cz = (z / CELL_SIZE).floor() as i32;
        let cy = world.height_at(cx, cz);
        let kind = match world.geography().map(|g| g.sample(x, z).biome) {
            Some(Biome::Forest | Biome::Rainforest | Biome::PineForest) => {
                if (h >> 24).is_multiple_of(3) {
                    PlantKind::Mushrooms
                } else {
                    PlantKind::Berries
                }
            }
            Some(Biome::Shrubland) => PlantKind::Herbs,
            _ => {
                if (h >> 24).is_multiple_of(4) {
                    PlantKind::Strawberries
                } else {
                    PlantKind::Clover
                }
            }
        };
        let headroom = if kind == PlantKind::Berries { 2 } else { 1 };
        let ground = BlockPos::new(cx, cy, cz);
        if world.block(ground) == Block::Grass
            && (1..=headroom).all(|dy| world.block(BlockPos::new(cx, cy + dy, cz)) == Block::Air)
        {
            out.push(WildPlant {
                index,
                ground,
                kind,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::WorldGeneration;

    #[test]
    fn forest_and_meadow_food_mixes_regrow_at_stable_supported_positions() {
        let mut world = World::generate(42, WorldGeneration::GeographyV6);
        let kinds = [
            PlantKind::Berries,
            PlantKind::Mushrooms,
            PlantKind::Clover,
            PlantKind::Strawberries,
        ];
        let mut samples: Vec<([f32; 3], WildPlant)> = Vec::new();
        'search: for x in (-14000..=14000).step_by(1000) {
            for z in (-14000..=14000).step_by(1000) {
                let center = [x as f32, 0., z as f32];
                for plant in plants(&world, 0, center, 100.) {
                    if kinds.contains(&plant.kind)
                        && !samples.iter().any(|(_, p)| p.kind == plant.kind)
                    {
                        samples.push((center, plant));
                    }
                }
                if samples.len() == kinds.len() {
                    break 'search;
                }
            }
        }
        assert_eq!(
            samples.len(),
            kinds.len(),
            "both woodland and meadow mixes must occur"
        );
        for (center, plant) in samples {
            let full = plants(&world, 0, center, 100.);
            let thin = plants(&world, 0, center, 40.);
            assert!(thin.iter().all(|p| full.contains(p)));
            assert!(thin.len() < full.len());
            assert!(plants(&world, 0, center, 0.).is_empty());
            assert_eq!(plants(&world, 0, center, 100.), full);
            world.set_block(plant.ground, Block::Stone).unwrap();
            assert!(!plants(&world, 0, center, 100.).contains(&plant));
            world.set_block(plant.ground, Block::Grass).unwrap();
            let cover = BlockPos::new(plant.ground.x, plant.ground.y + 1, plant.ground.z);
            world.set_block(cover, Block::Brick).unwrap();
            assert!(!plants(&world, 0, center, 100.).contains(&plant));
            world.set_block(cover, Block::Air).unwrap();
            assert_eq!(plants(&world, 0, center, 100.), full);
        }
    }
}
