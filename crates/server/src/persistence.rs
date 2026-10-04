use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Write},
    path::Path,
};

use rubblekin_core::world::{BlockEdit, World};
use serde::{Deserialize, Serialize};

use crate::npc::Forager;

const SAVE_VERSION: u32 = 1;
pub(crate) const MAX_EDITS: usize = 100_000;

/// The sidecar remains on disk, but its OS lock is released on close or crash.
/// Locking the save itself would not survive atomic file replacement.
pub(crate) fn lock_save(path: &Path) -> io::Result<File> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let filename = path
        .file_name()
        .ok_or_else(|| invalid("Save path must name a file"))?;
    let lock_path = parent.join(format!(".{}.lock", filename.to_string_lossy()));
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    lock.try_lock().map_err(|error| {
        io::Error::other(format!(
            "Cannot lock save {} (another server may be using it): {error}",
            path.display()
        ))
    })?;
    Ok(lock)
}

#[derive(Serialize, Deserialize)]
struct Save {
    version: u32,
    seed: u32,
    edits: Vec<BlockEdit>,
    npc: Forager,
    world_time: f64,
}

pub(crate) struct Simulation {
    pub world: World,
    pub npc: Forager,
    pub world_time: f64,
}

impl Simulation {
    pub fn load(path: &Path, seed: u32) -> io::Result<Self> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let world = World::new(seed);
                return Ok(Self {
                    npc: Forager::new(&world),
                    world,
                    world_time: 0.0,
                });
            }
            Err(error) => return Err(error),
        };
        if file.metadata()?.len() > 32 * 1024 * 1024 {
            return Err(invalid("Save exceeds the prototype's 32 MiB limit"));
        }
        let save: Save = serde_json::from_reader(BufReader::new(file)).map_err(|error| {
            invalid(format!(
                "Cannot read save {}: {error}; original left untouched",
                path.display()
            ))
        })?;
        if save.version != SAVE_VERSION {
            return Err(invalid(format!(
                "Unsupported save version {}",
                save.version
            )));
        }
        if !save.world_time.is_finite()
            || save.world_time < 0.0
            || !save.npc.validate()
            || save.edits.len() > MAX_EDITS
        {
            return Err(invalid("Save contains invalid simulation values"));
        }
        let world = World::from_edits(save.seed, &save.edits).map_err(invalid)?;
        Ok(Self {
            world,
            npc: save.npc,
            world_time: save.world_time,
        })
    }

    /// Acknowledgements are sent only after the replacement save is durable.
    /// Temp files live beside the save, so rename stays on the same filesystem.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let filename = path
            .file_name()
            .ok_or_else(|| invalid("Save path must name a file"))?;
        let temporary = parent.join(format!(
            ".{}.{}.tmp",
            filename.to_string_lossy(),
            std::process::id()
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temporary)?;
            let save = Save {
                version: SAVE_VERSION,
                seed: self.world.seed,
                edits: self.world.edits(),
                npc: self.npc.clone(),
                world_time: self.world_time,
            };
            serde_json::to_writer(&mut file, &save).map_err(io::Error::other)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            #[cfg(unix)]
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
