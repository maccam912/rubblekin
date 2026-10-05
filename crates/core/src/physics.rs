//! Small shared character controller. The body's position is its foot center;
//! an upright AABB collides against the same edited voxels and other characters
//! on client and server.

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
    move_character_with_obstacles(world, body, input, dt, &[]);
}

/// Uses the same upright body for every embodied player and NPC. The caller
/// supplies other characters' foot positions, excluding this character itself.
/// Characters block movement without pushing each other or stepping over them.
pub fn move_character_with_obstacles(
    world: &World,
    body: &mut Body,
    input: MoveInput,
    dt: f32,
    obstacles: &[[f32; 3]],
) {
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
        resolve_character_overlaps(world, body, obstacles);
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

    if !obstacles.is_empty() && !resolve_character_overlaps(world, body, obstacles) {
        body.velocity = [0.0; 3];
        return;
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
            let (terrain_blocked, character_blocked) =
                move_axis_with_obstacles(world, &mut body.position, axis, amount, obstacles);
            if terrain_blocked && !character_blocked && can_step {
                let mut raised = before;
                raised[1] += CELL_SIZE + contact_epsilon(before[1] + CELL_SIZE) * 2.0;
                // Check both upward clearance and the forward destination.
                if character_position_is_clear(world, raised, obstacles) {
                    let (terrain_blocked, character_blocked) =
                        move_axis_with_obstacles(world, &mut raised, axis, amount, obstacles);
                    if !terrain_blocked && !character_blocked {
                        body.position = raised;
                    }
                }
            }
        }
        let vertical = body.velocity[1] * step;
        body.on_ground = false;
        let (terrain_blocked, character_blocked) =
            move_axis_with_obstacles(world, &mut body.position, 1, vertical, obstacles);
        if terrain_blocked || character_blocked {
            body.on_ground = vertical < 0.0;
            body.velocity[1] = 0.0;
        }
        constrain_to_world(world, body);
    }
}

/// Half-open AABB intersection with the controller's coordinate-aware skin.
/// Characters at different heights can pass above or below each other.
pub fn characters_overlap(a: [f32; 3], b: [f32; 3]) -> bool {
    if a.iter().chain(b.iter()).any(|value| !value.is_finite()) {
        return false;
    }
    (0..3).all(|axis| character_axis_overlaps(a, b, axis))
}

fn character_axis_overlaps(a: [f32; 3], b: [f32; 3], axis: usize) -> bool {
    let extent = if axis == 1 {
        PLAYER_HEIGHT
    } else {
        PLAYER_RADIUS * 2.0
    };
    let skin = contact_epsilon(a[axis]).max(contact_epsilon(b[axis]));
    (a[axis] - b[axis]).abs() < extent - skin
}

/// Checks edited terrain, world bounds, and all supplied character bodies.
pub fn character_position_is_clear(
    world: &World,
    position: [f32; 3],
    obstacles: &[[f32; 3]],
) -> bool {
    if position.iter().any(|value| !value.is_finite()) {
        return false;
    }
    let horizontal = world.radius_cells() as f32 * CELL_SIZE - PLAYER_RADIUS;
    position[0].abs() <= horizontal
        && position[2].abs() <= horizontal
        && position[1] >= world.min_y() as f32 * CELL_SIZE
        && position[1] <= world.max_y() as f32 * CELL_SIZE - PLAYER_HEIGHT
        && !collides(world, position)
        && !obstacles
            .iter()
            .any(|other| characters_overlap(position, *other))
}

/// Recovers older saves and coincident starts by moving the supplied body to
/// the nearest clear horizontal space within eight meters. Recovery never
/// crosses solid terrain or changes another body. A packed or enclosed space
/// returns false so the caller can reject a spawn or choose another location.
pub fn resolve_character_overlaps(world: &World, body: &mut Body, obstacles: &[[f32; 3]]) -> bool {
    if character_position_is_clear(world, body.position, obstacles) {
        return true;
    }
    let original = body.position;
    if original.iter().any(|value| !value.is_finite()) || collides(world, original) {
        return false;
    }
    let mut candidates = Vec::new();
    // Exact contact positions resolve ordinary overlaps with minimal movement.
    for other in obstacles {
        if other.iter().any(|value| !value.is_finite())
            || !character_axis_overlaps(original, *other, 1)
        {
            continue;
        }
        for axis in [0, 2] {
            for sign in [1.0, -1.0] {
                let mut candidate = original;
                let boundary = other[axis] + sign * PLAYER_RADIUS * 2.0;
                candidate[axis] = boundary + sign * contact_epsilon(boundary) * 2.0;
                candidates.push(candidate);
            }
        }
    }
    // A small deterministic lattice finds room for a group sharing an old
    // waypoint, including when walls rule out the nearest contact positions.
    let spacing =
        PLAYER_RADIUS * 2.0 + contact_epsilon(original[0]).max(contact_epsilon(original[2])) * 4.0;
    for x in -14..=14 {
        for z in -14..=14 {
            if x != 0 || z != 0 {
                candidates.push([
                    original[0] + x as f32 * spacing,
                    original[1],
                    original[2] + z as f32 * spacing,
                ]);
            }
        }
    }
    let distance_squared = |position: &[f32; 3]| {
        (position[0] - original[0]).powi(2) + (position[2] - original[2]).powi(2)
    };
    candidates.sort_by(|a, b| distance_squared(a).total_cmp(&distance_squared(b)));
    for candidate in candidates {
        if distance_squared(&candidate) > 8.0 * 8.0
            || !character_position_is_clear(world, candidate, obstacles)
            || !terrain_segment_is_clear(world, original, candidate)
        {
            continue;
        }
        body.position = candidate;
        body.velocity[0] = 0.0;
        body.velocity[2] = 0.0;
        return true;
    }
    false
}

fn terrain_segment_is_clear(world: &World, from: [f32; 3], to: [f32; 3]) -> bool {
    let distance = ((to[0] - from[0]).powi(2) + (to[2] - from[2]).powi(2)).sqrt();
    let steps = (distance / (CELL_SIZE * 0.5)).ceil() as usize;
    (1..=steps).all(|step| {
        let fraction = step as f32 / steps as f32;
        let position = std::array::from_fn(|axis| from[axis] + (to[axis] - from[axis]) * fraction);
        !collides(world, position)
    })
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

fn move_axis_with_obstacles(
    world: &World,
    position: &mut [f32; 3],
    axis: usize,
    amount: f32,
    obstacles: &[[f32; 3]],
) -> (bool, bool) {
    let before = *position;
    let terrain_blocked = move_axis(world, position, axis, amount);
    if amount == 0.0 {
        return (terrain_blocked, false);
    }
    let extent = if axis == 1 {
        PLAYER_HEIGHT
    } else {
        PLAYER_RADIUS * 2.0
    };
    let mut character_blocked = false;
    for other in obstacles {
        if other.iter().any(|value| !value.is_finite())
            || !(0..3)
                .filter(|other_axis| *other_axis != axis)
                .all(|other_axis| character_axis_overlaps(before, *other, other_axis))
        {
            continue;
        }
        // Sweep to the first contact, including a complete crossing in one
        // substep. This also handles vertical creative flight and hard falls.
        let boundary = other[axis] - amount.signum() * extent;
        let skin = contact_epsilon(boundary);
        let crossed = if amount > 0.0 {
            before[axis] <= boundary + skin && position[axis] >= boundary - skin
        } else {
            before[axis] >= boundary - skin && position[axis] <= boundary + skin
        };
        if crossed {
            character_blocked = true;
            if amount > 0.0 {
                position[axis] = position[axis].min(boundary - skin);
            } else {
                position[axis] = position[axis].max(boundary + skin);
            }
        }
    }
    (terrain_blocked, character_blocked)
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

    #[test]
    fn characters_stop_walking_sprinting_and_sliding_without_overlapping() {
        let world = World::new(1);
        let obstacle = [1.25, 2.5, 0.25];
        let mut body = Body::new(world.spawn_position());
        body.on_ground = true;
        for _ in 0..30 {
            move_character_with_obstacles(
                &world,
                &mut body,
                MoveInput {
                    direction: [1.0, 0.0],
                    sprint: true,
                    ..Default::default()
                },
                0.25,
                &[obstacle],
            );
            assert!(!characters_overlap(body.position, obstacle));
            assert!(!collides(&world, body.position));
        }
        assert!((body.position[0] - (obstacle[0] - PLAYER_RADIUS * 2.0)).abs() < 0.001);
        assert!((body.position[1] - obstacle[1]).abs() < 0.001);
        // Standing against a character does not invoke terrain step climbing.
        assert!(body.on_ground);
        for _ in 0..30 {
            move_character_with_obstacles(
                &world,
                &mut body,
                MoveInput {
                    direction: [1.0, 1.0],
                    ..Default::default()
                },
                1.0 / 60.0,
                &[obstacle],
            );
            assert!(!characters_overlap(body.position, obstacle));
        }
        assert!(body.position[2] > obstacle[2] + PLAYER_RADIUS * 2.0);
        assert!(body.position[0] > obstacle[0]);
    }

    #[test]
    fn swept_character_contact_catches_crossing_and_vertical_flight() {
        let world = World::new(1);
        let mut position = [0.25, 20.0, 0.25];
        let obstacle = [1.25, 20.0, 0.25];
        let (_, blocked) = move_axis_with_obstacles(&world, &mut position, 0, 4.0, &[obstacle]);
        assert!(blocked);
        assert!((position[0] - (obstacle[0] - PLAYER_RADIUS * 2.0)).abs() < 0.001);
        assert!(!characters_overlap(position, obstacle));

        let mut body = Body::new([0.25, 6.0, 0.25]);
        let above = [0.25, 10.0, 0.25];
        for _ in 0..5 {
            move_character_with_obstacles(
                &world,
                &mut body,
                MoveInput {
                    fly: true,
                    sprint: true,
                    vertical: 1.0,
                    ..Default::default()
                },
                0.25,
                &[above],
            );
            assert!(!characters_overlap(body.position, above));
        }
        assert!((body.position[1] - (above[1] - PLAYER_HEIGHT)).abs() < 0.001);

        let below = [0.25, 6.0, 0.25];
        body = Body::new([0.25, 15.0, 0.25]);
        body.velocity[1] = -40.0;
        for _ in 0..10 {
            move_character_with_obstacles(&world, &mut body, MoveInput::default(), 0.25, &[below]);
            assert!(!characters_overlap(body.position, below));
        }
        assert!((body.position[1] - (below[1] + PLAYER_HEIGHT)).abs() < 0.001);
        assert!(body.on_ground);
    }

    #[test]
    fn vertically_separated_characters_pass_without_blocking() {
        let world = World::new(1);
        let mut body = Body::new([0.25, 8.0, 0.25]);
        let below = [1.25, 2.5, 0.25];
        move_character_with_obstacles(
            &world,
            &mut body,
            MoveInput {
                fly: true,
                sprint: true,
                direction: [1.0, 0.0],
                ..Default::default()
            },
            0.25,
            &[below],
        );
        assert!(body.position[0] > below[0] + PLAYER_RADIUS * 2.0);
        assert!(!characters_overlap(body.position, below));
        assert!(!characters_overlap(
            below,
            [below[0], below[1] + PLAYER_HEIGHT, below[2]]
        ));
    }

    #[test]
    fn coincident_characters_recover_as_a_group_without_crossing_terrain() {
        let mut world = World::new(1);
        for y in 5..=10 {
            for z in -3..=3 {
                world
                    .set_block(BlockPos::new(2, y, z), Block::Brick)
                    .unwrap();
            }
        }
        let spawn = world.spawn_position();
        let mut positions = [spawn; 12];
        for index in 0..positions.len() {
            let obstacles: Vec<_> = positions
                .iter()
                .enumerate()
                .filter_map(|(other_index, position)| (other_index != index).then_some(*position))
                .collect();
            let mut body = Body::new(positions[index]);
            assert!(resolve_character_overlaps(&world, &mut body, &obstacles));
            assert!(character_position_is_clear(
                &world,
                body.position,
                &obstacles
            ));
            assert!(terrain_segment_is_clear(
                &world,
                positions[index],
                body.position
            ));
            positions[index] = body.position;
        }
        for (index, position) in positions.iter().enumerate() {
            assert!(!collides(&world, *position));
            for other in positions.iter().skip(index + 1) {
                assert!(!characters_overlap(*position, *other));
            }
        }
    }

    #[test]
    fn enclosed_overlap_reports_failure_without_teleporting_through_walls() {
        let mut world = World::new(1);
        // A one-meter interior leaves room for one body but cannot separate
        // two co-located bodies, even though the world outside is empty.
        for y in 5..=10 {
            for offset in -2..=1 {
                for position in [
                    BlockPos::new(-2, y, offset),
                    BlockPos::new(1, y, offset),
                    BlockPos::new(offset, y, -2),
                    BlockPos::new(offset, y, 1),
                ] {
                    world.set_block(position, Block::Brick).unwrap();
                }
            }
        }
        let position = [0.0, 2.5, 0.0];
        let mut body = Body::new(position);
        let obstacles = [position];
        assert!(!resolve_character_overlaps(&world, &mut body, &obstacles));
        assert_eq!(body.position, position);
        assert!(!collides(&world, body.position));
    }

    #[test]
    fn character_contact_at_kilometer_coordinates_stays_clear() {
        let world = World::generate(42, crate::world::WorldGeneration::GeographyV1);
        let mut body = Body::new([12000.25, 3000.0, -9000.25]);
        let obstacle = [12001.25, 3000.0, -9000.25];
        for _ in 0..30 {
            move_character_with_obstacles(
                &world,
                &mut body,
                MoveInput {
                    direction: [1.0, 0.0],
                    fly: true,
                    sprint: true,
                    ..Default::default()
                },
                0.25,
                &[obstacle],
            );
            assert!(!characters_overlap(body.position, obstacle));
            assert_eq!(body.position[1], obstacle[1]);
        }
        assert!((body.position[0] - (obstacle[0] - PLAYER_RADIUS * 2.0)).abs() < 0.01);
        assert!(character_position_is_clear(
            &world,
            body.position,
            &[obstacle]
        ));
    }

    #[test]
    fn world_boundary_and_zero_duration_recovery_preserve_character_clearance() {
        let world = World::new(1);
        let obstacle = [79.4, 50.0, 0.25];
        let mut body = Body::new([77.8, 50.0, 0.25]);
        for _ in 0..10 {
            move_character_with_obstacles(
                &world,
                &mut body,
                MoveInput {
                    fly: true,
                    sprint: true,
                    direction: [1.0, 0.0],
                    ..Default::default()
                },
                0.25,
                &[obstacle],
            );
            assert!(character_position_is_clear(
                &world,
                body.position,
                &[obstacle]
            ));
        }
        assert!((body.position[0] - (obstacle[0] - PLAYER_RADIUS * 2.0)).abs() < 0.001);

        body = Body::new(world.spawn_position());
        let coincident = body.position;
        move_character_with_obstacles(
            &world,
            &mut body,
            MoveInput::default(),
            0.0,
            &[coincident, [f32::NAN, 2.5, 0.25]],
        );
        assert!(character_position_is_clear(
            &world,
            body.position,
            &[coincident]
        ));
    }
}
