//! Deterministic, finite voxel terrain. Positions address 50 cm cells; all
//! public floating point positions and ray distances are in meters.

use crate::geography::{Biome, Geography};
use crate::settlement::{ConstructionColumn, ResourceKind, SettlementPlan};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, RwLock},
};

pub const CELL_SIZE: f32 = 0.5;
pub const CHUNK_SIZE: i32 = 16;
pub const WORLD_RADIUS: i32 = 160;
pub const MIN_Y: i32 = -24;
pub const MAX_Y: i32 = 160;
pub const WATER_LEVEL: f32 = 0.85;
const WIDTH: usize = (WORLD_RADIUS * 2) as usize;
const GEOGRAPHY_RADIUS: i32 = 32768;
const GEOGRAPHY_MIN_Y: i32 = -1024;
const GEOGRAPHY_MAX_Y: i32 = 8192;
const COLUMN_CACHE_LIMIT: usize = 131_072;
#[cfg(test)]
#[path = "world_poi_tests.rs"]
mod poi_tests;

/// Saved terrain rules. Missing fields in old saves retain the original valley.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorldGeneration {
    #[default]
    ValleyV1,
    GeographyV1,
    GeographyV2,
    GeographyV3,
    GeographyV4,
    GeographyV5,
    GeographyV6,
}

impl WorldGeneration {
    pub const fn has_settlements(self) -> bool {
        matches!(
            self,
            Self::GeographyV3 | Self::GeographyV4 | Self::GeographyV5 | Self::GeographyV6
        )
    }
}

pub use crate::blocks::Block;

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
    generation: WorldGeneration,
    geography: Option<Arc<Geography>>,
    settlements: Option<Arc<SettlementPlan>>,
    geographic_columns: Arc<RwLock<HashMap<(i32, i32), GeographicColumn>>>,
    edited_columns: HashMap<(i32, i32), BTreeMap<i32, Block>>,
    heights: Vec<i32>,
    surfaces: Vec<Block>,
    column_tops: Vec<i32>,
    trees: HashMap<BlockPos, Block>,
    overrides: HashMap<BlockPos, Block>,
}

#[derive(Clone, Copy)]
struct GeographicColumn {
    height: i32,
    surface: Block,
    wood: Option<(i32, i32)>,
    leaves: Option<(i32, i32)>,
    construction: Option<ConstructionColumn>,
    deposit: Option<(ResourceKind, i32, i32)>,
    site: Option<crate::poi::SiteColumn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeKind {
    Broadleaf,
    Conifer,
    Scrub,
    Aspen,
    Cedar,
    Canopy,
}

impl TreeKind {
    pub fn leaf_color(self) -> [f32; 4] {
        match self {
            Self::Broadleaf | Self::Aspen | Self::Canopy => Block::Leaves.color(),
            Self::Conifer | Self::Cedar => [0.18, 0.34, 0.25, 1.0],
            Self::Scrub => [0.46, 0.48, 0.23, 1.0],
        }
    }
}

/// One deterministic tree, shared by editable columns and distant scenery.
/// Positions, heights, and crown radii are in voxel cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedTree {
    pub base: BlockPos,
    pub trunk_height: i32,
    pub kind: TreeKind,
    pub crown_radius: i32,
}

impl GeneratedTree {
    pub fn crown_y(self) -> i32 {
        self.base.y + self.trunk_height - 1
    }

    pub fn leaf_bounds(self, dx: i32, dz: i32) -> Option<(i32, i32)> {
        let horizontal = dx * dx + dz * dz;
        let (bottom, top) = match self.kind {
            TreeKind::Broadleaf => (-4, 4),
            TreeKind::Conifer => (-8, 2),
            TreeKind::Scrub => (-2, 2),
            TreeKind::Aspen => (-6, 3),
            TreeKind::Cedar => (-11, 3),
            TreeKind::Canopy => (-2, 2),
        };
        let mut low = i32::MAX;
        let mut high = i32::MIN;
        for dy in bottom..=top {
            let covered = match self.kind {
                TreeKind::Broadleaf => horizontal + dy * dy * 2 <= 24,
                TreeKind::Scrub => horizontal + dy * dy <= 5,
                TreeKind::Aspen => horizontal * 5 + (dy + 1) * (dy + 1) <= 25,
                TreeKind::Canopy => horizontal + dy * dy * 5 <= 30,
                TreeKind::Cedar => {
                    let radius = match dy {
                        ..=-8 => 5,
                        -7..=-5 => 4,
                        -4..=-2 => 3,
                        -1..=1 => 2,
                        _ => 1,
                    };
                    horizontal <= radius * radius
                }
                TreeKind::Conifer => {
                    let radius = match dy {
                        ..=-5 => 4,
                        -4..=-2 => 3,
                        -1..=0 => 2,
                        _ => 1,
                    };
                    horizontal <= radius * radius
                }
            };
            if covered {
                low = low.min(self.crown_y() + dy);
                high = high.max(self.crown_y() + dy);
            }
        }
        (low <= high).then_some((low, high))
    }
}

impl GeographicColumn {
    fn top(self) -> i32 {
        self.height
            .max(self.site.map_or(self.height, |site| site.top()))
            .max(self.wood.map_or(self.height, |(_, top)| top))
            .max(self.leaves.map_or(self.height, |(_, top)| top))
            .max(
                self.construction
                    .map_or(self.height, |construction| construction.top),
            )
    }

    fn vegetation(self, y: i32) -> Block {
        if self
            .wood
            .is_some_and(|(low, high)| (low..=high).contains(&y))
        {
            Block::Wood
        } else if self
            .leaves
            .is_some_and(|(low, high)| (low..=high).contains(&y))
        {
            Block::Leaves
        } else {
            Block::Air
        }
    }
}

impl World {
    pub fn new(seed: u32) -> Self {
        let mut world = Self {
            seed,
            generation: WorldGeneration::ValleyV1,
            geography: None,
            settlements: None,
            geographic_columns: Arc::new(RwLock::new(HashMap::new())),
            edited_columns: HashMap::new(),
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

    pub fn generate(seed: u32, generation: WorldGeneration) -> Self {
        if generation == WorldGeneration::ValleyV1 {
            return Self::new(seed);
        }
        let geography = if generation == WorldGeneration::GeographyV1 {
            Geography::generate(seed)
        } else {
            Geography::generate_v2(seed)
        };
        let mut world = Self {
            seed,
            generation,
            geography: Some(Arc::new(geography)),
            settlements: None,
            geographic_columns: Arc::new(RwLock::new(HashMap::new())),
            edited_columns: HashMap::new(),
            heights: Vec::new(),
            surfaces: Vec::new(),
            column_tops: Vec::new(),
            trees: HashMap::new(),
            overrides: HashMap::new(),
        };
        if generation.has_settlements() {
            // New scenery must not rescore timber catchments and move towns.
            // A private V4 view keeps original settlement decisions and caches
            // separate from the V5 trees used by the finished world.
            let planning_world = (matches!(
                generation,
                WorldGeneration::GeographyV5 | WorldGeneration::GeographyV6
            ))
            .then(|| Self {
                generation: WorldGeneration::GeographyV4,
                geographic_columns: Arc::new(RwLock::new(HashMap::new())),
                ..world.clone()
            });
            let mut plan = SettlementPlan::generate(planning_world.as_ref().unwrap_or(&world));
            if matches!(
                generation,
                WorldGeneration::GeographyV4
                    | WorldGeneration::GeographyV5
                    | WorldGeneration::GeographyV6
            ) {
                plan.add_regional_buildings(&world);
            }
            if matches!(
                generation,
                WorldGeneration::GeographyV5 | WorldGeneration::GeographyV6
            ) {
                plan.add_roadside_landmarks(&world);
            }
            if generation == WorldGeneration::GeographyV6 {
                plan.add_roadside_workyards(&world);
            }
            world.settlements = Some(Arc::new(plan.clone()));
            if let Ok(transit) = crate::airships::AirshipNetwork::try_new(&world) {
                plan.clear_landing_trees(&transit);
                if generation == WorldGeneration::GeographyV6 {
                    plan.add_exploration_sites(&world, &transit);
                    plan.add_wilderness_sites(&world, &transit);
                    plan.add_composed_sites(&world, &transit);
                }
            }
            // Landing selection sampled columns before its clearings and sites.
            world.geographic_columns.write().unwrap().clear();
            world.settlements = Some(Arc::new(plan));
        }
        world
    }

    pub fn settlements(&self) -> Option<&SettlementPlan> {
        self.settlements.as_deref()
    }

    pub fn generation(&self) -> WorldGeneration {
        self.generation
    }

    pub fn geography(&self) -> Option<&Geography> {
        self.geography.as_deref()
    }

    pub fn radius_cells(&self) -> i32 {
        if self.geography.is_some() {
            GEOGRAPHY_RADIUS
        } else {
            WORLD_RADIUS
        }
    }

    pub fn min_y(&self) -> i32 {
        if self.geography.is_some() {
            GEOGRAPHY_MIN_Y
        } else {
            MIN_Y
        }
    }

    pub fn max_y(&self) -> i32 {
        if self.geography.is_some() {
            GEOGRAPHY_MAX_Y
        } else {
            MAX_Y
        }
    }

    pub fn contains_block(&self, position: BlockPos) -> bool {
        let radius = self.radius_cells();
        (-radius..radius).contains(&position.x)
            && (-radius..radius).contains(&position.z)
            && (self.min_y()..self.max_y()).contains(&position.y)
    }

    /// Both upper boundaries are exclusive: x/z -160..160, y -24..160.
    pub fn is_editable(position: BlockPos) -> bool {
        column_index(position.x, position.z).is_some() && (MIN_Y..MAX_Y).contains(&position.y)
    }

    pub fn block(&self, position: BlockPos) -> Block {
        if !self.contains_block(position) {
            return Block::Air;
        }
        self.overrides
            .get(&position)
            .copied()
            .unwrap_or_else(|| self.base_block(position))
    }

    pub fn set_block(&mut self, position: BlockPos, block: Block) -> Result<(), String> {
        if !self.contains_block(position) {
            return Err(format!(
                "Block ({}, {}, {}) is outside the editable world",
                position.x, position.y, position.z
            ));
        }
        if block == self.base_block(position) {
            self.overrides.remove(&position);
            if let Some(column) = self.edited_columns.get_mut(&(position.x, position.z)) {
                column.remove(&position.y);
                if column.is_empty() {
                    self.edited_columns.remove(&(position.x, position.z));
                }
            }
        } else {
            self.overrides.insert(position, block);
            self.edited_columns
                .entry((position.x, position.z))
                .or_default()
                .insert(position.y, block);
        }
        if self.geography.is_some() {
            return Ok(());
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

    pub fn from_generation_edits(
        seed: u32,
        generation: WorldGeneration,
        edits: &[BlockEdit],
    ) -> Result<Self, String> {
        let mut world = Self::generate(seed, generation);
        for edit in edits {
            if !world.contains_block(edit.position) {
                return Err(format!(
                    "Saved block position {:?} is outside the world",
                    edit.position
                ));
            }
            world.set_block(edit.position, edit.block)?;
        }
        Ok(world)
    }

    /// Original terrain elevation, excluding trees and player edits.
    /// Out-of-world columns return MIN_Y - 1 (an empty column).
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        if self.geography.is_some() {
            self.geographic_column(x, z)
                .map_or(self.min_y() - 1, |column| column.height)
        } else {
            column_index(x, z).map_or(MIN_Y - 1, |index| self.heights[index])
        }
    }

    /// Original material on the terrain surface, excluding trees and edits.
    pub fn surface_block(&self, x: i32, z: i32) -> Block {
        if self.geography.is_some() {
            self.geographic_column(x, z)
                .map_or(Block::Air, |column| column.surface)
        } else {
            column_index(x, z).map_or(Block::Air, |index| self.surfaces[index])
        }
    }

    /// The renderer need only inspect terrain at and above the lowest changed
    /// cell or original surface; untouched deep bedrock has no exposed faces.
    /// Neighboring columns still need consideration when selecting a mesh floor.
    pub fn column_mesh_floor(&self, x: i32, z: i32) -> i32 {
        let height = self.height_at(x, z);
        self.edited_columns
            .get(&(x, z))
            .and_then(|column| column.first_key_value())
            .map_or(height, |(&lowest, _)| height.min(lowest))
    }

    fn column_top(&self, x: i32, z: i32) -> i32 {
        if self.geography.is_none() {
            return column_index(x, z).map_or(MIN_Y - 1, |index| self.column_tops[index]);
        }
        let Some(column) = self.geographic_column(x, z) else {
            return self.min_y() - 1;
        };
        let Some(edits) = self.edited_columns.get(&(x, z)) else {
            return column.top();
        };
        let mut top = column.top().max(
            edits
                .iter()
                .rev()
                .find_map(|(&y, &block)| block.is_solid().then_some(y))
                .unwrap_or(self.min_y() - 1),
        );
        while top >= self.min_y() && !self.block(BlockPos::new(x, top, z)).is_solid() {
            top -= 1;
        }
        top
    }

    /// Highest solid surface in meters, including edits and tree canopies.
    /// This is a height query, not a pathfinding or cave-floor query.
    pub fn surface_height(&self, x: f32, z: f32) -> f32 {
        if !x.is_finite() || !z.is_finite() {
            return self.min_y() as f32 * CELL_SIZE;
        }
        let x = (x / CELL_SIZE).floor() as i32;
        let z = (z / CELL_SIZE).floor() as i32;
        (self.column_top(x, z) + 1) as f32 * CELL_SIZE
    }

    /// Generated terrain, roofs and canopies without saved player edits.
    /// Additive deterministic transport uses this without changing terrain.
    pub fn original_surface_height(&self, x: f32, z: f32) -> f32 {
        if !x.is_finite() || !z.is_finite() {
            return self.min_y() as f32 * CELL_SIZE;
        }
        let x = (x / CELL_SIZE).floor() as i32;
        let z = (z / CELL_SIZE).floor() as i32;
        let top = if self.geography.is_some() {
            self.geographic_column(x, z)
                .map_or(self.min_y() - 1, GeographicColumn::top)
        } else {
            column_index(x, z).map_or(MIN_Y - 1, |index| {
                self.trees
                    .keys()
                    .filter(|p| p.x == x && p.z == z)
                    .map(|p| p.y)
                    .max()
                    .unwrap_or(self.heights[index])
                    .max(self.heights[index])
            })
        };
        (top + 1) as f32 * CELL_SIZE
    }

    /// Generated ground below trees and raised construction, in meters.
    pub fn original_ground_height(&self, x: f32, z: f32) -> f32 {
        let x = (x / CELL_SIZE).floor() as i32;
        let z = (z / CELL_SIZE).floor() as i32;
        self.geographic_column(x, z)
            .map_or(self.min_y() as f32 * CELL_SIZE, |column| {
                (column.height + 1) as f32 * CELL_SIZE
            })
    }

    /// Landing construction can clear generated trees, but must avoid roofs.
    pub(crate) fn original_structure_height(&self, x: f32, z: f32) -> f32 {
        let x = (x / CELL_SIZE).floor() as i32;
        let z = (z / CELL_SIZE).floor() as i32;
        self.geographic_column(x, z)
            .map_or(self.min_y() as f32 * CELL_SIZE, |column| {
                (column
                    .height
                    .max(column.site.map_or(column.height, |s| s.top()))
                    .max(column.construction.map_or(column.height, |c| c.top))
                    + 1) as f32
                    * CELL_SIZE
            })
    }

    pub fn spawn_position(&self) -> [f32; 3] {
        let anchor = self
            .settlements()
            .and_then(|plan| plan.villages.first())
            .map_or_else(
                || self.geography().map_or([0.0; 3], Geography::spawn),
                |v| v.center,
            );
        let center_x = (anchor[0] / CELL_SIZE).floor() as i32;
        let center_z = (anchor[2] / CELL_SIZE).floor() as i32;
        // A half-meter-wide character straddles neighboring columns. Account
        // for those columns too, and leave room for its head after player edits.
        for radius in 0_i32..=32 {
            for z in -radius..=radius {
                for x in -radius..=radius {
                    if x.abs().max(z.abs()) != radius {
                        continue;
                    }
                    let mut top = self.min_y();
                    for dz in -1..=1 {
                        for dx in -1..=1 {
                            top = top.max(self.column_top(center_x + x + dx, center_z + z + dz));
                        }
                    }
                    if top < self.max_y() - 5 {
                        return [
                            ((center_x + x) as f32 + 0.5) * CELL_SIZE,
                            (top + 1) as f32 * CELL_SIZE + 0.02,
                            ((center_z + z) as f32 + 0.5) * CELL_SIZE,
                        ];
                    }
                }
            }
        }
        // Filling every nearby column to the world ceiling leaves no ordinary
        // spawn. The controller's finite bounds still protect invalid states.
        [
            (center_x as f32 + 0.5) * CELL_SIZE,
            (self.max_y() - 4) as f32 * CELL_SIZE,
            (center_z as f32 + 0.5) * CELL_SIZE,
        ]
    }

    /// Stream center in cell coordinates. The stream's water is decorative;
    /// terrain in its shallow bed remains solid and editable.
    pub fn river_center(&self, z: i32) -> f32 {
        let phase = (self.seed % 1000) as f32 * 0.006;
        32.0 + libm::sinf(z as f32 * 0.024 + phase) * 10.0 + libm::sinf(z as f32 * 0.061) * 3.0
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
            -(self.radius_cells() as f32) * CELL_SIZE,
            self.min_y() as f32 * CELL_SIZE,
            -(self.radius_cells() as f32) * CELL_SIZE,
        ];
        let maximum = [
            self.radius_cells() as f32 * CELL_SIZE,
            self.max_y() as f32 * CELL_SIZE,
            self.radius_cells() as f32 * CELL_SIZE,
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
        let max_steps = (self.radius_cells() * 4 + self.max_y() - self.min_y()) as usize + 6;
        for _ in 0..max_steps {
            if distance > exit + 0.00001 || distance > max_distance + 0.00001 {
                return None;
            }
            if self.block(position) != Block::Air {
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
        if !self.contains_block(position) {
            return Block::Air;
        }
        if self.geography.is_some() {
            let column = self.geographic_column(position.x, position.z).unwrap();
            if let Some(block) = column.site.and_then(|site| site.block(position.y)) {
                return block;
            }
            if let Some(block) = column
                .construction
                .and_then(|asset| asset.block(position.y))
            {
                return block;
            }
            if let Some((kind, low, high)) = column.deposit
                && (low..=high).contains(&position.y)
            {
                return match kind {
                    ResourceKind::Clay => Block::Clay,
                    ResourceKind::Iron => Block::IronOre,
                    _ => Block::Stone,
                };
            }
            if position.y > column.height {
                return column.vegetation(position.y);
            }
            return if position.y == column.height {
                column.surface
            } else if position.y >= column.height - 3 && column.surface != Block::Stone {
                if column.surface == Block::Sand {
                    Block::Sand
                } else {
                    Block::Dirt
                }
            } else {
                Block::Stone
            };
        }
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
        let ridges = 0.65 + 0.35 * libm::sinf(xf * 0.048 + zf * 0.035 + broad * 2.2).abs();
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

    fn geographic_column(&self, x: i32, z: i32) -> Option<GeographicColumn> {
        if !self.contains_block(BlockPos::new(x, self.min_y(), z)) {
            return None;
        }
        {
            let cached = self
                .geographic_columns
                .read()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(column) = cached.get(&(x, z)) {
                return Some(*column);
            }
        }
        let geography = self.geography.as_deref()?;
        let meters_x = (x as f32 + 0.5) * CELL_SIZE;
        let meters_z = (z as f32 + 0.5) * CELL_SIZE;
        let sample = geography.sample(meters_x, meters_z);
        let height = (sample.height / CELL_SIZE).floor() as i32;
        let surface = match sample.biome {
            Biome::Ocean | Biome::Beach | Biome::Desert => Block::Sand,
            Biome::Alpine => Block::Stone,
            Biome::Snow => Block::Snow,
            _ if sample.water.is_some_and(|water| water > sample.height) => Block::Sand,
            _ => Block::Grass,
        };
        let mut column = GeographicColumn {
            height,
            surface,
            wood: None,
            leaves: None,
            construction: None,
            deposit: None,
            site: None,
        };

        let mut cleared = false;
        if let Some(plan) = self.settlements() {
            let planned = plan.column(x, z, column.height, column.surface);
            column.height = planned.height;
            column.surface = planned.surface;
            column.construction = planned.construction;
            column.deposit = planned.deposit;
            column.site = planned.site;
            cleared = planned.clear;
        }
        // Query only the tree owned by this twelve-meter square. Every crown
        // stays inside its square, including on negative coordinates.
        let grid_x = x.div_euclid(24);
        let grid_z = z.div_euclid(24);
        let (_, tree_x, tree_z) = self.tree_anchor(grid_x, grid_z);
        let dx = x - tree_x;
        let dz = z - tree_z;
        if !cleared
            && dx * dx + dz * dz
                <= if matches!(
                    self.generation,
                    WorldGeneration::GeographyV5 | WorldGeneration::GeographyV6
                ) {
                    30
                } else {
                    24
                }
            && let Some(tree) = self.tree_at(grid_x, grid_z)
        {
            if dx == 0 && dz == 0 {
                column.wood = Some((tree.base.y.max(height + 1), tree.crown_y()));
            }
            if let Some((low, high)) = tree.leaf_bounds(dx, dz)
                && high > height
            {
                column.leaves = Some((low.max(height + 1), high));
            }
        }
        let mut cached = self
            .geographic_columns
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if cached.len() >= COLUMN_CACHE_LIMIT {
            // The cache contains only deterministic base columns, never player
            // edits. Dropping it bounds memory without affecting world state.
            cached.clear();
        }
        cached.insert((x, z), column);
        Some(column)
    }

    fn tree_anchor(&self, grid_x: i32, grid_z: i32) -> (u32, i32, i32) {
        let tree_hash = hash(grid_x, grid_z, self.seed.wrapping_add(817));
        (
            tree_hash,
            grid_x * 24 + 6 + ((tree_hash >> 4) % 12) as i32,
            grid_z * 24 + 6 + ((tree_hash >> 12) % 12) as i32,
        )
    }

    /// The same candidate is used at every level of detail. No tree collection
    /// for the whole island is allocated, and this never consumes saved edits.
    pub fn tree_at(&self, grid_x: i32, grid_z: i32) -> Option<GeneratedTree> {
        let geography = self.geography()?;
        let (tree_hash, tree_x, tree_z) = self.tree_anchor(grid_x, grid_z);
        if !self.contains_block(BlockPos::new(tree_x, self.min_y(), tree_z)) {
            return None;
        }
        let tree_mx = (tree_x as f32 + 0.5) * CELL_SIZE;
        let tree_mz = (tree_z as f32 + 0.5) * CELL_SIZE;
        if self.settlements().is_some_and(|plan| {
            plan.clears_tree(
                tree_mx,
                tree_mz,
                if matches!(
                    self.generation,
                    WorldGeneration::GeographyV5 | WorldGeneration::GeographyV6
                ) {
                    3.0
                } else {
                    2.5
                },
            )
        }) {
            return None;
        }
        let sample = geography.sample(tree_mx, tree_mz);
        let refined = matches!(
            self.generation,
            WorldGeneration::GeographyV2
                | WorldGeneration::GeographyV3
                | WorldGeneration::GeographyV4
                | WorldGeneration::GeographyV5
                | WorldGeneration::GeographyV6
        );
        let (density, kind) = match sample.biome {
            Biome::Forest => (75, TreeKind::Broadleaf),
            Biome::Rainforest => (95, TreeKind::Broadleaf),
            Biome::PineForest => (72, TreeKind::Conifer),
            Biome::Shrubland => (18, TreeKind::Scrub),
            Biome::Grassland => (if refined { 5 } else { 16 }, TreeKind::Broadleaf),
            Biome::Tundra if !refined => (5, TreeKind::Broadleaf),
            _ => return None,
        };
        if tree_hash % 100 >= density {
            return None;
        }
        let spawn = geography.spawn();
        if (tree_mx - spawn[0]).powi(2) + (tree_mz - spawn[2]).powi(2) <= 12.0 * 12.0
            || sample.water.is_some_and(|water| water >= sample.height)
        {
            return None;
        }
        let slope_limit = if kind == TreeKind::Conifer { 2.5 } else { 1.5 };
        let gentle_slope = [(2.0, 0.0), (-2.0, 0.0), (0.0, 2.0), (0.0, -2.0)]
            .into_iter()
            .all(|(ox, oz)| {
                let nearby = geography.sample(tree_mx + ox, tree_mz + oz);
                (nearby.height - sample.height).abs() < slope_limit
                    && (!refined || nearby.water.is_none())
            });
        if !gentle_slope {
            return None;
        }
        let kind = if matches!(
            self.generation,
            WorldGeneration::GeographyV5 | WorldGeneration::GeographyV6
        ) {
            match (sample.biome, (tree_hash >> 24) % 3) {
                (Biome::Forest, 0) => TreeKind::Aspen,
                (Biome::PineForest, 0) => TreeKind::Cedar,
                (Biome::Rainforest, 0 | 1) => TreeKind::Canopy,
                _ => kind,
            }
        } else {
            kind
        };
        let trunk_height = match kind {
            TreeKind::Broadleaf => 10 + ((tree_hash >> 20) % 7) as i32,
            TreeKind::Conifer => 18 + ((tree_hash >> 20) % 7) as i32,
            TreeKind::Scrub => 3 + ((tree_hash >> 20) % 3) as i32,
            TreeKind::Aspen => 16 + ((tree_hash >> 20) % 7) as i32,
            TreeKind::Cedar => 22 + ((tree_hash >> 20) % 7) as i32,
            TreeKind::Canopy => 17 + ((tree_hash >> 20) % 8) as i32,
        };
        Some(GeneratedTree {
            base: BlockPos::new(
                tree_x,
                (sample.height / CELL_SIZE).floor() as i32 + 1,
                tree_z,
            ),
            trunk_height,
            kind,
            crown_radius: match kind {
                TreeKind::Scrub | TreeKind::Aspen => 2,
                TreeKind::Cedar | TreeKind::Canopy => 5,
                _ => 4,
            },
        })
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
    let center = world.geography().map_or([0.0; 3], |geography| {
        let planned = world
            .settlements()
            .and_then(|plan| plan.villages.first())
            .map_or_else(|| geography.spawn(), |v| v.center);
        [
            ((planned[0] / CELL_SIZE).floor() + 0.5) * CELL_SIZE,
            planned[1],
            ((planned[2] / CELL_SIZE).floor() + 0.5) * CELL_SIZE,
        ]
    });
    [(6.0, 5.0), (-7.0, 3.0), (4.0, -8.0)]
        .into_iter()
        .map(|(dx, dz)| {
            let x = center[0] + dx;
            let z = center[2] + dz;
            [x, world.surface_height(x, z), z]
        })
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

    // The released generator differs at these two seed-42 anchors between
    // macOS arm64 and Linux amd64: ground is within 0.000122 m of a 0.5 m
    // boundary, so floor() selects adjacent tree-base cells. Record that
    // measured ambiguity explicitly; every other integer remains exact.
    // A general epsilon cutoff would itself introduce a new rounding boundary.
    fn golden_tree_y(tree: GeneratedTree) -> i32 {
        let upper = match (tree.base.x, tree.base.z) {
            (-11_151, -14_201) => 1_372,
            (3_061, 9_727) => 1_762,
            _ => return tree.base.y,
        };
        assert!((upper - 1..=upper).contains(&tree.base.y));
        upper
    }

    fn world_identity(generation: WorldGeneration) -> u64 {
        use crate::village_assets::{block_at, dimensions};
        let world = World::generate(42, generation);
        let mut signature = 0xcbf29ce484222325_u64;
        let mut add = |n: u32| {
            for b in n.to_le_bytes() {
                signature ^= u64::from(b);
                signature = signature.wrapping_mul(0x100000001b3);
            }
        };
        for village in &world.settlements().unwrap().villages {
            add(village.id);
            for b in &village.buildings {
                for n in [
                    b.origin.x,
                    b.origin.y,
                    b.origin.z,
                    i32::from(b.rotation),
                    b.kind as i32,
                ] {
                    add(n as u32);
                }
                let [w, h, d] = dimensions(b.kind);
                for x in 0..w {
                    for y in 0..h {
                        for z in 0..d {
                            add(block_at(b.kind, x, y, z).unwrap() as u32);
                        }
                    }
                }
            }
            for lane in &village.lanes {
                for p in &lane.points {
                    for n in p {
                        add(n.to_bits());
                    }
                }
            }
        }
        for x in -1_360..1_360 {
            for z in -1_360..1_360 {
                if let Some(t) = world.tree_at(x, z) {
                    for n in [
                        t.base.x,
                        golden_tree_y(t),
                        t.base.z,
                        t.trunk_height,
                        t.crown_radius,
                        t.kind as i32,
                    ] {
                        add(n as u32);
                    }
                }
            }
        }
        for site in &world.settlements().unwrap().roadside_landmarks {
            let b = &site.building;
            for n in [
                b.origin.x,
                b.origin.y,
                b.origin.z,
                i32::from(b.rotation),
                b.kind as i32,
            ] {
                add(n as u32);
            }
            let [w, h, d] = dimensions(b.kind);
            for x in 0..w {
                for y in 0..h {
                    for z in 0..d {
                        add(block_at(b.kind, x, y, z).unwrap() as u32);
                    }
                }
            }
            for p in site.approach.iter().flat_map(|a| &a.points) {
                for n in p {
                    add(n.to_bits());
                }
            }
        }
        signature
    }

    #[test]
    fn geography_v4_geometry_with_side_landing_clearings_is_reproducible() {
        // Portable terrain math supersedes the platform-native snapshots.
        assert_eq!(
            world_identity(WorldGeneration::GeographyV4),
            8_555_870_435_748_570_619
        );
    }

    #[test]
    fn geography_v5_geometry_with_side_landing_clearings_is_reproducible() {
        assert_eq!(
            world_identity(WorldGeneration::GeographyV5),
            5_975_590_706_442_097_444
        );
    }

    #[test]
    fn v5_tree_species_share_exact_editable_crowns_and_keep_tree_anchors() {
        for generation in [WorldGeneration::GeographyV5, WorldGeneration::GeographyV6] {
            let world = World::generate(42, generation);
            let previous = World::generate(42, WorldGeneration::GeographyV4);
            let mut found = [false; 6];
            for gx in (-1_100..1_100).step_by(7) {
                for gz in (-1_100..1_100).step_by(7) {
                    let Some(tree) = world.tree_at(gx, gz) else {
                        continue;
                    };
                    let index = tree.kind as usize;
                    if found[index] {
                        continue;
                    }
                    let old = previous
                        .tree_at(gx, gz)
                        .expect("V5 reserves more space, never moves tree anchors");
                    assert_eq!(tree.base, old.base);
                    assert!(tree.crown_radius <= 5);
                    for dx in -6..=6 {
                        for dz in -6..=6 {
                            let leaves = tree.leaf_bounds(dx, dz);
                            if let Some((low, high)) = leaves {
                                assert!(
                                    dx.abs() <= tree.crown_radius && dz.abs() <= tree.crown_radius
                                );
                                for y in low..=high {
                                    let block = world.block(BlockPos::new(
                                        tree.base.x + dx,
                                        y,
                                        tree.base.z + dz,
                                    ));
                                    assert!(
                                        block == Block::Leaves
                                            || (dx == 0
                                                && dz == 0
                                                && y <= tree.crown_y()
                                                && block == Block::Wood),
                                        "{:?} crown differs at {dx},{y},{dz}: {block:?}",
                                        tree.kind
                                    );
                                }
                            }
                        }
                    }
                    found[index] = true;
                }
            }
            assert!(
                found.into_iter().all(|yes| yes),
                "seed42 misses a tree species: {found:?}"
            );
        }
    }

    #[test]
    fn shared_tree_shapes_have_distinct_bounded_canopies() {
        for (kind, trunk_height, low, high) in [
            (TreeKind::Broadleaf, 12, -3, 3),
            (TreeKind::Conifer, 20, -8, 2),
            (TreeKind::Scrub, 4, -2, 2),
        ] {
            let tree = GeneratedTree {
                base: BlockPos::new(-19, 100, -7),
                trunk_height,
                kind,
                crown_radius: if kind == TreeKind::Scrub { 2 } else { 4 },
            };
            assert_eq!(
                tree.leaf_bounds(0, 0),
                Some((tree.crown_y() + low, tree.crown_y() + high))
            );
            for dx in -6..=6 {
                for dz in -6..=6 {
                    if let Some((bottom, top)) = tree.leaf_bounds(dx, dz) {
                        assert!(dx.abs() <= tree.crown_radius && dz.abs() <= tree.crown_radius);
                        assert!(bottom >= tree.base.y && top < tree.base.y + 30);
                    }
                }
            }
        }
    }

    #[test]
    fn shared_trees_match_editable_voxels_across_biomes_and_negative_grids() {
        let world = World::generate(42, WorldGeneration::GeographyV2);
        let mut found = [false; 3];
        for gx in (-1_100..1_100).step_by(9) {
            for gz in (-1_100..1_100).step_by(9) {
                let Some(tree) = world.tree_at(gx, gz) else {
                    continue;
                };
                let index = match tree.kind {
                    TreeKind::Broadleaf => 0,
                    TreeKind::Conifer => 1,
                    TreeKind::Scrub => 2,
                    _ => panic!("new tree kinds must not enter GeographyV2"),
                };
                if found[index] {
                    continue;
                }
                assert!(tree.base.x.div_euclid(24) == gx && tree.base.z.div_euclid(24) == gz);
                assert_eq!(world.block(tree.base), Block::Wood);
                let dx = tree.crown_radius;
                let (low, high) = tree.leaf_bounds(dx, 0).unwrap();
                for y in low..=high {
                    let pos = BlockPos::new(tree.base.x + dx, y, tree.base.z);
                    assert_eq!(world.block(pos), Block::Leaves);
                }
                let outside = BlockPos::new(tree.base.x + 5, tree.crown_y() + 2, tree.base.z);
                assert_ne!(world.block(outside), Block::Leaves);
                found[index] = true;
            }
            if found.into_iter().all(|value| value) {
                break;
            }
        }
        assert_eq!(
            found, [true; 3],
            "seed 42 must visibly contain woods, pine, and scrub"
        );
    }

    fn geographic_world() -> World {
        static WORLD: std::sync::OnceLock<World> = std::sync::OnceLock::new();
        WORLD
            .get_or_init(|| World::generate(42, WorldGeneration::GeographyV1))
            .clone()
    }

    #[test]
    fn explicit_valley_generation_keeps_original_terrain_and_bounds() {
        let original = World::new(912);
        let explicit = World::generate(912, WorldGeneration::ValleyV1);
        assert_eq!(original.heights, explicit.heights);
        assert_eq!(original.surfaces, explicit.surfaces);
        assert_eq!(original.trees, explicit.trees);
        assert_eq!(explicit.generation(), WorldGeneration::ValleyV1);
        assert!(explicit.geography().is_none());
        assert_eq!(explicit.radius_cells(), WORLD_RADIUS);
        assert_eq!(explicit.min_y(), MIN_Y);
        assert_eq!(explicit.max_y(), MAX_Y);
        assert_eq!(WorldGeneration::default(), WorldGeneration::ValleyV1);
    }

    #[test]
    fn geographic_terrain_rays_and_edits_work_kilometers_from_origin() {
        let mut world = geographic_world();
        assert!(world.heights.is_empty());
        assert!(world.trees.is_empty());
        assert_eq!(world.radius_cells() as f32 * CELL_SIZE, 16384.0);
        let x = 6000;
        let z = -7000;
        let height = world.height_at(x, z);
        let meters_x = (x as f32 + 0.5) * CELL_SIZE;
        let meters_z = (z as f32 + 0.5) * CELL_SIZE;
        let sample = world.geography().unwrap().sample(meters_x, meters_z);
        assert_eq!(height, (sample.height / CELL_SIZE).floor() as i32);
        assert_eq!(
            world.block(BlockPos::new(x, height, z)),
            world.surface_block(x, z)
        );
        assert_eq!(world.block(BlockPos::new(x, height - 10, z)), Block::Stone);
        let target = BlockPos::new(x, height + 30, z);
        world.set_block(target, Block::Brick).unwrap();
        let hit = world
            .raycast(
                [meters_x, (target.y + 3) as f32 * CELL_SIZE, meters_z],
                [0.0, -1.0, 0.0],
                4.0,
            )
            .unwrap();
        assert_eq!(hit.position, target);
        assert_eq!(hit.previous, BlockPos::new(x, target.y + 1, z));
        assert!((hit.distance - 1.0).abs() < 0.001);
        assert_eq!(
            world.surface_height(meters_x, meters_z),
            (target.y + 1) as f32 * CELL_SIZE
        );
        world.set_block(target, Block::Air).unwrap();
        assert_eq!(world.edits().len(), 0);
        let dug = BlockPos::new(x, height - 8, z);
        world.set_block(dug, Block::Air).unwrap();
        assert_eq!(world.column_mesh_floor(x, z), height - 8);
        let edits = world.edits();
        let restored =
            World::from_generation_edits(42, WorldGeneration::GeographyV1, &edits).unwrap();
        assert_eq!(restored.edits(), edits);
        assert_eq!(restored.block(dug), Block::Air);
        assert_eq!(restored.height_at(x, z), height);
        assert!(World::from_edits(42, &edits).is_err());
    }

    #[test]
    fn geographic_bounds_spawn_and_surface_removals_follow_dynamic_world() {
        let mut world = geographic_world();
        let spawn = world.spawn_position();
        assert!(spawn.iter().all(|value| value.is_finite()));
        let x = (spawn[0] / CELL_SIZE).floor() as i32;
        let z = (spawn[2] / CELL_SIZE).floor() as i32;
        let top = world.height_at(x, z);
        let original_surface = world.surface_height(spawn[0], spawn[2]);
        assert_eq!(original_surface, (top + 1) as f32 * CELL_SIZE);
        world
            .set_block(BlockPos::new(x, top, z), Block::Air)
            .unwrap();
        assert_eq!(
            world.surface_height(spawn[0], spawn[2]),
            original_surface - CELL_SIZE
        );
        let minimum = BlockPos::new(
            -world.radius_cells(),
            world.min_y(),
            world.radius_cells() - 1,
        );
        assert!(world.contains_block(minimum));
        for invalid in [
            BlockPos::new(world.radius_cells(), 0, 0),
            BlockPos::new(0, world.max_y(), 0),
            BlockPos::new(0, world.min_y() - 1, 0),
        ] {
            assert!(!world.contains_block(invalid));
            assert_eq!(world.block(invalid), Block::Air);
            assert!(world.set_block(invalid, Block::Stone).is_err());
        }
        for patch in berry_patch_positions(&world) {
            assert!((patch[0] - spawn[0]).abs() < 10.0);
            assert!((patch[2] - spawn[2]).abs() < 10.0);
        }
    }

    #[test]
    fn geographic_berry_locations_keep_their_home_when_spawn_clearance_moves() {
        let mut world = geographic_world();
        let original_spawn = world.spawn_position();
        let patches = berry_patch_positions(&world);
        let x = (original_spawn[0] / CELL_SIZE).floor() as i32;
        let z = (original_spawn[2] / CELL_SIZE).floor() as i32;
        world
            .set_block(BlockPos::new(x, world.max_y() - 1, z), Block::Stone)
            .unwrap();
        let changed_spawn = world.spawn_position();
        assert!(changed_spawn[0] != original_spawn[0] || changed_spawn[2] != original_spawn[2]);
        for (before, after) in patches.into_iter().zip(berry_patch_positions(&world)) {
            assert_eq!([before[0], before[2]], [after[0], after[2]]);
        }
    }

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
