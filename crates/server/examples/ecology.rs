//! Reproducible headless ecology observation; does not open sockets or save worlds.
#![cfg(not(test))]
#[path = "../src/ecology.rs"]
mod ecology;

use ecology::Ecology;
use rubblekin_core::{
    wildlife::{Species, WildlifeAction},
    world::{World, WorldGeneration},
};
use std::{collections::HashMap, time::Instant};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let seed: u32 = args
        .first()
        .map_or(Ok(42), |s| s.parse())
        .expect("seed must be a u32");
    let hours: u32 = args
        .get(1)
        .map_or(Ok(6), |s| s.parse())
        .expect("hours must be an integer");
    let dt: f32 = args
        .get(2)
        .map_or(Ok(0.05), |s| s.parse())
        .expect("step must be a number");
    assert!(
        (1..=72).contains(&hours) && dt.is_finite() && (0.01..=0.25).contains(&dt),
        "use 1..72 hours and 0.01..0.25 seconds per step"
    );
    let world = World::generate(seed, WorldGeneration::GeographyV6);
    let mut ecology = Ecology::new(&world);
    let initial = ecology.animals.len() as u64;
    let start = Instant::now();
    let mut previous = HashMap::<u64, [f32; 3]>::new();
    let mut wolf_births = 0;
    let mut wolf_migrations = 0_u64;
    let mut wolf_arrivals = 0_u64;
    let mut births_by_habitat = vec![[0_u64; 2]; ecology.habitats.len()];
    let steps_per_hour = (3600. / dt).round() as u32;
    for hour in 0..=hours {
        let mut tick_total_ms = 0_f64;
        let mut tick_peak_ms = 0_f64;
        let mut ticks_over_25ms = 0_u32;
        let mut ticks_over_50ms = 0_u32;
        let mut wolf_losses = Vec::new();
        if hour > 0 {
            for step in 0..steps_per_hour {
                let predators_before: Vec<_> = ecology
                    .animals
                    .iter()
                    .filter(|a| a.species == Species::Wolf)
                    .cloned()
                    .collect();
                let tick_start = Instant::now();
                ecology.tick(&world, dt, &[]);
                let ms = tick_start.elapsed().as_secs_f64() * 1000.;
                tick_total_ms += ms;
                tick_peak_ms = tick_peak_ms.max(ms);
                ticks_over_25ms += u32::from(ms > 25.);
                ticks_over_50ms += u32::from(ms > 50.);
                for a in ecology.animals.iter().filter(|a| a.age == 0.) {
                    births_by_habitat[a.habitat as usize]
                        [usize::from(a.species == Species::Wolf)] += 1;
                    wolf_births += usize::from(a.species == Species::Wolf);
                }
                for a in predators_before {
                    if let Some(b) = ecology.animals.iter().find(|b| b.id == a.id) {
                        wolf_migrations +=
                            u64::from(a.destination.is_none() && b.destination.is_some());
                        wolf_arrivals +=
                            u64::from(a.destination == Some(b.habitat) && b.destination.is_none());
                        continue;
                    }
                    let home = ecology.habitats[a.habitat as usize].position;
                    let prey_distance = ecology
                        .animals
                        .iter()
                        .filter(|b| b.species == Species::Rabbit)
                        .map(|b| {
                            (a.body.position[0] - b.body.position[0])
                                .hypot(a.body.position[2] - b.body.position[2])
                        })
                        .min_by(f32::total_cmp);
                    wolf_losses.push(serde_json::json!({
                        "id":a.id,
                        "simulation_hour":(hour-1) as f32 + (step+1) as f32 * dt / 3600.,
                        "cause":if a.hunger >= 100. && a.starving + dt >= 600. {"starvation"} else {"age"},
                        "age_hours":a.age / 3600.,
                        "hunger":a.hunger,
                        "action":a.action,
                        "position":a.body.position,
                        "target":a.target,
                        "habitat":a.habitat,
                        "destination":a.destination,
                        "home_distance":(a.body.position[0]-home[0]).hypot(a.body.position[2]-home[2]),
                        "prey_distance":prey_distance,
                    }));
                }
            }
        }
        assert!(ecology.validate(&world));
        assert_eq!(
            ecology.animals.len() as u64,
            initial + ecology.births - ecology.hunted - ecology.deaths
        );
        assert_eq!(
            births_by_habitat.iter().flatten().sum::<u64>(),
            ecology.births
        );
        assert_eq!(
            births_by_habitat.iter().map(|b| b[1]).sum::<u64>(),
            wolf_births as u64
        );
        assert!(wolf_migrations <= ecology.migrations && wolf_arrivals <= ecology.arrivals);
        let rabbits = ecology
            .animals
            .iter()
            .filter(|a| a.species == Species::Rabbit)
            .count();
        let wolves = ecology.animals.len() - rabbits;
        let predators: Vec<_> = ecology
            .animals
            .iter()
            .filter(|a| a.species == Species::Wolf)
            .collect();
        let nearby = |a: &ecology::Animal, b: &ecology::Animal, radius: f32, height: f32| {
            (a.body.position[0] - b.body.position[0]).hypot(a.body.position[2] - b.body.position[2])
                < radius
                && (a.body.position[1] - b.body.position[1]).abs() < height
        };
        let wolves_with_mate = predators
            .iter()
            .filter(|a| {
                a.age > 1800.
                    && predators.iter().any(|b| {
                        a.id != b.id && b.age > 1800. && b.hunger < 40. && nearby(a, b, 24., 5.)
                    })
            })
            .count();
        let wolves_with_prey = predators
            .iter()
            .filter(|a| {
                ecology
                    .animals
                    .iter()
                    .any(|b| b.species == Species::Rabbit && nearby(a, b, 120., 20.))
            })
            .count();
        let wolf_starving = predators.iter().filter(|a| a.starving > 0.).count();
        let wolf_fed = predators.iter().filter(|a| a.hunger < 30.).count();
        let wolf_max_hunger = predators.iter().map(|a| a.hunger).fold(0., f32::max);
        let wolf_details: Vec<_> = predators
            .iter()
            .map(|a| {
                let home = ecology.habitats[a.habitat as usize].position;
                let prey_distance = ecology
                    .animals
                    .iter()
                    .filter(|b| b.species == Species::Rabbit)
                    .map(|b| {
                        (a.body.position[0] - b.body.position[0])
                            .hypot(a.body.position[2] - b.body.position[2])
                    })
                    .fold(f32::INFINITY, f32::min);
                let mate_distance = predators.iter()
                    .filter(|b| b.id != a.id && b.age > 1800. && b.hunger < 40. && (a.body.position[1]-b.body.position[1]).abs() < 5.)
                    .map(|b| (a.body.position[0]-b.body.position[0]).hypot(a.body.position[2]-b.body.position[2]))
                    .min_by(f32::total_cmp);
                serde_json::json!({"id":a.id,"hunger":a.hunger,"age_hours":a.age / 3600.,"breeding_seconds":a.breeding,"action":a.action,"habitat":a.habitat,"destination":a.destination,"position":a.body.position,"target":a.target,"prey_distance":prey_distance,"mate_distance":mate_distance,"home_distance":(a.body.position[0]-home[0]).hypot(a.body.position[2]-home[2])})
            })
            .collect();
        let migrating = ecology
            .animals
            .iter()
            .filter(|a| a.action == WildlifeAction::Migrating)
            .count();
        let stationary_migrants = ecology
            .animals
            .iter()
            .filter(|a| {
                a.action == WildlifeAction::Migrating
                    && previous.get(&a.id).is_some_and(|p| {
                        (p[0] - a.body.position[0]).hypot(p[2] - a.body.position[2]) < 1.
                    })
            })
            .count();
        let starving = ecology.animals.iter().filter(|a| a.starving > 0.).count();
        let food_min = ecology
            .habitats
            .iter()
            .map(|h| h.forage)
            .fold(100., f32::min);
        let food_mean = ecology.habitats.iter().map(|h| h.forage).sum::<f32>()
            / ecology.habitats.len().max(1) as f32;
        let habitat_details: Vec<_> = ecology.habitat_snapshots().iter().map(|h| {
            let prey_in_range = ecology.animals.iter().filter(|a| {
                a.species == Species::Rabbit
                    && (a.body.position[0]-h.position[0]).hypot(a.body.position[2]-h.position[2]) < 48.
                    && (a.body.position[1]-h.position[1]).abs() < 20.
            }).count();
            let births = births_by_habitat[h.id as usize];
            serde_json::json!({"id":h.id,"forage":h.forage,"rabbits":h.rabbits,"wolves":h.wolves,"prey_in_range":prey_in_range,"rabbit_births":births[0],"wolf_births":births[1]})
        }).collect();
        let packet_bytes =
            serde_json::to_vec(&rubblekin_core::protocol::ServerMessage::WildlifeState {
                animals: ecology.snapshots(),
                habitats: ecology.habitat_snapshots(),
            })
            .unwrap()
            .len();
        println!(
            "{}",
            serde_json::json!({"seed":seed,"hour":hour,"step_seconds":dt,"rabbits":rabbits,"wolves":wolves,"births":ecology.births,"wolf_births":wolf_births,"wolf_migrations":wolf_migrations,"wolf_arrivals":wolf_arrivals,"wolf_losses":wolf_losses,"wolf_fed":wolf_fed,"wolf_starving":wolf_starving,"wolf_max_hunger":wolf_max_hunger,"wolves_with_mate":wolves_with_mate,"wolves_with_prey":wolves_with_prey,"wolf_details":wolf_details,"hunted":ecology.hunted,"deaths":ecology.deaths,"migrations":ecology.migrations,"arrivals":ecology.arrivals,"migrating":migrating,"stationary_migrants":stationary_migrants,"starving":starving,"forage_min":food_min,"forage_mean":food_mean,"habitat_details":habitat_details,"packet_bytes":packet_bytes,"ecology_mean_ms":tick_total_ms / steps_per_hour as f64,"ecology_peak_ms":tick_peak_ms,"ticks_over_25ms":ticks_over_25ms,"ticks_over_50ms":ticks_over_50ms,"elapsed_seconds":start.elapsed().as_secs_f32()})
        );
        previous = ecology
            .animals
            .iter()
            .map(|a| (a.id, a.body.position))
            .collect();
    }
}
