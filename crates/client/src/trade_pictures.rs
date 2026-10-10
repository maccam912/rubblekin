//! The same resource pictures connect earned cargo to the existing market.
use bevy::prelude::*;
use bevy::{
    asset::RenderAssetUsages,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rubblekin_core::{economy::resource_index, settlement::ResourceKind, world::Block};

#[derive(Resource)]
pub(crate) struct TradePictures {
    resources: [Handle<Image>; 5],
    pub(crate) coins: Handle<Image>,
    pub(crate) arrow: Handle<Image>,
}

impl TradePictures {
    pub(crate) fn resource(&self, kind: ResourceKind) -> Handle<Image> {
        self.resources[resource_index(kind)].clone()
    }
}

impl FromWorld for TradePictures {
    fn from_world(world: &mut World) -> Self {
        let blocks = world.resource::<crate::block_textures::BlockIcons>();
        let icon = |block: Block| blocks.0[block.catalog_index().unwrap()].clone();
        let resources = [
            Handle::default(),
            icon(Block::Wood),
            icon(Block::Stone),
            icon(Block::Clay),
            icon(Block::IronOre),
        ];
        let mut images = world.resource_mut::<Assets<Image>>();
        let mut resources = resources;
        let food = [
            (crate::crops::CropKind::Grain, Vec3::new(-0.25, 0.10, 0.)),
            (crate::crops::CropKind::Leafy, Vec3::new(0.23, 0.30, 0.)),
            (crate::crops::CropKind::Roots, Vec3::new(0.16, 0.12, 0.)),
        ]
        .into_iter()
        .flat_map(|(kind, position)| crate::crops::plant_parts(position, 6, kind))
        .collect::<Vec<_>>();
        resources[0] = images.add(crate::activities::picture(&food));
        Self {
            resources,
            coins: images.add(coin_picture()),
            arrow: images.add(arrow_picture()),
        }
    }
}

fn arrow_picture() -> Image {
    let mut pixels = vec![0; 32 * 32 * 4];
    for y in 0_i32..32 {
        for x in 0_i32..32 {
            if ((3..=20).contains(&x) && (15..=17).contains(&y))
                || ((18..=29).contains(&x) && (y - 16).abs() * 3 <= (29 - x) * 2)
            {
                let at = (y as usize * 32 + x as usize) * 4;
                pixels[at..at + 4].copy_from_slice(&[242, 211, 143, 255]);
            }
        }
    }
    Image::new(
        Extent3d {
            width: 32,
            height: 32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn coin_picture() -> Image {
    // A stack of gold discs, with a light rim and shaded lower edge.
    let mut parts = Vec::new();
    for (x, y) in [(-0.19, 0.30), (-0.19, 0.43), (0.18, 0.56)] {
        for row in -5_i32..=5 {
            let width = if row.abs() > 3 { 0.22 } else { 0.40 };
            parts.push((
                Vec3::new(x, y + row as f32 * 0.021, 0.),
                Vec3::new(width, 0.024, 0.05),
                if row < -2 {
                    [0.65, 0.43, 0.12, 1.]
                } else if row > 2 {
                    [0.98, 0.86, 0.45, 1.]
                } else {
                    [0.91, 0.69, 0.23, 1.]
                },
            ));
        }
    }
    crate::activities::picture(&parts)
}
