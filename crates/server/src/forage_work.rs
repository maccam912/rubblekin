//! Gathering shares wildlife food; no terrain blocks or village stores change.
use crate::{
    ecology::Ecology,
    local_work::{self, ActiveWork},
};
use rubblekin_core::{
    economy::{
        PlayerEconomy, WORK_REACH, WorkKind, WorkOffer, WorkProgress, WorkReward, WorkSite,
        resource_index,
    },
    forage::{FORAGE_RADIUS, GATHER_COST, WildPlant, plants},
    physics::EYE_HEIGHT,
    settlement::ResourceKind,
    world::World,
};

const SECONDS: f32 = 6.;
const REWARD: WorkReward = WorkReward::Cargo {
    kind: ResourceKind::Food,
    amount: 1,
};

fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter().zip(b).map(|(a, b)| (a - b).powi(2)).sum()
}
fn plant(world: &World, ecology: &Ecology, site: WorkSite) -> Result<WildPlant, String> {
    let h = ecology
        .habitats
        .iter()
        .find(|h| site.kind == WorkKind::GatherForage && h.id == site.village_id)
        .ok_or("That wild habitat no longer exists.")?;
    if h.forage < GATHER_COST {
        return Err("This habitat needs time to regrow wild food.".into());
    }
    plants(world, h.id, h.position, h.forage)
        .into_iter()
        .find(|p| p.index == site.index)
        .ok_or_else(|| {
            "This plant has been grazed or covered. Find another clump or let it regrow.".into()
        })
}
fn access(world: &World, plant: WildPlant, position: [f32; 3]) -> Result<(), String> {
    let target = plant.position();
    let horizontal = (position[0] - target[0]).powi(2) + (position[2] - target[2]).powi(2);
    if !position.iter().all(|v| v.is_finite())
        || horizontal > WORK_REACH.powi(2)
        || (position[1] - target[1]).abs() > 1.5
    {
        return Err("Move closer to the wild plant.".into());
    }
    let eye = [position[0], position[1] + EYE_HEIGHT, position[2]];
    let aim = [target[0], target[1] + 0.25, target[2]];
    let direction = std::array::from_fn(|i| aim[i] - eye[i]);
    let distance = distance_squared(eye, aim).sqrt();
    if distance > 0.05 && world.raycast(eye, direction, distance).is_some() {
        return Err("Clear the path to the wild plant first.".into());
    }
    Ok(())
}
fn offer(
    world: &World,
    ecology: &Ecology,
    site: WorkSite,
    position: [f32; 3],
    ledger: &PlayerEconomy,
) -> Result<(WorkOffer, WildPlant), String> {
    let p = plant(world, ecology, site)?;
    Ok((plant_offer(world, p, site, position, ledger), p))
}
fn plant_offer(
    world: &World,
    p: WildPlant,
    site: WorkSite,
    position: [f32; 3],
    ledger: &PlayerEconomy,
) -> WorkOffer {
    WorkOffer {
        site,
        position: p.position(),
        label: format!("{} · Gather food", p.kind.name()),
        reward: REWARD,
        duration_seconds: SECONDS,
        unavailable_reason: access(world, p, position)
            .and_then(|()| local_work::check_reward(ledger, REWARD))
            .err(),
    }
}
pub(crate) fn nearest_offer(
    world: &World,
    ecology: &Ecology,
    position: [f32; 3],
    ledger: &PlayerEconomy,
) -> Option<WorkOffer> {
    ecology
        .habitats
        .iter()
        .filter(|h| {
            (h.position[0] - position[0]).hypot(h.position[2] - position[2])
                <= FORAGE_RADIUS + WORK_REACH
        })
        .flat_map(|h| {
            plants(world, h.id, h.position, h.forage)
                .into_iter()
                .map(move |p| {
                    let site = WorkSite {
                        village_id: h.id,
                        kind: WorkKind::GatherForage,
                        index: p.index,
                    };
                    plant_offer(world, p, site, position, ledger)
                })
        })
        .min_by(|a, b| {
            a.unavailable_reason
                .is_some()
                .cmp(&b.unavailable_reason.is_some())
                .then_with(|| {
                    distance_squared(a.position, position)
                        .total_cmp(&distance_squared(b.position, position))
                })
        })
}
pub(crate) fn start(
    world: &World,
    ecology: &Ecology,
    site: WorkSite,
    position: [f32; 3],
    now: f64,
) -> Result<ActiveWork, String> {
    let (offer, p) = offer(world, ecology, site, position, &PlayerEconomy::default())?;
    if let Some(reason) = &offer.unavailable_reason {
        return Err(reason.clone());
    }
    Ok(ActiveWork {
        progress: WorkProgress {
            offer,
            elapsed_seconds: 0.,
        },
        anchor: p.ground,
        start_position: position,
        started_at: now,
        last_update: now,
    })
}
pub(crate) fn advance(
    world: &World,
    ecology: &Ecology,
    active: &mut ActiveWork,
    position: [f32; 3],
    now: f64,
) -> Result<bool, String> {
    if distance_squared(position, active.start_position) > 0.8_f32.powi(2) {
        return Err("Gathering cancelled because you moved away.".into());
    }
    let p = plant(world, ecology, active.progress.offer.site)?;
    if p.ground != active.anchor {
        return Err("The wild plant's ground changed.".into());
    }
    access(world, p, position)?;
    active.progress.elapsed_seconds = ((now - active.started_at).max(0.) as f32).min(SECONDS);
    Ok(now - active.started_at >= f64::from(SECONDS))
}
pub(crate) fn complete(
    world: &World,
    ecology: &mut Ecology,
    ledger: &mut PlayerEconomy,
    active: &ActiveWork,
) -> Result<String, String> {
    let p = plant(world, ecology, active.progress.offer.site)?;
    if p.ground != active.anchor {
        return Err("The wild plant's ground changed.".into());
    }
    local_work::check_reward(ledger, REWARD)?;
    let h = ecology
        .habitats
        .iter_mut()
        .find(|h| h.id == active.progress.offer.site.village_id)
        .unwrap();
    h.forage -= GATHER_COST;
    ledger.cargo[resource_index(ResourceKind::Food)] += 1;
    ledger.revision += 1;
    Ok("Gathered 1 Food. Rabbits share this supply; let depleted patches regrow. Sell food at a village market.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::{
        economy::{CARGO_CAPACITY, DeliveryContract},
        physics::character_position_is_clear,
        world::{Block, BlockPos, WorldGeneration},
    };
    use std::sync::OnceLock;
    fn fixture() -> (&'static World, Ecology, WorkSite, [f32; 3]) {
        static WORLD: OnceLock<World> = OnceLock::new();
        let world = WORLD.get_or_init(|| World::generate(42, WorldGeneration::GeographyV6));
        let mut e = Ecology::new(world);
        e.habitats[0].forage = GATHER_COST;
        let h = &e.habitats[0];
        let p = plants(world, h.id, h.position, h.forage)
            .into_iter()
            .find(|p| character_position_is_clear(world, p.position(), &[]))
            .unwrap();
        let id = h.id;
        (
            world,
            e,
            WorkSite {
                village_id: id,
                kind: WorkKind::GatherForage,
                index: p.index,
            },
            p.position(),
        )
    }
    #[test]
    fn gathering_takes_six_seconds_and_competitors_cannot_overdraw_shared_food() {
        let (world, mut e, site, position) = fixture();
        let before = world.edits();
        let mut a = start(world, &e, site, position, 10.).unwrap();
        let mut b = start(world, &e, site, position, 10.).unwrap();
        assert!(!advance(world, &e, &mut a, position, 15.999).unwrap());
        assert!(advance(world, &e, &mut a, position, 16.).unwrap());
        let mut ledger = PlayerEconomy::default();
        complete(world, &mut e, &mut ledger, &a).unwrap();
        assert_eq!(ledger.cargo, [1, 0, 0, 0, 0]);
        assert_eq!((ledger.revision, ledger.coins), (1, 0));
        assert_eq!(e.habitats[0].forage, 0.);
        assert!(advance(world, &e, &mut b, position, 16.).is_err());
        let mut competitor = PlayerEconomy::default();
        assert!(complete(world, &mut e, &mut competitor, &b).is_err());
        assert_eq!(competitor, PlayerEconomy::default());
        assert_eq!(world.edits(), before);
        assert!(e.validate(world));
    }
    #[test]
    fn movement_edits_reach_and_cargo_are_rechecked_without_losing_food() {
        let (world, mut e, site, position) = fixture();
        let mut a = start(world, &e, site, position, 0.).unwrap();
        assert!(
            start(
                world,
                &e,
                site,
                [position[0] + 20., position[1], position[2]],
                0.
            )
            .is_err()
        );
        assert!(
            advance(
                world,
                &e,
                &mut a,
                [position[0] + 1., position[1], position[2]],
                1.
            )
            .is_err()
        );
        let mut ledger = PlayerEconomy {
            cargo: [CARGO_CAPACITY - 6, 0, 0, 0, 0],
            delivery: Some(DeliveryContract {
                origin: 0,
                destination: 1,
                kind: ResourceKind::Stone,
                amount: 6,
                reward: 12,
            }),
            ..Default::default()
        };
        assert!(
            nearest_offer(world, &e, position, &ledger)
                .unwrap()
                .unavailable_reason
                .unwrap()
                .contains("room")
        );
        assert!(complete(world, &mut e, &mut ledger, &a).is_err());
        assert_eq!(e.habitats[0].forage, GATHER_COST);
        assert_eq!(ledger.revision, 0);
        let mut edited = world.clone();
        edited
            .set_block(
                BlockPos::new(a.anchor.x, a.anchor.y + 1, a.anchor.z),
                Block::Brick,
            )
            .unwrap();
        assert!(advance(&edited, &e, &mut a, position, 6.).is_err());
        assert!(complete(&edited, &mut e, &mut PlayerEconomy::default(), &a).is_err());
        assert_eq!(e.habitats[0].forage, GATHER_COST);
        let mut invalid = site;
        invalid.village_id = u32::MAX;
        assert!(start(world, &e, invalid, position, 0.).is_err());
        invalid = site;
        invalid.index = u32::MAX;
        assert!(start(world, &e, invalid, position, 0.).is_err());
        // A real intervening wall cancels gathering even while staying close.
        let side = [position[0] + 2., position[1], position[2]];
        let mut blocked = world.clone();
        for y in a.anchor.y + 1..=a.anchor.y + 4 {
            blocked
                .set_block(BlockPos::new(a.anchor.x + 2, y, a.anchor.z), Block::Brick)
                .unwrap();
        }
        assert!(
            start(&blocked, &e, site, side, 0.)
                .err()
                .unwrap()
                .contains("path")
        );
    }
}
