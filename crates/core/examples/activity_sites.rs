//! Coordinates for the actual checked activity placements; no terrain changes.
//! cargo run -p rubblekin_core --example activity_sites -- 42
use rubblekin_core::{
    activities::plans,
    world::{World, WorldGeneration},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let seed = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "42".into())
        .parse()?;
    let world = World::generate(seed, WorldGeneration::GeographyV6);
    let plans = plans(&world);
    let sites = &world.settlements().unwrap().composed_sites;
    let rows: Vec<_> = plans.into_iter().map(|plan| {
        let site = sites.iter().find(|s| Some(s.id) == plan.site_id);
        serde_json::json!({"site": site.map(|s| s.arrangement.name()), "entrance": site.map(|s| s.entrance()), "route": site.map(|s| &s.route), "plan": plan})
    }).collect();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"seed": seed, "scenery_count": sites.len(), "activities": rows})
        )?
    );
    Ok(())
}
