//! Optional pilot conversation and NPC standing-place availability.

use rubblekin_core::{
    airships::{AirshipRide, AirshipSnapshot, MAX_AIRSHIP_SEATS, ride_position},
    physics::character_position_is_clear,
    world::World,
};

pub(crate) const PORT_REACH: f32 = 14.0;

pub(crate) fn can_reach_pilot(ship: &AirshipSnapshot, position: [f32; 3]) -> bool {
    distance(position, rubblekin_core::airships::pilot_position(ship)) <= PORT_REACH
}

pub(crate) fn free_seat(
    world: &World,
    ship: &AirshipSnapshot,
    occupied: &[AirshipRide],
    obstacles: &[[f32; 3]],
) -> Option<AirshipRide> {
    (0..MAX_AIRSHIP_SEATS).find_map(|seat| {
        let ride = AirshipRide {
            ship_id: ship.id,
            seat,
        };
        (!occupied.contains(&ride)
            && character_position_is_clear(world, ride_position(ship, seat), obstacles))
        .then_some(ride)
    })
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt()
}
