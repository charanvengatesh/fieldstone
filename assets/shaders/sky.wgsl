// The sky quad: a vertical gradient in a few dithered bands, a sun and a moon disc, and
// stars at night. Everything is drawn on the texel grid so it matches the blocks.
// src/sky.rs chooses the colours; src/debug.rs paints the same gradient for snapshots.

struct SkyParams {
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    // x, y: position on screen 0..1, z: radius, w: daylight
    sun: vec4<f32>,
    // x, y: position, z: radius, w: star visibility
    moon: vec4<f32>,
    // x: screen pixels per texel, y: aspect, z: seconds, w: bands
    params: vec4<f32>,
};

@group(2) @binding(0) var<uniform> sky: SkyParams;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

fn hash(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
}

fn bayer(t: vec2<f32>) -> f32 {
    var table = array<f32, 16>(
        0.0, 8.0, 2.0, 10.0,
        12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0,
        15.0, 7.0, 13.0, 5.0,
    );
    let x = (i32(t.x) % 4 + 4) % 4;
    let y = (i32(t.y) % 4 + 4) % 4;
    return (table[x + y * 4] + 0.5) / 16.0;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = floor(in.position.xy / max(sky.params.x, 1.0));
    // The mesh's uv runs top to bottom; screen fractions with y up.
    let screen = vec2<f32>(in.uv.x, 1.0 - in.uv.y);
    let bands = sky.params.w;

    // Gradient, quantised with a dither so the bands step on texels.
    let t = clamp(screen.y + (bayer(texel) - 0.5) / bands, 0.0, 1.0);
    let level = floor(t * bands) / bands;
    var color = mix(sky.horizon.rgb, sky.zenith.rgb, level);

    // Stars: sparse texels, twinkling slowly, only when it is dark.
    if (sky.moon.w > 0.0) {
        let cell = texel;
        let star = hash(cell);
        if (star > 0.996) {
            let twinkle = 0.6 + 0.4 * sin(sky.params.z * 2.0 + star * 90.0);
            color = mix(color, vec3<f32>(0.9, 0.92, 1.0), sky.moon.w * twinkle);
        }
    }

    // Discs, measured in screen heights so they stay round.
    let aspect = vec2<f32>(sky.params.y, 1.0);
    let to_sun = (screen - sky.sun.xy) * aspect;
    let to_moon = (screen - sky.moon.xy) * aspect;
    if (length(to_moon) < sky.moon.z && sky.sun.w < 0.6) {
        // A crescent: the far half is sky.
        let bite = length(to_moon - vec2<f32>(sky.moon.z * 0.45, sky.moon.z * 0.2));
        if (bite > sky.moon.z * 0.9) {
            color = vec3<f32>(0.86, 0.88, 0.80);
        }
    }
    if (length(to_sun) < sky.sun.z && sky.sun.w > 0.2) {
        color = mix(vec3<f32>(1.0, 0.72, 0.40), vec3<f32>(1.0, 0.98, 0.86), sky.sun.w);
    } else if (length(to_sun) < sky.sun.z * 1.6 && sky.sun.w > 0.2) {
        // A dithered halo.
        if (bayer(texel) < 0.35) {
            color = mix(color, vec3<f32>(1.0, 0.9, 0.7), 0.5);
        }
    }
    return vec4<f32>(color, 1.0);
}
