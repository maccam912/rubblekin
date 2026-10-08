//! Reproduce live inhabitant work without sockets, sleeps or durable saves.
#![cfg(not(test))]
#[allow(dead_code)]
#[path = "../src/airships.rs"]
mod airships;
#[path = "../src/navigation.rs"]
mod navigation;
#[allow(dead_code)]
#[path = "../src/npc.rs"]
mod npc;
#[allow(dead_code)]
#[path = "../src/villages.rs"]
mod villages;

use rubblekin_core::{
    airships::AirshipNetwork,
    world::{World, WorldGeneration},
};
use std::time::Instant;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let seed: u32 = args.first().map_or(Ok(42), |s| s.parse()).unwrap();
    let seconds: u32 = args.get(1).map_or(Ok(300), |s| s.parse()).unwrap();
    assert!(
        (10..=3600).contains(&seconds),
        "use10..3600 simulated seconds"
    );
    let world = World::generate(seed, WorldGeneration::GeographyV6);
    let network = AirshipNetwork::try_new(&world).unwrap();
    let mut life = villages::VillageLife::new(&world);
    let mut npc = npc::Forager::new(&world);
    life.sync_airship_riders(&network, 0.);
    life.resolve_overlaps(&world, &[npc.snapshot.position]);
    npc.resolve_overlaps(&world, &life.positions().collect::<Vec<_>>());
    let start = Instant::now();
    let mut time = 0_f64;
    let mut costs = Vec::with_capacity(200);
    let mut npc_ms = 0_f64;
    let mut village_ms = 0_f64;
    for tick in 1..=seconds * 20 {
        time += 0.05_f32 as f64;
        life.sync_airship_riders(&network, time);
        let positions: Vec<_> = life.positions().collect();
        let before = Instant::now();
        npc.tick_with_obstacles(&world, 0.05, &positions);
        let middle = Instant::now();
        life.tick_with_transport(&world, 0.05, &[npc.snapshot.position], &network, time, &[]);
        let after = Instant::now();
        npc_ms += middle.duration_since(before).as_secs_f64() * 1000.;
        village_ms += after.duration_since(middle).as_secs_f64() * 1000.;
        costs.push(after.duration_since(before).as_secs_f64() * 1000.);
        if tick % 200 == 0 || tick == seconds * 20 {
            costs.sort_by(f64::total_cmp);
            let count = costs.len();
            // Deterministic gameplay fingerprint for comparing optimizations;
            // transient navigation caches and timing values are not serialized.
            let state = serde_json::to_vec(&(&npc, &life)).unwrap();
            let fingerprint = state.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
            println!(
                "{}",
                serde_json::json!({
                    "seed":seed, "simulation_seconds":tick as f64 / 20.,
                    "wall_seconds":start.elapsed().as_secs_f64(),
                    "mean_ms":costs.iter().sum::<f64>() / count as f64,
                    "p95_ms":costs[count * 95 / 100], "peak_ms":costs[count-1],
                    "over_50ms":costs.iter().filter(|ms| **ms > 50.).count(),
                    "npc_mean_ms":npc_ms / count as f64,
                    "village_mean_ms":village_ms / count as f64,
                    "residents":life.residents().len(),
                    "state_fingerprint":format!("{fingerprint:016x}"),
                })
            );
            costs.clear();
            npc_ms = 0.;
            village_ms = 0.;
        }
    }
}
