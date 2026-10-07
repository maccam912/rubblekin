//! Finite quarry-pile extraction. The shared spent-cell record survives
//! creative restoration, reconnects and sales of the extracted cargo.
use std::collections::HashSet;

use rubblekin_core::{
    economy::{PlayerEconomy, WORK_REACH, WorkKind, WorkReward, WorkSite, resource_index},
    physics::Body,
    settlement::{BuildingPlot, ResourceKind, RoadsideLandmark},
    village_assets::{BuildingKind, dimensions, quarry_pile_cells},
    world::{Block, BlockEdit, BlockPos, CELL_SIZE, World},
};

use crate::{local_work, persistence::MAX_EDITS};

pub(crate) fn cells(building: &BuildingPlot) -> impl Iterator<Item = BlockPos> + '_ {
    let [width, _, depth] = dimensions(building.kind);
    quarry_pile_cells().map(move |[x, y, z]| {
        let [x, z] = match building.rotation % 4 {
            0 => [x, z],
            1 => [depth - 1 - z, x],
            2 => [width - 1 - x, depth - 1 - z],
            _ => [z, width - 1 - x],
        };
        BlockPos::new(
            building.origin.x + x,
            building.origin.y + y,
            building.origin.z + z,
        )
    })
}

fn nearest_village(world: &World, building: &BuildingPlot) -> Option<u32> {
    let position = building.entrance();
    world
        .settlements()?
        .villages
        .iter()
        .min_by(|a, b| {
            let distance = |center: [f32; 3]| {
                (center[0] - position[0]).powi(2) + (center[2] - position[2]).powi(2)
            };
            distance(a.center)
                .total_cmp(&distance(b.center))
                .then_with(|| a.id.cmp(&b.id))
        })
        .map(|village| village.id)
}

pub(crate) fn sites(world: &World) -> Vec<WorkSite> {
    world
        .settlements()
        .into_iter()
        .flat_map(|plan| &plan.roadside_landmarks)
        .enumerate()
        .filter(|(_, site)| site.building.kind == BuildingKind::QuarryYard)
        .filter_map(|(index, site)| {
            nearest_village(world, &site.building).map(|village_id| WorkSite {
                village_id,
                kind: WorkKind::QuarryStone,
                index: index as u32,
            })
        })
        .collect()
}

pub(crate) fn site(world: &World, id: WorkSite) -> Result<&RoadsideLandmark, String> {
    let site = world
        .settlements()
        .and_then(|plan| plan.roadside_landmarks.get(id.index as usize))
        .filter(|site| {
            id.kind == WorkKind::QuarryStone
                && site.building.kind == BuildingKind::QuarryYard
                && nearest_village(world, &site.building) == Some(id.village_id)
        })
        .ok_or("That quarry pile does not exist.")?;
    Ok(site)
}

/// Target feet are on the quarry floor beside the selected pile block. Actual
/// block-face visibility is checked separately by the shared edit validator.
pub(crate) fn target(
    world: &World,
    id: WorkSite,
    spent: &[BlockPos],
    position: [f32; 3],
) -> Result<(BlockPos, [f32; 3]), String> {
    let site = site(world, id)?;
    let feet = |cell: BlockPos| {
        [
            cell.x as f32 * CELL_SIZE + CELL_SIZE * 0.5,
            (site.building.origin.y + 1) as f32 * CELL_SIZE,
            cell.z as f32 * CELL_SIZE + CELL_SIZE * 0.5,
        ]
    };
    let distance = |cell| {
        feet(cell)
            .iter()
            .zip(position)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
    };
    let anchor = cells(&site.building)
        .min_by(|a, b| {
            let rank = |cell| {
                if spent.contains(&cell) || world.block(cell) != Block::Stone {
                    return 2;
                }
                let target = feet(cell);
                if (target[0] - position[0]).powi(2) + (target[2] - position[2]).powi(2)
                    > WORK_REACH.powi(2)
                    || (target[1] - position[1]).abs() > 1.5
                    || crate::validate_edit(
                        world,
                        &Body::new(position),
                        cell,
                        Block::Air,
                        std::iter::empty(),
                    )
                    .is_err()
                {
                    1
                } else {
                    0
                }
            };
            rank(*a)
                .cmp(&rank(*b))
                .then_with(|| distance(*a).total_cmp(&distance(*b)))
                // Prefer the top of an equally close stack, whose surface can be seen.
                .then_with(|| b.y.cmp(&a.y))
        })
        .ok_or("That quarry has no stone pile.")?;
    Ok((anchor, feet(anchor)))
}

pub(crate) fn available(
    world: &World,
    id: WorkSite,
    anchor: BlockPos,
    spent: &[BlockPos],
) -> Result<(), String> {
    let site = site(world, id)?;
    if !cells(&site.building).any(|cell| cell == anchor) {
        return Err("Only the quarry's designated stone pile can be collected.".into());
    }
    if spent.contains(&anchor) {
        return Err(
            "This stone has already been collected. Restoring it does not create more supplies."
                .into(),
        );
    }
    if world.block(anchor) != Block::Stone {
        return Err("This quarry stone is no longer here.".into());
    }
    Ok(())
}

pub(crate) fn valid_spent(world: &World, spent: &[BlockPos]) -> bool {
    let canonical: HashSet<_> = world
        .settlements()
        .into_iter()
        .flat_map(|plan| &plan.roadside_landmarks)
        .filter(|site| site.building.kind == BuildingKind::QuarryYard)
        .flat_map(|site| cells(&site.building))
        .collect();
    if spent.len() > canonical.len() {
        return false;
    }
    let mut unique = HashSet::new();
    spent
        .iter()
        .all(|cell| canonical.contains(cell) && unique.insert(*cell))
}

pub(crate) fn complete(
    world: &mut World,
    spent: &mut Vec<BlockPos>,
    ledger: &mut PlayerEconomy,
    active: &local_work::ActiveWork,
) -> Result<(String, BlockEdit), String> {
    available(world, active.progress.offer.site, active.anchor, spent)?;
    local_work::check_reward(
        ledger,
        WorkReward::Cargo {
            kind: ResourceKind::Stone,
            amount: 1,
        },
    )?;
    let edits = world.edits();
    if edits.len() >= MAX_EDITS && !edits.iter().any(|edit| edit.position == active.anchor) {
        return Err("The world has reached its saved edit limit.".into());
    }
    world.set_block(active.anchor, Block::Air)?;
    spent.push(active.anchor);
    spent.sort_by_key(|cell| (cell.x, cell.y, cell.z));
    ledger.cargo[resource_index(ResourceKind::Stone)] += 1;
    ledger.revision += 1;
    Ok((
        "Collected 1 stone. Carry it to a market to sell.".into(),
        BlockEdit {
            position: active.anchor,
            block: Block::Air,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::{
        economy::{WorkOffer, WorkProgress},
        world::WorldGeneration,
    };
    use std::sync::OnceLock;

    fn world() -> &'static World {
        static WORLD: OnceLock<World> = OnceLock::new();
        WORLD.get_or_init(|| World::generate(42, WorldGeneration::GeographyV6))
    }

    fn extraction(world: &World) -> local_work::ActiveWork {
        let id = sites(world)[0];
        let building = &site(world, id).unwrap().building;
        let anchor = cells(building).next().unwrap();
        let position = [
            anchor.x as f32 * CELL_SIZE,
            (building.origin.y + 1) as f32 * CELL_SIZE,
            anchor.z as f32 * CELL_SIZE,
        ];
        local_work::ActiveWork {
            progress: WorkProgress {
                offer: WorkOffer {
                    site: id,
                    position,
                    label: "Quarry".into(),
                    reward: WorkReward::Cargo {
                        kind: ResourceKind::Stone,
                        amount: 1,
                    },
                    duration_seconds: 6.0,
                    unavailable_reason: None,
                },
                elapsed_seconds: 6.0,
            },
            start_position: position,
            anchor,
            started_at: 0.0,
            last_update: 6.0,
        }
    }

    #[test]
    fn pile_cells_are_stone_in_every_rotation_and_exclude_the_floor_and_structure() {
        for rotation in 0..4 {
            let building = BuildingPlot {
                kind: BuildingKind::QuarryYard,
                origin: BlockPos::new(100, 200, 300),
                rotation,
            };
            let cells: Vec<_> = cells(&building).collect();
            assert_eq!(cells.len(), 50);
            for cell in cells {
                assert!(cell.y > building.origin.y);
                assert_eq!(building.asset_at(cell), Some(Block::Stone));
                let [x, z] = building.local_cell(cell.x, cell.z).unwrap();
                assert!((20..=23).contains(&x) && (5..=11).contains(&z));
            }
        }
    }

    #[test]
    fn extraction_is_shared_once_only_even_after_creative_restoration() {
        let mut world = world().clone();
        let active = extraction(&world);
        let mut spent = Vec::new();
        let mut ledger = PlayerEconomy::default();
        let (_, edit) = complete(&mut world, &mut spent, &mut ledger, &active).unwrap();
        assert_eq!(
            edit,
            BlockEdit {
                position: active.anchor,
                block: Block::Air
            }
        );
        assert_eq!(world.block(active.anchor), Block::Air);
        assert_eq!(ledger.cargo[resource_index(ResourceKind::Stone)], 1);
        assert_eq!((ledger.coins, ledger.revision), (0, 1));
        assert!(valid_spent(&world, &spent));
        world.set_block(active.anchor, Block::Stone).unwrap();
        let mut another_player = PlayerEconomy::default();
        assert!(complete(&mut world, &mut spent, &mut another_player, &active).is_err());
        assert_eq!(another_player.cargo_total(), 0);
        assert_eq!(spent.len(), 1);
        assert_eq!(world.block(active.anchor), Block::Stone);
        assert!(!valid_spent(&world, &[active.anchor, active.anchor]));
        assert!(!valid_spent(&world, &[BlockPos::new(0, 0, 0)]));
    }

    #[test]
    fn full_cargo_and_forged_targets_leave_the_pile_and_consumed_record_unchanged() {
        let mut world = world().clone();
        let mut active = extraction(&world);
        let mut spent = Vec::new();
        let mut ledger = PlayerEconomy {
            cargo: [24, 0, 0, 0, 0],
            ..Default::default()
        };
        assert!(complete(&mut world, &mut spent, &mut ledger, &active).is_err());
        assert_eq!(world.block(active.anchor), Block::Stone);
        assert!(spent.is_empty());
        assert_eq!(ledger.revision, 0);
        ledger.cargo[0] = 18;
        ledger.delivery = Some(rubblekin_core::economy::DeliveryContract {
            origin: 0,
            destination: 1,
            kind: ResourceKind::Food,
            amount: 6,
            reward: 12,
        });
        assert!(complete(&mut world, &mut spent, &mut ledger, &active).is_err());
        assert_eq!(world.block(active.anchor), Block::Stone);
        assert!(spent.is_empty());
        ledger.delivery = None;
        active.anchor.y = site(&world, active.progress.offer.site)
            .unwrap()
            .building
            .origin
            .y;
        ledger.cargo = [0; 5];
        assert!(complete(&mut world, &mut spent, &mut ledger, &active).is_err());
        assert!(spent.is_empty());
        assert_eq!(ledger.cargo_total(), 0);
    }
    fn reachable_work(world: &World, id: WorkSite) -> (local_work::ActiveWork, [f32; 3]) {
        let life = crate::villages::VillageLife::new(world);
        let building = &site(world, id).unwrap().building;
        for cell in cells(building) {
            for (dx, dz) in [(0, -2), (-2, 0), (0, 2), (2, 0)] {
                let position = [
                    (cell.x + dx) as f32 * CELL_SIZE + 0.25,
                    (building.origin.y + 1) as f32 * CELL_SIZE,
                    (cell.z + dz) as f32 * CELL_SIZE + 0.25,
                ];
                if rubblekin_core::physics::character_position_is_clear(world, position, &[])
                    && let Ok(work) = local_work::start(world, &life, &[], id, position, 10.0)
                {
                    return (work, position);
                }
            }
        }
        panic!("No reachable quarry work at {id:?}")
    }

    #[test]
    fn generated_piles_have_reachable_work_and_capture_one_visible_cell_for_six_seconds() {
        let world = world();
        let life = crate::villages::VillageLife::new(world);
        for id in sites(world) {
            let (mut active, position) = reachable_work(world, id);
            assert_eq!(world.block(active.anchor), Block::Stone);
            assert!(
                !local_work::advance(world, &life, &[], &mut active, position, 15.999).unwrap()
            );
            assert!(local_work::advance(world, &life, &[], &mut active, position, 16.0).unwrap());
            let mut changed = world.clone();
            changed.set_block(active.anchor, Block::Air).unwrap();
            assert!(
                local_work::advance(&changed, &life, &[], &mut active, position, 16.0).is_err()
            );
            assert!(
                local_work::advance(world, &life, &[active.anchor], &mut active, position, 16.0)
                    .is_err()
            );
            let mut away = position;
            away[0] += 1.0;
            assert!(local_work::advance(world, &life, &[], &mut active, away, 16.0).is_err());
            let floor = BlockPos::new(
                active.anchor.x,
                site(world, id).unwrap().building.origin.y,
                active.anchor.z,
            );
            assert!(world.block(floor).is_solid());
            assert!(available(world, id, floor, &[]).is_err());
        }
    }
}
