//! Dense pathless discoveries across dry land, independent of the route graph.
use super::*;
use crate::{airships::AirshipNetwork, geography::WORLD_SIZE};

const SPACING: f32 = 360.0;
const SEPARATION: f32 = 200.0;
const PATH_CLEARANCE: f32 = 120.0;

impl SettlementPlan {
    pub(crate) fn add_wilderness_sites(&mut self, world: &World, transit: &AirshipNetwork) {
        let geography = world.geography().expect("wilderness sites need geography");
        let bucket = |x: f32, z: f32| {
            (
                (x / SEPARATION).floor() as i32,
                (z / SEPARATION).floor() as i32,
            )
        };
        let mut occupied: HashMap<_, Vec<[f32; 2]>> = HashMap::new();
        for site in &self.roadside_landmarks {
            let p = site.building.entrance();
            occupied
                .entry(bucket(p[0], p[2]))
                .or_default()
                .push([p[0], p[2]]);
        }
        let docks: Vec<_> = transit
            .ports()
            .iter()
            .flat_map(|p| transit.dock_positions(p.village_id))
            .collect();
        let side = (WORLD_SIZE / SPACING).ceil() as i32;
        for gz in 0..side {
            for gx in 0..side {
                // One discovery per irregular cell, with bounded local retries
                // for water and slopes. No path is generated to these sites.
                for attempt in 0..6 {
                    let h = hash(gx * 7 + attempt, gz, world.seed.wrapping_add(18_319));
                    let jitter = |bits: u32| (bits & 1023) as f32 / 1023.0 * 160.0 - 80.0;
                    let x = -WORLD_SIZE * 0.5 + (gx as f32 + 0.5) * SPACING + jitter(h);
                    let z = -WORLD_SIZE * 0.5 + (gz as f32 + 0.5) * SPACING + jitter(h >> 10);
                    if x.abs().max(z.abs()) > WORLD_SIZE * 0.5 - 32.0 {
                        continue;
                    }
                    let sample = geography.sample(x, z);
                    if sample.water.is_some() || sample.biome == Biome::Ocean {
                        continue;
                    }
                    let key = bucket(x, z);
                    if (-1..=1).any(|dz| {
                        (-1..=1).any(|dx| {
                            occupied
                                .get(&(key.0 + dx, key.1 + dz))
                                .is_some_and(|sites| {
                                    sites
                                        .iter()
                                        .any(|p| distance2(x, z, p[0], p[1]) < SEPARATION.powi(2))
                                })
                        })
                    }) || self
                        .villages
                        .iter()
                        .any(|v| distance2(x, z, v.center[0], v.center[2]) < 220.0_f32.powi(2))
                        || !self.away_from_paths(x, z, PATH_CLEARANCE)
                        || docks
                            .iter()
                            .any(|p| distance2(x, z, p[0], p[2]) < 80.0_f32.powi(2))
                        || transit
                            .ramps()
                            .iter()
                            .any(|r| segment_distance(x, z, r.from, r.to).0 < 70.0)
                    {
                        continue;
                    }
                    let wooded = matches!(
                        sample.biome,
                        Biome::Forest | Biome::PineForest | Biome::Rainforest | Biome::Grassland
                    );
                    let kinds = [
                        BuildingKind::StoneArch,
                        BuildingKind::StandingStones,
                        if wooded {
                            BuildingKind::FallenGiant
                        } else {
                            BuildingKind::SplitBoulder
                        },
                        BuildingKind::TrailCamp,
                        BuildingKind::RuinedTower,
                        BuildingKind::RidgeCairn,
                        if wooded {
                            BuildingKind::DeadSnag
                        } else {
                            BuildingKind::StandingStones
                        },
                        BuildingKind::SplitBoulder,
                        BuildingKind::CartWreck,
                        BuildingKind::AbandonedKiln,
                    ];
                    let mut kind = kinds[((h >> 20) as usize) % kinds.len()];
                    if kind == BuildingKind::AbandonedKiln
                        && !self.resources.iter().any(|r| {
                            r.kind == ResourceKind::Clay
                                && distance2(x, z, r.center[0], r.center[2]) < 450.0_f32.powi(2)
                        })
                    {
                        kind = BuildingKind::TrailCamp;
                    }
                    // Smaller natural sites can use slopes that cannot support
                    // a camp or ruin, keeping rough-country discoveries dense.
                    let mut placed = None;
                    for kind in [kind, BuildingKind::SplitBoulder, BuildingKind::RidgeCairn] {
                        let Some(building) = place_building(geography, kind, x, z, (h >> 30) as u8)
                        else {
                            continue;
                        };
                        let [w, _, d] = building.dimensions();
                        let floor = (building.origin.y + 1) as f32 * CELL_SIZE;
                        // The surrounding terrain must reach the foundation.
                        // Validate the footprint AND its feathered apron.
                        if [-4.0, w as f32 * CELL_SIZE * 0.5, w as f32 * CELL_SIZE + 4.0]
                            .iter()
                            .any(|dx| {
                                [-4.0, d as f32 * CELL_SIZE * 0.5, d as f32 * CELL_SIZE + 4.0]
                                    .iter()
                                    .any(|dz| {
                                        let p = geography.sample(
                                            building.origin.x as f32 * CELL_SIZE + dx,
                                            building.origin.z as f32 * CELL_SIZE + dz,
                                        );
                                        p.water.is_some()
                                            || (p.height + CELL_SIZE - floor).abs() > 1.5
                                    })
                            })
                        {
                            continue;
                        }
                        placed = Some(building);
                        break;
                    }
                    if let Some(building) = placed {
                        let entry = building.entrance();
                        occupied
                            .entry(bucket(entry[0], entry[2]))
                            .or_default()
                            .push([entry[0], entry[2]]);
                        self.roadside_landmarks.push(RoadsideLandmark {
                            building,
                            approach: None,
                        });
                        break;
                    }
                }
            }
        }
        self.buckets.clear();
        self.build_index();
    }

    pub(super) fn away_from_paths(&self, x: f32, z: f32, clearance: f32) -> bool {
        // Reuse the plan's spatial index instead of scanning every metre of
        // every route for each of the island-wide candidates.
        let key = ((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32);
        let reach = (clearance / BUCKET).ceil() as i32;
        for dz in -reach..=reach {
            for dx in -reach..=reach {
                if let Some(features) = self.buckets.get(&(key.0 + dx, key.1 + dz)) {
                    for feature in features {
                        let (path, i) = match *feature {
                            Feature::Trail(t, i) => (&self.trails[t], i),
                            Feature::Lane(v, l, i) => (&self.villages[v].lanes[l], i),
                            Feature::RoadsidePath(s, i) => {
                                (self.roadside_landmarks[s].approach.as_ref().unwrap(), i)
                            }
                            _ => continue,
                        };
                        if segment_distance(x, z, path.points[i], path.points[i + 1]).0
                            < clearance + path.width * 0.5
                        {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        physics::{Body, MoveInput, move_character},
        world::WorldGeneration,
    };

    fn walk(world: &World, body: &mut Body, target: [f32; 3]) {
        for _ in 0..180 {
            let dx = target[0] - body.position[0];
            let dz = target[2] - body.position[2];
            let distance = libm::hypotf(dx, dz);
            if distance < 0.15 && (target[1] - body.position[1]).abs() < 0.55 {
                return;
            }
            let amount = (distance / 0.2).min(0.6);
            move_character(
                world,
                body,
                MoveInput {
                    direction: if distance > 0.02 {
                        [dx / distance * amount, dz / distance * amount]
                    } else {
                        [0.0; 2]
                    },
                    ..Default::default()
                },
                0.05,
            );
        }
        panic!(
            "pathless discovery blocked: {:?} -> {target:?}",
            body.position
        );
    }

    #[test]
    fn pathless_discoveries_are_dense_reproducible_and_physically_accessible_across_islands() {
        for seed in [42, 7, 99] {
            let world = World::generate(seed, WorldGeneration::GeographyV6);
            let plan = world.settlements().unwrap();
            let sites: Vec<_> = plan
                .roadside_landmarks
                .iter()
                .filter(|s| s.approach.is_none())
                .collect();
            assert!(
                sites.len() >= 1_500,
                "seed {seed}: only {} pathless discoveries",
                sites.len()
            );
            let kinds: HashSet<_> = sites.iter().map(|s| s.building.kind as u8).collect();
            assert!(kinds.len() >= 9);
            let quadrants: HashSet<_> = sites
                .iter()
                .map(|s| {
                    let p = s.building.entrance();
                    (p[0].is_sign_positive(), p[2].is_sign_positive())
                })
                .collect();
            assert_eq!(quadrants.len(), 4);
            assert!(
                sites
                    .iter()
                    .filter(|s| {
                        let p = s.building.entrance();
                        plan.trails.iter().all(|t| {
                            t.points.windows(2).all(|pair| {
                                segment_distance(p[0], p[2], pair[0], pair[1]).0 >= 1_000.0
                            })
                        })
                    })
                    .count()
                    > 500,
                "seed {seed}: discoveries still follow roads"
            );
            let transit = AirshipNetwork::try_new(&world).unwrap();
            let mut rebuilt = plan.clone();
            rebuilt.roadside_landmarks.retain(|s| s.approach.is_some());
            rebuilt.buckets.clear();
            rebuilt.build_index();
            rebuilt.add_wilderness_sites(&world, &transit);
            assert_eq!(
                rebuilt, *plan,
                "seed {seed}: scenery changed on regeneration"
            );

            for (i, site) in sites.iter().enumerate() {
                let b = &site.building;
                let entry = b.entrance();
                // Independently check actual route segments, including spurs.
                // A broad-phase box avoids doing expensive distance math for
                // every metre of distant routes.
                for trail in plan
                    .trails
                    .iter()
                    .chain(plan.villages.iter().flat_map(|v| &v.lanes))
                    .chain(
                        plan.roadside_landmarks
                            .iter()
                            .filter_map(|s| s.approach.as_ref()),
                    )
                {
                    for p in trail.points.windows(2) {
                        if entry[0] >= p[0][0].min(p[1][0]) - 110.0
                            && entry[0] <= p[0][0].max(p[1][0]) + 110.0
                            && entry[2] >= p[0][2].min(p[1][2]) - 110.0
                            && entry[2] <= p[0][2].max(p[1][2]) + 110.0
                        {
                            assert!(
                                segment_distance(entry[0], entry[2], p[0], p[1]).0 >= 100.0,
                                "seed {seed}: {:?} overlaps a route",
                                b.kind
                            );
                        }
                    }
                }
                assert!(
                    transit
                        .ports()
                        .iter()
                        .flat_map(|p| transit.dock_positions(p.village_id))
                        .all(|p| { distance2(entry[0], entry[2], p[0], p[2]) > 60.0_f32.powi(2) })
                );
                assert!(
                    sites.iter().skip(i + 1).all(|other| {
                        let p = other.building.entrance();
                        distance2(entry[0], entry[2], p[0], p[2]) > 180.0_f32.powi(2)
                    }),
                    "seed {seed}: overlapping wilderness sites"
                );
                let outward = match b.rotation {
                    0 => [0.0, -1.0],
                    1 => [1.0, 0.0],
                    2 => [0.0, 1.0],
                    _ => [-1.0, 0.0],
                };
                let mut outside = [
                    entry[0] + outward[0] * 6.0,
                    entry[1],
                    entry[2] + outward[1] * 6.0,
                ];
                outside[1] = (world.height_at(
                    (outside[0] / CELL_SIZE).floor() as i32,
                    (outside[2] / CELL_SIZE).floor() as i32,
                ) + 1) as f32
                    * CELL_SIZE;
                let mut body = Body::new(outside);
                walk(&world, &mut body, entry);
                let inside = [
                    entry[0] - outward[0] * 2.0,
                    entry[1],
                    entry[2] - outward[1] * 2.0,
                ];
                walk(&world, &mut body, inside);
                walk(&world, &mut body, entry);
                walk(&world, &mut body, outside);
            }
        }
    }
}
