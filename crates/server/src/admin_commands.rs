use super::*;
use rubblekin_core::admin_commands::{
    ADMIN_COMMAND_HELP, AdminCommand, TeleportDestination, parse_admin_command,
};
use rubblekin_core::{
    airships::{
        AIRSHIP_DECK_HALF_LENGTH, AIRSHIP_DECK_HALF_WIDTH, AirshipRide, AirshipSnapshot,
        deck_local_position,
    },
    physics::character_position_is_clear_with_airships,
};

struct Landing {
    position: [f32; 3],
    ride: Option<AirshipRide>,
    deck_position: Option<[f32; 3]>,
}

impl Landing {
    fn detached(position: [f32; 3]) -> Self {
        Self {
            position,
            ride: None,
            deck_position: None,
        }
    }
}

pub(super) fn handle(
    id: u64,
    command: &str,
    connections: &mut BTreeMap<u64, Connection>,
    sim: &Simulation,
    airships: &AirshipNetwork,
    config: &ServerConfig,
) {
    let result = if !connections[&id].admin_enabled(config) {
        Err("Admin commands are disabled on this server".into())
    } else if !connections.get_mut(&id).unwrap().admit_admin_request() {
        Err("Commands are arriving too quickly; try again in a moment".into())
    } else {
        parse_admin_command(command)
            .and_then(|command| execute(id, command, connections, sim, airships))
    };
    connections
        .get_mut(&id)
        .unwrap()
        .send(&ServerMessage::AdminCommandResult {
            text: result.unwrap_or_else(|error| error),
        });
}

fn execute(
    id: u64,
    command: AdminCommand,
    connections: &mut BTreeMap<u64, Connection>,
    sim: &Simulation,
    airships: &AirshipNetwork,
) -> Result<String, String> {
    if command == AdminCommand::Wildlife {
        let ecology = &sim.ecology;
        let origin = connections[&id]
            .player
            .as_ref()
            .map_or(sim.world.spawn_position(), |p| p.body.position);
        let mut animals = ecology.snapshots();
        animals.sort_by(|a, b| {
            let distance = |p: [f32; 3]| (p[0] - origin[0]).hypot(p[2] - origin[2]);
            distance(a.position).total_cmp(&distance(b.position))
        });
        let mut text = format!(
            "Wildlife: {} rabbits, {} wolves in {} habitats\nBirths {} · hunted {} · other deaths {} · migrations {}",
            animals
                .iter()
                .filter(|a| a.species == rubblekin_core::wildlife::Species::Rabbit)
                .count(),
            animals
                .iter()
                .filter(|a| a.species == rubblekin_core::wildlife::Species::Wolf)
                .count(),
            ecology.habitats.len(),
            ecology.births,
            ecology.hunted,
            ecology.deaths,
            ecology.migrations
        );
        for species in [
            rubblekin_core::wildlife::Species::Rabbit,
            rubblekin_core::wildlife::Species::Wolf,
        ] {
            for a in animals.iter().filter(|a| a.species == species).take(2) {
                text.push_str(&format!(
                    "\n{} #{}: {:.2} {:.2} {:.2} · {}",
                    species.name(),
                    a.id,
                    a.position[0],
                    a.position[1],
                    a.position[2],
                    a.action.label()
                ));
            }
        }
        return Ok(text);
    }
    let AdminCommand::Teleport {
        player,
        destination,
    } = command
    else {
        return Ok(ADMIN_COMMAND_HELP.into());
    };
    let target_id = player
        .as_deref()
        .map(|name| player_id(connections, name))
        .transpose()?
        .unwrap_or(id);
    let obstacles = character_obstacles(connections, sim, Some(target_id), airships);
    let landing = match destination {
        TeleportDestination::Coordinates(position) => {
            if !character_position_is_clear_with_airships(
                &sim.world,
                position,
                &obstacles,
                airships,
                sim.world_time,
            ) {
                return Err(
                    "Those coordinates are outside the world or blocked by terrain, a character, or an airship deck/ramp. Y is the height of your feet in meters".into(),
                );
            }
            Landing::detached(position)
        }
        TeleportDestination::Player(name) => {
            let destination_id = player_id(connections, &name)?;
            let destination = connections[&destination_id].player.as_ref().unwrap();
            if let Some(ride) = destination.ride {
                let ship = airships
                    .ship(ride.ship_id, sim.world_time)
                    .ok_or_else(|| "The destination's airship is no longer available".to_owned())?;
                let origin = destination
                    .deck_position
                    .unwrap_or_else(|| deck_local_position(&ship, destination.body.position));
                nearby_ship_position(
                    &sim.world,
                    &ship,
                    origin,
                    &obstacles,
                    airships,
                    sim.world_time,
                )
                .ok_or_else(|| {
                    format!("There is no clear deck space beside {}", destination.name)
                })?
            } else {
                Landing::detached(
                    nearby_position(
                        &sim.world,
                        &destination.body,
                        &obstacles,
                        airships,
                        sim.world_time,
                    )
                    .ok_or_else(|| {
                        format!("There is no clear space beside {}", destination.name)
                    })?,
                )
            }
        }
    };
    let connection = connections.get_mut(&target_id).unwrap();
    let player = connection.player.as_mut().unwrap();
    player.vehicle = None;
    player.glider_ride = None;
    player.gliding = false;
    let movement_epoch = player.movement_epoch.checked_add(1).ok_or_else(|| {
        "This player's movement stream has reached its limit; reconnect".to_owned()
    })?;
    player.body = Body::new(landing.position);
    player.body.on_ground = landing.ride.is_some();
    player.ride = landing.ride;
    player.deck_position = landing.deck_position;
    player.movement_epoch = movement_epoch;
    player.last_input_sequence = 0;
    // Neutral idle gravity resumes only after the ordinary input timeout. The
    // player's next commands retain their client-owned creative-flight toggle.
    connection.last_input = Instant::now();
    Ok(format!(
        "Teleported {} to {:.2} {:.2} {:.2}",
        player.name, landing.position[0], landing.position[1], landing.position[2]
    ))
}

fn player_id(connections: &BTreeMap<u64, Connection>, name: &str) -> Result<u64, String> {
    let normalized = name.to_lowercase();
    let mut matches = connections
        .iter()
        .filter(|(_, connection)| !connection.dead && connection.closing_at.is_none())
        .filter(|(_, connection)| {
            connection
                .player
                .as_ref()
                .is_some_and(|player| player.name.to_lowercase() == normalized)
        })
        .map(|(&id, _)| id);
    let Some(id) = matches.next() else {
        return Err(format!("No connected player named {name}"));
    };
    if matches.next().is_some() {
        return Err(format!(
            "More than one connected player is named {name}. Choose distinct names and reconnect"
        ));
    }
    Ok(id)
}

fn nearby_ship_position(
    world: &World,
    ship: &AirshipSnapshot,
    origin: [f32; 3],
    obstacles: &[[f32; 3]],
    airships: &AirshipNetwork,
    time: f64,
) -> Option<Landing> {
    // An upright world-axis body can extend sqrt(2) radii in ship coordinates.
    let edge_margin = PLAYER_RADIUS * std::f32::consts::SQRT_2;
    for ring in 0_i32..=3 {
        for z in -ring..=ring {
            for x in -ring..=ring {
                if x.abs().max(z.abs()) != ring || x * x + z * z > 11 {
                    continue;
                }
                let local = [origin[0] + x as f32 * 0.9, 0.0, origin[2] + z as f32 * 0.9];
                if local[0].abs() > AIRSHIP_DECK_HALF_WIDTH - edge_margin
                    || local[2].abs() > AIRSHIP_DECK_HALF_LENGTH - edge_margin
                {
                    continue;
                }
                let position = deck_position(ship, local);
                // The shared deck controller reserves 0.8m so rotating AABBs
                // remain separate. A small margin covers kilometer rounding.
                if obstacles.iter().any(|other| {
                    position[1] + PLAYER_HEIGHT > other[1]
                        && other[1] + PLAYER_HEIGHT > position[1]
                        && (position[0] - other[0]).hypot(position[2] - other[2]) < 0.82
                }) {
                    continue;
                }
                if character_position_is_clear_with_airships(
                    world, position, obstacles, airships, time,
                ) {
                    return Some(Landing {
                        position,
                        ride: Some(AirshipRide {
                            ship_id: ship.id,
                            seat: u8::MAX,
                        }),
                        deck_position: Some(local),
                    });
                }
            }
        }
    }
    None
}

fn nearby_position(
    world: &World,
    destination: &Body,
    obstacles: &[[f32; 3]],
    airships: &AirshipNetwork,
    time: f64,
) -> Option<[f32; 3]> {
    let origin = destination.position;
    // Stay within three meters horizontally and preserve airborne height.
    // Grounded destinations require a neighboring floor within one meter.
    let supported = destination.on_ground;
    for ring in 0_i32..=4 {
        for z in -ring..=ring {
            for x in -ring..=ring {
                if x.abs().max(z.abs()) != ring {
                    continue;
                }
                if x * x + z * z > 16 {
                    continue;
                }
                let px = origin[0] + x as f32 * 0.75;
                let pz = origin[2] + z as f32 * 0.75;
                if supported {
                    let mut floor = world.min_y() as f32 * CELL_SIZE;
                    for dx in [-PLAYER_RADIUS, 0.0, PLAYER_RADIUS] {
                        for dz in [-PLAYER_RADIUS, 0.0, PLAYER_RADIUS] {
                            floor = floor.max(world.surface_height(px + dx, pz + dz));
                        }
                    }
                    let position = [px, floor + 0.02, pz];
                    if (position[1] - origin[1]).abs() <= 1.0
                        && character_position_is_clear_with_airships(
                            world, position, obstacles, airships, time,
                        )
                    {
                        return Some(position);
                    }
                } else {
                    let position = [px, origin[1], pz];
                    if character_position_is_clear_with_airships(
                        world, position, obstacles, airships, time,
                    ) {
                        return Some(position);
                    }
                }
            }
        }
    }
    None
}
