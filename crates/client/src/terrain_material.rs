//! One embedded terrain shader keeps nearby voxels and distant land textured alike.
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

const TERRAIN_SHADER: Handle<Shader> = uuid_handle!("4b72b3eb-c590-4f4a-85aa-215027376ea2");
pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainTexture>;

pub struct TerrainMaterialPlugin;

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default());
        load_internal_asset!(app, TERRAIN_SHADER, "terrain.wgsl", Shader::from_wgsl);
    }
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct TerrainTexture {
    #[uniform(100)]
    distant: f32,
    #[texture(101)]
    #[sampler(102)]
    albedo: Option<Handle<Image>>,
}

impl MaterialExtension for TerrainTexture {
    fn fragment_shader() -> ShaderRef {
        TERRAIN_SHADER.into()
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
        },
    }
}
