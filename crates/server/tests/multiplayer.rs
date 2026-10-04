use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rubblekin_core::{
    physics::{MoveInput, move_character},
    protocol::*,
    world::{Block, BlockPos, CELL_SIZE, World},
};
use rubblekin_server::{ServerConfig, spawn};

static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

struct TestSave(PathBuf);

impl TestSave {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "rubblekin-test-{}-{unique}-{}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed)
        )))
    }

    fn config(&self, admin: bool) -> ServerConfig {
        ServerConfig {
            bind_addr: "127.0.0.1:0".into(),
            save_path: self.0.join("world.json"),
            seed: 42,
            generation: rubblekin_core::world::WorldGeneration::ValleyV1,
            allow_admin: admin,
        }
    }
}

impl Drop for TestSave {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Client {
    fn connect(addr: SocketAddr, name: &str) -> (Self, ServerMessage) {
        Self::connect_mode(addr, name, SessionMode::Player)
    }

    fn connect_mode(addr: SocketAddr, name: &str, mode: SessionMode) -> (Self, ServerMessage) {
        let mut client = Self::open(addr);
        client.send(ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            name: name.into(),
            mode,
        });
        let welcome = client.until(|message| matches!(message, ServerMessage::Welcome { .. }));
        (client, welcome)
    }

    fn open(addr: SocketAddr) -> Self {
        let writer = TcpStream::connect(addr).unwrap();
        writer
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        writer
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        writer.set_nodelay(true).unwrap();
        Self {
            reader: BufReader::new(writer.try_clone().unwrap()),
            writer,
        }
    }

    fn send(&mut self, message: ClientMessage) {
        let mut bytes = serde_json::to_vec(&message).unwrap();
        bytes.push(b'\n');
        self.writer.write_all(&bytes).unwrap();
    }

    fn until(&mut self, predicate: impl Fn(&ServerMessage) -> bool) -> ServerMessage {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                Instant::now() < deadline,
                "Timed out waiting for expected server message"
            );
            let mut line = String::new();
            assert!(
                self.reader.read_line(&mut line).unwrap() > 0,
                "Server disconnected"
            );
            let message = serde_json::from_str(&line).unwrap();
            if predicate(&message) {
                return message;
            }
        }
    }

    fn until_disconnected(&mut self, maximum_ack: u64) {
        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(0) => return,
                Ok(_) => {
                    if let ServerMessage::State { players, .. } =
                        serde_json::from_str(&line).unwrap()
                    {
                        assert!(players.iter().all(|p| p.last_input_sequence <= maximum_ack));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => return,
                Err(error) => panic!("Expected a disconnect, got {error}"),
            }
        }
    }
}

fn nearby_air() -> BlockPos {
    let world = World::new(42);
    BlockPos::new(
        4,
        (world.surface_height(2.25, 0.25) / CELL_SIZE).round() as i32,
        0,
    )
}

#[test]
fn geographic_world_replicates_edits_and_preserves_its_generator_across_restart() {
    use rubblekin_core::world::WorldGeneration;
    let save = TestSave::new();
    let mut config = save.config(true);
    config.generation = WorldGeneration::GeographyV1;
    let server = spawn(config.clone()).unwrap();
    let (mut builder, welcome) = Client::connect(server.addr, "Island builder");
    let (seed, body) = match welcome {
        ServerMessage::Welcome {
            seed,
            generation,
            session_id,
            players,
            ..
        } => {
            assert_eq!(generation, WorldGeneration::GeographyV1);
            (
                seed,
                players
                    .into_iter()
                    .find(|p| p.id == session_id)
                    .unwrap()
                    .body,
            )
        }
        _ => unreachable!(),
    };
    let world = World::generate(seed, WorldGeneration::GeographyV1);
    assert!(
        (body.position[1] - world.surface_height(body.position[0], body.position[2])).abs() < 0.1
    );
    let (mut observer, welcome) =
        Client::connect_mode(server.addr, "Surveyor", SessionMode::Observer);
    assert!(matches!(
        welcome,
        ServerMessage::Welcome {
            generation: WorldGeneration::GeographyV1,
            ..
        }
    ));
    let position = BlockPos::new(
        (body.position[0] / CELL_SIZE).floor() as i32 + 4,
        (body.position[1] / CELL_SIZE).floor() as i32 + 3,
        (body.position[2] / CELL_SIZE).floor() as i32,
    );
    builder.send(ClientMessage::Edit {
        request_id: 1,
        position,
        block: Block::Brick,
    });
    let response = builder.until(|message| {
        matches!(
            message,
            ServerMessage::BlockChanged { request_id: 1, .. }
                | ServerMessage::Rejected { request_id: 1, .. }
        )
    });
    assert!(
        matches!(response, ServerMessage::BlockChanged { .. }),
        "{response:?}"
    );
    observer.until(|message| matches!(message, ServerMessage::BlockChanged { edit, .. } if edit.position == position));
    drop(builder);
    drop(observer);
    server.stop().unwrap();

    // A saved edit kilometers from spawn exercises dynamic world bounds on
    // restore; old 160 m valley validation must not discard or reject it.
    let far = BlockPos::new(10000, world.height_at(10000, -12000), -12000);
    assert!(world.contains_block(far));
    let mut saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    saved["edits"].as_array_mut().unwrap().push(
        serde_json::to_value(rubblekin_core::world::BlockEdit {
            position: far,
            block: Block::Glass,
        })
        .unwrap(),
    );
    fs::write(&config.save_path, serde_json::to_vec(&saved).unwrap()).unwrap();
    config.generation = WorldGeneration::ValleyV1;
    config.seed = 999;
    let restarted = spawn(config).unwrap();
    let (client, welcome) = Client::connect(restarted.addr, "Returning");
    match welcome {
        ServerMessage::Welcome {
            generation,
            seed: restored_seed,
            edits,
            ..
        } => {
            assert_eq!(generation, WorldGeneration::GeographyV1);
            assert_eq!(restored_seed, seed);
            assert!(
                edits
                    .iter()
                    .any(|edit| edit.position == position && edit.block == Block::Brick)
            );
            assert!(
                edits
                    .iter()
                    .any(|edit| edit.position == far && edit.block == Block::Glass)
            );
        }
        _ => unreachable!(),
    }
    drop(client);
    restarted.stop().unwrap();
}

#[test]
fn observers_receive_the_live_world_without_an_avatar_and_cannot_mutate_it() {
    let save = TestSave::new();
    let server = spawn(save.config(true)).unwrap();
    let connected_at = Instant::now();
    let (mut observer, welcome) =
        Client::connect_mode(server.addr, "Admin camera", SessionMode::Observer);
    let observer_id = match welcome {
        ServerMessage::Welcome {
            session_id,
            mode,
            players,
            can_admin,
            ..
        } => {
            assert_eq!(mode, SessionMode::Observer);
            assert!(
                players.is_empty(),
                "An observer must never create an avatar"
            );
            assert!(!can_admin, "Observation is read-only");
            session_id
        }
        _ => unreachable!(),
    };
    let (mut player, welcome) = Client::connect(server.addr, "Builder");
    let player_id = match welcome {
        ServerMessage::Welcome {
            session_id,
            mode,
            players,
            can_admin,
            ..
        } => {
            assert_eq!(mode, SessionMode::Player);
            assert!(can_admin);
            assert_eq!(players.len(), 1);
            assert_eq!(players[0].id, session_id);
            assert_ne!(session_id, observer_id);
            session_id
        }
        _ => unreachable!(),
    };

    let position = nearby_air();
    observer.send(ClientMessage::Edit {
        request_id: 90,
        position,
        block: Block::Brick,
    });
    let rejection =
        observer.until(|message| matches!(message, ServerMessage::Rejected { request_id: 90, .. }));
    assert!(
        matches!(rejection, ServerMessage::Rejected { reason, .. } if reason.contains("read-only"))
    );
    observer.send(ClientMessage::Input {
        sequence: 1,
        dt: 0.05,
        input: MoveInput {
            direction: [1.0, 0.0],
            fly: true,
            ..Default::default()
        },
        yaw: 0.0,
    });
    let notice = observer.until(|message| matches!(message, ServerMessage::Notice { .. }));
    assert!(matches!(notice, ServerMessage::Notice { text } if text.contains("read-only")));
    for action in [
        AdminAction::SetNpcGoal {
            goal: Some(NpcAction::Rest),
        },
        AdminAction::SetNpcNeeds {
            hunger: 0.0,
            energy: 0.0,
        },
        AdminAction::SetNpcWeights {
            forage: 0.0,
            rest: 10.0,
        },
    ] {
        observer.send(ClientMessage::Admin { action });
        let notice = observer.until(|message| matches!(message, ServerMessage::Notice { .. }));
        assert!(matches!(notice, ServerMessage::Notice { text } if text.contains("read-only")));
    }
    let state = observer.until(|message| matches!(message, ServerMessage::State { .. }));
    match state {
        ServerMessage::State { players, npc, .. } => {
            assert_eq!(players.len(), 1);
            assert_eq!(players[0].id, player_id);
            assert_eq!(players[0].last_input_sequence, 0);
            assert!(!npc.forced);
            assert!(npc.hunger > 0.0 && npc.energy > 0.0);
        }
        _ => unreachable!(),
    }
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(save.config(true).save_path).unwrap()).unwrap();
    assert!(saved["edits"].as_array().unwrap().is_empty());
    assert!(saved["npc"]["forced_goal"].is_null());
    assert_eq!(saved["npc"]["forage_weight"], 1.0);
    assert_eq!(saved["npc"]["rest_weight"], 1.0);

    player.send(ClientMessage::Edit {
        request_id: 91,
        position,
        block: Block::Wood,
    });
    let edit = observer
        .until(|message| matches!(message, ServerMessage::BlockChanged { request_id: 91, .. }));
    assert!(
        matches!(edit, ServerMessage::BlockChanged { player_id: actor, edit, .. }
        if actor == player_id && edit.position == position && edit.block == Block::Wood)
    );
    observer.send(ClientMessage::Ping);
    observer.until(|message| matches!(message, ServerMessage::Pong));

    // An admitted camera has no PlayerSnapshot, but must outlive the handshake
    // timeout and keep receiving updates without movement input.
    let deadline = connected_at + Duration::from_secs(8);
    while connected_at.elapsed() < Duration::from_millis(5_200) {
        assert!(Instant::now() < deadline);
        let state = observer.until(|message| matches!(message, ServerMessage::State { .. }));
        assert!(matches!(state, ServerMessage::State { players, .. }
            if players.len() == 1 && players[0].id == player_id));
    }
    observer.send(ClientMessage::Ping);
    observer.until(|message| matches!(message, ServerMessage::Pong));
    drop(observer);
    player.send(ClientMessage::Ping);
    player.until(|message| matches!(message, ServerMessage::Pong));
    let state = player.until(|message| matches!(message, ServerMessage::State { .. }));
    assert!(matches!(state, ServerMessage::State { players, .. }
        if players.len() == 1 && players[0].id == player_id));
    server.stop().unwrap();
}

#[test]
fn observer_admission_and_old_protocol_fail_with_notices_before_disconnect() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    let mut denied = Client::open(server.addr);
    denied.send(ClientMessage::Hello {
        version: PROTOCOL_VERSION,
        name: "Camera".into(),
        mode: SessionMode::Observer,
    });
    let notice = denied.until(|message| matches!(message, ServerMessage::Notice { .. }));
    assert!(matches!(notice, ServerMessage::Notice { text } if text.contains("disabled")));
    denied.until_disconnected(0);

    // Actual protocol-v2 clients omit mode; deserialize that shape far enough
    // to explain the required update instead of silently dropping the socket.
    let mut outdated = Client::open(server.addr);
    outdated
        .writer
        .write_all(b"{\"Hello\":{\"version\":2,\"name\":\"Old client\"}}\n")
        .unwrap();
    let notice = outdated.until(|message| matches!(message, ServerMessage::Notice { .. }));
    assert!(matches!(notice, ServerMessage::Notice { text }
        if text.contains("version mismatch") && text.contains(&format!("server uses {PROTOCOL_VERSION}"))));
    outdated.until_disconnected(0);

    let (mut player, welcome) = Client::connect(server.addr, "Still available");
    assert!(matches!(welcome, ServerMessage::Welcome { players, .. } if players.len() == 1));
    player.send(ClientMessage::Ping);
    player.until(|message| matches!(message, ServerMessage::Pong));
    server.stop().unwrap();
}

#[test]
fn an_observer_cannot_repeat_hello_to_create_a_player() {
    let save = TestSave::new();
    let server = spawn(save.config(true)).unwrap();
    let (mut observer, _) = Client::connect_mode(server.addr, "Camera", SessionMode::Observer);
    observer.send(ClientMessage::Hello {
        version: PROTOCOL_VERSION,
        name: "Attempted player".into(),
        mode: SessionMode::Player,
    });
    observer.until_disconnected(0);
    let (_, welcome) = Client::connect(server.addr, "Only player");
    assert!(matches!(welcome, ServerMessage::Welcome { players, .. }
        if players.len() == 1 && players[0].name == "Only player"));
    server.stop().unwrap();
}

#[test]
fn two_clients_replicate_edits_reject_bad_edits_and_reconnect_to_current_world() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    let (mut alice, _) = Client::connect(server.addr, "Alice");
    let (mut bob, _) = Client::connect(server.addr, "Bob");
    let position = nearby_air();
    alice.send(ClientMessage::Edit {
        request_id: 1,
        position,
        block: Block::Brick,
    });
    for client in [&mut alice, &mut bob] {
        let message =
            client.until(|m| matches!(m, ServerMessage::BlockChanged { request_id: 1, .. }));
        assert!(
            matches!(message, ServerMessage::BlockChanged { edit, .. } if edit.position == position && edit.block == Block::Brick)
        );
    }
    thread::sleep(Duration::from_millis(120));
    alice.send(ClientMessage::Edit {
        request_id: 2,
        position: BlockPos::new(150, position.y, 0),
        block: Block::Stone,
    });
    let rejection = alice.until(|m| matches!(m, ServerMessage::Rejected { request_id: 2, .. }));
    assert!(
        matches!(rejection, ServerMessage::Rejected { reason, .. } if reason.contains("reach"))
    );
    thread::sleep(Duration::from_millis(120));
    alice.send(ClientMessage::Edit {
        request_id: 3,
        position: BlockPos::new(0, position.y, 0),
        block: Block::Stone,
    });
    let rejection = alice.until(|m| matches!(m, ServerMessage::Rejected { request_id: 3, .. }));
    assert!(
        matches!(rejection, ServerMessage::Rejected { reason, .. } if reason.contains("character"))
    );
    drop(alice);
    let (_, welcome) = Client::connect(server.addr, "Alice again");
    assert!(
        matches!(welcome, ServerMessage::Welcome { edits, .. } if edits.iter().any(|e| e.position == position && e.block == Block::Brick))
    );
    server.stop().unwrap();
}

#[test]
fn accepted_edits_npc_override_and_world_time_survive_a_restart() {
    let save = TestSave::new();
    let server = spawn(save.config(true)).unwrap();
    let (mut client, _) = Client::connect(server.addr, "Builder");
    let position = nearby_air();
    client.send(ClientMessage::Edit {
        request_id: 10,
        position,
        block: Block::Wood,
    });
    client.until(|m| matches!(m, ServerMessage::BlockChanged { request_id: 10, .. }));
    // Persistence is already present when the accepted edit is received.
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(save.config(true).save_path).unwrap()).unwrap();
    assert_eq!(saved["edits"].as_array().unwrap().len(), 1);
    client.send(ClientMessage::Admin {
        action: AdminAction::SetNpcGoal {
            goal: Some(NpcAction::Rest),
        },
    });
    client.until(|m| matches!(m, ServerMessage::State { npc, .. } if npc.forced && npc.action == NpcAction::Rest));
    server.stop().unwrap();
    let mut config = save.config(true);
    config.seed = 999;
    let server = spawn(config).unwrap();
    let (_, welcome) = Client::connect(server.addr, "Returned");
    match welcome {
        ServerMessage::Welcome {
            seed,
            edits,
            npc,
            world_time,
            ..
        } => {
            assert_eq!(seed, 42, "Existing worlds retain their saved seed");
            assert!(
                edits
                    .iter()
                    .any(|e| e.position == position && e.block == Block::Wood)
            );
            assert!(npc.forced);
            assert_eq!(npc.action, NpcAction::Rest);
            assert!(world_time > 0.0);
        }
        _ => unreachable!(),
    }
    server.stop().unwrap();
}

#[test]
fn world_and_forager_advance_with_no_clients_and_admin_is_disabled_by_default() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    let (client, welcome) = Client::connect(server.addr, "Observer");
    let (before_time, before_position) = match welcome {
        ServerMessage::Welcome {
            world_time,
            npc,
            can_admin,
            ..
        } => {
            assert!(!can_admin);
            (world_time, npc.position)
        }
        _ => unreachable!(),
    };
    drop(client);
    // A busy runner need not simulate 400 ms during a 650 ms sleep: the server
    // deliberately avoids catching up missed ticks. Observe an autosave instead
    // of reconnecting to poll, so progress must happen with no clients present.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(save.config(false).save_path).unwrap()).unwrap();
        let saved_time = saved["world_time"].as_f64().unwrap();
        let saved_position: [f32; 3] =
            serde_json::from_value(saved["npc"]["snapshot"]["position"].clone()).unwrap();
        if saved_time > before_time + 0.4 && saved_position != before_position {
            break;
        }
        assert!(!server.is_finished(), "Server stopped before idle progress");
        assert!(
            Instant::now() < deadline,
            "No saved progress with zero clients: time {before_time} -> {saved_time}, \
             NPC {before_position:?} -> {saved_position:?}"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let (mut client, welcome) = Client::connect(server.addr, "Observer returns");
    match welcome {
        ServerMessage::Welcome {
            world_time, npc, ..
        } => {
            assert!(world_time > before_time + 0.4);
            assert_ne!(npc.position, before_position);
        }
        _ => unreachable!(),
    }
    client.send(ClientMessage::Admin {
        action: AdminAction::SetNpcGoal {
            goal: Some(NpcAction::Rest),
        },
    });
    let notice = client.until(|m| matches!(m, ServerMessage::Notice { .. }));
    assert!(matches!(notice, ServerMessage::Notice { text } if text.contains("disabled")));
    // Its autonomous action may have changed while we waited for the autosave.
    client.until(|m| matches!(m, ServerMessage::State { npc, .. } if !npc.forced));
    server.stop().unwrap();
}

#[test]
fn corrupt_save_fails_startup_without_replacing_the_original() {
    let save = TestSave::new();
    fs::create_dir_all(&save.0).unwrap();
    let config = save.config(false);
    fs::write(&config.save_path, b"{not a save").unwrap();
    match spawn(config.clone()) {
        Err(error) => assert!(error.to_string().contains("Cannot read save"), "{error}"),
        Ok(_) => panic!("Corrupt save must fail to load"),
    }
    assert_eq!(fs::read(config.save_path).unwrap(), b"{not a save");
}

#[test]
fn malformed_client_cannot_stop_the_server() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    let mut bad = TcpStream::connect(server.addr).unwrap();
    bad.write_all(b"not-json\n").unwrap();
    let (mut good, _) = Client::connect(server.addr, "Good client");
    good.send(ClientMessage::Ping);
    good.until(|m| matches!(m, ServerMessage::Pong));
    server.stop().unwrap();
}

#[test]
fn a_save_has_exactly_one_writer_and_is_unlocked_after_shutdown() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    match spawn(save.config(false)) {
        Err(error) => assert!(error.to_string().contains("Cannot lock save"), "{error}"),
        Ok(_) => panic!("A second server must not overwrite a running world's save"),
    }
    server.stop().unwrap();
    spawn(save.config(false)).unwrap().stop().unwrap();
}

#[test]
fn movement_input_expires_and_logout_removes_the_avatar() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    let (mut moving, welcome) = Client::connect(server.addr, "Moving player");
    let id = match welcome {
        ServerMessage::Welcome { session_id, .. } => session_id,
        _ => unreachable!(),
    };
    let (mut observer, _) = Client::connect(server.addr, "Observer");
    moving.send(ClientMessage::Input {
        sequence: 1,
        dt: 0.05,
        input: MoveInput {
            direction: [1.0, 0.0],
            fly: true,
            ..Default::default()
        },
        yaw: 0.0,
    });
    let started = moving.until(|message| {
        matches!(message, ServerMessage::State { players, .. }
        if players.iter().any(|p| p.id == id && p.body.velocity[0] > 0.0))
    });
    let started_at = match started {
        ServerMessage::State { world_time, .. } => world_time,
        _ => unreachable!(),
    };
    let stopped = moving.until(|message| matches!(message, ServerMessage::State { world_time, .. } if *world_time > started_at + 0.8));
    match stopped {
        ServerMessage::State { players, .. } => {
            let player = players.iter().find(|p| p.id == id).unwrap();
            assert_eq!(player.body.velocity[0], 0.0);
        }
        _ => unreachable!(),
    }
    drop(moving);
    observer.until(|message| matches!(message, ServerMessage::State { players, .. } if !players.iter().any(|p| p.id == id)));
    server.stop().unwrap();
}

#[test]
fn movement_commands_execute_exactly_once_with_their_original_durations() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    let (mut client, welcome) = Client::connect(server.addr, "Predicting player");
    let (id, world, mut expected) = match welcome {
        ServerMessage::Welcome {
            session_id,
            seed,
            players,
            ..
        } => {
            let player = players.into_iter().find(|p| p.id == session_id).unwrap();
            assert_eq!(player.last_input_sequence, 0);
            (session_id, World::new(seed), player.body)
        }
        _ => unreachable!(),
    };
    // Different frame durations and a direction requiring normalization expose
    // both server-tick resampling and duplicate controller normalization.
    let walking = MoveInput {
        direction: [1.0, 1.0],
        ..Default::default()
    };
    let commands = [
        (walking, 0.007),
        (walking, 0.019),
        (walking, 0.011),
        (walking, 0.023),
        (MoveInput::default(), 0.005),
    ];
    for (index, (input, dt)) in commands.into_iter().enumerate() {
        move_character(&world, &mut expected, input, dt);
        client.send(ClientMessage::Input {
            sequence: index as u64 + 1,
            dt,
            input,
            yaw: 0.3,
        });
    }
    let applied = client.until(|message| {
        matches!(message, ServerMessage::State { players, .. }
            if players.iter().any(|p| p.id == id && p.last_input_sequence == 5))
    });
    let applied_time = match applied {
        ServerMessage::State {
            players,
            world_time,
            ..
        } => {
            let player = players.iter().find(|p| p.id == id).unwrap();
            assert_eq!(player.body.position, expected.position);
            assert_eq!(player.body.velocity, expected.velocity);
            assert_eq!(player.body.on_ground, expected.on_ground);
            assert_eq!(player.yaw, 0.3);
            world_time
        }
        _ => unreachable!(),
    };
    // A later server tick must not simulate the last command a second time.
    let idle = client.until(|message| {
        matches!(message, ServerMessage::State { world_time, .. }
            if *world_time > applied_time + 0.09)
    });
    match idle {
        ServerMessage::State { players, .. } => {
            let player = players.iter().find(|p| p.id == id).unwrap();
            assert_eq!(player.last_input_sequence, 5);
            assert_eq!(player.body.position, expected.position);
            assert_eq!(player.body.velocity, expected.velocity);
            assert_eq!(player.body.on_ground, expected.on_ground);
        }
        _ => unreachable!(),
    }
    server.stop().unwrap();
}

#[test]
fn movement_cannot_spend_more_than_the_servers_real_time_budget() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    let (mut client, _) = Client::connect(server.addr, "Too fast");
    // Half a second is the burst allowance. More in the same receive batch
    // must disconnect before it can be acknowledged.
    let mut batch = Vec::new();
    for sequence in 1..=64 {
        serde_json::to_writer(
            &mut batch,
            &ClientMessage::Input {
                sequence,
                dt: MAX_INPUT_DT,
                input: MoveInput {
                    direction: [1.0, 0.0],
                    fly: true,
                    ..Default::default()
                },
                yaw: 0.0,
            },
        )
        .unwrap();
        batch.push(b'\n');
    }
    client.writer.write_all(&batch).unwrap();
    client.until_disconnected(2);
    let (mut good, _) = Client::connect(server.addr, "Still healthy");
    good.send(ClientMessage::Ping);
    good.until(|message| matches!(message, ServerMessage::Pong));
    server.stop().unwrap();
}

#[test]
fn a_long_frame_after_a_short_frame_fits_the_bounded_network_burst_allowance() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    let (mut client, welcome) = Client::connect(server.addr, "Slow frame");
    let (id, mut expected) = match welcome {
        ServerMessage::Welcome {
            session_id,
            players,
            ..
        } => (
            session_id,
            players
                .into_iter()
                .find(|p| p.id == session_id)
                .unwrap()
                .body,
        ),
        _ => unreachable!(),
    };
    let world = World::new(42);
    let mut batch = Vec::new();
    for (index, dt) in [0.008, MAX_INPUT_DT].into_iter().enumerate() {
        let input = MoveInput {
            direction: [1.0, 0.0],
            fly: true,
            ..Default::default()
        };
        move_character(&world, &mut expected, input, dt);
        serde_json::to_writer(
            &mut batch,
            &ClientMessage::Input {
                sequence: index as u64 + 1,
                dt,
                input,
                yaw: 0.0,
            },
        )
        .unwrap();
        batch.push(b'\n');
    }
    client.writer.write_all(&batch).unwrap();
    let state = client.until(|message| {
        matches!(message, ServerMessage::State { players, .. }
            if players.iter().any(|p| p.id == id && p.last_input_sequence == 2))
    });
    match state {
        ServerMessage::State { players, .. } => {
            let player = players.iter().find(|p| p.id == id).unwrap();
            assert_eq!(player.body.position, expected.position);
            assert_eq!(player.body.velocity, expected.velocity);
        }
        _ => unreachable!(),
    }
    server.stop().unwrap();
}

#[test]
fn invalid_movement_duration_and_sequence_are_never_acknowledged() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    for (sequence, dt) in [
        (0, 0.01),
        (2, 0.01),
        (1, 0.0),
        (1, -0.01),
        (1, MAX_INPUT_DT + 0.001),
    ] {
        let (mut client, _) = Client::connect(server.addr, "Invalid command");
        client.send(ClientMessage::Input {
            sequence,
            dt,
            input: MoveInput {
                direction: [1.0, 0.0],
                ..Default::default()
            },
            yaw: 0.0,
        });
        client.until_disconnected(0);
    }
    let (mut client, welcome) = Client::connect(server.addr, "Duplicate command");
    let id = match welcome {
        ServerMessage::Welcome { session_id, .. } => session_id,
        _ => unreachable!(),
    };
    let command = ClientMessage::Input {
        sequence: 1,
        dt: 0.01,
        input: MoveInput::default(),
        yaw: 0.0,
    };
    client.send(command.clone());
    client.until(|message| {
        matches!(message, ServerMessage::State { players, .. }
            if players.iter().any(|p| p.id == id && p.last_input_sequence == 1))
    });
    client.send(command);
    client.until_disconnected(1);
    server.stop().unwrap();
}

#[cfg(unix)]
#[test]
fn dedicated_server_flushes_and_exits_cleanly_on_termination_signals() {
    use std::process::{Command, Stdio};
    let save = TestSave::new();
    for signal in ["-TERM", "-INT"] {
        let config = save.config(false);
        let mut child = Command::new(env!("CARGO_BIN_EXE_rubblekin-server"))
            .args(["--bind", "127.0.0.1:0", "--save"])
            .arg(&config.save_path)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut startup = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut startup)
            .unwrap();
        assert!(startup.contains("listening on"), "{startup}");
        thread::sleep(Duration::from_millis(150));
        assert!(
            Command::new("/bin/kill")
                .args([signal, &child.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "Graceful shutdown failed: {status}");
                break;
            }
            if Instant::now() > deadline {
                child.kill().unwrap();
                panic!("Server failed to shut down after {signal}");
            }
            thread::sleep(Duration::from_millis(10));
        }
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(config.save_path).unwrap()).unwrap();
        assert!(saved["world_time"].as_f64().unwrap() > 0.1);
    }
}
