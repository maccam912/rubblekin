//! Tiny deterministic pixel textures shared by terrain faces and inventory icons.
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rubblekin_core::{blocks::BlockTexture, world::Block};
const PIXELS: u32 = 16;
const TILE: u32 = 20; // Two repeated edge pixels prevent bilinear atlas bleeding.
const COLUMNS: u32 = 16;
fn dimensions() -> (u32, u32) {
    (
        COLUMNS * TILE,
        (Block::ALL.len() as u32 * 3).div_ceil(COLUMNS) * TILE,
    )
}
fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut n =
        x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    (n & 255) as f32 / 255.
}
fn shade(block: Block, face: u32, x: u32, y: u32) -> f32 {
    use BlockTexture::*;
    let grain = noise(x, y, block.catalog_index().unwrap_or(0) as u32);
    let mortar = |w, h| {
        y.is_multiple_of(h)
            || (x + if (y / h).is_multiple_of(2) { 0 } else { w / 2 }).is_multiple_of(w)
    };
    let value = match block.texture() {
        Smooth => 0.96 + grain * 0.04,
        Earth | Sand | Snow => 0.84 + grain * 0.16,
        Grass if face == 0 => 0.78 + grain * 0.22,
        Grass => {
            if y < 4 + x % 3 {
                0.91
            } else {
                0.65 + grain * 0.16
            }
        }
        Rock => {
            if mortar(8, 7) {
                0.68
            } else {
                0.83 + grain * 0.17
            }
        }
        Bricks => {
            if mortar(8, 4) {
                0.60
            } else {
                0.88 + grain * 0.12
            }
        }
        Tiles => {
            if x.is_multiple_of(8) || y.is_multiple_of(8) {
                0.62
            } else if x % 8 == 1 || y % 8 == 1 {
                1.0
            } else {
                0.87 + grain * 0.09
            }
        }
        Planks => {
            if y.is_multiple_of(4) || (x + y / 4 * 5).is_multiple_of(16) {
                0.62
            } else {
                0.87 + noise(x / 3, y, 5) * 0.13
            }
        }
        Parquet => {
            if (x + y).is_multiple_of(8) || (x + 16 - y).is_multiple_of(8) {
                0.66
            } else {
                0.87 + grain * 0.13
            }
        }
        Bark if face == 0 || face == 2 => {
            let dx = x as f32 - 7.5;
            let dy = y as f32 - 7.5;
            if ((dx * dx + dy * dy).sqrt() as u32).is_multiple_of(3) {
                0.65
            } else {
                0.92
            }
        }
        Bark => 0.65 + noise(x / 2, y / 6, 11) * 0.30 + grain * 0.05,
        Leaves => {
            if grain > 0.68 {
                1.0
            } else {
                0.65 + grain * 0.32
            }
        }
        Ore => {
            if noise(x / 2, y / 2, 22) > 0.68 {
                1.0
            } else {
                0.60 + grain * 0.15
            }
        }
        Veins => {
            if (x + y / 2 + (y as f32 * 0.7).sin() as u32) % 11 < 2 {
                0.67
            } else {
                0.94 + grain * 0.06
            }
        }
        Metal => {
            if x == 0 || y == 0 {
                0.56
            } else if (x == 2 || x == 13) && (y == 2 || y == 13) {
                0.65
            } else {
                0.87 + y as f32 / 160. + grain * 0.03
            }
        }
        Cloth => {
            if (x + y).is_multiple_of(2) {
                0.83
            } else {
                0.96 + grain * 0.04
            }
        }
    };
    value.clamp(0., 1.)
}
fn image(width: u32, height: u32, bytes: Vec<u8>, srgb: bool) -> Image {
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        if srgb {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        },
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}
pub fn atlas() -> Image {
    let (w, h) = dimensions();
    let mut bytes = vec![255; (w * h * 4) as usize];
    for (i, &block) in Block::ALL.iter().enumerate() {
        for face in 0..3 {
            let tile = i as u32 * 3 + face;
            let ox = tile % COLUMNS * TILE;
            let oy = tile / COLUMNS * TILE;
            for y in 0..TILE {
                for x in 0..TILE {
                    let v = (shade(
                        block,
                        face,
                        x.saturating_sub(2).min(15),
                        y.saturating_sub(2).min(15),
                    ) * 255.) as u8;
                    let at = (((oy + y) * w + ox + x) * 4) as usize;
                    bytes[at..at + 4].copy_from_slice(&[v, v, v, 255]);
                }
            }
        }
    }
    image(w, h, bytes, false)
}
/// UV.x is negative to distinguish block textures from the distant-map mask.
pub fn face_uv(block: Block, normal: [i32; 3], corner: [f32; 3]) -> [f32; 2] {
    let face = if normal[1] > 0 {
        0
    } else if normal[1] < 0 {
        2
    } else {
        1
    };
    let tile = block.catalog_index().unwrap() as u32 * 3 + face;
    let (w, h) = dimensions();
    let (u, v) = if normal[1] != 0 {
        (corner[0], corner[2])
    } else if normal[0] != 0 {
        (corner[2], 1. - corner[1])
    } else {
        (corner[0], 1. - corner[1])
    };
    [
        -1. - (tile % COLUMNS * TILE + 2) as f32 / w as f32 - u * PIXELS as f32 / w as f32,
        (tile / COLUMNS * TILE + 2) as f32 / h as f32 + v * PIXELS as f32 / h as f32,
    ]
}
#[derive(Resource)]
pub struct BlockIcons(pub Vec<Handle<Image>>);
impl FromWorld for BlockIcons {
    fn from_world(world: &mut World) -> Self {
        let mut images = world.resource_mut::<Assets<Image>>();
        Self(
            Block::ALL
                .iter()
                .map(|&block| {
                    // Isometric three-face cube. Every face samples the terrain pattern.
                    let mut bytes = vec![0; 32 * 32 * 4];
                    let tint = block.color();
                    for y in 0..30 {
                        for x in 0..32 {
                            let xf = x as f32 - 15.5;
                            let yf = y as f32;
                            let (face, u, v, light) =
                                if yf >= 1. + xf.abs() * 0.5 && yf < 16. - xf.abs() * 0.5 {
                                    (
                                        0,
                                        ((xf + yf - 1.) / 2.).max(0.) as u32,
                                        ((yf - 1. - xf) / 2.).max(0.) as u32,
                                        1.0,
                                    )
                                } else if yf >= 16. - xf.abs() * 0.5
                                    && yf < 29. - xf.abs() * 0.5
                                    && xf.abs() < 15.
                                {
                                    (
                                        1,
                                        xf.abs() as u32,
                                        (yf - (16. - xf.abs() * 0.5)) as u32,
                                        if xf < 0. { 0.80 } else { 0.65 },
                                    )
                                } else {
                                    continue;
                                };
                            let factor = shade(block, face, u.min(15), v.min(15)) * light;
                            let at = (y * 32 + x) * 4;
                            for c in 0..3 {
                                bytes[at + c] = (tint[c] * factor.powf(1. / 2.2) * 255.) as u8;
                            }
                            bytes[at + 3] = 255;
                        }
                    }
                    images.add(image(32, 32, bytes, true))
                })
                .collect(),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atlas_faces_stay_inside_their_padded_tiles_and_patterns_differ() {
        let atlas = atlas();
        let (w, h) = dimensions();
        assert_eq!(atlas.data.as_ref().unwrap().len(), (w * h * 4) as usize);
        for &block in Block::ALL {
            for normal in [[0, 1, 0], [1, 0, 0], [0, -1, 0]] {
                for corner in [[0.; 3], [1.; 3]] {
                    let uv = face_uv(block, normal, corner);
                    assert!(uv[0] < -1.);
                    assert!((0.0..1.0).contains(&(-uv[0] - 1.)));
                    assert!((0.0..1.0).contains(&uv[1]));
                }
            }
        }
        assert_ne!(shade(Block::OakLog, 0, 8, 8), shade(Block::OakLog, 1, 8, 8));
        assert_ne!(
            shade(Block::RedWool, 1, 0, 0),
            shade(Block::RedConcrete, 1, 0, 0)
        );
    }
}
