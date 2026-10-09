use super::*;
use crate::{
    physics::{Body, MoveInput, move_character},
    poi::{SiteArrangement, SitePlan},
};

fn walk(world: &World, body: &mut Body, target: [f32; 3]) {
    for _ in 0..1000 {
        let dx = target[0] - body.position[0];
        let dz = target[2] - body.position[2];
        let d = dx.hypot(dz);
        if d < 0.2 && (body.position[1] - target[1]).abs() < 0.55 {
            return;
        }
        let amount = (d / 0.2).min(0.6);
        move_character(
            world,
            body,
            MoveInput {
                direction: if d > 0.02 {
                    [dx / d * amount, dz / d * amount]
                } else {
                    [0.; 2]
                },
                ..Default::default()
            },
            0.05,
        );
    }
    let x = (body.position[0] / CELL_SIZE).floor() as i32;
    let z = (body.position[2] / CELL_SIZE).floor() as i32;
    let neighborhood: Vec<_> = (z - 3..=z + 3)
        .map(|q| {
            (
                q,
                world.height_at(x, q),
                (world.height_at(x, q) + 1..world.height_at(x, q) + 25)
                    .filter(|&y| world.block(BlockPos::new(x, y, q)).is_solid())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    panic!(
        "composed route blocked: {:?} -> {target:?}; {neighborhood:?}",
        body.position
    );
}

#[test]
fn thirty_six_terrain_adapted_recipes_have_walkable_returns_and_sparse_occupancy() {
    let base = World::generate(42, WorldGeneration::GeographyV6);
    let plan = base.settlements().unwrap();
    for arrangement in SiteArrangement::ALL {
        for n in 0..4 {
            let site = plan
                .roadside_landmarks
                .iter()
                .find_map(|s| {
                    let p = s.building.entrance();
                    SitePlan::fit(
                        (42u64 << 32) | (n * 5),
                        arrangement,
                        base.geography().unwrap(),
                        p[0],
                        p[2],
                    )
                })
                .unwrap();
            let mut world = base.clone();
            let mut standalone = plan.clone();
            // These examples occupy real terrain without the old single asset.
            standalone.roadside_landmarks.clear();
            standalone.composed_sites = vec![site.clone()];
            standalone.rebuild_poi_index();
            world.settlements = Some(Arc::new(standalone));
            world.geographic_columns = Arc::new(RwLock::new(HashMap::new()));
            let mut body = Body::new(site.entrance());
            for &target in site.route.iter().skip(1) {
                walk(&world, &mut body, target);
            }
            for &target in site.route.iter().rev().skip(1) {
                walk(&world, &mut body, target);
            }
            let [x0, z0, x1, z1] = site.bounds;
            let mut untouched = 0;
            for z in z0..z1 {
                for x in x0..x1 {
                    let original = (world
                        .geography()
                        .unwrap()
                        .sample((x as f32 + 0.5) * CELL_SIZE, (z as f32 + 0.5) * CELL_SIZE)
                        .height
                        / CELL_SIZE)
                        .floor() as i32;
                    let col = world.geographic_column(x, z).unwrap();
                    if col.site.is_none() && col.height == original {
                        untouched += 1;
                    }
                    if let Some(s) = col.site {
                        assert!(s.top() < world.max_y());
                    }
                }
            }
            assert!(
                untouched > 100,
                "broad bounds must not become a filled plot"
            );
        }
    }
}

#[test]
fn bounded_world_batch_is_reproducible_and_edits_survive_regeneration() {
    for seed in [42, 7, 99] {
        let mut world = World::generate(seed, WorldGeneration::GeographyV6);
        let sites = world.settlements().unwrap().composed_sites.clone();
        assert!(
            (18..=36).contains(&sites.len()),
            "seed {seed}: {} sites",
            sites.len()
        );
        let kinds: std::collections::HashSet<_> = sites.iter().map(|s| s.arrangement).collect();
        assert_eq!(kinds.len(), 9, "seed {seed}");
        let ids: std::collections::HashSet<_> = sites.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), sites.len());
        for site in &sites {
            let mut body = Body::new(site.entrance());
            for &target in site.route.iter().skip(1) {
                walk(&world, &mut body, target);
            }
            for &target in site.route.iter().rev().skip(1) {
                walk(&world, &mut body, target);
            }
        }
        let solid = sites[0].solids[0];
        let p = solid.min;
        assert_eq!(world.block(p), solid.block);
        world.set_block(p, Block::Air).unwrap();
        let q = BlockPos::new(p.x, p.y + 1, p.z);
        world.set_block(q, Block::PurpleWool).unwrap();
        let restored =
            World::from_generation_edits(seed, WorldGeneration::GeographyV6, &world.edits())
                .unwrap();
        assert_eq!(restored.settlements().unwrap().composed_sites, sites);
        assert_eq!(restored.block(p), Block::Air);
        assert_eq!(restored.block(q), Block::PurpleWool);
    }
}
