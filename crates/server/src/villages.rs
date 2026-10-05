//! Small resident-owned work cycles. Goods enter storage only after delivery;
//! a trader withdraws actual surplus and carries it along a generated trail.
use rubblekin_core::{
    physics::{Body, MoveInput, move_character},
    protocol::{ResidentAction, ResidentRole, ResidentSnapshot, ResourceCargo, VillageSnapshot},
    settlement::{ResourceKind, SettlementPlan, Village},
    world::{Block, BlockPos, CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};

pub(crate) const MAX_RESIDENTS: usize = 60;
const MAX_STOCK: f32 = 10_000.0;
const ARRIVAL: f32 = 0.45;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct VillageLife {
    villages: Vec<Economy>,
    residents: Vec<Resident>,
    /// Rebuilt on load; soil validity belongs to edited terrain, not the save.
    #[serde(skip)]
    soil_check_remaining: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Economy {
    snapshot: VillageSnapshot,
    /// Finite accessible deposits. Food is renewed by a cultivated crop cycle.
    remaining: [f32; 5],
    harvests: u64,
    deliveries: u64,
    trade_deliveries: u64,
    #[serde(skip)]
    cultivated_fraction: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    ToWork,
    Working,
    ToStore,
    ToHome,
    Resting,
    ToTradeStore,
    Loading,
    ToTrade,
    Unloading,
    Returning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Resident {
    snapshot: ResidentSnapshot,
    body: Body,
    village: usize,
    route: usize,
    phase: Phase,
    waypoint: usize,
    elapsed: f32,
    stuck: f32,
    trail: Option<usize>,
}

impl VillageLife {
    pub fn new(world: &World) -> Self {
        let Some(plan) = world.settlements() else {
            return Self::default();
        };
        let mut life = Self::default();
        for (village_index, village) in plan.villages.iter().enumerate() {
            let population = village.resident_routes.len().min(6);
            life.villages.push(Economy {
                snapshot: VillageSnapshot {
                    id: village.id,
                    food: 100.0 + village.resources.farming * 20.0,
                    timber: 24.0 + village.resources.timber * 20.0,
                    stone: 18.0 + village.resources.stone * 20.0,
                    clay: 8.0 + village.resources.clay * 20.0,
                    iron: 4.0 + village.resources.iron * 20.0,
                    crop_growth: 0.45,
                    population: population as u32,
                    housing_capacity: population as u32,
                    food_reserve: population as f32 * 8.0,
                    capacity_for_growth: false,
                },
                remaining: [
                    0.0,
                    village.resources.timber * 2000.0,
                    village.resources.stone * 3000.0,
                    village.resources.clay * 2000.0,
                    village.resources.iron * 1500.0,
                ],
                harvests: 0,
                deliveries: 0,
                trade_deliveries: 0,
                cultivated_fraction: 0.0,
            });
            for (route_index, route) in village.resident_routes.iter().take(6).enumerate() {
                if life.residents.len() == MAX_RESIDENTS {
                    break;
                }
                let trail = (route_index == population.saturating_sub(1))
                    .then(|| {
                        plan.trails
                            .iter()
                            .position(|trail| trail.from == village.id || trail.to == village.id)
                    })
                    .flatten();
                let role = if trail.is_some() {
                    ResidentRole::Trader
                } else {
                    match route.resource {
                        ResourceKind::Food => ResidentRole::Farmer,
                        ResourceKind::Timber => ResidentRole::Woodcutter,
                        ResourceKind::Stone | ResourceKind::Clay => ResidentRole::Quarrier,
                        ResourceKind::Iron => ResidentRole::Miner,
                    }
                };
                life.residents.push(Resident {
                    snapshot: ResidentSnapshot {
                        id: ((village.id as u64) << 8) | (route_index + 1) as u64,
                        village_id: village.id,
                        name: format!(
                            "{} {}",
                            ["Alder", "Fern", "Rowan", "Hazel", "Briar", "Willow"][route_index],
                            [
                                "Reed", "Brook", "Hill", "Oak", "Vale", "Ash", "Stone", "Moss",
                                "Pine", "Bell"
                            ][(village.id as usize + route_index) % 10]
                        ),
                        position: route.home,
                        role,
                        action: ResidentAction::Walking,
                        target: Some(route.home),
                        carrying: None,
                    },
                    body: Body::new(route.home),
                    village: village_index,
                    route: route_index,
                    phase: if trail.is_some() {
                        Phase::ToTradeStore
                    } else {
                        Phase::ToWork
                    },
                    waypoint: 0,
                    elapsed: 0.0,
                    stuck: 0.0,
                    trail,
                });
            }
        }
        life
    }

    pub fn residents(&self) -> Vec<ResidentSnapshot> {
        self.residents
            .iter()
            .map(|resident| resident.snapshot.clone())
            .collect()
    }

    pub fn villages(&self) -> Vec<VillageSnapshot> {
        self.villages
            .iter()
            .map(|village| village.snapshot.clone())
            .collect()
    }

    pub fn positions(&self) -> impl Iterator<Item = [f32; 3]> + '_ {
        self.residents.iter().map(|resident| resident.body.position)
    }

    pub fn validate(&self, world: &World) -> bool {
        let expected = Self::new(world);
        if self.villages.len() != expected.villages.len()
            || self.residents.len() != expected.residents.len()
            || self.residents.len() > MAX_RESIDENTS
        {
            return false;
        }
        for (economy, expected) in self.villages.iter().zip(&expected.villages) {
            let value = &economy.snapshot;
            if value.id != expected.snapshot.id
                || value.population != expected.snapshot.population
                || value.housing_capacity != expected.snapshot.housing_capacity
                || value.food_reserve != expected.snapshot.food_reserve
                || ![
                    value.food,
                    value.timber,
                    value.stone,
                    value.clay,
                    value.iron,
                ]
                .iter()
                .all(|v| v.is_finite() && (0.0..=MAX_STOCK).contains(v))
                || !value.crop_growth.is_finite()
                || !(0.0..=1.0).contains(&value.crop_growth)
                || !economy
                    .remaining
                    .iter()
                    .zip(expected.remaining)
                    .all(|(v, initial)| v.is_finite() && (0.0..=initial).contains(v))
            {
                return false;
            }
        }
        let Some(plan) = world.settlements() else {
            return true;
        };
        for (resident, expected) in self.residents.iter().zip(&expected.residents) {
            let local = &plan.villages[expected.village].resident_routes[expected.route];
            let waypoint_limit = if matches!(resident.phase, Phase::ToTrade | Phase::Returning) {
                expected.trail.map_or(0, |i| plan.trails[i].points.len())
            } else {
                local.path.len()
            };
            let valid_station = match resident.phase {
                Phase::Working => arrived(resident.body.position, local.work),
                Phase::Resting => arrived(resident.body.position, local.home),
                Phase::Loading => arrived(resident.body.position, local.path[local.store_index]),
                Phase::Unloading => expected.trail.is_some_and(|i| {
                    let trail = &plan.trails[i];
                    let end = if trail.from == expected.snapshot.village_id {
                        trail.points.len() - 1
                    } else {
                        0
                    };
                    arrived(resident.body.position, trail.points[end])
                }),
                _ => true,
            };
            if resident.village != expected.village
                || resident.route != expected.route
                || resident.trail != expected.trail
                || resident.snapshot.id != expected.snapshot.id
                || resident.snapshot.village_id != expected.snapshot.village_id
                || resident.snapshot.role != expected.snapshot.role
                || resident.snapshot.name.trim().is_empty()
                || resident.snapshot.name.chars().count() > 48
                || resident.snapshot.name.chars().any(char::is_control)
                || resident.snapshot.position != resident.body.position
                || !resident
                    .body
                    .position
                    .iter()
                    .chain(&resident.body.velocity)
                    .all(|v| v.is_finite())
                || resident.body.velocity.iter().any(|v| v.abs() > 40.0)
                || !world.contains_block(rubblekin_core::world::BlockPos::new(
                    (resident.body.position[0] / 0.5).floor() as i32,
                    (resident.body.position[1] / 0.5).floor() as i32,
                    (resident.body.position[2] / 0.5).floor() as i32,
                ))
                || resident
                    .snapshot
                    .target
                    .is_some_and(|p| !p.iter().all(|v| v.is_finite()))
                || resident.snapshot.carrying.as_ref().is_some_and(|cargo| {
                    !cargo.amount.is_finite() || !(0.0..=12.0).contains(&cargo.amount)
                })
                || !resident.elapsed.is_finite()
                || !(0.0..=60.0).contains(&resident.elapsed)
                || !resident.stuck.is_finite()
                || !(0.0..=10.0).contains(&resident.stuck)
                || resident.waypoint >= waypoint_limit
                || !valid_station
                || resident.phase == Phase::ToStore && resident.waypoint < local.store_index
                || matches!(resident.phase, Phase::ToHome | Phase::ToTradeStore)
                    && resident.waypoint > local.store_index
                || resident.trail.is_none()
                    && matches!(
                        resident.phase,
                        Phase::ToTradeStore
                            | Phase::Loading
                            | Phase::ToTrade
                            | Phase::Unloading
                            | Phase::Returning
                    )
            {
                return false;
            }
        }
        true
    }

    pub fn tick(&mut self, world: &World, dt: f32) {
        let Some(plan) = world.settlements() else {
            return;
        };
        self.soil_check_remaining -= dt;
        if self.soil_check_remaining <= 0.0 {
            for (economy, village) in self.villages.iter_mut().zip(&plan.villages) {
                economy.cultivated_fraction = cultivated_fraction(world, village);
                if economy.cultivated_fraction == 0.0 {
                    // Destroying a whole field destroys its crop. Restoring
                    // soil starts a new crop instead of reviving a mature one.
                    economy.snapshot.crop_growth = 0.0;
                }
            }
            self.soil_check_remaining = 1.0;
        }
        for (index, economy) in self.villages.iter_mut().enumerate() {
            let snapshot = &mut economy.snapshot;
            snapshot.food = (snapshot.food - snapshot.population as f32 * dt / 120.0).max(0.0);
            snapshot.crop_growth = (snapshot.crop_growth
                + dt * (0.5 + plan.villages[index].resources.farming)
                    * economy.cultivated_fraction
                    / 90.0)
                .min(1.0);
            // Capacity is explicit; new homes or people are not silently spawned.
            snapshot.capacity_for_growth = snapshot.food > snapshot.food_reserve * 2.0
                && snapshot.population < snapshot.housing_capacity;
        }
        for resident in &mut self.residents {
            let village = &plan.villages[resident.village];
            let route = &village.resident_routes[resident.route];
            let economy = &mut self.villages[resident.village];
            // Digging out a work/home floor must make the inhabitant fall too.
            // Grounded stationary residents need only one support lookup.
            if matches!(resident.phase, Phase::Working | Phase::Resting) {
                let p = resident.body.position;
                let support = BlockPos::new(
                    (p[0] / CELL_SIZE).floor() as i32,
                    ((p[1] - 0.05) / CELL_SIZE).floor() as i32,
                    (p[2] / CELL_SIZE).floor() as i32,
                );
                if !resident.body.on_ground || world.block(support) == Block::Air {
                    move_character(world, &mut resident.body, MoveInput::default(), dt);
                    resident.snapshot.position = resident.body.position;
                    if !arrived(
                        resident.body.position,
                        resident.snapshot.target.unwrap_or(route.home),
                    ) {
                        resident.phase = if resident.phase == Phase::Working {
                            Phase::ToWork
                        } else {
                            Phase::ToHome
                        };
                        resident.waypoint = if resident.phase == Phase::ToWork {
                            route.path.len() - 1
                        } else {
                            0
                        };
                        resident.elapsed = 0.0;
                    }
                }
            }
            match resident.phase {
                Phase::Working => {
                    resident.snapshot.action = ResidentAction::Working;
                    resident.snapshot.target = Some(route.work);
                    if !arrived(resident.body.position, route.work) {
                        continue;
                    }
                    resident.elapsed += dt;
                    if resident.elapsed < 6.0 {
                        continue;
                    }
                    let amount = if route.resource == ResourceKind::Food {
                        if economy.snapshot.crop_growth < 1.0 {
                            resident.elapsed = 6.0;
                            continue;
                        }
                        // An edit can arrive between periodic checks. Count
                        // actual planted soil again before creating any food.
                        economy.cultivated_fraction = cultivated_fraction(world, village);
                        if economy.cultivated_fraction == 0.0 {
                            economy.snapshot.crop_growth = 0.0;
                            resident.elapsed = 6.0;
                            continue;
                        }
                        economy.snapshot.crop_growth = 0.0;
                        economy.harvests = economy.harvests.saturating_add(1);
                        12.0 * economy.cultivated_fraction
                    } else {
                        let remaining = &mut economy.remaining[resource_index(route.resource)];
                        let amount = remaining.min(4.0);
                        *remaining -= amount;
                        amount
                    };
                    resident.snapshot.carrying = (amount > 0.0).then_some(ResourceCargo {
                        kind: route.resource,
                        amount,
                    });
                    resident.phase = Phase::ToStore;
                    resident.waypoint = route.path.len() - 1;
                    resident.elapsed = 0.0;
                }
                Phase::Resting => {
                    resident.snapshot.action = ResidentAction::Resting;
                    resident.snapshot.target = Some(route.home);
                    resident.elapsed += dt;
                    if resident.elapsed >= 8.0 {
                        resident.phase = if resident.trail.is_some() {
                            Phase::ToTradeStore
                        } else {
                            Phase::ToWork
                        };
                        resident.waypoint = 0;
                        resident.elapsed = 0.0;
                    }
                }
                Phase::Loading | Phase::Unloading => {}
                _ => advance_resident(world, plan, resident, dt),
            }
            if resident.phase == Phase::ToHome
                && arrived(resident.body.position, route.path[route.store_index])
                && let Some(cargo) = resident.snapshot.carrying.take()
            {
                let amount = deposit(&mut economy.snapshot, &cargo);
                if amount < cargo.amount {
                    resident.snapshot.carrying = Some(ResourceCargo {
                        amount: cargo.amount - amount,
                        ..cargo
                    });
                    resident.phase = Phase::ToStore;
                    resident.waypoint = route.store_index;
                }
                if amount > 0.0 {
                    economy.deliveries = economy.deliveries.saturating_add(1);
                }
            }
            resident.snapshot.position = resident.body.position;
        }
        // Separate phase so each transfer has one clear source and destination.
        for resident in &mut self.residents {
            if !matches!(resident.phase, Phase::Loading | Phase::Unloading) {
                continue;
            }
            let trail = &plan.trails[resident.trail.unwrap()];
            let id = plan.villages[resident.village].id;
            let partner_id = if trail.from == id {
                trail.to
            } else {
                trail.from
            };
            let partner = plan
                .villages
                .iter()
                .position(|v| v.id == partner_id)
                .unwrap();
            resident.snapshot.action = ResidentAction::Trading;
            if resident.phase == Phase::Loading {
                let destination = self.villages[partner].snapshot.clone();
                if let Some(cargo) = take_surplus(
                    &mut self.villages[resident.village].snapshot,
                    &destination,
                    None,
                ) {
                    resident.snapshot.carrying = Some(cargo);
                    resident.phase = Phase::ToTrade;
                    resident.waypoint = if trail.from == id {
                        0
                    } else {
                        trail.points.len() - 1
                    };
                } else {
                    resident.phase = Phase::ToHome;
                    resident.waypoint =
                        plan.villages[resident.village].resident_routes[resident.route].store_index;
                }
            } else {
                let outward = resident.snapshot.carrying.as_ref().map(|c| c.kind);
                if let Some(cargo) = resident.snapshot.carrying.take() {
                    let accepted = deposit(&mut self.villages[partner].snapshot, &cargo);
                    if accepted < cargo.amount {
                        resident.snapshot.carrying = Some(ResourceCargo {
                            amount: cargo.amount - accepted,
                            ..cargo
                        });
                    }
                    if accepted > 0.0 {
                        self.villages[partner].trade_deliveries =
                            self.villages[partner].trade_deliveries.saturating_add(1);
                    }
                }
                if resident.snapshot.carrying.is_none() {
                    let destination = self.villages[resident.village].snapshot.clone();
                    resident.snapshot.carrying =
                        take_surplus(&mut self.villages[partner].snapshot, &destination, outward);
                }
                resident.phase = Phase::Returning;
                resident.waypoint = if trail.from == id {
                    trail.points.len() - 1
                } else {
                    0
                };
            }
        }
    }
}

fn advance_resident(world: &World, plan: &SettlementPlan, resident: &mut Resident, dt: f32) {
    let village = &plan.villages[resident.village];
    let route = &village.resident_routes[resident.route];
    let (points, end, forward) = match resident.phase {
        Phase::ToWork => (&route.path, route.path.len() - 1, true),
        Phase::ToStore => (&route.path, route.store_index, false),
        Phase::ToHome => (&route.path, 0, false),
        Phase::ToTradeStore => (&route.path, route.store_index, true),
        Phase::ToTrade | Phase::Returning => {
            let trail = &plan.trails[resident.trail.unwrap()];
            let forward = (trail.from == village.id) == (resident.phase == Phase::ToTrade);
            (
                &trail.points,
                if forward { trail.points.len() - 1 } else { 0 },
                forward,
            )
        }
        _ => return,
    };
    let target = points[resident.waypoint];
    resident.snapshot.target = Some(target);
    resident.snapshot.action = if resident.stuck >= 8.0 {
        ResidentAction::Blocked
    } else if resident.snapshot.carrying.is_some() {
        ResidentAction::Delivering
    } else {
        ResidentAction::Walking
    };
    if arrived(resident.body.position, target) {
        resident.stuck = 0.0;
        if resident.waypoint != end {
            resident.waypoint = if forward {
                resident.waypoint + 1
            } else {
                resident.waypoint - 1
            };
        } else {
            resident.phase = match resident.phase {
                Phase::ToWork => Phase::Working,
                Phase::ToStore => Phase::ToHome,
                Phase::ToHome => Phase::Resting,
                Phase::ToTradeStore => Phase::Loading,
                Phase::ToTrade => Phase::Unloading,
                Phase::Returning => Phase::ToHome,
                _ => unreachable!(),
            };
            if resident.phase == Phase::ToHome {
                resident.waypoint = route.store_index;
            }
            resident.elapsed = 0.0;
        }
        return;
    }
    let distance = horizontal_distance(resident.body.position, target);
    let mut input = MoveInput::default();
    if distance > 0.03 {
        // Slow down for the final step so a long tick cannot overshoot a gate.
        let factor = 0.52_f32.min(distance / (3.8 * dt));
        input.direction = [
            (target[0] - resident.body.position[0]) / distance * factor,
            (target[2] - resident.body.position[2]) / distance * factor,
        ];
    }
    let old = resident.body.position;
    move_character(world, &mut resident.body, input, dt);
    if horizontal_distance(old, resident.body.position) < 0.01 {
        resident.stuck = (resident.stuck + dt).min(10.0);
    } else {
        resident.stuck = 0.0;
    }
}

fn resource_index(kind: ResourceKind) -> usize {
    match kind {
        ResourceKind::Food => 0,
        ResourceKind::Timber => 1,
        ResourceKind::Stone => 2,
        ResourceKind::Clay => 3,
        ResourceKind::Iron => 4,
    }
}

fn cultivated_fraction(world: &World, village: &Village) -> f32 {
    let mut planted = 0_u32;
    let mut cultivated = 0_u32;
    for field in &village.fields {
        for position in field.plant_positions() {
            planted += 1;
            if matches!(world.block(position), Block::Dirt | Block::Grass) {
                cultivated += 1;
            }
        }
    }
    if planted == 0 {
        0.0
    } else {
        cultivated as f32 / planted as f32
    }
}
fn stock(snapshot: &VillageSnapshot, kind: ResourceKind) -> f32 {
    match kind {
        ResourceKind::Food => snapshot.food,
        ResourceKind::Timber => snapshot.timber,
        ResourceKind::Stone => snapshot.stone,
        ResourceKind::Clay => snapshot.clay,
        ResourceKind::Iron => snapshot.iron,
    }
}
fn stock_mut(snapshot: &mut VillageSnapshot, kind: ResourceKind) -> &mut f32 {
    match kind {
        ResourceKind::Food => &mut snapshot.food,
        ResourceKind::Timber => &mut snapshot.timber,
        ResourceKind::Stone => &mut snapshot.stone,
        ResourceKind::Clay => &mut snapshot.clay,
        ResourceKind::Iron => &mut snapshot.iron,
    }
}
fn deposit(snapshot: &mut VillageSnapshot, cargo: &ResourceCargo) -> f32 {
    let storage = stock_mut(snapshot, cargo.kind);
    let accepted = cargo.amount.min(MAX_STOCK - *storage);
    *storage += accepted;
    accepted
}
fn take_surplus(
    source: &mut VillageSnapshot,
    destination: &VillageSnapshot,
    exclude: Option<ResourceKind>,
) -> Option<ResourceCargo> {
    let kind = [
        ResourceKind::Food,
        ResourceKind::Timber,
        ResourceKind::Stone,
        ResourceKind::Clay,
        ResourceKind::Iron,
    ]
    .into_iter()
    .filter(|kind| Some(*kind) != exclude)
    .filter(|kind| {
        stock(source, *kind)
            > if *kind == ResourceKind::Food {
                source.food_reserve + 6.0
            } else {
                14.0
            }
    })
    .max_by(|a, b| {
        (stock(source, *a) - stock(destination, *a))
            .total_cmp(&(stock(source, *b) - stock(destination, *b)))
    })?;
    if stock(source, kind) <= stock(destination, kind) + 1.0 {
        return None;
    }
    let amount = 6.0;
    *stock_mut(source, kind) -= amount;
    Some(ResourceCargo { kind, amount })
}
fn horizontal_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
fn arrived(a: [f32; 3], b: [f32; 3]) -> bool {
    horizontal_distance(a, b) <= ARRIVAL && (a[1] - b[1]).abs() < 0.8
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::world::{Block, BlockPos, CELL_SIZE, WorldGeneration};

    #[test]
    fn generated_residents_walk_harvest_and_deliver_at_real_targets() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let mut life = VillageLife::new(&world);
        assert!(!life.residents.is_empty());
        assert!(life.validate(&world));
        let initial = life.residents();
        // Keep this local controller/economy test small; other settlements are
        // still consumed/grown, but only the first village's people are moved.
        life.residents.retain(|r| r.village == 0);
        for _ in 0..1_500 {
            life.tick(&world, 0.1);
        }
        assert!(life.residents.iter().any(|r| {
            initial
                .iter()
                .any(|old| old.id == r.snapshot.id && old.position != r.body.position)
        }));
        assert!(life.villages[0].harvests > 0, "{:?}", life.residents());
        assert!(life.villages[0].deliveries > 0, "{:?}", life.residents());
        assert!(
            life.residents
                .iter()
                .all(|r| r.body.position.iter().all(|v| v.is_finite()))
        );
    }

    #[test]
    fn removing_or_replacing_planted_soil_prevents_food_and_restoring_it_grows_a_new_crop() {
        let mut world = World::generate(42, WorldGeneration::GeographyV3);
        let village = world.settlements().unwrap().villages[0].clone();
        let mut life = VillageLife::new(&world);
        life.residents.truncate(1);
        // Begin with a mature crop and a farmer already at the real exterior
        // field workstation. A fresh harvest check must defeat stale caching.
        let route = &village.resident_routes[0];
        let resident = &mut life.residents[0];
        resident.phase = Phase::Working;
        resident.elapsed = 6.0;
        resident.waypoint = route.path.len() - 1;
        resident.body = Body::new(route.work);
        resident.snapshot.position = route.work;
        resident.snapshot.target = Some(route.work);
        life.villages[0].snapshot.crop_growth = 1.0;
        life.villages[0].cultivated_fraction = 1.0;
        life.soil_check_remaining = 1.0;
        let planted: Vec<_> = village
            .fields
            .iter()
            .flat_map(|field| field.plant_positions())
            .collect();
        assert!(!planted.is_empty());
        for (index, &position) in planted.iter().enumerate() {
            world
                .set_block(
                    position,
                    if index.is_multiple_of(2) {
                        Block::Air
                    } else {
                        Block::Stone
                    },
                )
                .unwrap();
        }
        let food_before = life.villages[0].snapshot.food;
        life.tick(&world, 0.05);
        assert_eq!(life.villages[0].snapshot.crop_growth, 0.0);
        assert_eq!(life.villages[0].harvests, 0);
        assert!(life.residents[0].snapshot.carrying.is_none());
        for _ in 0..3_000 {
            life.tick(&world, 0.1);
        }
        assert_eq!(life.villages[0].snapshot.crop_growth, 0.0);
        assert_eq!(life.villages[0].harvests, 0);
        assert_eq!(life.villages[0].deliveries, 0);
        assert!(life.villages[0].snapshot.food < food_before);
        for (index, &position) in planted.iter().enumerate() {
            world
                .set_block(
                    position,
                    if index.is_multiple_of(2) {
                        Block::Dirt
                    } else {
                        Block::Grass
                    },
                )
                .unwrap();
        }
        for _ in 0..20 {
            life.tick(&world, 0.1);
        }
        assert!(life.villages[0].snapshot.crop_growth > 0.0);
        assert!(
            life.villages[0].snapshot.crop_growth < 1.0,
            "Restoring soil must not revive the destroyed mature crop"
        );
        for _ in 0..3_000 {
            life.tick(&world, 0.1);
        }
        assert!(life.villages[0].harvests > 0);
        assert!(life.villages[0].deliveries > 0);
    }

    #[test]
    fn surplus_is_withdrawn_and_delivered_without_creating_goods_or_spending_reserves() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let life = VillageLife::new(&world);
        let mut source = life.villages[0].snapshot.clone();
        let mut destination = source.clone();
        source.timber = 100.0;
        destination.timber = 2.0;
        let before = source.timber + destination.timber;
        let cargo = take_surplus(&mut source, &destination, None).unwrap();
        assert_eq!(cargo.kind, ResourceKind::Timber);
        assert_eq!(source.timber + destination.timber + cargo.amount, before);
        assert_eq!(deposit(&mut destination, &cargo), cargo.amount);
        assert_eq!(source.timber + destination.timber, before);
        source.food = source.food_reserve;
        source.timber = 0.0;
        source.stone = 0.0;
        source.clay = 0.0;
        source.iron = 0.0;
        assert!(take_surplus(&mut source, &destination, None).is_none());
        destination.timber = MAX_STOCK - 2.0;
        assert_eq!(deposit(&mut destination, &cargo), 2.0);
        assert_eq!(destination.timber, MAX_STOCK);
    }

    #[test]
    fn trader_carries_real_surplus_over_the_entire_generated_trail_before_transfer() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let plan = world.settlements().unwrap();
        let (trail_index, trail) = plan
            .trails
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let length = |t: &rubblekin_core::settlement::Trail| {
                    t.points
                        .windows(2)
                        .map(|p| horizontal_distance(p[0], p[1]))
                        .sum::<f32>()
                };
                length(a).total_cmp(&length(b))
            })
            .unwrap();
        let mut life = VillageLife::new(&world);
        let source_index = plan
            .villages
            .iter()
            .position(|v| v.id == trail.from)
            .unwrap();
        let destination_index = plan.villages.iter().position(|v| v.id == trail.to).unwrap();
        let mut trader = life
            .residents
            .iter()
            .find(|r| r.village == source_index && r.trail.is_some())
            .unwrap()
            .clone();
        trader.trail = Some(trail_index);
        let home = trader.body.position;
        life.residents = vec![trader];
        life.villages[source_index].snapshot.timber = 100.0;
        life.villages[destination_index].snapshot.timber = 0.0;
        let mut saw_cargo = false;
        let mut completed = false;
        for _ in 0..40_000 {
            life.tick(&world, 0.25);
            let resident = &life.residents[0];
            assert!(
                resident.stuck < 10.0,
                "Generated trail is obstructed: {:?}",
                resident.snapshot
            );
            if resident.phase == Phase::ToTrade && resident.snapshot.carrying.is_some() {
                saw_cargo = true;
                assert_eq!(
                    life.villages[destination_index].snapshot.timber, 0.0,
                    "Remote inventory cannot receive goods before physical arrival"
                );
            }
            if life.villages[destination_index].trade_deliveries > 0 {
                assert!(
                    horizontal_distance(resident.body.position, *trail.points.last().unwrap())
                        < ARRIVAL
                );
                assert_eq!(life.villages[source_index].snapshot.timber, 94.0);
                assert_eq!(life.villages[destination_index].snapshot.timber, 6.0);
                completed = true;
                break;
            }
        }
        assert!(saw_cargo);
        assert!(
            completed,
            "Trader failed to cross trail from {home:?}: {:?}",
            life.residents()
        );
    }

    #[test]
    fn blocked_work_route_reports_blocked_and_never_produces_from_home() {
        let mut world = World::generate(42, WorldGeneration::GeographyV3);
        let mut life = VillageLife::new(&world);
        life.residents.truncate(1);
        let route = world.settlements().unwrap().villages[0].resident_routes[0].clone();
        let gate = route.path[1];
        let x = (gate[0] / CELL_SIZE).floor() as i32;
        let z = (gate[2] / CELL_SIZE).floor() as i32;
        let y = (gate[1] / CELL_SIZE).floor() as i32;
        // A solid cube across a nearby waypoint, taller than automatic stepping.
        for dz in -3..=3 {
            for dx in -3..=3 {
                for dy in 0..8 {
                    world
                        .set_block(BlockPos::new(x + dx, y + dy, z + dz), Block::Stone)
                        .unwrap();
                }
            }
        }
        for _ in 0..400 {
            life.tick(&world, 0.05);
        }
        assert_eq!(life.villages[0].harvests, 0);
        assert_eq!(life.villages[0].deliveries, 0);
        assert_eq!(
            life.residents[0].snapshot.action,
            ResidentAction::Blocked,
            "{:?}",
            life.residents()
        );
        assert!(life.residents[0].snapshot.carrying.is_none());
    }

    #[test]
    fn corrupt_identifiers_and_route_indices_are_rejected_without_panicking() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let original = VillageLife::new(&world);
        let mut life = original.clone();
        life.residents[0].trail = Some(usize::MAX);
        assert!(!life.validate(&world));
        let mut life = original.clone();
        life.residents[0].waypoint = usize::MAX;
        assert!(!life.validate(&world));
        let mut life = original.clone();
        life.residents[0].snapshot.id += 1;
        assert!(!life.validate(&world));
        let mut life = original;
        life.villages[0].snapshot.food = -1.0;
        assert!(!life.validate(&world));
    }
}
