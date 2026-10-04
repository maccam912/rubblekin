//! A direct, authoritative prototype server. Message handling, validation and
//! simulation are intentionally visible in one place; there is no event bus.

mod npc;
mod persistence;

pub use npc::berry_patch_positions;

use std::{
    collections::{BTreeMap, VecDeque},
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use persistence::{MAX_EDITS, Simulation};
use rubblekin_core::{
    physics::{Body, EYE_HEIGHT, MoveInput, PLAYER_HEIGHT, PLAYER_RADIUS, move_character},
    protocol::*,
    world::{Block, BlockEdit, BlockPos, CELL_SIZE, World},
};

const TICK: Duration = Duration::from_millis(50);
const DT: f32 = 0.05;
const MAX_CLIENTS: usize = 32;
const MAX_OUTBOUND_BYTES: usize = 8 * 1024 * 1024;
// One maximum-length frame plus a bounded allowance for network batching.
const MAX_INPUT_CREDIT: f64 = 0.5;
const EDIT_REACH: f32 = 7.0;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_addr: String,
    pub save_path: PathBuf,
    pub seed: u32,
    /// Grants every connected client developer controls. For trusted servers only.
    pub allow_admin: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:7878".into(),
            save_path: "saves/world.json".into(),
            seed: 42,
            allow_admin: false,
        }
    }
}

pub struct ServerHandle {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<io::Result<()>>>,
}

impl ServerHandle {
    pub fn stop(mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        self.join()
    }

    /// Useful for a dedicated server: waits until the loop exits or fails.
    pub fn wait(mut self) -> io::Result<()> {
        self.join()
    }

    pub fn is_finished(&self) -> bool {
        self.thread
            .as_ref()
            .is_none_or(|thread| thread.is_finished())
    }

    fn join(&mut self) -> io::Result<()> {
        self.thread.take().map_or(Ok(()), |thread| {
            thread
                .join()
                .map_err(|_| io::Error::other("Server thread panicked"))?
        })
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Err(error) = self.join() {
            eprintln!("Server shutdown failed: {error}");
        }
    }
}

/// Binds and validates the save synchronously, so callers get startup failures.
pub fn spawn(config: ServerConfig) -> io::Result<ServerHandle> {
    let listener = TcpListener::bind(&config.bind_addr)?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let save_lock = persistence::lock_save(&config.save_path)?;
    let simulation = Simulation::load(&config.save_path, config.seed)?;
    simulation.save(&config.save_path)?;
    let stop = Arc::new(AtomicBool::new(false));
    let loop_stop = stop.clone();
    let thread = thread::Builder::new()
        .name("rubblekin-server".into())
        .spawn(move || {
            let _save_lock = save_lock;
            let result = run(listener, simulation, &config, &loop_stop);
            if let Err(error) = &result {
                eprintln!("Authoritative server stopped: {error}");
            }
            result
        })?;
    Ok(ServerHandle {
        addr,
        stop,
        thread: Some(thread),
    })
}

struct Connection {
    socket: TcpStream,
    incoming: Vec<u8>,
    outgoing: VecDeque<Vec<u8>>,
    write_offset: usize,
    queued_bytes: usize,
    player: Option<PlayerSnapshot>,
    connected_at: Instant,
    last_input: Instant,
    input_credit: f64,
    credit_updated: Instant,
    last_edit: Option<Instant>,
    dead: bool,
}

impl Connection {
    fn new(socket: TcpStream) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        socket.set_nodelay(true)?;
        let now = Instant::now();
        Ok(Self {
            socket,
            incoming: Vec::new(),
            outgoing: VecDeque::new(),
            write_offset: 0,
            queued_bytes: 0,
            player: None,
            connected_at: now,
            last_input: now,
            input_credit: MAX_INPUT_CREDIT,
            credit_updated: now,
            last_edit: None,
            dead: false,
        })
    }

    fn receive(&mut self) -> io::Result<Vec<ClientMessage>> {
        let mut buffer = [0_u8; 4096];
        for _ in 0..4 {
            match self.socket.read(&mut buffer) {
                Ok(0) => {
                    self.dead = true;
                    break;
                }
                Ok(count) => {
                    self.incoming.extend_from_slice(&buffer[..count]);
                    if self.incoming.len() > MAX_MESSAGE_BYTES {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Input buffer limit exceeded",
                        ));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        let mut messages = Vec::new();
        // Per-frame commands can arrive together between the 20 Hz server ticks.
        // Work and receive storage remain bounded even for a flooding client.
        for _ in 0..64 {
            let Some(end) = self.incoming.iter().position(|&byte| byte == b'\n') else {
                break;
            };
            let message = serde_json::from_slice(&self.incoming[..end])
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            self.incoming.drain(..=end);
            messages.push(message);
        }
        Ok(messages)
    }

    fn send(&mut self, message: &ServerMessage) {
        match serde_json::to_vec(message) {
            Ok(mut bytes) => {
                bytes.push(b'\n');
                if self.queued_bytes + bytes.len() > MAX_OUTBOUND_BYTES {
                    self.dead = true;
                    return;
                }
                self.queued_bytes += bytes.len();
                self.outgoing.push_back(bytes);
            }
            Err(error) => {
                eprintln!("Failed to encode server message: {error}");
                self.dead = true;
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut budget = 256 * 1024;
        while budget > 0 {
            let Some(bytes) = self.outgoing.front() else {
                break;
            };
            let end = bytes.len().min(self.write_offset + budget);
            match self.socket.write(&bytes[self.write_offset..end]) {
                Ok(0) => {
                    self.dead = true;
                    break;
                }
                Ok(count) => {
                    self.write_offset += count;
                    self.queued_bytes -= count;
                    budget -= count;
                    if self.write_offset == bytes.len() {
                        self.outgoing.pop_front();
                        self.write_offset = 0;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

fn run(
    listener: TcpListener,
    mut sim: Simulation,
    config: &ServerConfig,
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut connections = BTreeMap::<u64, Connection>::new();
    let mut next_id = 1_u64;
    let mut last_save = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let tick_started = Instant::now();
        for _ in 0..4 {
            match listener.accept() {
                Ok((socket, _)) if connections.len() < MAX_CLIENTS => {
                    if let Ok(connection) = Connection::new(socket) {
                        connections.insert(next_id, connection);
                        next_id += 1;
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        let mut inbox = Vec::new();
        for (&id, connection) in &mut connections {
            match connection.receive() {
                Ok(messages) => inbox.extend(messages.into_iter().map(|message| (id, message))),
                Err(_) => connection.dead = true,
            }
            if connection.player.is_none()
                && connection.connected_at.elapsed() > Duration::from_secs(5)
            {
                connection.dead = true;
            }
        }
        let mut edit_budget = 16;
        for (id, message) in inbox {
            if connections.get(&id).is_none_or(|client| client.dead) {
                continue;
            }
            handle_message(
                id,
                message,
                &mut connections,
                &mut sim,
                config,
                &mut edit_budget,
            )?;
        }
        connections.retain(|_, client| !client.dead);
        for connection in connections.values_mut() {
            if let Some(player) = &mut connection.player
                && connection.last_input.elapsed() > Duration::from_millis(500)
            {
                // Never extrapolate walking or a held jump. A stalled client
                // eventually resumes neutral gravity instead of floating.
                move_character(&sim.world, &mut player.body, MoveInput::default(), DT);
                // Idle simulation spends time too; retain only the bounded
                // reserve needed to accept a resumed long client frame.
                let now = Instant::now();
                connection.input_credit = (connection.input_credit
                    + now.duration_since(connection.credit_updated).as_secs_f64())
                .min(MAX_INPUT_CREDIT)
                    - DT as f64;
                connection.input_credit = connection.input_credit.max(0.0);
                connection.credit_updated = now;
            }
        }
        sim.npc.tick(&sim.world, DT);
        sim.world_time += DT as f64;
        let state = ServerMessage::State {
            players: players(&connections),
            npc: sim.npc.snapshot.clone(),
            world_time: sim.world_time,
        };
        broadcast(&mut connections, &state);
        if last_save.elapsed() >= Duration::from_secs(5) {
            sim.save(&config.save_path)?;
            last_save = Instant::now();
        }
        for connection in connections.values_mut() {
            if connection.flush().is_err() {
                connection.dead = true;
            }
        }
        connections.retain(|_, client| !client.dead);
        // Deliberately no unbounded catch-up loop when the server is overloaded.
        if let Some(remaining) = TICK.checked_sub(tick_started.elapsed()) {
            thread::sleep(remaining);
        }
    }
    sim.save(&config.save_path)
}

fn players(connections: &BTreeMap<u64, Connection>) -> Vec<PlayerSnapshot> {
    connections
        .values()
        .filter(|c| !c.dead)
        .filter_map(|c| c.player.clone())
        .collect()
}

fn broadcast(connections: &mut BTreeMap<u64, Connection>, message: &ServerMessage) {
    for connection in connections
        .values_mut()
        .filter(|c| c.player.is_some() && !c.dead)
    {
        connection.send(message);
    }
}

fn reject(
    connections: &mut BTreeMap<u64, Connection>,
    id: u64,
    request_id: u64,
    reason: impl Into<String>,
) {
    if let Some(connection) = connections.get_mut(&id) {
        connection.send(&ServerMessage::Rejected {
            request_id,
            reason: reason.into(),
        });
    }
}

fn handle_message(
    id: u64,
    message: ClientMessage,
    connections: &mut BTreeMap<u64, Connection>,
    sim: &mut Simulation,
    config: &ServerConfig,
    edit_budget: &mut usize,
) -> io::Result<()> {
    if let ClientMessage::Hello { version, name } = message {
        let connection = connections.get_mut(&id).unwrap();
        if version != PROTOCOL_VERSION || connection.player.is_some() {
            connection.dead = true;
            return Ok(());
        }
        let name: String = name
            .chars()
            .filter(|ch| !ch.is_control())
            .take(24)
            .collect();
        let name = if name.trim().is_empty() {
            format!("Player {id}")
        } else {
            name
        };
        connection.player = Some(PlayerSnapshot {
            id,
            name,
            body: Body::new(sim.world.spawn_position()),
            yaw: 0.0,
            last_input_sequence: 0,
        });
        let welcome = ServerMessage::Welcome {
            version: PROTOCOL_VERSION,
            player_id: id,
            seed: sim.world.seed,
            edits: sim.world.edits(),
            players: players(connections),
            npc: sim.npc.snapshot.clone(),
            world_time: sim.world_time,
            can_admin: config.allow_admin,
        };
        connections.get_mut(&id).unwrap().send(&welcome);
        return Ok(());
    }
    if connections.get(&id).is_none_or(|c| c.player.is_none()) {
        connections.get_mut(&id).unwrap().dead = true;
        return Ok(());
    }
    match message {
        ClientMessage::Input {
            sequence,
            dt,
            input,
            yaw,
        } => {
            let connection = connections.get_mut(&id).unwrap();
            let player = connection.player.as_mut().unwrap();
            if !yaw.is_finite()
                || !input.vertical.is_finite()
                || !input.direction.iter().all(|n| n.is_finite())
                || !dt.is_finite()
                || dt <= 0.0
                || dt > MAX_INPUT_DT
                || player.last_input_sequence.checked_add(1) != Some(sequence)
            {
                connection.dead = true;
                return Ok(());
            }

            // The server owns the time budget. Allow a bounded network burst,
            // but neither unlimited catch-up nor faster-than-real-time input.
            let now = Instant::now();
            connection.input_credit = (connection.input_credit
                + now.duration_since(connection.credit_updated).as_secs_f64())
            .min(MAX_INPUT_CREDIT);
            connection.credit_updated = now;
            if dt as f64 > connection.input_credit + 0.000_001 {
                connection.dead = true;
                return Ok(());
            }
            connection.input_credit = (connection.input_credit - dt as f64).max(0.0);
            // Use exactly the client's command boundaries and shared controller:
            // even a second direction normalization can change collision results.
            move_character(&sim.world, &mut player.body, input, dt);
            player.last_input_sequence = sequence;
            player.yaw = yaw.rem_euclid(std::f32::consts::TAU);
            connection.last_input = now;
        }
        ClientMessage::Edit {
            request_id,
            position,
            block,
        } => {
            let connection = connections.get_mut(&id).unwrap();
            if *edit_budget == 0
                || connection
                    .last_edit
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(100))
            {
                reject(
                    connections,
                    id,
                    request_id,
                    "Building too quickly; try again in a moment",
                );
                return Ok(());
            }
            connection.last_edit = Some(Instant::now());
            *edit_budget -= 1;
            let actor = connection.player.as_ref().unwrap().body.clone();
            let result = validate_edit(
                &sim.world,
                &actor,
                position,
                block,
                players(connections)
                    .iter()
                    .map(|p| p.body.position)
                    .chain(std::iter::once(sim.npc.snapshot.position)),
            );
            if let Err(reason) = result {
                reject(connections, id, request_id, reason);
                return Ok(());
            }
            let edits = sim.world.edits();
            if edits.len() >= MAX_EDITS && !edits.iter().any(|edit| edit.position == position) {
                reject(
                    connections,
                    id,
                    request_id,
                    "This prototype world has reached its saved edit limit",
                );
                return Ok(());
            }
            if let Err(reason) = sim.world.set_block(position, block) {
                reject(connections, id, request_id, reason);
                return Ok(());
            }
            // If saving fails, terminate visibly instead of acknowledging an edit
            // that might be lost, or continuing with disk and memory disagreeing.
            sim.save(&config.save_path)?;
            broadcast(
                connections,
                &ServerMessage::BlockChanged {
                    request_id,
                    player_id: id,
                    edit: BlockEdit { position, block },
                },
            );
        }
        ClientMessage::Admin { action } => {
            let result = if config.allow_admin {
                sim.npc.admin(&sim.world, action)
            } else {
                Err("Developer controls are disabled on this server".into())
            };
            let text = match result {
                Ok(()) => {
                    sim.save(&config.save_path)?;
                    "NPC settings updated".to_owned()
                }
                Err(reason) => reason,
            };
            connections
                .get_mut(&id)
                .unwrap()
                .send(&ServerMessage::Notice { text });
        }
        ClientMessage::Ping => connections.get_mut(&id).unwrap().send(&ServerMessage::Pong),
        ClientMessage::Hello { .. } => unreachable!(),
    }
    Ok(())
}

fn validate_edit(
    world: &World,
    actor: &Body,
    position: BlockPos,
    block: Block,
    occupied: impl Iterator<Item = [f32; 3]>,
) -> Result<(), String> {
    if !World::is_editable(position) {
        return Err("That block is outside the editable valley".into());
    }
    if !actor.position.iter().all(|n| n.is_finite()) {
        return Err("Invalid player position".into());
    }
    // The prototype uses half-meter terrain cells. This is gameplay geometry,
    // independent of the renderer's chunk size or mesh representation.
    let min = [
        position.x as f32 * CELL_SIZE,
        position.y as f32 * CELL_SIZE,
        position.z as f32 * CELL_SIZE,
    ];
    let center = min.map(|v| v + CELL_SIZE * 0.5);
    let eye = [
        actor.position[0],
        actor.position[1] + EYE_HEIGHT,
        actor.position[2],
    ];
    let existing = world.block(position);
    // A visible top face can have a hidden center (e.g. digging flat ground
    // at a shallow angle). Check actual surface points just inside the cell.
    const INSIDE: f32 = 0.001;
    let nearest = std::array::from_fn(|axis| {
        eye[axis].clamp(min[axis] + INSIDE, min[axis] + CELL_SIZE - INSIDE)
    });
    let mut points = vec![nearest, center];
    for axis in 0..3 {
        for side in [INSIDE, CELL_SIZE - INSIDE] {
            let mut face = center;
            face[axis] = min[axis] + side;
            points.push(face);
        }
    }
    let mut in_reach = false;
    let visible = points.into_iter().any(|point| {
        let direction: [f32; 3] = std::array::from_fn(|axis| point[axis] - eye[axis]);
        let distance = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
        if distance > EDIT_REACH {
            return false;
        }
        in_reach = true;
        match world.raycast(eye, direction, distance + INSIDE * 0.5) {
            Some(hit) => hit.position == position,
            None => existing == Block::Air,
        }
    });
    if !in_reach {
        return Err("That block is out of reach".into());
    }
    if existing == block {
        return Err("That block already has this material".into());
    }
    if !visible {
        return Err("Another block obstructs the edit".into());
    }
    if block != Block::Air {
        for feet in occupied {
            if min[0] < feet[0] + PLAYER_RADIUS
                && min[0] + CELL_SIZE > feet[0] - PLAYER_RADIUS
                && min[1] < feet[1] + PLAYER_HEIGHT
                && min[1] + CELL_SIZE > feet[1]
                && min[2] < feet[2] + PLAYER_RADIUS
                && min[2] + CELL_SIZE > feet[2] - PLAYER_RADIUS
            {
                return Err("A character is standing in that space".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_require_visible_space_and_world_bounds() {
        let mut world = World::new(42);
        let actor = Body::new(world.spawn_position());
        let target = BlockPos::new(4, 8, 0);
        let validate = |world: &World, position| {
            validate_edit(
                world,
                &actor,
                position,
                Block::Brick,
                std::iter::once(actor.position),
            )
        };
        assert!(validate(&world, target).is_ok());
        for y in 5..=10 {
            for z in -3..=3 {
                world
                    .set_block(BlockPos::new(2, y, z), Block::Stone)
                    .unwrap();
            }
        }
        assert!(validate(&world, target).unwrap_err().contains("obstructs"));
        world.set_block(BlockPos::new(2, 8, 0), Block::Air).unwrap();
        assert!(
            validate(&world, target).is_ok(),
            "An open sight line allows the edit"
        );
        assert!(
            validate(&world, BlockPos::new(i32::MAX, 8, 0))
                .unwrap_err()
                .contains("outside")
        );
    }

    #[test]
    fn visible_ground_can_be_dug_at_a_shallow_angle() {
        let world = World::new(42);
        let actor = Body::new(world.spawn_position());
        let target = BlockPos::new(10, 4, 0);
        assert_eq!(world.block(target), Block::Grass);
        assert!(validate_edit(&world, &actor, target, Block::Air, std::iter::empty()).is_ok());
    }
}
