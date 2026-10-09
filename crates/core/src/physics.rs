//! Small shared character controller. The body's position is its foot center;
//! an upright AABB collides against the same edited voxels and other characters
//! on client and server.

use crate::airships::{
    AIRSHIP_DECK_HALF_LENGTH, AIRSHIP_DECK_HALF_WIDTH, AirshipNetwork, AirshipRide,
    AirshipSnapshot, deck_local_position, deck_position,
};
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
    /// A stalled canopy keeps its nose down until it regains flying speed.
    #[serde(default)]
    pub glide_stalled: bool,
}

impl Body {
    pub fn new(position: [f32; 3]) -> Self {
        Self {
            position,
            velocity: [0.0; 3],
            on_ground: false,
            glide_stalled: false,
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
    /// Camera forward vector while gliding. Ordinary ground movement ignores it.
    #[serde(default)]
    pub glide_direction: Option<[f32; 3]>,
}

/// Advances at most 250 ms, subdividing collision movement to avoid tunneling.
/// Non-finite input is neutralized and a corrupt body is returned to spawn.
pub fn move_character(world: &World, body: &mut Body, input: MoveInput, dt: f32) {
    move_character_with_obstacles(world, body, input, dt, &[]);
}

pub const GLIDE_STALL_SPEED: f32 = 10.0;
pub const GLIDE_RECOVERY_SPEED: f32 = 16.0;
pub const GLIDE_MAX_SPEED: f32 = 80.0;

pub fn flight_speed(velocity: [f32; 3]) -> f32 {
    velocity.iter().map(|v| v * v).sum::<f32>().sqrt()
}

/// Camera-directed, energy-based flight. Gravity gains speed in a dive and
/// spends it in a climb; drag dissipates energy. Low speed forces a nose-down
/// recovery until the wing can fly again. Heading follows the camera directly,
/// so turning never leaves the player travelling sideways across the view.
pub fn move_gliding_body(
    world: &World,
    body: &mut Body,
    input: MoveInput,
    dt: f32,
    obstacles: &[[f32; 3]],
) {
    if !dt.is_finite() || dt <= 0.0 {
        return;
    }
    if body
        .position
        .iter()
        .chain(&body.velocity)
        .any(|v| !v.is_finite())
    {
        *body = Body::new(world.spawn_position());
        return;
    }
    let dt = dt.min(0.25);
    let aim = input
        .glide_direction
        .filter(|aim| aim.iter().all(|v| v.is_finite()) && flight_speed(*aim) > 0.01);
    let forward = aim.unwrap_or(body.velocity);
    let length = forward[0].hypot(forward[2]);
    let heading = if length > 0.001 {
        [forward[0] / length, forward[2] / length]
    } else {
        [0.0, -1.0]
    };
    let camera_pitch = (-forward[1]).atan2(length);
    body.on_ground = false;
    let steps = (dt / (1.0 / 120.0)).ceil() as usize;
    let step_dt = dt / steps as f32;
    for _ in 0..steps {
        let speed = flight_speed(body.velocity).min(GLIDE_MAX_SPEED);
        if body.glide_stalled {
            body.glide_stalled = speed < GLIDE_RECOVERY_SPEED;
        } else {
            body.glide_stalled = speed < GLIDE_STALL_SPEED;
        }
        let pitch = if speed > 0.01 {
            (-body.velocity[1]).atan2(body.velocity[0].hypot(body.velocity[2]))
        } else {
            0.9
        };
        let drag = 0.1
            + 0.002 * speed * speed
            + if input.jump {
                1.8 + 0.006 * speed * speed
            } else {
                0.0
            };
        // Small trimmed descent sustains an ordinary glide. Fast pull-outs keep
        // their kinetic energy instead of snapping to a fixed cruise speed.
        let trim = if aim.is_some() {
            (drag / 14.0).min(0.10).asin()
        } else {
            0.0
        };
        let target = if body.glide_stalled {
            0.9
        } else {
            (camera_pitch + trim + if input.sprint { 0.35 } else { 0.0 }
                - if input.jump { 0.12 } else { 0.0 })
            .clamp(-0.65, 1.4)
        };
        let pitch = pitch + (target - pitch) * (1.0 - (-4.0 * step_dt).exp());
        let speed = (speed + (14.0 * pitch.sin() - drag) * step_dt).clamp(0.0, GLIDE_MAX_SPEED);
        body.velocity = [
            heading[0] * speed * pitch.cos(),
            -speed * pitch.sin(),
            heading[1] * speed * pitch.cos(),
        ];
        for axis in [0, 2, 1] {
            let amount = body.velocity[axis] * step_dt;
            let (terrain, character) =
                move_axis_with_obstacles(world, &mut body.position, axis, amount, obstacles);
            if terrain || character {
                body.velocity[axis] = 0.0;
                if axis == 1 && amount < 0.0 {
                    body.on_ground = true;
                    body.glide_stalled = false;
                }
            }
        }
        constrain_to_world(world, body);
    }
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
    move_character_with_surfaces(world, body, input, dt, obstacles, &[]);
}

/// The ordinary controller plus actual moving deck and landing-ramp contact.
/// `time` selects the current ship pose; callers advance the simulation clock.
#[allow(clippy::too_many_arguments)]
pub fn move_character_with_airships(
    world: &World,
    body: &mut Body,
    input: MoveInput,
    dt: f32,
    obstacles: &[[f32; 3]],
    network: &AirshipNetwork,
    time: f64,
    ride: &mut Option<AirshipRide>,
    local: &mut Option<[f32; 3]>,
) {
    if input.fly {
        *ride = None;
        *local = None;
    }
    if let (Some(rider), Some(offset)) = (*ride, *local) {
        if let Some(ship) = network.ship(rider.ship_id, time) {
            body.position = deck_position(&ship, offset);
        } else {
            *ride = None;
            *local = None;
        }
    }
    let ships = network.ships(time);
    let mut occupied = obstacles.to_vec();
    occupied.extend(
        ships
            .iter()
            .map(crate::airships::pilot_position)
            .filter(|p| {
                (p[0] - body.position[0]).abs() < 14.0 && (p[2] - body.position[2]).abs() < 14.0
            }),
    );
    let mut surfaces: Vec<_> = ships
        .iter()
        .filter(|s| {
            ride.is_some_and(|r| r.ship_id == s.id)
                || (s.position[0] - body.position[0]).abs() < 14.0
                    && (s.position[2] - body.position[2]).abs() < 14.0
        })
        .cloned()
        .map(AirshipSurface::Deck)
        .collect();
    surfaces.extend(
        network
            .ramps()
            .iter()
            .filter(|r| {
                let margin = r.width * 0.5 + 3.5;
                body.position[0] >= r.from[0].min(r.to[0]) - margin
                    && body.position[0] <= r.from[0].max(r.to[0]) + margin
                    && body.position[2] >= r.from[2].min(r.to[2]) - margin
                    && body.position[2] <= r.from[2].max(r.to[2]) + margin
            })
            .map(|r| AirshipSurface::Ramp {
                from: r.from,
                to: r.to,
                width: r.width,
            }),
    );
    move_character_with_surfaces(world, body, input, dt, &occupied, &surfaces);
    if let Some(rider) = *ride {
        if let Some(ship) = ships.iter().find(|s| s.id == rider.ship_id) {
            let offset = deck_local_position(ship, body.position);
            if deck_contains(ship, body.position) && offset[1] >= -0.02 {
                *local = Some(offset);
                return;
            }
        }
        *ride = None;
        *local = None;
    }
    if body.on_ground
        && !input.fly
        && let Some(ship) = ships.iter().find(|s| {
            deck_contains(s, body.position) && (body.position[1] - s.position[1]).abs() < 0.03
        })
    {
        *ride = Some(AirshipRide {
            ship_id: ship.id,
            seat: u8::MAX,
        });
        *local = Some(deck_local_position(ship, body.position));
    }
}

#[derive(Clone)]
enum AirshipSurface {
    Deck(AirshipSnapshot),
    Ramp {
        from: [f32; 3],
        to: [f32; 3],
        width: f32,
    },
}

fn deck_contains(ship: &AirshipSnapshot, position: [f32; 3]) -> bool {
    let p = deck_local_position(ship, position);
    p[0].abs() <= AIRSHIP_DECK_HALF_WIDTH && p[2].abs() <= AIRSHIP_DECK_HALF_LENGTH
}

fn deck_contact_requires_rotation_clearance(ship: &AirshipSnapshot, position: [f32; 3]) -> bool {
    let p = deck_local_position(ship, position);
    // Include walking and jumping from the pier, so an entrant cannot acquire
    // a contact gap that becomes an overlap when the attached deck turns.
    // The character sweep still excludes vertically separated bodies.
    p[1] >= -0.03
        && p[0].abs() <= AIRSHIP_DECK_HALF_WIDTH + 0.8
        && p[2].abs() <= AIRSHIP_DECK_HALF_LENGTH + 0.8
}

impl AirshipSurface {
    fn height(&self, p: [f32; 3]) -> Option<f32> {
        match self {
            Self::Deck(ship) => deck_contains(ship, p).then_some(ship.position[1]),
            Self::Ramp { from, to, width } => {
                let dx = to[0] - from[0];
                let dz = to[2] - from[2];
                let distance = (dx * dx + dz * dz).sqrt();
                if distance < 0.001 {
                    return None;
                }
                let along = ((p[0] - from[0]) * dx + (p[2] - from[2]) * dz) / (distance * distance);
                let across = ((p[0] - from[0]) * dz - (p[2] - from[2]) * dx).abs() / distance;
                ((0.0..=1.0).contains(&along) && across <= *width * 0.5)
                    .then_some(from[1] + (to[1] - from[1]) * along)
            }
        }
    }
    fn blocks(&self, p: [f32; 3]) -> bool {
        self.height(p)
            .is_some_and(|h| p[1] < h - contact_epsilon(h) && p[1] + PLAYER_HEIGHT > h - 0.35)
    }
}

fn move_character_with_surfaces(
    world: &World,
    body: &mut Body,
    input: MoveInput,
    dt: f32,
    obstacles: &[[f32; 3]],
    surfaces: &[AirshipSurface],
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
            let (terrain_blocked, character_blocked) = move_axis_with_surfaces(
                world,
                &mut body.position,
                axis,
                amount,
                obstacles,
                surfaces,
            );
            if terrain_blocked && !character_blocked && can_step {
                let mut raised = before;
                raised[1] += CELL_SIZE + contact_epsilon(before[1] + CELL_SIZE) * 2.0;
                // Check both upward clearance and the forward destination.
                if character_position_is_clear(world, raised, obstacles)
                    && !surfaces.iter().any(|s| s.blocks(raised))
                {
                    let (terrain_blocked, character_blocked) = move_axis_with_surfaces(
                        world,
                        &mut raised,
                        axis,
                        amount,
                        obstacles,
                        surfaces,
                    );
                    if !terrain_blocked && !character_blocked {
                        body.position = raised;
                    }
                }
            }
        }
        let vertical = body.velocity[1] * step;
        let previous_y = body.position[1];
        body.on_ground = false;
        let (terrain_blocked, character_blocked) =
            move_axis_with_obstacles(world, &mut body.position, 1, vertical, obstacles);
        if terrain_blocked || character_blocked {
            body.on_ground = vertical < 0.0;
            body.velocity[1] = 0.0;
        }
        for surface in surfaces {
            let Some(height) = surface.height(body.position) else {
                continue;
            };
            if vertical <= 0.0
                && previous_y >= height - CELL_SIZE - contact_epsilon(height)
                && body.position[1] <= height
                && previous_y + PLAYER_HEIGHT > height
            {
                body.position[1] = height;
                body.velocity[1] = 0.0;
                body.on_ground = true;
            } else if vertical > 0.0
                && previous_y + PLAYER_HEIGHT <= height - 0.35
                && body.position[1] + PLAYER_HEIGHT >= height - 0.35
            {
                body.position[1] = height - 0.35 - PLAYER_HEIGHT - contact_epsilon(height);
                body.velocity[1] = 0.0;
            }
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

/// Checks the same terrain/body bounds plus the deck and ramp slabs used by
/// ordinary movement. Ship envelopes are decorative and have no body collision.
pub fn character_position_is_clear_with_airships(
    world: &World,
    position: [f32; 3],
    obstacles: &[[f32; 3]],
    network: &AirshipNetwork,
    time: f64,
) -> bool {
    character_position_is_clear(world, position, obstacles)
        && !network
            .ships(time)
            .into_iter()
            .any(|ship| AirshipSurface::Deck(ship).blocks(position))
        && !network.ramps().iter().any(|ramp| {
            AirshipSurface::Ramp {
                from: ramp.from,
                to: ramp.to,
                width: ramp.width,
            }
            .blocks(position)
        })
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
    move_axis_with_surfaces(world, position, axis, amount, obstacles, &[])
}

fn move_axis_with_surfaces(
    world: &World,
    position: &mut [f32; 3],
    axis: usize,
    amount: f32,
    obstacles: &[[f32; 3]],
    surfaces: &[AirshipSurface],
) -> (bool, bool) {
    let before = *position;
    let mut terrain_blocked = move_axis(world, position, axis, amount);
    if axis != 1 {
        for surface in surfaces {
            if !surface.blocks(*position) {
                continue;
            }
            // Follow a shallow continuous landing ramp from actual contact.
            if let (Some(old_height), Some(height)) =
                (surface.height(before), surface.height(*position))
                && (before[1] - old_height).abs() < 0.03
                && height >= old_height
                && height - before[1] <= CELL_SIZE
            {
                let mut raised = *position;
                raised[1] = height;
                if character_position_is_clear(world, raised, obstacles)
                    && !surfaces.iter().any(|s| s.blocks(raised))
                {
                    *position = raised;
                    continue;
                }
            }
            if surface.blocks(before) {
                continue;
            }
            let end = *position;
            let mut low = 0.0;
            let mut high = 1.0;
            for _ in 0..14 {
                let mid = (low + high) * 0.5;
                let mut candidate = before;
                candidate[axis] = before[axis] + (end[axis] - before[axis]) * mid;
                if surface.blocks(candidate) {
                    high = mid;
                } else {
                    low = mid;
                }
            }
            position[axis] = before[axis] + (end[axis] - before[axis]) * low;
            terrain_blocked = true;
        }
    }
    let mut character_position = before;
    let mut allowed = position[axis] - before[axis];
    if axis != 1
        && surfaces.iter().any(|s| {
            matches!(s,AirshipSurface::Deck(ship) if deck_contact_requires_rotation_clearance(ship,before))
        })
    {
        allowed=passenger_motion(before,axis,allowed,obstacles);
    }
    let character_blocked =
        move_axis_against_characters(&mut character_position, axis, allowed, obstacles);
    position[axis] = character_position[axis];
    (terrain_blocked, character_blocked)
}

fn passenger_motion(position: [f32; 3], axis: usize, amount: f32, obstacles: &[[f32; 3]]) -> f32 {
    let mut allowed = amount;
    for other in obstacles {
        if other.iter().any(|v| !v.is_finite())
            || other[1] + PLAYER_HEIGHT <= position[1] + contact_epsilon(position[1])
            || position[1] + PLAYER_HEIGHT <= other[1] + contact_epsilon(other[1])
        {
            continue;
        }
        let perpendicular = if axis == 0 { 2 } else { 0 };
        let across = position[perpendicular] - other[perpendicular];
        if across.abs() >= 0.8 {
            continue;
        }
        let reach = (0.8_f32.powi(2) - across * across).sqrt();
        let boundary = other[axis] - allowed.signum() * reach;
        let skin = contact_epsilon(boundary);
        if allowed > 0.0
            && position[axis] <= boundary + skin
            && position[axis] + allowed >= boundary
        {
            allowed = (boundary - position[axis] - skin).max(0.0);
        } else if allowed < 0.0
            && position[axis] >= boundary - skin
            && position[axis] + allowed <= boundary
        {
            allowed = (boundary - position[axis] + skin).min(0.0);
        }
    }
    allowed
}

/// Swept character contact without terrain or gravity. Moving decks use this
/// same world-axis body shape as ordinary walking and prediction/replay.
pub fn move_axis_against_characters(
    position: &mut [f32; 3],
    axis: usize,
    amount: f32,
    obstacles: &[[f32; 3]],
) -> bool {
    let before = *position;
    position[axis] += amount;
    if amount == 0.0 {
        return false;
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
    character_blocked
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

    fn village_airships() -> &'static (World, AirshipNetwork) {
        static WORLD: std::sync::OnceLock<(World, AirshipNetwork)> = std::sync::OnceLock::new();
        WORLD.get_or_init(|| {
            let world = World::generate(42, crate::world::WorldGeneration::GeographyV3);
            let network = AirshipNetwork::new(&world);
            (world, network)
        })
    }

    #[test]
    fn walking_up_the_real_landing_ramp_boards_without_messages_or_teleporting() {
        let (world, network) = village_airships();
        let ship = network
            .ships(15.0)
            .into_iter()
            .find(|s| s.route_id == 0 && s.docked_at == Some(0))
            .unwrap();
        let path = network.landing_path(ship.id, 0).unwrap();
        let mut body = Body::new(path[0]);
        let mut ride = None;
        let mut local = None;
        let mut waypoint = 1;
        for _ in 0..10_000 {
            let target = path[waypoint];
            let dx = target[0] - body.position[0];
            let dz = target[2] - body.position[2];
            let distance = (dx * dx + dz * dz).sqrt();
            if distance < 0.10 {
                if waypoint == path.len() - 1 {
                    break;
                }
                waypoint += 1;
                continue;
            }
            let before = body.position;
            move_character_with_airships(
                world,
                &mut body,
                MoveInput {
                    direction: [dx / distance, dz / distance],
                    ..Default::default()
                },
                0.05,
                &[],
                network,
                15.0,
                &mut ride,
                &mut local,
            );
            let travelled = ((body.position[0] - before[0]).powi(2)
                + (body.position[2] - before[2]).powi(2))
            .sqrt();
            assert!(
                travelled <= 3.8 * 0.05 + 0.03,
                "boarding teleported by {travelled}"
            );
        }
        assert_eq!(
            waypoint,
            path.len() - 1,
            "walker did not reach the actual deck: {:?} target={:?}",
            body.position,
            path[waypoint]
        );
        assert!(body.on_ground);
        assert_eq!(ride.unwrap().ship_id, ship.id);
        assert_eq!(ride.unwrap().seat, u8::MAX);
        assert!(local.unwrap()[1].abs() < 0.03);
        assert!((body.position[1] - ship.position[1]).abs() < 0.03);
    }

    #[test]
    fn moving_deck_carries_normal_jumps_and_walking_or_jumping_off_releases_the_body() {
        let (world, network) = village_airships();
        let ship = network
            .ships(80.0)
            .into_iter()
            .find(|s| s.route_id == 0)
            .unwrap();
        let mut local = Some([0.0, 0.0, 0.0]);
        let mut ride = Some(AirshipRide {
            ship_id: ship.id,
            seat: u8::MAX,
        });
        let mut body = Body::new(deck_position(&ship, local.unwrap()));
        body.on_ground = true;
        move_character_with_airships(
            world,
            &mut body,
            MoveInput {
                jump: true,
                ..Default::default()
            },
            0.05,
            &[],
            network,
            80.0,
            &mut ride,
            &mut local,
        );
        assert!(!body.on_ground && body.velocity[1] > 0.0 && local.unwrap()[1] > 0.0);
        for step in 1..20 {
            move_character_with_airships(
                world,
                &mut body,
                MoveInput::default(),
                0.05,
                &[],
                network,
                80.0 + step as f64 * 0.05,
                &mut ride,
                &mut local,
            );
        }
        assert!(body.on_ground && ride.is_some() && local.unwrap()[1].abs() < 0.03);
        let end_ship = network.ship(ship.id, 81.0).unwrap();
        assert!((body.position[1] - end_ship.position[1]).abs() < 0.7);
        for tick in 0..60 {
            let current = network.ship(ship.id, 81.0 + tick as f64 * 0.05).unwrap();
            let (sin, cos) = current.yaw.sin_cos();
            move_character_with_airships(
                world,
                &mut body,
                MoveInput {
                    direction: [cos, -sin],
                    ..Default::default()
                },
                0.05,
                &[],
                network,
                81.0 + tick as f64 * 0.05,
                &mut ride,
                &mut local,
            );
            if ride.is_none() {
                break;
            }
        }
        assert!(
            ride.is_none() && local.is_none(),
            "walking over an open edge must detach"
        );
        for tick in 0..20 {
            move_character_with_airships(
                world,
                &mut body,
                MoveInput::default(),
                0.05,
                &[],
                network,
                84.0 + tick as f64 * 0.05,
                &mut ride,
                &mut local,
            );
        }
        assert!(!body.on_ground && body.velocity[1] < 0.0);

        let current = network.ship(ship.id, 90.0).unwrap();
        local = Some([AIRSHIP_DECK_HALF_WIDTH - 0.01, 0.0, 0.0]);
        ride = Some(AirshipRide {
            ship_id: ship.id,
            seat: u8::MAX,
        });
        body = Body::new(deck_position(&current, local.unwrap()));
        body.on_ground = true;
        let (sin, cos) = current.yaw.sin_cos();
        move_character_with_airships(
            world,
            &mut body,
            MoveInput {
                direction: [cos, -sin],
                jump: true,
                ..Default::default()
            },
            0.25,
            &[],
            network,
            90.0,
            &mut ride,
            &mut local,
        );
        assert!(ride.is_none() && !body.on_ground && body.velocity[1] > 0.0);
        assert!(body.position[1] > current.position[1]);
    }

    #[test]
    fn deck_contacts_keep_passengers_separate_when_the_ship_turns() {
        let (world, network) = village_airships();
        let mut ship = network
            .ships(15.0)
            .into_iter()
            .find(|s| s.route_id == 0 && s.docked_at == Some(0))
            .unwrap();
        let npc_local = [1.0, 0.0, 0.0];
        let mut body = Body::new(deck_position(&ship, [-1.0, 0.0, 0.0]));
        body.on_ground = true;
        let (sin, cos) = ship.yaw.sin_cos();
        let surfaces = [AirshipSurface::Deck(ship.clone())];
        for _ in 0..20 {
            let npc = deck_position(&ship, npc_local);
            move_character_with_surfaces(
                world,
                &mut body,
                MoveInput {
                    direction: [cos, -sin],
                    sprint: true,
                    ..Default::default()
                },
                0.05,
                &[npc],
                &surfaces,
            );
            assert!(!characters_overlap(body.position, npc));
        }
        let local = deck_local_position(&ship, body.position);
        for step in 0..72 {
            ship.yaw = step as f32 * std::f32::consts::TAU / 72.0;
            body.position = deck_position(&ship, local);
            let npc = deck_position(&ship, npc_local);
            assert!(
                !characters_overlap(body.position, npc),
                "rotation overlapped passengers at yaw={}",
                ship.yaw
            );
        }
    }

    #[test]
    fn entering_from_the_pier_preserves_clearance_before_the_ship_turns() {
        let (world, network) = village_airships();
        let id = 4_294_967_297;
        let base = network.ship(id, 0.0).unwrap();
        let quarter = std::f32::consts::FRAC_PI_2;
        let orthogonal = (base.yaw / quarter).ceil() * quarter;
        let time = ((orthogonal - base.yaw) / std::f32::consts::PI
            * crate::airships::AIRSHIP_TURN_SECONDS as f32) as f64;
        let ship = network.ship(id, time).unwrap();
        let (sin, cos) = ship.yaw.sin_cos();
        let input = MoveInput {
            direction: [cos, -sin],
            sprint: true,
            ..Default::default()
        };
        for jumping in [false, true] {
            let mut body = Body::new(deck_position(&ship, [-5.0, 0.0, 0.0]));
            body.on_ground = true;
            let mut ride = None;
            let mut local = None;
            let mut maximum_jump = 0.0_f32;
            for npc_local in [[-3.4, 0.0, 0.0], [-2.8, 0.0, 0.0]] {
                let npc = deck_position(&ship, npc_local);
                for step in 0..8 {
                    move_character_with_airships(
                        world,
                        &mut body,
                        MoveInput {
                            jump: jumping && npc_local[0] == -3.4 && step == 0,
                            ..input
                        },
                        0.1,
                        &[npc],
                        network,
                        time,
                        &mut ride,
                        &mut local,
                    );
                    maximum_jump = maximum_jump.max(body.position[1] - ship.position[1]);
                    let separation = ((body.position[0] - npc[0]).powi(2)
                        + (body.position[2] - npc[2]).powi(2))
                    .sqrt();
                    assert!(separation >= 0.8, "unsafe entering gap={separation}");
                    assert!(!characters_overlap(body.position, npc));
                }
                if npc_local[0] == -3.4 {
                    assert!(ride.is_none(), "the occupied entry must remain a queue");
                }
            }
            if jumping {
                assert!(maximum_jump > 0.5, "the entrant must make a real jump");
            }
            assert_eq!(ride.unwrap().ship_id, id);
            let turned = network.ship(id, time + 2.5).unwrap();
            assert!(!characters_overlap(
                deck_position(&turned, local.unwrap()),
                deck_position(&turned, [-2.8, 0.0, 0.0])
            ));
        }
    }

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

#[cfg(test)]
mod glide_tests {
    use super::*;
    use crate::world::WorldGeneration;

    fn world() -> World {
        World::generate(42, WorldGeneration::GeographyV3)
    }
    fn advance(world: &World, body: &mut Body, aim: [f32; 3], seconds: f32) {
        for _ in 0..(seconds / 0.02).round() as usize {
            move_gliding_body(
                world,
                body,
                MoveInput {
                    glide_direction: Some(aim),
                    ..Default::default()
                },
                0.02,
                &[],
            );
        }
    }
    #[test]
    fn camera_heading_controls_flight_without_movement_keys_or_sideways_drift() {
        let world = world();
        let mut body = Body::new([0.0, 2500.0, 0.0]);
        body.velocity = [0.0, -2.0, -20.0];
        move_gliding_body(
            &world,
            &mut body,
            MoveInput {
                // Opposite movement keys must not override the camera in flight.
                direction: [-1.0, 0.0],
                glide_direction: Some([1.0, 0.0, 0.0]),
                ..Default::default()
            },
            0.25,
            &[],
        );
        assert!(body.position[0] > 4.0);
        assert!(body.position[2].abs() < 0.001);
        assert!(body.velocity[0] > 19.0);
    }
    #[test]
    fn dive_pullout_and_climb_trade_height_for_speed_without_a_cruise_reset() {
        let world = world();
        let mut body = Body::new([0.0, 2500.0, 0.0]);
        body.velocity = [18.0, -1.0, 0.0];
        advance(&world, &mut body, [0.5, -0.866, 0.0], 3.0);
        let dive_speed = flight_speed(body.velocity);
        let dive_descent = -body.velocity[1];
        assert!(dive_speed > 35.0);
        advance(&world, &mut body, [1.0, 0.0, 0.0], 0.8);
        assert!(flight_speed(body.velocity) > dive_speed * 0.85);
        assert!(-body.velocity[1] < dive_descent * 0.4);
        let height = body.position[1];
        let speed = flight_speed(body.velocity);
        advance(&world, &mut body, [0.94, 0.342, 0.0], 1.0);
        assert!(body.position[1] > height);
        assert!(body.velocity[1] > 0.0);
        assert!(flight_speed(body.velocity) < speed);
    }
    #[test]
    fn a_slow_climb_stalls_lowers_the_nose_and_recovers_before_control_returns() {
        let world = world();
        let mut body = Body::new([0.0, 2500.0, 0.0]);
        body.velocity = [12.0, 0.0, 0.0];
        let mut stalled = false;
        let mut lowered = false;
        let mut recovered = false;
        for _ in 0..400 {
            advance(&world, &mut body, [0.866, 0.5, 0.0], 0.02);
            stalled |= body.glide_stalled;
            lowered |= body.glide_stalled && body.velocity[1] < -3.0;
            if stalled && !body.glide_stalled {
                assert!(flight_speed(body.velocity) >= GLIDE_RECOVERY_SPEED - 0.1);
                recovered = true;
                break;
            }
        }
        assert!(stalled && lowered && recovered);
    }
    #[test]
    fn brake_and_dive_change_pitch_and_energy_and_timestep_does_not_change_the_route() {
        let world = world();
        let mut cruise = Body::new([0.0, 2500.0, 0.0]);
        cruise.velocity = [25.0, -2.0, 0.0];
        let mut brake = cruise.clone();
        let mut dive = cruise.clone();
        let mut fine = cruise.clone();
        for _ in 0..50 {
            for (body, jump, sprint) in [
                (&mut cruise, false, false),
                (&mut brake, true, false),
                (&mut dive, false, true),
            ] {
                move_gliding_body(
                    &world,
                    body,
                    MoveInput {
                        glide_direction: Some([1.0, 0.0, 0.0]),
                        jump,
                        sprint,
                        ..Default::default()
                    },
                    0.02,
                    &[],
                );
            }
        }
        for _ in 0..200 {
            move_gliding_body(
                &world,
                &mut fine,
                MoveInput {
                    glide_direction: Some([1.0, 0.0, 0.0]),
                    ..Default::default()
                },
                0.005,
                &[],
            );
        }
        assert!(flight_speed(brake.velocity) < flight_speed(cruise.velocity) - 3.0);
        assert!(dive.velocity[1] < cruise.velocity[1] - 5.0);
        assert!(flight_speed(dive.velocity) > flight_speed(cruise.velocity) + 2.0);
        for axis in 0..3 {
            assert!((cruise.velocity[axis] - fine.velocity[axis]).abs() < 0.05);
            assert!((cruise.position[axis] - fine.position[axis]).abs() < 0.05);
        }
    }
}
