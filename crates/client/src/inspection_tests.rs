//! Opening inspection must pick from the camera updated by this frame's input.
use super::*;
use bevy::gizmos::{AppGizmoBuilder, config::DefaultGizmoConfigGroup};
use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    time::Duration,
};

#[test]
fn opening_inspection_uses_the_new_view_and_keeps_its_target_until_reopened() {
    // A real connection satisfies controls without replacing any gameplay
    // system. Observer frames need no prediction or server acknowledgments.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let client = std::thread::spawn(move || {
        Connection::connect(&address, "Inspector".into(), SessionMode::Observer).unwrap()
    });
    let (socket, _) = listener.accept().unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut peer = BufReader::new(socket);
    peer.read_line(&mut String::new()).unwrap();
    serde_json::to_writer(peer.get_mut(), &join::tests::welcome(SessionMode::Observer)).unwrap();
    peer.get_mut().write_all(b"\n").unwrap();
    let (connection, welcome) = client.join().unwrap();
    let (mut world, mut session) = join::session_from_welcome(
        welcome,
        "test peer".into(),
        GraphicsQuality::default(),
        0.0,
        SessionMode::Observer,
    )
    .unwrap();
    let forward = BlockPos::new(0, 154, -8);
    let right = BlockPos::new(8, 154, 0);
    world.set_block(forward, Block::Brick).unwrap();
    world.set_block(right, Block::Wood).unwrap();
    session.observer.as_mut().unwrap().position = Vec3::new(0.25, 77.25, 0.25);
    session.yaw = 0.0;
    session.pitch = 0.0;
    session.captured = true;
    assert!(session.inspector && session.inspect_requested);
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_millis(16));
    let mut app = App::new();
    app.insert_resource(VoxelWorld(world))
        .insert_resource(session)
        .insert_resource(connection)
        .insert_resource(time)
        .insert_resource(GraphicsSettings::new(GraphicsQuality::default()))
        .init_resource::<pause::PauseMenu>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<AccumulatedMouseScroll>()
        .init_resource::<DiagnosticsStore>()
        .init_resource::<touch::TouchControls>()
        .init_resource::<Avatars>()
        .init_gizmo_group::<DefaultGizmoConfigGroup>()
        .add_systems(Update, (controls, camera, inspection::update).chain());
    app.world_mut()
        .spawn((Window::default(), CursorOptions::default()));
    let camera = app
        .world_mut()
        .spawn((GameCamera, CameraFollow::default(), Transform::default()))
        .id();
    let selected = |app: &App| app.world().resource::<Session>().inspected;
    let frame = |app: &mut App, tab: bool, motion: Vec2| {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        if tab {
            keys.press(KeyCode::Tab);
        }
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = motion;
        app.world_mut().run_schedule(Update);
    };
    let quarter_turn = std::f32::consts::FRAC_PI_2 / 0.0025;

    frame(&mut app, false, Vec2::ZERO);
    assert_eq!(
        selected(&app),
        Some(inspection::InspectTarget::Block(forward))
    );
    assert!(!app.world().resource::<Session>().inspect_requested);

    // Looking elsewhere while reading never changes the locked selection.
    frame(&mut app, false, Vec2::new(quarter_turn, 0.0));
    assert!(app.world().get::<Transform>(camera).unwrap().forward().x > 0.99);
    assert_eq!(
        selected(&app),
        Some(inspection::InspectTarget::Block(forward))
    );
    frame(&mut app, true, Vec2::ZERO);
    assert!(!app.world().resource::<Session>().inspector);
    assert_eq!(
        selected(&app),
        Some(inspection::InspectTarget::Block(forward))
    );
    frame(&mut app, false, Vec2::new(-quarter_turn, 0.0));

    // Tab and look arrive together: picking before camera would hit `forward`.
    frame(&mut app, true, Vec2::new(quarter_turn, 0.0));
    assert!(app.world().resource::<Session>().inspector);
    assert_eq!(
        selected(&app),
        Some(inspection::InspectTarget::Block(right))
    );
    frame(&mut app, true, Vec2::new(0.0, -400.0));
    assert_eq!(
        selected(&app),
        Some(inspection::InspectTarget::Block(right))
    );
    frame(&mut app, true, Vec2::ZERO);
    assert!(app.world().resource::<Session>().inspector);
    assert_eq!(selected(&app), None, "sky must clear the previous target");

    // The touch button uses the same request and the same-frame swipe pose.
    frame(&mut app, true, Vec2::ZERO);
    {
        let mut touch = app.world_mut().resource_mut::<touch::TouchControls>();
        touch.enabled = true;
        touch.inspect = true;
        touch.look = Vec2::new(-quarter_turn, 400.0);
    }
    frame(&mut app, false, Vec2::ZERO);
    assert!(app.world().resource::<Session>().inspector);
    assert_eq!(
        selected(&app),
        Some(inspection::InspectTarget::Block(forward))
    );
    app.world_mut()
        .resource_mut::<touch::TouchControls>()
        .inspect = false;

    // A pause consumes Tab and look without reopening or retargeting.
    {
        let mut pause = app.world_mut().resource_mut::<pause::PauseMenu>();
        pause.open = true;
        pause.input_blocked = true;
    }
    let before_pause = *app.world().get::<Transform>(camera).unwrap();
    frame(&mut app, true, Vec2::new(quarter_turn, 0.0));
    assert!(app.world().resource::<Session>().inspector);
    assert_eq!(
        selected(&app),
        Some(inspection::InspectTarget::Block(forward))
    );
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before_pause);
}
