#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod cli;
mod gui;

use rubblekin_launcher::{Launcher, Progress};

fn main() {
    #[cfg(windows)]
    // Restore output when invoked from a terminal, without opening a console
    // when a player double-clicks the launcher.
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
    if let Err(error) = run() {
        eprintln!("Rubblekin Launcher: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let options = cli::parse(std::env::args_os().skip(1))?;
    if options.help {
        print!("{}", cli::HELP);
        return Ok(());
    }
    if options.headless {
        run_headless(options)
    } else {
        gui::run(options)
    }
}

fn run_headless(options: cli::Options) -> Result<(), String> {
    let launcher = Launcher::open(options.data_dir)?;
    let client = if options.offline {
        launcher.cached()?.ok_or(
            "No verified client is installed yet. Connect to the internet for the first download.",
        )?
    } else {
        launcher.update(|progress| match progress {
            Progress::Checking => eprintln!("Checking for the latest Rubblekin client…"),
            Progress::Downloading { downloaded, total } => {
                if let Some(total) = total {
                    eprintln!("Downloading: {} / {} MiB", downloaded / 1_048_576, total / 1_048_576);
                } else {
                    eprintln!("Downloading: {} MiB", downloaded / 1_048_576);
                }
            }
            Progress::Verifying => eprintln!("Verifying the download…"),
            Progress::Installing => eprintln!("Installing the client…"),
        }).map_err(|error| format!("{error}\nIf a client is already installed, use --offline to explicitly start that version."))?
    };
    eprintln!("Starting Rubblekin ({}).", &client.commit[..12]);
    client.launch(&options.client_args)?;
    Ok(())
}
