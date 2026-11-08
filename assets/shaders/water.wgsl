// Water overlay. The mesh carries the water color in rgb and the bilinear "waterness" weight
// in alpha; this shader cuts the shoreline at 0.5, foams the edge, and ripples the surface.

struct WaterParams {
    // x: seconds, y: ripple strength, z: foam width, w: unused
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

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let t = water.params.x;
    let strength = water.params.y;
    let foam_w = water.params.z;
    let p = in.world_position.xy / 25.0;
    let w = in.color.a;

    // Two drifting noise layers make slow swells; a fine sine adds the glittery ripple.
    let n1 = noise(p * 0.9 + vec2<f32>(t * 0.08, t * 0.05));
    let n2 = noise(p * 2.3 - vec2<f32>(t * 0.11, -t * 0.07));
    let swell = (n1 * 0.6 + n2 * 0.4) - 0.5;
    let ripple = sin(p.x * 3.1 + p.y * 2.2 + t * 1.6 + swell * 6.0);
    let glint = smoothstep(0.86, 1.0, ripple) * 0.35;
    var rgb = in.color.rgb * (1.0 + swell * 0.35 * strength + ripple * 0.06 * strength) + vec3<f32>(glint);

    // Shoreline: cut at the contour, foam just inside it, slight brightening in the shallows.
    let edge = smoothstep(0.47, 0.53, w);
    let foam_band = smoothstep(0.5, 0.5 + foam_w, w) * (1.0 - smoothstep(0.5 + foam_w, 0.5 + foam_w * 2.4, w));
    let foam_wave = 0.5 + 0.5 * sin(t * 2.2 + p.x * 4.0 + p.y * 3.0 + n2 * 5.0);
    let foam = foam_band * (0.45 + 0.55 * foam_wave);
    let shallows = (1.0 - smoothstep(0.5, 0.85, w)) * 0.12;
    rgb = mix(rgb + shallows, vec3<f32>(0.93, 0.97, 1.0), foam * 0.85);

    return vec4<f32>(rgb, edge);
}
