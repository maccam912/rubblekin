//! GitHub client updates, kept separate from the launcher's small user interface.
//!
//! Only complete, checksum-verified installs become current. Game saves live in
//! a separate directory and are never part of installation or cleanup.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

pub type Result<T> = std::result::Result<T, String>;

const RELEASES: &str = "https://github.com/maccam912/rubblekin/releases";
const MAX_MANIFEST: u64 = 256 * 1024;
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
const MAX_EXECUTABLE: u64 = 512 * 1024 * 1024;
const MAX_ZIP_ENTRIES: u16 = 64;
const MAC_BUNDLE_FILES: [&str; 3] = [
    "Rubblekin.app/Contents/Info.plist",
    "Rubblekin.app/Contents/_CodeSignature/CodeResources",
    "Rubblekin.app/Contents/Resources/OFL.txt",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    Checking,
    Downloading { downloaded: u64, total: Option<u64> },
    Verifying,
    Installing,
}

pub struct Launcher {
    data_dir: PathBuf,
    target: &'static str,
    lock: File,
    #[cfg(test)]
    test_releases: Option<String>,
}

impl Drop for Launcher {
    fn drop(&mut self) {
        // A concurrent subprocess can inherit this descriptor until exec.
        // Closing only our copy would keep the lock alive in that interval.
        // The file still closes afterward, including if unlocking fails.
        let _ = self.lock.unlock();
    }
}

#[derive(Debug, Clone)]
pub struct InstalledClient {
    pub commit: String,
    pub executable: PathBuf,
    pub game_dir: PathBuf,
    log_path: PathBuf,
    executable_sha256: String,
}

impl InstalledClient {
    /// Launch with a stable working directory, preserving local saves on update.
    /// A cached executable is rechecked immediately before it is started.
    pub fn launch(&self, args: &[OsString]) -> Result<Child> {
        verify_file(&self.executable, &self.executable_sha256)?;
        let log = File::create(&self.log_path).map_err(|error| {
            format!(
                "Cannot open client log {}: {error}",
                self.log_path.display()
            )
        })?;
        let error_log = log.try_clone().map_err(|error| error.to_string())?;
        let mut command = Command::new(&self.executable);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        command
            .args(args)
            .current_dir(&self.game_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(error_log))
            .spawn()
            .map_err(|error| format!("Cannot start the installed client: {error}"))
    }
}

#[derive(Debug, Deserialize)]
struct Manifest {
    schema_version: u32,
    commit: String,
    tag: String,
    platforms: BTreeMap<String, Platform>,
}

#[derive(Debug, Deserialize)]
struct Platform {
    asset: String,
    sha256: String,
    executable: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallRecord {
    commit: String,
    target: String,
    archive_sha256: String,
    executable_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct State {
    schema_version: u32,
    current: InstallRecord,
    previous: Option<InstallRecord>,
}

impl Launcher {
    /// `data_dir` is an optional override for development and isolated tests.
    pub fn open(data_dir: Option<PathBuf>) -> Result<Self> {
        let target = target()?;
        let data_dir = match data_dir {
            Some(path) => path,
            None => dirs::data_local_dir()
                .ok_or("Cannot locate your local application data directory")?
                .join("rubblekin"),
        };
        fs::create_dir_all(&data_dir)
            .map_err(|error| format!("Cannot create {}: {error}", data_dir.display()))?;
        let data_dir = fs::canonicalize(data_dir).map_err(|error| error.to_string())?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(data_dir.join("launcher.lock"))
            .map_err(|error| format!("Cannot open the launcher lock: {error}"))?;
        lock.try_lock()
            .map_err(|error| format!("Another launcher may already be open: {error}"))?;
        for directory in ["clients", "game", "logs"] {
            fs::create_dir_all(data_dir.join(directory))
                .map_err(|error| format!("Cannot create the {directory} directory: {error}"))?;
        }
        fs::create_dir_all(data_dir.join("clients").join(target))
            .map_err(|error| format!("Cannot create the client install directory: {error}"))?;
        Ok(Self {
            data_dir,
            target,
            lock,
            #[cfg(test)]
            test_releases: None,
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Returns only the current, checksum-verified client. Errors never cause an
    /// automatic fallback or launch; the UI can offer cached play explicitly.
    pub fn cached(&self) -> Result<Option<InstalledClient>> {
        let Some(state) = self.read_state()? else {
            return Ok(None);
        };
        self.installed(&state.current).map(Some)
    }

    pub fn update(&self, mut progress: impl FnMut(Progress)) -> Result<InstalledClient> {
        progress(Progress::Checking);
        let agent = self.agent();
        let manifest = self.fetch_manifest(&agent)?;
        let target = target()?;
        let platform = manifest.platform(target)?;
        // Corrupt state must not prevent a fresh installation. It is replaced
        // only after the downloaded client has passed every check.
        let old_state = self.read_state().ok().flatten();
        if let Some(state) = &old_state
            && state.current.commit == manifest.commit
            && state.current.archive_sha256 == platform.sha256
            && let Ok(client) = self.installed(&state.current)
        {
            return Ok(client);
        }

        let clients = self.clients_dir();
        let staging = tempfile::Builder::new()
            .prefix(".install-")
            .tempdir_in(&clients)
            .map_err(|error| format!("Cannot create an update staging directory: {error}"))?;
        let archive_path = staging.path().join("download.zip");
        let url = format!(
            "{}/download/{}/{}",
            self.releases_url(),
            manifest.tag,
            platform.asset
        );
        self.download(&agent, &url, &archive_path, &platform.sha256, &mut progress)?;
        progress(Progress::Installing);
        let executable_sha256 =
            extract_executable(&archive_path, staging.path(), expected_executable(target)?)?;
        fs::remove_file(archive_path).map_err(|error| error.to_string())?;
        let record = InstallRecord {
            commit: manifest.commit.clone(),
            target: target.into(),
            archive_sha256: platform.sha256.clone(),
            executable_sha256,
        };
        let destination = clients.join(&record.commit);
        if destination.exists() {
            // A previous interrupted run may have installed this directory but
            // not changed the pointer. Reuse only identical, verified bytes.
            let existing = destination.join(expected_executable(target)?);
            if verify_file(&existing, &record.executable_sha256).is_err() {
                return Err(format!(
                    "The cached version {} is damaged. Close the game, remove {}, and retry; saves are in {}.",
                    &record.commit[..12],
                    destination.display(),
                    self.data_dir.join("game").display()
                ));
            }
        } else {
            fs::rename(staging.path(), &destination)
                .map_err(|error| format!("Cannot finish installing the client: {error}"))?;
        }
        let previous = old_state.and_then(|state| {
            let previous = if state.current.commit != record.commit {
                Some(state.current)
            } else {
                state.previous
            };
            previous.filter(|record| self.installed(record).is_ok())
        });
        let state = State {
            schema_version: 1,
            current: record,
            previous,
        };
        self.write_state(&state)?;
        self.cleanup(&state);
        self.installed(&state.current)
    }

    fn releases_url(&self) -> &str {
        #[cfg(test)]
        if let Some(url) = &self.test_releases {
            return url;
        }
        RELEASES
    }

    fn clients_dir(&self) -> PathBuf {
        self.data_dir.join("clients").join(self.target)
    }

    fn state_path(&self) -> PathBuf {
        self.data_dir.join(format!("current-{}.json", self.target))
    }

    fn agent(&self) -> ureq::Agent {
        let https_only = true;
        #[cfg(test)]
        let https_only = https_only && self.test_releases.is_none();
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .https_only(https_only)
                .user_agent(concat!("Rubblekin-Launcher/", env!("CARGO_PKG_VERSION")))
                .timeout_connect(Some(Duration::from_secs(15)))
                .timeout_recv_response(Some(Duration::from_secs(30)))
                .timeout_global(Some(Duration::from_secs(15 * 60)))
                .max_redirects(5)
                .build(),
        )
    }

    fn fetch_manifest(&self, agent: &ureq::Agent) -> Result<Manifest> {
        let url = format!(
            "{}/latest/download/client-manifest.json",
            self.releases_url()
        );
        let mut response = agent
            .get(&url)
            .header("Accept", "application/json")
            .config()
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .call()
            .map_err(|error| format!("Cannot check GitHub for a client update: {error}"))?;
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(MAX_MANIFEST + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Cannot read the release manifest: {error}"))?;
        if bytes.len() as u64 > MAX_MANIFEST {
            return Err("The release manifest exceeds the size limit".into());
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("The release manifest is invalid: {error}"))
    }

    fn download(
        &self,
        agent: &ureq::Agent,
        url: &str,
        path: &Path,
        expected_hash: &str,
        progress: &mut impl FnMut(Progress),
    ) -> Result<()> {
        let mut response = agent
            .get(url)
            .call()
            .map_err(|error| format!("Cannot download the client: {error}"))?;
        let total = response
            .headers()
            .get("content-length")
            .map(|value| {
                value
                    .to_str()
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or("The client download has an invalid size")
            })
            .transpose()?;
        if total.is_some_and(|size| size == 0 || size > MAX_DOWNLOAD) {
            return Err("The client download exceeds the size limit or is empty".into());
        }
        progress(Progress::Downloading {
            downloaded: 0,
            total,
        });
        let mut file = File::create(path).map_err(|error| error.to_string())?;
        let mut reader = response.body_mut().as_reader();
        let mut hash = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        let mut downloaded = 0;
        let mut last_progress = 0;
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| format!("The client download was interrupted: {error}"))?;
            if read == 0 {
                break;
            }
            downloaded += read as u64;
            if downloaded > MAX_DOWNLOAD || total.is_some_and(|size| downloaded > size) {
                return Err("The client download exceeded its size limit".into());
            }
            file.write_all(&buffer[..read]).map_err(|error| {
                format!("Cannot save the client download (check free disk space): {error}")
            })?;
            hash.update(&buffer[..read]);
            if downloaded - last_progress >= 1024 * 1024 {
                progress(Progress::Downloading { downloaded, total });
                last_progress = downloaded;
            }
        }
        if downloaded == 0 || total.is_some_and(|size| downloaded != size) {
            return Err("The client download is incomplete".into());
        }
        progress(Progress::Downloading { downloaded, total });
        progress(Progress::Verifying);
        if hash_hex(&hash.finalize()) != expected_hash {
            return Err(
                "The client download failed its SHA-256 check; the installed client was preserved"
                    .into(),
            );
        }
        file.sync_all().map_err(|error| error.to_string())?;
        Ok(())
    }

    fn read_state(&self) -> Result<Option<State>> {
        let path = self.state_path();
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Cannot read the installed version: {error}")),
        };
        let mut bytes = Vec::new();
        file.take(MAX_MANIFEST + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_MANIFEST {
            return Err("The installed-version record exceeds its size limit".into());
        }
        let state: State = serde_json::from_slice(&bytes)
            .map_err(|error| format!("The installed-version record is damaged: {error}"))?;
        if state.schema_version != 1 {
            return Err("The installed-version record uses an unsupported version".into());
        }
        state.current.validate()?;
        if let Some(previous) = &state.previous {
            previous.validate()?;
        }
        Ok(Some(state))
    }

    fn installed(&self, record: &InstallRecord) -> Result<InstalledClient> {
        record.validate()?;
        let executable = self
            .clients_dir()
            .join(&record.commit)
            .join(expected_executable(&record.target)?);
        verify_file(&executable, &record.executable_sha256)?;
        Ok(InstalledClient {
            commit: record.commit.clone(),
            executable,
            game_dir: self.data_dir.join("game"),
            log_path: self.data_dir.join("logs/client.log"),
            executable_sha256: record.executable_sha256.clone(),
        })
    }

    fn write_state(&self, state: &State) -> Result<()> {
        let mut file =
            tempfile::NamedTempFile::new_in(&self.data_dir).map_err(|error| error.to_string())?;
        serde_json::to_writer(&mut file, state).map_err(|error| error.to_string())?;
        file.as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        file.persist(self.state_path())
            .map_err(|error| format!("Cannot record the installed client: {error}"))?;
        Ok(())
    }

    fn cleanup(&self, state: &State) {
        let Ok(entries) = fs::read_dir(self.clients_dir()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let keep = name == state.current.commit
                || state
                    .previous
                    .as_ref()
                    .is_some_and(|record| name == record.commit);
            if !keep
                && (is_lower_hex(name, 40) || name.starts_with(".install-"))
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
            {
                // An older executable can still be running on Windows. Failure
                // is harmless and cleanup can succeed on a later update.
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}

impl Manifest {
    fn platform(&self, target: &str) -> Result<&Platform> {
        if self.schema_version != 1 {
            return Err(
                "This release needs a newer launcher; download the launcher again from GitHub"
                    .into(),
            );
        }
        if !is_lower_hex(&self.commit, 40) || self.tag != format!("client-{}", self.commit) {
            return Err("The release manifest has an invalid commit or tag".into());
        }
        let platform = self
            .platforms
            .get(target)
            .ok_or_else(|| format!("The latest release does not include a client for {target}"))?;
        if platform.asset != format!("rubblekin-client-{target}.zip")
            || platform.executable != expected_executable(target)?
            || !is_lower_hex(&platform.sha256, 64)
        {
            return Err(
                "The release manifest has an invalid asset, executable path, or SHA-256".into(),
            );
        }
        Ok(platform)
    }
}

impl InstallRecord {
    fn validate(&self) -> Result<()> {
        if !is_lower_hex(&self.commit, 40)
            || self.target != target()?
            || !is_lower_hex(&self.archive_sha256, 64)
            || !is_lower_hex(&self.executable_sha256, 64)
        {
            return Err("The installed-version record contains invalid paths or checksums".into());
        }
        Ok(())
    }
}

pub fn target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        _ => Err(
            "No Rubblekin release is available for this operating system and architecture".into(),
        ),
    }
}

fn expected_executable(target: &str) -> Result<&'static str> {
    match target {
        "x86_64-unknown-linux-gnu" => Ok("rubblekin"),
        "x86_64-pc-windows-msvc" => Ok("rubblekin.exe"),
        "aarch64-apple-darwin" | "x86_64-apple-darwin" => {
            Ok("Rubblekin.app/Contents/MacOS/rubblekin")
        }
        _ => Err("The release targets an unsupported platform".into()),
    }
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hash_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn verify_file(path: &Path, expected: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "Cannot read the installed client {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EXECUTABLE {
        return Err("The installed client is not a valid executable file".into());
    }
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let hash = hash_reader(&mut file, std::io::sink(), MAX_EXECUTABLE)?;
    if hash != expected {
        return Err(
            "The installed client failed its SHA-256 check; it will not be launched".into(),
        );
    }
    Ok(())
}

fn hash_reader(mut reader: impl Read, mut writer: impl Write, limit: u64) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut size = 0;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        size += read as u64;
        if size > limit {
            return Err("The executable exceeds its size limit".into());
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|error| error.to_string())?;
        hash.update(&buffer[..read]);
    }
    if size == 0 {
        return Err("The executable is empty".into());
    }
    Ok(hash_hex(&hash.finalize()))
}

fn extract_executable(archive_path: &Path, destination: &Path, expected: &str) -> Result<String> {
    let file = File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("The client archive is invalid: {error}"))?;
    if archive.len() > usize::from(MAX_ZIP_ENTRIES) {
        return Err("The client archive contains too many entries".into());
    }
    let checksum = extract_fixed_file(&mut archive, destination, expected, MAX_EXECUTABLE, true)?;
    extract_fixed_file(&mut archive, destination, "OFL.txt", 64 * 1024, false)?;
    if expected == "Rubblekin.app/Contents/MacOS/rubblekin" {
        for path in MAC_BUNDLE_FILES {
            extract_fixed_file(&mut archive, destination, path, 256 * 1024, false)?;
        }
    }
    Ok(checksum)
}

fn extract_fixed_file(
    archive: &mut zip::ZipArchive<File>,
    destination: &Path,
    name: &str,
    limit: u64,
    executable: bool,
) -> Result<String> {
    let mut member = archive
        .by_name(name)
        .map_err(|error| format!("The client archive is missing {name}: {error}"))?;
    if !member.is_file() || member.size() == 0 || member.size() > limit {
        return Err(format!(
            "The client archive entry {name} is a link, directory, empty, or too large"
        ));
    }
    // Also reject Unix special files; is_file() only excludes links and
    // directories in zip 8. No archive-supplied permissions are applied.
    if let Some(mode) = member.unix_mode()
        && mode & 0o170000 != 0
        && mode & 0o170000 != 0o100000
    {
        return Err(format!(
            "The client archive entry {name} is not a regular file"
        ));
    }
    let expected_size = member.size();
    // `name` is always a hardcoded target executable or allowlisted resource,
    // never an arbitrary ZIP member. No unrelated entry is extracted.
    let path = destination.join(name);
    fs::create_dir_all(path.parent().ok_or("Invalid archive destination")?)
        .map_err(|error| error.to_string())?;
    let mut output = File::create(path).map_err(|error| error.to_string())?;
    let checksum = hash_reader(&mut member, &mut output, limit)?;
    if output.metadata().map_err(|error| error.to_string())?.len() != expected_size {
        return Err(format!("The client archive entry {name} is incomplete"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        output
            .set_permissions(fs::Permissions::from_mode(if executable {
                0o755
            } else {
                0o644
            }))
            .map_err(|error| error.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    output.sync_all().map_err(|error| error.to_string())?;
    Ok(checksum)
}

#[cfg(test)]
mod tests;
