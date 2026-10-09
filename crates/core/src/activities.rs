//! Small visual activities. Plans contain real supported positions, never POI indices.
use crate::{
    physics::{Body, EYE_HEIGHT, MoveInput, character_position_is_clear, move_character},
    world::{BlockPos, CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};

pub const ACTIVITY_REACH: f32 = 2.8;
pub const RECIPE_VERSION: u32 = 1;

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
