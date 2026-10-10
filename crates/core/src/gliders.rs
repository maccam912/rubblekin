//! On-demand village launches and forgiving personal gliding. Flight plans are
//! transient; clients and server sample the same continuous trajectory.
use crate::{
    physics::{
        Body, MoveInput, character_position_is_clear, move_character_with_obstacles,
        move_gliding_body,
    },
    world::{CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};

pub const GLIDER_SEATS: u8 = 4;
pub const LAUNCH_RANGE: f32 = 16_000.0;
pub const STATION_REACH: f32 = 9.0;
pub const LAUNCH_SECONDS: f64 = 8.0;
pub const CARRIAGE_VISIBILITY: f32 = 1_000.0;

#[derive(Debug, Clone)]
pub struct WhipStation {
    pub village_id: u32,
    pub name: String,
    /// Launch beside an existing outgoing road, clear of town buildings/crops.
    pub position: [f32; 3],
    /// Arrivals still aim for the central street near the storehouse.
    pub landing_position: [f32; 3],
}

pub fn stations(world: &World) -> Vec<WhipStation> {
    world.settlements().map_or_else(Vec::new, |plan| {
        plan.villages
            .iter()
            .map(|v| {
                let port = crate::airships::port(world, v);
                // The original side berths already have clear footprints and
                // a walkable branch from town. Reuse the first one's ground
                // rather than covering the central street with the tall whip.
                let mut position = crate::airship_landings::landings(world, v, &port, 1)
                    .first()
                    .expect("village has a clear roadside launch berth")
                    .position;
                position[1] -= 0.35;
                WhipStation {
                    village_id: v.id,
                    name: v.name.clone(),
                    position,
                    landing_position: port.position,
                }
            })
            .collect()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GliderDestination {
    Village(u32),
    Player(u64),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GliderRide {
    pub carriage_id: u64,
    pub seat: u8,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GliderAction {
    Board {
        station_id: u32,
        destination: GliderDestination,
    },
    Launch,
    Leave,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GliderFlight {
    pub id: u64,
    pub station_id: u32,
    pub destination: GliderDestination,
    pub destination_name: String,
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub apex: f32,
    pub duration: f64,
    pub created_at: f64,
    pub started_at: Option<f64>,
}
#[derive(Debug, Clone, Copy)]
pub struct GliderPose {
    pub position: [f32; 3],
    pub yaw: f32,
    pub landed: bool,
    pub launching: bool,
}

pub fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).powi(2))
        .sum::<f32>()
        .sqrt()
}
pub fn horizontal_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    libm::hypotf(a[0] - b[0], a[2] - b[2])
}
pub fn reachable(a: [f32; 3], b: [f32; 3]) -> bool {
    b.iter().all(|v| v.is_finite()) && horizontal_distance(a, b) <= LAUNCH_RANGE
}
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

impl GliderFlight {
    pub fn plan(
        world: &World,
        station: &WhipStation,
        destination: GliderDestination,
        name: String,
        target: [f32; 3],
        id: u64,
        now: f64,
    ) -> Result<Self, &'static str> {
        if !reachable(station.position, target) {
            return Err(
                "That destination is outside this station's 16 km reach. Take another hop first.",
            );
        }
        let yaw = libm::atan2f(
            target[0] - station.position[0],
            target[2] - station.position[2],
        );
        let to = landing_near_facing(world, target, yaw)
            .ok_or("There is no clear, dry landing near that destination.")?;
        if !reachable(station.position, to) {
            return Err("The nearest safe landing is outside launch range.");
        }
        let mut from = station.position;
        from[1] += 0.65;
        // A shallow glide with a steeper final approach clears sampled ground. Extra clearance
        // covers trees and normal village roofs; final approach is checked below.
        let distance = horizontal_distance(from, to);
        let steps = (distance / 16.0).ceil().max(1.0) as usize;
        let mut apex = (from[1] + 500.0).max(to[1] + 300.0);
        for i in 0..steps {
            let u = i as f32 / steps as f32;
            let ground = world.surface_height(lerp(from[0], to[0], u), lerp(from[2], to[2], u));
            let clearance = 45.0 * ((1.0 - u) * distance / 120.0).clamp(0.0, 1.0);
            let descent = u.powi(4);
            apex = apex.max((ground + clearance - descent * to[1]) / (1.0 - descent));
        }
        let flight = Self {
            id,
            station_id: station.village_id,
            destination,
            destination_name: name,
            from,
            to,
            apex,
            duration: LAUNCH_SECONDS + (distance / 130.0).max(18.0) as f64,
            created_at: now,
            started_at: None,
        };
        // Use the actual edited cells for the final approach, including tree
        // canopies. Raise the launch height if a descent would clip an obstacle.
        let mut flight = flight;
        // Fixed-count samples can skip whole voxel columns on a long route.
        let approach_steps = (distance * 0.1 / (CELL_SIZE * 0.5)).ceil().max(80.0) as usize;
        for _ in 0..4 {
            if flight.apex > world.max_y() as f32 * CELL_SIZE - 4.0 {
                return Err("The mountains require an intermediate station on this route.");
            }
            let clear = (0..=approach_steps).all(|i| {
                let u = 0.90 + 0.10 * i as f32 / approach_steps as f32;
                let p = [
                    lerp(from[0], to[0], u),
                    lerp(flight.apex, to[1], u.powi(4)),
                    lerp(from[2], to[2], u),
                ];
                let pose = GliderPose {
                    position: p,
                    yaw: flight.yaw(),
                    landed: false,
                    launching: false,
                };
                std::iter::once(p)
                    .chain((0..GLIDER_SEATS).map(|seat| seat_position(pose, seat)))
                    .all(|p| character_position_is_clear(world, p, &[]))
            });
            if clear {
                return Ok(flight);
            }
            let mut required = flight.apex + 150.0;
            for i in 0..approach_steps {
                let u = 0.90 + 0.10 * i as f32 / approach_steps as f32;
                let pose = GliderPose {
                    position: [
                        lerp(from[0], to[0], u),
                        lerp(flight.apex, to[1], u.powi(4)),
                        lerp(from[2], to[2], u),
                    ],
                    yaw: flight.yaw(),
                    landed: false,
                    launching: false,
                };
                for p in std::iter::once(pose.position)
                    .chain((0..GLIDER_SEATS).map(|seat| seat_position(pose, seat)))
                {
                    if !character_position_is_clear(world, p, &[]) {
                        let mut roof = p[1];
                        for x in [
                            -crate::physics::PLAYER_RADIUS,
                            0.0,
                            crate::physics::PLAYER_RADIUS,
                        ] {
                            for z in [
                                -crate::physics::PLAYER_RADIUS,
                                0.0,
                                crate::physics::PLAYER_RADIUS,
                            ] {
                                roof = roof.max(world.surface_height(p[0] + x, p[2] + z));
                            }
                        }
                        let blend = u.powi(4);
                        required = required.max((roof + 0.2 - blend * to[1]) / (1.0 - blend));
                    }
                }
            }
            flight.apex = required;
        }
        Err("The landing approach is blocked. Choose another destination.")
    }
    fn yaw(&self) -> f32 {
        libm::atan2f(self.to[0] - self.from[0], self.to[2] - self.from[2])
    }
    pub fn pose(&self, now: f64) -> GliderPose {
        let elapsed = self.started_at.map_or(0.0, |start| (now - start).max(0.0));
        let launching = self.started_at.is_some() && elapsed < LAUNCH_SECONDS;
        let position = if elapsed < LAUNCH_SECONDS {
            let t = (elapsed / LAUNCH_SECONDS) as f32;
            [
                self.from[0],
                lerp(
                    self.from[1],
                    self.apex,
                    ease(((t - 0.25) / 0.75).clamp(0.0, 1.0)),
                ),
                self.from[2],
            ]
        } else {
            let t = ((elapsed - LAUNCH_SECONDS) / (self.duration - LAUNCH_SECONDS)).clamp(0.0, 1.0)
                as f32;
            let u = ease(t);
            [
                lerp(self.from[0], self.to[0], u),
                lerp(self.apex, self.to[1], u.powi(4)),
                lerp(self.from[2], self.to[2], u),
            ]
        };
        GliderPose {
            position,
            yaw: self.yaw(),
            landed: self.started_at.is_some() && elapsed >= self.duration,
            launching,
        }
    }
    pub fn velocity(&self, now: f64) -> [f32; 3] {
        let a = self.pose(now).position;
        let b = self.pose(now + 0.02).position;
        std::array::from_fn(|i| (b[i] - a[i]) / 0.02)
    }
}

pub fn seat_position(pose: GliderPose, seat: u8) -> [f32; 3] {
    let side = if seat.is_multiple_of(2) { -0.8 } else { 0.8 };
    let fore = if seat < 2 { 0.9 } else { -0.9 };
    let (s, c) = libm::sincosf(pose.yaw);
    [
        pose.position[0] + side * c + fore * s,
        pose.position[1],
        pose.position[2] - side * s + fore * c,
    ]
}

pub fn landing_near(world: &World, target: [f32; 3]) -> Option<[f32; 3]> {
    landing_near_facing(world, target, 0.0)
}

pub fn landing_near_facing(world: &World, target: [f32; 3], yaw: f32) -> Option<[f32; 3]> {
    let (sin, cos) = libm::sincosf(yaw);
    for ring in 0_i32..=16 {
        for z in -ring..=ring {
            for x in -ring..=ring {
                if x.abs().max(z.abs()) != ring {
                    continue;
                }
                let px = target[0] + x as f32 * 3.0;
                let pz = target[2] + z as f32 * 3.0;
                let radius = world.radius_cells() as f32 * CELL_SIZE;
                let bounds = [-radius, radius, -radius, radius];
                if px < bounds[0] + 3.0
                    || px > bounds[1] - 3.0
                    || pz < bounds[2] + 3.0
                    || pz > bounds[3] - 3.0
                {
                    continue;
                }
                let h = world.surface_height(px, pz);
                let p = [px, h + 0.65, pz];
                let dry = world
                    .geography()
                    .is_none_or(|g| g.sample(px, pz).water.is_none_or(|level| level <= h))
                    && (world.original_ground_height(px, pz) - h).abs() < 0.6
                    && world
                        .block(crate::world::BlockPos::new(
                            (px / CELL_SIZE).floor() as i32,
                            ((h - CELL_SIZE * 0.5) / CELL_SIZE).floor() as i32,
                            (pz / CELL_SIZE).floor() as i32,
                        ))
                        .is_solid();
                // A clear endpoint beside a tree or terrain lip can still
                // require an impossible descent. Keep the last four meters
                // flat and clear for the carriage and every passenger.
                if dry
                    && (0..=8).all(|step| {
                        let back = step as f32 * CELL_SIZE;
                        let pose = GliderPose {
                            position: [p[0] - sin * back, p[1], p[2] - cos * back],
                            yaw,
                            landed: true,
                            launching: false,
                        };
                        std::iter::once(pose.position)
                            .chain((0..GLIDER_SEATS).map(|seat| seat_position(pose, seat)))
                            .all(|s| {
                                (world.surface_height(s[0], s[2]) - h).abs() < CELL_SIZE * 0.75
                                    && character_position_is_clear(world, s, &[])
                            })
                    })
                {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Input, collision and automatic deployment share one implementation. Carriage
/// occupants use fixed separated seats; Jump leaves at the current flight pose.
#[allow(clippy::too_many_arguments)]
pub fn move_with_gliders(
    world: &World,
    body: &mut Body,
    input: MoveInput,
    dt: f32,
    obstacles: &[[f32; 3]],
    flights: &[GliderFlight],
    now: f64,
    ride: &mut Option<GliderRide>,
    gliding: &mut bool,
) {
    if input.fly {
        *ride = None;
        *gliding = false;
    }
    if let Some(r) = *ride {
        if let Some(flight) = flights.iter().find(|f| f.id == r.carriage_id) {
            let pose = flight.pose(now);
            body.position = seat_position(pose, r.seat);
            body.velocity = flight.velocity(now);
            body.on_ground = flight.started_at.is_none() || pose.landed;
            if input.jump || pose.landed {
                *ride = None;
                *gliding = !body.on_ground;
                // Step to the side of the craft, away from other passengers.
                body.position[0] += if r.seat.is_multiple_of(2) { -2.0 } else { 2.0 };
            } else {
                return;
            }
        } else {
            *ride = None;
            *gliding = true;
        }
    }
    if *gliding && !input.fly {
        move_gliding_body(world, body, input, dt, obstacles);
        if body.on_ground {
            *gliding = false;
        }
    } else {
        body.glide_stalled = false;
        move_character_with_obstacles(world, body, input, dt, obstacles);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn walk(world: &World, body: &mut Body, target: [f32; 3]) {
        for _ in 0..360 {
            let run = horizontal_distance(body.position, target);
            if run < 0.15 && body.on_ground {
                return;
            }
            let speed = (run / 0.13).min(0.6);
            let direction = if run > 0.03 {
                [
                    (target[0] - body.position[0]) / run * speed,
                    (target[2] - body.position[2]) / run * speed,
                ]
            } else {
                [0.0; 2]
            };
            crate::physics::move_character(
                world,
                body,
                MoveInput {
                    direction,
                    ..Default::default()
                },
                1.0 / 30.0,
            );
        }
        panic!(
            "station approach blocked: {:?} -> {target:?}",
            body.position
        );
    }
    #[test]
    fn range_requires_multiple_hops_and_seats_are_separated() {
        assert!(reachable([0.0; 3], [15_999.0, 500.0, 0.0]));
        assert!(!reachable([0.0; 3], [32_768.0, 0.0, 0.0]));
        assert!(!reachable([0.0; 3], [f32::NAN, 0.0, 0.0]));
        let pose = GliderPose {
            position: [0.0; 3],
            yaw: 1.2,
            landed: false,
            launching: false,
        };
        for a in 0..4 {
            for b in a + 1..4 {
                assert!(horizontal_distance(seat_position(pose, a), seat_position(pose, b)) > 1.5);
            }
        }
    }
    #[test]
    fn flight_is_continuous_and_reaches_destination_without_teleporting() {
        let f = GliderFlight {
            id: 1,
            station_id: 0,
            destination: GliderDestination::Village(1),
            destination_name: "Test".into(),
            from: [0.0; 3],
            to: [5000.0, 20.0, 0.0],
            apex: 700.0,
            duration: 60.0,
            created_at: 0.0,
            started_at: Some(10.0),
        };
        assert_eq!(f.pose(10.0).position, f.from);
        assert!((f.pose(18.0 - 0.001).position[1] - f.pose(18.0).position[1]).abs() < 0.01);
        assert_eq!(f.pose(70.0).position, f.to);
        assert!(f.pose(70.0).landed);
        assert!(f.pose(40.0).position[0] > 1000.0);
    }
    #[test]
    fn stations_have_reachable_landings_and_launches_clear_landforms() {
        for seed in [42, 7, 99, 43, 123, 2_689_504_302, 2_129_398_673] {
            let world = World::generate(seed, crate::world::WorldGeneration::GeographyV6);
            let stops = stations(&world);
            assert_eq!(stops.len(), world.settlements().unwrap().villages.len());
            for station in &stops {
                let village = world
                    .settlements()
                    .unwrap()
                    .villages
                    .iter()
                    .find(|v| v.id == station.village_id)
                    .unwrap();
                assert!(
                    horizontal_distance(station.position, village.center) > 40.0,
                    "seed {seed}: {} launch still occupies the town center",
                    station.name
                );
                assert_eq!(
                    station.landing_position,
                    crate::airships::port(&world, village).position
                );
                assert!(
                    horizontal_distance(station.position, station.landing_position) > STATION_REACH
                );
                assert!(character_position_is_clear(&world, station.position, &[]));
                let berth = crate::airship_landings::landings(
                    &world,
                    village,
                    &crate::airships::port(&world, village),
                    1,
                )
                .remove(0);
                let mut body = Body::new(station.landing_position);
                body.on_ground = true;
                let mut walked = Vec::new();
                for &point in &berth.approach {
                    walk(&world, &mut body, point);
                    walked.push(point);
                    if distance(body.position, station.position) <= STATION_REACH {
                        break;
                    }
                }
                assert!(distance(body.position, station.position) <= STATION_REACH);
                for point in walked.into_iter().rev() {
                    walk(&world, &mut body, point);
                }
                assert!(distance(body.position, station.landing_position) < 0.6);
                // Include the offset tower/winch, not just the carriage feet.
                for dx in [-2.0, 0.0, 4.0, 6.0, 8.0] {
                    for dz in [-2.5, 0.0, 2.5] {
                        assert!(
                            world.original_surface_height(
                                station.position[0] + dx,
                                station.position[2] + dz
                            ) <= station.position[1] + 0.01
                        );
                    }
                }
                let other = stops
                    .iter()
                    .filter(|s| s.village_id != station.village_id)
                    .min_by(|a, b| {
                        horizontal_distance(station.position, a.landing_position)
                            .total_cmp(&horizontal_distance(station.position, b.landing_position))
                    })
                    .unwrap();
                assert!(reachable(station.position, other.landing_position));
                let mut f = GliderFlight::plan(
                    &world,
                    station,
                    GliderDestination::Village(other.village_id),
                    other.name.clone(),
                    other.landing_position,
                    1,
                    0.0,
                )
                .unwrap_or_else(|error| {
                    panic!("seed {seed} {} -> {}: {error}", station.name, other.name)
                });
                assert!(horizontal_distance(f.to, other.landing_position) <= 46.0);
                f.started_at = Some(0.0);
                for i in 0..=32 {
                    let pose = f.pose(LAUNCH_SECONDS * i as f64 / 32.0);
                    for seat in 0..GLIDER_SEATS {
                        assert!(character_position_is_clear(
                            &world,
                            seat_position(pose, seat),
                            &[]
                        ));
                    }
                }
                for i in 0..=100 {
                    let p = f
                        .pose(LAUNCH_SECONDS + (f.duration - LAUNCH_SECONDS) * i as f64 / 100.0)
                        .position;
                    assert!(
                        p[1] + 0.1 >= world.surface_height(p[0], p[2]),
                        "seed {seed} {} -> {}, sample {i}: {p:?}, surface {}",
                        station.name,
                        other.name,
                        world.surface_height(p[0], p[2])
                    );
                    let pose =
                        f.pose(LAUNCH_SECONDS + (f.duration - LAUNCH_SECONDS) * i as f64 / 100.0);
                    for seat in 0..GLIDER_SEATS {
                        assert!(
                            character_position_is_clear(&world, seat_position(pose, seat), &[]),
                            "seed {seed} {} -> {}, sample {i}, seat {seat}",
                            station.name,
                            other.name
                        );
                    }
                }
            }
            let mut edited = world.clone();
            let s = &stops[0];
            let p = crate::world::BlockPos::new(
                (s.position[0] / CELL_SIZE).floor() as i32,
                (s.position[1] / CELL_SIZE).floor() as i32,
                (s.position[2] / CELL_SIZE).floor() as i32,
            );
            edited.set_block(p, crate::world::Block::Stone).unwrap();
            assert_eq!(stations(&edited)[0].position, s.position);
        }
    }
}

#[cfg(test)]
mod canopy_tests {
    use super::*;
    use crate::world::{Block, BlockPos};
    #[test]
    fn ordinary_ground_jumps_do_not_open_a_canopy() {
        let world = World::new(42);
        let mut body = Body::new(world.spawn_position());
        body.on_ground = true;
        let mut ride = None;
        let mut gliding = false;
        for i in 0..40 {
            move_with_gliders(
                &world,
                &mut body,
                MoveInput {
                    jump: i == 0,
                    ..Default::default()
                },
                0.05,
                &[],
                &[],
                0.0,
                &mut ride,
                &mut gliding,
            );
            assert!(!gliding);
        }
        assert!(body.on_ground);
    }
    #[test]
    fn canopy_steers_brakes_and_sweeps_against_a_wall() {
        let mut world = World::new(42);
        let mut body = Body::new([0.25, 25.0, 0.25]);
        body.velocity = [18.0, -20.0, 0.0];
        for y in 0..60 {
            for z in -10..10 {
                world
                    .set_block(BlockPos::new(4, y, z), Block::Brick)
                    .unwrap();
            }
        }
        let mut gliding = true;
        let mut ride = None;
        for _ in 0..2 {
            move_with_gliders(
                &world,
                &mut body,
                MoveInput {
                    direction: [1.0, 0.0],
                    glide_direction: Some([1.0, 0.0, 0.0]),
                    jump: true,
                    ..Default::default()
                },
                0.1,
                &[],
                &[],
                0.0,
                &mut ride,
                &mut gliding,
            );
        }
        assert!(body.position[0] <= 2.0 - crate::physics::PLAYER_RADIUS + 0.01);
        assert!(body.position[0] > 1.6);
        assert!(body.velocity[1].is_finite());
        assert!(gliding);
        let z = body.position[2];
        move_with_gliders(
            &world,
            &mut body,
            MoveInput {
                direction: [0.0, 1.0],
                glide_direction: Some([0.0, 0.0, 1.0]),
                sprint: true,
                ..Default::default()
            },
            0.25,
            &[],
            &[],
            0.0,
            &mut ride,
            &mut gliding,
        );
        assert!(body.position[2] > z);
        assert!(body.velocity[1] < 0.0);
    }
    #[test]
    fn jump_leaves_the_current_carriage_pose_and_landing_closes_the_canopy() {
        let world = World::new(42);
        let spawn = world.spawn_position();
        let f = GliderFlight {
            id: 1,
            station_id: 0,
            destination: GliderDestination::Village(1),
            destination_name: "Test".into(),
            from: [spawn[0], 30.0, spawn[2]],
            to: spawn,
            apex: 60.0,
            duration: 50.0,
            created_at: 0.0,
            started_at: Some(0.0),
        };
        let mut body = Body::new(spawn);
        let mut ride = Some(GliderRide {
            carriage_id: 1,
            seat: 0,
        });
        let mut gliding = false;
        move_with_gliders(
            &world,
            &mut body,
            MoveInput {
                jump: true,
                ..Default::default()
            },
            0.1,
            &[],
            &[f],
            10.0,
            &mut ride,
            &mut gliding,
        );
        assert!(ride.is_none() && gliding);
        assert!(body.position[1] > 50.0);
        body = Body::new(spawn);
        body.position[1] += 0.01;
        move_with_gliders(
            &world,
            &mut body,
            MoveInput::default(),
            0.1,
            &[],
            &[],
            0.0,
            &mut ride,
            &mut gliding,
        );
        assert!(body.on_ground);
        assert!(!gliding);
    }
}
