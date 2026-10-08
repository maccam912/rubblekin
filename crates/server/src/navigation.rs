//! Shared local crowd steering. Jobs own their routes; this owns only a short,
//! disposable walking detour, checked with the ordinary character controller.

use std::collections::VecDeque;

use rubblekin_core::{
    airships::{AirshipNetwork, AirshipRide},
    physics::{Body, MoveInput, move_character_with_airships, move_character_with_obstacles},
    world::{CELL_SIZE, World},
};

const SEARCH_EXPANSIONS_PER_SLICE: usize = 8;
const MAX_SEARCH_EXPANSIONS: usize = 192;

#[derive(Debug, Default, Clone)]
pub(crate) struct Navigation {
    goal: Option<[f32; 3]>,
    progress: Option<([f32; 3], f32)>,
    pub stalled: f32,
    passing_left: bool,
    detour: VecDeque<[f32; 3]>,
    detour_progress: Option<([f32; 3], f32)>,
    detour_stalled: f32,
    retry_in: f32,
    search: Option<DetourSearch>,
    search_wait: f32,
    search_deferred: bool,
}

impl Navigation {
    pub fn is_detouring(&self) -> bool {
        !self.detour.is_empty() && self.detour_stalled < 0.75 && self.stalled < 8.0
    }

    fn needs_search(&self) -> bool {
        self.stalled >= 0.75
            && (self.detour.is_empty() || self.detour_stalled >= 0.75)
            && self.retry_in == 0.0
    }

    /// Give the longest-waiting residents a search slice, preserving iteration
    /// order for ordinary movement. Idle residents consume no search work.
    pub fn limit_searches<'a>(navigation: impl Iterator<Item = &'a mut Self>, limit: usize) {
        let mut navigation: Vec<_> = navigation.collect();
        let mut waiting: Vec<_> = navigation
            .iter()
            .enumerate()
            .filter(|(_, nav)| nav.needs_search())
            .map(|(index, nav)| (index, nav.search_wait))
            .collect();
        waiting.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        waiting.truncate(limit);
        for (index, nav) in navigation.iter_mut().enumerate() {
            nav.search_deferred = !waiting.iter().any(|(chosen, _)| *chosen == index);
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Walker {
    pub body: Body,
    pub ride: Option<AirshipRide>,
    pub local: Option<[f32; 3]>,
}

impl Walker {
    pub fn on_foot(body: &Body) -> Self {
        Self {
            body: body.clone(),
            ride: None,
            local: None,
        }
    }
}

pub(crate) struct Walking<'a> {
    pub world: &'a World,
    pub obstacles: &'a [[f32; 3]],
    pub airships: Option<(&'a AirshipNetwork, f64)>,
}

impl Walking<'_> {
    fn step(&self, walker: &Walker, input: MoveInput, dt: f32) -> Walker {
        let mut next = walker.clone();
        if let Some((network, time)) = self.airships {
            move_character_with_airships(
                self.world,
                &mut next.body,
                input,
                dt,
                self.obstacles,
                network,
                time,
                &mut next.ride,
                &mut next.local,
            );
        } else {
            move_character_with_obstacles(self.world, &mut next.body, input, dt, self.obstacles);
        }
        next
    }

    fn supported(&self, walker: &Walker) -> bool {
        if walker.body.on_ground {
            return true;
        }
        // Ramp descent briefly clears on_ground while gravity catches up.
        let settled = self.step(walker, MoveInput::default(), 0.25);
        settled.body.on_ground
            && (settled.body.position[1] - walker.body.position[1]).abs() <= CELL_SIZE
            && distance(settled.body.position, walker.body.position) < 0.05
    }

    /// Check a short route-gate shortcut with the actual controller and support.
    /// Callers may omit temporary characters while checking terrain, then keep
    /// those characters in the live steering and collision step.
    pub fn has_straight_path(&self, start: &Walker, target: [f32; 3]) -> bool {
        let mut walker = start.clone();
        for _ in 0..40 {
            let remaining = distance(walker.body.position, target);
            if remaining <= 0.03 && (walker.body.position[1] - target[1]).abs() <= 0.15 {
                return true;
            }
            let factor = 0.52_f32.min(remaining / (3.8 * 0.05));
            walker = self.step(
                &walker,
                MoveInput {
                    direction: toward(walker.body.position, target).map(|d| d * factor),
                    ..Default::default()
                },
                0.05,
            );
            if !self.supported(&walker) {
                return false;
            }
        }
        false
    }

    /// The same steering and detour search serves workers, traders and Moss.
    /// A caller can protect a precision approach without duplicating steering.
    #[allow(clippy::too_many_arguments)]
    pub fn walk(
        &self,
        walker: &mut Walker,
        navigation: &mut Navigation,
        target: [f32; 3],
        speed: f32,
        jump: bool,
        dt: f32,
        allowed: impl Fn(&Body) -> bool,
    ) {
        if navigation.goal != Some(target) {
            navigation.goal = Some(target);
            navigation.passing_left = false;
            navigation.detour.clear();
            navigation.detour_progress = None;
            navigation.detour_stalled = 0.0;
            navigation.retry_in = 0.0;
            navigation.search = None;
            navigation.search_wait = 0.0;
        }
        navigation.retry_in = (navigation.retry_in - dt).max(0.0);
        while navigation.detour.front().is_some_and(|p| {
            distance(walker.body.position, *p) < 0.15
                && (walker.body.position[1] - p[1]).abs() < 0.55
        }) {
            navigation.detour.pop_front();
        }
        if navigation.needs_search() {
            navigation.search_wait += dt;
            if !navigation.search_deferred {
                if navigation.search.as_ref().is_none_or(|search| {
                    distance(search.origin, walker.body.position) > 0.15
                        || (search.origin[1] - walker.body.position[1]).abs() > 0.15
                        || search.nodes[0].walker.ride != walker.ride
                }) {
                    navigation.search = Some(DetourSearch::new(walker, target));
                }
                navigation.search_wait = 0.0;
                if let Some(path) =
                    self.advance_search(navigation.search.as_mut().unwrap(), &allowed)
                {
                    navigation.detour = path;
                    navigation.search = None;
                    navigation.retry_in = 2.0;
                    navigation.detour_progress = None;
                    navigation.detour_stalled = 0.0;
                }
            }
        } else {
            navigation.search = None;
            navigation.search_wait = 0.0;
        }
        let steering_target = navigation.detour.front().copied().unwrap_or(target);
        let waiting_for_search = navigation.needs_search();
        let remaining = distance(walker.body.position, steering_target);
        let direction = toward(walker.body.position, steering_target);
        let factor = speed.min(remaining / (3.8 * dt));
        let input = MoveInput {
            direction: if waiting_for_search {
                [0., 0.]
            } else {
                direction.map(|d| d * factor)
            },
            jump: !waiting_for_search && jump && navigation.detour.is_empty(),
            ..Default::default()
        };
        let pilots = self.airships.map_or_else(Vec::new, |(network, time)| {
            network
                .ships(time)
                .iter()
                .map(rubblekin_core::airships::pilot_position)
                .collect()
        });
        let passing = !waiting_for_search
            && remaining > 0.1
            && self.obstacles.iter().chain(&pilots).any(|p| {
                let dx = p[0] - walker.body.position[0];
                let dz = p[2] - walker.body.position[2];
                let along = dx * direction[0] + dz * direction[1];
                (p[1] - walker.body.position[1]).abs() < 1.7
                    && along > 0.0
                    && along < 1.2
                    && (dx * direction[1] - dz * direction[0]).abs() < 0.65
            });
        let before = walker.clone();
        let mut best_step = -1.0_f32;
        if passing {
            let preferred = if navigation.passing_left { -1.0 } else { 1.0 };
            for side in [preferred, -preferred] {
                let passing_input = MoveInput {
                    direction: [
                        (direction[0] - direction[1] * side * 0.8) * factor,
                        (direction[1] + direction[0] * side * 0.8) * factor,
                    ],
                    ..input
                };
                let next = self.step(&before, passing_input, dt);
                let step = distance(before.body.position, next.body.position);
                if step > best_step + 0.001
                    && allowed(&next.body)
                    && (input.jump || self.supported(&next))
                {
                    *walker = next;
                    best_step = step;
                    navigation.passing_left = side < 0.0;
                    if step >= 0.01 {
                        break;
                    }
                }
            }
        }
        if best_step < 0.0 {
            *walker = self.step(&before, input, dt);
            if !passing {
                navigation.passing_left = false;
            }
        }
        // Replanning or moving between detour gates must not hide a failure
        // to reach the real goal. Keep the two progress timers independent.
        record_progress(
            &mut navigation.progress,
            &mut navigation.stalled,
            target,
            before.body.position,
            walker.body.position,
            dt,
        );
        record_progress(
            &mut navigation.detour_progress,
            &mut navigation.detour_stalled,
            steering_target,
            before.body.position,
            walker.body.position,
            dt,
        );
    }

    fn advance_search(
        &self,
        search: &mut DetourSearch,
        allowed: &impl Fn(&Body) -> bool,
    ) -> Option<VecDeque<[f32; 3]>> {
        // Keep the same grid/192-node search, amortizing its real physics
        // probes across ticks instead of blocking the entire server loop.
        const RADIUS: i32 = 8;
        const WIDTH: usize = 17;
        let key =
            |cell: [i32; 2]| ((cell[1] + RADIUS) as usize * WIDTH) + (cell[0] + RADIUS) as usize;
        for _ in 0..SEARCH_EXPANSIONS_PER_SLICE {
            if search.expansions >= MAX_SEARCH_EXPANSIONS {
                return Some(VecDeque::new());
            }
            search.expansions += 1;
            let Some((slot, _)) = search.open.iter().enumerate().min_by(|(_, a), (_, b)| {
                let score = |index: usize| {
                    search.nodes[index].cost
                        + distance(search.nodes[index].walker.body.position, search.goal)
                };
                score(**a).total_cmp(&score(**b))
            }) else {
                return Some(VecDeque::new());
            };
            let index = search.open.swap_remove(slot);
            let position = search.nodes[index].walker.body.position;
            if search.nodes[index].cost > search.costs[key(search.nodes[index].cell)] + 0.001 {
                continue;
            }
            if index != 0
                && distance(position, search.goal) <= 0.35
                && (search.length > 3.0 || (position[1] - search.goal[1]).abs() < 0.55)
            {
                let mut path = VecDeque::new();
                let mut current = index;
                while let Some(parent) = search.nodes[current].parent {
                    path.push_front(search.nodes[current].walker.body.position);
                    current = parent;
                }
                return Some(path);
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
                let cell = [
                    search.nodes[index].cell[0] + dx,
                    search.nodes[index].cell[1] + dz,
                ];
                if cell.iter().any(|v| v.abs() > RADIUS) {
                    continue;
                }
                let next_target = [
                    search.origin[0] + cell[0] as f32 * CELL_SIZE,
                    position[1],
                    search.origin[2] + cell[1] as f32 * CELL_SIZE,
                ];
                let step_distance = distance(position, next_target);
                let input = MoveInput {
                    direction: toward(position, next_target),
                    ..Default::default()
                };
                let next = self.step(&search.nodes[index].walker, input, step_distance / 3.8);
                if distance(next.body.position, next_target) > 0.04
                    || !self.supported(&next)
                    || !allowed(&next.body)
                    || (next.body.position[1] - position[1]).abs() > CELL_SIZE + 0.05
                {
                    continue;
                }
                let cost = search.nodes[index].cost + step_distance;
                if cost + 0.001 >= search.costs[key(cell)] {
                    continue;
                }
                search.costs[key(cell)] = cost;
                search.nodes.push(SearchNode {
                    walker: next,
                    cell,
                    cost,
                    parent: Some(index),
                });
                search.open.push(search.nodes.len() - 1);
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
struct SearchNode {
    walker: Walker,
    cell: [i32; 2],
    cost: f32,
    parent: Option<usize>,
}

#[derive(Debug, Clone)]
struct DetourSearch {
    origin: [f32; 3],
    goal: [f32; 3],
    length: f32,
    nodes: Vec<SearchNode>,
    open: Vec<usize>,
    costs: [f32; 17 * 17],
    expansions: usize,
}

impl DetourSearch {
    fn new(start: &Walker, target: [f32; 3]) -> Self {
        let origin = start.body.position;
        let length = distance(origin, target);
        let direction = toward(origin, target);
        let goal = if length > 3.0 {
            [
                origin[0] + direction[0] * 3.0,
                origin[1],
                origin[2] + direction[1] * 3.0,
            ]
        } else {
            target
        };
        let mut costs = [f32::INFINITY; 17 * 17];
        costs[8 * 17 + 8] = 0.0;
        Self {
            origin,
            goal,
            length,
            nodes: vec![SearchNode {
                walker: start.clone(),
                cell: [0, 0],
                cost: 0.,
                parent: None,
            }],
            open: vec![0],
            costs,
            expansions: 0,
        }
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn record_progress(
    progress: &mut Option<([f32; 3], f32)>,
    stalled: &mut f32,
    target: [f32; 3],
    before: [f32; 3],
    after: [f32; 3],
    dt: f32,
) {
    let (old_target, checkpoint) = progress.get_or_insert((target, distance(before, target)));
    let remaining = distance(after, target);
    if *old_target != target || *checkpoint - remaining >= 0.1 {
        *old_target = target;
        *checkpoint = remaining;
        *stalled = 0.0;
    } else {
        *stalled = (*stalled + dt).min(10.0);
    }
}

fn toward(a: [f32; 3], b: [f32; 3]) -> [f32; 2] {
    let length = distance(a, b).max(0.001);
    [(b[0] - a[0]) / length, (b[2] - a[2]) / length]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::{
        physics::character_position_is_clear,
        world::{Block, BlockPos},
    };

    fn flat_path() -> World {
        let mut world = World::new(42);
        for x in -12..=12 {
            for z in -12..=12 {
                world
                    .set_block(BlockPos::new(x, 39, z), Block::Brick)
                    .unwrap();
                for y in 40..=44 {
                    world.set_block(BlockPos::new(x, y, z), Block::Air).unwrap();
                }
            }
        }
        world
    }

    #[test]
    fn short_route_gate_probe_rejects_a_wall_and_an_unsupported_drop() {
        let mut world = flat_path();
        let start = Walker::on_foot(&Body::new([-1.75, 20.0, 0.25]));
        let target = [1.25, 20.0, 0.25];
        let probe = |world: &World| {
            Walking {
                world,
                obstacles: &[],
                airships: None,
            }
            .has_straight_path(&start, target)
        };
        assert!(probe(&world));
        for y in 40..=44 {
            world
                .set_block(BlockPos::new(0, y, 0), Block::Brick)
                .unwrap();
        }
        assert!(
            !probe(&world),
            "an occupied gate must not skip a route corner through a wall"
        );
        for y in 36..=44 {
            for x in 0..=2 {
                for z in -2..=2 {
                    world.set_block(BlockPos::new(x, y, z), Block::Air).unwrap();
                }
            }
        }
        assert!(!probe(&world), "a shortcut needs continuous support");
    }

    #[test]
    fn opposing_walkers_pass_each_other_on_an_ordinary_path() {
        let world = flat_path();
        let targets = [[3.25, 20.0, 0.25], [-2.75, 20.0, 0.25]];
        for dt in [0.05, 0.25] {
            let mut walkers = targets.map(|p| Walker::on_foot(&Body::new(p)));
            walkers.swap(0, 1);
            let mut navigation = [Navigation::default(), Navigation::default()];
            let mut left_route = false;
            for _ in 0..400 {
                for i in 0..2 {
                    let obstacles = [walkers[1 - i].body.position];
                    Walking {
                        world: &world,
                        obstacles: &obstacles,
                        airships: None,
                    }
                    .walk(
                        &mut walkers[i],
                        &mut navigation[i],
                        targets[i],
                        0.52,
                        false,
                        dt,
                        |_| true,
                    );
                    assert!(character_position_is_clear(
                        &world,
                        walkers[i].body.position,
                        &obstacles
                    ));
                    left_route |= (walkers[i].body.position[2] - 0.25).abs() > 0.3;
                }
                if (0..2).all(|i| distance(walkers[i].body.position, targets[i]) < 0.15) {
                    break;
                }
            }
            assert!(left_route);
            for i in 0..2 {
                assert!(
                    distance(walkers[i].body.position, targets[i]) < 0.15,
                    "dt={dt} walker={i} position={:?}",
                    walkers[i].body.position
                );
            }
        }
    }

    #[test]
    fn blocked_walkers_share_search_slices_without_starving_later_residents() {
        let world = flat_path();
        let start = [0.25, 20., 0.25];
        let target = [3.25, 20., 0.25];
        let mut obstacles = vec![[1.05, 20., 0.25]];
        for x in [-0.55, 0.05, 0.65, 1.25] {
            obstacles.push([x, 20., -0.55]);
            obstacles.push([x, 20., 1.05]);
        }
        let walking = Walking {
            world: &world,
            obstacles: &obstacles,
            airships: None,
        };
        let mut walkers = std::array::from_fn::<_, 4, _>(|_| Walker::on_foot(&Body::new(start)));
        let mut navigation = std::array::from_fn::<_, 4, _>(|_| Navigation {
            goal: Some(target),
            stalled: 1.,
            ..Default::default()
        });
        let mut served = [false; 4];
        for _ in 0..4 {
            Navigation::limit_searches(navigation.iter_mut(), 2);
            assert_eq!(
                navigation.iter().filter(|nav| !nav.search_deferred).count(),
                2
            );
            let mut expanded = 0;
            for (index, (walker, nav)) in walkers.iter_mut().zip(&mut navigation).enumerate() {
                let before = nav.search.as_ref().map_or(0, |search| search.expansions);
                walking.walk(walker, nav, target, 0.52, false, 0.05, |_| true);
                let after = nav.search.as_ref().map_or(0, |search| search.expansions);
                assert!(after - before <= SEARCH_EXPANSIONS_PER_SLICE);
                expanded += after - before;
                served[index] |= after > 0;
                assert!(character_position_is_clear(
                    &world,
                    walker.body.position,
                    &obstacles
                ));
            }
            assert_eq!(expanded, 2 * SEARCH_EXPANSIONS_PER_SLICE);
        }
        assert!(served.into_iter().all(|served| served));
    }

    #[test]
    fn terrain_changed_during_a_search_still_blocks_motion_and_removal_allows_recovery() {
        let mut world = flat_path();
        let start = [0.25, 20., 0.25];
        let target = [3.25, 20., 0.25];
        let mut obstacles = vec![[1.05, 20., 0.25]];
        for x in [-0.55, 0.05, 0.65, 1.25] {
            obstacles.push([x, 20., -0.55]);
            obstacles.push([x, 20., 1.05]);
        }
        let mut walker = Walker::on_foot(&Body::new(start));
        let mut nav = Navigation {
            goal: Some(target),
            stalled: 1.,
            ..Default::default()
        };
        Walking {
            world: &world,
            obstacles: &obstacles,
            airships: None,
        }
        .walk(&mut walker, &mut nav, target, 0.52, false, 0.05, |_| true);
        assert!(
            nav.search.is_some(),
            "the search must span more than one tick"
        );
        let original = world.edits();
        for y in 40..=44 {
            for z in -2..=2 {
                world
                    .set_block(BlockPos::new(-2, y, z), Block::Stone)
                    .unwrap();
            }
        }
        for _ in 0..100 {
            Walking {
                world: &world,
                obstacles: &obstacles,
                airships: None,
            }
            .walk(&mut walker, &mut nav, target, 0.52, false, 0.05, |_| true);
            assert!(character_position_is_clear(
                &world,
                walker.body.position,
                &obstacles
            ));
            assert!(distance(start, walker.body.position) < 0.6);
        }
        for y in 40..=44 {
            for z in -2..=2 {
                world
                    .set_block(BlockPos::new(-2, y, z), Block::Air)
                    .unwrap();
            }
        }
        for _ in 0..500 {
            Walking {
                world: &world,
                obstacles: &obstacles,
                airships: None,
            }
            .walk(&mut walker, &mut nav, target, 0.52, false, 0.05, |_| true);
            assert!(character_position_is_clear(
                &world,
                walker.body.position,
                &obstacles
            ));
            if distance(walker.body.position, target) < 0.15 {
                break;
            }
        }
        assert!(distance(walker.body.position, target) < 0.15);
        assert_eq!(world.edits(), original);
    }

    #[test]
    fn crowd_detour_can_start_away_from_the_goal_then_rejoin_on_plain_ground() {
        let world = flat_path();
        // A U-shaped crowd opens behind the walker. Sidestepping toward the
        // goal cannot escape: the first useful move must be backwards.
        let mut obstacles = vec![[1.05, 20.0, 0.25]];
        for x in [-0.55, 0.05, 0.65, 1.25] {
            obstacles.push([x, 20.0, -0.55]);
            obstacles.push([x, 20.0, 1.05]);
        }
        let walking = Walking {
            world: &world,
            obstacles: &obstacles,
            airships: None,
        };
        for dt in [0.05, 0.25] {
            let start = [0.25, 20.0, 0.25];
            let target = [3.25, 20.0, 0.25];
            let mut walker = Walker::on_foot(&Body::new(start));
            let mut navigation = Navigation::default();
            let mut backed_up = false;
            let mut took_detour = false;
            for _ in 0..1_200 {
                let before = walker.body.position;
                walking.walk(
                    &mut walker,
                    &mut navigation,
                    target,
                    0.52,
                    false,
                    dt,
                    |_| true,
                );
                backed_up |= walker.body.position[0] < start[0] - 0.7;
                took_detour |= !navigation.detour.is_empty();
                assert!(character_position_is_clear(
                    &world,
                    walker.body.position,
                    &obstacles
                ));
                assert!(distance(before, walker.body.position) <= 3.0 * dt + 0.03);
                assert!((walker.body.position[1] - 20.0).abs() < 0.01);
                if distance(walker.body.position, target) < 0.15 {
                    break;
                }
            }
            assert!(
                backed_up && took_detour && distance(walker.body.position, target) < 0.15,
                "dt={dt} position={:?} navigation={navigation:?}",
                walker.body.position
            );
        }
    }

    #[test]
    fn enclosing_crowd_waits_safely_and_recovers_when_someone_moves() {
        let world = flat_path();
        let start = [0.25, 20.0, 0.25];
        let target = [3.25, 20.0, 0.25];
        let obstacles: Vec<_> = [-1, 0, 1]
            .into_iter()
            .flat_map(|x| {
                [-1, 0, 1]
                    .into_iter()
                    .filter(move |z| x != 0 || *z != 0)
                    .map(move |z| [start[0] + x as f32 * 0.6, 20.0, start[2] + z as f32 * 0.6])
            })
            .collect();
        let mut walker = Walker::on_foot(&Body::new(start));
        let mut navigation = Navigation::default();
        for _ in 0..200 {
            Walking {
                world: &world,
                obstacles: &obstacles,
                airships: None,
            }
            .walk(
                &mut walker,
                &mut navigation,
                target,
                0.52,
                false,
                0.05,
                |_| true,
            );
            assert!(character_position_is_clear(
                &world,
                walker.body.position,
                &obstacles
            ));
            assert!(distance(walker.body.position, start) < 0.2);
        }
        assert!(navigation.stalled >= 8.0);
        assert!(navigation.detour.is_empty());
        let obstacles: Vec<_> = obstacles
            .into_iter()
            .filter(|p| p[0] < start[0] + 0.5)
            .collect();
        for _ in 0..200 {
            Walking {
                world: &world,
                obstacles: &obstacles,
                airships: None,
            }
            .walk(
                &mut walker,
                &mut navigation,
                target,
                0.52,
                false,
                0.05,
                |_| true,
            );
            assert!(character_position_is_clear(
                &world,
                walker.body.position,
                &obstacles
            ));
            if distance(walker.body.position, target) < 0.15 {
                break;
            }
        }
        assert!(distance(walker.body.position, target) < 0.15);
        assert!(navigation.stalled < 1.0);
    }
}
