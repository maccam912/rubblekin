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
    if let ServerMessage::Welcome { players, .. } = &mut welcome {
        players.push(PlayerSnapshot {
            id: 31,
            name: "Another explorer".into(),
            body: Body::new([1.0, 3.0, 1.0]),
            yaw: 0.0,
            last_input_sequence: 0,
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
