//! Shared transient carriage allocation and routing for players and traders.
use rubblekin_core::gliders::*;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct GliderService {
    pub stations: Vec<WhipStation>,
    pub flights: Vec<GliderFlight>,
    pub(crate) next_id: u64,
    pub approaches: BTreeMap<u32, Vec<[f32; 3]>>,
    pub(crate) personal: BTreeMap<u64, (WhipStation, u64)>,
}
impl GliderService {
    pub fn new(world: &rubblekin_core::world::World) -> Self {
        let stops = stations(world);
        Self {
            approaches: stops
                .iter()
                .map(|s| (s.village_id, station_approach(world, s.village_id)))
                .collect(),
            stations: stops,
            next_id: 1,
            ..Self::default()
        }
    }
    /// Reuse an occupied trip only for its chosen destination. An empty trip
    /// can be replaced atomically after the new route has passed validation.
    #[allow(clippy::too_many_arguments)]
    pub fn waiting_flight(
        &mut self,
        world: &rubblekin_core::world::World,
        station_id: u32,
        destination: GliderDestination,
        name: String,
        target: [f32; 3],
        now: f64,
        occupied: &[GliderRide],
    ) -> Result<usize, &'static str> {
        let existing = self
            .flights
            .iter()
            .position(|f| f.station_id == station_id && f.started_at.is_none());
        if let Some(index) = existing {
            let flight = &self.flights[index];
            if occupied.iter().any(|r| r.carriage_id == flight.id) {
                return if flight.destination == destination {
                    Ok(index)
                } else {
                    Err(
                        "This occupied carriage has another destination. Join its trip or wait for departure.",
                    )
                };
            }
        } else if self.flights.len() >= 32 {
            return Err("All carriages are in use. Try again shortly.");
        }
        let station = self
            .stations
            .iter()
            .find(|s| s.village_id == station_id)
            .ok_or("That whip station is unavailable.")?;
        let flight =
            GliderFlight::plan(world, station, destination, name, target, self.next_id, now)?;
        self.next_id += 1;
        if let Some(index) = existing {
            self.flights[index] = flight;
            Ok(index)
        } else {
            self.flights.push(flight);
            Ok(self.flights.len() - 1)
        }
    }

    pub fn next_stop(&self, origin: u32, destination: u32) -> Option<u32> {
        let mut queue = std::collections::VecDeque::from([(origin, None)]);
        let mut visited = std::collections::BTreeSet::from([origin]);
        while let Some((id, first)) = queue.pop_front() {
            let source = self.stations.iter().find(|s| s.village_id == id)?;
            for target in &self.stations {
                if visited.contains(&target.village_id)
                    || !reachable(source.position, target.landing_position)
                {
                    continue;
                }
                let first = first.or(Some(target.village_id));
                if target.village_id == destination {
                    return first;
                }
                visited.insert(target.village_id);
                queue.push_back((target.village_id, first));
            }
        }
        None
    }

    pub fn launch_traders(
        &mut self,
        world: &rubblekin_core::world::World,
        now: f64,
        traders: &[GliderRide],
        players: &[GliderRide],
    ) {
        for flight in &mut self.flights {
            if flight.started_at.is_some()
                || now - flight.created_at < 3.0
                || !traders.iter().any(|r| r.carriage_id == flight.id)
                || players.iter().any(|r| r.carriage_id == flight.id)
            {
                continue;
            }
            let GliderDestination::Village(destination) = flight.destination else {
                continue;
            };
            let Some(station) = self
                .stations
                .iter()
                .find(|s| s.village_id == flight.station_id)
            else {
                continue;
            };
            let Some(target) = self.stations.iter().find(|s| s.village_id == destination) else {
                continue;
            };
            if let Ok(mut planned) = GliderFlight::plan(
                world,
                station,
                flight.destination,
                flight.destination_name.clone(),
                target.landing_position,
                flight.id,
                flight.created_at,
            ) {
                planned.started_at = Some(now);
                *flight = planned;
            }
        }
    }
}
