use super::*;
use rubblekin_core::physics::{characters_overlap, move_character};
use rubblekin_core::world::{Block, BlockPos};

fn snapshot(body: Body, last_input_sequence: u64) -> PlayerSnapshot {
    PlayerSnapshot {
        id: 1,
        name: "Walker".into(),
        body,
        yaw: 0.0,
        last_input_sequence,
    }
}

fn execute(world: &World, server: &mut PlayerSnapshot, message: ClientMessage) {
    let ClientMessage::Input {
        sequence,
        input,
        yaw,
        dt,
    } = message
    else {
        panic!("expected movement")
    };
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
