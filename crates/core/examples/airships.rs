//! Inspect the generated fleet without a client or save changes.
//! Feed simulation times (seconds) on stdin after the initial `ready` JSON line.
//! Example: printf '0\n180\n' | cargo run -p rubblekin_core --example airships -- 42
use std::io::{self, BufRead, Write};

use rubblekin_core::{
    airships::{AirshipNetwork, deck_position},
    world::{World, WorldGeneration},
};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() > 1 || args.first().is_some_and(|arg| arg == "--help") {
        println!(
            "airships [SEED]\nReads simulation times in seconds from stdin; prints one JSON line per time.\nUses unedited GeographyV3. Does not connect, move players, or modify saves."
        );
        return Ok(());
    }
    let seed = args.first().map_or(Ok(42), |arg| arg.parse::<u32>())?;
    let world = World::generate(seed, WorldGeneration::GeographyV3);
    let network = AirshipNetwork::try_new(&world)?;
    let villages = &world
        .settlements()
        .ok_or("No villages in generated world")?
        .villages;
    let mut output = io::stdout().lock();
    writeln!(
        output,
        "{}",
        json!({
            "ready": true,
            "seed": seed,
            "generation": "GeographyV3",
            "villages": villages.iter().map(|v| json!({
                "id": v.id, "name": v.name, "position": v.center,
                "port_position": network.port(v.id).map(|port| port.position),
            })).collect::<Vec<_>>(),
            "routes": network.routes().iter().map(|route| json!({
                "id": route.id, "from": route.from, "to": route.to,
                "travel_seconds": route.travel_seconds, "ships": route.ship_count,
            })).collect::<Vec<_>>(),
        })
    )?;
    output.flush()?;
    for line in io::stdin().lock().lines() {
        let line = line?;
        let time = match line.trim().parse::<f64>() {
            Ok(time) if time.is_finite() && time >= 0. => time,
            _ => {
                writeln!(
                    output,
                    "{}",
                    json!({"error": "Time must be finite and nonnegative"})
                )?;
                output.flush()?;
                continue;
            }
        };
        let ships: Vec<_> = network
            .ships(time)
            .iter()
            .map(|ship| {
                json!({
                    "id": ship.id,
                    "route": ship.route_id,
                    "from": ship.from_village,
                    "next": ship.next_village,
                    "docked_at": ship.docked_at,
                    "departure_in": ship.departure_in,
                    "arrival_in": ship.arrival_in,
                    "position": ship.position,
                    "yaw_radians": ship.yaw,
                    "deck_check_position": deck_position(ship, [-2.5, 0.1, 0.]),
                })
            })
            .collect();
        writeln!(output, "{}", json!({"time": time, "ships": ships}))?;
        output.flush()?;
    }
    Ok(())
}
