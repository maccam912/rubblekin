//! Actual resolved geometry and terrain-adapted examples, with human scale.
//! cargo run -p rubblekin_core --example poi_catalog -- artifacts/poi-compositions
use rubblekin_core::{
    poi::{SiteArrangement, SitePlan, SiteSolid},
    world::{Block, BlockPos, CELL_SIZE, World, WorldGeneration},
};
use serde_json::json;
use std::{fmt::Write, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let prefix = std::env::args()
        .nth(1)
        .unwrap_or("/tmp/poi-compositions".into());
    let world = World::generate(42, WorldGeneration::GeographyV6);
    let plan = world.settlements().unwrap();
    let mut samples = vec![];
    let mut used = std::collections::HashSet::new();
    for (recipe, a) in SiteArrangement::ALL.into_iter().enumerate() {
        for n in 0..4 {
            let site = plan
                .roadside_landmarks
                .iter()
                .enumerate()
                .find_map(|(i, s)| {
                    if used.contains(&i) {
                        return None;
                    }
                    let p = s.building.entrance();
                    SitePlan::fit(
                        (42u64 << 32) | ((recipe as u64) << 4) | (n * 5),
                        a,
                        world.geography().unwrap(),
                        p[0],
                        p[2],
                    )
                    .map(|s| (i, s))
                })
                .expect("four suitable terrain samples per arrangement");
            used.insert(site.0);
            samples.push(site.1);
        }
    }
    for gray in [false, true] {
        let mut svg = String::from(
            "<svg xmlns='http://www.w3.org/2000/svg' width='1440' height='3060' viewBox='0 0 1440 3060'><rect width='1440' height='3060' fill='#edf0e7'/>",
        );
        for (i, site) in samples.iter().enumerate() {
            let ox = (i % 4) as f32 * 360.;
            let oy = (i / 4) as f32 * 340.;
            let [x0, z0, x1, z1] = site.bounds;
            let floor = site.route[0][1] / CELL_SIZE - 1.;
            let scale = 250. / ((x1 - x0 + z1 - z0) as f32 * 0.866);
            let project = |p: [f32; 3]| {
                [
                    ox + 180. + ((p[0] - x0 as f32) - (p[2] - z0 as f32)) * 0.866 * scale,
                    oy + 88. + ((p[0] - x0 as f32) + (p[2] - z0 as f32)) * 0.5 * scale
                        - (p[1] - floor) * scale,
                ]
            };
            let mut solids = site.solids.clone();
            for z in (z0..z1).step_by(4) {
                for x in (x0..x1).step_by(4) {
                    let natural = (world
                        .geography()
                        .unwrap()
                        .sample((x as f32 + 0.5) * CELL_SIZE, (z as f32 + 0.5) * CELL_SIZE)
                        .height
                        / CELL_SIZE)
                        .floor() as i32;
                    let (height, block, _) = site.ground_at(x, z, natural, Block::Grass);
                    solids.push(SiteSolid {
                        min: BlockPos::new(x, height - 1, z),
                        size: [4.min(x1 - x), 2, 4.min(z1 - z)],
                        block,
                    });
                }
            }

            let p = site.entrance();
            solids.push(SiteSolid {
                min: BlockPos::new(
                    (p[0] / CELL_SIZE) as i32,
                    (p[1] / CELL_SIZE) as i32,
                    (p[2] / CELL_SIZE) as i32,
                ),
                size: [1, 3, 1],
                block: Block::BlueWool,
            });
            solids.sort_by_key(|s| s.min.x + s.min.z + s.size[0] + s.size[2]);
            for solid in solids {
                let [x, y, z] = [solid.min.x as f32, solid.min.y as f32, solid.min.z as f32];
                let [w, h, d] = solid.size.map(|v| v as f32);
                let faces = [
                    (
                        [
                            [x, y, z + d],
                            [x + w, y, z + d],
                            [x + w, y + h, z + d],
                            [x, y + h, z + d],
                        ],
                        0.68,
                    ),
                    (
                        [
                            [x + w, y, z],
                            [x + w, y, z + d],
                            [x + w, y + h, z + d],
                            [x + w, y + h, z],
                        ],
                        0.82,
                    ),
                    (
                        [
                            [x, y + h, z],
                            [x + w, y + h, z],
                            [x + w, y + h, z + d],
                            [x, y + h, z + d],
                        ],
                        1.0,
                    ),
                ];
                for (face, shade) in faces {
                    let points = face
                        .map(project)
                        .map(|p| format!("{:.1},{:.1}", p[0], p[1]))
                        .join(" ");
                    let c = solid.block.color();
                    let light = if gray { (c[0] + c[1] + c[2]) / 3. } else { 0. };
                    let rgb: [u8; 3] = std::array::from_fn(|i| {
                        ((if gray { light } else { c[i] }) * shade * 255.) as u8
                    });
                    write!(
                        svg,
                        "<polygon points='{points}' fill='rgb({},{},{})' stroke='#4d534b' stroke-width='.25'/>",
                        rgb[0], rgb[1], rgb[2]
                    )?;
                }
            }
            // An overhead route panel shows the grade changes and connections.
            let top_scale = 130. / (x1 - x0).max(z1 - z0) as f32;
            for patch in &site.ground {
                let [a, b, c, d] = patch.bounds;
                write!(
                    svg,
                    "<rect x='{}' y='{}' width='{}' height='{}' fill='#c5cbbd'/>",
                    ox + 20. + (a - x0) as f32 * top_scale,
                    oy + 195. + (b - z0) as f32 * top_scale,
                    (c - a) as f32 * top_scale,
                    (d - b) as f32 * top_scale
                )?;
            }
            let route = site
                .route
                .iter()
                .map(|p| {
                    format!(
                        "{},{}",
                        ox + 20. + (p[0] / CELL_SIZE - x0 as f32) * top_scale,
                        oy + 195. + (p[2] / CELL_SIZE - z0 as f32) * top_scale
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            write!(
                svg,
                "<polyline points='{route}' fill='none' stroke='#315a42' stroke-width='2'/>"
            )?;
            if !gray {
                write!(
                    svg,
                    "<text x='{}' y='{}' font-family='sans-serif' font-size='14'>{} · {}</text>",
                    ox + 16.,
                    oy + 20.,
                    site.arrangement.name(),
                    i % 4 + 1
                )?;
            }
        }
        svg.push_str("</svg>");
        fs::write(
            format!("{prefix}{}.svg", if gray { "-unlabelled" } else { "" }),
            svg,
        )?;
    }
    let describe = |s: &SitePlan| json!({"id":s.id,"orientation":s.orientation,"arrangement":s.arrangement.name(),"bounds_cells":s.bounds,"entrance":s.entrance(),"route":s.route,"solid_parts":s.solids.len(),"ground_patches":s.ground.len()});
    fs::write(
        format!("{prefix}.json"),
        serde_json::to_string_pretty(
            &json!({"seed":42,"samples":samples.iter().map(describe).collect::<Vec<_>>(),"world_sites":plan.composed_sites.iter().map(describe).collect::<Vec<_>>(),"small_discoveries":plan.roadside_landmarks.len()}),
        )?,
    )?;
    println!(
        "{} terrain-adapted review samples; {} world compositions; {} existing discoveries",
        samples.len(),
        plan.composed_sites.len(),
        plan.roadside_landmarks.len()
    );
    Ok(())
}
