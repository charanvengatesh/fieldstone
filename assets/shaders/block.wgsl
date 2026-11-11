// Block faces. The texture is the tile atlas; the vertex colour carries light as channels
// (r sun, g block light, b ambient) that are mixed by the time of day here, and in alpha
// the face's outline mask, water, top and submerged flags (pack_flags in src/world/mesh.rs).
// Water surfaces are drawn by water.wgsl, which shares the light and texel logic here.
//
// Three things happen here that make the result read as pixel art rather than as lit
// polygons: light is evaluated at the centre of each texel instead of each screen pixel,
// it is quantised into a few bands, and outlines darken whole texels of the tile.
// src/debug.rs mirrors this file on the CPU for snapshots; keep the two in step.

// Bevy's per-view globals (group 0 binding 1 in every 2D mesh pipeline). Plain WGSL
// cannot import Bevy's definition, so the fields are declared here; they must match
// bevy_render::globals::GlobalsUniform.
struct Globals {
    // seconds since start, wrapping after an hour
    time: f32,
    delta_time: f32,
    frame_count: u32,
};

@group(0) @binding(1) var<uniform> globals: Globals;

struct BlockParams {
    // y and z: atlas size in texels, w: light bands
    params: vec4<f32>,
    // x: daylight 0..1, y: time of day 0..1, z: the camera's focus depth
    day: vec4<f32>,
    // sunlight colour now
    sun_color: vec4<f32>,
    // light left in shade and at night
    ambient_color: vec4<f32>,
    // rgb: the colour of distance (the sky), w: its full strength
    haze: vec4<f32>,
};

// Depth behind the focus over which the haze reaches full strength; keep in step with
// HAZE_REACH in src/world/mesh.rs.
const HAZE_REACH: f32 = 28.0;

// How much haze a fragment at projected depth `depth` gets.
fn haze_at(depth: f32) -> f32 {
    return clamp((block.day.z - depth) / HAZE_REACH, 0.0, 1.0);
}

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
const SUBMERGED: vec3<f32> = vec3<f32>(0.50, 0.68, 0.92);

fn hash(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
}

// Sunlight is warm where it is strong and cool in shade.
fn tint(light: f32) -> vec3<f32> {
    var t = clamp((light - 0.45) / 0.55, 0.0, 1.0);
    t = t * t * (3.0 - 2.0 * t);
    return mix(SHADE_TINT, SUN_TINT, t) * light;
}

// Mixes the light channels (sun, block light, ambient) by the time of day.
fn compose(channels: vec3<f32>) -> vec3<f32> {
    let level = block.day.x;
    let sun = tint(channels.x) * block.sun_color.rgb * level;
    let ambient = block.ambient_color.rgb * channels.z * (0.22 * (1.0 - level));
    let torch = TORCH * channels.y * 1.15;
    return sun + ambient + torch;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = block.params.yz;
    let levels = block.params.w;
    let t = globals.time;

    // Derivatives first: they must be taken in uniform control flow.
    let uv_dx = dpdx(in.uv);
    let uv_dy = dpdy(in.uv);
    let light_dx = dpdx(in.color.rgb);
    let light_dy = dpdy(in.color.rgb);

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

    let flags = i32(round(in.color.a * 2048.0));
    let mask = flags % 16;
    let water = (flags / 16) % 2 == 1;
    let submerged = (flags / 64) % 2 == 1;
    let drop = (flags / 128) % 16;

    var light = compose(channels);

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
    let texel_color = textureSampleLevel(atlas, atlas_sampler, uv, 0.0);
    // Holes in cutout tiles: leaves, glass, props.
    if (texel_color.a < 0.5) {
        discard;
    }
    var color = texel_color.rgb * light + vec3<f32>(glint);
    if (submerged) {
        color = color * SUBMERGED;
    }

    // Outline: the border texels of the tile on the sides the mask names. Where the
    // ground falls away past an edge the ink is two texels deep and darker.
    let edge = ((mask & 1) != 0 && local.y < 0.5)
        || ((mask & 2) != 0 && local.x > TILE - 1.5)
        || ((mask & 4) != 0 && local.y > TILE - 1.5)
        || ((mask & 8) != 0 && local.x < 0.5);
    let drop_edge = ((drop & 1) != 0 && local.y < 1.5)
        || ((drop & 2) != 0 && local.x > TILE - 2.5)
        || ((drop & 4) != 0 && local.y > TILE - 2.5)
        || ((drop & 8) != 0 && local.x < 1.5);
    if (drop_edge) {
        color = color * vec3<f32>(0.38, 0.38, 0.46);
    } else if (edge) {
        color = color * vec3<f32>(0.50, 0.50, 0.58);
    }
    // Distance fades toward the sky.
    color = mix(color, block.haze.rgb, haze_at(in.world_position.z) * block.haze.w);
    return vec4<f32>(color, 1.0);
}
