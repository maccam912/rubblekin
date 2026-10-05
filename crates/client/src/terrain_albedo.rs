//! A bounded, once-per-world land-color atlas retains real biome and canopy
//! patches after individual distant trees become smaller than a screen pixel.
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rubblekin_core::{
    geography::{Biome, WORLD_SIZE},
    world::{BlockPos, CELL_SIZE, GeneratedTree, TreeKind, World, WorldGeneration},
};

const ATLAS_SIDE: u32 = 512;
const TEXEL_METERS: f32 = WORLD_SIZE / ATLAS_SIDE as f32;
const TREE_GRID_METERS: f32 = 12.0;

pub fn distant_albedo(world: &World) -> Image {
    let (side, data) = if world.geography().is_some() {
        let coverage = crown_coverage();
        let mut data = Vec::with_capacity((ATLAS_SIDE * ATLAS_SIDE * 4) as usize);
        for z in 0..ATLAS_SIDE {
            for x in 0..ATLAS_SIDE {
                data.extend_from_slice(&texel_color(world, x, z, coverage));
            }
        }
        (ATLAS_SIDE, data)
    } else {
        (1, vec![255; 4])
    };
    let mut image = Image::new(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    // The geographic patches are 64 m wide; linear filtering avoids a false
    // checkerboard of 64 m squares. Fine voxel grain belongs to the shader.
    image.sampler = ImageSampler::linear();
    image
}

/// Count the actual generated crown's projected voxel footprint, rather than
/// assigning a forest-colored blanket from its biome. This is at most 30% of
/// a twelve-meter tree square, and is averaged over four real candidates.
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

fn candidate_cells(mx: f32, mz: f32) -> [(i32, i32); 4] {
    let quarter = TEXEL_METERS * 0.25;
    [
        (-quarter, -quarter),
        (quarter, -quarter),
        (-quarter, quarter),
        (quarter, quarter),
    ]
    .map(|(dx, dz)| {
        (
            ((mx + dx) / TREE_GRID_METERS).floor() as i32,
            ((mz + dz) / TREE_GRID_METERS).floor() as i32,
        )
    })
}

fn texel_color(world: &World, x: u32, z: u32, coverage: [f32; 3]) -> [u8; 4] {
    let [mx, mz] = texel_center(x, z);
    let sample = world.geography().unwrap().sample(mx, mz);
    let base = sample.biome.color();
    if sample.water.is_some()
        || !matches!(
            sample.biome,
            Biome::Grassland
                | Biome::Forest
                | Biome::Rainforest
                | Biome::PineForest
                | Biome::Shrubland
                | Biome::Tundra
        )
    {
        return rgba_bytes(base);
    }
    let mut canopy = Vec3::ZERO;
    let mut fraction = 0.0;
    for (gx, gz) in candidate_cells(mx, mz) {
        // tree_at already rejects wet, steep, and above-treeline candidates.
        let Some(tree) = world.tree_at(gx, gz) else {
            continue;
        };
        let index = match tree.kind {
            TreeKind::Broadleaf => 0,
            TreeKind::Conifer => 1,
            TreeKind::Scrub => 2,
        };
        let weight = coverage[index] * 0.25;
        let leaf = if matches!(
            world.generation(),
            WorldGeneration::GeographyV2 | WorldGeneration::GeographyV3
        ) {
            tree.kind.leaf_color()
        } else {
            [0.24, 0.40, 0.31, 1.0]
        };
        canopy += linear_rgb(leaf) * weight;
        fraction += weight;
    }
    if fraction == 0.0 {
        return rgba_bytes(base);
    }
    let color = linear_rgb(base) * (1.0 - fraction) + canopy;
    let srgb = Srgba::from(LinearRgba::new(color.x, color.y, color.z, 1.0));
    rgba_bytes([srgb.red, srgb.green, srgb.blue, 1.0])
}

fn linear_rgb(color: [f32; 4]) -> Vec3 {
    let linear = LinearRgba::from(Srgba::new(color[0], color[1], color[2], color[3]));
    Vec3::new(linear.red, linear.green, linear.blue)
}

fn rgba_bytes(color: [f32; 4]) -> [u8; 4] {
    color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distant_land_atlas_is_bounded_and_keeps_real_forest_snow_and_water_colors() {
        let world = World::generate(42, WorldGeneration::GeographyV2);
        let started = std::time::Instant::now();
        let image = distant_albedo(&world);
        eprintln!(
            "512x512 distant land atlas generated in {:?}",
            started.elapsed()
        );
        assert_eq!(image.texture_descriptor.size.width, ATLAS_SIDE);
        assert_eq!(image.texture_descriptor.size.height, ATLAS_SIDE);
        assert_eq!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
        let bytes = image.data.as_ref().unwrap();
        assert_eq!(bytes.len(), 1024 * 1024);
        let geo = world.geography().unwrap();
        let coverage = crown_coverage();
        assert!(
            coverage
                .iter()
                .all(|fraction| *fraction > 0.0 && *fraction < 0.30)
        );
        let pixel = |x: u32, z: u32| -> [u8; 4] {
            let start = ((z * ATLAS_SIDE + x) * 4) as usize;
            bytes[start..start + 4].try_into().unwrap()
        };

        // A fixed seed-42 forest near the native long-distance audit position.
        let forest = (90, 206);
        let [x, z] = texel_center(forest.0, forest.1);
        let sample = geo.sample(x, z);
        eprintln!("forest texel {forest:?} at {x},{z}: {:?}", sample.biome);
        assert!(matches!(
            sample.biome,
            Biome::Forest | Biome::Rainforest | Biome::PineForest
        ));
        assert!(
            candidate_cells(x, z)
                .iter()
                .any(|&(gx, gz)| world.tree_at(gx, gz).is_some())
        );
        assert_ne!(pixel(forest.0, forest.1), rgba_bytes(sample.biome.color()));
        assert_eq!(
            pixel(forest.0, forest.1),
            texel_color(&world, forest.0, forest.1, coverage)
        );
        assert_eq!(
            image.data,
            distant_albedo(&world.clone()).data,
            "all texels are deterministic when the same world is rendered again"
        );

        let peak = geo
            .heights()
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .unwrap()
            .0;
        let [x, z] = geo.grid_position(peak);
        let snow_x = ((x + WORLD_SIZE * 0.5) / TEXEL_METERS).floor() as u32;
        let snow_z = ((z + WORLD_SIZE * 0.5) / TEXEL_METERS).floor() as u32;
        let [x, z] = texel_center(snow_x, snow_z);
        assert_eq!(geo.sample(x, z).biome, Biome::Snow);
        assert_eq!(pixel(snow_x, snow_z), rgba_bytes(Biome::Snow.color()));

        let [x, z] = texel_center(0, 0);
        let ocean = geo.sample(x, z);
        assert_eq!(ocean.biome, Biome::Ocean);
        assert!(ocean.water.is_some());
        assert_eq!(pixel(0, 0), rgba_bytes(ocean.biome.color()));

        let treeless = (0..ATLAS_SIDE)
            .step_by(8)
            .find_map(|z| {
                (0..ATLAS_SIDE).step_by(8).find_map(|x| {
                    let [mx, mz] = texel_center(x, z);
                    let sample = geo.sample(mx, mz);
                    (sample.biome == Biome::Grassland
                        && sample.water.is_none()
                        && candidate_cells(mx, mz)
                            .iter()
                            .all(|&(gx, gz)| world.tree_at(gx, gz).is_none()))
                    .then_some((x, z))
                })
            })
            .expect("seed-42 has treeless meadow texels");
        let [x, z] = texel_center(treeless.0, treeless.1);
        assert_eq!(
            pixel(treeless.0, treeless.1),
            rgba_bytes(geo.sample(x, z).biome.color())
        );
    }

    #[test]
    fn valley_albedo_is_a_single_white_texel() {
        let image = distant_albedo(&World::new(7));
        assert_eq!(image.texture_descriptor.size.width, 1);
        assert_eq!(image.texture_descriptor.size.height, 1);
        assert_eq!(image.data.unwrap(), [255; 4]);
    }
}
