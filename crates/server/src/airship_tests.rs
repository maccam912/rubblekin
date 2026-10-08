use super::*;
use rubblekin_core::airships::AirshipSnapshot;

pub(super) struct Fixture {
    pub(super) sim: Simulation,
    pub(super) network: AirshipNetwork,
    pub(super) connections: BTreeMap<u64, Connection>,
    _peers: Vec<TcpStream>,
    config: ServerConfig,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let network = AirshipNetwork::new(&world);
        let sim = Simulation {
            profiles: Default::default(),
            consumed_quarry_cells: Vec::new(),
            ecology: crate::ecology::Ecology::default(),
            npc: npc::Forager::new(&world),
            villages: villages::VillageLife::new(&world),
            world,
            world_time: 0.0,
        };
        Self {
            sim,
            network,
            connections: BTreeMap::new(),
            _peers: Vec::new(),
            config: ServerConfig::default(),
        }
    }

    pub(super) fn add_player(&mut self, id: u64, position: [f32; 3]) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (socket, _) = listener.accept().unwrap();
        let mut connection = Connection::new(socket).unwrap();
        connection.mode = Some(SessionMode::Player);
        connection.player = Some(PlayerSnapshot {
            id,
            name: format!("Passenger {id}"),
            body: Body::new(position),
            yaw: 0.0,
            last_input_sequence: 0,
            movement_epoch: 0,
            ride: None,
            deck_position: None,
        });
        self.connections.insert(id, connection);
        self._peers.push(peer);
    }

    pub(super) fn send(&mut self, id: u64, message: ClientMessage) {
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

    pub(super) fn docked_ship(&self) -> AirshipSnapshot {
        self.network
            .ships(self.sim.world_time)
            .into_iter()
            .find(|ship| ship.docked_at.is_some())
            .expect("A scheduled ship is boarding")
    }

    pub(super) fn notice(&self, id: u64) -> String {
        match serde_json::from_slice::<ServerMessage>(
            self.connections[&id].outgoing.back().unwrap(),
        )
        .unwrap()
        {
            ServerMessage::Notice { text } => text,
            other => panic!("Expected notice, got {other:?}"),
        }
    }
}

#[test]
fn optional_pilot_conversation_answers_without_boarding_or_changing_the_body() {
    let mut f = Fixture::new();
    let ship = f.docked_ship();
    let mut position = pilot_position(&ship);
    position[0] += 1.0;
    f.add_player(1, position);
    f.send(1, ClientMessage::TalkToPilot { ship_id: ship.id });
    match serde_json::from_slice::<ServerMessage>(f.connections[&1].outgoing.back().unwrap())
        .unwrap()
    {
        ServerMessage::PilotDialog { ship_id, text } => {
            assert_eq!(ship_id, ship.id);
            assert!(text.contains(&ship.pilot_name));
            assert!(text.contains("heading") || text.contains("sailing"));
        }
        other => panic!("Expected a pilot answer, got {other:?}"),
    }
    let player = f.connections[&1].player.as_ref().unwrap();
    assert!(player.ride.is_none());
    assert_eq!(player.body.position, position);
}

#[test]
fn pilot_introduction_remains_the_same_after_departure() {
    let mut f = Fixture::new();
    let docked = f.docked_ship();
    f.add_player(1, pilot_position(&docked));
    let mut replies = Vec::new();
    for time in [0.0, f64::from(docked.departure_in) + 1.0] {
        f.sim.world_time = time;
        let ship = f.network.ship(docked.id, time).unwrap();
        f.connections
            .get_mut(&1)
            .unwrap()
            .player
            .as_mut()
            .unwrap()
            .body
            .position = pilot_position(&ship);
        f.send(1, ClientMessage::TalkToPilot { ship_id: ship.id });
        let ServerMessage::PilotDialog { text, .. } =
            serde_json::from_slice(f.connections[&1].outgoing.back().unwrap()).unwrap()
        else {
            panic!("Expected a pilot answer");
        };
        for village_id in [docked.from_village, docked.next_village] {
            let village = f
                .sim
                .world
                .settlements()
                .unwrap()
                .villages
                .iter()
                .find(|village| village.id == village_id)
                .unwrap();
            assert!(text.contains(&village.name));
        }
        assert!(!text.contains("seconds"));
        assert!(!text.contains("aboard"));
        replies.push(text);
    }
    assert_eq!(replies[0], replies[1]);
}

#[test]
fn pilot_dialogue_rejects_remote_requests_and_observers_remain_read_only() {
    let mut f = Fixture::new();
    let ship = f.docked_ship();
    let pilot = pilot_position(&ship);
    f.add_player(1, [pilot[0] + 100.0, pilot[1], pilot[2]]);
    f.send(1, ClientMessage::TalkToPilot { ship_id: ship.id });
    assert!(f.notice(1).contains("closer"));
    assert!(f.connections[&1].player.as_ref().unwrap().ride.is_none());
    let observer = f.connections.get_mut(&1).unwrap();
    observer.mode = Some(SessionMode::Observer);
    observer.player = None;
    f.send(1, ClientMessage::TalkToPilot { ship_id: ship.id });
    assert!(f.notice(1).contains("read-only"));
    assert!(f.connections[&1].player.is_none());
}
