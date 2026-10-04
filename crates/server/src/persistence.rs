use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Write},
    path::Path,
};

use rubblekin_core::world::{BlockEdit, World, WorldGeneration};
use serde::{Deserialize, Serialize};

use crate::npc::Forager;

const SAVE_VERSION: u32 = 2;
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
    #[serde(default)]
    generation: Option<WorldGeneration>,
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
    pub fn load(path: &Path, seed: u32, generation: WorldGeneration) -> io::Result<Self> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let world = World::generate(seed, generation);
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
        if save.version != 1 && save.version != SAVE_VERSION {
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
        let generation = match (save.version, save.generation) {
            (1, None | Some(WorldGeneration::ValleyV1)) => WorldGeneration::ValleyV1,
            (2, Some(generation)) => generation,
            _ => {
                return Err(invalid(
                    "Save has an invalid or missing terrain generation version",
                ));
            }
        };
        let world =
            World::from_generation_edits(save.seed, generation, &save.edits).map_err(invalid)?;
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
                generation: Some(self.world.generation()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::world::{Block, BlockPos};

    struct TestPath(std::path::PathBuf);

    impl TestPath {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "rubblekin-generation-save-{}-{}.json",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )))
        }
    }

    impl Drop for TestPath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn version_one_save_keeps_its_valley_terrain_and_npc_when_new_default_is_geography() {
        let path = TestPath::new();
        let mut world = World::new(42);
        let position = BlockPos::new(3, 12, -4);
        world.set_block(position, Block::Brick).unwrap();
        let npc = Forager::new(&world);
        let npc_position = npc.snapshot.position;
        let sim = Simulation {
            world,
            npc,
            world_time: 1234.0,
        };
        sim.save(&path.0).unwrap();
        let mut old: serde_json::Value =
            serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        old["version"] = 1.into();
        old.as_object_mut().unwrap().remove("generation");
        old["npc"].as_object_mut().unwrap().remove("home");
        fs::write(&path.0, serde_json::to_vec(&old).unwrap()).unwrap();

        let loaded = Simulation::load(&path.0, 999, WorldGeneration::GeographyV1).unwrap();
        assert_eq!(loaded.world.generation(), WorldGeneration::ValleyV1);
        assert_eq!(loaded.world.seed, 42);
        assert_eq!(loaded.world.block(position), Block::Brick);
        assert_eq!(loaded.world.height_at(0, 0), 4);
        assert_eq!(loaded.npc.snapshot.position, npc_position);
        assert_eq!(loaded.world_time, 1234.0);
        loaded.save(&path.0).unwrap();
        let updated: serde_json::Value =
            serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        assert_eq!(updated["version"], 2);
        let again = Simulation::load(&path.0, 999, WorldGeneration::GeographyV1).unwrap();
        assert_eq!(again.world.generation(), WorldGeneration::ValleyV1);
        assert_eq!(again.world.edits(), sim.world.edits());
    }

    #[test]
    fn missing_or_unknown_generation_is_rejected_without_overwriting_save() {
        let path = TestPath::new();
        let world = World::new(42);
        let sim = Simulation {
            npc: Forager::new(&world),
            world,
            world_time: 0.0,
        };
        sim.save(&path.0).unwrap();
        let base: serde_json::Value = serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        for generation in [None, Some(serde_json::json!("FutureTerrain"))] {
            let mut value = base.clone();
            if let Some(generation) = generation {
                value["generation"] = generation;
            } else {
                value.as_object_mut().unwrap().remove("generation");
            }
            let bytes = serde_json::to_vec(&value).unwrap();
            fs::write(&path.0, &bytes).unwrap();
            assert!(Simulation::load(&path.0, 42, WorldGeneration::GeographyV1).is_err());
            assert_eq!(fs::read(&path.0).unwrap(), bytes);
        }
    }
}
