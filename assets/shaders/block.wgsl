// Block faces. The texture is the tile atlas; block_vertex.wgsl unpacks each vertex into
// light channels (sun, block light, ambient) that are mixed by the time of day here, and
// the face's outline mask, water, top and submerged flags (flag_bits in src/world/mesh.rs).
// Water surfaces are drawn by water.wgsl, which shares the light and texel logic here.
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

// What block_vertex.wgsl hands over: the world position, face-local cell coordinates,
// the light channels (sun, block light, ambient), the face's flags (flag_bits in
// src/world/mesh.rs) and its tile, face number and extent in cells.
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) face_local: vec2<f32>,
    @location(2) light: vec3<f32>,
    @location(3) @interpolate(flat) flags: u32,
    @location(4) @interpolate(flat) info: vec4<u32>,
};

const COLS: u32 = 16u;

// src/core/mod.rs hash01, bit for bit: pure u32 arithmetic.
fn hash01(seed: u32, x: i32, y: i32, salt: u32) -> f32 {
    var h = seed ^ 0x9E3779B9u;
    var vs = array<u32, 3>(bitcast<u32>(x), bitcast<u32>(y), salt);
    for (var i = 0; i < 3; i++) {
        h ^= (vs[i] + 0x7F4A7C15u) * 0x8532EBF5u;
        h = (h ^ (h >> 15u)) * 0x2C1B3C6Du;
        h ^= h >> 12u;
    }
    return f32(h >> 8u) / 16777216.0;
}

fn face_normal(face: u32) -> vec3<f32> {
    switch face {
        case 1u: { return vec3<f32>(1.0, 0.0, 0.0); }
        case 2u: { return vec3<f32>(0.0, 1.0, 0.0); }
        case 3u: { return vec3<f32>(-1.0, 0.0, 0.0); }
        case 4u: { return vec3<f32>(0.0, -1.0, 0.0); }
        default: { return vec3<f32>(0.0, 0.0, 1.0); }
    }
}

// The atlas texel a geometric texel samples for a tile turned rot quarter turns or
// mirrored (mesh.rs rotate_texel).
fn rotate_texel(g: vec2<f32>, rot: u32, flip: bool) -> vec2<f32> {
    let n = TILE - 1.0;
    if (flip) { return vec2<f32>(n - g.x, g.y); }
    switch rot {
        case 1u: { return vec2<f32>(n - g.y, g.x); }
        case 2u: { return vec2<f32>(n - g.x, n - g.y); }
        case 3u: { return vec2<f32>(g.y, n - g.x); }
        default: { return g; }
    }
}

// The edge bits that apply to one cell of a quad: a quad's edges land on its border cells
// only; the grid inks every cell border (mesh.rs border_edges).
fn border_edges(mask: i32, cell: vec2<f32>, extent: vec2<f32>, grid: bool) -> i32 {
    if (grid) { return 15; }
    var m = mask & 15;
    if (cell.y > 0.5) { m &= ~1; }
    if (cell.x < extent.x - 1.5) { m &= ~2; }
    if (cell.y < extent.y - 1.5) { m &= ~4; }
    if (cell.x > 0.5) { m &= ~8; }
    return m;
}

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
    let fl_dx = dpdx(in.face_local);
    let fl_dy = dpdy(in.face_local);
    let light_dx = dpdx(in.light);
    let light_dy = dpdy(in.light);

    let tile_index = in.info.x;
    let face = in.info.y;
    let extent = vec2<f32>(f32(in.info.z), f32(in.info.w));
    let fl = clamp(in.face_local, vec2<f32>(0.0), extent - vec2<f32>(1e-4));
    let cellf = floor(fl);
    // The geometric texel within the cell.
    let local = clamp(floor((fl - cellf) * TILE), vec2<f32>(0.0), vec2<f32>(TILE - 1.0));

    // Light channels at the texel's centre: step from this pixel to the centre in screen
    // space using the inverse of the face-local derivative matrix.
    var channels = in.light;
    let to_centre = cellf + (local + vec2<f32>(0.5)) / TILE - in.face_local;
    let det = fl_dx.x * fl_dy.y - fl_dx.y * fl_dy.x;
    if (abs(det) > 1e-14) {
        let d = vec2<f32>(
            (to_centre.x * fl_dy.y - to_centre.y * fl_dy.x) / det,
            (fl_dx.x * to_centre.y - fl_dx.y * to_centre.x) / det,
        );
        channels = channels + light_dx * d.x + light_dy * d.y;
    }
    channels = max(channels, vec3<f32>(0.0));

    let flags = i32(in.flags);
    let grid = (flags / 2048) % 2 == 1;
    let mask = border_edges(flags % 16, cellf, extent, grid);
    let water = (flags / 16) % 2 == 1;
    let submerged = (flags / 64) % 2 == 1;
    let drop = border_edges((flags / 128) % 16, cellf, extent, false);

    var light = compose(channels);

    // Bands: quantise brightness, keep the tint.
    let brightness = max(light.r, max(light.g, light.b));
    if (brightness > 1e-4) {
        let banded = max(round(brightness * levels), 1.0) / levels;
        light = light * (banded / brightness);
    }

    // Which way this block's tile is turned (mesh.rs tile_turn): tops by quarters, sides
    // mirrored, from a hash of the block; water and cross props not at all.
    let block_cell = floor(in.world_position.xyz - 0.5 * face_normal(face));
    var rot = 0u;
    var flip = false;
    if (!water && face != 5u) {
        let pick = hash01(0x5EEDu, i32(block_cell.x) + i32(block_cell.z) * 131, i32(block_cell.y), tile_index % 2u);
        if (face == 0u) {
            rot = u32(pick * 4.0) % 4u;
        } else {
            flip = pick < 0.5;
        }
    }
    var sample_at = rotate_texel(local, rot, flip);
    // Water: each row of the tile slides back and forth a texel, and the odd texel glints.
    var glint = 0.0;
    if (water) {
        let slide = floor(sin(t * 1.3 + sample_at.y * 0.9) * 1.5 + 0.5);
        sample_at.x = (sample_at.x + slide + TILE) % TILE;
        let spot = floor(in.world_position.xy) + vec2<f32>(floor(t * 2.0) * 17.0, 0.0);
        glint = step(0.997, hash(spot)) * 0.26;
    }

    let tile_origin = vec2<f32>(f32((tile_index % COLS) * 16u), f32((tile_index / COLS) * 16u));
    let uv = (tile_origin + sample_at + vec2<f32>(0.5)) / size;
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
    color = mix(color, block.haze.rgb, haze_at(in.world_position.xyz));
    return vec4<f32>(color, 1.0);
}
