use crate::{
    physics::{Body, MoveInput},
    world::{Block, BlockEdit, BlockPos, WorldGeneration},
};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 5;
pub const MAX_MESSAGE_BYTES: usize = 64 * 1024;
/// Maximum simulated duration of one movement command, including a long frame.
pub const MAX_INPUT_DT: f32 = 0.25;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionMode {
    #[default]
    Player,
    Observer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientMessage {
    Hello {
        version: u32,
        name: String,
        #[serde(default)]
        mode: SessionMode,
    },
    Input {
        sequence: u64,
        dt: f32,
        input: MoveInput,
        yaw: f32,
    },
    Edit {
        request_id: u64,
        position: BlockPos,
        block: Block,
    },
    Admin {
        action: AdminAction,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Welcome {
        version: u32,
        session_id: u64,
        mode: SessionMode,
        seed: u32,
        generation: WorldGeneration,
        edits: Vec<BlockEdit>,
        players: Vec<PlayerSnapshot>,
        npc: NpcSnapshot,
        world_time: f64,
        can_admin: bool,
    },
    State {
        players: Vec<PlayerSnapshot>,
        npc: NpcSnapshot,
        world_time: f64,
    },
    BlockChanged {
        request_id: u64,
        player_id: u64,
        edit: BlockEdit,
    },
    Rejected {
        request_id: u64,
        reason: String,
    },
    Notice {
        text: String,
    },
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub id: u64,
    pub name: String,
    pub body: Body,
    pub yaw: f32,
    /// Last movement command actually simulated, or zero before the first input.
    pub last_input_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NpcAction {
    Forage,
    Rest,
    Wander,
}

impl NpcAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Forage => "Foraging",
            Self::Rest => "Resting",
            Self::Wander => "Wandering",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcSnapshot {
    pub name: String,
    pub position: [f32; 3],
    pub hunger: f32,
    pub energy: f32,
    pub action: NpcAction,
    pub reason: String,
    pub berries: u32,
    pub forced: bool,
    pub target: Option<[f32; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdminAction {
    SetNpcGoal { goal: Option<NpcAction> },
    SetNpcNeeds { hunger: f32, energy: f32 },
    SetNpcWeights { forage: f32, rest: f32 },
}
