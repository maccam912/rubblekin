//! Two short, physical village activities. Unfinished work belongs only to its
//! connection; completed effects and wages join the existing durable ledger.
use rubblekin_core::{
    economy::{PlayerEconomy, WorkKind, WorkOffer, WorkProgress, WorkSite},
    physics::EYE_HEIGHT,
    village_assets::BuildingKind,
    world::{Block, BlockPos, CELL_SIZE, World},
};

use crate::{player_economy::MAX_COINS, villages::VillageLife};

const WORK_SECONDS: f32 = 6.0;
const WORK_REACH: f32 = 2.5;
const MOVE_LIMIT: f32 = 0.8;

pub(crate) struct ActiveWork {
    pub progress: WorkProgress,
    pub start_position: [f32; 3],
    pub anchor: BlockPos,
    pub started_at: f64,
    pub last_update: f64,
}

fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter().zip(b).map(|(a, b)| (a - b).powi(2)).sum()
}

fn site_target(
    world: &World,
    site: WorkSite,
    position: [f32; 3],
) -> Result<(BlockPos, [f32; 3], String), String> {
    let village = world
        .settlements()
        .and_then(|plan| {
            plan.villages
                .iter()
                .find(|village| village.id == site.village_id)
        })
        .ok_or("That work site no longer exists.")?;
    let anchor = match site.kind {
        WorkKind::TendField => {
            let field = village
                .fields
                .get(site.index as usize)
                .ok_or("That field does not exist.")?;
            field
                .plant_positions()
                .min_by(|a, b| {
                    let feet = |cell: &BlockPos| {
                        [
                            cell.x as f32 * CELL_SIZE + 0.25,
                            (cell.y + 1) as f32 * CELL_SIZE,
                            cell.z as f32 * CELL_SIZE + 0.25,
                        ]
                    };
                    distance_squared(feet(a), position)
                        .total_cmp(&distance_squared(feet(b), position))
                })
                .ok_or("That field has no planting soil.")?
        }
        WorkKind::WorkshopMaintenance => {
            let building = village
                .buildings
                .get(site.index as usize)
                .filter(|building| building.kind == BuildingKind::Workshop)
                .ok_or("That workshop does not exist.")?;
            let [width, _, depth] = building.dimensions();
            let (x, z) = (0..width)
                .flat_map(|x| (0..depth).map(move |z| (x, z)))
                .find(|(x, z)| {
                    building.local_cell(building.origin.x + x, building.origin.z + z)
                        == Some([14, 5])
                })
                .ok_or("The workshop has no workbench.")?;
            BlockPos::new(
                building.origin.x + x,
                building.origin.y + 2,
                building.origin.z + z,
            )
        }
    };
    let target = [
        anchor.x as f32 * CELL_SIZE + 0.25,
        if site.kind == WorkKind::TendField {
            (anchor.y + 1) as f32 * CELL_SIZE
        } else {
            (anchor.y - 1) as f32 * CELL_SIZE
        },
        anchor.z as f32 * CELL_SIZE + 0.25,
    ];
    let activity = match site.kind {
        WorkKind::TendField => "Tend field",
        WorkKind::WorkshopMaintenance => "Workshop maintenance",
    };
    Ok((anchor, target, format!("{} · {activity}", village.name)))
}

fn access(
    world: &World,
    site: WorkSite,
    anchor: BlockPos,
    target: [f32; 3],
    position: [f32; 3],
) -> Result<(), String> {
    let intact = match site.kind {
        WorkKind::TendField => matches!(world.block(anchor), Block::Dirt | Block::Grass),
        WorkKind::WorkshopMaintenance => {
            world.block(anchor) == Block::Stone
                && world.block(BlockPos::new(anchor.x, anchor.y - 1, anchor.z)) == Block::Wood
        }
    };
    if !intact {
        return Err(match site.kind {
            WorkKind::TendField => "This plot needs intact planting soil.",
            WorkKind::WorkshopMaintenance => "The workshop needs its intact workbench.",
        }
        .into());
    }
    let horizontal = (position[0] - target[0]).powi(2) + (position[2] - target[2]).powi(2);
    if !position.iter().all(|value| value.is_finite())
        || horizontal > WORK_REACH.powi(2)
        || (position[1] - target[1]).abs() > 1.5
    {
        return Err("Move closer to the work site.".into());
    }
    let eye = [position[0], position[1] + EYE_HEIGHT, position[2]];
    let aim = [target[0], target[1] + EYE_HEIGHT, target[2]];
    let direction = std::array::from_fn(|axis| aim[axis] - eye[axis]);
    let distance = distance_squared(eye, aim).sqrt();
    if distance > 0.05 && world.raycast(eye, direction, distance).is_some() {
        return Err("Clear the path to the work site first.".into());
    }
    Ok(())
}

pub(crate) fn offer(
    world: &World,
    life: &VillageLife,
    site: WorkSite,
    position: [f32; 3],
) -> Result<(WorkOffer, BlockPos), String> {
    let (anchor, target, label) = site_target(world, site, position)?;
    let unavailable_reason = access(world, site, anchor, target, position)
        .and_then(|()| life.local_work_available(site.village_id, site.kind))
        .err();
    Ok((
        WorkOffer {
            site,
            position: target,
            label,
            reward: match site.kind {
                WorkKind::TendField => 2,
                WorkKind::WorkshopMaintenance => 4,
            },
            duration_seconds: WORK_SECONDS,
            unavailable_reason,
        },
        anchor,
    ))
}

pub(crate) fn nearest_offer(
    world: &World,
    life: &VillageLife,
    position: [f32; 3],
) -> Option<WorkOffer> {
    let plan = world.settlements()?;
    plan.villages
        .iter()
        .flat_map(|village| {
            village
                .fields
                .iter()
                .enumerate()
                .map(|(index, _)| WorkSite {
                    village_id: village.id,
                    kind: WorkKind::TendField,
                    index: index as u32,
                })
                .chain(
                    village
                        .buildings
                        .iter()
                        .enumerate()
                        .filter(|(_, building)| building.kind == BuildingKind::Workshop)
                        .map(|(index, _)| WorkSite {
                            village_id: village.id,
                            kind: WorkKind::WorkshopMaintenance,
                            index: index as u32,
                        }),
                )
        })
        .filter_map(|site| {
            offer(world, life, site, position)
                .ok()
                .map(|(offer, _)| offer)
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
    life: &VillageLife,
    site: WorkSite,
    position: [f32; 3],
    now: f64,
) -> Result<ActiveWork, String> {
    let (offer, anchor) = offer(world, life, site, position)?;
    if let Some(reason) = &offer.unavailable_reason {
        return Err(reason.clone());
    }
    Ok(ActiveWork {
        progress: WorkProgress {
            offer,
            elapsed_seconds: 0.0,
        },
        anchor,
        start_position: position,
        started_at: now,
        last_update: now,
    })
}

pub(crate) fn advance(
    world: &World,
    life: &VillageLife,
    active: &mut ActiveWork,
    position: [f32; 3],
    now: f64,
) -> Result<bool, String> {
    if distance_squared(active.start_position, position) > MOVE_LIMIT.powi(2) {
        return Err("Work cancelled because you moved away.".into());
    }
    let offer = &active.progress.offer;
    access(world, offer.site, active.anchor, offer.position, position)?;
    life.local_work_available(offer.site.village_id, offer.site.kind)?;
    active.progress.elapsed_seconds = ((now - active.started_at).max(0.0) as f32).min(WORK_SECONDS);
    Ok(now - active.started_at >= f64::from(WORK_SECONDS))
}

pub(crate) fn complete(
    world: &World,
    life: &mut VillageLife,
    ledger: &mut PlayerEconomy,
    active: &ActiveWork,
) -> Result<String, String> {
    let offer = &active.progress.offer;
    if ledger.coins > MAX_COINS - offer.reward || ledger.revision >= u64::MAX - 1 {
        return Err("Your progress has reached its saved limit.".into());
    }
    life.complete_local_work(world, offer.site.village_id, offer.site.kind)?;
    ledger.coins += offer.reward;
    ledger.revision += 1;
    Ok(format!(
        "{} complete. Earned {} coins!",
        match offer.site.kind {
            WorkKind::TendField => "Field work",
            WorkKind::WorkshopMaintenance => "Workshop maintenance",
        },
        offer.reward
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::{physics::character_position_is_clear, world::WorldGeneration};
    use std::sync::OnceLock;

    fn world() -> &'static World {
        static WORLD: OnceLock<World> = OnceLock::new();
        WORLD.get_or_init(|| World::generate(42, WorldGeneration::GeographyV5))
    }

    fn field() -> (WorkSite, [f32; 3]) {
        let world = world();
        let village = &world.settlements().unwrap().villages[0];
        let site = WorkSite {
            village_id: village.id,
            kind: WorkKind::TendField,
            index: 0,
        };
        let (_, position, _) = site_target(world, site, village.center).unwrap();
        (site, position)
    }

    fn workshop() -> (WorkSite, [f32; 3]) {
        let world = world();
        let village = &world.settlements().unwrap().villages[0];
        let index = village
            .buildings
            .iter()
            .position(|building| building.kind == BuildingKind::Workshop)
            .unwrap();
        let site = WorkSite {
            village_id: village.id,
            kind: WorkKind::WorkshopMaintenance,
            index: index as u32,
        };
        let (anchor, target, _) = site_target(world, site, village.center).unwrap();
        let position = (-5..=5)
            .flat_map(|x| (-5..=5).map(move |z| [x as f32 * 0.5, z as f32 * 0.5]))
            .map(|offset| [target[0] + offset[0], target[1], target[2] + offset[1]])
            .find(|position| {
                character_position_is_clear(world, *position, &[])
                    && access(world, site, anchor, target, *position).is_ok()
            })
            .unwrap();
        (site, position)
    }

    #[test]
    fn tending_requires_six_seconds_at_intact_soil_and_improves_the_real_crop() {
        let world = world();
        let mut life = VillageLife::new(world);
        let (site, position) = field();
        let before = life.market_stocks(site.village_id).unwrap().crop_growth;
        let mut active = start(world, &life, site, position, 100.0).unwrap();
        assert!(!advance(world, &life, &mut active, position, 105.999).unwrap());
        assert!(advance(world, &life, &mut active, position, 106.0).unwrap());
        let mut ledger = PlayerEconomy::default();
        complete(world, &mut life, &mut ledger, &active).unwrap();
        assert!(life.market_stocks(site.village_id).unwrap().crop_growth > before);
        assert_eq!(ledger.coins, 2);
        assert_eq!(ledger.revision, 1);

        let mut moved = position;
        moved[0] += 0.81;
        assert!(advance(world, &life, &mut active, moved, 106.0).is_err());
        let mut damaged = world.clone();
        damaged.set_block(active.anchor, Block::Stone).unwrap();
        assert!(advance(&damaged, &life, &mut active, position, 106.0).is_err());
    }

    #[test]
    fn ready_crops_do_not_pay_for_no_work_and_empty_fields_can_be_planted() {
        let world = world();
        let (site, position) = field();
        let life = VillageLife::new(world);
        let mut value = serde_json::to_value(&life).unwrap();
        value["villages"][0]["snapshot"]["crop_growth"] = 1.0.into();
        let ready: VillageLife = serde_json::from_value(value.clone()).unwrap();
        assert!(start(world, &ready, site, position, 0.0).is_err());
        value["villages"][0]["snapshot"]["crop_growth"] = 0.0.into();
        value["villages"][0]["planted"] = false.into();
        let mut empty: VillageLife = serde_json::from_value(value).unwrap();
        let active = start(world, &empty, site, position, 0.0).unwrap();
        let mut ledger = PlayerEconomy::default();
        complete(world, &mut empty, &mut ledger, &active).unwrap();
        assert_eq!(
            empty.market_stocks(site.village_id).unwrap().crop_growth,
            0.01
        );
        assert_eq!(ledger.coins, 2);
        assert!(empty.validate(world));
    }

    #[test]
    fn maintenance_consumes_real_materials_above_reserves_and_needs_its_workbench() {
        let world = world();
        let mut life = VillageLife::new(world);
        let (site, position) = workshop();
        use rubblekin_core::settlement::ResourceKind;
        let before = life.market_stocks(site.village_id).unwrap();
        let (timber, stone) = (before.timber, before.stone);
        life.change_market_stock(site.village_id, ResourceKind::Timber, 9.0 - timber)
            .unwrap();
        life.change_market_stock(site.village_id, ResourceKind::Stone, 9.0 - stone)
            .unwrap();
        let active = start(world, &life, site, position, 0.0).unwrap();
        let mut ledger = PlayerEconomy::default();
        complete(world, &mut life, &mut ledger, &active).unwrap();
        let after = life.market_stocks(site.village_id).unwrap();
        assert_eq!((after.timber, after.stone), (8.0, 8.0));
        assert_eq!(ledger.coins, 4);
        assert!(complete(world, &mut life, &mut ledger, &active).is_err());
        assert_eq!(ledger.coins, 4);
        let mut damaged = world.clone();
        damaged.set_block(active.anchor, Block::Air).unwrap();
        assert!(start(&damaged, &VillageLife::new(world), site, position, 0.0).is_err());
    }

    #[test]
    fn distant_forged_and_occluded_sites_cannot_start_work() {
        let world = world();
        let life = VillageLife::new(world);
        let (site, position) = field();
        let mut distant = position;
        distant[0] += 100.0;
        assert!(start(world, &life, site, distant, 0.0).is_err());
        assert!(
            start(
                world,
                &life,
                WorkSite {
                    index: u32::MAX,
                    ..site
                },
                position,
                0.0
            )
            .is_err()
        );
        let (site, position) = workshop();
        let active = start(world, &life, site, position, 0.0).unwrap();
        let target = active.progress.offer.position;
        let midpoint = [
            (position[0] + target[0]) * 0.5,
            position[1] + EYE_HEIGHT,
            (position[2] + target[2]) * 0.5,
        ];
        let cell = midpoint.map(|coordinate| (coordinate / CELL_SIZE).floor() as i32);
        let mut blocked = world.clone();
        blocked
            .set_block(BlockPos::new(cell[0], cell[1], cell[2]), Block::Stone)
            .unwrap();
        assert!(start(&blocked, &life, site, position, 0.0).is_err());
    }
}
