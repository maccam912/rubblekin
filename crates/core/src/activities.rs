//! Small visual activities. Plans contain real supported positions, never POI indices.
use crate::{
    physics::{Body, EYE_HEIGHT, MoveInput, character_position_is_clear, move_character},
    poi::{SiteArrangement, SitePlan},
    world::{BlockPos, CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};

pub const ACTIVITY_REACH: f32 = 2.8;
pub const RECIPE_VERSION: u32 = 1;
pub const MAX_PLANS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityKind {
    SpilledSupplies,
    ShapeStones,
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
    pub answer: [u8; 3],
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
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivityAction {
    Take(u8),
    Place(u8),
    Return,
    Turn(u8),
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
    for _ in 0..160 {
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
                    answer: [shift, (shift + 1) % 3, (shift + 2) % 3],
                });
                break 'search;
            }
        }
    }
    plans
}

/// Keep most scenery quiet. One compatible scene per selected arrangement,
/// choosing the nearest usable instance without changing any terrain.
pub fn poi_plans(world: &World) -> Vec<ActivityPlan> {
    let Some(settlements) = world.settlements() else {
        return Vec::new();
    };
    let mut sites: Vec<_> = settlements.composed_sites.iter().collect();
    sites.sort_by(|a, b| {
        distance(a.entrance(), world.spawn_position())
            .total_cmp(&distance(b.entrance(), world.spawn_position()))
            .then(a.id.cmp(&b.id))
    });
    let mut used = Vec::new();
    sites
        .into_iter()
        .filter_map(|site| {
            if used.contains(&site.arrangement) || site.id <= 2 {
                return None;
            }
            let plan = at_site(world, site)?;
            used.push(site.arrangement);
            Some(plan)
        })
        .take(MAX_PLANS - 2)
        .collect()
}
pub fn plans(world: &World) -> Vec<ActivityPlan> {
    let mut plans = review_plans(world);
    plans.extend(poi_plans(world));
    plans
}
pub fn site_kind(arrangement: SiteArrangement) -> Option<ActivityKind> {
    match arrangement {
        SiteArrangement::KilnCourt
        | SiteArrangement::QuarrySteps
        | SiteArrangement::SplitCrossing => Some(ActivityKind::SpilledSupplies),
        SiteArrangement::GrovePortal | SiteArrangement::StoneSpan | SiteArrangement::BrokenRibs => {
            Some(ActivityKind::ShapeStones)
        }
        _ => None,
    }
}
fn at_site(world: &World, site: &SitePlan) -> Option<ActivityPlan> {
    let kind = site_kind(site.arrangement)?;
    // The first authored leg reaches the grove/court, quarry floor or bypass
    // beneath the broken bridge. Never use surface_height: it sees bridge tops.
    let from = site.route[0];
    let to = site.route[1];
    let length = distance(from, to);
    let direction = [(to[0] - from[0]) / length, (to[2] - from[2]) / length];
    let point = |t: f32, side: f32| {
        let approximate = [
            from[0] + (to[0] - from[0]) * t - direction[1] * side,
            from[1] + (to[1] - from[1]) * t,
            from[2] + (to[2] - from[2]) * t + direction[0] * side,
        ];
        let ground = (approximate[1] / CELL_SIZE).floor() as i32 - 1;
        [0, -1, 1, -2, 2, -3, 3, -4, 4].into_iter().find_map(|dy| {
            let p = [
                approximate[0],
                (ground + dy + 1) as f32 * CELL_SIZE + 0.01,
                approximate[2],
            ];
            supported(world, p).then_some(p)
        })
    };
    let (objects, sockets) = match (kind, site.arrangement) {
        (ActivityKind::SpilledSupplies, arrangement) => {
            let t = if arrangement == SiteArrangement::KilnCourt {
                0.8
            } else {
                0.95
            };
            (
                [
                    point(t - 2. / length, -0.45)?,
                    point(t, 0.45)?,
                    point(t - 1. / length, 0.)?,
                ],
                [
                    point(2. / length, -0.7)?,
                    point(3.5 / length, 0.)?,
                    point(5. / length, 0.7)?,
                ],
            )
        }
        (ActivityKind::ShapeStones, arrangement) => {
            let sockets = if arrangement == SiteArrangement::GrovePortal {
                [point(0.9, -0.75)?, point(0.75, 0.)?, point(0.9, 0.75)?]
            } else if arrangement == SiteArrangement::StoneSpan {
                [point(0.7, 0.)?, point(0.85, 0.)?, point(1., 0.)?]
            } else {
                [point(0.55, -0.75)?, point(0.8, 0.)?, point(0.6, 0.75)?]
            };
            (sockets, sockets)
        }
    };
    // Verify approach and return for every piece, plus the actual carried route.
    if objects
        .iter()
        .chain(&sockets)
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
            assert_eq!(
                activities.len(),
                6,
                "seed {seed}: {:?}",
                activities.iter().map(|p| p.site_id).collect::<Vec<_>>()
            );
            assert_eq!(activities, poi_plans(&world));
            let sites = &world.settlements().unwrap().composed_sites;
            assert!(activities.len() < sites.len() / 2);
            for p in activities {
                let site = sites.iter().find(|s| Some(s.id) == p.site_id).unwrap();
                assert_eq!(p.id, site.id);
                assert_eq!(Some(p.kind), site_kind(site.arrangement));
                for v in p.objects.into_iter().chain(p.sockets) {
                    assert!(supported(&world, v));
                    assert!(walk(&world, site.entrance(), v));
                    assert!(walk(&world, v, site.entrance()));
                }
                for i in 0..3 {
                    assert!(walk(&world, p.objects[i], p.sockets[i]));
                    assert!(walk(&world, p.sockets[i], p.objects[i]));
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
