//! Export all 64 states of the authored channel prototype for visual review.
//! cargo run -p rubblekin_core --example flow_garden_catalog
use rubblekin_core::activities::flow_garden::{
    BED, CENTERS, SOURCE, START_FACES, Spill, flow, ports,
};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut states = Vec::new();
    for a in 0..4 {
        for b in 0..4 {
            for c in 0..4 {
                let faces = [a, b, c];
                let water = flow(faces).unwrap();
                let ends: Vec<_> = (0..3)
                    .map(|i| ports(i, faces[i]).unwrap().map(|d| d.offset()))
                    .collect();
                let spill = match water.spill {
                    None => json!(null),
                    Some(Spill::Source) => json!({"source": true}),
                    Some(Spill::Channel { piece, edge }) => {
                        json!({"piece": piece, "edge": edge.offset()})
                    }
                };
                states.push(json!({
                    "faces": faces, "ports": ends, "wet": water.wet,
                    "garden": water.garden_watered, "spill": spill,
                }));
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string(&json!({
            "centers": CENTERS, "source": SOURCE, "bed": BED,
            "start": START_FACES, "states": states,
        }))?
    );
    Ok(())
}
