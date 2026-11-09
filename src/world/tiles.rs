//! Pixel terrain. Each terrain owns a 4x4 sheet of 16 px tiles indexed by which of a dual
//! cell's four corners are that terrain (bit 0 = bottom-left, 1 = bottom-right, 2 = top-left,
//! 3 = top-right). `tiles/<terrain>.png` in a pack replaces the generated sheet. Water terrains
//! get four animated frames.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite_render::AlphaMode2d;

use super::blend::{VERTS, Weights};
use super::chunk::{CELL, CHUNK, TILES};
use crate::art::{ShapeAssets, palette_color};
use crate::core::packs::LoadedPacks;
use crate::core::{ColorDef, Registry, TerrainDef, hash01};

pub const TILE: u32 = 16;
pub const WATER_FRAMES: usize = 4;
const FRAME_SECS: f32 = 0.35;

/// Materials per terrain: one for ground, `WATER_FRAMES` for water, plus the cliff sheet.
#[derive(Resource, Default)]
pub struct TileSheets {
    pub ground: HashMap<u16, Handle<ColorMaterial>>,
    pub water: HashMap<u16, Vec<Handle<ColorMaterial>>>,
    pub cliff: Option<Handle<ColorMaterial>>,
}

/// A chunk layer drawn with a water terrain's sheet; its material cycles through the frames.
#[derive(Component)]
pub struct WaterLayer(pub u16);

#[derive(Resource, Default)]
pub struct WaterClock(pub f32, pub usize);

pub fn build_sheets(
    assets: Res<ShapeAssets>,
    terrain: Res<Registry<TerrainDef>>,
    palette: Res<Registry<ColorDef>>,
    packs: Res<LoadedPacks>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut sheets: ResMut<TileSheets>,
) {
    if !assets.pixel() {
        return;
    }
    {
        let img = crate::art::load_png(&packs, "tiles/cliff.png").unwrap_or_else(cliff_sheet);
        sheets.cliff = Some(materials.add(ColorMaterial {
            texture: Some(images.add(img)),
            alpha_mode: AlphaMode2d::Blend,
            ..default()
        }));
    }
    for (i, t) in terrain.iter() {
        let color = palette_color(&palette, &t.color).to_srgba();
        let rgb = [color.red, color.green, color.blue];
        let make =
            |img: Image, materials: &mut Assets<ColorMaterial>, images: &mut Assets<Image>| {
                materials.add(ColorMaterial {
                    texture: Some(images.add(img)),
                    alpha_mode: AlphaMode2d::Blend,
                    ..default()
                })
            };
        if t.fish_table.is_some() {
            let frames = (0..WATER_FRAMES)
                .map(|f| {
                    let img = crate::art::load_png(&packs, &format!("tiles/{}_{f}.png", t.id))
                        .unwrap_or_else(|| sheet(rgb, i, Some(f)));
                    make(img, &mut materials, &mut images)
                })
                .collect();
            sheets.water.insert(i, frames);
        } else {
            let img = crate::art::load_png(&packs, &format!("tiles/{}.png", t.id))
                .unwrap_or_else(|| sheet(rgb, i, None));
            sheets
                .ground
                .insert(i, make(img, &mut materials, &mut images));
        }
    }
}

/// Generates the 4x4 sheet for one terrain. Edges follow the bilinear 0.5 contour of the
/// corner mask (sloped pixel edges), with a lighter rim inside the edge and a darker line
/// just outside it. Water frames move a few sparkle texels.
fn sheet(rgb: [f32; 3], terrain: u16, frame: Option<usize>) -> Image {
    let w = TILE * 4;
    let mut data = vec![0u8; (w * w * 4) as usize];
    let light = [
        (rgb[0] * 1.18 + 0.06).min(1.0),
        (rgb[1] * 1.18 + 0.06).min(1.0),
        (rgb[2] * 1.18 + 0.06).min(1.0),
    ];
    let dark = [rgb[0] * 0.72, rgb[1] * 0.72, rgb[2] * 0.72];
    for mask in 0..16u32 {
        let (col, row) = (mask % 4, mask / 4);
        let bit = |b: u32| if mask & (1 << b) != 0 { 1.0 } else { 0.0 };
        // Corner weights: bit 0 bottom-left (0,0), 1 bottom-right (1,0), 2 top-left (0,1), 3 top-right (1,1).
        let weight = |u: f32, v: f32| {
            bit(0) * (1.0 - u) * (1.0 - v)
                + bit(1) * u * (1.0 - v)
                + bit(2) * (1.0 - u) * v
                + bit(3) * u * v
        };
        let inside = |x: i32, y: i32| {
            // Texel (x, y) in tile space; y counts up from the bottom.
            if !(0..TILE as i32).contains(&x) || !(0..TILE as i32).contains(&y) {
                // Outside the tile: continue the pattern from the nearest edge.
                let cx = x.clamp(0, TILE as i32 - 1);
                let cy = y.clamp(0, TILE as i32 - 1);
                return weight(
                    (cx as f32 + 0.5) / TILE as f32,
                    (cy as f32 + 0.5) / TILE as f32,
                ) >= 0.5;
            }
            weight(
                (x as f32 + 0.5) / TILE as f32,
                (y as f32 + 0.5) / TILE as f32,
            ) >= 0.5
        };
        for y in 0..TILE as i32 {
            for x in 0..TILE as i32 {
                let full = mask == 15;
                let on = inside(x, y);
                let px = col * TILE + x as u32;
                // Image rows go top-down; our y goes bottom-up.
                let py = row * TILE + (TILE - 1 - y as u32);
                let idx = ((py * w + px) * 4) as usize;
                let world_x = px as i32;
                let world_y = py as i32;
                let grain = 0.95 + 0.1 * hash01(terrain as u32 + 31, world_x, world_y, 9);
                let mut out = [0.0f32; 4];
                if on {
                    let rim = !full
                        && (!inside(x - 1, y)
                            || !inside(x + 1, y)
                            || !inside(x, y - 1)
                            || !inside(x, y + 1));
                    let base = if rim { light } else { rgb };
                    out = [base[0] * grain, base[1] * grain, base[2] * grain, 1.0];
                    if let Some(f) = frame {
                        // Sparkles: a sparse set of texels that light up on one frame each.
                        let s = hash01(terrain as u32 + 77, world_x, world_y, 13);
                        let phase = (hash01(terrain as u32 + 78, world_x, world_y, 14)
                            * WATER_FRAMES as f32) as usize;
                        if s > 0.955 && phase == f {
                            out = [light[0] + 0.15, light[1] + 0.15, light[2] + 0.12, 1.0];
                        }
                        // Slow ripple bands: shift brightness along a diagonal per frame.
                        let band = ((world_x + world_y + (f as i32) * 2) / 3) % 4 == 0;
                        if band && !rim {
                            for ch in out.iter_mut().take(3) {
                                *ch *= 1.04;
                            }
                        }
                    }
                } else if !full
                    && (inside(x - 1, y)
                        || inside(x + 1, y)
                        || inside(x, y - 1)
                        || inside(x, y + 1))
                {
                    out = [dark[0], dark[1], dark[2], 1.0];
                }
                for k in 0..4 {
                    data[idx + k] = (out[k].clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: w,
            height: w,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::nearest();
    img
}

/// Corner mask of a terrain in a dual cell from the field's corner vertices.
fn mask_of(cell: &[Weights], terrain: u16) -> u32 {
    use super::blend::SUB;
    let corner = |sx: i32, sy: i32| cell[(sy * (SUB + 1) + sx) as usize].first().0 == terrain;
    (corner(0, 0) as u32)
        | (corner(SUB, 0) as u32) << 1
        | (corner(0, SUB) as u32) << 2
        | (corner(SUB, SUB) as u32) << 3
}

/// Textured quads for one terrain across the chunk. `base` draws every cell with the full
/// tile so the lowest layer leaves no holes. Returns `None` when the terrain is absent.
pub fn tile_mesh(
    field: &[Weights],
    terrain: u16,
    base: bool,
    height_of: &impl Fn(i32, i32) -> u8,
) -> Option<Mesh> {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut cols = Vec::new();
    let mut idx = Vec::new();
    for ty in 0..CHUNK {
        for tx in 0..CHUNK {
            let cell_i = (ty * CHUNK + tx) as usize * VERTS;
            let mask = if base {
                15
            } else {
                mask_of(&field[cell_i..cell_i + VERTS], terrain)
            };
            if mask == 0 {
                continue;
            }
            // Higher ground is a touch brighter; the dual cell takes its bottom-left tile's height.
            let h = height_of(tx, ty) as f32;
            let bright = 1.0 + 0.06 * h;
            cols.extend([[bright, bright, bright, 1.0]; 4]);
            let (col, row) = ((mask % 4) as f32, (mask / 4) as f32);
            let (u0, v0) = (col / 4.0, row / 4.0);
            let (u1, v1) = (u0 + 0.25, v0 + 0.25);
            let x0 = (tx as f32 + 0.5) * CELL;
            let y0 = (ty as f32 + 0.5) * CELL;
            let b = positions.len() as u32;
            positions.extend([
                [x0, y0, 0.0],
                [x0 + CELL, y0, 0.0],
                [x0 + CELL, y0 + CELL, 0.0],
                [x0, y0 + CELL, 0.0],
            ]);
            // Image v runs top-down: the tile's bottom edge is v1.
            uvs.extend([[u0, v1], [u1, v1], [u1, v0], [u0, v0]]);
            idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
        }
    }
    if positions.is_empty() {
        return None;
    }
    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(idx)),
    )
}

/// Terrains present in a chunk's field, lowest priority (registry index) first.
pub fn present(field: &[Weights]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    for w in field {
        let t = w.first().0;
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out.sort_unstable();
    out
}

/// Advances water frames and swaps materials on water layers.
pub fn animate_water(
    time: Res<Time>,
    sheets: Res<TileSheets>,
    mut clock: ResMut<WaterClock>,
    mut layers: Query<(&WaterLayer, &mut MeshMaterial2d<ColorMaterial>)>,
) {
    clock.0 += time.delta_secs();
    if clock.0 < FRAME_SECS {
        return;
    }
    clock.0 = 0.0;
    clock.1 = (clock.1 + 1) % WATER_FRAMES;
    for (layer, mut mat) in &mut layers {
        if let Some(frames) = sheets.water.get(&layer.0) {
            mat.0 = frames[clock.1].clone();
        }
    }
}

/// Cliff sheet: 8 tiles indexed by which of N (bit 0), E (bit 1), W (bit 2) neighbors are
/// higher. A higher north draws a rock face over the top of the cell with a lit lip; higher
/// east or west draws a shaded edge. The sheet is 4x2 tiles.
fn cliff_sheet() -> Image {
    let (w, h) = (TILE * 4, TILE * 2);
    let mut data = vec![0u8; (w * h * 4) as usize];
    let face = [0.36, 0.26, 0.18];
    let face_dark = [0.26, 0.18, 0.12];
    let lip = [0.62, 0.50, 0.34];
    let edge = [0.0, 0.0, 0.0];
    for mask in 0..8u32 {
        let (col, row) = (mask % 4, mask / 4);
        let (north, east, west) = (mask & 1 != 0, mask & 2 != 0, mask & 4 != 0);
        for y in 0..TILE {
            for x in 0..TILE {
                let px = col * TILE + x;
                let py = row * TILE + (TILE - 1 - y);
                let i = ((py * w + px) * 4) as usize;
                let mut out = [0.0f32; 4];
                // y counts up from the cell's bottom; the face occupies the top 7 texels.
                if north && y >= TILE - 7 {
                    let g = 0.92 + 0.16 * hash01(5, x as i32, y as i32 + mask as i32 * 7, 17);
                    let c = if y == TILE - 1 {
                        lip
                    } else if (x + y) % 5 == 0 {
                        face_dark
                    } else {
                        face
                    };
                    out = [c[0] * g, c[1] * g, c[2] * g, 1.0];
                    if y == TILE - 7 {
                        out = [edge[0], edge[1], edge[2], 0.55];
                    }
                }
                if east && x >= TILE - 2 && out[3] == 0.0 {
                    out = [
                        edge[0],
                        edge[1],
                        edge[2],
                        if x == TILE - 1 { 0.45 } else { 0.2 },
                    ];
                }
                if west && x < 2 && out[3] == 0.0 {
                    out = [edge[0], edge[1], edge[2], if x == 0 { 0.45 } else { 0.2 }];
                }
                for k in 0..4 {
                    data[i + k] = (out[k].clamp(0.0, 1.0) * 255.0).round() as u8;
                }
            }
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::nearest();
    img
}

/// Cliff quads for the chunk: one per cell with a higher N, E or W neighbor. Cells are the
/// real tiles here (not dual cells), so the face sits on the lower tile below a step.
pub fn cliff_mesh(height_of: &impl Fn(i32, i32) -> u8) -> Option<Mesh> {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut idx = Vec::new();
    for ty in 0..CHUNK {
        for tx in 0..CHUNK {
            let h = height_of(tx, ty);
            let mask = (height_of(tx, ty + 1) > h) as u32
                | ((height_of(tx + 1, ty) > h) as u32) << 1
                | ((height_of(tx - 1, ty) > h) as u32) << 2;
            if mask == 0 {
                continue;
            }
            let (col, row) = ((mask % 4) as f32, (mask / 4) as f32);
            let (u0, v0) = (col / 4.0, row / 2.0);
            let (u1, v1) = (u0 + 0.25, v0 + 0.5);
            let x0 = tx as f32 * CELL;
            let y0 = ty as f32 * CELL;
            let b = positions.len() as u32;
            positions.extend([
                [x0, y0, 0.0],
                [x0 + CELL, y0, 0.0],
                [x0 + CELL, y0 + CELL, 0.0],
                [x0, y0 + CELL, 0.0],
            ]);
            uvs.extend([[u0, v1], [u1, v1], [u1, v0], [u0, v0]]);
            idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
        }
    }
    if positions.is_empty() {
        return None;
    }
    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(idx)),
    )
}

#[allow(dead_code)]
const _: usize = TILES;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_cover_all_sixteen_tiles() {
        let img = sheet([0.3, 0.6, 0.2], 0, None);
        assert_eq!(img.size(), UVec2::splat(TILE * 4));
        // Full tile (mask 15, bottom-right of the sheet) is entirely opaque; mask 0 is empty.
        let full = img.get_color_at(3 * TILE + 8, 3 * TILE + 8).unwrap();
        let empty = img.get_color_at(8, 8).unwrap();
        assert!(full.alpha() > 0.99);
        assert!(empty.alpha() < 0.01);
    }
}
