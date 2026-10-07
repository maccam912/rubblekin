use super::*;
use serde_json::{Value, json};
use std::{
    io::Cursor,
    net::TcpListener,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
};
use zip::write::SimpleFileOptions;

const FIRST: &str = "1111111111111111111111111111111111111111";
const SECOND: &str = "2222222222222222222222222222222222222222";
const THIRD: &str = "3333333333333333333333333333333333333333";

fn archive(bytes: &[u8]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            expected_executable(target().unwrap()).unwrap(),
            SimpleFileOptions::default().unix_permissions(0o755),
        )
        .unwrap();
    writer.write_all(bytes).unwrap();
    writer
        .start_file("OFL.txt", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"Test font license").unwrap();
    bundle_resources(&mut writer);
    writer.finish().unwrap().into_inner()
}

fn bundle_resources(writer: &mut zip::ZipWriter<Cursor<Vec<u8>>>) {
    if target().unwrap().ends_with("apple-darwin") {
        for name in MAC_BUNDLE_FILES {
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"test bundle resource").unwrap();
        }
    }
}

fn manifest(commit: &str, archive: &[u8]) -> Value {
    let target = target().unwrap();
    json!({
        "schema_version": 1,
        "commit": commit,
        "tag": format!("client-{commit}"),
        "platforms": {
            target: {
                "asset": format!("rubblekin-client-{target}.zip"),
                "sha256": hash_hex(&Sha256::digest(archive)),
                "executable": expected_executable(target).unwrap(),
            }
        }
    })
}

#[derive(Clone)]
struct Reply {
    body: Vec<u8>,
    length: Option<u64>,
    status: u16,
}

impl Reply {
    fn new(body: Vec<u8>) -> Self {
        Self {
            body,
            length: None,
            status: 200,
        }
    }
}

/// A real loopback HTTP server exercises ureq's body framing, redirects and
/// transport failures without allowing a production override of GitHub URLs.
struct Fixture {
    releases: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Fixture {
    fn new(manifest: Value, asset: Reply) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let releases = format!("http://{}/releases", listener.local_addr().unwrap());
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let asset_path = format!(
            "/releases/download/{}/rubblekin-client-{}.zip",
            manifest["tag"].as_str().unwrap(),
            target().unwrap()
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let (stop, stopped) = mpsc::channel();
        let thread = thread::spawn(move || {
            loop {
                if stopped.try_recv().is_ok() {
                    break;
                }
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("Fixture accept failed: {error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).unwrap();
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    assert!(request.len() < 64 * 1024);
                }
                let request = String::from_utf8(request).unwrap();
                let path = request
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                recorded.lock().unwrap().push(path.to_owned());
                let reply = if path == "/releases/latest/download/client-manifest.json" {
                    Reply::new(manifest_bytes.clone())
                } else if path == asset_path {
                    asset.clone()
                } else {
                    Reply {
                        status: 404,
                        body: b"missing".to_vec(),
                        length: None,
                    }
                };
                let header = format!(
                    "HTTP/1.1 {} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    reply.status,
                    reply.length.unwrap_or(reply.body.len() as u64)
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&reply.body);
            }
        });
        Self {
            releases,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn release(commit: &str, executable: &[u8]) -> Self {
        let bytes = archive(executable);
        Self::new(manifest(commit, &bytes), Reply::new(bytes))
    }

    fn use_for(&self, launcher: &mut Launcher) {
        launcher.test_releases = Some(self.releases.clone());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        let result = self.thread.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

fn new_launcher() -> (tempfile::TempDir, Launcher) {
    let directory = tempfile::tempdir().unwrap();
    let launcher = Launcher::open(Some(directory.path().join("application-data"))).unwrap();
    (directory, launcher)
}

#[test]
fn validates_manifest_schema_target_paths_and_checksums() {
    let bytes = archive(b"client");
    let valid = manifest(FIRST, &bytes);
    let target = target().unwrap();
    assert!(
        serde_json::from_value::<Manifest>(valid.clone())
            .unwrap()
            .platform(target)
            .is_ok()
    );
    for (field, value) in [
        ("schema_version", json!(2)),
        ("commit", json!("../outside")),
        ("tag", json!("unrelated-release")),
        ("platforms", json!({})),
    ] {
        let mut broken = valid.clone();
        broken[field] = value;
        assert!(
            serde_json::from_value::<Manifest>(broken)
                .unwrap()
                .platform(target)
                .is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("asset", "https://other.example/client.zip"),
        ("asset", "../../outside.zip"),
        ("executable", "../outside"),
        ("sha256", "not-a-hash"),
        (
            "sha256",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ),
    ] {
        let mut broken = valid.clone();
        broken["platforms"][target][field] = json!(value);
        assert!(
            serde_json::from_value::<Manifest>(broken)
                .unwrap()
                .platform(target)
                .is_err(),
            "{field}"
        );
    }
}

#[test]
fn downloads_verifies_installs_and_reuses_current_version() {
    let (_directory, mut launcher) = new_launcher();
    let fixture = Fixture::release(FIRST, b"verified client");
    fixture.use_for(&mut launcher);
    let save = launcher.data_dir.join("game/saves/valley.json");
    fs::create_dir_all(save.parent().unwrap()).unwrap();
    fs::write(&save, "precious world").unwrap();
    let mut progress = Vec::new();
    let client = launcher.update(|event| progress.push(event)).unwrap();
    assert_eq!(client.commit, FIRST);
    assert_eq!(fs::read(&client.executable).unwrap(), b"verified client");
    assert_eq!(
        fs::read(launcher.clients_dir().join(FIRST).join("OFL.txt")).unwrap(),
        b"Test font license"
    );
    assert_eq!(fs::read_to_string(&save).unwrap(), "precious world");
    if target().unwrap().ends_with("apple-darwin") {
        for name in MAC_BUNDLE_FILES {
            assert_eq!(
                fs::read(launcher.clients_dir().join(FIRST).join(name)).unwrap(),
                b"test bundle resource"
            );
        }
    }
    assert!(client.executable.is_absolute());
    assert!(client.game_dir.is_absolute());
    assert_eq!(progress.first(), Some(&Progress::Checking));
    assert!(progress.contains(&Progress::Verifying));
    assert_eq!(progress.last(), Some(&Progress::Installing));
    assert_eq!(launcher.cached().unwrap().unwrap().commit, FIRST);
    launcher.update(|_| {}).unwrap();
    let requests = fixture.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|path| path.contains("client-manifest.json"))
            .count(),
        2
    );
    assert_eq!(
        requests
            .iter()
            .filter(|path| path.ends_with(".zip"))
            .count(),
        1
    );
}

#[test]
fn failed_hash_and_truncated_http_download_preserve_working_client() {
    let (_directory, mut launcher) = new_launcher();
    let first = Fixture::release(FIRST, b"old working client");
    first.use_for(&mut launcher);
    launcher.update(|_| {}).unwrap();
    let state_before = fs::read(launcher.state_path()).unwrap();
    let bytes = archive(b"new client");
    let mut bad_hash = manifest(SECOND, &bytes);
    bad_hash["platforms"][target().unwrap()]["sha256"] = json!("0".repeat(64));
    let broken = Fixture::new(bad_hash, Reply::new(bytes.clone()));
    broken.use_for(&mut launcher);
    assert!(launcher.update(|_| {}).unwrap_err().contains("SHA-256"));
    assert_eq!(launcher.cached().unwrap().unwrap().commit, FIRST);
    assert_eq!(fs::read(launcher.state_path()).unwrap(), state_before);
    let truncated = Fixture::new(
        manifest(SECOND, &bytes),
        Reply {
            body: bytes[..bytes.len() / 2].to_vec(),
            length: Some(bytes.len() as u64),
            status: 200,
        },
    );
    truncated.use_for(&mut launcher);
    assert!(launcher.update(|_| {}).is_err());
    assert_eq!(launcher.cached().unwrap().unwrap().commit, FIRST);
    assert_eq!(fs::read(launcher.state_path()).unwrap(), state_before);
    assert_eq!(fs::read_dir(launcher.clients_dir()).unwrap().count(), 1);
}

#[test]
fn missing_platform_http_error_and_oversized_download_leave_cache_available() {
    let (_directory, mut launcher) = new_launcher();
    let first = Fixture::release(FIRST, b"old working client");
    first.use_for(&mut launcher);
    launcher.update(|_| {}).unwrap();
    let bytes = archive(b"new client");
    let mut missing = manifest(SECOND, &bytes);
    missing["platforms"] = json!({});
    let missing = Fixture::new(missing, Reply::new(bytes.clone()));
    missing.use_for(&mut launcher);
    assert!(
        launcher
            .update(|_| {})
            .unwrap_err()
            .contains("does not include")
    );
    assert_eq!(missing.requests.lock().unwrap().len(), 1);
    let failure = Fixture::new(
        manifest(SECOND, &bytes),
        Reply {
            status: 503,
            body: b"down".to_vec(),
            length: None,
        },
    );
    failure.use_for(&mut launcher);
    assert!(launcher.update(|_| {}).is_err());
    let oversized = Fixture::new(
        manifest(SECOND, &bytes),
        Reply {
            status: 200,
            body: Vec::new(),
            length: Some(MAX_DOWNLOAD + 1),
        },
    );
    oversized.use_for(&mut launcher);
    assert!(launcher.update(|_| {}).unwrap_err().contains("size limit"));
    assert_eq!(launcher.cached().unwrap().unwrap().commit, FIRST);
}

#[test]
fn corrupt_state_fails_closed_and_can_be_rebuilt_from_verified_release() {
    let (_directory, mut launcher) = new_launcher();
    let fixture = Fixture::release(FIRST, b"client");
    fixture.use_for(&mut launcher);
    launcher.update(|_| {}).unwrap();
    fs::write(launcher.state_path(), b"partial JSON").unwrap();
    assert!(launcher.cached().is_err());
    // Existing directory is reused only after matching a freshly verified
    // download, so interrupted pointer writes do not require manual deletion.
    launcher.update(|_| {}).unwrap();
    assert_eq!(launcher.cached().unwrap().unwrap().commit, FIRST);
    let mut state: Value =
        serde_json::from_slice(&fs::read(launcher.state_path()).unwrap()).unwrap();
    state["current"]["commit"] = json!("../outside");
    fs::write(launcher.state_path(), serde_json::to_vec(&state).unwrap()).unwrap();
    assert!(launcher.cached().is_err());
    launcher.update(|_| {}).unwrap();
    assert_eq!(launcher.cached().unwrap().unwrap().commit, FIRST);
}

#[test]
fn interrupted_install_is_never_selected_and_completed_orphan_is_recovered() {
    let (_directory, mut launcher) = new_launcher();
    let partial = launcher.clients_dir().join(".install-interrupted");
    fs::create_dir(&partial).unwrap();
    fs::write(partial.join("download.zip"), b"partial download").unwrap();
    assert!(launcher.cached().unwrap().is_none());
    let fixture = Fixture::release(FIRST, b"client");
    fixture.use_for(&mut launcher);
    launcher.update(|_| {}).unwrap();
    assert!(!partial.exists());
    fs::remove_file(launcher.state_path()).unwrap();
    assert!(launcher.cached().unwrap().is_none());
    launcher.update(|_| {}).unwrap();
    assert_eq!(launcher.cached().unwrap().unwrap().commit, FIRST);
}

#[test]
fn locks_out_a_second_launcher_and_releases_lock_on_drop() {
    let (_directory, launcher) = new_launcher();
    let path = launcher.data_dir.clone();
    assert!(Launcher::open(Some(path.clone())).is_err());
    drop(launcher);
    Launcher::open(Some(path)).unwrap_or_else(|error| panic!("reopen after drop: {error}"));
}

#[cfg(unix)]
#[test]
fn releases_lock_while_a_spawn_inherited_descriptor_is_still_open() {
    let (_directory, launcher) = new_launcher();
    let path = launcher.data_dir.clone();
    // A concurrent fork/spawn duplicates open descriptors until exec closes
    // CLOEXEC files. Keep a duplicate to reproduce that interval without races.
    let inherited = launcher.lock.try_clone().unwrap();
    assert!(Launcher::open(Some(path.clone())).is_err());
    drop(launcher);
    let replacement = Launcher::open(Some(path.clone()))
        .unwrap_or_else(|error| panic!("reopen with inherited descriptor: {error}"));
    drop(inherited);
    assert!(Launcher::open(Some(path.clone())).is_err());
    drop(replacement);
    Launcher::open(Some(path)).unwrap_or_else(|error| panic!("reopen replacement: {error}"));
}

#[test]
fn checks_cached_bytes_again_immediately_before_launch() {
    let (_directory, mut launcher) = new_launcher();
    let fixture = Fixture::release(FIRST, b"client");
    fixture.use_for(&mut launcher);
    let client = launcher.update(|_| {}).unwrap();
    fs::write(&client.executable, b"corrupted").unwrap();
    assert!(launcher.cached().is_err());
    assert!(client.launch(&[]).unwrap_err().contains("SHA-256"));
}

#[test]
fn keeps_current_and_previous_versions_and_never_cleans_game_data() {
    let (_directory, mut launcher) = new_launcher();
    let other_target = launcher.data_dir.join("clients/another-target").join(FIRST);
    fs::create_dir_all(&other_target).unwrap();
    fs::write(other_target.join("client"), b"other architecture").unwrap();
    fs::write(launcher.data_dir.join("game/world.json"), b"saved world").unwrap();
    for (commit, executable) in [
        (FIRST, b"first".as_slice()),
        (SECOND, b"second"),
        (THIRD, b"third"),
    ] {
        let fixture = Fixture::release(commit, executable);
        fixture.use_for(&mut launcher);
        launcher.update(|_| {}).unwrap();
    }
    assert!(!launcher.clients_dir().join(FIRST).exists());
    assert!(launcher.clients_dir().join(SECOND).exists());
    assert!(launcher.clients_dir().join(THIRD).exists());
    assert_eq!(
        fs::read(other_target.join("client")).unwrap(),
        b"other architecture"
    );
    assert_eq!(
        fs::read(launcher.data_dir.join("game/world.json")).unwrap(),
        b"saved world"
    );
    assert_eq!(
        launcher
            .read_state()
            .unwrap()
            .unwrap()
            .previous
            .unwrap()
            .commit,
        SECOND
    );
}

#[test]
fn extracts_only_fixed_paths_and_rejects_symlink_executable() {
    let (_directory, mut launcher) = new_launcher();
    let expected = expected_executable(target().unwrap()).unwrap();
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(expected, SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"safe client").unwrap();
    writer
        .start_file("../../outside", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"must not be written").unwrap();
    writer
        .start_file("OFL.txt", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"license").unwrap();
    bundle_resources(&mut writer);
    let bytes = writer.finish().unwrap().into_inner();
    let fixture = Fixture::new(manifest(FIRST, &bytes), Reply::new(bytes));
    fixture.use_for(&mut launcher);
    launcher.update(|_| {}).unwrap();
    assert!(!launcher.data_dir.join("outside").exists());
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink(expected, "../../outside", SimpleFileOptions::default())
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let fixture = Fixture::new(manifest(SECOND, &bytes), Reply::new(bytes));
    fixture.use_for(&mut launcher);
    assert!(launcher.update(|_| {}).unwrap_err().contains("link"));
    assert_eq!(launcher.cached().unwrap().unwrap().commit, FIRST);
}

#[test]
fn enforces_streamed_executable_bound_even_when_metadata_lies() {
    assert!(hash_reader(Cursor::new(b"12345"), std::io::sink(), 4).is_err());
    assert!(hash_reader(Cursor::new(b""), std::io::sink(), 4).is_err());
    assert!(hash_reader(Cursor::new(b"1234"), std::io::sink(), 4).is_ok());
}

#[cfg(unix)]
#[test]
fn launches_client_with_stable_cwd_literal_arguments_and_logs() {
    let (_directory, mut launcher) = new_launcher();
    let script = b"#!/bin/sh\nprintf '%s\\n' \"$PWD\" > launch-cwd\nprintf '%s\\n' \"$@\" > launch-args\nprintf 'hello log\\n'\n";
    let fixture = Fixture::release(FIRST, script);
    fixture.use_for(&mut launcher);
    let client = launcher.update(|_| {}).unwrap();
    let mut child = client
        .launch(&[
            OsString::from("argument with spaces"),
            OsString::from("$(not a command)"),
        ])
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(
        fs::read_to_string(client.game_dir.join("launch-cwd"))
            .unwrap()
            .trim(),
        client.game_dir.to_str().unwrap()
    );
    assert_eq!(
        fs::read_to_string(client.game_dir.join("launch-args")).unwrap(),
        "argument with spaces\n$(not a command)\n"
    );
    assert_eq!(fs::read_to_string(client.log_path).unwrap(), "hello log\n");
}

#[test]
#[ignore = "requires native packaged client"]
fn installs_packaged_client_and_starts_help() {
    let archive_path = std::env::var_os("RUBBLEKIN_TEST_CLIENT_ARCHIVE")
        .expect("Set RUBBLEKIN_TEST_CLIENT_ARCHIVE to a native client release ZIP");
    let bytes = fs::read(archive_path).expect("Read the native packaged client");
    let fixture = Fixture::new(manifest(FIRST, &bytes), Reply::new(bytes));
    let (_directory, mut launcher) = new_launcher();
    fixture.use_for(&mut launcher);
    let client = launcher.update(|_| {}).expect("Install the native package");

    #[cfg(target_os = "macos")]
    {
        let output = Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(launcher.clients_dir().join(FIRST).join("Rubblekin.app"))
            .output()
            .expect("Check the installed macOS bundle signature");
        assert!(
            output.status.success(),
            "Installed bundle signature failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let mut child = client
        .launch(&[OsString::from("--help")])
        .expect("Start the installed native client");
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().expect("Poll the native client") {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(30) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Installed client --help did not exit within 30 seconds");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let log = fs::read_to_string(&client.log_path).expect("Read the installed client's output");
    assert!(
        status.success(),
        "Installed client failed ({status}): {log}"
    );
    assert!(
        log.contains("Rubblekin"),
        "Client help output was missing: {log}"
    );
}
