//! GeographyV4 assets. Original village assets remain unchanged for old saves.
use super::{BuildingKind, cottage, front_door};
use crate::world::Block;

pub(super) fn house(kind: BuildingKind, x: i32, y: i32, z: i32, w: i32, d: i32) -> Block {
    if y == 0 || front_door(x, y, z, w) {
        return cottage(x, y, z, w, d);
    }
    if (w - 5..=w - 4).contains(&x) && (d - 5..=d - 4).contains(&z) {
        return if y <= 2 { Block::Stone } else { Block::Brick };
    }
    let inset = x.min(w - 1 - x);
    let roof = match kind {
        BuildingKind::TimberCabin => 7 + inset / 2,
        BuildingKind::MasonryCottage => 7 + inset.min(z.min(d - 1 - z)) / 2,
        _ => 7 + inset,
    };
    if y == roof {
        return match kind {
            BuildingKind::TimberCabin => Block::Wood,
            BuildingKind::MasonryCottage => Block::Stone,
            _ => Block::Brick,
        };
    }
    if y > roof {
        return Block::Air;
    }
    let wall = (x == 1 || x == w - 2 || z == 1 || z == d - 2)
        && (1..w - 1).contains(&x)
        && (1..d - 1).contains(&z);
    if wall {
        // Upper gable windows and thick corner timbers distinguish the silhouettes.
        if (3..=4).contains(&y)
            && (((x == 1 || x == w - 2) && (5..=7).contains(&z))
                || ((z == 1 || z == d - 2) && (3..=4).contains(&x)))
            || kind == BuildingKind::UplandHouse
                && (8..=9).contains(&y)
                && (z == 1 || z == d - 2)
                && (6..=7).contains(&x)
        {
            return Block::Glass;
        }
        return match kind {
            BuildingKind::TimberCabin => {
                if y == 1 {
                    Block::Stone
                } else {
                    Block::Wood
                }
            }
            BuildingKind::MasonryCottage => {
                if y == 1 || x == 1 || x == w - 2 {
                    Block::Stone
                } else {
                    Block::Sand
                }
            }
            _ => {
                if y == 6 || x == 1 || x == w - 2 || x == w / 2 {
                    Block::Wood
                } else {
                    Block::Sand
                }
            }
        };
    }
    // Keep beds, benches, hearths and the clear central aisle in every region.
    if y <= 2 {
        cottage(x, y, z, w, d)
    } else {
        Block::Air
    }
}

pub(super) fn windmill(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return if (8..=15).contains(&x) && (6..=15).contains(&z) {
            Block::Wood
        } else {
            Block::Stone
        };
    }
    // A stationary four-sail cross is made of the same editable cells as the mill.
    let dx = x - 12;
    let dy = y - 22;
    if (2..=3).contains(&z) && dx.abs().max(dy.abs()) <= 10 {
        if (dx - dy).abs() <= 1 || (dx + dy).abs() <= 1 {
            return Block::Wood;
        }
        if dx.abs().max(dy.abs()) >= 4 && ((dx - dy - 2).abs() <= 1 || (dx + dy + 2).abs() <= 1) {
            return Block::Sand;
        }
    }
    if (11..=12).contains(&x) && (21..=22).contains(&y) && (4..=7).contains(&z) {
        return Block::Wood;
    }
    if (7..=16).contains(&x) && (5..=16).contains(&z) {
        let roof = 21 + (x - 7).min(16 - x);
        if y == roof {
            return Block::Wood;
        }
        if y > roof {
            return Block::Air;
        }
        let wall = x == 8 || x == 15 || z == 6 || z == 15;
        if wall && (8..=15).contains(&x) && (6..=15).contains(&z) {
            if z == 6 && (11..=13).contains(&x) && y <= 5 {
                return Block::Air;
            }
            if (10..=12).contains(&y)
                && ((x == 8 || x == 15) && (10..=11).contains(&z)
                    || z == 15 && (11..=12).contains(&x))
            {
                return Block::Glass;
            }
            return if y <= 3 || y == 18 {
                Block::Stone
            } else {
                Block::Sand
            };
        }
        if y <= 2 && (9..=10).contains(&x) && (11..=13).contains(&z) {
            return Block::Stone;
        }
        if y == 1 && (13..=14).contains(&x) && (12..=14).contains(&z) {
            return Block::Sand;
        }
    }
    Block::Air
}

pub(super) fn lookout(x: i32, y: i32, z: i32) -> Block {
    if y == 0 {
        return Block::Stone;
    }
    if (1..=16).contains(&x) && (1..=22).contains(&z) {
        let perimeter = x == 1 || x == 16 || z == 1 || z == 22;
        if perimeter && y <= 6 && !front_door(x, y, z, 18) {
            return Block::Stone;
        }
        if (x == 2 || x == 15) && (z == 2 || z == 21) && y <= 19 {
            return Block::Wood;
        }
        // Two broad stair flights, a turning landing and a top opening. Each
        // 50 cm riser is within the shared controller's supported step height.
        if (3..=5).contains(&x) && (4..=15).contains(&z) && y <= 1 + (z - 4) / 2 {
            return Block::Wood;
        }
        if (3..=13).contains(&x) && (16..=19).contains(&z) && y == 6 {
            return Block::Wood;
        }
        if (11..=13).contains(&x) && (4..=15).contains(&z) && y <= 7 + (15 - z) / 2 {
            return Block::Wood;
        }
        let stair_opening = (10..=14).contains(&x) && (4..=16).contains(&z);
        if y == 13 && !stair_opening {
            return Block::Wood;
        }
        if perimeter && (14..=15).contains(&y) {
            return Block::Wood;
        }
        if y == 19 + (x - 1).min(16 - x) / 3 {
            return Block::Brick;
        }
    }
    Block::Air
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        physics::{Body, MoveInput, move_character},
        village_assets::{block_at, dimensions},
        world::{BlockPos, CELL_SIZE, World, WorldGeneration},
    };

    fn walk(world: &World, body: &mut Body, target: [f32; 2]) {
        for _ in 0..900 {
            let dx = target[0] - body.position[0];
            let dz = target[1] - body.position[2];
            let distance = libm::hypotf(dx, dz);
            if distance < 0.08 {
                return;
            }
            let speed = (distance / 0.13).min(0.6);
            move_character(
                world,
                body,
                MoveInput {
                    direction: [dx / distance * speed, dz / distance * speed],
                    ..Default::default()
                },
                1.0 / 30.0,
            );
        }
        panic!("blocked towards {target:?}, feet {:?}", body.position);
    }

    #[test]
    fn regional_interiors_and_lookout_stairs_use_real_character_physics() {
        for kind in [
            BuildingKind::TimberCabin,
            BuildingKind::MasonryCottage,
            BuildingKind::UplandHouse,
            BuildingKind::Windmill,
            BuildingKind::Lookout,
        ] {
            let mut world = World::new(42);
            let [w, h, d] = dimensions(kind);
            for x in -2..w + 2 {
                for z in -5..d + 2 {
                    for y in 0..h + 3 {
                        world
                            .set_block(
                                BlockPos::new(x, 100 + y, z),
                                block_at(kind, x, y, z).unwrap_or(if y == 0 {
                                    Block::Stone
                                } else {
                                    Block::Air
                                }),
                            )
                            .unwrap();
                    }
                }
            }
            let mut body = Body::new([(w / 2) as f32 * CELL_SIZE + 0.25, 50.5, -1.0]);
            let x = body.position[0];
            walk(
                &world,
                &mut body,
                [
                    x,
                    if kind == BuildingKind::Windmill {
                        5.25
                    } else {
                        2.25
                    },
                ],
            );
            assert!((body.position[1] - 50.5).abs() < 0.1, "{kind:?}");
            if kind == BuildingKind::Lookout {
                for [x, z] in [
                    [9.5, 3.0],
                    [4.5, 3.0],
                    [4.5, 17.5],
                    [12.5, 17.5],
                    [12.5, 3.5],
                    [8.5, 3.5],
                ] {
                    walk(&world, &mut body, [x * CELL_SIZE, z * CELL_SIZE]);
                }
                assert!(
                    (body.position[1] - 57.0).abs() < 0.1,
                    "lookout deck {:?}",
                    body.position
                );
                for [x, z] in [
                    [12.5, 3.5],
                    [12.5, 17.5],
                    [4.5, 17.5],
                    [4.5, 3.5],
                    [9.5, 3.5],
                    [9.5, -2.0],
                ] {
                    walk(&world, &mut body, [x * CELL_SIZE, z * CELL_SIZE]);
                }
                assert!(
                    (body.position[1] - 50.5).abs() < 0.1,
                    "lookout exit {:?}",
                    body.position
                );
            }
        }
    }

    #[test]
    fn geography_v4_preserves_working_plots_and_adds_reproducible_editable_landmarks() {
        for seed in [42, 7, 99] {
            let old = World::generate(seed, WorldGeneration::GeographyV3);
            let mut world = World::generate(seed, WorldGeneration::GeographyV4);
            let plan = world.settlements().unwrap();
            assert_eq!(
                old.geography().unwrap().heights(),
                world.geography().unwrap().heights()
            );
            let mut landmarks = 0;
            for (a, b) in old
                .settlements()
                .unwrap()
                .villages
                .iter()
                .zip(&plan.villages)
            {
                assert_eq!(a.id, b.id);
                assert_eq!(a.resident_routes, b.resident_routes);
                assert_eq!(a.fields, b.fields);
                assert_eq!(a.market, b.market);
                assert_eq!(a.store, b.store);
                assert_eq!(&a.buildings[6..9], &b.buildings[6..9]);
                for (x, y) in a.buildings.iter().zip(&b.buildings) {
                    assert_eq!(x.origin, y.origin);
                    assert_eq!(x.entrance(), y.entrance());
                }
                let mut kinds = Vec::new();
                for p in &b.buildings {
                    if !kinds.contains(&p.kind) {
                        kinds.push(p.kind);
                    }
                }
                if let Some(landmark) = b.buildings.get(9) {
                    assert!(landmark.kind.is_landmark());
                    assert_eq!(
                        b.lanes.last().unwrap().points.last(),
                        Some(&landmark.entrance())
                    );
                    landmarks += 1;
                    let [w, _, d] = landmark.dimensions();
                    let aabb = [
                        landmark.origin.x,
                        landmark.origin.z,
                        landmark.origin.x + w,
                        landmark.origin.z + d,
                    ];
                    let separated = |other: [i32; 4]| {
                        aabb[2] <= other[0]
                            || aabb[0] >= other[2]
                            || aabb[3] <= other[1]
                            || aabb[1] >= other[3]
                    };
                    for other in &b.buildings[..9] {
                        let [w, _, d] = other.dimensions();
                        assert!(separated([
                            other.origin.x,
                            other.origin.z,
                            other.origin.x + w,
                            other.origin.z + d
                        ]));
                    }
                    for field in &b.fields {
                        assert!(separated([
                            field.origin.x,
                            field.origin.z,
                            field.origin.x + field.width,
                            field.origin.z + field.depth
                        ]));
                    }
                    // All original lane samples stay outside the new footprint.
                    for p in a.lanes.iter().flat_map(|l| &l.points) {
                        assert!(
                            landmark
                                .local_cell(
                                    (p[0] / CELL_SIZE).floor() as i32,
                                    (p[2] / CELL_SIZE).floor() as i32
                                )
                                .is_none()
                        );
                    }
                }
                assert!(kinds.len() >= 5, "{} lacks regional homes", b.name);
            }
            assert!(landmarks >= 4, "seed {seed}: only {landmarks} landmarks");
            let plot = &plan
                .villages
                .iter()
                .find(|v| v.buildings.len() > 9)
                .unwrap()
                .buildings[9];
            let edit = plot.origin;
            world.set_block(edit, Block::Air).unwrap();
            let restored =
                World::from_generation_edits(seed, WorldGeneration::GeographyV4, &world.edits())
                    .unwrap();
            assert_eq!(restored.settlements(), world.settlements());
            assert_eq!(restored.block(edit), Block::Air);
            assert_eq!(restored.edits(), world.edits());
        }
    }

    #[test]
    fn generated_v4_landmarks_roads_and_airships_remain_physically_accessible() {
        for seed in [42, 7, 99] {
            let world = World::generate(seed, WorldGeneration::GeographyV4);
            let network = crate::airships::AirshipNetwork::try_new(&world).unwrap();
            assert!(!network.routes().is_empty());
            for village in &world.settlements().unwrap().villages {
                for route in &village.resident_routes {
                    let mut body = Body::new(route.home);
                    for p in route
                        .path
                        .iter()
                        .skip(1)
                        .chain(route.path.iter().rev().skip(1))
                    {
                        walk(&world, &mut body, [p[0], p[2]]);
                        assert!(
                            (body.position[1] - p[1]).abs() < 0.8,
                            "seed{seed} {} resident left its road at {p:?}",
                            village.name
                        );
                    }
                }
                let Some(plot) = village.buildings.get(9) else {
                    continue;
                };
                let lane = village.lanes.last().unwrap();
                let mut body = Body::new(lane.points[0]);
                for p in lane.points.iter().skip(1) {
                    walk(&world, &mut body, [p[0], p[2]]);
                }
                let [w, _, d] = dimensions(plot.kind).map(|n| n as f32);
                let local = |[x, z]: [f32; 2]| {
                    let [x, z] = match plot.rotation % 4 {
                        0 => [x, z],
                        1 => [d - z, x],
                        2 => [w - x, d - z],
                        _ => [z, w - x],
                    };
                    [
                        (plot.origin.x as f32 + x) * CELL_SIZE,
                        (plot.origin.z as f32 + z) * CELL_SIZE,
                    ]
                };
                if plot.kind == BuildingKind::Lookout {
                    for p in [
                        [9.5, 3.0],
                        [4.5, 3.0],
                        [4.5, 17.5],
                        [12.5, 17.5],
                        [12.5, 3.5],
                        [8.5, 3.5],
                    ] {
                        walk(&world, &mut body, local(p));
                    }
                    assert!(
                        (body.position[1] - (plot.origin.y + 14) as f32 * CELL_SIZE).abs() < 0.1,
                        "seed{seed} {} lookout deck {:?}",
                        village.name,
                        body.position
                    );
                    for p in [
                        [12.5, 3.5],
                        [12.5, 17.5],
                        [4.5, 17.5],
                        [4.5, 3.0],
                        [9.5, 3.0],
                        [9.5, -3.0],
                    ] {
                        walk(&world, &mut body, local(p));
                    }
                } else {
                    walk(&world, &mut body, local([12.5, 10.5]));
                    walk(&world, &mut body, local([12.5, -3.0]));
                }
                for p in lane.points.iter().rev() {
                    walk(&world, &mut body, [p[0], p[2]]);
                }
            }
        }
    }

    #[test]
    fn landmark_aprons_clear_whole_tree_crowns_and_keep_surrounding_woods() {
        for seed in [42, 7, 99] {
            let original = World::generate(seed, WorldGeneration::GeographyV3);
            let world = World::generate(seed, WorldGeneration::GeographyV4);
            let mut removed = 0;
            let mut retained = 0;
            for village in &world.settlements().unwrap().villages {
                let Some(plot) = village.buildings.get(9) else {
                    continue;
                };
                let [w, _, d] = plot.dimensions();
                let bounds = [
                    plot.origin.x as f32 * CELL_SIZE,
                    plot.origin.z as f32 * CELL_SIZE,
                    (plot.origin.x + w) as f32 * CELL_SIZE,
                    (plot.origin.z + d) as f32 * CELL_SIZE,
                ];
                for gx in plot.origin.x.div_euclid(24) - 7..=plot.origin.x.div_euclid(24) + 7 {
                    for gz in plot.origin.z.div_euclid(24) - 7..=plot.origin.z.div_euclid(24) + 7 {
                        let Some(tree) = original.tree_at(gx, gz) else {
                            continue;
                        };
                        let x = (tree.base.x as f32 + 0.5) * CELL_SIZE;
                        let z = (tree.base.z as f32 + 0.5) * CELL_SIZE;
                        let crown = (tree.crown_radius as f32 + 0.5) * CELL_SIZE;
                        if x + crown >= bounds[0] - 12.0
                            && x - crown <= bounds[2] + 12.0
                            && z + crown >= bounds[1] - 12.0
                            && z - crown <= bounds[3] + 12.0
                        {
                            assert!(
                                world.tree_at(gx, gz).is_none(),
                                "seed{seed} {} {:?} canopy occupies landmark apron",
                                village.name,
                                tree.base
                            );
                            removed += 1;
                        }
                        if (x < bounds[0] - 25.0
                            || x > bounds[2] + 25.0
                            || z < bounds[1] - 25.0
                            || z > bounds[3] + 25.0)
                            && world.tree_at(gx, gz) == Some(tree)
                        {
                            retained += 1;
                        }
                    }
                }
            }
            assert!(removed > 0, "seed{seed} never exercised apron clearing");
            assert!(retained > 0, "seed{seed} lost surrounding woods");
        }
    }
}
