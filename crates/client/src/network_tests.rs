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
            ClientMessage::Hello { .. }
        ));

        let welcome = ServerMessage::Welcome {
            version: PROTOCOL_VERSION,
            player_id: 17,
            seed: 42,
            edits: Vec::new(),
            players: vec![PlayerSnapshot {
                id: 17,
                name: "Joiner".into(),
                body: Body::new([0.25, 2.52, 0.25]),
                yaw: 0.0,
                last_input_sequence: 0,
            }],
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

    let (mut connection, welcome) = Connection::connect(&address, "Joiner".into()).unwrap();
    assert!(matches!(
        welcome,
        ServerMessage::Welcome { player_id: 17, .. }
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
        ..Default::default()
    })
    .unwrap();
    let (mut connection, welcome) =
        Connection::connect(&server.addr.to_string(), "Walker".into()).unwrap();
    let ServerMessage::Welcome {
        player_id,
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
        .find(|p| p.id == player_id)
        .unwrap()
        .body
        .clone();
    let mut prediction = Prediction::default();
    let mut delayed = VecDeque::new();
    let mut acknowledged = 0;
    let mut reconcile = |prediction: &mut Prediction, body: &mut Body, state: PlayerSnapshot| {
        let before = body.clone();
        acknowledged = state.last_input_sequence;
        prediction.reconcile(&world, body, &state).unwrap();
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
                    players.into_iter().find(|p| p.id == player_id).unwrap(),
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
                .advance(&world, &mut body, input, 0.0, dt)
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
                let state = players.into_iter().find(|p| p.id == player_id).unwrap();
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
