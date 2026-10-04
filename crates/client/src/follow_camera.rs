//! Smooth camera follow without delaying character movement or mouse look.
use bevy::prelude::*;
use rubblekin_core::world::World;

// Reach 95% of a terrain step in about 170 ms, independently of frame rate.
const FOLLOW_RATE: f32 = 18.0;
const SNAP_DISTANCE: f32 = 4.0;
const TERRAIN_CLEARANCE: f32 = 0.2;

#[derive(Component, Default)]
pub struct CameraFollow {
    eye: Option<Vec3>,
}

impl CameraFollow {
    pub fn advance(&mut self, target: Vec3, dt: f32) -> Vec3 {
        let eye = match self.eye {
            Some(eye) if eye.distance_squared(target) <= SNAP_DISTANCE * SNAP_DISTANCE => {
                eye.lerp(target, 1.0 - (-FOLLOW_RATE * dt.max(0.0)).exp())
            }
            // Start at the joined player, and don't sweep across a respawn/correction.
            _ => target,
        };
        self.eye = Some(eye);
        eye
    }
}

pub fn transform(
    world: &World,
    eye: Vec3,
    follow_eye: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
) -> Transform {
    let shoulder = Vec3::new(yaw.cos(), 0.0, yaw.sin()) * 0.8;
    let forward = Vec3::new(
        yaw.sin() * pitch.cos(),
        -pitch.sin(),
        -yaw.cos() * pitch.cos(),
    );
    let desired = follow_eye + shoulder - forward * distance;
    let offset = desired - eye;
    let length = offset.length();
    let direction = offset.normalize_or_zero();
    // Check the final smoothed candidate from the actual eye. A lagged follow
    // point can be behind a wall; lerping a collision-corrected view is unsafe.
    let position = world
        .raycast(eye.to_array(), direction.to_array(), length)
        .map(|hit| eye + direction * (hit.distance - TERRAIN_CLEARANCE).max(0.0))
        .unwrap_or(desired);
    Transform::from_translation(position).looking_to(forward, Vec3::Y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::world::{Block, BlockPos, CELL_SIZE};

    fn eye() -> Vec3 {
        // Open air above the test world's terrain and trees.
        Vec3::new(0.25, 20.25, 0.25)
    }

    #[test]
    fn terrain_step_moves_camera_partway_then_settles_without_overshoot() {
        let world = World::new(1);
        let mut follow = CameraFollow::default();
        let start = eye();
        let anchor = follow.advance(start, 1.0 / 60.0);
        let initial_view = transform(&world, start, anchor, 0.0, 0.0, 6.5);
        let stepped = start + Vec3::Y * CELL_SIZE;
        let anchor = follow.advance(stepped, 1.0 / 60.0);
        let view = transform(&world, stepped, anchor, 0.0, 0.0, 6.5);
        assert!(view.translation.y > initial_view.translation.y);
        assert!(view.translation.y < initial_view.translation.y + CELL_SIZE);
        let mut previous = anchor.y;
        for _ in 0..30 {
            let anchor = follow.advance(stepped, 1.0 / 60.0);
            assert!(anchor.y >= previous && anchor.y <= stepped.y);
            previous = anchor.y;
        }
        assert!((previous - stepped.y).abs() < 0.001);
    }

    #[test]
    fn follow_settles_consistently_at_different_frame_rates() {
        let start = eye();
        let target = start + Vec3::new(1.0, CELL_SIZE, -0.5);
        let settle = |fps: u32| {
            let mut follow = CameraFollow::default();
            follow.advance(start, 0.0);
            let mut anchor = start;
            for _ in 0..fps / 2 {
                anchor = follow.advance(target, 1.0 / fps as f32);
            }
            anchor
        };
        assert!(settle(30).distance(settle(144)) < 0.00002);
        assert!(settle(30).distance(target) < 0.001);
    }

    #[test]
    fn new_camera_and_large_corrections_start_at_the_target() {
        let mut follow = CameraFollow::default();
        assert_eq!(follow.advance(eye(), 0.0), eye());
        let relocated = eye() + Vec3::X * 12.0;
        assert_eq!(follow.advance(relocated, 1.0 / 60.0), relocated);
        let mut rejoined = CameraFollow::default();
        assert_eq!(rejoined.advance(eye(), 1.0 / 60.0), eye());
    }

    #[test]
    fn look_and_zoom_are_immediate_while_follow_is_still_settling() {
        let world = World::new(1);
        let mut follow = CameraFollow::default();
        follow.advance(eye(), 0.0);
        let target = eye() + Vec3::Y * CELL_SIZE;
        let anchor = follow.advance(target, 1.0 / 60.0);
        let yaw = std::f32::consts::FRAC_PI_2;
        let pitch: f32 = 0.4;
        let forward = Vec3::new(pitch.cos(), -pitch.sin(), 0.0);
        let pivot = anchor + Vec3::Z * 0.8;
        for distance in [6.5, 1.0] {
            let view = transform(&world, target, anchor, yaw, pitch, distance);
            assert!(view.forward().as_vec3().distance(forward) < 0.00001);
            assert!(view.translation.distance(pivot - forward * distance) < 0.00001);
        }
    }

    #[test]
    fn newly_placed_wall_clips_the_final_smoothed_view_immediately() {
        let mut world = World::new(1);
        let mut follow = CameraFollow::default();
        follow.advance(eye(), 0.0);
        let target = eye() + Vec3::Y * CELL_SIZE;
        let anchor = follow.advance(target, 1.0 / 60.0);
        assert!(
            transform(&world, target, anchor, 0.0, 0.0, 6.5)
                .translation
                .z
                > 6.0
        );
        for x in 0..=3 {
            for y in 39..=43 {
                world
                    .set_block(BlockPos::new(x, y, 6), Block::Brick)
                    .unwrap();
            }
        }
        let view = transform(&world, target, anchor, 0.0, 0.0, 6.5);
        assert!(view.translation.z < 3.0);
        let offset = view.translation - target;
        assert!(
            world
                .raycast(target.to_array(), offset.to_array(), offset.length())
                .is_none()
        );
    }

    #[test]
    fn lagged_follow_point_on_other_side_of_wall_does_not_pull_view_through_it() {
        let mut world = World::new(1);
        for y in 39..=43 {
            for z in -1..=16 {
                world
                    .set_block(BlockPos::new(0, y, z), Block::Brick)
                    .unwrap();
            }
        }
        let mut follow = CameraFollow::default();
        follow.advance(eye() - Vec3::X, 0.0);
        let target = eye() + Vec3::X * 0.5;
        let anchor = follow.advance(target, 0.01);
        let view = transform(&world, target, anchor, 0.0, 0.0, 6.5);
        assert!(view.translation.x > CELL_SIZE);
        let offset = view.translation - target;
        assert!(
            world
                .raycast(target.to_array(), offset.to_array(), offset.length())
                .is_none()
        );
    }
}
