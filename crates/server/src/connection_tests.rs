use super::*;
use std::io::BufRead;

fn pair() -> (Connection, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    peer.set_nonblocking(true).unwrap();
    (Connection::new(listener.accept().unwrap().0).unwrap(), peer)
}

fn state(time: f64) -> ServerMessage {
    let world = World::new(42);
    let mut npc = npc::Forager::new(&world).snapshot;
    // Similar to a populated world's full snapshots, without generating an island.
    npc.reason = "x".repeat(32 * 1024);
    ServerMessage::State {
        players: Vec::new(),
        npc,
        residents: Vec::new(),
        villages: Vec::new(),
        world_time: time,
    }
}

fn wildlife() -> ServerMessage {
    ServerMessage::WildlifeState {
        animals: Vec::new(),
        habitats: Vec::new(),
    }
}

fn drain(
    connection: &mut Connection,
    peer: &mut TcpStream,
    last: &ServerMessage,
) -> Vec<ServerMessage> {
    let mut suffix = serde_json::to_vec(last).unwrap();
    suffix.push(b'\n');
    let mut received = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !received.ends_with(&suffix) && Instant::now() < deadline {
        connection.flush().unwrap();
        let mut buffer = [0; 65536];
        match peer.read(&mut buffer) {
            Ok(0) => panic!("loading client disconnected"),
            Ok(count) => received.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => thread::yield_now(),
            Err(error) => panic!("{error}"),
        }
    }
    assert!(received.ends_with(&suffix), "socket did not recover");
    assert!(connection.outgoing.is_empty());
    received
        .lines()
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect()
}

#[test]
fn unread_loading_client_survives_more_than_a_minute_of_snapshots_and_receives_events() {
    let (mut connection, mut peer) = pair();
    let mut snapshot = state(0.0);
    let edit = BlockEdit {
        position: BlockPos::new(3, 10, 2),
        block: Block::Wood,
    };
    // 75 seconds at 20 Hz; no sleeps or real-time deadline. The real socket
    // fills while the peer is busy loading and deliberately does not read.
    for tick in 0..1500 {
        let ServerMessage::State { world_time, .. } = &mut snapshot else {
            unreachable!()
        };
        *world_time = tick as f64 * 0.05;
        connection.send(&snapshot);
        if tick % 4 == 0 {
            connection.send(&wildlife());
        }
        if tick == 500 {
            connection.send(&ServerMessage::BlockChanged {
                request_id: 19,
                player_id: 7,
                edit,
            });
        }
        connection.flush().unwrap();
        assert!(
            !connection.dead,
            "loading client disconnected at tick {tick}"
        );
    }
    assert!(connection.queued_bytes < 100 * 1024);
    connection.send(&ServerMessage::Pong);
    let messages = drain(&mut connection, &mut peer, &ServerMessage::Pong);
    assert_eq!(
        messages
            .iter()
            .filter(|message| matches!(message,
                ServerMessage::BlockChanged { request_id: 19, edit: actual, .. } if *actual == edit
            ))
            .count(),
        1,
        "terrain changes must arrive exactly once"
    );
    assert!(messages.iter().any(|message| matches!(message,
        ServerMessage::State { world_time, .. } if *world_time == 1499.0 * 0.05
    )));
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, ServerMessage::WildlifeState { .. }))
    );
    assert!(matches!(messages.last(), Some(ServerMessage::Pong)));
}

#[test]
fn partially_written_snapshot_finishes_before_new_snapshots_and_reliable_events() {
    let (mut connection, mut peer) = pair();
    connection.send(&state(1.0));
    // Reproduce a short socket write halfway through a JSON frame.
    let prefix = serde_json::to_vec(&state(1.0)).unwrap()[..17].to_vec();
    connection.socket.write_all(&prefix).unwrap();
    connection.write_offset = prefix.len();
    connection.queued_bytes -= prefix.len();
    connection.send(&state(2.0));
    connection.send(&wildlife());
    connection.send(&ServerMessage::Notice {
        text: "first event".into(),
    });
    connection.send(&state(3.0));
    connection.send(&ServerMessage::Pong);
    connection.send(&wildlife());
    connection.send(&state(4.0));
    let messages = drain(&mut connection, &mut peer, &state(4.0));
    assert_eq!(messages.len(), 5);
    assert!(matches!(
        messages[0],
        ServerMessage::State {
            world_time: 1.0,
            ..
        }
    ));
    assert!(matches!(&messages[1], ServerMessage::Notice { text } if text == "first event"));
    assert!(matches!(messages[2], ServerMessage::Pong));
    assert!(matches!(messages[3], ServerMessage::WildlifeState { .. }));
    assert!(matches!(
        messages[4],
        ServerMessage::State {
            world_time: 4.0,
            ..
        }
    ));
    assert_eq!(connection.queued_bytes, 0);
    assert_eq!(connection.write_offset, 0);
}

#[test]
fn reliable_event_backlog_still_disconnects_at_the_existing_limit() {
    let (mut connection, _peer) = pair();
    let event = ServerMessage::Notice {
        text: "x".repeat(64 * 1024),
    };
    for _ in 0..129 {
        connection.send(&event);
        if connection.dead {
            break;
        }
    }
    assert!(connection.dead);
    assert!(connection.queued_bytes <= MAX_OUTBOUND_BYTES);
}
