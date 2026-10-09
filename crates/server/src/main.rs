use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use rubblekin_core::world::WorldGeneration;
use rubblekin_server::{ServerConfig, spawn};

fn main() -> io::Result<()> {
    let mut config = ServerConfig {
        bind_addr: "0.0.0.0:7878".into(),
        ..Default::default()
    };
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--bind" => config.bind_addr = value(&mut args, "--bind")?,
            "--save" => config.save_path = PathBuf::from(value(&mut args, "--save")?),
            "--generation" => {
                config.generation = match value(&mut args, "--generation")?.as_str() {
                    "v3" => WorldGeneration::GeographyV3,
                    "v4" => WorldGeneration::GeographyV4,
                    "v5" => WorldGeneration::GeographyV5,
                    "v6" => WorldGeneration::GeographyV6,
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "--generation requires v3, v4, v5 or v6",
                        ));
                    }
                };
            }
            "--seed" => {
                config.seed = value(&mut args, "--seed")?.parse().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "--seed requires an unsigned integer",
                    )
                })?
            }
            "--allow-admin" => config.allow_admin = true,
            "--help" | "-h" => {
                println!(
                    "rubblekin-server [--bind 0.0.0.0:7878] [--save saves/world.json] [--seed 42] [--generation v3|v4|v5|v6] [--allow-admin]\n\n--allow-admin grants developer controls to EVERY connected player and allows read-only observer sessions. Use only on a trusted development server.\nThe player named exactly maccam912 always gets developer controls, even without --allow-admin. Names are not authenticated.\nExisting saves retain their seed and generation. New save paths default to v6 villages; --generation v3 reproduces the earlier terrain. The world keeps simulating without players; server downtime is not replayed."
                );
                return Ok(());
            }
            unknown => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Unknown argument {unknown}; use --help"),
                ));
            }
        }
    }
    let stopping = Arc::new(AtomicBool::new(false));
    let signal_flag = stopping.clone();
    ctrlc::set_handler(move || signal_flag.store(true, Ordering::Relaxed))
        .map_err(io::Error::other)?;
    let server = spawn(config)?;
    println!("Rubblekin server listening on {}", server.addr);
    while !stopping.load(Ordering::Relaxed) && !server.is_finished() {
        thread::sleep(Duration::from_millis(100));
    }
    server.stop()
}

fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> io::Result<String> {
    args.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{flag} requires a value"),
        )
    })
}
