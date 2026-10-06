//! Run against a disposable/local DSN to verify the real reporting path without a window.
#[path = "../src/crash_reporting.rs"]
mod crash_reporting;

fn main() {
    let _guard = crash_reporting::init();
    crash_reporting::context("verification", "Low");
    match std::env::args().nth(1).as_deref() {
        Some("panic") => panic!("Rubblekin intentional crash-report verification"),
        Some("abort") => std::process::abort(),
        _ => crash_reporting::startup_error(&std::io::Error::other(
            "Rubblekin intentional startup-error verification",
        )),
    }
}
