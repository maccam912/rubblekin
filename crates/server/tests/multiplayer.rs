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
    physics::{MoveInput, PLAYER_RADIUS, characters_overlap, move_character},
    protocol::*,
    world::{Block, BlockPos, CELL_SIZE, World, WorldGeneration},
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
    fn connect_profile(addr: SocketAddr, profile: &str) -> (Self, ServerMessage) {
        let mut client = Self::open(addr);
        client.send(ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            name: "Courier".into(),
            mode: SessionMode::Player,
            profile_id: Some(profile.into()),
        });
        let welcome = client.until(|message| matches!(message, ServerMessage::Welcome { .. }));
        (client, welcome)
    }

    fn teleport(&mut self, position: [f32; 3]) {
        self.send(ClientMessage::AdminCommand {
            command: format!("tp {} {} {}", position[0], position[1], position[2]),
        });
        let reply =
            self.until(|message| matches!(message, ServerMessage::AdminCommandResult { .. }));
        assert!(
            matches!(reply, ServerMessage::AdminCommandResult {text} if text.contains("Teleported"))
        );
    }

    fn market(
        &mut self,
        request_id: u64,
        village_id: Option<u32>,
        revision: u64,
        action: rubblekin_core::economy::MarketAction,
    ) -> ServerMessage {
        thread::sleep(Duration::from_millis(110));
        self.send(ClientMessage::Market {
            request_id,
            village_id,
            revision,
            action,
        });
        self.until(|message| matches!(message, ServerMessage::MarketState {request_id: response, ..} if *response == request_id))
    }

    fn connect(addr: SocketAddr, name: &str) -> (Self, ServerMessage) {
        Self::connect_mode(addr, name, SessionMode::Player)
    }

    fn connect_mode(addr: SocketAddr, name: &str, mode: SessionMode) -> (Self, ServerMessage) {
        let mut client = Self::open(addr);
        client.send(ClientMessage::Hello {
            profile_id: None,
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
fn market_delivery_is_durable_private_and_resumes_without_teleporting_cargo() {
    use rubblekin_core::economy::MarketAction;
    const PROFILE: &str = "00000000000000000000000000000042";
    let save = TestSave::new();
    let mut config = save.config(true);
    config.generation = WorldGeneration::GeographyV4;
    let world = World::generate(42, config.generation);
    let origin = &world.settlements().unwrap().villages[0];
    let server = spawn(config.clone()).unwrap();
    let (mut client, welcome) = Client::connect_profile(server.addr, PROFILE);
    assert!(!serde_json::to_string(&welcome).unwrap().contains(PROFILE));
    let initial =
        client.until(|message| matches!(message, ServerMessage::MarketState { request_id: 0, .. }));
    assert!(
        matches!(initial, ServerMessage::MarketState {ledger, ..} if ledger.coins == 0 && ledger.cargo_total() == 0)
    );

    let mut duplicate = Client::open(server.addr);
    duplicate.send(ClientMessage::Hello {
        version: PROTOCOL_VERSION,
        name: "Other name".into(),
        mode: SessionMode::Player,
        profile_id: Some(PROFILE.into()),
    });
    let denied = duplicate.until(|message| matches!(message, ServerMessage::Notice { .. }));
    assert!(matches!(denied, ServerMessage::Notice {text} if text.contains("already playing")));
    drop(duplicate);

    client.teleport(origin.market);
    let quote = client.market(1, Some(origin.id), 0, MarketAction::View);
    let ServerMessage::MarketState {
        market: Some(view),
        accepted: true,
        ..
    } = quote
    else {
        panic!("{quote:?}")
    };
    let offer = view.delivery_offer.unwrap();
    let accepted = client.market(
        2,
        Some(origin.id),
        0,
        MarketAction::AcceptDelivery {
            offer: offer.clone(),
        },
    );
    assert!(
        matches!(accepted, ServerMessage::MarketState {accepted: true, ref ledger, ..} if ledger.delivery.as_ref() == Some(&offer))
    );
    let on_disk: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(on_disk["profiles"][PROFILE]["ledger"]["revision"], 1);
    assert_eq!(
        on_disk["profiles"][PROFILE]["ledger"]["delivery"]["amount"],
        6
    );
    let repeat = client.market(
        3,
        Some(origin.id),
        0,
        MarketAction::AcceptDelivery {
            offer: offer.clone(),
        },
    );
    assert!(
        matches!(repeat, ServerMessage::MarketState {accepted: false, ref ledger, ..} if ledger.revision == 1)
    );
    let remote = client.market(4, Some(offer.destination), 1, MarketAction::Deliver);
    assert!(
        matches!(remote, ServerMessage::MarketState {accepted: false, ref ledger, ..} if ledger.coins == 0)
    );
    drop(client);
    server.stop().unwrap();

    let server = spawn(config.clone()).unwrap();
    let (mut client, welcome) = Client::connect_profile(server.addr, PROFILE);
    let ServerMessage::Welcome {
        session_id,
        players,
        ..
    } = welcome
    else {
        panic!()
    };
    let player = players
        .iter()
        .find(|player| player.id == session_id)
        .unwrap();
    assert!(rubblekin_core::economy::can_reach_market(
        player.body.position,
        origin.market
    ));
    let resumed =
        client.until(|message| matches!(message, ServerMessage::MarketState { request_id: 0, .. }));
    assert!(
        matches!(resumed, ServerMessage::MarketState {ref ledger, ..} if ledger.delivery.as_ref() == Some(&offer))
    );
    let destination = world
        .settlements()
        .unwrap()
        .villages
        .iter()
        .find(|village| village.id == offer.destination)
        .unwrap();
    // Admin movement isolates authoritative reach/transaction checks from the
    // separately tested walking controller and the native physical-trip check.
    client.teleport(destination.market);
    let delivered = client.market(5, Some(destination.id), 1, MarketAction::Deliver);
    assert!(
        matches!(delivered, ServerMessage::MarketState {accepted: true, ref ledger, ..} if ledger.coins == 12 && ledger.delivery.is_none())
    );
    let on_disk: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(on_disk["profiles"][PROFILE]["ledger"]["coins"], 12);
    let repeated = client.market(6, Some(destination.id), 1, MarketAction::Deliver);
    assert!(
        matches!(repeated, ServerMessage::MarketState {accepted: false, ref ledger, ..} if ledger.coins == 12)
    );
    let (mut other, _) = Client::connect_profile(server.addr, "00000000000000000000000000000043");
    let own =
        other.until(|message| matches!(message, ServerMessage::MarketState { request_id: 0, .. }));
    assert!(
        matches!(own, ServerMessage::MarketState {ledger, ..} if ledger.coins == 0 && ledger.delivery.is_none())
    );
    drop(other);
    drop(client);
    server.stop().unwrap();
}

#[test]
fn market_save_failure_never_acknowledges_or_replaces_the_canonical_ledger() {
    use rubblekin_core::economy::MarketAction;
    const PROFILE: &str = "00000000000000000000000000000044";
    let save = TestSave::new();
    let mut config = save.config(true);
    config.generation = WorldGeneration::GeographyV4;
    let world = World::generate(42, config.generation);
    let origin = &world.settlements().unwrap().villages[0];
    let server = spawn(config.clone()).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    client.teleport(origin.market);
    let reply = client.market(1, Some(origin.id), 0, MarketAction::View);
    let ServerMessage::MarketState {
        market: Some(view), ..
    } = reply
    else {
        panic!("{reply:?}")
    };
    let offer = view.delivery_offer.unwrap();
    let before = fs::read(&config.save_path).unwrap();
    let temporary = save
        .0
        .join(format!(".world.json.{}.tmp", std::process::id()));
    fs::create_dir(&temporary).unwrap();
    thread::sleep(Duration::from_millis(110));
    client.send(ClientMessage::Market {
        request_id: 2,
        village_id: Some(origin.id),
        revision: 0,
        action: MarketAction::AcceptDelivery { offer },
    });
    loop {
        let mut line = String::new();
        match client.reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => assert!(!matches!(
                serde_json::from_str::<ServerMessage>(&line).unwrap(),
                ServerMessage::MarketState {
                    request_id: 2,
                    accepted: true,
                    ..
                }
            )),
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(error) => panic!("Expected a save-failure disconnect: {error}"),
        }
    }
    assert!(server.stop().is_err());
    assert_eq!(fs::read(&config.save_path).unwrap(), before);
    fs::remove_dir(&temporary).unwrap();
    let restarted = spawn(config).unwrap();
    let (mut client, _) = Client::connect_profile(restarted.addr, PROFILE);
    let state =
        client.until(|message| matches!(message, ServerMessage::MarketState { request_id: 0, .. }));
    assert!(
        matches!(state, ServerMessage::MarketState {ledger, ..} if ledger.revision == 0 && ledger.delivery.is_none())
    );
    drop(client);
    restarted.stop().unwrap();
}

fn assert_distinct_bodies(positions: impl IntoIterator<Item = [f32; 3]>) {
    let positions: Vec<_> = positions.into_iter().collect();
    for (index, position) in positions.iter().enumerate() {
        for other in &positions[index + 1..] {
            assert!(
                !characters_overlap(*position, *other),
                "Bodies overlap: {position:?} / {other:?}"
            );
        }
    }
}

#[test]
fn console_permission_results_reach_players_and_read_only_observers_over_tcp() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    let (mut player, welcome) = Client::connect(server.addr, "Ian");
    let initial = match welcome {
        ServerMessage::Welcome {
            players, can_admin, ..
        } => {
            assert!(!can_admin);
            players[0].clone()
        }
        _ => unreachable!(),
    };
    player.send(ClientMessage::AdminCommand {
        command: "teleport 20 50 20".into(),
    });
    assert!(matches!(
        player.until(|message| matches!(message, ServerMessage::AdminCommandResult { .. })),
        ServerMessage::AdminCommandResult { text } if text.contains("disabled")
    ));
    match player.until(|message| matches!(message, ServerMessage::State { .. })) {
        ServerMessage::State { players, .. } => {
            assert_eq!(players[0].body.position[0], initial.body.position[0]);
            assert_eq!(players[0].body.position[2], initial.body.position[2]);
            assert_eq!(players[0].movement_epoch, 0);
        }
        _ => unreachable!(),
    }
    drop(player);
    server.stop().unwrap();

    let server = spawn(save.config(true)).unwrap();
    let (mut player, _) = Client::connect(server.addr, "Violet");
    let (mut observer, welcome) =
        Client::connect_mode(server.addr, "Camera", SessionMode::Observer);
    assert!(matches!(
        welcome,
        ServerMessage::Welcome {
            can_admin: false,
            ..
        }
    ));
    observer.send(ClientMessage::AdminCommand {
        command: "teleport Violet 20 50 20".into(),
    });
    assert!(matches!(
        observer.until(|message| matches!(message, ServerMessage::AdminCommandResult { .. })),
        ServerMessage::AdminCommandResult { text } if text.contains("read-only")
    ));
    player.send(ClientMessage::AdminCommand {
        command: "tp 20 50 20".into(),
    });
    assert!(matches!(
        player.until(|message| matches!(message, ServerMessage::AdminCommandResult { .. })),
        ServerMessage::AdminCommandResult { text } if text.starts_with("Teleported Violet")
    ));
    assert!(matches!(
        observer.until(|message| matches!(message, ServerMessage::State { players, .. } if players.iter().any(|p| p.movement_epoch == 1))),
        ServerMessage::State { players, .. } if players.len() == 1 && players[0].body.position[0] == 20.0 && players[0].body.position[2] == 20.0
    ));
    drop(player);
    drop(observer);
    server.stop().unwrap();
}

#[test]
fn joining_players_get_free_space_and_server_movement_stops_at_other_players() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    let (mut first, welcome) = Client::connect(server.addr, "First body");
    let first_id = match welcome {
        ServerMessage::Welcome { session_id, .. } => session_id,
        _ => unreachable!(),
    };
    let (mut second, welcome) = Client::connect(server.addr, "Second body");
    let (second_id, mut positions) = match welcome {
        ServerMessage::Welcome {
            session_id,
            players,
            npc,
            ..
        } => {
            for (index, player) in players.iter().enumerate() {
                assert!(!characters_overlap(player.body.position, npc.position));
                for other in &players[index + 1..] {
                    assert!(!characters_overlap(
                        player.body.position,
                        other.body.position
                    ));
                }
            }
            (session_id, players)
        }
        _ => unreachable!(),
    };
    let first_position = positions
        .iter()
        .find(|p| p.id == first_id)
        .unwrap()
        .body
        .position;
    let second_position = positions
        .iter()
        .find(|p| p.id == second_id)
        .unwrap()
        .body
        .position;
    // Align the second body beside the first. Neither spawn overlaps, and this
    // movement remains outside the first body's horizontal extent.
    let z_distance = first_position[2] - second_position[2];
    second.send(ClientMessage::Input {
        movement_epoch: 0,
        sequence: 1,
        dt: z_distance.abs() / 7.0,
        input: MoveInput {
            direction: [0.0, z_distance.signum()],
            fly: true,
            ..Default::default()
        },
        yaw: 0.0,
    });
    if let ServerMessage::State { players, .. } = second.until(|message| {
        matches!(message, ServerMessage::State { players, .. }
            if players.iter().any(|p| p.id == second_id && p.last_input_sequence == 1))
    }) {
        positions = players;
    }
    let blocker = positions
        .iter()
        .find(|p| p.id == second_id)
        .unwrap()
        .body
        .position;
    assert!((blocker[2] - first_position[2]).abs() < 0.001);
    for sequence in 1..=2 {
        first.send(ClientMessage::Input {
            movement_epoch: 0,
            sequence,
            dt: MAX_INPUT_DT,
            input: MoveInput {
                direction: [-1.0, 0.0],
                fly: true,
                ..Default::default()
            },
            yaw: 0.0,
        });
    }
    let state = first.until(|message| {
        matches!(message, ServerMessage::State { players, .. }
            if players.iter().any(|p| p.id == first_id && p.last_input_sequence == 2))
    });
    if let ServerMessage::State { players, .. } = state {
        let mover = players
            .iter()
            .find(|p| p.id == first_id)
            .unwrap()
            .body
            .position;
        let blocker = players
            .iter()
            .find(|p| p.id == second_id)
            .unwrap()
            .body
            .position;
        assert!(!characters_overlap(mover, blocker));
        assert!(mover[0] >= blocker[0] + PLAYER_RADIUS * 2.0 - 0.001);
        assert!(mover[0] < first_position[0]);
    }
    server.stop().unwrap();
}

#[test]
fn server_movement_cannot_pass_through_a_resting_npc() {
    let save = TestSave::new();
    let server = spawn(save.config(true)).unwrap();
    let (mut client, welcome) = Client::connect(server.addr, "NPC collision");
    let id = match welcome {
        ServerMessage::Welcome { session_id, .. } => session_id,
        _ => unreachable!(),
    };
    client.send(ClientMessage::Admin {
        action: AdminAction::SetNpcGoal {
            goal: Some(NpcAction::Rest),
        },
    });
    client.until(|message| {
        matches!(message, ServerMessage::State { npc, .. }
        if npc.forced && npc.action == NpcAction::Rest)
    });
    for sequence in 1..=2 {
        client.send(ClientMessage::Input {
            movement_epoch: 0,
            sequence,
            dt: MAX_INPUT_DT,
            input: MoveInput {
                direction: [1.0, 0.0],
                fly: true,
                ..Default::default()
            },
            yaw: 0.0,
        });
    }
    let state = client.until(|message| {
        matches!(message, ServerMessage::State { players, .. }
            if players.iter().any(|p| p.id == id && p.last_input_sequence == 2))
    });
    if let ServerMessage::State { players, npc, .. } = state {
        let position = players.iter().find(|p| p.id == id).unwrap().body.position;
        assert!(!characters_overlap(position, npc.position));
        assert!(position[0] <= npc.position[0] - PLAYER_RADIUS * 2.0 + 0.001);
        assert!(position[0] > 1.0);
    }
    server.stop().unwrap();
}

#[test]
fn old_village_save_recovers_coincident_residents_before_welcoming_players() {
    let save = TestSave::new();
    let mut config = save.config(true);
    config.generation = WorldGeneration::GeographyV3;
    let server = spawn(config.clone()).unwrap();
    server.stop().unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    let world = World::generate(42, WorldGeneration::GeographyV3);
    let position = serde_json::to_value(world.spawn_position()).unwrap();
    // Old residents had no needs and could coexist at the same waypoint.
    // Recreate that case beside player spawn with valid saved route identities.
    for resident in value["villages"]["residents"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .take(2)
    {
        resident["snapshot"]["position"] = position.clone();
        resident["body"]["position"] = position.clone();
        resident["phase"] = "ToWork".into();
        resident["waypoint"] = 0.into();
        resident["elapsed"] = 0.into();
        let snapshot = resident["snapshot"].as_object_mut().unwrap();
        snapshot.remove("hunger");
        snapshot.remove("energy");
        snapshot.remove("reason");
    }
    fs::write(&config.save_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let server = spawn(config).unwrap();
    let (_client, welcome) = Client::connect(server.addr, "Resident collision spawn");
    if let ServerMessage::Welcome {
        players,
        npc,
        residents,
        ..
    } = welcome
    {
        assert_distinct_bodies(
            players
                .iter()
                .map(|player| player.body.position)
                .chain(std::iter::once(npc.position))
                .chain(residents.iter().map(|resident| resident.position)),
        );
        assert!(
            residents
                .iter()
                .all(|resident| resident.hunger.is_finite() && resident.energy.is_finite())
        );
    }
    server.stop().unwrap();
}

#[test]
fn idle_gravity_lands_on_another_player_without_merging_bodies() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    let (mut first, welcome) = Client::connect(server.addr, "Landing body");
    let id = match welcome {
        ServerMessage::Welcome { session_id, .. } => session_id,
        _ => unreachable!(),
    };
    let (_second, welcome) = Client::connect(server.addr, "Supporting body");
    let (start, target, second_id) = match welcome {
        ServerMessage::Welcome {
            players,
            session_id,
            ..
        } => (
            players
                .iter()
                .find(|player| player.id == id)
                .unwrap()
                .body
                .position,
            players
                .iter()
                .find(|player| player.id == session_id)
                .unwrap()
                .body
                .position,
            session_id,
        ),
        _ => unreachable!(),
    };
    first.send(ClientMessage::Input {
        movement_epoch: 0,
        sequence: 1,
        dt: MAX_INPUT_DT,
        input: MoveInput {
            fly: true,
            vertical: 1.0,
            ..Default::default()
        },
        yaw: 0.0,
    });
    let direction = [target[0] - start[0], target[2] - start[2]];
    let distance = direction[0].hypot(direction[1]);
    first.send(ClientMessage::Input {
        movement_epoch: 0,
        sequence: 2,
        dt: distance / 7.0,
        input: MoveInput {
            fly: true,
            direction: direction.map(|value| value / distance),
            ..Default::default()
        },
        yaw: 0.0,
    });
    let state = first.until(|message| matches!(message, ServerMessage::State { players, .. }
        if players.iter().any(|player| player.id == id && player.last_input_sequence == 2 && player.body.on_ground)));
    if let ServerMessage::State { players, .. } = state {
        let landed = players
            .iter()
            .find(|player| player.id == id)
            .unwrap()
            .body
            .position;
        let supporting = players
            .iter()
            .find(|player| player.id == second_id)
            .unwrap()
            .body
            .position;
        assert!(!characters_overlap(landed, supporting));
        assert!((landed[1] - supporting[1] - rubblekin_core::physics::PLAYER_HEIGHT).abs() < 0.002);
    }
    server.stop().unwrap();
}

#[test]
fn geographic_world_replicates_edits_and_preserves_its_generator_across_restart() {
    assert_geographic_world_restart(WorldGeneration::GeographyV1);
}

#[test]
fn geography_v2_replicates_edits_and_preserves_its_generator_across_restart() {
    assert_geographic_world_restart(WorldGeneration::GeographyV2);
}

#[test]
fn village_people_and_economy_replicate_simulate_idle_and_survive_restart() {
    let save = TestSave::new();
    let mut config = save.config(true);
    config.generation = WorldGeneration::GeographyV3;
    let server = spawn(config.clone()).unwrap();
    let (mut first, welcome) =
        Client::connect_mode(server.addr, "Village observer", SessionMode::Observer);
    let (initial_residents, initial_villages, started) = match welcome {
        ServerMessage::Welcome {
            generation,
            residents,
            villages,
            world_time,
            ..
        } => {
            assert_eq!(generation, WorldGeneration::GeographyV3);
            assert!(!villages.is_empty());
            assert_eq!(
                residents.len(),
                villages
                    .iter()
                    .map(|v| v.population as usize)
                    .sum::<usize>()
            );
            assert!(residents.len() <= 60);
            assert_distinct_bodies(residents.iter().map(|resident| resident.position));
            (residents, villages, world_time)
        }
        _ => unreachable!(),
    };
    let (mut second, welcome) =
        Client::connect_mode(server.addr, "Second observer", SessionMode::Observer);
    assert!(
        matches!(welcome, ServerMessage::Welcome { residents, villages, .. }
        if residents.iter().map(|r| r.id).collect::<Vec<_>>() == initial_residents.iter().map(|r| r.id).collect::<Vec<_>>()
        && villages.iter().map(|v| v.id).collect::<Vec<_>>() == initial_villages.iter().map(|v| v.id).collect::<Vec<_>>())
    );
    for client in [&mut first, &mut second] {
        let state = client.until(|message| matches!(message, ServerMessage::State { residents, world_time, .. }
            if *world_time > started + 0.1 && residents.iter().zip(&initial_residents).any(|(a,b)| a.position != b.position)));
        if let ServerMessage::State {
            players,
            npc,
            residents,
            ..
        } = &state
        {
            assert_distinct_bodies(
                players
                    .iter()
                    .map(|player| player.body.position)
                    .chain(std::iter::once(npc.position))
                    .chain(residents.iter().map(|resident| resident.position)),
            );
        }
        assert!(serde_json::to_vec(&state).unwrap().len() < MAX_MESSAGE_BYTES);
    }
    drop(first);
    drop(second);
    // Observe durable progress with zero clients, instead of assuming a fixed
    // wall-clock sleep always corresponds to a fixed simulation duration.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
        let progressed = value["world_time"].as_f64().unwrap() > started + 0.2
            && value["villages"]["villages"][0]["snapshot"]["crop_growth"]
                .as_f64()
                .unwrap()
                > initial_villages[0].crop_growth as f64
            && value["villages"]["residents"]
                .as_array()
                .unwrap()
                .iter()
                .zip(&initial_residents)
                .any(|(resident, initial)| {
                    serde_json::from_value::<ResidentSnapshot>(resident["snapshot"].clone())
                        .unwrap()
                        .position
                        != initial.position
                });
        if progressed {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Village residents/economy did not progress in the idle autosave"
        );
        thread::sleep(Duration::from_millis(50));
    }
    server.stop().unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    let saved_villages = value["villages"]["villages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| serde_json::from_value::<VillageSnapshot>(v["snapshot"].clone()).unwrap())
        .collect::<Vec<_>>();
    let saved_residents = value["villages"]["residents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| serde_json::from_value::<ResidentSnapshot>(r["snapshot"].clone()).unwrap())
        .collect::<Vec<_>>();
    config.seed = 999;
    config.generation = WorldGeneration::ValleyV1;
    let restarted = spawn(config).unwrap();
    let (_observer, welcome) =
        Client::connect_mode(restarted.addr, "Returned observer", SessionMode::Observer);
    match welcome {
        ServerMessage::Welcome {
            generation,
            seed,
            residents,
            villages,
            world_time,
            ..
        } => {
            assert_eq!(generation, WorldGeneration::GeographyV3);
            assert_eq!(seed, 42);
            assert!(world_time >= value["world_time"].as_f64().unwrap());
            for (current, saved) in residents.iter().zip(&saved_residents) {
                assert_eq!(current.id, saved.id);
                assert_eq!(current.role, saved.role);
                assert!(
                    current
                        .position
                        .iter()
                        .zip(saved.position)
                        .all(|(a, b)| (a - b).abs() < 1.0)
                );
            }
            for (current, saved) in villages.iter().zip(&saved_villages) {
                assert_eq!(current.id, saved.id);
                assert_eq!(current.population, saved.population);
                assert!((current.food - saved.food).abs() < 1.0);
                assert!((current.crop_growth - saved.crop_growth).abs() < 0.02);
            }
        }
        _ => unreachable!(),
    }
    restarted.stop().unwrap();
}

fn assert_geographic_world_restart(world_generation: WorldGeneration) {
    let save = TestSave::new();
    let mut config = save.config(true);
    config.generation = world_generation;
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
            assert_eq!(generation, world_generation);
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
    let world = World::generate(seed, world_generation);
    assert!(
        (body.position[1] - world.surface_height(body.position[0], body.position[2])).abs() < 0.1
    );
    let (mut observer, welcome) =
        Client::connect_mode(server.addr, "Surveyor", SessionMode::Observer);
    assert!(matches!(
        welcome,
        ServerMessage::Welcome {
            generation,
            ..
        } if generation == world_generation
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
    config.generation = match world_generation {
        WorldGeneration::GeographyV1 => WorldGeneration::GeographyV2,
        WorldGeneration::GeographyV2 => WorldGeneration::ValleyV1,
        WorldGeneration::ValleyV1 | WorldGeneration::GeographyV3 | WorldGeneration::GeographyV4 => {
            unreachable!()
        }
    };
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
            assert_eq!(generation, world_generation);
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
        movement_epoch: 0,
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
    observer.send(ClientMessage::Market {
        request_id: 92,
        village_id: None,
        revision: 0,
        action: rubblekin_core::economy::MarketAction::Deliver,
    });
    let notice = observer.until(|message| matches!(message, ServerMessage::Notice { .. }));
    assert!(matches!(notice, ServerMessage::Notice { text } if text.contains("read-only")));
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
        profile_id: None,
        version: PROTOCOL_VERSION,
        name: "Camera".into(),
        mode: SessionMode::Observer,
    });
    let notice = denied.until(|message| matches!(message, ServerMessage::Notice { .. }));
    assert!(matches!(notice, ServerMessage::Notice { text } if text.contains("disabled")));
    denied.until_disconnected(0);

    // Protocol-v2 clients omit mode. Protocol-v4 clients know GeographyV1 but
    // cannot generate V2. Both receive an update notice before disconnecting.
    for hello in [
        "{\"Hello\":{\"version\":2,\"name\":\"Old client\"}}\n",
        "{\"Hello\":{\"version\":4,\"name\":\"Old island client\",\"mode\":\"Player\"}}\n",
    ] {
        let mut outdated = Client::open(server.addr);
        outdated.writer.write_all(hello.as_bytes()).unwrap();
        let notice = outdated.until(|message| matches!(message, ServerMessage::Notice { .. }));
        assert!(matches!(notice, ServerMessage::Notice { text }
            if text.contains("version mismatch") && text.contains(&format!("server uses {PROTOCOL_VERSION}"))));
        outdated.until_disconnected(0);
    }

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
        profile_id: None,
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
fn failed_durable_edit_never_reaches_the_player_or_observer() {
    let save = TestSave::new();
    let config = save.config(true);
    let server = spawn(config.clone()).unwrap();
    let (mut player, _) = Client::connect(server.addr, "Builder");
    let (mut observer, _) = Client::connect_mode(server.addr, "Camera", SessionMode::Observer);
    let original = fs::read(&config.save_path).unwrap();
    // Block the temporary save file without touching the last committed world.
    let temporary = save
        .0
        .join(format!(".world.json.{}.tmp", std::process::id()));
    fs::create_dir(&temporary).unwrap();
    player.send(ClientMessage::Edit {
        request_id: 51,
        position: nearby_air(),
        block: Block::Brick,
    });
    for client in [&mut player, &mut observer] {
        loop {
            let mut line = String::new();
            match client.reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => assert!(!matches!(
                    serde_json::from_str::<ServerMessage>(&line).unwrap(),
                    ServerMessage::BlockChanged { request_id: 51, .. }
                )),
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("Expected failed-save disconnect, got {error}"),
            }
        }
    }
    assert!(server.stop().is_err());
    assert_eq!(fs::read(&config.save_path).unwrap(), original);
    fs::remove_dir(temporary).unwrap();
    let restarted = spawn(config).unwrap();
    let (_, welcome) = Client::connect(restarted.addr, "Returning builder");
    assert!(matches!(welcome, ServerMessage::Welcome { edits, .. } if edits.is_empty()));
    restarted.stop().unwrap();
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
        movement_epoch: 0,
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
            movement_epoch: 0,
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
                movement_epoch: 0,
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
                movement_epoch: 0,
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
            movement_epoch: 0,
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
        movement_epoch: 0,
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
