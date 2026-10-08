//! Read-only route pacing report and reproducible native staging coordinates.
//! cargo run -p rubblekin_core --example exploration -- 42 /tmp/exploration.json
use rubblekin_core::world::{World, WorldGeneration};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let seed = args.get(1).map_or(Ok(42), |s| s.parse())?;
    let output = args.get(2).map_or("/tmp/exploration.json", String::as_str);
    let generation = if args.get(3).is_some_and(|s| s == "v5") {
        WorldGeneration::GeographyV5
    } else {
        WorldGeneration::GeographyV6
    };
    let start = std::time::Instant::now();
    let world = World::generate(seed, generation);
    let plan = world.settlements().unwrap();
    let sites: Vec<_> = plan.roadside_landmarks.iter().map(|s| json!({
        "kind": s.building.kind, "origin": s.building.origin, "rotation": s.building.rotation,
        "entrance": s.building.entrance(), "trail_anchor": s.approach.points[0],
        "approach": s.approach.points,
    })).collect();
    let mut all_gaps = Vec::new();
    let trails: Vec<_> = plan
        .trails
        .iter()
        .map(|trail| {
            let mut distances = vec![0.0_f32];
            for pair in trail.points.windows(2) {
                distances.push(
                    distances.last().unwrap()
                        + (pair[1][0] - pair[0][0]).hypot(pair[1][2] - pair[0][2]),
                );
            }
            let length = *distances.last().unwrap();
            let mut stops = vec![0.0, length];
            for site in &plan.roadside_landmarks {
                if let Some(index) = trail
                    .points
                    .iter()
                    .position(|p| *p == site.approach.points[0])
                {
                    stops.push(distances[index]);
                }
            }
            stops.sort_by(f32::total_cmp);
            stops.dedup();
            let gaps: Vec<_> = stops.windows(2).map(|p| p[1] - p[0]).collect();
            all_gaps.extend_from_slice(&gaps);
            json!({"from": trail.from, "to": trail.to, "length_m": length,
            "stops_m": stops, "gaps_m": gaps, "points": trail.points})
        })
        .collect();
    all_gaps.sort_by(f32::total_cmp);
    let percentile =
        |fraction: f32| all_gaps[((all_gaps.len() - 1) as f32 * fraction).round() as usize];
    let within_two_minutes = all_gaps.iter().filter(|&&d| d <= 456.0).count();
    std::fs::write(
        output,
        serde_json::to_vec_pretty(&json!({"seed": seed,
            "generation": generation, "sites": sites, "trails": trails,
            "walking_speed_mps": 3.8, "median_gap_m": percentile(0.5),
            "p90_gap_m": percentile(0.9), "max_gap_m": all_gaps.last(),
            "gaps_at_most_two_minutes": within_two_minutes, "total_gaps": all_gaps.len(),
        }))?,
    )?;
    println!(
        "Seed {seed}: {} sites, {} routes in {:.2?}",
        sites.len(),
        trails.len(),
        start.elapsed()
    );
    println!(
        "Nominal walking gaps: median {:.0}s, p90 {:.0}s, max {:.0}s; {within_two_minutes}/{} within 2 min",
        percentile(0.5) / 3.8,
        percentile(0.9) / 3.8,
        all_gaps.last().unwrap() / 3.8,
        all_gaps.len()
    );
    if args.iter().any(|s| s == "--diagnose") {
        for trail in &trails {
            let gaps = trail["gaps_m"].as_array().unwrap();
            let stops = trail["stops_m"].as_array().unwrap();
            let points = trail["points"].as_array().unwrap();
            for (i, gap) in gaps
                .iter()
                .enumerate()
                .filter(|(_, g)| g.as_f64().unwrap() > 750.0)
            {
                let middle = stops[i].as_f64().unwrap() + gap.as_f64().unwrap() * 0.5;
                let mut distance = 0.0;
                let mut index = 1;
                while index < points.len() - 1 && distance < middle {
                    distance += (points[index][0].as_f64().unwrap()
                        - points[index - 1][0].as_f64().unwrap())
                    .hypot(
                        points[index][2].as_f64().unwrap() - points[index - 1][2].as_f64().unwrap(),
                    );
                    index += 1;
                }
                let anchor: [f32; 3] = serde_json::from_value(points[index].clone())?;
                let previous: [f32; 3] = serde_json::from_value(points[index - 1].clone())?;
                let dx = anchor[0] - previous[0];
                let dz = anchor[2] - previous[2];
                let run = dx.hypot(dz).max(0.01);
                println!(
                    "Gap {}->{} {:.0}m at {anchor:?}",
                    trail["from"],
                    trail["to"],
                    gap.as_f64().unwrap()
                );
                for side in [-30.0, -16.0, -8.0, 0.0, 8.0, 16.0, 30.0] {
                    let sample = world
                        .geography()
                        .unwrap()
                        .sample(anchor[0] - dz / run * side, anchor[2] + dx / run * side);
                    println!(
                        "  side {side}: ground delta {:.1}m, water {:?}, {:?}",
                        sample.height + 0.5 - anchor[1],
                        sample.water,
                        sample.biome
                    );
                }
            }
        }
    }
    for site in plan
        .roadside_landmarks
        .iter()
        .filter(|s| s.building.kind.is_exploration_site())
    {
        println!(
            "{:?}: {:?}, rotation {}",
            site.building.kind,
            site.building.entrance(),
            site.building.rotation
        );
    }
    Ok(())
}
