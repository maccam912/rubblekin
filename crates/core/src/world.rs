//! Deterministic, finite voxel terrain. Positions address 50 cm cells; all
//! public floating point positions and ray distances are in meters.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const CELL_SIZE: f32 = 0.5;
pub const CHUNK_SIZE: i32 = 16;
pub const WORLD_RADIUS: i32 = 160;
pub const MIN_Y: i32 = -24;
pub const MAX_Y: i32 = 160;
pub const WATER_LEVEL: f32 = 0.85;
const WIDTH: usize = (WORLD_RADIUS * 2) as usize;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Block {
    #[default]
    Air,
    Grass,
    Dirt,
    Stone,
    Sand,
    Wood,
    Leaves,
    Brick,
    Glass,
}

impl Block {
    pub fn is_solid(self) -> bool {
        self != Self::Air
    }

    /// Material colors in sRGB. The renderer supplies material transparency.
    pub fn color(self) -> [f32; 4] {
        match self {
            Self::Air => [0.0, 0.0, 0.0, 0.0],
            Self::Grass => [0.40, 0.57, 0.28, 1.0],
            Self::Dirt => [0.43, 0.31, 0.20, 1.0],
            Self::Stone => [0.54, 0.57, 0.58, 1.0],
            Self::Sand => [0.75, 0.68, 0.48, 1.0],
            Self::Wood => [0.43, 0.29, 0.16, 1.0],
            Self::Leaves => [0.25, 0.43, 0.23, 1.0],
            Self::Brick => [0.62, 0.32, 0.23, 1.0],
            Self::Glass => [0.57, 0.78, 0.83, 1.0],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Air => "Air",
            Self::Grass => "Grass",
            Self::Dirt => "Earth",
            Self::Stone => "Stone",
            Self::Sand => "Sand",
            Self::Wood => "Wood",
            Self::Leaves => "Leaves",
            Self::Brick => "Brick",
            Self::Glass => "Glass",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockEdit {
    pub position: BlockPos,
    pub block: Block,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    pub position: BlockPos,
    /// Last empty cell crossed, or `position` when the ray starts in a solid.
    pub previous: BlockPos,
    pub distance: f32,
}

#[derive(Clone)]
pub struct World {
    pub seed: u32,
    heights: Vec<i32>,
    surfaces: Vec<Block>,
    column_tops: Vec<i32>,
    trees: HashMap<BlockPos, Block>,
    overrides: HashMap<BlockPos, Block>,
}

impl World {
    pub fn new(seed: u32) -> Self {
        let mut world = Self {
            seed,
            heights: vec![0; WIDTH * WIDTH],
            surfaces: vec![Block::Grass; WIDTH * WIDTH],
            column_tops: vec![0; WIDTH * WIDTH],
            trees: HashMap::new(),
            overrides: HashMap::new(),
        };
        for z in -WORLD_RADIUS..WORLD_RADIUS {
            for x in -WORLD_RADIUS..WORLD_RADIUS {
                let index = column_index(x, z).unwrap();
                let (height, material) = world.generate_column(x, z);
                world.heights[index] = height;
                world.column_tops[index] = height;
                world.surfaces[index] = material;
            }
        }
        world.generate_trees();
        world
    }

    /// Both upper boundaries are exclusive: x/z -160..160, y -24..160.
    pub fn is_editable(position: BlockPos) -> bool {
        column_index(position.x, position.z).is_some() && (MIN_Y..MAX_Y).contains(&position.y)
    }

    pub fn block(&self, position: BlockPos) -> Block {
        if !Self::is_editable(position) {
            return Block::Air;
        }
        self.overrides
            .get(&position)
            .copied()
            .unwrap_or_else(|| self.base_block(position))
    }

    pub fn set_block(&mut self, position: BlockPos, block: Block) -> Result<(), String> {
        if !Self::is_editable(position) {
            return Err(format!(
                "Block ({}, {}, {}) is outside the editable world",
                position.x, position.y, position.z
            ));
        }
        if block == self.base_block(position) {
            self.overrides.remove(&position);
        } else {
            self.overrides.insert(position, block);
        }
        let index = column_index(position.x, position.z).unwrap();
        if block.is_solid() {
            self.column_tops[index] = self.column_tops[index].max(position.y);
        } else if self.column_tops[index] == position.y {
            self.column_tops[index] = (MIN_Y..position.y)
                .rev()
                .find(|&y| {
                    self.block(BlockPos::new(position.x, y, position.z))
                        .is_solid()
                })
                .unwrap_or(MIN_Y - 1);
        }
        Ok(())
    }

    /// Stable ordering makes saves reproducible and straightforward to inspect.
    pub fn edits(&self) -> Vec<BlockEdit> {
        let mut edits: Vec<_> = self
            .overrides
            .iter()
            .map(|(&position, &block)| BlockEdit { position, block })
            .collect();
        edits.sort_unstable_by_key(|edit| (edit.position.x, edit.position.y, edit.position.z));
        edits
    }

    pub fn from_edits(seed: u32, edits: &[BlockEdit]) -> Result<Self, String> {
        if let Some(edit) = edits.iter().find(|edit| !Self::is_editable(edit.position)) {
            return Err(format!(
                "Saved block position {:?} is outside the world",
                edit.position
            ));
        }
        let mut world = Self::new(seed);
        for edit in edits {
            world.set_block(edit.position, edit.block)?;
        }
        Ok(world)
    }

    /// Original terrain elevation, excluding trees and player edits.
    /// Out-of-world columns return MIN_Y - 1 (an empty column).
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        column_index(x, z).map_or(MIN_Y - 1, |index| self.heights[index])
    }

    /// Highest solid surface in meters, including edits and tree canopies.
    /// This is a height query, not a pathfinding or cave-floor query.
    pub fn surface_height(&self, x: f32, z: f32) -> f32 {
        if !x.is_finite() || !z.is_finite() {
            return MIN_Y as f32 * CELL_SIZE;
        }
        let x = (x / CELL_SIZE).floor() as i32;
        let z = (z / CELL_SIZE).floor() as i32;
        column_index(x, z).map_or(MIN_Y as f32 * CELL_SIZE, |index| {
            (self.column_tops[index] + 1) as f32 * CELL_SIZE
        })
    }

    pub fn spawn_position(&self) -> [f32; 3] {
        // A half-meter-wide character straddles neighboring columns. Account
        // for those columns too, and leave room for its head after player edits.
        for radius in 0_i32..=32 {
            for z in -radius..=radius {
                for x in -radius..=radius {
                    if x.abs().max(z.abs()) != radius {
                        continue;
                    }
                    let mut top = MIN_Y;
                    for dz in -1..=1 {
                        for dx in -1..=1 {
                            let index = column_index(x + dx, z + dz).unwrap();
                            top = top.max(self.column_tops[index]);
                        }
                    }
                    if top < MAX_Y - 5 {
                        return [
                            (x as f32 + 0.5) * CELL_SIZE,
                            (top + 1) as f32 * CELL_SIZE + 0.02,
                            (z as f32 + 0.5) * CELL_SIZE,
                        ];
                    }
                }
            }
        }
        // Filling every nearby column to the world ceiling leaves no ordinary
        // spawn. The controller's finite bounds still protect invalid states.
        [0.25, (MAX_Y - 4) as f32 * CELL_SIZE, 0.25]
    }

    /// Stream center in cell coordinates. The stream's water is decorative;
    /// terrain in its shallow bed remains solid and editable.
    pub fn river_center(&self, z: i32) -> f32 {
        let phase = (self.seed % 1000) as f32 * 0.006;
        32.0 + (z as f32 * 0.024 + phase).sin() * 10.0 + (z as f32 * 0.061).sin() * 3.0
    }

    /// Grid DDA with clipping to the finite world. Direction need not be unit
    /// length. Invalid vectors are rejected and traversal is bounded.
    pub fn raycast(
        &self,
        origin: [f32; 3],
        direction: [f32; 3],
        max_distance: f32,
    ) -> Option<RayHit> {
        if origin
            .iter()
            .chain(direction.iter())
            .any(|v| !v.is_finite())
            || !max_distance.is_finite()
            || max_distance < 0.0
        {
            return None;
        }
        let length = direction
            .iter()
            .map(|&v| (v as f64).powi(2))
            .sum::<f64>()
            .sqrt();
        if length < 1e-12 {
            return None;
        }
        let direction = direction.map(|v| (v as f64 / length) as f32);
        let minimum = [
            -(WORLD_RADIUS as f32) * CELL_SIZE,
            MIN_Y as f32 * CELL_SIZE,
            -(WORLD_RADIUS as f32) * CELL_SIZE,
        ];
        let maximum = [
            WORLD_RADIUS as f32 * CELL_SIZE,
            MAX_Y as f32 * CELL_SIZE,
            WORLD_RADIUS as f32 * CELL_SIZE,
        ];
        let mut entry = 0.0_f32;
        let mut exit = max_distance;
        for axis in 0..3 {
            if direction[axis].abs() < 1e-12 {
                if origin[axis] < minimum[axis] || origin[axis] >= maximum[axis] {
                    return None;
                }
            } else {
                let a = (minimum[axis] - origin[axis]) / direction[axis];
                let b = (maximum[axis] - origin[axis]) / direction[axis];
                entry = entry.max(a.min(b));
                exit = exit.min(a.max(b));
            }
        }
        if entry > exit || exit < 0.0 {
            return None;
        }
        // A small inward offset gives negative-facing boundary entry the cell
        // it actually enters, while reported distances remain at the boundary.
        let start = if entry > 0.0 { entry + 0.00001 } else { entry };
        let start_point: [f32; 3] = std::array::from_fn(|i| origin[i] + direction[i] * start);
        let mut cell = start_point.map(|v| (v / CELL_SIZE).floor() as i32);
        for axis in 0..3 {
            if direction[axis] < 0.0
                && (start_point[axis] / CELL_SIZE - (start_point[axis] / CELL_SIZE).round()).abs()
                    < 1e-6
            {
                cell[axis] -= 1;
            }
        }
        let steps = direction.map(|v| {
            if v > 0.0 {
                1
            } else if v < 0.0 {
                -1
            } else {
                0
            }
        });
        let delta = direction.map(|v| {
            if v == 0.0 {
                f32::INFINITY
            } else {
                CELL_SIZE / v.abs()
            }
        });
        let mut next: [f32; 3] = std::array::from_fn(|i| {
            if steps[i] == 0 {
                f32::INFINITY
            } else {
                let boundary = (cell[i] + i32::from(steps[i] > 0)) as f32 * CELL_SIZE;
                (boundary - origin[i]) / direction[i]
            }
        });
        let mut position = BlockPos::new(cell[0], cell[1], cell[2]);
        let mut previous = position;
        let mut distance = entry;
        // A straight ray cannot cross more planes than the sum of dimensions.
        for _ in 0..(WIDTH * 2 + (MAX_Y - MIN_Y) as usize + 6) {
            if distance > exit + 0.00001 || distance > max_distance + 0.00001 {
                return None;
            }
            if self.block(position).is_solid() {
                return Some(RayHit {
                    position,
                    previous,
                    distance: distance.max(0.0),
                });
            }
            let axis = if next[0] <= next[1] && next[0] <= next[2] {
                0
            } else if next[1] <= next[2] {
                1
            } else {
                2
            };
            previous = position;
            distance = next[axis];
            cell[axis] += steps[axis];
            next[axis] += delta[axis];
            position = BlockPos::new(cell[0], cell[1], cell[2]);
        }
        None
    }

    fn base_block(&self, position: BlockPos) -> Block {
        let Some(index) = column_index(position.x, position.z) else {
            return Block::Air;
        };
        if !(MIN_Y..MAX_Y).contains(&position.y) {
            return Block::Air;
        }
        let height = self.heights[index];
        if position.y > height {
            return self.trees.get(&position).copied().unwrap_or(Block::Air);
        }
        if position.y == height {
            self.surfaces[index]
        } else if position.y >= height - 3 && self.surfaces[index] != Block::Stone {
            if self.surfaces[index] == Block::Sand {
                Block::Sand
            } else {
                Block::Dirt
            }
        } else {
            Block::Stone
        }
    }

    fn generate_column(&self, x: i32, z: i32) -> (i32, Block) {
        let xf = x as f32;
        let zf = z as f32;
        let distance = (xf * xf + zf * zf).sqrt();
        let clearing = smoothstep(14.0, 35.0, distance);
        let broad = value_noise(xf / 48.0, zf / 48.0, self.seed);
        let fine = value_noise(xf / 17.0, zf / 17.0, self.seed.wrapping_add(31));
        let ridge_distance = (xf.abs() * 0.92).max(zf.abs() * 0.83);
        let mountains = smoothstep(38.0, 152.0, ridge_distance);
        let ridges = 0.65 + 0.35 * (xf * 0.048 + zf * 0.035 + broad * 2.2).sin().abs();
        let mut height = 4.0
            + clearing * (broad * 4.0 + fine * 1.5)
            + mountains * mountains * (66.0 + broad * 27.0) * ridges;
        let stream_distance = (xf - self.river_center(z)).abs();
        let valley = smoothstep(5.0, 25.0, stream_distance);
        height = 4.0 + (height - 4.0) * valley;
        if stream_distance < 5.0 {
            height = if stream_distance < 2.2 {
                0.0
            } else {
                (stream_distance - 2.2) / 2.8 * 4.0
            };
        }
        let height = height.round().clamp(0.0, (MAX_Y - 15) as f32) as i32;
        let material = if stream_distance < 5.0 {
            Block::Sand
        } else if height > 37 {
            Block::Stone
        } else {
            Block::Grass
        };
        (height, material)
    }

    fn generate_trees(&mut self) {
        // One candidate per large grid square prevents dense overlapping crowns.
        for gz in -9_i32..9 {
            for gx in -9_i32..9 {
                let h = hash(gx, gz, self.seed.wrapping_add(817));
                if h.is_multiple_of(4) {
                    continue;
                }
                let x = gx * 17 + 5 + ((h >> 4) % 9) as i32;
                let z = gz * 17 + 5 + ((h >> 12) % 9) as i32;
                if x * x + z * z < 36 * 36 {
                    continue;
                }
                let Some(index) = column_index(x, z) else {
                    continue;
                };
                if self.surfaces[index] != Block::Grass
                    || (x as f32 - self.river_center(z)).abs() < 9.0
                    || self.height_at(x, z) > 31
                {
                    continue;
                }
                let bottom = self.height_at(x, z) + 1;
                let trunk_height = 7 + ((h >> 20) % 4) as i32;
                for y in bottom..bottom + trunk_height {
                    self.insert_tree(BlockPos::new(x, y, z), Block::Wood);
                }
                let crown = bottom + trunk_height - 1;
                for dy in -3_i32..=3 {
                    for dz in -4_i32..=4 {
                        for dx in -4_i32..=4 {
                            if dx * dx + dz * dz + dy * dy * 2 > 19 {
                                continue;
                            }
                            let pos = BlockPos::new(x + dx, crown + dy, z + dz);
                            if !self.trees.contains_key(&pos) {
                                self.insert_tree(pos, Block::Leaves);
                            }
                        }
                    }
                }
            }
        }
    }

    fn insert_tree(&mut self, position: BlockPos, block: Block) {
        if Self::is_editable(position) && position.y > self.height_at(position.x, position.z) {
            self.trees.insert(position, block);
            let index = column_index(position.x, position.z).unwrap();
            self.column_tops[index] = self.column_tops[index].max(position.y);
        }
    }
}

fn column_index(x: i32, z: i32) -> Option<usize> {
    if !(-WORLD_RADIUS..WORLD_RADIUS).contains(&x) || !(-WORLD_RADIUS..WORLD_RADIUS).contains(&z) {
        return None;
    }
    Some((z + WORLD_RADIUS) as usize * WIDTH + (x + WORLD_RADIUS) as usize)
}

/// Renewable food sources for the first forager. These are logical scene
/// locations; the client draws their bushes and the server owns availability.
pub fn berry_patch_positions(world: &World) -> Vec<[f32; 3]> {
    [(6.0, 5.0), (-7.0, 3.0), (4.0, -8.0)]
        .into_iter()
        .map(|(x, z)| [x, world.surface_height(x, z), z])
        .collect()
}

fn smoothstep(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash(x: i32, z: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1)
        ^ (z as u32).wrapping_mul(0x85EB_CA77)
        ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^ (h >> 16)
}

fn value_noise(x: f32, z: f32, seed: u32) -> f32 {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let tx = smoothstep(0.0, 1.0, x - ix as f32);
    let tz = smoothstep(0.0, 1.0, z - iz as f32);
    let sample = |dx, dz| hash(ix + dx, iz + dz, seed) as f32 / u32::MAX as f32 * 2.0 - 1.0;
    let a = sample(0, 0) * (1.0 - tx) + sample(1, 0) * tx;
    let b = sample(0, 1) * (1.0 - tx) + sample(1, 1) * tx;
    a * (1.0 - tz) + b * tz
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_round_trip_and_restoring_base_removes_override() {
        let mut world = World::new(23);
        let pos = BlockPos::new(-1, 4, -1);
        let base = world.block(pos);
        world.set_block(pos, Block::Air).unwrap();
        world
            .set_block(BlockPos::new(1, 14, 2), Block::Brick)
            .unwrap();
        let edits = world.edits();
        let restored = World::from_edits(23, &edits).unwrap();
        assert_eq!(restored.edits(), edits);
        assert_eq!(restored.block(pos), Block::Air);
        assert_eq!(restored.surface_height(0.75, 1.25), 7.5);
        world.set_block(pos, base).unwrap();
        assert_eq!(world.edits().len(), 1);
    }

    #[test]
    fn negative_coordinates_use_floor_and_top_tracks_removals() {
        let mut world = World::new(1);
        world
            .set_block(BlockPos::new(-1, 20, -1), Block::Brick)
            .unwrap();
        assert_eq!(world.surface_height(-0.01, -0.01), 10.5);
        assert_eq!(world.surface_height(0.01, 0.01), 2.5);
        world
            .set_block(BlockPos::new(-1, 20, -1), Block::Air)
            .unwrap();
        assert_eq!(world.surface_height(-0.01, -0.01), 2.5);
    }

    #[test]
    fn bounds_are_exclusive_and_invalid_saves_fail() {
        let mut world = World::new(1);
        assert!(World::is_editable(BlockPos::new(-160, -24, 159)));
        for position in [
            BlockPos::new(160, 1, 0),
            BlockPos::new(0, 160, 0),
            BlockPos::new(i32::MIN, 1, 0),
            BlockPos::new(0, -25, 0),
        ] {
            assert!(!World::is_editable(position));
            assert_eq!(world.block(position), Block::Air);
            assert!(world.set_block(position, Block::Stone).is_err());
            assert!(
                World::from_edits(
                    1,
                    &[BlockEdit {
                        position,
                        block: Block::Stone
                    }]
                )
                .is_err()
            );
        }
    }

    #[test]
    fn ray_hits_negative_cells_with_correct_distance_and_neighbor() {
        let mut world = World::new(7);
        let target = BlockPos::new(-3, 20, -1);
        world.set_block(target, Block::Brick).unwrap();
        let hit = world
            .raycast([0.25, 10.25, -0.25], [-2.0, 0.0, 0.0], 5.0)
            .unwrap();
        assert_eq!(hit.position, target);
        assert_eq!(hit.previous, BlockPos::new(-2, 20, -1));
        assert!((hit.distance - 1.25).abs() < 0.0001);
        assert!(
            world
                .raycast([0.25, 10.25, -0.25], [-1.0, 0.0, 0.0], 1.0)
                .is_none()
        );
    }

    #[test]
    fn ray_boundaries_and_invalid_inputs_are_safe() {
        let mut world = World::new(7);
        world
            .set_block(BlockPos::new(-1, 20, 0), Block::Brick)
            .unwrap();
        let hit = world
            .raycast([0.0, 10.25, 0.25], [-1.0, 0.0, 0.0], 1.0)
            .unwrap();
        assert_eq!(hit.position, BlockPos::new(-1, 20, 0));
        assert_eq!(hit.distance, 0.0);
        assert!(world.raycast([0.0; 3], [0.0; 3], 10.0).is_none());
        assert!(
            world
                .raycast([f32::NAN; 3], [0.0, -1.0, 0.0], 10.0)
                .is_none()
        );
        let hit = world
            .raycast([0.25, 1000.0, 0.25], [0.0, -1.0, 0.0], 1200.0)
            .unwrap();
        assert_eq!(hit.position.y, 4);
        assert!((hit.distance - 997.5).abs() < 0.001);
    }

    #[test]
    fn generation_is_seeded_and_trees_are_editable() {
        let first = World::new(912);
        let same = World::new(912);
        let other = World::new(913);
        assert_eq!(first.heights, same.heights);
        assert_eq!(first.trees, same.trees);
        assert_ne!(first.heights, other.heights);
        let mut world = first.clone();
        let (&position, &material) = first.trees.iter().next().unwrap();
        assert_eq!(world.block(position), material);
        world.set_block(position, Block::Air).unwrap();
        assert_eq!(world.block(position), Block::Air);
        let loaded = World::from_edits(912, &world.edits()).unwrap();
        assert_eq!(loaded.block(position), Block::Air);
    }

    #[test]
    fn spawn_avoids_edits_against_world_ceiling() {
        let mut world = World::new(23);
        world
            .set_block(BlockPos::new(0, MAX_Y - 1, 0), Block::Stone)
            .unwrap();
        let spawn = world.spawn_position();
        assert!(spawn[0].abs() > CELL_SIZE || spawn[2].abs() > CELL_SIZE);
        assert!(spawn[1] + 1.7 < MAX_Y as f32 * CELL_SIZE);
        assert_eq!(world.block(BlockPos::new(0, MAX_Y - 1, 0)), Block::Stone);
    }
}
