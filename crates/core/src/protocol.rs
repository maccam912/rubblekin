use crate::{
    airships::AirshipRide,
    physics::{Body, MoveInput},
    settlement::ResourceKind,
    world::{Block, BlockEdit, BlockPos, WorldGeneration},
};
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 8;
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
    TalkToPilot {
        ship_id: u64,
    },
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
        #[serde(default)]
        residents: Vec<ResidentSnapshot>,
        #[serde(default)]
        villages: Vec<VillageSnapshot>,
        world_time: f64,
        can_admin: bool,
    },
    State {
        players: Vec<PlayerSnapshot>,
        npc: NpcSnapshot,
        #[serde(default)]
        residents: Vec<ResidentSnapshot>,
        #[serde(default)]
        villages: Vec<VillageSnapshot>,
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
    PilotDialog {
        ship_id: u64,
        text: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSnapshot {
    pub id: u64,
    pub name: String,
    pub body: Body,
    pub yaw: f32,
    /// Last movement command actually simulated, or zero before the first input.
    pub last_input_sequence: u64,
    #[serde(default)]
    pub ride: Option<AirshipRide>,
    /// Ship-local [side, height, fore] offset; absent on the ground.
    #[serde(default)]
    pub deck_position: Option<[f32; 3]>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResidentRole {
    Farmer,
    Woodcutter,
    Quarrier,
    Miner,
    Trader,
}

impl ResidentRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Farmer => "Farmer",
            Self::Woodcutter => "Woodcutter",
            Self::Quarrier => "Quarrier",
            Self::Miner => "Miner",
            Self::Trader => "Trader",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResidentAction {
    Walking,
    Working,
    Delivering,
    Trading,
    Resting,
    Blocked,
    Eating,
    SeekingFood,
    GoingHome,
    Planting,
    Tending,
    Harvesting,
    WaitingForAirship,
    RidingAirship,
}

impl ResidentAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Walking => "Walking to work",
            Self::Working => "Working",
            Self::Delivering => "Delivering goods",
            Self::Trading => "Trading",
            Self::Resting => "Resting at home",
            Self::Blocked => "Path blocked",
            Self::Eating => "Eating a meal",
            Self::SeekingFood => "Going for food",
            Self::GoingHome => "Going home to sleep",
            Self::Planting => "Planting crops",
            Self::Tending => "Tending crops",
            Self::Harvesting => "Harvesting crops",
            Self::WaitingForAirship => "Waiting for an airship",
            Self::RidingAirship => "Riding an airship",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceCargo {
    pub kind: ResourceKind,
    pub amount: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResidentSnapshot {
    pub id: u64,
    pub village_id: u32,
    pub name: String,
    pub position: [f32; 3],
    pub role: ResidentRole,
    pub action: ResidentAction,
    pub target: Option<[f32; 3]>,
    pub carrying: Option<ResourceCargo>,
    #[serde(default = "resident_initial_hunger")]
    pub hunger: f32,
    #[serde(default = "resident_initial_energy")]
    pub energy: f32,
    #[serde(default = "resident_initial_reason")]
    pub reason: String,
    #[serde(default)]
    pub ride: Option<AirshipRide>,
    #[serde(default)]
    pub deck_position: Option<[f32; 3]>,
}

fn resident_initial_hunger() -> f32 {
    25.0
}
fn resident_initial_energy() -> f32 {
    85.0
}
fn resident_initial_reason() -> String {
    "Heading to the next job".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VillageSnapshot {
    pub id: u32,
    pub food: f32,
    pub timber: f32,
    pub stone: f32,
    pub clay: f32,
    pub iron: f32,
    pub crop_growth: f32,
    pub population: u32,
    pub housing_capacity: u32,
    pub food_reserve: f32,
    pub capacity_for_growth: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AdminAction {
    SetNpcGoal { goal: Option<NpcAction> },
    SetNpcNeeds { hunger: f32, energy: f32 },
    SetNpcWeights { forage: f32, rest: f32 },
}
