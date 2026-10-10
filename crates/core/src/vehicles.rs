//! One personal, transient vehicle per rider. Poses and propulsion are shared
//! by authoritative movement, prediction and replay; no client pose requests.
use crate::{
    environment,
    physics::{Body, MoveInput, character_position_is_clear},
    world::{BlockPos, CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VehicleKind {
    Bike,
    Kayak,
    Sailboat,
}
impl VehicleKind {
    pub const ALL: [Self; 3] = [Self::Bike, Self::Kayak, Self::Sailboat];
    pub fn name(self) -> &'static str {
        match self {
            Self::Bike => "Mountain bike",
            Self::Kayak => "Kayak",
            Self::Sailboat => "Sailboat",
        }
    }
    pub fn watercraft(self) -> bool {
        self != Self::Bike
    }
    pub fn dimensions(self) -> [f32; 3] {
        match self {
            Self::Bike => [0.32, 0.85, 0.9],
            Self::Kayak => [0.42, 0.18, 1.5],
            Self::Sailboat => [0.85, 0.35, 2.0],
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vehicle {
    pub kind: VehicleKind,
    pub heading: f32,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum VehicleAction {
    Spawn(VehicleKind),
    Dismount,
}

fn forward(heading: f32) -> [f32; 2] {
    [libm::sinf(heading), -libm::cosf(heading)]
}
fn norm(v: [f32; 2]) -> f32 {
    libm::hypotf(v[0], v[1])
}
fn solid(world: &World, p: [f32; 3]) -> bool {
    let p = p.map(|v| (v / CELL_SIZE).floor() as i32);
    world.block(BlockPos::new(p[0], p[1], p[2])).is_solid()
}

/// Check the hull footprint and draft, not just the rider's centre. Samples
/// are closer than one voxel so thin edited walls and banks cannot be skipped.
pub fn position_is_clear(
    world: &World,
    p: [f32; 3],
    vehicle: Vehicle,
    obstacles: &[[f32; 3]],
) -> bool {
    if !vehicle.heading.is_finite() || !character_position_is_clear(world, p, obstacles) {
        return false;
    }
    let [width, draft, length] = vehicle.kind.dimensions();
    let f = forward(vehicle.heading);
    let nx = (width * 2.0 / 0.24).ceil() as usize;
    let nz = (length * 2.0 / 0.24).ceil() as usize;
    for x in 0..=nx {
        for z in 0..=nz {
            let side = -width + 2.0 * width * x as f32 / nx as f32;
            let along = -length + 2.0 * length * z as f32 / nz as f32;
            let q = [
                p[0] + f[1] * side + f[0] * along,
                p[1],
                p[2] - f[0] * side + f[1] * along,
            ];
            if vehicle.kind.watercraft() {
                let Some(level) = environment::water_surface(world, q[0], q[2]) else {
                    return false;
                };
                // Hulls can bridge a rapid or a drop into a lake; buoyancy
                // settles their centre continuously instead of snapping down.
                if level - p[1] > 2.0 {
                    return false;
                }
                for h in [-draft, 0.0, 0.25] {
                    if solid(world, [q[0], p[1] + h, q[2]]) {
                        return false;
                    }
                }
                if vehicle.kind == VehicleKind::Sailboat {
                    for h in [0.75, 1.25, 1.75, 2.25, 2.75, 3.25] {
                        if solid(world, [q[0], p[1] + h, q[2]]) {
                            return false;
                        }
                    }
                }
            } else if solid(world, [q[0], p[1] + 0.55, q[2]])
                || solid(world, [q[0], p[1] + 0.85, q[2]])
            {
                return false;
            }
            if obstacles.iter().any(|o| {
                (q[0] - o[0]).abs() < 0.28
                    && (q[2] - o[2]).abs() < 0.28
                    && q[1] + 0.8 > o[1]
                    && q[1] < o[1] + 1.7
            }) {
                return false;
            }
        }
    }
    true
}

/// The existing upright character controller represents a vehicle envelope
/// with a small bounded row of contact bodies. NPCs and both peers share it.
pub fn obstacle_positions(position: [f32; 3], vehicle: Option<Vehicle>) -> Vec<[f32; 3]> {
    let Some(vehicle) = vehicle else {
        return vec![position];
    };
    let [width, _, length] = vehicle.kind.dimensions();
    let side = (width - 0.28).max(0.);
    let fore = (length - 0.28).max(0.);
    let nx = (side * 2. / 0.5).ceil().max(1.) as usize;
    let nz = (fore * 2. / 0.5).ceil().max(1.) as usize;
    let f = forward(vehicle.heading);
    let mut points = vec![position];
    for x in 0..=nx {
        for z in 0..=nz {
            let across = -side + side * 2. * x as f32 / nx as f32;
            let along = -fore + fore * 2. * z as f32 / nz as f32;
            points.push([
                position[0] + f[1] * across + f[0] * along,
                position[1],
                position[2] - f[0] * across + f[1] * along,
            ]);
        }
    }
    points
}

pub fn spawn(
    world: &World,
    body: &mut Body,
    kind: VehicleKind,
    heading: f32,
    obstacles: &[[f32; 3]],
) -> Result<Vehicle, &'static str> {
    let vehicle = Vehicle {
        kind,
        heading: heading.rem_euclid(std::f32::consts::TAU),
    };
    let mut p = body.position;
    if kind.watercraft() {
        let Some(level) = environment::water_surface(world, p[0], p[2]) else {
            return Err("Move into open water to launch this boat.");
        };
        if (p[1] - level).abs() > 2.0 {
            return Err("Come down to the water to launch this boat.");
        }
        p[1] = level + 0.02;
    } else if !body.on_ground
        || environment::water_surface(world, p[0], p[2]).is_some_and(|level| p[1] < level + 0.2)
    {
        return Err("Stand on dry ground to ride a bike.");
    }
    if !position_is_clear(world, p, vehicle, obstacles) {
        return Err("Find more clear space for the whole vehicle.");
    }
    body.position = p;
    body.velocity = [0.0; 3];
    body.glide_stalled = false;
    Ok(vehicle)
}

/// A simple auto-trimmed polar: a 45-degree no-go zone, strong beam reach,
/// slower run. Momentum persists through irons, but wind cannot drive upwind.
pub fn sail_drive(heading: [f32; 2], wind: [f32; 2]) -> f32 {
    let speed = norm(wind);
    if speed < 0.01 {
        return 0.0;
    }
    let down = (heading[0] * wind[0] + heading[1] * wind[1]) / speed;
    let from = (-down).clamp(-1.0, 1.0);
    if from > std::f32::consts::FRAC_1_SQRT_2 {
        return 0.0;
    }
    let angle = libm::acosf(from);
    let close = ((angle - std::f32::consts::FRAC_PI_4) / 0.25).clamp(0.0, 1.0);
    speed * close * (0.48 + 0.52 * libm::sinf(angle)) * 0.65
}

pub fn advance(
    world: &World,
    body: &mut Body,
    input: MoveInput,
    dt: f32,
    obstacles: &[[f32; 3]],
    now: f64,
    vehicle: &mut Option<Vehicle>,
) -> bool {
    let Some(mut craft) = *vehicle else {
        return false;
    };
    if input.fly {
        *vehicle = None;
        return false;
    }
    if !dt.is_finite() || dt <= 0.0 {
        return true;
    }
    if !craft.heading.is_finite()
        || body
            .position
            .iter()
            .chain(&body.velocity)
            .any(|v| !v.is_finite())
    {
        *vehicle = None;
        *body = Body::new(world.spawn_position());
        return false;
    }
    let direction = input.direction.map(|v| if v.is_finite() { v } else { 0.0 });
    let throttle = norm(direction).min(1.0);
    let steps = (dt.min(0.25) / (1.0 / 120.0)).ceil() as usize;
    let step = dt.min(0.25) / steps as f32;
    for _ in 0..steps {
        let old_heading = craft.heading;
        if throttle > 0.05 {
            let target = libm::atan2f(direction[0], -direction[1]);
            let delta = (target - craft.heading + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            let turn = match craft.kind {
                VehicleKind::Bike => 2.2,
                VehicleKind::Kayak => 1.8,
                VehicleKind::Sailboat => 0.8,
            };
            craft.heading = (craft.heading + delta.clamp(-turn * step, turn * step))
                .rem_euclid(std::f32::consts::TAU);
            if !position_is_clear(world, body.position, craft, obstacles) {
                craft.heading = old_heading;
            }
        }
        let f = forward(craft.heading);
        if craft.kind == VehicleKind::Bike {
            let slope = if body.on_ground {
                [
                    (world.surface_height(body.position[0] + 0.75, body.position[2])
                        - world.surface_height(body.position[0] - 0.75, body.position[2]))
                        / 1.5,
                    (world.surface_height(body.position[0], body.position[2] + 0.75)
                        - world.surface_height(body.position[0], body.position[2] - 0.75))
                        / 1.5,
                ]
            } else {
                [0.0; 2]
            };
            let v = [body.velocity[0], body.velocity[2]];
            let along = v[0] * f[0] + v[1] * f[1];
            for (axis, k) in [(0, 0), (2, 1)] {
                let pedal = if input.jump {
                    0.0
                } else {
                    throttle * if input.sprint { 5.0 } else { 3.2 }
                };
                let acceleration = f[k] * pedal
                    - slope[k].clamp(-1.5, 1.5) * 8.0
                    - v[k] * (0.12 + norm(v) * 0.018 + if input.jump { 5.0 } else { 0.0 })
                    + (f[k] * along - v[k]) * 4.0;
                body.velocity[axis] = (v[k] + acceleration * step).clamp(-28.0, 28.0);
            }
            let before = body.clone();
            crate::physics::move_rolling_body(world, body, step, obstacles);
            if !position_is_clear(world, body.position, craft, obstacles) {
                *body = before;
                body.velocity[0] = 0.;
                body.velocity[2] = 0.;
            }
        } else {
            let flow = environment::current(world, body.position[0], body.position[2]);
            let wind = environment::wind(world.seed, now);
            let v = [body.velocity[0] - flow[0], body.velocity[2] - flow[1]];
            let along = v[0] * f[0] + v[1] * f[1];
            let thrust = if input.jump {
                0.0
            } else {
                match craft.kind {
                    VehicleKind::Kayak => throttle * if input.sprint { 4.0 } else { 2.6 },
                    VehicleKind::Sailboat => sail_drive(f, wind),
                    _ => 0.0,
                }
            };
            for (axis, k) in [(0, 0), (2, 1)] {
                let leeway = if craft.kind == VehicleKind::Sailboat {
                    wind[k] * 0.035
                } else {
                    0.0
                };
                let drag = 0.3 + norm(v) * 0.07 + if input.jump { 2.0 } else { 0.0 };
                body.velocity[axis] +=
                    (f[k] * thrust + leeway - v[k] * drag + (f[k] * along - v[k]) * 1.3) * step;
            }
            for axis in [0, 2] {
                let old = body.position;
                let mut next = old;
                next[axis] += body.velocity[axis] * step;
                if position_is_clear(world, next, craft, obstacles) {
                    body.position = next;
                } else {
                    body.velocity[axis] = 0.0;
                }
            }
            if let Some(level) =
                environment::water_surface(world, body.position[0], body.position[2])
            {
                let target = level + 0.02;
                let mut next = body.position;
                if body.position[1] > target {
                    body.velocity[1] = (body.velocity[1] - 22. * step).max(-12.);
                    next[1] = (body.position[1] + body.velocity[1] * step).max(target);
                } else {
                    next[1] = (body.position[1] + 4. * step).min(target);
                    body.velocity[1] = 0.;
                }
                if position_is_clear(world, next, craft, obstacles) {
                    body.position = next;
                }
                if (body.position[1] - target).abs() < 0.005 {
                    body.velocity[1] = 0.;
                }
            }
            body.on_ground = false;
        }
        // Terrain edits can invalidate a mounted pose. Release it safely so
        // the ordinary controller can recover from the new obstruction.
        if !position_is_clear(world, body.position, craft, &[]) {
            *vehicle = None;
            return false;
        }
    }
    *vehicle = Some(craft);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Block, WorldGeneration};
    fn island() -> &'static World {
        static WORLD: std::sync::OnceLock<World> = std::sync::OnceLock::new();
        WORLD.get_or_init(|| World::generate(42, WorldGeneration::GeographyV2))
    }
    #[test]
    fn sailing_requires_tacking_and_retains_momentum_when_braked() {
        let world = island();
        let wind = environment::wind(world.seed, 0.);
        let speed = norm(wind);
        let up = wind.map(|v| -v / speed);
        let heading = libm::atan2f(up[0], -up[1]);
        let mut stuck = Body::new([15_000., 0.02, 15_000.]);
        let mut craft =
            Some(spawn(world, &mut stuck, VehicleKind::Sailboat, heading, &[]).unwrap());
        let mut tack = stuck.clone();
        let mut tack_craft = craft;
        tack_craft.as_mut().unwrap().heading += 1.05;
        for _ in 0..600 {
            advance(
                world,
                &mut stuck,
                MoveInput::default(),
                0.05,
                &[],
                0.,
                &mut craft,
            );
            advance(
                world,
                &mut tack,
                MoveInput::default(),
                0.05,
                &[],
                0.,
                &mut tack_craft,
            );
        }
        let upwind_distance =
            (tack.position[0] - 15_000.) * up[0] + (tack.position[2] - 15_000.) * up[1];
        assert!(
            upwind_distance > 10.,
            "tacking must make upwind progress: {upwind_distance}"
        );
        assert!(
            stuck.velocity[0] * up[0] + stuck.velocity[2] * up[1] <= 0.,
            "a stalled sail may drift downwind but cannot make upwind progress"
        );
        let before = norm([tack.velocity[0], tack.velocity[2]]);
        advance(
            world,
            &mut tack,
            MoveInput {
                jump: true,
                ..Default::default()
            },
            0.05,
            &[],
            0.,
            &mut tack_craft,
        );
        let after = norm([tack.velocity[0], tack.velocity[2]]);
        assert!(
            after > 0. && after < before,
            "brake dissipates momentum over time"
        );
    }
    #[test]
    fn whole_hull_rejects_shallow_banks_and_edited_walls() {
        let mut world = island().clone();
        let p = [15_000., 0.02, 15_000.];
        let mut body = Body::new(p);
        assert!(spawn(&world, &mut body, VehicleKind::Kayak, 0., &[]).is_ok());
        let wall = BlockPos::new(30_000, 0, 29_997);
        world.set_block(wall, Block::Stone).unwrap();
        assert!(spawn(&world, &mut body, VehicleKind::Kayak, 0., &[]).is_err());
        assert!(
            character_position_is_clear(&world, p, &[]),
            "wall is beyond the rider but intersects the bow"
        );
        assert_eq!(body.position, p, "a rejected spawn must not move the rider");
        assert!(
            spawn(
                &world,
                &mut Body::new(world.spawn_position()),
                VehicleKind::Kayak,
                0.,
                &[]
            )
            .is_err()
        );
    }
    #[test]
    fn a_coasting_bike_accelerates_downhill_and_stops_at_a_thin_wall() {
        let mut world = World::new(42);
        for x in -8..60 {
            for z in -4..=4 {
                world
                    .set_block(BlockPos::new(x, 60 + x.div_euclid(4), z), Block::Stone)
                    .unwrap();
            }
        }
        let mut body = Body::new([18.25, 35., 0.25]);
        body.position[1] = world.surface_height(body.position[0], body.position[2]);
        body.on_ground = true;
        let mut craft = Some(
            spawn(
                &world,
                &mut body,
                VehicleKind::Bike,
                -std::f32::consts::FRAC_PI_2,
                &[],
            )
            .unwrap(),
        );
        for _ in 0..80 {
            advance(
                &world,
                &mut body,
                MoveInput::default(),
                0.05,
                &[],
                0.,
                &mut craft,
            );
        }
        assert!(
            body.velocity[0] < -1.0,
            "downhill gravity must move a coasting bike: {:?}",
            body.velocity
        );
        assert!(body.position[0] < 16.0);
        assert!(craft.is_some());
        let wall_x = (body.position[0] / CELL_SIZE).floor() as i32 - 4;
        for y in 58..85 {
            for z in -4..=4 {
                world
                    .set_block(BlockPos::new(wall_x, y, z), Block::Stone)
                    .unwrap();
            }
        }
        for _ in 0..120 {
            advance(
                &world,
                &mut body,
                MoveInput::default(),
                0.05,
                &[],
                0.,
                &mut craft,
            );
        }
        assert!(body.position[0] > wall_x as f32 * CELL_SIZE + 0.8);
        assert!(
            craft.is_some(),
            "contact brakes the bike rather than deleting it"
        );
    }
    #[test]
    fn a_neutral_kayak_follows_a_real_river_bend_with_continuous_momentum() {
        let world = island();
        let geo = world.geography().unwrap();
        let mut fixture = None;
        for (i, next) in geo.drainage().iter().enumerate() {
            let Some(next) = *next else { continue };
            let Some(after) = geo.drainage()[next] else {
                continue;
            };
            if geo.flow_accumulation()[i] < 400. {
                continue;
            }
            let a = geo.grid_position(i);
            let b = geo.grid_position(next);
            let c = geo.grid_position(after);
            let ab = [b[0] - a[0], b[1] - a[1]];
            let bc = [c[0] - b[0], c[1] - b[1]];
            if (ab[0] * bc[1] - ab[1] * bc[0]).abs() < 1. {
                continue;
            }
            let x = a[0] + ab[0] * 0.6;
            let z = a[1] + ab[1] * 0.6;
            let Some(y) = environment::water_surface(world, x, z) else {
                continue;
            };
            if y < 40.
                || norm(environment::current(world, b[0], b[1])) < 1.0
                || norm(environment::current(
                    world,
                    b[0] + bc[0] * 0.35,
                    b[1] + bc[1] * 0.35,
                )) < 1.0
            {
                continue;
            }
            let p = [x, y + 0.02, z];
            let heading = libm::atan2f(ab[0], -ab[1]);
            let mut body = Body::new(p);
            if norm(environment::current(world, x, z)) > 1.5
                && let Ok(craft) = spawn(world, &mut body, VehicleKind::Kayak, heading, &[])
            {
                fixture = Some((body, craft, b));
                break;
            }
        }
        let (mut body, craft, bend) = fixture.expect("a navigable bent river exists");
        let start = body.position;
        let mut craft = Some(craft);
        let mut closest = f32::INFINITY;
        for _ in 0..1200 {
            let old = body.position;
            assert!(
                advance(
                    world,
                    &mut body,
                    MoveInput::default(),
                    0.05,
                    &[],
                    0.,
                    &mut craft
                ),
                "kayak lost water at {:?}",
                body.position
            );
            assert!(
                crate::physics::flight_speed([
                    body.position[0] - old[0],
                    body.position[1] - old[1],
                    body.position[2] - old[2]
                ]) < 0.5,
                "no position snapping"
            );
            closest = closest.min(norm([
                body.position[0] - bend[0],
                body.position[2] - bend[1],
            ]));
        }
        assert!(
            closest < 8.,
            "current must carry the kayak through the bend, closest {closest}; start {start:?} bend {bend:?} end {:?} velocity {:?} current {:?}",
            body.position,
            body.velocity,
            environment::current(world, body.position[0], body.position[2])
        );
        assert!(
            norm([body.position[0] - start[0], body.position[2] - start[2]]) > 40.,
            "drift must continue after the bend: {:?} -> {:?}",
            start,
            body.position
        );
        assert!(environment::water_surface(world, body.position[0], body.position[2]).is_some());
    }
    #[test]
    fn passing_people_stop_motion_without_putting_a_vehicle_away() {
        let world = World::new(42);
        let mut body = Body::new(world.spawn_position());
        crate::physics::move_character(&world, &mut body, MoveInput::default(), 0.25);
        let mut craft = Some(spawn(&world, &mut body, VehicleKind::Bike, 0., &[]).unwrap());
        let person = [body.position[0], body.position[1], body.position[2] - 0.85];
        assert!(!position_is_clear(
            &world,
            body.position,
            craft.unwrap(),
            &[person]
        ));
        for _ in 0..20 {
            assert!(advance(
                &world,
                &mut body,
                MoveInput {
                    direction: [0., -1.],
                    ..Default::default()
                },
                0.05,
                &[person],
                0.,
                &mut craft
            ));
        }
        assert!(craft.is_some());
        assert_eq!(body.velocity[2], 0.);
        assert!(
            obstacle_positions(body.position, craft)
                .iter()
                .any(|p| (p[2] - person[2]).abs() < 0.3)
        );
    }
}
