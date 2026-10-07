//! Durable guest identities, scoped to a world/server and display-name slot.
//! The token stays in the private handshake and is never a public player ID.
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};

pub(crate) const PROFILE_FILE: &str = "player-profiles.json";
const MAX_PROFILES: usize = 128;
const MAX_BYTES: u64 = 256 * 1024;

pub(crate) fn local_scope(save: &Path) -> io::Result<String> {
    Ok(format!("local:{}", std::path::absolute(save)?.display()))
}

pub(crate) fn remote_scope(address: &str) -> String {
    format!("server:{}", address.trim().to_lowercase())
}

pub(crate) fn identity(path: &Path, scope: &str, name: &str) -> io::Result<String> {
    identity_at(path, scope, name)
}

fn valid_token(token: &str) -> bool {
    token.len() == 32
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn identity_at(path: &Path, scope: &str, name: &str) -> io::Result<String> {
    if scope.len() > 1024 || name.is_empty() || name.chars().count() > 24 {
        return Err(io::Error::other("Player profile scope or name is invalid"));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let filename = path
        .file_name()
        .ok_or_else(|| io::Error::other("Player profile path must name a file"))?
        .to_string_lossy();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join(format!(".{filename}.lock")))?;
    lock.try_lock().map_err(|_| {
        io::Error::other("Another game is updating player profiles. Try joining again.")
    })?;
    let mut profiles: BTreeMap<String, String> = match File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_BYTES {
                return Err(io::Error::other("Player profile file is too large"));
            }
            serde_json::from_slice(&bytes).map_err(|_| {
                io::Error::other(
                    "Cannot read player-profiles.json. Keep the original file to recover your trading progress.",
                )
            })?
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => return Err(error),
    };
    if profiles.len() > MAX_PROFILES
        || profiles
            .iter()
            .any(|(key, token)| key.len() > 1152 || !valid_token(token))
    {
        return Err(io::Error::other(
            "Player profile file has invalid entries; original left untouched",
        ));
    }
    let key = format!("{scope}\n{}", name.trim().to_lowercase());
    if let Some(token) = profiles.get(&key) {
        return Ok(token.clone());
    }
    if profiles.len() == MAX_PROFILES {
        return Err(io::Error::other("Player profile slots are full"));
    }
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random)
        .map_err(|error| io::Error::other(format!("Cannot create player identity: {error}")))?;
    let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    profiles.insert(key, token.clone());
    let temporary = parent.join(format!(".{filename}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        serde_json::to_writer(&mut file, &profiles).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok::<_, io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(std::path::PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "rubblekin-profile-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn file(&self) -> std::path::PathBuf {
            self.0.join(PROFILE_FILE)
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn identity_is_durable_and_scoped_without_display_case_reset() {
        let dir = TestDirectory::new();
        let first = identity_at(&dir.file(), "server:first:7878", "Ian").unwrap();
        assert!(valid_token(&first));
        assert_eq!(
            first,
            identity_at(&dir.file(), "server:first:7878", "IAN").unwrap()
        );
        assert_ne!(
            first,
            identity_at(&dir.file(), "server:second:7878", "Ian").unwrap()
        );
        assert_ne!(
            first,
            identity_at(&dir.file(), "server:first:7878", "Violet").unwrap()
        );
        assert_eq!(
            first,
            identity_at(&dir.file(), "server:first:7878", "Ian").unwrap()
        );
    }

    #[test]
    fn corrupt_or_invalid_profiles_do_not_reset_progress() {
        let dir = TestDirectory::new();
        for original in [b"invalid json".as_slice(), b"{\"profile\":\"wrong\"}"] {
            fs::write(dir.file(), original).unwrap();
            assert!(identity_at(&dir.file(), "server:first:7878", "Ian").is_err());
            assert_eq!(fs::read(dir.file()).unwrap(), original);
        }
    }

    #[test]
    fn profile_writer_lock_prevents_lost_entries() {
        let dir = TestDirectory::new();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.0.join(".player-profiles.json.lock"))
            .unwrap();
        lock.try_lock().unwrap();
        assert!(identity_at(&dir.file(), "server:first:7878", "Ian").is_err());
        assert!(!dir.file().exists());
        drop(lock);
        assert!(identity_at(&dir.file(), "server:first:7878", "Ian").is_ok());
    }

    #[test]
    fn local_world_scope_is_independent_of_server_port() {
        assert_eq!(
            remote_scope(" EXAMPLE.ORG:7878 "),
            "server:example.org:7878"
        );
        let dir = TestDirectory::new();
        assert_eq!(
            local_scope(&dir.0.join("world.json")).unwrap(),
            local_scope(&dir.0.join("world.json")).unwrap()
        );
        assert_ne!(
            local_scope(&dir.0.join("world.json")).unwrap(),
            local_scope(&dir.0.join("other.json")).unwrap()
        );
    }
}
