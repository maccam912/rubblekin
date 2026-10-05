use super::{airship_tests::Fixture, *};

fn input(f: &mut Fixture, id: u64, input: MoveInput) {
    let connection = f.connections.get_mut(&id).unwrap();
    // This direct-handler fixture advances simulation without waiting for wall
    // time. Existing socket tests cover the real wall-clock movement budget.
    connection.input_credit = MAX_INPUT_CREDIT;
    let sequence = connection.player.as_ref().unwrap().last_input_sequence + 1;
    f.send(
        id,
        ClientMessage::Input {
            sequence,
            dt: 0.05,
            input,
            yaw: 0.0,
        },
    );
}

#[test]
fn ordinary_contact_boards_without_dialog_and_moving_ship_keeps_the_actual_deck_position() {
    let mut f = Fixture::new();
    let ship = f.docked_ship();
    let position = deck_position(&ship, [-1.5, 0.02, -2.0]);
    f.add_player(1, position);
    input(&mut f, 1, MoveInput::default());
    let player = f.connections[&1].player.as_ref().unwrap();
    assert_eq!(player.ride.unwrap().ship_id, ship.id);
    assert_eq!(player.ride.unwrap().seat, u8::MAX);
    let local = player.deck_position.unwrap();
    assert!((local[0] + 1.5).abs() < 0.01);
    assert!(player.body.on_ground);
    assert!(
        f.connections[&1].outgoing.is_empty(),
        "No boarding dialogue or request is needed"
    );

    // Preserve a person's chosen spot rather than returning them to a seat.
    let start = deck_position(&ship, [0.0, 0.0, 0.0]);
    let target = deck_position(&ship, [0.0, 0.0, 1.0]);
    let direction = [target[0] - start[0], target[2] - start[2]];
    for _ in 0..4 {
        input(
            &mut f,
            1,
            MoveInput {
                direction,
                ..Default::default()
            },
        );
    }
    let moved = f.connections[&1]
        .player
        .as_ref()
        .unwrap()
        .deck_position
        .unwrap();
    assert!(moved[2] > local[2] + 0.5);
    f.sim.world_time = ship.departure_in as f64 + 15.0;
    carry_airship_players(&mut f.connections, &f.network, f.sim.world_time);
    let flying = f.network.ship(ship.id, f.sim.world_time).unwrap();
    let player = f.connections[&1].player.as_ref().unwrap();
    assert_eq!(player.deck_position, Some(moved));
    assert_eq!(player.body.position, deck_position(&flying, moved));
}

#[test]
fn ordinary_jump_and_walking_over_the_edge_detach_in_flight_without_an_exit_request() {
    for jumping in [false, true] {
        let mut f = Fixture::new();
        let docked = f.docked_ship();
        f.sim.world_time = docked.departure_in as f64 + 15.0;
        let ship = f.network.ship(docked.id, f.sim.world_time).unwrap();
        assert!(ship.docked_at.is_none());
        f.add_player(1, deck_position(&ship, [3.0, 0.02, 2.0]));
        input(&mut f, 1, MoveInput::default());
        assert!(f.connections[&1].player.as_ref().unwrap().ride.is_some());
        let from = deck_position(&ship, [0.0, 0.0, 0.0]);
        let to = deck_position(&ship, [1.0, 0.0, 0.0]);
        let direction = [to[0] - from[0], to[2] - from[2]];
        for step in 0..35 {
            f.sim.world_time += 0.05;
            carry_airship_players(&mut f.connections, &f.network, f.sim.world_time);
            input(
                &mut f,
                1,
                MoveInput {
                    direction,
                    jump: jumping && step == 0,
                    ..Default::default()
                },
            );
        }
        let player = f.connections[&1].player.as_ref().unwrap();
        assert!(
            player.ride.is_none(),
            "Walking/jumping beyond the deck must leave the ship"
        );
        assert!(player.deck_position.is_none());
        assert!(!player.body.on_ground);
        assert!(player.body.velocity[1] < 0.0);
        let previous = player.body.position;
        f.sim.world_time += 0.05;
        carry_airship_players(&mut f.connections, &f.network, f.sim.world_time);
        assert_eq!(
            f.connections[&1].player.as_ref().unwrap().body.position,
            previous,
            "The departing ship cannot carry a detached character"
        );
    }
}
