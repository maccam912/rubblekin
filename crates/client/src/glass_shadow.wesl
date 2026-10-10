// Shadow coverage follows glass opacity; the main surface stays alpha blended.
// Matching front/back texels avoid doubled attenuation through a glass cube.
import bevy_pbr::{prepass::io as prepass_io, render::{pbr_bindings,mesh_bindings::mesh,pbr_prepass_functions}};
@if(BINDLESS)
import bevy_pbr::render::pbr_bindings::material_indices;

fn coverage(in: prepass_io::VertexOutput) {
    @if(BINDLESS) {
        let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
        let opacity = pbr_bindings::material_array[material_indices[slot].material].base_color.a;
    } @else {
        let opacity = pbr_bindings::material.base_color.a;
    }
    let pixel = vec2<u32>(in.position.xy) & vec2<u32>(3u);
    let bayer = array<u32,16>(0u,8u,2u,10u,12u,4u,14u,6u,3u,11u,1u,9u,15u,7u,13u,5u);
    if opacity <= (f32(bayer[pixel.y*4u+pixel.x])+0.5)/16.0 { discard; }
}
@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in:prepass_io::VertexOutput)->prepass_io::FragmentOutput {
    coverage(in);
    var out:prepass_io::FragmentOutput;
    @if(UNCLIPPED_DEPTH_ORTHO_EMULATION) out.frag_depth=in.unclipped_depth;
    @if(NORMAL_PREPASS) out.normal=vec4(in.world_normal*0.5+vec3(0.5),1.0);
    @if(MOTION_VECTOR_PREPASS) out.motion_vector=pbr_prepass_functions::calculate_motion_vector(in.world_position,in.previous_world_position);
    return out;
}
@else
@fragment
fn fragment(in:prepass_io::VertexOutput) { coverage(in); }
