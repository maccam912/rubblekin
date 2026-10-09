//! One inexpensive background draw, driven by the existing shared world clock.
use crate::{GameCamera, GameEntity, Session, graphics::GraphicsSettings};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    camera::visibility::NoFrustumCulling,
    light::NotShadowCaster,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

/// Twenty minutes above the horizon and twenty below, including gradual twilight.
const CYCLE_SECONDS: f64 = 40.0 * 60.0;
// A 0.15-degree step holds the shadow projection still between updates. The
// visible sun, sky colors and light intensity continue moving every frame.
const LIGHT_STEP_SECONDS: f64 = 1.0;
const SKY_SHADER: Handle<Shader> = uuid_handle!("d219b706-e0b1-4b9d-b802-4432891f9b8a");

pub(crate) struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default())
            .init_resource::<SkyAssets>();
        load_internal_asset!(app, SKY_SHADER, "sky.wesl", Shader::from_wesl);
    }
}

#[derive(Clone, Debug, Reflect, ShaderType)]
struct SkyColors {
    zenith: Vec4,
    horizon: Vec4,
    sun_direction: Vec4,
    sun_color: Vec4,
    clouds: Vec4,
    // Sunset strength, daylight strength, spare, spare.
    phase: Vec4,
}

#[derive(Asset, AsBindGroup, Clone, Debug, Reflect)]
pub(crate) struct SkyMaterial {
    #[uniform(0)]
    colors: SkyColors,
}

impl Material for SkyMaterial {
    fn vertex_shader() -> ShaderRef {
        SKY_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SKY_SHADER.into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// Reuse both handles across leave/rejoin instead of accumulating sky assets.
#[derive(Resource, Default)]
pub(crate) struct SkyAssets(Option<(Handle<Mesh>, Handle<SkyMaterial>)>);

#[derive(Component)]
pub(crate) struct SkyDome;

#[derive(Component)]
pub(crate) struct SunLight;

struct SkyState {
    colors: SkyColors,
    light_direction: Vec3,
    light_color: Color,
    illuminance: f32,
    ambient_color: Color,
    ambient_brightness: f32,
}

fn smooth(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rgb(r: f32, g: f32, b: f32) -> Vec3 {
    let c = LinearRgba::from(Color::srgb(r, g, b));
    Vec3::new(c.red, c.green, c.blue)
}

fn color(rgb: Vec3) -> Color {
    Color::linear_rgb(rgb.x, rgb.y, rgb.z)
}

fn sun_direction(world_seconds: f64) -> Vec3 {
    // New worlds start in the morning. Reduce f64 time before converting to f32
    // so an old world's sun does not jitter or lose its daily period.
    let angle = (world_seconds.rem_euclid(CYCLE_SECONDS) / CYCLE_SECONDS * std::f64::consts::TAU
        + std::f64::consts::FRAC_PI_4) as f32;
    let (height, horizontal) = angle.sin_cos();
    Vec3::new(horizontal * 0.9165151, height, horizontal * 0.4)
}

fn state(world_seconds: f64) -> SkyState {
    let sun = sun_direction(world_seconds);
    let height = sun.y;
    // Reduce the old-world clock before quantizing, retaining precision and
    // exactly the same steps on every client. Bevy already snaps cascade
    // translation to texels; rotating that grid every frame still shimmers.
    let light_seconds =
        (world_seconds.rem_euclid(CYCLE_SECONDS) / LIGHT_STEP_SECONDS).floor() * LIGHT_STEP_SECONDS;
    let shadow_sun = sun_direction(light_seconds);
    let daylight = smooth(-0.18, 0.24, height);
    let sunset = 1.0 - smooth(0.0, 0.24, (height + 0.015).abs());
    let stars = 1.0 - smooth(-0.26, 0.015, height);
    let zenith = rgb(0.035, 0.055, 0.14)
        .lerp(rgb(0.12, 0.39, 0.74), daylight)
        .lerp(rgb(0.28, 0.19, 0.35), sunset * 0.65);
    let horizon = rgb(0.20, 0.27, 0.43)
        .lerp(rgb(0.69, 0.83, 0.95), daylight)
        .lerp(rgb(0.94, 0.43, 0.24), sunset * 0.65);
    let clouds = rgb(0.24, 0.30, 0.44)
        .lerp(rgb(0.97, 0.98, 1.0), daylight)
        .lerp(rgb(1.0, 0.59, 0.37), sunset * 0.8);
    let sunlight = rgb(1.0, 0.43, 0.16).lerp(rgb(1.0, 0.95, 0.85), smooth(0.02, 0.45, height));
    SkyState {
        colors: SkyColors {
            zenith: zenith.extend(1.0),
            horizon: horizon.extend(1.0),
            sun_direction: sun.extend(stars),
            sun_color: sunlight.extend(smooth(-0.035, 0.015, height)),
            // Cloud noise repeats every 256 cells. These two speeds travel
            // exactly whole periods in 256,000 seconds, keeping wrap seamless.
            clouds: clouds.extend(world_seconds.rem_euclid(256_000.0) as f32),
            phase: Vec4::new(sunset, daylight, 0.0, 0.0),
        },
        light_direction: if shadow_sun.y >= 0.0 {
            shadow_sun
        } else {
            -shadow_sun
        },
        light_color: if height >= 0.0 {
            color(sunlight)
        } else {
            Color::srgb(0.65, 0.75, 1.0)
        },
        // Fade the sun out at the horizon; a gentle cool fill supplies night
        // relief. Ambient light keeps paths and shaded faces readable all night.
        illuminance: 10_500.0 * smooth(0.0, 0.22, height) + 900.0 * smooth(0.0, 0.30, -height),
        ambient_color: color(rgb(0.65, 0.74, 1.0).lerp(rgb(0.78, 0.86, 1.0), daylight)),
        ambient_brightness: 400.0 + 200.0 * daylight,
    }
}

pub(crate) fn setup(
    mut commands: Commands,
    session: Res<Session>,
    graphics: Res<GraphicsSettings>,
    mut cache: ResMut<SkyAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let sky = state(session.airship_clock.time);
    let (mesh, material) = cache.0.get_or_insert_with(|| {
        (
            meshes.add(Cuboid::new(2.0, 2.0, 2.0)),
            materials.add(SkyMaterial {
                colors: sky.colors.clone(),
            }),
        )
    });
    commands.spawn((
        GameEntity,
        SkyDome,
        Mesh3d(mesh.clone()),
        MeshMaterial3d(material.clone()),
        Transform::default(),
        NoFrustumCulling,
        NotShadowCaster,
    ));
    commands.spawn((
        GameEntity,
        SunLight,
        DirectionalLight {
            illuminance: sky.illuminance,
            color: sky.light_color,
            shadow_maps_enabled: graphics.quality.shadows(),
            ..default()
        },
        graphics.cascades(),
        light_transform(sky.light_direction),
    ));
}

fn light_transform(direction: Vec3) -> Transform {
    // Perpendicular to the sun's orbit, including when it passes overhead.
    Transform::IDENTITY.looking_to(-direction, Vec3::new(-0.4, 0.0, 0.9165151))
}

#[allow(clippy::type_complexity)]
pub(crate) fn update(
    session: Res<Session>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut cameras: Query<
        (&Transform, &mut DistanceFog),
        (With<GameCamera>, Without<SkyDome>, Without<SunLight>),
    >,
    mut domes: Query<
        (&mut Transform, &MeshMaterial3d<SkyMaterial>),
        (With<SkyDome>, Without<GameCamera>, Without<SunLight>),
    >,
    mut lights: Query<
        (&mut Transform, &mut DirectionalLight),
        (With<SunLight>, Without<GameCamera>, Without<SkyDome>),
    >,
) {
    let sky = state(session.airship_clock.time);
    ambient.color = sky.ambient_color;
    ambient.brightness = sky.ambient_brightness;
    let horizon = color(sky.colors.horizon.truncate());
    clear.0 = horizon;
    for (camera, mut fog) in &mut cameras {
        fog.color = horizon;
        for (mut dome, material) in &mut domes {
            dome.translation = camera.translation;
            if let Some(mut material) = materials.get_mut(&material.0) {
                material.colors = sky.colors.clone();
            }
        }
    }
    for (mut transform, mut light) in &mut lights {
        transform.set_if_neq(light_transform(sky.light_direction));
        light.color = sky.light_color;
        light.illuminance = sky.illuminance;
    }
}

#[cfg(test)]
#[path = "sky_tests.rs"]
mod tests;
