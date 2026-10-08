//! Bounded geography-first village planning. This plan is part of GeographyV3,
//! regenerated from its seed; generated buildings never enter player edits.
use crate::geography::{Biome, GRID_SIDE, GeoSample, Geography};
use crate::village_assets::{self, BuildingKind};
use crate::world::{Block, BlockPos, CELL_SIZE, World};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

#[path = "settlement_exploration.rs"]
mod exploration;
#[path = "settlement_scenic.rs"]
mod scenic;

const BUCKET: f32 = 32.0;
const SITE_SPACING: f32 = 1_600.0;
const MAX_VILLAGES: usize = 10;
const CATCHMENT_STEP: f32 = 32.0;
const CATCHMENT_RADIUS: i32 = 12;
// Optional landmarks need room for their silhouette and a clear approach view.
// This remains versioned content: V3 contains none of these building kinds.
const LANDMARK_TREE_APRON: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceKind {
    Food,
    Timber,
    Stone,
    Clay,
    Iron,
}
impl ResourceKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Food => "Food",
            Self::Timber => "Timber",
            Self::Stone => "Stone",
            Self::Clay => "Clay",
            Self::Iron => "Iron",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VillageKind {
    Farming,
    Timber,
    Quarry,
    Mining,
}
impl VillageKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Farming => "Farming village",
            Self::Timber => "Timber village",
            Self::Quarry => "Quarry village",
            Self::Mining => "Mining village",
        }
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ResourceScores {
    pub farming: f32,
    pub timber: f32,
    pub stone: f32,
    pub clay: f32,
    pub iron: f32,
    pub freshwater: f32,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceDeposit {
    pub kind: ResourceKind,
    pub center: [f32; 3],
    pub radius: f32,
    pub richness: f32,
}
#[derive(Debug, Clone, PartialEq)]
pub struct BuildingPlot {
    pub kind: BuildingKind,
    pub origin: BlockPos,
    pub rotation: u8,
}
impl BuildingPlot {
    pub fn dimensions(&self) -> [i32; 3] {
        let [w, h, d] = village_assets::dimensions(self.kind);
        if self.rotation.is_multiple_of(2) {
            [w, h, d]
        } else {
            [d, h, w]
        }
    }
    pub fn local_cell(&self, x: i32, z: i32) -> Option<[i32; 2]> {
        let dx = x - self.origin.x;
        let dz = z - self.origin.z;
        let [w, _, d] = village_assets::dimensions(self.kind);
        let [rw, _, rd] = self.dimensions();
        if !(0..rw).contains(&dx) || !(0..rd).contains(&dz) {
            return None;
        }
        Some(match self.rotation % 4 {
            0 => [dx, dz],
            1 => [dz, d - 1 - dx],
            2 => [w - 1 - dx, d - 1 - dz],
            _ => [w - 1 - dz, dx],
        })
    }
    pub fn asset_at(&self, position: BlockPos) -> Option<Block> {
        let [x, z] = self.local_cell(position.x, position.z)?;
        village_assets::block_at(self.kind, x, position.y - self.origin.y, z)
    }
    pub fn entrance(&self) -> [f32; 3] {
        let [w, _, d] = village_assets::dimensions(self.kind);
        let [dx, dz] = match self.rotation % 4 {
            0 => [w / 2, -3],
            1 => [d + 2, w / 2],
            2 => [w - 1 - w / 2, d + 2],
            _ => [-3, w - 1 - w / 2],
        };
        [
            (self.origin.x + dx) as f32 * CELL_SIZE + 0.25,
            (self.origin.y + 1) as f32 * CELL_SIZE,
            (self.origin.z + dz) as f32 * CELL_SIZE + 0.25,
        ]
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct FieldPlot {
    pub origin: BlockPos,
    pub width: i32,
    pub depth: i32,
}
impl FieldPlot {
    /// Shared planting grid. A plant requires this exact soil cell to remain
    /// earth after player edits; decorative growth itself has no collision.
    pub fn plant_positions(&self) -> impl Iterator<Item = BlockPos> + '_ {
        (1..self.width - 1).step_by(3).flat_map(move |x| {
            (1..self.depth - 1)
                .step_by(2)
                .map(move |z| BlockPos::new(self.origin.x + x, self.origin.y, self.origin.z + z))
        })
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct ResidentRoute {
    pub home: [f32; 3],
    pub work: [f32; 3],
    pub path: Vec<[f32; 3]>,
    pub store_index: usize,
    pub resource: ResourceKind,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Trail {
    pub from: u32,
    pub to: u32,
    pub points: Vec<[f32; 3]>,
    pub width: f32,
    /// Generated deck/ground profile, independent of navigation feet.
    terrain_heights: Vec<f32>,
}
impl Trail {
    fn surface_point(&self, index: usize) -> [f32; 3] {
        let mut p = self.points[index];
        if let Some(height) = self.terrain_heights.get(index) {
            p[1] = *height;
        }
        p
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Village {
    pub id: u32,
    pub name: String,
    pub center: [f32; 3],
    pub kind: VillageKind,
    pub resources: ResourceScores,
    pub freshwater_distance: f32,
    pub buildings: Vec<BuildingPlot>,
    pub fields: Vec<FieldPlot>,
    pub lanes: Vec<Trail>,
    pub resident_routes: Vec<ResidentRoute>,
    pub store: [f32; 3],
    pub market: [f32; 3],
    /// Foot elevation at center and fitted x/z grades; roads share this
    /// profile so independently connected door approaches agree at junctions.
    pub ground_profile: [f32; 3],
}
impl Village {
    pub fn lane_height(&self, x: f32, z: f32) -> f32 {
        lane_height(&self.buildings, self.center, self.ground_profile, x, z)
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
enum Feature {
    Building(usize, usize),
    Field(usize, usize),
    Lane(usize, usize, usize),
    Trail(usize, usize),
    Deposit(usize),
    RoadsideBuilding(usize),
    RoadsidePath(usize, usize),
    LandingClearance(usize),
}
/// A generated walking destination, independent of village jobs and transit.
#[derive(Debug, Clone, PartialEq)]
pub struct RoadsideLandmark {
    pub building: BuildingPlot,
    pub approach: Trail,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettlementPlan {
    pub villages: Vec<Village>,
    pub trails: Vec<Trail>,
    pub resources: Vec<ResourceDeposit>,
    pub roadside_landmarks: Vec<RoadsideLandmark>,
    landing_clearances: Vec<[f32; 4]>,
    buckets: HashMap<(i32, i32), Vec<Feature>>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConstructionColumn {
    pub floor: i32,
    pub top: i32,
    kind: BuildingKind,
    local: [i32; 2],
}
impl ConstructionColumn {
    pub fn block(self, y: i32) -> Option<Block> {
        village_assets::block_at(self.kind, self.local[0], y - self.floor, self.local[1])
    }
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlannedColumn {
    pub height: i32,
    pub surface: Block,
    pub clear: bool,
    pub construction: Option<ConstructionColumn>,
    pub deposit: Option<(ResourceKind, i32, i32)>,
}
#[derive(Clone)]
struct Candidate {
    x: f32,
    z: f32,
    water: f32,
    scores: ResourceScores,
    kind: VillageKind,
    score: f32,
}

impl SettlementPlan {
    /// V4 is additive to the frozen V3 plan: retain IDs, working plots, routes
    /// and door levels, then choose regional homes and one optional landmark.
    pub(crate) fn add_regional_buildings(&mut self, world: &World) {
        let g = world.geography().expect("settlements require geography");
        for village in &mut self.villages {
            let h = hash(village.id as i32, 0, world.seed.wrapping_add(419));
            let regional = if village.center[1] > 220.0 {
                BuildingKind::UplandHouse
            } else {
                match village.kind {
                    VillageKind::Timber => BuildingKind::TimberCabin,
                    VillageKind::Quarry | VillageKind::Mining => BuildingKind::MasonryCottage,
                    VillageKind::Farming => match h % 3 {
                        0 => BuildingKind::TimberCabin,
                        1 => BuildingKind::MasonryCottage,
                        _ => BuildingKind::UplandHouse,
                    },
                }
            };
            for (index, home) in village.buildings.iter_mut().take(6).enumerate() {
                // Most homes share regional architecture; an occasional old
                // cottage keeps each street from becoming a repeated stamp.
                if index as u32 != h % 6 {
                    home.kind = regional;
                }
            }
            let kind = if village.kind == VillageKind::Farming {
                BuildingKind::Windmill
            } else {
                BuildingKind::Lookout
            };
            if let Some((plot, lane)) = landmark_site(g, village, &self.trails, kind, h) {
                village.buildings.push(plot);
                village.lanes.push(lane);
            }
        }
        self.buckets.clear();
        self.build_index();
    }

    pub fn generate(world: &World) -> Self {
        let geography = world.geography().expect("settlements require geography");
        let resources = generate_deposits(geography, world.seed);
        let mut plan = Self {
            villages: Vec::new(),
            trails: Vec::new(),
            resources,
            roadside_landmarks: Vec::new(),
            landing_clearances: Vec::new(),
            buckets: HashMap::new(),
        };
        let freshwater = freshwater_index(geography);
        let mut candidates = Vec::new();
        // 128 m sampling and a bounded shortlist, never a scan of voxel cells.
        for gz in (64..GRID_SIDE - 64).step_by(2) {
            for gx in (64..GRID_SIDE - 64).step_by(2) {
                let [x, z] = geography.grid_position(gz * GRID_SIDE + gx);
                let sample = geography.sample(x, z);
                if sample.height < 8.0 || sample.height > 1_050.0 || sample.water.is_some() {
                    continue;
                }
                let Some(water) = water_distance(x, z, &freshwater) else {
                    continue;
                };
                if !(65.0..=420.0).contains(&water) || !viable_site(geography, x, z) {
                    continue;
                }
                let farming = fertility(sample, local_slope(geography, x, z, 8.0));
                if farming < 0.16 {
                    continue;
                }
                let preliminary = farming
                    + match sample.biome {
                        Biome::Forest | Biome::Rainforest | Biome::PineForest => 0.5,
                        _ => 0.0,
                    }
                    + (1.0 - water / 500.0) * 0.25;
                candidates.push(Candidate {
                    x,
                    z,
                    water,
                    scores: ResourceScores::default(),
                    kind: VillageKind::Farming,
                    score: preliminary,
                });
            }
        }
        // Keep opportunities in different geographic districts before costly
        // reachable-catchment evaluation, so rich western woods cannot exhaust
        // the shortlist and exclude the rest of the island.
        let mut districts: HashMap<(i32, i32), Vec<Candidate>> = HashMap::new();
        for c in candidates {
            districts
                .entry((
                    (c.x / 2_000.0).floor() as i32,
                    (c.z / 2_000.0).floor() as i32,
                ))
                .or_default()
                .push(c);
        }
        let mut candidates = Vec::new();
        let mut district_keys: Vec<_> = districts.keys().copied().collect();
        district_keys.sort_unstable();
        for key in district_keys {
            let mut group = districts.remove(&key).unwrap();
            group.sort_by(|a, b| {
                b.score
                    .total_cmp(&a.score)
                    .then(a.x.total_cmp(&b.x))
                    .then(a.z.total_cmp(&b.z))
            });
            candidates.extend(group.into_iter().take(3));
        }
        for c in &mut candidates {
            let Some((scores, bank_distance)) = catchment_scores(world, &plan.resources, c.x, c.z)
            else {
                c.score = -1.0;
                continue;
            };
            c.scores = scores;
            c.water = bank_distance;
            let (kind, advantage) = specialty(c.scores);
            c.kind = kind;
            c.score = c.scores.farming * 0.8 + advantage * 0.85 + c.scores.freshwater * 0.3;
        }
        candidates.retain(|c| c.score > 0.0 && c.scores.farming > 0.06);
        let spawn = geography.spawn();
        while plan.villages.len() < MAX_VILLAGES {
            let chosen = candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    plan.villages.iter().all(|v| {
                        distance2(c.x, c.z, v.center[0], v.center[2]) >= SITE_SPACING * SITE_SPACING
                    })
                })
                .max_by(|(_, a), (_, b)| {
                    let merit = |c: &Candidate| {
                        if plan.villages.is_empty() {
                            c.score
                                / (1.0 + distance2(c.x, c.z, spawn[0], spawn[2]).sqrt() / 1_000.0)
                        } else {
                            let near = plan
                                .villages
                                .iter()
                                .map(|v| distance2(c.x, c.z, v.center[0], v.center[2]).sqrt())
                                .fold(f32::INFINITY, f32::min);
                            c.score * (near / 2_000.0).sqrt().min(2.0)
                        }
                    };
                    merit(a)
                        .total_cmp(&merit(b))
                        .then(b.x.total_cmp(&a.x))
                        .then(b.z.total_cmp(&a.z))
                })
                .map(|(i, _)| i);
            let Some(index) = chosen else {
                break;
            };
            let candidate = candidates.remove(index);
            if let Some(mut village) = layout_village(
                geography,
                world.seed,
                plan.villages.len() as u32,
                &candidate,
            ) {
                if let Some(other) = plan
                    .villages
                    .iter()
                    .find(|other| other.name == village.name)
                {
                    let qualifier = if village.center[1] >= other.center[1] {
                        "Upper"
                    } else {
                        "Lower"
                    };
                    village.name = format!("{qualifier} {}", village.name);
                }
                if plan.villages.iter().any(|other| other.name == village.name) {
                    village.name = format!(
                        "{} {}",
                        if village.center[0] < 0.0 {
                            "West"
                        } else {
                            "East"
                        },
                        village.name
                    );
                }
                plan.villages.push(village);
            }
        }
        plan.trails = connect_villages(geography, &plan.villages);
        let linked: HashSet<_> = plan
            .trails
            .iter()
            .flat_map(|t| [t.from, t.to])
            .chain([0])
            .collect();
        plan.villages.retain(|v| linked.contains(&v.id));
        for t in &mut plan.trails {
            t.terrain_heights = t.points.iter().map(|p| p[1]).collect();
        }
        plan.build_index();
        plan.normalize_navigation(geography);
        plan
    }

    fn support_height(&self, g: &Geography, p: [f32; 3]) -> f32 {
        let minx = ((p[0] - crate::physics::PLAYER_RADIUS) / CELL_SIZE).floor() as i32;
        let maxx = ((p[0] + crate::physics::PLAYER_RADIUS) / CELL_SIZE).floor() as i32;
        let minz = ((p[2] - crate::physics::PLAYER_RADIUS) / CELL_SIZE).floor() as i32;
        let maxz = ((p[2] + crate::physics::PLAYER_RADIUS) / CELL_SIZE).floor() as i32;
        let mut top = i32::MIN;
        for z in minz..=maxz {
            for x in minx..=maxx {
                let s = g.sample((x as f32 + 0.5) * CELL_SIZE, (z as f32 + 0.5) * CELL_SIZE);
                let column = self.column(x, z, (s.height / CELL_SIZE).floor() as i32, Block::Grass);
                top = top.max(column.height);
            }
        }
        (top + 1) as f32 * CELL_SIZE
    }
    fn normalize_navigation(&mut self, g: &Geography) {
        let paths: Vec<Vec<Vec<f32>>> = self
            .villages
            .iter()
            .map(|v| {
                v.resident_routes
                    .iter()
                    .map(|r| r.path.iter().map(|&p| self.support_height(g, p)).collect())
                    .collect()
            })
            .collect();
        let centers: Vec<_> = self
            .villages
            .iter()
            .map(|v| self.support_height(g, v.center))
            .collect();
        let stores: Vec<_> = self
            .villages
            .iter()
            .map(|v| self.support_height(g, v.store))
            .collect();
        let markets: Vec<_> = self
            .villages
            .iter()
            .map(|v| self.support_height(g, v.market))
            .collect();
        let trails: Vec<Vec<f32>> = self
            .trails
            .iter()
            .map(|t| {
                t.points
                    .iter()
                    .map(|&p| self.support_height(g, p))
                    .collect()
            })
            .collect();
        for (vi, v) in self.villages.iter_mut().enumerate() {
            v.center[1] = centers[vi];
            v.store[1] = stores[vi];
            v.market[1] = markets[vi];
            for (ri, r) in v.resident_routes.iter_mut().enumerate() {
                for (pi, p) in r.path.iter_mut().enumerate() {
                    p[1] = paths[vi][ri][pi];
                }
                r.home = r.path[0];
                r.work = *r.path.last().unwrap();
            }
        }
        for (ti, t) in self.trails.iter_mut().enumerate() {
            for (pi, p) in t.points.iter_mut().enumerate() {
                p[1] = trails[ti][pi];
            }
        }
    }

    fn feature_building(&self, feature: Feature) -> Option<&BuildingPlot> {
        match feature {
            Feature::Building(vi, bi) => Some(&self.villages[vi].buildings[bi]),
            Feature::RoadsideBuilding(index) => Some(&self.roadside_landmarks[index].building),
            _ => None,
        }
    }

    fn build_index(&mut self) {
        let mut bounds = Vec::new();
        for (index, [x0, z0, x1, z1]) in self.landing_clearances.iter().copied().enumerate() {
            bounds.push((
                Feature::LandingClearance(index),
                x0 - 3.0,
                z0 - 3.0,
                x1 + 3.0,
                z1 + 3.0,
            ));
        }
        for (vi, v) in self.villages.iter().enumerate() {
            for (bi, b) in v.buildings.iter().enumerate() {
                let [w, _, d] = b.dimensions();
                let tree_margin = if b.kind.is_landmark() {
                    LANDMARK_TREE_APRON + 3.0
                } else {
                    4.0
                };
                bounds.push((
                    Feature::Building(vi, bi),
                    b.origin.x as f32 * CELL_SIZE - tree_margin,
                    b.origin.z as f32 * CELL_SIZE - tree_margin,
                    (b.origin.x + w) as f32 * CELL_SIZE + tree_margin,
                    (b.origin.z + d) as f32 * CELL_SIZE + tree_margin,
                ));
            }
            for (fi, f) in v.fields.iter().enumerate() {
                bounds.push((
                    Feature::Field(vi, fi),
                    f.origin.x as f32 * CELL_SIZE - 3.0,
                    f.origin.z as f32 * CELL_SIZE - 3.0,
                    (f.origin.x + f.width) as f32 * CELL_SIZE + 3.0,
                    (f.origin.z + f.depth) as f32 * CELL_SIZE + 3.0,
                ));
            }
            for (li, l) in v.lanes.iter().enumerate() {
                segment_bounds(&mut bounds, |s| Feature::Lane(vi, li, s), l);
            }
        }
        for (ti, t) in self.trails.iter().enumerate() {
            segment_bounds(&mut bounds, |s| Feature::Trail(ti, s), t);
        }
        for (index, site) in self.roadside_landmarks.iter().enumerate() {
            let b = &site.building;
            let [w, _, d] = b.dimensions();
            let margin = LANDMARK_TREE_APRON + 3.0;
            bounds.push((
                Feature::RoadsideBuilding(index),
                b.origin.x as f32 * CELL_SIZE - margin,
                b.origin.z as f32 * CELL_SIZE - margin,
                (b.origin.x + w) as f32 * CELL_SIZE + margin,
                (b.origin.z + d) as f32 * CELL_SIZE + margin,
            ));
            segment_bounds(
                &mut bounds,
                |segment| Feature::RoadsidePath(index, segment),
                &site.approach,
            );
        }
        for (di, d) in self.resources.iter().enumerate() {
            bounds.push((
                Feature::Deposit(di),
                d.center[0] - d.radius,
                d.center[2] - d.radius,
                d.center[0] + d.radius,
                d.center[2] + d.radius,
            ));
        }
        for (feature, x0, z0, x1, z1) in bounds {
            for z in (z0 / BUCKET).floor() as i32..=(z1 / BUCKET).floor() as i32 {
                for x in (x0 / BUCKET).floor() as i32..=(x1 / BUCKET).floor() as i32 {
                    self.buckets.entry((x, z)).or_default().push(feature);
                }
            }
        }
    }

    pub fn clears_tree(&self, x: f32, z: f32, radius: f32) -> bool {
        let Some(features) = self
            .buckets
            .get(&((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32))
        else {
            return false;
        };
        features.iter().any(|f| match *f {
            Feature::Building(..) | Feature::RoadsideBuilding(_) => {
                let b = self.feature_building(*f).unwrap();
                let [w, _, d] = b.dimensions();
                let apron = if b.kind.is_landmark() {
                    LANDMARK_TREE_APRON
                } else {
                    1.5
                };
                x >= b.origin.x as f32 * CELL_SIZE - radius - apron
                    && x <= (b.origin.x + w) as f32 * CELL_SIZE + radius + apron
                    && z >= b.origin.z as f32 * CELL_SIZE - radius - apron
                    && z <= (b.origin.z + d) as f32 * CELL_SIZE + radius + apron
            }
            Feature::Field(vi, fi) => {
                let p = &self.villages[vi].fields[fi];
                x >= p.origin.x as f32 * CELL_SIZE - radius
                    && x <= (p.origin.x + p.width) as f32 * CELL_SIZE + radius
                    && z >= p.origin.z as f32 * CELL_SIZE - radius
                    && z <= (p.origin.z + p.depth) as f32 * CELL_SIZE + radius
            }
            Feature::Lane(vi, li, si) => {
                let t = &self.villages[vi].lanes[li];
                segment_distance(x, z, t.points[si], t.points[si + 1]).0 <= t.width * 0.5 + radius
            }
            Feature::Trail(ti, si) => {
                let t = &self.trails[ti];
                segment_distance(x, z, t.points[si], t.points[si + 1]).0 <= t.width * 0.5 + radius
            }
            Feature::RoadsidePath(index, si) => {
                let t = &self.roadside_landmarks[index].approach;
                segment_distance(x, z, t.points[si], t.points[si + 1]).0 <= t.width * 0.5 + radius
            }
            Feature::Deposit(_) => false,
            Feature::LandingClearance(index) => {
                let [x0, z0, x1, z1] = self.landing_clearances[index];
                x >= x0 - radius && x <= x1 + radius && z >= z0 - radius && z <= z1 + radius
            }
        })
    }

    pub(crate) fn column(&self, x: i32, z: i32, height: i32, surface: Block) -> PlannedColumn {
        let mx = (x as f32 + 0.5) * CELL_SIZE;
        let mz = (z as f32 + 0.5) * CELL_SIZE;
        let mut out = PlannedColumn {
            height,
            surface,
            clear: false,
            construction: None,
            deposit: None,
        };
        let Some(features) = self
            .buckets
            .get(&((mx / BUCKET).floor() as i32, (mz / BUCKET).floor() as i32))
        else {
            return out;
        };
        // Natural deposits first; paths, fields, and foundations take priority.
        for feature in features {
            if let Feature::Deposit(di) = *feature {
                let d = &self.resources[di];
                let r2 = distance2(mx, mz, d.center[0], d.center[2]);
                if r2 < d.radius * d.radius {
                    let material_depth = match d.kind {
                        ResourceKind::Iron => 4,
                        ResourceKind::Clay => 3,
                        _ => 2,
                    };
                    let visible = r2 < d.radius * d.radius * 0.16;
                    let top = height
                        + if visible && d.kind == ResourceKind::Stone {
                            1
                        } else if visible {
                            0
                        } else {
                            -2
                        };
                    out.deposit = Some((d.kind, top - material_depth, top));
                    if visible {
                        out.surface = match d.kind {
                            ResourceKind::Iron => Block::IronOre,
                            ResourceKind::Clay => Block::Clay,
                            _ => Block::Stone,
                        };
                        out.height = top;
                    }
                }
            }
        }
        let mut on_road = false;
        let mut total = 0.0_f64;
        let mut weights = 0.0_f64;
        let mut on_approach = false;
        let mut approach_total = 0.0_f64;
        let mut approach_weights = 0.0_f64;
        for feature in features {
            let roadside = matches!(feature, Feature::RoadsidePath(..));
            let (trail, si, village) = match *feature {
                Feature::Trail(ti, si) => (&self.trails[ti], si, None),
                Feature::RoadsidePath(index, si) => {
                    (&self.roadside_landmarks[index].approach, si, None)
                }
                Feature::Lane(vi, li, si) => {
                    (&self.villages[vi].lanes[li], si, Some(&self.villages[vi]))
                }
                _ => continue,
            };
            let a = trail.surface_point(si);
            let b = trail.surface_point(si + 1);
            let (distance, t) = segment_distance(mx, mz, a, b);
            if distance <= trail.width * 0.5 {
                if roadside {
                    on_approach = true;
                } else {
                    on_road = true;
                }
            }
            if distance < 32.0 {
                let foot = village.map_or(a[1] + (b[1] - a[1]) * t, |v| v.lane_height(mx, mz));
                let weight = (1.0 - distance / 32.0).powi(3) as f64;
                if roadside {
                    approach_total += foot as f64 * weight;
                    approach_weights += weight;
                } else {
                    total += foot as f64 * weight;
                    weights += weight;
                }
            }
        }
        // A short scenic spur may blend into the through-road, but must not
        // raise its existing waypoints and strand traders on the junction.
        if on_approach && !on_road {
            total += approach_total;
            weights += approach_weights;
            on_road = true;
        }
        if on_road {
            let mut road_height = (total / weights) as f32;
            // Global routes cannot raise or bury a town's door landings.
            // The transition outside its lots is continuous and spans64m.
            for v in &self.villages {
                let distance = distance2(mx, mz, v.center[0], v.center[2]).sqrt();
                if distance < 128.0 {
                    let blend = ((128.0 - distance) / 64.0).clamp(0.0, 1.0);
                    road_height = road_height * (1.0 - blend) + v.lane_height(mx, mz) * blend;
                }
            }
            out.height = (road_height / CELL_SIZE).round() as i32 - 1;
            out.surface = if out.height - height > 3 {
                Block::Wood
            } else {
                Block::Dirt
            };
            out.clear = true;
            out.deposit = None;
        }
        for feature in features {
            match *feature {
                Feature::Field(vi, fi) => {
                    let f = &self.villages[vi].fields[fi];
                    let dx = x - f.origin.x;
                    let dz = z - f.origin.z;
                    if (0..f.width).contains(&dx) && (0..f.depth).contains(&dz) {
                        out.height = f.origin.y;
                        out.surface = Block::Dirt;
                        out.clear = true;
                        out.deposit = None;
                    }
                }
                Feature::Building(..) | Feature::RoadsideBuilding(_) => {
                    let b = self.feature_building(*feature).unwrap();
                    let [w, h, d] = b.dimensions();
                    let entry = b.entrance();
                    let ex = (entry[0] / CELL_SIZE).floor() as i32;
                    let ez = (entry[2] / CELL_SIZE).floor() as i32;
                    if matches!(
                        b.kind,
                        BuildingKind::StoneArch
                            | BuildingKind::StandingStones
                            | BuildingKind::FallenGiant
                    ) && !on_road
                    {
                        // Feather natural discoveries into their surroundings
                        // instead of exposing a rectangular raised grass slab.
                        let dx = (b.origin.x - x).max(0).max(x - (b.origin.x + w - 1)) as f32
                            * CELL_SIZE;
                        let dz = (b.origin.z - z).max(0).max(z - (b.origin.z + d - 1)) as f32
                            * CELL_SIZE;
                        let distance = dx.hypot(dz);
                        if distance > 0.0 && distance < 8.0 {
                            let blend = 1.0 - distance / 8.0;
                            out.height = (out.height as f32 * (1.0 - blend)
                                + b.origin.y as f32 * blend)
                                .round() as i32;
                            out.deposit = None;
                        }
                    }
                    if (x >= b.origin.x
                        && x < b.origin.x + w
                        && z >= b.origin.z
                        && z < b.origin.z + d)
                        || ((x - ex).abs() <= 1 && (z - ez).abs() <= 1)
                    {
                        out.height = b.origin.y;
                        out.surface = Block::Stone;
                        out.clear = true;
                        out.deposit = None;
                        if let Some(local) = b.local_cell(x, z) {
                            let top = (0..h)
                                .rev()
                                .find(|&y| {
                                    village_assets::block_at(b.kind, local[0], y, local[1])
                                        .is_some_and(Block::is_solid)
                                })
                                .unwrap_or(0)
                                + b.origin.y;
                            out.construction = Some(ConstructionColumn {
                                floor: b.origin.y,
                                top,
                                kind: b.kind,
                                local,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }
}

fn segment_bounds(
    out: &mut Vec<(Feature, f32, f32, f32, f32)>,
    feature: impl Fn(usize) -> Feature,
    t: &Trail,
) {
    for (si, p) in t.points.windows(2).enumerate() {
        let r = t.width * 0.5 + 32.0;
        out.push((
            feature(si),
            p[0][0].min(p[1][0]) - r,
            p[0][2].min(p[1][2]) - r,
            p[0][0].max(p[1][0]) + r,
            p[0][2].max(p[1][2]) + r,
        ));
    }
}
fn distance2(x: f32, z: f32, ox: f32, oz: f32) -> f32 {
    (x - ox).powi(2) + (z - oz).powi(2)
}
fn segment_distance(x: f32, z: f32, a: [f32; 3], b: [f32; 3]) -> (f32, f32) {
    let dx = b[0] - a[0];
    let dz = b[2] - a[2];
    let n = dx * dx + dz * dz;
    let t = if n > 0.0 {
        (((x - a[0]) * dx + (z - a[2]) * dz) / n).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (distance2(x, z, a[0] + dx * t, a[2] + dz * t).sqrt(), t)
}
fn hash(x: i32, z: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9e3779b1)
        ^ (z as u32).wrapping_mul(0x85ebca77)
        ^ seed.wrapping_mul(0xc2b2ae3d);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846ca68b);
    h ^ (h >> 16)
}
fn local_slope(g: &Geography, x: f32, z: f32, step: f32) -> f32 {
    let h = g.sample(x, z).height;
    [(step, 0.0), (-step, 0.0), (0.0, step), (0.0, -step)]
        .into_iter()
        .map(|(dx, dz)| (g.sample(x + dx, z + dz).height - h).abs() / step)
        .fold(0.0, f32::max)
}
fn fertility(s: GeoSample, slope: f32) -> f32 {
    if s.water.is_some()
        || matches!(
            s.biome,
            Biome::Ocean | Biome::Beach | Biome::Alpine | Biome::Snow | Biome::Desert
        )
    {
        return 0.0;
    }
    let moisture = (1.0 - (s.moisture - 0.63).abs() * 1.6).clamp(0.0, 1.0);
    let warmth = ((s.temperature - 0.1) / 0.55).clamp(0.0, 1.0);
    moisture * warmth * (1.0 - slope / 0.5).clamp(0.0, 1.0)
}
fn viable_site(g: &Geography, x: f32, z: f32) -> bool {
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for dz in [-40.0, 0.0, 40.0] {
        for dx in [-40.0, 0.0, 40.0] {
            let s = g.sample(x + dx, z + dz);
            if s.water.is_some() || local_slope(g, x + dx, z + dz, 4.0) > 0.5 {
                return false;
            }
            low = low.min(s.height);
            high = high.max(s.height);
        }
    }
    high - low <= 12.0
}
fn freshwater_index(g: &Geography) -> HashMap<(i32, i32), Vec<[f32; 2]>> {
    let mut out: HashMap<(i32, i32), Vec<[f32; 2]>> = HashMap::new();
    for (i, &flow) in g.flow_accumulation().iter().enumerate() {
        if g.heights()[i] > 4.0
            && (flow >= 350.0
                || g.water_heights()[i].is_finite() && g.water_heights()[i] > g.heights()[i] + 0.6)
        {
            let p = g.grid_position(i);
            out.entry(((p[0] / 512.0).floor() as i32, (p[1] / 512.0).floor() as i32))
                .or_default()
                .push(p);
        }
    }
    out
}
fn water_distance(x: f32, z: f32, index: &HashMap<(i32, i32), Vec<[f32; 2]>>) -> Option<f32> {
    let bx = (x / 512.0).floor() as i32;
    let bz = (z / 512.0).floor() as i32;
    let mut distance = f32::INFINITY;
    for dz in -1..=1 {
        for dx in -1..=1 {
            if let Some(points) = index.get(&(bx + dx, bz + dz)) {
                for p in points {
                    distance = distance.min(distance2(x, z, p[0], p[1]).sqrt());
                }
            }
        }
    }
    distance.is_finite().then_some(distance)
}
fn generate_deposits(g: &Geography, seed: u32) -> Vec<ResourceDeposit> {
    let mut out = Vec::new();
    for gz in (40..GRID_SIDE - 40).step_by(4) {
        for gx in (40..GRID_SIDE - 40).step_by(4) {
            let [mut x, mut z] = g.grid_position(gz * GRID_SIDE + gx);
            let h = hash(gx as i32, gz as i32, seed.wrapping_add(197));
            x += ((h >> 4) % 96) as f32 - 48.0;
            z += ((h >> 12) % 96) as f32 - 48.0;
            let s = g.sample(x, z);
            if s.water.is_some() || s.height < 8.0 || s.height > 1_500.0 {
                continue;
            }
            let slope = local_slope(g, x, z, 12.0);
            let geology = hash(
                (x / 1_024.0).floor() as i32,
                (z / 1_024.0).floor() as i32,
                seed.wrapping_add(997),
            );
            let kind = if h % 100 < 55 && geology % 100 < 22 && s.height > 180.0 {
                ResourceKind::Iron
            } else if h % 100 < 49 && s.moisture > 0.5 && s.height < 700.0 && slope < 0.35 {
                ResourceKind::Clay
            } else if h % 100 < 80 {
                ResourceKind::Stone
            } else {
                continue;
            };
            let radius = 7.0 + ((h >> 20) % 9) as f32;
            let richness = 0.35 + ((h >> 24) % 65) as f32 / 100.0;
            out.push(ResourceDeposit {
                kind,
                center: [x, s.height, z],
                radius,
                richness,
            });
        }
    }
    out
}
fn dry_connection(g: &Geography, a: [f32; 2], b: [f32; 2]) -> bool {
    let length = distance2(a[0], a[1], b[0], b[1]).sqrt();
    let steps = (length / 4.0).ceil().max(1.0) as usize;
    let mut previous = g.sample(a[0], a[1]);
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let s = g.sample(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
        if s.water.is_some() || (s.height - previous.height).abs() > length / steps as f32 * 0.5 {
            return false;
        }
        previous = s;
    }
    true
}
fn catchment_scores(
    world: &World,
    deposits: &[ResourceDeposit],
    x: f32,
    z: f32,
) -> Option<(ResourceScores, f32)> {
    let g = world.geography().unwrap();
    let mut reached = HashMap::from([((0_i32, 0_i32), 0.0_f32)]);
    let mut queue = VecDeque::from([(0_i32, 0_i32)]);
    let mut trees = HashSet::new();
    let mut farm = 0.0;
    let mut timber = 0.0;
    let mut bank_distance = f32::INFINITY;
    while let Some((dx, dz)) = queue.pop_front() {
        let mx = x + dx as f32 * CATCHMENT_STEP;
        let mz = z + dz as f32 * CATCHMENT_STEP;
        let s = g.sample(mx, mz);
        let travel = reached[&(dx, dz)];
        farm += fertility(s, local_slope(g, mx, mz, 8.0));
        for (ox, oz) in [
            (1.0, 0.0),
            (-1.0, 0.0),
            (0.0, 1.0),
            (0.0, -1.0),
            (0.7, 0.7),
            (-0.7, 0.7),
            (0.7, -0.7),
            (-0.7, -0.7),
        ] {
            for distance in [4.0, 8.0, 12.0, 16.0] {
                let wet = g.sample(mx + ox * distance, mz + oz * distance);
                if wet
                    .water
                    .is_some_and(|water| water > 4.0 && (s.height - water).abs() < 3.0)
                {
                    bank_distance = bank_distance.min(travel + distance);
                    break;
                }
            }
        }
        let tx = (mx / CELL_SIZE).floor() as i32;
        let tz = (mz / CELL_SIZE).floor() as i32;
        for ox in -1..=1 {
            for oz in -1..=1 {
                if let Some(tree) = world.tree_at(tx.div_euclid(24) + ox, tz.div_euclid(24) + oz)
                    && !trees.contains(&(tree.base.x, tree.base.z))
                    && dry_connection(
                        g,
                        [mx, mz],
                        [
                            (tree.base.x as f32 + 0.5) * CELL_SIZE,
                            (tree.base.z as f32 + 0.5) * CELL_SIZE,
                        ],
                    )
                {
                    trees.insert((tree.base.x, tree.base.z));
                    timber += tree.trunk_height as f32 / 20.0;
                }
            }
        }
        for (ox, oz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let next = (dx + ox, dz + oz);
            if next.0.abs() > CATCHMENT_RADIUS
                || next.1.abs() > CATCHMENT_RADIUS
                || reached.contains_key(&next)
            {
                continue;
            }
            let nx = x + next.0 as f32 * CATCHMENT_STEP;
            let nz = z + next.1 as f32 * CATCHMENT_STEP;
            if dry_connection(g, [mx, mz], [nx, nz]) {
                reached.insert(next, travel + CATCHMENT_STEP);
                queue.push_back(next);
            }
        }
    }
    if !bank_distance.is_finite() || bank_distance > 600.0 {
        return None;
    }
    let area = ((CATCHMENT_RADIUS * 2 + 1).pow(2)) as f32;
    let mut scores = ResourceScores {
        farming: (farm / area).clamp(0.0, 1.0),
        timber: (timber / area / 6.0).clamp(0.0, 1.0),
        freshwater: (1.0 - bank_distance / 800.0).clamp(0.0, 1.0),
        ..Default::default()
    };
    for d in deposits {
        let dx = ((d.center[0] - x) / CATCHMENT_STEP).round() as i32;
        let dz = ((d.center[2] - z) / CATCHMENT_STEP).round() as i32;
        if let Some(&travel) = reached.get(&(dx, dz)) {
            let nx = x + dx as f32 * CATCHMENT_STEP;
            let nz = z + dz as f32 * CATCHMENT_STEP;
            if dry_connection(g, [nx, nz], [d.center[0], d.center[2]]) {
                let accessibility = (1.0 - travel / 900.0).max(0.0);
                let value = d.richness * d.radius / 24.0 * accessibility;
                match d.kind {
                    ResourceKind::Stone => scores.stone += value,
                    ResourceKind::Clay => scores.clay += value,
                    ResourceKind::Iron => scores.iron += value,
                    _ => {}
                }
            }
        }
    }
    scores.stone = scores.stone.clamp(0.0, 1.0);
    scores.clay = scores.clay.clamp(0.0, 1.0);
    scores.iron = scores.iron.clamp(0.0, 1.0);
    Some((scores, bank_distance))
}
fn specialty(s: ResourceScores) -> (VillageKind, f32) {
    [
        (VillageKind::Farming, s.farming),
        (VillageKind::Timber, s.timber * 1.3),
        (VillageKind::Quarry, s.stone * 0.85),
        (VillageKind::Mining, s.iron * 1.4),
    ]
    .into_iter()
    .max_by(|a, b| a.1.total_cmp(&b.1))
    .unwrap()
}

fn place_building(
    g: &Geography,
    kind: BuildingKind,
    x: f32,
    z: f32,
    rotation: u8,
) -> Option<BuildingPlot> {
    let [w, _, d] = village_assets::dimensions(kind);
    let [rw, rd] = if rotation.is_multiple_of(2) {
        [w, d]
    } else {
        [d, w]
    };
    let ox = (x / CELL_SIZE).round() as i32 - rw / 2;
    let oz = (z / CELL_SIZE).round() as i32 - rd / 2;
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for dx in [0, rw / 2, rw - 1] {
        for dz in [0, rd / 2, rd - 1] {
            let s = g.sample(
                (ox + dx) as f32 * CELL_SIZE + 0.25,
                (oz + dz) as f32 * CELL_SIZE + 0.25,
            );
            if s.water.is_some() {
                return None;
            }
            low = low.min(s.height);
            high = high.max(s.height);
        }
    }
    if high - low > 3.5 {
        return None;
    }
    Some(BuildingPlot {
        kind,
        origin: BlockPos::new(ox, (high / CELL_SIZE).ceil() as i32, oz),
        rotation,
    })
}
fn nav_point(g: &Geography, x: f32, z: f32) -> [f32; 3] {
    let x = (x / CELL_SIZE).floor() * CELL_SIZE + 0.25;
    let z = (z / CELL_SIZE).floor() * CELL_SIZE + 0.25;
    let sample = g.sample(x, z);
    let height = sample.water.unwrap_or(sample.height).max(sample.height);
    [x, (height / CELL_SIZE).floor() * CELL_SIZE + CELL_SIZE, z]
}
fn graded_path(g: &Geography, anchors: &[[f32; 3]], spacing: f32) -> Vec<[f32; 3]> {
    let mut points = Vec::new();
    let mut distances = Vec::new();
    let mut travelled = 0.0;
    for p in anchors.windows(2) {
        let length = distance2(p[0][0], p[0][2], p[1][0], p[1][2]).sqrt();
        let steps = (length / spacing).ceil().max(1.0) as usize;
        for i in 0..=steps {
            if !points.is_empty() && i == 0 {
                continue;
            }
            let t = i as f32 / steps as f32;
            let x = p[0][0] + (p[1][0] - p[0][0]) * t;
            let z = p[0][2] + (p[1][2] - p[0][2]) * t;
            let raw = g.sample(x, z);
            let y = raw.water.unwrap_or(raw.height).max(raw.height) + CELL_SIZE;
            points.push([x, y, z]);
            distances.push(travelled + length * t);
        }
        travelled += length;
    }
    if points.is_empty() {
        return points;
    }
    let start = anchors[0][1];
    let end = anchors.last().unwrap()[1];
    for (p, &distance) in points.iter_mut().zip(&distances) {
        let a = (distance / 5.0).clamp(0.0, 1.0);
        let b = ((travelled - distance) / 5.0).clamp(0.0, 1.0);
        p[1] = p[1] * a * b + start * (1.0 - a) + end * (1.0 - b);
    }
    points[0][1] = start;
    points.last_mut().unwrap()[1] = end;
    let grade = 0.28;
    for i in 1..points.len() {
        let run = distances[i] - distances[i - 1];
        points[i][1] = points[i][1].clamp(
            points[i - 1][1] - run * grade,
            points[i - 1][1] + run * grade,
        );
    }
    points.last_mut().unwrap()[1] = end;
    for i in (0..points.len() - 1).rev() {
        let run = distances[i + 1] - distances[i];
        points[i][1] = points[i][1].clamp(
            points[i + 1][1] - run * grade,
            points[i + 1][1] + run * grade,
        );
    }
    // A crossing must remain above its static water surface. Raise graded
    // approaches around a bridge instead of forcing the deck down into a bed.
    for p in &mut points {
        let s = g.sample(p[0], p[2]);
        if let Some(water) = s.water {
            p[1] = p[1].max(water + CELL_SIZE);
        }
    }
    for i in 1..points.len() {
        let run = distances[i] - distances[i - 1];
        points[i][1] = points[i][1].max(points[i - 1][1] - run * grade);
    }
    for i in (0..points.len() - 1).rev() {
        let run = distances[i + 1] - distances[i];
        points[i][1] = points[i][1].max(points[i + 1][1] - run * grade);
    }
    for p in &mut points {
        p[1] = (p[1] / CELL_SIZE).round() * CELL_SIZE;
    }
    points
}
fn lane_height(
    buildings: &[BuildingPlot],
    center: [f32; 3],
    profile: [f32; 3],
    x: f32,
    z: f32,
) -> f32 {
    let mut height = profile[0] + (x - center[0]) * profile[1] + (z - center[2]) * profile[2];
    for building in buildings {
        if building.kind.is_landmark() {
            continue;
        }
        let entry = building.entrance();
        height = height.max(entry[1] - distance2(x, z, entry[0], entry[2]).sqrt() * 0.25);
    }
    (height / CELL_SIZE).round() * CELL_SIZE
}

fn landmark_site(
    g: &Geography,
    village: &Village,
    trails: &[Trail],
    kind: BuildingKind,
    seed: u32,
) -> Option<(BuildingPlot, Trail)> {
    let clear = |x: f32, z: f32, margin: f32| {
        village.buildings.iter().all(|b| {
            let [w, _, d] = b.dimensions();
            x < b.origin.x as f32 * CELL_SIZE - margin
                || x > (b.origin.x + w) as f32 * CELL_SIZE + margin
                || z < b.origin.z as f32 * CELL_SIZE - margin
                || z > (b.origin.z + d) as f32 * CELL_SIZE + margin
        }) && village.fields.iter().all(|f| {
            x < f.origin.x as f32 * CELL_SIZE - margin
                || x > (f.origin.x + f.width) as f32 * CELL_SIZE + margin
                || z < f.origin.z as f32 * CELL_SIZE - margin
                || z > (f.origin.z + f.depth) as f32 * CELL_SIZE + margin
        })
    };
    for radius in [64.0, 80.0, 96.0, 112.0] {
        for direction in 0..24 {
            let angle = (direction as f32 + (seed % 24) as f32) * std::f32::consts::TAU / 24.0;
            let x = village.center[0] + angle.cos() * radius;
            let z = village.center[2] + angle.sin() * radius;
            // Existing airship approaches use offsets up to 24 m beside trails.
            // Leave another 21 m for the deck, ramps and this building's eaves.
            if !clear(x, z, 12.0)
                || trails.iter().any(|t| {
                    t.points
                        .windows(2)
                        .any(|p| segment_distance(x, z, p[0], p[1]).0 < 45.0)
                })
            {
                continue;
            }
            let rotation = if angle.cos().abs() > angle.sin().abs() {
                if angle.cos() > 0.0 { 3 } else { 1 }
            } else if angle.sin() > 0.0 {
                0
            } else {
                2
            };
            let Some(mut plot) = place_building(g, kind, x, z, rotation) else {
                continue;
            };
            let entrance = plot.entrance();
            let floor = village.lane_height(entrance[0], entrance[2]);
            let natural_floor = (plot.origin.y + 1) as f32 * CELL_SIZE;
            if !(0.0..=3.0).contains(&(floor - natural_floor)) {
                continue;
            }
            plot.origin.y = (floor / CELL_SIZE).round() as i32 - 1;
            let entry = plot.entrance();
            let Some(anchor) = village
                .lanes
                .iter()
                .flat_map(|l| &l.points)
                .filter(|p| clear(p[0], p[2], 1.5))
                .min_by(|a, b| {
                    distance2(a[0], a[2], entry[0], entry[2])
                        .total_cmp(&distance2(b[0], b[2], entry[0], entry[2]))
                })
            else {
                continue;
            };
            let points = local_path(
                g,
                &village.buildings,
                village.center,
                village.ground_profile,
                &[*anchor, entry],
            );
            if points
                .iter()
                .any(|p| !clear(p[0], p[2], 1.5) || g.sample(p[0], p[2]).water.is_some())
            {
                continue;
            }
            return Some((
                plot,
                Trail {
                    from: village.id,
                    to: village.id,
                    points,
                    width: 2.5,
                    terrain_heights: Vec::new(),
                },
            ));
        }
    }
    None
}
fn local_path(
    g: &Geography,
    buildings: &[BuildingPlot],
    center: [f32; 3],
    profile: [f32; 3],
    anchors: &[[f32; 3]],
) -> Vec<[f32; 3]> {
    let mut points = graded_path(g, anchors, 1.0);
    for p in &mut points {
        p[1] = lane_height(buildings, center, profile, p[0], p[2]);
    }
    points
}
fn layout_village(g: &Geography, seed: u32, id: u32, c: &Candidate) -> Option<Village> {
    let h = hash(c.x as i32, c.z as i32, seed.wrapping_add(731));
    let x = c.x;
    let z = c.z;
    let mut buildings = Vec::new();
    let mut center = nav_point(g, x, z);
    let sx =
        ((g.sample(x + 40.0, z).height - g.sample(x - 40.0, z).height) / 80.0).clamp(-0.12, 0.12);
    let sz =
        ((g.sample(x, z + 40.0).height - g.sample(x, z - 40.0).height) / 80.0).clamp(-0.12, 0.12);
    let ground_profile = [center[1], sx, sz];
    // Site-specific oval proportions and independent lot offsets preserve
    // useful frontages without repeating one town grid across the island.
    let stretch_x = 0.90 + (h % 21) as f32 / 100.0;
    let stretch_z = 0.90 + ((h >> 8) % 18) as f32 / 100.0;
    for (i, (dx, dz, rotation)) in [
        (-22.0, -18.0, 1),
        (0.0, -27.0, 2),
        (23.0, -17.0, 3),
        (24.0, 16.0, 3),
        (0.0, 28.0, 0),
        (-24.0, 15.0, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let lot = hash(i as i32, id as i32, h);
        let jitter_x = ((lot % 17) as f32 - 8.0) * 0.25;
        let jitter_z = (((lot >> 8) % 13) as f32 - 6.0) * 0.25;
        buildings.push(place_building(
            g,
            BuildingKind::Cottage,
            x + dx * stretch_x + jitter_x,
            z + dz * stretch_z + jitter_z,
            rotation,
        )?);
    }
    buildings.push(place_building(g, BuildingKind::Storehouse, x - 12.0, z, 1)?);
    buildings.push(place_building(g, BuildingKind::Market, x + 12.0, z, 3)?);
    buildings.push(place_building(g, BuildingKind::Workshop, x - 40.0, z, 1)?);
    // Keep each doorway above the shared street contour. The maximum of
    // gentle approach ramps is continuous even where two lanes intersect.
    for b in &mut buildings {
        let e = b.entrance();
        let plane = ground_profile[0] + (e[0] - center[0]) * sx + (e[2] - center[2]) * sz;
        b.origin.y = b.origin.y.max((plane / CELL_SIZE).ceil() as i32 - 1);
    }
    for _ in 0..3 {
        let floors: Vec<_> = buildings
            .iter()
            .map(|b| {
                let e = b.entrance();
                lane_height(&buildings, center, ground_profile, e[0], e[2])
            })
            .collect();
        for (b, floor) in buildings.iter_mut().zip(floors) {
            b.origin.y = b.origin.y.max((floor / CELL_SIZE).round() as i32 - 1);
        }
    }
    center[1] = lane_height(&buildings, center, ground_profile, center[0], center[2]);
    let store = buildings[6].entrance();
    let market = buildings[7].entrance();
    let workshop = buildings[8].entrance();
    let mut fields = Vec::new();
    for (fi, (dx, dz)) in [(34.0, -10.0), (-12.0, 38.0)].into_iter().enumerate() {
        let field_hash = hash(fi as i32, id as i32, h.wrapping_add(413));
        let dx = dx + (field_hash % 5) as f32 * 0.5;
        let dz = dz + ((field_hash >> 8) % 4) as f32 * 0.5;
        let origin_x = ((x + dx) / CELL_SIZE).floor() as i32;
        let origin_z = ((z + dz) / CELL_SIZE).floor() as i32;
        let width = 20 + ((field_hash >> 16) % 5) as i32 * 2;
        let depth = 24 + ((field_hash >> 24) % 5) as i32 * 2;
        let mut high = f32::NEG_INFINITY;
        let mut low = f32::INFINITY;
        for ox in [0, width / 2, width - 1] {
            for oz in [0, depth / 2, depth - 1] {
                let s = g.sample(
                    (origin_x + ox) as f32 * CELL_SIZE + 0.25,
                    (origin_z + oz) as f32 * CELL_SIZE + 0.25,
                );
                if s.water.is_some() {
                    return None;
                }
                high = high.max(s.height);
                low = low.min(s.height);
            }
        }
        if high - low > 4.0 {
            return None;
        }
        fields.push(FieldPlot {
            origin: BlockPos::new(origin_x, (high / CELL_SIZE).ceil() as i32, origin_z),
            width,
            depth,
        });
    }
    let field_work = |f: &FieldPlot| {
        nav_point(
            g,
            (f.origin.x - 5) as f32 * CELL_SIZE + 0.25,
            (f.origin.z + 4) as f32 * CELL_SIZE + 0.25,
        )
    };
    let food_targets = [field_work(&fields[0]), field_work(&fields[1])];
    let specialty = match c.kind {
        VillageKind::Farming => ResourceKind::Food,
        VillageKind::Timber => ResourceKind::Timber,
        VillageKind::Quarry => ResourceKind::Stone,
        VillageKind::Mining => ResourceKind::Iron,
    };
    let mut lanes = vec![
        Trail {
            from: id,
            to: id,
            points: local_path(g, &buildings, center, ground_profile, &[center, store]),
            width: 3.0,
            terrain_heights: Vec::new(),
        },
        Trail {
            from: id,
            to: id,
            points: local_path(g, &buildings, center, ground_profile, &[center, market]),
            width: 3.0,
            terrain_heights: Vec::new(),
        },
    ];
    let mut resident_routes = Vec::new();
    for (i, b) in buildings[..6].iter().enumerate() {
        let home = b.entrance();
        let food_job = i < 3 || (i < 5 && c.kind == VillageKind::Farming);
        let mut work = if food_job {
            food_targets[i % 2]
        } else {
            workshop
        };
        work[1] = lane_height(&buildings, center, ground_profile, work[0], work[2]);
        let resource = if i < 3 {
            ResourceKind::Food
        } else if i < 5 {
            specialty
        } else if c.scores.clay > 0.05 {
            ResourceKind::Clay
        } else {
            ResourceKind::Timber
        };
        let approach = if i == 1 || i == 4 {
            center
        } else {
            nav_point(
                g,
                x + if i == 0 || i == 5 { -3.0 } else { 3.0 },
                z + if i < 3 { -8.0 } else { 8.0 },
            )
        };
        let mut path = local_path(
            g,
            &buildings,
            center,
            ground_profile,
            &[home, approach, center, store],
        );
        let store_index = path.len() - 1;
        let work_anchors = if food_job && i.is_multiple_of(2) {
            vec![
                store,
                center,
                nav_point(g, x, z - 9.0),
                nav_point(g, x + 31.0, z - 9.0),
                work,
            ]
        } else if food_job {
            vec![store, center, nav_point(g, x - 10.0, z + 31.0), work]
        } else {
            vec![
                store,
                center,
                nav_point(g, x, z - 9.0),
                nav_point(g, x - 29.0, z - 9.0),
                nav_point(g, x - 29.0, z),
                work,
            ]
        };
        let mut work_path = local_path(g, &buildings, center, ground_profile, &work_anchors);
        work_path.remove(0);
        path.extend(work_path);
        lanes.push(Trail {
            from: id,
            to: id,
            points: path.clone(),
            width: 2.5,
            terrain_heights: Vec::new(),
        });
        resident_routes.push(ResidentRoute {
            home,
            work,
            path,
            store_index,
            resource,
        });
    }
    let prefixes = [
        "Alder", "Willow", "Birch", "Oak", "Reed", "Pine", "Fern", "Moss", "Stone", "Ash",
    ];
    let suffix = match c.kind {
        VillageKind::Farming => "mead",
        VillageKind::Timber => "wood",
        VillageKind::Quarry => "bank",
        VillageKind::Mining => "vale",
    };
    Some(Village {
        id,
        name: format!(
            "{}{}",
            prefixes[(h as usize + id as usize) % prefixes.len()],
            suffix
        ),
        center,
        kind: c.kind,
        resources: c.scores,
        freshwater_distance: c.water,
        buildings,
        fields,
        lanes,
        resident_routes,
        store,
        market,
        ground_profile,
    })
}

#[derive(Clone, Copy)]
struct RouteNode {
    index: usize,
    cost: f32,
    estimate: f32,
}
impl PartialEq for RouteNode {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.estimate.to_bits() == other.estimate.to_bits()
    }
}
impl Eq for RouteNode {}
impl Ord for RouteNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .estimate
            .total_cmp(&self.estimate)
            .then(other.index.cmp(&self.index))
    }
}
impl PartialOrd for RouteNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
const ROUTE_SIDE: usize = 257;
const ROUTE_STEP: f32 = 128.0;
fn route_position(index: usize) -> [f32; 2] {
    [
        (index % ROUTE_SIDE) as f32 * ROUTE_STEP - 16_384.0,
        (index / ROUTE_SIDE) as f32 * ROUTE_STEP - 16_384.0,
    ]
}
fn route_index(x: f32, z: f32) -> usize {
    let ix = ((x + 16_384.0) / ROUTE_STEP)
        .round()
        .clamp(0.0, (ROUTE_SIDE - 1) as f32) as usize;
    let iz = ((z + 16_384.0) / ROUTE_STEP)
        .round()
        .clamp(0.0, (ROUTE_SIDE - 1) as f32) as usize;
    iz * ROUTE_SIDE + ix
}
fn coarse_route(
    g: &Geography,
    samples: &[GeoSample],
    from: [f32; 3],
    to: [f32; 3],
    blocked: &HashSet<usize>,
    edges: &mut HashMap<(usize, usize), bool>,
) -> Option<Vec<[f32; 3]>> {
    let start = route_index(from[0], from[2]);
    let end = route_index(to[0], to[2]);
    let mut costs = vec![f32::INFINITY; samples.len()];
    let mut previous = vec![usize::MAX; samples.len()];
    let mut queue = BinaryHeap::new();
    costs[start] = 0.0;
    queue.push(RouteNode {
        index: start,
        cost: 0.0,
        estimate: 0.0,
    });
    let goal = route_position(end);
    let mut visits = 0;
    while let Some(node) = queue.pop() {
        if node.cost > costs[node.index] {
            continue;
        }
        if node.index == end {
            let mut chain = Vec::new();
            let mut index = end;
            while index != start {
                let [x, z] = route_position(index);
                chain.push(nav_point(g, x, z));
                index = previous[index];
            }
            chain.reverse();
            let mut out = vec![from];
            out.extend(chain);
            out.push(to);
            return Some(out);
        }
        visits += 1;
        if visits > 45_000 {
            return None;
        }
        let ix = node.index % ROUTE_SIDE;
        let iz = node.index / ROUTE_SIDE;
        let base = samples[node.index];
        for (dx, dz) in [
            (1_i32, 0_i32),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (-1, 1),
            (1, -1),
            (-1, -1),
        ] {
            let nx = ix as i32 + dx;
            let nz = iz as i32 + dz;
            if nx < 1 || nz < 1 || nx >= ROUTE_SIDE as i32 - 1 || nz >= ROUTE_SIDE as i32 - 1 {
                continue;
            }
            let next = nz as usize * ROUTE_SIDE + nx as usize;
            if blocked.contains(&next) {
                continue;
            }
            let sample = samples[next];
            let run = if dx != 0 && dz != 0 {
                ROUTE_STEP * std::f32::consts::SQRT_2
            } else {
                ROUTE_STEP
            };
            let slope = (sample.height - base.height).abs() / run;
            if sample.height < 4.0
                || slope > 0.35
                || sample
                    .water
                    .is_some_and(|water| water - sample.height > 4.0)
            {
                continue;
            }
            let key = (node.index.min(next), node.index.max(next));
            let safe = *edges.entry(key).or_insert_with(|| {
                let a = route_position(node.index);
                let b = route_position(next);
                let steps = (run / 16.0).ceil() as usize;
                let mut previous = base;
                let mut wet = 0.0;
                for i in 1..=steps {
                    let t = i as f32 / steps as f32;
                    let s = g.sample(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
                    if s.height < 4.0
                        || (s.height - previous.height).abs() > run / steps as f32 * 0.65
                        || s.water.is_some_and(|w| w - s.height > 8.0)
                    {
                        return false;
                    }
                    if s.water.is_some() {
                        wet += run / steps as f32;
                        if wet > 72.0 {
                            return false;
                        }
                    } else {
                        wet = 0.0;
                    }
                    previous = s;
                }
                true
            });
            if !safe {
                continue;
            }
            let cost = node.cost
                + run * (1.0 + slope * 8.0 + if sample.water.is_some() { 3.0 } else { 0.0 });
            if cost < costs[next] {
                costs[next] = cost;
                previous[next] = node.index;
                let [x, z] = route_position(next);
                queue.push(RouteNode {
                    index: next,
                    cost,
                    estimate: cost + distance2(x, z, goal[0], goal[1]).sqrt(),
                });
            }
        }
    }
    None
}
fn connect_villages(g: &Geography, villages: &[Village]) -> Vec<Trail> {
    if villages.len() < 2 {
        return Vec::new();
    }
    let samples: Vec<_> = (0..ROUTE_SIDE * ROUTE_SIDE)
        .map(|i| {
            let [x, z] = route_position(i);
            g.sample(x, z)
        })
        .collect();
    let blocked: HashSet<_> = villages
        .iter()
        .map(|v| route_index(v.center[0], v.center[2]))
        .collect();
    let mut edges = HashMap::new();
    let mut connected = HashSet::from([0]);
    let mut attempted = HashSet::new();
    let mut trails = Vec::new();
    while connected.len() < villages.len() {
        let edge = (0..villages.len())
            .filter(|i| connected.contains(i))
            .flat_map(|a| {
                (0..villages.len())
                    .filter(|b| !connected.contains(b))
                    .map(move |b| (a, b))
            })
            .filter(|edge| !attempted.contains(edge))
            .min_by(|&(a, b), &(c, d)| {
                let merit = |i: usize, j: usize| {
                    let a = &villages[i];
                    let b = &villages[j];
                    let complement = if a.kind != b.kind { 0.8 } else { 1.0 };
                    distance2(a.center[0], a.center[2], b.center[0], b.center[2]) * complement
                };
                merit(a, b).total_cmp(&merit(c, d))
            });
        let Some((a, b)) = edge else {
            break;
        };
        attempted.insert((a, b));
        let direction = if villages[b].center[2] >= villages[a].center[2] {
            1.0
        } else {
            -1.0
        };
        let exit = |v: &Village, dir: f32| {
            vec![
                v.center,
                nav_point(g, v.center[0] + 5.0, v.center[2] + dir * 7.0),
                nav_point(g, v.center[0] + 9.0, v.center[2] + dir * 7.0),
                nav_point(g, v.center[0] + 9.0, v.center[2] + dir * 120.0),
            ]
        };
        let source = exit(&villages[a], direction);
        let mut destination = exit(&villages[b], -direction);
        if let Some(mut anchors) = coarse_route(
            g,
            &samples,
            *source.last().unwrap(),
            *destination.last().unwrap(),
            &blocked,
            &mut edges,
        ) {
            anchors.remove(0);
            anchors.pop();
            let mut all = source;
            all.extend(anchors);
            destination.reverse();
            all.extend(destination);
            let anchors = all;
            let mut points = graded_path(g, &anchors, 2.0);
            for p in &mut points {
                for v in [&villages[a], &villages[b]] {
                    let distance = distance2(p[0], p[2], v.center[0], v.center[2]).sqrt();
                    if distance < 112.0 {
                        let blend = ((112.0 - distance) / 40.0).clamp(0.0, 1.0);
                        p[1] =
                            (p[1] * (1.0 - blend) + v.lane_height(p[0], p[2]) * blend) / CELL_SIZE;
                        p[1] = p[1].round() * CELL_SIZE;
                    }
                }
            }
            for p in &mut points {
                let sample = g.sample(p[0], p[2]);
                if let Some(water) = sample.water {
                    p[1] = p[1].max(water + CELL_SIZE);
                }
            }
            for i in 1..points.len() {
                let run = distance2(
                    points[i][0],
                    points[i][2],
                    points[i - 1][0],
                    points[i - 1][2],
                )
                .sqrt();
                points[i][1] = points[i][1].max(points[i - 1][1] - run * 0.24);
            }
            for i in (0..points.len() - 1).rev() {
                let run = distance2(
                    points[i][0],
                    points[i][2],
                    points[i + 1][0],
                    points[i + 1][2],
                )
                .sqrt();
                points[i][1] = points[i][1].max(points[i + 1][1] - run * 0.24);
            }
            for p in &mut points {
                p[1] = (p[1] / CELL_SIZE).round() * CELL_SIZE;
            }
            // Reject a coarse shortcut over a deep lake. Rivers remain narrow
            // crossings with a wood deck and graded dry approaches.
            let mut wet_run = 0.0;
            let mut safe = true;
            for pair in points.windows(2) {
                let s = g.sample(pair[1][0], pair[1][2]);
                if s.water.is_some() {
                    wet_run += distance2(pair[0][0], pair[0][2], pair[1][0], pair[1][2]).sqrt();
                    if wet_run > 72.0 || s.water.is_some_and(|water| water - s.height > 10.0) {
                        safe = false;
                        break;
                    }
                } else {
                    wet_run = 0.0;
                }
            }
            if safe {
                trails.push(Trail {
                    from: villages[a].id,
                    to: villages[b].id,
                    points,
                    width: 3.0,
                    terrain_heights: Vec::new(),
                });
                connected.insert(b);
            }
        }
    }
    trails
}

#[cfg(test)]
mod tests {
    use super::*;
    fn world() -> World {
        static WORLD: std::sync::OnceLock<World> = std::sync::OnceLock::new();
        WORLD
            .get_or_init(|| World::generate(42, crate::world::WorldGeneration::GeographyV3))
            .clone()
    }
    #[test]
    fn inhabited_island_has_real_scored_sites_and_bounded_generated_assets() {
        let world = world();
        let plan = world.settlements().unwrap();
        assert!(plan.villages.len() >= 4);
        assert!(plan.villages.len() <= MAX_VILLAGES);
        assert!(!plan.resources.is_empty());
        assert!(!plan.trails.is_empty());
        assert!(world.edits().is_empty());
        for v in &plan.villages {
            assert!(v.resources.farming > 0.05);
            assert!((4.0..=600.0).contains(&v.freshwater_distance));
            assert_eq!(v.resident_routes.len(), 6);
            assert_eq!(v.buildings.len(), 9);
            for b in &v.buildings {
                let [w, _, d] = b.dimensions();
                for x in 0..w {
                    for z in 0..d {
                        let pos = BlockPos::new(b.origin.x + x, b.origin.y, b.origin.z + z);
                        assert!(world.block(pos).is_solid());
                        assert_eq!(
                            world.block(BlockPos::new(pos.x, b.origin.y + 1, pos.z)),
                            b.asset_at(BlockPos::new(pos.x, b.origin.y + 1, pos.z))
                                .unwrap()
                        );
                    }
                }
            }
        }
        for (i, a) in plan.villages.iter().enumerate() {
            for b in &plan.villages[i + 1..] {
                assert!(
                    distance2(a.center[0], a.center[2], b.center[0], b.center[2])
                        >= SITE_SPACING * SITE_SPACING
                );
            }
        }
    }
    #[test]
    fn resident_paths_are_clear_and_physically_walkable() {
        let world = world();
        let plan = world.settlements().unwrap();
        for v in &plan.villages {
            for route in &v.resident_routes {
                assert_eq!(route.path[route.store_index], v.store);
                for &p in &route.path {
                    let x = (p[0] / CELL_SIZE).floor() as i32;
                    let z = (p[2] / CELL_SIZE).floor() as i32;
                    let y = world.height_at(x, z) + 1;
                    for dz in -1..=1 {
                        for dx in -1..=1 {
                            for dy in 1..4 {
                                assert!(
                                    !world
                                        .block(BlockPos::new(x + dx, y + dy, z + dz))
                                        .is_solid(),
                                    "{} route {:?} blocked at {:?}",
                                    v.name,
                                    route.resource,
                                    BlockPos::new(x + dx, y + dy, z + dz)
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn generated_materials_are_editable_and_restore_without_plan_in_save() {
        let mut world = world();
        for d in world
            .settlements()
            .unwrap()
            .resources
            .clone()
            .iter()
            .take(40)
        {
            let x = (d.center[0] / CELL_SIZE).floor() as i32;
            let z = (d.center[2] / CELL_SIZE).floor() as i32;
            let y = world.height_at(x, z);
            let pos = BlockPos::new(x, y, z);
            let block = world.block(pos);
            assert!(matches!(block, Block::Stone | Block::IronOre | Block::Clay));
            world.set_block(pos, Block::Air).unwrap();
            assert_eq!(world.block(pos), Block::Air);
            world.set_block(pos, block).unwrap();
        }
        assert!(world.edits().is_empty());
    }
    #[test]
    fn deterministic_settlements_keep_v2_landforms_and_legacy_generators() {
        let inhabited = world();
        let repeated = World::generate(42, crate::world::WorldGeneration::GeographyV3);
        assert_eq!(inhabited.settlements(), repeated.settlements());
        let legacy = World::generate(42, crate::world::WorldGeneration::GeographyV2);
        assert!(legacy.settlements().is_none());
        assert_eq!(
            legacy.geography().unwrap().heights(),
            inhabited.geography().unwrap().heights()
        );
        assert_eq!(
            legacy.geography().unwrap().drainage(),
            inhabited.geography().unwrap().drainage()
        );
        for x in (-12_000..12_000).step_by(911) {
            for z in (-12_000..12_000).step_by(977) {
                assert_eq!(
                    legacy.geography().unwrap().sample(x as f32, z as f32),
                    inhabited.geography().unwrap().sample(x as f32, z as f32)
                );
            }
        }
        let plot = &inhabited.settlements().unwrap().villages[0].buildings[0];
        let pos = plot.origin;
        let mut edited = inhabited.clone();
        edited.set_block(pos, Block::Air).unwrap();
        let restored = World::from_generation_edits(
            42,
            crate::world::WorldGeneration::GeographyV3,
            &edited.edits(),
        )
        .unwrap();
        assert_eq!(restored.settlements(), inhabited.settlements());
        assert_eq!(restored.block(pos), Block::Air);
        assert_eq!(restored.edits(), edited.edits());
    }
    #[test]
    fn multiple_seeds_have_usable_spaced_sites_with_true_specialties() {
        for seed in [7, 99] {
            let world = World::generate(seed, crate::world::WorldGeneration::GeographyV3);
            let plan = world.settlements().unwrap();
            assert!((4..=MAX_VILLAGES).contains(&plan.villages.len()));
            assert!(plan.resources.len() < 8_192);
            assert!(plan.trails.len() >= plan.villages.len() - 2);
            let names: HashSet<_> = plan.villages.iter().map(|v| &v.name).collect();
            assert_eq!(names.len(), plan.villages.len());
            for (i, v) in plan.villages.iter().enumerate() {
                assert_eq!(v.kind, specialty(v.resources).0);
                assert!(v.resources.farming > 0.06);
                assert!((4.0..=600.0).contains(&v.freshwater_distance));
                for b in &plan.villages[i + 1..] {
                    assert!(
                        distance2(v.center[0], v.center[2], b.center[0], b.center[2])
                            >= SITE_SPACING * SITE_SPACING
                    );
                }
                let specialty = match v.kind {
                    VillageKind::Quarry => Some(ResourceKind::Stone),
                    VillageKind::Mining => Some(ResourceKind::Iron),
                    _ => None,
                };
                if let Some(kind) = specialty {
                    assert!(plan.resources.iter().any(|d| d.kind == kind
                        && distance2(v.center[0], v.center[2], d.center[0], d.center[2])
                            < 600.0 * 600.0));
                }
            }
        }
    }
}
