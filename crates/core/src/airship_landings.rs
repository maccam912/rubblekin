//! Additive, low landings beside existing village roads. Original columns are
//! used so player edits never move a berth or change a saved flight timetable.
use crate::{
    airships::{AirshipPort, AirshipRamp},
    physics::PLAYER_RADIUS,
    settlement::Village,
    world::{CELL_SIZE, World},
};

pub(crate) struct Landing {
    pub position: [f32; 3],
    /// Ground marker to the ship center, through the existing road network.
    pub approach: Vec<[f32; 3]>,
    pub ramps: Vec<AirshipRamp>,
    branch: Vec<[f32; 3]>,
}

pub(crate) fn landings(
    world: &World,
    village: &Village,
    port: &AirshipPort,
    count: usize,
) -> Vec<Landing> {
    if count == 0 {
        return Vec::new();
    }
    let Some(plan) = world.settlements() else {
        return Vec::new();
    };
    let mut selected: Vec<Landing> = Vec::new();
    for trail in plan
        .trails
        .iter()
        .filter(|trail| trail.from == village.id || trail.to == village.id)
    {
        let path: Vec<_> = if trail.from == village.id {
            trail.points.clone()
        } else {
            trail.points.iter().rev().copied().collect()
        };
        let mut travelled = 0.0;
        let mut next_probe = 45.0;
        for index in 1..path.len() {
            travelled += distance(path[index - 1], path[index]);
            if travelled < next_probe {
                continue;
            }
            if travelled > 1_600.0 {
                break;
            }
            next_probe = travelled + 12.0;
            let anchor = path[index];
            let delta = [
                anchor[0] - path[index - 1][0],
                anchor[2] - path[index - 1][2],
            ];
            let length = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt().max(0.01);
            // Every berth is a side branch. A road-centered fallback covers
            // the through-route with the pier and a turning ship's full deck.
            for side in [
                24.0, -24.0, 30.0, -30.0, 36.0, -36.0, 18.0, -18.0, 48.0, -48.0, 60.0, -60.0,
            ] {
                let center = [
                    anchor[0] - delta[1] / length * side,
                    anchor[1],
                    anchor[2] + delta[0] / length * side,
                ];
                if selected
                    .iter()
                    .any(|landing| distance(landing.position, center) < 25.0)
                {
                    continue;
                }
                let Some(height) = clear_footprint(world, village, center) else {
                    continue;
                };
                let Some(side_path) = ground_link(world, anchor, center) else {
                    continue;
                };
                let mut approach: Vec<_> = port.approach.iter().rev().copied().collect();
                approach.extend(path[..=index].iter().skip(1));
                let branch_start = approach.len() - 1;
                let position = [center[0], height + 0.35, center[2]];
                let ramp_start = approach.len() + 1;
                approach.extend(side_path.iter().skip(1));
                let ramps = fit_ramp(&mut approach[ramp_start..], position, f32::INFINITY);
                let branch = approach[branch_start..].to_vec();
                if selected.iter().any(|landing| {
                    ramps_obstruct_branch(&ramps, &landing.branch)
                        || ramps_obstruct_branch(&landing.ramps, &branch)
                }) {
                    continue;
                }
                selected.push(Landing {
                    position,
                    approach,
                    ramps,
                    branch,
                });
                if selected.len() == count {
                    return selected;
                }
            }
        }
    }
    selected
}

fn ramps_obstruct_branch(ramps: &[AirshipRamp], branch: &[[f32; 3]]) -> bool {
    branch.iter().any(|point| {
        ramps.iter().any(|ramp| {
            let dx = ramp.to[0] - ramp.from[0];
            let dz = ramp.to[2] - ramp.from[2];
            let squared = dx * dx + dz * dz;
            let along = ((point[0] - ramp.from[0]) * dx + (point[2] - ramp.from[2]) * dz) / squared;
            let across = ((point[0] - ramp.from[0]) * dz - (point[2] - ramp.from[2]) * dx).abs()
                / squared.sqrt();
            (0.0..=1.0).contains(&along)
                && across <= ramp.width * 0.5 + PLAYER_RADIUS
                && ramp.from[1] + (ramp.to[1] - ramp.from[1]) * along > point[1] + 0.45
        })
    })
}

fn clear_footprint(world: &World, village: &Village, center: [f32; 3]) -> Option<f32> {
    // A turning 8x17m deck fits in this circle. Keep crops out of the landing.
    let radius = 9.75;
    // Check the whole turning footprint against every nearby trail and lane,
    // including bends and roads other than the branch's parent segment.
    let plan = world.settlements()?;
    if plan
        .trails
        .iter()
        .chain(plan.villages.iter().flat_map(|v| &v.lanes))
        .any(|road| {
            road.points.windows(2).any(|p| {
                segment_distance(center, p[0], p[1]) < radius + road.width * 0.5 + PLAYER_RADIUS
            })
        })
    {
        return None;
    }
    if village.fields.iter().any(|field| {
        let min_x = field.origin.x as f32 * CELL_SIZE;
        let min_z = field.origin.z as f32 * CELL_SIZE;
        let x = center[0].clamp(min_x, min_x + field.width as f32 * CELL_SIZE);
        let z = center[2].clamp(min_z, min_z + field.depth as f32 * CELL_SIZE);
        (center[0] - x).powi(2) + (center[2] - z).powi(2) < radius * radius
    }) {
        return None;
    }
    let mut low = f32::INFINITY;
    let mut high = f32::NEG_INFINITY;
    for z in -20_i32..=20 {
        for x in -20_i32..=20 {
            let dx = x as f32 * CELL_SIZE;
            let dz = z as f32 * CELL_SIZE;
            if dx * dx + dz * dz > radius * radius {
                continue;
            }
            let p = [center[0] + dx, 0.0, center[2] + dz];
            let ground = terrain_height(world, p);
            if world.original_structure_height(p[0], p[2]) > ground + 0.01
                || world.geography().is_some_and(|g| {
                    g.sample(p[0], p[2])
                        .water
                        .is_some_and(|water| water > ground - 0.1)
                })
            {
                return None;
            }
            low = low.min(ground);
            high = high.max(ground);
            if high - low > 1.5 {
                return None;
            }
        }
    }
    Some(high)
}

fn terrain_height(world: &World, p: [f32; 3]) -> f32 {
    (world.height_at(
        (p[0] / CELL_SIZE).floor() as i32,
        (p[2] / CELL_SIZE).floor() as i32,
    ) + 1) as f32
        * CELL_SIZE
}

fn ground_link(world: &World, a: [f32; 3], b: [f32; 3]) -> Option<Vec<[f32; 3]>> {
    let steps = (distance(a, b) / 0.5).ceil().max(1.0) as usize;
    let mut path = Vec::new();
    for index in 0..=steps {
        let t = index as f32 / steps as f32;
        let mut p = [a[0] + (b[0] - a[0]) * t, 0.0, a[2] + (b[2] - a[2]) * t];
        let mut floor = f32::NEG_INFINITY;
        for dx in [-PLAYER_RADIUS, 0.0, PLAYER_RADIUS] {
            for dz in [-PLAYER_RADIUS, 0.0, PLAYER_RADIUS] {
                let probe = [p[0] + dx, 0.0, p[2] + dz];
                let height = terrain_height(world, probe);
                if world.original_structure_height(probe[0], probe[2]) > height + 0.01 {
                    return None;
                }
                floor = floor.max(height);
            }
        }
        p[1] = floor;
        if path
            .last()
            .is_some_and(|previous: &[f32; 3]| (previous[1] - p[1]).abs() > 0.51)
        {
            return None;
        }
        path.push(p);
    }
    Some(path)
}

fn fit_ramp(path: &mut [[f32; 3]], position: [f32; 3], limit: f32) -> Vec<AirshipRamp> {
    // Reach deck height before crossing its turning footprint. The final ten
    // meters form a flat pier; the preceding ten meters climb from the road.
    let mut remaining = 0.0;
    let mut start = path.len() - 1;
    for index in (1..path.len()).rev() {
        remaining += distance(path[index], path[index - 1]);
        start = index - 1;
        if remaining >= limit {
            break;
        }
    }
    let base = path[start][1] + 0.02;
    let mut walked = 0.0;
    let climb = (remaining - 10.0).max(1.0);
    for index in start..path.len() {
        if index > start {
            walked += distance(path[index - 1], path[index]);
        }
        path[index][1] =
            (base + (position[1] - base) * (walked / climb).min(1.0)).max(path[index][1] + 0.02);
    }
    path[start..]
        .windows(2)
        .filter(|p| distance(p[0], p[1]) > 0.01)
        .map(|p| ramp_segment(p[0], p[1]))
        .collect()
}

fn ramp_segment(mut from: [f32; 3], mut to: [f32; 3]) -> AirshipRamp {
    // Adjacent finite strips need physical overlap at bends; otherwise a foot
    // can fall through the outside corner and get trapped below the next slab.
    let length = distance(from, to);
    let dx = (to[0] - from[0]) / length * 0.1;
    let dz = (to[2] - from[2]) / length * 0.1;
    from[0] -= dx;
    from[2] -= dz;
    to[0] += dx;
    to[2] += dz;
    AirshipRamp {
        from,
        to,
        width: 2.0,
    }
}

fn segment_distance(point: [f32; 3], a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = b[0] - a[0];
    let dz = b[2] - a[2];
    let squared = dx * dx + dz * dz;
    let t = if squared > 0.0 {
        (((point[0] - a[0]) * dx + (point[2] - a[2]) * dz) / squared).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (point[0] - a[0] - t * dx).hypot(point[2] - a[2] - t * dz)
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::WorldGeneration;

    #[test]
    fn each_village_has_clear_low_separated_berths_and_connected_ramps_across_five_seeds() {
        for seed in [42, 7, 99, 43, 123] {
            let world = World::generate(seed, WorldGeneration::GeographyV3);
            let plan = world.settlements().unwrap();
            for village in &plan.villages {
                let count = plan
                    .trails
                    .iter()
                    .filter(|t| t.from == village.id || t.to == village.id)
                    .count();
                let port = AirshipPort {
                    village_id: village.id,
                    position: village.store,
                    approach: village.lanes[0].points.clone(),
                };
                let berths = landings(&world, village, &port, count);
                assert_eq!(
                    berths.len(),
                    count,
                    "seed={seed} village={} has too few clear landings",
                    village.id
                );
                for (index, berth) in berths.iter().enumerate() {
                    let ground = clear_footprint(&world, village, berth.position).unwrap();
                    assert!((berth.position[1] - ground - 0.35).abs() < 0.001);
                    assert!(!berth.ramps.is_empty());
                    assert!(!berth.branch.is_empty());
                    for road in plan
                        .trails
                        .iter()
                        .chain(plan.villages.iter().flat_map(|v| &v.lanes))
                    {
                        assert!(
                            road.points.windows(2).all(|p| segment_distance(
                                berth.position,
                                p[0],
                                p[1]
                            ) >= 9.75
                                + road.width * 0.5
                                + PLAYER_RADIUS),
                            "seed {seed}: berth covers a trail"
                        );
                    }
                    for dx in [-8.0, 0.0, 8.0] {
                        for dz in [-4.0, 0.0, 4.0] {
                            let p = [berth.position[0] + dx, 0.0, berth.position[2] + dz];
                            assert!(
                                world.original_surface_height(p[0], p[2])
                                    <= terrain_height(&world, p) + 0.01,
                                "landing clearing left a tree or roof inside the deck"
                            );
                        }
                    }
                    assert_eq!(berth.approach.last().unwrap(), &berth.position);
                    assert!(
                        berth
                            .ramps
                            .iter()
                            .all(|r| r.width == 2.0 && distance(r.from, r.to) > 0.0)
                    );
                    for other in &berths[index + 1..] {
                        assert!(distance(berth.position, other.position) >= 25.0);
                    }
                }
            }
        }
    }
}
