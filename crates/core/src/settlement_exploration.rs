//! Frequent, seeded discoveries beside the existing village walking routes.
use super::*;
use crate::airships::AirshipNetwork;

/// 105 seconds at ordinary 3.8 m/s walking speed, before detours and stops.
const DISCOVERY_SPACING: f32 = 400.0;
const SITE_SEPARATION: f32 = 220.0;

impl SettlementPlan {
    pub(crate) fn clear_landing_trees(&mut self, transit: &AirshipNetwork) {
        for port in transit.ports() {
            for [x, _, z] in transit.dock_positions(port.village_id) {
                self.landing_clearances
                    .push([x - 10.0, z - 10.0, x + 10.0, z + 10.0]);
            }
        }
        for ramp in transit.ramps() {
            self.landing_clearances.push([
                ramp.from[0].min(ramp.to[0]) - 1.5,
                ramp.from[2].min(ramp.to[2]) - 1.5,
                ramp.from[0].max(ramp.to[0]) + 1.5,
                ramp.from[2].max(ramp.to[2]) + 1.5,
            ]);
        }
        self.buckets.clear();
        self.build_index();
    }

    pub(crate) fn add_exploration_sites(&mut self, world: &World, transit: &AirshipNetwork) {
        let geography = world.geography().expect("exploration sites need geography");
        for trail_index in 0..self.trails.len() {
            let trail = &self.trails[trail_index];
            let mut travelled = 0.0;
            let mut next = DISCOVERY_SPACING * 0.6;
            let mut ordinal = 0;
            let anchors: Vec<_> = trail
                .points
                .windows(2)
                .filter_map(|p| {
                    travelled += distance2(p[0][0], p[0][2], p[1][0], p[1][2]).sqrt();
                    if travelled < next {
                        return None;
                    }
                    next += DISCOVERY_SPACING;
                    ordinal += 1;
                    Some((p[0], p[1], ordinal))
                })
                .collect();
            for (a, anchor, ordinal) in anchors {
                // Retry nearby points when a scheduled stop falls on steep or
                // wet ground. Keep the search local and bounded on every seed.
                let index = self.trails[trail_index]
                    .points
                    .iter()
                    .position(|p| *p == anchor)
                    .unwrap();
                let mut placed = false;
                for delta in [0, 20, -20, 40, -40, 70, -70, 100, -100] {
                    let candidate_index = index
                        .saturating_add_signed(delta)
                        .clamp(1, self.trails[trail_index].points.len() - 1);
                    let anchor = self.trails[trail_index].points[candidate_index];
                    let previous = if delta == 0 {
                        a
                    } else {
                        self.trails[trail_index].points[candidate_index - 1]
                    };
                    if self.villages.iter().any(|v| {
                        distance2(anchor[0], anchor[2], v.center[0], v.center[2])
                            < 180.0_f32.powi(2)
                    }) {
                        continue;
                    }
                    let h = hash(trail_index as i32, ordinal, world.seed.wrapping_add(6107));
                    let kinds = [
                        BuildingKind::StoneArch,
                        BuildingKind::StandingStones,
                        BuildingKind::FallenGiant,
                        BuildingKind::TrailCamp,
                        BuildingKind::RuinedTower,
                        BuildingKind::AbandonedKiln,
                    ];
                    let mut kind = kinds[((ordinal as u32 + h % 3 + trail_index as u32)
                        % kinds.len() as u32) as usize];
                    let biome = geography.sample(anchor[0], anchor[2]).biome;
                    if kind == BuildingKind::FallenGiant
                        && !matches!(
                            biome,
                            Biome::Forest
                                | Biome::PineForest
                                | Biome::Rainforest
                                | Biome::Grassland
                        )
                    {
                        kind = BuildingKind::StandingStones;
                    }
                    if kind == BuildingKind::AbandonedKiln
                        && !self.resources.iter().any(|r| {
                            r.kind == ResourceKind::Clay
                                && distance2(anchor[0], anchor[2], r.center[0], r.center[2])
                                    < 450.0_f32.powi(2)
                        })
                    {
                        kind = BuildingKind::TrailCamp;
                    }
                    let dx = anchor[0] - previous[0];
                    let dz = anchor[2] - previous[2];
                    let run = dx.hypot(dz).max(0.01);
                    let sign = if h.is_multiple_of(2) { 1.0 } else { -1.0 };
                    for side in [22.0, -22.0, 30.0, -30.0, 38.0, -38.0].map(|s| s * sign) {
                        let center = [anchor[0] - dz / run * side, anchor[2] + dx / run * side];
                        if let Some(site) =
                            self.exploration_site(world, transit, kind, anchor, center)
                        {
                            self.roadside_landmarks.push(site);
                            placed = true;
                            break;
                        }
                    }
                    if placed {
                        break;
                    }
                }
            }
        }
        self.buckets.clear();
        self.build_index();
    }

    fn exploration_site(
        &self,
        world: &World,
        transit: &AirshipNetwork,
        kind: BuildingKind,
        anchor: [f32; 3],
        center: [f32; 2],
    ) -> Option<RoadsideLandmark> {
        if self.roadside_landmarks.iter().any(|site| {
            let p = site.building.entrance();
            distance2(center[0], center[1], p[0], p[2]) < SITE_SEPARATION.powi(2)
        }) || self
            .villages
            .iter()
            .any(|v| distance2(center[0], center[1], v.center[0], v.center[2]) < 180.0_f32.powi(2))
        {
            return None;
        }
        // Protect actual selected berths and gangways, rather than excluding
        // the entire 1.3 km radius around every town from interesting content.
        if transit
            .ports()
            .iter()
            .flat_map(|p| transit.dock_positions(p.village_id))
            .any(|p| distance2(center[0], center[1], p[0], p[2]) < 55.0_f32.powi(2))
            || transit
                .ramps()
                .iter()
                .any(|r| segment_distance(center[0], center[1], r.from, r.to).0 < 50.0)
        {
            return None;
        }
        if self.trails.iter().any(|trail| {
            trail
                .points
                .windows(2)
                .any(|p| segment_distance(center[0], center[1], p[0], p[1]).0 < 16.0)
        }) {
            return None;
        }
        let to_road = [anchor[0] - center[0], anchor[2] - center[1]];
        let rotation = if to_road[0].abs() > to_road[1].abs() {
            if to_road[0] > 0.0 { 1 } else { 3 }
        } else if to_road[1] > 0.0 {
            2
        } else {
            0
        };
        let geography = world.geography()?;
        let mut building = place_building(geography, kind, center[0], center[1], rotation)?;
        let floor = (anchor[1] / CELL_SIZE).round() * CELL_SIZE;
        let natural_floor = (building.origin.y + 1) as f32 * CELL_SIZE;
        if (floor - natural_floor).abs() > 1.5 {
            return None;
        }
        building.origin.y = (floor / CELL_SIZE).round() as i32 - 1;
        let entry = building.entrance();
        let steps = distance2(anchor[0], anchor[2], entry[0], entry[2])
            .sqrt()
            .ceil()
            .max(1.0) as usize;
        let mut points: Vec<_> = (0..=steps)
            .map(|i| {
                let t = i as f32 / steps as f32;
                [
                    anchor[0] + (entry[0] - anchor[0]) * t,
                    floor,
                    anchor[2] + (entry[2] - anchor[2]) * t,
                ]
            })
            .collect();
        // Carry the graded path across the 1.5 m outside entrance apron to
        // the actual asset floor; a natural ridge must not seal the doorway.
        let inward = match rotation {
            0 => [0.0, 1.0],
            1 => [-1.0, 0.0],
            2 => [0.0, -1.0],
            _ => [1.0, 0.0],
        };
        points.extend((1..=4).map(|step| {
            [
                entry[0] + inward[0] * step as f32 * CELL_SIZE,
                floor,
                entry[2] + inward[1] * step as f32 * CELL_SIZE,
            ]
        }));
        if points.iter().any(|p| {
            let sample = geography.sample(p[0], p[2]);
            sample.water.is_some() || (sample.height + CELL_SIZE - floor).abs() > 2.0
        }) {
            return None;
        }
        Some(RoadsideLandmark {
            building,
            approach: Trail {
                from: self
                    .trails
                    .iter()
                    .find(|t| t.points.contains(&anchor))?
                    .from,
                to: self
                    .trails
                    .iter()
                    .find(|t| t.points.contains(&anchor))?
                    .from,
                points,
                width: 2.0,
                terrain_heights: Vec::new(),
            },
        })
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
        for _ in 0..600 {
            let dx = target[0] - body.position[0];
            let dz = target[2] - body.position[2];
            let distance = dx.hypot(dz);
            if distance < 0.12 && (target[1] - body.position[1]).abs() < 0.55 {
                return;
            }
            let amount = (distance / 0.13).min(0.6);
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
                1.0 / 30.0,
            );
        }
        panic!("discovery route blocked: {:?} -> {target:?}", body.position);
    }

    #[test]
    fn discoveries_keep_transit_clear_and_every_generated_approach_walkable() {
        for seed in [42, 7, 99] {
            let world = World::generate(seed, WorldGeneration::GeographyV6);
            let old = World::generate(seed, WorldGeneration::GeographyV5);
            let plan = world.settlements().unwrap();
            assert_eq!(plan.trails, old.settlements().unwrap().trails);
            let transit = AirshipNetwork::try_new(&world).unwrap();
            let previous_transit = AirshipNetwork::try_new(&old).unwrap();
            assert_eq!(
                transit.ports(),
                previous_transit.ports(),
                "seed {seed}: port moved"
            );
            let sites: Vec<_> = plan
                .roadside_landmarks
                .iter()
                .filter(|s| s.building.kind.is_exploration_site())
                .collect();
            assert!(
                sites.len() >= 80,
                "seed {seed}: only {} discoveries",
                sites.len()
            );
            let mut kinds = HashSet::new();
            for site in sites {
                let b = &site.building;
                kinds.insert(b.kind as u8);
                let root = site.approach.points[0];
                let (through, index) = plan
                    .trails
                    .iter()
                    .find_map(|t| t.points.iter().position(|p| *p == root).map(|i| (t, i)))
                    .unwrap();
                let junction = &through.points
                    [index.saturating_sub(8)..=(index + 8).min(through.points.len() - 1)];
                for p in junction {
                    for dx in [-0.25, 0.25] {
                        for dz in [-0.25, 0.25] {
                            let x = ((p[0] + dx) / CELL_SIZE).floor() as i32;
                            let z = ((p[2] + dz) / CELL_SIZE).floor() as i32;
                            assert_eq!(
                                world.height_at(x, z),
                                old.height_at(x, z),
                                "seed {seed}: {:?} changed road",
                                b.kind
                            );
                        }
                    }
                }
                let mut body = Body::new(root);
                for &p in site.approach.points.iter().skip(1) {
                    walk(&world, &mut body, p);
                }
                let [w, _, d] = village_assets::dimensions(b.kind).map(|n| n as f32);
                let local = |x: f32, z: f32, rise: f32| {
                    let [x, z] = match b.rotation % 4 {
                        0 => [x, z],
                        1 => [d - z, x],
                        2 => [w - x, d - z],
                        _ => [z, w - x],
                    };
                    [
                        (b.origin.x as f32 + x) * CELL_SIZE,
                        (b.origin.y + 1) as f32 * CELL_SIZE + rise,
                        (b.origin.z as f32 + z) * CELL_SIZE,
                    ]
                };
                walk(&world, &mut body, local(w * 0.5, 3.0, 0.0));
                match b.kind {
                    BuildingKind::StoneArch => {
                        walk(&world, &mut body, local(w * 0.5, d - 3.0, 0.0));
                        walk(&world, &mut body, local(w * 0.5, 3.0, 0.0));
                    }
                    BuildingKind::StandingStones
                    | BuildingKind::TrailCamp
                    | BuildingKind::FallenGiant
                    | BuildingKind::AbandonedKiln => {
                        walk(&world, &mut body, local(w * 0.5, 9.0, 0.0));
                        walk(&world, &mut body, local(w * 0.5, 3.0, 0.0));
                    }
                    BuildingKind::RuinedTower => {
                        for z in [
                            9.0, 12.0, 13.5, 15.5, 17.5, 19.5, 21.5, 23.5, 21.5, 19.5, 17.5, 15.5,
                            13.5, 12.0, 9.0, 3.0,
                        ] {
                            let rise = if z < 13.0 {
                                0.0
                            } else {
                                (1.0 + ((z - 13.0) / 2.0_f32).floor()).min(5.0) * CELL_SIZE
                            };
                            walk(&world, &mut body, local(w * 0.5 + 0.5, z, rise));
                        }
                    }
                    _ => unreachable!(),
                }
                for &p in site.approach.points.iter().rev() {
                    walk(&world, &mut body, p);
                }
            }
            assert_eq!(kinds.len(), 6, "seed {seed}: discovery variety missing");
        }
    }
}
