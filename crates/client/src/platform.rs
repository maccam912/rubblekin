//! Platform defaults stay outside gameplay and the shared protocol.
use crate::graphics::GraphicsQuality;
use bevy::{prelude::*, window::WindowResolution};

pub fn frame_time(mut strategy: ResMut<bevy::time::TimeUpdateStrategy>) {
    // Render timestamps can arrive late, then release several long deltas in
    // quick succession. Movement commands must spend actual client frame time
    // to stay within the authoritative server's real-time allowance.
    *strategy = bevy::time::TimeUpdateStrategy::ManualInstant(bevy::platform::time::Instant::now());
}

pub fn prepare_data_directory() -> std::io::Result<()> {
    #[cfg(target_os = "android")]
    {
        let app = bevy::android::ANDROID_APP
            .get()
            .ok_or_else(|| std::io::Error::other("Android activity is unavailable"))?;
        let directory = app
            .internal_data_path()
            .ok_or_else(|| std::io::Error::other("Android private storage is unavailable"))?;
        std::fs::create_dir_all(&directory)?;
        // Set once before starting workers: local saves and screenshots belong
        // to this app's private storage and survive ordinary APK updates.
        std::env::set_current_dir(directory)?;
    }
    Ok(())
}

pub fn default_graphics() -> GraphicsQuality {
    if cfg!(target_os = "android") {
        GraphicsQuality::Low
    } else {
        GraphicsQuality::default()
    }
}

pub fn window(touch_preview: bool) -> Window {
    let mut window = Window {
        title: "Rubblekin · Mountains and valleys".into(),
        present_mode: bevy::window::PresentMode::AutoVsync,
        ..default()
    };
    if cfg!(target_os = "android") {
        // Java's immersive UI hides system bars. Winit fullscreen sets
        // FLAG_FULLSCREEN, which prevents Android from resizing for the IME.
        // Keep native density so touch targets use logical pixels.
    } else {
        let (width, height) = if touch_preview {
            (840, 400)
        } else {
            (1440, 900)
        };
        window.resolution = WindowResolution::new(width, height).with_scale_factor_override(1.0);
    }
    window
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffered_render_timestamps_do_not_create_bursts_of_movement_time() {
        let (sender, receiver) = bevy::time::create_time_channels();
        let mut app = App::new();
        app.add_plugins(bevy::time::TimePlugin)
            .insert_resource(receiver)
            .add_systems(First, frame_time.before(bevy::time::TimeSystems));
        app.update();
        let now = bevy::platform::time::Instant::now();
        for seconds in [3, 4] {
            sender
                .0
                .send(now + std::time::Duration::from_secs(seconds))
                .unwrap();
            app.update();
            let bevy::time::TimeUpdateStrategy::ManualInstant(instant) =
                app.world().resource::<bevy::time::TimeUpdateStrategy>()
            else {
                panic!("client frames must supply their own timestamp");
            };
            assert_eq!(
                app.world().resource::<Time<Real>>().last_update(),
                Some(*instant)
            );
        }
    }
}
