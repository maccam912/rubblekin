//! Small resident-owned work cycles. Goods enter storage only after delivery;
//! a trader withdraws actual surplus and carries it along a generated trail.
use rubblekin_core::{
    airships::{
        AIRSHIP_DWELL_SECONDS, AIRSHIP_TURN_SECONDS, AirshipNetwork, AirshipRide,
        MAX_AIRSHIP_SEATS, deck_position, initial_deck_position, ride_position,
    },
    economy::{WorkKind, WorkReward},
    physics::{
        Body, MoveInput, character_position_is_clear, move_character_with_airships,
        move_character_with_obstacles, resolve_character_overlaps,
    },
    protocol::{ResidentAction, ResidentRole, ResidentSnapshot, ResourceCargo, VillageSnapshot},
    settlement::{ResourceKind, SettlementPlan, Village},
    world::{Block, BlockPos, CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

use crate::{
    airships::free_seat,
    navigation::{NPC_WALK_SCALE, Navigation, Walker, Walking},
};

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
    /// Local crowd detours are disposable and rebuilt against live obstacles.
    #[serde(skip)]
    navigation: Navigation,
    trail: Option<usize>,
    #[serde(default)]
    resume: Option<Resume>,
    #[serde(default)]
    farm_waypoint: usize,
    #[serde(default)]
    farm_path: Vec<FarmPoint>,
    /// Travel changes how a resident reaches a goal, never their job or cargo.
    #[serde(default)]
    transit: Option<Transit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Transit {
    origin: u32,
    destination: u32,
    next_stop: u32,
    stage: TransitStage,
    waypoint: usize,
    ride: Option<AirshipRide>,
    #[serde(default)]
    reservation: Option<AirshipRide>,
    #[serde(default)]
    deck_position: Option<[f32; 3]>,
}

impl Transit {
    fn reserved_place(&self) -> Option<AirshipRide> {
        if self.stage == TransitStage::Boarding {
            self.reservation
        } else {
            self.ride.filter(|ride| ride.seat < MAX_AIRSHIP_SEATS)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum TransitStage {
    ToPort,
    Waiting,
    Boarding,
    Riding,
    Alighting,
    ToDestination,
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
                        ride: None,
                        deck_position: None,
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
                    navigation: Navigation::default(),
                    trail,
                    resume: None,
                    farm_waypoint: 0,
                    farm_path: Vec::new(),
                    transit: None,
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

    pub(crate) fn market_stocks(&self, id: u32) -> Option<&VillageSnapshot> {
        self.villages
            .iter()
            .find(|village| village.snapshot.id == id)
            .map(|village| &village.snapshot)
    }

    pub(crate) fn local_work_available(
        &self,
        village_id: u32,
        kind: WorkKind,
    ) -> Result<(), String> {
        let economy = self
            .villages
            .iter()
            .find(|village| village.snapshot.id == village_id)
            .ok_or("That village is unavailable.")?;
        match kind {
            WorkKind::GatherForage => Err("Wild food comes from its shared habitat.".into()),
            WorkKind::QuarryStone | WorkKind::Salvage => {
                Err("Loose resources come from their physical pile.".into())
            }
            WorkKind::TendField if economy.planted && economy.snapshot.crop_growth >= 1.0 => {
                Err("These crops are ready to harvest.".into())
            }
            WorkKind::HarvestField if !economy.planted || economy.snapshot.crop_growth < 1.0 => {
                Err("These crops are not ready to harvest.".into())
            }
            WorkKind::HarvestField
                if economy.snapshot.food < economy.snapshot.food_reserve + 12.0 =>
            {
                Err("The village needs this harvest for its food reserves.".into())
            }
            WorkKind::WorkshopMaintenance
                if economy.snapshot.timber < 9.0 || economy.snapshot.stone < 9.0 =>
            {
                Err("The workshop needs spare timber and stone above its reserves.".into())
            }
            _ => Ok(()),
        }
    }

    pub(crate) fn local_work_reward(
        &self,
        world: &World,
        village_id: u32,
        kind: WorkKind,
    ) -> Result<WorkReward, String> {
        Ok(match kind {
            WorkKind::GatherForage => return Err("Wild food comes from its shared habitat.".into()),
            WorkKind::QuarryStone | WorkKind::Salvage => {
                return Err("Loose resources come from their physical pile.".into());
            }
            WorkKind::TendField => WorkReward::Coins(2),
            WorkKind::WorkshopMaintenance => WorkReward::Coins(4),
            WorkKind::HarvestField => {
                let village = world
                    .settlements()
                    .and_then(|plan| {
                        plan.villages
                            .iter()
                            .find(|village| village.id == village_id)
                    })
                    .ok_or("That village is unavailable.")?;
                WorkReward::Cargo {
                    kind: ResourceKind::Food,
                    amount: (12.0 * cultivated_fraction(world, village)).floor() as u32,
                }
            }
        })
    }

    pub(crate) fn complete_local_work(
        &mut self,
        world: &World,
        village_id: u32,
        kind: WorkKind,
    ) -> Result<WorkReward, String> {
        self.local_work_available(village_id, kind)?;
        let reward = self.local_work_reward(world, village_id, kind)?;
        if matches!(reward, WorkReward::Cargo { amount: 0, .. }) {
            return Err("Not enough intact crops remain for one unit of food.".into());
        }
        let village = world
            .settlements()
            .and_then(|plan| {
                plan.villages
                    .iter()
                    .find(|village| village.id == village_id)
            })
            .ok_or("That village is unavailable.")?;
        let economy = self
            .villages
            .iter_mut()
            .find(|village| village.snapshot.id == village_id)
            .unwrap();
        match kind {
            WorkKind::GatherForage => return Err("Wild food comes from its shared habitat.".into()),
            WorkKind::QuarryStone | WorkKind::Salvage => {
                return Err("Loose resources come from their physical pile.".into());
            }
            WorkKind::TendField => {
                let cultivated = cultivated_fraction(world, village);
                if cultivated <= 0.0 {
                    return Err("This field needs intact planting soil.".into());
                }
                economy.cultivated_fraction = cultivated;
                if economy.planted {
                    economy.snapshot.crop_growth =
                        (economy.snapshot.crop_growth + 0.08 * cultivated).min(1.0);
                } else {
                    economy.planted = true;
                    economy.snapshot.crop_growth = 0.01;
                }
            }
            WorkKind::WorkshopMaintenance => {
                economy.snapshot.timber -= 1.0;
                economy.snapshot.stone -= 1.0;
            }
            WorkKind::HarvestField => {
                take_ripe_crop(economy, cultivated_fraction(world, village))
                    .ok_or("These crops are no longer ready to harvest.")?;
            }
        }
        Ok(reward)
    }

    pub(crate) fn change_market_stock(
        &mut self,
        id: u32,
        kind: ResourceKind,
        delta: f32,
    ) -> Result<(), String> {
        let economy = self
            .villages
            .iter_mut()
            .find(|village| village.snapshot.id == id)
            .ok_or("The market is unavailable.")?;
        let storage = stock_mut(&mut economy.snapshot, kind);
        let next = *storage + delta;
        if !next.is_finite() || !(0.0..=MAX_STOCK).contains(&next) {
            return Err("The market cannot accept that quantity right now.".into());
        }
        *storage = next;
        Ok(())
    }

    pub fn positions(&self) -> impl Iterator<Item = [f32; 3]> + '_ {
        self.residents.iter().map(|resident| resident.body.position)
    }

    /// Retire saved airship journeys; residents resume their ordinary walking goals.
    pub fn disable_airships(&mut self) {
        for resident in &mut self.residents {
            resident.transit = None;
            resident.snapshot.ride = None;
            resident.snapshot.deck_position = None;
        }
    }

    /// Riders retain their relative berth when the server restarts.
    pub fn sync_airship_riders(&mut self, network: &AirshipNetwork, time: f64) {
        for resident in &mut self.residents {
            if let Some(ride) = resident
                .transit
                .as_ref()
                .and_then(|transit| transit.ride.as_ref())
                && let Some(ship) = network.ship(ride.ship_id, time)
            {
                let transit = resident.transit.as_ref().unwrap();
                let local = transit
                    .deck_position
                    .unwrap_or_else(|| initial_deck_position(ride.seat));
                resident.body.position = deck_position(&ship, local);
                if transit.stage == TransitStage::Riding {
                    resident.body.velocity = [0.0; 3];
                    resident.body.on_ground = true;
                }
                resident.snapshot.position = resident.body.position;
                resident.snapshot.ride = Some(*ride);
                resident.snapshot.deck_position = Some(local);
            }
        }
    }

    pub fn validate_transport(&self, world: &World, network: &AirshipNetwork, time: f64) -> bool {
        if !time.is_finite() || time < 0.0 || !self.validate(world) {
            return false;
        }
        let mut occupied = Vec::new();
        for resident in &self.residents {
            let Some(transit) = &resident.transit else {
                continue;
            };
            let Some(port) = network.port(transit.origin) else {
                return false;
            };
            if let Some(reservation) = transit.reservation {
                let Some(ship) = network.ship(reservation.ship_id, time) else {
                    return false;
                };
                if !matches!(
                    transit.stage,
                    TransitStage::Boarding | TransitStage::Alighting
                ) || reservation.seat >= MAX_AIRSHIP_SEATS
                    || !(transit.stage == TransitStage::Alighting
                        && network
                            .landing_path(reservation.ship_id, transit.next_stop)
                            .is_some())
                        && !((ship.from_village == transit.origin
                            && ship.next_village == transit.next_stop)
                            || (ship.next_village == transit.origin
                                && ship.from_village == transit.next_stop))
                {
                    return false;
                }
                if transit.stage == TransitStage::Boarding {
                    if occupied.contains(&reservation) {
                        return false;
                    }
                    occupied.push(reservation);
                }
            }
            if transit
                .deck_position
                .is_some_and(|local| !local.iter().all(|value| value.is_finite()))
                || transit.ride.is_none() && transit.deck_position.is_some()
                || transit.stage == TransitStage::Riding
                    && transit
                        .ride
                        .is_some_and(|ride| ride.seat < MAX_AIRSHIP_SEATS)
                    && transit.deck_position.is_some()
            {
                return false;
            }
            let path_length = if matches!(
                transit.stage,
                TransitStage::Boarding | TransitStage::Alighting
            ) {
                let Some(reservation) = transit.reservation else {
                    return false;
                };
                let village = if transit.stage == TransitStage::Alighting {
                    transit.next_stop
                } else {
                    transit.origin
                };
                let Some(path) = network.landing_path(reservation.ship_id, village) else {
                    return false;
                };
                if reservation.seat >= MAX_AIRSHIP_SEATS {
                    return false;
                }
                path.len()
            } else {
                transit_path(world.settlements().unwrap(), network, port.village_id).len()
            };
            if network.port(transit.destination).is_none()
                || network.port(transit.next_stop).is_none()
                || transit.waypoint >= path_length
                || (transit.stage == TransitStage::ToDestination
                    && transit.origin != transit.destination)
                || (transit.stage != TransitStage::ToDestination
                    && transit.origin == transit.destination)
            {
                return false;
            }
            if let Some(ride) = &transit.ride {
                let Some(ship) = network.ship(ride.ship_id, time) else {
                    return false;
                };
                let local = transit
                    .deck_position
                    .unwrap_or_else(|| initial_deck_position(ride.seat));
                let unslotted = ride.seat == u8::MAX;
                // Older saves can record contact with another craft while
                // walking off a shared landing. Keep the real attachment only
                // when that craft also visits the intended exit port; the next
                // transit step reconciles its route/landing metadata. All pose,
                // local bounds and original reservation checks still apply.
                let incidental_exit = unslotted
                    && transit.stage == TransitStage::Alighting
                    && network
                        .landing_path(ride.ship_id, transit.next_stop)
                        .is_some();
                if (ride.seat >= MAX_AIRSHIP_SEATS && !unslotted)
                    || unslotted
                        && !matches!(
                            transit.stage,
                            TransitStage::Boarding | TransitStage::Riding | TransitStage::Alighting
                        )
                    || !unslotted && transit.stage != TransitStage::Riding
                    || !incidental_exit
                        && !((ship.from_village == transit.origin
                            && ship.next_village == transit.next_stop)
                            || (ship.next_village == transit.origin
                                && ship.from_village == transit.next_stop))
                    || !unslotted
                        && occupied.iter().any(|other: &AirshipRide| {
                            other.ship_id == ride.ship_id && other.seat == ride.seat
                        })
                    || resident
                        .body
                        .position
                        .iter()
                        .zip(deck_position(&ship, local))
                        .any(|(a, b)| (a - b).abs() > 0.05)
                    || resident
                        .snapshot
                        .deck_position
                        .is_some_and(|value| value != local)
                    || transit.stage == TransitStage::Riding && resident.body.velocity != [0.0; 3]
                    || unslotted && transit.deck_position.is_none()
                    || unslotted
                        && (local[0].abs()
                            > rubblekin_core::airships::AIRSHIP_DECK_HALF_WIDTH + 0.02
                            || local[2].abs()
                                > rubblekin_core::airships::AIRSHIP_DECK_HALF_LENGTH + 0.02
                            || local[1] < -0.03)
                    || transit.reservation.is_some_and(|reservation| {
                        reservation.ship_id != ride.ship_id && !incidental_exit
                    })
                {
                    return false;
                }
                if !unslotted {
                    occupied.push(*ride);
                }
            }
        }
        true
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
                                    && farm_needs_entrance(
                                        &resident.farm_path,
                                        resume.farm_waypoint,
                                    ) == 0
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
                || resident.transit.as_ref().is_some_and(|transit| {
                    !matches!(resident.phase, Phase::ToTrade | Phase::Returning)
                        || transit.destination != travel_destination(plan, resident)
                        || !plan.villages.iter().any(|v| v.id == transit.origin)
                        || !plan.villages.iter().any(|v| v.id == transit.next_stop)
                        || transit.stage == TransitStage::Riding && transit.ride.is_none()
                        || !matches!(
                            transit.stage,
                            TransitStage::Riding | TransitStage::Boarding | TransitStage::Alighting
                        ) && transit.ride.is_some()
                })
                || resident.snapshot.ride
                    != resident.transit.as_ref().and_then(|transit| transit.ride)
                || resident
                    .snapshot
                    .deck_position
                    .is_some_and(|local| !local.iter().all(|v| v.is_finite()))
                || resident.snapshot.ride.is_none() && resident.snapshot.deck_position.is_some()
                || resident.transit.as_ref().is_some_and(|transit| {
                    transit.deck_position.is_some()
                        && resident.snapshot.deck_position != transit.deck_position
                })
            {
                return false;
            }
        }
        true
    }

    pub fn resolve_overlaps(&mut self, world: &World, external: &[[f32; 3]]) {
        let mut positions: Vec<_> = self.positions().collect();
        for (index, resident) in self.residents.iter_mut().enumerate() {
            if resident
                .transit
                .as_ref()
                .is_some_and(|transit| transit.ride.is_some())
            {
                // The transport owns support beneath seated passengers.
                continue;
            }
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

    #[cfg(test)]
    pub fn tick_with_obstacles(&mut self, world: &World, dt: f32, external: &[[f32; 3]]) {
        self.tick_internal(world, dt, external, None);
    }

    pub fn tick_with_transport(
        &mut self,
        world: &World,
        dt: f32,
        external: &[[f32; 3]],
        network: &AirshipNetwork,
        time: f64,
        external_rides: &[AirshipRide],
    ) {
        self.tick_internal(world, dt, external, Some((network, time, external_rides)));
    }

    fn tick_internal(
        &mut self,
        world: &World,
        dt: f32,
        external: &[[f32; 3]],
        transport: Option<(&AirshipNetwork, f64, &[AirshipRide])>,
    ) {
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
        Navigation::limit_searches(self.residents.iter_mut().map(|r| &mut r.navigation), 2);
        let mut positions: Vec<_> = self.positions().collect();
        let mut rides: Vec<_> = self
            .residents
            .iter()
            .map(|resident| resident.transit.as_ref().and_then(Transit::reserved_place))
            .collect();
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
            if let Some((network, time, external_rides)) = transport {
                let mut occupied = external_rides.to_vec();
                occupied.extend(
                    rides
                        .iter()
                        .enumerate()
                        .filter_map(|(other, ride)| (other != index).then_some(*ride).flatten()),
                );
                if advance_transit(
                    world, plan, resident, dt, &obstacles, network, time, &occupied,
                ) {
                    resident.snapshot.position = resident.body.position;
                    resident.snapshot.ride =
                        resident.transit.as_ref().and_then(|transit| transit.ride);
                    positions[index] = resident.body.position;
                    resident.snapshot.deck_position =
                        resident.transit.as_ref().and_then(|transit| {
                            transit.deck_position.or_else(|| {
                                transit.ride.map(|ride| initial_deck_position(ride.seat))
                            })
                        });
                    rides[index] = resident.transit.as_ref().and_then(Transit::reserved_place);
                    continue;
                }
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
                        if farm_needs_entrance(&resident.farm_path, resident.farm_waypoint) == 0 {
                            resident.farm_waypoint -= 1;
                        }
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

fn travel_destination(plan: &SettlementPlan, resident: &Resident) -> u32 {
    if resident.phase == Phase::Returning {
        return resident.snapshot.village_id;
    }
    let trail = &plan.trails[resident.trail.unwrap()];
    if trail.from == resident.snapshot.village_id {
        trail.to
    } else {
        trail.from
    }
}

fn travel_origin(plan: &SettlementPlan, resident: &Resident) -> u32 {
    if resident.phase == Phase::ToTrade {
        return resident.snapshot.village_id;
    }
    let trail = &plan.trails[resident.trail.unwrap()];
    if trail.from == resident.snapshot.village_id {
        trail.to
    } else {
        trail.from
    }
}

fn transit_path(plan: &SettlementPlan, network: &AirshipNetwork, village: u32) -> Vec<[f32; 3]> {
    let Some(village) = plan.villages.iter().find(|v| v.id == village) else {
        return Vec::new();
    };
    let Some(port) = network.port(village.id) else {
        return Vec::new();
    };
    port.approach.clone()
}

fn nearest_waypoint(position: [f32; 3], path: &[[f32; 3]]) -> usize {
    path.iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            horizontal_distance(position, **a).total_cmp(&horizontal_distance(position, **b))
        })
        .map_or(0, |(index, _)| index)
}

fn path_distance(position: [f32; 3], path: &[[f32; 3]]) -> f32 {
    path.first()
        .map_or(0.0, |&first| horizontal_distance(position, first))
        + path
            .windows(2)
            .map(|p| horizontal_distance(p[0], p[1]))
            .sum::<f32>()
}

/// An inter-village job keeps its existing destination and cargo underneath a
/// transport leg. Local jobs and journeys already on a trail remain walking.
#[allow(clippy::too_many_arguments)]
fn advance_transit(
    world: &World,
    plan: &SettlementPlan,
    resident: &mut Resident,
    dt: f32,
    obstacles: &[[f32; 3]],
    network: &AirshipNetwork,
    time: f64,
    occupied: &[AirshipRide],
) -> bool {
    if !matches!(resident.phase, Phase::ToTrade | Phase::Returning) {
        return false;
    }
    if resident.transit.is_none() {
        let origin = travel_origin(plan, resident);
        let destination = travel_destination(plan, resident);
        let trail = &plan.trails[resident.trail.unwrap()];
        let forward =
            (trail.from == resident.snapshot.village_id) == (resident.phase == Phase::ToTrade);
        let start = if forward { 0 } else { trail.points.len() - 1 };
        let source = plan.villages.iter().find(|v| v.id == origin).unwrap();
        if resident.waypoint != start
            || !(at_station(resident.body.position, source.center)
                || at_station(resident.body.position, source.store))
        {
            return false;
        }
        let path = transit_path(plan, network, origin);
        let Some(destination_port) = network.port(destination) else {
            return false;
        };
        if path.is_empty() {
            return false;
        }
        let waypoint = nearest_waypoint(resident.body.position, &path);
        let approach_time = path_distance(resident.body.position, &path[waypoint..]) / 1.976;
        let Some(leg) = network.next_leg(origin, destination, time + approach_time as f64) else {
            return false;
        };
        let Some(boarding_path) = network.landing_path(leg.ship_id, origin) else {
            return false;
        };
        let boarding_time =
            path_distance(network.port(origin).unwrap().position, boarding_path) / 1.976;
        let Some(leg) = network.next_leg(
            origin,
            destination,
            time + (approach_time + boarding_time) as f64,
        ) else {
            return false;
        };
        let final_path = transit_path(plan, network, destination);
        let exit_time = path_distance(
            destination_port.position,
            &final_path.iter().rev().copied().collect::<Vec<_>>(),
        ) / 1.976;
        let landing_time = network
            .landing_path(leg.ship_id, destination)
            .map_or(0.0, |path| {
                path.windows(2)
                    .map(|p| horizontal_distance(p[0], p[1]))
                    .sum::<f32>()
                    / 1.976
            });
        let walk_time = (horizontal_distance(resident.body.position, trail.points[start])
            + trail
                .points
                .windows(2)
                .map(|p| horizontal_distance(p[0], p[1]))
                .sum::<f32>())
            / 1.976;
        if approach_time
            + boarding_time
            + leg.destination_arrival_in
            + AIRSHIP_TURN_SECONDS as f32
            + landing_time
            + exit_time
            >= walk_time
        {
            return false;
        }
        resident.transit = Some(Transit {
            origin,
            destination,
            next_stop: leg.destination,
            stage: TransitStage::ToPort,
            waypoint,
            ride: None,
            reservation: None,
            deck_position: None,
        });
    }
    let mut transit = resident.transit.take().unwrap();
    let path = transit_path(plan, network, transit.origin);
    let port = network.port(transit.origin).unwrap();
    match transit.stage {
        TransitStage::ToPort | TransitStage::ToDestination => {
            let forward = transit.stage == TransitStage::ToPort;
            let end = if forward { path.len() - 1 } else { 0 };
            let target = path[transit.waypoint];
            resident.snapshot.action = ResidentAction::Walking;
            resident.snapshot.target = Some(target);
            resident.snapshot.reason = if forward {
                "Airship is quicker; walking to the village port"
            } else {
                "Arrived by airship; walking to the trade destination"
            }
            .into();
            if if transit.waypoint == end {
                at_station(resident.body.position, target)
            } else {
                arrived(resident.body.position, target)
            } {
                resident.stuck = 0.0;
                if transit.waypoint != end {
                    transit.waypoint = if forward {
                        transit.waypoint + 1
                    } else {
                        transit.waypoint - 1
                    };
                } else if forward {
                    transit.stage = TransitStage::Waiting;
                } else {
                    let trail = &plan.trails[resident.trail.unwrap()];
                    let forward = (trail.from == resident.snapshot.village_id)
                        == (resident.phase == Phase::ToTrade);
                    resident.waypoint = if forward { trail.points.len() - 1 } else { 0 };
                    return true;
                }
            } else {
                walk_toward(world, resident, target, dt, obstacles);
                if resident.stuck >= 8.0 {
                    resident.snapshot.action = ResidentAction::Blocked;
                    resident.snapshot.reason =
                        "The walking approach to the airship port is blocked".into();
                }
            }
        }
        TransitStage::Waiting => {
            move_character_with_obstacles(
                world,
                &mut resident.body,
                MoveInput::default(),
                dt,
                obstacles,
            );
            resident.snapshot.action = ResidentAction::WaitingForAirship;
            resident.snapshot.target = Some(port.position);
            resident.snapshot.reason =
                "Waiting at the port for an airship toward the destination".into();
            if !at_station(resident.body.position, port.position) {
                transit.stage = TransitStage::ToPort;
                transit.waypoint = nearest_waypoint(resident.body.position, &path);
            } else if let Some(leg) = network.next_leg(transit.origin, transit.destination, time)
                && let Some(ship) = network.ship(leg.ship_id, time)
                && let Some(ride) = free_seat(world, &ship, occupied, obstacles)
            {
                transit.reservation = Some(ride);
                transit.next_stop = leg.destination;
                transit.stage = TransitStage::Boarding;
                transit.waypoint = 0;
            }
        }
        TransitStage::Boarding => {
            reconcile_boarding_contact(
                world,
                resident,
                &mut transit,
                obstacles,
                network,
                time,
                occupied,
            );
            if transit.stage == TransitStage::Riding {
                resident.transit = Some(transit);
                return true;
            }
            let booked_ship = network
                .ship(transit.reservation.unwrap().ship_id, time)
                .unwrap();
            if transit.ride.is_none()
                && booked_ship.from_village == transit.origin
                && booked_ship.docked_at != Some(transit.origin)
                && let Some(leg) = network.next_leg(transit.origin, transit.destination, time)
                && let Some(ship) = network.ship(leg.ship_id, time)
                && let Some(place) = free_seat(world, &ship, occupied, obstacles)
            {
                // Fleet mates share a berth. Missing a departure does not send
                // the resident back through the entire village approach.
                if leg.ship_id != booked_ship.id || leg.destination != transit.next_stop {
                    let path = network.landing_path(leg.ship_id, transit.origin).unwrap();
                    transit.waypoint = nearest_waypoint(resident.body.position, path);
                }
                transit.reservation = Some(place);
                transit.next_stop = leg.destination;
            }
            let reservation = transit.reservation.unwrap();
            let ship = network.ship(reservation.ship_id, time).unwrap();
            let path = network
                .landing_path(reservation.ship_id, transit.origin)
                .unwrap();
            bypass_occupied_landing_gate(
                world,
                resident,
                &mut transit,
                path,
                true,
                obstacles,
                network,
                time,
            );
            let target = if transit.ride.is_some() {
                ride_position(&ship, reservation.seat)
            } else {
                path[transit.waypoint]
            };
            resident.snapshot.action = ResidentAction::Walking;
            resident.snapshot.target = Some(target);
            resident.snapshot.reason = "Walking along the landing and onto the airship deck".into();
            if transit.ride.is_some()
                && horizontal_distance(resident.body.position, target) <= 0.03
                && (resident.body.position[1] - target[1]).abs() <= 0.05
            {
                resident.body.position = target;
                resident.body.velocity = [0.0; 3];
                resident.body.on_ground = true;
                transit.ride = Some(reservation);
                transit.reservation = None;
                transit.deck_position = None;
                transit.waypoint = 0;
                transit.stage = TransitStage::Riding;
            } else {
                if transit.ride.is_none()
                    && horizontal_distance(resident.body.position, target) <= ARRIVAL
                    && transit.waypoint + 1 < path.len()
                {
                    transit.waypoint += 1;
                }
                walk_on_transport(
                    world,
                    resident,
                    target,
                    dt,
                    obstacles,
                    network,
                    time,
                    &mut transit,
                );
                // Contact may be made with another fleet mate during this
                // movement step; persist the actual craft immediately.
                reconcile_boarding_contact(
                    world,
                    resident,
                    &mut transit,
                    obstacles,
                    network,
                    time,
                    occupied,
                );
            }
        }
        TransitStage::Riding => {
            let ride = transit.ride.unwrap();
            let ship = network
                .ship(ride.ship_id, time)
                .expect("a validated airship remains scheduled");
            let local = transit
                .deck_position
                .unwrap_or_else(|| initial_deck_position(ride.seat));
            resident.body.position = deck_position(&ship, local);
            resident.body.velocity = [0.0; 3];
            resident.body.on_ground = true;
            resident.snapshot.action = ResidentAction::RidingAirship;
            resident.snapshot.target = network.port(transit.next_stop).map(|port| port.position);
            resident.snapshot.reason = if ship.docked_at == Some(transit.next_stop) {
                "Arrived; waiting for the airship to finish turning at the landing"
            } else {
                "Riding an airship toward the trade destination"
            }
            .into();
            if ship.docked_at == Some(transit.next_stop)
                && ship.departure_in as f64 <= AIRSHIP_DWELL_SECONDS - AIRSHIP_TURN_SECONDS
            {
                transit.reservation = Some(AirshipRide {
                    ship_id: ride.ship_id,
                    seat: if ride.seat < MAX_AIRSHIP_SEATS {
                        ride.seat
                    } else {
                        0
                    },
                });
                transit.ride = Some(AirshipRide {
                    ship_id: ride.ship_id,
                    seat: u8::MAX,
                });
                transit.deck_position = Some(local);
                transit.waypoint = network
                    .landing_path(ride.ship_id, transit.next_stop)
                    .unwrap()
                    .len()
                    - 1;
                transit.stage = TransitStage::Alighting;
                resident.stuck = 0.0;
            }
        }
        TransitStage::Alighting => {
            reconcile_alighting_contact(&mut transit, network);
            let reservation = transit.reservation.unwrap();
            let ship = network.ship(reservation.ship_id, time).unwrap();
            let path = network
                .landing_path(reservation.ship_id, transit.next_stop)
                .unwrap();
            bypass_occupied_landing_gate(
                world,
                resident,
                &mut transit,
                path,
                false,
                obstacles,
                network,
                time,
            );
            if transit.ride.is_some() {
                let pilot = rubblekin_core::airships::pilot_position(&ship);
                while transit.waypoint > 0
                    && obstacles
                        .iter()
                        .chain(std::iter::once(&pilot))
                        .any(|position| {
                            horizontal_distance(path[transit.waypoint], *position) < 0.9
                                && (path[transit.waypoint][1] - position[1]).abs() < 1.7
                        })
                {
                    transit.waypoint -= 1;
                }
            }
            let target = path[transit.waypoint];
            resident.snapshot.action = ResidentAction::Walking;
            resident.snapshot.target = Some(target);
            resident.snapshot.reason =
                "Walking off the airship and back to the village road".into();
            if transit.ride.is_some()
                && (ship.docked_at != Some(transit.next_stop)
                    || ship.departure_in as f64 > AIRSHIP_DWELL_SECONDS - AIRSHIP_TURN_SECONDS)
            {
                move_character_with_airships(
                    world,
                    &mut resident.body,
                    MoveInput::default(),
                    dt,
                    obstacles,
                    network,
                    time,
                    &mut transit.ride,
                    &mut transit.deck_position,
                );
                resident.snapshot.action = ResidentAction::RidingAirship;
                resident.snapshot.reason = if ship.docked_at == Some(transit.next_stop) {
                    "Arrived; waiting for the airship to finish turning at the landing"
                } else {
                    "Landing was blocked; staying aboard until the destination docks again"
                }
                .into();
            } else if horizontal_distance(resident.body.position, target) <= ARRIVAL {
                if transit.waypoint > 0 {
                    transit.waypoint -= 1;
                } else {
                    transit.origin = transit.next_stop;
                    transit.ride = None;
                    transit.reservation = None;
                    transit.deck_position = None;
                    transit.waypoint = transit_path(plan, network, transit.origin).len() - 1;
                    transit.stage = if transit.origin == transit.destination {
                        TransitStage::ToDestination
                    } else {
                        TransitStage::Waiting
                    };
                }
            } else {
                walk_on_transport(
                    world,
                    resident,
                    target,
                    dt,
                    obstacles,
                    network,
                    time,
                    &mut transit,
                );
            }
            reconcile_alighting_contact(&mut transit, network);
        }
    }
    resident.transit = Some(transit);
    true
}

fn reconcile_alighting_contact(transit: &mut Transit, network: &AirshipNetwork) {
    let Some(ride) = transit.ride else {
        return;
    };
    if transit
        .reservation
        .is_some_and(|r| r.ship_id == ride.ship_id)
    {
        return;
    }
    let Some(path) = network.landing_path(ride.ship_id, transit.next_stop) else {
        // Like boarding, crossing an unrelated dock does not attach this job
        // to a craft that cannot reach its intended port. The pier remains.
        transit.ride = None;
        transit.deck_position = None;
        return;
    };
    transit.reservation = Some(AirshipRide {
        ship_id: ride.ship_id,
        seat: 0,
    });
    // An attached passenger exits from this craft's deck when it docks again,
    // even if its airborne position is now nearer a different static gate.
    transit.waypoint = path.len() - 1;
}

#[allow(clippy::too_many_arguments)]
fn bypass_occupied_landing_gate(
    world: &World,
    resident: &Resident,
    transit: &mut Transit,
    path: &[[f32; 3]],
    forward: bool,
    obstacles: &[[f32; 3]],
    network: &AirshipNetwork,
    time: f64,
) {
    let end = if forward { path.len() - 1 } else { 0 };
    if transit.ride.is_some()
        || transit.waypoint == end
        || resident.navigation.stalled < 0.3
        || !obstacles
            .iter()
            .any(|p| rubblekin_core::physics::characters_overlap(path[transit.waypoint], *p))
    {
        return;
    }
    let next = if forward {
        transit.waypoint + 1
    } else {
        transit.waypoint - 1
    };
    // A character can occupy the exact gate even when there is room to pass.
    // Preserve corners/steps with a terrain-and-ramp probe; live movement still
    // steers around that character. Endpoints and reserved deck places stay exact.
    if (Walking {
        world,
        obstacles: &[],
        airships: Some((network, time)),
    })
    .has_straight_path(&Walker::on_foot(&resident.body), path[next])
    {
        transit.waypoint = next;
    }
}

#[allow(clippy::too_many_arguments)]
fn reconcile_boarding_contact(
    world: &World,
    resident: &mut Resident,
    transit: &mut Transit,
    obstacles: &[[f32; 3]],
    network: &AirshipNetwork,
    time: f64,
    occupied: &[AirshipRide],
) {
    let Some(ride) = transit.ride else {
        return;
    };
    let ship = network.ship(ride.ship_id, time).unwrap();
    if ship.from_village != transit.origin || ship.next_village != transit.next_stop {
        // A road can cross another landing. Its static pier still supports the
        // walk, but a craft on another route must not carry this job away.
        transit.ride = None;
        transit.deck_position = None;
        return;
    }
    let reservation = transit.reservation.unwrap();
    if reservation.ship_id == ride.ship_id
        && !occupied.contains(&reservation)
        && resident.stuck < 8.0
    {
        return;
    }
    if let Some(place) = free_seat(world, &ship, occupied, obstacles) {
        transit.reservation = Some(place);
        resident.stuck = 0.0;
    } else {
        // The NPC is already physically aboard. Keep its actual standing
        // position if the reserved spots filled while it was approaching.
        transit.reservation = None;
        transit.stage = TransitStage::Riding;
        transit.waypoint = 0;
        resident.body.velocity = [0.0; 3];
        resident.body.on_ground = true;
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_on_transport(
    world: &World,
    resident: &mut Resident,
    target: [f32; 3],
    dt: f32,
    obstacles: &[[f32; 3]],
    network: &AirshipNetwork,
    time: f64,
    transit: &mut Transit,
) {
    let walking = Walking {
        world,
        obstacles,
        airships: Some((network, time)),
    };
    let mut walker = Walker {
        body: resident.body.clone(),
        ride: transit.ride,
        local: transit.deck_position,
    };
    walking.walk(
        &mut walker,
        &mut resident.navigation,
        target,
        0.52,
        false,
        dt,
        |_| true,
    );
    resident.body = walker.body;
    transit.ride = walker.ride;
    transit.deck_position = walker.local;
    resident.stuck = resident.navigation.stalled;
    if resident.stuck >= 8.0 {
        resident.snapshot.action = ResidentAction::Blocked;
        resident.snapshot.reason = "The airship boarding or landing path is blocked".into();
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
                        resident.farm_waypoint =
                            farm_needs_entrance(&resident.farm_path, resume.farm_waypoint);
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
    let delivering_at_store = resident.phase == Phase::ToHome
        && resident.snapshot.carrying.is_some()
        && resident.waypoint == route.store_index;
    if resident.waypoint != end && !delivering_at_store && resident.stuck >= 0.3 {
        let next = if forward {
            resident.waypoint + 1
        } else {
            resident.waypoint - 1
        };
        if obstacles
            .iter()
            .any(|p| rubblekin_core::physics::characters_overlap(target, *p))
            && farm_edge_is_walkable(world, resident.body.position, points[next])
        {
            // A shared junction is a transit gate, not a workstation. Walk
            // onward only when the actual terrain controller can reach it.
            resident.waypoint = next;
            resident.stuck = 0.0;
            resident.snapshot.target = Some(points[next]);
            walk_toward(world, resident, points[next], dt, obstacles);
            return;
        }
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
                let factor = 0.52_f32.min(distance / (3.8 * 0.05)) * NPC_WALK_SCALE;
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

/// A closed field tour has the same entrance at either end. Use the shorter
/// physical branch for a needs trip instead of traversing the rest of the tour.
fn farm_needs_entrance(path: &[FarmPoint], task: usize) -> usize {
    let distance = |points: &[FarmPoint]| {
        points
            .windows(2)
            .map(|points| horizontal_distance(points[0].position, points[1].position))
            .sum::<f32>()
    };
    if distance(&path[..=task]) <= distance(&path[task..]) {
        0
    } else {
        path.len() - 1
    }
}

/// A collision sidestep can leave a narrow field ledge and make its next gate
/// unreachable. Rejoin an earlier nearby gate by walking over proven terrain,
/// keeping the saved work task, elapsed progress, and physical cargo intact.
fn rejoin_farm_path(world: &World, resident: &mut Resident) -> bool {
    if resident.stuck < 1.0
        || resident.navigation.is_detouring()
        || !resident.body.on_ground
        || resident.farm_waypoint == 0
    {
        return false;
    }
    let current = &resident.farm_path[resident.farm_waypoint];
    if current.work || farm_edge_is_walkable(world, resident.body.position, current.position) {
        return false;
    }
    let candidate_after = match resident.phase {
        Phase::ToFarmExit => {
            farm_needs_entrance(
                &resident.farm_path,
                resident.resume.as_ref().unwrap().farm_waypoint,
            ) > resident.farm_waypoint
        }
        Phase::ToFarmResume => {
            resident.resume.as_ref().unwrap().farm_waypoint < resident.farm_waypoint
        }
        _ => false,
    };
    let candidate = resident
        .farm_path
        .iter()
        .enumerate()
        .filter(|(index, point)| {
            (if candidate_after {
                *index > resident.farm_waypoint
            } else {
                *index < resident.farm_waypoint
            }) && !point.work
                && horizontal_distance(resident.body.position, point.position) <= 2.0
        })
        .filter(|(_, point)| farm_edge_is_walkable(world, resident.body.position, point.position))
        .min_by(|(_, a), (_, b)| {
            horizontal_distance(resident.body.position, a.position)
                .total_cmp(&horizontal_distance(resident.body.position, b.position))
        })
        .map(|(index, _)| index);
    if let Some(index) = candidate {
        resident.farm_waypoint = index;
        resident.stuck = 0.0;
        true
    } else {
        false
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
    let task = resident.resume.as_ref().unwrap().farm_waypoint;
    let end = if exiting {
        farm_needs_entrance(&resident.farm_path, task)
    } else {
        task
    };
    let next = if resident.farm_waypoint < end {
        resident.farm_waypoint + 1
    } else if resident.farm_waypoint > end {
        resident.farm_waypoint - 1
    } else {
        end
    };
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
        if rejoin_farm_path(world, resident)
            || skip_occupied_farm_gate(world, resident, next, obstacles)
        {
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
    if exiting && resident.farm_waypoint == end {
        if end == resident.farm_path.len() - 1
            && resident
                .farm_path
                .iter()
                .rposition(|point| point.work)
                .is_some_and(|last_work| task > last_work)
        {
            // This needs trip already completed the return from the crops.
            // Resume here, rather than walking back along the finished leg
            // only to return again (and potentially need another meal).
            resident.resume.as_mut().unwrap().farm_waypoint = end;
        }
        resident.phase = if resident.snapshot.hunger >= HUNGRY && economy.snapshot.food >= MEAL {
            Phase::ToFood
        } else if resident.snapshot.energy <= TIRED {
            Phase::ToRest
        } else {
            Phase::Resuming
        };
    } else if !exiting && resident.farm_waypoint == end {
        let resume = resident.resume.take().unwrap();
        resident.phase = Phase::Working;
        resident.elapsed = resume.elapsed;
    } else {
        resident.farm_waypoint = next;
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
        if rejoin_farm_path(world, resident)
            || skip_occupied_farm_gate(world, resident, resident.farm_waypoint + 1, obstacles)
        {
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
                if let Some(amount) = take_ripe_crop(economy, cultivated_fraction(world, village)) {
                    resident.snapshot.carrying = Some(ResourceCargo {
                        kind: ResourceKind::Food,
                        amount,
                    });
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
    let jump = resident.snapshot.role == ResidentRole::Farmer
        && matches!(
            resident.phase,
            Phase::Working | Phase::ToFarmExit | Phase::ToFarmResume
        )
        && resident.stuck >= 0.2
        && resident.body.on_ground
        && target[1] - resident.body.position[1] > 0.3
        && distance < 1.25;
    let precision_farm_gate = matches!(
        resident.phase,
        Phase::Working | Phase::ToFarmExit | Phase::ToFarmResume
    ) && resident
        .farm_path
        .get(resident.farm_waypoint)
        .is_some_and(|point| !point.work && !point.route_gate);
    let mut walker = Walker::on_foot(&resident.body);
    Walking {
        world,
        obstacles,
        airships: None,
    }
    .walk(
        &mut walker,
        &mut resident.navigation,
        target,
        0.52,
        jump,
        dt,
        |body| !precision_farm_gate || farm_edge_is_walkable(world, body.position, target),
    );
    resident.body = walker.body;
    resident.stuck = resident.navigation.stalled;
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

/// NPC and player harvests consume the same village crop exactly once. Stores
/// receive no food until the harvester physically brings its cargo to them.
fn take_ripe_crop(economy: &mut Economy, cultivated: f32) -> Option<f32> {
    if !economy.planted || economy.snapshot.crop_growth < 1.0 || cultivated <= 0.0 {
        return None;
    }
    economy.cultivated_fraction = cultivated;
    economy.snapshot.crop_growth = 0.0;
    economy.planted = false;
    economy.harvests = economy.harvests.saturating_add(1);
    Some(12.0 * cultivated)
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

    fn departing_trader(world: &World, life: &mut VillageLife) -> usize {
        let plan = world.settlements().unwrap();
        let index = life
            .residents
            .iter()
            .enumerate()
            .filter(|(_, resident)| resident.trail.is_some())
            .max_by(|(_, a), (_, b)| {
                let length = |resident: &Resident| {
                    plan.trails[resident.trail.unwrap()]
                        .points
                        .windows(2)
                        .map(|p| horizontal_distance(p[0], p[1]))
                        .sum::<f32>()
                };
                length(a).total_cmp(&length(b))
            })
            .unwrap()
            .0;
        let resident = &mut life.residents[index];
        let trail = &plan.trails[resident.trail.unwrap()];
        resident.phase = Phase::ToTrade;
        resident.waypoint = if trail.from == resident.snapshot.village_id {
            0
        } else {
            trail.points.len() - 1
        };
        resident.body = Body::new(plan.villages[resident.village].store);
        resident.snapshot.position = resident.body.position;
        resident.snapshot.carrying = Some(ResourceCargo {
            kind: ResourceKind::Timber,
            amount: 6.0,
        });
        index
    }

    #[test]
    fn every_incident_landing_allows_physical_npc_boarding_and_alighting() {
        for seed in [42, 43] {
            let world = World::generate(seed, WorldGeneration::GeographyV3);
            let network = AirshipNetwork::new(&world);
            let plan = world.settlements().unwrap();
            let life = VillageLife::new(&world);
            for route in network.routes() {
                for (origin, destination) in [(route.from, route.to), (route.to, route.from)] {
                    let mut resident = life
                        .residents
                        .iter()
                        .find(|r| r.snapshot.village_id == origin && r.trail.is_some())
                        .unwrap()
                        .clone();
                    resident.trail = Some(
                        plan.trails
                            .iter()
                            .position(|trail| {
                                trail.from == origin && trail.to == destination
                                    || trail.to == origin && trail.from == destination
                            })
                            .unwrap(),
                    );
                    resident.phase = Phase::ToTrade;
                    let leg = network.next_leg(origin, destination, 0.0).unwrap();
                    let time = (leg.departure_in as f64 - 1.0).max(0.0);
                    let ship = network.ship(leg.ship_id, time).unwrap();
                    assert_eq!(ship.docked_at, Some(origin));
                    let path = network.landing_path(ship.id, origin).unwrap();
                    resident.body = Body::new(path[0]);
                    resident.transit = Some(Transit {
                        origin,
                        destination,
                        next_stop: destination,
                        stage: TransitStage::Boarding,
                        waypoint: 0,
                        ride: None,
                        reservation: Some(AirshipRide {
                            ship_id: ship.id,
                            seat: 0,
                        }),
                        deck_position: None,
                    });
                    for boarding in [true, false] {
                        let mut tick_time = time;
                        if !boarding {
                            // Start once this craft has turned, then advance
                            // the clock throughout the exit. A shared landing
                            // can contact another craft still turning; a frozen
                            // clock would make its legitimate wait permanent.
                            tick_time = (time
                                - (AIRSHIP_DWELL_SECONDS - AIRSHIP_TURN_SECONDS - 1.0))
                                .max(0.0);
                            let transit = resident.transit.as_mut().unwrap();
                            let ride = transit.ride.unwrap();
                            transit.stage = TransitStage::Alighting;
                            transit.origin = destination;
                            transit.destination = origin;
                            transit.next_stop = origin;
                            transit.waypoint = path.len() - 1;
                            transit.reservation = Some(ride);
                            transit.deck_position = Some(initial_deck_position(ride.seat));
                            transit.ride = Some(AirshipRide {
                                ship_id: ship.id,
                                seat: u8::MAX,
                            });
                            resident.phase = Phase::Returning;
                        }
                        let mut completed = false;
                        for _ in 0..20_000 {
                            advance_transit(
                                &world,
                                plan,
                                &mut resident,
                                0.25,
                                &[],
                                &network,
                                tick_time,
                                &[],
                            );
                            if !boarding {
                                tick_time += 0.25;
                            }
                            let transit = resident.transit.as_ref().unwrap();
                            if transit.stage
                                == if boarding {
                                    TransitStage::Riding
                                } else {
                                    TransitStage::ToDestination
                                }
                            {
                                completed = true;
                                break;
                            }
                        }
                        assert!(
                            completed,
                            "seed={seed} village={origin} route={} boarding={boarding} body={:?} transit={:?} target={:?} nearby={:?} ramps={:?}",
                            route.id,
                            resident.body,
                            resident.transit,
                            resident.snapshot.target,
                            &path[resident
                                .transit
                                .as_ref()
                                .unwrap()
                                .waypoint
                                .saturating_sub(2)
                                ..(resident.transit.as_ref().unwrap().waypoint + 3)
                                    .min(path.len())],
                            network
                                .ramps()
                                .iter()
                                .filter(|r| horizontal_distance(r.from, resident.body.position)
                                    < 5.0
                                    || horizontal_distance(r.to, resident.body.position) < 5.0)
                                .collect::<Vec<_>>()
                        );
                        if boarding {
                            assert_eq!(
                                resident.transit.as_ref().unwrap().ride.unwrap().ship_id,
                                ship.id
                            );
                        } else {
                            assert!(resident.transit.as_ref().unwrap().ride.is_none());
                            assert!(
                                horizontal_distance(
                                    resident.body.position,
                                    network.port(origin).unwrap().position
                                ) <= ARRIVAL
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn stationary_player_on_landing_gate_can_be_passed_without_losing_cargo() {
        for seed in [42, 43] {
            let world = World::generate(seed, WorldGeneration::GeographyV3);
            let network = AirshipNetwork::new(&world);
            let plan = world.settlements().unwrap();
            let life = VillageLife::new(&world);
            for route in network.routes() {
                for (origin, destination) in [(route.from, route.to), (route.to, route.from)] {
                    let leg = network.next_leg(origin, destination, 0.0).unwrap();
                    let time = (leg.departure_in as f64 - 1.0).max(0.0);
                    let ship = network.ship(leg.ship_id, time).unwrap();
                    let path = network.landing_path(ship.id, origin).unwrap();
                    let gate = path
                        .iter()
                        .rposition(|p| horizontal_distance(*p, ship.position) >= 12.0)
                        .unwrap();
                    assert!(
                        gate >= 1 && gate + 1 < path.len(),
                        "seed={seed} route={} origin={origin} gate={gate} len={} path={path:?}",
                        route.id,
                        path.len()
                    );
                    let player = path[gate];
                    for boarding in [true, false] {
                        let before_gate = path[..gate]
                            .iter()
                            .rposition(|p| horizontal_distance(*p, player) >= 2.0)
                            .unwrap();
                        let after_gate = gate
                            + 1
                            + path[gate + 1..]
                                .iter()
                                .position(|p| horizontal_distance(*p, player) >= 2.0)
                                .unwrap();
                        let start = if boarding { before_gate } else { after_gate };
                        let finish = if boarding { after_gate } else { before_gate };
                        let mut resident = life
                            .residents
                            .iter()
                            .find(|r| r.snapshot.village_id == origin && r.trail.is_some())
                            .unwrap()
                            .clone();
                        resident.body = Body::new(path[start]);
                        resident.body.on_ground = true;
                        resident.phase = if boarding {
                            Phase::ToTrade
                        } else {
                            Phase::Returning
                        };
                        resident.transit = Some(Transit {
                            origin: if boarding { origin } else { destination },
                            destination: if boarding { destination } else { origin },
                            next_stop: if boarding { destination } else { origin },
                            stage: if boarding {
                                TransitStage::Boarding
                            } else {
                                TransitStage::Alighting
                            },
                            waypoint: start,
                            ride: None,
                            reservation: Some(AirshipRide {
                                ship_id: ship.id,
                                seat: 0,
                            }),
                            deck_position: None,
                        });
                        let cargo = Some(ResourceCargo {
                            kind: ResourceKind::Timber,
                            amount: 6.0,
                        });
                        resident.snapshot.carrying = cargo.clone();
                        // Establish a physically reachable bypass using the same controller,
                        // support checks and body collision as the live resident.
                        let mut walker = Walker::on_foot(&resident.body);
                        let mut navigation = Navigation::default();
                        let obstacles = [player];
                        let walking = Walking {
                            world: &world,
                            obstacles: &obstacles,
                            airships: Some((&network, time)),
                        };
                        for _ in 0..160 {
                            walking.walk(
                                &mut walker,
                                &mut navigation,
                                path[finish],
                                0.52,
                                false,
                                0.25,
                                |_| true,
                            );
                            if horizontal_distance(walker.body.position, path[finish]) < ARRIVAL {
                                break;
                            }
                        }
                        assert!(
                            horizontal_distance(walker.body.position, path[finish]) < ARRIVAL,
                            "fixture has no demonstrated bypass: seed={seed} route={} origin={origin} boarding={boarding} gate={gate} body={:?}",
                            route.id,
                            walker.body
                        );
                        let initial = resident;
                        for dt in [0.05, 0.25] {
                            // Repeat with the player staying put and stepping aside after
                            // contact. Neither case may become a persistent oscillation.
                            for step_aside in [false, true] {
                                let mut resident = initial.clone();
                                let mut passed = false;
                                for tick in 0..(20.0 / dt) as usize {
                                    let active_obstacles = if step_aside && tick as f32 * dt >= 1.0
                                    {
                                        &[][..]
                                    } else {
                                        &obstacles[..]
                                    };
                                    let before = resident.body.position;
                                    advance_transit(
                                        &world,
                                        plan,
                                        &mut resident,
                                        dt,
                                        active_obstacles,
                                        &network,
                                        time,
                                        &[],
                                    );
                                    assert!(
                                        active_obstacles.iter().all(|player| {
                                            !rubblekin_core::physics::characters_overlap(
                                                resident.body.position,
                                                *player,
                                            )
                                        }),
                                        "overlap seed={seed} route={} origin={origin} boarding={boarding} dt={dt}",
                                        route.id
                                    );
                                    assert!(
                                        horizontal_distance(before, resident.body.position)
                                            <= 3.0 * dt + 0.03
                                    );
                                    assert_eq!(resident.snapshot.carrying, cargo);
                                    let transit = resident.transit.as_ref().unwrap();
                                    passed = if boarding {
                                        transit.waypoint > finish
                                            || transit.stage == TransitStage::Riding
                                    } else {
                                        transit.waypoint < finish
                                    };
                                    if passed {
                                        break;
                                    }
                                }
                                assert!(
                                    passed,
                                    "failed to pass/resume: seed={seed} route={} origin={origin} boarding={boarding} dt={dt} step_aside={step_aside} body={:?} transit={:?}",
                                    route.id, resident.body, resident.transit
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn blocked_disembarkation_at_departure_stays_aboard_and_resumes_at_the_same_port() {
        for seed in [42, 43] {
            let world = World::generate(seed, WorldGeneration::GeographyV3);
            let network = AirshipNetwork::new(&world);
            let plan = world.settlements().unwrap();
            let life = VillageLife::new(&world);
            let route = &network.routes()[0];
            for (origin, destination) in [(route.from, route.to), (route.to, route.from)] {
                let leg = network.next_leg(origin, destination, 0.0).unwrap();
                let start = leg.departure_in as f64 - 1.0;
                let ship = network.ship(leg.ship_id, start).unwrap();
                let local = initial_deck_position(27);
                let mut resident = life
                    .residents
                    .iter()
                    .find(|r| r.snapshot.village_id == origin && r.trail.is_some())
                    .unwrap()
                    .clone();
                resident.body = Body::new(deck_position(&ship, local));
                resident.body.on_ground = true;
                resident.phase = Phase::Returning;
                resident.transit = Some(Transit {
                    origin: destination,
                    destination: origin,
                    next_stop: origin,
                    stage: TransitStage::Alighting,
                    waypoint: network.landing_path(ship.id, origin).unwrap().len() - 1,
                    ride: Some(AirshipRide {
                        ship_id: ship.id,
                        seat: u8::MAX,
                    }),
                    reservation: Some(AirshipRide {
                        ship_id: ship.id,
                        seat: 27,
                    }),
                    deck_position: Some(local),
                });
                let cargo = Some(ResourceCargo {
                    kind: ResourceKind::Timber,
                    amount: 6.0,
                });
                resident.snapshot.carrying = cargo.clone();
                let cycle = (AIRSHIP_DWELL_SECONDS + route.travel_seconds) * 2.0;
                let mut departed = false;
                let mut disembarked = false;
                for tick in 0..((cycle + 30.0) / 0.25).ceil() as usize {
                    let time = start + tick as f64 * 0.25;
                    let ship = network.ship(leg.ship_id, time).unwrap();
                    // Other passengers enclose this resident during the final
                    // docked second, then leave room once the craft departs.
                    let obstacles: Vec<_> = if tick < 4 {
                        (-1..=1)
                            .flat_map(|x| {
                                (-1..=1).filter_map(move |z| (x != 0 || z != 0).then_some((x, z)))
                            })
                            .map(|(x, z)| {
                                deck_position(
                                    &ship,
                                    [local[0] + x as f32 * 0.6, 0.0, local[2] + z as f32 * 0.6],
                                )
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };
                    advance_transit(
                        &world,
                        plan,
                        &mut resident,
                        0.25,
                        &obstacles,
                        &network,
                        time,
                        &[],
                    );
                    assert_eq!(resident.snapshot.carrying, cargo);
                    assert!(obstacles.iter().all(
                        |p| !rubblekin_core::physics::characters_overlap(
                            resident.body.position,
                            *p
                        )
                    ));
                    let transit = resident.transit.as_ref().unwrap();
                    departed |= ship.docked_at != Some(origin);
                    if ship.docked_at != Some(origin)
                        || ship.departure_in as f64 > AIRSHIP_DWELL_SECONDS - AIRSHIP_TURN_SECONDS
                    {
                        assert_eq!(
                            transit.ride.map(|r| r.ship_id),
                            Some(ship.id),
                            "resident must remain aboard while flying or turning after a blocked departure"
                        );
                        assert_eq!(transit.stage, TransitStage::Alighting);
                    }
                    if transit.ride.is_none() {
                        assert!(departed && ship.docked_at == Some(origin));
                        // Descending onto the ramp briefly clears on_ground.
                        let height = resident.body.position[1];
                        let mut body = resident.body.clone();
                        let mut ride = None;
                        let mut local = None;
                        move_character_with_airships(
                            &world,
                            &mut body,
                            MoveInput::default(),
                            0.25,
                            &[],
                            &network,
                            time,
                            &mut ride,
                            &mut local,
                        );
                        assert!(
                            body.on_ground && (body.position[1] - height).abs() <= CELL_SIZE,
                            "seed={seed} origin={origin} time={time} ship={ship:?} initial={:?} settled={body:?} transit={transit:?}",
                            resident.body
                        );
                        disembarked = true;
                        break;
                    }
                }
                assert!(
                    disembarked,
                    "seed={seed} origin={origin}: did not resume after returning to the blocked port"
                );
            }
        }
    }

    #[test]
    fn opposing_traders_pass_on_each_landing_without_overlap_or_losing_cargo() {
        for seed in [42, 43] {
            let world = World::generate(seed, WorldGeneration::GeographyV3);
            let network = AirshipNetwork::new(&world);
            let plan = world.settlements().unwrap();
            let life = VillageLife::new(&world);
            for route in network.routes() {
                for (origin, destination) in [(route.from, route.to), (route.to, route.from)] {
                    for dt in [0.05, 0.25] {
                        let leg = network.next_leg(origin, destination, 0.0).unwrap();
                        let time = (leg.departure_in as f64 - 1.0).max(0.0);
                        let ship = network.ship(leg.ship_id, time).unwrap();
                        let path = network.landing_path(ship.id, origin).unwrap();
                        let mut boarding = life
                            .residents
                            .iter()
                            .find(|r| r.snapshot.village_id == origin && r.trail.is_some())
                            .unwrap()
                            .clone();
                        boarding.phase = Phase::ToTrade;
                        let waypoint = path
                            .iter()
                            .rposition(|p| horizontal_distance(*p, ship.position) >= 14.0)
                            .unwrap();
                        boarding.body = Body::new(path[waypoint]);
                        boarding.body.on_ground = true;
                        boarding.transit = Some(Transit {
                            origin,
                            destination,
                            next_stop: destination,
                            stage: TransitStage::Boarding,
                            waypoint,
                            ride: None,
                            reservation: Some(AirshipRide {
                                ship_id: ship.id,
                                seat: 0,
                            }),
                            deck_position: None,
                        });
                        let mut alighting = boarding.clone();
                        alighting.phase = Phase::Returning;
                        let exit_waypoint = path
                            .iter()
                            .rposition(|p| horizontal_distance(*p, ship.position) >= 11.0)
                            .unwrap();
                        alighting.body = Body::new(path[exit_waypoint]);
                        alighting.body.on_ground = true;
                        alighting.transit = Some(Transit {
                            origin: destination,
                            destination: origin,
                            next_stop: origin,
                            stage: TransitStage::Alighting,
                            waypoint: exit_waypoint,
                            ride: None,
                            reservation: Some(AirshipRide {
                                ship_id: ship.id,
                                seat: 7,
                            }),
                            deck_position: None,
                        });
                        let cargo = Some(ResourceCargo {
                            kind: ResourceKind::Timber,
                            amount: 6.0,
                        });
                        boarding.snapshot.carrying = cargo.clone();
                        alighting.snapshot.carrying = cargo.clone();
                        // Runtime passing preferences/checkpoints are rebuilt on load.
                        boarding.stuck = 10.0;
                        alighting.stuck = 10.0;
                        boarding = serde_json::from_slice(&serde_json::to_vec(&boarding).unwrap())
                            .unwrap();
                        alighting =
                            serde_json::from_slice(&serde_json::to_vec(&alighting).unwrap())
                                .unwrap();
                        let mut completed = [false; 2];
                        for _ in 0..4_000 {
                            let before = [boarding.body.position, alighting.body.position];
                            if !completed[0] {
                                advance_transit(
                                    &world,
                                    plan,
                                    &mut boarding,
                                    dt,
                                    &[alighting.body.position],
                                    &network,
                                    time,
                                    &[],
                                );
                                completed[0] = boarding.transit.as_ref().unwrap().stage
                                    == TransitStage::Riding;
                            }
                            if !completed[1] {
                                advance_transit(
                                    &world,
                                    plan,
                                    &mut alighting,
                                    dt,
                                    &[boarding.body.position],
                                    &network,
                                    time,
                                    &[],
                                );
                                completed[1] = alighting.transit.as_ref().unwrap().waypoint
                                    < waypoint.saturating_sub(8);
                            }
                            assert!(
                                character_position_is_clear(
                                    &world,
                                    boarding.body.position,
                                    &[alighting.body.position]
                                ),
                                "route={} village={origin}: traders overlapped",
                                route.id
                            );
                            assert!(
                                horizontal_distance(before[0], boarding.body.position)
                                    <= 3.0 * dt + 0.03
                            );
                            assert!(
                                horizontal_distance(before[1], alighting.body.position)
                                    <= 3.0 * dt + 0.03
                            );
                            assert_eq!(boarding.snapshot.carrying, cargo);
                            assert_eq!(alighting.snapshot.carrying, cargo);
                            if completed == [true; 2] {
                                break;
                            }
                        }
                        assert_eq!(
                            completed, [true; 2],
                            "route={} village={origin} dt={dt}: boarding={:?}; alighting={:?}",
                            route.id, boarding, alighting
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_trader_chooses_scheduled_transport_keeps_cargo_and_delivers_after_walking_from_the_port() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let network = AirshipNetwork::new(&world);
        let plan = world.settlements().unwrap();
        let mut life = VillageLife::new(&world);
        let index = departing_trader(&world, &mut life);
        let resident = life.residents[index].clone();
        let destination = travel_destination(plan, &resident);
        let partner = plan
            .villages
            .iter()
            .position(|v| v.id == destination)
            .unwrap();
        life.villages[partner].snapshot.timber = 0.0;
        life.residents = vec![resident];
        let mut rode = false;
        let mut walked_from_port = false;
        let mut delivered = false;
        for tick in 1..8_000 {
            let time = tick as f64 * 0.25;
            life.tick_with_transport(&world, 0.25, &[], &network, time, &[]);
            let resident = &life.residents[0];
            rode |= resident.snapshot.ride.is_some();
            walked_from_port |= resident
                .transit
                .as_ref()
                .is_some_and(|t| t.stage == TransitStage::ToDestination);
            if life.villages[partner].trade_deliveries == 0 {
                assert_eq!(
                    resident.snapshot.carrying,
                    Some(ResourceCargo {
                        kind: ResourceKind::Timber,
                        amount: 6.0
                    })
                );
                assert_eq!(life.villages[partner].snapshot.timber, 0.0);
            } else {
                assert!(rode && walked_from_port);
                assert!(at_station(
                    resident.body.position,
                    plan.villages[partner].center
                ));
                assert_eq!(life.villages[partner].snapshot.timber, 6.0);
                delivered = true;
                break;
            }
        }
        assert!(
            delivered,
            "Trader did not complete its physical transit: {:?}; state={:?}; surface={}; nearby={:?}",
            life.residents(),
            life.residents[0].transit,
            world.surface_height(
                life.residents[0].body.position[0],
                life.residents[0].body.position[2]
            ),
            network
                .ramps()
                .iter()
                .filter(|r| horizontal_distance(r.from, life.residents[0].body.position) < 5.0)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn full_airships_wait_and_board_only_a_free_seat_without_changing_the_job() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let network = AirshipNetwork::new(&world);
        let plan = world.settlements().unwrap();
        let mut life = VillageLife::new(&world);
        let index = departing_trader(&world, &mut life);
        let resident = &mut life.residents[index];
        let origin = travel_origin(plan, resident);
        let destination = travel_destination(plan, resident);
        let leg = network.next_leg(origin, destination, 0.0).unwrap();
        let time = (leg.departure_in as f64 - 1.0).max(0.0);
        let ship = network.ship(leg.ship_id, time).unwrap();
        assert_eq!(ship.docked_at, Some(origin));
        resident.body = Body::new(network.port(origin).unwrap().position);
        resident.snapshot.position = resident.body.position;
        resident.transit = Some(Transit {
            origin,
            destination,
            next_stop: leg.destination,
            stage: TransitStage::Waiting,
            waypoint: transit_path(plan, &network, origin).len() - 1,
            ride: None,
            reservation: None,
            deck_position: None,
        });
        let cargo = resident.snapshot.carrying.clone();
        let full: Vec<_> = (0..MAX_AIRSHIP_SEATS)
            .map(|seat| AirshipRide {
                ship_id: ship.id,
                seat,
            })
            .collect();
        assert!(advance_transit(
            &world,
            plan,
            resident,
            0.05,
            &[],
            &network,
            time,
            &full
        ));
        assert!(resident.transit.as_ref().unwrap().ride.is_none());
        assert_eq!(resident.snapshot.carrying, cargo);
        assert_eq!(resident.phase, Phase::ToTrade);
        assert!(advance_transit(
            &world,
            plan,
            resident,
            0.05,
            &[],
            &network,
            time,
            &full[1..]
        ));
        assert_eq!(
            resident.transit.as_ref().unwrap().reservation,
            Some(AirshipRide {
                ship_id: ship.id,
                seat: 0
            })
        );
        assert_eq!(resident.snapshot.carrying, cargo);
    }

    #[test]
    fn missed_departures_rebook_and_actual_contact_uses_the_boarded_fleet_mate() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let network = AirshipNetwork::new(&world);
        let plan = world.settlements().unwrap();
        let mut life = VillageLife::new(&world);
        let index = departing_trader(&world, &mut life);
        let resident = &mut life.residents[index];
        let origin = travel_origin(plan, resident);
        let destination = travel_destination(plan, resident);
        let first = network.next_leg(origin, destination, 0.0).unwrap();
        let booked = AirshipRide {
            ship_id: first.ship_id,
            seat: 7,
        };
        let path = network.landing_path(booked.ship_id, origin).unwrap();
        let waypoint = path.len() - 12;
        resident.body = Body::new(path[waypoint]);
        resident.snapshot.position = resident.body.position;
        resident.transit = Some(Transit {
            origin,
            destination,
            next_stop: first.destination,
            stage: TransitStage::Boarding,
            waypoint,
            ride: None,
            reservation: Some(booked),
            deck_position: None,
        });
        let missed_time = first.departure_in as f64 + 1.0;
        let next = network.next_leg(origin, destination, missed_time).unwrap();
        assert_ne!(next.ship_id, booked.ship_id);
        let before = resident.body.position;
        life.tick_with_transport(&world, 0.05, &[], &network, missed_time, &[]);
        let resident = &life.residents[index];
        assert_eq!(
            resident
                .transit
                .as_ref()
                .unwrap()
                .reservation
                .unwrap()
                .ship_id,
            next.ship_id
        );
        assert!(resident.transit.as_ref().unwrap().waypoint >= waypoint);
        assert!(horizontal_distance(before, resident.body.position) <= 0.11);
        assert!(life.validate_transport(&world, &network, missed_time));

        let (time, actual) = (1..2_000)
            .find_map(|tick| {
                let time = tick as f64;
                network
                    .ships(time)
                    .into_iter()
                    .find(|ship| {
                        ship.id != booked.ship_id
                            && ship.from_village == origin
                            && ship.next_village == first.destination
                            && ship.docked_at == Some(origin)
                            && ship.departure_in > 1.0
                    })
                    .map(|ship| (time, ship))
            })
            .unwrap();
        let resident = &mut life.residents[index];
        let local = initial_deck_position(32);
        let actual_ride = AirshipRide {
            ship_id: actual.id,
            seat: u8::MAX,
        };
        resident.body = Body::new(deck_position(&actual, local));
        resident.snapshot.position = resident.body.position;
        resident.snapshot.ride = Some(actual_ride);
        resident.snapshot.deck_position = Some(local);
        resident.transit = Some(Transit {
            origin,
            destination,
            next_stop: first.destination,
            stage: TransitStage::Boarding,
            waypoint: path.len() - 1,
            ride: Some(actual_ride),
            reservation: Some(booked),
            deck_position: Some(local),
        });
        let cargo = resident.snapshot.carrying.clone();
        let before = resident.body.position;
        life.tick_with_transport(&world, 0.05, &[], &network, time, &[]);
        let resident = &life.residents[index];
        let transit = resident.transit.as_ref().unwrap();
        assert_eq!(transit.ride.unwrap().ship_id, actual.id);
        assert_eq!(transit.reservation.unwrap().ship_id, actual.id);
        assert!(horizontal_distance(before, resident.body.position) <= 0.11);
        assert_eq!(resident.snapshot.carrying, cargo);
        assert!(life.validate_transport(&world, &network, time));

        // A craft that fills while the resident approaches retains the actual
        // standing body instead of losing its goal or moving it to a berth.
        let resident = &mut life.residents[index];
        resident.transit.as_mut().unwrap().reservation = Some(booked);
        let before = resident.body.position;
        let full: Vec<_> = (0..MAX_AIRSHIP_SEATS)
            .map(|seat| AirshipRide {
                ship_id: actual.id,
                seat,
            })
            .collect();
        life.tick_with_transport(&world, 0.05, &[], &network, time, &full);
        let resident = &life.residents[index];
        assert_eq!(
            resident.transit.as_ref().unwrap().stage,
            TransitStage::Riding
        );
        assert_eq!(resident.snapshot.ride, Some(actual_ride));
        assert!(resident.transit.as_ref().unwrap().reservation.is_none());
        assert_eq!(resident.body.position, before);
        assert_eq!(resident.snapshot.carrying, cargo);
        assert!(life.validate_transport(&world, &network, time));

        let wrong_ship = network
            .ships(time)
            .into_iter()
            .find(|ship| ship.route_id != actual.route_id)
            .unwrap();
        let resident = &mut life.residents[index];
        let mut transit = resident.transit.take().unwrap();
        transit.stage = TransitStage::Boarding;
        transit.reservation = Some(booked);
        transit.ride = Some(AirshipRide {
            ship_id: wrong_ship.id,
            seat: u8::MAX,
        });
        transit.deck_position = Some(local);
        reconcile_boarding_contact(&world, resident, &mut transit, &[], &network, time, &[]);
        assert!(transit.ride.is_none() && transit.deck_position.is_none());
    }

    #[test]
    fn incidental_alighting_contact_from_native_save_roundtrips_and_keeps_its_destination() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let network = AirshipNetwork::new(&world);
        let time = 875.6000130474567;
        let mut life = VillageLife::new(&world);
        let index = life
            .residents
            .iter()
            .position(|r| r.snapshot.id == 2310)
            .unwrap();
        let resident = &mut life.residents[index];
        // Transit state from a native shared-landing contact: physics boarded
        // route 0 while exiting route 7. Use its current generated ship pose;
        // the October 8 side-berth repair deliberately moved old landings.
        resident.phase = Phase::ToTrade;
        resident.waypoint = 5595;
        resident.body = Body {
            position: [2178.2615, 299.35, 3017.2905],
            velocity: [0.0; 3],
            on_ground: true,
            glide_stalled: false,
        };
        resident.transit = Some(Transit {
            origin: 9,
            destination: 5,
            next_stop: 5,
            stage: TransitStage::Alighting,
            waypoint: 0,
            ride: Some(AirshipRide {
                ship_id: 4294967297,
                seat: u8::MAX,
            }),
            reservation: Some(AirshipRide {
                ship_id: 34359738369,
                seat: 0,
            }),
            deck_position: Some([0.022818793, 0.0, 0.5619632]),
        });
        let transit = resident.transit.as_ref().unwrap();
        let ship = network.ship(transit.ride.unwrap().ship_id, time).unwrap();
        resident.body.position = deck_position(&ship, transit.deck_position.unwrap());
        let cargo = Some(ResourceCargo {
            kind: ResourceKind::Stone,
            amount: 6.0,
        });
        resident.snapshot.carrying = cargo.clone();
        resident.snapshot.position = resident.body.position;
        resident.snapshot.ride = resident.transit.as_ref().unwrap().ride;
        resident.snapshot.deck_position = resident.transit.as_ref().unwrap().deck_position;
        assert!(life.validate(&world));
        assert!(
            life.validate_transport(&world, &network, time),
            "a physically attached passenger can still reach its intended shared port"
        );
        let mut wrong_pose = life.clone();
        wrong_pose.residents[index].body.position[0] += 0.5;
        wrong_pose.residents[index].snapshot.position = wrong_pose.residents[index].body.position;
        assert!(!wrong_pose.validate_transport(&world, &network, time));
        let mut wrong_route = life.clone();
        let unrelated = network
            .ships(time)
            .into_iter()
            .find(|s| network.landing_path(s.id, 5).is_none())
            .unwrap();
        let local = initial_deck_position(27);
        let resident = &mut wrong_route.residents[index];
        resident.body.position = deck_position(&unrelated, local);
        resident.snapshot.position = resident.body.position;
        resident.transit.as_mut().unwrap().ride = Some(AirshipRide {
            ship_id: unrelated.id,
            seat: u8::MAX,
        });
        resident.transit.as_mut().unwrap().deck_position = Some(local);
        resident.snapshot.ride = resident.transit.as_ref().unwrap().ride;
        resident.snapshot.deck_position = Some(local);
        assert!(!wrong_route.validate_transport(&world, &network, time));

        // New contact is reconciled in the same tick, before it can be saved.
        let mut contact = life.clone();
        let leg = network.next_leg(5, 0, 0.0).unwrap();
        let contact_time = leg.departure_in as f64 - 1.0;
        let ship = network.ship(leg.ship_id, contact_time).unwrap();
        let resident = &mut contact.residents[index];
        resident.body.position = deck_position(&ship, local);
        resident.snapshot.position = resident.body.position;
        resident.transit.as_mut().unwrap().ride = None;
        resident.transit.as_mut().unwrap().deck_position = None;
        resident.snapshot.ride = None;
        resident.snapshot.deck_position = None;
        contact.tick_with_transport(&world, 0.05, &[], &network, contact_time, &[]);
        let transit = contact.residents[index].transit.as_ref().unwrap();
        assert_eq!(transit.ride.unwrap().ship_id, ship.id);
        assert_eq!(transit.reservation.unwrap().ship_id, ship.id);
        assert!(contact.validate_transport(&world, &network, contact_time));

        let path = std::env::temp_dir().join(format!(
            "rubblekin-native-alighting-{}.json",
            std::process::id()
        ));
        let simulation = crate::persistence::Simulation {
            activities: crate::activities::Activities::default(),
            gliders: crate::gliders::GliderService::default(),
            profiles: Default::default(),
            consumed_resource_cells: Vec::new(),
            ecology: crate::ecology::Ecology::default(),
            world: world.clone(),
            npc: crate::npc::Forager::new(&world),
            world_time: time,
            villages: life,
        };
        simulation.save(&path).unwrap();
        let mut restored =
            crate::persistence::Simulation::load(&path, 999, WorldGeneration::ValleyV1).unwrap();
        std::fs::remove_file(path).unwrap();
        restored
            .villages
            .tick_with_transport(&world, 0.05, &[], &network, time, &[]);
        let resident = &restored.villages.residents[index];
        let transit = resident.transit.as_ref().unwrap();
        assert_eq!(transit.destination, 5);
        assert_eq!(transit.next_stop, 5);
        assert_eq!(transit.origin, 9);
        assert_eq!(transit.reservation.unwrap().ship_id, 4294967297);
        assert_eq!(transit.ride.unwrap().ship_id, 4294967297);
        assert_eq!(resident.snapshot.carrying, cargo);
        assert_eq!(resident.phase, Phase::ToTrade);
        assert!(restored.villages.validate_transport(&world, &network, time));
        let resident = &mut restored.villages.residents[index];
        for tick in 1..2_000 {
            advance_transit(
                &world,
                world.settlements().unwrap(),
                resident,
                0.25,
                &[],
                &network,
                time + tick as f64 * 0.25,
                &[],
            );
            assert_eq!(resident.snapshot.carrying, cargo);
            if resident.transit.as_ref().unwrap().stage == TransitStage::ToDestination {
                break;
            }
        }
        assert_eq!(
            resident.transit.as_ref().unwrap().stage,
            TransitStage::ToDestination
        );
        assert!(
            horizontal_distance(resident.body.position, network.port(5).unwrap().position)
                <= ARRIVAL
        );
    }

    #[test]
    fn physical_boarding_alighting_and_standing_rides_survive_save_and_reject_corrupt_metadata() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let network = AirshipNetwork::new(&world);
        let plan = world.settlements().unwrap();
        let mut life = VillageLife::new(&world);
        let index = departing_trader(&world, &mut life);
        let resident = &mut life.residents[index];
        let origin = travel_origin(plan, resident);
        let destination = travel_destination(plan, resident);
        let leg = network.next_leg(origin, destination, 0.0).unwrap();
        let time = (leg.departure_in as f64 - 1.0).max(0.0);
        let ship = network.ship(leg.ship_id, time).unwrap();
        let reservation = AirshipRide {
            ship_id: ship.id,
            seat: 7,
        };
        let ride = AirshipRide {
            ship_id: ship.id,
            seat: u8::MAX,
        };
        let local = initial_deck_position(32);
        resident.body = Body::new(deck_position(&ship, local));
        resident.body.on_ground = true;
        resident.transit = Some(Transit {
            origin,
            destination,
            next_stop: leg.destination,
            stage: TransitStage::Boarding,
            waypoint: network.landing_path(ship.id, origin).unwrap().len() - 1,
            ride: Some(ride),
            reservation: Some(reservation),
            deck_position: Some(local),
        });
        life.sync_airship_riders(&network, time);
        assert!(life.validate_transport(&world, &network, time));

        let mut invalid_ship = life.clone();
        invalid_ship.residents[index]
            .transit
            .as_mut()
            .unwrap()
            .reservation
            .as_mut()
            .unwrap()
            .ship_id = (ship.id & 0xffff_ffff_0000_0000) | 0xffff_fffe;
        assert!(!invalid_ship.validate_transport(&world, &network, time));
        let mut missing_reservation = life.clone();
        missing_reservation.residents[index]
            .transit
            .as_mut()
            .unwrap()
            .reservation = None;
        assert!(!missing_reservation.validate_transport(&world, &network, time));
        let mut nonfinite = life.clone();
        nonfinite.residents[index]
            .transit
            .as_mut()
            .unwrap()
            .deck_position = Some([f32::NAN, 0.0, 0.0]);
        assert!(!nonfinite.validate_transport(&world, &network, time));
        let mut mismatched_local = life.clone();
        mismatched_local.residents[index].snapshot.deck_position = Some([0.0; 3]);
        assert!(!mismatched_local.validate_transport(&world, &network, time));

        let boarding = life.clone();
        life.residents[index].transit.as_mut().unwrap().stage = TransitStage::Riding;
        life.residents[index].transit.as_mut().unwrap().reservation = None;
        life.residents[index].transit.as_mut().unwrap().waypoint = 0;
        assert!(life.validate_transport(&world, &network, time));
        let standing = life.clone();
        let mut invalid_stage = standing.clone();
        invalid_stage.residents[index]
            .transit
            .as_mut()
            .unwrap()
            .reservation = Some(reservation);
        assert!(!invalid_stage.validate_transport(&world, &network, time));

        let arrival_time = leg.arrival_in as f64 + 1.0;
        let arrival_ship = network.ship(ship.id, arrival_time).unwrap();
        assert_eq!(arrival_ship.docked_at, Some(leg.destination));
        let transit = life.residents[index].transit.as_mut().unwrap();
        transit.stage = TransitStage::Alighting;
        transit.reservation = Some(reservation);
        transit.waypoint = network
            .landing_path(ship.id, leg.destination)
            .unwrap()
            .len()
            - 1;
        life.sync_airship_riders(&network, arrival_time);
        assert!(life.validate_transport(&world, &network, arrival_time));

        for (name, state, saved_time) in [
            ("boarding", boarding, time),
            ("standing", standing, time),
            ("alighting", life, arrival_time),
        ] {
            let save_path = std::env::temp_dir().join(format!(
                "rubblekin-airship-physical-{name}-{}.json",
                std::process::id()
            ));
            let simulation = crate::persistence::Simulation {
                activities: crate::activities::Activities::default(),
                gliders: crate::gliders::GliderService::default(),
                profiles: Default::default(),
                consumed_resource_cells: Vec::new(),
                ecology: crate::ecology::Ecology::default(),
                world: world.clone(),
                npc: crate::npc::Forager::new(&world),
                world_time: saved_time,
                villages: state.clone(),
            };
            simulation.save(&save_path).unwrap();
            let restored =
                crate::persistence::Simulation::load(&save_path, 999, WorldGeneration::ValleyV1)
                    .unwrap();
            std::fs::remove_file(save_path).unwrap();
            assert_eq!(restored.world_time, saved_time);
            assert_eq!(restored.villages.residents(), state.residents());
            assert!(
                restored
                    .villages
                    .validate_transport(&restored.world, &network, saved_time)
            );
        }
    }

    #[test]
    fn riding_and_waiting_state_roundtrips_and_rejects_invalid_or_duplicated_seats() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let network = AirshipNetwork::new(&world);
        let plan = world.settlements().unwrap();
        let mut life = VillageLife::new(&world);
        let index = life
            .residents
            .iter()
            .position(|resident| {
                resident.trail.is_some()
                    && life
                        .residents
                        .iter()
                        .filter(|other| other.trail == resident.trail)
                        .count()
                        > 1
            })
            .unwrap();
        let resident = &mut life.residents[index];
        resident.phase = Phase::ToTrade;
        let trail = &plan.trails[resident.trail.unwrap()];
        resident.waypoint = if trail.from == resident.snapshot.village_id {
            0
        } else {
            trail.points.len() - 1
        };
        let origin = travel_origin(plan, resident);
        let destination = travel_destination(plan, resident);
        let leg = network.next_leg(origin, destination, 0.0).unwrap();
        let ride = AirshipRide {
            ship_id: leg.ship_id,
            seat: 7,
        };
        resident.transit = Some(Transit {
            origin,
            destination,
            next_stop: leg.destination,
            stage: TransitStage::Riding,
            waypoint: transit_path(plan, &network, origin).len() - 1,
            ride: Some(ride),
            reservation: None,
            deck_position: None,
        });
        resident.snapshot.ride = Some(ride);
        life.sync_airship_riders(&network, 120.0);
        assert!(life.validate_transport(&world, &network, 120.0));
        let bytes = serde_json::to_vec(&life).unwrap();
        let mut loaded: VillageLife = serde_json::from_slice(&bytes).unwrap();
        assert!(loaded.validate_transport(&world, &network, 120.0));
        let mut waiting = loaded.clone();
        let resident = &mut waiting.residents[index];
        resident.transit.as_mut().unwrap().stage = TransitStage::Waiting;
        resident.transit.as_mut().unwrap().ride = None;
        resident.snapshot.ride = None;
        resident.snapshot.deck_position = None;
        resident.body = Body::new(network.port(origin).unwrap().position);
        resident.snapshot.position = resident.body.position;
        assert!(waiting.validate_transport(&world, &network, 120.0));
        let restored_waiting: VillageLife =
            serde_json::from_slice(&serde_json::to_vec(&waiting).unwrap()).unwrap();
        assert_eq!(restored_waiting.residents(), waiting.residents());
        assert!(restored_waiting.validate_transport(&world, &network, 120.0));
        assert_eq!(loaded.residents(), life.residents());
        let save_path = std::env::temp_dir().join(format!(
            "rubblekin-airship-residents-{}.json",
            std::process::id()
        ));
        let simulation = crate::persistence::Simulation {
            activities: crate::activities::Activities::default(),
            gliders: crate::gliders::GliderService::default(),
            profiles: Default::default(),
            consumed_resource_cells: Vec::new(),
            ecology: crate::ecology::Ecology::default(),
            world: world.clone(),
            npc: crate::npc::Forager::new(&world),
            world_time: 120.0,
            villages: life.clone(),
        };
        simulation.save(&save_path).unwrap();
        let restored =
            crate::persistence::Simulation::load(&save_path, 999, WorldGeneration::ValleyV1)
                .unwrap();
        std::fs::remove_file(save_path).unwrap();
        assert_eq!(restored.world_time, 120.0);
        assert_eq!(restored.villages.residents(), life.residents());
        assert!(restored.villages.validate_transport(
            &restored.world,
            &network,
            restored.world_time
        ));
        loaded.resolve_overlaps(&world, &[]);
        assert_eq!(
            loaded.residents[index].body.position,
            life.residents[index].body.position
        );
        assert!(loaded.validate_transport(&world, &network, 120.0));
        loaded.residents[index]
            .transit
            .as_mut()
            .unwrap()
            .ride
            .as_mut()
            .unwrap()
            .seat = MAX_AIRSHIP_SEATS;
        loaded.residents[index].snapshot.ride.as_mut().unwrap().seat = MAX_AIRSHIP_SEATS;
        assert!(!loaded.validate_transport(&world, &network, 120.0));
        let mut invalid_waypoint = life.clone();
        invalid_waypoint.residents[index]
            .transit
            .as_mut()
            .unwrap()
            .waypoint = usize::MAX;
        assert!(!invalid_waypoint.validate_transport(&world, &network, 120.0));
        let mut inconsistent_stage = life.clone();
        inconsistent_stage.residents[index]
            .transit
            .as_mut()
            .unwrap()
            .stage = TransitStage::Waiting;
        assert!(!inconsistent_stage.validate_transport(&world, &network, 120.0));
        let mut duplicate = life;
        let other = duplicate
            .residents
            .iter()
            .position(|resident| {
                resident.trail == duplicate.residents[index].trail
                    && resident.snapshot.id != duplicate.residents[index].snapshot.id
            })
            .unwrap();
        duplicate.residents[other].phase = Phase::ToTrade;
        duplicate.residents[other].transit = duplicate.residents[index].transit.clone();
        // Keep the other trader's original goal, while deliberately occupying
        // the same physical seat on that route.
        let destination = travel_destination(plan, &duplicate.residents[other]);
        let origin = travel_origin(plan, &duplicate.residents[other]);
        let transit = duplicate.residents[other].transit.as_mut().unwrap();
        transit.destination = destination;
        transit.origin = origin;
        transit.next_stop = destination;
        transit.waypoint = transit_path(plan, &network, origin).len() - 1;
        duplicate.residents[other].body = duplicate.residents[index].body.clone();
        duplicate.residents[other].snapshot.position = duplicate.residents[other].body.position;
        duplicate.residents[other].snapshot.ride = Some(ride);
        assert!(!duplicate.validate_transport(&world, &network, 120.0));
    }

    #[test]
    fn local_job_goals_do_not_start_airship_journeys() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let network = AirshipNetwork::new(&world);
        let plan = world.settlements().unwrap();
        let mut life = VillageLife::new(&world);
        let farmer = life
            .residents
            .iter_mut()
            .find(|r| r.snapshot.role == ResidentRole::Farmer)
            .unwrap();
        assert!(!advance_transit(
            &world,
            plan,
            farmer,
            0.05,
            &[],
            &network,
            0.0,
            &[]
        ));
        assert!(farmer.transit.is_none());
    }

    #[test]
    fn npc_and_player_harvests_compete_for_the_same_crop_in_either_order() {
        use crate::local_work;
        use rubblekin_core::economy::{PlayerEconomy, WorkSite};
        let world = World::generate(42, WorldGeneration::GeographyV5);
        let village = &world.settlements().unwrap().villages[0];
        let mut life = VillageLife::new(&world);
        life.villages[0].snapshot.crop_growth = 1.0;
        let farmer = &mut life.residents[0];
        assert_eq!(farmer.snapshot.role, ResidentRole::Farmer);
        farmer.farm_path = farm_path(&world, village, farmer.route);
        farmer.farm_waypoint = farmer
            .farm_path
            .iter()
            .position(|point| point.work)
            .unwrap();
        let position = farmer.farm_path[farmer.farm_waypoint].position;
        farmer.body = Body::new(position);
        farmer.snapshot.position = position;
        farmer.phase = Phase::Working;
        farmer.elapsed = 2.0;
        let site = WorkSite {
            village_id: village.id,
            kind: WorkKind::HarvestField,
            index: 0,
        };
        let active = local_work::start(&world, &life, &[], site, position, 0.0).unwrap();
        let mut player_first = life.clone();
        let food = life.villages[0].snapshot.food;

        advance_farmer(
            &world,
            village,
            &mut life.residents[0],
            &mut life.villages[0],
            0.05,
            1.0,
            &[],
        );
        assert_eq!(
            life.residents[0].snapshot.carrying.as_ref().unwrap().amount,
            12.0
        );
        let mut ledger = PlayerEconomy::default();
        assert!(local_work::complete(&world, &mut life, &mut ledger, &active).is_err());
        assert_eq!(ledger.cargo_total(), 0);
        assert_eq!(life.villages[0].harvests, 1);
        assert_eq!(life.villages[0].snapshot.food, food);

        local_work::complete(&world, &mut player_first, &mut ledger, &active).unwrap();
        advance_farmer(
            &world,
            village,
            &mut player_first.residents[0],
            &mut player_first.villages[0],
            0.05,
            1.0,
            &[],
        );
        assert!(player_first.residents[0].snapshot.carrying.is_none());
        assert_eq!(ledger.cargo[0], 12);
        assert_eq!(player_first.villages[0].harvests, 1);
        assert_eq!(player_first.villages[0].snapshot.food, food);
    }

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
    fn returning_cargo_cannot_bypass_an_occupied_store_gate() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let plan = world.settlements().unwrap();
        let life = VillageLife::new(&world);
        let mut resident = life
            .residents
            .iter()
            .find(|r| r.trail.is_some())
            .unwrap()
            .clone();
        let route = &plan.villages[resident.village].resident_routes[resident.route];
        resident.phase = Phase::ToHome;
        resident.waypoint = route.store_index;
        resident.body = Body::new(route.path[route.store_index - 1]);
        resident.snapshot.carrying = Some(ResourceCargo {
            kind: ResourceKind::Timber,
            amount: 6.0,
        });
        resident.stuck = 1.0;
        let blocked_store = [route.path[route.store_index]];
        assert!(!at_station(resident.body.position, blocked_store[0]));
        advance_resident(&world, plan, &mut resident, 0.05, &blocked_store);
        assert_eq!(resident.waypoint, route.store_index);
        assert_eq!(resident.snapshot.carrying.as_ref().unwrap().amount, 6.0);

        // Without cargo this is just another transit gate on the way home.
        resident.snapshot.carrying = None;
        resident.stuck = 1.0;
        advance_resident(&world, plan, &mut resident, 0.05, &blocked_store);
        assert_eq!(resident.waypoint, route.store_index - 1);
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
            activities: crate::activities::Activities::default(),
            gliders: crate::gliders::GliderService::default(),
            profiles: Default::default(),
            consumed_resource_cells: Vec::new(),
            ecology: crate::ecology::Ecology::default(),
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
    fn saved_town_center_food_queue_recovers_then_resumes_each_job() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let village = &world.settlements().unwrap().villages[5];
        let mut life = VillageLife::new(&world);
        // Reproduce the separated, mutually blocked queue observed in a
        // 2029-second save. Every resident has food available but cannot reach it.
        let queue = [
            (46, [2175.2358, 268.0, 3063.968]),
            (13, [2176.042, 268.0, 3065.0896]),
            (47, [2175.7964, 268.0, 3063.968]),
            (48, [2175.6794, 268.0, 3064.5288]),
            (43, [2176.5674, 268.0, 3065.6504]),
        ];
        for resident in life
            .residents
            .iter_mut()
            .filter(|r| r.village == 5 && r.trail.is_none())
        {
            let (waypoint, position) = queue[resident.route];
            resident.body = Body::new(position);
            resident.snapshot.position = position;
            resident.snapshot.hunger = 100.0;
            resident.snapshot.energy = 0.0;
            resident.phase = Phase::ToFood;
            resident.waypoint = waypoint;
            resident.stuck = 10.0;
            resident.resume = Some(Resume {
                phase: Phase::ToWork,
                waypoint: village.resident_routes[resident.route].path.len() - 1,
                elapsed: 0.0,
                farm_waypoint: 0,
            });
        }
        let encoded = serde_json::to_vec(&life).unwrap();
        let mut life: VillageLife = serde_json::from_slice(&encoded).unwrap();
        assert!(life.validate(&world));
        life.residents
            .retain(|r| r.village == 5 && r.trail.is_none());
        let mut ate = [false; 5];
        let mut worked = [false; 5];
        for _ in 0..12_000 {
            life.tick(&world, 0.05);
            for resident in &life.residents {
                ate[resident.route] |= resident.snapshot.action == ResidentAction::Eating;
                worked[resident.route] |= matches!(
                    resident.snapshot.action,
                    ResidentAction::Working
                        | ResidentAction::Planting
                        | ResidentAction::Tending
                        | ResidentAction::Harvesting
                );
                for other in life.residents.iter().filter(|r| r.route > resident.route) {
                    assert!(!rubblekin_core::physics::characters_overlap(
                        resident.body.position,
                        other.body.position
                    ));
                }
            }
        }
        assert!(ate.into_iter().all(|value| value), "{:?}", life.residents());
        assert!(
            worked.into_iter().all(|value| value),
            "{:?}",
            life.residents()
        );
        assert!(life.villages[5].deliveries > 0);
    }

    #[test]
    fn needs_trip_keeps_completed_farm_return_progress_and_cargo() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let village = &world.settlements().unwrap().villages[6];
        let life = VillageLife::new(&world);
        for cargo in [
            None,
            Some(ResourceCargo {
                kind: ResourceKind::Food,
                amount: 12.0,
            }),
        ] {
            let mut resident = life
                .residents
                .iter()
                .find(|r| r.snapshot.id == 1538)
                .unwrap()
                .clone();
            resident.farm_path = farm_path(&world, village, resident.route);
            let last_work = resident.farm_path.iter().rposition(|p| p.work).unwrap();
            let end = resident.farm_path.len() - 1;
            resident.phase = Phase::ToFarmExit;
            resident.resume = Some(Resume {
                phase: Phase::Working,
                waypoint: resident.waypoint,
                elapsed: 0.75,
                farm_waypoint: last_work + 30,
            });
            assert_eq!(
                farm_needs_entrance(&resident.farm_path, last_work + 30),
                end
            );
            resident.farm_waypoint = end;
            resident.body = Body::new(resident.farm_path[end].position);
            resident.snapshot.position = resident.body.position;
            resident.snapshot.hunger = 100.0;
            resident.snapshot.carrying = cargo.clone();
            advance_farm_detour(&world, &mut resident, &life.villages[6], 0.05, &[]);
            assert_eq!(resident.phase, Phase::ToFood);
            assert_eq!(resident.resume.as_ref().unwrap().farm_waypoint, end);
            assert_eq!(resident.snapshot.carrying, cargo);
            // The saved task is now the physically completed entrance gate.
            resident.phase = Phase::ToFarmResume;
            advance_farm_detour(&world, &mut resident, &life.villages[6], 0.05, &[]);
            assert_eq!(resident.phase, Phase::Working);
            assert_eq!(resident.elapsed, 0.75);
            assert_eq!(resident.farm_waypoint, end);
            assert_eq!(resident.snapshot.carrying, cargo);
        }
    }

    #[test]
    fn every_village_keeps_working_after_repeated_meals_and_rest() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let mut life = VillageLife::new(&world);
        // Each village's complete local crowd participates; long-distance
        // traders and their transport have separate journey regressions.
        life.residents.retain(|resident| resident.trail.is_none());
        let mut worked_late = vec![false; life.residents.len()];
        let mut late_counts = Vec::new();
        // At the server's 20 Hz, repeated needs trips must not leave a village
        // gridlocked at a shared center gate after its first successful tours.
        for tick in 0..30_000 {
            if tick == 24_000 {
                late_counts = life
                    .villages
                    .iter()
                    .map(|economy| (economy.harvests, economy.deliveries))
                    .collect::<Vec<_>>();
            }
            let had_cargo: Vec<_> = life
                .residents
                .iter()
                .map(|r| {
                    r.snapshot
                        .carrying
                        .as_ref()
                        .is_some_and(|cargo| cargo.amount > 0.0)
                })
                .collect();
            life.tick(&world, 0.05);
            for (index, resident) in life.residents.iter().enumerate() {
                for other in life.residents.iter().skip(index + 1) {
                    assert!(
                        !rubblekin_core::physics::characters_overlap(
                            resident.body.position,
                            other.body.position,
                        ),
                        "Residents {} and {} overlap at tick {tick}",
                        resident.snapshot.id,
                        other.snapshot.id,
                    );
                }
                if tick < 24_000 {
                    continue;
                }
                let route = &world.settlements().unwrap().villages[resident.village]
                    .resident_routes[resident.route];
                if had_cargo[index] && resident.snapshot.carrying.is_none() {
                    assert!(at_station(
                        resident.body.position,
                        route.path[route.store_index]
                    ));
                    worked_late[index] = true;
                }
                match resident.snapshot.action {
                    ResidentAction::Planting
                    | ResidentAction::Tending
                    | ResidentAction::Harvesting => {
                        let target = resident.snapshot.target.unwrap();
                        assert!(at_station(resident.body.position, target));
                        let soil = BlockPos::new(
                            (target[0] / CELL_SIZE).floor() as i32,
                            ((target[1] - CELL_SIZE) / CELL_SIZE).round() as i32,
                            (target[2] / CELL_SIZE).floor() as i32,
                        );
                        assert!(matches!(world.block(soil), Block::Dirt | Block::Grass));
                        worked_late[index] = true;
                    }
                    ResidentAction::Working if resident.phase == Phase::Working => {
                        assert!(at_station(resident.body.position, route.work));
                        worked_late[index] = true;
                    }
                    _ => {}
                }
            }
        }
        let idle: Vec<_> = life
            .residents
            .iter()
            .enumerate()
            .filter(|(index, resident)| resident.trail.is_none() && !worked_late[*index])
            .map(|(_, resident)| {
                (
                    resident.snapshot.id,
                    resident.phase,
                    resident.waypoint,
                    resident.stuck,
                    resident.body.position,
                    resident.snapshot.target,
                )
            })
            .collect();
        assert!(
            idle.is_empty(),
            "Local workers stopped working or physically delivering during the final 300 seconds: {idle:?}",
        );
        for (index, economy) in life.villages.iter().enumerate() {
            let (harvests, deliveries) = late_counts[index];
            assert!(
                economy.harvests > harvests && economy.deliveries > deliveries,
                "Village {index} stopped harvesting or physically delivering: {harvests}/{deliveries} -> {}/{}",
                economy.harvests,
                economy.deliveries,
            );
        }
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

    #[test]
    fn fallen_farmer_walks_to_the_entrance_and_keeps_completed_return_with_cargo() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let mut life = VillageLife::new(&world);
        let village = &world.settlements().unwrap().villages[3];
        let route = &village.resident_routes[2];
        let path = farm_path(&world, village, 2);
        // This generated return corner is supported by neighboring raised
        // columns. A small collision sidestep falls onto ground 1.5 m lower.
        let gate = path
            .iter()
            .rposition(|point| point.position == [-1247.25, 226.5, -5638.75])
            .unwrap();
        let task = gate + 1;
        let mut resident = life
            .residents
            .iter()
            .find(|resident| resident.village == 3 && resident.route == 2)
            .unwrap()
            .clone();
        resident.phase = Phase::ToFarmExit;
        resident.waypoint = route.path.len() - 1;
        resident.farm_waypoint = gate;
        resident.farm_path = path.clone();
        resident.body = Body::new([-1247.22, 225.0, -5638.78]);
        move_character_with_obstacles(&world, &mut resident.body, MoveInput::default(), 0.05, &[]);
        resident.snapshot.position = resident.body.position;
        resident.snapshot.hunger = 75.0;
        resident.snapshot.energy = 20.0;
        resident.stuck = 10.0;
        resident.resume = Some(Resume {
            phase: Phase::Working,
            waypoint: resident.waypoint,
            elapsed: 0.7,
            farm_waypoint: task,
        });
        let cargo = ResourceCargo {
            kind: ResourceKind::Food,
            amount: 12.0,
        };
        resident.snapshot.carrying = Some(cargo.clone());
        assert!(resident.body.on_ground);
        assert!(!farm_edge_is_walkable(
            &world,
            resident.body.position,
            path[gate].position
        ));
        assert!(farm_edge_is_walkable(
            &world,
            resident.body.position,
            path[0].position
        ));
        life.residents = vec![resident];
        let mut visited_entrance = false;
        let mut resumed = false;
        for _ in 0..6_000 {
            let previous = life.residents[0].body.position;
            life.tick(&world, 0.05);
            let resident = &life.residents[0];
            assert!(horizontal_distance(previous, resident.body.position) <= 0.15);
            assert_eq!(resident.snapshot.carrying, Some(cargo.clone()));
            visited_entrance |= at_station(resident.body.position, path[0].position)
                && (resident.farm_waypoint == 0 || resident.farm_waypoint == path.len() - 1);
            if resident.phase == Phase::Working && resident.resume.is_none() {
                assert!(visited_entrance);
                assert_eq!(resident.farm_path, path);
                assert_eq!(resident.farm_waypoint, path.len() - 1);
                assert!((resident.elapsed - 0.7).abs() < 0.001);
                assert!(farm_point_reached(resident, path.last().unwrap()));
                assert!(resident.snapshot.hunger < HUNGRY && resident.snapshot.energy > 75.0);
                resumed = true;
                break;
            }
        }
        assert!(resumed, "{:?}", life.residents());
    }

    #[test]
    fn late_field_needs_trips_use_the_nearby_entrance_and_resume_from_that_end() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let original = VillageLife::new(&world);
        let index = original
            .residents
            .iter()
            .position(|resident| resident.village == 6 && resident.route == 1)
            .unwrap();
        let village = &world.settlements().unwrap().villages[6];
        let route = &village.resident_routes[1];
        let path = farm_path(&world, village, 1);
        assert!(path.len() > 300);
        let last_work = path.iter().rposition(|point| point.work).unwrap();
        for task in [last_work, path.len() - 10] {
            let resumed_task = if task > last_work {
                path.len() - 1
            } else {
                task
            };
            assert_eq!(farm_needs_entrance(&path, task), path.len() - 1);
            for legacy_retreat in [false, true] {
                let mut life = original.clone();
                let resident = &mut life.residents[index];
                resident.phase = if legacy_retreat {
                    Phase::ToFarmExit
                } else {
                    Phase::Working
                };
                resident.waypoint = route.path.len() - 1;
                resident.farm_waypoint = if legacy_retreat { task - 1 } else { task };
                resident.body = Body::new(path[resident.farm_waypoint].position);
                resident.snapshot.position = resident.body.position;
                resident.snapshot.hunger = 75.0;
                // Exercise unfinished crop work with a meal; the completed
                // return case covers both a meal and a rest on this long tour.
                resident.snapshot.energy = if task == last_work { 100.0 } else { 20.0 };
                resident.farm_path = path.clone();
                resident.elapsed = 0.7;
                if legacy_retreat {
                    resident.resume = Some(Resume {
                        phase: Phase::Working,
                        waypoint: resident.waypoint,
                        elapsed: 0.7,
                        farm_waypoint: task,
                    });
                    resident.elapsed = 0.0;
                }
                let cargo = ResourceCargo {
                    kind: ResourceKind::Food,
                    amount: 5.0,
                };
                resident.snapshot.carrying = Some(cargo.clone());
                assert!(life.validate(&world));
                life = serde_json::from_slice(&serde_json::to_vec(&life).unwrap()).unwrap();
                assert!(life.validate(&world));
                let resident = life.residents[index].clone();
                life.residents = vec![resident];
                let mut forward_exit = false;
                let mut visited_resume = false;
                let mut resumed = false;
                for _ in 0..6_000 {
                    let previous = life.residents[0].body.position;
                    life.tick(&world, 0.05);
                    let resident = &life.residents[0];
                    assert!(horizontal_distance(previous, resident.body.position) <= 0.15);
                    assert_eq!(resident.snapshot.carrying, Some(cargo.clone()));
                    forward_exit |=
                        resident.phase == Phase::ToFarmExit && resident.farm_waypoint > task;
                    visited_resume |= resident.phase == Phase::ToFarmResume
                        && resident.snapshot.hunger < HUNGRY
                        && resident.snapshot.energy > 75.0;
                    if matches!(resident.phase, Phase::ToFarmExit | Phase::ToFarmResume) {
                        let mut validation = original.clone();
                        validation.residents[index] = resident.clone();
                        assert!(
                            validation.validate(&world),
                            "Invalid forward exit/resumption: {:?}",
                            resident
                        );
                    }
                    if forward_exit
                        && visited_resume
                        && resident.phase == Phase::Working
                        && resident.resume.is_none()
                    {
                        assert_eq!(resident.farm_waypoint, resumed_task);
                        assert_eq!(resident.farm_path, path);
                        assert!((resident.elapsed - 0.7).abs() < 0.001);
                        assert!(farm_point_reached(resident, &path[resumed_task]));
                        if task > last_work {
                            assert!(
                                resident.snapshot.hunger < HUNGRY
                                    && resident.snapshot.energy > 75.0
                            );
                        }
                        resumed = true;
                        break;
                    }
                }
                assert!(
                    resumed,
                    "task={task}, legacy={legacy_retreat}: {:?}",
                    life.residents()
                );
            }
        }
    }
}
