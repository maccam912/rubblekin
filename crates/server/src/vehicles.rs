use crate::{Connection, persistence::Simulation};
use rubblekin_core::{
    protocol::{PlayerSnapshot, ServerMessage},
    vehicles::{self, VehicleAction},
};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub(crate) fn handle(
    id: u64,
    action: VehicleAction,
    connections: &mut BTreeMap<u64, Connection>,
    sim: &Simulation,
    obstacles: &[[f32; 3]],
) {
    let c = connections.get_mut(&id).unwrap();
    let now = Instant::now();
    if c.last_vehicle_request
        .is_some_and(|last| now.duration_since(last) < Duration::from_millis(200))
    {
        return;
    }
    c.last_vehicle_request = Some(now);
    let Some(player) = c.player.as_mut() else {
        return;
    };
    let result = apply(player, action, &sim.world, obstacles);
    let text = match result {
        Ok(text) => text,
        Err(reason) => reason.to_owned(),
    };
    c.send(&ServerMessage::Notice { text });
}
fn apply(
    player: &mut PlayerSnapshot,
    action: VehicleAction,
    world: &rubblekin_core::world::World,
    obstacles: &[[f32; 3]],
) -> Result<String, &'static str> {
    let epoch = player
        .movement_epoch
        .checked_add(1)
        .ok_or("Reconnect to reset movement.")?;
    let text = match action {
        VehicleAction::Spawn(kind) => {
            if player.glider_ride.is_some() || player.gliding || player.ride.is_some() {
                return Err("Land and leave your current ride first.");
            }
            let vehicle = vehicles::spawn(world, &mut player.body, kind, player.yaw, obstacles)?;
            player.vehicle = Some(vehicle);
            format!(
                "{}: steer with movement; Jump brakes. Menu → Vehicles → Leave vehicle.",
                kind.name()
            )
        }
        VehicleAction::Dismount => {
            if player.vehicle.take().is_none() {
                return Err("You are already on foot.");
            }
            player.body.velocity = [0.0; 3];
            "Vehicle put away. You are on foot.".to_owned()
        }
    };
    player.movement_epoch = epoch;
    player.last_input_sequence = 0;
    Ok(text)
}
