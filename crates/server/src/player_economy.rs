//! Private guest progress and direct, durable market transactions.
use std::collections::BTreeMap;

use rubblekin_core::{
    airships::{
        AIRSHIP_DECK_HALF_LENGTH, AIRSHIP_DECK_HALF_WIDTH, AirshipNetwork, AirshipRide,
        deck_position,
    },
    economy::{
        CARGO_CAPACITY, DELIVERY_AMOUNT, DeliveryContract, MarketAction, MarketGood, MarketView,
        PlayerEconomy, RESOURCES, can_reach_market, resource_index,
    },
    physics::{Body, EYE_HEIGHT, character_position_is_clear_with_airships},
    protocol::PlayerSnapshot,
    settlement::{ResourceKind, Village},
    world::{CELL_SIZE, World},
};
use serde::{Deserialize, Serialize};

use crate::villages::VillageLife;

pub(crate) const MAX_PROFILES: usize = 1024;
const MAX_COINS: u64 = 1_000_000_000;
const MAX_STOCK: f32 = 10_000.0;
pub(crate) type Profiles = BTreeMap<String, SavedPlayer>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SavedPlayer {
    pub ledger: PlayerEconomy,
    pub position: [f32; 3],
    pub yaw: f32,
    pub ride: Option<AirshipRide>,
    pub deck_position: Option<[f32; 3]>,
}

impl SavedPlayer {
    pub fn new(player: &PlayerSnapshot) -> Self {
        Self {
            ledger: PlayerEconomy::default(),
            position: player.body.position,
            yaw: player.yaw,
            ride: player.ride,
            deck_position: player.deck_position,
        }
    }

    pub fn checkpoint(&mut self, player: &PlayerSnapshot) {
        self.position = player.body.position;
        self.yaw = player.yaw;
        self.ride = player.ride;
        self.deck_position = player.deck_position;
    }

    /// Return beside the saved position, or to the same moving deck. Never
    /// substitute world spawn, which could transport a delivery across town.
    pub fn restore(
        &self,
        world: &World,
        network: &AirshipNetwork,
        time: f64,
        obstacles: &[[f32; 3]],
        id: u64,
        name: String,
    ) -> Option<PlayerSnapshot> {
        let ship = self.ride.and_then(|ride| network.ship(ride.ship_id, time));
        let origin = match (&ship, self.deck_position) {
            (Some(ship), Some(local)) => deck_position(ship, local),
            _ => self.position,
        };
        for ring in 0_i32..=4 {
            for z in -ring..=ring {
                for x in -ring..=ring {
                    if x.abs().max(z.abs()) != ring {
                        continue;
                    }
                    let mut local = self.deck_position;
                    let position = if let (Some(ship), Some(offset)) = (&ship, &mut local) {
                        offset[0] += x as f32 * 0.75;
                        offset[2] += z as f32 * 0.75;
                        if offset[0].abs() > AIRSHIP_DECK_HALF_WIDTH - 0.4
                            || offset[2].abs() > AIRSHIP_DECK_HALF_LENGTH - 0.4
                        {
                            continue;
                        }
                        deck_position(ship, *offset)
                    } else {
                        [
                            origin[0] + x as f32 * 0.75,
                            origin[1],
                            origin[2] + z as f32 * 0.75,
                        ]
                    };
                    if character_position_is_clear_with_airships(
                        world, position, obstacles, network, time,
                    ) {
                        return Some(PlayerSnapshot {
                            id,
                            name,
                            body: Body::new(position),
                            yaw: self.yaw,
                            last_input_sequence: 0,
                            movement_epoch: 0,
                            ride: self.ride,
                            deck_position: local,
                        });
                    }
                }
            }
        }
        None
    }
}

pub(crate) fn valid_profile_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn validate_profiles(
    profiles: &Profiles,
    world: &World,
    network: &AirshipNetwork,
) -> bool {
    profiles.len() <= MAX_PROFILES
        && profiles.iter().all(|(id, player)| {
            let position = player.position;
            let radius = world.radius_cells() as f32 * CELL_SIZE;
            valid_profile_id(id)
                && position.iter().all(|v| v.is_finite())
                && player.yaw.is_finite()
                && position[0].abs() <= radius
                && position[2].abs() <= radius
                && position[1] >= world.min_y() as f32 * CELL_SIZE - 2.0
                && position[1] <= world.max_y() as f32 * CELL_SIZE + 64.0
                && player.ledger.coins <= MAX_COINS
                && player.ledger.revision < u64::MAX
                && player.ledger.cargo_total() <= CARGO_CAPACITY
                && match (player.ride, player.deck_position) {
                    (None, None) => true,
                    (Some(ride), Some(local)) => {
                        ride.seat == u8::MAX
                            && network.ship(ride.ship_id, 0.0).is_some()
                            && local.iter().all(|v| v.is_finite())
                            && local[0].abs() <= AIRSHIP_DECK_HALF_WIDTH + 0.4
                            && local[2].abs() <= AIRSHIP_DECK_HALF_LENGTH + 0.4
                            && (-0.05..=64.0).contains(&local[1])
                    }
                    _ => false,
                }
                && player
                    .ledger
                    .delivery
                    .as_ref()
                    .is_none_or(|job| valid_contract(world, job))
        })
}

fn valid_contract(world: &World, job: &DeliveryContract) -> bool {
    job.amount == DELIVERY_AMOUNT
        && job.reward == 12
        && job.origin != job.destination
        && world.settlements().is_some_and(|plan| {
            plan.trails.iter().any(|trail| {
                (trail.from == job.origin && trail.to == job.destination)
                    || (trail.to == job.origin && trail.from == job.destination)
            })
        })
}

fn village(world: &World, id: u32) -> Result<&Village, String> {
    world
        .settlements()
        .and_then(|plan| plan.villages.iter().find(|v| v.id == id))
        .ok_or_else(|| "That village has no market.".into())
}

pub(crate) fn near_market(world: &World, id: u32, position: [f32; 3]) -> Result<(), String> {
    let market = village(world, id)?.market;
    if !can_reach_market(position, market) {
        return Err("Walk up to the village market first.".into());
    }
    let eye = [position[0], position[1] + EYE_HEIGHT, position[2]];
    let target = [market[0], market[1] + EYE_HEIGHT, market[2]];
    let direction = std::array::from_fn(|i| target[i] - eye[i]);
    let distance = direction.iter().map(|v| v * v).sum::<f32>().sqrt();
    if distance > 0.05 && world.raycast(eye, direction, distance).is_some() {
        return Err("The path to the market counter is blocked.".into());
    }
    Ok(())
}

pub(crate) fn market_view(
    world: &World,
    life: &VillageLife,
    id: u32,
) -> Result<MarketView, String> {
    let village = village(world, id)?;
    let stocks = life.market_stocks(id).ok_or("The market is unavailable.")?;
    let goods = RESOURCES
        .into_iter()
        .map(|kind| {
            let stock = stock(stocks, kind);
            let reserve = if kind == ResourceKind::Food {
                stocks.food_reserve
            } else {
                8.0
            };
            // Local production sets the baseline; scarce goods cost up to three
            // more coins. One/five-unit orders cross at most one 24-unit threshold,
            // and a two-coin spread prevents immediate round-trip profit.
            let scores = village.resources;
            let score = match kind {
                ResourceKind::Food => scores.farming,
                ResourceKind::Timber => scores.timber,
                ResourceKind::Stone => scores.stone,
                ResourceKind::Clay => scores.clay,
                ResourceKind::Iron => scores.iron,
            };
            let base = [3, 4, 4, 5, 8][resource_index(kind)];
            let pressure = 3_u64.saturating_sub((stock.max(0.0) / 24.0).floor() as u64);
            let buy_price = (base + 4_u64)
                .saturating_sub((score.clamp(0.0, 1.0) * 5.0).floor() as u64)
                .max(3)
                + pressure;
            MarketGood {
                kind,
                stock,
                exportable: (stock - reserve).max(0.0).floor() as u32,
                buy_price,
                sell_price: buy_price - 2,
            }
        })
        .collect();
    Ok(MarketView {
        village_id: id,
        goods,
        delivery_offer: delivery_offer(world, life, id),
    })
}

fn stock(value: &rubblekin_core::protocol::VillageSnapshot, kind: ResourceKind) -> f32 {
    match kind {
        ResourceKind::Food => value.food,
        ResourceKind::Timber => value.timber,
        ResourceKind::Stone => value.stone,
        ResourceKind::Clay => value.clay,
        ResourceKind::Iron => value.iron,
    }
}

fn delivery_offer(world: &World, life: &VillageLife, origin: u32) -> Option<DeliveryContract> {
    let source = life.market_stocks(origin)?;
    let mut best: Option<(f32, DeliveryContract)> = None;
    for trail in &world.settlements()?.trails {
        let destination = if trail.from == origin {
            trail.to
        } else if trail.to == origin {
            trail.from
        } else {
            continue;
        };
        let target = life.market_stocks(destination)?;
        for kind in RESOURCES {
            let supply = stock(source, kind);
            let demand = stock(target, kind);
            let reserve = if kind == ResourceKind::Food {
                source.food_reserve
            } else {
                8.0
            };
            let desired = match kind {
                ResourceKind::Food => target.food_reserve + 60.0,
                ResourceKind::Timber | ResourceKind::Stone => 48.0,
                ResourceKind::Clay => 32.0,
                ResourceKind::Iron => 24.0,
            };
            let difference = desired - demand;
            if supply - reserve >= DELIVERY_AMOUNT as f32
                && demand + DELIVERY_AMOUNT as f32 <= MAX_STOCK
                && difference >= DELIVERY_AMOUNT as f32
                && best
                    .as_ref()
                    .is_none_or(|(previous, _)| difference > *previous)
            {
                best = Some((
                    difference,
                    DeliveryContract {
                        origin,
                        destination,
                        kind,
                        amount: DELIVERY_AMOUNT,
                        reward: 12,
                    },
                ));
            }
        }
    }
    best.map(|(_, offer)| offer)
}

/// The caller has checked identity, revision and physical market access. Every
/// rejection leaves both ledger and goods unchanged; success must be saved.
pub(crate) fn transact(
    world: &World,
    life: &mut VillageLife,
    ledger: &mut PlayerEconomy,
    village_id: u32,
    action: &MarketAction,
) -> Result<String, String> {
    if ledger.revision >= u64::MAX - 1 {
        return Err("This profile has reached its transaction limit.".into());
    }
    let view = market_view(world, life, village_id)?;
    let notice = match action {
        MarketAction::View => return Ok(String::new()),
        MarketAction::Buy {
            kind,
            quantity,
            unit_price,
        }
        | MarketAction::Sell {
            kind,
            quantity,
            unit_price,
        } => {
            if ![1, 5].contains(quantity) {
                return Err("Trade one or five units at a time.".into());
            }
            let good = &view.goods[resource_index(*kind)];
            let buying = matches!(action, MarketAction::Buy { .. });
            let price = if buying {
                good.buy_price
            } else {
                good.sell_price
            };
            if *unit_price != price {
                return Err("The quote changed. Check the current price.".into());
            }
            let total = price * u64::from(*quantity);
            let index = resource_index(*kind);
            if buying {
                if *quantity > good.exportable {
                    return Err("The village needs to keep those supplies.".into());
                }
                if ledger.cargo_total() + quantity > CARGO_CAPACITY {
                    return Err("Your cargo is full.".into());
                }
                if ledger.coins < total {
                    return Err(
                        "You need more coins. Deliver a village parcel to earn some.".into(),
                    );
                }
                life.change_market_stock(village_id, *kind, -(*quantity as f32))?;
                ledger.coins -= total;
                ledger.cargo[index] += quantity;
                format!(
                    "Bought {quantity} {} for {total} coins.",
                    kind.name().to_lowercase()
                )
            } else {
                if ledger.cargo[index] < *quantity {
                    return Err(
                        "You do not carry that much. Delivery parcels cannot be sold.".into(),
                    );
                }
                if ledger.coins > MAX_COINS - total {
                    return Err("Your coin purse is full.".into());
                }
                life.change_market_stock(village_id, *kind, *quantity as f32)?;
                ledger.cargo[index] -= quantity;
                ledger.coins += total;
                format!(
                    "Sold {quantity} {} for {total} coins.",
                    kind.name().to_lowercase()
                )
            }
        }
        MarketAction::AcceptDelivery { offer } => {
            if ledger.delivery.is_some() {
                return Err("Finish or return your current delivery first.".into());
            }
            if ledger.cargo_total() + DELIVERY_AMOUNT > CARGO_CAPACITY {
                return Err("Make room for six units of delivery cargo.".into());
            }
            if view.delivery_offer.as_ref() != Some(offer) {
                return Err(
                    "That delivery is no longer available. Check the current offer.".into(),
                );
            }
            life.change_market_stock(village_id, offer.kind, -(offer.amount as f32))?;
            ledger.delivery = Some(offer.clone());
            "Parcel loaded. Carry it to the destination market to earn 12 coins.".into()
        }
        MarketAction::Deliver | MarketAction::ReturnDelivery => {
            let job = ledger
                .delivery
                .as_ref()
                .ok_or("You have no delivery parcel.")?;
            let returning = matches!(action, MarketAction::ReturnDelivery);
            if village_id
                != if returning {
                    job.origin
                } else {
                    job.destination
                }
            {
                return Err(if returning {
                    "Return the parcel at its origin market."
                } else {
                    "This is not the parcel's destination market."
                }
                .into());
            }
            if !returning && ledger.coins > MAX_COINS - job.reward {
                return Err("Your coin purse is full.".into());
            }
            life.change_market_stock(village_id, job.kind, job.amount as f32)?;
            if !returning {
                ledger.coins += job.reward;
            }
            ledger.delivery = None;
            if returning {
                "Parcel returned.".into()
            } else {
                "Delivery complete. Earned 12 coins!".into()
            }
        }
    };
    ledger.revision += 1;
    Ok(notice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::world::WorldGeneration;
    use std::sync::OnceLock;

    fn world() -> &'static World {
        static WORLD: OnceLock<World> = OnceLock::new();
        WORLD.get_or_init(|| World::generate(42, WorldGeneration::GeographyV4))
    }

    #[test]
    fn delivery_conserves_goods_and_pays_only_at_the_destination_once() {
        let world = world();
        let mut life = VillageLife::new(world);
        let offer = world
            .settlements()
            .unwrap()
            .villages
            .iter()
            .find_map(|village| delivery_offer(world, &life, village.id))
            .unwrap();
        let before_origin = stock(life.market_stocks(offer.origin).unwrap(), offer.kind);
        let before_destination = stock(life.market_stocks(offer.destination).unwrap(), offer.kind);
        let mut ledger = PlayerEconomy::default();
        transact(
            world,
            &mut life,
            &mut ledger,
            offer.origin,
            &MarketAction::AcceptDelivery {
                offer: offer.clone(),
            },
        )
        .unwrap();
        assert_eq!(
            stock(life.market_stocks(offer.origin).unwrap(), offer.kind),
            before_origin - 6.0
        );
        assert_eq!(
            stock(life.market_stocks(offer.destination).unwrap(), offer.kind),
            before_destination
        );
        assert_eq!(ledger.cargo_total(), 6);
        assert_eq!(ledger.coins, 0);
        let loaded = ledger.clone();
        assert!(
            transact(
                world,
                &mut life,
                &mut ledger,
                offer.origin,
                &MarketAction::Deliver
            )
            .is_err()
        );
        let price = market_view(world, &life, offer.origin).unwrap().goods
            [resource_index(offer.kind)]
        .sell_price;
        assert!(
            transact(
                world,
                &mut life,
                &mut ledger,
                offer.origin,
                &MarketAction::Sell {
                    kind: offer.kind,
                    quantity: 1,
                    unit_price: price
                }
            )
            .is_err()
        );
        assert_eq!(ledger, loaded);
        transact(
            world,
            &mut life,
            &mut ledger,
            offer.destination,
            &MarketAction::Deliver,
        )
        .unwrap();
        assert_eq!(
            stock(life.market_stocks(offer.destination).unwrap(), offer.kind),
            before_destination + 6.0
        );
        assert_eq!(ledger.coins, 12);
        assert_eq!(ledger.cargo_total(), 0);
        assert!(
            transact(
                world,
                &mut life,
                &mut ledger,
                offer.destination,
                &MarketAction::Deliver
            )
            .is_err()
        );
        assert_eq!(ledger.coins, 12);
    }

    #[test]
    fn returning_a_parcel_requires_its_origin_and_refunds_goods_without_coins() {
        let world = world();
        let mut life = VillageLife::new(world);
        let offer = world
            .settlements()
            .unwrap()
            .villages
            .iter()
            .find_map(|village| delivery_offer(world, &life, village.id))
            .unwrap();
        let before = life.villages();
        let mut ledger = PlayerEconomy::default();
        transact(
            world,
            &mut life,
            &mut ledger,
            offer.origin,
            &MarketAction::AcceptDelivery {
                offer: offer.clone(),
            },
        )
        .unwrap();
        assert!(
            transact(
                world,
                &mut life,
                &mut ledger,
                offer.destination,
                &MarketAction::ReturnDelivery
            )
            .is_err()
        );
        transact(
            world,
            &mut life,
            &mut ledger,
            offer.origin,
            &MarketAction::ReturnDelivery,
        )
        .unwrap();
        assert_eq!(life.villages(), before);
        assert_eq!(ledger.coins, 0);
        assert_eq!(ledger.cargo_total(), 0);
    }

    #[test]
    fn bulk_trade_respects_capacity_reserves_quotes_and_has_no_same_market_profit() {
        let world = world();
        let mut life = VillageLife::new(world);
        let village = world.settlements().unwrap().villages[0].id;
        let view = market_view(world, &life, village).unwrap();
        let good = view.goods.iter().find(|good| good.exportable >= 5).unwrap();
        let mut ledger = PlayerEconomy {
            coins: 100,
            ..Default::default()
        };
        let stocks = life.villages();
        for quantity in [0, 6, u32::MAX] {
            assert!(
                transact(
                    world,
                    &mut life,
                    &mut ledger,
                    village,
                    &MarketAction::Buy {
                        kind: good.kind,
                        quantity,
                        unit_price: good.buy_price
                    }
                )
                .is_err()
            );
        }
        assert!(
            transact(
                world,
                &mut life,
                &mut ledger,
                village,
                &MarketAction::Buy {
                    kind: good.kind,
                    quantity: 5,
                    unit_price: 0
                }
            )
            .is_err()
        );
        assert_eq!(life.villages(), stocks);
        assert_eq!(ledger.coins, 100);
        transact(
            world,
            &mut life,
            &mut ledger,
            village,
            &MarketAction::Buy {
                kind: good.kind,
                quantity: 5,
                unit_price: good.buy_price,
            },
        )
        .unwrap();
        transact(
            world,
            &mut life,
            &mut ledger,
            village,
            &MarketAction::Sell {
                kind: good.kind,
                quantity: 5,
                unit_price: good.sell_price,
            },
        )
        .unwrap();
        assert_eq!(life.villages(), stocks);
        assert_eq!(ledger.coins, 90);
        ledger.cargo = [24, 0, 0, 0, 0];
        assert!(
            transact(
                world,
                &mut life,
                &mut ledger,
                village,
                &MarketAction::Buy {
                    kind: good.kind,
                    quantity: 1,
                    unit_price: good.buy_price
                }
            )
            .is_err()
        );
        let reserve = life.market_stocks(village).unwrap().food_reserve;
        let current = life.market_stocks(village).unwrap().food;
        life.change_market_stock(village, ResourceKind::Food, reserve + 1.0 - current)
            .unwrap();
        ledger.cargo = [0; 5];
        let food = &market_view(world, &life, village).unwrap().goods[0];
        assert_eq!(food.exportable, 1);
        assert!(
            transact(
                world,
                &mut life,
                &mut ledger,
                village,
                &MarketAction::Buy {
                    kind: ResourceKind::Food,
                    quantity: 5,
                    unit_price: food.buy_price
                }
            )
            .is_err()
        );
        assert_eq!(life.market_stocks(village).unwrap().food, reserve + 1.0);
    }

    #[test]
    fn every_generated_market_is_reachable_but_remote_and_occluded_requests_are_not() {
        let world = world();
        for village in &world.settlements().unwrap().villages {
            assert!(near_market(world, village.id, village.market).is_ok());
            let mut above = village.market;
            above[1] += 2.0;
            assert!(near_market(world, village.id, above).is_err());
        }
        let village = &world.settlements().unwrap().villages[0];
        let actor = [[2.0, 0.0], [-2.0, 0.0], [0.0, 2.0], [0.0, -2.0]]
            .into_iter()
            .map(|offset| {
                [
                    village.market[0] + offset[0],
                    village.market[1],
                    village.market[2] + offset[1],
                ]
            })
            .find(|position| near_market(world, village.id, *position).is_ok())
            .unwrap();
        let mut blocked = world.clone();
        let midpoint = [
            (actor[0] + village.market[0]) * 0.5,
            actor[1] + EYE_HEIGHT,
            (actor[2] + village.market[2]) * 0.5,
        ];
        let cell = midpoint.map(|coordinate| (coordinate / CELL_SIZE).floor() as i32);
        blocked
            .set_block(
                rubblekin_core::world::BlockPos::new(cell[0], cell[1], cell[2]),
                rubblekin_core::world::Block::Stone,
            )
            .unwrap();
        assert!(near_market(&blocked, village.id, actor).is_err());
    }

    #[test]
    fn reconnect_keeps_ground_cargo_location_and_aboard_local_position() {
        let world = world();
        let network = AirshipNetwork::new(world);
        let position = world.settlements().unwrap().villages[1].market;
        let mut saved = SavedPlayer {
            ledger: PlayerEconomy::default(),
            position,
            yaw: 0.5,
            ride: None,
            deck_position: None,
        };
        let restored = saved
            .restore(world, &network, 0.0, &[], 5, "Guest".into())
            .unwrap();
        assert_eq!(restored.body.position, position);
        assert_ne!(restored.body.position, world.spawn_position());
        let ship = network.ships(0.0)[0].clone();
        saved.ride = Some(AirshipRide {
            ship_id: ship.id,
            seat: u8::MAX,
        });
        saved.deck_position = Some([-2.0, 0.0, 0.0]);
        saved.position = deck_position(&ship, saved.deck_position.unwrap());
        let restored = saved
            .restore(world, &network, 160.0, &[], 5, "Guest".into())
            .unwrap();
        assert_eq!(restored.ride, saved.ride);
        assert_eq!(restored.deck_position, saved.deck_position);
        assert_eq!(
            restored.body.position,
            deck_position(
                &network.ship(ship.id, 160.0).unwrap(),
                saved.deck_position.unwrap()
            )
        );
        // A ship can move across a disconnected ground/falling player's last
        // location. Recovery must not recreate them inside its solid deck.
        saved.ride = None;
        saved.deck_position = None;
        saved.position = deck_position(&ship, [-2.0, -1.0, 0.0]);
        assert!(!character_position_is_clear_with_airships(
            world,
            saved.position,
            &[],
            &network,
            0.0
        ));
        if let Some(restored) = saved.restore(world, &network, 0.0, &[], 5, "Guest".into()) {
            assert!(character_position_is_clear_with_airships(
                world,
                restored.body.position,
                &[],
                &network,
                0.0
            ));
        }
    }

    #[test]
    fn prices_respond_to_stock_without_one_or_five_unit_round_trip_profit() {
        let world = world();
        let village = world.settlements().unwrap().villages[0].id;
        for initial_stock in [23.0, 24.0, 25.0, 47.0, 48.0, 49.0, 71.0, 72.0, 73.0] {
            for quantity in [1, 5] {
                for buy_first in [false, true] {
                    let mut life = VillageLife::new(world);
                    let stock = life.market_stocks(village).unwrap().timber;
                    life.change_market_stock(village, ResourceKind::Timber, initial_stock - stock)
                        .unwrap();
                    let mut ledger = PlayerEconomy {
                        coins: 1000,
                        cargo: [0, 5, 0, 0, 0],
                        ..Default::default()
                    };
                    for buy in [buy_first, !buy_first] {
                        let good = &market_view(world, &life, village).unwrap().goods[1];
                        let action = if buy {
                            MarketAction::Buy {
                                kind: ResourceKind::Timber,
                                quantity,
                                unit_price: good.buy_price,
                            }
                        } else {
                            MarketAction::Sell {
                                kind: ResourceKind::Timber,
                                quantity,
                                unit_price: good.sell_price,
                            }
                        };
                        transact(world, &mut life, &mut ledger, village, &action).unwrap();
                    }
                    assert!(
                        ledger.coins < 1000,
                        "stock={initial_stock}, qty={quantity}, buy_first={buy_first}"
                    );
                    assert_eq!(ledger.cargo, [0, 5, 0, 0, 0]);
                    assert_eq!(life.market_stocks(village).unwrap().timber, initial_stock);
                }
            }
        }
    }

    #[test]
    fn initial_villages_have_delivery_work() {
        for seed in [42, 43] {
            let world = World::generate(seed, WorldGeneration::GeographyV4);
            let life = VillageLife::new(&world);
            for village in &world.settlements().unwrap().villages {
                assert!(
                    delivery_offer(&world, &life, village.id).is_some(),
                    "seed={seed} village={}",
                    village.name
                );
            }
        }
    }
}
