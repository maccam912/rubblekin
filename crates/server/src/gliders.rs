//! Small transient carriage service. Every request rechecks the current player,
//! station, seat capacity and live target; no client supplies flight coordinates.
use crate::{Connection, persistence::Simulation};
use rubblekin_core::{
    gliders::*,
    physics::Body,
    protocol::{PlayerSnapshot, ServerMessage},
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct GliderService {
    pub stations: Vec<WhipStation>,
    pub flights: Vec<GliderFlight>,
    next_id: u64,
    personal: BTreeMap<u64, (WhipStation, u64)>,
}
impl GliderService {
    pub fn new(world: &rubblekin_core::world::World) -> Self {
        Self {
            stations: stations(world),
            next_id: 1,
            ..Self::default()
        }
    }
}
fn notice(connections: &mut BTreeMap<u64, Connection>, id: u64, text: &str) {
    connections
        .get_mut(&id)
        .unwrap()
        .send(&ServerMessage::Notice { text: text.into() });
}
fn reset_movement(player: &mut PlayerSnapshot) {
    player.movement_epoch += 1;
    player.last_input_sequence = 0;
}

pub(crate) fn handle(
    id: u64,
    action: GliderAction,
    connections: &mut BTreeMap<u64, Connection>,
    sim: &mut Simulation,
) {
    let Some(player) = connections.get(&id).and_then(|c| c.player.as_ref()) else {
        return;
    };
    match action {
        GliderAction::Emergency => {
            if player.vehicle.is_some() || player.glider_ride.is_some() || player.ride.is_some() {
                notice(
                    connections,
                    id,
                    "Leave your ride before deploying a return station.",
                );
                return;
            }
            if sim.gliders.personal.contains_key(&id) {
                notice(
                    connections,
                    id,
                    "Your return station is already waiting. Board or put it away first.",
                );
                return;
            }
            if sim.gliders.flights.len() >= 32 {
                notice(
                    connections,
                    id,
                    "All carriages are in use. Try again shortly.",
                );
                return;
            }
            let Some(mut origin) = landing_near(&sim.world, player.body.position) else {
                notice(
                    connections,
                    id,
                    "Move to clear, dry ground to deploy your return station.",
                );
                return;
            };
            origin[1] -= 0.65;
            let station = WhipStation {
                temporary: true,
                village_id: u32::MAX.saturating_sub(sim.gliders.next_id as u32),
                name: "Personal return".into(),
                position: origin,
                landing_position: origin,
            };
            let mut towns = sim.gliders.stations.clone();
            towns.sort_by(|a, b| {
                horizontal_distance(origin, a.landing_position)
                    .total_cmp(&horizontal_distance(origin, b.landing_position))
            });
            let Some(flight) = towns.iter().find_map(|town| {
                GliderFlight::plan_return(
                    &sim.world,
                    &station,
                    GliderDestination::Village(town.village_id),
                    town.name.clone(),
                    town.landing_position,
                    sim.gliders.next_id,
                    sim.world_time,
                )
                .ok()
            }) else {
                notice(
                    connections,
                    id,
                    "No safe route to town from here. Move to another clearing and try again.",
                );
                return;
            };
            let obstacles: Vec<_> = connections
                .values()
                .filter(|c| !c.dead)
                .filter_map(|c| c.player.as_ref())
                .filter(|p| p.id != id)
                .map(|p| p.body.position)
                .chain(std::iter::once(sim.npc.snapshot.position))
                .chain(sim.villages.positions())
                .collect();
            if !rubblekin_core::physics::character_position_is_clear(
                &sim.world,
                seat_position(flight.pose(sim.world_time), 0),
                &obstacles,
            ) {
                notice(
                    connections,
                    id,
                    "Someone is in the launch space. Move clear and try again.",
                );
                return;
            }
            let flight_id = flight.id;
            let player = connections.get_mut(&id).unwrap().player.as_mut().unwrap();
            player.glider_ride = Some(GliderRide {
                carriage_id: flight.id,
                seat: 0,
            });
            player.gliding = false;
            player.body = Body::new(seat_position(flight.pose(sim.world_time), 0));
            player.body.on_ground = true;
            reset_movement(player);
            sim.gliders.next_id += 1;
            sim.gliders.flights.push(flight);
            sim.gliders
                .personal
                .insert(id, (station.clone(), flight_id));
            connections
                .get_mut(&id)
                .unwrap()
                .send(&ServerMessage::EmergencyStation {
                    station: Some(station),
                });
            notice(
                connections,
                id,
                "Your personal return station is ready. Launch once to return to town.",
            );
            let state = ServerMessage::State {
                players: crate::players(connections, sim),
                npc: sim.npc.snapshot.clone(),
                residents: sim.villages.residents(),
                villages: sim.villages.villages(),
                gliders: sim.gliders.flights.clone(),
                world_time: sim.world_time,
            };
            connections.get_mut(&id).unwrap().send(&state);
        }
        GliderAction::Board {
            station_id,
            destination,
        } => {
            if player.vehicle.is_some() {
                notice(connections, id, "Leave your vehicle before boarding.");
                return;
            }
            if player.glider_ride.is_some() {
                notice(
                    connections,
                    id,
                    "You are already aboard. Launch or jump out first.",
                );
                return;
            }
            let Some(station) = sim
                .gliders
                .stations
                .iter()
                .find(|s| s.village_id == station_id)
            else {
                notice(connections, id, "That whip station is unavailable.");
                return;
            };
            if rubblekin_core::gliders::distance(player.body.position, station.position)
                > STATION_REACH
            {
                notice(connections, id, "Move closer to the whip station to board.");
                return;
            }
            let existing = sim
                .gliders
                .flights
                .iter()
                .position(|f| f.station_id == station_id && f.started_at.is_none());
            let index = if let Some(index) = existing {
                index
            } else {
                if sim.gliders.flights.len() >= 32 {
                    notice(
                        connections,
                        id,
                        "All carriages are in use. Try again shortly.",
                    );
                    return;
                }
                let target = match destination {
                    GliderDestination::Village(v) => sim
                        .gliders
                        .stations
                        .iter()
                        .find(|s| s.village_id == v && v != station_id)
                        .map(|s| (s.name.clone(), s.landing_position)),
                    GliderDestination::Player(other) => connections
                        .get(&other)
                        .filter(|c| !c.dead && other != id)
                        .and_then(|c| c.player.as_ref())
                        .map(|p| (format!("Meet {}", p.name), p.body.position)),
                };
                let Some((name, position)) = target else {
                    notice(connections, id, "That destination is no longer available.");
                    return;
                };
                let result = GliderFlight::plan(
                    &sim.world,
                    station,
                    destination,
                    name,
                    position,
                    sim.gliders.next_id,
                    sim.world_time,
                );
                let flight = match result {
                    Ok(f) => f,
                    Err(reason) => {
                        notice(connections, id, reason);
                        return;
                    }
                };
                sim.gliders.next_id += 1;
                sim.gliders.flights.push(flight);
                sim.gliders.flights.len() - 1
            };
            let flight = &sim.gliders.flights[index];
            let occupied: Vec<_> = connections
                .values()
                .filter(|c| !c.dead)
                .filter_map(|c| c.player.as_ref().and_then(|p| p.glider_ride))
                .filter(|r| r.carriage_id == flight.id)
                .map(|r| r.seat)
                .collect();
            let Some(seat) = (0..GLIDER_SEATS).find(|s| !occupied.contains(s)) else {
                notice(
                    connections,
                    id,
                    "This four-seat carriage is full. Wait for its departure.",
                );
                return;
            };
            let player = connections.get_mut(&id).unwrap().player.as_mut().unwrap();
            player.glider_ride = Some(GliderRide {
                carriage_id: flight.id,
                seat,
            });
            player.gliding = false;
            player.ride = None;
            player.deck_position = None;
            player.body = Body::new(seat_position(flight.pose(sim.world_time), seat));
            player.body.on_ground = true;
            reset_movement(player);
            notice(
                connections,
                id,
                "Aboard. Friends can join the other seats; anyone aboard can launch.",
            );
        }
        GliderAction::Launch => {
            let Some(ride) = player.glider_ride else {
                notice(connections, id, "Board a carriage before launching.");
                return;
            };
            let Some(index) = sim
                .gliders
                .flights
                .iter()
                .position(|f| f.id == ride.carriage_id && f.started_at.is_none())
            else {
                notice(connections, id, "This carriage has already launched.");
                return;
            };
            let flight = &sim.gliders.flights[index];
            let personal = sim
                .gliders
                .personal
                .get(&id)
                .filter(|(_, f)| *f == flight.id);
            let Some(station) = personal.map(|(s, _)| s).or_else(|| {
                sim.gliders
                    .stations
                    .iter()
                    .find(|s| s.village_id == flight.station_id)
            }) else {
                return;
            };
            let target = match flight.destination {
                GliderDestination::Village(v) => sim
                    .gliders
                    .stations
                    .iter()
                    .find(|s| s.village_id == v)
                    .map(|s| s.landing_position),
                GliderDestination::Player(other) => connections
                    .get(&other)
                    .filter(|c| !c.dead)
                    .and_then(|c| c.player.as_ref())
                    .map(|p| p.body.position),
            };
            let Some(target) = target else {
                notice(
                    connections,
                    id,
                    "Your friend left the server. Jump out and choose another destination.",
                );
                return;
            };
            let planner = if personal.is_some() {
                GliderFlight::plan_return
            } else {
                GliderFlight::plan
            };
            let result = planner(
                &sim.world,
                station,
                flight.destination,
                flight.destination_name.clone(),
                target,
                flight.id,
                flight.created_at,
            );
            match result {
                Ok(mut f) => {
                    f.started_at = Some(sim.world_time);
                    sim.gliders.flights[index] = f;
                    if sim.gliders.personal.remove(&id).is_some() {
                        connections
                            .get_mut(&id)
                            .unwrap()
                            .send(&ServerMessage::EmergencyStation { station: None });
                    }
                    notice(
                        connections,
                        id,
                        "Launching! Stay aboard for automatic landing, or Jump to explore.",
                    );
                }
                Err(reason) => notice(connections, id, reason),
            }
        }
        GliderAction::Leave => {
            let player = connections.get_mut(&id).unwrap().player.as_mut().unwrap();
            if let Some(ride) = player.glider_ride.take() {
                if let Some(f) = sim
                    .gliders
                    .flights
                    .iter()
                    .find(|f| f.id == ride.carriage_id)
                {
                    let pose = f.pose(sim.world_time);
                    player.body.position = seat_position(pose, ride.seat);
                    player.body.velocity = f.velocity(sim.world_time);
                    player.gliding = f.started_at.is_some() && !pose.landed;
                }
                player.body.position[0] += if ride.seat.is_multiple_of(2) {
                    -2.0
                } else {
                    2.0
                };
                player.body.on_ground = false;
                reset_movement(player);
            }
            if let Some((_, flight)) = sim.gliders.personal.remove(&id) {
                sim.gliders.flights.retain(|f| f.id != flight);
                connections
                    .get_mut(&id)
                    .unwrap()
                    .send(&ServerMessage::EmergencyStation { station: None });
            }
        }
    }
}

pub(crate) fn tick(connections: &mut BTreeMap<u64, Connection>, sim: &mut Simulation) {
    let abandoned: Vec<_> = sim
        .gliders
        .personal
        .iter()
        .filter(|(id, (_, flight))| {
            connections.get(id).is_none_or(|c| {
                c.dead
                    || c.player
                        .as_ref()
                        .is_none_or(|p| p.glider_ride.is_none_or(|r| r.carriage_id != *flight))
            })
        })
        .map(|(id, _)| *id)
        .collect();
    for id in abandoned {
        if let Some((_, flight)) = sim.gliders.personal.remove(&id) {
            sim.gliders.flights.retain(|f| f.id != flight);
        }
        if let Some(c) = connections.get_mut(&id).filter(|c| !c.dead) {
            c.send(&ServerMessage::EmergencyStation { station: None });
        }
    }
    for connection in connections.values_mut().filter(|c| !c.dead) {
        let Some(player) = &mut connection.player else {
            continue;
        };
        let Some(ride) = player.glider_ride else {
            continue;
        };
        if let Some(flight) = sim
            .gliders
            .flights
            .iter()
            .find(|f| f.id == ride.carriage_id)
        {
            let pose = flight.pose(sim.world_time);
            player.body.position = seat_position(pose, ride.seat);
            player.body.velocity = flight.velocity(sim.world_time);
            player.body.on_ground = flight.started_at.is_none() || pose.landed;
            if pose.landed {
                player.glider_ride = None;
                player.gliding = false;
                reset_movement(player);
            }
        } else {
            player.glider_ride = None;
            player.gliding = true;
            reset_movement(player);
        }
    }
    let players: Vec<_> = connections
        .values()
        .filter(|c| !c.dead)
        .filter_map(|c| c.player.as_ref())
        .collect();
    sim.gliders.flights.retain(|f| {
        if players
            .iter()
            .any(|p| p.glider_ride.is_some_and(|r| r.carriage_id == f.id))
        {
            return true;
        }
        let age = sim.world_time - f.created_at;
        let finished = f
            .started_at
            .is_none_or(|start| sim.world_time >= start + f.duration);
        // Occupied vehicles never disappear. Empty vehicles remain at least
        // twenty seconds, and a visible landing lingers for twenty seconds.
        let linger = f
            .started_at
            .is_some_and(|start| sim.world_time < start + f.duration + 20.0);
        age < 20.0
            || (finished && linger)
            || (age < 600.0
                && players.iter().any(|p| {
                    horizontal_distance(p.body.position, f.pose(sim.world_time).position)
                        < CARRIAGE_VISIBILITY
                }))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::airship_tests::Fixture;
    use rubblekin_core::protocol::ClientMessage;
    fn fixture() -> Fixture {
        let mut f = Fixture::new();
        f.network = rubblekin_core::airships::AirshipNetwork::default();
        f.sim.gliders = GliderService::new(&f.sim.world);
        // Village IDs follow generated site order, not flight connectivity.
        // Exercise seats/lifecycle on a nearby hop instead of assuming that
        // the first two towns can clear every mountain between them.
        let stops = &mut f.sim.gliders.stations;
        let nearest = (1..stops.len())
            .min_by(|&a, &b| {
                horizontal_distance(stops[0].position, stops[a].position)
                    .total_cmp(&horizontal_distance(stops[0].position, stops[b].position))
            })
            .unwrap();
        stops.swap(1, nearest);
        f
    }
    #[test]
    fn emergency_station_is_personal_single_use_and_cleans_up_on_disconnect() {
        let mut f = fixture();
        let origin = f.sim.gliders.stations[0].position;
        f.add_player(1, origin);
        f.add_player(2, [origin[0] + 12., origin[1], origin[2]]);
        handle(1, GliderAction::Emergency, &mut f.connections, &mut f.sim);
        assert_eq!(f.sim.gliders.personal.len(), 1);
        let (station, flight) = f.sim.gliders.personal[&1].clone();
        let ride = f.connections[&1]
            .player
            .as_ref()
            .unwrap()
            .glider_ride
            .unwrap();
        assert_eq!(ride.carriage_id, flight);
        handle(
            2,
            GliderAction::Board {
                station_id: station.village_id,
                destination: f.sim.gliders.flights[0].destination,
            },
            &mut f.connections,
            &mut f.sim,
        );
        assert!(
            f.connections[&2]
                .player
                .as_ref()
                .unwrap()
                .glider_ride
                .is_none()
        );
        handle(1, GliderAction::Emergency, &mut f.connections, &mut f.sim);
        assert_eq!(f.sim.gliders.flights.len(), 1);
        handle(1, GliderAction::Launch, &mut f.connections, &mut f.sim);
        assert!(f.sim.gliders.personal.is_empty());
        assert!(f.sim.gliders.flights[0].started_at.is_some());
        handle(1, GliderAction::Launch, &mut f.connections, &mut f.sim);
        assert_eq!(f.sim.gliders.flights.len(), 1);
        f.sim.world_time = 1000.;
        tick(&mut f.connections, &mut f.sim);
        assert!(
            f.connections[&1]
                .player
                .as_ref()
                .unwrap()
                .glider_ride
                .is_none()
        );
        assert!(rubblekin_core::physics::character_position_is_clear(
            &f.sim.world,
            f.connections[&1].player.as_ref().unwrap().body.position,
            &[]
        ));
        handle(2, GliderAction::Emergency, &mut f.connections, &mut f.sim);
        assert!(f.sim.gliders.personal.contains_key(&2));
        f.connections.get_mut(&2).unwrap().dead = true;
        tick(&mut f.connections, &mut f.sim);
        assert!(f.sim.gliders.personal.is_empty());
    }
    #[test]
    fn emergency_return_waives_hop_range_but_keeps_safe_landings() {
        let mut f = fixture();
        let stops = f.sim.gliders.stations.clone();
        let (origin, target) = stops
            .iter()
            .flat_map(|a| stops.iter().map(move |b| (a, b)))
            .max_by(|(a, b), (c, d)| {
                horizontal_distance(a.position, b.landing_position)
                    .total_cmp(&horizontal_distance(c.position, d.landing_position))
            })
            .unwrap();
        assert!(horizontal_distance(origin.position, target.landing_position) > LAUNCH_RANGE);
        assert!(
            GliderFlight::plan(
                &f.sim.world,
                origin,
                GliderDestination::Village(target.village_id),
                target.name.clone(),
                target.landing_position,
                1,
                0.
            )
            .is_err()
        );
        let flight = GliderFlight::plan_return(
            &f.sim.world,
            origin,
            GliderDestination::Village(target.village_id),
            target.name.clone(),
            target.landing_position,
            1,
            0.,
        )
        .unwrap();
        assert!(rubblekin_core::physics::character_position_is_clear(
            &f.sim.world,
            flight.to,
            &[]
        ));
        f.sim.gliders.stations = vec![target.clone()];
        f.add_player(1, origin.position);
        handle(1, GliderAction::Emergency, &mut f.connections, &mut f.sim);
        let ride = f.connections[&1]
            .player
            .as_ref()
            .unwrap()
            .glider_ride
            .unwrap();
        let flight = f
            .sim
            .gliders
            .flights
            .iter()
            .find(|f| f.id == ride.carriage_id)
            .unwrap();
        assert!(horizontal_distance(flight.from, flight.to) > LAUNCH_RANGE);
    }
    fn board(f: &mut Fixture, id: u64, destination: GliderDestination) {
        let s = &f.sim.gliders.stations[0];
        handle(
            id,
            GliderAction::Board {
                station_id: s.village_id,
                destination,
            },
            &mut f.connections,
            &mut f.sim,
        );
    }
    #[test]
    fn airborne_profiles_resume_with_canopy_without_a_saved_carriage() {
        let mut f = fixture();
        let s = f.sim.gliders.stations[0].clone();
        let d = GliderDestination::Village(f.sim.gliders.stations[1].village_id);
        f.add_player(1, s.position);
        board(&mut f, 1, d);
        handle(1, GliderAction::Launch, &mut f.connections, &mut f.sim);
        f.sim.world_time = 20.0;
        tick(&mut f.connections, &mut f.sim);
        let player = f.connections[&1].player.as_ref().unwrap();
        let mut saved = crate::player_economy::SavedPlayer::new(player);
        saved.ledger.coins = 7;
        saved.ledger.cargo[0] = 2;
        let bytes = serde_json::to_vec(&saved).unwrap();
        let saved: crate::player_economy::SavedPlayer = serde_json::from_slice(&bytes).unwrap();
        let restored = saved
            .restore(
                &f.sim.world,
                &rubblekin_core::airships::AirshipNetwork::default(),
                20.0,
                &[],
                9,
                "Again".into(),
            )
            .unwrap();
        assert!(restored.glider_ride.is_none() && restored.gliding);
        assert_eq!(restored.body.position, player.body.position);
        assert_eq!(saved.ledger.coins, 7);
        assert_eq!(saved.ledger.cargo[0], 2);
    }
    #[test]
    fn four_seats_solo_launch_jump_and_arrival() {
        let mut f = fixture();
        let origin = f.sim.gliders.stations[0].position;
        let destination = GliderDestination::Village(f.sim.gliders.stations[1].village_id);
        for id in 1..=5 {
            f.add_player(id, origin);
            board(&mut f, id, destination);
        }
        assert_eq!(f.sim.gliders.flights.len(), 1);
        let seats: Vec<_> = f
            .connections
            .values()
            .filter_map(|c| c.player.as_ref().unwrap().glider_ride.map(|r| r.seat))
            .collect();
        assert_eq!(seats, vec![0, 1, 2, 3]);
        assert!(
            f.connections[&5]
                .player
                .as_ref()
                .unwrap()
                .glider_ride
                .is_none()
        );
        handle(2, GliderAction::Launch, &mut f.connections, &mut f.sim);
        f.sim.world_time = 20.0;
        tick(&mut f.connections, &mut f.sim);
        let before = f.connections[&1].player.as_ref().unwrap().body.position;
        handle(1, GliderAction::Leave, &mut f.connections, &mut f.sim);
        let player = f.connections[&1].player.as_ref().unwrap();
        assert!(player.gliding);
        assert!(player.glider_ride.is_none());
        assert!((player.body.position[1] - before[1]).abs() < 0.01);
        f.sim.world_time = 1000.0;
        tick(&mut f.connections, &mut f.sim);
        assert!(
            f.connections[&2]
                .player
                .as_ref()
                .unwrap()
                .glider_ride
                .is_none()
        );
        let p = f.connections[&2].player.as_ref().unwrap().body.position;
        assert!(horizontal_distance(p, f.sim.gliders.stations[1].landing_position) < 60.0);
        assert!(horizontal_distance(p, f.sim.gliders.stations[1].position) > STATION_REACH);
        for c in f.connections.values_mut() {
            c.dead = true;
        }
        tick(&mut f.connections, &mut f.sim);
        assert!(f.sim.gliders.flights.is_empty());
        // A fresh single passenger can depart without filling the carriage.
        f.add_player(10, origin);
        board(&mut f, 10, destination);
        handle(10, GliderAction::Launch, &mut f.connections, &mut f.sim);
        assert!(f.sim.gliders.flights[0].started_at.is_some());
    }
    #[test]
    fn remote_boarding_observers_and_stale_friends_are_rejected() {
        let mut f = fixture();
        let station = f.sim.gliders.stations[0].clone();
        let target = f.sim.gliders.stations[1].clone();
        f.add_player(
            1,
            [
                station.position[0] + 30.0,
                station.position[1],
                station.position[2],
            ],
        );
        f.add_player(2, target.position);
        board(&mut f, 1, GliderDestination::Player(2));
        assert!(f.sim.gliders.flights.is_empty());
        f.connections
            .get_mut(&1)
            .unwrap()
            .player
            .as_mut()
            .unwrap()
            .body
            .position = station.position;
        board(&mut f, 1, GliderDestination::Player(2));
        assert_eq!(f.sim.gliders.flights.len(), 1);
        f.connections.get_mut(&2).unwrap().dead = true;
        handle(1, GliderAction::Launch, &mut f.connections, &mut f.sim);
        assert!(f.sim.gliders.flights[0].started_at.is_none());
        assert!(f.notice(1).contains("left the server"));
        f.connections.get_mut(&2).unwrap().dead = false;
        f.connections
            .get_mut(&2)
            .unwrap()
            .player
            .as_mut()
            .unwrap()
            .body
            .position[0] += LAUNCH_RANGE * 2.0;
        handle(1, GliderAction::Launch, &mut f.connections, &mut f.sim);
        assert!(f.sim.gliders.flights[0].started_at.is_none());
        f.connections.get_mut(&1).unwrap().mode =
            Some(rubblekin_core::protocol::SessionMode::Observer);
        f.send(
            1,
            ClientMessage::Glider {
                action: GliderAction::Launch,
            },
        );
        assert!(f.notice(1).contains("read-only"));
    }
    #[test]
    fn used_carriages_linger_only_while_visible_and_occupied_never_despawn() {
        let mut f = fixture();
        let s = f.sim.gliders.stations[0].clone();
        let d = GliderDestination::Village(f.sim.gliders.stations[1].village_id);
        f.add_player(1, s.position);
        board(&mut f, 1, d);
        handle(1, GliderAction::Launch, &mut f.connections, &mut f.sim);
        f.sim.world_time = 30.0;
        tick(&mut f.connections, &mut f.sim);
        assert_eq!(f.sim.gliders.flights.len(), 1);
        handle(1, GliderAction::Leave, &mut f.connections, &mut f.sim);
        tick(&mut f.connections, &mut f.sim);
        assert_eq!(f.sim.gliders.flights.len(), 1);
        f.connections
            .get_mut(&1)
            .unwrap()
            .player
            .as_mut()
            .unwrap()
            .body
            .position = s.position;
        tick(&mut f.connections, &mut f.sim);
        assert!(f.sim.gliders.flights.is_empty());
    }
}
