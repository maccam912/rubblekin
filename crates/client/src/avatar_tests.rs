use super::*;
use rubblekin_core::physics::characters_overlap;

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
