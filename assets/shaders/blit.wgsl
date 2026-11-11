// A pass that changes nothing: the picture, texel for texel. The compositor uses it to put
// the picture on the window (nearest sampling does the pixel scaling); it is also the
// shape every pack pass takes. Bindings are the same for all passes:
//
//   @group(2) @binding(0) post: PostParams   frame = (seconds, pixels per texel, width, height)
//                                            params = four vec4s from shaders.ron
//   @group(2) @binding(1) picture             the pass before this one (or the world)
//   @group(2) @binding(2) picture_sampler     nearest
//
// The fragment's texel is floor(in.uv * post.frame.zw).

struct PostParams {
    frame: vec4<f32>,
    params: array<vec4<f32>, 4>,
};

@group(2) @binding(0) var<uniform> post: PostParams;
@group(2) @binding(1) var picture: texture_2d<f32>;
@group(2) @binding(2) var picture_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(picture, picture_sampler, in.uv);
}
