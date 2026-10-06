//! Run against a disposable/local DSN to verify the real reporting path without a window.
#[path = "../src/crash_reporting.rs"]
mod crash_reporting;

fn main() {
    let _guard = crash_reporting::init();
    crash_reporting::context("verification", "Low");
    match std::env::args().nth(1).as_deref() {
        Some("panic") => panic!("Rubblekin intentional crash-report verification"),
        Some("abort") => std::process::abort(),
        Some("renderer") => {
            use bevy::{
                app::AppExit,
                prelude::*,
                render::error_handler::{ErrorType, RenderError},
            };
            let mut app = App::new();
            app.add_message::<AppExit>()
                .init_resource::<crash_reporting::RendererFailureReported>();
            let mut render_world = World::new();
            let error = RenderError {
                ty: ErrorType::DeviceLost,
                description: "Intentional renderer-report verification".into(),
                source: None,
            };
            // Bevy may poll the failed renderer repeatedly before it exits.
            for _ in 0..2 {
                crash_reporting::renderer_error(&error, app.world_mut(), &mut render_world);
            }
        }
        _ => crash_reporting::startup_error(&std::io::Error::other(
            "Rubblekin intentional startup-error verification",
        )),
    }
}
