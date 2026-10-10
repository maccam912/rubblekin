use super::*;
use rubblekin_core::physics::{characters_overlap, move_character};
use rubblekin_core::world::{Block, BlockPos};

fn snapshot(body: Body, last_input_sequence: u64) -> PlayerSnapshot {
    PlayerSnapshot {
        parcel_destination: None,
        glider_ride: None,
        vehicle: None,
        gliding: false,
        id: 1,
        name: "Walker".into(),
        body,
        yaw: 0.0,
        last_input_sequence,
        movement_epoch: 0,
        ride: None,
        deck_position: None,
    }
}

fn airship_fixture() -> (
    World,
    AirshipNetwork,
    rubblekin_core::airships::AirshipSnapshot,
) {
    let world =
        World::from_generation_edits(42, rubblekin_core::world::WorldGeneration::GeographyV3, &[])
            .unwrap();
    let network = AirshipNetwork::new(&world);
    let ship = network
        .ships(60.0)
        .into_iter()
        .find(|ship| ship.docked_at.is_none())
        .unwrap();
    (world, network, ship)
}

#[test]
fn airship_replay_keeps_local_walk_and_normal_jump_after_delayed_acknowledgments() {
    use rubblekin_core::airships::deck_position;
    let (world, network, ship) = airship_fixture();
    let mut ride = Some(AirshipRide {
        ship_id: ship.id,
        seat: u8::MAX,
    });
    let mut local = Some([0.0; 3]);
    let mut body = Body::new(deck_position(&ship, local.unwrap()));
    body.on_ground = true;
    let initial = snapshot(body.clone(), 0);
    let mut prediction = Prediction::default();
    let direction = [ship.yaw.cos(), -ship.yaw.sin()];
    let walking = MoveInput {
        direction,
        ..Default::default()
    };
    prediction
        .advance_airships(
            &world,
            &mut body,
            walking,
            0.0,
            0.1,
            &[],
            &network,
            60.0,
            &mut ride,
            &mut local,
        )
        .unwrap();
    let first_local = local.unwrap();
    let mut acknowledged = snapshot(body.clone(), 1);
    acknowledged.ride = ride;
    acknowledged.deck_position = local;
    let jumping = MoveInput {
        direction,
        jump: true,
        ..Default::default()
    };
    let command = prediction
        .advance_airships(
            &world,
            &mut body,
            jumping,
            0.0,
            0.1,
            &[],
            &network,
            60.1,
            &mut ride,
            &mut local,
        )
        .unwrap();
    assert!(matches!(
        command,
        ClientMessage::Input {
            sequence: 2,
            input: MoveInput { jump: true, .. },
            ..
        }
    ));
    assert!(body.velocity[1] > 0.0 && !body.on_ground);
    let expected_local = local.unwrap();
    let later = network.ship(ship.id, 61.0).unwrap();
    assert_ne!(later.position, ship.position);
    acknowledged.body.position = deck_position(&later, first_local);
    prediction
        .reconcile_airships(
            &world,
            &mut body,
            &acknowledged,
            |_| vec![],
            &network,
            61.0,
            &mut ride,
            &mut local,
        )
        .unwrap();
    for (actual, expected) in local.unwrap().iter().zip(expected_local) {
        assert!((actual - expected).abs() < 0.002);
    }
    let expected = deck_position(&later, local.unwrap());
    for (actual, expected) in body.position.iter().zip(expected) {
        assert!((actual - expected).abs() < 0.002);
    }
    assert!(!body.on_ground);
    assert_eq!(prediction.pending.len(), 1);
    let mut final_ack = snapshot(body.clone(), 2);
    final_ack.ride = ride;
    final_ack.deck_position = local;
    prediction
        .reconcile_airships(
            &world,
            &mut body,
            &final_ack,
            |_| vec![],
            &network,
            61.0,
            &mut ride,
            &mut local,
        )
        .unwrap();
    assert!(prediction.pending.is_empty());
    assert!(initial.ride.is_none());
}

#[test]
fn delayed_airship_replay_projects_nearby_passengers_at_each_pending_ship_pose() {
    use rubblekin_core::airships::{deck_local_position, deck_position};
    use std::cell::RefCell;

    let (world, network, _) = airship_fixture();
    for turning in [false, true] {
        let delay = if turning { 1.0 } else { 0.025 };
        let (world_time, ship) = (0..2_000)
            .find_map(|second| {
                let time = second as f64;
                network.ships(time).into_iter().find_map(|ship| {
                    let next = network.ship(ship.id, time + delay).unwrap();
                    let horizontal = ((next.position[0] - ship.position[0]).powi(2)
                        + (next.position[2] - ship.position[2]).powi(2))
                    .sqrt();
                    let suitable = if turning {
                        ship.docked_at.is_some()
                            && ship.docked_at == next.docked_at
                            && (ship.yaw - next.yaw).abs() > 0.25
                    } else {
                        ship.docked_at.is_none()
                            && next.docked_at.is_none()
                            && (ship.position[1] - next.position[1]).abs() < 0.001
                            && horizontal > 1.0
                    };
                    suitable.then_some((time, ship))
                })
            })
            .expect("fixture needs a cruising ship and a turning berth");
        let pending_times = [world_time + delay, world_time + delay + 1.0 / 60.0];
        let first_pending_ship = network.ship(ship.id, pending_times[0]).unwrap();
        let initial_local = if turning { [2.0, 0.0, 4.0] } else { [0.0; 3] };
        // This companion is clear at every shared deck pose, but its old
        // world-space position coincides with our later carried position.
        let companion_local =
            deck_local_position(&ship, deck_position(&first_pending_ship, initial_local));
        let stale_companion = deck_position(&ship, companion_local);
        assert!(!characters_overlap(
            deck_position(&ship, initial_local),
            stale_companion
        ));
        assert!(characters_overlap(
            deck_position(&first_pending_ship, initial_local),
            stale_companion
        ));

        let mut ride = Some(AirshipRide {
            ship_id: ship.id,
            seat: u8::MAX,
        });
        let mut local = Some(initial_local);
        let mut body = Body::new(deck_position(&ship, initial_local));
        body.on_ground = true;
        let mut authoritative = snapshot(body.clone(), 0);
        authoritative.ride = ride;
        authoritative.deck_position = local;
        let mut prediction = Prediction::default();
        for time in pending_times {
            let pose = network.ship(ship.id, time).unwrap();
            let companion = deck_position(&pose, companion_local);
            prediction
                .advance_airships(
                    &world,
                    &mut body,
                    MoveInput::default(),
                    0.0,
                    1.0 / 60.0,
                    &[companion],
                    &network,
                    time,
                    &mut ride,
                    &mut local,
                )
                .unwrap();
            assert!(!characters_overlap(body.position, companion));
        }
        let expected_body = body.clone();
        let expected_local = local.unwrap();

        // Verify this fixture actually reproduces the old collision error.
        prediction
            .reconcile_airships(
                &world,
                &mut body,
                &authoritative,
                |_| vec![stale_companion],
                &network,
                world_time,
                &mut ride,
                &mut local,
            )
            .unwrap();
        assert!(
            local
                .unwrap()
                .iter()
                .zip(expected_local)
                .any(|(actual, expected)| (actual - expected).abs() > 0.05),
            "stale passenger pose did not reproduce the correction jump"
        );

        let projected_times = RefCell::new(Vec::new());
        prediction
            .reconcile_airships(
                &world,
                &mut body,
                &authoritative,
                |time| {
                    projected_times.borrow_mut().push(time);
                    vec![deck_position(
                        &network.ship(ship.id, time).unwrap(),
                        companion_local,
                    )]
                },
                &network,
                world_time,
                &mut ride,
                &mut local,
            )
            .unwrap();
        assert_eq!(*projected_times.borrow(), pending_times);
        assert_same_body(&body, &expected_body);
        assert_eq!(local, Some(expected_local));
        assert_eq!(ride, authoritative.ride);
    }
}

#[test]
fn client_movement_can_walk_off_a_flying_deck_without_an_exit_action() {
    use rubblekin_core::airships::deck_position;
    let (world, network, ship) = airship_fixture();
    let mut ride = Some(AirshipRide {
        ship_id: ship.id,
        seat: u8::MAX,
    });
    let mut local = Some([3.7, 0.0, 0.0]);
    let mut body = Body::new(deck_position(&ship, local.unwrap()));
    body.on_ground = true;
    let mut prediction = Prediction::default();
    let input = MoveInput {
        direction: [ship.yaw.cos(), -ship.yaw.sin()],
        sprint: true,
        ..Default::default()
    };
    prediction
        .advance_airships(
            &world,
            &mut body,
            input,
            0.0,
            0.25,
            &[],
            &network,
            60.0,
            &mut ride,
            &mut local,
        )
        .unwrap();
    assert!(ride.is_none() && local.is_none());
    assert!(!body.on_ground);
}

fn execute(world: &World, server: &mut PlayerSnapshot, message: ClientMessage) {
    let ClientMessage::Input {
        sequence,
        movement_epoch,
        input,
        yaw,
        dt,
    } = message
    else {
        panic!("expected movement")
    };
    assert_eq!(movement_epoch, server.movement_epoch);
    move_character(world, &mut server.body, input, dt);
    server.last_input_sequence = sequence;
    server.yaw = yaw;
}

fn assert_same_body(actual: &Body, expected: &Body) {
    assert_eq!(actual.position, expected.position);
    assert_eq!(actual.velocity, expected.velocity);
    assert_eq!(actual.on_ground, expected.on_ground);
}

#[test]
fn delayed_stop_snapshot_preserves_unacknowledged_press_and_release() {
    let world = World::new(1);
    let mut body = Body::new([0.25, 8.0, 0.25]);
    let mut server = snapshot(body.clone(), 0);
    let mut prediction = Prediction::default();
    let walking = MoveInput {
        direction: [1.0, 0.0],
        fly: true,
        ..Default::default()
    };
    let stopped = MoveInput {
        fly: true,
        ..Default::default()
    };
    let first = prediction
        .advance(&world, &mut body, walking, 0.0, 0.04, &[])
        .unwrap();
    execute(&world, &mut server, first);
    // Server has seen the first movement but not the later tap and key release.
    let second = prediction
        .advance(&world, &mut body, walking, 0.0, 0.03, &[])
        .unwrap();
    let third = prediction
        .advance(&world, &mut body, stopped, 0.0, 0.01, &[])
        .unwrap();
    let stopped_at = body.clone();
    for _ in 0..3 {
        prediction
            .reconcile(&world, &mut body, &server, &[])
            .unwrap();
        assert_same_body(&body, &stopped_at);
    }
    execute(&world, &mut server, second);
    execute(&world, &mut server, third);
    prediction
        .reconcile(&world, &mut body, &server, &[])
        .unwrap();
    assert_same_body(&body, &stopped_at);
    assert!(prediction.pending.is_empty());
}

#[test]
fn uneven_frames_and_delayed_snapshots_do_not_pull_back_on_stairs_or_cliffs() {
    let mut world = World::new(1);
    // Half-meter terraces followed by a wall too tall to walk up.
    for x in -3..=55 {
        let top = if x < 4 {
            4
        } else if x < 40 {
            5 + (x - 4) / 4
        } else {
            25
        };
        for z in -3..=3 {
            for y in 5..=30 {
                world
                    .set_block(
                        BlockPos::new(x, y, z),
                        if y <= top { Block::Brick } else { Block::Air },
                    )
                    .unwrap();
            }
        }
    }
    for latency in [0.005, 0.09, 0.2] {
        let mut body = Body::new([0.25, 2.5, 0.25]);
        body.on_ground = true;
        let mut server = snapshot(body.clone(), 0);
        let mut prediction = Prediction::default();
        let mut outbound = VecDeque::new();
        let mut inbound = VecDeque::new();
        let mut now = 0.0_f64;
        let mut next_tick = 0.0;
        let mut highest: f32 = body.position[1];
        let mut stopped_at = None;
        for frame in 0..650 {
            let dt = if frame == 155 {
                0.25
            } else {
                [1.0 / 144.0, 1.0 / 60.0, 1.0 / 30.0, 0.011][frame % 4]
            };
            now += dt as f64;
            if now >= next_tick {
                // The server runs at 20 Hz, with scheduling jitter/oversleep.
                next_tick = now + 0.053;
                while outbound.front().is_some_and(|(at, _)| *at <= now) {
                    let (_, message) = outbound.pop_front().unwrap();
                    execute(&world, &mut server, message);
                }
                inbound.push_back((now + latency, server.clone()));
            }
            while inbound.front().is_some_and(|(at, _)| *at <= now) {
                let (_, state) = inbound.pop_front().unwrap();
                let before = body.clone();
                prediction
                    .reconcile(&world, &mut body, &state, &[])
                    .unwrap();
                assert_same_body(&body, &before);
            }
            let input = MoveInput {
                direction: if now < 7.0 { [1.0, 0.0] } else { [0.0, 0.0] },
                sprint: now > 3.0 && now < 5.0,
                jump: now > 1.0 && now < 1.12,
                ..Default::default()
            };
            let command = prediction
                .advance(&world, &mut body, input, 0.0, dt, &[])
                .unwrap();
            outbound.push_back((now + latency, command));
            highest = highest.max(body.position[1]);
            if now >= 7.0 {
                let position = *stopped_at.get_or_insert(body.position);
                assert_eq!(
                    body.position[0], position[0],
                    "moved horizontally after release"
                );
            }
        }
        assert!(
            highest > 6.0,
            "fixture must actually climb multiple terraces"
        );
        assert!(
            (body.position[0] - (20.0 - rubblekin_core::physics::PLAYER_RADIUS)).abs() < 0.001,
            "must stop at the tall cliff"
        );
    }
}

#[test]
fn genuine_authoritative_correction_still_applies_and_replays_newer_input() {
    let world = World::new(1);
    let mut body = Body::new([0.25, 8.0, 0.25]);
    let mut prediction = Prediction::default();
    let command = prediction
        .advance(
            &world,
            &mut body,
            MoveInput {
                direction: [1.0, 0.0],
                fly: true,
                ..Default::default()
            },
            0.0,
            0.05,
            &[],
        )
        .unwrap();
    let corrected = snapshot(Body::new([0.25, 12.0, 0.25]), 0);
    let mut expected = corrected.clone();
    execute(&world, &mut expected, command);
    prediction
        .reconcile(&world, &mut body, &corrected, &[])
        .unwrap();
    assert_same_body(&body, &expected.body);
}

#[test]
fn predicted_movement_and_acknowledgment_replay_stop_at_other_characters() {
    let world = World::new(1);
    let start = Body::new([0.25, 8.0, 0.25]);
    let obstacles = [[1.25, 8.0, 0.25]];
    let mut body = start.clone();
    let mut server = snapshot(start, 0);
    let mut prediction = Prediction::default();
    let input = MoveInput {
        direction: [1.0, 0.0],
        fly: true,
        ..Default::default()
    };
    let mut commands = VecDeque::new();
    for _ in 0..8 {
        commands.push_back(
            prediction
                .advance(&world, &mut body, input, 0.0, 0.05, &obstacles)
                .unwrap(),
        );
        assert!(!characters_overlap(body.position, obstacles[0]));
    }
    let stopped_at = body.clone();
    // Replaying the full unacknowledged walk must keep the same contact point.
    prediction
        .reconcile(&world, &mut body, &server, &obstacles)
        .unwrap();
    assert_same_body(&body, &stopped_at);
    for command in commands {
        let ClientMessage::Input {
            sequence,
            input,
            dt,
            ..
        } = command
        else {
            panic!("expected movement");
        };
        move_character_with_obstacles(&world, &mut server.body, input, dt, &obstacles);
        server.last_input_sequence = sequence;
        prediction
            .reconcile(&world, &mut body, &server, &obstacles)
            .unwrap();
        assert_same_body(&body, &stopped_at);
        assert!(!characters_overlap(body.position, obstacles[0]));
    }
    assert!(body.position[0] < obstacles[0][0] - rubblekin_core::physics::PLAYER_RADIUS);
}

#[test]
fn teleport_discards_pending_ground_walk_and_jump_and_restarts_the_input_sequence() {
    let world = World::new(1);
    let mut body = Body::new(world.spawn_position());
    body.on_ground = true;
    let mut authoritative = snapshot(body.clone(), 0);
    let mut prediction = Prediction::default();
    let walking = MoveInput {
        direction: [1.0, 0.0],
        ..Default::default()
    };
    let first = prediction
        .advance(&world, &mut body, walking, 0.0, 0.05, &[])
        .unwrap();
    execute(&world, &mut authoritative, first);
    prediction
        .reconcile(&world, &mut body, &authoritative, &[])
        .unwrap();
    assert_eq!(prediction.acknowledged, 1);
    prediction
        .advance(&world, &mut body, walking, 0.0, 0.05, &[])
        .unwrap();
    prediction
        .advance(
            &world,
            &mut body,
            MoveInput {
                jump: true,
                ..walking
            },
            0.0,
            0.05,
            &[],
        )
        .unwrap();
    assert!(body.velocity[1] > 0.0 && !body.on_ground);
    assert_eq!(prediction.pending.len(), 2);

    authoritative.body = Body::new([12.25, 20.0, 12.25]);
    authoritative.movement_epoch = 1;
    authoritative.last_input_sequence = 0;
    prediction
        .reconcile(&world, &mut body, &authoritative, &[])
        .unwrap();
    assert_same_body(&body, &authoritative.body);
    assert!(prediction.pending.is_empty());
    assert_eq!(prediction.pending_seconds, 0.0);
    assert_eq!(prediction.movement_epoch(), 1);
    assert_eq!(prediction.acknowledged, 0);

    let next = prediction
        .advance(&world, &mut body, walking, 0.0, 0.05, &[])
        .unwrap();
    assert!(matches!(
        next,
        ClientMessage::Input {
            movement_epoch: 1,
            sequence: 1,
            ..
        }
    ));
    execute(&world, &mut authoritative, next);
    assert_same_body(&body, &authoritative.body);
    prediction
        .reconcile(&world, &mut body, &authoritative, &[])
        .unwrap();
    assert_same_body(&body, &authoritative.body);
    assert!(prediction.pending.is_empty());
}

#[test]
fn teleport_discards_pending_deck_motion_and_adopts_authoritative_platform_state() {
    use rubblekin_core::airships::deck_position;

    let (world, network, ship) = airship_fixture();
    let initial_ride = AirshipRide {
        ship_id: ship.id,
        seat: u8::MAX,
    };
    for destination_local in [None, Some([-2.0, 0.0, -2.0])] {
        let mut ride = Some(initial_ride);
        let mut local = Some([0.0; 3]);
        let mut body = Body::new(deck_position(&ship, local.unwrap()));
        body.on_ground = true;
        let mut prediction = Prediction::default();
        let walking = MoveInput {
            direction: [ship.yaw.cos(), -ship.yaw.sin()],
            ..Default::default()
        };
        for (time, jump) in [(60.0, false), (60.1, true)] {
            prediction
                .advance_airships(
                    &world,
                    &mut body,
                    MoveInput { jump, ..walking },
                    0.0,
                    0.1,
                    &[],
                    &network,
                    time,
                    &mut ride,
                    &mut local,
                )
                .unwrap();
        }
        assert!(body.velocity[1] > 0.0 && !body.on_ground);
        assert_eq!(prediction.pending.len(), 2);

        let destination_ship = network.ship(ship.id, 61.0).unwrap();
        let destination = destination_local.map_or_else(
            || world.spawn_position(),
            |offset| deck_position(&destination_ship, offset),
        );
        let mut authoritative = snapshot(Body::new(destination), 0);
        authoritative.body.on_ground = true;
        authoritative.movement_epoch = 1;
        authoritative.ride = destination_local.map(|_| initial_ride);
        authoritative.deck_position = destination_local;
        prediction
            .reconcile_airships(
                &world,
                &mut body,
                &authoritative,
                |_| panic!("teleport replayed an old command"),
                &network,
                61.0,
                &mut ride,
                &mut local,
            )
            .unwrap();
        assert_same_body(&body, &authoritative.body);
        assert_eq!(ride, authoritative.ride);
        assert_eq!(local, authoritative.deck_position);
        assert!(prediction.pending.is_empty());
        assert_eq!(prediction.pending_seconds, 0.0);

        let mut expected_body = authoritative.body.clone();
        let mut expected_ride = authoritative.ride;
        let mut expected_local = authoritative.deck_position;
        move_character_with_airships(
            &world,
            &mut expected_body,
            walking,
            0.05,
            &[],
            &network,
            61.05,
            &mut expected_ride,
            &mut expected_local,
        );
        let next = prediction
            .advance_airships(
                &world,
                &mut body,
                walking,
                0.0,
                0.05,
                &[],
                &network,
                61.05,
                &mut ride,
                &mut local,
            )
            .unwrap();
        assert!(matches!(
            next,
            ClientMessage::Input {
                movement_epoch: 1,
                sequence: 1,
                ..
            }
        ));
        assert_same_body(&body, &expected_body);
        assert_eq!(ride, expected_ride);
        assert_eq!(local, expected_local);
    }
}

#[test]
fn older_teleport_epochs_are_rejected_without_changing_prediction_or_body() {
    let world = World::new(1);
    let mut current = snapshot(Body::new([0.25, 20.0, 0.25]), 3);
    current.movement_epoch = 4;
    let mut prediction = Prediction::from_snapshot(&current);
    let mut body = current.body.clone();
    let next = prediction
        .advance(
            &world,
            &mut body,
            MoveInput {
                direction: [1.0, 0.0],
                fly: true,
                ..Default::default()
            },
            0.0,
            0.05,
            &[],
        )
        .unwrap();
    assert!(matches!(
        next,
        ClientMessage::Input {
            movement_epoch: 4,
            sequence: 4,
            ..
        }
    ));
    let before = body.clone();
    let mut stale = current.clone();
    stale.movement_epoch = 3;
    stale.body = Body::new([12.25, 20.0, 12.25]);
    assert!(
        prediction
            .reconcile(&world, &mut body, &stale, &[])
            .is_err()
    );
    assert_same_body(&body, &before);
    assert_eq!(prediction.movement_epoch(), 4);
    assert_eq!(prediction.pending.len(), 1);
    assert_eq!(prediction.pending_seconds, 0.05);
    assert_eq!(prediction.sequence, 4);
    assert_eq!(prediction.acknowledged, 3);

    // Same-epoch acknowledgments retain the original forward/backward bounds.
    current.last_input_sequence = 5;
    assert!(
        prediction
            .reconcile(&world, &mut body, &current, &[])
            .is_err()
    );
    current.last_input_sequence = 2;
    assert!(
        prediction
            .reconcile(&world, &mut body, &current, &[])
            .is_err()
    );
    assert_same_body(&body, &before);
    assert_eq!(prediction.pending.len(), 1);
}

#[test]
fn prediction_backlog_and_invalid_acknowledgments_are_bounded() {
    let world = World::new(1);
    let mut body = Body::new(world.spawn_position());
    let mut prediction = Prediction::default();
    for _ in 0..8 {
        prediction
            .advance(&world, &mut body, MoveInput::default(), 0.0, 0.25, &[])
            .unwrap();
    }
    let before = body.clone();
    assert!(
        prediction
            .advance(&world, &mut body, MoveInput::default(), 0.0, 0.01, &[])
            .is_err()
    );
    assert_same_body(&body, &before);
    assert!(
        prediction
            .reconcile(&world, &mut body, &snapshot(before.clone(), 9), &[])
            .is_err()
    );
    prediction
        .reconcile(&world, &mut body, &snapshot(before.clone(), 8), &[])
        .unwrap();
    assert!(
        prediction
            .reconcile(&world, &mut body, &snapshot(before, 7), &[])
            .is_err()
    );
    let mut prediction = Prediction::default();
    for _ in 0..MAX_PENDING_INPUTS {
        prediction
            .advance(&world, &mut body, MoveInput::default(), 0.0, 0.001, &[])
            .unwrap();
    }
    assert!(
        prediction
            .advance(&world, &mut body, MoveInput::default(), 0.0, 0.001, &[])
            .is_err()
    );
}

#[test]
fn full_movement_history_waits_for_progress_and_times_out_on_wall_time() {
    let world = World::new(1);
    let mut body = Body::new(world.spawn_position());
    let mut prediction = Prediction::default();
    for _ in 0..8 {
        prediction
            .advance(&world, &mut body, MoveInput::default(), 0.0, 0.25, &[])
            .unwrap();
    }
    let now = Instant::now();
    assert!(!prediction.ready_to_advance(0.25, true, now).unwrap());
    assert!(prediction.waiting());
    // Receiving snapshots without a newer acknowledgment is not progress.
    prediction.acknowledge(&snapshot(body.clone(), 0)).unwrap();
    assert!(prediction.waiting());
    assert!(
        !prediction
            .ready_to_advance(0.25, true, now + Duration::from_secs(9))
            .unwrap()
    );
    assert_eq!(prediction.pending.len(), 8);
    assert_eq!(prediction.pending_seconds, 2.0);
    assert_eq!(prediction.sequence, 8);
    assert!(
        prediction
            .ready_to_advance(0.25, true, now + SYNC_WAIT_TIMEOUT)
            .unwrap_err()
            .contains("timed out")
    );
    // A real acknowledgment releases capacity and clears the timeout/notice.
    prediction.acknowledge(&snapshot(body, 4)).unwrap();
    assert!(!prediction.waiting());
    assert!(
        prediction
            .ready_to_advance(0.25, true, now + SYNC_WAIT_TIMEOUT)
            .unwrap()
    );
    assert_eq!(prediction.pending.len(), 4);
}

#[test]
fn input_count_and_transport_pressure_pause_without_spending_sequences() {
    let now = Instant::now();
    let mut prediction = Prediction::default();
    // Pause before predicting a command that a full socket outbox cannot hold.
    assert!(!prediction.ready_to_advance(0.01, false, now).unwrap());
    assert!(prediction.pending.is_empty());
    assert_eq!(prediction.sequence, 0);
    assert!(
        prediction
            .ready_to_advance(0.01, true, now + Duration::from_secs(4))
            .unwrap()
    );
    assert!(!prediction.waiting());
    for _ in 0..MAX_PENDING_INPUTS {
        prediction
            .record(MoveInput::default(), 0.0, 0.001, None)
            .unwrap();
    }
    assert!(!prediction.ready_to_advance(0.001, true, now).unwrap());
    let mut authoritative = snapshot(Body::new([0.25, 20.0, 0.25]), 0);
    authoritative.movement_epoch = 1;
    prediction.acknowledge(&authoritative).unwrap();
    assert!(!prediction.waiting());
    assert!(prediction.pending.is_empty());
    assert!(prediction.ready_to_advance(0.25, true, now).unwrap());
    assert!(matches!(
        prediction
            .record(MoveInput::default(), 0.0, 0.25, None)
            .unwrap(),
        ClientMessage::Input {
            sequence: 1,
            movement_epoch: 1,
            ..
        }
    ));
}

#[test]
fn carriage_jump_replays_from_acknowledgment_without_reboarding() {
    use rubblekin_core::gliders::*;
    let world = World::new(42);
    let network = AirshipNetwork::default();
    let f = GliderFlight {
        emergency: false,
        id: 1,
        station_id: 0,
        destination: GliderDestination::Village(1),
        destination_name: "Test".into(),
        from: [0.0, 40.0, 0.0],
        to: [20.0, 10.0, 0.0],
        apex: 60.0,
        duration: 40.0,
        created_at: 0.0,
        started_at: Some(0.0),
    };
    let mut state = snapshot(Body::new([0.0, 60.0, 0.0]), 0);
    state.glider_ride = Some(GliderRide {
        carriage_id: 1,
        seat: 0,
    });
    let mut prediction = Prediction::from_snapshot(&state);
    let mut body = state.body.clone();
    let mut ride = state.glider_ride;
    let mut gliding = false;
    prediction
        .advance_gliders(
            &world,
            &mut body,
            MoveInput {
                jump: true,
                ..Default::default()
            },
            0.0,
            0.1,
            &[],
            &network,
            10.0,
            std::slice::from_ref(&f),
            &mut ride,
            &mut gliding,
        )
        .unwrap();
    let expected = body.position;
    prediction
        .reconcile_gliders(
            &world,
            &mut body,
            &state,
            |_| Vec::new(),
            &network,
            10.0,
            &[f],
            &mut ride,
            &mut gliding,
        )
        .unwrap();
    assert_eq!(body.position, expected);
    assert!(ride.is_none() && gliding);
}

#[test]
fn delayed_vehicle_acknowledgments_replay_momentum_and_a_new_epoch_discards_old_pedalling() {
    use rubblekin_core::vehicles::{self, VehicleKind};
    let world = World::new(42);
    let mut body = Body::new(world.spawn_position());
    move_character(&world, &mut body, MoveInput::default(), 0.25);
    let mut vehicle = Some(vehicles::spawn(&world, &mut body, VehicleKind::Bike, 0., &[]).unwrap());
    let mut prediction = Prediction::default();
    let mut acknowledged = None;
    for sequence in 1..=10 {
        let input = MoveInput {
            direction: [0., -1.],
            sprint: true,
            ..Default::default()
        };
        prediction
            .advance_vehicle(
                &world,
                &mut body,
                input,
                0.,
                0.05,
                &[],
                sequence as f64 * 0.05,
                &mut vehicle,
            )
            .unwrap();
        if sequence == 5 {
            let mut state = snapshot(body.clone(), sequence);
            state.vehicle = vehicle;
            acknowledged = Some(state);
        }
    }
    let expected = body.clone();
    let expected_vehicle = vehicle;
    prediction
        .reconcile_vehicle(
            &world,
            &mut body,
            &acknowledged.unwrap(),
            |_| vec![],
            0.25,
            &mut vehicle,
        )
        .unwrap();
    for axis in 0..3 {
        assert!((body.position[axis] - expected.position[axis]).abs() < 0.00001);
        assert!((body.velocity[axis] - expected.velocity[axis]).abs() < 0.00001);
    }
    assert_eq!(vehicle, expected_vehicle);
    let mut reset = snapshot(Body::new(world.spawn_position()), 0);
    reset.movement_epoch = 1;
    reset.vehicle = vehicle;
    prediction
        .reconcile_vehicle(&world, &mut body, &reset, |_| vec![], 1., &mut vehicle)
        .unwrap();
    assert_eq!(body.position, reset.body.position);
    assert_eq!(body.velocity, [0.; 3]);
    assert!(prediction.pending.is_empty());
}
