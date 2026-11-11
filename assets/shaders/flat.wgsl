// Flat things: vertex colour times a tint, optionally through a texture. The figure, chips
// and dust, the ground shadow and the crack overlay use it.

struct FlatParams {
    tint: vec4<f32>,
    // x: 1 to sample the texture, y: 1 to drop texels under half alpha
    flags: vec4<f32>,
};

@group(3) @binding(0) var<uniform> flat: FlatParams;
@group(3) @binding(1) var picture: texture_2d<f32>;
@group(3) @binding(2) var picture_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(5) color: vec4<f32>,
};

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = in.color * flat.tint;
    if (flat.flags.x > 0.5) {
        let texel = textureSample(picture, picture_sampler, in.uv);
        if (flat.flags.y > 0.5 && texel.a < 0.5) {
            discard;
        }
        color = color * texel;
    }
    return color;
}
