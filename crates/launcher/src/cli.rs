use std::{ffi::OsString, path::PathBuf};

#[derive(Default)]
pub struct Options {
    pub data_dir: Option<PathBuf>,
    pub headless: bool,
    pub offline: bool,
    pub help: bool,
    pub client_args: Vec<OsString>,
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--data-dir") => {
                options.data_dir = Some(args.next().ok_or("--data-dir needs a directory")?.into());
            }
            Some("--headless") => options.headless = true,
            Some("--offline") => options.offline = true,
            Some("--help" | "-h") => options.help = true,
            Some("--") => {
                options.client_args.extend(args);
                break;
            }
            _ => {
                return Err(format!(
                    "Unknown launcher option {arg:?}. Put client options after --."
                ));
            }
        }
    }
    Ok(options)
}

pub const HELP: &str = "Rubblekin Launcher

Checks GitHub for the latest client, installs it, and starts the game.

  --headless          Print update progress without an update window
  --offline           Explicitly start the verified installed client
  --data-dir PATH     Override the installation and game-data directory
  --help              Show this help
  -- CLIENT_OPTIONS   Pass the remaining arguments to the game

Example: rubblekin-launcher -- --low --connect server.example:7878

Downloads: https://github.com/maccam912/rubblekin/releases/latest
";
