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

struct Vertex {
    @builtin(instance_index) instance: u32,
    @location(0) data: vec3<u32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    // Face-local cell coordinates: corner times extent.
    @location(1) face_local: vec2<f32>,
    @location(2) light: vec3<f32>,
    @location(3) @interpolate(flat) flags: u32,
    // tile index, face (0 top, 1 +x, 2 +y, 3 -x, 4 -y, 5 cross), cells wide, cells high
    @location(4) @interpolate(flat) info: vec4<u32>,
};

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let w0 = v.data.x;
    let w1 = v.data.y;
    let w2 = v.data.z;
    let local = vec3<f32>(f32(w0 & 31u), f32((w0 >> 5u) & 31u), f32((w0 >> 10u) & 63u));
    let face = (w0 >> 16u) & 7u;
    let corner = vec2<f32>(f32((w0 >> 19u) & 1u), f32((w0 >> 20u) & 1u));
    let extent = vec2<u32>(((w0 >> 21u) & 31u) + 1u, ((w0 >> 26u) & 31u) + 1u);
    let tile = w1 & 4095u;
    let sun = f32((w1 >> 12u) & 1023u) / 256.0;
    let lamp = f32((w1 >> 22u) & 63u) / 63.0;
    let ambient = f32((w2 >> 12u) & 255u) / 128.0;

    // The affine 4x3 transform, stored transposed; the same unpacking as Bevy's
    // affine3_to_square.
    let m = mesh[v.instance].world_from_local;
    let world_from_local = transpose(mat4x4<f32>(m[0], m[1], m[2], vec4<f32>(0.0, 0.0, 0.0, 1.0)));
    let world = world_from_local * vec4<f32>(local, 1.0);

    var out: VertexOutput;
    out.position = view.clip_from_world * world;
    out.world_position = world;
    out.face_local = corner * vec2<f32>(extent);
    out.light = vec3<f32>(sun, lamp, ambient);
    out.flags = w2 & 4095u;
    out.info = vec4<u32>(tile, face, extent.x, extent.y);
    return out;
}
