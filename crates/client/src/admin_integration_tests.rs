//! The console must own gameplay input, including the frame it closes.
use super::*;
use bevy::{
    gizmos::config::DefaultGizmoConfigGroup,
    input::touch::TouchInput,
    window::{PrimaryWindow, WindowFocused},
};
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    time::Duration,
};

fn fixture() -> (App, BufReader<TcpStream>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let client = std::thread::spawn(move || {
        Connection::connect(&address, "Tester".into(), SessionMode::Player).unwrap()
    });
    let (socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut peer = BufReader::new(socket);
    peer.read_line(&mut String::new()).unwrap();
    let mut welcome = join::tests::welcome(SessionMode::Player);
    if let ServerMessage::Welcome { can_admin, .. } = &mut welcome {
        *can_admin = true;
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
    session.body = Body::new([0.25, 20.0, 0.25]);
    session.captured = true;
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(25));
    let mut app = App::new();
    app.insert_resource(VoxelWorld(world))
        .insert_resource(session)
        .insert_resource(connection)
        .insert_resource(time)
        .insert_resource(GraphicsSettings::new(GraphicsQuality::Low))
        .init_resource::<admin_console::AdminConsole>()
        .init_resource::<pause::PauseMenu>()
        .init_resource::<airships::PilotConversation>()
        .insert_resource(touch::TouchControls::new(false))
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<AccumulatedMouseScroll>()
        .init_resource::<DiagnosticsStore>()
        .init_gizmo_group::<DefaultGizmoConfigGroup>()
        .add_message::<join::MenuKey>()
        .add_message::<TouchInput>()
        .add_message::<WindowFocused>()
        .add_systems(
            Update,
            (
                admin_console::read,
                airships::read,
                pause::read,
                controls,
                edit_blocks,
            )
                .chain(),
        );
    app.world_mut().spawn((
        PrimaryWindow,
        Window {
            focused: true,
            ..default()
        },
        CursorOptions::default(),
    ));
    app.world_mut().spawn((
        GameCamera,
        Transform::from_xyz(0.25, 6.0, 0.25).looking_to(Vec3::NEG_Y, Vec3::Z),
    ));
    (app, peer)
}

fn next_message(peer: &mut BufReader<TcpStream>) -> ClientMessage {
    let mut line = String::new();
    peer.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

#[test]
fn console_open_and_close_frames_block_movement_building_and_shortcuts() {
    let (mut app, mut peer) = fixture();
    let initial = {
        let session = app.world().resource::<Session>();
        (
            session.yaw,
            session.pitch,
            session.selected,
            session.inspector,
            session.help,
        )
    };
    for key in [
        KeyCode::Backquote,
        KeyCode::KeyW,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::Digit1,
        KeyCode::Tab,
        KeyCode::F2,
        KeyCode::F6,
        KeyCode::Space,
    ] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::splat(300.0);
    app.world_mut().run_schedule(Update);
    assert!(app.world().resource::<admin_console::AdminConsole>().open);
    assert!(!app.world().resource::<Session>().captured);
    assert!(!app.world().resource::<Session>().flying);
    assert!(!app.world().resource::<airships::PilotConversation>().open());
    assert!(!app.world().resource::<pause::PauseMenu>().open);
    assert_eq!(
        app.world().resource::<GraphicsSettings>().quality,
        GraphicsQuality::Low
    );
    let session = app.world().resource::<Session>();
    assert_eq!(
        (
            session.yaw,
            session.pitch,
            session.selected,
            session.inspector,
            session.help
        ),
        initial
    );
    assert!(
        matches!(next_message(&mut peer), ClientMessage::Input { input, .. }
        if input.direction == [0.0; 2] && !input.jump && !input.fly)
    );

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.world_mut().run_schedule(Update);
    assert!(!app.world().resource::<admin_console::AdminConsole>().open);
    assert!(
        !app.world().resource::<pause::PauseMenu>().open,
        "Escape also opened pause"
    );
    assert!(
        app.world().resource::<Session>().captured,
        "closing restores cursor capture"
    );
    assert!(
        matches!(next_message(&mut peer), ClientMessage::Input { input, .. }
        if input.direction == [0.0; 2] && !input.jump)
    );
    assert_eq!(app.world().resource::<Session>().target, None);
}

#[test]
fn a_short_remote_teleport_snaps_the_avatar_instead_of_smoothing_through_space() {
    let (world, mut session) = join::session_from_welcome(
        join::tests::welcome(SessionMode::Player),
        "test peer".into(),
        GraphicsQuality::Low,
        0.0,
        SessionMode::Player,
    )
    .unwrap();
    let mut remote = session.players[0].clone();
    remote.id = 2;
    remote.body = Body::new([5.0, 20.0, 0.25]);
    session.players.push(remote);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(10));
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(VoxelWorld(world))
        .insert_resource(time)
        .init_resource::<Avatars>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_systems(Update, update_avatars);
    app.update();
    let entity = app.world().resource::<Avatars>().players[&2];
    let target = [6.0, 20.0, 0.25];
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        let remote = &mut session.players[1];
        remote.body.position = target;
        remote.movement_epoch = 1;
    }
    app.update();
    assert_eq!(
        app.world()
            .get::<Transform>(entity)
            .unwrap()
            .translation
            .to_array(),
        target
    );
}
