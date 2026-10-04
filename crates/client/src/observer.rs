//! Local camera motion for a read-only session. It has no character body,
//! collision, prediction, or movement messages on the wire.
use bevy::prelude::*;

const DEFAULT_SPEED: f32 = 12.0;
const BOOST: f32 = 5.0;

pub struct ObserverCamera {
    pub position: Vec3,
    pub speed: f32,
}

impl ObserverCamera {
    pub fn new(spawn: [f32; 3]) -> Self {
        Self {
            position: Vec3::from_array(spawn) + Vec3::new(0.0, 5.0, 6.0),
            speed: DEFAULT_SPEED,
        }
    }

    pub fn adjust_speed(&mut self, scroll: f32) {
        if scroll.is_finite() {
            self.speed = (self.speed * 1.25_f32.powf(scroll.clamp(-16.0, 16.0))).clamp(2.0, 64.0);
        }
    }

    pub fn speed(&self, boost: bool) -> f32 {
        self.speed * if boost { BOOST } else { 1.0 }
    }

    /// Local x = right, y = world up, z = forward along the view.
    pub fn advance(&mut self, input: Vec3, yaw: f32, pitch: f32, boost: bool, dt: f32) {
        if !input.is_finite() || !yaw.is_finite() || !pitch.is_finite() || !dt.is_finite() {
            return;
        }
        let right = Vec3::new(yaw.cos(), 0.0, yaw.sin());
        let direction = (right * input.x + Vec3::Y * input.y + forward(yaw, pitch) * input.z)
            .normalize_or_zero();
        self.position += direction * self.speed(boost) * dt.clamp(0.0, 0.25);
    }

    pub fn transform(&self, yaw: f32, pitch: f32) -> Transform {
        Transform::from_translation(self.position).looking_to(forward(yaw, pitch), Vec3::Y)
    }
}

fn forward(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(
        yaw.sin() * pitch.cos(),
        -pitch.sin(),
        -yaw.cos() * pitch.cos(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flies_in_view_direction_with_world_vertical_and_bounded_diagonal_speed() {
        let mut camera = ObserverCamera::new([0.0; 3]);
        let start = camera.position;
        camera.advance(Vec3::Z, std::f32::consts::FRAC_PI_2, -0.5, false, 0.1);
        assert!(camera.position.x > start.x && camera.position.y > start.y);
        assert!((camera.position.distance(start) - 1.2).abs() < 0.0001);
        let start = camera.position;
        camera.advance(Vec3::ONE, 0.0, 0.5, true, 0.1);
        assert!((camera.position.distance(start) - 6.0).abs() < 0.0001);
        let start = camera.position;
        camera.advance(Vec3::Y, 0.7, 0.5, false, 0.1);
        assert!((camera.position - start - Vec3::Y * 1.2).length() < 0.0001);
    }

    #[test]
    fn speed_and_stalled_frames_are_bounded_and_stationary_input_stops_immediately() {
        let mut camera = ObserverCamera::new([0.0; 3]);
        for _ in 0..10 {
            camera.adjust_speed(100.0);
        }
        assert_eq!(camera.speed, 64.0);
        let start = camera.position;
        camera.advance(Vec3::Z, 0.0, 0.0, true, 20.0);
        assert!((camera.position.distance(start) - 80.0).abs() < 0.0001);
        let stopped = camera.position;
        camera.advance(Vec3::ZERO, 0.0, 0.0, true, 0.1);
        camera.advance(Vec3::Z, 0.0, 0.0, true, f32::NAN);
        assert_eq!(camera.position, stopped);
        for _ in 0..10 {
            camera.adjust_speed(-100.0);
        }
        assert_eq!(camera.speed, 2.0);
        camera.adjust_speed(f32::NAN);
        assert_eq!(camera.speed, 2.0);
    }

    #[test]
    fn view_is_at_the_camera_position_and_reset_returns_to_the_valley() {
        let spawn = [0.25, 2.5, 0.25];
        let mut camera = ObserverCamera::new(spawn);
        camera.advance(Vec3::Z, 0.0, 0.0, true, 0.25);
        let view = camera.transform(0.4, 0.2);
        assert_eq!(view.translation, camera.position);
        assert!((view.forward().as_vec3() - forward(0.4, 0.2)).length() < 0.0001);
        camera = ObserverCamera::new(spawn);
        assert_eq!(camera.position, Vec3::new(0.25, 7.5, 6.25));
        assert_eq!(camera.speed, DEFAULT_SPEED);
    }
}
