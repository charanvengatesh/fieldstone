// Water surfaces: the same light and texel logic as block.wgsl, blended over whatever lies
// under the surface instead of writing depth. Rows slide, the odd texel glints, and where
// the outline mask marks a shore the border texels turn to foam.
// src/debug.rs draw_water mirrors this file on the CPU for snapshots; keep the two in step.
//
// Three things happen here that make the result read as pixel art rather than as lit
// polygons: light is evaluated at the centre of each texel instead of each screen pixel,
// it is quantised into a few bands, and outlines darken whole texels of the tile.
// src/debug.rs mirrors this file on the CPU for snapshots; keep the two in step.

// Bevy's per-view globals (group 0 binding 11 in the 3D mesh pipeline). Plain WGSL cannot
// import Bevy's definition, so the fields are declared here; they must match
// bevy_render::globals::GlobalsUniform.
struct Globals {
    // seconds since start, wrapping after an hour
    time: f32,
    delta_time: f32,
    frame_count: u32,
};

@group(0) @binding(11) var<uniform> globals: Globals;

struct BlockParams {
    // y and z: atlas size in texels, w: light bands
    params: vec4<f32>,
    // x: daylight 0..1, y: time of day 0..1
    day: vec4<f32>,
    // the camera's focus (xyz) and the unit vector from it toward the eye
    focus: vec4<f32>,
    toward_eye: vec4<f32>,
    // sunlight colour now
    sun_color: vec4<f32>,
    // light left in shade and at night
    ambient_color: vec4<f32>,
    // rgb: the colour of distance (the sky), w: the horizon in blocks behind the focus
    haze: vec4<f32>,
};

// The light haze reaches HAZE_STRENGTH of the sky colour by HAZE_KNEE of the way to the
// horizon (block.haze.w, blocks behind the focus), then fog thickens to the whole sky
// colour at the horizon, where streaming stops. Keep in step with src/world/mesh.rs.
const HAZE_STRENGTH: f32 = 0.22;
const HAZE_KNEE: f32 = 0.6;

fn haze_at(world: vec3<f32>) -> f32 {
    let t = clamp(dot(block.focus.xyz - world, block.toward_eye.xyz) / max(block.haze.w, 1.0), 0.0, 1.0);
    return HAZE_STRENGTH * smoothstep(0.0, HAZE_KNEE, t) + (1.0 - HAZE_STRENGTH) * smoothstep(HAZE_KNEE, 1.0, t);
}

@group(3) @binding(0) var<uniform> block: BlockParams;
@group(3) @binding(1) var atlas: texture_2d<f32>;
@group(3) @binding(2) var atlas_sampler: sampler;

// The 3D mesh pipeline's vertex output (bevy_pbr forward_io), the fields this shader
// reads: world position is the true world position, colour is the mesh's COLOR attribute.
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(5) color: vec4<f32>,
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
    let top = (flags / 32) % 2 == 1;

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
    var color = textureSampleLevel(atlas, atlas_sampler, uv, 0.0).rgb * light + vec3<f32>(glint);

    // Foam along a shore instead of ink.
    let edge = ((mask & 1) != 0 && local.y < 0.5)
        || ((mask & 2) != 0 && local.x > TILE - 1.5)
        || ((mask & 4) != 0 && local.y > TILE - 1.5)
        || ((mask & 8) != 0 && local.x < 0.5);
    if (edge && top) {
        color = mix(color, light * 0.9, 0.7);
    }
    color = mix(color, block.haze.rgb, haze_at(in.world_position.xyz));
    return vec4<f32>(color, select(0.85, WATER_ALPHA, top));
}
