//! Repeatable downhill handling probe on generated mountain terrain.
//! Prints candidate native playtest starts; it does not measure subjective fun.
use rubblekin_core::{
    physics::{Body, MoveInput},
    vehicles::{self, VehicleKind},
    world::{World, WorldGeneration},
};
fn main() {
    let seed = std::env::args()
        .nth(1)
        .map_or(2_689_504_302, |s| s.parse().expect("seed"));
    let world = World::generate(seed, WorldGeneration::GeographyV6);
    let geo = world.geography().unwrap();
    let mut runs = 0;
    for i in (0..geo.drainage().len()).step_by(7) {
        let [x, z] = geo.grid_position(i).map(|v| (v * 2.).floor() * 0.5 + 0.25);
        let y = world.surface_height(x, z);
        if !(500.0..1800.0).contains(&y) {
            continue;
        }
        let slope = [
            world.surface_height(x - 4., z) - world.surface_height(x + 4., z),
            world.surface_height(x, z - 4.) - world.surface_height(x, z + 4.),
        ];
        let length = slope[0].hypot(slope[1]);
        if !(2.0..8.0).contains(&length) {
            continue;
        }
        let direction = slope.map(|s| s / length);
        let mut body = Body::new([x, y + 0.5, z]);
        body.on_ground = true;
        let heading = direction[0].atan2(-direction[1]);
        let Ok(craft) = vehicles::spawn(&world, &mut body, VehicleKind::Bike, heading, &[]) else {
            continue;
        };
        let mut craft = Some(craft);
        let start = body.position;
        let mut peak = 0.0_f32;
        let mut air = 0.0_f32;
        for _ in 0..240 {
            vehicles::advance(
                &world,
                &mut body,
                MoveInput::default(),
                0.05,
                &[],
                0.0,
                &mut craft,
            );
            peak = peak.max(body.velocity[0].hypot(body.velocity[2]));
            if !body.on_ground {
                air += 0.05;
            }
        }
        println!(
            "start={start:?} heading={heading:.4} travel={:.1}m drop={:.1}m peak={peak:.1}m/s airborne={air:.1}s mounted={}",
            (body.position[0] - x).hypot(body.position[2] - z),
            start[1] - body.position[1],
            craft.is_some()
        );
        if body.velocity[0].hypot(body.velocity[2]) < 1.0 {
            println!(
                "  stop={:?} vel={:?} grounded={} tilt={:?}",
                body.position,
                body.velocity,
                body.on_ground,
                craft.map(|c| c.tilt)
            );
        }
        runs += 1;
        if runs == 12 {
            break;
        }
    }
}
