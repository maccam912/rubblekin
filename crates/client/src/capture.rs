//! Automatic scene captures wait for a joined world; F12 captures any view.
use crate::{GameEntity, Session};
use bevy::{
    app::AppExit,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};

const SCENE_SETTLE_SECONDS: f64 = 8.0;

#[derive(Default, PartialEq, Eq)]
enum AutomaticCapture {
    #[default]
    Waiting,
    Requested,
    Saved,
    Failed,
}

#[derive(Resource)]
pub(crate) struct Capture {
    path: Option<String>,
    exit_after: Option<f64>,
    scene_since: Option<f64>,
    automatic: AutomaticCapture,
    exit_sent: bool,
}

impl Capture {
    pub(crate) fn new(path: Option<String>, exit_after: Option<f32>) -> Self {
        Self {
            path,
            exit_after: exit_after.map(f64::from),
            scene_since: None,
            automatic: AutomaticCapture::Waiting,
            exit_sent: false,
        }
    }
}

pub(crate) fn capture_frame(
    mut commands: Commands,
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut capture: ResMut<Capture>,
    session: Option<Res<Session>>,
    cameras: Query<(), (With<GameEntity>, With<Camera3d>)>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs_f64();
    if capture.exit_sent {
        return;
    }
    // The explicit app deadline wins even if the scene becomes ready in this
    // frame. Do not queue an image that rendering cannot finish before exit.
    if capture.exit_after.is_some_and(|after| now >= after) {
        if let Some(path) = &capture.path
            && capture.automatic != AutomaticCapture::Saved
        {
            warn!(
                "Exit deadline reached without a saved automatic screenshot: {path}. Check capture errors or allow more time for joining, scene preparation and the 8-second settling interval."
            );
        }
        capture.exit_sent = true;
        exit.write(AppExit::Success);
        return;
    }
    if keys.just_pressed(KeyCode::F12) {
        let _ = std::fs::create_dir_all("artifacts");
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(format!(
                "artifacts/screenshot-{}.png",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            )));
    }
    if capture.path.is_none() || capture.automatic != AutomaticCapture::Waiting {
        return;
    }
    let Some(session) = session.filter(|_| !cameras.is_empty()) else {
        capture.scene_since = None;
        return;
    };
    if session.is_added() {
        capture.scene_since = Some(now);
    }
    let since = *capture.scene_since.get_or_insert(now);
    // Bevy discards duplicate screenshot targets in a frame without delivering
    // their observers. Let F12 own this frame and request the automatic image
    // on the next one, preserving its pending path and settling interval.
    if now - since < SCENE_SETTLE_SECONDS || keys.just_pressed(KeyCode::F12) {
        return;
    }
    let path = capture.path.clone().unwrap();
    capture.automatic = AutomaticCapture::Requested;
    info!(
        "Requesting visual capture: {:.1} FPS; {} explorers; {} terrain edits; NPC {}",
        session.fps,
        session.players.len(),
        session.edits,
        session.npc.action.label()
    );
    commands.spawn(Screenshot::primary_window()).observe(
        move |event: On<ScreenshotCaptured>, mut capture: ResMut<Capture>| {
            // Match Bevy's screenshot writer: remove HDR brightness alpha and
            // infer the image format from the requested extension. Report the
            // real disk result instead of calling a queued request a capture.
            let result = event
                .image
                .clone()
                .try_into_dynamic()
                .map_err(|error| error.to_string())
                .and_then(|image| {
                    image
                        .to_rgb8()
                        .save(&path)
                        .map_err(|error| error.to_string())
                });
            capture.automatic = match result {
                Ok(()) => {
                    info!("Screenshot saved to {path}");
                    AutomaticCapture::Saved
                }
                Err(error) => {
                    error!("Cannot save automatic screenshot to {path}: {error}");
                    AutomaticCapture::Failed
                }
            };
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::GraphicsQuality;
    use rubblekin_core::protocol::SessionMode;
    use std::time::Duration;

    fn app(exit_after: Option<f32>) -> App {
        let mut app = App::new();
        app.insert_resource(Capture::new(
            Some("unused-capture-test.png".into()),
            exit_after,
        ))
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_message::<AppExit>()
        .add_systems(Update, capture_frame);
        app
    }

    fn join(app: &mut App) -> Entity {
        let (_, session) = crate::join::session_from_welcome(
            crate::join::tests::welcome(SessionMode::Player),
            "capture test".into(),
            GraphicsQuality::Low,
            0.,
            SessionMode::Player,
        )
        .unwrap();
        app.insert_resource(session);
        app.world_mut()
            .spawn((GameEntity, Camera3d::default()))
            .id()
    }

    fn advance(app: &mut App, seconds: u64) {
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_secs(seconds));
        app.update();
    }

    fn requests(app: &mut App) -> usize {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Screenshot>>()
            .iter(world)
            .count()
    }

    fn deliver_image(app: &mut App) {
        use bevy::{
            asset::RenderAssetUsages,
            render::render_resource::{Extent3d, TextureDimension, TextureFormat},
        };
        let world = app.world_mut();
        let entity = world
            .query_filtered::<Entity, With<Screenshot>>()
            .single(world)
            .unwrap();
        world.trigger(ScreenshotCaptured {
            entity,
            image: Image::new_fill(
                Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                &[32, 64, 128, 255],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            ),
        });
    }

    #[test]
    fn automatic_capture_waits_for_a_delayed_world_then_fires_once_across_rejoin() {
        let mut app = app(None);
        advance(&mut app, 30);
        assert_eq!(
            requests(&mut app),
            0,
            "loading time is not scene settling time"
        );
        let camera = join(&mut app);
        app.update();
        advance(&mut app, 7);
        assert_eq!(requests(&mut app), 0);
        advance(&mut app, 1);
        assert_eq!(requests(&mut app), 1);
        app.world_mut().remove_resource::<Session>();
        app.world_mut().despawn(camera);
        advance(&mut app, 20);
        join(&mut app);
        app.update();
        advance(&mut app, 20);
        assert_eq!(
            requests(&mut app),
            1,
            "automatic capture is once per launch"
        );
    }

    #[test]
    fn leaving_before_capture_restarts_the_settling_interval() {
        let mut app = app(None);
        let camera = join(&mut app);
        app.update();
        advance(&mut app, 7);
        app.world_mut().remove_resource::<Session>();
        app.world_mut().despawn(camera);
        advance(&mut app, 20);
        assert_eq!(requests(&mut app), 0);
        join(&mut app);
        app.update();
        advance(&mut app, 7);
        assert_eq!(requests(&mut app), 0);
        advance(&mut app, 1);
        assert_eq!(requests(&mut app), 1);
    }

    #[test]
    fn absolute_exit_deadline_wins_before_a_simultaneous_capture() {
        for joined in [false, true] {
            let mut app = app(Some(8.));
            if joined {
                join(&mut app);
            }
            app.update();
            advance(&mut app, 8);
            assert_eq!(requests(&mut app), 0);
            assert!(app.world().resource::<Capture>().exit_sent);
            assert_eq!(app.world().resource::<Messages<AppExit>>().len(), 1);
        }
    }

    #[test]
    fn automatic_capture_records_completion_only_after_writing_the_delivered_image() {
        let path = std::env::temp_dir().join(format!(
            "rubblekin-scene-capture-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut app = app(None);
        app.world_mut().resource_mut::<Capture>().path = Some(path.to_string_lossy().into());
        join(&mut app);
        app.update();
        advance(&mut app, 8);
        assert!(app.world().resource::<Capture>().automatic == AutomaticCapture::Requested);
        assert!(!path.exists());
        deliver_image(&mut app);
        assert!(app.world().resource::<Capture>().automatic == AutomaticCapture::Saved);
        assert!(
            std::fs::read(&path)
                .unwrap()
                .starts_with(b"\x89PNG\r\n\x1a\n")
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_failed_capture_is_not_marked_saved_or_retried() {
        let directory = std::env::temp_dir().join(format!(
            "rubblekin-missing-capture-dir-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = directory.join("capture.png");
        let mut app = app(Some(10.));
        app.world_mut().resource_mut::<Capture>().path = Some(path.to_string_lossy().into());
        join(&mut app);
        app.update();
        advance(&mut app, 8);
        deliver_image(&mut app);
        assert!(app.world().resource::<Capture>().automatic == AutomaticCapture::Failed);
        assert!(!path.exists());
        advance(&mut app, 1);
        assert_eq!(
            requests(&mut app),
            1,
            "failed requests still fire only once"
        );
        advance(&mut app, 1);
        assert!(app.world().resource::<Capture>().exit_sent);
    }

    #[test]
    fn f12_captures_the_menu_without_waiting_for_an_automatic_world_capture() {
        let mut app = app(None);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F12);
        app.update();
        assert_eq!(requests(&mut app), 1);
        assert!(app.world().resource::<Capture>().scene_since.is_none());
        assert!(app.world().resource::<Capture>().automatic == AutomaticCapture::Waiting);
    }

    #[test]
    fn f12_defers_a_ready_automatic_capture_to_avoid_a_duplicate_render_target() {
        let mut app = app(None);
        join(&mut app);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F12);
        advance(&mut app, 8);
        assert_eq!(
            requests(&mut app),
            1,
            "only the manual request targets this frame"
        );
        assert!(app.world().resource::<Capture>().automatic == AutomaticCapture::Waiting);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        assert_eq!(
            requests(&mut app),
            2,
            "automatic capture is queued in the next frame"
        );
        assert!(app.world().resource::<Capture>().automatic == AutomaticCapture::Requested);
    }
}
