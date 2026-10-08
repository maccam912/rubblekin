use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufReader, BufWriter, Write},
    path::Path,
};

use rubblekin_core::{
    airships::AirshipNetwork,
    world::{BlockEdit, BlockPos, World, WorldGeneration},
};
use serde::{Deserialize, Serialize};

use crate::{
    npc::Forager,
    player_economy::{Profiles, validate_profiles},
    quarry_work,
    villages::VillageLife,
};

const SAVE_VERSION: u32 = 7;
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
    #[serde(default)]
    villages: Option<VillageLife>,
    #[serde(default)]
    profiles: Option<Profiles>,
    #[serde(default)]
    consumed_quarry_cells: Option<Vec<BlockPos>>,
    #[serde(default)]
    ecology: Option<crate::ecology::Ecology>,
}

pub(crate) struct Simulation {
    pub world: World,
    pub npc: Forager,
    pub world_time: f64,
    pub villages: VillageLife,
    pub profiles: Profiles,
    pub consumed_quarry_cells: Vec<BlockPos>,
    pub ecology: crate::ecology::Ecology,
}

impl Simulation {
    pub fn load(path: &Path, seed: u32, generation: WorldGeneration) -> io::Result<Self> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let world = World::generate(seed, generation);
                return Ok(Self {
                    ecology: crate::ecology::Ecology::new(&world),
                    npc: Forager::new(&world),
                    villages: VillageLife::new(&world),
                    profiles: Profiles::default(),
                    consumed_quarry_cells: Vec::new(),
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
        if ![1, 2, 3, 4, 5, 6, SAVE_VERSION].contains(&save.version) {
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
            (2, Some(generation)) if !generation.has_settlements() => generation,
            (3..=SAVE_VERSION, Some(generation)) => generation,
            _ => {
                return Err(invalid(
                    "Save has an invalid or missing terrain generation version",
                ));
            }
        };
        let world =
            World::from_generation_edits(save.seed, generation, &save.edits).map_err(invalid)?;
        let villages = match save.villages {
            Some(villages) => villages,
            None if !generation.has_settlements() => VillageLife::default(),
            None => {
                return Err(invalid(
                    "Village world is missing its saved residents and economy",
                ));
            }
        };
        if !villages.validate(&world) {
            return Err(invalid(
                "Save contains invalid village residents or economy",
            ));
        }
        let airships = AirshipNetwork::try_new(&world).map_err(invalid)?;
        if !villages.validate_transport(&world, &airships, save.world_time) {
            return Err(invalid(
                "Save contains an invalid airship journey; original left untouched",
            ));
        }
        let profiles = match save.profiles {
            Some(profiles) => profiles,
            None if save.version < 4 => Profiles::default(),
            None => {
                return Err(invalid(
                    "Save is missing its player progress; original left untouched",
                ));
            }
        };
        if !validate_profiles(&profiles, &world, &airships) {
            return Err(invalid(
                "Save contains invalid player progress; original left untouched",
            ));
        }
        let consumed_quarry_cells = match save.consumed_quarry_cells {
            Some(cells) => cells,
            None if save.version < 5 => Vec::new(),
            None => {
                return Err(invalid(
                    "Save is missing its consumed quarry cells; original left untouched",
                ));
            }
        };
        if !quarry_work::valid_spent(&world, &consumed_quarry_cells) {
            return Err(invalid(
                "Save contains invalid or duplicate consumed quarry cells; original left untouched",
            ));
        }
        let mut saved_ecology = save.ecology;
        if save.version < 7
            && let Some(ecology) = &mut saved_ecology
        {
            // V6 assigned the destination as home at departure. Its travelling
            // animals can finish that saved journey before using arrival-based homes.
            for animal in &mut ecology.animals {
                if animal.action == rubblekin_core::wildlife::WildlifeAction::Migrating {
                    animal.destination = Some(animal.habitat);
                }
            }
        }
        let ecology = match saved_ecology {
            Some(ecology) if ecology.validate(&world) => ecology,
            None if save.version < 6 => crate::ecology::Ecology::new(&world),
            _ => {
                return Err(invalid(
                    "Save contains invalid or missing wildlife; original left untouched",
                ));
            }
        };
        Ok(Self {
            ecology,
            world,
            npc: save.npc,
            world_time: save.world_time,
            villages,
            profiles,
            consumed_quarry_cells,
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
                villages: Some(self.villages.clone()),
                profiles: Some(self.profiles.clone()),
                consumed_quarry_cells: Some(self.consumed_quarry_cells.clone()),
                ecology: Some(self.ecology.clone()),
            };
            write_save(&mut file, &save)?;
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

fn write_save(file: &mut impl Write, save: &Save) -> io::Result<()> {
    // Serde writes individual JSON tokens. Buffer those small writes, then
    // propagate the final flush failure before syncing or replacing the save.
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, save).map_err(io::Error::other)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::world::{Block, BlockPos};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn wildlife_round_trips_and_current_missing_or_corrupt_state_fails_untouched() {
        let path = TestPath::new();
        let mut sim = Simulation::load(&path.0, 42, WorldGeneration::GeographyV6).unwrap();
        for _ in 0..40 {
            sim.ecology.tick(&sim.world, 0.05, &[]);
        }
        sim.save(&path.0).unwrap();
        let loaded = Simulation::load(&path.0, 0, WorldGeneration::ValleyV1).unwrap();
        assert_eq!(
            serde_json::to_value(&loaded.ecology).unwrap(),
            serde_json::to_value(&sim.ecology).unwrap()
        );
        let original: serde_json::Value =
            serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        for corruption in ["missing ecology", "home", "destination", "missing journey"] {
            let mut invalid = original.clone();
            match corruption {
                "missing ecology" => {
                    invalid.as_object_mut().unwrap().remove("ecology");
                }
                "home" => invalid["ecology"]["animals"][0]["habitat"] = 9999.into(),
                "destination" => invalid["ecology"]["animals"][0]["destination"] = 9999.into(),
                _ => {
                    invalid["ecology"]["animals"][0]["action"] = "Migrating".into();
                    invalid["ecology"]["animals"][0]["destination"] = serde_json::Value::Null;
                }
            }
            let bytes = serde_json::to_vec(&invalid).unwrap();
            fs::write(&path.0, &bytes).unwrap();
            assert!(Simulation::load(&path.0, 0, WorldGeneration::ValleyV1).is_err());
            assert_eq!(fs::read(&path.0).unwrap(), bytes);
        }
    }
    #[test]
    fn version_six_migrants_recover_their_saved_journey_without_reseeding() {
        use rubblekin_core::wildlife::WildlifeAction;
        let path = TestPath::new();
        let mut sim = Simulation::load(&path.0, 42, WorldGeneration::GeographyV6).unwrap();
        let destination = sim.ecology.habitats[1].position;
        sim.ecology.animals[0].destination = Some(1);
        sim.ecology.animals[0].action = WildlifeAction::Migrating;
        sim.ecology.animals[0].target = destination;
        let position = sim.ecology.animals[0].body.position;
        sim.save(&path.0).unwrap();
        let mut old: serde_json::Value =
            serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        old["version"] = 6.into();
        old["ecology"]["animals"][0]["habitat"] = 1.into();
        for animal in old["ecology"]["animals"].as_array_mut().unwrap() {
            animal.as_object_mut().unwrap().remove("destination");
        }
        fs::write(&path.0, serde_json::to_vec(&old).unwrap()).unwrap();
        let mut loaded = Simulation::load(&path.0, 0, WorldGeneration::ValleyV1).unwrap();
        assert_eq!(loaded.ecology.animals.len(), sim.ecology.animals.len());
        assert_eq!(loaded.ecology.animals[0].body.position, position);
        assert_eq!(loaded.ecology.animals[0].destination, Some(1));
        loaded.ecology.tick(&loaded.world, 0.05, &[]);
        let target = loaded.ecology.animals[0].target;
        assert!((target[0] - destination[0]).hypot(target[2] - destination[2]) <= 14.01);
        assert_eq!(loaded.ecology.animals[0].destination, Some(1));
        loaded.save(&path.0).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        assert_eq!(saved["version"], 7);
        assert_eq!(saved["ecology"]["animals"][0]["destination"], 1);
    }

    #[test]
    fn old_saves_gain_empty_profiles_but_current_missing_or_invalid_progress_fails_untouched() {
        use crate::player_economy::SavedPlayer;
        use rubblekin_core::economy::PlayerEconomy;
        let path = TestPath::new();
        let mut sim = Simulation::load(&path.0, 42, WorldGeneration::ValleyV1).unwrap();
        sim.save(&path.0).unwrap();
        let original: serde_json::Value =
            serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        for version in [1, 2, 3] {
            let mut old = original.clone();
            old["version"] = version.into();
            old.as_object_mut().unwrap().remove("profiles");
            fs::write(&path.0, serde_json::to_vec(&old).unwrap()).unwrap();
            let loaded = Simulation::load(&path.0, 99, WorldGeneration::GeographyV4).unwrap();
            assert!(loaded.profiles.is_empty());
            assert_eq!(loaded.world.seed, 42);
            assert_eq!(loaded.world.generation(), WorldGeneration::ValleyV1);
        }
        let profile = "0123456789abcdef0123456789abcdef";
        sim.profiles.insert(
            profile.into(),
            SavedPlayer {
                ledger: PlayerEconomy {
                    coins: 12,
                    cargo: [1, 2, 3, 4, 5],
                    ..Default::default()
                },
                position: sim.world.spawn_position(),
                yaw: 0.0,
                ride: None,
                deck_position: None,
            },
        );
        sim.save(&path.0).unwrap();
        let valid: serde_json::Value = serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        assert_eq!(
            Simulation::load(&path.0, 0, WorldGeneration::GeographyV4)
                .unwrap()
                .profiles[profile]
                .ledger,
            sim.profiles[profile].ledger
        );
        for mutation in 0..5 {
            let mut invalid = valid.clone();
            match mutation {
                0 => {
                    invalid.as_object_mut().unwrap().remove("profiles");
                }
                1 => invalid["profiles"][profile]["ledger"]["coins"] = u64::MAX.into(),
                2 => {
                    invalid["profiles"][profile]["ledger"]["cargo"] =
                        serde_json::json!([24, 1, 0, 0, 0])
                }
                3 => {
                    invalid["profiles"][profile]["position"] = serde_json::json!([1_000_000, 0, 0])
                }
                4 => invalid["profiles"][profile]["deck_position"] = serde_json::json!([0, 0, 0]),
                _ => unreachable!(),
            }
            let bytes = serde_json::to_vec(&invalid).unwrap();
            fs::write(&path.0, &bytes).unwrap();
            assert!(
                Simulation::load(&path.0, 0, WorldGeneration::GeographyV4).is_err(),
                "mutation={mutation}"
            );
            assert_eq!(fs::read(&path.0).unwrap(), bytes);
        }
    }

    #[test]
    fn save_five_requires_canonical_unique_quarry_cells_and_migrates_versions_one_to_four() {
        let path = TestPath::new();
        let sim = Simulation::load(&path.0, 42, WorldGeneration::ValleyV1).unwrap();
        sim.save(&path.0).unwrap();
        let base: serde_json::Value = serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        for version in 1..=4 {
            let mut old = base.clone();
            old["version"] = version.into();
            old.as_object_mut().unwrap().remove("consumed_quarry_cells");
            fs::write(&path.0, serde_json::to_vec(&old).unwrap()).unwrap();
            let loaded = Simulation::load(&path.0, 0, WorldGeneration::GeographyV6).unwrap();
            assert!(loaded.consumed_quarry_cells.is_empty());
            assert_eq!(loaded.world.generation(), WorldGeneration::ValleyV1);
        }
        fs::remove_file(&path.0).unwrap();
        let mut sim = Simulation::load(&path.0, 42, WorldGeneration::GeographyV6).unwrap();
        let id = quarry_work::sites(&sim.world)[0];
        let anchor = quarry_work::cells(&quarry_work::site(&sim.world, id).unwrap().building)
            .next()
            .unwrap();
        sim.world.set_block(anchor, Block::Air).unwrap();
        sim.consumed_quarry_cells.push(anchor);
        sim.save(&path.0).unwrap();
        let loaded = Simulation::load(&path.0, 0, WorldGeneration::ValleyV1).unwrap();
        assert_eq!(loaded.consumed_quarry_cells, [anchor]);
        assert_eq!(loaded.world.block(anchor), Block::Air);
        // A builder may restore a spent block. The shared record must still load
        // and prohibit a second payment even though its edit override vanishes.
        sim.world.set_block(anchor, Block::Stone).unwrap();
        sim.save(&path.0).unwrap();
        let loaded = Simulation::load(&path.0, 0, WorldGeneration::ValleyV1).unwrap();
        assert_eq!(loaded.consumed_quarry_cells, [anchor]);
        assert_eq!(loaded.world.block(anchor), Block::Stone);
        assert!(
            quarry_work::available(&loaded.world, id, anchor, &loaded.consumed_quarry_cells)
                .is_err()
        );
        let valid: serde_json::Value = serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        for cells in [
            None,
            Some(serde_json::json!([anchor, anchor])),
            Some(serde_json::json!([BlockPos::new(0, 0, 0)])),
        ] {
            let mut invalid = valid.clone();
            if let Some(cells) = cells {
                invalid["consumed_quarry_cells"] = cells;
            } else {
                invalid
                    .as_object_mut()
                    .unwrap()
                    .remove("consumed_quarry_cells");
            }
            let bytes = serde_json::to_vec(&invalid).unwrap();
            fs::write(&path.0, &bytes).unwrap();
            assert!(Simulation::load(&path.0, 0, WorldGeneration::ValleyV1).is_err());
            assert_eq!(fs::read(&path.0).unwrap(), bytes);
        }
    }

    static NEXT_PATH: AtomicU64 = AtomicU64::new(0);

    struct TestPath(std::path::PathBuf);

    impl TestPath {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "rubblekin-generation-save-{}-{}-{}.json",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_PATH.fetch_add(1, Ordering::Relaxed)
            )))
        }
    }

    impl Drop for TestPath {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn buffered_save_reports_failure_when_the_final_bytes_cannot_be_written() {
        struct FullDisk;
        impl Write for FullDisk {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::StorageFull, "Disk is full"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let world = World::new(42);
        let save = Save {
            version: SAVE_VERSION,
            seed: world.seed,
            generation: Some(world.generation()),
            edits: Vec::new(),
            npc: Forager::new(&world),
            world_time: 0.0,
            villages: Some(VillageLife::default()),
            profiles: Some(Profiles::default()),
            consumed_quarry_cells: Some(Vec::new()),
            ecology: Some(crate::ecology::Ecology::default()),
        };
        // This entire save fits in the buffer. Serialization succeeds before
        // the explicit final flush tries to write it; dropping BufWriter alone
        // would silently discard this error and falsely report a saved world.
        assert!(
            serde_json::to_vec(&save).unwrap().len() + 1 < BufWriter::new(io::sink()).capacity()
        );
        let error = write_save(&mut FullDisk, &save).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::StorageFull);
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
            profiles: Default::default(),
            consumed_quarry_cells: Vec::new(),
            ecology: crate::ecology::Ecology::default(),
            world,
            npc,
            world_time: 1234.0,
            villages: VillageLife::default(),
        };
        sim.save(&path.0).unwrap();
        let mut old: serde_json::Value =
            serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        old["version"] = 1.into();
        old.as_object_mut().unwrap().remove("profiles");
        old.as_object_mut().unwrap().remove("generation");
        old["npc"].as_object_mut().unwrap().remove("home");
        fs::write(&path.0, serde_json::to_vec(&old).unwrap()).unwrap();

        let loaded = Simulation::load(&path.0, 999, WorldGeneration::GeographyV2).unwrap();
        assert_eq!(loaded.world.generation(), WorldGeneration::ValleyV1);
        assert_eq!(loaded.world.seed, 42);
        assert_eq!(loaded.world.block(position), Block::Brick);
        assert_eq!(loaded.world.height_at(0, 0), 4);
        assert_eq!(loaded.npc.snapshot.position, npc_position);
        assert_eq!(loaded.world_time, 1234.0);
        loaded.save(&path.0).unwrap();
        let updated: serde_json::Value =
            serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        assert_eq!(updated["version"], SAVE_VERSION);
        let again = Simulation::load(&path.0, 999, WorldGeneration::GeographyV2).unwrap();
        assert_eq!(again.world.generation(), WorldGeneration::ValleyV1);
        assert_eq!(again.world.edits(), sim.world.edits());
    }

    #[test]
    fn geography_v1_save_keeps_original_columns_trees_and_edits_with_v2_default() {
        let path = TestPath::new();
        let mut world = World::generate(42, WorldGeneration::GeographyV1);
        let trunk = BlockPos::new(5023, 410, -3594);
        let crown = BlockPos::new(5023, 420, -3594);
        let cut = BlockPos::new(6000, 374, -7000);
        let placed = BlockPos::new(10000, 300, -12000);
        // Captured from the original GeographyV1 generator, before V2. These
        // samples catch a terrain or tree change hidden behind stable metadata.
        assert_eq!(world.block(trunk), Block::Wood);
        assert_eq!(world.block(crown), Block::Leaves);
        assert_eq!(world.block(cut), Block::Stone);
        world.set_block(trunk, Block::Air).unwrap();
        world.set_block(cut, Block::Air).unwrap();
        world.set_block(placed, Block::Brick).unwrap();
        let sim = Simulation {
            profiles: Default::default(),
            consumed_quarry_cells: Vec::new(),
            ecology: crate::ecology::Ecology::default(),
            npc: Forager::new(&world),
            world,
            world_time: 217.5,
            villages: VillageLife::default(),
        };
        sim.save(&path.0).unwrap();

        let loaded = Simulation::load(&path.0, 999, WorldGeneration::GeographyV2).unwrap();
        assert_eq!(loaded.world.generation(), WorldGeneration::GeographyV1);
        assert_eq!(loaded.world.seed, 42);
        assert_eq!(loaded.world.edits(), sim.world.edits());
        assert_eq!(loaded.world_time, 217.5);
        assert_eq!(loaded.npc.snapshot.position, sim.npc.snapshot.position);
        for (x, z, height, surface) in [
            (0, 0, 458, Block::Grass),
            (6000, -7000, 384, Block::Sand),
            (10000, -12000, 269, Block::Sand),
            (-16000, 10000, 245, Block::Grass),
            (12000, 16000, 1587, Block::Sand),
            (5023, -3594, 409, Block::Grass),
        ] {
            assert_eq!(loaded.world.height_at(x, z), height, "column {x},{z}");
            assert_eq!(loaded.world.surface_block(x, z), surface, "column {x},{z}");
        }
        assert_eq!(loaded.world.block(trunk), Block::Air);
        assert_eq!(loaded.world.block(crown), Block::Leaves);
        assert_eq!(loaded.world.block(cut), Block::Air);
        assert_eq!(loaded.world.block(placed), Block::Brick);
        loaded.save(&path.0).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        assert_eq!(saved["version"], SAVE_VERSION);
        assert_eq!(saved["generation"], "GeographyV1");
        // An actual pre-village version-2 save has no village state field.
        let mut pre_villages = saved;
        pre_villages["version"] = 2.into();
        pre_villages.as_object_mut().unwrap().remove("profiles");
        pre_villages.as_object_mut().unwrap().remove("villages");
        fs::write(&path.0, serde_json::to_vec(&pre_villages).unwrap()).unwrap();
        let legacy = Simulation::load(&path.0, 999, WorldGeneration::GeographyV3).unwrap();
        assert_eq!(legacy.world.generation(), WorldGeneration::GeographyV1);
        assert_eq!(legacy.world.edits(), sim.world.edits());
        assert!(legacy.villages.residents().is_empty());
        assert_eq!(legacy.npc.snapshot.position, sim.npc.snapshot.position);
    }

    #[test]
    fn new_geography_v2_save_roundtrips_generator_seed_and_far_edits() {
        let path = TestPath::new();
        let mut sim = Simulation::load(&path.0, 42, WorldGeneration::GeographyV2).unwrap();
        assert_eq!(sim.world.generation(), WorldGeneration::GeographyV2);
        let height = sim.world.height_at(10000, -12000);
        let cut = BlockPos::new(10000, height - 8, -12000);
        let placed = BlockPos::new(10000, height + 30, -12000);
        sim.world.set_block(cut, Block::Air).unwrap();
        sim.world.set_block(placed, Block::Glass).unwrap();
        sim.world_time = 412.5;
        sim.save(&path.0).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        assert_eq!(saved["version"], SAVE_VERSION);
        assert_eq!(saved["generation"], "GeographyV2");

        let loaded = Simulation::load(&path.0, 999, WorldGeneration::ValleyV1).unwrap();
        assert_eq!(loaded.world.generation(), WorldGeneration::GeographyV2);
        assert_eq!(loaded.world.seed, 42);
        assert_eq!(loaded.world.height_at(10000, -12000), height);
        assert_eq!(loaded.world.edits(), sim.world.edits());
        assert_eq!(loaded.world.block(cut), Block::Air);
        assert_eq!(loaded.world.block(placed), Block::Glass);
        assert_eq!(loaded.world_time, 412.5);
        assert_eq!(loaded.npc.snapshot.position, sim.npc.snapshot.position);
    }

    #[test]
    fn missing_or_unknown_generation_is_rejected_without_overwriting_save() {
        let path = TestPath::new();
        let world = World::new(42);
        let sim = Simulation {
            profiles: Default::default(),
            consumed_quarry_cells: Vec::new(),
            ecology: crate::ecology::Ecology::default(),
            npc: Forager::new(&world),
            world,
            world_time: 0.0,
            villages: VillageLife::default(),
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
            assert!(Simulation::load(&path.0, 42, WorldGeneration::GeographyV2).is_err());
            assert_eq!(fs::read(&path.0).unwrap(), bytes);
        }
    }

    #[test]
    fn village_roster_work_cargo_and_stores_roundtrip_without_reseeding() {
        let path = TestPath::new();
        let mut sim = Simulation::load(&path.0, 42, WorldGeneration::GeographyV3).unwrap();
        let initial = sim.villages.residents();
        for _ in 0..40 {
            sim.villages.tick(&sim.world, 0.05);
            sim.world_time += 0.05;
        }
        assert_ne!(sim.villages.residents(), initial);
        let residents = sim.villages.residents();
        let villages = sim.villages.villages();
        assert!(sim.villages.validate(&sim.world));
        sim.save(&path.0).unwrap();
        let loaded = Simulation::load(&path.0, 999, WorldGeneration::GeographyV2).unwrap();
        assert_eq!(loaded.world.seed, 42);
        assert_eq!(loaded.world.generation(), WorldGeneration::GeographyV3);
        assert_eq!(loaded.villages.residents(), residents);
        assert_eq!(loaded.villages.villages(), villages);
        assert_eq!(loaded.world_time, sim.world_time);
    }

    #[test]
    fn version_three_village_saves_add_need_defaults_without_resetting_the_world() {
        let path = TestPath::new();
        let sim = Simulation::load(&path.0, 42, WorldGeneration::GeographyV3).unwrap();
        sim.save(&path.0).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        for resident in value["villages"]["residents"].as_array_mut().unwrap() {
            let state = resident.as_object_mut().unwrap();
            state.remove("resume");
            state.remove("farm_waypoint");
            let snapshot = state["snapshot"].as_object_mut().unwrap();
            for field in ["hunger", "energy", "reason"] {
                snapshot.remove(field);
            }
        }
        for economy in value["villages"]["villages"].as_array_mut().unwrap() {
            economy.as_object_mut().unwrap().remove("planted");
        }
        fs::write(&path.0, serde_json::to_vec(&value).unwrap()).unwrap();
        let mut loaded = Simulation::load(&path.0, 999, WorldGeneration::GeographyV2).unwrap();
        assert_eq!(loaded.world.seed, 42);
        assert_eq!(loaded.world.generation(), WorldGeneration::GeographyV3);
        assert_eq!(loaded.villages.villages(), sim.villages.villages());
        let residents = loaded.villages.residents();
        for (resident, original) in residents.iter().zip(sim.villages.residents()) {
            assert_eq!(resident.id, original.id);
            assert_eq!(resident.position, original.position);
            assert_eq!(resident.carrying, original.carrying);
            assert_eq!(resident.hunger, 25.0);
            assert_eq!(resident.energy, 85.0);
        }
        for _ in 0..40 {
            loaded.villages.tick(&loaded.world, 0.05);
        }
        assert!(loaded.villages.validate(&loaded.world));
        loaded.save(&path.0).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        assert!(saved["villages"]["residents"][0]["snapshot"]["hunger"].is_number());
    }

    #[test]
    fn invalid_village_save_is_rejected_and_left_untouched() {
        let path = TestPath::new();
        let sim = Simulation::load(&path.0, 42, WorldGeneration::GeographyV3).unwrap();
        sim.save(&path.0).unwrap();
        let original: serde_json::Value =
            serde_json::from_slice(&fs::read(&path.0).unwrap()).unwrap();
        let mutations = [
            "missing",
            "id",
            "route",
            "stock",
            "hunger",
            "energy",
            "farm_waypoint",
            "resume",
            "reason",
        ];
        for mutation in mutations {
            let mut value = original.clone();
            match mutation {
                "missing" => {
                    value.as_object_mut().unwrap().remove("villages");
                }
                "id" => value["villages"]["residents"][0]["snapshot"]["id"] = 999_999.into(),
                "route" => value["villages"]["residents"][0]["waypoint"] = 999_999.into(),
                "stock" => value["villages"]["villages"][0]["snapshot"]["food"] = (-1).into(),
                "hunger" => value["villages"]["residents"][0]["snapshot"]["hunger"] = 101.into(),
                "energy" => value["villages"]["residents"][0]["snapshot"]["energy"] = (-1).into(),
                "farm_waypoint" => {
                    value["villages"]["residents"][0]["farm_waypoint"] = 999_999.into()
                }
                "resume" => {
                    value["villages"]["residents"][0]["resume"] =
                        serde_json::json!({ "phase": "ToTrade", "waypoint": 0, "elapsed": 0.0 })
                }
                "reason" => {
                    value["villages"]["residents"][0]["snapshot"]["reason"] =
                        "invalid\nreason".into()
                }
                _ => unreachable!(),
            }
            let bytes = serde_json::to_vec(&value).unwrap();
            fs::write(&path.0, &bytes).unwrap();
            assert!(
                Simulation::load(&path.0, 42, WorldGeneration::GeographyV3).is_err(),
                "{mutation}"
            );
            assert_eq!(fs::read(&path.0).unwrap(), bytes);
        }
    }
}
