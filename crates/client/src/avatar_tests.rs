use super::*;
use rubblekin_core::gliders::{GliderDestination, GliderFlight, GliderRide, seat_position};
use rubblekin_core::physics::characters_overlap;
use std::time::Duration;

fn avatar_app() -> App {
    let (world, session) = join::session_from_welcome(
        join::tests::welcome(SessionMode::Player),
        "test".into(),
        GraphicsQuality::Low,
        0.0,
        SessionMode::Player,
    )
    .unwrap();
    let mut app = App::new();
    app.insert_resource(VoxelWorld(world))
        .insert_resource(session)
        .insert_resource(Time::<()>::default())
        .init_resource::<Avatars>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_systems(Update, update_avatars);
    app
}

fn frame(app: &mut App) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_millis(17));
    app.world_mut().run_schedule(Update);
}

fn vehicle_app() -> App {
    let mut app = avatar_app();
    app.init_resource::<Assets<Font>>()
        .init_resource::<pause::PauseMenu>()
        .init_resource::<world_map::WorldMap>()
        .init_resource::<admin_console::AdminConsole>()
        .init_resource::<airships::PilotConversation>()
        .init_resource::<market::MarketPanel>()
        .init_resource::<touch::TouchControls>()
        .add_systems(Startup, vehicles::setup)
        .add_systems(
            Update,
            (vehicles::update, vehicles::animate_parts)
                .chain()
                .after(update_avatars),
        );
    app.world_mut().run_schedule(Startup);
    app
}

fn player_limbs(app: &mut App, id: u64) -> Vec<(bool, Transform)> {
    let entity = app.world().resource::<Avatars>().players[&id];
    app.world_mut()
        .query::<(&Limb, &ChildOf, &Transform)>()
        .iter(app.world())
        .filter(|(_, parent, _)| parent.parent() == entity)
        .map(|(limb, _, transform)| (limb.arm, *transform))
        .collect()
}

#[test]
fn trader_carriage_avatar_uses_the_continuous_seat_and_seated_limbs() {
    let mut app = avatar_app();
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        session.gliders.push(GliderFlight {
            emergency: false,
            id: 92,
            station_id: 0,
            destination: GliderDestination::Village(1),
            destination_name: "Town".into(),
            from: [10., 50., 20.],
            to: [500., 50., 400.],
            apex: 600.,
            duration: 30.,
            created_at: 0.,
            started_at: Some(0.),
        });
        session
            .residents
            .push(rubblekin_core::protocol::ResidentSnapshot {
                id: 41,
                village_id: 0,
                name: "Trader".into(),
                position: [10., 50., 20.],
                role: rubblekin_core::protocol::ResidentRole::Trader,
                action: rubblekin_core::protocol::ResidentAction::Delivering,
                target: Some([500., 50., 400.]),
                carrying: Some(rubblekin_core::protocol::ResourceCargo {
                    kind: rubblekin_core::settlement::ResourceKind::Timber,
                    amount: 6.,
                }),
                hunger: 25.,
                energy: 85.,
                reason: "Flying to trade".into(),
                ride: None,
                deck_position: None,
                glider_ride: Some(GliderRide {
                    carriage_id: 92,
                    seat: 2,
                }),
            });
    }
    frame(&mut app);
    for time in [0., 8.5, 15., 23., 30.] {
        app.world_mut().resource_mut::<Session>().airship_clock.time = time;
        frame(&mut app);
        let entity = app.world().resource::<Avatars>().residents[&41];
        let pose = app.world().resource::<Session>().gliders[0].pose(time);
        let root = app.world().get::<Transform>(entity).unwrap();
        assert!(
            root.translation
                .distance(Vec3::from_array(seat_position(pose, 2)) - Vec3::Y * 0.30)
                < 0.0001
        );
        for (limb, parent, transform) in app
            .world_mut()
            .query::<(&Limb, &ChildOf, &Transform)>()
            .iter(app.world())
        {
            if parent.parent() == entity && !limb.arm {
                let hip = transform.transform_point(Vec3::Y * 0.5);
                assert!((hip.y - 0.70).abs() < 0.0001);
            }
        }
    }
    app.world_mut().resource_mut::<Session>().residents[0].glider_ride = None;
    frame(&mut app);
    let entity = app.world().resource::<Avatars>().residents[&41];
    for (limb, parent, transform) in app
        .world_mut()
        .query::<(&Limb, &ChildOf, &Transform)>()
        .iter(app.world())
    {
        if parent.parent() == entity && !limb.arm {
            assert_eq!(transform.scale.y, 0.66);
        }
    }
}

#[test]
fn carriage_passengers_follow_continuous_seats_without_walking_between_packets() {
    let mut app = avatar_app();
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        session.gliders.push(GliderFlight {
            emergency: false,
            id: 5,
            station_id: 0,
            destination: GliderDestination::Village(1),
            destination_name: "Town".into(),
            from: [10.0, 50.0, 20.0],
            to: [500.0, 50.0, 400.0],
            apex: 600.0,
            duration: 30.0,
            created_at: 0.0,
            started_at: Some(0.0),
        });
        let ride = GliderRide {
            carriage_id: 5,
            seat: 0,
        };
        session.glider_ride = Some(ride);
        session.players[0].glider_ride = Some(ride);
        session.body.on_ground = true;
        let mut remote = session.players[0].clone();
        remote.id = 31;
        remote.glider_ride.as_mut().unwrap().seat = 3;
        remote.body.on_ground = true;
        session.players.push(remote);
    }
    frame(&mut app);
    for time in [0.0, 2.0, 8.2, 12.5, 19.0, 28.0, 30.0] {
        app.world_mut().resource_mut::<Session>().airship_clock.time = time;
        frame(&mut app);
        let session = app.world().resource::<Session>();
        let pose = session.gliders[0].pose(time);
        let avatars = app.world().resource::<Avatars>();
        for (id, seat) in [(17, 0), (31, 3)] {
            let entity = avatars.players[&id];
            let actual = app.world().get::<Transform>(entity).unwrap();
            let expected = Vec3::from_array(seat_position(pose, seat)) - Vec3::Y * 0.30;
            assert!(actual.translation.distance(expected) < 0.0001);
            let forward = actual.rotation * Vec3::NEG_Z;
            let craft_forward = Quat::from_rotation_y(pose.yaw) * Vec3::Z;
            assert!(forward.dot(craft_forward) > 0.9999);
        }
        for id in [17, 31] {
            for (arm, limb) in player_limbs(&mut app, id) {
                if arm {
                    assert!(limb.rotation.dot(Quat::from_rotation_x(0.45)).abs() > 0.99999);
                } else {
                    let hip = limb.transform_point(Vec3::Y * 0.5);
                    assert!(
                        hip.distance(Vec3::new(hip.x, 0.70, 0.)) < 0.0001,
                        "carriage hip: {hip:?}"
                    );
                    // Deck top is -0.045 relative to the carriage; the rider
                    // root is lowered 0.30 from its physical seat.
                    let toe_bottom = [-0.5, 0.5]
                        .map(|z| limb.transform_point(Vec3::new(0., -0.5, z)).y - 0.30)
                        .into_iter()
                        .fold(f32::INFINITY, f32::min);
                    assert!(
                        (-0.045..=-0.005).contains(&toe_bottom),
                        "seated feet must clear and rest near the deck: {toe_bottom}"
                    );
                }
            }
        }
    }
    let local = app.world().resource::<Avatars>().players[&17];
    assert!(
        app.world_mut()
            .query::<(&GroundShadow, &ChildOf, &Visibility)>()
            .iter(app.world())
            .filter(|(_, parent, _)| parent.parent() == local)
            .all(|(_, _, visibility)| *visibility == Visibility::Hidden)
    );
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        session.glider_ride = None;
        session.players[0].glider_ride = None;
        session.body.position = [0.25, 2.52, 0.25];
    }
    frame(&mut app);
    for (arm, limb) in player_limbs(&mut app, 17) {
        assert_eq!(limb.translation.y, if arm { 1.0 } else { 0.38 });
        assert_eq!(limb.translation.z, 0.0);
        assert!(limb.rotation.angle_between(Quat::from_rotation_x(1.2)) > 0.5);
    }
    frame(&mut app);
    assert!(
        player_limbs(&mut app, 17)
            .iter()
            .all(|(_, limb)| limb.rotation == Quat::IDENTITY)
    );
    app.world_mut().resource_mut::<Session>().body.position[0] += 0.1;
    frame(&mut app);
    assert!(
        player_limbs(&mut app, 17)
            .iter()
            .any(|(_, limb)| limb.rotation != Quat::IDENTITY)
    );
}

#[test]
fn personal_gliding_holds_the_risers_and_landing_restores_idle_limbs() {
    let mut app = avatar_app();
    app.world_mut().resource_mut::<Session>().gliding = true;
    frame(&mut app);
    for _ in 0..4 {
        app.world_mut().resource_mut::<Session>().body.position[0] += 0.3;
        frame(&mut app);
        for (arm, limb) in player_limbs(&mut app, 17) {
            let angle = if arm { 2.6 } else { 0.12 };
            assert!(limb.rotation.dot(Quat::from_rotation_x(angle)).abs() > 0.99999);
        }
    }
    app.world_mut().resource_mut::<Session>().gliding = false;
    frame(&mut app);
    assert!(
        player_limbs(&mut app, 17)
            .iter()
            .all(|(_, limb)| limb.rotation == Quat::IDENTITY)
    );
}

#[test]
fn coasting_bikes_and_drifting_kayaks_rest_while_active_riders_animate() {
    use rubblekin_core::vehicles::{Vehicle, VehicleKind};
    for kind in [VehicleKind::Bike, VehicleKind::Kayak] {
        let mut app = avatar_app();
        app.world_mut().resource_mut::<Session>().vehicle = Some(Vehicle {
            tilt: 0.0,
            kind,
            heading: 0.,
        });
        frame(&mut app);
        frame(&mut app);
        let still = player_limbs(&mut app, 17);
        for _ in 0..5 {
            frame(&mut app);
        }
        assert_eq!(still, player_limbs(&mut app, 17));
        app.world_mut().resource_mut::<Session>().body.velocity = [3., 0., 0.];
        frame(&mut app);
        assert_eq!(
            still,
            player_limbs(&mut app, 17),
            "world motion alone must not pedal or paddle"
        );
        app.world_mut().resource_mut::<Session>().vehicle_effort = 1.;
        frame(&mut app);
        assert_ne!(still, player_limbs(&mut app, 17));
        let pedalling = player_limbs(&mut app, 17);
        app.world_mut().resource_mut::<Session>().vehicle_effort = 0.;
        frame(&mut app);
        assert_eq!(
            if kind == VehicleKind::Bike {
                pedalling
            } else {
                still
            },
            player_limbs(&mut app, 17)
        );
    }
}

#[test]
fn remote_vehicles_share_the_rendered_rider_pose_through_packets_teleports_and_dismount() {
    use rubblekin_core::vehicles::{Vehicle, VehicleKind};
    for kind in VehicleKind::ALL {
        let mut app = vehicle_app();
        {
            let mut session = app.world_mut().resource_mut::<Session>();
            let mut remote = session.players[0].clone();
            remote.id = 31;
            remote.body.position = [10., 8., 10.];
            remote.vehicle = Some(Vehicle {
                tilt: 0.0,
                kind,
                heading: 0.7,
            });
            session.players.push(remote);
        }
        frame(&mut app);
        frame(&mut app);
        for _ in 0..3 {
            app.world_mut().resource_mut::<Session>().players[1]
                .body
                .position[0] += 0.7;
            for _ in 0..4 {
                frame(&mut app);
                let entity = app.world().resource::<Avatars>().players[&31];
                let rider = *app.world().get::<Transform>(entity).unwrap();
                let mut query = app
                    .world_mut()
                    .query_filtered::<&Transform, With<vehicles::Craft>>();
                let craft = query.single(app.world()).unwrap();
                assert!(
                    craft
                        .translation
                        .distance(rider.translation - rider.rotation * vehicles::seat_offset(kind))
                        < 0.0001
                );
            }
        }
        {
            let mut session = app.world_mut().resource_mut::<Session>();
            session.players[1].movement_epoch += 1;
            session.players[1].body.position[2] += 0.5;
        }
        frame(&mut app);
        let target = Vec3::from_array(app.world().resource::<Session>().players[1].body.position);
        assert!(
            app.world_mut()
                .query_filtered::<&Transform, With<vehicles::Craft>>()
                .single(app.world())
                .unwrap()
                .translation
                .distance(target)
                < 0.0001
        );
        app.world_mut().resource_mut::<Session>().players[1].vehicle = None;
        frame(&mut app);
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<vehicles::Craft>>()
                .iter(app.world())
                .count(),
            0
        );
    }
}

#[test]
fn bike_wheels_roll_with_signed_travel_and_hold_through_stops_and_teleports() {
    use rubblekin_core::vehicles::{Vehicle, VehicleKind};
    for heading in [0., 0.7, std::f32::consts::PI] {
        let mut app = vehicle_app();
        let start = Vec3::new(10., 80., 10.);
        {
            let mut session = app.world_mut().resource_mut::<Session>();
            session.body.position = start.to_array();
            session.vehicle = Some(Vehicle {
                tilt: 0.0,
                kind: VehicleKind::Bike,
                heading,
            });
            session.vehicle_effort = 0.;
        }
        frame(&mut app);
        let rotation = vehicles::rotation(
            &app.world().resource::<VoxelWorld>().0,
            start.to_array(),
            app.world().resource::<Session>().vehicle.unwrap(),
        );
        let forward = rotation * Vec3::NEG_Z;
        let side = rotation * Vec3::X;
        for (offset, expected_angle, teleported) in [
            (forward * 0.30, -1., false),
            (forward * 0.30, -1., false),
            (forward * 0.30 + side * 0.40, -1., false),
            (side * 0.40, 0., false),
            (side * 0.40 + forward * 0.50, 0., true),
            (side * 0.40 + forward * 20., 0., false),
        ] {
            {
                let mut session = app.world_mut().resource_mut::<Session>();
                session.body.position = (start + offset).to_array();
                if teleported {
                    session.players[0].movement_epoch += 1;
                }
            }
            frame(&mut app);
            let wheels: Vec<_> = app
                .world_mut()
                .query_filtered::<&Transform, With<vehicles::Wheel>>()
                .iter(app.world())
                .copied()
                .collect();
            assert_eq!(wheels.len(), 2);
            for wheel in wheels {
                assert!(
                    wheel
                        .rotation
                        .dot(Quat::from_rotation_x(expected_angle))
                        .abs()
                        > 0.99999,
                    "heading={heading}, offset={offset:?}, wheel={wheel:?}"
                );
            }
        }
        app.world_mut().resource_mut::<Session>().vehicle = None;
        frame(&mut app);
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<vehicles::Wheel>>()
                .iter(app.world())
                .count(),
            0
        );
    }
}

#[test]
fn kayak_paddle_follows_both_hands_while_resting_and_stroking() {
    use rubblekin_core::vehicles::{Vehicle, VehicleKind};
    let mut app = vehicle_app();
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        session.body.position = [10., 80., 10.];
        session.vehicle = Some(Vehicle {
            tilt: 0.0,
            kind: VehicleKind::Kayak,
            heading: 0.7,
        });
    }
    for effort in [0., 1., 1., 0.] {
        app.world_mut().resource_mut::<Session>().vehicle_effort = effort;
        for _ in 0..12 {
            frame(&mut app);
            let rider = app.world().resource::<Avatars>().players[&17];
            let root = *app.world().get::<Transform>(rider).unwrap();
            let hands: Vec<_> = player_limbs(&mut app, 17)
                .into_iter()
                .filter(|(arm, _)| *arm)
                .map(|(_, arm)| root.transform_point(arm.transform_point(Vec3::NEG_Y * 0.5)))
                .collect();
            assert_eq!(hands.len(), 2);
            let craft = *app
                .world_mut()
                .query_filtered::<&Transform, With<vehicles::Craft>>()
                .single(app.world())
                .unwrap();
            let paddle = *app
                .world_mut()
                .query_filtered::<&Transform, With<vehicles::Paddle>>()
                .single(app.world())
                .unwrap();
            let centre = craft.transform_point(paddle.translation);
            let axis = craft.rotation * paddle.rotation * Vec3::X;
            assert!(centre.distance((hands[0] + hands[1]) * 0.5) < 0.0001);
            for hand in hands {
                assert!(
                    (hand - centre).cross(axis).length() < 0.0001,
                    "shaft must pass through the hand: {hand:?}, centre={centre:?}, axis={axis:?}"
                );
            }
        }
    }
    app.world_mut().resource_mut::<Session>().vehicle = None;
    frame(&mut app);
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<vehicles::Paddle>>()
            .iter(app.world())
            .count(),
        0
    );
}

#[test]
fn bike_hands_reach_the_handlebar_and_arm_shapes_restore_after_riding() {
    use rubblekin_core::{
        vehicles::{Vehicle, VehicleKind},
        world::WorldGeneration,
    };
    let world = GameWorld::generate(2_689_504_302, WorldGeneration::GeographyV6);
    for heading in [0., 0.7, std::f32::consts::PI] {
        let mut app = vehicle_app();
        app.insert_resource(VoxelWorld(world.clone()));
        {
            let mut session = app.world_mut().resource_mut::<Session>();
            session.body.position = [-386.25, 183.5, 7808.25];
            session.vehicle = Some(Vehicle {
                tilt: 0.0,
                kind: VehicleKind::Bike,
                heading,
            });
            let mut remote = session.players[0].clone();
            remote.id = 31;
            remote.body.position = [-384.25, 183.5, 7808.25];
            remote.vehicle = session.vehicle;
            session.players.push(remote);
        }
        frame(&mut app);
        for effort in [0., 1.] {
            app.world_mut().resource_mut::<Session>().vehicle_effort = effort;
            frame(&mut app);
            for id in [17, 31] {
                let rider = app.world().resource::<Avatars>().players[&id];
                let root = *app.world().get::<Transform>(rider).unwrap();
                for (_, arm) in player_limbs(&mut app, id)
                    .into_iter()
                    .filter(|(arm, _)| *arm)
                {
                    let hand = root.transform_point(arm.transform_point(Vec3::NEG_Y * 0.5));
                    let grip = root.translation
                        + root.rotation
                            * (vehicles::bike_grip(arm.translation.x.signum())
                                - vehicles::seat_offset(VehicleKind::Bike));
                    assert!(
                        hand.distance(grip) < 0.001,
                        "hand must meet the grip: {hand:?} {grip:?}"
                    );
                    let shoulder = arm.transform_point(Vec3::Y * 0.5);
                    assert!((shoulder.y - 1.26).abs() < 0.0001 && shoulder.z.abs() < 0.0001);
                }
            }
        }
        for mounted in [
            Some(Vehicle {
                tilt: 0.0,
                kind: VehicleKind::Kayak,
                heading,
            }),
            None,
        ] {
            {
                let mut session = app.world_mut().resource_mut::<Session>();
                session.vehicle = mounted;
                session.players[1].vehicle = mounted;
                session.vehicle_effort = 0.;
            }
            frame(&mut app);
            for id in [17, 31] {
                for (_, arm) in player_limbs(&mut app, id)
                    .into_iter()
                    .filter(|(arm, _)| *arm)
                {
                    assert_eq!(arm.scale, Vec3::new(0.16, 0.52, 0.20));
                    assert_eq!(arm.translation.y, 1.);
                    assert_eq!(arm.translation.z, 0.);
                    assert_eq!(arm.translation.x.abs(), 0.31);
                }
            }
        }
    }
}

#[test]
fn bike_feet_follow_opposed_pedals_and_hold_their_phase_while_coasting() {
    use rubblekin_core::{
        vehicles::{Vehicle, VehicleKind},
        world::WorldGeneration,
    };
    let mut app = vehicle_app();
    app.insert_resource(VoxelWorld(GameWorld::generate(
        2_689_504_302,
        WorldGeneration::GeographyV6,
    )));
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        session.body.position = [-386.25, 183.5, 7808.25];
        session.vehicle = Some(Vehicle {
            tilt: 0.0,
            kind: VehicleKind::Bike,
            heading: 0.7,
        });
        session.vehicle_effort = 1.;
        let mut remote = session.players[0].clone();
        remote.id = 31;
        remote.vehicle = session.vehicle;
        remote.body.position = [-384.25, 183.5, 7808.25];
        remote.body.velocity = [3., 0., 0.];
        session.players.push(remote);
    }
    frame(&mut app);
    for _ in 0..20 {
        frame(&mut app);
        let parts: Vec<_> = app
            .world_mut()
            .query::<(&vehicles::PedalPart, &Transform, &ChildOf)>()
            .iter(app.world())
            .map(|(part, transform, parent)| {
                (
                    part.rider,
                    part.side,
                    part.crank,
                    *transform,
                    parent.parent(),
                )
            })
            .collect();
        for id in [17, 31] {
            let rider = app.world().resource::<Avatars>().players[&id];
            let root = *app.world().get::<Transform>(rider).unwrap();
            let legs: Vec<_> = player_limbs(&mut app, id)
                .into_iter()
                .filter(|(arm, _)| !arm)
                .collect();
            let mut centres = Vec3::ZERO;
            for (_, leg) in legs {
                let side = leg.translation.x.signum();
                let (_, _, _, pedal, parent) = parts
                    .iter()
                    .find(|(owner, s, crank, _, _)| *owner == id && *s == side && !crank)
                    .unwrap();
                let craft = *app.world().get::<Transform>(*parent).unwrap();
                let foot = root.transform_point(leg.transform_point(Vec3::NEG_Y * 0.5));
                let top = craft.transform_point(pedal.transform_point(Vec3::Y * 0.5));
                assert!(
                    foot.distance(top) < 0.002,
                    "foot must meet the pedal: {foot:?} {top:?}"
                );
                assert_eq!(
                    pedal.rotation,
                    Quat::IDENTITY,
                    "pedals stay flat relative to the hull"
                );
                centres += pedal.translation * 0.5;
                let (_, _, _, crank, _) = parts
                    .iter()
                    .find(|(owner, s, crank, _, _)| *owner == id && *s == side && *crank)
                    .unwrap();
                assert!(
                    crank
                        .transform_point(Vec3::Y * 0.5)
                        .distance(pedal.translation)
                        < 0.00001
                );
            }
            assert!(
                centres.distance(Vec3::new(0., 0.34, 0.)) < 0.00001,
                "pedals stay opposed about the axle"
            );
        }
    }
    let phases = app.world().resource::<Avatars>().player_pedal_phase.clone();
    assert!(phases.values().all(|phase| *phase > 1.));
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        session.vehicle_effort = 0.;
        session.body.velocity = [3., 0., 0.];
        session.players[1].body.velocity = [0.; 3];
    }
    let resting = player_limbs(&mut app, 17);
    for _ in 0..5 {
        frame(&mut app);
    }
    assert_eq!(phases, app.world().resource::<Avatars>().player_pedal_phase);
    assert_eq!(
        resting,
        player_limbs(&mut app, 17),
        "coasting holds the last pedal pose"
    );
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        session.vehicle = None;
        session.players[1].vehicle = None;
    }
    frame(&mut app);
    assert!(
        app.world()
            .resource::<Avatars>()
            .player_pedal_phase
            .is_empty()
    );
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<vehicles::PedalPart>>()
            .iter(app.world())
            .count(),
        0
    );
    for (_, leg) in player_limbs(&mut app, 17)
        .into_iter()
        .filter(|(arm, _)| !arm)
    {
        assert_eq!(leg.scale, Vec3::new(0.18, 0.66, 0.22));
        assert_eq!(
            leg.translation,
            Vec3::new(leg.translation.x.signum() * 0.13, 0.38, 0.)
        );
    }
}

#[test]
fn vehicle_riders_sit_on_the_actual_seats_and_follow_the_hulls_pitch() {
    use rubblekin_core::{
        vehicles::{Vehicle, VehicleKind},
        world::WorldGeneration,
    };
    let world = GameWorld::generate(2_689_504_302, WorldGeneration::GeographyV6);
    for (kind, cushion) in [
        (VehicleKind::Bike, Vec3::new(0., 0.89, 0.20)),
        (VehicleKind::Kayak, Vec3::new(0., 0.20, 0.12)),
        (VehicleKind::Sailboat, Vec3::new(0., 0.42, 0.65)),
    ] {
        let mut app = avatar_app();
        app.insert_resource(VoxelWorld(world.clone()));
        let craft = Vehicle {
            tilt: if kind == VehicleKind::Bike { 0.30 } else { 0.0 },
            kind,
            heading: std::f32::consts::PI,
        };
        let position = [64., 49.955513, -11067.809];
        {
            let mut session = app.world_mut().resource_mut::<Session>();
            session.body.position = position;
            session.vehicle = Some(craft);
            let mut remote = session.players[0].clone();
            remote.id = 31;
            remote.body.position = position;
            remote.vehicle = Some(craft);
            session.players.push(remote);
        }
        frame(&mut app);
        let rotation = vehicles::rotation(&world, position, craft);
        assert!(
            rotation.angle_between(Quat::from_rotation_y(-craft.heading)) > 0.05,
            "{kind:?} fixture must tilt"
        );
        for id in [17, 31] {
            let root = app
                .world()
                .get::<Transform>(app.world().resource::<Avatars>().players[&id])
                .unwrap();
            let butt = root.transform_point(Vec3::Y * 0.70);
            let seat = Vec3::from_array(position) + rotation * cushion;
            assert!(
                butt.distance(seat) < 0.0001,
                "{kind:?} must meet its seat: {butt:?} versus {seat:?}"
            );
            assert!(root.rotation.dot(rotation).abs() > 0.99999);
        }
        // The first update inserts the avatar children; the next animates them.
        frame(&mut app);
        if kind != VehicleKind::Bike {
            let floor = if kind == VehicleKind::Kayak {
                0.11
            } else {
                0.03
            };
            for id in [17, 31] {
                for (_, limb) in player_limbs(&mut app, id)
                    .into_iter()
                    .filter(|(arm, _)| !arm)
                {
                    let hip = limb.transform_point(Vec3::Y * 0.5);
                    assert!(
                        hip.distance(Vec3::new(hip.x, 0.70, 0.)) < 0.0001,
                        "{kind:?} rider {id} hip: {hip:?}, limb: {limb:?}"
                    );
                    let offset = vehicles::seat_offset(kind);
                    let toe_bottom = [-0.5, 0.5]
                        .map(|z| (limb.transform_point(Vec3::new(0., -0.5, z)) + offset).y)
                        .into_iter()
                        .fold(f32::INFINITY, f32::min);
                    assert!(
                        (floor..=floor + 0.04).contains(&toe_bottom),
                        "{kind:?} seated foot must clear its pitched hull floor: {toe_bottom} versus {floor}"
                    );
                    for y in [-0.5, 0.5] {
                        for z in [-0.5, 0.5] {
                            let bottom = (limb.transform_point(Vec3::new(0., y, z)) + offset).y;
                            assert!(bottom >= floor - 0.0001, "leg penetrates {kind:?} floor");
                        }
                    }
                }
            }
            {
                let mut session = app.world_mut().resource_mut::<Session>();
                session.vehicle = None;
                for player in &mut session.players {
                    player.vehicle = None;
                }
            }
            frame(&mut app);
            for id in [17, 31] {
                for (_, limb) in player_limbs(&mut app, id)
                    .into_iter()
                    .filter(|(arm, _)| !arm)
                {
                    assert_eq!(limb.scale, Vec3::new(0.18, 0.66, 0.22));
                    assert_eq!(limb.translation.y, 0.38);
                    assert_eq!(limb.translation.z, 0.);
                }
            }
        }
        frame(&mut app);
        if kind == VehicleKind::Bike {
            let entity = app.world().resource::<Avatars>().players[&17];
            let root = *app.world().get::<Transform>(entity).unwrap();
            let mut query = app
                .world_mut()
                .query::<(&GroundShadow, &ChildOf, &Transform)>();
            let (_, _, shadow) = query
                .iter(app.world())
                .find(|(_, parent, _)| parent.parent() == entity)
                .unwrap();
            let centre = root.transform_point(shadow.translation);
            assert!(
                centre.distance(Vec3::from_array(position) + Vec3::Y * 0.018) < 0.002,
                "shadow contact centre: {centre:?}; root={root:?}; child={shadow:?}"
            );
            let normal = root.rotation * shadow.rotation * Vec3::Z;
            assert!(
                normal.dot(Vec3::Y) > 0.9999,
                "contact shadows must stay horizontal"
            );
        }
    }
}

#[test]
fn a_wall_compressed_camera_hides_only_self_and_reveals_when_the_view_clears() {
    let mut app = avatar_app();
    let mut world = GameWorld::new(42);
    for x in -3..=5 {
        for y in 38..=44 {
            world
                .set_block(BlockPos::new(x, y, 2), Block::Wood)
                .unwrap();
        }
    }
    app.insert_resource(VoxelWorld(world)).add_systems(
        Update,
        (camera, update_self_visibility)
            .chain()
            .after(update_avatars),
    );
    let view = app
        .world_mut()
        .spawn((GameCamera, Transform::default(), CameraFollow::default()))
        .id();
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        session.body.position = [0.25, 20.25 - EYE_HEIGHT, 0.25];
        session.yaw = 0.;
        session.pitch = 0.;
        session.camera_distance = 4.;
        let mut remote = session.players[0].clone();
        remote.id = 31;
        remote.body.position = [3., 20., 0.25];
        session.players.push(remote);
    }
    frame(&mut app);
    let local = app.world().resource::<Avatars>().players[&17];
    let eye = app
        .world()
        .get::<Transform>(local)
        .unwrap()
        .transform_point(Vec3::Y * EYE_HEIGHT);
    assert!(
        app.world()
            .get::<Transform>(view)
            .unwrap()
            .translation
            .distance(eye)
            < 0.8
    );
    assert_eq!(
        app.world().get::<Visibility>(local),
        Some(&Visibility::Hidden)
    );
    let remote = app.world().resource::<Avatars>().players[&31];
    assert_eq!(
        app.world().get::<Visibility>(remote),
        Some(&Visibility::Inherited)
    );
    assert!(
        app.world_mut()
            .query::<(Entity, &Avatar, &Visibility)>()
            .iter(app.world())
            .filter(|(entity, _, _)| *entity != local)
            .all(|(_, _, visibility)| *visibility == Visibility::Inherited),
        "the nearby NPC and other players remain visible"
    );
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        session.yaw = std::f32::consts::PI;
        session.camera_distance = 1.;
    }
    frame(&mut app);
    assert_eq!(
        app.world().get::<Visibility>(local),
        Some(&Visibility::Inherited)
    );
    assert!(
        app.world()
            .get::<Transform>(view)
            .unwrap()
            .translation
            .distance(eye)
            > 1.2
    );
}

#[test]
fn close_view_visibility_uses_the_pitched_seat_and_does_not_flicker_at_the_boundary() {
    use rubblekin_core::{
        vehicles::{Vehicle, VehicleKind},
        world::WorldGeneration,
    };
    let world = GameWorld::generate(2_689_504_302, WorldGeneration::GeographyV6);
    for kind in VehicleKind::ALL {
        let mut app = avatar_app();
        app.insert_resource(VoxelWorld(world.clone()))
            .add_systems(Update, update_self_visibility.after(update_avatars));
        let view = app
            .world_mut()
            .spawn((GameCamera, Transform::default()))
            .id();
        {
            let mut session = app.world_mut().resource_mut::<Session>();
            session.body.position = [64., 49.955513, -11067.809];
            session.vehicle = Some(Vehicle {
                tilt: 0.0,
                kind,
                heading: std::f32::consts::PI,
            });
        }
        frame(&mut app);
        let local = app.world().resource::<Avatars>().players[&17];
        let eye = app
            .world()
            .get::<Transform>(local)
            .unwrap()
            .transform_point(Vec3::Y * EYE_HEIGHT);
        for (distance, expected) in [
            (0.8, Visibility::Hidden),
            (1.1, Visibility::Hidden),
            (1.3, Visibility::Inherited),
            (1.1, Visibility::Inherited),
        ] {
            app.world_mut()
                .get_mut::<Transform>(view)
                .unwrap()
                .translation = eye + Vec3::Y * distance;
            frame(&mut app);
            assert_eq!(
                app.world().get::<Visibility>(local),
                Some(&expected),
                "{kind:?} at {distance}"
            );
        }
    }
}

#[test]
fn riding_camera_matches_the_seated_pose_and_smoothly_returns_to_standing() {
    use rubblekin_core::{
        vehicles::{Vehicle, VehicleKind},
        world::WorldGeneration,
    };
    let world = GameWorld::generate(2_689_504_302, WorldGeneration::GeographyV6);
    for kind in VehicleKind::ALL {
        let mut app = avatar_app();
        app.insert_resource(VoxelWorld(world.clone()))
            .add_systems(Update, camera.after(update_avatars));
        let view = app
            .world_mut()
            .spawn((GameCamera, Transform::default(), CameraFollow::default()))
            .id();
        {
            let mut session = app.world_mut().resource_mut::<Session>();
            session.body.position = [64., 49.955513, -11067.809];
            session.vehicle = Some(Vehicle {
                tilt: 0.0,
                kind,
                heading: std::f32::consts::PI,
            });
            session.pitch = 0.;
        }
        frame(&mut app);
        let root = *app
            .world()
            .get::<Transform>(app.world().resource::<Avatars>().players[&17])
            .unwrap();
        let seated_eye = root.transform_point(Vec3::Y * EYE_HEIGHT);
        let previous = app.world().get::<Transform>(view).unwrap().translation.y;
        assert!(
            (previous - seated_eye.y).abs() < 0.0001,
            "{kind:?} camera must frame the actual pitched seat"
        );
        // The river bank can legitimately clip immediately into the standing
        // view. Measure dismount smoothing on an unobstructed, valid surface.
        let position = if kind == VehicleKind::Bike {
            let ground = GameWorld::new(42);
            let position = ground.spawn_position();
            app.insert_resource(VoxelWorld(ground));
            position
        } else {
            [15_000., 0.02, 15_000.]
        };
        app.world_mut().resource_mut::<Session>().body.position = position;
        *app.world_mut().get_mut::<CameraFollow>(view).unwrap() = CameraFollow::default();
        frame(&mut app);
        let root = app
            .world()
            .get::<Transform>(app.world().resource::<Avatars>().players[&17])
            .unwrap();
        let seated_eye = root.transform_point(Vec3::Y * EYE_HEIGHT);
        let mut previous = app.world().get::<Transform>(view).unwrap().translation.y;
        assert!((previous - seated_eye.y).abs() < 0.0001);
        app.world_mut().resource_mut::<Session>().vehicle = None;
        let standing_eye = app.world().resource::<Session>().body.position[1] + EYE_HEIGHT;
        for _ in 0..45 {
            frame(&mut app);
            let current = app.world().get::<Transform>(view).unwrap().translation.y;
            assert!(
                (current - previous).abs() < 0.20,
                "{kind:?} dismount view changed from {previous} to {current}; seated={seated_eye:?}, standing={standing_eye}"
            );
            assert!(
                current >= seated_eye.y.min(standing_eye) - 0.0001
                    && current <= seated_eye.y.max(standing_eye) + 0.0001
            );
            previous = current;
        }
        assert!((previous - standing_eye).abs() < 0.0001);
    }
}

#[test]
fn carriage_camera_follows_the_actual_seated_eye_during_continuous_flight() {
    let mut app = avatar_app();
    app.add_systems(Update, gliders::carry.before(update_avatars))
        .add_systems(Update, camera.after(update_avatars));
    let view = app
        .world_mut()
        .spawn((GameCamera, Transform::default(), CameraFollow::default()))
        .id();
    {
        let mut session = app.world_mut().resource_mut::<Session>();
        session.gliders.push(GliderFlight {
            emergency: false,
            id: 5,
            station_id: 0,
            destination: GliderDestination::Village(1),
            destination_name: "Town".into(),
            from: [10., 200., 20.],
            to: [500., 200., 400.],
            apex: 600.,
            duration: 30.,
            created_at: 0.,
            started_at: Some(0.),
        });
        session.glider_ride = Some(GliderRide {
            carriage_id: 5,
            seat: 0,
        });
        session.pitch = 0.;
    }
    for time in [0., 2., 8.2, 12.5, 19., 28., 30.] {
        app.world_mut().resource_mut::<Session>().airship_clock.time = time;
        frame(&mut app);
        let root = app
            .world()
            .get::<Transform>(app.world().resource::<Avatars>().players[&17])
            .unwrap();
        let expected = root.transform_point(Vec3::Y * EYE_HEIGHT);
        let actual = app.world().get::<Transform>(view).unwrap();
        assert!(
            (actual.translation.y - expected.y).abs() < 0.0001,
            "camera must follow the rendered seat at flight time {time}: {:?}, {expected:?}",
            actual.translation
        );
    }
}

#[test]
fn avatar_smoothing_falls_back_when_its_blend_crosses_another_person() {
    let world = GameWorld::new(1);
    let desired = [0.25, 8.0, 0.25];
    let mut positions = [desired, [1.25, 8.0, 0.25]];
    let rendered =
        smoothed_avatar_position(&world, Vec3::new(2.25, 8.0, 0.25), &mut positions, 0, 0.5);
    assert_eq!(rendered.to_array(), desired);
    assert!(!characters_overlap(positions[0], positions[1]));
}

#[test]
fn avatar_smoothing_keeps_previous_rendered_poses_clear_during_a_crossing() {
    let world = GameWorld::new(1);
    let mut positions = [[-0.5, 8.0, 0.25], [0.5, 8.0, 0.25]];
    smoothed_avatar_position(&world, Vec3::new(1.5, 8.0, 0.25), &mut positions, 0, 0.5);
    smoothed_avatar_position(&world, Vec3::new(-1.5, 8.0, 0.25), &mut positions, 1, 0.5);
    assert!(!characters_overlap(positions[0], positions[1]));
    assert!(
        positions
            .iter()
            .all(|position| { character_position_is_clear(&world, *position, &[]) })
    );
}

#[test]
fn npc_render_pose_stays_clear_of_a_player_predicted_ahead_of_the_snapshot() {
    let world = GameWorld::new(1);
    let player_snapshot = [0.25, 8.0, 0.25];
    let npc_snapshot = [1.25, 8.0, 0.25];
    assert!(!characters_overlap(player_snapshot, npc_snapshot));
    let predicted_player = [1.45, 8.0, 0.25];
    let mut positions = [predicted_player, npc_snapshot];
    let rendered = smoothed_avatar_position(
        &world,
        Vec3::from_array(npc_snapshot),
        &mut positions,
        1,
        0.5,
    );
    assert_eq!(positions[0], predicted_player);
    assert!(!characters_overlap(rendered.to_array(), predicted_player));
    assert!(character_position_is_clear(
        &world,
        rendered.to_array(),
        &[predicted_player],
    ));
}
