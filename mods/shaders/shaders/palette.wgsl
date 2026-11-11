// Palette: each channel snapped to a few levels, with a 4 x 4 ordered dither on the picture's
// texels so gradients become patterns rather than bands.

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

fn bayer(t: vec2<i32>) -> f32 {
    var table = array<f32, 16>(
        0.0, 8.0, 2.0, 10.0,
        12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0,
        15.0, 7.0, 13.0, 5.0,
    );
    let x = (t.x % 4 + 4) % 4;
    let y = (t.y % 4 + 4) % 4;
    return (table[x + y * 4] + 0.5) / 16.0 - 0.5;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let levels = max(post.params[0].x, 2.0);
    let dither = post.params[0].y;
    let texel = vec2<i32>(floor(in.uv * post.frame.zw));
    let c = textureSample(picture, picture_sampler, in.uv);
    let steps = levels - 1.0;
    let noise = bayer(texel) * dither / steps;
    let q = floor((c.rgb + noise) * steps + 0.5) / steps;
    return vec4<f32>(clamp(q, vec3<f32>(0.0), vec3<f32>(1.0)), c.a);
}
