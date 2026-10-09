//! Authored surface arrangements, resolved once into ordinary editable cells.
//! Coordinates and half-open bounds are in cells. Broad bounds never clear land.
use crate::{
    geography::Geography,
    world::{Block, BlockPos, CELL_SIZE},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SiteArrangement {
    GrovePortal,
    StoneSpan,
    BrokenRibs,
    KilnCourt,
    QuarrySteps,
    ExtractionFace,
    LowCauseway,
    SplitCrossing,
    HillsideStairs,
}
impl SiteArrangement {
    pub const ALL: [Self; 9] = [
        Self::GrovePortal,
        Self::StoneSpan,
        Self::BrokenRibs,
        Self::KilnCourt,
        Self::QuarrySteps,
        Self::ExtractionFace,
        Self::LowCauseway,
        Self::SplitCrossing,
        Self::HillsideStairs,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::GrovePortal => "Grove portal",
            Self::StoneSpan => "Stone span",
            Self::BrokenRibs => "Broken stone ribs",
            Self::KilnCourt => "Abandoned kiln court",
            Self::QuarrySteps => "Quarry steps",
            Self::ExtractionFace => "Old extraction face",
            Self::LowCauseway => "Low causeway",
            Self::SplitCrossing => "Split crossing",
            Self::HillsideStairs => "Hillside crossing stairs",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::GrovePortal => {
                "Unequal stone portals frame an open grove. The path passes through both openings."
            }
            Self::StoneSpan => "A broad stone span shelters a lower passage between its shoulders.",
            Self::BrokenRibs => {
                "Surviving stone ribs surround a basin; fallen pieces lie beside the open route."
            }
            Self::KilnCourt => {
                "Cold kilns and a roofless drying shelter surround a working court. One corner has fallen outward."
            }
            Self::QuarrySteps => {
                "Broad working ledges descend beside a haul ramp. An uncut stone pillar survives at the back."
            }
            Self::ExtractionFace => {
                "A long exposed work face overlooks a lower haul aisle and a raised inspection ledge."
            }
            Self::LowCauseway => {
                "A long low crossing follows a dry swale. Broken parapets leave the walking surface open."
            }
            Self::SplitCrossing => {
                "Two bridge ends survive above a dry cut. Fallen masonry lies beside the bypass beneath the missing span."
            }
            Self::HillsideStairs => {
                "Broad switchback steps reach a surviving crossing support. The return follows the same open terraces."
            }
        }
    }
    pub fn symbol(self) -> &'static str {
        match self {
            Self::GrovePortal | Self::StoneSpan | Self::BrokenRibs => "A",
            Self::KilnCourt | Self::QuarrySteps | Self::ExtractionFace => "Q",
            _ => "X",
        }
    }
}

/// Resolved solid geometry also supplies the distant silhouette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SiteSolid {
    pub min: BlockPos,
    pub size: [i32; 3],
    pub block: Block,
}
impl SiteSolid {
    pub fn contains(self, p: BlockPos) -> bool {
        (self.min.x..self.min.x + self.size[0]).contains(&p.x)
            && (self.min.y..self.min.y + self.size[1]).contains(&p.y)
            && (self.min.z..self.min.z + self.size[2]).contains(&p.z)
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct SiteGround {
    pub bounds: [i32; 4],
    /// Ground cells at the front/back of the patch; one-cell steps at most.
    pub ends: [i32; 2],
    pub axis: usize,
    pub surface: Block,
}
impl SiteGround {
    fn height(&self, x: i32, z: i32) -> i32 {
        let a = self.axis;
        let run = (self.bounds[a + 2] - self.bounds[a] - 1).max(1);
        let coordinate = if a == 0 { x } else { z };
        self.ends[0]
            + (self.ends[1] - self.ends[0]) * (coordinate - self.bounds[a]).clamp(0, run) / run
    }
}

/// Identity is seed + candidate grid cell, never a vector index or load order.
/// Routes are ordinary walking waypoints, useful to future activity placement.
#[derive(Debug, Clone, PartialEq)]
pub struct SitePlan {
    pub id: u64,
    pub arrangement: SiteArrangement,
    pub orientation: u8,
    pub bounds: [i32; 4],
    pub solids: Vec<SiteSolid>,
    pub ground: Vec<SiteGround>,
    pub route: Vec<[f32; 3]>,
}

#[derive(Debug, Clone, Copy)]
struct Span {
    low: i32,
    high: i32,
    block: Block,
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct SiteColumn {
    spans: [Option<Span>; 4],
}
impl SiteColumn {
    pub fn top(self) -> i32 {
        self.spans
            .iter()
            .flatten()
            .map(|s| s.high)
            .max()
            .unwrap_or(i32::MIN)
    }
    pub fn block(self, y: i32) -> Option<Block> {
        self.spans
            .iter()
            .flatten()
            .rev()
            .find(|s| (s.low..=s.high).contains(&y))
            .map(|s| s.block)
    }
}

impl SitePlan {
    pub fn clears_tree(&self, x: f32, z: f32, radius: f32) -> bool {
        let within = |b: [i32; 4]| {
            x >= b[0] as f32 * CELL_SIZE - radius - 1.0
                && x <= b[2] as f32 * CELL_SIZE + radius + 1.0
                && z >= b[1] as f32 * CELL_SIZE - radius - 1.0
                && z <= b[3] as f32 * CELL_SIZE + radius + 1.0
        };
        self.ground.iter().any(|g| within(g.bounds))
            || self
                .solids
                .iter()
                .any(|s| within([s.min.x, s.min.z, s.min.x + s.size[0], s.min.z + s.size[2]]))
    }
    pub fn entrance(&self) -> [f32; 3] {
        self.route[0]
    }
    pub fn owns_block(&self, p: BlockPos) -> bool {
        self.solids.iter().any(|s| s.contains(p))
            || self.ground.iter().any(|g| {
                (g.bounds[0]..g.bounds[2]).contains(&p.x)
                    && (g.bounds[1]..g.bounds[3]).contains(&p.z)
                    && p.y == g.height(p.x, p.z)
            })
    }
    pub(crate) fn column(
        &self,
        x: i32,
        z: i32,
        height: &mut i32,
        surface: &mut Block,
    ) -> (bool, Option<SiteColumn>) {
        let (ground, material, mut clear) = self.ground_at(x, z, *height, *surface);
        *height = ground;
        *surface = material;
        let mut spans: [Option<Span>; 4] = [None; 4];
        for s in &self.solids {
            if !(s.min.x..s.min.x + s.size[0]).contains(&x)
                || !(s.min.z..s.min.z + s.size[2]).contains(&z)
            {
                continue;
            }
            clear = true;
            let span = Span {
                low: s.min.y,
                high: s.min.y + s.size[1] - 1,
                block: s.block,
            };
            if let Some(existing) = spans
                .iter_mut()
                .flatten()
                .find(|p| p.block == span.block && p.low <= span.high + 1 && span.low <= p.high + 1)
            {
                existing.low = existing.low.min(span.low);
                existing.high = existing.high.max(span.high);
            } else {
                *spans
                    .iter_mut()
                    .find(|s| s.is_none())
                    .expect("authored POI column exceeds four solid intervals") = Some(span);
            }
        }
        (clear, spans[0].is_some().then_some(SiteColumn { spans }))
    }

    pub fn ground_at(&self, x: i32, z: i32, height: i32, surface: Block) -> (i32, Block, bool) {
        // Exact occupied patches take precedence over neighboring aprons. Blend
        // once toward the nearest patch outside them; overlapping aprons must
        // never create a cliff across a designated walking route.
        let distance = |g: &SiteGround| {
            (g.bounds[0] - x)
                .max(0)
                .max(x - g.bounds[2] + 1)
                .max((g.bounds[1] - z).max(0).max(z - g.bounds[3] + 1))
        };
        if let Some(g) = self.ground.iter().rev().find(|g| distance(g) == 0) {
            return (g.height(x, z), g.surface, true);
        }
        if let Some(g) = self.ground.iter().min_by_key(|g| distance(g)) {
            let d = distance(g);
            if d < 12 {
                return (
                    (height * d + g.height(x, z) * (12 - d)) / 12,
                    surface,
                    false,
                );
            }
        }
        (height, surface, false)
    }

    /// Fit a recipe to gentle, dry terrain. Local grades stay within three metres of the base;
    /// no assumption about a real river, deposit, inhabitant or live event.
    pub fn fit(
        id: u64,
        arrangement: SiteArrangement,
        geography: &Geography,
        x: f32,
        z: f32,
    ) -> Option<Self> {
        let n = ((id ^ (id >> 32)) & 3) as i32;
        let (w, d) = match arrangement {
            SiteArrangement::GrovePortal => (48 + n * 4, 72 + n * 6),
            SiteArrangement::StoneSpan => (88 + n * 8, 48 + n * 4),
            SiteArrangement::BrokenRibs => (72 + n * 6, 80 + n * 8),
            SiteArrangement::KilnCourt => (64 + n * 6, 64 + n * 4),
            SiteArrangement::QuarrySteps => (80 + n * 8, 96 + n * 6),
            SiteArrangement::ExtractionFace => (48 + n * 4, 112 + n * 8),
            SiteArrangement::LowCauseway => (40 + n * 4, 128 + n * 10),
            SiteArrangement::SplitCrossing => (80 + n * 4, 112 + n * 8),
            SiteArrangement::HillsideStairs => (64 + n * 4, 96 + n * 8),
        };
        let orientation = ((id >> 2) & 3) as u8;
        let (rw, rd) = if orientation.is_multiple_of(2) {
            (w, d)
        } else {
            (d, w)
        };
        let ox = (x / CELL_SIZE).floor() as i32 - rw / 2;
        let oz = (z / CELL_SIZE).floor() as i32 - rd / 2;
        let base = (geography.sample(x, z).height / CELL_SIZE).floor() as i32;
        for dz in (-12..=rd + 12).step_by(8) {
            for dx in (-12..=rw + 12).step_by(8) {
                let s =
                    geography.sample((ox + dx) as f32 * CELL_SIZE, (oz + dz) as f32 * CELL_SIZE);
                if s.water.is_some()
                    || s.height < 4.0
                    || (s.height - base as f32 * CELL_SIZE).abs() > 1.5
                {
                    return None;
                }
            }
        }
        let mut site = Self {
            id,
            arrangement,
            orientation,
            bounds: [ox - 12, oz - 12, ox + rw + 12, oz + rd + 12],
            solids: vec![],
            ground: vec![],
            route: vec![],
        };
        let cx = w / 2;
        // Build in local coordinates, then translate once. Human-scale widths
        // and headroom stay fixed as the spaces between structures vary.
        match arrangement {
            SiteArrangement::GrovePortal => {
                site.patch([cx - 4, 0, cx + 4, d], [0, 0], Block::Dirt);
                site.arch(cx - 20, 10, 40, 24 + n * 2, 6);
                site.arch(cx - 16, d - 20, 32, 18 + n, 6);
                site.waypoints(&[(cx, 0, 0), (cx, d / 2, 0), (cx, d - 1, 0)]);
            }
            SiteArrangement::StoneSpan => {
                site.patch([0, d / 2 - 5, w, d / 2 + 5], [0, 0], Block::Dirt);
                site.arch(4, d / 2 - 7, w - 8, 28 + n * 3, 14);
                // The route crosses the opening perpendicular to its long span.
                site.patch([cx - 4, 0, cx + 4, d], [0, 0], Block::Dirt);
                site.waypoints(&[(cx, 0, 0), (cx, d / 2, 0), (cx, d - 1, 0)]);
            }
            SiteArrangement::BrokenRibs => {
                site.patch([cx - 5, 0, cx + 5, d], [0, 0], Block::Grass);
                for i in 0..3 {
                    site.arch(
                        6 + i * 2,
                        12 + i * (d - 24) / 3,
                        w - 12 - i * 4,
                        26 - i * 5 + n,
                        5,
                    );
                }
                site.solids.truncate(site.solids.len() - 2);
                site.cube([w - 16, 1, d - 18], [9, 3, 9], Block::Stone);
                site.waypoints(&[(cx, 0, 0), (cx, d / 2, 0), (cx, d - 1, 0)]);
            }
            SiteArrangement::KilnCourt => {
                site.patch([4, 0, w - 4, d - 4], [0, 0], Block::Dirt);
                site.kiln(6, 12, true);
                site.kiln(w - 20, d - 24, false);
                // Roofless drying shelter: supports and a surviving lintel.
                for px in [8, w - 10] {
                    site.cube([px, 1, d - 12], [2, 10, 2], Block::Wood);
                }
                site.cube([8, 11, d - 12], [w - 16, 2, 2], Block::Wood);
                site.cube([4, 1, 8], [4, 2, 3], Block::Brick);
                site.waypoints(&[(cx, 0, 0), (cx, d / 2, 0), (cx, d - 8, 0)]);
            }
            SiteArrangement::QuarrySteps => {
                // Three broad ledges share the central continuously descending haul ramp.
                for i in 0..3 {
                    let a = 8 + i * (d - 16) / 3;
                    let b = 8 + (i + 1) * (d - 16) / 3;
                    site.patch([8, a, w - 8, b], [-i * 3, -i * 3], Block::Stone);
                }
                site.patch([cx - 5, 0, cx + 5, d / 2], [0, -6], Block::Dirt);
                site.patch([cx - 5, d / 2, cx + 5, d], [-6, 0], Block::Dirt);
                site.cube([9, -5, d - 22], [8, 18 + n * 2, 8], Block::Stone);
                site.waypoints(&[(cx, 0, 0), (cx, d / 2, -6), (cx, d - 1, 0)]);
            }
            SiteArrangement::ExtractionFace => {
                site.patch([cx - 5, 0, cx + 5, d], [0, 0], Block::Dirt);
                site.patch([5, 12, cx - 8, d - 12], [0, 0], Block::Stone);
                site.cube([5, 1, 12], [5, 10 + n * 2, d - 24], Block::Stone);
                site.cube([10, 1, 12], [cx - 18, 3, d - 24], Block::Stone);
                for pz in [22, d - 30] {
                    site.cube([w - 10, 1, pz], [3, 6, 6], Block::Wood);
                }
                site.waypoints(&[(cx, 0, 0), (cx, d / 2, 0), (cx, d - 1, 0)]);
            }
            SiteArrangement::LowCauseway => {
                // Dry swales beside the crossing leave the surrounding land intact.
                for px in [cx - 13, cx + 7] {
                    site.patch([px, 16, px + 6, d - 16], [-2, -2], Block::Grass);
                }
                site.patch([cx - 5, 0, cx + 5, d], [0, 0], Block::Stone);
                for pz in (16..d - 12).step_by(18) {
                    site.cube([cx - 7, 1, pz], [2, 3, 8], Block::Stone);
                    site.cube([cx + 5, 1, pz + 4], [2, 2, 6], Block::Stone);
                }
                site.waypoints(&[(cx, 0, 0), (cx, d / 2, 0), (cx, d - 1, 0)]);
            }
            SiteArrangement::SplitCrossing => {
                site.patch([cx - 5, 0, cx + 5, d / 2], [0, -6], Block::Dirt);
                site.patch([cx - 5, d / 2, cx + 5, d], [-6, 0], Block::Dirt);
                // Deliberately missing centre span; a lower bypass stays readable.
                for pz in [d / 2 - 24, d / 2 + 10] {
                    site.cube([cx - 14, 8, pz], [28, 3, 14], Block::Stone);
                    for px in [cx - 14, cx + 10] {
                        site.cube([px, -6, pz + 6], [4, 14, 8], Block::Stone);
                    }
                }
                site.cube([cx + 9, -5, d / 2 - 4], [9, 4, 7], Block::Stone);
                site.waypoints(&[(cx, 0, 0), (cx, d / 2, -6), (cx, d - 1, 0)]);
            }
            SiteArrangement::HillsideStairs => {
                site.patch([8, 0, 16, d - 12], [0, 6], Block::Stone);
                site.patch([8, d - 12, w - 8, d], [6, 6], Block::Stone);
                site.patch([w - 16, 0, w - 8, d - 12], [0, 6], Block::Stone);
                site.cube([cx - 6, 7, d - 10], [12, 12 + n * 2, 4], Block::Stone);
                site.cube([cx - 12, 7, d - 12], [4, 3, 5], Block::Stone);
                site.waypoints(&[
                    (12, 0, 0),
                    (12, d - 3, 6),
                    (w - 12, d - 3, 6),
                    (w - 12, 0, 0),
                ]);
            }
        }
        let rotate_bounds = |[x0, z0, x1, z1]: [i32; 4]| match orientation {
            0 => [x0, z0, x1, z1],
            1 => [d - z1, x0, d - z0, x1],
            2 => [w - x1, d - z1, w - x0, d - z0],
            _ => [z0, w - x1, z1, w - x0],
        };
        for s in &mut site.solids {
            let b = rotate_bounds([s.min.x, s.min.z, s.min.x + s.size[0], s.min.z + s.size[2]]);
            s.min.x = ox + b[0];
            s.min.z = oz + b[1];
            s.min.y += base;
            s.size[0] = b[2] - b[0];
            s.size[2] = b[3] - b[1];
        }
        for g in &mut site.ground {
            let b = rotate_bounds(g.bounds);
            g.bounds = [ox + b[0], oz + b[1], ox + b[2], oz + b[3]];
            g.axis = if orientation.is_multiple_of(2) { 1 } else { 0 };
            if orientation == 1 || orientation == 2 {
                g.ends.reverse();
            }
            g.ends[0] += base;
            g.ends[1] += base;
        }
        for p in &mut site.route {
            let (a, b) = match orientation {
                0 => (p[0], p[2]),
                1 => (d as f32 - 1. - p[2], p[0]),
                2 => (w as f32 - 1. - p[0], d as f32 - 1. - p[2]),
                _ => (p[2], w as f32 - 1. - p[0]),
            };
            p[0] = (a + ox as f32 + 0.5) * CELL_SIZE;
            p[2] = (b + oz as f32 + 0.5) * CELL_SIZE;
        }
        for i in 0..site.route.len() {
            let p = site.route[i];
            let x = (p[0] / CELL_SIZE).floor() as i32;
            let z = (p[2] / CELL_SIZE).floor() as i32;
            let natural = (geography.sample(p[0], p[2]).height / CELL_SIZE).floor() as i32;
            site.route[i][1] =
                (site.ground_at(x, z, natural, Block::Grass).0 + 1) as f32 * CELL_SIZE;
        }
        Some(site)
    }
    fn cube(&mut self, min: [i32; 3], size: [i32; 3], block: Block) {
        self.solids.push(SiteSolid {
            min: BlockPos::new(min[0], min[1], min[2]),
            size,
            block,
        });
    }
    fn patch(&mut self, bounds: [i32; 4], ends: [i32; 2], surface: Block) {
        self.ground.push(SiteGround {
            bounds,
            ends,
            axis: 1,
            surface,
        });
    }
    fn waypoints(&mut self, points: &[(i32, i32, i32)]) {
        self.route = points
            .iter()
            .map(|&(x, z, y)| [x as f32, y as f32, z as f32])
            .collect();
    }
    fn arch(&mut self, x: i32, z: i32, w: i32, h: i32, depth: i32) {
        // Stepped shoulders and unequal crown blocks; broad clear aperture.
        for i in 0..8 {
            let a = i * w / 8;
            let b = (i + 1) * w / 8;
            let top = h - (i - 3).abs() * 2;
            let low = if i == 0 || i == 7 { 1 } else { top - 4 };
            self.cube([x + a, low, z], [b - a, top - low + 1, depth], Block::Stone);
            if i == 0 || i == 7 {
                self.patch([x + a, z, x + b, z + depth], [0, 0], Block::Grass);
            }
        }
    }
    fn kiln(&mut self, x: i32, z: i32, collapsed: bool) {
        self.cube([x + 10, 1, z], [2, 8, 14], Block::Brick);
        if collapsed {
            self.cube([x, 1, z], [2, 2, 4], Block::Brick);
            self.cube([x, 1, z + 4], [2, 8, 10], Block::Brick);
        } else {
            self.cube([x, 1, z], [2, 8, 14], Block::Brick);
        }
        self.cube([x, 1, z + 12], [12, 8, 2], Block::Brick);
        self.cube([x, 9, z + 5], [12, 2, 9], Block::Brick);
        self.cube([x + 3, 11, z + 10], [3, 8, 3], Block::Brick);
    }
}
