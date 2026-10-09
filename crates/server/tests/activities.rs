//! Real TCP ownership, duplicate reward, disconnect and restart checks.
use rubblekin_core::{activities::*, protocol::*, world::WorldGeneration};
use rubblekin_server::{ServerConfig, spawn};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    id: u64,
    next: u64,
}
impl Client {
    fn connect(addr: SocketAddr, profile: &str) -> Self {
        Self::connect_mode(addr, Some(profile), SessionMode::Player)
    }
    fn connect_mode(addr: SocketAddr, profile: Option<&str>, mode: SessionMode) -> Self {
        let writer = TcpStream::connect(addr).unwrap();
        writer
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        let mut c = Self {
            reader: BufReader::new(writer.try_clone().unwrap()),
            writer,
            id: 0,
            next: 1,
        };
        c.send(ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            name: "Helper".into(),
            mode,
            profile_id: profile.map(str::to_owned),
        });
        if let ServerMessage::Welcome { session_id, .. } =
            c.until(|m| matches!(m, ServerMessage::Welcome { .. }))
        {
            c.id = session_id;
        }
        c
    }
    fn send(&mut self, m: ClientMessage) {
        let mut bytes = serde_json::to_vec(&m).unwrap();
        bytes.push(b'\n');
        self.writer.write_all(&bytes).unwrap();
    }
    fn until(&mut self, p: impl Fn(&ServerMessage) -> bool) -> ServerMessage {
        let start = std::time::Instant::now();
        loop {
            assert!(start.elapsed() < Duration::from_secs(20));
            let mut line = String::new();
            assert!(self.reader.read_line(&mut line).unwrap() > 0);
            let m: ServerMessage = serde_json::from_str(&line).unwrap();
            if p(&m) {
                return m;
            }
        }
    }
    fn state(&mut self) -> Vec<ActivitySnapshot> {
        match self.until(|m| matches!(m, ServerMessage::ActivityState { .. })) {
            ServerMessage::ActivityState { activities, .. } => activities,
            _ => unreachable!(),
        }
    }
    fn tp(&mut self, p: [f32; 3]) {
        thread::sleep(Duration::from_millis(160));
        self.send(ClientMessage::AdminCommand {
            command: format!("tp {} {} {}", p[0], p[1], p[2]),
        });
        let reply = self.until(|m| matches!(m, ServerMessage::AdminCommandResult { .. }));
        assert!(
            matches!(reply,ServerMessage::AdminCommandResult {text} if text.contains("Teleported"))
        );
    }
    fn action(
        &mut self,
        a: &ActivitySnapshot,
        action: ActivityAction,
    ) -> (bool, Vec<ActivitySnapshot>) {
        thread::sleep(Duration::from_millis(160));
        let request_id = self.next;
        self.next += 1;
        self.send(ClientMessage::Activity {
            request_id,
            activity_id: a.plan.id,
            revision: a.revision,
            action,
        });
        match self.until(
            |m| matches!(m,ServerMessage::ActivityState {request_id:id,..} if *id==request_id),
        ) {
            ServerMessage::ActivityState {
                accepted,
                activities,
                ..
            } => (accepted, activities),
            _ => unreachable!(),
        }
    }
}
struct Save(PathBuf);
impl Drop for Save {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn old_save9_gains_poi_scenes_and_their_identity_and_receipts_survive_restart() {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let save = Save(std::env::temp_dir().join(format!(
        "rubblekin-poi-activities-{}-{suffix}",
        std::process::id()
    )));
    let config = ServerConfig {
        bind_addr: "127.0.0.1:0".into(),
        save_path: save.0.join("world.json"),
        seed: 42,
        generation: WorldGeneration::GeographyV6,
        allow_admin: true,
    };
    let server = spawn(config.clone()).unwrap();
    server.stop().unwrap();
    // A genuine earlier Save9 record has just the two introduction scenes.
    let mut old: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    old["activities"]["records"]
        .as_array_mut()
        .unwrap()
        .truncate(2);
    assert_eq!(old["version"], 9);
    fs::write(&config.save_path, serde_json::to_vec(&old).unwrap()).unwrap();
    let server = spawn(config.clone()).unwrap();
    let profile = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut client = Client::connect(server.addr, profile);
    let mut states = client.state();
    assert_eq!(states.len(), 8);
    let index = states
        .iter()
        .position(|a| a.plan.site_id.is_some() && a.plan.kind == ActivityKind::SpilledSupplies)
        .unwrap();
    let plan = states[index].plan.clone();
    client.tp(plan.objects[0]);
    let (ok, s) = client.action(&states[index], ActivityAction::Take(0));
    assert!(ok);
    states = s;
    client.tp(plan.sockets[0]);
    let (ok, s) = client.action(&states[index], ActivityAction::Place(0));
    assert!(ok);
    states = s;
    assert_eq!(states[index].props[0], PropState::Placed);
    drop(client);
    server.stop().unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(saved["profiles"][profile]["ledger"]["coins"], 2);
    let server = spawn(config.clone()).unwrap();
    let mut client = Client::connect(server.addr, profile);
    let restored = client.state();
    assert_eq!(restored[index].plan, plan);
    assert_eq!(restored[index].props[0], PropState::Placed);
    client.tp(plan.objects[0]);
    assert!(!client.action(&restored[index], ActivityAction::Take(0)).0);
    drop(client);
    server.stop().unwrap();
    let mut bad: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    bad["activities"]["records"][index]["plan"]["site_id"] = 999.into();
    let bytes = serde_json::to_vec(&bad).unwrap();
    fs::write(&config.save_path, &bytes).unwrap();
    assert!(spawn(config).is_err());
    assert_eq!(fs::read(save.0.join("world.json")).unwrap(), bytes);
}
#[test]
fn cooperative_activities_save_pay_once_and_recover_a_disconnected_prop() {
    let save = Save(std::env::temp_dir().join(format!(
            "rubblekin-activities-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    let config = ServerConfig {
        bind_addr: "127.0.0.1:0".into(),
        save_path: save.0.join("world.json"),
        seed: 42,
        generation: WorldGeneration::ValleyV1,
        allow_admin: true,
    };
    let server = spawn(config.clone()).unwrap();
    let p = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let q = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let mut first = Client::connect(server.addr, p);
    let mut states = first.state();
    assert_eq!(states.len(), 2);
    let mut second = Client::connect(server.addr, q);
    second.state();
    let plan = states[0].plan.clone();
    first.tp(plan.objects[0]);
    let (ok, s) = first.action(&states[0], ActivityAction::Take(0));
    assert!(ok);
    states = s;
    second.tp([
        plan.objects[0][0] + 0.8,
        plan.objects[0][1],
        plan.objects[0][2],
    ]);
    let (ok, _) = second.action(&states[0], ActivityAction::Take(0));
    assert!(!ok);
    first.tp(plan.sockets[1]);
    let (ok, _) = first.action(&states[0], ActivityAction::Place(1));
    assert!(!ok);
    first.tp(plan.sockets[0]);
    let (ok, s) = first.action(&states[0], ActivityAction::Place(0));
    assert!(ok);
    states = s;
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(saved["profiles"][p]["ledger"]["coins"], 2);
    // Replay the identical request, including its old revision.
    let request_id = first.next - 1;
    first.send(ClientMessage::Activity {
        request_id,
        activity_id: 1,
        revision: 1,
        action: ActivityAction::Place(0),
    });
    let reply = first
        .until(|m| matches!(m,ServerMessage::ActivityState {request_id:id,..} if *id==request_id));
    assert!(matches!(
        reply,
        ServerMessage::ActivityState {
            accepted: false,
            ..
        }
    ));
    second.tp(plan.objects[1]);
    let (ok, s) = second.action(&states[0], ActivityAction::Take(1));
    assert!(ok);
    states = s;
    assert_eq!(states[0].props[1], PropState::Held(second.id));
    drop(second);
    states=match first.until(|m|matches!(m,ServerMessage::ActivityState {activities,..} if activities[0].props[1]==PropState::Home)) {ServerMessage::ActivityState {activities,..}=>activities,_=>unreachable!()};
    let mut second = Client::connect(server.addr, q);
    second.state();
    for i in 1..3 {
        second.tp(plan.objects[i]);
        let (ok, s) = second.action(&states[0], ActivityAction::Take(i as u8));
        assert!(ok);
        states = s;
        second.tp(plan.sockets[i]);
        let (ok, s) = second.action(&states[0], ActivityAction::Place(i as u8));
        assert!(ok);
        states = s;
    }
    assert!(states[0].complete);
    for i in 0..3 {
        while states[1].faces[i] != states[1].plan.answer[i] {
            first.tp(states[1].plan.sockets[i]);
            let (ok, s) = first.action(&states[1], ActivityAction::Turn(i as u8));
            assert!(ok);
            states = s;
        }
    }
    assert!(states[1].complete);
    drop(first);
    drop(second);
    server.stop().unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&config.save_path).unwrap()).unwrap();
    assert_eq!(saved["profiles"][p]["ledger"]["coins"], 2);
    assert_eq!(saved["profiles"][q]["ledger"]["coins"], 4);
    let server = spawn(config.clone()).unwrap();
    let mut c = Client::connect(server.addr, p);
    let restored = c.state();
    assert!(restored.iter().all(|a| a.complete));
    c.tp(plan.objects[0]);
    assert!(!c.action(&restored[0], ActivityAction::Take(0)).0);
    drop(c);
    server.stop().unwrap();
    let bytes = fs::read(&config.save_path).unwrap();
    let mut bad: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    bad.as_object_mut().unwrap().remove("activities");
    fs::write(&config.save_path, serde_json::to_vec(&bad).unwrap()).unwrap();
    assert!(spawn(config.clone()).is_err());
    assert_eq!(
        fs::read(&config.save_path).unwrap(),
        serde_json::to_vec(&bad).unwrap()
    );
}

#[test]
fn observer_cannot_mutate_and_a_failed_save_confirms_no_reward() {
    let save = Save(std::env::temp_dir().join(format!(
            "rubblekin-activity-save-failure-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
    let config = ServerConfig {
        bind_addr: "127.0.0.1:0".into(),
        save_path: save.0.join("world.json"),
        seed: 42,
        generation: WorldGeneration::ValleyV1,
        allow_admin: true,
    };
    let server = spawn(config.clone()).unwrap();
    let mut observer = Client::connect_mode(server.addr, None, SessionMode::Observer);
    let states = observer.state();
    observer.send(ClientMessage::Activity {
        request_id: 1,
        activity_id: 1,
        revision: 0,
        action: ActivityAction::Take(0),
    });
    assert!(
        matches!(observer.until(|m|matches!(m,ServerMessage::Notice {..})),ServerMessage::Notice {text} if text.contains("read-only"))
    );
    drop(observer);
    let profile = "cccccccccccccccccccccccccccccccc";
    let mut c = Client::connect(server.addr, profile);
    c.state();
    let plan = states[0].plan.clone();
    c.tp(plan.objects[0]);
    let (ok, states) = c.action(&states[0], ActivityAction::Take(0));
    assert!(ok);
    c.tp(plan.sockets[0]);
    let before = fs::read(&config.save_path).unwrap();
    let temporary = save
        .0
        .join(format!(".world.json.{}.tmp", std::process::id()));
    fs::create_dir(&temporary).unwrap();
    thread::sleep(Duration::from_millis(160));
    let request_id = c.next;
    c.send(ClientMessage::Activity {
        request_id,
        activity_id: 1,
        revision: states[0].revision,
        action: ActivityAction::Place(0),
    });
    loop {
        let mut line = String::new();
        match c.reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let m: ServerMessage = serde_json::from_str(&line).unwrap();
                assert!(
                    !matches!(m,ServerMessage::ActivityState {request_id:id,accepted:true,..} if id==request_id)
                );
                assert!(!matches!(m,ServerMessage::MarketState {ledger,..} if ledger.coins>0));
            }
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(e) => panic!("Expected save-failure disconnect: {e}"),
        }
    }
    assert!(server.stop().is_err());
    assert_eq!(fs::read(&config.save_path).unwrap(), before);
    fs::remove_dir(&temporary).unwrap();
    let server = spawn(config).unwrap();
    let mut c = Client::connect(server.addr, profile);
    let restored = c.state();
    assert_eq!(restored[0].props[0], PropState::Home);
    assert!(!restored[0].complete);
    let reply = c.until(|m| matches!(m, ServerMessage::MarketState { .. }));
    assert!(matches!(reply,ServerMessage::MarketState {ledger,..} if ledger.coins==0));
    drop(c);
    server.stop().unwrap();
}
