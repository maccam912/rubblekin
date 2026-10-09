//! A bounded first composition batch among the existing frequent discoveries.
use super::*;
use crate::{
    airships::AirshipNetwork,
    poi::{SiteArrangement, SitePlan},
};

impl SettlementPlan {
    /// The same sparse terrain patches used by collision, for sampled scenery.
    pub fn composed_ground_at(&self, x: i32, z: i32, height: i32, surface: Block) -> (i32, Block) {
        let key = (
            ((x as f32 + 0.5) * CELL_SIZE / BUCKET).floor() as i32,
            ((z as f32 + 0.5) * CELL_SIZE / BUCKET).floor() as i32,
        );
        if let Some(features) = self.buckets.get(&key) {
            for feature in features {
                if let Feature::ComposedSite(index) = *feature {
                    let (h, b, _) = self.composed_sites[index].ground_at(x, z, height, surface);
                    return (h, b);
                }
            }
        }
        (height, surface)
    }
    #[cfg(test)]
    pub(crate) fn rebuild_poi_index(&mut self) {
        self.buckets.clear();
        self.build_index();
    }
    pub(crate) fn add_composed_sites(&mut self, world: &World, transit: &AirshipNetwork) {
        let g = world.geography().expect("composed sites need geography");
        // Hash-ordered cells spread a capped review batch over the island. The
        // existing roadside/wilderness order and consumed-supply IDs stay intact.
        let mut cells: Vec<_> = (-11..=11)
            .flat_map(|z| {
                (-11..=11).map(move |x| {
                    let h = hash(x, z, world.seed.wrapping_add(92_071));
                    (h, x, z)
                })
            })
            .collect();
        cells.sort_unstable();
        for (h, gx, gz) in cells {
            if self.composed_sites.len() >= 36 {
                break;
            }
            for attempt in 0..4 {
                let jitter = hash(gx * 4 + attempt, gz, world.seed.wrapping_add(41_309));
                let x = gx as f32 * 1_400.0 + (jitter & 511) as f32 - 255.0;
                let z = gz as f32 * 1_400.0 + ((jitter >> 9) & 511) as f32 - 255.0;
                if !self.away_from_paths(x, z, 130.0)
                    || self
                        .villages
                        .iter()
                        .any(|v| distance2(x, z, v.center[0], v.center[2]) < 300.0_f32.powi(2))
                    || self.roadside_landmarks.iter().any(|s| {
                        let p = s.building.entrance();
                        distance2(x, z, p[0], p[2]) < 100.0_f32.powi(2)
                    })
                    || transit
                        .ports()
                        .iter()
                        .flat_map(|p| transit.dock_positions(p.village_id))
                        .any(|p| distance2(x, z, p[0], p[2]) < 160.0_f32.powi(2))
                    || self.composed_sites.iter().any(|s| {
                        let p = s.entrance();
                        distance2(x, z, p[0], p[2]) < 600.0_f32.powi(2)
                    })
                {
                    continue;
                }
                let start = (h as usize) % SiteArrangement::ALL.len();
                let mut choices: Vec<_> = (0..9)
                    .map(|i| SiteArrangement::ALL[(start + i) % 9])
                    .collect();
                choices.sort_by_key(|a| {
                    (
                        self.composed_sites
                            .iter()
                            .filter(|s| {
                                s.arrangement == *a && {
                                    let p = s.entrance();
                                    distance2(x, z, p[0], p[2]) < 2_000.0_f32.powi(2)
                                }
                            })
                            .count(),
                        self.composed_sites
                            .iter()
                            .filter(|s| s.arrangement == *a)
                            .count(),
                    )
                });
                let arrangement = choices[0];
                let id = ((world.seed as u64) << 32) | ((gx + 11) as u64 * 23 + (gz + 11) as u64);
                if let Some(site) = SitePlan::fit(id, arrangement, g, x, z) {
                    self.composed_sites.push(site);
                    break;
                }
            }
        }
        self.buckets.clear();
        self.build_index();
    }
}
