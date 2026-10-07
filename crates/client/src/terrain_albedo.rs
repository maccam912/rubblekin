//! A once-per-world painted map for smooth distant height fields. Landforms,
//! hydrology, canopy, fields, and roads all come from the actual generated world.
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rubblekin_core::{
    geography::{Biome, GRID_SIDE, GRID_SPACING, GeoSample, Geography, WORLD_SIZE},
    settlement::Trail,
    village_assets::BuildingKind,
    world::{CELL_SIZE, GeneratedTree, World, WorldGeneration},
};

const BASE_SIDE: u32 = 2048;
const DESKTOP_SIDE: u32 = 8192;
const TREE_GRID_CELLS: i32 = 24;
const ROAD_COLOR: [u8; 4] = [181, 153, 101, 255];
const FIELD_COLOR: [u8; 4] = [167, 164, 82, 255];
const ROOF_COLOR: [u8; 4] = [148, 101, 75, 255];

#[derive(Clone, Copy)]
struct MapGrid {
    side: u32,
    texel_meters: f32,
}

impl MapGrid {
    fn new(side: u32) -> Self {
        Self {
            side,
            texel_meters: WORLD_SIZE / side as f32,
        }
    }

    fn center(self, x: u32, z: u32) -> [f32; 2] {
        [x, z].map(|value| (value as f32 + 0.5) * self.texel_meters - WORLD_SIZE * 0.5)
    }

    fn bounds(self, min: f32, max: f32) -> [u32; 2] {
        [min, max].map(|value| {
            ((value + WORLD_SIZE * 0.5) / self.texel_meters)
                .floor()
                .clamp(0.0, (self.side - 1) as f32) as u32
        })
    }
}

pub fn atlas_side(max_texture_side: u32, mobile: bool) -> u32 {
    let requested = if mobile { BASE_SIDE } else { DESKTOP_SIDE };
    let supported = max_texture_side.max(1);
    // Mips and raster grids use power-of-two sizes, including smaller devices.
    requested.min(1 << supported.ilog2())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreparationStage {
    Ground,
    Trees,
    WaterAndRoads,
    Mips,
}

#[cfg(test)]
pub fn distant_albedo(world: &World, side: u32) -> Image {
    prepare_albedo(world, side, |_| Ok(())).expect("uncancelled atlas preparation")
}

/// The callback reports real preparation phases and permits a join worker to
/// cancel between bounded groups of rows, without allocating renderer assets.
pub(crate) fn prepare_albedo(
    world: &World,
    side: u32,
    mut progress: impl FnMut(PreparationStage) -> Result<(), String>,
) -> Result<Image, String> {
    progress(PreparationStage::Ground)?;
    let (side, mut data) = if let Some(geo) = world.geography() {
        let grid = MapGrid::new(side);
        let palette = biome_palette();
        let mut data = Vec::with_capacity(mip_byte_len(side));
        for z in 0..side {
            if z % 64 == 0 {
                progress(PreparationStage::Ground)?;
            }
            for x in 0..side {
                let [mx, mz] = grid.center(x, z);
                let sample = geo.sample(mx, mz);
                data.extend_from_slice(&map_color(sample, map_relief(geo, mx, mz), &palette));
            }
        }
        paint_trees_with_progress(world, grid, &mut data, &mut || {
            progress(PreparationStage::Trees)
        })?;
        progress(PreparationStage::WaterAndRoads)?;
        paint_rivers(geo, grid, &mut data);
        progress(PreparationStage::WaterAndRoads)?;
        paint_settlements(world, grid, &mut data);
        (side, data)
    } else {
        (1, vec![255; 4])
    };
    let levels =
        append_mips_with_progress(&mut data, side, &mut || progress(PreparationStage::Mips))?;
    // Image::new validates only a base level, so initialize explicitly with the
    // complete level-ordered mip chain. This adds one third to the base size.
    let mut image = Image::new_uninit(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        // Bevy moves the upload pixels out while retaining Image metadata for
        // world-map sizing. Avoid a permanent second 341 MiB desktop copy.
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::linear();
    Ok(image)
}

fn biome_index(biome: Biome) -> usize {
    match biome {
        Biome::Ocean => 0,
        Biome::Beach => 1,
        Biome::Grassland => 2,
        Biome::Forest => 3,
        Biome::Rainforest => 4,
        Biome::Desert => 5,
        Biome::Tundra => 6,
        Biome::Alpine => 7,
        Biome::Snow => 8,
        Biome::PineForest => 9,
        Biome::Shrubland => 10,
    }
}

fn biome_palette() -> [Vec3; 11] {
    [
        Biome::Ocean,
        Biome::Beach,
        Biome::Grassland,
        Biome::Forest,
        Biome::Rainforest,
        Biome::Desert,
        Biome::Tundra,
        Biome::Alpine,
        Biome::Snow,
        Biome::PineForest,
        Biome::Shrubland,
    ]
    .map(|biome| linear_rgb(biome.color()))
}

/// Enumerate the same candidates used by editable and medium-LOD trees once.
/// Rasterize a small crown-colored dot; no mesh or per-tree asset is allocated.
#[cfg(test)]
fn paint_trees(world: &World, grid: MapGrid, data: &mut [u8]) {
    paint_trees_with_progress(world, grid, data, &mut || Ok(())).unwrap();
}

fn paint_trees_with_progress(
    world: &World,
    grid: MapGrid,
    data: &mut [u8],
    progress: &mut impl FnMut() -> Result<(), String>,
) -> Result<(), String> {
    let first = (-world.radius_cells()).div_euclid(TREE_GRID_CELLS);
    let last = (world.radius_cells() - 1).div_euclid(TREE_GRID_CELLS);
    let refined = matches!(
        world.generation(),
        WorldGeneration::GeographyV2
            | WorldGeneration::GeographyV3
            | WorldGeneration::GeographyV4
            | WorldGeneration::GeographyV5
            | WorldGeneration::GeographyV6
    );
    for gz in first..=last {
        if (gz - first) % 64 == 0 {
            progress()?;
        }
        for gx in first..=last {
            if let Some(tree) = world.tree_at(gx, gz) {
                let color = rgba_bytes(if refined {
                    tree.kind.leaf_color()
                } else {
                    [0.24, 0.40, 0.31, 1.0]
                });
                paint_tree(data, grid, tree, color, world.geography());
            }
        }
    }
    Ok(())
}

fn paint_tree(
    data: &mut [u8],
    grid: MapGrid,
    tree: GeneratedTree,
    color: [u8; 4],
    geo: Option<&Geography>,
) {
    let radius = (tree.crown_radius as f32 + 0.5) * CELL_SIZE;
    let center = [tree.base.x, tree.base.z].map(|cell| (cell as f32 + 0.5) * CELL_SIZE);
    // Exact pixel-area coverage keeps small crowns visible at low resolution
    // without inflating them into an opaque 16 m square. The footprint is only
    // 4.5 m across for trees and 2.5 m for scrub.
    let min = center.map(|axis| axis - radius);
    let max = center.map(|axis| axis + radius);
    let [x0, x1] = grid.bounds(min[0], max[0]);
    let [z0, z1] = grid.bounds(min[1], max[1]);
    for z in z0..=z1 {
        for x in x0..=x1 {
            let point = grid.center(x, z);
            if geo.is_some_and(|geo| geo.sample(point[0], point[1]).water.is_some()) {
                continue;
            }
            let coverage = rect_coverage(grid, point, min, max);
            let offset = ((z * grid.side + x) * 4) as usize;
            blend_pixel(&mut data[offset..offset + 4], color, coverage);
        }
    }
}

/// Restrained map hillshade derived directly from the existing 64 m height
/// grid. It emphasizes large folds, leaving the renderer to light the mesh.
fn map_relief(geo: &Geography, x: f32, z: f32) -> f32 {
    let gx = (x + WORLD_SIZE * 0.5) / GRID_SPACING;
    let gz = (z + WORLD_SIZE * 0.5) / GRID_SPACING;
    let ix = (gx.floor() as usize).min(GRID_SIDE - 2);
    let iz = (gz.floor() as usize).min(GRID_SIDE - 2);
    let heights = geo.heights();
    let i = iz * GRID_SIDE + ix;
    let dx = ((heights[i + 1] - heights[i]) * (1.0 - gz.fract())
        + (heights[i + GRID_SIDE + 1] - heights[i + GRID_SIDE]) * gz.fract())
        / GRID_SPACING;
    let dz = ((heights[i + GRID_SIDE] - heights[i]) * (1.0 - gx.fract())
        + (heights[i + GRID_SIDE + 1] - heights[i + 1]) * gx.fract())
        / GRID_SPACING;
    ((1.0 + dx * 0.14 + dz * 0.09) / (1.0 + dx * dx * 0.04 + dz * dz * 0.04).sqrt())
        .clamp(0.84, 1.09)
}

fn map_color(sample: GeoSample, relief: f32, palette: &[Vec3; 11]) -> [u8; 4] {
    if let Some(level) = sample.water {
        // Match the review map's turquoise shallows and muted deep water.
        let depth = ((level - sample.height) / 100.0).clamp(0.0, 1.0);
        return rgba_bytes([
            0.20 - depth * 0.10,
            0.49 - depth * 0.16,
            0.62 - depth * 0.16,
            1.0,
        ]);
    }
    let color = palette[biome_index(sample.biome)];
    let srgb = Srgba::from(LinearRgba::new(color.x, color.y, color.z, 1.0));
    rgba_bytes([
        srgb.red * relief,
        srgb.green * relief,
        srgb.blue * relief,
        1.0,
    ])
}

/// Wet vertices whose filled level is close to their ground belong to channels,
/// rather than lakes or ocean. Verify the downstream midpoint against actual
/// water: drainage alone also contains many dry gullies, which must stay dry.
fn river_segments(geo: &Geography) -> Vec<(Vec2, Vec2)> {
    let mut segments = Vec::new();
    for (index, next) in geo.drainage().iter().enumerate() {
        let Some(next) = next else {
            continue;
        };
        let ground = geo.heights()[index];
        let water = geo.water_heights()[index];
        if ground <= 0.0 || !water.is_finite() || water > ground + 0.6 {
            continue;
        }
        let a = Vec2::from_array(geo.grid_position(index));
        let b = Vec2::from_array(geo.grid_position(*next));
        let mid = (a + b) * 0.5;
        let sample = geo.sample(mid.x, mid.y);
        if sample
            .water
            .is_some_and(|level| level > 0.0 && level - sample.height <= 3.0)
        {
            segments.push((a, b));
        }
    }
    segments
}

fn river_mask(geo: &Geography, grid: MapGrid) -> Vec<u8> {
    let mut mask = vec![0; (grid.side * grid.side) as usize];
    for (a, b) in river_segments(geo) {
        // Even at 4 m per texel, narrow tributaries can miss texel centers. A
        // restrained one-texel stroke keeps the actual channel continuous.
        stamp_segment(&mut mask, grid, a, b, grid.texel_meters * 0.55);
    }
    mask
}

fn paint_rivers(geo: &Geography, grid: MapGrid, data: &mut [u8]) {
    let mask = river_mask(geo, grid);
    let shallow_water = rgba_bytes([0.198, 0.487, 0.617, 1.0]);
    for (index, coverage) in mask.into_iter().enumerate() {
        if coverage == 0 {
            continue;
        }
        let [x, z] = grid.center(index as u32 % grid.side, index as u32 / grid.side);
        // Keep exact lake, broad river, and ocean colors/depths. Only fill
        // sampling gaps alongside verified channel centerlines.
        if geo.sample(x, z).water.is_none() {
            blend_pixel(
                &mut data[index * 4..index * 4 + 4],
                shallow_water,
                coverage as f32 / 255.0,
            );
        }
    }
}

fn paint_settlements(world: &World, grid: MapGrid, data: &mut [u8]) {
    let Some(plan) = world.settlements() else {
        return;
    };
    for village in &plan.villages {
        for field in &village.fields {
            paint_rect(
                data,
                grid,
                [
                    field.origin.x as f32 * CELL_SIZE,
                    field.origin.z as f32 * CELL_SIZE,
                ],
                [
                    field.width as f32 * CELL_SIZE,
                    field.depth as f32 * CELL_SIZE,
                ],
                FIELD_COLOR,
            );
        }
    }
    let mask = route_mask(
        grid,
        plan.trails
            .iter()
            .chain(plan.villages.iter().flat_map(|v| &v.lanes))
            .chain(plan.roadside_landmarks.iter().map(|site| &site.approach)),
    );
    for (pixel, coverage) in data.as_chunks_mut::<4>().0.iter_mut().zip(mask) {
        blend_pixel(pixel, ROAD_COLOR, coverage as f32 / 255.0);
    }
    for building in plan
        .villages
        .iter()
        .flat_map(|v| &v.buildings)
        .chain(plan.roadside_landmarks.iter().map(|site| &site.building))
    {
        let [width, _, depth] = building.dimensions();
        paint_rect(
            data,
            grid,
            [
                building.origin.x as f32 * CELL_SIZE,
                building.origin.z as f32 * CELL_SIZE,
            ],
            [width as f32 * CELL_SIZE, depth as f32 * CELL_SIZE],
            match building.kind {
                BuildingKind::TimberCabin
                | BuildingKind::Windmill
                | BuildingKind::TrailPavilion => [128, 101, 72, 255],
                BuildingKind::MasonryCottage
                | BuildingKind::TrailRuin
                | BuildingKind::Waystone
                | BuildingKind::QuarryYard => [139, 142, 133, 255],
                _ => ROOF_COLOR,
            },
        );
    }
}

/// A small antialiased footprint, never a village icon. Distant fields/roofs
/// retain their actual centers and orientation, with a 0.6-texel minimum size.
fn paint_rect(data: &mut [u8], grid: MapGrid, origin: [f32; 2], size: [f32; 2], color: [u8; 4]) {
    let center = [origin[0] + size[0] * 0.5, origin[1] + size[1] * 0.5];
    let half = [
        size[0].max(grid.texel_meters * 0.6) * 0.5,
        size[1].max(grid.texel_meters * 0.6) * 0.5,
    ];
    let min = [center[0] - half[0], center[1] - half[1]];
    let max = [center[0] + half[0], center[1] + half[1]];
    let [x0, x1] = grid.bounds(min[0], max[0]);
    let [z0, z1] = grid.bounds(min[1], max[1]);
    for z in z0..=z1 {
        for x in x0..=x1 {
            let coverage = rect_coverage(grid, grid.center(x, z), min, max);
            let offset = ((z * grid.side + x) * 4) as usize;
            blend_pixel(&mut data[offset..offset + 4], color, coverage);
        }
    }
}

fn rect_coverage(grid: MapGrid, point: [f32; 2], min: [f32; 2], max: [f32; 2]) -> f32 {
    let overlap = |axis: usize| {
        (max[axis].min(point[axis] + grid.texel_meters * 0.5)
            - min[axis].max(point[axis] - grid.texel_meters * 0.5))
        .max(0.0)
    };
    overlap(0) * overlap(1) / grid.texel_meters.powi(2)
}

fn route_mask<'a>(grid: MapGrid, trails: impl Iterator<Item = &'a Trail>) -> Vec<u8> {
    let mut mask = vec![0; (grid.side * grid.side) as usize];
    for trail in trails {
        // One texel across keeps real 3 m trails legible at distance. The
        // course follows every route bend; a coverage mask avoids dark joins.
        let radius = (trail.width * 0.5).max(grid.texel_meters * 0.45);
        for pair in trail.points.windows(2) {
            let a = Vec2::new(pair[0][0], pair[0][2]);
            let b = Vec2::new(pair[1][0], pair[1][2]);
            stamp_segment(&mut mask, grid, a, b, radius);
        }
    }
    mask
}

fn stamp_segment(mask: &mut [u8], grid: MapGrid, a: Vec2, b: Vec2, radius: f32) {
    let padding = radius + grid.texel_meters * 0.5;
    let [x0, x1] = grid.bounds(a.x.min(b.x) - padding, a.x.max(b.x) + padding);
    let [z0, z1] = grid.bounds(a.y.min(b.y) - padding, a.y.max(b.y) + padding);
    let delta = b - a;
    let length_squared = delta.length_squared();
    for z in z0..=z1 {
        for x in x0..=x1 {
            let p = Vec2::from_array(grid.center(x, z));
            let t = if length_squared > 0.0 {
                ((p - a).dot(delta) / length_squared).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let distance = p.distance(a + delta * t);
            let coverage =
                ((radius + grid.texel_meters * 0.5 - distance) / grid.texel_meters).clamp(0.0, 1.0);
            let index = (z * grid.side + x) as usize;
            mask[index] = mask[index].max((coverage * 255.0).round() as u8);
        }
    }
}

fn blend_pixel(pixel: &mut [u8], color: [u8; 4], coverage: f32) {
    for channel in 0..3 {
        pixel[channel] = (pixel[channel] as f32 * (1.0 - coverage)
            + color[channel] as f32 * coverage)
            .round() as u8;
    }
}

fn linear_rgb(color: [f32; 4]) -> Vec3 {
    let linear = LinearRgba::from(Srgba::new(color[0], color[1], color[2], color[3]));
    Vec3::new(linear.red, linear.green, linear.blue)
}

fn rgba_bytes(color: [f32; 4]) -> [u8; 4] {
    color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn mip_byte_len(mut side: u32) -> usize {
    let mut bytes = 0;
    loop {
        bytes += (side * side * 4) as usize;
        if side <= 1 {
            return bytes;
        }
        side /= 2;
    }
}

/// Box-filter in linear light: directly averaging sRGB bytes darkens small
/// bright roads, coastlines, and snow. Store levels largest to smallest.
#[cfg(test)]
fn append_mips(data: &mut Vec<u8>, side: u32) -> u32 {
    append_mips_with_progress(data, side, &mut || Ok(())).unwrap()
}

fn append_mips_with_progress(
    data: &mut Vec<u8>,
    mut side: u32,
    progress: &mut impl FnMut() -> Result<(), String>,
) -> Result<u32, String> {
    progress()?;
    data.reserve(mip_byte_len(side) - data.len());
    let lookup: [f32; 256] = std::array::from_fn(|i| linear_rgb([i as f32 / 255.0; 4]).x);
    let mut offset = 0;
    let mut levels = 1;
    while side > 1 {
        let next_side = side / 2;
        let next_offset = data.len();
        data.resize(next_offset + (next_side * next_side * 4) as usize, 255);
        for z in 0..next_side {
            if z % 64 == 0 {
                progress()?;
            }
            for x in 0..next_side {
                let corners = [
                    offset + ((z * 2 * side + x * 2) * 4) as usize,
                    offset + ((z * 2 * side + x * 2 + 1) * 4) as usize,
                    offset + (((z * 2 + 1) * side + x * 2) * 4) as usize,
                    offset + (((z * 2 + 1) * side + x * 2 + 1) * 4) as usize,
                ];
                let mut rgb = [0.0; 3];
                for (channel, value) in rgb.iter_mut().enumerate() {
                    *value = corners
                        .iter()
                        .map(|i| lookup[data[i + channel] as usize])
                        .sum::<f32>()
                        * 0.25;
                }
                let srgb = Srgba::from(LinearRgba::new(rgb[0], rgb[1], rgb[2], 1.0));
                let pixel = rgba_bytes([srgb.red, srgb.green, srgb.blue, 1.0]);
                let target = next_offset + ((z * next_side + x) * 4) as usize;
                data[target..target + 4].copy_from_slice(&pixel);
            }
        }
        offset = next_offset;
        side = next_side;
        levels += 1;
    }
    Ok(levels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::world::{Block, BlockPos, TreeKind};

    const ATLAS_SIDE: u32 = BASE_SIDE;
    const TEST_GRID: MapGrid = MapGrid {
        side: ATLAS_SIDE,
        texel_meters: WORLD_SIZE / ATLAS_SIDE as f32,
    };

    fn black_map(grid: MapGrid) -> Vec<u8> {
        [0, 0, 0, 255].repeat((grid.side * grid.side) as usize)
    }

    #[test]
    fn atlas_preparation_can_cancel_during_ground_trees_and_mip_rows() {
        let world = World::generate(42, WorldGeneration::GeographyV2);
        for target in [
            PreparationStage::Ground,
            PreparationStage::Trees,
            PreparationStage::Mips,
        ] {
            let mut callbacks = 0;
            let result = prepare_albedo(&world, 256, |stage| {
                if stage == target {
                    callbacks += 1;
                    if callbacks == 3 {
                        return Err("cancelled atlas".into());
                    }
                }
                Ok(())
            });
            assert!(matches!(result, Err(error) if error == "cancelled atlas"));
            assert_eq!(callbacks, 3, "{target:?} checks while the phase is running");
        }
    }

    #[test]
    fn atlas_resolution_respects_desktop_mobile_and_device_texture_limits() {
        for (limit, desktop, mobile) in [
            (0, 1, 1),
            (1, 1, 1),
            (1024, 1024, 1024),
            (2048, 2048, 2048),
            (4096, 4096, 2048),
            (6000, 4096, 2048),
            (8192, 8192, 2048),
            (16384, 8192, 2048),
        ] {
            assert_eq!(atlas_side(limit, false), desktop);
            assert_eq!(atlas_side(limit, true), mobile);
        }
        assert_eq!(MapGrid::new(DESKTOP_SIDE).texel_meters, 4.0);
        assert_eq!(mip_byte_len(BASE_SIDE), 22_369_620);
        assert_eq!(mip_byte_len(DESKTOP_SIDE), 357_913_940);
        for side in [1, 2, 4, 8, 32] {
            let mut data = [23, 47, 89, 255].repeat((side * side) as usize);
            assert_eq!(append_mips(&mut data, side), side.ilog2() + 1);
            assert_eq!(data.len(), mip_byte_len(side));
            assert!(data.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255));
        }
    }

    #[test]
    fn atlas_upload_moves_pixels_and_keeps_world_map_dimensions() {
        use bevy::render::{render_asset::RenderAsset, texture::GpuImage};

        let mut image = distant_albedo(&World::new(42), BASE_SIDE);
        assert_eq!(image.asset_usage, RenderAssetUsages::RENDER_WORLD);
        let size = image.size();
        let pixel_address = image.data.as_ref().unwrap().as_ptr();
        let upload = GpuImage::take_gpu_data(&mut image, None).unwrap();
        assert_eq!(upload.data.as_ref().unwrap().as_ptr(), pixel_address);
        assert!(image.data.is_none());
        assert_eq!(image.size(), size);
        assert_eq!(image.texture_descriptor.mip_level_count, 1);
    }

    #[test]
    fn crown_dots_use_the_exact_negative_half_cell_anchor_and_physical_size() {
        let grid = MapGrid {
            side: 32,
            texel_meters: 4.0,
        };
        let tree = GeneratedTree {
            base: BlockPos::new(-32684, 100, -32716),
            trunk_height: 12,
            kind: TreeKind::Broadleaf,
            crown_radius: 4,
        };
        let mut data = black_map(grid);
        paint_tree(&mut data, grid, tree, [255; 4], None);
        for z in 0..grid.side {
            for x in 0..grid.side {
                // The actual crown spans [-16344,-16339.5] and
                // [-16360,-16355.5], including its half-cell center offset.
                let value = match (x, z) {
                    (10, 6) => 255,
                    (11, 6) | (10, 7) => 32,
                    (11, 7) => 4,
                    _ => 0,
                };
                let offset = ((z * grid.side + x) * 4) as usize;
                assert_eq!(&data[offset..offset + 4], &[value, value, value, 255]);
            }
        }

        let mut scrub = black_map(grid);
        paint_tree(
            &mut scrub,
            grid,
            GeneratedTree {
                kind: TreeKind::Scrub,
                crown_radius: 2,
                ..tree
            },
            [255; 4],
            None,
        );
        let offset = ((6 * grid.side + 10) * 4) as usize;
        assert_eq!(&scrub[offset..offset + 4], &[100, 100, 100, 255]);
        assert_eq!(
            scrub
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[0] != 0)
                .count(),
            1,
            "a 2.5 m scrub crown stays smaller than a 4 m texel"
        );
    }

    #[test]
    fn a_crown_touching_wet_texels_keeps_the_actual_water_color() {
        let world = World::generate(42, WorldGeneration::GeographyV2);
        let geo = world.geography().unwrap();
        let grid = MapGrid {
            side: 32,
            texel_meters: 4.0,
        };
        let tree = GeneratedTree {
            base: BlockPos::new(-32684, 100, -32716),
            trunk_height: 12,
            kind: TreeKind::Broadleaf,
            crown_radius: 4,
        };
        let water = [25, 84, 117, 255];
        let mut data = water.repeat((grid.side * grid.side) as usize);
        for (x, z) in [(10, 6), (11, 6), (10, 7), (11, 7)] {
            let [mx, mz] = grid.center(x, z);
            assert!(geo.sample(mx, mz).water.is_some());
        }
        paint_tree(&mut data, grid, tree, [64, 110, 59, 255], Some(geo));
        assert!(data.as_chunks::<4>().0.iter().all(|pixel| *pixel == water));
    }

    fn trees_overlapping_pixel(world: &World, grid: MapGrid, x: u32, z: u32) -> usize {
        let center = grid.center(x, z);
        let min = center.map(|axis| axis - grid.texel_meters * 0.5);
        let max = center.map(|axis| axis + grid.texel_meters * 0.5);
        let first = min.map(|axis| ((axis - 3.0) / 12.0).floor() as i32);
        let last = max.map(|axis| ((axis + 3.0) / 12.0).floor() as i32);
        let mut count = 0;
        for gz in first[1]..=last[1] {
            for gx in first[0]..=last[0] {
                let Some(tree) = world.tree_at(gx, gz) else {
                    continue;
                };
                let position =
                    [tree.base.x, tree.base.z].map(|cell| (cell as f32 + 0.5) * CELL_SIZE);
                let radius = (tree.crown_radius as f32 + 0.5) * CELL_SIZE;
                if (0..2).all(|axis| {
                    position[axis] + radius > min[axis] && position[axis] - radius < max[axis]
                }) {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn painted_dots_match_generated_negative_positions_colors_and_empty_candidates() {
        for generation in [
            WorldGeneration::GeographyV2,
            WorldGeneration::GeographyV1,
            WorldGeneration::GeographyV5,
            WorldGeneration::GeographyV6,
        ] {
            let world = World::generate(42, generation);
            let geo = world.geography().unwrap();
            let mut data = black_map(TEST_GRID);
            paint_trees(&world, TEST_GRID, &mut data);
            let kinds = if matches!(
                generation,
                WorldGeneration::GeographyV5 | WorldGeneration::GeographyV6
            ) {
                vec![
                    TreeKind::Broadleaf,
                    TreeKind::Conifer,
                    TreeKind::Scrub,
                    TreeKind::Aspen,
                    TreeKind::Cedar,
                    TreeKind::Canopy,
                ]
            } else if generation == WorldGeneration::GeographyV2 {
                vec![TreeKind::Broadleaf, TreeKind::Conifer, TreeKind::Scrub]
            } else {
                vec![TreeKind::Broadleaf]
            };
            let mut found = vec![false; kinds.len()];
            let first = (-world.radius_cells()).div_euclid(TREE_GRID_CELLS);
            let last = (world.radius_cells() - 1).div_euclid(TREE_GRID_CELLS);
            'candidates: for gz in first..=last {
                for gx in first..=last {
                    let Some(tree) = world.tree_at(gx, gz) else {
                        continue;
                    };
                    let Some(kind_index) = kinds.iter().position(|kind| *kind == tree.kind) else {
                        continue;
                    };
                    if found[kind_index] || (tree.base.x >= 0 && tree.base.z >= 0) {
                        continue;
                    }
                    let center =
                        [tree.base.x, tree.base.z].map(|cell| (cell as f32 + 0.5) * CELL_SIZE);
                    let [x, _] = TEST_GRID.bounds(center[0], center[0]);
                    let [z, _] = TEST_GRID.bounds(center[1], center[1]);
                    let point = TEST_GRID.center(x, z);
                    let radius = (tree.crown_radius as f32 + 0.5) * CELL_SIZE;
                    if (0..2).any(|axis| {
                        (center[axis] - point[axis]).abs() + radius > TEST_GRID.texel_meters * 0.5
                    }) || geo.sample(point[0], point[1]).water.is_some()
                        || trees_overlapping_pixel(&world, TEST_GRID, x, z) != 1
                    {
                        continue;
                    }
                    assert_eq!(world.block(tree.base), Block::Wood);
                    let color = if generation == WorldGeneration::GeographyV1 {
                        [61_u8, 102, 79]
                    } else {
                        match tree.kind {
                            TreeKind::Broadleaf | TreeKind::Aspen | TreeKind::Canopy => {
                                [64, 110, 59]
                            }
                            TreeKind::Conifer | TreeKind::Cedar => [46, 87, 64],
                            TreeKind::Scrub => [117, 122, 59],
                        }
                    };
                    let coverage = (radius * 2.0 / TEST_GRID.texel_meters).powi(2);
                    let expected = color.map(|channel| (channel as f32 * coverage).round() as u8);
                    let offset = ((z * TEST_GRID.side + x) * 4) as usize;
                    assert_eq!(
                        &data[offset..offset + 4],
                        &[expected[0], expected[1], expected[2], 255],
                        "{generation:?} {:?} at {:?} has its exact crown-colored dot",
                        tree.kind,
                        tree.base
                    );
                    found[kind_index] = true;
                    if found.iter().all(|found| *found) {
                        break 'candidates;
                    }
                }
            }
            assert!(
                found.iter().all(|found| *found),
                "all requested tree kinds found"
            );

            let spawn = geo.spawn();
            let [spawn_x, _] = TEST_GRID.bounds(spawn[0], spawn[0]);
            let [spawn_z, _] = TEST_GRID.bounds(spawn[2], spawn[2]);
            let empty = (spawn_z.saturating_sub(4)..=spawn_z + 4).find_map(|z| {
                (spawn_x.saturating_sub(4)..=spawn_x + 4).find_map(|x| {
                    let point = TEST_GRID.center(x, z);
                    let gx = (point[0] / 12.0).floor() as i32;
                    let gz = (point[1] / 12.0).floor() as i32;
                    (world.tree_at(gx, gz).is_none()
                        && trees_overlapping_pixel(&world, TEST_GRID, x, z) == 0)
                        .then_some((x, z))
                })
            });
            let (x, z) = empty.expect("spawn clearing has a genuinely empty candidate pixel");
            let offset = ((z * TEST_GRID.side + x) * 4) as usize;
            assert_eq!(&data[offset..offset + 4], &[0, 0, 0, 255]);
        }
    }

    #[test]
    fn atlas_keeps_real_water_land_and_has_a_bounded_filtered_mip_chain() {
        let world = World::generate(42, WorldGeneration::GeographyV2);
        let started = std::time::Instant::now();
        let image = distant_albedo(&world, ATLAS_SIDE);
        eprintln!(
            "2048x2048 distant map with mips generated in {:?}",
            started.elapsed()
        );
        assert_eq!(image.texture_descriptor.size.width, ATLAS_SIDE);
        assert_eq!(image.texture_descriptor.size.height, ATLAS_SIDE);
        assert_eq!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
        assert_eq!(image.texture_descriptor.mip_level_count, 12);
        let bytes = image.data.as_ref().unwrap();
        assert_eq!(bytes.len(), 22_369_620);
        assert!(bytes.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255));
        let geo = world.geography().unwrap();
        let palette = biome_palette();
        let channels = river_mask(geo, TEST_GRID);
        let mut saw_river = false;
        let mut saw_lake = false;
        let mut saw_snow = false;
        for z in (0..ATLAS_SIDE).step_by(5) {
            for x in (0..ATLAS_SIDE).step_by(5) {
                let [mx, mz] = TEST_GRID.center(x, z);
                let sample = geo.sample(mx, mz);
                let offset = ((z * ATLAS_SIDE + x) * 4) as usize;
                let pixel = &bytes[offset..offset + 4];
                if let Some(level) = sample.water {
                    assert_eq!(pixel, map_color(sample, 0.1, &palette));
                    assert!(pixel[2] > pixel[1] && pixel[1] > pixel[0]);
                    if level > 1.0 {
                        saw_river |= level - sample.height < 3.0;
                        saw_lake |= level - sample.height > 8.0;
                    }
                } else if sample.biome == Biome::Snow {
                    saw_snow = true;
                    if channels[(z * ATLAS_SIDE + x) as usize] == 0 {
                        assert!(pixel[0] > 180 && pixel[1] > 190 && pixel[2] > 190);
                    }
                }
            }
        }
        assert!(
            saw_river && saw_lake && saw_snow,
            "seed-42 contains all three features"
        );
    }

    #[test]
    fn a_narrow_real_channel_stays_continuous_without_flooding_its_dry_bank() {
        let world = World::generate(42, WorldGeneration::GeographyV2);
        let geo = world.geography().unwrap();
        let mask = river_mask(geo, TEST_GRID);
        let pixel_at = |point: Vec2| {
            let [x, _] = TEST_GRID.bounds(point.x, point.x);
            let [z, _] = TEST_GRID.bounds(point.y, point.y);
            (z * ATLAS_SIDE + x) as usize
        };
        let (a, b, bank) = river_segments(geo)
            .into_iter()
            .find_map(|(a, b)| {
                let mid = (a + b) * 0.5;
                let normal = (b - a).normalize().perp();
                let bank = mid + normal * 64.0;
                let narrow = [-12.0, 12.0].into_iter().all(|distance| {
                    let edge = mid + normal * distance;
                    geo.sample(edge.x, edge.y).water.is_none()
                });
                let misses_texel = (1..10).any(|step| {
                    let index = pixel_at(a.lerp(b, step as f32 / 10.0));
                    let [x, z] =
                        TEST_GRID.center(index as u32 % ATLAS_SIDE, index as u32 / ATLAS_SIDE);
                    geo.sample(x, z).water.is_none()
                });
                (narrow
                    && misses_texel
                    && geo.sample(bank.x, bank.y).water.is_none()
                    && mask[pixel_at(bank)] == 0)
                    .then_some((a, b, bank))
            })
            .expect("seed-42 has a narrow channel missed by the 16 m texel centers");
        for step in 0..=20 {
            assert!(mask[pixel_at(a.lerp(b, step as f32 / 20.0))] > 40);
        }
        let mut data = vec![80; (ATLAS_SIDE * ATLAS_SIDE * 4) as usize];
        paint_rivers(geo, TEST_GRID, &mut data);
        let bank_offset = pixel_at(bank) * 4;
        assert_eq!(&data[bank_offset..bank_offset + 4], &[80; 4]);
        assert!(
            data.as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[2] > pixel[0])
        );
    }

    #[test]
    fn actual_trails_lanes_fields_and_roofs_are_painted_without_a_village_marker() {
        let world = World::generate(42, WorldGeneration::GeographyV3);
        let plan = world.settlements().unwrap();
        let mask = route_mask(
            TEST_GRID,
            plan.trails
                .iter()
                .chain(plan.villages.iter().flat_map(|v| &v.lanes)),
        );
        for trail in &plan.trails {
            for point in trail.points.iter().step_by(31) {
                let [x, _] = TEST_GRID.bounds(point[0], point[0]);
                let [z, _] = TEST_GRID.bounds(point[2], point[2]);
                assert!(
                    mask[(z * ATLAS_SIDE + x) as usize] > 40,
                    "actual trail center remains visible"
                );
            }
        }
        assert!(
            mask.iter().filter(|coverage| **coverage != 0).count()
                < (ATLAS_SIDE * ATLAS_SIDE / 100) as usize
        );
        let mut data = vec![80; (ATLAS_SIDE * ATLAS_SIDE * 4) as usize];
        paint_settlements(&world, TEST_GRID, &mut data);
        for village in &plan.villages {
            for building in &village.buildings {
                let [width, _, depth] = building.dimensions();
                let mx = (building.origin.x as f32 + width as f32 * 0.5) * CELL_SIZE;
                let mz = (building.origin.z as f32 + depth as f32 * 0.5) * CELL_SIZE;
                let [x, _] = TEST_GRID.bounds(mx, mx);
                let [z, _] = TEST_GRID.bounds(mz, mz);
                let offset = ((z * ATLAS_SIDE + x) * 4) as usize;
                assert!(
                    data[offset] > data[offset + 2],
                    "real roofs use warm map colors"
                );
            }
        }
    }

    #[test]
    fn mip_filter_averages_in_linear_light_and_valleys_remain_a_white_texel() {
        let mut pixels = vec![
            0, 0, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255,
        ];
        assert_eq!(append_mips(&mut pixels, 2), 2);
        assert!((pixels[16] as i32 - 188).abs() <= 1);
        assert_eq!(&pixels[16..20], &[pixels[16], pixels[16], pixels[16], 255]);
        let image = distant_albedo(&World::new(7), ATLAS_SIDE);
        assert_eq!(image.texture_descriptor.size.width, 1);
        assert_eq!(image.texture_descriptor.size.height, 1);
        assert_eq!(image.texture_descriptor.mip_level_count, 1);
        assert_eq!(image.data.unwrap(), [255; 4]);
    }
}
