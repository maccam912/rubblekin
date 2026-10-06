//! The map shares the distant terrain atlas. Legacy valleys need a small
//! overhead image because their terrain has no geography atlas.
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rubblekin_core::world::{BlockPos, CELL_SIZE, WATER_LEVEL, World};

/// Position in the map image: +X is right and +Z is down, matching the atlas.
/// The world bounds are in cells while player and village positions use meters.
pub(crate) fn map_uv(world: &World, position: [f32; 3]) -> Vec2 {
    let radius = world.radius_cells() as f32 * CELL_SIZE;
    let coordinate = |meters: f32| {
        if meters.is_finite() {
            ((meters + radius) / (radius * 2.0)).clamp(0.0, 1.0)
        } else {
            0.5
        }
    };
    Vec2::new(coordinate(position[0]), coordinate(position[2]))
}

/// One pixel per legacy voxel column, including current edits and tree crowns.
/// This is only needed when the shared distant geography atlas is unavailable.
pub(crate) fn valley_image(world: &World) -> Image {
    let radius = world.radius_cells();
    let side = (radius * 2) as u32;
    let mut data = Vec::with_capacity((side * side * 4) as usize);
    for z in -radius..radius {
        // The valley's rendered stream is a ribbon between these centers.
        let river_x = (world.river_center(z) + world.river_center(z + 1)) * 0.5 * CELL_SIZE;
        for x in -radius..radius {
            let meters_x = (x as f32 + 0.5) * CELL_SIZE;
            let meters_z = (z as f32 + 0.5) * CELL_SIZE;
            let surface = world.surface_height(meters_x, meters_z);
            let top = (surface / CELL_SIZE) as i32 - 1;
            let color = if (meters_x - river_x).abs() <= 2.1 && surface < WATER_LEVEL {
                [0.23, 0.52, 0.59, 1.0]
            } else {
                let block = world.block(BlockPos::new(x, top, z));
                let [red, green, blue, _] = block.color();
                let height = |dx: i32, dz: i32| {
                    world.height_at(
                        (x + dx).clamp(-radius, radius - 1),
                        (z + dz).clamp(-radius, radius - 1),
                    ) as f32
                };
                let dx = (height(1, 0) - height(-1, 0)) * 0.5;
                let dz = (height(0, 1) - height(0, -1)) * 0.5;
                let relief = ((1.0 + dx * 0.14 + dz * 0.09)
                    / (1.0 + dx * dx * 0.04 + dz * dz * 0.04).sqrt())
                .clamp(0.84, 1.09);
                [red * relief, green * relief, blue * relief, 1.0]
            };
            data.extend(color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
    }
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
    image.sampler = ImageSampler::linear();
    image
}

#[cfg(test)]
mod tests {
    use super::*;
    use rubblekin_core::{
        geography::WORLD_SIZE,
        world::{Block, WorldGeneration},
    };

    fn verify_coordinates(world: &World, radius_meters: f32) {
        assert_eq!(map_uv(world, [0.0; 3]), Vec2::splat(0.5));
        assert_eq!(
            map_uv(world, [-radius_meters, 100.0, -radius_meters]),
            Vec2::ZERO
        );
        assert_eq!(
            map_uv(world, [radius_meters, -100.0, radius_meters]),
            Vec2::ONE
        );
        assert_eq!(
            map_uv(world, [radius_meters * 0.5, 0.0, -radius_meters * 0.5]),
            Vec2::new(0.75, 0.25)
        );
        assert_eq!(
            map_uv(world, [-radius_meters * 2.0, 0.0, radius_meters * 2.0]),
            Vec2::new(0.0, 1.0)
        );
    }

    #[test]
    fn legacy_map_coordinates_use_meter_bounds_and_atlas_orientation() {
        let world = World::new(42);
        verify_coordinates(&world, 80.0);
        assert_eq!(
            map_uv(&world, [f32::NAN, 0.0, f32::INFINITY]),
            Vec2::splat(0.5)
        );
    }

    #[test]
    fn geographic_map_coordinates_cover_the_actual_atlas_extent() {
        let world = World::generate(42, WorldGeneration::GeographyV1);
        verify_coordinates(&world, WORLD_SIZE * 0.5);
        assert_eq!(
            map_uv(&world, [8192.0, 0.0, -8192.0]),
            Vec2::new(0.75, 0.25)
        );
    }

    #[test]
    fn valley_image_keeps_edited_columns_in_the_correct_corner_and_draws_water() {
        let mut world = World::new(42);
        let radius = world.radius_cells();
        let northwest = BlockPos::new(-radius + 1, world.max_y() - 1, -radius + 3);
        let southeast = BlockPos::new(radius - 2, world.max_y() - 1, radius - 4);
        world.set_block(northwest, Block::Brick).unwrap();
        world.set_block(southeast, Block::Glass).unwrap();
        let image = valley_image(&world);
        assert_eq!(image.width(), 320);
        assert_eq!(image.height(), 320);
        let data = image.data.as_ref().unwrap();
        let pixel = |x: i32, z: i32| {
            let index = (((z + radius) * radius * 2 + x + radius) * 4) as usize;
            &data[index..index + 4]
        };
        let warm = pixel(northwest.x, northwest.z);
        let cold = pixel(southeast.x, southeast.z);
        assert!(warm[0] > warm[2], "northwest brick is warm red");
        assert!(cold[2] > cold[0], "southeast glass is cool blue");
        let river_x = world.river_center(0).floor() as i32;
        let water = pixel(river_x, 0);
        assert!(water[2] > water[1] && water[1] > water[0]);
        assert!(data.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255));
    }
}
