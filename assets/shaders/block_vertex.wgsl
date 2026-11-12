// The chunk vertex stage: unpacks the one attribute a chunk mesh carries (three words,
// see pack_vertex in src/world/mesh.rs) into the position, the atlas uv, the light
// channels and the flags that block.wgsl and water.wgsl read.
//
// Plain WGSL cannot import Bevy's definitions, so the view and mesh bindings are declared
// here; they must match bevy_render's View (the first field is all that is read) and
// bevy_pbr's Mesh (copied whole: chunks are batched and addressed by instance index, so
// the stride must be exact).

struct View {
    clip_from_world: mat4x4<f32>,
    unjittered_clip_from_world: mat4x4<f32>,
    world_from_clip: mat4x4<f32>,
    world_from_view: mat4x4<f32>,
    view_from_world: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> view: View;

struct Mesh {
    world_from_local: mat3x4<f32>,
    previous_world_from_local: mat3x4<f32>,
    local_from_world_transpose_a: mat2x4<f32>,
    local_from_world_transpose_b: f32,
    flags: u32,
    lightmap_uv_rect: vec2<u32>,
    first_vertex_index: u32,
    current_skin_index: u32,
    material_and_lightmap_bind_group_slot: u32,
    tag: u32,
    morph_descriptor_index: u32,
    metadata_index: u32,
};

@group(2) @binding(0) var<storage> mesh: array<Mesh>;

struct BlockParams {
    params: vec4<f32>,
    day: vec4<f32>,
    focus: vec4<f32>,
    toward_eye: vec4<f32>,
    sun_color: vec4<f32>,
    ambient_color: vec4<f32>,
    haze: vec4<f32>,
};

@group(3) @binding(0) var<uniform> block: BlockParams;

struct Vertex {
    @builtin(instance_index) instance: u32,
    @location(0) data: vec3<u32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) light: vec3<f32>,
    @location(3) @interpolate(flat) flags: u32,
};

const TILE: f32 = 16.0;
const COLS: u32 = 16u;
// How far, in texels, uvs stay inside their tile so sampling never bleeds (mesh.rs INSET).
const INSET: f32 = 0.01;

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let w0 = v.data.x;
    let w1 = v.data.y;
    let w2 = v.data.z;
    let local = vec3<f32>(f32(w0 & 31u), f32((w0 >> 5u) & 31u), f32((w0 >> 10u) & 63u));
    let corner = vec2<f32>(f32((w0 >> 19u) & 1u), f32((w0 >> 20u) & 1u));
    let tile = w1 & 4095u;
    let sun = f32((w1 >> 12u) & 1023u) / 256.0;
    let lamp = f32((w1 >> 22u) & 63u) / 63.0;
    let ambient = f32((w2 >> 12u) & 255u) / 128.0;

    // The affine 4x3 transform, stored transposed; the same unpacking as Bevy's
    // affine3_to_square.
    let m = mesh[v.instance].world_from_local;
    let world_from_local = transpose(mat4x4<f32>(m[0], m[1], m[2], vec4<f32>(0.0, 0.0, 0.0, 1.0)));
    let world = world_from_local * vec4<f32>(local, 1.0);

    let origin = vec2<f32>(f32((tile % COLS) * 16u), f32((tile / COLS) * 16u));
    let size = block.params.yz;
    let uv = (origin + vec2<f32>(INSET) + corner * (TILE - 2.0 * INSET)) / size;

    var out: VertexOutput;
    out.position = view.clip_from_world * world;
    out.world_position = world;
    out.uv = uv;
    out.light = vec3<f32>(sun, lamp, ambient);
    out.flags = w2 & 4095u;
    return out;
}
