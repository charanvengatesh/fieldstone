// Ground layer. Vertex rgb is the terrain color and alpha its smooth field weight; the
// shoreline is decided per pixel at 0.5 with screen-space anti-aliasing, so edges stay crisp
// and curved at any zoom. A faint per-pixel grain keeps flat fields from reading as plastic.

struct GroundParams {
    // x: grain strength, y: 1 when this is the opaque base layer, z: time, w: unused
    params: vec4<f32>,
};

@group(2) @binding(0) var<uniform> ground: GroundParams;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) @interpolate(flat) instance_index: u32,
};

fn hash(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let grain_strength = ground.params.x;
    let is_base = ground.params.y > 0.5;
    let p = in.world_position.xy / 25.0;

    // Two grain scales: broad mottling and fine speckle.
    let broad = noise(p * 0.9) - 0.5;
    let fine = noise(p * 7.0) - 0.5;
    let grain = 1.0 + (broad * 0.08 + fine * 0.05) * grain_strength;
    let rgb = in.color.rgb * grain;

    if is_base {
        return vec4<f32>(rgb, 1.0);
    }
    let w = in.color.a;
    let aa = max(fwidth(w), 0.002) * 0.9;
    let alpha = smoothstep(0.5 - aa, 0.5 + aa, w);
    return vec4<f32>(rgb, alpha);
}
