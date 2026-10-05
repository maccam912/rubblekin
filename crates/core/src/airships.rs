//! Scheduled village transport. Ports and routes are additive world entities;
//! they never replace generated terrain or become saved block edits.
use crate::{
    geography::{GRID_SIDE, GRID_SPACING, WORLD_SIZE},
    physics::PLAYER_RADIUS,
    settlement::Village,
    world::{CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};

pub const MAX_AIRSHIP_SEATS: u8 = 64;
pub const AIRSHIP_CAPACITY: u8 = MAX_AIRSHIP_SEATS;
pub const AIRSHIP_DWELL_SECONDS: f64 = 30.0;
pub const AIRSHIP_TURN_SECONDS: f64 = 10.0;
pub const AIRSHIP_HEADWAY_SECONDS: f64 = 180.0;
pub const AIRSHIP_SPEED: f32 = 48.0;
pub const AIRSHIP_VERTICAL_SPEED: f32 = 12.0;
pub const AIRSHIP_DECK_HALF_WIDTH: f32 = 4.0;
pub const AIRSHIP_DECK_HALF_LENGTH: f32 = 8.5;
/// The finite 32.768 km world and at most nine shuttle edges need fewer than
/// this many ships, even at the maximum corner-to-corner flight distance.
pub const MAX_AIRSHIPS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirshipRide {
    pub ship_id: u64,
    /// An NPC's reserved spot, or u8::MAX for a freely moving passenger.
    pub seat: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AirshipPort {
    pub village_id: u32,
    /// Existing walkable village marker linking the individual landings.
    pub position: [f32; 3],
    /// Existing lane from the village store landing to the boarding marker.
    pub approach: Vec<[f32; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AirshipRamp {
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub width: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AirshipRoute {
    pub id: u32,
    pub from: u32,
    pub to: u32,
    pub cruise_height: f32,
    /// Flight time in either direction, excluding the dock dwell.
    pub travel_seconds: f64,
    pub ship_count: u32,
    from_dock: [f32; 3],
    to_dock: [f32; 3],
    from_yaw: f32,
    to_yaw: f32,
    phase_offset: f64,
    from_approach: Vec<[f32; 3]>,
    to_approach: Vec<[f32; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AirshipSnapshot {
    pub id: u64,
    pub route_id: u32,
    /// Deck foot origin; passengers and pilot stand at this elevation.
    pub position: [f32; 3],
    pub yaw: f32,
    pub from_village: u32,
    pub next_village: u32,
    pub docked_at: Option<u32>,
    pub departure_in: f32,
    pub arrival_in: f32,
    pub pilot_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AirshipJourney {
    pub ship_id: u64,
    /// The next stop, which may be a transfer toward the requested destination.
    pub destination: u32,
    pub departure_in: f32,
    /// Total ETA from the requested time, including waiting before departure.
    pub arrival_in: f32,
    /// ETA for the final requested destination, including any transfers.
    pub destination_arrival_in: f32,
}

#[derive(Debug, Clone, Default)]
pub struct AirshipNetwork {
    ports: Vec<AirshipPort>,
    routes: Vec<AirshipRoute>,
    ramps: Vec<AirshipRamp>,
}

impl AirshipNetwork {
    pub fn new(world: &World) -> Self {
        Self::try_new(world).expect("server-validated world has airship landings")
    }

    pub fn try_new(world: &World) -> Result<Self, String> {
        let Some(settlements) = world.settlements() else {
            return Ok(Self::default());
        };
        let mut network = Self {
            ports: settlements
                .villages
                .iter()
                .map(|v| port(world, v))
                .collect(),
            routes: Vec::new(),
            ramps: Vec::new(),
        };
        // Existing trails already form the village trade graph. Airships fly
        // direct scenic legs over it, with transfers at connecting villages.
        let mut edges: Vec<_> = settlements
            .trails
            .iter()
            .map(|trail| (trail.from.min(trail.to), trail.from.max(trail.to)))
            .collect();
        edges.sort_unstable();
        edges.dedup();
        let mut landings = std::collections::BTreeMap::new();
        for port in &network.ports {
            let count = edges
                .iter()
                .filter(|(a, b)| *a == port.village_id || *b == port.village_id)
                .count();
            let village = settlements
                .villages
                .iter()
                .find(|v| v.id == port.village_id)
                .unwrap();
            let berths = crate::airship_landings::landings(world, village, port, count);
            if berths.len() != count {
                return Err(format!(
                    "Cannot place every airship landing at {} (village {}, seed {}): found {} of {} clear reachable berths along existing roads",
                    village.name,
                    village.id,
                    world.seed,
                    berths.len(),
                    count
                ));
            }
            network
                .ramps
                .extend(berths.iter().flat_map(|b| b.ramps.clone()));
            landings.insert(port.village_id, berths);
        }
        for (index, &(from, to)) in edges.iter().enumerate() {
            let from_port = network.port(from).expect("trail village has a port");
            let to_port = network.port(to).expect("trail village has a port");
            let berth = |village: u32| {
                let incident: Vec<_> = edges
                    .iter()
                    .enumerate()
                    .filter(|(_, (a, b))| *a == village || *b == village)
                    .map(|(i, _)| i)
                    .collect();
                let ordinal = incident.iter().position(|&i| i == index).unwrap();
                (ordinal, incident.len())
            };
            let (from_ordinal, _) = berth(from);
            let (to_ordinal, _) = berth(to);
            let direction = (to_port.position[0] - from_port.position[0])
                .atan2(to_port.position[2] - from_port.position[2]);
            let from_yaw = direction + std::f32::consts::PI;
            let to_yaw = direction;
            let from_landing = &landings[&from][from_ordinal];
            let to_landing = &landings[&to][to_ordinal];
            let from_dock = from_landing.position;
            let to_dock = to_landing.position;
            let cruise_height = cruise_height(world, from_dock, to_dock) + index as f32 * 30.0;
            let travel_seconds = flight_duration(from_dock, to_dock, cruise_height + 30.0);
            let cycle = (AIRSHIP_DWELL_SECONDS + travel_seconds) * 2.0;
            let ship_count = (cycle / AIRSHIP_HEADWAY_SECONDS).ceil().max(1.0) as u32;
            network.routes.push(AirshipRoute {
                id: index as u32,
                from,
                to,
                cruise_height,
                travel_seconds,
                ship_count,
                from_dock,
                to_dock,
                from_yaw,
                to_yaw,
                // Stable route offsets keep arrivals naturally distributed.
                phase_offset: index as f64 * 37.0,
                from_approach: from_landing.approach.clone(),
                to_approach: to_landing.approach.clone(),
            });
        }
        debug_assert!(
            network
                .routes
                .iter()
                .map(|r| r.ship_count as usize)
                .sum::<usize>()
                <= MAX_AIRSHIPS
        );
        Ok(network)
    }

    pub fn ports(&self) -> &[AirshipPort] {
        &self.ports
    }

    pub fn ramps(&self) -> &[AirshipRamp] {
        &self.ramps
    }

    pub fn landing_path(&self, ship_id: u64, village: u32) -> Option<&[[f32; 3]]> {
        let (route_id, _) = decode_ship_id(ship_id)?;
        let route = self.routes.iter().find(|r| r.id == route_id)?;
        if village == route.from {
            Some(&route.from_approach)
        } else if village == route.to {
            Some(&route.to_approach)
        } else {
            None
        }
    }

    pub fn port(&self, village_id: u32) -> Option<&AirshipPort> {
        self.ports.iter().find(|p| p.village_id == village_id)
    }

    pub fn routes(&self) -> &[AirshipRoute] {
        &self.routes
    }

    pub fn dock_positions(&self, village_id: u32) -> Vec<[f32; 3]> {
        self.routes
            .iter()
            .filter_map(|r| {
                if r.from == village_id {
                    Some(r.from_dock)
                } else if r.to == village_id {
                    Some(r.to_dock)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn ships(&self, time: f64) -> Vec<AirshipSnapshot> {
        self.routes
            .iter()
            .flat_map(|route| {
                (0..route.ship_count).map(move |index| sample_ship(route, index, time))
            })
            .collect()
    }

    pub fn ship(&self, id: u64, time: f64) -> Option<AirshipSnapshot> {
        let (route, index) = decode_ship_id(id)?;
        let route = self.routes.iter().find(|r| r.id == route)?;
        (index < route.ship_count).then(|| sample_ship(route, index, time))
    }

    pub fn ride_position(&self, ship: &AirshipSnapshot, seat: u8) -> [f32; 3] {
        ride_position(ship, seat)
    }

    /// Earliest scheduled arrival through the tiny village graph. The returned
    /// ship is the first leg; its final ETA includes waiting at every transfer.
    pub fn next_leg(&self, from: u32, to: u32, time: f64) -> Option<AirshipJourney> {
        if from == to || !time.is_finite() || time < 0.0 {
            return None;
        }
        let from_index = self.ports.iter().position(|p| p.village_id == from)?;
        let to_index = self.ports.iter().position(|p| p.village_id == to)?;
        let mut arrival = vec![f64::INFINITY; self.ports.len()];
        let mut visited = vec![false; self.ports.len()];
        let mut first = vec![None; self.ports.len()];
        arrival[from_index] = time;
        for _ in 0..self.ports.len() {
            let Some(index) = (0..self.ports.len())
                .filter(|&i| !visited[i])
                .min_by(|&a, &b| arrival[a].total_cmp(&arrival[b]))
            else {
                break;
            };
            if !arrival[index].is_finite() {
                break;
            }
            if index == to_index {
                return first[index].map(|mut journey: AirshipJourney| {
                    journey.destination_arrival_in = (arrival[index] - time) as f32;
                    journey
                });
            }
            visited[index] = true;
            let village = self.ports[index].village_id;
            for route in self
                .routes
                .iter()
                .filter(|r| r.from == village || r.to == village)
            {
                let destination = if route.from == village {
                    route.to
                } else {
                    route.from
                };
                let next = self
                    .ports
                    .iter()
                    .position(|p| p.village_id == destination)
                    .unwrap();
                let leg = next_departure(route, village, arrival[index]);
                let next_arrival = arrival[index] + leg.arrival_in as f64;
                if next_arrival < arrival[next] {
                    arrival[next] = next_arrival;
                    first[next] = first[index].or(Some(leg));
                }
            }
        }
        None
    }
}

fn ship_id(route: u32, index: u32) -> u64 {
    (((route as u64) + 1) << 32) | ((index as u64) + 1)
}

fn decode_ship_id(id: u64) -> Option<(u32, u32)> {
    Some((
        ((id >> 32) as u32).checked_sub(1)?,
        (id as u32).checked_sub(1)?,
    ))
}

fn route_phase(route: &AirshipRoute, index: u32, time: f64) -> f64 {
    let cycle = (AIRSHIP_DWELL_SECONDS + route.travel_seconds) * 2.0;
    let time = if time.is_finite() && time >= 0.0 {
        time
    } else {
        0.0
    };
    (time + route.phase_offset + index as f64 * cycle / route.ship_count as f64).rem_euclid(cycle)
}

fn sample_ship(route: &AirshipRoute, index: u32, time: f64) -> AirshipSnapshot {
    let half = AIRSHIP_DWELL_SECONDS + route.travel_seconds;
    let phase = route_phase(route, index, time);
    let returning = phase >= half;
    let local = if returning { phase - half } else { phase };
    let (source, destination, from, to, outgoing_yaw, incoming_yaw) = if returning {
        (
            route.to_dock,
            route.from_dock,
            route.to,
            route.from,
            route.to_yaw,
            route.from_yaw,
        )
    } else {
        (
            route.from_dock,
            route.to_dock,
            route.from,
            route.to,
            route.from_yaw,
            route.to_yaw,
        )
    };
    let docked = local < AIRSHIP_DWELL_SECONDS;
    let departure_in = if docked {
        AIRSHIP_DWELL_SECONDS - local
    } else {
        0.0
    };
    let flight_time = (local - AIRSHIP_DWELL_SECONDS).max(0.0);
    let position = if docked {
        source
    } else {
        flight_position(
            source,
            destination,
            route.cruise_height + if returning { 30.0 } else { 0.0 },
            flight_time,
            route.travel_seconds,
        )
    };
    // Turn slowly at the berth instead of rotating every passenger abruptly
    // when the shuttle reverses at its destination.
    let yaw = if docked {
        incoming_yaw + std::f32::consts::PI * (local / AIRSHIP_TURN_SECONDS).min(1.0) as f32
    } else {
        outgoing_yaw
    };
    AirshipSnapshot {
        id: ship_id(route.id, index),
        route_id: route.id,
        position,
        yaw,
        from_village: from,
        next_village: to,
        docked_at: docked.then_some(from),
        departure_in: departure_in as f32,
        arrival_in: (departure_in + route.travel_seconds - flight_time) as f32,
        pilot_name: format!("Pilot {}-{}", route.id + 1, index + 1),
    }
}

fn next_departure(route: &AirshipRoute, from: u32, time: f64) -> AirshipJourney {
    let half = AIRSHIP_DWELL_SECONDS + route.travel_seconds;
    let cycle = half * 2.0;
    let start = if from == route.from { 0.0 } else { half };
    (0..route.ship_count)
        .map(|index| {
            let phase = route_phase(route, index, time);
            let local = phase - start;
            let departure = if (0.0..AIRSHIP_DWELL_SECONDS).contains(&local) {
                AIRSHIP_DWELL_SECONDS - local
            } else {
                (start - phase).rem_euclid(cycle) + AIRSHIP_DWELL_SECONDS
            };
            AirshipJourney {
                ship_id: ship_id(route.id, index),
                destination: if from == route.from {
                    route.to
                } else {
                    route.from
                },
                departure_in: departure as f32,
                arrival_in: (departure + route.travel_seconds) as f32,
                destination_arrival_in: (departure + route.travel_seconds) as f32,
            }
        })
        .min_by(|a, b| a.departure_in.total_cmp(&b.departure_in))
        .unwrap()
}

fn approach_column(source: [f32; 3], destination: [f32; 3]) -> [f32; 3] {
    let distance = horizontal_distance(source, destination);
    [
        source[0] + (destination[2] - source[2]) / distance * 14.0,
        source[1],
        source[2] - (destination[0] - source[0]) / distance * 14.0,
    ]
}

fn flight_duration(source: [f32; 3], destination: [f32; 3], height: f32) -> f64 {
    6.0 + (height - source[1]) as f64 / AIRSHIP_VERTICAL_SPEED as f64
        + horizontal_distance(source, destination) as f64 / AIRSHIP_SPEED as f64
        + (height - destination[1]) as f64 / AIRSHIP_VERTICAL_SPEED as f64
}

fn flight_position(
    source: [f32; 3],
    destination: [f32; 3],
    height: f32,
    time: f64,
    duration: f64,
) -> [f32; 3] {
    let climb = (height - source[1]) as f64 / AIRSHIP_VERTICAL_SPEED as f64;
    let descent = (height - destination[1]) as f64 / AIRSHIP_VERTICAL_SPEED as f64;
    // The lower direction uses the same leg duration, flying slightly slower.
    // Separate approach columns prevent passing through an arriving shuttle.
    let horizontal = duration - 6.0 - climb - descent;
    let lift = 30.0;
    let raised_source = [source[0], source[1] + lift, source[2]];
    let raised_destination = [destination[0], destination[1] + lift, destination[2]];
    let source_column = approach_column(raised_source, raised_destination);
    let destination_column = approach_column(raised_destination, raised_source);
    let destination_column = [
        destination[0] - (destination_column[0] - destination[0]),
        raised_destination[1],
        destination[2] - (destination_column[2] - destination[2]),
    ];
    let mut top_source = source_column;
    top_source[1] = height;
    let mut top_destination = destination_column;
    top_destination[1] = height;
    let segments = [
        (
            source,
            raised_source,
            lift as f64 / AIRSHIP_VERTICAL_SPEED as f64,
        ),
        (raised_source, source_column, 3.0),
        (
            source_column,
            top_source,
            climb - lift as f64 / AIRSHIP_VERTICAL_SPEED as f64,
        ),
        (top_source, top_destination, horizontal),
        (
            top_destination,
            destination_column,
            descent - lift as f64 / AIRSHIP_VERTICAL_SPEED as f64,
        ),
        (destination_column, raised_destination, 3.0),
        (
            raised_destination,
            destination,
            lift as f64 / AIRSHIP_VERTICAL_SPEED as f64,
        ),
    ];
    let mut remaining = time;
    for (a, b, seconds) in segments {
        if remaining < seconds {
            return interpolate(a, b, (remaining / seconds) as f32);
        }
        remaining -= seconds;
    }
    destination
}

fn cruise_height(world: &World, a: [f32; 3], b: [f32; 3]) -> f32 {
    let geography = world
        .geography()
        .expect("airships require village geography");
    let corridor = GRID_SPACING * 2.0;
    let grid = |p: f32| ((p + WORLD_SIZE * 0.5) / GRID_SPACING).floor() as i32;
    let min_x = grid(a[0].min(b[0]) - corridor).max(0) as usize;
    let max_x = grid(a[0].max(b[0]) + corridor).min(GRID_SIDE as i32 - 1) as usize;
    let min_z = grid(a[2].min(b[2]) - corridor).max(0) as usize;
    let max_z = grid(a[2].max(b[2]) + corridor).min(GRID_SIDE as i32 - 1) as usize;
    let mut height = a[1].max(b[1]);
    for z in min_z..=max_z {
        for x in min_x..=max_x {
            let i = z * GRID_SIDE + x;
            let [px, pz] = geography.grid_position(i);
            if segment_distance([px, 0.0, pz], a, b) <= corridor {
                height = height.max(geography.heights()[i]);
            }
        }
    }
    // Bilinear geography is bounded by its cell corners; detail adds <=1.07m.
    // Ninety meters also covers every original canopy, roof and trail deck,
    // the suspended hull, and a visible scenic gap above mountain ridges.
    height + 90.0
}

fn segment_distance(p: [f32; 3], a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = b[0] - a[0];
    let dz = b[2] - a[2];
    let n = dx * dx + dz * dz;
    let t = if n > 0.0 {
        (((p[0] - a[0]) * dx + (p[2] - a[2]) * dz) / n).clamp(0.0, 1.0)
    } else {
        0.0
    };
    horizontal_distance(p, interpolate(a, b, t))
}

/// All passengers occupy distinct seats and remain inside the visual deck.
pub fn ride_position(ship: &AirshipSnapshot, seat: u8) -> [f32; 3] {
    deck_position(ship, initial_deck_position(seat))
}

pub fn initial_deck_position(seat: u8) -> [f32; 3] {
    let seat = seat.min(MAX_AIRSHIP_SEATS - 1);
    [
        (seat % 8) as f32 * 0.8 - 2.8,
        0.0,
        (seat / 8) as f32 * 0.8 - 2.8,
    ]
}

pub fn deck_position(ship: &AirshipSnapshot, local: [f32; 3]) -> [f32; 3] {
    transform(ship.position, ship.yaw, local)
}

pub fn deck_local_position(ship: &AirshipSnapshot, world: [f32; 3]) -> [f32; 3] {
    let (sin, cos) = ship.yaw.sin_cos();
    let x = world[0] - ship.position[0];
    let z = world[2] - ship.position[2];
    [
        x * cos - z * sin,
        world[1] - ship.position[1],
        x * sin + z * cos,
    ]
}

pub fn pilot_position(ship: &AirshipSnapshot) -> [f32; 3] {
    transform(ship.position, ship.yaw, [0.0, 0.0, -6.5])
}

fn transform(origin: [f32; 3], yaw: f32, offset: [f32; 3]) -> [f32; 3] {
    let (sin, cos) = yaw.sin_cos();
    [
        origin[0] + offset[0] * cos + offset[2] * sin,
        origin[1] + offset[1],
        origin[2] - offset[0] * sin + offset[2] * cos,
    ]
}

fn port(world: &World, village: &Village) -> AirshipPort {
    // The first generated lane joins the existing central street and storage
    // entrance. Walk toward town by four meters to keep the doorway free.
    let lane: Vec<_> = village.lanes[0]
        .points
        .iter()
        .copied()
        .map(|mut p| {
            p[1] = original_support_height(world, p);
            p
        })
        .collect();
    let mut approach = lane.clone();
    let mut remaining = 4.0;
    let mut previous = village.store;
    for &point in lane.iter().rev().skip(1) {
        let distance = horizontal_distance(previous, point);
        if distance >= remaining && distance > 0.0 {
            let mut position = interpolate(previous, point, remaining / distance);
            position[1] = original_support_height(world, position);
            approach.push(position);
            return AirshipPort {
                village_id: village.id,
                position,
                approach,
            };
        }
        remaining -= distance;
        approach.push(point);
        previous = point;
    }
    let mut position = village.store;
    position[1] = original_support_height(world, position);
    AirshipPort {
        village_id: village.id,
        position,
        approach: vec![position],
    }
}

fn original_support_height(world: &World, p: [f32; 3]) -> f32 {
    let min_x = ((p[0] - PLAYER_RADIUS) / CELL_SIZE).floor() as i32;
    let max_x = ((p[0] + PLAYER_RADIUS) / CELL_SIZE).floor() as i32;
    let min_z = ((p[2] - PLAYER_RADIUS) / CELL_SIZE).floor() as i32;
    let max_z = ((p[2] + PLAYER_RADIUS) / CELL_SIZE).floor() as i32;
    let mut top = i32::MIN;
    for z in min_z..=max_z {
        for x in min_x..=max_x {
            top = top.max(world.height_at(x, z));
        }
    }
    (top + 1) as f32 * CELL_SIZE
}

fn horizontal_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn interpolate(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        physics::character_position_is_clear,
        world::{Block, BlockPos, WorldGeneration},
    };
    use std::sync::OnceLock;

    fn village_world() -> &'static World {
        static WORLD: OnceLock<World> = OnceLock::new();
        WORLD.get_or_init(|| World::generate(42, WorldGeneration::GeographyV3))
    }

    #[test]
    fn ports_are_clear_connected_existing_lanes_and_edits_do_not_change_the_plan() {
        let world = village_world();
        let network = AirshipNetwork::new(world);
        assert_eq!(
            network.ports.len(),
            world.settlements().unwrap().villages.len()
        );
        for (port, village) in network
            .ports
            .iter()
            .zip(&world.settlements().unwrap().villages)
        {
            assert_eq!(port.approach[0], village.center);
            assert!(horizontal_distance(port.position, village.store) <= 4.01);
            assert!(
                character_position_is_clear(world, port.position, &[]),
                "port {}",
                port.village_id
            );
            for segment in port.approach.windows(2) {
                let steps = (horizontal_distance(segment[0], segment[1]) / 0.25)
                    .ceil()
                    .max(1.0) as usize;
                let mut previous = segment[0];
                for step in 1..=steps {
                    let mut p = interpolate(segment[0], segment[1], step as f32 / steps as f32);
                    p[1] = original_support_height(world, p);
                    assert!(
                        character_position_is_clear(world, p, &[]),
                        "blocked port approach {} at {p:?}",
                        port.village_id
                    );
                    assert!(
                        (p[1] - previous[1]).abs() <= CELL_SIZE + 0.01,
                        "steep port approach"
                    );
                    previous = p;
                }
            }
        }
        assert!(world.edits().is_empty());
        let mut edited = world.clone();
        let p = network.ports[0].position;
        edited
            .set_block(
                BlockPos::new(
                    (p[0] / CELL_SIZE).floor() as i32,
                    (p[1] / CELL_SIZE).floor() as i32 + 60,
                    (p[2] / CELL_SIZE).floor() as i32,
                ),
                Block::Brick,
            )
            .unwrap();
        let rebuilt = AirshipNetwork::new(&edited);
        assert_eq!(network.ports, rebuilt.ports);
        assert_eq!(network.routes, rebuilt.routes);
        assert_eq!(network.ships(123_456.75), rebuilt.ships(123_456.75));
        assert!(AirshipNetwork::new(&World::new(42)).ships(0.0).is_empty());
    }

    #[test]
    fn departures_are_frequent_every_village_is_reachable_and_transfers_include_waiting() {
        let world = village_world();
        let network = AirshipNetwork::new(world);
        let fleet = network.ships(0.0);
        assert!(fleet.len() <= MAX_AIRSHIPS);
        let first_port = &network.ports[0];
        println!(
            "seed42 routes={} ships={} firstport={:?} spawn_distance={:.2} initial_docked={:?}",
            network.routes.len(),
            fleet.len(),
            first_port.position,
            horizontal_distance(first_port.position, world.spawn_position()),
            fleet
                .iter()
                .filter(|s| s.docked_at == Some(first_port.village_id))
                .map(|s| s.id)
                .collect::<Vec<_>>()
        );
        let ship = fleet
            .iter()
            .find(|s| s.docked_at == Some(first_port.village_id))
            .unwrap();
        let path = network
            .landing_path(ship.id, first_port.village_id)
            .unwrap();
        println!(
            "seed42 ramps={} firstdock={:?} boarding_walk={:.2}m",
            network.ramps.len(),
            ship.position,
            path.windows(2)
                .map(|p| horizontal_distance(p[0], p[1]))
                .sum::<f32>()
        );
        for route in &network.routes {
            let cycle = (AIRSHIP_DWELL_SECONDS + route.travel_seconds) * 2.0;
            assert!(cycle / route.ship_count as f64 <= AIRSHIP_HEADWAY_SECONDS);
            for time in [0.0, 31.0, 179.0, 180.0, 10_000.0, 1_000_000_000.0] {
                for from in [route.from, route.to] {
                    let leg = next_departure(route, from, time);
                    assert!(leg.departure_in <= AIRSHIP_HEADWAY_SECONDS as f32 + 0.01);
                    let ship = network
                        .ship(leg.ship_id, time + leg.departure_in as f64 - 0.01)
                        .unwrap();
                    assert_eq!(ship.docked_at, Some(from));
                    let arrived = network
                        .ship(leg.ship_id, time + leg.arrival_in as f64 + 0.01)
                        .unwrap();
                    assert_eq!(arrived.docked_at, Some(leg.destination));
                }
            }
        }
        let mut transfers = 0;
        for from in &network.ports {
            for to in &network.ports {
                if from.village_id == to.village_id {
                    continue;
                }
                let journey = network
                    .next_leg(from.village_id, to.village_id, 73.25)
                    .unwrap();
                assert!(journey.destination_arrival_in >= journey.arrival_in);
                if journey.destination != to.village_id {
                    transfers += 1;
                    let transfer = network
                        .next_leg(
                            journey.destination,
                            to.village_id,
                            73.25 + journey.arrival_in as f64,
                        )
                        .unwrap();
                    assert!(
                        (journey.destination_arrival_in
                            - journey.arrival_in
                            - transfer.destination_arrival_in)
                            .abs()
                            < 0.05
                    );
                }
            }
        }
        assert!(transfers > 0);
        assert!(network.next_leg(0, 0, 0.0).is_none());
        assert!(network.next_leg(0, u32::MAX, 0.0).is_none());
        assert!(network.ship(0, 0.0).is_none());
    }

    #[test]
    fn flights_and_rotating_docks_clear_terrain_roofs_canopies_and_other_ships() {
        let world = village_world();
        let network = AirshipNetwork::new(world);
        let max_cycle = network
            .routes
            .iter()
            .map(|r| (AIRSHIP_DWELL_SECONDS + r.travel_seconds) * 2.0)
            .fold(0.0, f64::max);
        for tick in 0..=(max_cycle / 60.0).ceil() as usize {
            let time = tick as f64 * 60.0;
            let ships = network.ships(time);
            for ship in &ships {
                for seat in 0..=MAX_AIRSHIP_SEATS {
                    let p = if seat == MAX_AIRSHIP_SEATS {
                        pilot_position(ship)
                    } else {
                        ride_position(ship, seat)
                    };
                    assert!(
                        character_position_is_clear(world, p, &[]),
                        "terrain intersects ship={} seat={} time={time} p={p:?}",
                        ship.id,
                        seat
                    );
                }
            }
            for (i, ship) in ships.iter().enumerate() {
                for other in &ships[i + 1..] {
                    if (ship.position[1] - other.position[1]).abs() >= 2.0
                        || horizontal_distance(ship.position, other.position) > 14.0
                    {
                        continue;
                    }
                    for seat in 0..=MAX_AIRSHIP_SEATS {
                        let p = if seat == MAX_AIRSHIP_SEATS {
                            pilot_position(ship)
                        } else {
                            ride_position(ship, seat)
                        };
                        for other_seat in 0..=MAX_AIRSHIP_SEATS {
                            let q = if other_seat == MAX_AIRSHIP_SEATS {
                                pilot_position(other)
                            } else {
                                ride_position(other, other_seat)
                            };
                            assert!(
                                horizontal_distance(p, q).powi(2) + (p[1] - q[1]).powi(2) >= 4.0,
                                "ships intersect: {} and {} time={time}",
                                ship.id,
                                other.id
                            );
                        }
                    }
                }
            }
        }
        // Dense center/corner cruise probes check the mountain corridor too.
        for route in &network.routes {
            for direction in [false, true] {
                let (a, b) = if direction {
                    (route.to_dock, route.from_dock)
                } else {
                    (route.from_dock, route.to_dock)
                };
                let height = route.cruise_height + if direction { 30.0 } else { 0.0 };
                for tick in 0..=(route.travel_seconds / 2.0).ceil() as usize {
                    let p = flight_position(
                        a,
                        b,
                        height,
                        (tick as f64 * 2.0).min(route.travel_seconds),
                        route.travel_seconds,
                    );
                    for [x, z] in [
                        [0.0, 0.0],
                        [-4.0, -8.5],
                        [4.0, -8.5],
                        [-4.0, 8.5],
                        [4.0, 8.5],
                    ] {
                        let q = transform(p, route.from_yaw, [x, 0.0, z]);
                        assert!(
                            q[1] > world.original_surface_height(q[0], q[2]) + 0.30,
                            "flight terrain at {q:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn saved_ride_wire_defaults_and_passenger_spots_are_distinct() {
        let ship = AirshipSnapshot {
            id: 1,
            route_id: 0,
            position: [8000.0, 400.0, -8000.0],
            yaw: 1.3,
            from_village: 0,
            next_village: 1,
            docked_at: None,
            departure_in: 0.0,
            arrival_in: 100.0,
            pilot_name: "Pilot".into(),
        };
        for seat in 0..MAX_AIRSHIP_SEATS {
            let p = ride_position(&ship, seat);
            for other in seat + 1..MAX_AIRSHIP_SEATS {
                assert!(horizontal_distance(p, ride_position(&ship, other)) > PLAYER_RADIUS * 2.0);
            }
            assert!(horizontal_distance(p, pilot_position(&ship)) > PLAYER_RADIUS * 2.0);
        }
        let snapshot: crate::protocol::PlayerSnapshot = serde_json::from_value(serde_json::json!({
            "id":1,"name":"Player","body":{"position":[0,0,0],"velocity":[0,0,0],"on_ground":false},"yaw":0,"last_input_sequence":0,"movement_epoch":0
        })).unwrap();
        assert!(snapshot.ride.is_none());
        assert!(snapshot.deck_position.is_none());
        let ride = AirshipRide {
            ship_id: 1,
            seat: 63,
        };
        assert_eq!(
            serde_json::from_str::<AirshipRide>(&serde_json::to_string(&ride).unwrap()).unwrap(),
            ride
        );
    }

    #[test]
    fn other_islands_have_safe_ports_bounded_fleets_and_connected_services() {
        for seed in [7, 99] {
            let world = World::generate(seed, WorldGeneration::GeographyV3);
            let network = AirshipNetwork::new(&world);
            assert_eq!(
                network.ports.len(),
                world.settlements().unwrap().villages.len()
            );
            assert!(network.ships(0.0).len() <= MAX_AIRSHIPS);
            for port in network.ports() {
                assert!(
                    character_position_is_clear(&world, port.position, &[]),
                    "seed={seed} port={}",
                    port.village_id
                );
                for destination in network
                    .ports()
                    .iter()
                    .filter(|p| p.village_id != port.village_id)
                {
                    assert!(
                        network
                            .next_leg(port.village_id, destination.village_id, 51.0)
                            .is_some()
                    );
                }
            }
            for route in &network.routes {
                assert!(
                    (AIRSHIP_DWELL_SECONDS + route.travel_seconds) * 2.0 / route.ship_count as f64
                        <= AIRSHIP_HEADWAY_SECONDS
                );
                for direction in [false, true] {
                    let (a, b) = if direction {
                        (route.to_dock, route.from_dock)
                    } else {
                        (route.from_dock, route.to_dock)
                    };
                    let height = route.cruise_height + if direction { 30.0 } else { 0.0 };
                    for tick in 0..=(route.travel_seconds / 8.0).ceil() as usize {
                        let p = flight_position(
                            a,
                            b,
                            height,
                            (tick as f64 * 8.0).min(route.travel_seconds),
                            route.travel_seconds,
                        );
                        assert!(
                            character_position_is_clear(&world, p, &[]),
                            "seed={seed} route={} p={p:?}",
                            route.id
                        );
                    }
                }
            }
        }
    }
}
