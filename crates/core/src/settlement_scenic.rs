//! Bounded roadside discoveries. These paths never enter the trade/airship graph.
use super::*;

impl SettlementPlan {
    pub(crate) fn add_roadside_landmarks(&mut self, world: &World) {
        self.add_trail_sites(world, false);
    }

    pub(crate) fn add_roadside_workyards(&mut self, world: &World) {
        self.add_trail_sites(world, true);
    }

    fn add_trail_sites(&mut self, world: &World, new_places: bool) {
        let g = world
            .geography()
            .expect("roadside landmarks require geography");
        for (trail_index, trail) in self.trails.iter().enumerate() {
            let length: f32 = trail
                .points
                .windows(2)
                .map(|p| distance2(p[0][0], p[0][2], p[1][0], p[1][2]).sqrt())
                .sum();
            let mut accepted = 0;
            let fractions = if new_places {
                [0.2, 0.8, 0.45, 0.55, 0.9]
            } else {
                [0.35, 0.65, 0.5, 0.25, 0.75]
            };
            for fraction in fractions {
                if accepted == if new_places { 1 } else { 2 } {
                    break;
                }
                let mut travelled = 0.0;
                let Some(segment) = trail.points.windows(2).find(|p| {
                    travelled += distance2(p[0][0], p[0][2], p[1][0], p[1][2]).sqrt();
                    travelled >= length * fraction
                }) else {
                    continue;
                };
                let anchor = segment[1];
                // Landing candidates extend up to 1.2 km along roads. Keep
                // another 100 m and never alter a town's street grading plane.
                if self.villages.iter().any(|v| {
                    distance2(anchor[0], anchor[2], v.center[0], v.center[2]) < 1_300.0_f32.powi(2)
                }) {
                    continue;
                }
                let dx = segment[1][0] - segment[0][0];
                let dz = segment[1][2] - segment[0][2];
                let run = dx.hypot(dz).max(0.01);
                let kind = if new_places {
                    if (trail_index + world.seed as usize).is_multiple_of(2) {
                        BuildingKind::TrailPavilion
                    } else {
                        BuildingKind::QuarryYard
                    }
                } else if (trail_index + accepted + world.seed as usize).is_multiple_of(2) {
                    BuildingKind::TrailRuin
                } else {
                    BuildingKind::Waystone
                };
                let sides = if new_places {
                    [24.0, -24.0, 32.0, -32.0]
                } else {
                    [20.0, -20.0, 28.0, -28.0]
                };
                for side in sides {
                    let center = [anchor[0] - dz / run * side, anchor[2] + dx / run * side];
                    if self.villages.iter().any(|v| {
                        distance2(center[0], center[1], v.center[0], v.center[2])
                            < 1_300.0_f32.powi(2)
                    }) {
                        continue;
                    }
                    if self.roadside_landmarks.iter().any(|site| {
                        let p = site.building.entrance();
                        distance2(center[0], center[1], p[0], p[2]) < 300.0_f32.powi(2)
                    }) {
                        continue;
                    }
                    if kind == BuildingKind::QuarryYard
                        && !self.resources.iter().any(|deposit| {
                            deposit.kind == ResourceKind::Stone
                                && distance2(
                                    center[0],
                                    center[1],
                                    deposit.center[0],
                                    deposit.center[2],
                                ) < 450.0_f32.powi(2)
                        })
                    {
                        continue;
                    }
                    if self.trails.iter().any(|t| {
                        t.points.windows(2).any(|p| {
                            segment_distance(center[0], center[1], p[0], p[1]).0
                                < if new_places { 16.0 } else { 12.0 }
                        })
                    }) {
                        continue;
                    }
                    let to_road = [anchor[0] - center[0], anchor[2] - center[1]];
                    let rotation = if to_road[0].abs() > to_road[1].abs() {
                        if to_road[0] > 0.0 { 1 } else { 3 }
                    } else if to_road[1] > 0.0 {
                        2
                    } else {
                        0
                    };
                    let Some(mut building) =
                        place_building(g, kind, center[0], center[1], rotation)
                    else {
                        continue;
                    };
                    let floor = (anchor[1] / CELL_SIZE).round() * CELL_SIZE;
                    let natural_floor = (building.origin.y + 1) as f32 * CELL_SIZE;
                    if (floor - natural_floor).abs() > 1.5 {
                        continue;
                    }
                    building.origin.y = (floor / CELL_SIZE).round() as i32 - 1;
                    let entry = building.entrance();
                    let steps = (distance2(anchor[0], anchor[2], entry[0], entry[2]).sqrt() / 1.0)
                        .ceil()
                        .max(1.0) as usize;
                    let points: Vec<_> = (0..=steps)
                        .map(|i| {
                            let t = i as f32 / steps as f32;
                            [
                                anchor[0] + (entry[0] - anchor[0]) * t,
                                floor,
                                anchor[2] + (entry[2] - anchor[2]) * t,
                            ]
                        })
                        .collect();
                    if points.iter().any(|p| {
                        let sample = g.sample(p[0], p[2]);
                        sample.water.is_some() || (sample.height + CELL_SIZE - floor).abs() > 2.0
                    }) {
                        continue;
                    }
                    self.roadside_landmarks.push(RoadsideLandmark {
                        building,
                        approach: Some(Trail {
                            from: trail.from,
                            to: trail.from,
                            points,
                            width: 2.0,
                            terrain_heights: Vec::new(),
                        }),
                    });
                    accepted += 1;
                    break;
                }
            }
        }
        self.buckets.clear();
        self.build_index();
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
        for _ in 0..900 {
            let dx = target[0] - body.position[0];
            let dz = target[2] - body.position[2];
            let distance = dx.hypot(dz);
            if distance < 0.1 && (target[1] - body.position[1]).abs() < 0.6 {
                return;
            }
            let direction = if distance > 0.03 {
                let speed = (distance / 0.13).min(0.6);
                [dx / distance * speed, dz / distance * speed]
            } else {
                [0.0; 2]
            };
            move_character(
                world,
                body,
                MoveInput {
                    direction,
                    ..Default::default()
                },
                1.0 / 30.0,
            );
        }
        panic!("roadside walk blocked: {:?} -> {target:?}", body.position);
    }

    #[test]
    fn roadside_sites_are_bounded_clear_and_walkable_without_changing_transit() {
        verify_roadside(
            WorldGeneration::GeographyV5,
            WorldGeneration::GeographyV4,
            2,
        );
    }

    #[test]
    fn v6_pavilions_and_quarries_are_walkable_without_moving_v5_places() {
        verify_roadside(
            WorldGeneration::GeographyV6,
            WorldGeneration::GeographyV5,
            3,
        );
    }

    fn verify_roadside(
        generation: WorldGeneration,
        old_generation: WorldGeneration,
        max_per_trail: usize,
    ) {
        for seed in [42, 7, 99] {
            let world = World::generate(seed, generation);
            let previous = World::generate(seed, old_generation);
            let plan = world.settlements().unwrap();
            let old = previous.settlements().unwrap();
            assert!(
                plan.villages == old.villages,
                "seed{seed}: {generation:?} moved or rescored existing villages"
            );
            assert!(
                plan.trails == old.trails,
                "seed{seed}: {generation:?} changed transit trails"
            );
            assert!(
                (4..=plan.trails.len() * max_per_trail).contains(
                    &plan
                        .roadside_landmarks
                        .iter()
                        .filter(|s| !s.building.kind.is_exploration_site())
                        .count()
                )
            );
            assert_eq!(plan.resources, old.resources);
            assert_eq!(
                &plan.roadside_landmarks[..old.roadside_landmarks.len()],
                old.roadside_landmarks
            );
            if generation == WorldGeneration::GeographyV6 {
                let new_sites = &plan.roadside_landmarks[old.roadside_landmarks.len()..];
                assert!(
                    new_sites
                        .iter()
                        .any(|s| s.building.kind == BuildingKind::TrailPavilion),
                    "seed{seed}: no pavilion"
                );
                assert!(
                    new_sites
                        .iter()
                        .any(|s| s.building.kind == BuildingKind::QuarryYard),
                    "seed{seed}: no quarry"
                );
                eprintln!(
                    "seed{seed}: {} V5 sites + {} new V6 sites",
                    old.roadside_landmarks.len(),
                    new_sites.len()
                );
                for site in &old.roadside_landmarks {
                    let b = &site.building;
                    let [w, h, d] = b.dimensions();
                    for x in b.origin.x..b.origin.x + w {
                        for z in b.origin.z..b.origin.z + d {
                            for y in b.origin.y..b.origin.y + h {
                                let pos = BlockPos::new(x, y, z);
                                assert_eq!(
                                    world.block(pos),
                                    previous.block(pos),
                                    "seed{seed}: altered V5 site at {pos:?}"
                                );
                            }
                        }
                    }
                }
            }
            crate::airships::AirshipNetwork::try_new(&world).unwrap();
            for site in plan
                .roadside_landmarks
                .iter()
                .filter(|s| !s.building.kind.is_exploration_site())
            {
                let b = &site.building;
                let entry = b.entrance();
                assert!(plan.villages.iter().all(|v| distance2(
                    v.center[0],
                    v.center[2],
                    entry[0],
                    entry[2]
                ) > 1_280.0_f32.powi(2)));
                let [w, _, d] = b.dimensions();
                let center = [
                    (b.origin.x as f32 + w as f32 * 0.5) * CELL_SIZE,
                    (b.origin.z as f32 + d as f32 * 0.5) * CELL_SIZE,
                ];
                assert!(plan.trails.iter().all(|t| {
                    t.points
                        .windows(2)
                        .all(|p| segment_distance(center[0], center[1], p[0], p[1]).0 > 11.5)
                }));
                // The short spur's road blending must not strand walkers on
                // the existing through-route on either side of the junction.
                let (through, index) = plan
                    .trails
                    .iter()
                    .find_map(|trail| {
                        trail
                            .points
                            .iter()
                            .position(|p| *p == site.approach.as_ref().unwrap().points[0])
                            .map(|index| (trail, index))
                    })
                    .expect("approach starts on the unchanged route");
                let junction = &through.points
                    [index.saturating_sub(20)..=(index + 20).min(through.points.len() - 1)];
                let mut passerby = Body::new(junction[0]);
                for p in junction {
                    for dx in [-0.25, 0.25] {
                        for dz in [-0.25, 0.25] {
                            let x = ((p[0] + dx) / CELL_SIZE).floor() as i32;
                            let z = ((p[2] + dz) / CELL_SIZE).floor() as i32;
                            assert_eq!(
                                world.height_at(x, z),
                                previous.height_at(x, z),
                                "scenic approach changed the through-road at {x},{z}"
                            );
                        }
                    }
                }
                for &p in junction.iter().skip(1).chain(junction.iter().rev().skip(1)) {
                    walk(&world, &mut passerby, p);
                }
                let mut body = Body::new(site.approach.as_ref().unwrap().points[0]);
                for &p in site.approach.as_ref().unwrap().points.iter().skip(1) {
                    walk(&world, &mut body, p);
                }
                let [w, _, d] = village_assets::dimensions(b.kind).map(|n| n as f32);
                let local = |x: f32, z: f32| {
                    let [x, z] = match b.rotation % 4 {
                        0 => [x, z],
                        1 => [d - z, x],
                        2 => [w - x, d - z],
                        _ => [z, w - x],
                    };
                    [
                        (b.origin.x as f32 + x) * CELL_SIZE,
                        entry[1],
                        (b.origin.z as f32 + z) * CELL_SIZE,
                    ]
                };
                walk(&world, &mut body, local(w * 0.5 + 0.5, 3.0));
                if matches!(
                    b.kind,
                    BuildingKind::TrailRuin | BuildingKind::TrailPavilion
                ) {
                    walk(&world, &mut body, local(w * 0.5 + 0.5, d - 2.5));
                    walk(&world, &mut body, local(w * 0.5 + 0.5, 3.0));
                }
                if b.kind == BuildingKind::QuarryYard {
                    assert!(plan.resources.iter().any(|deposit| deposit.kind
                        == ResourceKind::Stone
                        && distance2(center[0], center[1], deposit.center[0], deposit.center[2])
                            < 450.0_f32.powi(2)));
                    for z in [14.5, 16.5, 18.5, 20.5, 22.5, 20.5, 18.5, 16.5, 14.5, 3.0] {
                        let mut target = local(w * 0.5 + 0.5, z);
                        if z >= 16.0 {
                            target[1] += (1.0 + ((z - 16.0) / 2.0).floor()) * CELL_SIZE;
                        }
                        walk(&world, &mut body, target);
                    }
                }
                walk(&world, &mut body, entry);
                for &p in site.approach.as_ref().unwrap().points.iter().rev() {
                    walk(&world, &mut body, p);
                }
            }
        }
    }

    #[test]
    fn roadside_landmarks_are_reproduced_before_saved_edits() {
        for generation in [WorldGeneration::GeographyV5, WorldGeneration::GeographyV6] {
            let mut world = World::generate(42, generation);
            let position = world.settlements().unwrap().roadside_landmarks[0]
                .building
                .origin;
            world.set_block(position, Block::Air).unwrap();
            let restored = World::from_generation_edits(42, generation, &world.edits()).unwrap();
            assert_eq!(restored.settlements(), world.settlements());
            assert_eq!(restored.block(position), Block::Air);
        }
    }
}
