//! Render the actual generated voxel assets as a compact vector review sheet.
//! Add `--trees` after the output path for the tree silhouette sheet.
//! Run: cargo run -p rubblekin_core --example village_asset_catalog -- artifacts/village-assets.svg

use rubblekin_core::{
    village_assets::{BuildingKind, block_at, dimensions},
    world::{Block, BlockPos, GeneratedTree, TreeKind},
};
use std::{fmt::Write, fs};

#[derive(Clone, Copy)]
enum Asset {
    Building(BuildingKind),
    Tree(GeneratedTree),
}
impl Asset {
    fn dimensions(self) -> [i32; 3] {
        match self {
            Self::Building(kind) => dimensions(kind),
            Self::Tree(tree) => [
                tree.crown_radius * 2 + 3,
                tree.leaf_bounds(0, 0).unwrap().1 + 1,
                tree.crown_radius * 2 + 3,
            ],
        }
    }
    fn block(self, x: i32, y: i32, z: i32) -> Option<Block> {
        match self {
            Self::Building(kind) => block_at(kind, x, y, z),
            Self::Tree(tree) => {
                let [w, h, d] = self.dimensions();
                if !(0..w).contains(&x) || !(0..h).contains(&y) || !(0..d).contains(&z) {
                    return None;
                }
                Some(
                    if x == tree.base.x && z == tree.base.z && y <= tree.crown_y() {
                        Block::Wood
                    } else if tree
                        .leaf_bounds(x - tree.base.x, z - tree.base.z)
                        .is_some_and(|(lo, hi)| (lo..=hi).contains(&y))
                    {
                        Block::Leaves
                    } else {
                        Block::Air
                    },
                )
            }
        }
    }
    fn color(self, block: Block) -> [f32; 4] {
        if block == Block::Leaves
            && let Self::Tree(tree) = self
        {
            return tree.kind.leaf_color();
        }
        block.color()
    }
}

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
    let mut assets: Vec<_> = [
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
        (
            BuildingKind::TrailRuin,
            "Trail ruin",
            "Open arches, a roofless hall, and an old hearth",
        ),
        (
            BuildingKind::Waystone,
            "Waystone",
            "A roadside marker and a place to rest",
        ),
    ]
    .into_iter()
    .map(|(kind, title, detail)| (Asset::Building(kind), title, detail))
    .collect();
    let trees = std::env::args().any(|arg| arg == "--trees");
    if trees {
        assets = [
            (
                TreeKind::Broadleaf,
                13,
                4,
                "Broadleaf",
                "Original rounded canopy",
            ),
            (
                TreeKind::Conifer,
                21,
                4,
                "Conifer",
                "Original conical crown",
            ),
            (TreeKind::Scrub, 4, 2, "Scrub", "Original low dryland shrub"),
            (
                TreeKind::Aspen,
                20,
                2,
                "Aspen",
                "Narrow upright crown in temperate forests",
            ),
            (
                TreeKind::Cedar,
                25,
                5,
                "Cedar",
                "Broad tapered crown in pine forests",
            ),
            (
                TreeKind::Canopy,
                21,
                5,
                "Canopy tree",
                "High spreading crown in rainforests",
            ),
        ]
        .into_iter()
        .map(|(kind, trunk_height, crown_radius, title, detail)| {
            (
                Asset::Tree(GeneratedTree {
                    base: BlockPos::new(crown_radius + 1, 0, crown_radius + 1),
                    trunk_height,
                    kind,
                    crown_radius,
                }),
                title,
                detail,
            )
        })
        .collect();
    }
    let height = 80 + assets.len().div_ceil(3) * 365;
    let title = if trees {
        "Rubblekin tree silhouettes"
    } else {
        "Rubblekin village and trail assets"
    };
    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="{height}" viewBox="0 0 1200 {height}"><rect width="1200" height="{height}" fill="#18232b"/><g font-family="system-ui,sans-serif" fill="#edf3e9"><text x="32" y="37" font-size="24" font-weight="700">{title}</text><text x="32" y="61" font-size="13" fill="#b5c5c6">Actual editable 50 cm geometry and material colors · panels scaled to fit</text></g>"##
    );
    for (index, (asset, title, detail)) in assets.into_iter().enumerate() {
        let [width, height, depth] = asset.dimensions();
        let panel_x = 24.0 + (index % 3) as f32 * 388.0;
        let panel_y = 80.0 + (index / 3) as f32 * 365.0;
        let scale = 7.5_f32
            .min(340.0 / (width + depth) as f32)
            .min(270.0 / (height as f32 + (width + depth) as f32 * 0.5));
        let offset = [
            panel_x + (364.0 - (width + depth) as f32 * scale) / 2.0,
            panel_y + 280.0 - width as f32 * 0.5 * scale,
        ];
        let mut faces = Vec::new();
        for x in 0..width {
            for y in 0..height {
                for z in 0..depth {
                    let block = asset.block(x, y, z).unwrap();
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
                        if asset
                            .block(neighbor[0], neighbor[1], neighbor[2])
                            .unwrap_or(Block::Air)
                            .is_solid()
                        {
                            continue;
                        }
                        let mut color = asset.color(block);
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
