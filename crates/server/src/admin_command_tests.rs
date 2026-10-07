use super::*;
use rubblekin_core::{admin_commands::MAX_ADMIN_COMMAND_BYTES, airships::AirshipRide};

struct Fixture {
    sim: Simulation,
    network: AirshipNetwork,
    connections: BTreeMap<u64, Connection>,
    config: ServerConfig,
    peers: Vec<TcpStream>,
}

impl Fixture {
    fn new() -> Self {
        let world = World::new(42);
        let network = AirshipNetwork::new(&world);
        let sim = Simulation {
            profiles: Default::default(),
            consumed_quarry_cells: Vec::new(),
            npc: npc::Forager::new(&world),
            villages: villages::VillageLife::new(&world),
            world,
            world_time: 0.0,
        };
        Self {
            sim,
            network,
            connections: BTreeMap::new(),
            config: ServerConfig {
                allow_admin: true,
                save_path: std::env::temp_dir()
                    .join(format!(
                        "rubblekin-admin-test-{}-{}",
                        std::process::id(),
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_nanos()
                    ))
                    .join("world.json"),
                ..Default::default()
            },
            peers: Vec::new(),
        }
    }

    fn player(&mut self, id: u64, name: &str, position: [f32; 3]) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (socket, _) = listener.accept().unwrap();
        let mut connection = Connection::new(socket).unwrap();
        connection.mode = Some(SessionMode::Player);
        connection.player = Some(PlayerSnapshot {
            id,
            name: name.into(),
            body: Body::new(position),
            yaw: 0.0,
            last_input_sequence: 0,
            movement_epoch: 0,
            ride: None,
            deck_position: None,
        });
        self.connections.insert(id, connection);
        self.peers.push(peer);
    }

    fn send(&mut self, id: u64, message: ClientMessage) {
        handle_message(
            id,
            message,
            &mut self.connections,
            &mut self.sim,
            &self.network,
            &self.config,
            &mut 16,
        )
        .unwrap();
    }

    fn command(&mut self, id: u64, command: &str) -> String {
        // These tests exercise repeated individual requests; the rate test
        // below explicitly leaves the timestamp in place.
        self.connections.get_mut(&id).unwrap().last_admin_request = None;
        self.send(
            id,
            ClientMessage::AdminCommand {
                command: command.into(),
            },
        );
        self.result(id)
    }

    fn result(&self, id: u64) -> String {
        match serde_json::from_slice::<ServerMessage>(
            self.connections[&id].outgoing.back().unwrap(),
        )
        .unwrap()
        {
            ServerMessage::AdminCommandResult { text } => text,
            other => panic!("Expected admin result, got {other:?}"),
        }
    }

    fn snapshot(&self, id: u64) -> &PlayerSnapshot {
        self.connections[&id].player.as_ref().unwrap()
    }

    fn npc_weights(&mut self, id: u64, forage: f32, rest: f32) -> String {
        self.send(
            id,
            ClientMessage::Admin {
                action: AdminAction::SetNpcWeights { forage, rest },
            },
        );
        match serde_json::from_slice::<ServerMessage>(
            self.connections[&id].outgoing.back().unwrap(),
        )
        .unwrap()
        {
            ServerMessage::Notice { text } => text,
            other => panic!("Expected NPC-control notice, got {other:?}"),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.config.save_path.parent().unwrap());
    }
}

#[test]
fn queued_movement_can_spend_a_busy_servers_elapsed_time_but_not_more() {
    let mut f = Fixture::new();
    f.player(1, "Delayed", [10.0, 60.0, 10.0]);
    let connection = f.connections.get_mut(&1).unwrap();
    connection.input_credit = 0.0;
    connection.credit_updated = Instant::now() - Duration::from_millis(1010);
    for sequence in 1..=4 {
        f.send(
            1,
            ClientMessage::Input {
                movement_epoch: 0,
                sequence,
                dt: MAX_INPUT_DT,
                input: MoveInput {
                    fly: true,
                    ..Default::default()
                },
                yaw: 0.0,
            },
        );
        assert!(
            !f.connections[&1].dead,
            "server work must not discard legitimate time"
        );
        assert_eq!(f.snapshot(1).last_input_sequence, sequence);
    }
    f.send(
        1,
        ClientMessage::Input {
            movement_epoch: 0,
            sequence: 5,
            dt: MAX_INPUT_DT,
            input: MoveInput {
                fly: true,
                ..Default::default()
            },
            yaw: 0.0,
        },
    );
    assert!(
        f.connections[&1].dead,
        "elapsed time is still a strict budget"
    );
    assert_eq!(f.snapshot(1).last_input_sequence, 4);
}

#[test]
fn admin_gate_observer_read_only_and_help_do_not_mutate_players() {
    let mut f = Fixture::new();
    f.player(1, "Ian", [10.0, 60.0, 10.0]);
    f.player(2, "Violet", [15.0, 60.0, 15.0]);
    f.config.allow_admin = false;
    assert!(
        f.command(1, "teleport Violet 20 50 20")
            .contains("disabled")
    );
    assert_eq!(f.snapshot(2).body.position, [15.0, 60.0, 15.0]);
    f.config.allow_admin = true;
    let help = f.command(1, "help");
    assert!(help.contains("teleport PLAYER DESTINATION"));
    assert_eq!(f.snapshot(1).movement_epoch, 0);
    let observer = f.connections.get_mut(&1).unwrap();
    observer.mode = Some(SessionMode::Observer);
    observer.player = None;
    assert!(
        f.command(1, "teleport Violet 20 50 20")
            .contains("read-only")
    );
    assert_eq!(f.snapshot(2).body.position, [15.0, 60.0, 15.0]);
}

#[test]
fn self_and_named_coordinates_clear_motion_and_passenger_state_and_restart_epoch() {
    let mut f = Fixture::new();
    f.player(1, "Ian", [10.0, 60.0, 10.0]);
    f.player(2, "Violet Koski", [15.0, 60.0, 15.0]);
    let player = f.connections.get_mut(&2).unwrap().player.as_mut().unwrap();
    player.body.velocity = [3.0, -8.0, 1.0];
    player.body.on_ground = true;
    player.last_input_sequence = 99;
    player.ride = Some(AirshipRide {
        ship_id: 5,
        seat: 2,
    });
    player.deck_position = Some([1.0, 0.0, 2.0]);
    assert!(
        f.command(1, "teleport \"violet koski\" 20 50 20")
            .starts_with("Teleported Violet Koski")
    );
    let player = f.snapshot(2);
    assert_eq!(player.body.position, [20.0, 50.0, 20.0]);
    assert_eq!(player.body.velocity, [0.0; 3]);
    assert!(!player.body.on_ground);
    assert!(player.ride.is_none() && player.deck_position.is_none());
    assert_eq!(player.last_input_sequence, 0);
    assert_eq!(player.movement_epoch, 1);
    assert_eq!(f.snapshot(1).movement_epoch, 0);
    assert!(f.command(1, "tp -20 40 -20").starts_with("Teleported Ian"));
    assert_eq!(f.snapshot(1).body.position, [-20.0, 40.0, -20.0]);
    assert_eq!(f.snapshot(1).movement_epoch, 1);
}

#[test]
fn exact_case_insensitive_names_reject_missing_and_ambiguous_players() {
    let mut f = Fixture::new();
    f.player(1, "Ian", [10.0, 60.0, 10.0]);
    f.player(2, "Violet", [15.0, 60.0, 15.0]);
    f.player(3, "VIOLET", [-15.0, 60.0, -15.0]);
    assert!(
        f.command(1, "teleport Ian Vio")
            .contains("No connected player")
    );
    assert!(
        f.command(1, "teleport violet 20 50 20")
            .contains("More than one")
    );
    assert!(f.command(1, "teleport Violet").contains("More than one"));
    assert_eq!(f.snapshot(1).movement_epoch, 0);
    assert_eq!(f.snapshot(2).movement_epoch, 0);
}

#[test]
fn named_destinations_leave_every_character_separate_and_reject_enclosed_space() {
    let mut f = Fixture::new();
    f.player(1, "Ian", [10.0, 60.0, 10.0]);
    f.player(2, "Violet", [15.0, 60.0, 15.0]);
    f.player(3, "Molly", [14.25, 60.0, 14.25]);
    assert!(
        f.command(1, "teleport Ian Violet")
            .starts_with("Teleported Ian")
    );
    let origin = f.snapshot(2).body.position;
    let position = f.snapshot(1).body.position;
    assert_ne!(position, origin);
    assert!(character_position_is_clear(
        &f.sim.world,
        position,
        &character_obstacles(&f.connections, &f.sim, Some(1), &f.network),
    ));
    assert!((position[0] - origin[0]).hypot(position[2] - origin[2]) <= 3.0);
    assert_eq!(f.snapshot(2).movement_epoch, 0);
    assert!(f.command(3, "tp Ian").starts_with("Teleported Molly"));
    // A ceiling/floor enclosure with only the destination's occupied body clear.
    for y in 119..=123 {
        for z in 23..=36 {
            for x in 23..=36 {
                let in_body = (29..=30).contains(&x) && (29..=30).contains(&z) && y >= 120;
                if !in_body {
                    f.sim
                        .world
                        .set_block(BlockPos::new(x, y, z), Block::Stone)
                        .unwrap();
                }
            }
        }
    }
    assert!(f.command(1, "tp Violet").contains("no clear space"));
    assert_eq!(f.snapshot(1).body.position, position);
}

#[test]
fn solid_occupied_nonfinite_and_body_out_of_bounds_coordinates_are_rejected() {
    let mut f = Fixture::new();
    f.player(1, "Ian", [10.0, 60.0, 10.0]);
    f.player(2, "Violet", [15.0, 60.0, 15.0]);
    f.sim
        .world
        .set_block(BlockPos::new(40, 100, 40), Block::Stone)
        .unwrap();
    for command in [
        "tp 20 50 20",
        "tp 15 60 15",
        "tp 80 50 0",
        "tp 0 79 0",
        "tp 0 -13 0",
        "tp NaN 50 0",
        "tp 1e30 50 0",
    ] {
        assert!(
            !f.command(1, command).starts_with("Teleported"),
            "Accepted {command}"
        );
        assert_eq!(f.snapshot(1).body.position, [10.0, 60.0, 10.0]);
        assert_eq!(f.snapshot(1).movement_epoch, 0);
    }
    let npc = f.sim.npc.snapshot.position;
    assert!(
        !f.command(1, &format!("tp {} {} {}", npc[0], npc[1], npc[2]))
            .starts_with("Teleported")
    );
}

#[test]
fn stale_inputs_do_not_spend_credit_or_advance_ack_and_new_epoch_retains_flight() {
    let mut f = Fixture::new();
    f.player(1, "Ian", [10.0, 60.0, 10.0]);
    f.command(1, "tp 20 50 20");
    let credit = f.connections[&1].input_credit;
    for sequence in [1, 2, 99] {
        f.send(
            1,
            ClientMessage::Input {
                sequence,
                movement_epoch: 0,
                dt: 0.25,
                input: MoveInput {
                    direction: [1.0, 0.0],
                    ..Default::default()
                },
                yaw: 0.0,
            },
        );
    }
    assert_eq!(f.snapshot(1).body.position, [20.0, 50.0, 20.0]);
    assert_eq!(f.snapshot(1).last_input_sequence, 0);
    assert_eq!(f.connections[&1].input_credit, credit);
    assert!(!f.connections[&1].dead);
    f.send(
        1,
        ClientMessage::Input {
            sequence: 1,
            movement_epoch: 1,
            dt: 0.05,
            input: MoveInput {
                direction: [1.0, 0.0],
                fly: true,
                ..Default::default()
            },
            yaw: 1.0,
        },
    );
    assert_eq!(f.snapshot(1).body.position[1], 50.0);
    assert!(f.snapshot(1).body.position[0] > 20.0);
    assert_eq!(f.snapshot(1).last_input_sequence, 1);
    assert!(!f.connections[&1].dead);
    f.send(
        1,
        ClientMessage::Input {
            sequence: 2,
            movement_epoch: 2,
            dt: 0.05,
            input: MoveInput::default(),
            yaw: 0.0,
        },
    );
    assert!(f.connections[&1].dead);
}

#[test]
fn command_length_and_request_rate_are_bounded_without_disconnect() {
    let mut f = Fixture::new();
    f.player(1, "Ian", [10.0, 60.0, 10.0]);
    assert!(
        f.command(1, &"a".repeat(MAX_ADMIN_COMMAND_BYTES + 1))
            .contains("at most")
    );
    f.send(
        1,
        ClientMessage::AdminCommand {
            command: "tp 20 50 20".into(),
        },
    );
    assert!(f.result(1).contains("too quickly"));
    assert_eq!(f.snapshot(1).movement_epoch, 0);
    assert!(!f.connections[&1].dead);
}

#[test]
fn npc_control_bursts_share_the_console_budget_without_mutating_or_saving() {
    let mut f = Fixture::new();
    f.player(1, "Ian", [10.0, 60.0, 10.0]);
    f.player(2, "Violet", [15.0, 60.0, 15.0]);
    assert_eq!(f.npc_weights(1, 2.0, 3.0), "NPC settings updated");
    let saved = std::fs::read(&f.config.save_path).unwrap();
    let npc = serde_json::to_value(&f.sim.npc).unwrap();
    assert_eq!(npc["forage_weight"], 2.0);
    assert_eq!(npc["rest_weight"], 3.0);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&saved).unwrap()["npc"],
        npc
    );

    // Hold the clock inside the rate window even on a heavily loaded runner.
    let blocked_until = Instant::now() + Duration::from_secs(60);
    f.connections.get_mut(&1).unwrap().last_admin_request = Some(blocked_until);
    for _ in 0..63 {
        assert!(f.npc_weights(1, 9.0, 9.0).contains("too quickly"));
    }
    f.send(
        1,
        ClientMessage::AdminCommand {
            command: "tp 20 50 20".into(),
        },
    );
    assert!(f.result(1).contains("too quickly"));
    assert_eq!(f.snapshot(1).movement_epoch, 0);
    assert_eq!(serde_json::to_value(&f.sim.npc).unwrap(), npc);
    assert_eq!(std::fs::read(&f.config.save_path).unwrap(), saved);
    assert_eq!(f.connections[&1].last_admin_request, Some(blocked_until));
    assert!(!f.connections[&1].dead);

    // Other players retain their own allowance, and the sender can retry after
    // the existing 100 ms interval without reconnecting.
    assert_eq!(f.npc_weights(2, 4.0, 5.0), "NPC settings updated");
    f.connections.get_mut(&1).unwrap().last_admin_request =
        Some(Instant::now() - Duration::from_millis(100));
    assert_eq!(f.npc_weights(1, 6.0, 7.0), "NPC settings updated");
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&f.config.save_path).unwrap()).unwrap();
    assert_eq!(saved["npc"]["forage_weight"], 6.0);
    assert_eq!(saved["npc"]["rest_weight"], 7.0);
}

#[test]
fn console_then_npc_controls_share_the_budget_and_keep_permission_errors() {
    let mut f = Fixture::new();
    f.player(1, "Ian", [10.0, 60.0, 10.0]);
    assert!(f.command(1, "help").contains("teleport PLAYER DESTINATION"));
    assert!(f.connections[&1].last_admin_request.is_some());
    f.connections.get_mut(&1).unwrap().last_admin_request =
        Some(Instant::now() + Duration::from_secs(60));
    let npc = serde_json::to_value(&f.sim.npc).unwrap();
    assert!(f.npc_weights(1, 2.0, 3.0).contains("too quickly"));
    f.config.allow_admin = false;
    assert!(f.npc_weights(1, 2.0, 3.0).contains("disabled"));
    f.config.allow_admin = true;
    f.connections.get_mut(&1).unwrap().mode = Some(SessionMode::Observer);
    assert!(f.npc_weights(1, 2.0, 3.0).contains("read-only"));
    assert_eq!(serde_json::to_value(&f.sim.npc).unwrap(), npc);
    assert!(!f.config.save_path.exists());
    assert!(!f.connections[&1].dead);
}

#[test]
fn named_ground_destinations_prefer_an_edited_supported_floor_to_nearby_air() {
    let mut f = Fixture::new();
    f.player(1, "Ian", [10.0, 70.0, 10.0]);
    f.player(2, "Violet", [15.25, 60.02, 15.25]);
    f.connections
        .get_mut(&2)
        .unwrap()
        .player
        .as_mut()
        .unwrap()
        .body
        .on_ground = true;
    f.sim
        .world
        .set_block(BlockPos::new(30, 119, 30), Block::Stone)
        .unwrap();
    f.sim
        .world
        .set_block(BlockPos::new(33, 120, 30), Block::Stone)
        .unwrap();
    assert!(f.command(1, "tp Violet").starts_with("Teleported Ian"));
    assert_eq!(f.snapshot(1).body.position, [16.75, 60.52, 15.25]);
    assert!(f.snapshot(1).ride.is_none());
}

#[test]
fn named_airship_teleports_attach_inside_rotated_deck_and_follow_its_fast_motion() {
    use rubblekin_core::{
        airships::{AIRSHIP_DECK_HALF_LENGTH, AIRSHIP_DECK_HALF_WIDTH},
        physics::character_position_is_clear_with_airships,
    };
    let mut f = Fixture::new();
    let world = World::generate(42, WorldGeneration::GeographyV3);
    f.network = AirshipNetwork::new(&world);
    f.sim = Simulation {
        profiles: Default::default(),
        consumed_quarry_cells: Vec::new(),
        npc: npc::Forager::new(&world),
        villages: villages::VillageLife::new(&world),
        world,
        world_time: 0.0,
    };
    let (time, ship) = [45.0, 75.0, 105.0, 135.0]
        .into_iter()
        .find_map(|time| {
            f.network
                .ships(time)
                .into_iter()
                .find(|ship| ship.docked_at.is_none() && ship.yaw.sin().abs() > 0.1)
                .map(|ship| (time, ship))
        })
        .expect("A rotated ship is in flight");
    f.sim.world_time = time;
    let edge = [
        AIRSHIP_DECK_HALF_WIDTH - 0.01,
        0.0,
        AIRSHIP_DECK_HALF_LENGTH - 0.1,
    ];
    f.player(1, "Ian", [0.0, 3000.0, 0.0]);
    f.player(2, "Violet", deck_position(&ship, edge));
    let destination = f.connections.get_mut(&2).unwrap().player.as_mut().unwrap();
    destination.ride = Some(AirshipRide {
        ship_id: ship.id,
        seat: u8::MAX,
    });
    destination.deck_position = Some(edge);
    destination.body.on_ground = true;
    assert!(f.command(1, "tp Violet").starts_with("Teleported Ian"));
    let local = f
        .snapshot(1)
        .deck_position
        .expect("Teleported passenger has a local offset");
    assert_eq!(f.snapshot(1).ride.unwrap().ship_id, ship.id);
    let edge_margin = PLAYER_RADIUS * std::f32::consts::SQRT_2;
    assert!(local[0].abs() <= AIRSHIP_DECK_HALF_WIDTH - edge_margin);
    assert!(local[2].abs() <= AIRSHIP_DECK_HALF_LENGTH - edge_margin);
    assert_eq!(local[1], 0.0);
    for sequence in 1..=30 {
        f.sim.world_time += DT as f64;
        carry_airship_players(&mut f.connections, &f.network, f.sim.world_time);
        let ship = f.network.ship(ship.id, f.sim.world_time).unwrap();
        assert_eq!(f.snapshot(1).body.position, deck_position(&ship, local));
        let connection = f.connections.get_mut(&1).unwrap();
        connection.input_credit = MAX_INPUT_CREDIT;
        f.send(
            1,
            ClientMessage::Input {
                sequence,
                movement_epoch: 1,
                dt: DT,
                input: MoveInput::default(),
                yaw: 0.0,
            },
        );
        assert_eq!(f.snapshot(1).ride.unwrap().ship_id, ship.id);
        assert!(f.snapshot(1).body.on_ground);
        assert!(character_position_is_clear_with_airships(
            &f.sim.world,
            f.snapshot(1).body.position,
            &character_obstacles(&f.connections, &f.sim, Some(1), &f.network),
            &f.network,
            f.sim.world_time,
        ));
    }
    // Terrain/body clearance alone permits this point; the actual deck slab
    // must reject a coordinate body straddling its thickness.
    let ship = f.network.ship(ship.id, f.sim.world_time).unwrap();
    let below = deck_position(&ship, [0.0, -0.2, 0.0]);
    let obstacles = character_obstacles(&f.connections, &f.sim, Some(1), &f.network);
    assert!(character_position_is_clear(&f.sim.world, below, &obstacles));
    assert!(!character_position_is_clear_with_airships(
        &f.sim.world,
        below,
        &obstacles,
        &f.network,
        f.sim.world_time
    ));
    assert!(
        !f.command(1, &format!("tp {} {} {}", below[0], below[1], below[2]))
            .starts_with("Teleported")
    );
    assert_eq!(f.snapshot(1).movement_epoch, 1);
    assert!(f.snapshot(1).ride.is_some());
}

#[test]
fn an_isolated_grounded_destination_does_not_drop_the_teleported_player_off_a_cliff() {
    let mut f = Fixture::new();
    f.player(1, "Ian", [10.0, 70.0, 10.0]);
    f.player(2, "Violet", [15.25, 60.02, 15.25]);
    f.connections
        .get_mut(&2)
        .unwrap()
        .player
        .as_mut()
        .unwrap()
        .body
        .on_ground = true;
    f.sim
        .world
        .set_block(BlockPos::new(30, 119, 30), Block::Stone)
        .unwrap();
    assert!(f.command(1, "tp Violet").contains("no clear space"));
    assert_eq!(f.snapshot(1).movement_epoch, 0);
    assert_eq!(f.snapshot(1).body.position, [10.0, 70.0, 10.0]);
}
