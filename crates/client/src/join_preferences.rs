//! The last submitted join name lives beside game data, outside versioned clients.
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

pub(crate) const FILE: &str = "join-preferences.json";
const DEFAULT_NAME: &str = "Wayfarer";

pub(crate) fn validate_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 24 || name.chars().any(char::is_control) {
        return Err("Enter a character name of 1–24 characters.");
    }
    Ok(name.into())
}

pub(crate) fn load(path: &Path) -> String {
    let result = (|| {
        let mut bytes = Vec::new();
        File::open(path)?.take(4097).read_to_end(&mut bytes)?;
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid join preferences");
        if bytes.len() > 4096 {
            return Err(invalid());
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if value["version"].as_u64() != Some(1) {
            return Err(invalid());
        }
        validate_name(value["name"].as_str().ok_or_else(invalid)?).map_err(|_| invalid())
    })();
    match result {
        Ok(name) => name,
        Err(error) => {
            if error.kind() != io::ErrorKind::NotFound {
                eprintln!("Could not load remembered character name: {error}; using default");
            }
            DEFAULT_NAME.into()
        }
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!("json.{}.tmp", std::process::id()))
}

pub(crate) fn save(path: &Path, name: &str) -> io::Result<()> {
    let name =
        validate_name(name).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let temporary = temporary_path(path);
    let result = (|| {
        let mut file = File::create(&temporary)?;
        serde_json::to_writer_pretty(
            &mut file,
            &serde_json::json!({
                "version": 1,
                "name": name,
            }),
        )?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        // Close before replacement so this works on Windows as well as Unix.
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "rubblekin-join-preferences-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self) -> PathBuf {
            self.0.join(FILE)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn remembers_the_latest_name_across_fresh_loads_with_case_and_unicode() {
        let directory = TestDirectory::new();
        assert_eq!(load(&directory.file()), DEFAULT_NAME);
        assert!(!directory.file().exists());
        save(&directory.file(), "  Violet 雪  ").unwrap();
        assert_eq!(load(&directory.file()), "Violet 雪");
        save(&directory.file(), "Ian").unwrap();
        assert_eq!(load(&directory.file()), "Ian");
        let longest = "雪".repeat(24);
        save(&directory.file(), &longest).unwrap();
        assert_eq!(load(&directory.file()), longest);
    }

    #[test]
    fn missing_or_invalid_preferences_use_the_default_without_rewriting_data() {
        let directory = TestDirectory::new();
        for original in [
            "invalid json".to_string(),
            r#"{"version":2,"name":"Ian"}"#.into(),
            r#"{"version":1}"#.into(),
            r#"{"version":1,"name":12}"#.into(),
            r#"{"version":1,"name":"   "}"#.into(),
            r#"{"version":1,"name":"hello\nworld"}"#.into(),
            serde_json::json!({"version": 1, "name": "x".repeat(25)}).to_string(),
            " ".repeat(4097),
        ] {
            fs::write(directory.file(), &original).unwrap();
            assert_eq!(load(&directory.file()), DEFAULT_NAME);
            assert_eq!(fs::read_to_string(directory.file()).unwrap(), original);
        }
    }

    #[test]
    fn failed_or_invalid_writes_keep_the_previous_name() {
        let directory = TestDirectory::new();
        save(&directory.file(), "Violet").unwrap();
        let original = fs::read(directory.file()).unwrap();
        assert!(save(&directory.file(), " ").is_err());
        fs::create_dir(temporary_path(&directory.file())).unwrap();
        assert!(save(&directory.file(), "Ian").is_err());
        assert_eq!(fs::read(directory.file()).unwrap(), original);
        assert_eq!(load(&directory.file()), "Violet");
    }
}
