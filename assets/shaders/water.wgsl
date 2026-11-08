// Water overlay. The mesh carries the water color in rgb and the smooth "waterness" weight in
// alpha; this shader cuts the shoreline at 0.5, draws a thin foam line, brightens the shallows,
// and moves a quiet caustic pattern with sparse sparkles over the surface.

struct WaterParams {
    // x: seconds, y: ripple strength, z: foam width (weight units), w: unused
    params: vec4<f32>,
};

@group(2) @binding(0) var<uniform> water: WaterParams;

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

// Two noise layers drifting against each other; their product makes soft caustic cells.
fn caustic(p: vec2<f32>, t: f32) -> f32 {
    let a = noise(p * 1.7 + vec2<f32>(t * 0.05, t * 0.035));
    let b = noise(p * 1.9 - vec2<f32>(t * 0.04, -t * 0.045) + 7.3);
    return a * b;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = water.params.x;
    let strength = water.params.y;
    let foam_w = water.params.z;
    let p = in.world_position.xy / 25.0;
    let w = in.color.a;

    // Surface: gentle brightness variation, no bands.
    let c = caustic(p, t);
    let swell = (noise(p * 0.35 + vec2<f32>(t * 0.02, 0.0)) - 0.5) * 0.10;
    var rgb = in.color.rgb * (1.0 + swell * strength) + vec3<f32>(c * 0.18 * strength);

    // Sparse sparkles that come and go.
    let s = noise(p * 6.0 + vec2<f32>(t * 0.3, -t * 0.2));
    let sparkle = smoothstep(0.90, 0.98, s) * (0.5 + 0.5 * sin(t * 3.0 + p.x * 9.0 + p.y * 7.0));
    rgb = rgb + vec3<f32>(sparkle * 0.35);

    // Shoreline: cut at the contour, a thin foam line just inside it that breathes, and a
    // lighter shelf fading into deeper water.
    let aa = max(fwidth(w), 0.002) * 0.9;
    let edge = smoothstep(0.5 - aa, 0.5 + aa, w);
    let breathe = 0.6 + 0.4 * sin(t * 1.3 + p.x * 1.5 + p.y * 1.1 + c * 4.0);
    let line = smoothstep(0.5, 0.5 + foam_w * 0.4, w) * (1.0 - smoothstep(0.5 + foam_w * 0.4, 0.5 + foam_w * 1.6, w));
    let shelf = (1.0 - smoothstep(0.52, 0.78, w)) * 0.16;
    rgb = rgb + vec3<f32>(shelf);
    rgb = mix(rgb, vec3<f32>(0.94, 0.97, 1.0), line * (0.55 + 0.35 * breathe));

    return vec4<f32>(rgb, edge);
}
