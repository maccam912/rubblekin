//! Read-only station and launch-plan inspection for native staging.
use rubblekin_core::{
    gliders::{GliderDestination, GliderFlight, reachable, stations},
    world::{World, WorldGeneration},
};
use serde_json::json;
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let seed = args.first().map_or(42, |s| s.parse().expect("seed"));
    let generation = if args.get(1).is_some_and(|s| s == "v3") {
        WorldGeneration::GeographyV3
    } else {
        WorldGeneration::GeographyV6
    };
    let world = World::generate(seed, generation);
    let stops = stations(&world);
    let result: Vec<_> = stops
        .iter()
        .map(|station| {
            let destinations: Vec<_> = stops
                .iter()
                .filter(|other| {
                    other.village_id != station.village_id
                        && reachable(station.position, other.landing_position)
                })
                .map(|other| {
                    match GliderFlight::plan(
                        &world,
                        station,
                        GliderDestination::Village(other.village_id),
                        other.name.clone(),
                        other.landing_position,
                        1,
                        0.0,
                    ) {
                        Ok(flight) => json!({"id":other.village_id,"name":other.name,"flight":flight}),
                        Err(error) => json!({"id":other.village_id,"error":error}),
                    }
                })
                .collect();
            json!({"id":station.village_id,"name":station.name,"position":station.position,"landing_position":station.landing_position,"destinations":destinations})
        })
        .collect();
    println!(
        "{}",
        json!({"seed":seed,"generation":generation,"stations":result})
    );
}
