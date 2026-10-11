//! Investigate paddled river stops against generated terrain, without a renderer.
//! Straight headings also reach dry banks at bends; not every stop is a defect.
use rubblekin_core::{
    environment,
    physics::{Body, MoveInput},
    vehicles::{self, VehicleKind},
    world::{World, WorldGeneration},
};
fn main() {
    let seed = std::env::args()
        .nth(1)
        .map_or(42, |s| s.parse().expect("seed"));
    let world = World::generate(seed, WorldGeneration::GeographyV6);
    let geo = world.geography().unwrap();
    let mut checked = 0;
    let mut snags = 0;
    for (i, next) in geo.drainage().iter().enumerate() {
        let Some(next) = *next else { continue };
        if geo.flow_accumulation()[i] < 400. {
            continue;
        }
        let a = geo.grid_position(i);
        let b = geo.grid_position(next);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        for sign in [1., -1.] {
            let direction = [(b[0] - a[0]) / len * sign, (b[1] - a[1]) / len * sign];
            let x = a[0] + (b[0] - a[0]) * 0.5;
            let z = a[1] + (b[1] - a[1]) * 0.5;
            let Some(y) = environment::water_surface(&world, x, z) else {
                continue;
            };
            if y < 40. {
                continue;
            }
            let mut body = Body::new([x, y + 0.02, z]);
            let Ok(craft) = vehicles::spawn(
                &world,
                &mut body,
                VehicleKind::Kayak,
                direction[0].atan2(-direction[1]),
                &[],
            ) else {
                continue;
            };
            let mut craft = Some(craft);
            body.velocity = [direction[0] * 5., 0., direction[1] * 5.];
            let start = body.position;
            let mut quiet = 0;
            for tick in 0..800 {
                let before = body.position;
                vehicles::advance(
                    &world,
                    &mut body,
                    MoveInput {
                        direction,
                        sprint: true,
                        ..Default::default()
                    },
                    0.05,
                    &[],
                    0.,
                    &mut craft,
                );
                if craft.is_none() {
                    break;
                }
                let delta = (before[0] - body.position[0]).hypot(before[2] - body.position[2]);
                quiet = if delta < 0.0001 { quiet + 1 } else { 0 };
                if quiet >= 30 {
                    let p = body.position;
                    let next = [p[0] + direction[0] * 0.1, p[1], p[2] + direction[1] * 0.1];
                    let clear: Vec<_> = [0., 0.05, 0.1, 0.3, 0.5, 1., 2.]
                        .into_iter()
                        .map(|lift| {
                            let mut q = next;
                            q[1] += lift;
                            (
                                lift,
                                vehicles::position_is_clear(&world, q, craft.unwrap(), &[]),
                            )
                        })
                        .collect();
                    println!(
                        "seed={seed} segment={i} sign={sign} tick={tick} from={start:?} stuck={p:?} direction={direction:?} velocity={:?} water={:?} next_water={:?} lifts={clear:?}",
                        body.velocity,
                        environment::water_surface(&world, p[0], p[2]),
                        environment::water_surface(&world, next[0], next[2])
                    );
                    let v = craft.unwrap();
                    let f = [v.heading.sin(), -v.heading.cos()];
                    let slope = vehicles::pitch(&world, next, v).tan();
                    let mut blockers = 0;
                    for xi in 0..=4 {
                        for zi in 0..=13 {
                            let side = -0.42 + 0.84 * xi as f32 / 4.;
                            let along = -1.5 + 3. * zi as f32 / 13.;
                            let q = [
                                next[0] + f[1] * side + f[0] * along,
                                next[1] + along * slope,
                                next[2] - f[0] * side + f[1] * along,
                            ];
                            let water = environment::water_surface(&world, q[0], q[2]);
                            let geo = world.geography().unwrap().sample(q[0], q[2]);
                            if water.is_none() && blockers < 6 {
                                println!(
                                    " dry_hull={q:?} generated_water={:?} bed={} surface={}",
                                    geo.water,
                                    geo.height,
                                    world.surface_height(q[0], q[2])
                                );
                                blockers += 1;
                            }
                            for h in [-v.kind.dimensions()[1], 0., 0.25] {
                                let cell = q
                                    .map(|v| (v / rubblekin_core::world::CELL_SIZE).floor() as i32);
                                let cell = rubblekin_core::world::BlockPos::new(
                                    cell[0],
                                    ((q[1] + h) / rubblekin_core::world::CELL_SIZE).floor() as i32,
                                    cell[2],
                                );
                                let block = world.block(cell);
                                if block.is_solid() && blockers < 6 {
                                    println!(
                                        " blocker={cell:?} block={block:?} hull={q:?} h={h} water={water:?} bed={} surface={}",
                                        geo.height,
                                        world.surface_height(q[0], q[2])
                                    );
                                    blockers += 1;
                                }
                            }
                        }
                    }
                    snags += 1;
                    break;
                }
            }
            checked += 1;
            if snags >= 4 || checked >= 30 {
                println!("checked={checked} snags={snags}");
                return;
            }
        }
    }
    println!("checked={checked} snags={snags}");
}
