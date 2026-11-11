// Outline: a texel darkens where its brightness or hue jumps against the texel to its
// right or below, so every block edge and every figure gets a line of ink.

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

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.3, 0.59, 0.11));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let threshold = post.params[0].x;
    let ink = post.params[0].y;
    let step = 1.0 / post.frame.zw;
    let here = textureSample(picture, picture_sampler, in.uv);
    let right = textureSample(picture, picture_sampler, in.uv + vec2<f32>(step.x, 0.0));
    let below = textureSample(picture, picture_sampler, in.uv + vec2<f32>(0.0, step.y));
    let jump = max(
        max(abs(luma(here.rgb) - luma(right.rgb)), abs(luma(here.rgb) - luma(below.rgb))),
        max(length(here.rgb - right.rgb), length(here.rgb - below.rgb)) * 0.6,
    );
    if (jump > threshold) {
        return vec4<f32>(here.rgb * (1.0 - ink), here.a);
    }
    return here;
}
