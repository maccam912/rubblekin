//! Regression tests for the socket transport, included as a child of `network`.

use super::*;
use rubblekin_core::{
    physics::Body,
    protocol::{NpcAction, NpcSnapshot, PlayerSnapshot},
    world::{Block, BlockEdit, BlockPos},
};
use std::{
    io::{BufRead, BufReader},
    net::TcpListener,
    sync::mpsc,
    thread,
};

#[test]
fn preserves_terrain_delta_coalesced_with_welcome() {
    for mode in [SessionMode::Player, SessionMode::Observer] {
        preserves_delta_for_mode(mode);
    }
}

#[test]
fn cancelled_handshake_reports_real_stages_and_closes_its_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let (received_hello, hello_received) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut reader = BufReader::new(socket.try_clone().unwrap());
        let mut hello = String::new();
        reader.read_line(&mut hello).unwrap();
        assert!(matches!(
            serde_json::from_str::<ClientMessage>(&hello).unwrap(),
            ClientMessage::Hello { .. }
        ));
        received_hello.send(()).unwrap();
        // The server deliberately never sends a welcome. Cancellation should
        // close this socket without waiting for the ten-second handshake timer.
        assert_eq!(socket.read(&mut [0u8; 1]).unwrap(), 0);
    });
    let mut stages = Vec::new();
    let result = Connection::connect_with_progress(
        &address,
        "Tester".into(),
        SessionMode::Player,
        |stage| {
            if stages.last() != Some(&stage) {
                stages.push(stage);
            }
            if stage == ConnectionStage::AwaitingWelcome && hello_received.try_recv().is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Cancelled by player",
                ));
            }
            Ok(())
        },
    );
    assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::Interrupted));
    assert_eq!(
        stages,
        [
            ConnectionStage::ResolvingAddress,
            ConnectionStage::Connecting,
            ConnectionStage::AwaitingWelcome
        ]
    );
    server.join().unwrap();
}

fn preserves_delta_for_mode(mode: SessionMode) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
    let address = listener.local_addr().unwrap().to_string();
    let (release, released) = mpsc::channel::<()>();
    let edit = BlockEdit {
        position: BlockPos::new(3, 10, 2),
        block: Block::Wood,
    };
    let worker = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut hello = String::new();
        BufReader::new(socket.try_clone().unwrap())
            .read_line(&mut hello)
            .unwrap();
        assert!(matches!(
            serde_json::from_str::<ClientMessage>(&hello).unwrap(),
            ClientMessage::Hello { version: PROTOCOL_VERSION, mode: actual, ref name }
                if actual == mode && name == "Joiner"
        ));

        let welcome = ServerMessage::Welcome {
            version: PROTOCOL_VERSION,
            session_id: 17,
            mode,
            seed: 42,
            generation: rubblekin_core::world::WorldGeneration::ValleyV1,
            edits: Vec::new(),
            players: if mode == SessionMode::Player {
                vec![PlayerSnapshot {
                    id: 17,
                    name: "Joiner".into(),
                    body: Body::new([0.25, 2.52, 0.25]),
                    yaw: 0.0,
                    last_input_sequence: 0,
                    movement_epoch: 0,
                    ride: None,
                    deck_position: None,
                }]
            } else {
                Vec::new()
            },
            npc: NpcSnapshot {
                name: "Moss".into(),
                position: [3.0, 2.5, 0.0],
                hunger: 60.0,
                energy: 80.0,
                action: NpcAction::Forage,
                reason: "Looking for berries".into(),
                berries: 0,
                forced: false,
                target: Some([6.0, 2.5, 5.0]),
            },
            residents: Vec::new(),
            villages: Vec::new(),
            world_time: 0.0,
            can_admin: false,
        };
        let changed = ServerMessage::BlockChanged {
            request_id: 9,
            player_id: 3,
            edit,
        };
        // The new edit follows the welcome snapshot in the same socket write.
        // Returning from the handshake must preserve the rest of this batch.
        let mut batch = serde_json::to_vec(&welcome).unwrap();
        batch.push(b'\n');
        batch.extend(serde_json::to_vec(&changed).unwrap());
        batch.push(b'\n');
        socket.write_all(&batch).unwrap();
        let _ = released.recv_timeout(Duration::from_secs(5));
    });

    let (mut connection, welcome) = Connection::connect(&address, "Joiner".into(), mode).unwrap();
    assert!(matches!(
        welcome,
        ServerMessage::Welcome { session_id: 17, mode: actual, .. } if actual == mode
    ));
    // Allow normal TCP fragmentation while ensuring the accepted terrain delta
    // is delivered exactly once after the handshake, never silently discarded.
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut received = Vec::new();
    while Instant::now() < deadline && received.is_empty() {
        received.extend(connection.poll());
        if received.is_empty() {
            thread::sleep(Duration::from_millis(5));
        }
    }
    assert_eq!(
        received.len(),
        1,
        "coalesced terrain edit was lost or duplicated"
    );
    assert!(matches!(
        &received[0],
        ServerMessage::BlockChanged { edit: actual, .. } if *actual == edit
    ));
    assert!(
        connection.poll().is_empty(),
        "edit delivered more than once"
    );
    release.send(()).unwrap();
    worker.join().unwrap();
}

#[test]
fn localhost_prediction_stays_put_when_delayed_movement_acknowledgments_arrive() {
    use crate::prediction::Prediction;
    use rubblekin_core::{physics::MoveInput, world::World};
    use rubblekin_server::{ServerConfig, spawn};

    let directory = std::env::temp_dir().join(format!(
        "rubblekin-client-movement-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let server = spawn(ServerConfig {
        bind_addr: "127.0.0.1:0".into(),
        save_path: directory.join("world.json"),
        generation: rubblekin_core::world::WorldGeneration::ValleyV1,
        ..Default::default()
    })
    .unwrap();
    let (mut connection, welcome) = Connection::connect(
        &server.addr.to_string(),
        "Walker".into(),
        SessionMode::Player,
    )
    .unwrap();
    let ServerMessage::Welcome {
        session_id,
        seed,
        edits,
        players,
        ..
    } = welcome
    else {
        panic!("expected welcome")
    };
    let world = World::from_edits(seed, &edits).unwrap();
    let mut body = players
        .iter()
        .find(|p| p.id == session_id)
        .unwrap()
        .body
        .clone();
    let mut prediction = Prediction::default();
    let mut delayed = VecDeque::new();
    let mut acknowledged = 0;
    let mut reconcile = |prediction: &mut Prediction, body: &mut Body, state: PlayerSnapshot| {
        let before = body.clone();
        acknowledged = state.last_input_sequence;
        prediction.reconcile(&world, body, &state, &[]).unwrap();
        assert_eq!(
            body.position, before.position,
            "localhost snapshot moved the predicted player"
        );
        assert_eq!(body.velocity, before.velocity);
        assert_eq!(body.on_ground, before.on_ground);
    };
    for frame in 0..90 {
        let dt = [1.0 / 120.0, 1.0 / 90.0, 1.0 / 60.0][frame % 3];
        thread::sleep(Duration::from_secs_f32(dt));
        for message in connection.poll() {
            if let ServerMessage::State { players, .. } = message {
                delayed.push_back((
                    Instant::now() + Duration::from_millis(80),
                    players.into_iter().find(|p| p.id == session_id).unwrap(),
                ));
            }
        }
        while delayed.front().is_some_and(|(at, _)| *at <= Instant::now()) {
            let (_, state) = delayed.pop_front().unwrap();
            reconcile(&mut prediction, &mut body, state);
        }
        // Normal grounded movement followed by W release, with real server ticks.
        let input = MoveInput {
            direction: if frame < 60 { [1.0, 0.0] } else { [0.0, 0.0] },
            ..Default::default()
        };
        connection.send(
            prediction
                .advance(&world, &mut body, input, 0.0, dt, &[])
                .unwrap(),
        );
        assert!(connection.error.is_none(), "{:?}", connection.error);
    }
    // Apply the remaining ordered snapshots and the final acknowledgment.
    for (_, state) in delayed {
        reconcile(&mut prediction, &mut body, state);
    }
    let deadline = Instant::now() + Duration::from_millis(400);
    while Instant::now() < deadline {
        let mut done = false;
        for message in connection.poll() {
            if let ServerMessage::State { players, .. } = message {
                let state = players.into_iter().find(|p| p.id == session_id).unwrap();
                done |= state.last_input_sequence == 90;
                reconcile(&mut prediction, &mut body, state);
            }
        }
        if done {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(acknowledged, 90, "server did not acknowledge the stop");
    assert_eq!(body.velocity[0], 0.0);
    drop(connection);
    server.stop().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn localhost_self_and_other_player_teleports_replace_pending_prediction() {
    use crate::prediction::Prediction;
    use rubblekin_core::{physics::MoveInput, world::World};
    use rubblekin_server::{ServerConfig, spawn};

    fn player_state(
        connection: &mut Connection,
        id: u64,
        epoch: u64,
        acknowledged: u64,
    ) -> PlayerSnapshot {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            for message in connection.poll() {
                if let ServerMessage::State { players, .. } = message
                    && let Some(player) = players.into_iter().find(|player| {
                        player.id == id
                            && player.movement_epoch == epoch
                            && player.last_input_sequence == acknowledged
                    })
                {
                    return player;
                }
            }
            assert!(connection.error.is_none(), "{:?}", connection.error);
            thread::sleep(Duration::from_millis(5));
        }
        panic!("server did not send epoch {epoch}, acknowledgment {acknowledged}");
    }

    let directory = std::env::temp_dir().join(format!(
        "rubblekin-client-teleport-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let server = spawn(ServerConfig {
        bind_addr: "127.0.0.1:0".into(),
        save_path: directory.join("world.json"),
        generation: rubblekin_core::world::WorldGeneration::ValleyV1,
        allow_admin: true,
        ..Default::default()
    })
    .unwrap();
    let address = server.addr.to_string();
    let (mut ian, welcome) =
        Connection::connect(&address, "Ian".into(), SessionMode::Player).unwrap();
    let (mut violet, _) =
        Connection::connect(&address, "Violet".into(), SessionMode::Player).unwrap();
    let ServerMessage::Welcome {
        session_id,
        seed,
        edits,
        players,
        ..
    } = welcome
    else {
        panic!("expected welcome")
    };
    let world = World::from_edits(seed, &edits).unwrap();
    let initial = players
        .into_iter()
        .find(|player| player.id == session_id)
        .unwrap();
    let mut prediction = Prediction::from_snapshot(&initial);
    let mut body = initial.body.clone();
    let walking = MoveInput {
        direction: [1.0, 0.0],
        fly: true,
        ..Default::default()
    };
    for (epoch, command) in [
        (1, "teleport 12.25 20 12.25"),
        (2, "teleport Ian 24.25 20 24.25"),
    ] {
        // Keep both commands pending until the authoritative teleport arrives.
        let pending_walk = prediction
            .advance(&world, &mut body, walking, 0.0, 0.02, &[])
            .unwrap();
        let pending_jump = prediction
            .advance(
                &world,
                &mut body,
                MoveInput {
                    direction: [1.0, 0.0],
                    jump: true,
                    ..Default::default()
                },
                0.0,
                0.02,
                &[],
            )
            .unwrap();
        let request = ClientMessage::AdminCommand {
            command: command.into(),
        };
        if epoch == 1 {
            ian.send(request);
        } else {
            violet.send(request);
        }
        let teleported = player_state(&mut ian, session_id, epoch, 0);
        // Commands sent from the previous location must be ignored even when
        // they reach the server after another player's teleport command.
        ian.send(pending_walk);
        ian.send(pending_jump);
        prediction
            .reconcile(&world, &mut body, &teleported, &[])
            .unwrap();
        assert_eq!(body.position, teleported.body.position);
        assert_eq!(body.velocity, teleported.body.velocity);
        assert_eq!(prediction.movement_epoch(), epoch);

        let next = prediction
            .advance(&world, &mut body, walking, 0.0, 0.02, &[])
            .unwrap();
        assert!(matches!(
            next,
            ClientMessage::Input {
                movement_epoch: actual,
                sequence: 1,
                ..
            } if actual == epoch
        ));
        ian.send(next);
        let acknowledged = player_state(&mut ian, session_id, epoch, 1);
        let before = body.clone();
        prediction
            .reconcile(&world, &mut body, &acknowledged, &[])
            .unwrap();
        assert_eq!(body.position, before.position);
        assert_eq!(body.velocity, before.velocity);
        assert_eq!(body.on_ground, before.on_ground);
        assert!(ian.error.is_none(), "{:?}", ian.error);
    }
    drop(ian);
    drop(violet);
    server.stop().unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}
