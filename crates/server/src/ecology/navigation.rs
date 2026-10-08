//! Disposable local detours for small animal bodies. Habitat choices remain in
//! ecology; this only helps a physical journey get past a nearby obstacle.
use std::collections::VecDeque;

use rubblekin_core::{
    physics::Body,
    wildlife::{Species, move_animal, position_is_clear},
    world::{CELL_SIZE, World},
};

#[derive(Clone, Default)]
pub(super) struct Navigation {
    goal: Option<[f32; 3]>,
    checkpoint: Option<([f32; 3], f32)>,
    stalled: f32,
    retry: f32,
    path: VecDeque<[f32; 3]>,
}

impl Navigation {
    pub fn search_urgency(&self) -> Option<f32> {
        (self.stalled >= 0.75 && self.retry <= 0.).then_some(self.stalled)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn steer(
        &mut self,
        world: &World,
        body: &Body,
        species: Species,
        target: [f32; 3],
        dt: f32,
        may_search: bool,
        allowed: impl Fn([f32; 3]) -> bool,
    ) -> Option<([f32; 2], f32)> {
        if self.goal.is_none_or(|p| distance(p, target) > 4.) {
            *self = Self {
                goal: Some(target),
                ..Self::default()
            };
        }
        self.retry = (self.retry - dt).max(0.);
        while self.path.front().is_some_and(|p| {
            distance(*p, body.position) < 0.18 && (p[1] - body.position[1]).abs() < 0.6
        }) {
            self.path.pop_front();
            self.checkpoint = None;
        }
        let steering_target = self.path.front().copied().unwrap_or(target);
        let (checkpoint_target, remaining) = self
            .checkpoint
            .get_or_insert((steering_target, distance(body.position, steering_target)));
        let now = distance(body.position, *checkpoint_target);
        if distance(*checkpoint_target, steering_target) > 2. || *remaining - now >= 0.2 {
            *checkpoint_target = steering_target;
            *remaining = distance(body.position, steering_target);
            self.stalled = 0.;
        } else {
            self.stalled = (self.stalled + dt).min(30.);
        }
        if may_search && self.search_urgency().is_some() {
            self.path = detour(world, body, species, target, &allowed);
            self.checkpoint = None;
            self.stalled = 0.;
            self.retry = 4.;
        }
        let steering_target = self.path.front().copied().unwrap_or(target);
        let direction = if self.path.is_empty() {
            fan(world, body, species, steering_target, &allowed)
        } else {
            let direction = toward(body.position, steering_target);
            let length = distance(body.position, steering_target).min(CELL_SIZE);
            let next = [
                body.position[0] + direction[0] * length,
                body.position[1],
                body.position[2] + direction[1] * length,
            ];
            // Follow the same whole-body support that found the route. A center
            // ray can see a lower slope while the feet still rest on its edge.
            if walk_step(world, body, species, next, &allowed).is_some() {
                Some(direction)
            } else {
                self.path.clear();
                fan(world, body, species, target, &allowed)
            }
        };
        direction.map(|direction| {
            // Small waypoints must not be overshot at the faster hunting speed
            // or during a coarse test step. Live motion still uses move_animal.
            let cap = if self.path.is_empty() {
                f32::MAX
            } else {
                distance(body.position, steering_target) / dt
            };
            (direction, cap)
        })
    }
}

pub(super) fn fan(
    world: &World,
    body: &Body,
    species: Species,
    target: [f32; 3],
    allowed: &impl Fn([f32; 3]) -> bool,
) -> Option<[f32; 2]> {
    let direction = toward(body.position, target);
    // Check a short approach at its target, rather than rejecting it because
    // of an obstacle beyond the requested movement.
    let probe_distance = distance(body.position, target).min(0.9);
    for angle in [0_f32, 0.5, -0.5, 1., -1., 1.5, -1.5] {
        let d = [
            direction[0] * angle.cos() - direction[1] * angle.sin(),
            direction[0] * angle.sin() + direction[1] * angle.cos(),
        ];
        let p = [
            body.position[0] + d[0] * probe_distance,
            body.position[1],
            body.position[2] + d[1] * probe_distance,
        ];
        if !allowed(p) {
            continue;
        }
        // Probe the actual edited surface, including raised floors. The terrain
        // generator's original height cannot describe a changed column.
        let Some(hit) = world.raycast([p[0], p[1] + 0.55, p[2]], [0., -1., 0.], 2.1) else {
            continue;
        };
        let ground = (hit.position.y + 1) as f32 * CELL_SIZE;
        if ground >= p[1] - 1.5
            // Settled feet include the controller's small contact skin.
            && ground <= p[1] + CELL_SIZE + 0.01
            && (position_is_clear(world, p, species)
                || position_is_clear(world, [p[0], p[1] + 0.5, p[2]], species))
        {
            return Some(d);
        }
    }
    None
}

fn walk_step(
    world: &World,
    body: &Body,
    species: Species,
    target: [f32; 3],
    allowed: &impl Fn([f32; 3]) -> bool,
) -> Option<Body> {
    let mut next = body.clone();
    move_animal(
        world,
        &mut next,
        species,
        toward(body.position, target),
        3.,
        false,
        distance(body.position, target) / 3.,
    );
    if distance(next.position, target) > 0.04 {
        return None;
    }
    move_animal(world, &mut next, species, [0., 0.], 0., false, 0.2);
    (next.on_ground
        && allowed(next.position)
        && next.position[1] <= body.position[1] + 0.55
        && next.position[1] >= body.position[1] - 1.5)
        .then_some(next)
}

fn detour(
    world: &World,
    body: &Body,
    species: Species,
    target: [f32; 3],
    allowed: &impl Fn([f32; 3]) -> bool,
) -> VecDeque<[f32; 3]> {
    const RADIUS: i32 = 12;
    const WIDTH: usize = 25;
    const MAX_EXPANSIONS: usize = 192;
    let origin = body.position;
    let length = distance(origin, target);
    let direction = toward(origin, target);
    let goal = if length > 3. {
        [
            origin[0] + direction[0] * 3.,
            origin[1],
            origin[2] + direction[1] * 3.,
        ]
    } else {
        target
    };
    struct Node {
        body: Body,
        cell: [i32; 2],
        cost: f32,
        parent: Option<usize>,
    }
    let mut nodes = vec![Node {
        body: body.clone(),
        cell: [0, 0],
        cost: 0.,
        parent: None,
    }];
    let mut open = vec![0_usize];
    let mut costs = [f32::INFINITY; WIDTH * WIDTH];
    let key = |p: [i32; 2]| (p[1] + RADIUS) as usize * WIDTH + (p[0] + RADIUS) as usize;
    costs[key([0, 0])] = 0.;
    for _ in 0..MAX_EXPANSIONS {
        let Some(slot) = (0..open.len()).min_by(|&a, &b| {
            let score = |i: usize| nodes[i].cost + distance(nodes[i].body.position, goal);
            score(open[a]).total_cmp(&score(open[b]))
        }) else {
            break;
        };
        let index = open.swap_remove(slot);
        let position = nodes[index].body.position;
        if nodes[index].cost > costs[key(nodes[index].cell)] + 0.001 {
            continue;
        }
        if index != 0
            && distance(position, goal) < 0.4
            && (length > 3. || (position[1] - goal[1]).abs() < 0.7)
        {
            let mut path = VecDeque::new();
            let mut current = index;
            while let Some(parent) = nodes[current].parent {
                path.push_front(nodes[current].body.position);
                current = parent;
            }
            return path;
        }
        for [dx, dz] in [
            [1, 0],
            [1, 1],
            [0, 1],
            [-1, 1],
            [-1, 0],
            [-1, -1],
            [0, -1],
            [1, -1],
        ] {
            let cell = [nodes[index].cell[0] + dx, nodes[index].cell[1] + dz];
            if cell.iter().any(|v| v.abs() > RADIUS) {
                continue;
            }
            let next_target = [
                origin[0] + cell[0] as f32 * CELL_SIZE,
                position[1],
                origin[2] + cell[1] as f32 * CELL_SIZE,
            ];
            let step_distance = distance(position, next_target);
            let Some(next) = walk_step(world, &nodes[index].body, species, next_target, allowed)
            else {
                continue;
            };
            let cost = nodes[index].cost + step_distance;
            if cost + 0.001 >= costs[key(cell)] {
                continue;
            }
            costs[key(cell)] = cost;
            nodes.push(Node {
                body: next,
                cell,
                cost,
                parent: Some(index),
            });
            open.push(nodes.len() - 1);
        }
    }
    VecDeque::new()
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}
fn toward(a: [f32; 3], b: [f32; 3]) -> [f32; 2] {
    let length = distance(a, b).max(0.001);
    [(b[0] - a[0]) / length, (b[2] - a[2]) / length]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::world::{Block, BlockPos};
    use std::sync::OnceLock;

    fn sample_world() -> &'static World {
        static WORLD: OnceLock<World> = OnceLock::new();
        WORLD.get_or_init(|| {
            World::generate(42, rubblekin_core::world::WorldGeneration::GeographyV6)
        })
    }

    fn assert_stranded_wolf_progress(position: [f32; 3], target: [f32; 3]) {
        let world = sample_world();
        let mut body = Body::new(position);
        body.on_ground = true;
        let initial = distance(position, target);
        let mut navigation = Navigation::default();
        for _ in 0..600 {
            let steering = navigation.steer(world, &body, Species::Wolf, target, 0.05, true, |p| {
                super::super::habitable(world, p[0], p[2])
            });
            let (direction, speed) =
                steering.map_or(([0., 0.], 0.), |(d, cap)| (d, 2_f32.min(cap)));
            let before = body.position;
            move_animal(
                world,
                &mut body,
                Species::Wolf,
                direction,
                speed,
                false,
                0.05,
            );
            assert!(position_is_clear(world, body.position, Species::Wolf));
            assert!(
                distance(before, body.position) <= 0.11,
                "movement stays swept"
            );
        }
        assert!(
            distance(body.position, target) < initial - 30.,
            "wolf made no sustained progress from {position:?}: {:?}",
            body.position
        );
        assert!(world.edits().is_empty());
    }

    #[test]
    fn observed_uphill_migrant_can_follow_its_half_meter_steps() {
        assert_stranded_wolf_progress(
            [-11_420.261, 184.998, -316.3383],
            [-11179.498, 181., -51.80479],
        );
    }

    #[test]
    fn observed_downhill_migrant_can_follow_supported_edges() {
        assert_stranded_wolf_progress(
            [4_851.615, 206.498, -9_782.635],
            [5127.974, 135.5, -9566.264],
        );
    }

    #[test]
    fn observed_long_downhill_journey_keeps_making_physical_progress() {
        assert_stranded_wolf_progress(
            [4290.1113, 400.998, -9_377.145],
            [4444.7183, 216., -8287.5625],
        );
    }

    fn pad() -> World {
        let mut world = World::new(42);
        for x in -18..=18 {
            for z in -18..=18 {
                for y in 99..=107 {
                    world
                        .set_block(
                            BlockPos::new(x, y, z),
                            if y == 99 { Block::Grass } else { Block::Air },
                        )
                        .unwrap();
                }
            }
        }
        world
    }

    fn dead_end() -> World {
        let mut world = pad();
        for x in -4..=0 {
            for z in -4_i32..=4 {
                if x == 0 || z.abs() == 4 {
                    for y in 100..=104 {
                        world
                            .set_block(BlockPos::new(x, y, z), Block::Stone)
                            .unwrap();
                    }
                }
            }
        }
        world
    }

    #[test]
    fn both_species_back_out_of_a_dead_end_and_reach_the_goal_without_editing_it() {
        let world = dead_end();
        let edits = world.edits().len();
        let target = [2., 50., 0.];
        for species in [Species::Rabbit, Species::Wolf] {
            let mut body = Body::new([-1., 50., 0.]);
            body.on_ground = true;
            let mut navigation = Navigation::default();
            let mut backed_away = false;
            for _ in 0..800 {
                if let Some((d, cap)) =
                    navigation.steer(&world, &body, species, target, 0.05, true, |_| true)
                {
                    move_animal(&world, &mut body, species, d, 2_f32.min(cap), false, 0.05);
                } else {
                    move_animal(&world, &mut body, species, [0., 0.], 0., false, 0.05);
                }
                backed_away |= body.position[0] < -2.5;
                assert!(position_is_clear(&world, body.position, species));
                assert!(body.position[1] >= 49.99);
                if distance(body.position, target) < 0.4 {
                    break;
                }
            }
            assert!(
                backed_away,
                "{species:?} must leave the open back of the obstacle"
            );
            assert!(
                distance(body.position, target) < 0.4,
                "{species:?} stopped at {:?}",
                body.position
            );
        }
        assert_eq!(world.edits().len(), edits);
    }

    #[test]
    fn a_new_obstacle_on_a_cached_route_is_avoided_without_tunnelling_or_editing() {
        let mut world = dead_end();
        let species = Species::Wolf;
        let target = [2., 50., 0.];
        let mut body = Body::new([-1., 50., 0.]);
        body.on_ground = true;
        let mut navigation = Navigation::default();
        let mut obstacle = None;
        for _ in 0..1200 {
            let steering = navigation.steer(&world, &body, species, target, 0.05, true, |_| true);
            if obstacle.is_none()
                && let Some(p) = navigation
                    .path
                    .iter()
                    .find(|p| distance(**p, body.position) > 2. && distance(**p, target) > 1.)
            {
                let cell = BlockPos::new(
                    (p[0] / CELL_SIZE).floor() as i32,
                    100,
                    (p[2] / CELL_SIZE).floor() as i32,
                );
                for y in 100..=104 {
                    world
                        .set_block(BlockPos::new(cell.x, y, cell.z), Block::Stone)
                        .unwrap();
                }
                obstacle = Some((cell, world.edits().len()));
            }
            let (direction, speed) =
                steering.map_or(([0., 0.], 0.), |(d, cap)| (d, 2_f32.min(cap)));
            move_animal(&world, &mut body, species, direction, speed, false, 0.05);
            assert!(position_is_clear(&world, body.position, species));
            assert!(body.position[1] >= 49.99);
            if distance(body.position, target) < 0.4 {
                break;
            }
        }
        let (cell, edits) = obstacle.expect("the original route must exist before the edit");
        assert_eq!(world.block(cell), Block::Stone);
        assert_eq!(world.edits().len(), edits);
        assert!(
            distance(body.position, target) < 0.4,
            "stopped at {:?}",
            body.position
        );
    }

    #[test]
    fn fan_uses_edited_support_and_rejects_an_unsupported_or_forbidden_step() {
        let mut world = pad();
        let body = Body::new([0., 50., 0.]);
        assert!(fan(&world, &body, Species::Wolf, [2., 50., 0.], &|_| true).is_some());
        assert!(fan(&world, &body, Species::Wolf, [2., 50., 0.], &|_| false).is_none());
        for x in -3..=3 {
            for z in -3..=3 {
                for y in 95..=99 {
                    world.set_block(BlockPos::new(x, y, z), Block::Air).unwrap();
                }
            }
        }
        assert!(fan(&world, &body, Species::Wolf, [2., 50., 0.], &|_| true).is_none());
    }

    #[test]
    fn settled_contact_skin_does_not_reject_a_walkable_half_meter_step() {
        let mut world = pad();
        for x in 0..=18 {
            for z in -18..=18 {
                world
                    .set_block(BlockPos::new(x, 100, z), Block::Grass)
                    .unwrap();
            }
        }
        let mut body = Body::new([-0.75, 50., 0.]);
        move_animal(&world, &mut body, Species::Wolf, [0., 0.], 0., false, 0.05);
        assert!(body.on_ground && body.position[1] < 50.);
        let target = [0.25, 50.5, 0.];
        assert_eq!(
            fan(&world, &body, Species::Wolf, target, &|_| true),
            Some([1., 0.])
        );
        for _ in 0..10 {
            move_animal(&world, &mut body, Species::Wolf, [1., 0.], 2., false, 0.05);
            assert!(position_is_clear(&world, body.position, Species::Wolf));
        }
        assert!(body.position[0] > 0.2 && body.position[1] > 50.49);
    }

    #[test]
    fn a_short_waypoint_is_not_rejected_for_a_wall_beyond_it() {
        let mut world = pad();
        for z in -18..=18 {
            for y in 100..=104 {
                world
                    .set_block(BlockPos::new(0, y, z), Block::Stone)
                    .unwrap();
            }
        }
        let body = Body::new([-1.25, 50., 0.]);
        let target = [-0.75, 50., 0.];
        assert!(position_is_clear(&world, target, Species::Wolf));
        assert_eq!(
            fan(&world, &body, Species::Wolf, target, &|_| true),
            Some([1., 0.])
        );
        assert!(
            fan(&world, &body, Species::Wolf, [2., 50., 0.], &|_| true)
                .is_none_or(|d| d != [1., 0.])
        );
    }

    #[test]
    fn removing_support_on_a_cached_route_is_detected_before_the_body_falls() {
        let mut world = dead_end();
        let mut body = Body::new([-1., 50., 0.]);
        body.on_ground = true;
        let target = [2., 50., 0.];
        let mut navigation = Navigation::default();
        let mut changed = false;
        for _ in 0..1200 {
            if !changed
                && let Some(p) = navigation
                    .path
                    .iter()
                    .find(|p| distance(**p, body.position) > 2. && distance(**p, target) > 1.)
            {
                let x = (p[0] / CELL_SIZE).floor() as i32;
                let z = (p[2] / CELL_SIZE).floor() as i32;
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        world
                            .set_block(BlockPos::new(x + dx, 99, z + dz), Block::Air)
                            .unwrap();
                    }
                }
                changed = true;
            }
            let steering =
                navigation.steer(&world, &body, Species::Wolf, target, 0.05, true, |_| true);
            let (direction, speed) =
                steering.map_or(([0., 0.], 0.), |(d, cap)| (d, 2_f32.min(cap)));
            move_animal(
                &world,
                &mut body,
                Species::Wolf,
                direction,
                speed,
                false,
                0.05,
            );
            assert!(position_is_clear(&world, body.position, Species::Wolf));
            assert!(body.position[1] >= 49.99, "cached support was removed");
            if distance(body.position, target) < 0.4 {
                break;
            }
        }
        assert!(changed);
        assert!(
            distance(body.position, target) < 0.4,
            "stopped at {:?}",
            body.position
        );
    }
}
