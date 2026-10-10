// Nearby voxels keep fine filtered grain. Distant heightmaps use the world map,
// without procedural square grain or imitation voxel faces.
import bevy_pbr::{
    render::{
        forward_io::{VertexOutput, FragmentOutput},
        mesh_view_bindings::view,
        pbr_fragment::pbr_input_from_standard_material,
        pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    },
};

import bevy_pbr::render::{
    forward_io::{Vertex, decompress_vertex},
    mesh_functions,
    view_transformations::position_world_to_clip,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> distant: f32;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var distant_albedo: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var distant_sampler: sampler;

@group(constants::MATERIAL_BIND_GROUP) @binding(103) var block_albedo: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var block_sampler: sampler;

@group(constants::MATERIAL_BIND_GROUP) @binding(105) var<uniform> weather: vec4<f32>;

@vertex
fn vertex(input: Vertex) -> VertexOutput {
    let v = decompress_vertex(input, input.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(input.instance_index);
    var out: VertexOutput;
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0));
    @if(VERTEX_UVS_A) {
        // Authored vegetation carries its height above the rooted base in uv.y.
        // Blocks and distant ground keep their existing texture coordinates.
        if abs(v.uv.x) < 0.1 && v.uv.y > 0.0 {
            let wave = 0.55 + 0.45 * sin(weather.z + out.world_position.x * 0.31 + out.world_position.z * 0.23);
            out.world_position.x += weather.x * v.uv.y * wave * 0.025;
            out.world_position.z += weather.y * v.uv.y * wave * 0.025;
        }
        out.uv = v.uv;
    }
    out.position = position_world_to_clip(out.world_position.xyz);
    @if(VERTEX_NORMALS) out.world_normal = mesh_functions::mesh_normal_local_to_world(v.normal, input.instance_index);
    @if(VERTEX_COLORS) out.color = v.color;
    @if(VERTEX_UVS_B) out.uv_b = v.uv_b;
    @if(VERTEX_TANGENTS) out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, v.tangent, input.instance_index);
    @if(VERTEX_OUTPUT_INSTANCE_INDEX) out.instance_index = input.instance_index;
    @if(VISIBILITY_RANGE_DITHER) out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(input.instance_index, world_from_local[3]);
    return out;
}

fn grain(position: vec3<f32>, size: f32) -> f32 {
    let cell = vec3<i32>(floor(position / size));
    var value = (bitcast<u32>(cell.x) * 0x9e3779b9u)
        ^ (bitcast<u32>(cell.y) * 0x85ebca6bu)
        ^ (bitcast<u32>(cell.z) * 0xc2b2ae35u);
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
    // Only heightmap ground carries this mask. Local silhouettes and edited
    // voxels keep their material colors. Blend near the detailed chunk boundary;
    // map visibility depends on distance, not on screen resolution or slope.
    let map_color = textureSample(distant_albedo, distant_sampler,
        (in.world_position.xz + vec2<f32>(16384.0)) / 32768.0).rgb;
    let map_surface = distant * max(in.uv.x, 0.0);
    let block_color = textureSample(block_albedo, block_sampler, vec2<f32>(-in.uv.x - 1.0, in.uv.y)).rgb;
    pbr.material.base_color = vec4<f32>(pbr.material.base_color.rgb * select(vec3<f32>(1.0), mix(block_color, vec3<f32>(0.88), smoothstep(0.03, 0.18, footprint)), in.uv.x < -0.5), pbr.material.base_color.a);
    let map_weight = map_surface * smoothstep(48.0, 144.0,
        distance(in.world_position.xyz, view.world_position));
    pbr.material.base_color = vec4<f32>(
        mix(pbr.material.base_color.rgb, map_color, map_weight),
        pbr.material.base_color.a);
    let fine = 1.0 - smoothstep(0.04, 0.14, footprint);
    let block = 1.0 - smoothstep(0.3, 1.0, footprint);
    let texture = 1.0 + (1.0 - map_surface) * (
        grain(position, 0.125) * 0.16 * fine
        + grain(position, 0.5) * 0.08 * block);
    pbr.material.base_color = vec4<f32>(pbr.material.base_color.rgb * texture, pbr.material.base_color.a);
    pbr.material.base_color = alpha_discard(pbr.material.flags, pbr.material.alpha_cutoff, pbr.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr);
    out.color = main_pass_post_lighting_processing(pbr, out.color);
    return out;
}
