// Stylized sky in one twelve-triangle background draw. No textures, volumetric
// passes, cloud entities, or additional shadow maps.
#import bevy_pbr::mesh_view_bindings::view
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping::{tone_mapping, screen_space_dither}
#endif

struct SkyColors {
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    sun_direction: vec4<f32>,
    sun_color: vec4<f32>,
    clouds: vec4<f32>,
    phase: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: SkyColors;

struct SkyVertex {
    @location(0) position: vec3<f32>,
}
struct SkyFragment {
    @builtin(position) position: vec4<f32>,
    @location(0) direction: vec3<f32>,
}

@vertex
fn vertex(in: SkyVertex) -> SkyFragment {
    var out: SkyFragment;
    // A direction (w = 0) removes camera translation without subtracting large
    // world coordinates. Reverse-Z depth zero puts the sky behind all scenery,
    // even in legacy worlds with a much closer far clipping plane.
    out.position = view.clip_from_world * vec4<f32>(in.position, 0.0);
    out.position.z = 0.0;
    out.direction = in.position;
    return out;
}

fn hash(p: vec2<f32>) -> f32 {
    var q = fract(vec3<f32>(p.xyx) * 0.1031);
    q += dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}

fn noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    // Periodic lattice keeps cloud drift continuous across its time wrap.
    let a = hash(cell - floor(cell / 256.0) * 256.0);
    let b_cell = cell + vec2<f32>(1.0, 0.0);
    let c_cell = cell + vec2<f32>(0.0, 1.0);
    let d_cell = cell + vec2<f32>(1.0);
    let b = hash(b_cell - floor(b_cell / 256.0) * 256.0);
    let c = hash(c_cell - floor(c_cell / 256.0) * 256.0);
    let d = hash(d_cell - floor(d_cell / 256.0) * 256.0);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn stars(ray: vec3<f32>) -> f32 {
    let uv = vec2<f32>(atan2(ray.z, ray.x) / 6.2831853 + 0.5,
        asin(clamp(ray.y, -1.0, 1.0)) / 3.1415927 + 0.5) * vec2<f32>(640.0, 320.0);
    let cell = floor(uv);
    let seed = hash(cell);
    let center = vec2<f32>(hash(cell + 13.7), hash(cell + 71.3)) * 0.6 + 0.2;
    let radius = mix(0.055, 0.14, hash(cell + 29.1));
    let aa = max(length(fwidth(uv)) * 0.45, 0.015);
    let disk = 1.0 - smoothstep(max(radius - aa, 0.0), radius + aa,
        length(fract(uv) - center));
    let twinkle = 0.86 + 0.14 * sin(sky.clouds.w * 0.6 + seed * 100.0);
    return disk * step(0.975, seed) * twinkle * smoothstep(0.015, 0.18, ray.y);
}

@fragment
fn fragment(in: SkyFragment) -> @location(0) vec4<f32> {
    let ray = normalize(in.direction);
    let altitude = max(ray.y, 0.0);
    var result = mix(sky.horizon.rgb, sky.zenith.rgb, pow(altitude, 0.45));
    let sun_dot = dot(ray, sky.sun_direction.xyz);
    let toward_sun = pow(max(dot(normalize(vec3<f32>(ray.x, 0.001, ray.z)),
        normalize(vec3<f32>(sky.sun_direction.x, 0.001, sky.sun_direction.z))), 0.0), 4.0);
    let warm_horizon = exp(-altitude * 7.0) * sky.phase.x * (0.2 + 0.6 * toward_sun);
    result = mix(result, vec3<f32>(1.0, 0.20, 0.035), warm_horizon);
    let sun_aa = max(fwidth(sun_dot), 0.000004);
    let disk = smoothstep(0.99982 - sun_aa, 0.99982 + sun_aa, sun_dot);
    let halo = pow(max(sun_dot, 0.0), 160.0) * 0.22;
    result += sky.sun_color.rgb * (disk * 2.5 + halo) * sky.sun_color.w;
    result += vec3<f32>(0.75, 0.84, 1.0) * stars(ray) * sky.sun_direction.w;

    // Intersect a high horizontal cloud sheet. World coordinates give clouds
    // parallax during travel; wind moves their bounded three-octave pattern.
    let ray_y = select(min(ray.y, -0.001), max(ray.y, 0.001), ray.y >= 0.0);
    let cloud_distance = (3200.0 - view.world_position.y) / ray_y;
    let point = (view.world_position.xz + ray.xz * cloud_distance) / 1000.0
        + sky.clouds.w * vec2<f32>(0.004, 0.001);
    let shape = noise(point) * 0.58 + noise(point * 2.0 + 17.0) * 0.28
        + noise(point * 4.0 + 43.0) * 0.14;
    let cloud = smoothstep(0.48, 0.67, shape) * 0.96
        * (1.0 - smoothstep(50000.0, 140000.0, abs(cloud_distance)))
        * step(0.0, cloud_distance);
    let cloud_color = sky.clouds.rgb * mix(0.64, 1.0, smoothstep(0.46, 0.76, shape));
    result = mix(result, cloud_color, cloud);
    var output = vec4<f32>(result, 1.0);
#ifdef TONEMAP_IN_SHADER
    output = tone_mapping(output, view.color_grading);
#ifdef DEBAND_DITHER
    let dithered = pow(max(output.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2))
        + screen_space_dither(in.position.xy);
    output = vec4<f32>(pow(max(dithered, vec3<f32>(0.0)), vec3<f32>(2.2)), 1.0);
#endif
#endif
    return output;
}
