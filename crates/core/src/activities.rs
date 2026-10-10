//! Small visual activities. Plans contain real supported positions, never POI indices.
use crate::{
    physics::{Body, EYE_HEIGHT, MoveInput, character_position_is_clear, move_character},
    poi::{SiteArrangement, SitePlan},
    world::{BlockPos, CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};

pub const ACTIVITY_REACH: f32 = 2.8;
pub const RECIPE_VERSION: u32 = 3;
pub const REPAIR_SECONDS: f32 = 6.;
pub const MAX_PLANS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityKind {
    SpilledSupplies,
    ShapeStones,
    CartRepair,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityPlan {
    pub id: u64,
    pub recipe_version: u32,
    pub kind: ActivityKind,
    /// Stable seed/grid identity of a composed site, never its vector index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_id: Option<u64>,
    /// Foot positions: both the prop and its approach must be accessible.
    pub objects: [[f32; 3]; 3],
    pub sockets: [[f32; 3]; 3],
    /// Supported reference boards elsewhere on the route. Legacy review scenes
    /// keep their references immediately above the controls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clues: Option<[[f32; 3]; 3]>,
    pub answer: [u8; 3],
}
impl ActivityPlan {
    pub fn points(&self) -> impl Iterator<Item = &[f32; 3]> {
        self.objects
            .iter()
            .chain(&self.sockets)
            .chain(self.clues.iter().flatten())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropState {
    Home,
    Held(u64),
    Placed,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivitySnapshot {
    pub plan: ActivityPlan,
    pub revision: u64,
    pub props: [PropState; 3],
    pub faces: [u8; 3],
    pub complete: bool,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair: Option<RepairProgress>,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RepairProgress {
    pub player_id: u64,
    pub elapsed_seconds: f32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityAction {
    Take(u8),
    Place(u8),
    Return,
    Turn(u8),
    /// Fit a cart part using one unit of real Timber cargo.
    Contribute(u8),
    Hammer,
}

pub fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt()
}
/// The prop is at hand height; terrain remains editable and is checked again on use.
pub fn can_interact(world: &World, player: [f32; 3], point: [f32; 3]) -> bool {
    if !player.iter().chain(point.iter()).all(|v| v.is_finite())
        || distance(player, point) > ACTIVITY_REACH
        || !supported(world, point)
    {
        return false;
    }
    let eye = [player[0], player[1] + EYE_HEIGHT, player[2]];
    let target = [point[0], point[1] + 0.8, point[2]];
    let direction = std::array::from_fn(|i| target[i] - eye[i]);
    world
        .raycast(eye, direction, distance(eye, target))
        .is_none()
}
pub fn supported(world: &World, point: [f32; 3]) -> bool {
    let below = BlockPos::new(
        (point[0] / CELL_SIZE).floor() as i32,
        ((point[1] - 0.04) / CELL_SIZE).floor() as i32,
        (point[2] / CELL_SIZE).floor() as i32,
    );
    world.block(below).is_solid() && character_position_is_clear(world, point, &[])
}
/// Check an ordinary walking route with the real controller, including the return.
fn walk(world: &World, from: [f32; 3], to: [f32; 3]) -> bool {
    let mut body = Body::new(from);
    // Full POI routes can be longer than the original village review area.
    let steps = ((distance(from, to) / 3. + 4.) / 0.05).ceil() as usize;
    for _ in 0..steps.min(1_000) {
        if distance(body.position, to) < 0.6 {
            return true;
        }
        let d = [to[0] - body.position[0], to[2] - body.position[2]];
        move_character(
            world,
            &mut body,
            MoveInput {
                direction: d,
                ..Default::default()
            },
            0.05,
        );
        if !body.position.iter().all(|v| v.is_finite()) {
            return false;
        }
    }
    false
}
/// Two introductory activities at the established spawn. No new POI geometry,
/// terrain edits, NPCs, or island-wide event distribution is introduced here.
pub fn review_plans(world: &World) -> Vec<ActivityPlan> {
    let origin = world.spawn_position();
    let mut plans = Vec::new();
    for kind in [ActivityKind::SpilledSupplies, ActivityKind::ShapeStones] {
        'search: for radius in [7., 11., 15., 19.] {
            for step in 0..16 {
                let angle = step as f32 * std::f32::consts::TAU / 16.;
                let center = [
                    origin[0] + angle.cos() * radius,
                    origin[1],
                    origin[2] + angle.sin() * radius,
                ];
                if plans
                    .iter()
                    .any(|p: &ActivityPlan| distance(p.sockets[1], center) < 9.)
                {
                    continue;
                }
                let point = |x: f32, z: f32| {
                    let x = center[0] + x;
                    let z = center[2] + z;
                    [x, world.surface_height(x, z) + 0.01, z]
                };
                let sockets = [point(-1.4, 0.), point(0., 0.), point(1.4, 0.)];
                let objects = if kind == ActivityKind::SpilledSupplies {
                    [point(-2., 3.), point(1.7, 4.), point(3., 2.)]
                } else {
                    sockets
                };
                if sockets.iter().chain(objects.iter()).any(|p| {
                    (p[1] - origin[1]).abs() > 3.
                        || !supported(world, *p)
                        || !walk(world, origin, *p)
                        || !walk(world, *p, origin)
                }) {
                    continue;
                }
                let shift = (world.seed % 3) as u8;
                plans.push(ActivityPlan {
                    id: if kind == ActivityKind::SpilledSupplies {
                        1
                    } else {
                        2
                    },
                    recipe_version: RECIPE_VERSION,
                    kind,
                    site_id: None,
                    objects,
                    sockets,
                    clues: None,
                    answer: [shift, (shift + 1) % 3, (shift + 2) % 3],
                });
                break 'search;
            }
        }
    }
    plans
}

/// Independently select sites across the island, rather than consuming the
/// entire budget near spawn. Half of compatible sites remain quiet.
pub fn poi_plans(world: &World) -> Vec<ActivityPlan> {
    let Some(settlements) = world.settlements() else {
        return Vec::new();
    };
    let mut sites: Vec<_> = settlements.composed_sites.iter().collect();
    sites.sort_by_key(|s| s.id);
    sites
        .into_iter()
        .filter_map(|site| {
            // SplitMix finalizer: seed and grid both influence a stable choice.
            let mut hash = site.id ^ 0x9e37_79b9_7f4a_7c15;
            hash = (hash ^ (hash >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            hash = (hash ^ (hash >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            hash ^= hash >> 31;
            if hash & 1 != 0 || site.id <= 2 {
                return None;
            }
            at_site(world, site)
        })
        .take(MAX_PLANS)
        .collect()
}
pub fn plans(world: &World) -> Vec<ActivityPlan> {
    if world.settlements().is_some() {
        poi_plans(world)
    } else {
        review_plans(world)
    }
}
pub fn site_kind(arrangement: SiteArrangement) -> Option<ActivityKind> {
    match arrangement {
        SiteArrangement::KilnCourt
        | SiteArrangement::QuarrySteps
        | SiteArrangement::SplitCrossing => Some(ActivityKind::SpilledSupplies),
        SiteArrangement::GrovePortal | SiteArrangement::StoneSpan | SiteArrangement::BrokenRibs => {
            Some(ActivityKind::ShapeStones)
        }
        SiteArrangement::ExtractionFace => Some(ActivityKind::CartRepair),
        _ => None,
    }
}
pub fn at_site(world: &World, site: &SitePlan) -> Option<ActivityPlan> {
    let kind = site_kind(site.arrangement)?;
    // Use the entire authored route, including the descent and far-side return.
    // Never use surface_height: it sees bridge tops above the lower bypass.
    let from = *site.route.first()?;
    let to = *site.route.last()?;
    let length = distance(from, to);
    if site.route.len() < 2 || length < 1. {
        return None;
    }
    let direction = [(to[0] - from[0]) / length, (to[2] - from[2]) / length];
    let point = |t: f32, side: f32| {
        let step = t * (site.route.len() - 1) as f32;
        let leg = (step.floor() as usize).min(site.route.len() - 2);
        let fraction = step - leg as f32;
        let a = site.route[leg];
        let b = site.route[leg + 1];
        let approximate = [
            a[0] + (b[0] - a[0]) * fraction - direction[1] * side,
            a[1] + (b[1] - a[1]) * fraction,
            a[2] + (b[2] - a[2]) * fraction + direction[0] * side,
        ];
        let ground = (approximate[1] / CELL_SIZE).floor() as i32 - 1;
        // A route fraction can land exactly on a half-meter stair edge.
        // Move at most one meter along the route to support the whole body.
        [0., 0.5, -0.5, 1., -1.].into_iter().find_map(|along| {
            [0, -1, 1, -2, 2, -3, 3, -4, 4].into_iter().find_map(|dy| {
                let p = [
                    approximate[0] + direction[0] * along,
                    (ground + dy + 1) as f32 * CELL_SIZE + 0.01,
                    approximate[2] + direction[1] * along,
                ];
                supported(world, p).then_some(p)
            })
        })
    };
    let (objects, sockets, clues) = match (kind, site.arrangement) {
        (ActivityKind::SpilledSupplies, arrangement) => {
            let rack = if arrangement == SiteArrangement::QuarrySteps {
                0.5
            } else {
                0.08
            };
            (
                [point(0.22, -0.7)?, point(0.58, 0.8)?, point(0.9, -1.0)?],
                [point(rack, -1.4)?, point(rack, 0.)?, point(rack, 1.4)?],
                None,
            )
        }
        (ActivityKind::ShapeStones, _) => {
            let sockets = [point(0.18, -1.4)?, point(0.48, 1.4)?, point(0.76, -1.4)?];
            // Each board stands further along the route than its matching
            // control. One/two/three raised pips link the pairs without text.
            let clues = [point(0.32, 1.4)?, point(0.62, -1.4)?, point(0.92, 1.4)?];
            (sockets, sockets, Some(clues))
        }
        (ActivityKind::CartRepair, _) => {
            let sockets = [point(0.15, -0.9)?, point(0.15, 0.9)?, point(0.15, 0.)?];
            if sockets.iter().any(|p| (p[1] - sockets[2][1]).abs() > 0.1) {
                return None;
            }
            (sockets, sockets, None)
        }
    };
    // Verify approach and return for every piece, plus the actual carried route.
    if objects
        .iter()
        .chain(&sockets)
        .chain(clues.iter().flatten())
        .any(|p| !walk(world, from, *p) || !walk(world, *p, from))
        || (kind == ActivityKind::SpilledSupplies
            && (0..3).any(|i| {
                !walk(world, objects[i], sockets[i]) || !walk(world, sockets[i], objects[i])
            }))
    {
        return None;
    }
    let shift = (site.id % 3) as u8;
    Some(ActivityPlan {
        id: site.id,
        recipe_version: RECIPE_VERSION,
        kind,
        site_id: Some(site.id),
        objects,
        sockets,
        clues,
        answer: [shift, (shift + 1) % 3, (shift + 2) % 3],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_area_is_supported_walkable_and_reproducible() {
        let world = World::new(42);
        let plans = review_plans(&world);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans, review_plans(&world));
        for plan in plans {
            for p in plan.objects.into_iter().chain(plan.sockets) {
                assert!(supported(&world, p));
                assert!(walk(&world, world.spawn_position(), p));
                assert!(walk(&world, p, world.spawn_position()));
            }
        }
    }
    #[test]
    fn geographic_review_area_has_both_families() {
        for seed in [42, 7, 99] {
            let world = World::generate(seed, crate::world::WorldGeneration::GeographyV6);
            let plans = review_plans(&world);
            assert_eq!(plans.len(), 2, "seed {seed}");
        }
    }
    #[test]
    fn poi_activities_use_stable_sites_and_walkable_outward_carried_and_return_routes() {
        for seed in [42, 7, 99] {
            let world = World::generate(seed, crate::world::WorldGeneration::GeographyV6);
            let activities = poi_plans(&world);
            assert!(
                activities.len() >= 6 && activities.len() <= MAX_PLANS,
                "seed {seed}: {}",
                activities.len()
            );
            assert_eq!(activities, poi_plans(&world));
            assert_eq!(
                plans(&world),
                activities,
                "Island worlds have no town review scenes"
            );
            let sites = &world.settlements().unwrap().composed_sites;
            assert!(activities.len() < sites.len() / 2);
            for kind in [
                ActivityKind::SpilledSupplies,
                ActivityKind::ShapeStones,
                ActivityKind::CartRepair,
            ] {
                assert!(
                    activities.iter().any(|p| p.kind == kind),
                    "seed {seed} needs both activity families"
                );
            }
            if seed == 42 {
                assert!(
                    activities
                        .iter()
                        .any(|p| sites.iter().any(|s| Some(s.id) == p.site_id
                            && s.arrangement == SiteArrangement::QuarrySteps)),
                    "The quarry descent must host a usable scene"
                );
            }
            let extent = |axis| {
                activities
                    .iter()
                    .map(|p| p.objects[0][axis])
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
                        (lo.min(v), hi.max(v))
                    })
            };
            let (xmin, xmax) = extent(0);
            let (zmin, zmax) = extent(2);
            assert!(
                (xmax - xmin).max(zmax - zmin) > 6_000.,
                "Encounters must extend beyond the starting region"
            );
            eprintln!(
                "seed {seed}: {} activities / {} sites; extent {:.0} × {:.0} m",
                activities.len(),
                sites.len(),
                xmax - xmin,
                zmax - zmin
            );
            for p in activities {
                let site = sites.iter().find(|s| Some(s.id) == p.site_id).unwrap();
                assert_eq!(p.id, site.id);
                assert_eq!(Some(p.kind), site_kind(site.arrangement));
                assert!(distance(world.spawn_position(), site.entrance()) > 250.);
                if site.arrangement == SiteArrangement::QuarrySteps {
                    assert!(
                        site.entrance()[1] - p.sockets[1][1] > 2.,
                        "The rack belongs on the quarry floor"
                    );
                }
                if p.kind == ActivityKind::CartRepair {
                    assert!(distance(p.sockets[0], p.sockets[1]) > 1.5);
                    assert!(p.points().all(|v| (v[1] - p.sockets[2][1]).abs() < 0.1));
                } else {
                    assert!(
                        distance(p.objects[0], p.objects[2]) > 12.,
                        "Use the site, not a single pad"
                    );
                }
                for v in p.points() {
                    assert!(supported(&world, *v));
                    assert!(walk(&world, site.entrance(), *v));
                    assert!(walk(&world, *v, site.entrance()));
                }
                for i in 0..3 {
                    assert!(walk(&world, p.objects[i], p.sockets[i]));
                    assert!(walk(&world, p.sockets[i], p.objects[i]));
                    if let Some(clues) = p.clues {
                        assert!(distance(clues[i], p.sockets[i]) > ACTIVITY_REACH);
                        assert!(walk(&world, p.sockets[i], clues[i]));
                        assert!(walk(&world, clues[i], p.sockets[i]));
                    }
                }
            }
        }
    }
    #[test]
    fn edits_and_walls_invalidate_interactions_without_repairing_terrain() {
        let mut world = World::new(42);
        let p = review_plans(&world)[0].objects[0];
        assert!(can_interact(&world, p, p));
        let below = BlockPos::new(
            (p[0] / CELL_SIZE).floor() as i32,
            ((p[1] - 0.04) / CELL_SIZE).floor() as i32,
            (p[2] / CELL_SIZE).floor() as i32,
        );
        world.set_block(below, crate::world::Block::Air).unwrap();
        assert!(!can_interact(&world, p, p));
        assert!(!can_interact(&world, [f32::NAN, 0., 0.], p));
    }
}
