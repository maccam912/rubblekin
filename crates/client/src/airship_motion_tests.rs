//! Packet delivery must not turn continuous ship motion into snapshot steps.
use super::*;
use bevy::ecs::schedule::ScheduleLabel;
use rubblekin_core::{
    airships::{AirshipSnapshot, pilot_position},
    protocol::SessionMode,
    world::WorldGeneration,
};
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    time::{Duration, Instant},
};

#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum MotionSchedule {
    Receive,
    Frame,
}

fn connection() -> (Connection, ServerMessage, BufReader<TcpStream>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let client = std::thread::spawn(move || {
        Connection::connect(&address, "Passenger".into(), SessionMode::Player).unwrap()
    });
    let (socket, _) = listener.accept().unwrap();
    socket.set_nodelay(true).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut peer = BufReader::new(socket);
    peer.read_line(&mut String::new()).unwrap();
    let mut welcome = join::tests::welcome(SessionMode::Player);
    if let ServerMessage::Welcome { generation, .. } = &mut welcome {
        *generation = WorldGeneration::GeographyV3;
    }
    serde_json::to_writer(peer.get_mut(), &welcome).unwrap();
    peer.get_mut().write_all(b"\n").unwrap();
    let (connection, welcome) = client.join().unwrap();
    (connection, welcome, peer)
}

fn flight_segment(session: &Session, rising: bool) -> (AirshipSnapshot, f64, f32) {
    let id = session.airships.ships(0.0)[0].id;
    // Stay inside one motion stage, so stage changes cannot mask packet jitter.
    for second in 0..2_000 {
        let time = second as f64;
        let ship = session.airships.ship(id, time).unwrap();
        let next = session.airships.ship(id, time + 4.0).unwrap();
        let delta = Vec3::from_array(next.position) - Vec3::from_array(ship.position);
        if ship.docked_at.is_none()
            && next.docked_at.is_none()
            && if rising {
                delta.y > 40.0 && delta.xz().length() < 0.001
            } else {
                delta.y.abs() < 0.001 && delta.xz().length() > 120.0
            }
        {
            return (ship, time, delta.length() / 4.0);
        }
    }
    panic!("fixture has no sufficiently long flight segment");
}

#[allow(clippy::too_many_arguments)]
fn setup_receive_terrain(
    mut commands: Commands,
    world: Res<VoxelWorld>,
    session: Res<Session>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut terrain_materials: ResMut<Assets<terrain_material::TerrainMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let terrain = terrain::setup_terrain(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut terrain_materials,
        &mut images,
        &world.0,
        session.body.position,
        0,
        graphics::MIN_TREE_DISTANCE,
        2048,
    );
    commands.insert_resource(terrain);
}

fn assert_pose(app: &App, ship_entity: Entity, pilot_head: Entity, ship_id: u64) -> Vec3 {
    let session = app.world().resource::<Session>();
    let ship = session
        .airships
        .ship(ship_id, session.airship_clock.time)
        .unwrap();
    let pose = app.world().get::<Transform>(ship_entity).unwrap();
    assert_eq!(pose.translation, Vec3::from_array(ship.position));
    assert!(pose.rotation.angle_between(Quat::from_rotation_y(ship.yaw)) < 0.0001);
    let head = app.world().get::<Transform>(pilot_head).unwrap();
    assert!(
        pose.transform_point(head.translation)
            .distance(Vec3::from_array(pilot_position(&ship)) + Vec3::Y * 1.5)
            < 0.002
    );
    let avatars = app.world().resource::<Avatars>();
    for player in &session.players {
        let local = player.deck_position.unwrap();
        let desired = Vec3::from_array(deck_position(&ship, local));
        let avatar = app
            .world()
            .get::<Transform>(avatars.players[&player.id])
            .unwrap();
        assert_eq!(avatar.translation, desired);
        if player.id == session.id {
            assert_eq!(session.body.position, desired.to_array());
            assert_eq!(session.deck_position, Some(local));
        }
    }
    pose.translation
}

fn check_packet_motion(rising: bool) {
    let (connection, welcome, mut peer) = connection();
    let (world, mut session) = join::session_from_welcome(
        welcome,
        "test peer".into(),
        GraphicsQuality::Low,
        0.0,
        SessionMode::Player,
    )
    .unwrap();
    session.airships = AirshipNetwork::new(&world);
    let (ship, start, speed) = flight_segment(&session, rising);
    let ride = AirshipRide {
        ship_id: ship.id,
        seat: u8::MAX,
    };
    let local = [-2.0, 0.0, 2.0];
    session.ride = Some(ride);
    session.deck_position = Some(local);
    session.body = Body::new(deck_position(&ship, local));
    session.body.on_ground = true;
    session.world_time = start;
    session.airship_clock = airships::AirshipClock::new(start, 0.0);
    session.players[0].ride = Some(ride);
    session.players[0].deck_position = Some(local);
    session.players[0].body = session.body.clone();
    session.players.push(PlayerSnapshot {
        glider_ride: None,
        gliding: false,
        id: 31,
        name: "Remote passenger".into(),
        body: Body::new(deck_position(&ship, [2.0, 0.0, 2.0])),
        yaw: 0.0,
        last_input_sequence: 0,
        movement_epoch: 0,
        ride: Some(ride),
        deck_position: Some([2.0, 0.0, 2.0]),
    });
    let mut app = App::new();
    app.insert_resource(VoxelWorld(world))
        .insert_resource(session)
        .insert_resource(connection)
        .init_resource::<Time>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<terrain_material::TerrainMaterial>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<Font>>()
        .init_resource::<airships::PilotConversation>()
        .init_resource::<Avatars>()
        .add_systems(Startup, (setup_receive_terrain, airships::setup).chain())
        .add_systems(MotionSchedule::Receive, receive_network)
        .add_systems(
            MotionSchedule::Frame,
            (
                airships::advance_clock,
                update_avatars,
                airships::update_scene,
            )
                .chain(),
        );
    app.update();
    app.world_mut().run_schedule(MotionSchedule::Frame);
    let ship_entity = {
        let world = app.world_mut();
        let mut query = world.query_filtered::<(Entity, &Transform), With<airships::Ship>>();
        query
            .iter(world)
            .find(|(_, pose)| pose.translation == Vec3::from_array(ship.position))
            .unwrap()
            .0
    };
    let pilot_head = app
        .world()
        .get::<Children>(ship_entity)
        .unwrap()
        .iter()
        .find(|child| {
            app.world()
                .get::<Transform>(*child)
                .unwrap()
                .scale
                .distance(Vec3::splat(0.38))
                < 0.0001
        })
        .unwrap();
    let mut previous = assert_pose(&app, ship_entity, pilot_head, ship.id);
    let mut sent_tick = 0;
    let mut coalesced = false;
    for frame in 0..100 {
        let millis = [11, 19, 13, 24, 16, 9, 22, 17][frame % 8];
        let dt = Duration::from_millis(millis);
        app.world_mut().resource_mut::<Time>().advance_by(dt);
        let elapsed = app.world().resource::<Time>().elapsed_secs_f64();
        if frame % 5 == 1 {
            let delay = [0.02, 0.10, 0.0, 0.08][(frame / 5) % 4];
            let latest_tick = ((elapsed - delay).max(0.0) / 0.05).floor() as usize;
            if latest_tick > sent_tick {
                coalesced |= latest_tick - sent_tick > 1;
                let mut batch = Vec::new();
                for tick in sent_tick + 1..=latest_tick {
                    let world_time = start + tick as f64 * 0.05;
                    let session = app.world().resource::<Session>();
                    let mut players = session.players.clone();
                    let ship = session.airships.ship(ship.id, world_time).unwrap();
                    for player in &mut players {
                        player.body.position = deck_position(&ship, player.deck_position.unwrap());
                    }
                    serde_json::to_writer(
                        &mut batch,
                        &ServerMessage::State {
                            gliders: Vec::new(),
                            players,
                            npc: session.npc.clone(),
                            residents: Vec::new(),
                            villages: Vec::new(),
                            world_time,
                        },
                    )
                    .unwrap();
                    batch.push(b'\n');
                }
                peer.get_mut().write_all(&batch).unwrap();
                let before = app.world().resource::<Session>().airship_clock.time;
                let newest = start + latest_tick as f64 * 0.05;
                let deadline = Instant::now() + Duration::from_secs(2);
                loop {
                    app.world_mut().run_schedule(MotionSchedule::Receive);
                    let session = app.world().resource::<Session>();
                    assert_eq!(session.airship_clock.time, before, "packet moved the clock");
                    if session.world_time == newest {
                        break;
                    }
                    assert!(Instant::now() < deadline, "state did not reach the client");
                    std::thread::sleep(Duration::from_millis(1));
                }
                sent_tick = latest_tick;
            }
        }
        let previous_time = app.world().resource::<Session>().airship_clock.time;
        app.world_mut().run_schedule(MotionSchedule::Frame);
        assert!(app.world().resource::<Connection>().error.is_none());
        let advance = app.world().resource::<Session>().airship_clock.time - previous_time;
        assert!(advance > dt.as_secs_f64() * 0.65);
        assert!(advance <= dt.as_secs_f64() * 1.101);
        let position = assert_pose(&app, ship_entity, pilot_head, ship.id);
        let displacement = position - previous;
        if rising {
            assert!(displacement.y > 0.0, "rising ship reversed");
            assert!(displacement.xz().length() < 0.001);
        } else {
            assert!(displacement.y.abs() < 0.001);
        }
        assert!(displacement.length() > speed * dt.as_secs_f32() * 0.64);
        assert!(displacement.length() <= speed * dt.as_secs_f32() * 1.12 + 0.003);
        previous = position;
    }
    assert!(
        coalesced,
        "fixture never delivered multiple snapshots together"
    );
}

#[test]
fn delayed_and_coalesced_states_keep_rising_ship_and_passengers_smooth() {
    check_packet_motion(true);
}

#[test]
fn delayed_and_coalesced_states_keep_cruising_ship_and_passengers_smooth() {
    check_packet_motion(false);
}
