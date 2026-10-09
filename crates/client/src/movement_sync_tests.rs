//! Exercise bounded movement and recovery through the actual controls and TCP receive path.
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};

fn fixture() -> (App, BufReader<TcpStream>, PlayerSnapshot) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let client = std::thread::spawn(move || {
        Connection::connect(&address, "Tester".into(), SessionMode::Player).unwrap()
    });
    let (socket, _) = listener.accept().unwrap();
    socket.set_nodelay(true).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut peer = BufReader::new(socket);
    peer.read_line(&mut String::new()).unwrap();
    let mut welcome = join::tests::welcome(SessionMode::Player);
    if let ServerMessage::Welcome { players, .. } = &mut welcome {
        players[0].body = Body::new([0.25, 20.0, 0.25]);
    }
    serde_json::to_writer(peer.get_mut(), &welcome).unwrap();
    peer.get_mut().write_all(b"\n").unwrap();
    let (connection, welcome) = client.join().unwrap();
    let (world, mut session) = join::session_from_welcome(
        welcome,
        "test peer".into(),
        GraphicsQuality::Low,
        0.0,
        SessionMode::Player,
    )
    .unwrap();
    session.captured = true;
    session.flying = true;
    let player = session.players[0].clone();
    let mut app = App::new();
    let mut queue = bevy::ecs::world::CommandQueue::default();
    let mut meshes = Assets::<Mesh>::default();
    let scene = terrain::setup_terrain(
        &mut Commands::new(&mut queue, app.world()),
        &mut meshes,
        &mut Assets::default(),
        &mut Assets::default(),
        &mut Assets::default(),
        &world,
        session.body.position,
        0,
        128.0,
        1,
    );
    queue.apply(app.world_mut());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(250));
    app.insert_resource(VoxelWorld(world))
        .insert_resource(session)
        .insert_resource(connection)
        .insert_resource(scene)
        .insert_resource(meshes)
        .insert_resource(time)
        .insert_resource(GraphicsSettings::new(GraphicsQuality::Low))
        .init_resource::<airships::PilotConversation>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<AccumulatedMouseScroll>()
        .init_resource::<DiagnosticsStore>()
        .init_resource::<touch::TouchControls>()
        .add_systems(Update, controls)
        .add_systems(PostUpdate, receive_network);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        CursorOptions::default(),
    ));
    (app, peer, player)
}

fn next_message(peer: &mut BufReader<TcpStream>) -> ClientMessage {
    let mut line = String::new();
    peer.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

fn acknowledge(app: &mut App, peer: &mut BufReader<TcpStream>, player: &PlayerSnapshot) {
    let session = app.world().resource::<Session>();
    let state = ServerMessage::State {
        gliders: Vec::new(),
        players: vec![player.clone()],
        npc: session.npc.clone(),
        residents: session.residents.clone(),
        villages: session.villages.clone(),
        world_time: session.world_time,
    };
    serde_json::to_writer(peer.get_mut(), &state).unwrap();
    peer.get_mut().write_all(b"\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        app.world_mut().run_schedule(PostUpdate);
        assert!(app.world().resource::<Connection>().error.is_none());
        if app.world().resource::<Session>().players[0].last_input_sequence
            == player.last_input_sequence
        {
            break;
        }
        assert!(Instant::now() < deadline, "acknowledgment never arrived");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn mouse_camera_aim_is_sent_and_steers_the_canopy_without_walking_keys() {
    let (mut app, mut peer, _) = fixture();
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        session.flying = false;
        session.gliding = true;
        session.yaw = std::f32::consts::FRAC_PI_2;
        session.pitch = 0.0;
        session.body.velocity = [0.0, -2.0, -25.0];
    }
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::new(-40.0, 240.0);
    app.world_mut().run_schedule(Update);
    let ClientMessage::Input { input, .. } = next_message(&mut peer) else {
        panic!("missing movement")
    };
    assert_eq!(input.direction, [0.0; 2]);
    let aim = Vec3::from_array(
        input
            .glide_direction
            .expect("camera aim missing from input"),
    );
    assert!(aim.x > 0.8 && aim.y < -0.5);
    let session = app.world().resource::<Session>();
    assert!(session.body.velocity[0] > 15.0 && session.body.velocity[1] < -8.0);
    let heading = Vec2::new(session.body.velocity[0], session.body.velocity[2]).normalize();
    assert!(heading.dot(Vec2::new(aim.x, aim.z).normalize()) > 0.9999);
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::new(0.0, -480.0);
    app.world_mut().run_schedule(Update);
    let ClientMessage::Input { input, .. } = next_message(&mut peer) else {
        panic!("missing pull-up")
    };
    assert!(input.glide_direction.unwrap()[1] > 0.5);
    assert!(app.world().resource::<Session>().body.velocity[1] > 0.0);
}

#[test]
fn delayed_acknowledgments_pause_and_resume_without_disconnect_or_replayed_keys() {
    let (mut app, mut peer, mut authoritative) = fixture();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    let mut stopped_at = None;
    // Four seconds of rendered frame time, with no movement acknowledgment.
    // The first two seconds may be predicted; the remainder must wait safely.
    for frame in 0..16 {
        if frame == 8 {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(KeyCode::KeyW);
            stopped_at = Some(app.world().resource::<Session>().body.clone());
        }
        app.world_mut().run_schedule(Update);
        assert!(
            app.world().resource::<Connection>().error.is_none(),
            "client disconnected after {} seconds: {:?}",
            (frame + 1) as f32 * 0.25,
            app.world().resource::<Connection>().error
        );
        if frame < 8 {
            let ClientMessage::Input {
                sequence,
                input,
                dt,
                ..
            } = next_message(&mut peer)
            else {
                panic!("expected movement input");
            };
            assert_eq!(sequence, frame + 1);
            rubblekin_core::physics::move_character(
                &app.world().resource::<VoxelWorld>().0,
                &mut authoritative.body,
                input,
                dt,
            );
            authoritative.last_input_sequence = sequence;
        } else {
            assert_eq!(
                app.world().resource::<Session>().body.position,
                stopped_at.as_ref().unwrap().position
            );
        }
    }
    // A bounded prediction pause keeps the transport open for ordinary events.
    app.world_mut()
        .resource_mut::<Connection>()
        .send(ClientMessage::Ping);
    assert!(matches!(next_message(&mut peer), ClientMessage::Ping));
    acknowledge(&mut app, &mut peer, &authoritative);
    assert_eq!(
        app.world().resource::<Session>().body.position,
        authoritative.body.position
    );
    app.world_mut().run_schedule(Update);
    let ClientMessage::Input {
        sequence, input, ..
    } = next_message(&mut peer)
    else {
        panic!("expected resumed movement input");
    };
    assert_eq!(sequence, 9, "waiting frames must not spend input sequences");
    assert_eq!(input.direction, [0.0; 2], "released input was replayed");
    assert_eq!(
        app.world().resource::<Session>().body.position,
        authoritative.body.position
    );
}
