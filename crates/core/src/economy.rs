//! Player trade uses real village goods and a small, private coin ledger.
use crate::settlement::ResourceKind;
use serde::{Deserialize, Serialize};

pub const CARGO_CAPACITY: u32 = 24;
pub const DELIVERY_AMOUNT: u32 = 6;
pub const MARKET_REACH: f32 = 3.0;
pub const WORK_REACH: f32 = 2.5;

pub fn can_reach_market(position: [f32; 3], market: [f32; 3]) -> bool {
    position
        .iter()
        .chain(market.iter())
        .all(|value| value.is_finite())
        && (position[0] - market[0]).powi(2) + (position[2] - market[2]).powi(2)
            <= MARKET_REACH * MARKET_REACH
        && (position[1] - market[1]).abs() <= 1.5
}
pub const RESOURCES: [ResourceKind; 5] = [
    ResourceKind::Food,
    ResourceKind::Timber,
    ResourceKind::Stone,
    ResourceKind::Clay,
    ResourceKind::Iron,
];

pub fn resource_index(kind: ResourceKind) -> usize {
    match kind {
        ResourceKind::Food => 0,
        ResourceKind::Timber => 1,
        ResourceKind::Stone => 2,
        ResourceKind::Clay => 3,
        ResourceKind::Iron => 4,
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerEconomy {
    pub revision: u64,
    pub coins: u64,
    pub cargo: [u32; 5],
    pub delivery: Option<DeliveryContract>,
}

impl PlayerEconomy {
    pub fn cargo_total(&self) -> u32 {
        self.cargo
            .iter()
            .copied()
            .fold(0, u32::saturating_add)
            .saturating_add(self.delivery.as_ref().map_or(0, |job| job.amount))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryContract {
    pub origin: u32,
    pub destination: u32,
    pub kind: ResourceKind,
    pub amount: u32,
    pub reward: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketGood {
    pub kind: ResourceKind,
    pub stock: f32,
    pub exportable: u32,
    pub buy_price: u64,
    pub sell_price: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketView {
    pub village_id: u32,
    pub goods: Vec<MarketGood>,
    pub delivery_offer: Option<DeliveryContract>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketAction {
    View,
    Buy {
        kind: ResourceKind,
        quantity: u32,
        unit_price: u64,
    },
    Sell {
        kind: ResourceKind,
        quantity: u32,
        unit_price: u64,
    },
    AcceptDelivery {
        offer: DeliveryContract,
    },
    Deliver,
    ReturnDelivery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkKind {
    TendField,
    WorkshopMaintenance,
    HarvestField,
    QuarryStone,
    GatherForage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkReward {
    Coins(u64),
    Cargo { kind: ResourceKind, amount: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSite {
    /// Village ID for village/quarry work; habitat ID for GatherForage.
    pub village_id: u32,
    pub kind: WorkKind,
    pub index: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkOffer {
    pub site: WorkSite,
    pub position: [f32; 3],
    pub label: String,
    pub reward: WorkReward,
    pub duration_seconds: f32,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkProgress {
    pub offer: WorkOffer,
    pub elapsed_seconds: f32,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkState {
    pub offer: Option<WorkOffer>,
    pub active: Option<WorkProgress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkAction {
    View,
    Start { site: WorkSite },
    Cancel,
}
