// CRT: every other texel row darker, a soft vignette, and a little barrel curve if asked.

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
    let scan = post.params[0].x;
    let vignette = post.params[0].y;
    let curve = post.params[0].z;
    // Barrel: push texels away from the middle a little more toward the corners.
    let centred = in.uv * 2.0 - 1.0;
    let r2 = dot(centred, centred);
    let warped = centred * (1.0 + curve * r2);
    let uv = warped * 0.5 + 0.5;
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    var c = textureSample(picture, picture_sampler, uv);
    let row = i32(floor(uv.y * post.frame.w));
    if (row % 2 == 1) {
        c = vec4<f32>(c.rgb * (1.0 - scan), c.a);
    }
    let edge = 1.0 - vignette * smoothstep(0.5, 1.6, r2);
    return vec4<f32>(c.rgb * edge, c.a);
}
