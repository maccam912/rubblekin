//! Seeded, whole-world geography in meters. Version 1 is deliberately fixed:
//! changing its constants changes the terrain under saved voxel edits.
//!
//! A 64 m height field describes a 32.768 km region. Domain-warped mountain
//! ranges undergo stream-power erosion, downstream sediment transport, and
//! thermal weathering. A priority flood resolves enclosed basins before each
//! erosion pass and supplies the final connected drainage network and lakes.
//! Detailed columns interpolate this plan and carve the same river segments
//! used by distant terrain. This is an independent implementation.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};
// Seeded terrain must use the same math on Android, Linux, Windows and macOS.
// Platform libm differences amplify through drainage/erosion and can change
// village IDs, making a client's station point at another town on the server.
use libm::{hypotf, logf, powf};

pub const WORLD_SIZE: f32 = 32_768.0;
pub const GRID_SIDE: usize = 513;
pub const GRID_SPACING: f32 = 64.0;
pub const SEA_LEVEL: f32 = 0.0;
const HALF_WORLD: f32 = WORLD_SIZE * 0.5;
const EROSION_PASSES: usize = 48;
const RIVER_CATCHMENT: f32 = 350.0;
const NO_DOWNSTREAM: usize = usize::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Biome {
    Ocean,
    Beach,
    Grassland,
    Forest,
    Rainforest,
    Desert,
    Tundra,
    Alpine,
    Snow,
    PineForest,
    Shrubland,
}

impl Biome {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ocean => "Ocean",
            Self::Beach => "Beach",
            Self::Grassland => "Meadow",
            Self::Forest => "Broadleaf woods",
            Self::Rainforest => "Wet forest",
            Self::Desert => "Desert",
            Self::Tundra => "Tundra",
            Self::Alpine => "Alpine",
            Self::Snow => "Snow",
            Self::PineForest => "Pine forest",
            Self::Shrubland => "Dry scrub",
        }
    }

    /// Surface colors in sRGB; exposed for detailed and distant terrain alike.
    pub fn color(self) -> [f32; 4] {
        match self {
            Self::Ocean => [0.46, 0.46, 0.36, 1.0],
            Self::Beach => [0.74, 0.69, 0.48, 1.0],
            Self::Grassland => [0.42, 0.55, 0.25, 1.0],
            Self::Forest => [0.29, 0.45, 0.24, 1.0],
            Self::Rainforest => [0.25, 0.43, 0.26, 1.0],
            Self::Desert => [0.72, 0.58, 0.36, 1.0],
            Self::Tundra => [0.55, 0.57, 0.40, 1.0],
            Self::Alpine => [0.54, 0.55, 0.53, 1.0],
            Self::Snow => [0.87, 0.91, 0.92, 1.0],
            Self::PineForest => [0.31, 0.43, 0.34, 1.0],
            Self::Shrubland => [0.59, 0.57, 0.31, 1.0],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeoSample {
    /// Ground surface, including river beds and local detail.
    pub height: f32,
    /// Ocean, lake, or river surface; absent on dry ground.
    pub water: Option<f32>,
    pub moisture: f32,
    pub temperature: f32,
    pub biome: Biome,
}

#[derive(Clone)]
pub struct Geography {
    seed: u32,
    version: u32,
    heights: Vec<f32>,
    filled: Vec<f32>,
    water: Vec<f32>,
    moisture: Vec<f32>,
    temperature: Vec<f32>,
    downstream: Vec<Option<usize>>,
    flow: Vec<f32>,
    spawn: [f32; 3],
    erosion_moved: f64,
}

impl Geography {
    pub fn generate(seed: u32) -> Self {
        Self::generate_version(seed, 1)
    }

    pub fn generate_v2(seed: u32) -> Self {
        Self::generate_version(seed, 2)
    }

    fn generate_version(seed: u32, version: u32) -> Self {
        let count = GRID_SIDE * GRID_SIDE;
        let mut heights = Vec::with_capacity(count);
        let mut moisture = Vec::with_capacity(count);
        for z in 0..GRID_SIDE {
            for x in 0..GRID_SIDE {
                let [mx, mz] = grid_position(z * GRID_SIDE + x);
                heights.push(initial_height(mx, mz, seed));
                moisture.push(rainfall(mx, mz, seed));
            }
        }
        // Deep noise basins otherwise become kilometer-wide reservoirs. Cut
        // their actual spillways before the slower catchment erosion: the
        // water then drains through a valley and remaining lakes keep a level
        // surface. This is terrain displacement, not suppression of water.
        let initial_drainage = drain(&heights, &moisture);
        let mut erosion_moved = breach_spillways(&mut heights, &initial_drainage);
        for _ in 0..EROSION_PASSES {
            let drainage = drain(&heights, &moisture);
            erosion_moved += erode(&mut heights, &drainage);
        }
        let drainage = drain(&heights, &moisture);
        let mut temperature = Vec::with_capacity(count);
        let mut water = vec![f32::NAN; count];
        for i in 0..count {
            let [x, z] = grid_position(i);
            temperature.push(climate_temperature(x, z, heights[i], seed));
            // Moist western slopes and drier eastern rain shadows. This is a
            // coarse prevailing-wind approximation, not atmospheric physics.
            let west = i.saturating_sub(12).max(i / GRID_SIDE * GRID_SIDE);
            let shadow = ((heights[west] - heights[i]) / 1_100.0).clamp(0.0, 0.35);
            moisture[i] = if version >= 2 {
                refined_moisture(x, z, heights[i], heights[west], seed, drainage.flow[i])
            } else {
                (moisture[i] - shadow + (logf(drainage.flow[i].max(1.0)) * 0.035).min(0.25))
                    .clamp(0.0, 1.0)
            };
            if heights[i] < SEA_LEVEL
                || drainage.filled[i] > heights[i] + 0.6
                || drainage.flow[i] >= RIVER_CATCHMENT
            {
                water[i] = drainage.filled[i].max(SEA_LEVEL);
            }
        }
        let mut geography = Self {
            seed,
            version,
            heights,
            filled: drainage.filled,
            water,
            moisture,
            temperature,
            downstream: drainage
                .downstream
                .into_iter()
                .map(|i| (i != NO_DOWNSTREAM).then_some(i))
                .collect(),
            flow: drainage.flow,
            spawn: [0.0; 3],
            erosion_moved,
        };
        geography.spawn = geography.choose_spawn();
        geography
    }

    pub fn sample(&self, x: f32, z: f32) -> GeoSample {
        if !x.is_finite() || !z.is_finite() || x.abs().max(z.abs()) > HALF_WORLD {
            return GeoSample {
                height: -240.0,
                water: Some(SEA_LEVEL),
                moisture: 1.0,
                temperature: 0.5,
                biome: Biome::Ocean,
            };
        }
        let gx = (x + HALF_WORLD) / GRID_SPACING;
        let gz = (z + HALF_WORLD) / GRID_SPACING;
        let ix = (gx.floor() as usize).min(GRID_SIDE - 2);
        let iz = (gz.floor() as usize).min(GRID_SIDE - 2);
        let tx = gx - ix as f32;
        let tz = gz - iz as f32;
        let interp = |field: &[f32]| {
            let i = iz * GRID_SIDE + ix;
            lerp(
                lerp(field[i], field[i + 1], tx),
                lerp(field[i + GRID_SIDE], field[i + GRID_SIDE + 1], tx),
                tz,
            )
        };
        let base = interp(&self.heights);
        let filled = interp(&self.filled);
        let mut moisture = interp(&self.moisture);
        let temperature = interp(&self.temperature);
        let mut water = if base < SEA_LEVEL {
            Some(SEA_LEVEL)
        } else {
            None
        };
        if water.is_none() {
            let corners = [
                iz * GRID_SIDE + ix,
                iz * GRID_SIDE + ix + 1,
                (iz + 1) * GRID_SIDE + ix,
                (iz + 1) * GRID_SIDE + ix + 1,
            ];
            for i in corners {
                let level = self.filled[i];
                if level > self.heights[i] + 0.6
                    && base < level
                    && (self.version == 1 || filled > base + 0.6)
                {
                    // A lake corner can border a lower downstream valley.
                    // Its level belongs to the flooded basin, and must not
                    // extend above that valley's interpolated spill surface.
                    let level = if self.version >= 2 {
                        level.min(filled)
                    } else {
                        level
                    };
                    water = Some(water.map_or(level, |other| other.min(level)));
                }
            }
        }
        let detail_amount = smoothstep(0.5, 5.0, (base - filled).abs() + 0.5);
        // Bounded sub-grid texture: the globally planned landforms remain
        // visible at distance; shorelines and river beds stay continuous.
        let detail = value_noise(x / 31.0, z / 31.0, self.seed.wrapping_add(81)) * 0.85
            + value_noise(x / 11.0, z / 11.0, self.seed.wrapping_add(82)) * 0.22;
        let mut height = base + detail * if water.is_some() { detail_amount } else { 1.0 };
        let mut nearest_river = None::<(f32, f32)>;
        let mut owned_river = None::<(f32, f32, f32)>;
        // Only adjacent cells can contain a segment close enough to affect
        // this sample. River widths remain below half the grid spacing.
        for cz in iz.saturating_sub(1)..=(iz + 2).min(GRID_SIDE - 1) {
            for cx in ix.saturating_sub(1)..=(ix + 2).min(GRID_SIDE - 1) {
                let i = cz * GRID_SIDE + cx;
                if self.flow[i] < RIVER_CATCHMENT || self.heights[i] < -1.0 {
                    continue;
                }
                let Some(next) = self.downstream[i] else {
                    continue;
                };
                let a = grid_position(i);
                let b = grid_position(next);
                let vx = b[0] - a[0];
                let vz = b[1] - a[1];
                let t = (((x - a[0]) * vx + (z - a[1]) * vz) / (vx * vx + vz * vz)).clamp(0.0, 1.0);
                let distance = ((x - a[0] - vx * t).powi(2) + (z - a[1] - vz * t).powi(2)).sqrt();
                let width = (self.flow[i].sqrt() * 0.32).clamp(5.0, 29.0);
                if distance > width * 2.5 {
                    continue;
                }
                let surface = lerp(self.filled[i], self.filled[next], t).max(SEA_LEVEL);
                let surface = if self.version >= 2 {
                    surface.min(filled.max(SEA_LEVEL))
                } else {
                    surface
                };
                let depth = (width * 0.10).clamp(0.75, 2.6);
                let bed = surface - depth;
                if self.version >= 2 {
                    let relative_distance = distance / width;
                    let blend = 1.0 - smoothstep(0.65, 1.0, relative_distance);
                    let carved = height.min(lerp(height, bed, blend));
                    if blend > 0.0 && owned_river.is_none_or(|(lowest, _, _)| carved < lowest) {
                        owned_river = Some((carved, relative_distance, surface));
                    }
                } else {
                    let blend = 1.0 - smoothstep(width * 0.65, width * 2.5, distance);
                    height = height.min(lerp(height, bed, blend));
                    moisture = moisture.max(0.72 * blend);
                    if distance < width
                        && nearest_river.is_none_or(|(nearest, _)| distance < nearest)
                    {
                        nearest_river = Some((distance, surface));
                    }
                }
            }
        }
        if let Some((carved, distance, surface)) = owned_river {
            // Terrain and water share one channel owner. Carving every nearby
            // tributary while retaining a higher channel's water cuts away its
            // banks, leaving an opaque elevated slab. The bed reaches the
            // untouched bank at the same width where the water ends.
            let blend = 1.0 - smoothstep(0.65, 1.0, distance);
            height = carved;
            moisture = moisture.max(0.72 * blend);
            if distance < 1.0 && water.is_none() {
                // Lakes and ocean already own their flooded footprint. A
                // neighboring outlet cannot replace that continuous surface
                // with a lower river endpoint and open a dry hole in a basin.
                water = Some(surface);
            }
        }
        if let Some((_, surface)) = nearest_river {
            // Neighboring tributaries can have different elevations. Taking
            // their maximum would lift the receiving river uphill around a
            // junction; the nearest channel owns its local water surface.
            water = Some(surface);
        }
        if let Some(level) = water
            && height >= level
        {
            water = None;
        }
        let biome = if self.version >= 2 {
            refined_biome(height, moisture, temperature, water)
        } else {
            biome(height, moisture, temperature, water)
        };
        GeoSample {
            height,
            water,
            moisture,
            temperature,
            biome,
        }
    }

    pub fn spawn(&self) -> [f32; 3] {
        self.spawn
    }

    pub fn heights(&self) -> &[f32] {
        &self.heights
    }

    /// NaN denotes a dry grid vertex. River surfaces are also available at
    /// arbitrary positions through `sample`; grid vertices alone are too
    /// sparse to represent their widths.
    pub fn water_heights(&self) -> &[f32] {
        &self.water
    }

    pub fn drainage(&self) -> &[Option<usize>] {
        &self.downstream
    }

    /// Catchment in rainfall-weighted 64 m cells (one cell is 4,096 m²).
    pub fn flow_accumulation(&self) -> &[f32] {
        &self.flow
    }

    pub fn grid_position(&self, index: usize) -> [f32; 2] {
        grid_position(index)
    }

    /// Summed material displaced over all passes, in cubic meters. Eroded
    /// material can move repeatedly; this is not net elevation loss.
    pub fn eroded_volume(&self) -> f64 {
        self.erosion_moved * f64::from(GRID_SPACING * GRID_SPACING)
    }

    fn choose_spawn(&self) -> [f32; 3] {
        let mut best = (f32::INFINITY, GRID_SIDE * GRID_SIDE / 2);
        for z in 24..GRID_SIDE - 24 {
            for x in 24..GRID_SIDE - 24 {
                let i = z * GRID_SIDE + x;
                let h = self.heights[i];
                if !(70.0..500.0).contains(&h)
                    || self.water[i].is_finite()
                    || self.temperature[i] < 0.25
                {
                    continue;
                }
                let mut steepness = 0.0_f32;
                let mut nearby_river = 0.0_f32;
                for (neighbor, _) in neighbors(i) {
                    steepness = steepness.max((self.heights[neighbor] - h).abs());
                    nearby_river = nearby_river.max(self.flow[neighbor]);
                }
                let [mx, mz] = grid_position(i);
                let score = steepness * 7.0
                    + (h - 170.0).abs() * 0.06
                    + (hypotf(mx, mz) / HALF_WORLD) * 12.0
                    - nearby_river.min(1_000.0).sqrt() * 0.12;
                if score < best.0 && self.safe_spawn(mx, mz) {
                    best = (score, i);
                }
            }
        }
        let [x, z] = grid_position(best.1);
        let sample = self.sample(x, z);
        [
            x,
            sample.height.max(sample.water.unwrap_or(sample.height)) + 0.1,
            z,
        ]
    }

    fn safe_spawn(&self, x: f32, z: f32) -> bool {
        let center = self.sample(x, z);
        if center.water.is_some() {
            return false;
        }
        for dx in [-2.0, 0.0, 2.0] {
            for dz in [-2.0, 0.0, 2.0] {
                let nearby = self.sample(x + dx, z + dz);
                if nearby.water.is_some() || (nearby.height - center.height).abs() >= 1.0 {
                    return false;
                }
            }
        }
        true
    }
}

fn biome(height: f32, moisture: f32, temperature: f32, water: Option<f32>) -> Biome {
    if height < SEA_LEVEL {
        Biome::Ocean
    } else if height < 4.0 || water.is_some() {
        Biome::Beach
    } else if temperature < 0.10 || height > 1_850.0 {
        Biome::Snow
    } else if height > 1_350.0 {
        Biome::Alpine
    } else if temperature < 0.28 {
        Biome::Tundra
    } else if moisture < 0.27 && temperature > 0.5 {
        Biome::Desert
    } else if moisture > 0.70 && temperature > 0.66 {
        Biome::Rainforest
    } else if moisture > 0.47 {
        Biome::Forest
    } else {
        Biome::Grassland
    }
}

/// Version 2 uses broader dry/wet climate regions without changing the rain
/// used by erosion. The island's established landforms and drainage stay put.
fn refined_moisture(x: f32, z: f32, height: f32, west_height: f32, seed: u32, flow: f32) -> f32 {
    let shadow = ((west_height - height) / 1_100.0).clamp(0.0, 0.35);
    (0.46 - x / WORLD_SIZE * 0.52
        + value_noise(x / 4_800.0, z / 4_800.0, seed.wrapping_add(41)) * 0.38
        + value_noise(x / 1_600.0, z / 1_600.0, seed.wrapping_add(42)) * 0.08
        - shadow
        + (logf(flow.max(1.0)) * 0.012).min(0.12))
    .clamp(0.08, 0.95)
}

fn refined_biome(height: f32, moisture: f32, temperature: f32, water: Option<f32>) -> Biome {
    if height < SEA_LEVEL {
        Biome::Ocean
    } else if height < 4.0 || water.is_some() {
        Biome::Beach
    } else if temperature < 0.10 || height > 1_850.0 {
        Biome::Snow
    } else if height > 1_350.0 {
        Biome::Alpine
    } else if temperature < 0.28 {
        Biome::Tundra
    } else if moisture < 0.28 && temperature > 0.50 {
        Biome::Desert
    } else if moisture < 0.40 {
        Biome::Shrubland
    } else if moisture > 0.74 && temperature > 0.66 {
        Biome::Rainforest
    } else if moisture > 0.45 && temperature < 0.46 {
        Biome::PineForest
    } else if moisture > 0.54 {
        Biome::Forest
    } else {
        Biome::Grassland
    }
}

fn initial_height(x: f32, z: f32, seed: u32) -> f32 {
    let warp_x = value_noise(x / 6_100.0, z / 6_100.0, seed.wrapping_add(1)) * 1_300.0;
    let warp_z = value_noise(x / 6_100.0, z / 6_100.0, seed.wrapping_add(2)) * 1_300.0;
    let wx = x + warp_x;
    let wz = z + warp_z;
    // Keep the finite-world boundary policy in one place. The current region
    // uses an irregular island, surrounded by a strip of ocean at every edge.
    let radius = hypotf(x / 14_600.0, z / 13_800.0);
    let coast = 0.94 - radius
        + value_noise(wx / 4_300.0, wz / 4_300.0, seed.wrapping_add(3)) * 0.14
        + value_noise(wx / 1_500.0, wz / 1_500.0, seed.wrapping_add(4)) * 0.04;
    if coast < 0.0 {
        return (coast * 1_100.0).max(-240.0);
    }
    let inland = smoothstep(0.0, 0.23, coast);
    let range = smoothstep(
        -0.55,
        0.62,
        value_noise(wx / 7_500.0, wz / 7_500.0, seed.wrapping_add(10)),
    );
    let ridge = 1.0
        - (value_noise(wx / 2_600.0, wz / 2_600.0, seed.wrapping_add(11))
            + value_noise(wx / 920.0, wz / 920.0, seed.wrapping_add(12)) * 0.30
            + value_noise(wx / 380.0, wz / 380.0, seed.wrapping_add(16)) * 0.12
            + value_noise(wx / 170.0, wz / 170.0, seed.wrapping_add(17)) * 0.045)
            .abs()
            .min(1.0);
    let foothills = value_noise(wx / 1_400.0, wz / 1_400.0, seed.wrapping_add(13)) * 0.5 + 0.5;
    let roughness = value_noise(wx / 340.0, wz / 340.0, seed.wrapping_add(14)) * 76.0
        + value_noise(wx / 140.0, wz / 140.0, seed.wrapping_add(15)) * 21.0;
    let coastal_hills = smoothstep(0.0, 0.12, coast)
        * (value_noise(wx / 1_100.0, wz / 1_100.0, seed.wrapping_add(18)) * 62.0
            + value_noise(wx / 460.0, wz / 460.0, seed.wrapping_add(19)) * 30.0
            + value_noise(wx / 190.0, wz / 190.0, seed.wrapping_add(20)) * 12.0);
    coast * 170.0
        + coastal_hills
        + inland
            * (65.0
                + foothills * foothills * 150.0
                + 2_240.0 * range * range * powf(ridge, 2.1)
                + roughness * (0.20 + range * 0.8))
}

fn rainfall(x: f32, z: f32, seed: u32) -> f32 {
    (0.51 - x / WORLD_SIZE * 0.32
        + value_noise(x / 5_800.0, z / 5_800.0, seed.wrapping_add(41)) * 0.28
        + value_noise(x / 1_600.0, z / 1_600.0, seed.wrapping_add(42)) * 0.08)
        .clamp(0.12, 0.95)
}

fn climate_temperature(x: f32, z: f32, height: f32, seed: u32) -> f32 {
    (0.64 + z / WORLD_SIZE * 0.42 - height.max(0.0) / 2_700.0
        + value_noise(x / 4_800.0, z / 4_800.0, seed.wrapping_add(51)) * 0.10)
        .clamp(0.0, 1.0)
}

struct Drainage {
    filled: Vec<f32>,
    downstream: Vec<usize>,
    flow: Vec<f32>,
    order: Vec<usize>,
}

#[derive(Clone, Copy)]
struct FloodCell {
    height: f32,
    index: usize,
}

impl PartialEq for FloodCell {
    fn eq(&self, other: &Self) -> bool {
        self.height == other.height && self.index == other.index
    }
}
impl Eq for FloodCell {}
impl PartialOrd for FloodCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for FloodCell {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering makes BinaryHeap a deterministic minimum heap.
        other
            .height
            .total_cmp(&self.height)
            .then_with(|| other.index.cmp(&self.index))
    }
}

fn drain(heights: &[f32], rainfall: &[f32]) -> Drainage {
    let count = heights.len();
    let mut filled = heights.to_vec();
    let mut downstream = vec![NO_DOWNSTREAM; count];
    let mut seen = vec![false; count];
    let mut heap = BinaryHeap::new();
    for z in 0..GRID_SIDE {
        for x in 0..GRID_SIDE {
            if x == 0 || z == 0 || x == GRID_SIDE - 1 || z == GRID_SIDE - 1 {
                let i = z * GRID_SIDE + x;
                filled[i] = heights[i].max(SEA_LEVEL);
                seen[i] = true;
                heap.push(FloodCell {
                    height: filled[i],
                    index: i,
                });
            }
        }
    }
    let mut order = Vec::with_capacity(count);
    let mut rank = vec![0; count];
    while let Some(cell) = heap.pop() {
        rank[cell.index] = order.len();
        order.push(cell.index);
        for (next, _) in neighbors(cell.index) {
            if seen[next] {
                continue;
            }
            seen[next] = true;
            filled[next] = heights[next].max(cell.height);
            downstream[next] = cell.index;
            heap.push(FloodCell {
                height: filled[next],
                index: next,
            });
        }
    }
    // Steepest descent on slopes, priority-flood parent across flat lakes.
    // Both choices strictly decrease (filled elevation, visit rank), so no
    // directed cycles can occur, even across perfectly level ocean cells.
    for i in 0..count {
        let mut steepest = 0.0;
        for (next, distance) in neighbors(i) {
            if rank[next] >= rank[i] {
                continue;
            }
            let slope = (filled[i] - filled[next]) / distance;
            if slope > steepest {
                steepest = slope;
                downstream[i] = next;
            }
        }
    }
    let mut flow: Vec<f32> = rainfall.iter().map(|rain| 0.45 + rain * 1.1).collect();
    for &i in order.iter().rev() {
        if downstream[i] != NO_DOWNSTREAM {
            flow[downstream[i]] += flow[i];
        }
    }
    Drainage {
        filled,
        downstream,
        flow,
        order,
    }
}

fn erode(heights: &mut [f32], drainage: &Drainage) -> f64 {
    let mut delta = vec![0.0; heights.len()];
    let mut sediment = vec![0.0; heights.len()];
    let mut moved = 0.0;
    for &i in drainage.order.iter().rev() {
        let next = drainage.downstream[i];
        if next == NO_DOWNSTREAM || heights[i] < SEA_LEVEL {
            continue;
        }
        let [x, z] = grid_position(i);
        let [nx, nz] = grid_position(next);
        let slope = ((heights[i] - heights[next]) / hypotf(x - nx, z - nz)).max(0.0);
        let lake = drainage.filled[i] > heights[i] + 0.6;
        let incision = if lake {
            0.0
        } else {
            (1.2 * powf(drainage.flow[i], 0.43) * slope.sqrt())
                .min(14.0)
                .min((heights[i] - heights[next]).max(0.0) * 0.22)
        };
        delta[i] -= incision;
        moved += f64::from(incision);
        let load = sediment[i] + incision;
        // Transport most material through steep reaches; deposit some in
        // valleys/lakes. Any remaining load leaving land is exported to sea.
        let fraction = if lake {
            0.35
        } else {
            0.015 / (1.0 + slope * 20.0)
        };
        let deposit = (load * fraction).min(1.3);
        delta[i] += deposit;
        sediment[next] += load - deposit;
        let mut steepest = (i, 0.0);
        for (neighbor, distance) in neighbors(i) {
            let excess = heights[i] - heights[neighbor] - distance * 0.72;
            if excess > steepest.1 {
                steepest = (neighbor, excess);
            }
        }
        if steepest.0 != i {
            let crumble = (steepest.1 * 0.065).min(5.0);
            delta[i] -= crumble;
            delta[steepest.0] += crumble;
            moved += f64::from(crumble);
        }
    }
    for (height, change) in heights.iter_mut().zip(delta) {
        *height += change;
    }
    moved
}

fn breach_spillways(heights: &mut [f32], drainage: &Drainage) -> f64 {
    // Equal fill elevations identify each basin's shared outlet. BTreeMap
    // keeps traversal deterministic; ties prefer the lower grid index.
    let mut basins = BTreeMap::<u32, usize>::new();
    for i in 0..heights.len() {
        if heights[i] > SEA_LEVEL && drainage.filled[i] - heights[i] > 65.0 {
            basins
                .entry(drainage.filled[i].to_bits())
                .and_modify(|bottom| {
                    if heights[i] < heights[*bottom] {
                        *bottom = i;
                    }
                })
                .or_insert(i);
        }
    }
    let mut moved = 0.0;
    for &bottom in basins.values().rev() {
        let original_level = drainage.filled[bottom];
        // Leave a natural lake above its floor. Limit deep outlet cuts so
        // high mountain cirques can still retain larger alpine lakes.
        let mut level = (heights[bottom] + 28.0).max(original_level - 320.0);
        let mut i = bottom;
        while drainage.filled[i] > level {
            if heights[i] > level {
                let depth = heights[i] - level;
                let radius = (depth / (GRID_SPACING * 0.85)).ceil().min(8.0) as isize;
                let cx = (i % GRID_SIDE) as isize;
                let cz = (i / GRID_SIDE) as isize;
                for dz in -radius..=radius {
                    for dx in -radius..=radius {
                        let x = cx + dx;
                        let z = cz + dz;
                        if x < 1
                            || z < 1
                            || x >= GRID_SIDE as isize - 1
                            || z >= GRID_SIDE as isize - 1
                        {
                            continue;
                        }
                        let index = z as usize * GRID_SIDE + x as usize;
                        let valley = level + hypotf(dx as f32, dz as f32) * GRID_SPACING * 0.85;
                        if heights[index] > valley {
                            moved += f64::from(heights[index] - valley);
                            heights[index] = valley;
                        }
                    }
                }
            }
            let next = drainage.downstream[i];
            if next == NO_DOWNSTREAM {
                break;
            }
            i = next;
            level = (level - 0.035).max(SEA_LEVEL);
        }
    }
    moved
}

fn neighbors(index: usize) -> impl Iterator<Item = (usize, f32)> {
    let x = (index % GRID_SIDE) as isize;
    let z = (index / GRID_SIDE) as isize;
    [
        (-1, -1),
        (0, -1),
        (1, -1),
        (-1, 0),
        (1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ]
    .into_iter()
    .filter_map(move |(dx, dz)| {
        let nx = x + dx;
        let nz = z + dz;
        if nx < 0 || nz < 0 || nx >= GRID_SIDE as isize || nz >= GRID_SIDE as isize {
            None
        } else {
            Some((
                nz as usize * GRID_SIDE + nx as usize,
                GRID_SPACING
                    * if dx != 0 && dz != 0 {
                        std::f32::consts::SQRT_2
                    } else {
                        1.0
                    },
            ))
        }
    })
}

fn grid_position(index: usize) -> [f32; 2] {
    [
        (index % GRID_SIDE) as f32 * GRID_SPACING - HALF_WORLD,
        (index / GRID_SIDE) as f32 * GRID_SPACING - HALF_WORLD,
    ]
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
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
    lerp(
        lerp(sample(0, 0), sample(1, 0), tx),
        lerp(sample(0, 1), sample(1, 1), tx),
        tz,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    fn geography() -> &'static Geography {
        static WORLD: OnceLock<Geography> = OnceLock::new();
        WORLD.get_or_init(|| Geography::generate(42))
    }

    fn revised_geography() -> &'static Geography {
        static WORLD: OnceLock<Geography> = OnceLock::new();
        WORLD.get_or_init(|| Geography::generate_v2(42))
    }

    fn planned_sample(field: &[f32], x: f32, z: f32) -> f32 {
        let gx = (x + HALF_WORLD) / GRID_SPACING;
        let gz = (z + HALF_WORLD) / GRID_SPACING;
        let ix = (gx.floor() as usize).min(GRID_SIDE - 2);
        let iz = (gz.floor() as usize).min(GRID_SIDE - 2);
        let i = iz * GRID_SIDE + ix;
        lerp(
            lerp(field[i], field[i + 1], gx - ix as f32),
            lerp(
                field[i + GRID_SIDE],
                field[i + GRID_SIDE + 1],
                gx - ix as f32,
            ),
            gz - iz as f32,
        )
    }

    #[test]
    fn revised_river_banks_remove_elevated_water_without_moving_saved_v1_landforms() {
        let original = geography();
        let revised = revised_geography();
        assert_eq!(original.heights, revised.heights);
        assert_eq!(original.filled, revised.filled);
        assert_eq!(original.downstream, revised.downstream);
        let old = original.sample(5480.0, 9088.0);
        let old_bank = original.sample(5480.0, 9088.5);
        assert!((old.height - 679.4122).abs() < 0.002);
        assert!((old.water.unwrap() - 701.0676).abs() < 0.002);
        assert!(old_bank.water.is_none());
        assert!(old.water.unwrap() - old_bank.height > 22.0);
        // The high incoming channel no longer fills the lower channel's
        // broad carved halo. Both former slab/bank samples are dry.
        assert!(revised.sample(5480.0, 9088.0).water.is_none());
        assert!(revised.sample(5480.0, 9088.5).water.is_none());
        let channel = revised.sample(5504.0, 9088.0);
        assert!(channel.water.is_some_and(|level| level > channel.height));
        assert!(channel.water.unwrap() - channel.height <= 2.7);
        // A retained basin still has a level lake; this fix changes channel
        // sampling rather than arbitrarily suppressing genuinely deep lakes.
        let lake = revised.sample(4656.0, 8208.0);
        assert!((lake.water.unwrap() - 987.2089).abs() < 0.002);
    }

    #[test]
    fn revised_channel_surfaces_follow_spill_heights_and_shallow_cross_sections() {
        let world = revised_geography();
        let mut checked = 0;
        for i in 0..world.heights.len() {
            if world.flow[i] < RIVER_CATCHMENT || world.heights[i] < 0.0 {
                continue;
            }
            let Some(next) = world.downstream[i] else {
                continue;
            };
            let a = grid_position(i);
            let b = grid_position(next);
            let length = hypotf(b[0] - a[0], b[1] - a[1]);
            let perpendicular = [-(b[1] - a[1]) / length, (b[0] - a[0]) / length];
            let width = (world.flow[i].sqrt() * 0.32).clamp(5.0, 29.0);
            for t in [0.0, 0.5, 1.0] {
                for offset in [-1.05, -1.0, -0.95, 0.0, 0.95, 1.0, 1.05] {
                    let x = lerp(a[0], b[0], t) + perpendicular[0] * width * offset;
                    let z = lerp(a[1], b[1], t) + perpendicular[1] * width * offset;
                    let sample = world.sample(x, z);
                    let Some(level) = sample.water else { continue };
                    let base = planned_sample(&world.heights, x, z);
                    let filled = planned_sample(&world.filled, x, z);
                    assert!(
                        level <= filled.max(SEA_LEVEL) + 0.002,
                        "raised water at {x},{z}"
                    );
                    if filled - base <= 0.6 {
                        // At most 2.6 m of channel depth plus bounded local
                        // detail; another tributary cannot remove this bank.
                        assert!(level - sample.height <= 3.7, "thick channel at {x},{z}");
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 1000);
    }

    #[test]
    fn revised_climate_creates_visible_regions_of_the_approved_biomes() {
        let world = Geography::generate_v2(42);
        let mut counts = BTreeMap::<&str, usize>::new();
        for z in (0..GRID_SIDE).step_by(2) {
            for x in (0..GRID_SIDE).step_by(2) {
                let [mx, mz] = grid_position(z * GRID_SIDE + x);
                let sample = world.sample(mx, mz);
                if sample.water.is_none() {
                    *counts.entry(sample.biome.name()).or_default() += 1;
                }
            }
        }
        for name in [
            "Meadow",
            "Broadleaf woods",
            "Pine forest",
            "Dry scrub",
            "Desert",
            "Tundra",
            "Alpine",
            "Snow",
        ] {
            assert!(
                counts.get(name).copied().unwrap_or(0) >= 32,
                "missing substantial {name} region: {counts:?}"
            );
        }
        assert_eq!(refined_biome(100.0, 0.82, 0.70, None), Biome::Rainforest);
        assert_eq!(refined_biome(100.0, 0.60, 0.40, None), Biome::PineForest);
        assert_eq!(refined_biome(100.0, 0.35, 0.60, None), Biome::Shrubland);
    }

    #[test]
    fn geographic_scale_erosion_and_safe_spawn() {
        let world = geography();
        assert_eq!(GRID_SPACING * (GRID_SIDE - 1) as f32, WORLD_SIZE);
        assert!(
            world
                .heights
                .iter()
                .all(|h| h.is_finite() && (-260.0..3_000.0).contains(h))
        );
        assert!(world.heights.iter().any(|h| *h > 1_800.0));
        assert!(world.eroded_volume() > 1_000_000.0);
        let changed = world
            .heights
            .iter()
            .enumerate()
            .filter(|&(i, h)| {
                let [x, z] = grid_position(i);
                (*h - initial_height(x, z, 42)).abs() > 1.0
            })
            .count();
        assert!(changed > 10_000, "erosion changed only {changed} cells");
        let [x, y, z] = world.spawn();
        let sample = world.sample(x, z);
        assert!(sample.water.is_none());
        assert!((70.0..501.0).contains(&y));
        for dx in [-2.0, 0.0, 2.0] {
            for dz in [-2.0, 0.0, 2.0] {
                assert!((world.sample(x + dx, z + dz).height - sample.height).abs() < 1.0);
            }
        }
    }

    #[test]
    fn drainage_reaches_ocean_without_cycles_or_uphill_water() {
        let world = geography();
        let count = world.heights.len();
        let mut state = vec![0u8; count];
        for start in 0..count {
            let mut path = Vec::new();
            let mut index = start;
            while state[index] == 0 {
                state[index] = 1;
                path.push(index);
                match world.downstream[index] {
                    Some(next) => {
                        assert!(world.filled[next] <= world.filled[index]);
                        assert!(world.flow[next] >= world.flow[index]);
                        index = next;
                    }
                    None => {
                        assert!(world.heights[index] <= SEA_LEVEL);
                        break;
                    }
                }
            }
            assert!(
                state[index] != 1 || world.downstream[index].is_none(),
                "drainage cycle"
            );
            for i in path {
                state[i] = 2;
            }
        }
        assert!(world.flow.iter().any(|f| *f > 1_000.0));
        assert!(
            world
                .heights
                .iter()
                .zip(&world.filled)
                .any(|(h, filled)| *h > 20.0 && filled - h > 2.0)
        );
    }

    #[test]
    fn seeded_landforms_and_finite_boundary_samples() {
        let world = geography();
        let other = Geography::generate(43);
        assert_ne!(world.heights, other.heights);
        let same = Geography::generate(42);
        assert_eq!(world.heights, same.heights);
        assert_eq!(world.downstream, same.downstream);
        assert_eq!(world.spawn, same.spawn);
        for (x, z) in [
            (0.0, 0.0),
            (-16_384.0, 16_384.0),
            (16_384.0, -16_384.0),
            (f32::NAN, 0.0),
            (f32::MAX, f32::MIN),
        ] {
            let sample = world.sample(x, z);
            assert!(sample.height.is_finite());
            assert!(sample.water.is_none_or(f32::is_finite));
            assert!((0.0..=1.0).contains(&sample.temperature));
            assert!((0.0..=1.0).contains(&sample.moisture));
        }
    }

    #[test]
    fn river_samples_have_connected_wet_beds() {
        let world = geography();
        let mut checked = 0;
        for i in 0..world.heights.len() {
            if world.flow[i] < RIVER_CATCHMENT || world.heights[i] < 10.0 {
                continue;
            }
            let Some(next) = world.downstream[i] else {
                continue;
            };
            let a = grid_position(i);
            let b = grid_position(next);
            let mut previous = f32::INFINITY;
            for step in 0..=16 {
                let t = step as f32 / 16.0;
                let sample = world.sample(lerp(a[0], b[0], t), lerp(a[1], b[1], t));
                assert!(sample.water.is_some_and(|level| level > sample.height));
                let level = sample.water.unwrap();
                let expected = lerp(world.filled[i], world.filled[next], t).max(SEA_LEVEL);
                assert!((level - expected).abs() < 0.001);
                assert!(level <= previous + 0.001, "sampled river climbs uphill");
                previous = level;
            }
            checked += 1;
        }
        assert!(checked > 100);
    }

    #[test]
    fn spawn_checks_widened_rivers_between_grid_vertices() {
        // This seed's best coarse dry vertex lies in a neighboring river's
        // detailed width. The final spawn must check the actual surface.
        let world = Geography::generate(912);
        let [x, _, z] = world.spawn();
        let center = world.sample(x, z);
        assert!(center.water.is_none());
        for dx in -4..=4 {
            for dz in -4..=4 {
                let sample = world.sample(x + dx as f32 * 0.5, z + dz as f32 * 0.5);
                assert!(sample.water.is_none());
                assert!((sample.height - center.height).abs() < 1.0);
            }
        }
    }
}
