//! Smooth camera follow without delaying character movement or mouse look.
use bevy::prelude::*;
use rubblekin_core::{
    airships::{AirshipSnapshot, deck_local_position, deck_position},
    world::World,
};

// Reach 95% of a terrain step in about 170 ms, independently of frame rate.
const FOLLOW_RATE: f32 = 18.0;
const SNAP_DISTANCE: f32 = 4.0;
const TERRAIN_CLEARANCE: f32 = 0.2;

#[derive(Component, Default)]
pub struct CameraFollow {
    // World coordinates on foot, deck coordinates while following this ship.
    eye: Option<Vec3>,
    ship_id: Option<u64>,
}

impl CameraFollow {
    pub fn advance(&mut self, target: Vec3, dt: f32) -> Vec3 {
        if self.ship_id.take().is_some() {
            self.eye = None;
        }
        self.advance_eye(target, dt)
    }

    /// Carry the camera with the deck and smooth only the passenger's motion.
    /// World-space lag can cross the correction snap threshold every few
    /// frames when a fast ship carries a low-frame-rate client.
    pub fn advance_on_airship(&mut self, target: Vec3, dt: f32, ship: &AirshipSnapshot) -> Vec3 {
        if self.ship_id != Some(ship.id) {
            self.eye = None;
            self.ship_id = Some(ship.id);
        }
        let local_target = Vec3::from_array(deck_local_position(ship, target.to_array()));
        let local_eye = self.advance_eye(local_target, dt);
        Vec3::from_array(deck_position(ship, local_eye.to_array()))
    }

    fn advance_eye(&mut self, target: Vec3, dt: f32) -> Vec3 {
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

    fn ship() -> AirshipSnapshot {
        AirshipSnapshot {
            id: 1,
            route_id: 0,
            position: [0.0, 100.0, 0.0],
            yaw: 0.0,
            from_village: 0,
            next_village: 1,
            docked_at: None,
            departure_in: 0.0,
            arrival_in: 30.0,
            pilot_name: "Pilot".into(),
        }
    }

    fn ship_eye(ship: &AirshipSnapshot, local: Vec3) -> Vec3 {
        Vec3::from_array(deck_position(ship, local.to_array()))
    }

    #[test]
    fn fast_airship_cruise_has_uniform_camera_motion_even_at_low_frame_rates() {
        for fps in [15, 20, 60] {
            let dt = 1.0 / fps as f32;
            let mut ship = ship();
            let local = Vec3::new(2.0, 1.6, 3.0);
            let mut follow = CameraFollow::default();
            let mut previous = follow.advance_on_airship(ship_eye(&ship, local), 0.0, &ship);
            for frame in 1..=fps * 4 {
                ship.position[0] = 48.0 * frame as f32 * dt;
                let target = ship_eye(&ship, local);
                let anchor = follow.advance_on_airship(target, dt, &ship);
                assert!(anchor.distance(target) < 0.0001, "{fps} FPS lost the deck");
                assert!(
                    (anchor - previous - Vec3::X * 48.0 * dt).length() < 0.0001,
                    "{fps} FPS camera motion was not uniform at frame {frame}"
                );
                previous = anchor;
            }
        }
    }

    #[test]
    fn turning_and_climbing_airship_preserves_deck_walk_smoothing() {
        for fps in [15, 20, 60] {
            let dt = 1.0 / fps as f32;
            let mut ship = ship();
            let mut follow = CameraFollow::default();
            follow.advance_on_airship(ship_eye(&ship, Vec3::Y * 1.6), 0.0, &ship);
            for frame in 1..=fps * 2 {
                let elapsed = frame as f32 * dt;
                ship.position = [48.0 * elapsed, 100.0 + 12.0 * elapsed, 0.0];
                ship.yaw = std::f32::consts::FRAC_PI_2 * elapsed / 2.0;
                let local = Vec3::new(elapsed, 1.6, 0.0);
                let anchor = follow.advance_on_airship(ship_eye(&ship, local), dt, &ship);
                let actual_local = Vec3::from_array(deck_local_position(&ship, anchor.to_array()));
                // A one-metre/second deck walk retains ordinary follow lag;
                // ship translation and rotation contribute none of their own.
                let decay = (-FOLLOW_RATE * dt).exp();
                let lag = dt * decay * (1.0 - decay.powi(frame)) / (1.0 - decay);
                assert!(
                    actual_local.distance(local - Vec3::X * lag) < 0.0001,
                    "{fps} FPS changed deck follow while climbing or turning"
                );
            }
        }
    }

    #[test]
    fn boarding_leaving_and_changing_airships_reset_the_follow_anchor() {
        let mut follow = CameraFollow::default();
        let mut ship = ship();
        let start = eye();
        follow.advance(start, 0.0);
        assert_ne!(follow.advance(start + Vec3::X, 0.01), start + Vec3::X);
        let boarded = ship_eye(&ship, Vec3::new(2.0, 1.6, 0.0));
        assert_eq!(follow.advance_on_airship(boarded, 0.01, &ship), boarded);
        let walk = boarded + Vec3::X;
        assert_ne!(follow.advance_on_airship(walk, 0.01, &ship), walk);
        ship.id = 2;
        assert_eq!(follow.advance_on_airship(walk, 0.01, &ship), walk);
        assert_ne!(
            follow.advance_on_airship(walk + Vec3::X, 0.01, &ship),
            walk + Vec3::X
        );
        let landed = walk + Vec3::new(0.5, -0.5, 0.0);
        assert_eq!(follow.advance(landed, 0.01), landed);
        let step = landed + Vec3::Y * CELL_SIZE;
        let anchor = follow.advance(step, 0.01);
        assert!(anchor.y > landed.y && anchor.y < step.y);
    }

    #[test]
    fn large_passenger_corrections_still_snap_in_deck_coordinates() {
        let mut ship = ship();
        let mut follow = CameraFollow::default();
        follow.advance_on_airship(ship_eye(&ship, Vec3::Y * 1.6), 0.0, &ship);
        ship.position[0] = 48.0;
        let corrected = ship_eye(&ship, Vec3::new(5.0, 1.6, 0.0));
        assert_eq!(follow.advance_on_airship(corrected, 0.1, &ship), corrected);
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
