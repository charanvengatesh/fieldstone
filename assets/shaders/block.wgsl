// Block faces. The texture is the tile atlas; the vertex colour carries light in rgb and,
// in alpha, the face's outline mask and water flag (see pack_flags in src/world/mesh.rs).
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

fn hash(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
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

    // Light at the texel's centre: step from this pixel to the centre in screen space
    // using the inverse of the uv derivative matrix.
    var light = in.color.rgb;
    let to_centre = (texel + vec2<f32>(0.5) - px) / size;
    let det = uv_dx.x * uv_dy.y - uv_dx.y * uv_dy.x;
    if (abs(det) > 1e-14) {
        let d = vec2<f32>(
            (to_centre.x * uv_dy.y - to_centre.y * uv_dy.x) / det,
            (uv_dx.x * to_centre.y - uv_dx.y * to_centre.x) / det,
        );
        light = light + light_dx * d.x + light_dy * d.y;
    }
    light = max(light, vec3<f32>(0.0));

    // Bands: quantise brightness, keep the tint.
    let brightness = max(light.r, max(light.g, light.b));
    if (brightness > 1e-4) {
        let banded = max(round(brightness * levels), 1.0) / levels;
        light = light * (banded / brightness);
    }

    let flags = i32(round(in.color.a * 32.0));
    let mask = flags % 16;
    let water = flags >= 16;

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

    // Outline: the border texels of the tile on the sides the mask names.
    let edge = ((mask & 1) != 0 && local.y < 0.5)
        || ((mask & 2) != 0 && local.x > TILE - 1.5)
        || ((mask & 4) != 0 && local.y > TILE - 1.5)
        || ((mask & 8) != 0 && local.x < 0.5);
    if (edge) {
        color = color * vec3<f32>(0.50, 0.50, 0.58);
    }
    return vec4<f32>(color, 1.0);
}
