//! A direct, authoritative prototype server. Message handling, validation and
//! simulation are intentionally visible in one place; there is no event bus.

mod admin_commands;
mod airships;
mod local_work;
mod navigation;
mod npc;
mod persistence;
mod player_economy;
mod quarry_work;
mod villages;

#[cfg(test)]
mod admin_command_tests;
#[cfg(test)]
mod airship_tests;
#[cfg(test)]
mod deck_tests;

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
    airships::{AirshipNetwork, deck_position, initial_deck_position, pilot_position},
    economy::{MarketAction, PlayerEconomy, WorkAction, WorkKind, WorkState},
    physics::{
        Body, EYE_HEIGHT, MoveInput, PLAYER_HEIGHT, PLAYER_RADIUS, character_position_is_clear,
        move_character_with_airships,
    },
    protocol::*,
    world::{Block, BlockEdit, BlockPos, CELL_SIZE, World, WorldGeneration},
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
    /// Used only for a new save. Existing worlds keep their generation version.
    pub generation: WorldGeneration,
    /// Grants players developer controls and permits read-only observer sessions.
    /// For trusted servers only.
    pub allow_admin: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:7878".into(),
            save_path: "saves/world.json".into(),
            seed: 42,
            generation: WorldGeneration::GeographyV6,
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
    let mut simulation = Simulation::load(&config.save_path, config.seed, config.generation)?;
    let airships = AirshipNetwork::try_new(&simulation.world)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    simulation
        .villages
        .sync_airship_riders(&airships, simulation.world_time);
    // Older village saves can contain residents at the same work/home point.
    // Separate them against terrain before saving or welcoming the first client.
    simulation
        .villages
        .resolve_overlaps(&simulation.world, &[simulation.npc.snapshot.position]);
    simulation.npc.resolve_overlaps(
        &simulation.world,
        &simulation.villages.positions().collect::<Vec<_>>(),
    );
    let npc_positions: Vec<_> = std::iter::once(simulation.npc.snapshot.position)
        .chain(simulation.villages.positions())
        .collect();
    if npc_positions.iter().enumerate().any(|(index, position)| {
        let obstacles: Vec<_> = npc_positions
            .iter()
            .enumerate()
            .filter_map(|(other, position)| (other != index).then_some(*position))
            .collect();
        !character_position_is_clear(&simulation.world, *position, &obstacles)
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Cannot recover NPC collision with terrain or another character; original save left untouched",
        ));
    }
    simulation.save(&config.save_path)?;
    let stop = Arc::new(AtomicBool::new(false));
    let loop_stop = stop.clone();
    let thread = thread::Builder::new()
        .name("rubblekin-server".into())
        .spawn(move || {
            let _save_lock = save_lock;
            let result = run(listener, simulation, airships, &config, &loop_stop);
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
    mode: Option<SessionMode>,
    player: Option<PlayerSnapshot>,
    profile_id: Option<String>,
    last_market_request: Option<Instant>,
    last_market_view: Option<Instant>,
    last_work_view: Option<Instant>,
    active_work: Option<local_work::ActiveWork>,
    last_work_action_id: Option<u64>,
    connected_at: Instant,
    closing_at: Option<Instant>,
    last_input: Instant,
    input_credit: f64,
    credit_updated: Instant,
    last_edit: Option<Instant>,
    last_admin_request: Option<Instant>,
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
            mode: None,
            player: None,
            profile_id: None,
            last_market_request: None,
            last_market_view: None,
            last_work_view: None,
            active_work: None,
            last_work_action_id: None,
            connected_at: now,
            closing_at: None,
            last_input: now,
            input_credit: MAX_INPUT_CREDIT,
            credit_updated: now,
            last_edit: None,
            last_admin_request: None,
            dead: false,
        })
    }

    fn admit_admin_request(&mut self) -> bool {
        let now = Instant::now();
        if self
            .last_admin_request
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(100))
        {
            return false;
        }
        self.last_admin_request = Some(now);
        true
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
        if self.closing_at.is_some() && self.outgoing.is_empty() {
            self.dead = true;
        }
        Ok(())
    }

    fn close_with_notice(&mut self, text: String) {
        self.send(&ServerMessage::Notice { text });
        self.closing_at = Some(Instant::now());
    }
}

fn run(
    listener: TcpListener,
    mut sim: Simulation,
    airships: AirshipNetwork,
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
            if connection.closing_at.is_none() {
                match connection.receive() {
                    Ok(messages) => inbox.extend(messages.into_iter().map(|message| (id, message))),
                    Err(error) => {
                        eprintln!(
                            "Session {id} disconnected while reading client messages: {error}"
                        );
                        connection.dead = true;
                    }
                }
            }
            if (connection.mode.is_none()
                && connection.connected_at.elapsed() > Duration::from_secs(5))
                || connection
                    .closing_at
                    .is_some_and(|started| started.elapsed() > Duration::from_secs(5))
            {
                connection.dead = true;
            }
        }
        let mut edit_budget = 16;
        checkpoint_players(&connections, &mut sim);
        for (id, message) in inbox {
            if connections
                .get(&id)
                .is_none_or(|client| client.dead || client.closing_at.is_some())
            {
                continue;
            }
            handle_message(
                id,
                message,
                &mut connections,
                &mut sim,
                &airships,
                config,
                &mut edit_budget,
            )?;
        }
        // A busy tick can accumulate more than half a second of legitimate
        // input. Spend its actual elapsed time across the bounded receive
        // batch before limiting the reserve kept for the next batch.
        for connection in connections.values_mut() {
            connection.input_credit = connection.input_credit.min(MAX_INPUT_CREDIT);
        }
        save_disconnected_players(&connections, &mut sim, config)?;
        connections.retain(|_, client| !client.dead);
        let next_world_time = sim.world_time + DT as f64;
        sim.villages.sync_airship_riders(&airships, next_world_time);
        carry_airship_players(&mut connections, &airships, next_world_time);
        sim.world_time = next_world_time;
        let player_ids: Vec<_> = connections
            .iter()
            .filter(|(_, connection)| connection.player.is_some())
            .map(|(&id, _)| id)
            .collect();
        for id in player_ids {
            let obstacles = character_obstacles(&connections, &sim, Some(id), &airships);
            let connection = connections.get_mut(&id).unwrap();
            if let Some(player) = &mut connection.player
                && connection.last_input.elapsed() > Duration::from_millis(500)
            {
                // Never extrapolate walking or a held jump. A stalled client
                // eventually resumes neutral gravity instead of floating.
                move_character_with_airships(
                    &sim.world,
                    &mut player.body,
                    MoveInput::default(),
                    DT,
                    &obstacles,
                    &airships,
                    sim.world_time,
                    &mut player.ride,
                    &mut player.deck_position,
                );
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
        let player_positions: Vec<_> = players(&connections)
            .iter()
            .map(|player| player.body.position)
            .collect();
        let mut npc_obstacles = player_positions.clone();
        npc_obstacles.extend(sim.villages.positions());
        sim.npc.tick_with_obstacles(&sim.world, DT, &npc_obstacles);
        let mut resident_obstacles = player_positions;
        resident_obstacles.push(sim.npc.snapshot.position);
        let passenger_seats: Vec<_> = connections
            .values()
            .filter_map(|connection| connection.player.as_ref().and_then(|player| player.ride))
            .collect();
        sim.villages.tick_with_transport(
            &sim.world,
            DT,
            &resident_obstacles,
            &airships,
            next_world_time,
            &passenger_seats,
        );
        sim.world_time = next_world_time;
        advance_local_work(&mut connections, &mut sim, config)?;
        checkpoint_players(&connections, &mut sim);
        let state = ServerMessage::State {
            players: players(&connections),
            npc: sim.npc.snapshot.clone(),
            residents: sim.villages.residents(),
            villages: sim.villages.villages(),
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
        save_disconnected_players(&connections, &mut sim, config)?;
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

fn checkpoint_players(connections: &BTreeMap<u64, Connection>, sim: &mut Simulation) {
    for connection in connections.values() {
        if let (Some(id), Some(player)) = (&connection.profile_id, &connection.player)
            && let Some(saved) = sim.profiles.get_mut(id)
        {
            saved.checkpoint(player);
        }
    }
}

fn save_disconnected_players(
    connections: &BTreeMap<u64, Connection>,
    sim: &mut Simulation,
    config: &ServerConfig,
) -> io::Result<()> {
    if connections
        .values()
        .any(|c| c.dead && c.profile_id.is_some() && c.player.is_some())
    {
        checkpoint_players(connections, sim);
        sim.save(&config.save_path)?;
    }
    Ok(())
}

fn character_obstacles(
    connections: &BTreeMap<u64, Connection>,
    sim: &Simulation,
    exclude_player: Option<u64>,
    airships: &AirshipNetwork,
) -> Vec<[f32; 3]> {
    connections
        .iter()
        .filter(|(id, connection)| Some(**id) != exclude_player && !connection.dead)
        .filter_map(|(_, connection)| connection.player.as_ref().map(|p| p.body.position))
        .chain(std::iter::once(sim.npc.snapshot.position))
        .chain(sim.villages.positions())
        .chain(airships.ships(sim.world_time).iter().map(pilot_position))
        .collect()
}

fn carry_airship_players(
    connections: &mut BTreeMap<u64, Connection>,
    network: &AirshipNetwork,
    time: f64,
) {
    for connection in connections.values_mut() {
        if let Some(player) = &mut connection.player
            && let Some(ride) = player.ride
            && let Some(ship) = network.ship(ride.ship_id, time)
        {
            let local = *player
                .deck_position
                .get_or_insert_with(|| initial_deck_position(ride.seat));
            player.body.position = deck_position(&ship, local);
        }
    }
}

fn free_player_spawn(world: &World, obstacles: &[[f32; 3]]) -> Option<[f32; 3]> {
    let origin = world.spawn_position();
    // Bounded nearest-first search keeps reconnects beside spawn while leaving
    // existing characters in place. Observer sessions have no physical body.
    for ring in 0_i32..=16 {
        for z in -ring..=ring {
            for x in -ring..=ring {
                if x.abs().max(z.abs()) != ring {
                    continue;
                }
                let px = origin[0] + x as f32 * 0.75;
                let pz = origin[2] + z as f32 * 0.75;
                let position = if ring == 0 {
                    origin
                } else {
                    let mut floor = world.min_y() as f32 * CELL_SIZE;
                    for dx in [-PLAYER_RADIUS, 0.0, PLAYER_RADIUS] {
                        for dz in [-PLAYER_RADIUS, 0.0, PLAYER_RADIUS] {
                            floor = floor.max(world.surface_height(px + dx, pz + dz));
                        }
                    }
                    [px, floor + 0.02, pz]
                };
                if character_position_is_clear(world, position, obstacles) {
                    return Some(position);
                }
            }
        }
    }
    None
}

fn broadcast(connections: &mut BTreeMap<u64, Connection>, message: &ServerMessage) {
    for connection in connections
        .values_mut()
        .filter(|c| c.mode.is_some() && c.closing_at.is_none() && !c.dead)
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
    airships: &AirshipNetwork,
    config: &ServerConfig,
    edit_budget: &mut usize,
) -> io::Result<()> {
    if let ClientMessage::Hello {
        version,
        name,
        mode,
        profile_id,
    } = message
    {
        let connection = connections.get_mut(&id).unwrap();
        if connection.mode.is_some() {
            connection.dead = true;
            return Ok(());
        }
        if version != PROTOCOL_VERSION {
            connection.close_with_notice(format!(
                "Protocol version mismatch: server uses {PROTOCOL_VERSION}, client uses {version}. Update both client and server."
            ));
            return Ok(());
        }
        if mode == SessionMode::Observer && !config.allow_admin {
            connection.close_with_notice("Admin observation is disabled on this server".into());
            return Ok(());
        }
        let profile_id = profile_id.filter(|_| mode == SessionMode::Player);
        if let Some(profile) = &profile_id {
            let reason = if !player_economy::valid_profile_id(profile) {
                Some("Invalid guest profile. Reopen the game to try again.")
            } else if connections
                .values()
                .any(|c| !c.dead && c.profile_id.as_ref() == Some(profile))
            {
                Some("This guest profile is already playing. Leave the other session first.")
            } else if !sim.profiles.contains_key(profile)
                && sim.profiles.len() >= player_economy::MAX_PROFILES
            {
                Some("This world has reached its saved guest limit.")
            } else {
                None
            };
            if let Some(reason) = reason {
                connections
                    .get_mut(&id)
                    .unwrap()
                    .close_with_notice(reason.into());
                return Ok(());
            }
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
        let player = if mode == SessionMode::Player {
            let obstacles = character_obstacles(connections, sim, Some(id), airships);
            if let Some(saved) = profile_id
                .as_ref()
                .and_then(|profile| sim.profiles.get(profile))
            {
                let Some(player) =
                    saved.restore(&sim.world, airships, sim.world_time, &obstacles, id, name)
                else {
                    connections.get_mut(&id).unwrap().close_with_notice(
                        "Your saved location is blocked. Clear nearby space before rejoining; your cargo is safe.".into());
                    return Ok(());
                };
                Some(player)
            } else {
                let Some(position) = free_player_spawn(&sim.world, &obstacles) else {
                    connections.get_mut(&id).unwrap().close_with_notice(
                        "There is no free space near spawn. Try again after someone moves.".into(),
                    );
                    return Ok(());
                };
                Some(PlayerSnapshot {
                    id,
                    name,
                    body: Body::new(position),
                    yaw: 0.0,
                    last_input_sequence: 0,
                    movement_epoch: 0,
                    ride: None,
                    deck_position: None,
                })
            }
        } else {
            None
        };
        let connection = connections.get_mut(&id).unwrap();
        connection.mode = Some(mode);
        connection.player = player;
        connection.profile_id = profile_id.clone();
        if let (Some(profile), Some(player)) = (&profile_id, &connection.player) {
            sim.profiles
                .entry(profile.clone())
                .or_insert_with(|| player_economy::SavedPlayer::new(player))
                .checkpoint(player);
            checkpoint_players(connections, sim);
            sim.save(&config.save_path)?;
        }
        let welcome = ServerMessage::Welcome {
            version: PROTOCOL_VERSION,
            session_id: id,
            mode,
            seed: sim.world.seed,
            generation: sim.world.generation(),
            edits: sim.world.edits(),
            players: players(connections),
            npc: sim.npc.snapshot.clone(),
            residents: sim.villages.residents(),
            villages: sim.villages.villages(),
            world_time: sim.world_time,
            can_admin: config.allow_admin && mode == SessionMode::Player,
        };
        connections.get_mut(&id).unwrap().send(&welcome);
        if profile_id.is_some() {
            send_market_state(connections, sim, id, 0, None, String::new(), true);
            send_work_state(connections, sim, id, 0, String::new(), true);
        }
        return Ok(());
    }
    if connections.get(&id).is_none_or(|c| c.mode.is_none()) {
        connections.get_mut(&id).unwrap().dead = true;
        return Ok(());
    }
    if connections[&id].mode == Some(SessionMode::Observer) {
        match &message {
            ClientMessage::Edit { request_id, .. } => {
                reject(
                    connections,
                    id,
                    *request_id,
                    "Observer sessions are read-only",
                );
                return Ok(());
            }
            ClientMessage::Input { .. }
            | ClientMessage::Admin { .. }
            | ClientMessage::TalkToPilot { .. }
            | ClientMessage::Market { .. }
            | ClientMessage::Work { .. } => {
                connections
                    .get_mut(&id)
                    .unwrap()
                    .send(&ServerMessage::Notice {
                        text: "Observer sessions are read-only".into(),
                    });
                return Ok(());
            }
            ClientMessage::AdminCommand { .. } => {
                connections
                    .get_mut(&id)
                    .unwrap()
                    .send(&ServerMessage::AdminCommandResult {
                        text: "Observer sessions are read-only".into(),
                    });
                return Ok(());
            }
            ClientMessage::Ping => {}
            ClientMessage::Hello { .. } => unreachable!(),
        }
    }
    match message {
        ClientMessage::Work { request_id, action } => {
            handle_work(id, request_id, action, connections, sim);
        }
        ClientMessage::Market {
            request_id,
            village_id,
            revision,
            action,
        } => {
            handle_market(
                id,
                request_id,
                village_id,
                revision,
                action,
                connections,
                sim,
                config,
                edit_budget,
            )?;
        }
        ClientMessage::Input {
            sequence,
            movement_epoch,
            dt,
            input,
            yaw,
        } => {
            let current_epoch = connections[&id].player.as_ref().unwrap().movement_epoch;
            if movement_epoch < current_epoch {
                // A teleport invalidates every command predicted at the old
                // origin. Do not spend its time budget or acknowledge it.
                return Ok(());
            }
            if movement_epoch != current_epoch {
                eprintln!(
                    "Session {id} disconnected: unexpected movement epoch {movement_epoch}, expected {current_epoch}"
                );
                connections.get_mut(&id).unwrap().dead = true;
                return Ok(());
            }
            let obstacles = character_obstacles(connections, sim, Some(id), airships);
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
                eprintln!(
                    "Session {id} disconnected: invalid movement input (sequence {sequence}, last {}, dt {dt})",
                    player.last_input_sequence
                );
                connection.dead = true;
                return Ok(());
            }

            // The server owns the time budget. Do not discard time accrued
            // while this server was busy before spending queued commands.
            // The receive loop bounds the batch and then caps its reserve.
            let now = Instant::now();
            connection.input_credit += now.duration_since(connection.credit_updated).as_secs_f64();
            connection.credit_updated = now;
            if dt as f64 > connection.input_credit + 0.000_001 {
                eprintln!(
                    "Session {id} disconnected: movement time {dt} exceeded available credit {} at sequence {sequence}",
                    connection.input_credit
                );
                connection.dead = true;
                return Ok(());
            }
            connection.input_credit = (connection.input_credit - dt as f64).max(0.0);
            // Use exactly the client's command boundaries and shared controller:
            // even a second direction normalization can change collision results.
            move_character_with_airships(
                &sim.world,
                &mut player.body,
                input,
                dt,
                &obstacles,
                airships,
                sim.world_time,
                &mut player.ride,
                &mut player.deck_position,
            );
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
                    .chain(std::iter::once(sim.npc.snapshot.position))
                    .chain(sim.villages.positions()),
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
            let result = if !config.allow_admin {
                Err("Developer controls are disabled on this server".into())
            } else if !connections.get_mut(&id).unwrap().admit_admin_request() {
                Err("NPC controls are arriving too quickly; try again in a moment".into())
            } else {
                sim.npc.admin(&sim.world, action)
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
        ClientMessage::AdminCommand { command } => {
            admin_commands::handle(id, &command, connections, sim, airships, config);
        }
        ClientMessage::TalkToPilot { ship_id } => {
            let Some(ship) = airships.ship(ship_id, sim.world_time) else {
                transport_notice(connections, id, "That airship is no longer available");
                return Ok(());
            };
            let player = connections[&id].player.as_ref().unwrap();
            if !airships::can_reach_pilot(&ship, player.body.position) {
                transport_notice(connections, id, "Move closer to the airship pilot");
                return Ok(());
            }
            let village_name = |village_id| {
                sim.world
                    .settlements()
                    .and_then(|plan| plan.villages.iter().find(|v| v.id == village_id))
                    .map_or_else(|| format!("Village {village_id}"), |v| v.name.clone())
            };
            // The conversation can stay open through departure and the return
            // trip. Its live client timetable owns all phase-dependent details.
            let text = format!(
                "I'm {}, sailing between {} and {}. Enjoy the view!",
                ship.pilot_name,
                village_name(ship.from_village),
                village_name(ship.next_village),
            );
            connections
                .get_mut(&id)
                .unwrap()
                .send(&ServerMessage::PilotDialog { ship_id, text });
        }
        ClientMessage::Ping => connections.get_mut(&id).unwrap().send(&ServerMessage::Pong),
        ClientMessage::Hello { .. } => unreachable!(),
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn send_market_state(
    connections: &mut BTreeMap<u64, Connection>,
    sim: &Simulation,
    id: u64,
    request_id: u64,
    village_id: Option<u32>,
    notice: String,
    accepted: bool,
) {
    let connection = connections.get_mut(&id).unwrap();
    let ledger = connection
        .profile_id
        .as_ref()
        .and_then(|profile| sim.profiles.get(profile))
        .map_or_else(PlayerEconomy::default, |saved| saved.ledger.clone());
    let market = village_id.and_then(|village| {
        let player = connection.player.as_ref()?;
        player_economy::near_market(&sim.world, village, player.body.position).ok()?;
        player_economy::market_view(&sim.world, &sim.villages, village).ok()
    });
    connection.send(&ServerMessage::MarketState {
        request_id,
        ledger,
        market,
        notice,
        accepted,
    });
}

#[allow(clippy::too_many_arguments)]
fn handle_market(
    id: u64,
    request_id: u64,
    village_id: Option<u32>,
    revision: u64,
    action: MarketAction,
    connections: &mut BTreeMap<u64, Connection>,
    sim: &mut Simulation,
    config: &ServerConfig,
    budget: &mut usize,
) -> io::Result<()> {
    let result = (|| -> Result<String, String> {
        let connection = connections.get_mut(&id).unwrap();
        let profile = connection
            .profile_id
            .as_ref()
            .ok_or("Rejoin with a saved guest profile to use markets.")?
            .clone();
        let player = connection
            .player
            .as_ref()
            .ok_or("Only players can use markets.")?;
        let last_request = if action == MarketAction::View {
            &mut connection.last_market_view
        } else {
            &mut connection.last_market_request
        };
        if last_request.is_some_and(|last| last.elapsed() < Duration::from_millis(100)) {
            return Err("Please wait a moment before the next market request.".into());
        }
        *last_request = Some(Instant::now());
        if let Some(village) = village_id {
            player_economy::near_market(&sim.world, village, player.body.position)?;
        }
        if action == MarketAction::View {
            return Ok(String::new());
        }
        let village = village_id.ok_or("Walk to a village market first.")?;
        let ledger = &sim.profiles[&profile].ledger;
        if ledger.revision != revision {
            return Err(
                "Your cargo changed. Check the refreshed market before trying again.".into(),
            );
        }
        if *budget == 0 {
            return Err("The market is busy. Try again in a moment.".into());
        }
        *budget -= 1;
        player_economy::transact(
            &sim.world,
            &mut sim.villages,
            &mut sim.profiles.get_mut(&profile).unwrap().ledger,
            village,
            &action,
        )
    })();
    if result.is_ok() && action != MarketAction::View {
        checkpoint_players(connections, sim);
        sim.save(&config.save_path)?;
    }
    let (notice, accepted) = match result {
        Ok(notice) => (notice, true),
        Err(error) => (error, false),
    };
    send_market_state(
        connections,
        sim,
        id,
        request_id,
        village_id,
        notice,
        accepted,
    );
    if action == MarketAction::View {
        send_work_state(connections, sim, id, request_id, String::new(), true);
    }
    Ok(())
}

fn send_work_state(
    connections: &mut BTreeMap<u64, Connection>,
    sim: &Simulation,
    id: u64,
    request_id: u64,
    notice: String,
    accepted: bool,
) {
    let connection = connections.get_mut(&id).unwrap();
    let ledger = connection
        .profile_id
        .as_ref()
        .and_then(|id| sim.profiles.get(id))
        .map_or_else(PlayerEconomy::default, |saved| saved.ledger.clone());
    let work = WorkState {
        offer: connection.player.as_ref().and_then(|player| {
            local_work::nearest_offer(
                &sim.world,
                &sim.villages,
                &sim.consumed_quarry_cells,
                player.body.position,
                &ledger,
            )
        }),
        active: connection
            .active_work
            .as_ref()
            .map(|active| active.progress.clone()),
    };
    connection.send(&ServerMessage::WorkState {
        request_id,
        work,
        ledger,
        notice,
        accepted,
    });
}

fn handle_work(
    id: u64,
    request_id: u64,
    action: WorkAction,
    connections: &mut BTreeMap<u64, Connection>,
    sim: &Simulation,
) {
    let result = (|| -> Result<String, String> {
        let connection = connections.get_mut(&id).unwrap();
        if connection.profile_id.is_none() {
            return Err("Rejoin with a saved guest profile to do village work.".into());
        }
        let player = connection
            .player
            .as_ref()
            .ok_or("Only players can do village work.")?;
        if action != WorkAction::View {
            if request_id == 0
                || connection
                    .last_work_action_id
                    .is_some_and(|previous| request_id <= previous)
            {
                return Err(
                    "That work request was already handled. Check the current work state.".into(),
                );
            }
            connection.last_work_action_id = Some(request_id);
        }
        // Passive views cannot block a deliberate action. Starts share the
        // market mutation cap; cancelling remains immediate.
        if action != WorkAction::Cancel {
            let last_request = if action == WorkAction::View {
                &mut connection.last_work_view
            } else {
                &mut connection.last_market_request
            };
            if last_request.is_some_and(|last| last.elapsed() < Duration::from_millis(100)) {
                return Err("Please wait a moment before the next work request.".into());
            }
            *last_request = Some(Instant::now());
        }
        match action {
            WorkAction::View => Ok(String::new()),
            WorkAction::Cancel => {
                let was_active = connection.active_work.take().is_some();
                Ok(if was_active {
                    "Work cancelled.".into()
                } else {
                    String::new()
                })
            }
            WorkAction::Start { site } => {
                if connection.active_work.is_some() {
                    return Err("Finish or cancel your current work first.".into());
                }
                let active = local_work::start(
                    &sim.world,
                    &sim.villages,
                    &sim.consumed_quarry_cells,
                    site,
                    player.body.position,
                    sim.world_time,
                )?;
                let ledger = &sim.profiles[connection.profile_id.as_ref().unwrap()].ledger;
                local_work::check_reward(ledger, active.progress.offer.reward)?;
                connection.active_work = Some(active);
                Ok("Stay at the work site for six seconds.".into())
            }
        }
    })();
    let (notice, accepted) = match result {
        Ok(notice) => (notice, true),
        Err(reason) => (reason, false),
    };
    send_work_state(connections, sim, id, request_id, notice, accepted);
}

fn advance_local_work(
    connections: &mut BTreeMap<u64, Connection>,
    sim: &mut Simulation,
    config: &ServerConfig,
) -> io::Result<()> {
    let mut replies = Vec::new();
    let mut block_changes = Vec::new();
    let mut completed = false;
    for (&id, connection) in connections.iter_mut() {
        let Some(mut active) = connection.active_work.take() else {
            continue;
        };
        let (Some(profile), Some(player)) = (&connection.profile_id, &connection.player) else {
            continue;
        };
        match local_work::advance(
            &sim.world,
            &sim.villages,
            &sim.consumed_quarry_cells,
            &mut active,
            player.body.position,
            sim.world_time,
        ) {
            Err(reason) => replies.push((id, format!("Work stopped: {reason}"), false)),
            Ok(true) => {
                let ledger = &mut sim.profiles.get_mut(profile).unwrap().ledger;
                let result = if active.progress.offer.site.kind == WorkKind::QuarryStone {
                    quarry_work::complete(
                        &mut sim.world,
                        &mut sim.consumed_quarry_cells,
                        ledger,
                        &active,
                    )
                    .map(|(notice, edit)| {
                        block_changes.push((id, edit));
                        notice
                    })
                } else {
                    local_work::complete(&sim.world, &mut sim.villages, ledger, &active)
                };
                match result {
                    Ok(notice) => {
                        completed = true;
                        replies.push((id, notice, true));
                    }
                    Err(reason) => replies.push((id, reason, false)),
                }
            }
            Ok(false) => {
                if sim.world_time - active.last_update >= 0.25 {
                    active.last_update = sim.world_time;
                    replies.push((id, String::new(), true));
                }
                connection.active_work = Some(active);
            }
        }
    }
    // One save can contain simultaneous completions. No completion message is
    // queued until all effects and player positions have been durably written.
    if completed {
        checkpoint_players(connections, sim);
        sim.save(&config.save_path)?;
    }
    for (player_id, edit) in block_changes {
        broadcast(
            connections,
            &ServerMessage::BlockChanged {
                request_id: 0,
                player_id,
                edit,
            },
        );
    }
    for (id, notice, accepted) in replies {
        send_work_state(connections, sim, id, 0, notice, accepted);
    }
    Ok(())
}

fn transport_notice(connections: &mut BTreeMap<u64, Connection>, id: u64, text: &str) {
    connections
        .get_mut(&id)
        .unwrap()
        .send(&ServerMessage::Notice { text: text.into() });
}

fn validate_edit(
    world: &World,
    actor: &Body,
    position: BlockPos,
    block: Block,
    occupied: impl Iterator<Item = [f32; 3]>,
) -> Result<(), String> {
    if !world.contains_block(position) {
        return Err("That block is outside the editable world".into());
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
    fn passive_work_and_market_views_do_not_spend_the_action_rate_limit() {
        use rubblekin_core::economy::{WorkKind, WorkSite};

        let world = World::generate(42, WorldGeneration::GeographyV5);
        let village = &world.settlements().unwrap().villages[0];
        let soil = village.fields[0].plant_positions().next().unwrap();
        let site = WorkSite {
            village_id: village.id,
            kind: WorkKind::TendField,
            index: 0,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let _peer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let mut connection = Connection::new(listener.accept().unwrap().0).unwrap();
        let player = PlayerSnapshot {
            id: 1,
            name: "Worker".into(),
            body: Body::new([
                soil.x as f32 * 0.5 + 0.25,
                (soil.y + 1) as f32 * 0.5,
                soil.z as f32 * 0.5 + 0.25,
            ]),
            yaw: 0.0,
            last_input_sequence: 0,
            movement_epoch: 0,
            ride: None,
            deck_position: None,
        };
        let profile = "00000000000000000000000000000001".to_owned();
        let saved = player_economy::SavedPlayer::new(&player);
        connection.mode = Some(SessionMode::Player);
        connection.player = Some(player);
        connection.profile_id = Some(profile.clone());
        let mut connections = BTreeMap::from([(1, connection)]);
        let mut sim = Simulation {
            npc: npc::Forager::new(&world),
            villages: villages::VillageLife::new(&world),
            world,
            profiles: BTreeMap::from([(profile, saved)]),
            consumed_quarry_cells: Vec::new(),
            world_time: 0.0,
        };
        let mut budget = 1;
        handle_market(
            1,
            100,
            None,
            0,
            MarketAction::View,
            &mut connections,
            &mut sim,
            &ServerConfig::default(),
            &mut budget,
        )
        .unwrap();
        handle_work(1, 101, WorkAction::View, &mut connections, &sim);
        assert!(connections[&1].last_market_view.is_some());
        assert!(connections[&1].last_work_view.is_some());
        assert!(connections[&1].last_market_request.is_none());
        assert!(connections[&1].last_work_action_id.is_none());
        // Future timestamps keep the limit closed without timing assumptions
        // on slow CI machines. Both reads remain capped independently.
        let blocked_until = Instant::now() + Duration::from_secs(60);
        connections.get_mut(&1).unwrap().last_work_view = Some(blocked_until);
        connections.get_mut(&1).unwrap().last_market_view = Some(blocked_until);
        handle_work(1, 102, WorkAction::View, &mut connections, &sim);
        let reply: ServerMessage =
            serde_json::from_slice(connections[&1].outgoing.back().unwrap()).unwrap();
        assert!(matches!(
            reply,
            ServerMessage::WorkState {
                accepted: false,
                ..
            }
        ));
        handle_market(
            1,
            103,
            None,
            0,
            MarketAction::View,
            &mut connections,
            &mut sim,
            &ServerConfig::default(),
            &mut budget,
        )
        .unwrap();
        let replies = &connections[&1].outgoing;
        let reply: ServerMessage = serde_json::from_slice(&replies[replies.len() - 2]).unwrap();
        assert!(matches!(
            reply,
            ServerMessage::MarketState {
                accepted: false,
                ..
            }
        ));

        handle_work(1, 1, WorkAction::Start { site }, &mut connections, &sim);
        assert!(connections[&1].active_work.is_some());
        connections.get_mut(&1).unwrap().last_market_request = Some(blocked_until);
        handle_work(1, 2, WorkAction::Cancel, &mut connections, &sim);
        assert!(connections[&1].active_work.is_none());
        handle_work(1, 3, WorkAction::Start { site }, &mut connections, &sim);
        let reply: ServerMessage =
            serde_json::from_slice(connections[&1].outgoing.back().unwrap()).unwrap();
        assert!(
            matches!(reply, ServerMessage::WorkState { accepted: false, notice, .. }
            if notice.contains("wait a moment"))
        );
        assert!(connections[&1].active_work.is_none());
        assert_eq!(connections[&1].last_work_action_id, Some(3));
        assert_eq!(budget, 1);
    }

    #[test]
    fn a_full_server_spawns_every_player_in_distinct_clear_space() {
        let world = World::new(42);
        let mut occupied = vec![npc::Forager::new(&world).snapshot.position];
        for _ in 0..MAX_CLIENTS {
            let position = free_player_spawn(&world, &occupied).unwrap();
            assert!(character_position_is_clear(&world, position, &occupied));
            occupied.push(position);
        }
    }

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
