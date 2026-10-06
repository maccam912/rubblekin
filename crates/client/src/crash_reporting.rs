//! Sentry is optional until a project DSN is configured. Initialize before Bevy/workers.

use bevy::{
    app::AppExit,
    prelude::*,
    render::{
        error_handler::{RenderError, RenderErrorPolicy},
        renderer::RenderAdapterInfo,
    },
};

#[derive(Resource, Default)]
pub struct RendererFailureReported(bool);

pub fn renderer_error(
    error: &RenderError,
    main_world: &mut World,
    render_world: &mut World,
) -> RenderErrorPolicy {
    // Bevy polls this handler again until the app exits. Report the original
    // failure once, before renderer teardown can panic and obscure its cause.
    let mut reported = main_world.resource_mut::<RendererFailureReported>();
    if !reported.0 {
        reported.0 = true;
        let adapter = render_world
            .get_resource::<RenderAdapterInfo>()
            .map(|info| {
                format!(
                    "{} ({:?}), driver {} {}",
                    info.name, info.backend, info.driver, info.driver_info
                )
            })
            .unwrap_or_else(|| "unknown GPU".into());
        let message = format!(
            "Renderer {:?}: {} [GPU: {adapter}]",
            error.ty, error.description
        );
        error!("{message}");
        startup_error(&std::io::Error::other(message));
        main_world.write_message(AppExit::error());
    }
    RenderErrorPolicy::StopRendering
}

#[cfg(not(target_os = "android"))]
pub fn init() -> Option<sentry::ClientInitGuard> {
    use std::time::Duration;
    let dsn = std::env::var("SENTRY_DSN")
        .ok()
        .or_else(|| option_env!("SENTRY_DSN").map(str::to_owned))?;
    if dsn.trim().is_empty() {
        return None;
    }
    let dsn = match dsn.parse::<sentry::types::Dsn>() {
        Ok(dsn) => dsn,
        Err(_) => {
            eprintln!("Rubblekin: invalid SENTRY_DSN; crash reporting is disabled");
            return None;
        }
    };
    let environment = std::env::var("SENTRY_ENVIRONMENT")
        .ok()
        .or_else(|| option_env!("SENTRY_ENVIRONMENT").map(str::to_owned))
        .unwrap_or_else(|| "development".into());
    let mut options = sentry::ClientOptions::default();
    options.dsn = Some(dsn);
    let guard = sentry::init(
        options
            .release(format!("rubblekin@{}", env!("RUBBLEKIN_BUILD_COMMIT")))
            .environment(environment)
            .send_default_pii(false)
            .shutdown_timeout(Duration::from_secs(2))
            .before_send(|mut event| {
                // Hostnames/usernames aren't needed to diagnose a game crash.
                event.server_name = None;
                event.user = None;
                Some(event)
            })
            .add_integration(
                sentry::integrations::minidump::MinidumpIntegration::new()
                    .crashes_dir(std::env::temp_dir().join("rubblekin-crashes"))
                    .process_name("Rubblekin crash reporter")
                    .before_capture(|scope, _| {
                        scope.set_tag("build.commit", env!("RUBBLEKIN_BUILD_COMMIT"));
                        scope.set_tag("client.platform", std::env::consts::OS);
                    }),
            ),
    );
    sentry::configure_scope(|scope| {
        scope.set_tag("build.commit", env!("RUBBLEKIN_BUILD_COMMIT"));
        scope.set_tag("client.platform", std::env::consts::OS);
    });
    Some(guard)
}

#[cfg(not(target_os = "android"))]
pub fn startup_error(error: &(dyn std::error::Error + 'static)) {
    sentry::capture_error(error);
}

#[cfg(not(target_os = "android"))]
pub fn context(phase: &str, graphics: &str) {
    sentry::configure_scope(|scope| {
        scope.set_tag("client.phase", phase);
        scope.set_tag("graphics", graphics);
    });
    sentry::with_integration(
        |reporter: &sentry::integrations::minidump::MinidumpIntegration, _| {
            reporter.set_tag("client.phase".into(), Some(phase.into()));
            reporter.set_tag("graphics".into(), Some(graphics.into()));
        },
    );
}

#[cfg(target_os = "android")]
pub fn init() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Android's SDK persists this event and also captures native fatal signals.
        let message = info.to_string();
        let stack = std::backtrace::Backtrace::force_capture().to_string();
        if let Err(error) = android_report("reportRustPanic", &message, &stack) {
            eprintln!("Rubblekin: could not record Rust panic: {error}");
        }
        previous(info);
    }));
}

#[cfg(target_os = "android")]
pub fn startup_error(error: &(dyn std::error::Error + 'static)) {
    if let Err(error) = android_report("reportRustError", &error.to_string(), "") {
        eprintln!("Rubblekin: could not record Rust error: {error}");
    }
}

#[cfg(target_os = "android")]
pub fn context(phase: &str, graphics: &str) {
    let _ = android_report("setCrashContext", phase, graphics);
}

#[cfg(target_os = "android")]
fn android_report(method: &str, message: &str, stack: &str) -> jni::errors::Result<()> {
    use jni::{
        JavaVM, jni_sig,
        objects::{JObject, JValue},
        refs::Global,
        strings::JNIString,
    };
    let Some(app) = bevy::android::ANDROID_APP.get() else {
        return Ok(());
    };
    // SAFETY: AndroidApp owns the JVM and activity throughout this call. Borrow the
    // global activity reference; do not take ownership of or delete it.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        let raw = app.activity_as_ptr().cast();
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
        let message = env.new_string(message)?;
        let stack = env.new_string(stack)?;
        env.call_method(
            activity,
            JNIString::from(method),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;)V"),
            &[
                JValue::Object(message.as_ref()),
                JValue::Object(stack.as_ref()),
            ],
        )?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{ecs::message::Messages, render::error_handler::ErrorType};

    #[test]
    fn device_loss_stops_rendering_and_requests_exit_only_once() {
        let mut app = App::new();
        app.add_message::<AppExit>()
            .init_resource::<RendererFailureReported>();
        let mut render_world = World::new();
        let error = RenderError {
            ty: ErrorType::DeviceLost,
            description: "Device is lost".into(),
            source: None,
        };
        for _ in 0..2 {
            assert!(matches!(
                renderer_error(&error, app.world_mut(), &mut render_world),
                RenderErrorPolicy::StopRendering
            ));
        }
        assert!(app.world().resource::<RendererFailureReported>().0);
        assert_eq!(app.world().resource::<Messages<AppExit>>().len(), 1);
    }
}
