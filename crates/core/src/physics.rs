//! Small shared character controller. The body's position is its foot center;
//! an upright AABB collides against the same edited voxels on client and server.

use crate::world::{BlockPos, CELL_SIZE, World};
use serde::{Deserialize, Serialize};

pub const PLAYER_RADIUS: f32 = 0.28;
pub const PLAYER_HEIGHT: f32 = 1.7;
pub const EYE_HEIGHT: f32 = 1.4;
const GRAVITY: f32 = 22.0;
const JUMP_SPEED: f32 = 7.2;
const CONTACT_EPSILON: f32 = 0.00001;

/// Contact boundaries are half-open. At kilometer coordinates, f32 rounding
/// exceeds the valley's ten-micrometer skin; reserve two rounding units so a
/// touching wall does not become an overlap on the next controller tick. The
/// skin stays below four millimeters throughout this world's horizontal span.
fn contact_epsilon(coordinate: f32) -> f32 {
    CONTACT_EPSILON.max(coordinate.abs() * f32::EPSILON * 2.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Body {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub on_ground: bool,
}

impl Body {
    pub fn new(position: [f32; 3]) -> Self {
        Self {
            position,
            velocity: [0.0; 3],
            on_ground: false,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct MoveInput {
    /// Desired world-space x/z movement. Lengths above one are normalized.
    pub direction: [f32; 2],
    pub jump: bool,
    pub sprint: bool,
    pub fly: bool,
    /// Creative flight only, clamped to -1..1.
    pub vertical: f32,
}

/// Advances at most 250 ms, subdividing collision movement to avoid tunneling.
/// Non-finite input is neutralized and a corrupt body is returned to spawn.
pub fn move_character(world: &World, body: &mut Body, input: MoveInput, dt: f32) {
    if body
        .position
        .iter()
        .chain(body.velocity.iter())
        .any(|value| !value.is_finite())
    {
        *body = Body::new(world.spawn_position());
    }
    constrain_to_world(world, body);
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    let dt = dt.min(0.25);
    let steps = (dt / (1.0 / 120.0)).ceil() as usize;
    let step = dt / steps as f32;
    let mut direction = input
        .direction
        .map(|value| if value.is_finite() { value } else { 0.0 });
    let length = ((direction[0] as f64).powi(2) + (direction[1] as f64).powi(2)).sqrt();
    if length > 1.0 {
        direction = direction.map(|value| (value as f64 / length) as f32);
    }
    let speed = if input.fly {
        if input.sprint { 12.0 } else { 7.0 }
    } else if input.sprint {
        6.2
    } else {
        3.8
    };
    body.velocity[0] = direction[0] * speed;
    body.velocity[2] = direction[1] * speed;
    if input.fly {
        let vertical = if input.vertical.is_finite() {
            input.vertical.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        body.velocity[1] = vertical * speed;
        // Flight's vertical and horizontal axes obey the same speed budget.
        let length = body
            .velocity
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        if length > speed {
            body.velocity
                .iter_mut()
                .for_each(|value| *value *= speed / length);
        }
        body.on_ground = false;
    } else if input.jump && body.on_ground {
        body.velocity[1] = JUMP_SPEED;
        body.on_ground = false;
    }

    // Terrain may have been edited around a character since its last tick.
    // Try the nearest space above it rather than leaving it trapped in a block.
    if collides(world, body.position) {
        let old = body.position;
        let mut recovered = false;
        for cells in 1..=8 {
            body.position[1] = old[1] + cells as f32 * CELL_SIZE;
            if !collides(world, body.position) {
                body.velocity[1] = 0.0;
                recovered = true;
                break;
            }
        }
        if !recovered {
            body.position = world.spawn_position();
            body.velocity = [0.0; 3];
        }
    }

    for _ in 0..steps {
        if !input.fly {
            body.velocity[1] = (body.velocity[1] - GRAVITY * step).max(-40.0);
        }
        let can_step = body.on_ground && !input.fly;
        for axis in [0, 2] {
            let amount = body.velocity[axis] * step;
            if amount == 0.0 {
                continue;
            }
            let before = body.position;
            let blocked = move_axis(world, &mut body.position, axis, amount);
            if blocked && can_step {
                let mut raised = before;
                raised[1] += CELL_SIZE + contact_epsilon(before[1] + CELL_SIZE) * 2.0;
                // Check both upward clearance and the forward destination.
                if !collides(world, raised) {
                    let forward_blocked = move_axis(world, &mut raised, axis, amount);
                    if !forward_blocked {
                        body.position = raised;
                    }
                }
            }
        }
        let vertical = body.velocity[1] * step;
        body.on_ground = false;
        if move_axis(world, &mut body.position, 1, vertical) {
            body.on_ground = vertical < 0.0;
            body.velocity[1] = 0.0;
        }
        constrain_to_world(world, body);
    }
}

fn constrain_to_world(world: &World, body: &mut Body) {
    let horizontal = world.radius_cells() as f32 * CELL_SIZE;
    for axis in [0, 2] {
        let bounded =
            body.position[axis].clamp(-horizontal + PLAYER_RADIUS, horizontal - PLAYER_RADIUS);
        if bounded != body.position[axis] {
            body.velocity[axis] = 0.0;
            body.position[axis] = bounded;
        }
    }
    let minimum = world.min_y() as f32 * CELL_SIZE;
    let maximum = world.max_y() as f32 * CELL_SIZE - PLAYER_HEIGHT;
    if body.position[1] < minimum {
        body.position[1] = minimum;
        body.velocity[1] = 0.0;
        body.on_ground = true;
    } else if body.position[1] > maximum {
        body.position[1] = maximum;
        body.velocity[1] = body.velocity[1].min(0.0);
    }
}

fn bounds(position: [f32; 3]) -> ([i32; 3], [i32; 3]) {
    let min = [
        position[0] - PLAYER_RADIUS,
        position[1],
        position[2] - PLAYER_RADIUS,
    ];
    let max = [
        position[0] + PLAYER_RADIUS,
        position[1] + PLAYER_HEIGHT,
        position[2] + PLAYER_RADIUS,
    ];
    (
        min.map(|v| ((v + contact_epsilon(v)) / CELL_SIZE).floor() as i32),
        max.map(|v| ((v - contact_epsilon(v)) / CELL_SIZE).floor() as i32),
    )
}

fn collides(world: &World, position: [f32; 3]) -> bool {
    let (minimum, maximum) = bounds(position);
    for y in minimum[1]..=maximum[1] {
        for z in minimum[2]..=maximum[2] {
            for x in minimum[0]..=maximum[0] {
                if world.block(BlockPos::new(x, y, z)).is_solid() {
                    return true;
                }
            }
        }
    }
    false
}

fn move_axis(world: &World, position: &mut [f32; 3], axis: usize, amount: f32) -> bool {
    if amount == 0.0 {
        return false;
    }
    position[axis] += amount;
    let (minimum, maximum) = bounds(*position);
    let mut blocked = false;
    for y in minimum[1]..=maximum[1] {
        for z in minimum[2]..=maximum[2] {
            for x in minimum[0]..=maximum[0] {
                if !world.block(BlockPos::new(x, y, z)).is_solid() {
                    continue;
                }
                blocked = true;
                let cell = [x, y, z][axis];
                let low_extent = if axis == 1 { 0.0 } else { PLAYER_RADIUS };
                let high_extent = if axis == 1 {
                    PLAYER_HEIGHT
                } else {
                    PLAYER_RADIUS
                };
                if amount > 0.0 {
                    position[axis] = position[axis].min(cell as f32 * CELL_SIZE - high_extent);
                } else {
                    position[axis] = position[axis].max((cell + 1) as f32 * CELL_SIZE + low_extent);
                }
            }
        }
    }
    blocked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Block;

    fn tick(world: &World, body: &mut Body, input: MoveInput, count: usize) {
        for _ in 0..count {
            move_character(world, body, input, 1.0 / 60.0);
        }
    }

    #[test]
    fn geographic_collision_and_flight_keep_full_world_bounds() {
        let world = World::generate(42, crate::world::WorldGeneration::GeographyV1);
        let spawn = world.spawn_position();
        let mut body = Body::new([spawn[0], spawn[1] + 4.0, spawn[2]]);
        tick(&world, &mut body, MoveInput::default(), 120);
        assert!(body.on_ground);
        assert!(!collides(&world, body.position));
        assert!(body.position[1] >= world.min_y() as f32 * CELL_SIZE);
        let start = [5000.25, 3000.0, -4000.25];
        body = Body::new(start);
        tick(
            &world,
            &mut body,
            MoveInput {
                direction: [1.0, 0.0],
                fly: true,
                ..Default::default()
            },
            30,
        );
        assert!(body.position[0] > start[0] + 1.0);
        assert!((body.position[1] - start[1]).abs() < 0.001);
        assert_eq!(body.position[2], start[2]);
    }

    #[test]
    fn kilometer_coordinates_touch_walls_without_false_overlap_recovery() {
        let mut world = World::generate(42, crate::world::WorldGeneration::GeographyV1);
        let (x, y, z) = (24000, 6000, 18000);
        for px in x - 3..=x + 7 {
            for pz in z - 3..=z + 3 {
                world
                    .set_block(BlockPos::new(px, y, pz), Block::Brick)
                    .unwrap();
            }
        }
        for py in y + 1..=y + 6 {
            for pz in z - 3..=z + 3 {
                world
                    .set_block(BlockPos::new(x + 2, py, pz), Block::Brick)
                    .unwrap();
            }
        }
        let mut body = Body::new([
            (x as f32 + 0.5) * CELL_SIZE,
            (y + 1) as f32 * CELL_SIZE,
            (z as f32 + 0.5) * CELL_SIZE,
        ]);
        let floor = body.position[1];
        tick(
            &world,
            &mut body,
            MoveInput {
                direction: [1.0, 0.0],
                ..Default::default()
            },
            40,
        );
        assert!(
            (body.position[1] - floor).abs() < 0.005,
            "wall contact falsely recovered body upwards: {:?}",
            body.position
        );
        assert!((body.position[0] - ((x + 2) as f32 * CELL_SIZE - PLAYER_RADIUS)).abs() < 0.005);
        assert!(!collides(&world, body.position));
        tick(&world, &mut body, MoveInput::default(), 15);
        assert!((body.position[1] - floor).abs() < 0.005);
        assert!(body.on_ground);

        // Contact with a low ceiling must not trigger upward recovery either.
        for px in x - 3..=x + 7 {
            for pz in z - 3..=z + 3 {
                world
                    .set_block(BlockPos::new(px, y + 5, pz), Block::Brick)
                    .unwrap();
            }
        }
        move_character(
            &world,
            &mut body,
            MoveInput {
                jump: true,
                ..Default::default()
            },
            0.1,
        );
        assert!(body.position[1] <= (y + 5) as f32 * CELL_SIZE - PLAYER_HEIGHT + 0.005);
        assert!(!collides(&world, body.position));
        tick(&world, &mut body, MoveInput::default(), 30);
        assert!((body.position[1] - floor).abs() < 0.005);
        assert!(body.on_ground);

        // Removing the ceiling and shortening the wall to one cell leaves an
        // ordinary half-meter step, which must remain walkable at this scale.
        for px in x - 3..=x + 7 {
            for pz in z - 3..=z + 3 {
                world
                    .set_block(BlockPos::new(px, y + 5, pz), Block::Air)
                    .unwrap();
            }
        }
        for py in y + 2..=y + 6 {
            for pz in z - 3..=z + 3 {
                world
                    .set_block(BlockPos::new(x + 2, py, pz), Block::Air)
                    .unwrap();
            }
        }
        tick(
            &world,
            &mut body,
            MoveInput {
                direction: [1.0, 0.0],
                ..Default::default()
            },
            10,
        );
        assert!(body.position[0] > (x + 2) as f32 * CELL_SIZE + PLAYER_RADIUS);
        assert!((body.position[1] - (floor + CELL_SIZE)).abs() < 0.005);
        assert!(!collides(&world, body.position));
    }

    #[test]
    fn falls_and_lands_without_penetration_then_jumps() {
        let world = World::new(1);
        let mut body = Body::new([0.25, 10.0, 0.25]);
        tick(&world, &mut body, MoveInput::default(), 180);
        assert!((body.position[1] - 2.5).abs() < 0.001);
        assert!(body.on_ground);
        assert!(!collides(&world, body.position));
        move_character(
            &world,
            &mut body,
            MoveInput {
                jump: true,
                ..Default::default()
            },
            0.1,
        );
        assert!(body.position[1] > 3.0);
        assert!(!body.on_ground);
        tick(&world, &mut body, MoveInput::default(), 180);
        assert!(body.on_ground);
        assert!((body.position[1] - 2.5).abs() < 0.001);
    }

    #[test]
    fn diagonal_walk_has_same_speed_as_straight_walk() {
        let world = World::new(1);
        let mut straight = Body::new(world.spawn_position());
        let mut diagonal = straight.clone();
        tick(
            &world,
            &mut straight,
            MoveInput {
                direction: [1.0, 0.0],
                ..Default::default()
            },
            30,
        );
        tick(
            &world,
            &mut diagonal,
            MoveInput {
                direction: [1.0, 1.0],
                ..Default::default()
            },
            30,
        );
        let distance = |body: &Body| {
            ((body.position[0] - 0.25).powi(2) + (body.position[2] - 0.25).powi(2)).sqrt()
        };
        assert!((distance(&straight) - distance(&diagonal)).abs() < 0.001);
    }

    #[test]
    fn wall_stops_sprint_and_half_meter_step_is_walkable() {
        let mut world = World::new(1);
        for z in -3..=3 {
            for y in 5..=11 {
                world
                    .set_block(BlockPos::new(5, y, z), Block::Brick)
                    .unwrap();
            }
        }
        let mut body = Body::new(world.spawn_position());
        tick(
            &world,
            &mut body,
            MoveInput {
                direction: [1.0, 0.0],
                sprint: true,
                ..Default::default()
            },
            60,
        );
        assert!((body.position[0] - (2.5 - PLAYER_RADIUS)).abs() < 0.001);
        assert!(!collides(&world, body.position));
        for z in -3..=3 {
            for y in 6..=11 {
                world.set_block(BlockPos::new(5, y, z), Block::Air).unwrap();
            }
        }
        tick(
            &world,
            &mut body,
            MoveInput {
                direction: [1.0, 0.0],
                ..Default::default()
            },
            10,
        );
        assert!(body.position[0] > 2.7);
        assert!(body.position[1] >= 3.0 - 0.001);
        assert!(!collides(&world, body.position));
    }

    #[test]
    fn jump_hits_ceiling_and_cannot_step_into_low_tunnel() {
        let mut world = World::new(1);
        for x in -2..=3 {
            for z in -2..=2 {
                world
                    .set_block(BlockPos::new(x, 9, z), Block::Brick)
                    .unwrap();
            }
        }
        let mut body = Body::new([0.25, 2.5, 0.25]);
        body.on_ground = true;
        tick(
            &world,
            &mut body,
            MoveInput {
                jump: true,
                ..Default::default()
            },
            5,
        );
        assert!(body.position[1] <= 4.5 - PLAYER_HEIGHT + 0.001);
        assert!(!collides(&world, body.position));
    }

    #[test]
    fn flight_is_bounded_and_invalid_numbers_are_neutralized() {
        let world = World::new(1);
        let mut body = Body::new([79.7, 77.0, 0.0]);
        tick(
            &world,
            &mut body,
            MoveInput {
                direction: [1.0, 1.0],
                fly: true,
                sprint: true,
                vertical: 1.0,
                ..Default::default()
            },
            120,
        );
        assert!(body.position[0] <= 80.0 - PLAYER_RADIUS);
        assert!(body.position[1] <= 80.0 - PLAYER_HEIGHT);
        body.velocity[1] = f32::NAN;
        move_character(
            &world,
            &mut body,
            MoveInput {
                direction: [f32::INFINITY, 0.0],
                ..Default::default()
            },
            0.016,
        );
        assert!(
            body.position
                .iter()
                .chain(body.velocity.iter())
                .all(|v| v.is_finite())
        );
        assert!((body.position[0] - 0.25).abs() < 0.001);
    }

    #[test]
    fn dug_column_is_a_real_hole_and_low_frame_rate_does_not_tunnel() {
        let mut world = World::new(1);
        for x in -1..=1 {
            for z in -1..=1 {
                for y in -4..=4 {
                    world.set_block(BlockPos::new(x, y, z), Block::Air).unwrap();
                }
            }
        }
        let mut body = Body::new([0.25, 15.0, 0.25]);
        for _ in 0..10 {
            move_character(&world, &mut body, MoveInput::default(), 0.25);
        }
        assert!((body.position[1] - -2.0).abs() < 0.001);
        assert!(body.on_ground);
        assert!(!collides(&world, body.position));
    }
}
