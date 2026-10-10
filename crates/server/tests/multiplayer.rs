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

    #[track_caller]
    fn until(&mut self, predicate: impl Fn(&ServerMessage) -> bool) -> ServerMessage {
        self.until_for(predicate, Duration::from_secs(5))
    }

    #[track_caller]
    fn until_for(
        &mut self,
        predicate: impl Fn(&ServerMessage) -> bool,
        timeout: Duration,
    ) -> ServerMessage {
        let started = Instant::now();
        let deadline = started + timeout;
        let mut last_message = String::from("none");
        loop {
            assert!(
                Instant::now() < deadline,
                "Timed out after {timeout:?} waiting for expected server message; last message: {last_message}"
            );
            let mut line = String::new();
            let count = match self.reader.read_line(&mut line) {
                Ok(count) => count,
                Err(error) => panic!(
                    "Server read failed after {:?}: {error}; last message: {last_message}",
                    started.elapsed()
                ),
            };
            assert!(
                count > 0,
                "Server disconnected; last message: {last_message}"
            );
            let message = serde_json::from_str(&line).unwrap();
            if predicate(&message) {
                return message;
            }
            last_message = line.trim_end().chars().take(400).collect();
        }
    }

    fn work(
        &mut self,
        request_id: u64,
        action: rubblekin_core::economy::WorkAction,
    ) -> ServerMessage {
        thread::sleep(Duration::from_millis(110));
        self.send(ClientMessage::Work { request_id, action });
        self.until(|message| matches!(message, ServerMessage::WorkState {request_id: response, ..} if *response == request_id))
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

fn wild_foraging_fixture(
    config: &ServerConfig,
) -> (World, rubblekin_core::economy::WorkSite, [f32; 3], [f32; 3]) {
    use rubblekin_core::{
        economy::{WorkKind, WorkSite},
        forage::{GATHER_COST, plants},
        physics::character_position_is_clear,
    };
    spawn(config.clone()).unwrap().stop().unwrap();
    let mut save: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    save["ecology"]["habitats"][0]["forage"] = GATHER_COST.into();
    // Controlled shared-food race, without unrelated grazing during the six seconds.
    save["ecology"]["animals"] = serde_json::json!([]);
    let h = &save["ecology"]["habitats"][0];
    let id = h["id"].as_u64().unwrap() as u32;
    let center: [f32; 3] = serde_json::from_value(h["position"].clone()).unwrap();
    let world = World::generate(config.seed, config.generation);
    let (p, second) = plants(&world, id, center, GATHER_COST)
        .into_iter()
        .find_map(|p| {
            if !character_position_is_clear(&world, p.position(), &[]) {
                return None;
            }
            [(-1., 0.), (1., 0.), (0., -1.), (0., 1.)]
                .into_iter()
                .map(|(x, z)| [p.position()[0] + x, p.position()[1], p.position()[2] + z])
                .find(|q| {
                    character_position_is_clear(&world, *q, &[])
                        && world
                            .raycast([q[0], q[1] + 0.05, q[2]], [0., -1., 0.], 0.1)
                            .is_some()
                })
                .map(|q| (p, q))
        })
        .unwrap();
    fs::write(&config.save_path, serde_json::to_vec(&save).unwrap()).unwrap();
    (
        world,
        WorkSite {
            village_id: id,
            kind: WorkKind::GatherForage,
            index: p.index,
        },
        p.position(),
        second,
    )
}

#[test]
fn wild_foraging_race_saves_food_and_depletion_together_and_sells_at_market() {
    use rubblekin_core::{
        economy::{MarketAction, WorkAction, WorkKind},
        settlement::ResourceKind,
    };
    const FIRST: &str = "00000000000000000000000000000062";
    const SECOND: &str = "00000000000000000000000000000063";
    let save = TestSave::new();
    let config = ServerConfig {
        generation: WorldGeneration::GeographyV6,
        ..save.config(true)
    };
    let (world, site, position, second_position) = wild_foraging_fixture(&config);
    let server = spawn(config.clone()).unwrap();
    let (mut first, _) = Client::connect_profile(server.addr, FIRST);
    let (mut second, _) = Client::connect_profile(server.addr, SECOND);
    assert!(matches!(
        first.work(1, WorkAction::Start { site }),
        ServerMessage::WorkState {
            accepted: false,
            ..
        }
    ));
    first.teleport(position);
    second.teleport(second_position);
    assert!(
        matches!(first.work(2,WorkAction::View),ServerMessage::WorkState {work,..} if work.offer.as_ref().is_some_and(|o|o.site.kind==WorkKind::GatherForage && o.unavailable_reason.is_none()))
    );
    let start = first.work(3, WorkAction::Start { site });
    assert!(
        matches!(start, ServerMessage::WorkState { accepted: true, .. }),
        "{start:?}"
    );
    let start = second.work(1, WorkAction::Start { site });
    assert!(
        matches!(start, ServerMessage::WorkState { accepted: true, .. }),
        "{start:?}"
    );
    assert!(
        matches!(first.until_for(|m|matches!(m,ServerMessage::WorkState {request_id:0,work,ledger,..} if work.active.is_none() && ledger.cargo[0]==1),Duration::from_secs(15)),ServerMessage::WorkState {accepted:true,ledger,..} if ledger.revision==1 && ledger.coins==0)
    );
    assert!(
        matches!(second.until_for(|m|matches!(m,ServerMessage::WorkState {request_id:0,work,accepted:false,..} if work.active.is_none()),Duration::from_secs(5)),ServerMessage::WorkState {ledger,..} if ledger.cargo_total()==0)
    );
    let durable: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(durable["profiles"][FIRST]["ledger"]["cargo"][0], 1);
    assert_eq!(durable["profiles"][SECOND]["ledger"]["cargo"][0], 0);
    assert!(
        durable["ecology"]["habitats"][0]["forage"]
            .as_f64()
            .unwrap()
            < 1.
    );
    assert!(durable["edits"].as_array().unwrap().is_empty());
    assert!(
        matches!(first.work(3,WorkAction::Start {site}),ServerMessage::WorkState {accepted:false,ledger,..} if ledger.cargo[0]==1)
    );
    let town = &world.settlements().unwrap().villages[0];
    first.teleport(town.market);
    let ServerMessage::MarketState {
        market: Some(market),
        ..
    } = first.market(4, Some(town.id), 1, MarketAction::View)
    else {
        panic!("Expected quote")
    };
    let price = market.goods[0].sell_price;
    assert!(
        matches!(first.market(5,Some(town.id),1,MarketAction::Sell {kind:ResourceKind::Food,quantity:1,unit_price:price}),ServerMessage::MarketState {accepted:true,ledger,..} if ledger.cargo[0]==0 && ledger.coins==price && ledger.revision==2)
    );
    drop((first, second));
    server.stop().unwrap();
    let server = spawn(config.clone()).unwrap();
    let (mut first, _) = Client::connect_profile(server.addr, FIRST);
    assert!(
        matches!(first.until(|m|matches!(m,ServerMessage::WorkState {request_id:0,..})),ServerMessage::WorkState {ledger,..} if ledger.cargo_total()==0 && ledger.coins==price && ledger.revision==2)
    );
    first.teleport(position);
    assert!(matches!(
        first.work(1, WorkAction::Start { site }),
        ServerMessage::WorkState {
            accepted: false,
            ..
        }
    ));
    drop(first);
    server.stop().unwrap();
}

#[test]
fn wild_foraging_save_failure_confirms_neither_food_nor_shared_depletion() {
    use rubblekin_core::economy::WorkAction;
    const PROFILE: &str = "00000000000000000000000000000064";
    let save = TestSave::new();
    let config = ServerConfig {
        generation: WorldGeneration::GeographyV6,
        ..save.config(true)
    };
    let (_, site, position, _) = wild_foraging_fixture(&config);
    let server = spawn(config.clone()).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    client.teleport(position);
    assert!(matches!(
        client.work(1, WorkAction::Start { site }),
        ServerMessage::WorkState { accepted: true, .. }
    ));
    client.until_for(|m|matches!(m,ServerMessage::WorkState {work,..} if work.active.as_ref().is_some_and(|a|a.elapsed_seconds>=5.25)),Duration::from_secs(15));
    let before = fs::read(&config.save_path).unwrap();
    let temporary = save
        .0
        .join(format!(".world.json.{}.tmp", std::process::id()));
    fs::create_dir(&temporary).unwrap();
    loop {
        let mut line = String::new();
        match client.reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let message = match serde_json::from_str::<ServerMessage>(&line) {
                    Ok(m) => m,
                    Err(e) if !line.ends_with('\n') && e.is_eof() => break,
                    Err(e) => panic!("Invalid complete frame: {e}"),
                };
                match message {
                    ServerMessage::WorkState { ledger, .. } => assert_eq!(ledger.cargo_total(), 0),
                    ServerMessage::WildlifeState { habitats, .. } => assert!(
                        habitats
                            .iter()
                            .find(|h| h.id == site.village_id)
                            .unwrap()
                            .forage
                            >= 5.
                    ),
                    _ => {}
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(e) => panic!("Expected save-failure disconnect: {e}"),
        }
    }
    assert!(server.stop().is_err());
    assert_eq!(fs::read(&config.save_path).unwrap(), before);
    fs::remove_dir(&temporary).unwrap();
    let server = spawn(config.clone()).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    assert!(
        matches!(client.until(|m|matches!(m,ServerMessage::WorkState {request_id:0,..})),ServerMessage::WorkState {ledger,..} if ledger.cargo_total()==0 && ledger.revision==0)
    );
    client.teleport(position);
    assert!(matches!(
        client.work(1, WorkAction::Start { site }),
        ServerMessage::WorkState { accepted: true, .. }
    ));
    client.work(2, WorkAction::Cancel);
    drop(client);
    server.stop().unwrap();
}

#[test]
fn local_work_times_cancels_and_persists_only_completed_useful_activity() {
    use rubblekin_core::economy::{WorkAction, WorkKind, WorkSite};
    const PROFILE: &str = "00000000000000000000000000000055";
    let save = TestSave::new();
    let mut config = save.config(true);
    config.generation = WorldGeneration::GeographyV5;
    let world = World::generate(42, config.generation);
    let village = &world.settlements().unwrap().villages[0];
    let soil = village.fields[0].plant_positions().next().unwrap();
    let position = [
        soil.x as f32 * CELL_SIZE + 0.25,
        (soil.y + 1) as f32 * CELL_SIZE,
        soil.z as f32 * CELL_SIZE + 0.25,
    ];
    let site = WorkSite {
        village_id: village.id,
        kind: WorkKind::TendField,
        index: 0,
    };
    let server = spawn(config.clone()).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    let remote = client.work(1, WorkAction::Start { site });
    assert!(
        matches!(remote,ServerMessage::WorkState {accepted:false,ledger,..} if ledger.coins==0)
    );
    client.teleport(position);
    let started = client.work(2, WorkAction::Start { site });
    assert!(
        matches!(started,ServerMessage::WorkState {accepted:true,work,ledger,..} if work.active.is_some() && ledger.coins==0)
    );
    let duplicate = client.work(3, WorkAction::Start { site });
    assert!(
        matches!(duplicate,ServerMessage::WorkState {accepted:false,work,..} if work.active.is_some())
    );
    let cancel = client.work(4, WorkAction::Cancel);
    assert!(
        matches!(cancel,ServerMessage::WorkState {accepted:true,work,ledger,..} if work.active.is_none() && ledger.coins==0)
    );
    client.work(5, WorkAction::Start { site });
    let mut away = position;
    away[0] += 3.0;
    // Authoritative developer movement isolates timer cancellation from the
    // independently tested movement controller.
    client.teleport(away);
    let stopped = client.until(|message| {
        matches!(
            message,
            ServerMessage::WorkState {
                request_id: 0,
                accepted: false,
                ..
            }
        )
    });
    assert!(
        matches!(stopped,ServerMessage::WorkState {work,ledger,..} if work.active.is_none() && ledger.coins==0)
    );
    thread::sleep(Duration::from_millis(110));
    client.teleport(position);
    client.work(6, WorkAction::Start { site });
    drop(client);
    server.stop().unwrap();

    let server = spawn(config.clone()).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    let initial =
        client.until(|message| matches!(message, ServerMessage::WorkState { request_id: 0, .. }));
    assert!(
        matches!(initial,ServerMessage::WorkState {work,ledger,..} if work.active.is_none() && ledger.coins==0)
    );
    let started = client.work(7, WorkAction::Start { site });
    assert!(
        matches!(started, ServerMessage::WorkState { accepted: true, .. }),
        "{started:?}"
    );
    let wall_start = Instant::now();
    let completion = client.until_for(
        |message| {
            matches!(message,ServerMessage::WorkState {request_id:0,ledger,work,notice,..}
        if work.active.is_none() && ledger.coins==2 && notice.contains("Earned"))
        },
        Duration::from_secs(15),
    );
    assert!(
        wall_start.elapsed() >= Duration::from_millis(5700),
        "Work completed too early"
    );
    assert!(
        matches!(completion,ServerMessage::WorkState {accepted:true,ledger,..} if ledger.revision==1)
    );
    let on_disk: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(on_disk["profiles"][PROFILE]["ledger"]["coins"], 2);
    let replay = client.work(7, WorkAction::Start { site });
    assert!(
        matches!(replay, ServerMessage::WorkState {accepted:false, ledger, work, ..}
        if ledger.coins == 2 && work.active.is_none())
    );
    let cancel = client.work(8, WorkAction::Cancel);
    assert!(
        matches!(cancel,ServerMessage::WorkState {ledger,work,..} if ledger.coins==2 && work.active.is_none())
    );
    let next = client.work(9, WorkAction::Start { site });
    assert!(
        matches!(next,ServerMessage::WorkState {accepted:true,work,..} if work.active.is_some())
    );
    let old_cancel = client.work(8, WorkAction::Cancel);
    assert!(
        matches!(old_cancel,ServerMessage::WorkState {accepted:false,work,..} if work.active.is_some())
    );
    client.work(10, WorkAction::Cancel);
    drop(client);
    server.stop().unwrap();
    let server = spawn(config).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    let resumed =
        client.until(|message| matches!(message, ServerMessage::WorkState { request_id: 0, .. }));
    assert!(
        matches!(resumed,ServerMessage::WorkState {ledger,work,..} if ledger.coins==2 && work.active.is_none())
    );
    drop(client);
    server.stop().unwrap();
}

fn ripe_harvest_fixture(
    config: &ServerConfig,
) -> (World, rubblekin_core::economy::WorkSite, [f32; 3]) {
    use rubblekin_core::economy::{WorkKind, WorkSite};
    // Start from a real, validated fresh save. Resting residents isolate the
    // socket/persistence test; actual NPC competition has a separate test.
    spawn(config.clone()).unwrap().stop().unwrap();
    let world = World::generate(config.seed, config.generation);
    let village = &world.settlements().unwrap().villages[0];
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    value["villages"]["villages"][0]["snapshot"]["crop_growth"] = 1.0.into();
    value["villages"]["villages"][0]["planted"] = true.into();
    for resident in value["villages"]["residents"].as_array_mut().unwrap() {
        resident["snapshot"]["energy"] = 0.0.into();
        resident["snapshot"]["hunger"] = 0.0.into();
    }
    fs::write(&config.save_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let soil = village.fields[0].plant_positions().next().unwrap();
    let position = [
        soil.x as f32 * CELL_SIZE + 0.25,
        (soil.y + 1) as f32 * CELL_SIZE,
        soil.z as f32 * CELL_SIZE + 0.25,
    ];
    let site = WorkSite {
        village_id: village.id,
        kind: WorkKind::HarvestField,
        index: 0,
    };
    (world, site, position)
}

#[test]
fn harvest_cargo_is_durable_and_reaches_stores_only_through_a_physical_market_sale() {
    use rubblekin_core::{
        economy::{MarketAction, WorkAction},
        settlement::ResourceKind,
    };
    const PROFILE: &str = "00000000000000000000000000000057";
    let save = TestSave::new();
    let config = ServerConfig {
        generation: WorldGeneration::GeographyV6,
        ..save.config(true)
    };
    let (world, site, position) = ripe_harvest_fixture(&config);
    let village = &world.settlements().unwrap().villages[0];
    let server = spawn(config.clone()).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    let (mut observer, _) = Client::connect_mode(server.addr, "Observer", SessionMode::Observer);
    observer.send(ClientMessage::Work {
        request_id: 1,
        action: WorkAction::Start { site },
    });
    let denial = observer.until(|message| matches!(message, ServerMessage::Notice { .. }));
    assert!(matches!(denial, ServerMessage::Notice { text } if text.contains("read-only")));
    drop(observer);
    assert!(matches!(
        client.work(1, WorkAction::Start { site }),
        ServerMessage::WorkState {
            accepted: false,
            ..
        }
    ));
    client.teleport(position);
    let started = client.work(2, WorkAction::Start { site });
    assert!(
        matches!(&started, ServerMessage::WorkState { accepted:true, ledger, .. } if ledger.cargo_total()==0),
        "{started:?}"
    );
    let started_at = Instant::now();
    let complete = client.until_for(
        |message| {
            matches!(message, ServerMessage::WorkState { request_id:0, work, ledger, .. }
        if work.active.is_none() && ledger.cargo[0]==12)
        },
        Duration::from_secs(15),
    );
    assert!(started_at.elapsed() >= Duration::from_millis(5700));
    assert!(
        matches!(complete, ServerMessage::WorkState { ledger, .. } if ledger.coins==0 && ledger.revision==1)
    );
    let harvested: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(harvested["profiles"][PROFILE]["ledger"]["cargo"][0], 12);
    assert_eq!(
        harvested["villages"]["villages"][0]["snapshot"]["crop_growth"],
        0.0
    );
    let food_before_sale = harvested["villages"]["villages"][0]["snapshot"]["food"]
        .as_f64()
        .unwrap();
    assert!(
        matches!(client.work(2, WorkAction::Start { site }), ServerMessage::WorkState { accepted:false, ledger, .. } if ledger.cargo[0]==12)
    );
    let remote = client.market(
        3,
        Some(village.id),
        1,
        MarketAction::Sell {
            kind: ResourceKind::Food,
            quantity: 5,
            unit_price: 1,
        },
    );
    assert!(
        matches!(remote, ServerMessage::MarketState { accepted:false, ledger, .. } if ledger.cargo[0]==12)
    );
    client.teleport(village.market);
    let view = client.market(4, Some(village.id), 1, MarketAction::View);
    let ServerMessage::MarketState {
        market: Some(view), ..
    } = view
    else {
        panic!("Expected market quote")
    };
    let unit_price = view.goods[0].sell_price;
    let sold = client.market(
        5,
        Some(village.id),
        1,
        MarketAction::Sell {
            kind: ResourceKind::Food,
            quantity: 5,
            unit_price,
        },
    );
    assert!(
        matches!(sold, ServerMessage::MarketState { accepted:true, ledger, .. } if ledger.cargo[0]==7 && ledger.coins==5*unit_price && ledger.revision==2)
    );
    let sold: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(
        sold["villages"]["villages"][0]["snapshot"]["food"]
            .as_f64()
            .unwrap(),
        food_before_sale + 5.0
    );
    drop(client);
    server.stop().unwrap();
    let server = spawn(config).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    let resumed =
        client.until(|message| matches!(message, ServerMessage::WorkState { request_id: 0, .. }));
    assert!(
        matches!(resumed, ServerMessage::WorkState { work, ledger, .. } if work.active.is_none() && ledger.cargo[0]==7 && ledger.coins==5*unit_price && ledger.revision==2)
    );
    drop(client);
    server.stop().unwrap();
}

#[test]
fn failed_harvest_save_confirms_no_cargo_and_preserves_the_ripe_crop_on_restart() {
    use rubblekin_core::economy::WorkAction;
    const PROFILE: &str = "00000000000000000000000000000058";
    let save = TestSave::new();
    let config = ServerConfig {
        generation: WorldGeneration::GeographyV6,
        ..save.config(true)
    };
    let (_, site, position) = ripe_harvest_fixture(&config);
    let server = spawn(config.clone()).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    client.teleport(position);
    let started = client.work(1, WorkAction::Start { site });
    assert!(
        matches!(started, ServerMessage::WorkState { accepted: true, .. }),
        "{started:?}"
    );
    client.until_for(
        |message| {
            matches!(message, ServerMessage::WorkState { work, .. }
        if work.active.as_ref().is_some_and(|active| active.elapsed_seconds>=5.25))
        },
        Duration::from_secs(15),
    );
    let before = fs::read(&config.save_path).unwrap();
    let temporary = save
        .0
        .join(format!(".world.json.{}.tmp", std::process::id()));
    fs::create_dir(&temporary).unwrap();
    loop {
        let mut line = String::new();
        match client.reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => assert!(
                !matches!(serde_json::from_str::<ServerMessage>(&line).unwrap(), ServerMessage::WorkState { ledger, .. } if ledger.cargo_total()>0)
            ),
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(error) => panic!("Expected save-failure disconnect: {error}"),
        }
    }
    assert!(server.stop().is_err());
    assert_eq!(fs::read(&config.save_path).unwrap(), before);
    fs::remove_dir(&temporary).unwrap();
    let server = spawn(config.clone()).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    let state =
        client.until(|message| matches!(message, ServerMessage::WorkState { request_id: 0, .. }));
    assert!(
        matches!(state, ServerMessage::WorkState { work, ledger, .. } if work.active.is_none() && ledger.cargo_total()==0 && ledger.revision==0)
    );
    let restored: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(
        restored["villages"]["villages"][0]["snapshot"]["crop_growth"],
        1.0
    );
    assert_eq!(restored["villages"]["villages"][0]["planted"], true);
    drop(client);
    server.stop().unwrap();
}

#[test]
fn local_work_save_failure_does_not_confirm_wages_and_restart_keeps_old_progress() {
    use rubblekin_core::economy::{WorkAction, WorkKind, WorkSite};
    const PROFILE: &str = "00000000000000000000000000000056";
    let save = TestSave::new();
    let mut config = save.config(true);
    config.generation = WorldGeneration::GeographyV5;
    let world = World::generate(42, config.generation);
    let village = &world.settlements().unwrap().villages[0];
    let soil = village.fields[0].plant_positions().next().unwrap();
    let position = [
        soil.x as f32 * CELL_SIZE + 0.25,
        (soil.y + 1) as f32 * CELL_SIZE,
        soil.z as f32 * CELL_SIZE + 0.25,
    ];
    let server = spawn(config.clone()).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    client.teleport(position);
    let started = client.work(
        1,
        WorkAction::Start {
            site: WorkSite {
                village_id: village.id,
                kind: WorkKind::TendField,
                index: 0,
            },
        },
    );
    assert!(
        matches!(started, ServerMessage::WorkState { accepted: true, .. }),
        "{started:?}"
    );
    client.until_for(
        |message| {
            matches!(message,ServerMessage::WorkState{work,..}
        if work.active.as_ref().is_some_and(|active|active.elapsed_seconds>=5.25))
        },
        Duration::from_secs(15),
    );
    let before = fs::read(&config.save_path).unwrap();
    let temporary = save
        .0
        .join(format!(".world.json.{}.tmp", std::process::id()));
    fs::create_dir(&temporary).unwrap();
    loop {
        let mut line = String::new();
        match client.reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => assert!(
                !matches!(serde_json::from_str::<ServerMessage>(&line).unwrap(),ServerMessage::WorkState{ledger,..} if ledger.coins>0)
            ),
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(error) => panic!("Expected save-failure disconnect: {error}"),
        }
    }
    assert!(server.stop().is_err());
    assert_eq!(fs::read(&config.save_path).unwrap(), before);
    fs::remove_dir(&temporary).unwrap();
    let server = spawn(config).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    let state =
        client.until(|message| matches!(message, ServerMessage::WorkState { request_id: 0, .. }));
    assert!(
        matches!(state,ServerMessage::WorkState{work,ledger,..} if ledger.coins==0 && ledger.revision==0 && work.active.is_none())
    );
    drop(client);
    server.stop().unwrap();
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
    let (viewer, public) =
        Client::connect_mode(server.addr, "Parcel viewer", SessionMode::Observer);
    assert!(!serde_json::to_string(&public).unwrap().contains(PROFILE));
    assert!(matches!(public, ServerMessage::Welcome {players,..}
        if players.iter().any(|p|p.parcel_destination==Some(offer.destination))));
    drop(viewer);
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
    assert_eq!(player.parcel_destination, Some(offer.destination));
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
    client.until(|message| {
        matches!(message,ServerMessage::State {players,..}
        if players.iter().find(|p|p.id==session_id).is_some_and(|p|p.parcel_destination.is_none()))
    });
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
fn easter_egg_name_grants_player_admin_without_enabling_other_players() {
    let save = TestSave::new();
    let server = spawn(save.config(false)).unwrap();
    let (mut admin, welcome) = Client::connect(server.addr, "maccam912");
    let admin_id = match welcome {
        ServerMessage::Welcome {
            can_admin,
            session_id,
            ..
        } => {
            assert!(can_admin);
            session_id
        }
        _ => unreachable!(),
    };
    admin.teleport([20.0, 50.0, 20.0]);
    assert!(matches!(
        admin.until(|m| matches!(m, ServerMessage::State { players, .. } if players.iter().any(|p| p.id == admin_id && p.movement_epoch == 1))),
        ServerMessage::State { players, .. } if players.iter().any(|p| p.id == admin_id && p.body.position[0] == 20.0 && p.body.position[2] == 20.0)
    ));
    thread::sleep(Duration::from_millis(110));
    admin.send(ClientMessage::Admin {
        action: AdminAction::SetNpcGoal {
            goal: Some(NpcAction::Rest),
        },
    });
    assert!(matches!(
        admin.until(|m| matches!(m, ServerMessage::Notice { .. })),
        ServerMessage::Notice { text } if text == "NPC settings updated"
    ));
    admin.until(|m| matches!(m, ServerMessage::State { npc, .. } if npc.forced && npc.action == NpcAction::Rest));

    for name in ["Ian", "Maccam912", "maccam912 ", "maccam912x"] {
        let (mut player, welcome) = Client::connect(server.addr, name);
        assert!(matches!(
            welcome,
            ServerMessage::Welcome {
                can_admin: false,
                ..
            }
        ));
        player.send(ClientMessage::AdminCommand {
            command: "tp 30 50 30".into(),
        });
        assert!(matches!(
            player.until(|m| matches!(m, ServerMessage::AdminCommandResult { .. })),
            ServerMessage::AdminCommandResult { text } if text.contains("disabled")
        ));
        player.send(ClientMessage::Admin {
            action: AdminAction::SetNpcGoal { goal: None },
        });
        assert!(matches!(
            player.until(|m| matches!(m, ServerMessage::Notice { .. })),
            ServerMessage::Notice { text } if text.contains("disabled")
        ));
    }
    let (mut target, _) = Client::connect(server.addr, "Violet");
    admin.send(ClientMessage::AdminCommand {
        command: "tp Violet 30 50 30".into(),
    });
    assert!(matches!(
        admin.until(|m| matches!(m, ServerMessage::AdminCommandResult { .. })),
        ServerMessage::AdminCommandResult { text } if text.starts_with("Teleported Violet")
    ));
    target.until(|m| matches!(m, ServerMessage::State { players, .. } if players.iter().any(|p| p.name == "Violet" && p.movement_epoch == 1 && p.body.position[0] == 30.0 && p.body.position[2] == 30.0)));

    // The name exception grants player controls, not observer admission.
    let mut observer = Client::open(server.addr);
    observer.send(ClientMessage::Hello {
        version: PROTOCOL_VERSION,
        name: "maccam912".into(),
        mode: SessionMode::Observer,
        profile_id: None,
    });
    assert!(matches!(
        observer.until(|m| matches!(m, ServerMessage::Notice { .. })),
        ServerMessage::Notice { text } if text.contains("observation is disabled")
    ));
    drop(admin);
    let (_, welcome) = Client::connect(server.addr, "maccam912");
    assert!(matches!(
        welcome,
        ServerMessage::Welcome {
            can_admin: true,
            ..
        }
    ));
    server.stop().unwrap();
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
        WorldGeneration::ValleyV1
        | WorldGeneration::GeographyV3
        | WorldGeneration::GeographyV4
        | WorldGeneration::GeographyV5
        | WorldGeneration::GeographyV6 => {
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
    observer.send(ClientMessage::Work {
        request_id: 93,
        action: rubblekin_core::economy::WorkAction::Start {
            site: rubblekin_core::economy::WorkSite {
                village_id: 0,
                kind: rubblekin_core::economy::WorkKind::TendField,
                index: 0,
            },
        },
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
    let started = Instant::now();
    let (mut client, welcome) = Client::connect(server.addr, "Delayed burst");
    let (id, initial_x) = match welcome {
        ServerMessage::Welcome {
            session_id,
            players,
            ..
        } => (
            session_id,
            players
                .iter()
                .find(|p| p.id == session_id)
                .unwrap()
                .body
                .position[0],
        ),
        _ => unreachable!(),
    };
    // Sixteen requested seconds get only the half-second burst allowance plus
    // elapsed server time. Excess time is corrected while sequences are acked.
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
    let corrected = client.until(|message| {
        matches!(message, ServerMessage::State { players, .. }
            if players.iter().any(|p| p.id == id && p.last_input_sequence == 64))
    });
    if let ServerMessage::State { players, .. } = corrected {
        let x = players.iter().find(|p| p.id == id).unwrap().body.position[0];
        assert!(x - initial_x <= (0.5 + started.elapsed().as_secs_f32()) * 7.0 + 0.01);
    }
    // The same session remains usable after correcting the delayed batch.
    thread::sleep(Duration::from_millis(50));
    client.send(ClientMessage::Input {
        movement_epoch: 0,
        sequence: 65,
        dt: 0.01,
        input: MoveInput {
            fly: true,
            ..Default::default()
        },
        yaw: 0.0,
    });
    client.until(|message| {
        matches!(message, ServerMessage::State { players, .. }
            if players.iter().any(|p| p.id == id && p.last_input_sequence == 65))
    });
    client.send(ClientMessage::Ping);
    client.until(|message| matches!(message, ServerMessage::Pong));
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

fn quarry_fixture(
    config: &ServerConfig,
) -> (World, rubblekin_core::economy::WorkSite, [f32; 3], BlockPos) {
    use rubblekin_core::{
        economy::{WorkKind, WorkSite},
        village_assets::quarry_pile_cells,
        world::BlockEdit,
    };
    let world = World::generate(42, config.generation);
    let building = &world.settlements().unwrap().roadside_landmarks[17].building;
    let anchor = BlockPos::new(7473, 473, 3347);
    assert_eq!(world.block(anchor), Block::Stone);
    // Deplete the other finite pile cells in a disposable world. Both players
    // must capture this same remaining cell, rather than choosing adjacent rocks.
    let local: Vec<_> = quarry_pile_cells().collect();
    let [width, height, depth] = building.dimensions();
    let mut edits = Vec::new();
    for x in building.origin.x..building.origin.x + width {
        for z in building.origin.z..building.origin.z + depth {
            let [lx, lz] = building.local_cell(x, z).unwrap();
            for y in 1..height {
                let position = BlockPos::new(x, building.origin.y + y, z);
                if position != anchor && local.contains(&[lx, y, lz]) {
                    edits.push(BlockEdit {
                        position,
                        block: Block::Air,
                    });
                }
            }
        }
    }
    assert_eq!(edits.len(), 49);
    spawn(config.clone()).unwrap().stop().unwrap();
    let mut save: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    save["edits"] = serde_json::to_value(edits).unwrap();
    fs::write(&config.save_path, serde_json::to_vec(&save).unwrap()).unwrap();
    (
        world,
        WorkSite {
            village_id: 5,
            kind: WorkKind::QuarryStone,
            index: 17,
        },
        [3736.75, 236.0, 1672.75],
        anchor,
    )
}

fn salvage_fixture(
    config: &ServerConfig,
    kind: rubblekin_core::village_assets::BuildingKind,
    resource: rubblekin_core::settlement::ResourceKind,
) -> (
    World,
    rubblekin_core::economy::WorkSite,
    [f32; 3],
    BlockPos,
    [f32; 3],
) {
    use rubblekin_core::{
        economy::{WORK_REACH, WorkKind, WorkSite},
        physics::{EYE_HEIGHT, character_position_is_clear},
        settlement::ResourceKind,
        world::BlockEdit,
    };
    let mut world = World::generate(42, config.generation);
    let (index, site) = world
        .settlements()
        .unwrap()
        .roadside_landmarks
        .iter()
        .enumerate()
        .find(|(_, site)| site.building.kind == kind)
        .unwrap();
    let building = site.building.clone();
    let village_id = world
        .settlements()
        .unwrap()
        .villages
        .iter()
        .min_by(|a, b| {
            let distance =
                |p: [f32; 3]| (p[0] - building.entrance()[0]).hypot(p[2] - building.entrance()[2]);
            distance(a.center)
                .total_cmp(&distance(b.center))
                .then(a.id.cmp(&b.id))
        })
        .unwrap()
        .id;
    let expected = match resource {
        ResourceKind::Timber => Block::Wood,
        ResourceKind::Clay => Block::Clay,
        _ => Block::Stone,
    };
    let cells: Vec<_> = building.resource_pile_cells().collect();
    let anchor = *cells
        .iter()
        .find(|cell| world.block(**cell) == expected)
        .unwrap();
    let edits: Vec<_> = cells
        .into_iter()
        .filter(|cell| *cell != anchor)
        .map(|position| BlockEdit {
            position,
            block: Block::Air,
        })
        .collect();
    for edit in &edits {
        world.set_block(edit.position, edit.block).unwrap();
    }
    let aim = [
        (anchor.x as f32 + 0.5) * CELL_SIZE,
        (anchor.y as f32 + 0.5) * CELL_SIZE,
        (anchor.z as f32 + 0.5) * CELL_SIZE,
    ];
    let mut positions = Vec::new();
    for dx in -4..=4 {
        for dz in -4..=4 {
            let p = [
                ((anchor.x + dx) as f32 + 0.5) * CELL_SIZE,
                (building.origin.y + 1) as f32 * CELL_SIZE,
                ((anchor.z + dz) as f32 + 0.5) * CELL_SIZE,
            ];
            let eye = [p[0], p[1] + EYE_HEIGHT, p[2]];
            let direction = std::array::from_fn(|axis| aim[axis] - eye[axis]);
            if (p[0] - aim[0]).hypot(p[2] - aim[2]) <= WORK_REACH
                && world
                    .block(BlockPos::new(
                        anchor.x + dx,
                        building.origin.y,
                        anchor.z + dz,
                    ))
                    .is_solid()
                && character_position_is_clear(&world, p, &[])
                && world
                    .raycast(eye, direction, 7.)
                    .is_some_and(|hit| hit.position == anchor)
            {
                positions.push(p);
            }
        }
    }
    let (position, second) = positions
        .iter()
        .find_map(|a| {
            positions
                .iter()
                .find(|b| (a[0] - b[0]).hypot(a[2] - b[2]) >= 1.)
                .map(|b| (*a, *b))
        })
        .expect("two reachable standing places at one salvage cell");
    spawn(config.clone()).unwrap().stop().unwrap();
    let mut save: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    save["edits"] = serde_json::to_value(edits).unwrap();
    fs::write(&config.save_path, serde_json::to_vec(&save).unwrap()).unwrap();
    (
        world,
        WorkSite {
            village_id,
            kind: WorkKind::Salvage,
            index: index as u32,
        },
        position,
        anchor,
        second,
    )
}

#[test]
fn quarry_race_is_durable_once_only_broadcasts_removal_and_sells_physical_cargo() {
    finite_resource_race(
        rubblekin_core::village_assets::BuildingKind::QuarryYard,
        rubblekin_core::settlement::ResourceKind::Stone,
    );
}

#[test]
fn wreck_timber_race_is_durable_once_only_and_sells_physical_cargo() {
    finite_resource_race(
        rubblekin_core::village_assets::BuildingKind::CartWreck,
        rubblekin_core::settlement::ResourceKind::Timber,
    );
}

#[test]
fn wreck_stone_race_is_durable_once_only_and_sells_physical_cargo() {
    finite_resource_race(
        rubblekin_core::village_assets::BuildingKind::CartWreck,
        rubblekin_core::settlement::ResourceKind::Stone,
    );
}

#[test]
fn kiln_clay_race_is_durable_once_only_and_sells_physical_cargo() {
    finite_resource_race(
        rubblekin_core::village_assets::BuildingKind::AbandonedKiln,
        rubblekin_core::settlement::ResourceKind::Clay,
    );
}

fn finite_resource_race(
    building_kind: rubblekin_core::village_assets::BuildingKind,
    resource: rubblekin_core::settlement::ResourceKind,
) {
    use rubblekin_core::{
        economy::{MarketAction, WorkAction},
        settlement::ResourceKind,
    };
    const FIRST: &str = "00000000000000000000000000000059";
    const SECOND: &str = "00000000000000000000000000000060";
    let save = TestSave::new();
    let config = ServerConfig {
        generation: WorldGeneration::GeographyV6,
        ..save.config(true)
    };
    let (world, site, position, anchor, second_position) =
        if building_kind == rubblekin_core::village_assets::BuildingKind::QuarryYard {
            let (world, site, position, anchor) = quarry_fixture(&config);
            (
                world,
                site,
                position,
                anchor,
                [position[0] - 1., position[1], position[2]],
            )
        } else {
            salvage_fixture(&config, building_kind, resource)
        };
    let slot = rubblekin_core::economy::resource_index(resource);
    let block = match resource {
        ResourceKind::Timber => Block::Wood,
        ResourceKind::Clay => Block::Clay,
        _ => Block::Stone,
    };
    let server = spawn(config.clone()).unwrap();
    let (mut first, _) = Client::connect_profile(server.addr, FIRST);
    let (mut second, _) = Client::connect_profile(server.addr, SECOND);
    let (mut observer, _) = Client::connect_mode(server.addr, "Observer", SessionMode::Observer);
    observer.send(ClientMessage::Work {
        request_id: 1,
        action: WorkAction::Start { site },
    });
    assert!(
        matches!(observer.until(|m| matches!(m, ServerMessage::Notice {..})), ServerMessage::Notice {text} if text.contains("read-only"))
    );
    assert!(matches!(
        first.work(1, WorkAction::Start { site }),
        ServerMessage::WorkState {
            accepted: false,
            ..
        }
    ));
    first.teleport(position);
    second.teleport(second_position);
    let started = first.work(2, WorkAction::Start { site });
    assert!(
        matches!(started, ServerMessage::WorkState { accepted: true, .. }),
        "{started:?}"
    );
    let started_at = Instant::now();
    let started = second.work(1, WorkAction::Start { site });
    assert!(
        matches!(started, ServerMessage::WorkState { accepted: true, .. }),
        "{started:?}"
    );
    // Keep every peer reading during the six-second race. Leaving both players
    // unread while waiting on the observer can fill TCP buffers with old
    // snapshots. Use the existing race budget for all three peers together.
    let (changed, first_result, second_result) = thread::scope(|scope| {
        let first_wait = scope.spawn(|| {
            first.until_for(
                |m| matches!(m, ServerMessage::WorkState {request_id:0,work,ledger,..} if work.active.is_none() && ledger.cargo[slot]==1),
                Duration::from_secs(15),
            )
        });
        let second_wait = scope.spawn(|| {
            second.until_for(
                |m| matches!(m, ServerMessage::WorkState {request_id:0,work,accepted:false,..} if work.active.is_none()),
                Duration::from_secs(15),
            )
        });
        let changed = observer.until_for(
            |m| matches!(m, ServerMessage::BlockChanged {edit,..} if edit.position == anchor),
            Duration::from_secs(15),
        );
        (
            changed,
            first_wait.join().unwrap(),
            second_wait.join().unwrap(),
        )
    });
    assert!(started_at.elapsed() >= Duration::from_millis(5700));
    assert!(
        matches!(changed, ServerMessage::BlockChanged { request_id:0, edit,..} if edit.block == Block::Air)
    );
    let durable: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(
        durable["consumed_resource_cells"],
        serde_json::json!([anchor])
    );
    assert_eq!(durable["profiles"][FIRST]["ledger"]["cargo"][slot], 1);
    assert_eq!(durable["profiles"][SECOND]["ledger"]["cargo"][slot], 0);
    assert!(
        matches!(first_result, ServerMessage::WorkState {accepted:true,ledger,..} if ledger.coins==0 && ledger.revision==1)
    );
    assert!(
        matches!(second_result, ServerMessage::WorkState {ledger,..} if ledger.cargo_total()==0)
    );
    assert!(matches!(
        first.work(2, WorkAction::Start { site }),
        ServerMessage::WorkState {
            accepted: false,
            ..
        }
    ));
    // Restoring this exact paid stone must not mint cargo again for either profile.
    first.send(ClientMessage::Edit {
        request_id: 3,
        position: anchor,
        block,
    });
    first.until(|m| matches!(m, ServerMessage::BlockChanged { request_id: 3, .. }));
    let replay = second.work(2, WorkAction::Start { site });
    assert!(
        matches!(replay, ServerMessage::WorkState {accepted:false,ledger,..} if ledger.cargo_total()==0)
    );
    let remote = first.market(
        4,
        Some(site.village_id),
        1,
        MarketAction::Sell {
            kind: resource,
            quantity: 1,
            unit_price: 1,
        },
    );
    assert!(
        matches!(remote, ServerMessage::MarketState {accepted:false,ledger,..} if ledger.cargo[slot]==1)
    );
    let village = world
        .settlements()
        .unwrap()
        .villages
        .iter()
        .find(|v| v.id == site.village_id)
        .unwrap();
    first.teleport(village.market);
    let ServerMessage::MarketState {
        market: Some(market),
        ..
    } = first.market(5, Some(site.village_id), 1, MarketAction::View)
    else {
        panic!("Expected quote")
    };
    let price = market.goods[slot].sell_price;
    let sold = first.market(
        6,
        Some(site.village_id),
        1,
        MarketAction::Sell {
            kind: resource,
            quantity: 1,
            unit_price: price,
        },
    );
    assert!(
        matches!(sold, ServerMessage::MarketState {accepted:true,ledger,..} if ledger.coins==price && ledger.cargo[slot]==0 && ledger.revision==2)
    );
    drop((first, second, observer));
    server.stop().unwrap();
    let server = spawn(config.clone()).unwrap();
    let (mut first, welcome) = Client::connect_profile(server.addr, FIRST);
    assert!(
        matches!(welcome, ServerMessage::Welcome {edits,..} if !edits.iter().any(|edit| edit.position==anchor))
    );
    assert!(
        matches!(first.until(|m| matches!(m, ServerMessage::WorkState {request_id:0,..})), ServerMessage::WorkState {ledger,..} if ledger.coins==price && ledger.cargo_total()==0)
    );
    first.teleport(position);
    assert!(matches!(
        first.work(1, WorkAction::Start { site }),
        ServerMessage::WorkState {
            accepted: false,
            ..
        }
    ));
    let durable: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(
        durable["consumed_resource_cells"],
        serde_json::json!([anchor])
    );
    drop(first);
    server.stop().unwrap();
}

#[test]
fn quarry_save_failure_confirms_neither_block_removal_nor_cargo_and_restart_keeps_stone() {
    finite_resource_save_failure(false);
}

#[test]
fn salvage_save_failure_confirms_neither_block_removal_nor_cargo_and_restart_keeps_clay() {
    finite_resource_save_failure(true);
}

fn finite_resource_save_failure(salvage: bool) {
    use rubblekin_core::economy::WorkAction;
    const PROFILE: &str = "00000000000000000000000000000061";
    let save = TestSave::new();
    let config = ServerConfig {
        generation: WorldGeneration::GeographyV6,
        ..save.config(true)
    };
    let (site, position, anchor) = if salvage {
        let (_, site, position, anchor, _) = salvage_fixture(
            &config,
            rubblekin_core::village_assets::BuildingKind::AbandonedKiln,
            rubblekin_core::settlement::ResourceKind::Clay,
        );
        (site, position, anchor)
    } else {
        let (_, site, position, anchor) = quarry_fixture(&config);
        (site, position, anchor)
    };
    let server = spawn(config.clone()).unwrap();
    let (mut client, _) = Client::connect_profile(server.addr, PROFILE);
    let (mut observer, _) = Client::connect_mode(server.addr, "Observer", SessionMode::Observer);
    client.teleport(position);
    assert!(matches!(
        client.work(1, WorkAction::Start { site }),
        ServerMessage::WorkState { accepted: true, .. }
    ));
    client.until_for(|m| matches!(m,ServerMessage::WorkState {work,..} if work.active.as_ref().is_some_and(|w|w.elapsed_seconds>=5.25)),Duration::from_secs(15));
    let before = fs::read(&config.save_path).unwrap();
    let temporary = save
        .0
        .join(format!(".world.json.{}.tmp", std::process::id()));
    fs::create_dir(&temporary).unwrap();
    for connection in [&mut client, &mut observer] {
        loop {
            let mut line = String::new();
            match connection.reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    // Fatal shutdown can interrupt an ordinary large State frame.
                    // A partial EOF frame is not a protocol confirmation.
                    let message = match serde_json::from_str::<ServerMessage>(&line) {
                        Ok(message) => message,
                        Err(error) if !line.ends_with('\n') && error.is_eof() => break,
                        Err(error) => panic!("Invalid complete frame: {error}"),
                    };
                    match message {
                        ServerMessage::BlockChanged { edit, .. } => {
                            assert_ne!(edit.position, anchor)
                        }
                        ServerMessage::WorkState { ledger, .. } => {
                            assert_eq!(ledger.cargo_total(), 0)
                        }
                        _ => {}
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(error) => panic!("Expected save-failure disconnect: {error}"),
            }
        }
    }
    assert!(server.stop().is_err());
    assert_eq!(fs::read(&config.save_path).unwrap(), before);
    fs::remove_dir(temporary).unwrap();
    let server = spawn(config.clone()).unwrap();
    let (mut client, welcome) = Client::connect_profile(server.addr, PROFILE);
    assert!(
        matches!(welcome,ServerMessage::Welcome {edits,..} if !edits.iter().any(|edit|edit.position==anchor))
    );
    assert!(
        matches!(client.until(|m|matches!(m,ServerMessage::WorkState {request_id:0,..})),ServerMessage::WorkState {ledger,work,..} if ledger.cargo_total()==0 && work.active.is_none())
    );
    assert!(matches!(
        client.work(1, WorkAction::Start { site }),
        ServerMessage::WorkState { accepted: true, .. }
    ));
    client.work(2, WorkAction::Cancel);
    let durable: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(durable["consumed_resource_cells"], serde_json::json!([]));
    drop(client);
    server.stop().unwrap();
}

#[test]
fn wildlife_replicates_advances_without_clients_and_keeps_its_saved_population() {
    let save = TestSave::new();
    let mut config = save.config(true);
    config.generation = WorldGeneration::GeographyV6;
    let server = spawn(config.clone()).unwrap();
    let (mut observer, _) =
        Client::connect_mode(server.addr, "Wildlife review", SessionMode::Observer);
    let first = observer.until(|m| matches!(m, ServerMessage::WildlifeState { .. }));
    let ServerMessage::WildlifeState { animals, habitats } = first else {
        unreachable!()
    };
    assert!(animals.len() > 100);
    assert!(habitats.len() > 40);
    let first_hunger = animals[0].hunger;
    drop(observer);
    thread::sleep(Duration::from_millis(1100));
    let (mut observer, _) =
        Client::connect_mode(server.addr, "Wildlife return", SessionMode::Observer);
    let next = observer.until(|m| matches!(m, ServerMessage::WildlifeState { .. }));
    let ServerMessage::WildlifeState { animals: next, .. } = next else {
        unreachable!()
    };
    assert!(
        next.iter()
            .any(|a| a.id == animals[0].id && a.hunger != first_hunger),
        "wildlife needs must advance with nobody connected"
    );
    drop(observer);
    server.stop().unwrap();
    let mut saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(saved["version"], 11);
    saved["ecology"]["animals"][0]["hunger"] = 12.345.into();
    fs::write(&config.save_path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let server = spawn(config.clone()).unwrap();
    let (mut observer, _) =
        Client::connect_mode(server.addr, "Wildlife restart", SessionMode::Observer);
    let message = observer.until(|m| matches!(m, ServerMessage::WildlifeState { .. }));
    let ServerMessage::WildlifeState { animals, .. } = message else {
        unreachable!()
    };
    assert!(
        animals[0].hunger < 15.,
        "restart must load needs instead of reseeding animals"
    );
    observer.send(ClientMessage::AdminCommand {
        command: "wildlife".into(),
    });
    let reply = observer.until(|m| matches!(m, ServerMessage::AdminCommandResult { .. }));
    assert!(
        matches!(reply,ServerMessage::AdminCommandResult{text} if text.contains("rabbits")&&text.contains("wolves"))
    );
    drop(observer);
    server.stop().unwrap();
}

#[test]
fn creative_catalog_blocks_replicate_and_survive_server_restart() {
    let save = TestSave::new();
    let config = save.config(false);
    let server = spawn(config.clone()).unwrap();
    let (mut builder, _) = Client::connect(server.addr, "Creative builder");
    let (mut spectator, _) = Client::connect(server.addr, "Creative neighbor");
    let position = nearby_air();
    for (index, &block) in Block::ALL.iter().enumerate() {
        if index != 0 {
            thread::sleep(Duration::from_millis(110));
        }
        let request_id = index as u64 + 1;
        builder.send(ClientMessage::Edit {
            request_id,
            position,
            block,
        });
        for client in [&mut builder, &mut spectator] {
            let reply = client.until(|m| matches!(m, ServerMessage::BlockChanged { request_id: id, .. } | ServerMessage::Rejected { request_id: id, .. } if *id == request_id));
            assert!(
                matches!(reply, ServerMessage::BlockChanged { edit, .. } if edit.position == position && edit.block == block),
                "{block:?}: {reply:?}"
            );
        }
    }
    drop(builder);
    drop(spectator);
    server.stop().unwrap();
    let server = spawn(config).unwrap();
    let (_, welcome) = Client::connect(server.addr, "Creative builder returned");
    assert!(
        matches!(welcome,ServerMessage::Welcome { edits, .. } if edits.iter().any(|e| e.position == position && e.block == *Block::ALL.last().unwrap()))
    );
    server.stop().unwrap();
}

#[test]
fn whip_carriage_is_shared_and_jump_out_replicates_over_tcp() {
    use rubblekin_core::gliders::{GliderAction, GliderDestination, stations};
    let save = TestSave::new();
    let mut config = save.config(true);
    config.generation = WorldGeneration::GeographyV3;
    let world = World::generate(42, config.generation);
    let stops = stations(&world);
    let origin = &stops[0];
    let destination = stops
        .iter()
        .filter(|s| s.village_id != origin.village_id)
        .min_by(|a, b| {
            rubblekin_core::gliders::horizontal_distance(origin.position, a.position).total_cmp(
                &rubblekin_core::gliders::horizontal_distance(origin.position, b.position),
            )
        })
        .unwrap();
    let server = spawn(config).unwrap();
    let (mut first, welcome) = Client::connect(server.addr, "WhipOne");
    let id = match welcome {
        ServerMessage::Welcome { session_id, .. } => session_id,
        _ => unreachable!(),
    };
    first.teleport(origin.position);
    let (mut second, _) = Client::connect(server.addr, "WhipTwo");
    let mut nearby = origin.position;
    nearby[0] += 1.0;
    second.teleport(nearby);
    let action = GliderAction::Board {
        station_id: origin.village_id,
        destination: GliderDestination::Village(destination.village_id),
    };
    first.send(ClientMessage::Glider {
        action: action.clone(),
    });
    first.until(|m|matches!(m,ServerMessage::State{players,..} if players.iter().any(|p|p.id==id && p.glider_ride.is_some())));
    second.send(ClientMessage::Glider { action });
    second.until(|m|matches!(m,ServerMessage::State{players,..} if players.iter().filter(|p|p.glider_ride.is_some()).count()==2));
    thread::sleep(Duration::from_millis(220));
    first.send(ClientMessage::Glider {
        action: GliderAction::Launch,
    });
    let state=second.until(|m|matches!(m,ServerMessage::State{gliders,..} if gliders.iter().any(|f|f.started_at.is_some())));
    if let ServerMessage::State {
        players, gliders, ..
    } = state
    {
        assert_eq!(gliders.len(), 1);
        assert_eq!(
            players
                .iter()
                .filter(|p| p
                    .glider_ride
                    .is_some_and(|r| r.carriage_id == gliders[0].id))
                .count(),
            2
        );
    }
    thread::sleep(Duration::from_millis(2300));
    first.send(ClientMessage::Glider {
        action: GliderAction::Leave,
    });
    second.until(|m|matches!(m,ServerMessage::State{players,..} if players.iter().any(|p|p.id==id && p.gliding && p.glider_ride.is_none())));
    let (mut observer, _) = Client::connect_mode(server.addr, "Observer", SessionMode::Observer);
    observer.send(ClientMessage::Glider {
        action: GliderAction::Launch,
    });
    assert!(
        matches!(observer.until(|m|matches!(m,ServerMessage::Notice{..})),ServerMessage::Notice{text} if text.contains("read-only"))
    );
    server.stop().unwrap();
}
