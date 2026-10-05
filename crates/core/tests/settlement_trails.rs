use rubblekin_core::{
    physics::{Body, MoveInput, move_character},
    world::{World, WorldGeneration},
};

/// Traders use the same controller and arrival thresholds. Sampled road points
/// being clear alone does not prove that steps and crossings can be traversed.
#[test]
fn every_generated_trail_is_physically_walkable_in_both_directions() {
    let world = World::generate(42, WorldGeneration::GeographyV3);
    let plan = world.settlements().unwrap();
    assert!(!plan.trails.is_empty());
    let mut failures = Vec::new();
    for (trail_index, trail) in plan.trails.iter().enumerate() {
        for reverse in [false, true] {
            let points: Vec<_> = if reverse {
                trail.points.iter().rev().copied().collect()
            } else {
                trail.points.clone()
            };
            let mut body = Body::new(points[0]);
            for (waypoint, destination) in points.iter().enumerate().skip(1) {
                let mut reached = false;
                for _ in 0..100 {
                    let dx = destination[0] - body.position[0];
                    let dz = destination[2] - body.position[2];
                    let distance = dx.hypot(dz);
                    if distance <= 0.45 && (body.position[1] - destination[1]).abs() < 0.8 {
                        reached = true;
                        break;
                    }
                    let direction = if distance > 0.03 {
                        let factor = 0.52_f32.min(distance / (3.8 * 0.05));
                        [dx / distance * factor, dz / distance * factor]
                    } else {
                        [0.0; 2]
                    };
                    move_character(
                        &world,
                        &mut body,
                        MoveInput {
                            direction,
                            ..Default::default()
                        },
                        0.05,
                    );
                }
                if !reached {
                    failures.push(format!(
                        "trail {trail_index} {} -> {}, reverse {reverse}, waypoint {waypoint}/{}: target {destination:?}, stopped at {:?}",
                        trail.from,
                        trail.to,
                        points.len(),
                        body.position,
                    ));
                    break;
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
