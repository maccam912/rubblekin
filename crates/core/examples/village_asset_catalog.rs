//! Render the actual generated voxel assets as a compact vector review sheet.
//! Run: cargo run -p rubblekin_core --example village_asset_catalog -- artifacts/village-assets.svg

use rubblekin_core::{
    village_assets::{BuildingKind, block_at, dimensions},
    world::Block,
};
use std::{fmt::Write, fs};

fn project([x, y, z]: [i32; 3], offset: [f32; 2], scale: f32) -> [f32; 2] {
    [
        offset[0] + (x + z) as f32 * scale,
        offset[1] + (x - z) as f32 * scale * 0.5 - y as f32 * scale,
    ]
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "artifacts/village-assets.svg".to_owned());
    let mut svg = String::from(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="1168" viewBox="0 0 1200 1168"><rect width="1200" height="1168" fill="#18232b"/><g font-family="system-ui,sans-serif" fill="#edf3e9"><text x="32" y="37" font-size="24" font-weight="700">Rubblekin village assets</text><text x="32" y="61" font-size="13" fill="#b5c5c6">Editable 50 cm voxels · walk-in interiors · original homes + regional architecture + village landmarks</text></g>"##,
    );
    let assets = [
        (
            BuildingKind::Cottage,
            "Cottage",
            "Hearth, bed, bench, and table",
        ),
        (
            BuildingKind::Workshop,
            "Workshop",
            "Sloping roof and side workbenches",
        ),
        (
            BuildingKind::Storehouse,
            "Storehouse",
            "Timber barn and storage bins",
        ),
        (
            BuildingKind::Market,
            "Market",
            "Open awning and trading counters",
        ),
        (
            BuildingKind::TimberCabin,
            "Timber cabin",
            "Log walls, wood gable, furnished interior",
        ),
        (
            BuildingKind::MasonryCottage,
            "Masonry cottage",
            "Stone corners and a low hipped roof",
        ),
        (
            BuildingKind::UplandHouse,
            "Upland house",
            "Steep tiled gable and upper windows",
        ),
        (
            BuildingKind::Windmill,
            "Windmill",
            "Editable stationary sails and a grain mill",
        ),
        (
            BuildingKind::Lookout,
            "Lookout",
            "Walkable stairs to a sheltered viewing deck",
        ),
    ];
    for (index, (kind, title, detail)) in assets.into_iter().enumerate() {
        let [width, height, depth] = dimensions(kind);
        let panel_x = 24.0 + (index % 3) as f32 * 388.0;
        let panel_y = 80.0 + (index / 3) as f32 * 365.0;
        let scale = 7.5_f32
            .min(340.0 / (width + depth) as f32)
            .min(270.0 / (height as f32 + (width + depth) as f32 * 0.5));
        let offset = [
            panel_x + (364.0 - (width + depth) as f32 * scale) / 2.0,
            panel_y + 10.0 + (height as f32 + depth as f32 * 0.5) * scale,
        ];
        let mut faces = Vec::new();
        for x in 0..width {
            for y in 0..height {
                for z in 0..depth {
                    let block = block_at(kind, x, y, z).unwrap();
                    if block == Block::Air {
                        continue;
                    }
                    for (neighbor, vertices, light) in [
                        (
                            [x + 1, y, z],
                            [
                                [x + 1, y, z],
                                [x + 1, y, z + 1],
                                [x + 1, y + 1, z + 1],
                                [x + 1, y + 1, z],
                            ],
                            0.76,
                        ),
                        (
                            [x, y, z - 1],
                            [[x, y, z], [x + 1, y, z], [x + 1, y + 1, z], [x, y + 1, z]],
                            0.88,
                        ),
                        (
                            [x, y + 1, z],
                            [
                                [x, y + 1, z],
                                [x + 1, y + 1, z],
                                [x + 1, y + 1, z + 1],
                                [x, y + 1, z + 1],
                            ],
                            1.0,
                        ),
                    ] {
                        if block_at(kind, neighbor[0], neighbor[1], neighbor[2])
                            .unwrap_or(Block::Air)
                            .is_solid()
                        {
                            continue;
                        }
                        let mut color = block.color();
                        color.iter_mut().take(3).for_each(|v| *v *= light);
                        let depth = vertices.iter().map(|p| p[0] - p[2] + p[1]).sum::<i32>();
                        faces.push((depth, vertices.map(|p| project(p, offset, scale)), color));
                    }
                }
            }
        }
        faces.sort_by_key(|(depth, _, _)| *depth);
        for (_, points, color) in faces {
            let points = points
                .iter()
                .map(|[x, y]| format!("{x:.1},{y:.1}"))
                .collect::<Vec<_>>()
                .join(" ");
            write!(
                svg,
                "<polygon points=\"{points}\" fill=\"rgb({},{},{})\" stroke=\"#18232b\" stroke-opacity=\"0.10\" stroke-width=\"0.4\"/>",
                (color[0] * 255.0) as u32,
                (color[1] * 255.0) as u32,
                (color[2] * 255.0) as u32,
            )
            .unwrap();
        }
        let title_y = 302.0 + panel_y;
        let detail_y = 328.0 + panel_y;
        write!(
            svg,
            "<g font-family=\"system-ui,sans-serif\" fill=\"#edf3e9\"><text x=\"{panel_x}\" y=\"{title_y}\" font-size=\"19\" font-weight=\"600\">{title}</text><text x=\"{panel_x}\" y=\"{detail_y}\" font-size=\"12\" fill=\"#b5c5c6\">{detail}</text></g>"
        )
        .unwrap();
    }
    svg.push_str("</svg>");
    fs::write(&path, svg).expect("write village asset catalog");
    println!("Wrote {path}");
}
