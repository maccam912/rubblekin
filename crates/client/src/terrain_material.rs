//! Nearby voxel grain and map-colored heightmaps share one small material shader.
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

const TERRAIN_SHADER: Handle<Shader> = uuid_handle!("4b72b3eb-c590-4f4a-85aa-215027376ea2");
const GLASS_SHADOW_SHADER: Handle<Shader> = uuid_handle!("450dfbb2-9ab3-4e58-941f-b8df7e45a37a");
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainTexture>;

pub struct TerrainMaterialPlugin;

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
        load_internal_asset!(app, TERRAIN_SHADER, "terrain.wesl", Shader::from_wesl);
        load_internal_asset!(
            app,
            GLASS_SHADOW_SHADER,
            "glass_shadow.wesl",
            Shader::from_wesl
        );
    }
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct TerrainTexture {
    #[uniform(100)]
    distant: f32,
    #[texture(101)]
    #[sampler(102)]
    albedo: Option<Handle<Image>>,
    #[texture(103)]
    #[sampler(104)]
    blocks: Option<Handle<Image>>,
    #[uniform(105)]
    pub weather: Vec4,
}

impl MaterialExtension for TerrainTexture {
    fn vertex_shader() -> ShaderRef {
        TERRAIN_SHADER.into()
    }
    fn fragment_shader() -> ShaderRef {
        TERRAIN_SHADER.into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        GLASS_SHADOW_SHADER.into()
    }
}

pub fn terrain_material(albedo: Option<Handle<Image>>) -> TerrainMaterial {
    TerrainMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 1.0,
            reflectance: 0.12,
            ..default()
        },
        extension: TerrainTexture {
            distant: f32::from(albedo.is_some()),
            albedo,
            blocks: None,
            weather: Vec4::ZERO,
        },
    }
}

pub fn block_material(atlas: Handle<Image>) -> TerrainMaterial {
    let mut material = terrain_material(None);
    material.extension.blocks = Some(atlas);
    material
}
