// World-aligned pixel grain. Derivative filtering removes subpixel patterns;
// larger layers retain visible texture on coarse distant landscape meshes.
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> distant: f32;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var distant_albedo: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var distant_sampler: sampler;

fn grain(position: vec3<f32>, size: f32) -> f32 {
    let cell = vec3<i32>(floor(position / size));
    var value = bitcast<u32>(cell.x) * 0x9e3779b9u
        ^ bitcast<u32>(cell.y) * 0x85ebca6bu
        ^ bitcast<u32>(cell.z) * 0xc2b2ae35u;
    value = (value ^ (value >> 16u)) * 0x7feb352du;
    value = (value ^ (value >> 15u)) * 0x846ca68bu;
    return f32((value ^ (value >> 16u)) & 65535u) / 65535.0 - 0.5;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    // A constant bias avoids numerical flips on exact half-meter block planes.
    let position = in.world_position.xyz + vec3<f32>(0.013);
    let footprint = max(length(dpdx(position)), length(dpdy(position)));
    // Only generated landscape vertices carry this mask. Batched tree crowns
    // keep their species colors, and detailed edited blocks keep their material.
    let map_color = textureSample(distant_albedo, distant_sampler,
        (in.world_position.xz + vec2<f32>(16384.0)) / 32768.0).rgb;
    let map_weight = distant * in.uv.x * smoothstep(0.3, 3.0, footprint);
    pbr.material.base_color = vec4<f32>(
        mix(pbr.material.base_color.rgb, map_color, map_weight),
        pbr.material.base_color.a);
    let fine = 1.0 - smoothstep(0.04, 0.14, footprint);
    let block = 1.0 - smoothstep(0.3, 1.0, footprint);
    let medium = 1.0 - smoothstep(3.0, 12.0, footprint);
    let broad = 1.0 - smoothstep(24.0, 80.0, footprint);
    let texture = 1.0 + grain(position, 0.125) * 0.16 * fine
        + grain(position, 0.5) * 0.08 * block
        + grain(position, 8.0) * 0.16 * medium
        + grain(position, 64.0) * 0.14 * broad;
    pbr.material.base_color = vec4<f32>(pbr.material.base_color.rgb * texture, pbr.material.base_color.a);
    pbr.material.base_color = alpha_discard(pbr.material, pbr.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    out.color = main_pass_post_lighting_processing(pbr, out.color);
    return out;
}
