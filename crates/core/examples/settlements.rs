//! Export the generated settlement/resource plan for review without running a client.
//! cargo run -p rubblekin_core --example settlements -- 42 /tmp/villages.json
use rubblekin_core::world::{World, WorldGeneration};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let seed = args.get(1).map_or(Ok(42), |s| s.parse())?;
    let output = args.get(2).map_or("villages.json", String::as_str);
    let start = std::time::Instant::now();
    let world = World::generate(seed, WorldGeneration::GeographyV3);
    let plan = world.settlements().ok_or("missing settlement plan")?;
    let villages: Vec<_> = plan
        .villages
        .iter()
        .map(|v| {
            json!({
                "id":v.id, "name":v.name, "kind":v.kind.name(), "center":v.center,
                "freshwater_distance":v.freshwater_distance, "scores":v.resources,
                "buildings":v.buildings.len(), "fields":v.fields.len(),
                "residents":v.resident_routes.len(),
            })
        })
        .collect();
    let trails: Vec<_> = plan
        .trails
        .iter()
        .map(|t| json!({"from":t.from,"to":t.to,"points":t.points,"width":t.width}))
        .collect();
    let resources: Vec<_> = plan.resources.iter().map(|r| json!({"kind":r.kind.name(),"center":r.center,"radius":r.radius,"richness":r.richness})).collect();
    std::fs::write(
        output,
        serde_json::to_vec_pretty(&json!({
            "seed":seed, "world_size_m":32768, "spawn":world.spawn_position(),
            "villages":villages, "trails":trails, "resources":resources,
        }))?,
    )?;
    println!(
        "Seed {seed}: {} villages, {} trails, {} deposits in {:.2?}; {output}",
        plan.villages.len(),
        plan.trails.len(),
        plan.resources.len(),
        start.elapsed()
    );
    for village in &plan.villages {
        println!(
            "{}: {} at [{:.0}, {:.0}], water {:.0}m, food {:.2}, timber {:.2}, stone {:.2}, clay {:.2}, iron {:.2}",
            village.name,
            village.kind.name(),
            village.center[0],
            village.center[2],
            village.freshwater_distance,
            village.resources.farming,
            village.resources.timber,
            village.resources.stone,
            village.resources.clay,
            village.resources.iron
        );
    }
    Ok(())
}
