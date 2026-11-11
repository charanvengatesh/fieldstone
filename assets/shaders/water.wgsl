// Water surfaces: the same light and texel logic as block.wgsl, blended over whatever lies
// under the surface instead of writing depth. Rows slide, the odd texel glints, and where
// the outline mask marks a shore the border texels turn to foam.
// src/debug.rs draw_water mirrors this file on the CPU for snapshots; keep the two in step.
//
// Three things happen here that make the result read as pixel art rather than as lit
// polygons: light is evaluated at the centre of each texel instead of each screen pixel,
// it is quantised into a few bands, and outlines darken whole texels of the tile. Blocks
// in front of the player are dithered away so the player stays in view.
// src/debug.rs mirrors this file on the CPU for snapshots; keep the two in step.

struct BlockParams {
    // x: seconds, y and z: atlas size in texels, w: light bands
    params: vec4<f32>,
    // the player on screen: x, y texels, z depth, w see-through radius (0 = off)
    player: vec4<f32>,
    // x: screen pixels per texel
    screen: vec4<f32>,
    // x: daylight 0..1, y: time of day 0..1, z: cloud drift in blocks
    day: vec4<f32>,
    // sunlight colour now
    sun_color: vec4<f32>,
    // light left in shade and at night
    ambient_color: vec4<f32>,
    // inverse of the view projection: screen texels and depth back to world blocks
    inverse: mat4x4<f32>,
};

@group(2) @binding(0) var<uniform> block: BlockParams;
@group(2) @binding(1) var atlas: texture_2d<f32>;
@group(2) @binding(2) var atlas_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) @interpolate(flat) instance_index: u32,
};

const TILE: f32 = 16.0;
const SHADE_TINT: vec3<f32> = vec3<f32>(0.80, 0.88, 1.08);
const SUN_TINT: vec3<f32> = vec3<f32>(1.06, 1.0, 0.90);
const TORCH: vec3<f32> = vec3<f32>(1.0, 0.80, 0.55);
const WATER_ALPHA: f32 = 0.66;

fn hash(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
}

fn hash2(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(fract(sin(h) * 43758.547) + 1.0);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2<f32>(1.0, 0.0));
    let c = hash2(i + vec2<f32>(0.0, 1.0));
    let d = hash2(i + vec2<f32>(1.0, 1.0));
    let x1 = a + (b - a) * u.x;
    let x2 = c + (d - c) * u.x;
    return x1 + (x2 - x1) * u.y;
}

// Cloud shadow over a world point: 0.88 under a cloud, 1 elsewhere, none at night.
fn cloud_cover(world: vec2<f32>, drift: f32, daylight: f32) -> f32 {
    if (daylight < 0.5) {
        return 1.0;
    }
    let p = world / 40.0 + vec2<f32>(drift, drift * 0.4) / 40.0;
    let n = value_noise(p) * 0.65 + value_noise(p * 2.1 + vec2<f32>(7.3)) * 0.35;
    return select(1.0, 0.88, n > 0.56);
}

// Sunlight is warm where it is strong and cool in shade.
fn tint(light: f32) -> vec3<f32> {
    var t = clamp((light - 0.45) / 0.55, 0.0, 1.0);
    t = t * t * (3.0 - 2.0 * t);
    return mix(SHADE_TINT, SUN_TINT, t) * light;
}

// Mixes the light channels (sun, block light, ambient) by the time of day.
fn compose(channels: vec3<f32>, cloud: f32) -> vec3<f32> {
    let level = block.day.x;
    let sun = tint(channels.x) * block.sun_color.rgb * level * cloud;
    let ambient = block.ambient_color.rgb * channels.z * (0.22 * (1.0 - level));
    let torch = TORCH * channels.y * 1.15;
    return sun + ambient + torch;
}

// 4 x 4 ordered dither threshold in 0..1 for a texel at integer coordinates.
fn bayer(t: vec2<f32>) -> f32 {
    var table = array<f32, 16>(
        0.0, 8.0, 2.0, 10.0,
        12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0,
        15.0, 7.0, 13.0, 5.0,
    );
    let x = i32(t.x) % 4;
    let y = i32(t.y) % 4;
    let i = (x + 4) % 4 + ((y + 4) % 4) * 4;
    return (table[i] + 0.5) / 16.0;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = block.params.yz;
    let levels = block.params.w;
    let t = block.params.x;

    // Derivatives first: they must be taken in uniform control flow.
    let uv_dx = dpdx(in.uv);
    let uv_dy = dpdy(in.uv);
    let light_dx = dpdx(in.color.rgb);
    let light_dy = dpdy(in.color.rgb);

    // See-through: texels nearer than the player and within an ellipse around them are
    // dropped through a dither that thins toward the edge.
    if (block.player.w > 0.0 && in.world_position.z > block.player.z + 1.3) {
        let d = (in.world_position.xy - block.player.xy) / vec2<f32>(block.player.w, block.player.w * 0.75);
        let r = length(d);
        if (r < 1.0) {
            let cover = smoothstep(1.0, 0.3, r);
            let texel_on_screen = floor(in.position.xy / max(block.screen.x, 1.0));
            if (bayer(texel_on_screen) < cover) {
                discard;
            }
        }
    }

    let px = in.uv * size;
    let texel = floor(px);
    let cell = floor(texel / TILE) * TILE;
    let local = texel - cell;

    // Light channels at the texel's centre: step from this pixel to the centre in screen
    // space using the inverse of the uv derivative matrix.
    var channels = in.color.rgb;
    let to_centre = (texel + vec2<f32>(0.5) - px) / size;
    let det = uv_dx.x * uv_dy.y - uv_dx.y * uv_dy.x;
    if (abs(det) > 1e-14) {
        let d = vec2<f32>(
            (to_centre.x * uv_dy.y - to_centre.y * uv_dy.x) / det,
            (uv_dx.x * to_centre.y - uv_dx.y * to_centre.x) / det,
        );
        channels = channels + light_dx * d.x + light_dy * d.y;
    }
    channels = max(channels, vec3<f32>(0.0));

    let flags = i32(round(in.color.a * 128.0));
    let mask = flags % 16;
    let water = (flags / 16) % 2 == 1;
    let top = (flags / 32) % 2 == 1;

    // Clouds pass over the tops of things, by world position.
    let world = (block.inverse * vec4<f32>(in.world_position.xyz, 0.0)).xyz;
    var cloud = 1.0;
    if (top) {
        cloud = cloud_cover(world.xy, block.day.z, block.day.x);
    }
    var light = compose(channels, cloud);

    // Bands: quantise brightness, keep the tint.
    let brightness = max(light.r, max(light.g, light.b));
    if (brightness > 1e-4) {
        let banded = max(round(brightness * levels), 1.0) / levels;
        light = light * (banded / brightness);
    }

    // Water: each row of the tile slides back and forth a texel, and the odd texel glints.
    var sample_at = local;
    var glint = 0.0;
    if (water) {
        let slide = floor(sin(t * 1.3 + local.y * 0.9) * 1.5 + 0.5);
        sample_at.x = (local.x + slide + TILE) % TILE;
        let spot = floor(in.world_position.xy) + vec2<f32>(floor(t * 2.0) * 17.0, 0.0);
        glint = step(0.997, hash(spot)) * 0.26;
    }

    let uv = (cell + sample_at + vec2<f32>(0.5)) / size;
    var color = textureSampleLevel(atlas, atlas_sampler, uv, 0.0).rgb * light + vec3<f32>(glint);

    // Foam along a shore instead of ink.
    let edge = ((mask & 1) != 0 && local.y < 0.5)
        || ((mask & 2) != 0 && local.x > TILE - 1.5)
        || ((mask & 4) != 0 && local.y > TILE - 1.5)
        || ((mask & 8) != 0 && local.x < 0.5);
    if (edge && top) {
        color = mix(color, light * 0.9, 0.7);
    }
    return vec4<f32>(color, select(0.85, WATER_ALPHA, top));
}
