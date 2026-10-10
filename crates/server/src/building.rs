use crate::*;
use rubblekin_core::building::{FILL_REACH, Selection};

#[allow(clippy::too_many_arguments)]
pub(crate) fn fill(
    id: u64,
    request_id: u64,
    first: BlockPos,
    second: BlockPos,
    block: Block,
    connections: &mut BTreeMap<u64, Connection>,
    sim: &mut Simulation,
    config: &ServerConfig,
    budget: &mut usize,
) -> io::Result<()> {
    let c = connections.get_mut(&id).unwrap();
    if *budget == 0
        || c.last_edit
            .is_some_and(|t| t.elapsed() < Duration::from_millis(200))
    {
        reject(
            connections,
            id,
            request_id,
            "Building too quickly; try again in a moment",
        );
        return Ok(());
    }
    c.last_edit = Some(Instant::now());
    *budget -= 1;
    let actor = c.player.as_ref().unwrap().clone();
    let selection = match Selection::new(first, second) {
        Ok(s) => s,
        Err(reason) => {
            reject(connections, id, request_id, reason);
            return Ok(());
        }
    };
    let result = validate(
        &sim.world,
        &actor,
        selection,
        block,
        players(connections, sim)
            .iter()
            .map(|p| p.body.position)
            .chain(std::iter::once(sim.npc.snapshot.position))
            .chain(sim.villages.positions()),
    );
    let edits = match result {
        Ok(e) => e,
        Err(reason) => {
            reject(connections, id, request_id, reason);
            return Ok(());
        }
    };
    // Stage the complete transaction before changing the authoritative world.
    let mut world = sim.world.clone();
    for e in &edits {
        world
            .set_block(e.position, e.block)
            .map_err(io::Error::other)?;
    }
    if world.edits().len() > MAX_EDITS {
        reject(
            connections,
            id,
            request_id,
            "This prototype world has reached its saved edit limit",
        );
        return Ok(());
    }
    sim.world = world;
    sim.save(&config.save_path)?;
    broadcast(
        connections,
        &ServerMessage::BlocksChanged {
            request_id,
            player_id: id,
            edits,
        },
    );
    activities::broadcast(connections, sim);
    Ok(())
}

fn validate(
    world: &World,
    player: &PlayerSnapshot,
    selection: Selection,
    block: Block,
    occupied: impl Iterator<Item = [f32; 3]>,
) -> Result<Vec<BlockEdit>, String> {
    if player.glider_ride.is_some() || player.ride.is_some() || player.vehicle.is_some() {
        return Err("Leave your ride before filling a selection".into());
    }
    let feet = player.body.position;
    if !feet.iter().all(|n| n.is_finite()) {
        return Err("Invalid player position".into());
    }
    let characters: Vec<_> = occupied.collect();
    let mut edits = Vec::new();
    for p in selection.cells() {
        if !world.contains_block(p) {
            return Err("The selection extends outside the editable world".into());
        }
        let min = [
            p.x as f32 * CELL_SIZE,
            p.y as f32 * CELL_SIZE,
            p.z as f32 * CELL_SIZE,
        ];
        let distance = min
            .iter()
            .zip(feet)
            .map(|(a, b)| (a + CELL_SIZE * 0.5 - b).powi(2))
            .sum::<f32>()
            .sqrt();
        if distance > FILL_REACH {
            return Err("Stay within 64 m of the entire selection to fill it".into());
        }
        if world.block(p) == block {
            continue;
        }
        if block.is_solid()
            && characters.iter().any(|c| {
                min[0] < c[0] + PLAYER_RADIUS
                    && min[0] + CELL_SIZE > c[0] - PLAYER_RADIUS
                    && min[1] < c[1] + PLAYER_HEIGHT
                    && min[1] + CELL_SIZE > c[1]
                    && min[2] < c[2] + PLAYER_RADIUS
                    && min[2] + CELL_SIZE > c[2] - PLAYER_RADIUS
            })
        {
            return Err("A character is inside the selection; move clear before filling".into());
        }
        edits.push(BlockEdit { position: p, block });
    }
    if edits.is_empty() {
        return Err("The selection already has this material".into());
    }
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::airship_tests::Fixture;
    #[test]
    fn bulk_fill_is_shared_durable_and_rejection_is_atomic() {
        let mut f = Fixture::new();
        f.config.save_path = std::env::temp_dir().join(format!(
            "rubblekin-fill-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let s = f.sim.world.spawn_position();
        f.add_player(1, s);
        f.add_player(2, [s[0] + 12., s[1], s[2]]);
        let first = BlockPos::new(
            (s[0] / CELL_SIZE) as i32 + 8,
            (s[1] / CELL_SIZE) as i32 + 8,
            (s[2] / CELL_SIZE) as i32,
        );
        let second = BlockPos::new(first.x + 7, first.y + 1, first.z + 7);
        f.send(
            1,
            ClientMessage::Fill {
                request_id: 41,
                first,
                second,
                block: Block::Brick,
            },
        );
        assert_eq!(f.sim.world.block(second), Block::Brick);
        for id in [1, 2] {
            assert!(f.connections[&id].outgoing.iter().any(|o| matches!(
                serde_json::from_slice::<ServerMessage>(&o.bytes).unwrap(),
                ServerMessage::BlocksChanged { request_id:41,ref edits,.. } if edits.len()==128)));
        }
        let restored = Simulation::load(&f.config.save_path, 42, f.sim.world.generation()).unwrap();
        assert_eq!(restored.world.block(first), Block::Brick);
        let before = f.sim.world.edits();
        f.connections.get_mut(&1).unwrap().last_edit = None;
        let p = f.connections[&2].player.as_ref().unwrap().body.position;
        let p = BlockPos::new(
            (p[0] / CELL_SIZE).floor() as i32,
            (p[1] / CELL_SIZE).floor() as i32,
            (p[2] / CELL_SIZE).floor() as i32,
        );
        f.send(
            1,
            ClientMessage::Fill {
                request_id: 42,
                first: p,
                second: BlockPos::new(p.x + 2, p.y + 2, p.z + 2),
                block: Block::Brick,
            },
        );
        assert_eq!(f.sim.world.edits(), before);
        std::fs::remove_file(&f.config.save_path).unwrap();
        f.connections.get_mut(&1).unwrap().mode = Some(SessionMode::Observer);
        f.send(
            1,
            ClientMessage::Fill {
                request_id: 43,
                first,
                second,
                block: Block::Glass,
            },
        );
        assert_eq!(f.sim.world.edits(), before);
    }
}
