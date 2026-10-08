//! Small, editable village buildings made from the existing 50 cm voxel palette.
//!
//! Coordinates start at each asset's minimum corner. The floor is `y = 0`,
//! the entrance faces negative z, and every in-bounds cell has a value: air
//! clears room for occupants, roofs, and the open doorway. Rotation and terrain
//! foundations belong to the village plan, rather than the asset itself.

use crate::world::Block;
use serde::{Deserialize, Serialize};

#[path = "village_assets_exploration.rs"]
mod exploration;
#[path = "village_assets_regional.rs"]
mod regional;
#[path = "village_assets_scenic.rs"]
mod scenic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuildingKind {
    Cottage,
    Workshop,
    Storehouse,
    Market,
    TimberCabin,
    MasonryCottage,
    UplandHouse,
    Windmill,
    Lookout,
    TrailRuin,
    Waystone,
    TrailPavilion,
    QuarryYard,
    StoneArch,
    StandingStones,
    FallenGiant,
    TrailCamp,
    RuinedTower,
    AbandonedKiln,
    RidgeCairn,
    TrailBench,
    CartWreck,
    SurveyPost,
    DeadSnag,
    SplitBoulder,
    CliffDeck,
}

impl BuildingKind {
    pub const fn is_landmark(self) -> bool {
        matches!(
            self,
            Self::Windmill
                | Self::Lookout
                | Self::TrailRuin
                | Self::Waystone
                | Self::TrailPavilion
                | Self::QuarryYard
                | Self::StoneArch
                | Self::StandingStones
                | Self::FallenGiant
                | Self::TrailCamp
                | Self::RuinedTower
                | Self::AbandonedKiln
                | Self::RidgeCairn
                | Self::TrailBench
                | Self::CartWreck
                | Self::SurveyPost
                | Self::DeadSnag
                | Self::SplitBoulder
                | Self::CliffDeck
        )
    }

    pub const fn is_small_discovery(self) -> bool {
        matches!(
            self,
            Self::RidgeCairn
                | Self::TrailBench
                | Self::CartWreck
                | Self::SurveyPost
                | Self::DeadSnag
                | Self::SplitBoulder
                | Self::CliffDeck
        )
    }

    pub const fn is_exploration_site(self) -> bool {
        matches!(
            self,
            Self::StoneArch
                | Self::StandingStones
                | Self::FallenGiant
                | Self::TrailCamp
                | Self::RuinedTower
                | Self::AbandonedKiln
                | Self::RidgeCairn
                | Self::TrailBench
                | Self::CartWreck
                | Self::SurveyPost
                | Self::DeadSnag
                | Self::SplitBoulder
                | Self::CliffDeck
        )
    }
}

/// Width, height, and depth in voxel cells, including eaves and chimneys.
pub const fn dimensions(kind: BuildingKind) -> [i32; 3] {
    match kind {
        BuildingKind::Cottage => [14, 14, 16],
        BuildingKind::Workshop => [18, 13, 14],
        BuildingKind::Storehouse => [16, 13, 18],
        BuildingKind::Market => [16, 11, 12],
        BuildingKind::TimberCabin | BuildingKind::MasonryCottage => [14, 14, 16],
        BuildingKind::UplandHouse => [14, 18, 16],
        BuildingKind::Windmill => [24, 34, 22],
        BuildingKind::Lookout => [18, 22, 24],
        BuildingKind::TrailRuin => [24, 12, 24],
        BuildingKind::Waystone => [10, 12, 10],
        BuildingKind::TrailPavilion => [24, 16, 20],
        BuildingKind::QuarryYard => [28, 14, 24],
        BuildingKind::StoneArch => [36, 24, 24],
        BuildingKind::StandingStones => [30, 18, 30],
        BuildingKind::FallenGiant => [36, 16, 28],
        BuildingKind::TrailCamp => [28, 14, 28],
        BuildingKind::RuinedTower => [26, 28, 30],
        BuildingKind::AbandonedKiln => [28, 16, 28],
        BuildingKind::RidgeCairn => [10, 9, 12],
        BuildingKind::TrailBench => [12, 11, 12],
        BuildingKind::CartWreck => [20, 10, 16],
        BuildingKind::SurveyPost => [12, 13, 14],
        BuildingKind::DeadSnag => [12, 24, 12],
        BuildingKind::SplitBoulder => [16, 15, 16],
        BuildingKind::CliffDeck => [12, 6, 14],
    }
}

/// An unobstructed foot cell at the center of the front entrance.
pub const fn entrance(kind: BuildingKind) -> [i32; 3] {
    [dimensions(kind)[0] / 2, 1, 0]
}

/// Corner piles beneath the elevated viewing deck; ground limits their depth.
pub fn cliff_deck_support(x: i32, z: i32) -> bool {
    (x == 1 || x == 10) && (z == 1 || z == 12)
}

/// Collectable loose stone only; the quarry floor and structure are excluded.
pub fn quarry_pile_cells() -> impl Iterator<Item = [i32; 3]> {
    (20..=23).flat_map(|x| {
        (5..=11)
            .filter(|z| (z - 5) % 3 < 2)
            .flat_map(move |z| (1..=2 + (x - 20) / 2).map(move |y| [x, y, z]))
    })
}

pub fn block_at(kind: BuildingKind, x: i32, y: i32, z: i32) -> Option<Block> {
    let [width, height, depth] = dimensions(kind);
    if !(0..width).contains(&x) || !(0..height).contains(&y) || !(0..depth).contains(&z) {
        return None;
    }
    Some(match kind {
        BuildingKind::Cottage => cottage(x, y, z, width, depth),
        BuildingKind::Workshop => workshop(x, y, z, width, depth),
        BuildingKind::Storehouse => storehouse(x, y, z, width, depth),
        BuildingKind::Market => market(x, y, z, width, depth),
        BuildingKind::TimberCabin | BuildingKind::MasonryCottage | BuildingKind::UplandHouse => {
            regional::house(kind, x, y, z, width, depth)
        }
        BuildingKind::Windmill => regional::windmill(x, y, z),
        BuildingKind::Lookout => regional::lookout(x, y, z),
        BuildingKind::TrailRuin => scenic::ruin(x, y, z),
        BuildingKind::Waystone => scenic::waystone(x, y, z),
        BuildingKind::TrailPavilion => scenic::pavilion(x, y, z),
        BuildingKind::QuarryYard => scenic::quarry(x, y, z),
        BuildingKind::StoneArch => exploration::arch(x, y, z),
        BuildingKind::StandingStones => exploration::stones(x, y, z),
        BuildingKind::FallenGiant => exploration::fallen_giant(x, y, z),
        BuildingKind::TrailCamp => exploration::camp(x, y, z),
        BuildingKind::RuinedTower => exploration::tower(x, y, z),
        BuildingKind::AbandonedKiln => exploration::kiln(x, y, z),
        BuildingKind::RidgeCairn => exploration::cairn(x, y, z),
        BuildingKind::TrailBench => exploration::bench(x, y, z),
        BuildingKind::CartWreck => exploration::cart(x, y, z),
        BuildingKind::SurveyPost => exploration::survey(x, y, z),
        BuildingKind::DeadSnag => exploration::snag(x, y, z),
        BuildingKind::SplitBoulder => exploration::boulder(x, y, z),
        BuildingKind::CliffDeck => exploration::cliff_deck(x, y, z),
    })
}

fn front_door(x: i32, y: i32, z: i32, width: i32) -> bool {
    z <= 1 && (width / 2 - 1..=width / 2 + 1).contains(&x) && (1..=5).contains(&y)
}

fn cottage(x: i32, y: i32, z: i32, width: i32, depth: i32) -> Block {
    if y == 0 {
        return if x == 0 || x == width - 1 || z == depth - 1 {
            Block::Stone
        } else {
            Block::Wood
        };
    }
    if front_door(x, y, z, width) {
        return Block::Air;
    }
    // A brick chimney and stone hearth share one footprint at the rear side.
    if (width - 5..=width - 4).contains(&x) && (depth - 5..=depth - 4).contains(&z) {
        return if y <= 2 { Block::Stone } else { Block::Brick };
    }
    let roof_y = 7 + x.min(width - 1 - x) / 2;
    if y == roof_y {
        return Block::Brick;
    }
    if y > roof_y {
        return Block::Air;
    }
    let wall = x == 1 || x == width - 2 || z == 1 || z == depth - 2;
    let in_walls = (1..width - 1).contains(&x) && (1..depth - 1).contains(&z);
    if wall && in_walls {
        if y == 1 {
            return Block::Stone;
        }
        let window = (3..=4).contains(&y)
            && (((x == 1 || x == width - 2) && (5..=7).contains(&z))
                || (z == depth - 2 && (4..=6).contains(&x))
                || (z == 1 && (3..=4).contains(&x)));
        if window {
            return Block::Glass;
        }
        let timber = y == 6 || x == 1 || x == width - 2 || x == width / 2;
        return if timber { Block::Wood } else { Block::Sand };
    }
    if y == 1 {
        // Bed, bench, and a low table leave the central entrance aisle clear.
        if (2..=3).contains(&x) && (depth - 6..=depth - 3).contains(&z) {
            return Block::Snow;
        }
        if x == 2 && (3..=6).contains(&z)
            || (width - 5..=width - 4).contains(&x) && (4..=5).contains(&z)
        {
            return Block::Wood;
        }
    }
    Block::Air
}

fn workshop(x: i32, y: i32, z: i32, width: i32, depth: i32) -> Block {
    if y == 0 {
        return Block::Stone;
    }
    if front_door(x, y, z, width) {
        return Block::Air;
    }
    // A shed roof slopes toward the rear; the chimney identifies the hearth.
    let roof_y = 7 + (depth - 1 - z) / 3;
    if (3..=4).contains(&x) && (depth - 5..=depth - 4).contains(&z) {
        return if y <= 2 { Block::Stone } else { Block::Brick };
    }
    if y == roof_y {
        return if z % 3 == 0 {
            Block::Wood
        } else {
            Block::Brick
        };
    }
    if y > roof_y {
        return Block::Air;
    }
    let in_walls = (1..width - 1).contains(&x) && (1..depth - 1).contains(&z);
    let wall = x == 1 || x == width - 2 || z == 1 || z == depth - 2;
    if wall && in_walls {
        if (3..=4).contains(&y)
            && ((x == width - 2 && (4..=8).contains(&z)) || (z == 1 && (3..=5).contains(&x)))
        {
            return Block::Glass;
        }
        return if y <= 2 {
            Block::Stone
        } else if y == 6 || x == 1 || x == width - 2 || x % 5 == 1 {
            Block::Wood
        } else {
            Block::Sand
        };
    }
    // Side workbenches and a stone work surface, never across the doorway.
    if y == 1
        && (((width - 5..=width - 3).contains(&x) && (4..=depth - 4).contains(&z))
            || ((3..=4).contains(&x) && (3..=5).contains(&z)))
    {
        return Block::Wood;
    }
    if y == 2 && (width - 5..=width - 3).contains(&x) && (4..=depth - 4).contains(&z) {
        return Block::Stone;
    }
    Block::Air
}

fn storehouse(x: i32, y: i32, z: i32, width: i32, depth: i32) -> Block {
    if y == 0 {
        return Block::Stone;
    }
    if front_door(x, y, z, width) {
        return Block::Air;
    }
    // A lengthwise gable and timber walls read as a barn beside the cottages.
    let roof_y = 8 + x.min(width - 1 - x) / 2;
    if y == roof_y {
        return Block::Wood;
    }
    if y > roof_y {
        return Block::Air;
    }
    let in_walls = (1..width - 1).contains(&x) && (1..depth - 1).contains(&z);
    let wall = x == 1 || x == width - 2 || z == 1 || z == depth - 2;
    if wall && in_walls {
        if y == 1 {
            return Block::Stone;
        }
        if (4..=5).contains(&y) && (x == 1 || x == width - 2) && (6..=8).contains(&z) {
            return Block::Glass;
        }
        let frame = ((x == 1 || x == width - 2) && z % 5 == 1)
            || ((z == 1 || z == depth - 2) && x % 5 == 1);
        return if y == 7 || frame {
            Block::Stone
        } else {
            Block::Wood
        };
    }
    // Repeated storage bins and stacked grain sacks flank an open aisle.
    let side_bin = (2..=4).contains(&x) || (width - 5..=width - 3).contains(&x);
    if side_bin && (4..=depth - 4).contains(&z) && z % 4 != 0 && y <= 2 {
        return if y == 2 && z % 4 == 2 {
            Block::Sand
        } else {
            Block::Wood
        };
    }
    Block::Air
}

fn market(x: i32, y: i32, z: i32, width: i32, depth: i32) -> Block {
    if y == 0 {
        return if x % 3 == 0 || z % 3 == 0 {
            Block::Stone
        } else {
            Block::Dirt
        };
    }
    let roof_y = 8 + x.min(width - 1 - x) / 4;
    if y == roof_y {
        // Alternating warm cloth bands distinguish the open trading awning.
        return if x / 3 % 2 == 0 {
            Block::Sand
        } else {
            Block::Brick
        };
    }
    let post = (x == 1 || x == width - 2) && (z == 1 || z == depth - 2);
    if post && y < roof_y {
        return if y == 1 { Block::Stone } else { Block::Wood };
    }
    if y == 7 && (x == 1 || x == width - 2 || z == 1 || z == depth - 2) {
        return Block::Wood;
    }
    let counter = (2..=5).contains(&x) || (width - 6..=width - 3).contains(&x);
    if counter && (6..=depth - 3).contains(&z) {
        if y <= 2 {
            return Block::Wood;
        }
        if y == 3 && z == depth - 3 && x % 3 != 0 {
            return Block::Sand;
        }
    }
    Block::Air
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        physics::{Body, MoveInput, PLAYER_HEIGHT, PLAYER_RADIUS, move_character},
        world::{BlockPos, CELL_SIZE, World, WorldGeneration},
    };

    const KINDS: [BuildingKind; 4] = [
        BuildingKind::Cottage,
        BuildingKind::Workshop,
        BuildingKind::Storehouse,
        BuildingKind::Market,
    ];

    #[test]
    fn geography_v3_building_signature_stays_frozen() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let mut signature = 0xcbf29ce484222325_u64;
        let mut add = |n: u32| {
            for b in n.to_le_bytes() {
                signature ^= u64::from(b);
                signature = signature.wrapping_mul(0x100000001b3);
            }
        };
        for village in &world.settlements().unwrap().villages {
            add(village.id);
            for b in &village.buildings {
                for n in [b.origin.x, b.origin.y, b.origin.z, i32::from(b.rotation)] {
                    add(n as u32);
                }
                let [w, h, d] = dimensions(b.kind);
                for x in 0..w {
                    for y in 0..h {
                        for z in 0..d {
                            add(block_at(b.kind, x, y, z).unwrap() as u32);
                        }
                    }
                }
            }
            for route in &village.resident_routes {
                for p in &route.path {
                    for n in p {
                        add(n.to_bits());
                    }
                }
            }
        }
        assert_eq!(signature, 4_310_952_318_501_764_209);
    }

    #[test]
    fn assets_are_bounded_small_and_leave_character_sized_entrances() {
        let mut solids = 0;
        for kind in KINDS {
            let [width, height, depth] = dimensions(kind);
            assert!(width * depth <= 320, "{kind:?} has an excessive footprint");
            assert!(
                height <= 16,
                "{kind:?} exceeds the local mesh height budget"
            );
            let [door_x, door_y, door_z] = entrance(kind);
            let opening_width = (0..width)
                .filter(|x| block_at(kind, *x, door_y, door_z + 1) == Some(Block::Air))
                .count() as f32
                * CELL_SIZE;
            let opening_height = (door_y..height)
                .take_while(|y| block_at(kind, door_x, *y, door_z + 1) == Some(Block::Air))
                .count() as f32
                * CELL_SIZE;
            assert!(opening_width > PLAYER_RADIUS * 2.0, "{kind:?}");
            assert!(opening_height > PLAYER_HEIGHT, "{kind:?}");
            for x in 0..width {
                for y in 0..height {
                    for z in 0..depth {
                        solids += block_at(kind, x, y, z).unwrap().is_solid() as usize;
                    }
                }
            }
            for x in door_x - 1..=door_x + 1 {
                for y in door_y..door_y + 5 {
                    for z in door_z..=door_z + 1 {
                        assert_eq!(block_at(kind, x, y, z), Some(Block::Air), "{kind:?}");
                    }
                }
                assert!(block_at(kind, x, 0, door_z).unwrap().is_solid());
            }
            for outside in [
                [-1, 0, 0],
                [width, 0, 0],
                [0, -1, 0],
                [0, height, 0],
                [0, 0, -1],
                [0, 0, depth],
            ] {
                assert_eq!(block_at(kind, outside[0], outside[1], outside[2]), None);
            }
        }
        assert!(solids < 8_000, "asset solid-cell budget exceeded: {solids}");
    }

    #[test]
    fn every_building_can_be_entered_using_the_real_character_controller() {
        let mut world = World::new(42);
        for (index, kind) in KINDS.into_iter().enumerate() {
            let [width, height, depth] = dimensions(kind);
            let origin = [-65 + index as i32 * 35, 100, -30];
            // One level landing joins the asset floor with a village lane.
            for x in -2..width + 2 {
                for z in -5..depth + 2 {
                    for y in 0..height {
                        let block = block_at(kind, x, y, z).unwrap_or(if y == 0 {
                            Block::Stone
                        } else {
                            Block::Air
                        });
                        world
                            .set_block(
                                BlockPos::new(origin[0] + x, origin[1] + y, origin[2] + z),
                                block,
                            )
                            .unwrap();
                    }
                }
            }
            let [door_x, _, _] = entrance(kind);
            let mut body = Body::new([
                (origin[0] + door_x) as f32 * CELL_SIZE + CELL_SIZE / 2.0,
                (origin[1] + 1) as f32 * CELL_SIZE,
                (origin[2] - 2) as f32 * CELL_SIZE,
            ]);
            let floor = body.position[1];
            for _ in 0..8 {
                move_character(
                    &world,
                    &mut body,
                    MoveInput {
                        direction: [0.0, 1.0],
                        ..Default::default()
                    },
                    0.1,
                );
            }
            assert!(
                body.position[2] > (origin[2] + 3) as f32 * CELL_SIZE,
                "{kind:?} blocked entrance: {:?}",
                body.position
            );
            assert!((body.position[1] - floor).abs() < 0.01, "{kind:?}");
        }
    }

    #[test]
    fn generated_rotated_entrances_join_their_actual_village_landings() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let plan = world.settlements().unwrap();
        assert!(!plan.villages.is_empty());
        for village in &plan.villages {
            for building in &village.buildings {
                let mut body = Body::new(building.entrance());
                let floor = body.position[1];
                let direction = match building.rotation % 4 {
                    0 => [0.0, 1.0],
                    1 => [-1.0, 0.0],
                    2 => [0.0, -1.0],
                    _ => [1.0, 0.0],
                };
                for _ in 0..8 {
                    move_character(
                        &world,
                        &mut body,
                        MoveInput {
                            direction,
                            ..Default::default()
                        },
                        0.1,
                    );
                }
                let start = building.entrance();
                let progress = (body.position[0] - start[0]) * direction[0]
                    + (body.position[2] - start[2]) * direction[1];
                assert!(
                    progress > 2.5,
                    "{} {:?} rotation {} entrance blocked after {progress:.2}m: {:?}",
                    village.name,
                    building.kind,
                    building.rotation,
                    body.position
                );
                assert!(
                    (body.position[1] - floor).abs() < 0.02,
                    "{} {:?} entrance floor mismatch: {:?}",
                    village.name,
                    building.kind,
                    body.position
                );
            }
        }
    }

    #[test]
    fn generated_residents_can_walk_home_store_work_and_back() {
        let mut failures = Vec::new();
        for seed in [42, 7, 99] {
            let world = World::generate(seed, WorldGeneration::GeographyV3);
            let plan = world.settlements().unwrap();
            assert!(!plan.villages.is_empty(), "seed {seed} has no villages");
            for village in &plan.villages {
                for (route_index, route) in village.resident_routes.iter().enumerate() {
                    let mut body = Body::new(route.home);
                    for destination in route
                        .path
                        .iter()
                        .skip(1)
                        .chain(route.path.iter().rev().skip(1))
                    {
                        let mut reached = false;
                        for _ in 0..100 {
                            let dx = destination[0] - body.position[0];
                            let dz = destination[2] - body.position[2];
                            let distance = dx.hypot(dz);
                            if distance < 0.35 && (body.position[1] - destination[1]).abs() < 0.8 {
                                reached = true;
                                break;
                            }
                            let direction = if distance > 0.03 {
                                let factor = 0.52_f32.min(distance / (3.8 / 30.0));
                                [dx / distance * factor, dz / distance * factor]
                            } else {
                                [0.0; 2]
                            };
                            move_character(
                                &world,
                                &mut body,
                                MoveInput {
                                    direction,
                                    ..Default::default()
                                },
                                1.0 / 30.0,
                            );
                        }
                        if !reached {
                            failures.push(format!(
                            "seed {seed}: {} resident {route_index} blocked approaching {destination:?} from {:?}",
                            village.name, body.position
                        ));
                            break;
                        }
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn roofs_have_slopes_and_assets_have_distinct_material_silhouettes() {
        let top = |kind, x, z| {
            let height = dimensions(kind)[1];
            (1..height)
                .rev()
                .find(|y| block_at(kind, x, *y, z).unwrap().is_solid())
                .unwrap()
        };
        for kind in [BuildingKind::Cottage, BuildingKind::Storehouse] {
            let [width, _, depth] = dimensions(kind);
            assert!(top(kind, width / 2, depth / 2) > top(kind, 0, depth / 2));
        }
        let [width, _, depth] = dimensions(BuildingKind::Workshop);
        assert!(
            top(BuildingKind::Workshop, width / 2, 0)
                > top(BuildingKind::Workshop, width / 2, depth - 1)
        );
        assert_eq!(block_at(BuildingKind::Market, 0, 4, 6), Some(Block::Air));
        assert_eq!(block_at(BuildingKind::Market, 1, 4, 1), Some(Block::Wood));
    }
}
