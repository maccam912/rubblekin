//! Small resident-owned work cycles. Goods enter storage only after delivery;
//! a trader withdraws actual surplus and carries it along a generated trail.
use rubblekin_core::{
    physics::{
        Body, MoveInput, character_position_is_clear, move_character_with_obstacles,
        resolve_character_overlaps,
    },
    protocol::{ResidentAction, ResidentRole, ResidentSnapshot, ResourceCargo, VillageSnapshot},
    settlement::{ResourceKind, SettlementPlan, Village},
    world::{Block, BlockPos, CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

pub(crate) const MAX_RESIDENTS: usize = 60;
const MAX_STOCK: f32 = 10_000.0;
const ARRIVAL: f32 = 0.45;
const STATION_REACH: f32 = 0.9;
const HUNGRY: f32 = 60.0;
const TIRED: f32 = 30.0;
const MEAL: f32 = 2.0;

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
    #[serde(default = "initially_planted")]
    planted: bool,
}

fn initially_planted() -> bool {
    true
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
    ToFood,
    Eating,
    ToRest,
    Resuming,
    ToFarmExit,
    ToFarmResume,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Resume {
    phase: Phase,
    waypoint: usize,
    elapsed: f32,
    #[serde(default)]
    farm_waypoint: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct FarmPoint {
    position: [f32; 3],
    work: bool,
    route_gate: bool,
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
    #[serde(default)]
    resume: Option<Resume>,
    #[serde(default)]
    farm_waypoint: usize,
    #[serde(default)]
    farm_path: Vec<FarmPoint>,
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
                planted: true,
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
                        hunger: 25.0 + route_index as f32 * 3.0,
                        energy: 85.0 - route_index as f32 * 2.0,
                        reason: "Heading to the next job".into(),
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
                    resume: None,
                    farm_waypoint: 0,
                    farm_path: Vec::new(),
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
                || !economy.planted && value.crop_growth != 0.0
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
                Phase::Working | Phase::ToFarmExit | Phase::ToFarmResume
                    if resident.snapshot.role == ResidentRole::Farmer =>
                {
                    if resident.farm_path.is_empty() {
                        at_station(resident.body.position, local.work)
                    } else {
                        resident.farm_path.windows(2).any(|points| {
                            horizontal_segment_distance(
                                resident.body.position,
                                points[0].position,
                                points[1].position,
                            ) <= 2.0
                        }) || at_station(resident.body.position, local.work)
                    }
                }
                Phase::Working => at_station(resident.body.position, local.work),
                Phase::Resting => at_station(resident.body.position, local.home),
                Phase::Eating => at_station(resident.body.position, local.path[local.store_index]),
                Phase::Loading => at_station(resident.body.position, local.path[local.store_index]),
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
            let valid_farm_path =
                validate_farm_path(world, &plan.villages[expected.village], local, resident);
            if resident.village != expected.village
                || resident.route != expected.route
                || resident.trail != expected.trail
                || resident.snapshot.id != expected.snapshot.id
                || resident.snapshot.village_id != expected.snapshot.village_id
                || resident.snapshot.role != expected.snapshot.role
                || resident.snapshot.name.trim().is_empty()
                || resident.snapshot.name.chars().count() > 48
                || resident.snapshot.name.chars().any(char::is_control)
                || [resident.snapshot.hunger, resident.snapshot.energy]
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=100.0).contains(v))
                || resident.snapshot.reason.chars().count() > 160
                || resident.snapshot.reason.chars().any(char::is_control)
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
                || !valid_farm_path
                || matches!(resident.phase, Phase::ToFarmExit | Phase::ToFarmResume)
                    && (resident.snapshot.role != ResidentRole::Farmer
                        || resident.farm_path.is_empty()
                        || resident.resume.as_ref().is_none_or(|resume| {
                            resume.phase != Phase::Working
                                || resume.farm_waypoint == 0
                                || resume.farm_waypoint >= resident.farm_path.len()
                                || resident.farm_waypoint > resume.farm_waypoint
                        }))
                || resident.resume.as_ref().is_some_and(|resume| {
                    resume.waypoint >= local.path.len()
                        || !resume.elapsed.is_finite()
                        || !(0.0..=60.0).contains(&resume.elapsed)
                        || (resume.phase == Phase::Working
                            && resident.snapshot.role == ResidentRole::Farmer
                            && resume.farm_waypoint >= resident.farm_path.len().max(1))
                        || !matches!(
                            resume.phase,
                            Phase::ToWork
                                | Phase::Working
                                | Phase::ToStore
                                | Phase::ToHome
                                | Phase::ToTradeStore
                                | Phase::Loading
                                | Phase::Resting
                        )
                        || resident.trail.is_none()
                            && matches!(resume.phase, Phase::ToTradeStore | Phase::Loading)
                })
                || matches!(
                    resident.phase,
                    Phase::ToFood
                        | Phase::Eating
                        | Phase::ToRest
                        | Phase::Resuming
                        | Phase::ToFarmExit
                        | Phase::ToFarmResume
                ) && resident.resume.is_none()
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

    pub fn resolve_overlaps(&mut self, world: &World, external: &[[f32; 3]]) {
        let mut positions: Vec<_> = self.positions().collect();
        for (index, resident) in self.residents.iter_mut().enumerate() {
            let mut obstacles = external.to_vec();
            obstacles.extend(
                positions
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| (i != index).then_some(*p)),
            );
            move_character_with_obstacles(
                world,
                &mut resident.body,
                MoveInput::default(),
                0.000001,
                &obstacles,
            );
            resolve_character_overlaps(world, &mut resident.body, &obstacles);
            positions[index] = resident.body.position;
            resident.snapshot.position = resident.body.position;
            if let Some(plan) = world.settlements() {
                let route = &plan.villages[resident.village].resident_routes[resident.route];
                match resident.phase {
                    Phase::Working
                        if !at_station(resident.body.position, route.work)
                            && resident.snapshot.role != ResidentRole::Farmer =>
                    {
                        resident.phase = Phase::ToWork;
                        resident.waypoint = route.path.len() - 1;
                    }
                    Phase::Resting if !at_station(resident.body.position, route.home) => {
                        resident.phase = if resident.resume.is_some() {
                            Phase::ToRest
                        } else {
                            Phase::ToHome
                        };
                        resident.waypoint = 0;
                    }
                    Phase::Eating
                        if !at_station(resident.body.position, route.path[route.store_index]) =>
                    {
                        resident.phase = Phase::ToFood;
                        resident.waypoint = route.store_index;
                    }
                    Phase::Loading
                        if !at_station(resident.body.position, route.path[route.store_index]) =>
                    {
                        resident.phase = Phase::ToTradeStore;
                        resident.waypoint = route.store_index;
                    }
                    Phase::Unloading => {
                        let trail = &plan.trails[resident.trail.unwrap()];
                        let end = if trail.from == resident.snapshot.village_id {
                            trail.points.len() - 1
                        } else {
                            0
                        };
                        if !arrived(resident.body.position, trail.points[end]) {
                            resident.phase = Phase::ToTrade;
                            resident.waypoint = end;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    #[cfg(test)]
    pub fn tick(&mut self, world: &World, dt: f32) {
        self.tick_with_obstacles(world, dt, &[]);
    }

    pub fn tick_with_obstacles(&mut self, world: &World, dt: f32, external: &[[f32; 3]]) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let dt = dt.min(0.25);
        let Some(plan) = world.settlements() else {
            return;
        };
        self.soil_check_remaining -= dt;
        if self.soil_check_remaining <= 0.0 {
            for (economy, village) in self.villages.iter_mut().zip(&plan.villages) {
                economy.cultivated_fraction = cultivated_fraction(world, village);
                if economy.cultivated_fraction > 0.0 {
                    for resident in self
                        .residents
                        .iter_mut()
                        .filter(|r| r.snapshot.village_id == village.id && r.farm_path.len() == 1)
                    {
                        resident.farm_path.clear();
                        resident.farm_waypoint = 0;
                    }
                }
                if economy.cultivated_fraction == 0.0 {
                    economy.snapshot.crop_growth = 0.0;
                    economy.planted = false;
                }
            }
            self.soil_check_remaining = 1.0;
        }
        for (index, economy) in self.villages.iter_mut().enumerate() {
            let snapshot = &mut economy.snapshot;
            // Food leaves stores when a real resident completes a meal.
            if economy.planted {
                snapshot.crop_growth = (snapshot.crop_growth
                    + dt * (0.5 + plan.villages[index].resources.farming)
                        * economy.cultivated_fraction
                        / 90.0)
                    .min(1.0);
            }
            snapshot.capacity_for_growth = snapshot.food > snapshot.food_reserve * 2.0
                && snapshot.population < snapshot.housing_capacity;
        }
        let mut positions: Vec<_> = self.positions().collect();
        for (index, resident) in self.residents.iter_mut().enumerate() {
            let village = &plan.villages[resident.village];
            let route = &village.resident_routes[resident.route];
            let economy = &mut self.villages[resident.village];
            let mut obstacles = external.to_vec();
            obstacles.extend(
                positions
                    .iter()
                    .enumerate()
                    .filter_map(|(i, p)| (i != index).then_some(*p)),
            );
            resident.snapshot.hunger = (resident.snapshot.hunger + dt * 0.2).min(100.0);
            if resident.phase != Phase::Resting {
                resident.snapshot.energy = (resident.snapshot.energy - dt * 0.12).max(0.0);
            }
            // An existing tour returns to its entrance before a needs detour.
            // Long-distance traders use their next store/home stop, preserving
            // their physical cargo and avoiding repeated mid-trail reversals.
            let can_interrupt = matches!(
                resident.phase,
                Phase::ToWork
                    | Phase::ToStore
                    | Phase::ToHome
                    | Phase::ToTradeStore
                    | Phase::Loading
            ) || resident.phase == Phase::Working;
            if resident.resume.is_none() && can_interrupt {
                let hungry = resident.snapshot.hunger >= HUNGRY && economy.snapshot.food >= MEAL;
                let tired = resident.snapshot.energy <= TIRED;
                if hungry || tired {
                    resident.resume = Some(Resume {
                        phase: resident.phase,
                        waypoint: resident.waypoint,
                        elapsed: resident.elapsed,
                        farm_waypoint: resident.farm_waypoint,
                    });
                    if resident.snapshot.role == ResidentRole::Farmer && resident.farm_waypoint > 0
                    {
                        resident.phase = Phase::ToFarmExit;
                        resident.farm_waypoint -= 1;
                    } else {
                        resident.phase = if hungry { Phase::ToFood } else { Phase::ToRest };
                    }
                    // Retrace the current route through its actual next gate.
                    resident.elapsed = 0.0;
                }
            }
            let shortage = resident.snapshot.hunger >= HUNGRY && economy.snapshot.food < MEAL;
            let work_rate = if shortage { 0.4 } else { 1.0 };
            if matches!(resident.phase, Phase::Resting | Phase::Eating)
                || resident.phase == Phase::Working
                    && resident.snapshot.role != ResidentRole::Farmer
            {
                move_character_with_obstacles(
                    world,
                    &mut resident.body,
                    MoveInput::default(),
                    dt,
                    &obstacles,
                );
            }
            match resident.phase {
                Phase::ToFarmExit | Phase::ToFarmResume => {
                    advance_farm_detour(world, resident, economy, dt, &obstacles);
                }
                Phase::Working if resident.snapshot.role == ResidentRole::Farmer => {
                    if resident.farm_path.is_empty() {
                        resident.farm_path = farm_path(world, village, resident.route);
                        if resident.farm_waypoint >= resident.farm_path.len() {
                            resident.farm_waypoint = 0;
                        }
                    }
                    advance_farmer(world, village, resident, economy, dt, work_rate, &obstacles);
                }
                Phase::Working => {
                    resident.snapshot.action = ResidentAction::Working;
                    resident.snapshot.target = Some(route.work);
                    resident.snapshot.reason = if shortage {
                        "No meal in storage; working slowly to keep the village supplied"
                    } else {
                        "Collecting resources at the workplace"
                    }
                    .into();
                    if at_station(resident.body.position, route.work) {
                        resident.elapsed += dt * work_rate;
                        resident.snapshot.energy = (resident.snapshot.energy - dt * 0.08).max(0.0);
                        if resident.elapsed >= 6.0 {
                            let remaining = &mut economy.remaining[resource_index(route.resource)];
                            let amount = remaining.min(4.0);
                            *remaining -= amount;
                            resident.snapshot.carrying = (amount > 0.0).then_some(ResourceCargo {
                                kind: route.resource,
                                amount,
                            });
                            resident.phase = Phase::ToStore;
                            resident.waypoint = route.path.len() - 1;
                            resident.elapsed = 0.0;
                        }
                    } else {
                        resident.phase = Phase::ToWork;
                        resident.waypoint = route.path.len() - 1;
                        resident.elapsed = 0.0;
                    }
                }
                Phase::Eating => {
                    resident.snapshot.action = ResidentAction::Eating;
                    resident.snapshot.target = Some(route.path[route.store_index]);
                    resident.snapshot.reason = "Taking a meal from village storage".into();
                    if !at_station(resident.body.position, route.path[route.store_index]) {
                        resident.phase = Phase::ToFood;
                        resident.waypoint = route.store_index;
                    } else {
                        resident.elapsed += dt;
                        if resident.elapsed >= 2.0 {
                            if economy.snapshot.food >= MEAL {
                                economy.snapshot.food -= MEAL;
                                resident.snapshot.hunger =
                                    (resident.snapshot.hunger - 45.0).max(0.0);
                            }
                            resident.elapsed = 0.0;
                            resident.phase = if resident.snapshot.energy <= TIRED {
                                Phase::ToRest
                            } else {
                                Phase::Resuming
                            };
                            resident.waypoint = route.store_index;
                        }
                    }
                }
                Phase::Resting => {
                    resident.snapshot.action = ResidentAction::Resting;
                    resident.snapshot.target = Some(route.home);
                    resident.snapshot.reason = "Sleeping at home to recover energy".into();
                    if at_station(resident.body.position, route.home) {
                        resident.elapsed += dt;
                        resident.snapshot.energy = (resident.snapshot.energy + dt * 3.0).min(100.0);
                        if resident.snapshot.energy >= 85.0 && resident.elapsed >= 2.0 {
                            resident.phase = if resident.resume.is_some() {
                                Phase::Resuming
                            } else if resident.trail.is_some() {
                                Phase::ToTradeStore
                            } else {
                                Phase::ToWork
                            };
                            resident.waypoint = 0;
                            resident.elapsed = 0.0;
                        }
                    } else {
                        resident.phase = if resident.resume.is_some() {
                            Phase::ToRest
                        } else {
                            Phase::ToHome
                        };
                        resident.waypoint = 0;
                    }
                }
                Phase::Loading | Phase::Unloading => {}
                _ => advance_resident(world, plan, resident, dt, &obstacles),
            }
            if resident.phase == Phase::ToHome
                && at_station(resident.body.position, route.path[route.store_index])
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
            positions[index] = resident.body.position;
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
                if resident.snapshot.hunger >= HUNGRY
                    && self.villages[partner].snapshot.food >= MEAL
                {
                    self.villages[partner].snapshot.food -= MEAL;
                    resident.snapshot.hunger = (resident.snapshot.hunger - 45.0).max(0.0);
                }
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

fn advance_resident(
    world: &World,
    plan: &SettlementPlan,
    resident: &mut Resident,
    dt: f32,
    obstacles: &[[f32; 3]],
) {
    let village = &plan.villages[resident.village];
    let route = &village.resident_routes[resident.route];
    let (points, end, forward) = match resident.phase {
        Phase::ToWork => (&route.path, route.path.len() - 1, true),
        Phase::ToFood => (
            &route.path,
            route.store_index,
            resident.waypoint <= route.store_index,
        ),
        Phase::ToRest => (&route.path, 0, false),
        Phase::Resuming => {
            let end = resident
                .resume
                .as_ref()
                .expect("needs trip retains its job")
                .waypoint;
            (&route.path, end, resident.waypoint <= end)
        }
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
    resident.snapshot.reason = match resident.phase {
        Phase::ToFood => "Hungry; walking to village food storage",
        Phase::ToRest => "Tired; walking home to sleep",
        Phase::Resuming => "Needs handled; returning to the interrupted job",
        _ if resident.snapshot.carrying.is_some() => "Carrying real goods to their destination",
        _ => "Following the village route to the next job",
    }
    .into();
    resident.snapshot.action = if resident.stuck >= 8.0 {
        ResidentAction::Blocked
    } else if resident.phase == Phase::ToFood {
        ResidentAction::SeekingFood
    } else if resident.phase == Phase::ToRest {
        ResidentAction::GoingHome
    } else if resident.snapshot.carrying.is_some() {
        ResidentAction::Delivering
    } else {
        ResidentAction::Walking
    };
    if if resident.waypoint == end && !matches!(resident.phase, Phase::ToTrade | Phase::Returning) {
        at_station(resident.body.position, target)
    } else {
        arrived(resident.body.position, target)
    } {
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
                Phase::ToFood => Phase::Eating,
                Phase::ToRest => Phase::Resting,
                Phase::Resuming => {
                    let resume = resident.resume.as_ref().unwrap();
                    if resume.phase == Phase::Working
                        && resident.snapshot.role == ResidentRole::Farmer
                        && resume.farm_waypoint > 0
                    {
                        resident.phase = Phase::ToFarmResume;
                        resident.farm_waypoint = 0;
                        resident.elapsed = 0.0;
                    } else {
                        let resume = resident.resume.take().unwrap();
                        resident.elapsed = resume.elapsed;
                        resident.phase = resume.phase;
                    }
                    return;
                }
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
    walk_toward(world, resident, target, dt, obstacles);
}

fn horizontal_segment_distance(p: [f32; 3], a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = b[0] - a[0];
    let dz = b[2] - a[2];
    let length = dx * dx + dz * dz;
    let t = if length > 0.0 {
        ((p[0] - a[0]) * dx + (p[2] - a[2]) * dz) / length
    } else {
        0.0
    }
    .clamp(0.0, 1.0);
    horizontal_distance(p, [a[0] + dx * t, p[1], a[2] + dz * t])
}

fn validate_farm_path(
    world: &World,
    village: &Village,
    route: &rubblekin_core::settlement::ResidentRoute,
    resident: &Resident,
) -> bool {
    let path = &resident.farm_path;
    if path.is_empty() {
        return resident.farm_waypoint == 0
            && !matches!(resident.phase, Phase::ToFarmExit | Phase::ToFarmResume);
    }
    if resident.snapshot.role != ResidentRole::Farmer
        || path.len() > 1024
        || resident.farm_waypoint >= path.len()
        || path.first().unwrap().position != route.work
        || path.last().unwrap().position != route.work
    {
        return false;
    }
    path.iter().all(|point| {
        let p = point.position;
        let finite = p.iter().all(|v| v.is_finite())
            && world.contains_block(BlockPos::new(
                (p[0] / CELL_SIZE).floor() as i32,
                (p[1] / CELL_SIZE).floor() as i32,
                (p[2] / CELL_SIZE).floor() as i32,
            ));
        let route_point = village.resident_routes.iter().any(|r| r.path.contains(&p));
        let local = village.fields.iter().any(|field| {
            p[0] >= (field.origin.x - 24) as f32 * CELL_SIZE
                && p[0] <= (field.origin.x + field.width + 24) as f32 * CELL_SIZE
                && p[2] >= (field.origin.z - 24) as f32 * CELL_SIZE
                && p[2] <= (field.origin.z + field.depth + 24) as f32 * CELL_SIZE
        });
        let actual_field = village.fields.iter().any(|field| {
            p[0] >= field.origin.x as f32 * CELL_SIZE
                && p[0] < (field.origin.x + field.width) as f32 * CELL_SIZE
                && p[2] >= field.origin.z as f32 * CELL_SIZE
                && p[2] < (field.origin.z + field.depth) as f32 * CELL_SIZE
                && p[1] == (field.origin.y + 1) as f32 * CELL_SIZE
        });
        finite
            && (route_point || local)
            && (!point.route_gate || route_point)
            && (!point.work || actual_field)
    }) && path.windows(2).all(|points| {
        horizontal_distance(points[0].position, points[1].position) <= 4.0
            && (points[0].position[1] - points[1].position[1]).abs() <= 4.0
    })
}

/// Small farm-only navigation: use actual edited ground and half-meter steps
/// to enter the field, then visit a personal row and retrace the same entrance.
/// It never changes the generator or teleports a farmer onto a raised plot.
fn farm_path(world: &World, village: &Village, route_index: usize) -> Vec<FarmPoint> {
    let own = farm_local_path(world, village, route_index, route_index);
    if own.iter().any(|p| p.work) {
        return own;
    }
    // A flattened plot can be too high to enter with ordinary walking/hops.
    // Help on the other existing field via the same proven village routes.
    let alternative = (route_index + 1) % village.fields.len();
    let other = farm_local_path(world, village, alternative, route_index);
    if !other.iter().any(|p| p.work) {
        return own;
    }
    let source = &village.resident_routes[route_index];
    let destination = &village.resident_routes[alternative];
    let mut approach: Vec<_> = source.path[source.store_index..]
        .iter()
        .rev()
        .map(|p| FarmPoint {
            position: *p,
            work: false,
            route_gate: true,
        })
        .collect();
    approach.extend(
        destination.path[destination.store_index + 1..]
            .iter()
            .map(|p| FarmPoint {
                position: *p,
                work: false,
                route_gate: true,
            }),
    );
    let back: Vec<_> = approach[..approach.len() - 1]
        .iter()
        .rev()
        .map(|p| FarmPoint {
            position: p.position,
            work: false,
            route_gate: true,
        })
        .collect();
    approach.extend(other.into_iter().skip(1));
    approach.extend(back);
    approach
}

fn farm_local_path(
    world: &World,
    village: &Village,
    source_route: usize,
    personal_row: usize,
) -> Vec<FarmPoint> {
    let route = &village.resident_routes[source_route];
    let field = &village.fields[source_route % village.fields.len()];
    let start = (
        (route.work[0] / CELL_SIZE).floor() as i32,
        (route.work[2] / CELL_SIZE).floor() as i32,
    );
    let desired_x = field.origin.x + 1 + 3 * personal_row as i32;
    let desired_z = field.origin.z + 3 + 2 * personal_row as i32;
    let ground = |(x, z): (i32, i32)| {
        let px = (x as f32 + 0.5) * CELL_SIZE;
        let pz = (z as f32 + 0.5) * CELL_SIZE;
        let y = [
            [-0.281, -0.281],
            [-0.281, 0.281],
            [0.281, -0.281],
            [0.281, 0.281],
        ]
        .into_iter()
        .map(|d| world.surface_height(px + d[0], pz + d[1]))
        .fold(f32::NEG_INFINITY, f32::max);
        [px, y, pz]
    };
    let mut visited = HashMap::from([(start, (start, route.work))]);
    let mut queue = VecDeque::from([start]);
    let mut best: Option<((i32, i32), i32)> = None;
    while let Some(cell) = queue.pop_front() {
        let position = visited[&cell].1;
        if (field.origin.x + 1..field.origin.x + field.width - 1).contains(&cell.0)
            && (field.origin.z + 1..field.origin.z + field.depth - 1).contains(&cell.1)
            && matches!(
                world.block(BlockPos::new(cell.0, field.origin.y, cell.1)),
                Block::Dirt | Block::Grass
            )
            && (position[1] - (field.origin.y + 1) as f32 * CELL_SIZE).abs() < 0.01
        {
            let distance = (cell.0 - desired_x).abs() + (cell.1 - desired_z).abs();
            if best.is_none_or(|(_, old)| distance < old) {
                best = Some((cell, distance));
            }
            if distance == 0 {
                break;
            }
        }
        for next in [
            (cell.0 + 1, cell.1),
            (cell.0 - 1, cell.1),
            (cell.0, cell.1 + 1),
            (cell.0, cell.1 - 1),
        ] {
            if visited.contains_key(&next)
                || !(field.origin.x - 24..=field.origin.x + field.width + 24).contains(&next.0)
                || !(field.origin.z - 24..=field.origin.z + field.depth + 24).contains(&next.1)
            {
                continue;
            }
            let target = ground(next);
            if (target[1] - position[1]).abs() > 1.0 + 0.01
                || !character_position_is_clear(world, target, &[])
                || (target[1] - position[1]).abs() > 0.01
                    && !farm_edge_is_walkable(world, position, target)
            {
                continue;
            }
            visited.insert(next, (cell, target));
            queue.push_back(next);
        }
    }
    let Some((mut cell, _)) = best else {
        return vec![FarmPoint {
            position: route.work,
            work: false,
            route_gate: false,
        }];
    };
    let mut points = Vec::new();
    while cell != start {
        let (previous, position) = visited[&cell];
        points.push(FarmPoint {
            position,
            work: false,
            route_gate: false,
        });
        cell = previous;
    }
    points.push(FarmPoint {
        position: route.work,
        work: false,
        route_gate: false,
    });
    points.reverse();
    points.last_mut().unwrap().work = true;
    let first = points.last().unwrap().position;
    // Separate rows and staggered starts keep farmers at distinct work sites.
    for offset in [3, 6] {
        let z = (first[2] / CELL_SIZE).floor() as i32 + offset;
        if z >= field.origin.z + field.depth - 1 {
            break;
        }
        let target = [first[0], first[1], (z as f32 + 0.5) * CELL_SIZE];
        if character_position_is_clear(world, target, &[]) {
            points.push(FarmPoint {
                position: target,
                work: true,
                route_gate: false,
            });
        }
    }
    let back: Vec<_> = points[..points.len() - 1]
        .iter()
        .rev()
        .map(|p| FarmPoint {
            position: p.position,
            work: false,
            route_gate: false,
        })
        .collect();
    points.extend(back);
    points
}

fn farm_edge_is_walkable(world: &World, a: [f32; 3], b: [f32; 3]) -> bool {
    for (start, end) in [(a, b), (b, a)] {
        let mut body = Body::new(start);
        move_character_with_obstacles(world, &mut body, MoveInput::default(), 0.05, &[]);
        for step in 0..30 {
            let distance = horizontal_distance(body.position, end);
            let mut input = MoveInput::default();
            if distance > 0.001 {
                let factor = 0.52_f32.min(distance / (3.8 * 0.05));
                input.direction = [
                    (end[0] - body.position[0]) / distance * factor,
                    (end[2] - body.position[2]) / distance * factor,
                ];
            }
            input.jump = step >= 4 && body.on_ground && end[1] - body.position[1] > 0.3;
            move_character_with_obstacles(world, &mut body, input, 0.05, &[]);
        }
        if horizontal_distance(body.position, end) > 0.02
            || (body.position[1] - end[1]).abs() > 0.15
        {
            return false;
        }
    }
    true
}

fn farm_point_reached(resident: &Resident, point: &FarmPoint) -> bool {
    if resident.farm_waypoint == 0
        || resident.farm_waypoint == resident.farm_path.len() - 1
        || point.work
    {
        at_station(resident.body.position, point.position)
    } else if point.route_gate {
        arrived(resident.body.position, point.position)
    } else {
        horizontal_distance(resident.body.position, point.position) <= 0.02
            && (resident.body.position[1] - point.position[1]).abs() < 0.15
    }
}

fn skip_occupied_farm_gate(
    world: &World,
    resident: &mut Resident,
    next: usize,
    obstacles: &[[f32; 3]],
) -> bool {
    let current = &resident.farm_path[resident.farm_waypoint];
    if current.work
        || resident.stuck < 0.3
        || next == resident.farm_waypoint
        || next >= resident.farm_path.len()
        || horizontal_distance(resident.body.position, current.position) > 0.9
        || !obstacles
            .iter()
            .any(|p| rubblekin_core::physics::characters_overlap(current.position, *p))
    {
        return false;
    }
    if farm_edge_is_walkable(
        world,
        resident.body.position,
        resident.farm_path[next].position,
    ) {
        // Work sites still require physical arrival. Only an occupied transit
        // gate can be bypassed after checking the actual terrain movement.
        resident.farm_waypoint = next;
        resident.stuck = 0.0;
        return true;
    }
    false
}

fn advance_farm_detour(
    world: &World,
    resident: &mut Resident,
    economy: &Economy,
    dt: f32,
    obstacles: &[[f32; 3]],
) {
    let point = &resident.farm_path[resident.farm_waypoint];
    let target = point.position;
    let exiting = resident.phase == Phase::ToFarmExit;
    resident.snapshot.target = Some(target);
    resident.snapshot.action = if resident.stuck >= 8.0 {
        ResidentAction::Blocked
    } else if exiting && resident.snapshot.hunger >= HUNGRY {
        ResidentAction::SeekingFood
    } else if exiting {
        ResidentAction::GoingHome
    } else {
        ResidentAction::Walking
    };
    resident.snapshot.reason = if exiting {
        "Retracing the field path to handle hunger or tiredness"
    } else {
        "Returning along the field path to the interrupted job"
    }
    .into();
    if !farm_point_reached(resident, point) {
        let next = if exiting {
            resident.farm_waypoint.saturating_sub(1)
        } else {
            (resident.farm_waypoint + 1).min(resident.resume.as_ref().unwrap().farm_waypoint)
        };
        if skip_occupied_farm_gate(world, resident, next, obstacles) {
            let next_target = resident.farm_path[resident.farm_waypoint].position;
            resident.snapshot.target = Some(next_target);
            walk_toward(world, resident, next_target, dt, obstacles);
        } else {
            walk_toward(world, resident, target, dt, obstacles);
        }
        return;
    }
    move_character_with_obstacles(
        world,
        &mut resident.body,
        MoveInput::default(),
        dt,
        obstacles,
    );
    resident.stuck = 0.0;
    if exiting && resident.farm_waypoint == 0 {
        resident.phase = if resident.snapshot.hunger >= HUNGRY && economy.snapshot.food >= MEAL {
            Phase::ToFood
        } else if resident.snapshot.energy <= TIRED {
            Phase::ToRest
        } else {
            Phase::Resuming
        };
    } else if !exiting && resident.farm_waypoint == resident.resume.as_ref().unwrap().farm_waypoint
    {
        let resume = resident.resume.take().unwrap();
        resident.phase = Phase::Working;
        resident.elapsed = resume.elapsed;
    } else if exiting {
        resident.farm_waypoint -= 1;
    } else {
        resident.farm_waypoint += 1;
    }
}

fn advance_farmer(
    world: &World,
    village: &Village,
    resident: &mut Resident,
    economy: &mut Economy,
    dt: f32,
    work_rate: f32,
    obstacles: &[[f32; 3]],
) {
    let route = &village.resident_routes[resident.route];
    let point = &resident.farm_path[resident.farm_waypoint];
    let target = point.position;
    resident.snapshot.target = Some(target);
    let reached = farm_point_reached(resident, point);
    if !reached {
        resident.snapshot.action = if resident.stuck >= 8.0 {
            ResidentAction::Blocked
        } else if resident.snapshot.carrying.is_some() {
            ResidentAction::Delivering
        } else {
            ResidentAction::Walking
        };
        resident.snapshot.reason = "Walking between assigned crop rows".into();
        if skip_occupied_farm_gate(world, resident, resident.farm_waypoint + 1, obstacles) {
            let next = resident.farm_path[resident.farm_waypoint].position;
            resident.snapshot.target = Some(next);
            walk_toward(world, resident, next, dt, obstacles);
        } else {
            walk_toward(world, resident, target, dt, obstacles);
        }
        return;
    }
    move_character_with_obstacles(
        world,
        &mut resident.body,
        MoveInput::default(),
        dt,
        obstacles,
    );
    if point.work && !at_station(resident.body.position, target) {
        return;
    }
    if point.work && resident.snapshot.carrying.is_none() {
        let soil = BlockPos::new(
            (target[0] / CELL_SIZE).floor() as i32,
            ((target[1] - CELL_SIZE) / CELL_SIZE).round() as i32,
            (target[2] / CELL_SIZE).floor() as i32,
        );
        if matches!(world.block(soil), Block::Dirt | Block::Grass) {
            resident.snapshot.action = if !economy.planted {
                ResidentAction::Planting
            } else if economy.snapshot.crop_growth >= 1.0 {
                ResidentAction::Harvesting
            } else {
                ResidentAction::Tending
            };
            resident.snapshot.reason = if work_rate < 1.0 {
                "No meal in storage; farming slowly to restore the food supply"
            } else if !economy.planted {
                "Planting the next crop in the field"
            } else if economy.snapshot.crop_growth >= 1.0 {
                "Gathering ripe crops before carrying them to storage"
            } else {
                "Tending crops in an assigned row"
            }
            .into();
            resident.elapsed += dt * work_rate;
            resident.snapshot.energy = (resident.snapshot.energy - dt * 0.08).max(0.0);
            if resident.elapsed < 2.0 {
                return;
            }
            if !economy.planted {
                economy.planted = true;
                economy.snapshot.crop_growth = 0.01;
            } else if economy.snapshot.crop_growth >= 1.0 {
                economy.cultivated_fraction = cultivated_fraction(world, village);
                if economy.cultivated_fraction > 0.0 {
                    resident.snapshot.carrying = Some(ResourceCargo {
                        kind: ResourceKind::Food,
                        amount: 12.0 * economy.cultivated_fraction,
                    });
                    economy.snapshot.crop_growth = 0.0;
                    economy.planted = false;
                    economy.harvests = economy.harvests.saturating_add(1);
                }
            } else {
                // Actual tending slightly advances the planted crop; growth
                // still comes from time, fertility and intact planted soil.
                economy.snapshot.crop_growth =
                    (economy.snapshot.crop_growth + 0.004 * economy.cultivated_fraction).min(1.0);
            }
        }
    }
    resident.elapsed = 0.0;
    resident.farm_waypoint += 1;
    if resident.farm_waypoint == resident.farm_path.len() {
        resident.farm_waypoint = 0;
        if resident.snapshot.carrying.is_some() {
            resident.phase = Phase::ToStore;
            resident.waypoint = route.path.len() - 1;
        } else if resident.farm_path.len() == 1 {
            resident.snapshot.action = ResidentAction::Blocked;
            resident.snapshot.reason =
                "Field access or planted soil is blocked; no remote harvest".into();
        }
    }
}

fn walk_toward(
    world: &World,
    resident: &mut Resident,
    target: [f32; 3],
    dt: f32,
    obstacles: &[[f32; 3]],
) {
    let distance = horizontal_distance(resident.body.position, target);
    let mut input = MoveInput::default();
    if distance > 0.001 {
        let factor = 0.52_f32.min(distance / (3.8 * dt));
        let direction = [
            (target[0] - resident.body.position[0]) / distance,
            (target[2] - resident.body.position[2]) / distance,
        ];
        let near = obstacles.iter().find(|p| {
            let dx = p[0] - resident.body.position[0];
            let dz = p[2] - resident.body.position[2];
            (p[1] - resident.body.position[1]).abs() < 1.7
                && dx * direction[0] + dz * direction[1] > 0.0
                && dx * direction[0] + dz * direction[1] < 1.2
                && (dx * direction[1] - dz * direction[0]).abs() < 0.65
        });
        if near.is_some() && distance > 0.1 {
            let side = 1.0;
            input.direction = [
                (direction[0] - direction[1] * side * 0.8) * factor,
                (direction[1] + direction[0] * side * 0.8) * factor,
            ];
        } else {
            input.direction = direction.map(|d| d * factor);
        }
    }
    input.jump = resident.snapshot.role == ResidentRole::Farmer
        && matches!(
            resident.phase,
            Phase::Working | Phase::ToFarmExit | Phase::ToFarmResume
        )
        && resident.stuck >= 0.2
        && resident.body.on_ground
        && target[1] - resident.body.position[1] > 0.3
        && distance < 1.25;
    let old = resident.body.position;
    move_character_with_obstacles(world, &mut resident.body, input, dt, obstacles);
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
fn at_station(a: [f32; 3], b: [f32; 3]) -> bool {
    horizontal_distance(a, b) <= STATION_REACH && (a[1] - b[1]).abs() < 0.8
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
        let farmers = life
            .residents
            .iter()
            .filter(|r| r.snapshot.role == ResidentRole::Farmer)
            .count();
        let mut tours = vec![0_usize; life.residents.len()];
        let mut soil_targets: Vec<Vec<[f32; 3]>> = vec![Vec::new(); life.residents.len()];
        for _ in 0..6_000 {
            let previous: Vec<_> = life
                .residents
                .iter()
                .map(|r| (r.farm_waypoint, r.phase, r.farm_path.len()))
                .collect();
            life.tick(&world, 0.1);
            for (i, r) in life.residents.iter().enumerate() {
                if r.snapshot.role != ResidentRole::Farmer {
                    continue;
                }
                if previous[i].1 == Phase::Working
                    && previous[i].2 > 1
                    && previous[i].0 == previous[i].2 - 1
                    && r.farm_waypoint == 0
                {
                    tours[i] += 1;
                }
                if matches!(
                    r.snapshot.action,
                    ResidentAction::Planting | ResidentAction::Tending | ResidentAction::Harvesting
                ) {
                    let target = r.snapshot.target.unwrap();
                    assert!(at_station(r.body.position, target));
                    let soil = BlockPos::new(
                        (target[0] / CELL_SIZE).floor() as i32,
                        ((target[1] - CELL_SIZE) / CELL_SIZE).round() as i32,
                        (target[2] / CELL_SIZE).floor() as i32,
                    );
                    assert!(matches!(world.block(soil), Block::Dirt | Block::Grass));
                    if !soil_targets[i].contains(&target) {
                        soil_targets[i].push(target);
                    }
                }
                for other in life.residents.iter().skip(i + 1) {
                    assert!(!rubblekin_core::physics::characters_overlap(
                        r.body.position,
                        other.body.position
                    ));
                }
            }
        }
        assert!(farmers >= 3);
        for (i, r) in life
            .residents
            .iter()
            .enumerate()
            .filter(|(_, r)| r.snapshot.role == ResidentRole::Farmer)
        {
            assert!(
                tours[i] >= 2,
                "Farmer failed repeated field tours: {:?}; tours={tours:?}",
                r.snapshot
            );
            assert!(
                soil_targets[i].len() >= 2,
                "Farmer did not visit distinct crop rows: {:?}",
                r.snapshot
            );
        }
        assert!(life.villages[0].harvests >= 2);
        assert!(life.villages[0].deliveries >= 2);
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
        life.soil_check_remaining = 0.0;
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
        for _ in 0..1_000 {
            life.tick(&world, 0.1);
            if life.villages[0].snapshot.crop_growth > 0.0 {
                break;
            }
        }
        assert!(
            life.villages[0].snapshot.crop_growth > 0.0,
            "{:?}",
            life.residents()
        );
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
    fn every_generated_farmer_has_a_distinct_walkable_soil_tour_across_three_seeds() {
        for seed in [42, 7, 99] {
            let world = World::generate(seed, WorldGeneration::GeographyV3);
            let life = VillageLife::new(&world);
            let plan = world.settlements().unwrap();
            let mut work_sites = std::collections::HashSet::new();
            for original in life
                .residents
                .iter()
                .filter(|r| r.snapshot.role == ResidentRole::Farmer)
            {
                let village = &plan.villages[original.village];
                let route = &village.resident_routes[original.route];
                let mut resident = original.clone();
                resident.body = Body::new(route.work);
                resident.phase = Phase::Working;
                resident.waypoint = route.path.len() - 1;
                resident.farm_path = farm_path(&world, village, resident.route);
                let first = resident
                    .farm_path
                    .iter()
                    .find(|p| p.work)
                    .unwrap_or_else(|| {
                        panic!(
                            "Seed {seed}, village {}, farmer {} cannot reach cultivated soil",
                            village.id, resident.route
                        )
                    });
                assert!(
                    work_sites.insert((
                        village.id,
                        (first.position[0] / CELL_SIZE).floor() as i32,
                        (first.position[2] / CELL_SIZE).floor() as i32
                    )),
                    "Farmers share a work site in seed {seed}"
                );
                let mut economy = life.villages[original.village].clone();
                let mut left_entrance = false;
                let mut completed = false;
                for _ in 0..5_000 {
                    advance_farmer(&world, village, &mut resident, &mut economy, 0.05, 1.0, &[]);
                    left_entrance |= resident.farm_waypoint > 0;
                    assert!(character_position_is_clear(
                        &world,
                        resident.body.position,
                        &[]
                    ));
                    if left_entrance && resident.farm_waypoint == 0 {
                        completed = true;
                        break;
                    }
                }
                assert!(
                    completed,
                    "Seed {seed}, village {}, farmer {} failed physical field roundtrip: {:?}, waypoint {}, target {:?}",
                    village.id,
                    resident.route,
                    resident.body.position,
                    resident.farm_waypoint,
                    resident.farm_path[resident.farm_waypoint].position
                );
                assert!(at_station(resident.body.position, route.work));
            }
        }
    }

    #[test]
    fn a_farmer_retraces_the_field_for_a_meal_and_sleep_then_resumes_the_saved_work() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let mut life = VillageLife::new(&world);
        life.residents.truncate(1);
        let village = &world.settlements().unwrap().villages[0];
        let path = farm_path(&world, village, 0);
        let task = path.iter().position(|p| p.work).unwrap();
        let resident = &mut life.residents[0];
        resident.phase = Phase::Working;
        resident.waypoint = village.resident_routes[0].path.len() - 1;
        resident.farm_waypoint = task;
        resident.body = Body::new(path[task].position);
        resident.snapshot.position = resident.body.position;
        resident.farm_path = path;
        resident.snapshot.hunger = 75.0;
        resident.snapshot.energy = 20.0;
        resident.elapsed = 0.7;
        let cargo = ResourceCargo {
            kind: ResourceKind::Food,
            amount: 5.0,
        };
        resident.snapshot.carrying = Some(cargo.clone());
        let stock_before = life.villages[0].snapshot.food;
        let mut exited = false;
        let mut resumed = false;
        for _ in 0..4_000 {
            life.tick(&world, 0.05);
            let resident = &life.residents[0];
            exited |= resident.phase == Phase::ToFarmExit;
            assert_eq!(resident.snapshot.carrying, Some(cargo.clone()));
            if exited && resident.phase == Phase::Working && resident.resume.is_none() {
                assert_eq!(resident.farm_waypoint, task);
                assert!((resident.elapsed - 0.7).abs() < 0.001);
                assert!(at_station(
                    resident.body.position,
                    resident.farm_path[task].position
                ));
                assert!(resident.snapshot.hunger < HUNGRY && resident.snapshot.energy > 75.0);
                resumed = true;
                break;
            }
        }
        assert!(exited && resumed, "{:?}", life.residents());
        assert_eq!(life.villages[0].snapshot.food, stock_before - MEAL);
    }

    #[test]
    fn saved_airborne_alternate_field_tour_survives_changed_access_and_delivers_its_cargo() {
        use crate::{npc::Forager, persistence::Simulation};
        let mut world = World::generate(42, WorldGeneration::GeographyV3);
        let mut life = VillageLife::new(&world);
        let plan = world.settlements().unwrap();
        let index = life
            .residents
            .iter()
            .position(|r| r.village == 6 && r.route == 1)
            .unwrap();
        let village = &plan.villages[6];
        let original_path = farm_path(&world, village, 1);
        assert!(original_path.iter().any(|p| p.route_gate));
        let gate = original_path
            .iter()
            .position(|p| {
                p.route_gate
                    && !village.fields.iter().any(|field| {
                        p.position[0] >= (field.origin.x - 24) as f32 * CELL_SIZE
                            && p.position[0]
                                <= (field.origin.x + field.width + 24) as f32 * CELL_SIZE
                            && p.position[2] >= (field.origin.z - 24) as f32 * CELL_SIZE
                            && p.position[2]
                                <= (field.origin.z + field.depth + 24) as f32 * CELL_SIZE
                    })
            })
            .unwrap();
        let resident = &mut life.residents[index];
        resident.phase = Phase::Working;
        resident.waypoint = village.resident_routes[1].path.len() - 1;
        resident.farm_waypoint = gate;
        resident.farm_path = original_path.clone();
        resident.body = Body::new(original_path[gate].position);
        move_character_with_obstacles(&world, &mut resident.body, MoveInput::default(), 0.1, &[]);
        move_character_with_obstacles(
            &world,
            &mut resident.body,
            MoveInput {
                jump: true,
                ..Default::default()
            },
            0.2,
            &[],
        );
        assert!(!resident.body.on_ground);
        assert!(resident.body.position[1] > original_path[gate].position[1] + 0.8);
        resident.snapshot.position = resident.body.position;
        resident.snapshot.hunger = 20.0;
        resident.snapshot.energy = 100.0;
        let cargo = ResourceCargo {
            kind: ResourceKind::Food,
            amount: 5.0,
        };
        resident.snapshot.carrying = Some(cargo.clone());
        let blocked = original_path
            .iter()
            .find(|p| !p.route_gate && !p.work && p.position != village.resident_routes[1].work)
            .unwrap()
            .position;
        let obstacle = BlockPos::new(
            (blocked[0] / CELL_SIZE).floor() as i32,
            (blocked[1] / CELL_SIZE).floor() as i32,
            (blocked[2] / CELL_SIZE).floor() as i32,
        );
        world.set_block(obstacle, Block::Brick).unwrap();
        assert_ne!(
            farm_path(&world, &world.settlements().unwrap().villages[6], 1),
            original_path
        );
        assert!(life.validate(&world));
        let file = std::env::temp_dir().join(format!(
            "rubblekin-farm-tour-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let sim = Simulation {
            npc: Forager::new(&world),
            world,
            world_time: 123.0,
            villages: life,
        };
        sim.save(&file).unwrap();
        let mut loaded = Simulation::load(&file, 999, WorldGeneration::GeographyV2).unwrap();
        std::fs::remove_file(&file).unwrap();
        assert_eq!(loaded.world.seed, 42);
        assert_eq!(loaded.villages.residents[index].farm_path, original_path);
        assert_eq!(loaded.villages.residents[index].farm_waypoint, gate);
        assert_eq!(
            loaded.villages.residents[index].snapshot.carrying,
            Some(cargo)
        );
        assert!(!loaded.villages.residents[index].body.on_ground);
        loaded.world.set_block(obstacle, Block::Air).unwrap();
        let farmer = loaded.villages.residents[index].clone();
        loaded.villages.residents = vec![farmer];
        let stock_before = loaded.villages.villages[6].snapshot.food;
        for _ in 0..5_000 {
            loaded.villages.tick(&loaded.world, 0.05);
            if loaded.villages.villages[6].deliveries > 0 {
                break;
            }
        }
        assert!(
            loaded.villages.villages[6].deliveries > 0,
            "{:?}",
            loaded.villages.residents()
        );
        assert_eq!(
            loaded.villages.villages[6].snapshot.food,
            stock_before + 5.0
        );
        assert!(loaded.villages.residents[0].snapshot.carrying.is_none());
    }

    #[test]
    fn farmers_can_hop_the_same_tall_step_while_working_retreating_and_resuming() {
        let geographic = World::generate(42, WorldGeneration::GeographyV3);
        let original = VillageLife::new(&geographic).residents[0].clone();
        let mut world = World::new(1);
        for x in 2..=8 {
            for z in -3..=3 {
                for y in 5..=6 {
                    world
                        .set_block(BlockPos::new(x, y, z), Block::Brick)
                        .unwrap();
                }
            }
        }
        let target = [1.25, 3.5, 0.25];
        for phase in [Phase::Working, Phase::ToFarmExit, Phase::ToFarmResume] {
            let mut resident = original.clone();
            resident.phase = phase;
            resident.body = Body::new([0.25, 2.5, 0.25]);
            let mut jumped = false;
            let mut reached = false;
            for _ in 0..200 {
                walk_toward(&world, &mut resident, target, 0.05, &[]);
                jumped |= resident.body.velocity[1] > 1.0;
                assert!(character_position_is_clear(
                    &world,
                    resident.body.position,
                    &[]
                ));
                if horizontal_distance(resident.body.position, target) < 0.12
                    && (resident.body.position[1] - target[1]).abs() < 0.15
                {
                    reached = true;
                    break;
                }
            }
            assert!(
                jumped && reached,
                "Farm phase {phase:?} could not hop the step: {:?}",
                resident.body
            );
        }
    }

    #[test]
    fn invalid_field_detour_resume_metadata_is_rejected_before_it_can_index_a_path() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let mut valid = VillageLife::new(&world);
        let village = &world.settlements().unwrap().villages[0];
        let path = farm_path(&world, village, 0);
        let task = path.iter().position(|p| p.work).unwrap();
        let resident = &mut valid.residents[0];
        resident.phase = Phase::ToFarmResume;
        resident.waypoint = village.resident_routes[0].path.len() - 1;
        resident.body = Body::new(path[0].position);
        resident.snapshot.position = resident.body.position;
        resident.farm_path = path;
        resident.resume = Some(Resume {
            phase: Phase::Working,
            waypoint: resident.waypoint,
            elapsed: 0.7,
            farm_waypoint: task,
        });
        assert!(valid.validate(&world));
        for phase in [Phase::ToFarmExit, Phase::ToFarmResume] {
            let mut invalid = valid.clone();
            invalid.residents[0].phase = phase;
            let resume = invalid.residents[0].resume.as_mut().unwrap();
            resume.phase = Phase::ToWork;
            resume.farm_waypoint = usize::MAX;
            assert!(!invalid.validate(&world));
            let mut invalid = valid.clone();
            invalid.residents[0].phase = phase;
            invalid.residents[0].resume.as_mut().unwrap().farm_waypoint = usize::MAX;
            assert!(!invalid.validate(&world));
            let mut invalid = valid.clone();
            invalid.residents[0].phase = phase;
            invalid.residents[0].resume.as_mut().unwrap().farm_waypoint = 0;
            assert!(!invalid.validate(&world));
            let mut invalid = valid.clone();
            invalid.residents[0].phase = phase;
            invalid.residents[0].farm_waypoint = task + 1;
            assert!(!invalid.validate(&world));
            let mut invalid = valid.clone();
            invalid.residents[0].phase = phase;
            invalid.residents[0].resume = None;
            assert!(!invalid.validate(&world));
        }
    }

    #[test]
    fn saved_mid_field_needs_retreat_restores_the_job_and_physical_cargo() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let mut life = VillageLife::new(&world);
        let village = &world.settlements().unwrap().villages[0];
        let path = farm_path(&world, village, 0);
        let task = path.iter().position(|p| p.work).unwrap();
        let resident = &mut life.residents[0];
        resident.phase = Phase::Working;
        resident.waypoint = village.resident_routes[0].path.len() - 1;
        resident.farm_waypoint = task;
        resident.body = Body::new(path[task].position);
        resident.snapshot.position = resident.body.position;
        resident.farm_path = path;
        resident.snapshot.hunger = 75.0;
        resident.snapshot.energy = 20.0;
        resident.elapsed = 0.7;
        let cargo = ResourceCargo {
            kind: ResourceKind::Food,
            amount: 5.0,
        };
        resident.snapshot.carrying = Some(cargo.clone());
        life.tick(&world, 0.05);
        assert_eq!(life.residents[0].phase, Phase::ToFarmExit);
        assert!(life.validate(&world));
        let saved_path = life.residents[0].farm_path.clone();
        let saved_waypoint = life.residents[0].farm_waypoint;
        let mut loaded: VillageLife =
            serde_json::from_slice(&serde_json::to_vec(&life).unwrap()).unwrap();
        assert!(loaded.validate(&world));
        assert_eq!(loaded.residents.len(), life.residents.len());
        assert_eq!(loaded.residents[0].farm_waypoint, saved_waypoint);
        assert_eq!(loaded.residents[0].farm_path, saved_path);
        assert_eq!(
            loaded.residents[0].resume.as_ref().unwrap().farm_waypoint,
            task
        );
        loaded.residents.truncate(1);
        let mut resumed = false;
        for _ in 0..4_000 {
            loaded.tick(&world, 0.05);
            let resident = &loaded.residents[0];
            assert_eq!(resident.snapshot.carrying, Some(cargo.clone()));
            if resident.phase == Phase::Working && resident.resume.is_none() {
                assert_eq!(resident.farm_waypoint, task);
                assert!((resident.elapsed - 0.7).abs() < 0.001);
                assert!(resident.snapshot.hunger < HUNGRY && resident.snapshot.energy > 75.0);
                resumed = true;
                break;
            }
        }
        assert!(resumed, "{:?}", loaded.residents());
    }

    #[test]
    fn meals_use_real_food_once_sleep_recovers_energy_and_jobs_keep_their_cargo() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let mut life = VillageLife::new(&world);
        life.residents.truncate(1);
        let route = &world.settlements().unwrap().villages[0].resident_routes[0];
        let resident = &mut life.residents[0];
        resident.phase = Phase::ToStore;
        resident.waypoint = route.store_index;
        resident.body = Body::new(route.path[route.store_index]);
        resident.snapshot.position = resident.body.position;
        resident.snapshot.hunger = 75.0;
        resident.snapshot.energy = 20.0;
        let cargo = ResourceCargo {
            kind: ResourceKind::Timber,
            amount: 3.0,
        };
        resident.snapshot.carrying = Some(cargo.clone());
        let food = life.villages[0].snapshot.food;
        let mut ate = false;
        let mut slept = false;
        let mut resumed = false;
        for _ in 0..3_000 {
            life.tick(&world, 0.05);
            let resident = &life.residents[0];
            ate |= resident.snapshot.action == ResidentAction::Eating;
            slept |= resident.snapshot.action == ResidentAction::Resting;
            assert_eq!(
                resident.snapshot.carrying,
                Some(cargo.clone()),
                "A needs trip must retain the job's physical cargo"
            );
            if ate && slept && resident.resume.is_none() {
                resumed = true;
                assert_eq!(resident.phase, Phase::ToStore);
                assert!(resident.snapshot.hunger < HUNGRY);
                assert!(resident.snapshot.energy > 80.0);
                break;
            }
        }
        assert!(ate && slept && resumed, "{:?}", life.residents());
        assert_eq!(life.villages[0].snapshot.food, food - MEAL);
    }

    #[test]
    fn food_shortage_slows_farm_work_without_consuming_or_creating_food() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let mut life = VillageLife::new(&world);
        life.residents.truncate(1);
        let village = &world.settlements().unwrap().villages[0];
        let path = farm_path(&world, village, 0);
        let task = path
            .iter()
            .position(|p| p.work)
            .expect("Generated farmer can enter the soil");
        let resident = &mut life.residents[0];
        resident.phase = Phase::Working;
        resident.waypoint = village.resident_routes[0].path.len() - 1;
        resident.farm_waypoint = task;
        resident.body = Body::new(path[task].position);
        resident.snapshot.position = resident.body.position;
        resident.farm_path = path;
        resident.snapshot.hunger = 75.0;
        life.villages[0].snapshot.food = 0.0;
        life.villages[0].snapshot.crop_growth = 0.5;
        for _ in 0..4 {
            life.tick(&world, 0.25);
        }
        assert!((life.residents[0].elapsed - 0.4).abs() < 0.001);
        assert_eq!(life.villages[0].snapshot.food, 0.0);
        assert!(life.residents[0].snapshot.reason.contains("farming slowly"));
        assert_eq!(life.residents[0].snapshot.action, ResidentAction::Tending);
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
