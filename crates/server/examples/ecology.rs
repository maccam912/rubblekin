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
    let steps_per_hour = (3600. / dt).round() as u32;
    for hour in 0..=hours {
        if hour > 0 {
            for _ in 0..steps_per_hour {
                ecology.tick(&world, dt, &[]);
                wolf_births += ecology
                    .animals
                    .iter()
                    .filter(|a| a.species == Species::Wolf && a.age == 0.)
                    .count();
            }
        }
        assert!(ecology.validate(&world));
        assert_eq!(
            ecology.animals.len() as u64,
            initial + ecology.births - ecology.hunted - ecology.deaths
        );
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
                predators
                    .iter()
                    .any(|b| a.id != b.id && b.hunger < 40. && nearby(a, b, 24., 5.))
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
                let prey_distance = ecology
                    .animals
                    .iter()
                    .filter(|b| b.species == Species::Rabbit)
                    .map(|b| {
                        (a.body.position[0] - b.body.position[0])
                            .hypot(a.body.position[2] - b.body.position[2])
                    })
                    .fold(f32::INFINITY, f32::min);
                serde_json::json!({"id":a.id,"hunger":a.hunger,"action":a.action,"habitat":a.habitat,"position":a.body.position,"target":a.target,"prey_distance":prey_distance})
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
        let packet_bytes =
            serde_json::to_vec(&rubblekin_core::protocol::ServerMessage::WildlifeState {
                animals: ecology.snapshots(),
                habitats: ecology.habitat_snapshots(),
            })
            .unwrap()
            .len();
        println!(
            "{}",
            serde_json::json!({"seed":seed,"hour":hour,"step_seconds":dt,"rabbits":rabbits,"wolves":wolves,"births":ecology.births,"wolf_births":wolf_births,"wolf_fed":wolf_fed,"wolf_starving":wolf_starving,"wolf_max_hunger":wolf_max_hunger,"wolves_with_mate":wolves_with_mate,"wolves_with_prey":wolves_with_prey,"wolf_details":wolf_details,"hunted":ecology.hunted,"deaths":ecology.deaths,"migrations":ecology.migrations,"arrivals":ecology.arrivals,"migrating":migrating,"stationary_migrants":stationary_migrants,"starving":starving,"forage_min":food_min,"forage_mean":food_mean,"packet_bytes":packet_bytes,"elapsed_seconds":start.elapsed().as_secs_f32()})
        );
        previous = ecology
            .animals
            .iter()
            .map(|a| (a.id, a.body.position))
            .collect();
    }
}
