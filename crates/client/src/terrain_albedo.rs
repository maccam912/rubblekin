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
    world::{BlockPos, CELL_SIZE, GeneratedTree, TreeKind, World, WorldGeneration},
};

const ATLAS_SIDE: u32 = 2048;
const TEXEL_METERS: f32 = WORLD_SIZE / ATLAS_SIDE as f32;
const CANOPY_SIDE: u32 = 512;
const TREE_GRID_METERS: f32 = 12.0;
const ROAD_COLOR: [u8; 4] = [181, 153, 101, 255];
const FIELD_COLOR: [u8; 4] = [167, 164, 82, 255];
const ROOF_COLOR: [u8; 4] = [148, 101, 75, 255];

pub fn distant_albedo(world: &World) -> Image {
    let (side, mut data) = if let Some(geo) = world.geography() {
        // Four real tree candidates per 64 m cache cell retain canopy patches
        // without repeating expensive tree/slope/settlement queries 16x.
        let canopy = canopy_samples(world);
        let palette = biome_palette();
        let mut data = Vec::with_capacity((ATLAS_SIDE * ATLAS_SIDE * 4) as usize);
        for z in 0..ATLAS_SIDE {
            for x in 0..ATLAS_SIDE {
                let [mx, mz] = texel_center(x, z);
                let sample = geo.sample(mx, mz);
                data.extend_from_slice(&map_color(
                    sample,
                    map_relief(geo, mx, mz),
                    canopy_at(&canopy, x, z),
                    &palette,
                ));
            }
        }
        paint_rivers(geo, &mut data);
        paint_settlements(world, &mut data);
        (ATLAS_SIDE, data)
    } else {
        (1, vec![255; 4])
    };
    let levels = append_mips(&mut data, side);
    // Image::new validates only a base level, so initialize explicitly with the
    // complete level-ordered mip chain. This adds one third to the 16 MiB base.
    let mut image = Image::new_uninit(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::linear();
    image
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

/// Project actual generated crowns rather than assigning every forest texel a
/// blanket of leaves. A separate coarse cache smooths their aggregate coverage.
fn crown_coverage() -> [f32; 3] {
    [TreeKind::Broadleaf, TreeKind::Conifer, TreeKind::Scrub].map(|kind| {
        let tree = GeneratedTree {
            base: BlockPos::new(0, 0, 0),
            trunk_height: 20,
            kind,
            crown_radius: if kind == TreeKind::Scrub { 2 } else { 4 },
        };
        let mut cells = 0;
        for x in -tree.crown_radius..=tree.crown_radius {
            for z in -tree.crown_radius..=tree.crown_radius {
                cells += usize::from(tree.leaf_bounds(x, z).is_some());
            }
        }
        (cells as f32 * CELL_SIZE * CELL_SIZE / TREE_GRID_METERS.powi(2)).min(0.30)
    })
}

fn texel_center(x: u32, z: u32) -> [f32; 2] {
    [
        (x as f32 + 0.5) * TEXEL_METERS - WORLD_SIZE * 0.5,
        (z as f32 + 0.5) * TEXEL_METERS - WORLD_SIZE * 0.5,
    ]
}

fn canopy_samples(world: &World) -> Vec<Vec4> {
    let coverage = crown_coverage();
    let refined = matches!(
        world.generation(),
        WorldGeneration::GeographyV2 | WorldGeneration::GeographyV3
    );
    let colors = [TreeKind::Broadleaf, TreeKind::Conifer, TreeKind::Scrub].map(|kind| {
        linear_rgb(if refined {
            kind.leaf_color()
        } else {
            [0.24, 0.40, 0.31, 1.0]
        })
    });
    let spacing = WORLD_SIZE / CANOPY_SIDE as f32;
    let mut samples = Vec::with_capacity((CANOPY_SIDE * CANOPY_SIDE) as usize);
    for z in 0..CANOPY_SIDE {
        for x in 0..CANOPY_SIDE {
            let mx = (x as f32 + 0.5) * spacing - WORLD_SIZE * 0.5;
            let mz = (z as f32 + 0.5) * spacing - WORLD_SIZE * 0.5;
            let mut sample = Vec4::ZERO;
            for (dx, dz) in [(-0.25, -0.25), (0.25, -0.25), (-0.25, 0.25), (0.25, 0.25)] {
                let gx = ((mx + dx * spacing) / TREE_GRID_METERS).floor() as i32;
                let gz = ((mz + dz * spacing) / TREE_GRID_METERS).floor() as i32;
                let Some(tree) = world.tree_at(gx, gz) else {
                    continue;
                };
                let index = match tree.kind {
                    TreeKind::Broadleaf => 0,
                    TreeKind::Conifer => 1,
                    TreeKind::Scrub => 2,
                };
                let weight = coverage[index] * 0.25;
                sample += colors[index].extend(1.0) * weight;
            }
            samples.push(sample);
        }
    }
    samples
}

fn canopy_at(cache: &[Vec4], x: u32, z: u32) -> Vec4 {
    let scale = CANOPY_SIDE as f32 / ATLAS_SIDE as f32;
    let cx = ((x as f32 + 0.5) * scale - 0.5).max(0.0);
    let cz = ((z as f32 + 0.5) * scale - 0.5).max(0.0);
    let ix = cx.floor() as u32;
    let iz = cz.floor() as u32;
    let next_x = (ix + 1).min(CANOPY_SIDE - 1);
    let next_z = (iz + 1).min(CANOPY_SIDE - 1);
    let sample = |x, z| cache[(z * CANOPY_SIDE + x) as usize];
    sample(ix, iz).lerp(sample(next_x, iz), cx.fract()).lerp(
        sample(ix, next_z).lerp(sample(next_x, next_z), cx.fract()),
        cz.fract(),
    )
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

fn map_color(sample: GeoSample, relief: f32, canopy: Vec4, palette: &[Vec3; 11]) -> [u8; 4] {
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
    let base = palette[biome_index(sample.biome)];
    let canopy = if matches!(
        sample.biome,
        Biome::Grassland | Biome::Forest | Biome::Rainforest | Biome::PineForest | Biome::Shrubland
    ) {
        canopy
    } else {
        Vec4::ZERO
    };
    let color = base * (1.0 - canopy.w) + canopy.truncate();
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

fn river_mask(geo: &Geography) -> Vec<u8> {
    let mut mask = vec![0; (ATLAS_SIDE * ATLAS_SIDE) as usize];
    for (a, b) in river_segments(geo) {
        // At 16 m per texel, narrow tributaries miss texel centers. A
        // restrained one-texel stroke keeps the actual channel continuous.
        stamp_segment(&mut mask, a, b, TEXEL_METERS * 0.55);
    }
    mask
}

fn paint_rivers(geo: &Geography, data: &mut [u8]) {
    let mask = river_mask(geo);
    let shallow_water = rgba_bytes([0.198, 0.487, 0.617, 1.0]);
    for (index, coverage) in mask.into_iter().enumerate() {
        if coverage == 0 {
            continue;
        }
        let [x, z] = texel_center(index as u32 % ATLAS_SIDE, index as u32 / ATLAS_SIDE);
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

fn paint_settlements(world: &World, data: &mut [u8]) {
    let Some(plan) = world.settlements() else {
        return;
    };
    for village in &plan.villages {
        for field in &village.fields {
            paint_rect(
                data,
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
        plan.trails
            .iter()
            .chain(plan.villages.iter().flat_map(|v| &v.lanes)),
    );
    for (pixel, coverage) in data.as_chunks_mut::<4>().0.iter_mut().zip(mask) {
        blend_pixel(pixel, ROAD_COLOR, coverage as f32 / 255.0);
    }
    for village in &plan.villages {
        for building in &village.buildings {
            let [width, _, depth] = building.dimensions();
            paint_rect(
                data,
                [
                    building.origin.x as f32 * CELL_SIZE,
                    building.origin.z as f32 * CELL_SIZE,
                ],
                [width as f32 * CELL_SIZE, depth as f32 * CELL_SIZE],
                ROOF_COLOR,
            );
        }
    }
}

/// A small antialiased footprint, never a village icon. Distant fields/roofs
/// retain their actual centers and orientation, with a 0.6-texel minimum size.
fn paint_rect(data: &mut [u8], origin: [f32; 2], size: [f32; 2], color: [u8; 4]) {
    let center = [origin[0] + size[0] * 0.5, origin[1] + size[1] * 0.5];
    let half = [
        size[0].max(TEXEL_METERS * 0.6) * 0.5,
        size[1].max(TEXEL_METERS * 0.6) * 0.5,
    ];
    let min = [center[0] - half[0], center[1] - half[1]];
    let max = [center[0] + half[0], center[1] + half[1]];
    let [x0, x1] = pixel_bounds(min[0], max[0]);
    let [z0, z1] = pixel_bounds(min[1], max[1]);
    for z in z0..=z1 {
        for x in x0..=x1 {
            let [mx, mz] = texel_center(x, z);
            let overlap_x = (max[0].min(mx + TEXEL_METERS * 0.5)
                - min[0].max(mx - TEXEL_METERS * 0.5))
            .max(0.0);
            let overlap_z = (max[1].min(mz + TEXEL_METERS * 0.5)
                - min[1].max(mz - TEXEL_METERS * 0.5))
            .max(0.0);
            let coverage = overlap_x * overlap_z / TEXEL_METERS.powi(2);
            let offset = ((z * ATLAS_SIDE + x) * 4) as usize;
            blend_pixel(&mut data[offset..offset + 4], color, coverage);
        }
    }
}

fn route_mask<'a>(trails: impl Iterator<Item = &'a Trail>) -> Vec<u8> {
    let mut mask = vec![0; (ATLAS_SIDE * ATLAS_SIDE) as usize];
    for trail in trails {
        // One texel across keeps real 3 m trails legible at distance. The
        // course follows every route bend; a coverage mask avoids dark joins.
        let radius = (trail.width * 0.5).max(TEXEL_METERS * 0.45);
        for pair in trail.points.windows(2) {
            let a = Vec2::new(pair[0][0], pair[0][2]);
            let b = Vec2::new(pair[1][0], pair[1][2]);
            stamp_segment(&mut mask, a, b, radius);
        }
    }
    mask
}

fn stamp_segment(mask: &mut [u8], a: Vec2, b: Vec2, radius: f32) {
    let padding = radius + TEXEL_METERS * 0.5;
    let [x0, x1] = pixel_bounds(a.x.min(b.x) - padding, a.x.max(b.x) + padding);
    let [z0, z1] = pixel_bounds(a.y.min(b.y) - padding, a.y.max(b.y) + padding);
    let delta = b - a;
    let length_squared = delta.length_squared();
    for z in z0..=z1 {
        for x in x0..=x1 {
            let p = Vec2::from_array(texel_center(x, z));
            let t = if length_squared > 0.0 {
                ((p - a).dot(delta) / length_squared).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let distance = p.distance(a + delta * t);
            let coverage =
                ((radius + TEXEL_METERS * 0.5 - distance) / TEXEL_METERS).clamp(0.0, 1.0);
            let index = (z * ATLAS_SIDE + x) as usize;
            mask[index] = mask[index].max((coverage * 255.0).round() as u8);
        }
    }
}

fn pixel_bounds(min: f32, max: f32) -> [u32; 2] {
    [min, max].map(|value| {
        ((value + WORLD_SIZE * 0.5) / TEXEL_METERS)
            .floor()
            .clamp(0.0, (ATLAS_SIDE - 1) as f32) as u32
    })
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

/// Box-filter in linear light: directly averaging sRGB bytes darkens small
/// bright roads, coastlines, and snow. Store levels largest to smallest.
fn append_mips(data: &mut Vec<u8>, mut side: u32) -> u32 {
    let lookup: [f32; 256] = std::array::from_fn(|i| linear_rgb([i as f32 / 255.0; 4]).x);
    let mut offset = 0;
    let mut levels = 1;
    while side > 1 {
        let next_side = side / 2;
        let next_offset = data.len();
        data.resize(next_offset + (next_side * next_side * 4) as usize, 255);
        for z in 0..next_side {
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
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_keeps_real_water_land_and_has_a_bounded_filtered_mip_chain() {
        let world = World::generate(42, WorldGeneration::GeographyV2);
        let started = std::time::Instant::now();
        let image = distant_albedo(&world);
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
        let channels = river_mask(geo);
        let mut saw_river = false;
        let mut saw_lake = false;
        let mut saw_snow = false;
        for z in (0..ATLAS_SIDE).step_by(5) {
            for x in (0..ATLAS_SIDE).step_by(5) {
                let [mx, mz] = texel_center(x, z);
                let sample = geo.sample(mx, mz);
                let offset = ((z * ATLAS_SIDE + x) * 4) as usize;
                let pixel = &bytes[offset..offset + 4];
                if let Some(level) = sample.water {
                    assert_eq!(pixel, map_color(sample, 0.1, Vec4::ONE, &palette));
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
        let mask = river_mask(geo);
        let pixel_at = |point: Vec2| {
            let [x, _] = pixel_bounds(point.x, point.x);
            let [z, _] = pixel_bounds(point.y, point.y);
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
                    let [x, z] = texel_center(index as u32 % ATLAS_SIDE, index as u32 / ATLAS_SIDE);
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
        paint_rivers(geo, &mut data);
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
            plan.trails
                .iter()
                .chain(plan.villages.iter().flat_map(|v| &v.lanes)),
        );
        for trail in &plan.trails {
            for point in trail.points.iter().step_by(31) {
                let [x, _] = pixel_bounds(point[0], point[0]);
                let [z, _] = pixel_bounds(point[2], point[2]);
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
        paint_settlements(&world, &mut data);
        for village in &plan.villages {
            for building in &village.buildings {
                let [width, _, depth] = building.dimensions();
                let mx = (building.origin.x as f32 + width as f32 * 0.5) * CELL_SIZE;
                let mz = (building.origin.z as f32 + depth as f32 * 0.5) * CELL_SIZE;
                let [x, _] = pixel_bounds(mx, mx);
                let [z, _] = pixel_bounds(mz, mz);
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
        let image = distant_albedo(&World::new(7));
        assert_eq!(image.texture_descriptor.size.width, 1);
        assert_eq!(image.texture_descriptor.size.height, 1);
        assert_eq!(image.texture_descriptor.mip_level_count, 1);
        assert_eq!(image.data.unwrap(), [255; 4]);
    }
}
