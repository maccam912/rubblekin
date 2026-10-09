//! Exercise the actual observer systems against a peer that records every write.
use super::*;
use bevy::{
    diagnostic::DiagnosticsStore,
    gizmos::{AppGizmoBuilder, config::DefaultGizmoConfigGroup},
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    light::DirectionalLightShadowMap,
};
use rubblekin_core::protocol::{ClientMessage, PlayerSnapshot};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    time::Duration,
};

#[test]
fn observer_controls_move_only_the_camera_and_never_send_gameplay_messages() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let client = std::thread::spawn(move || {
        Connection::connect(&address, "Observer".into(), SessionMode::Observer).unwrap()
    });
    let (socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut peer = BufReader::new(socket);
    let mut line = String::new();
    peer.read_line(&mut line).unwrap();
    assert!(matches!(
        serde_json::from_str::<ClientMessage>(&line).unwrap(),
        ClientMessage::Hello {
            mode: SessionMode::Observer,
            ..
        }
    ));
    let mut welcome = tests::welcome(SessionMode::Observer);
    if let ServerMessage::Welcome {
        players,
        generation,
        ..
    } = &mut welcome
    {
        // Include real villages so the paused V shortcut would have a valid
        // destination if it accidentally escaped the input gate.
        *generation = rubblekin_core::world::WorldGeneration::GeographyV3;
        players.push(PlayerSnapshot {
            glider_ride: None,
            gliding: false,
            id: 31,
            name: "Another explorer".into(),
            body: Body::new([1.0, 3.0, 1.0]),
            yaw: 0.0,
            last_input_sequence: 0,
            movement_epoch: 0,
            ride: None,
            deck_position: None,
        });
    }
    serde_json::to_writer(peer.get_mut(), &welcome).unwrap();
    peer.get_mut().write_all(b"\n").unwrap();
    let (connection, welcome) = client.join().unwrap();
    let (world, mut session) = session_from_welcome(
        welcome,
        "test peer".into(),
        GraphicsQuality::default(),
        0.0,
        SessionMode::Observer,
    )
    .unwrap();
    session.captured = true;
    let body_before = session.body.clone();
    let camera_before = session.observer.as_ref().unwrap().position;
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(100));
    let mut app = App::new();
    app.insert_resource(VoxelWorld(world))
        .insert_resource(session)
        .insert_resource(connection)
        .insert_resource(time)
        .insert_resource(crate::graphics::GraphicsSettings::new(
            GraphicsQuality::default(),
        ))
        .init_resource::<crate::pause::PauseMenu>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<AccumulatedMouseScroll>()
        .init_resource::<DirectionalLightShadowMap>()
        .init_resource::<DiagnosticsStore>()
        .init_resource::<Avatars>()
        .init_resource::<crate::touch::TouchControls>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_gizmo_group::<DefaultGizmoConfigGroup>()
        .add_systems(
            Update,
            (
                crate::controls,
                crate::camera,
                crate::edit_blocks,
                crate::update_avatars,
            )
                .chain(),
        );
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..default()
            },
            CursorOptions::default(),
        ))
        .id();
    let camera = app
        .world_mut()
        .spawn((
            crate::GameCamera,
            crate::follow_camera::CameraFollow::default(),
            Transform::default(),
        ))
        .id();
    for key in [
        KeyCode::KeyW,
        KeyCode::KeyE,
        KeyCode::ShiftLeft,
        KeyCode::ControlLeft,
        KeyCode::KeyF,
        KeyCode::F6,
        KeyCode::F9,
        KeyCode::BracketLeft,
    ] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }
    for button in [MouseButton::Left, MouseButton::Right] {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(button);
    }
    // Enough simulated time to fail the prediction backlog if this camera ever
    // takes the ordinary player-input path without server acknowledgments.
    for _ in 0..30 {
        app.world_mut().run_schedule(Update);
    }
    let session = app.world().resource::<Session>();
    let moved = session.observer.as_ref().unwrap().position;
    assert!(moved.distance(camera_before) > 100.0);
    assert_eq!(session.body.position, body_before.position);
    assert_eq!(session.body.velocity, body_before.velocity);
    assert_eq!(session.body.on_ground, body_before.on_ground);
    assert!(!session.flying);
    assert!(session.target.is_none());
    assert_eq!(session.next_request, 1);
    assert!(app.world().resource::<Connection>().error.is_none());
    assert_eq!(
        app.world().get::<Transform>(camera).unwrap().translation,
        moved
    );
    let avatars = app.world().resource::<Avatars>();
    assert_eq!(avatars.players.len(), 1);
    assert!(avatars.players.contains_key(&31));
    assert!(!avatars.players.contains_key(&session.id));
    assert!(avatars.npc.is_some());
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<crate::Avatar>>()
            .iter(app.world())
            .count(),
        2
    );

    // A pause stops held motion and all keyboard gameplay shortcuts while the
    // ordinary frame systems keep updating. Reset and village visit are tested
    // separately so one accidentally allowed shortcut cannot conceal another.
    let before_pause = app.world().resource::<Session>();
    let paused_position = before_pause.observer.as_ref().unwrap().position;
    let paused_speed = before_pause.observer.as_ref().unwrap().speed;
    let paused_yaw = before_pause.yaw;
    let paused_pitch = before_pause.pitch;
    let paused_selection = before_pause.selected;
    let paused_inspector = before_pause.inspector;
    let paused_help = before_pause.help;
    {
        let mut pause = app.world_mut().resource_mut::<crate::pause::PauseMenu>();
        pause.open = true;
        pause.input_blocked = true;
    }
    for reset_key in [KeyCode::Home, KeyCode::KeyR, KeyCode::KeyV] {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        for key in [
            KeyCode::KeyW,
            KeyCode::KeyE,
            KeyCode::ShiftLeft,
            KeyCode::ArrowRight,
            KeyCode::ArrowDown,
            KeyCode::KeyF,
            KeyCode::Tab,
            KeyCode::KeyH,
            KeyCode::Digit6,
            KeyCode::F6,
            KeyCode::F9,
            KeyCode::BracketLeft,
            reset_key,
        ] {
            keys.press(key);
        }
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(80.0, 40.0);
        app.world_mut()
            .resource_mut::<AccumulatedMouseScroll>()
            .delta = Vec2::new(0.0, 3.0);
        app.world_mut().run_schedule(Update);
        let session = app.world().resource::<Session>();
        let observer = session.observer.as_ref().unwrap();
        assert_eq!(observer.position, paused_position, "{reset_key:?}");
        assert_eq!(observer.speed, paused_speed, "{reset_key:?}");
        assert_eq!(session.yaw, paused_yaw);
        assert_eq!(session.pitch, paused_pitch);
        assert_eq!(session.selected, paused_selection);
        assert_eq!(session.inspector, paused_inspector);
        assert_eq!(session.help, paused_help);
        assert!(!session.captured);
        assert!(!session.flying);
        assert!(session.target.is_none());
        assert_eq!(session.next_request, 1);
        let cursor = app.world().get::<CursorOptions>(window).unwrap();
        assert!(cursor.visible);
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    }
    {
        let mut pause = app.world_mut().resource_mut::<crate::pause::PauseMenu>();
        pause.open = false;
        pause.input_blocked = false;
    }
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::ZERO;
    app.world_mut()
        .resource_mut::<AccumulatedMouseScroll>()
        .delta = Vec2::ZERO;

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .reset_all();
    app.world_mut().run_schedule(Update);
    assert_eq!(
        app.world()
            .resource::<Session>()
            .observer
            .as_ref()
            .unwrap()
            .position,
        moved
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.world_mut().run_schedule(Update);
    let session = app.world().resource::<Session>();
    assert_eq!(session.observer.as_ref().unwrap().position, moved);
    assert!(!session.captured);
    assert!(app.world().get::<CursorOptions>(window).unwrap().visible);

    // Both shortcuts restore the view and chosen speed after flying away.
    // R is reachable on keyboards without a dedicated Home key.
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    for reset_key in [KeyCode::Home, KeyCode::KeyR] {
        app.world_mut().resource_mut::<Session>().captured = true;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(80.0, 40.0);
        app.world_mut()
            .resource_mut::<AccumulatedMouseScroll>()
            .delta = Vec2::new(0.0, 3.0);
        app.world_mut().run_schedule(Update);
        let session = app.world().resource::<Session>();
        let observer = session.observer.as_ref().unwrap();
        assert_ne!(observer.position, camera_before);
        assert!(observer.speed > 12.0);
        assert_ne!(session.yaw, -0.45);
        assert_ne!(session.pitch, 0.12);

        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::ZERO;
        app.world_mut()
            .resource_mut::<AccumulatedMouseScroll>()
            .delta = Vec2::ZERO;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(reset_key);
        app.world_mut().run_schedule(Update);
        let session = app.world().resource::<Session>();
        let observer = session.observer.as_ref().unwrap();
        assert_eq!(observer.position, camera_before, "{reset_key:?}");
        assert_eq!(observer.speed, 12.0, "{reset_key:?}");
        assert_eq!(session.yaw, -0.45, "{reset_key:?}");
        assert_eq!(session.pitch, 0.12, "{reset_key:?}");
        assert_eq!(session.body.position, body_before.position);
        assert_eq!(
            app.world().get::<Transform>(camera).unwrap().translation,
            camera_before
        );
    }

    // Ping is an ordered marker after all frame work: anything before it would
    // expose an accidental Input, Edit, or Admin write from the real systems.
    app.world_mut()
        .resource_mut::<Connection>()
        .send(ClientMessage::Ping);
    line.clear();
    peer.read_line(&mut line).unwrap();
    assert!(
        matches!(
            serde_json::from_str::<ClientMessage>(&line).unwrap(),
            ClientMessage::Ping
        ),
        "Observer sent unexpected gameplay input: {line}"
    );
}

#[test]
fn paused_player_sends_neutral_input_and_suppresses_edits_and_admin_commands() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let client = std::thread::spawn(move || {
        Connection::connect(&address, "Paused admin".into(), SessionMode::Player).unwrap()
    });
    let (socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut peer = BufReader::new(socket);
    let mut line = String::new();
    peer.read_line(&mut line).unwrap();
    assert!(matches!(
        serde_json::from_str::<ClientMessage>(&line).unwrap(),
        ClientMessage::Hello {
            mode: SessionMode::Player,
            ..
        }
    ));
    let mut welcome = tests::welcome(SessionMode::Player);
    if let ServerMessage::Welcome { can_admin, .. } = &mut welcome {
        *can_admin = true;
    }
    serde_json::to_writer(peer.get_mut(), &welcome).unwrap();
    peer.get_mut().write_all(b"\n").unwrap();
    let (connection, welcome) = client.join().unwrap();
    let (world, mut session) = session_from_welcome(
        welcome,
        "test peer".into(),
        GraphicsQuality::default(),
        0.0,
        SessionMode::Player,
    )
    .unwrap();
    session.captured = true;
    session.inspector = false;
    session.help = false;
    let position_before = session.body.position;
    let yaw_before = session.yaw;
    let pitch_before = session.pitch;
    let zoom_before = session.camera_distance;
    let selection_before = session.selected;
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(100));
    let mut app = App::new();
    app.insert_resource(VoxelWorld(world))
        .insert_resource(session)
        .insert_resource(connection)
        .insert_resource(time)
        .insert_resource(crate::graphics::GraphicsSettings::new(
            GraphicsQuality::default(),
        ))
        .init_resource::<crate::pause::PauseMenu>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<AccumulatedMouseScroll>()
        .init_resource::<DiagnosticsStore>()
        .init_resource::<crate::touch::TouchControls>()
        .init_gizmo_group::<DefaultGizmoConfigGroup>()
        .add_systems(Update, (crate::controls, crate::edit_blocks).chain());
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..default()
            },
            CursorOptions::default(),
        ))
        .id();
    app.world_mut().spawn((
        crate::GameCamera,
        Transform::from_xyz(0.25, 6.0, 0.25).looking_to(Vec3::NEG_Y, Vec3::Z),
    ));
    {
        let mut pause = app.world_mut().resource_mut::<crate::pause::PauseMenu>();
        pause.open = true;
        pause.input_blocked = true;
    }
    for button in [MouseButton::Left, MouseButton::Right] {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(button);
    }
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::new(80.0, 40.0);
    app.world_mut()
        .resource_mut::<AccumulatedMouseScroll>()
        .delta = Vec2::new(0.0, 3.0);
    for admin_key in [
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::BracketLeft,
        KeyCode::BracketRight,
    ] {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        for key in [
            KeyCode::KeyW,
            KeyCode::Space,
            KeyCode::KeyE,
            KeyCode::ShiftLeft,
            KeyCode::ControlLeft,
            KeyCode::ArrowRight,
            KeyCode::ArrowDown,
            KeyCode::KeyF,
            KeyCode::Digit6,
            KeyCode::Tab,
            KeyCode::KeyH,
            admin_key,
        ] {
            keys.press(key);
        }
        app.world_mut().run_schedule(Update);
        line.clear();
        peer.read_line(&mut line).unwrap();
        assert!(
            matches!(
                serde_json::from_str::<ClientMessage>(&line).unwrap(),
                ClientMessage::Input { input, .. }
                    if input.direction == [0.0; 2]
                        && input.vertical == 0.0
                        && !input.jump && !input.sprint && !input.fly
            ),
            "Paused player sent gameplay input: {line}"
        );
        let session = app.world().resource::<Session>();
        assert_eq!(session.body.position[0], position_before[0]);
        assert_eq!(session.body.position[2], position_before[2]);
        assert_eq!(session.yaw, yaw_before);
        assert_eq!(session.pitch, pitch_before);
        assert_eq!(session.camera_distance, zoom_before);
        assert_eq!(session.selected, selection_before);
        assert!(!session.flying);
        assert!(!session.inspector);
        assert!(!session.help);
        assert!(!session.captured);
        assert_eq!(session.next_request, 1);
    }

    // Closing the menu can restore capture, but the close-frame click and held
    // keys must still produce neutral input and no terrain or admin command.
    app.world_mut()
        .resource_mut::<crate::pause::PauseMenu>()
        .open = false;
    app.world_mut()
        .resource_mut::<crate::pause::PauseMenu>()
        .just_closed = true;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    for key in [
        KeyCode::KeyW,
        KeyCode::KeyF,
        KeyCode::F6,
        KeyCode::ControlLeft,
    ] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }
    app.world_mut().run_schedule(Update);
    line.clear();
    peer.read_line(&mut line).unwrap();
    assert!(matches!(
        serde_json::from_str::<ClientMessage>(&line).unwrap(),
        ClientMessage::Input { input, .. } if input.direction == [0.0; 2] && !input.fly
    ));
    let session = app.world().resource::<Session>();
    assert!(session.captured);
    assert!(!session.flying);
    assert_eq!(session.yaw, yaw_before);
    assert_eq!(session.pitch, pitch_before);
    assert_eq!(session.next_request, 1);
    assert_eq!(
        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
        CursorGrabMode::Locked
    );

    // Any leaked Edit or Admin packet appears ahead of this ordered marker.
    app.world_mut()
        .resource_mut::<Connection>()
        .send(ClientMessage::Ping);
    line.clear();
    peer.read_line(&mut line).unwrap();
    assert!(
        matches!(
            serde_json::from_str::<ClientMessage>(&line).unwrap(),
            ClientMessage::Ping
        ),
        "Paused player sent an unexpected command: {line}"
    );
}
