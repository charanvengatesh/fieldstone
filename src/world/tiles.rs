//! Pixel-art tiles. Every block gets two 16 x 16 tiles, one for its top and one for its
//! sides, drawn from its colours by its [`Pattern`] with a small hue-shifted ramp: shadows
//! lean cool, highlights lean warm. Tiles wrap seamlessly. A pack may replace either with a
//! PNG. All tiles live in one atlas texture.

use bevy::asset::RenderAssetUsages;
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::core::packs::LoadedPacks;
use crate::core::{BlockDef, Pattern, Registry, Rgb, Shape, hash01};

/// Texels per side of a tile.
pub const TILE: u32 = 16;
/// Tiles per atlas row.
const COLS: u32 = 16;
const T: i32 = TILE as i32;
/// Noise seed shared by every rock-like tile.
const STONE_SEED: u32 = 0x0005_700E;

/// One tile, row-major. Row 0 is `v = 0`: for side tiles that is the bottom of the face.
pub type Tile = Vec<[u8; 4]>;

/// Where each block's tiles sit in the atlas, and what the mesher needs to know per block.
#[derive(Resource, Clone)]
pub struct Tiles {
    /// Atlas size in texels.
    pub size: UVec2,
    /// Per block: whether the shader animates it.
    pub water: Vec<bool>,
    /// Per block: minimum brightness.
    pub glow: Vec<f32>,
    /// Per block: light given off, 0 to 15.
    pub light: Vec<u8>,
    /// Per block: whether light and sight pass through it (air, water, cutouts, props).
    pub passes: Vec<bool>,
    /// Per block: drawn as two crossed upright quads instead of a cube.
    pub cross: Vec<bool>,
    /// Per block: the faces behind it are drawn too (water, cutouts, props).
    pub see_through: Vec<bool>,
    /// Per block: blocks with the same base and top colour share a number, and no outline
    /// is drawn between them (ore set in stone).
    pub family: Vec<u16>,
    /// The atlas pixels, kept on the CPU for software snapshots and item icons.
    pub image: Image,
}

impl Tiles {
    /// Texel origin of a block's tile: `face` 0 is the top, 1 the sides.
    pub fn origin(&self, block: u16, face: u32) -> Vec2 {
        let i = block as u32 * 2 + face;
        Vec2::new(((i % COLS) * TILE) as f32, ((i / COLS) * TILE) as f32)
    }
}

/// A colour and how many steps lighter or darker than it to draw.
#[derive(Clone, Copy)]
struct Px {
    color: Rgb,
    step: i32,
}

/// A `step` that means "no texel here": the tile is see-through at that spot.
const HOLE: i32 = i32::MIN;

/// Moves a colour along its ramp: negative steps darken and cool, positive lighten and warm.
pub fn ramp(c: Rgb, step: i32) -> Rgb {
    let step = step.clamp(-2, 2);
    let f = [0.60, 0.80, 1.0, 1.14, 1.28][(step + 2) as usize];
    let (mut r, mut g, mut b) = (c[0] as f32 * f, c[1] as f32 * f, c[2] as f32 * f);
    if step < 0 {
        r -= 5.0 * -step as f32;
        b += 10.0 * -step as f32;
    } else if step > 0 {
        r += 7.0 * step as f32;
        g += 5.0 * step as f32;
        b -= 4.0 * step as f32;
    }
    [r, g, b].map(|v| v.clamp(0.0, 255.0).round() as u8)
}

/// Draws a block's top and side tiles from its pattern.
pub fn draw(def: &BlockDef) -> (Tile, Tile) {
    // Each block gets its own noise so two blocks with one pattern do not share speckles.
    let seed = def.id.bytes().fold(0x811C_9DC5u32, |h, c| {
        (h ^ c as u32).wrapping_mul(0x0100_0193)
    });
    let h = |x: i32, y: i32, salt: u32| hash01(seed, x.rem_euclid(T), y.rem_euclid(T), salt);
    let base = def.color;
    let top = def.top.unwrap_or(base);
    let accent = def.accent.unwrap_or(ramp(base, 2));
    let px = |color: Rgb, step: i32| Px { color, step };
    let hole = Px {
        color: base,
        step: HOLE,
    };
    // A prop is drawn on the side tile (upright, row 0 at the ground); its top tile is the
    // same picture, for the item icon.
    let within = |x: i32, lo: i32, hi: i32| x >= lo && x <= hi;

    let grain = |c: Rgb, x: i32, y: i32| {
        let r = h(x, y, 1);
        let clump = h(x / 2, y / 2, 2);
        px(
            c,
            if r < 0.07 || clump < 0.05 {
                -1
            } else if r > 0.95 {
                1
            } else {
                0
            },
        )
    };
    let stone = |c: Rgb, x: i32, y: i32| {
        // Broad, quiet blotches with ragged edges, and the odd dark fleck.
        // One shared seed, so ore tiles carry the same rock as the stone around them.
        let hs =
            |x: i32, y: i32, salt: u32| hash01(STONE_SEED, x.rem_euclid(T), y.rem_euclid(T), salt);
        let jx = (x + (hs(x, y, 4) * 2.0) as i32) / 4;
        let jy = (y + (hs(y, x, 5) * 2.0) as i32) / 4;
        let blotch = hash01(STONE_SEED, jx.rem_euclid(4), jy.rem_euclid(4), 3);
        px(
            c,
            if hs(x, y, 6) < 0.012 {
                -2
            } else if blotch < 0.22 {
                -1
            } else if blotch > 0.90 {
                1
            } else {
                0
            },
        )
    };

    let paint = |side: bool| -> Tile {
        let mut tile = Vec::with_capacity((TILE * TILE) as usize);
        for y in 0..T {
            for x in 0..T {
                let p = match def.pattern {
                    Pattern::Smooth => px(if side { base } else { top }, 0),
                    Pattern::Grain => grain(if side { base } else { top }, x, y),
                    Pattern::Stone => stone(if side { base } else { top }, x, y),
                    Pattern::Sand => {
                        let r = h(x, y, 1);
                        let ripple = (x + y * 2).rem_euclid(8) == 0 && r > 0.55;
                        px(
                            if side { base } else { top },
                            if ripple || r > 0.95 {
                                1
                            } else if r < 0.05 {
                                -1
                            } else {
                                0
                            },
                        )
                    }
                    Pattern::Snow => {
                        let r = h(x, y, 1);
                        let drift = h(x / 3, y / 2, 2);
                        px(
                            if side { base } else { top },
                            if drift < 0.05 || r < 0.015 {
                                -1
                            } else if r > 0.97 {
                                1
                            } else {
                                0
                            },
                        )
                    }
                    Pattern::Grass if !side => {
                        let r = h(x, y, 1);
                        // Sparse blades: a light texel with a dark one beneath it.
                        let blade = h(x, y, 8) > 0.945;
                        let under_blade = h(x, y + 1, 8) > 0.945;
                        px(
                            top,
                            if blade {
                                1
                            } else if under_blade || r < 0.04 {
                                -1
                            } else {
                                0
                            },
                        )
                    }
                    Pattern::Grass => {
                        // Soil with a ragged lip of grass hanging from the top edge.
                        let depth = 3 + (h(x, 0, 7) * 3.0) as i32;
                        let from_top = T - 1 - y;
                        if from_top < depth - 1 {
                            px(top, if from_top == 0 { 1 } else { 0 })
                        } else if from_top == depth - 1 {
                            px(top, -1)
                        } else if from_top == depth {
                            px(base, -1)
                        } else {
                            grain(base, x, y)
                        }
                    }
                    Pattern::Leaves if def.cutout && h(x / 2, y / 2, 3) < 0.10 => hole,
                    Pattern::Leaves => {
                        let clump = h(x / 2, y / 2, 3);
                        let r = h(x, y, 1);
                        let step = if clump < 0.10 {
                            -2
                        } else if clump < 0.34 {
                            -1
                        } else if clump > 0.80 {
                            1
                        } else {
                            0
                        };
                        px(base, if r < 0.10 { step - 1 } else { step })
                    }
                    Pattern::Log if !side => {
                        let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
                        let d = (dx * dx + dy * dy).sqrt();
                        if d > 7.2 {
                            px(base, -1)
                        } else if d < 1.2 {
                            px(top, 1)
                        } else {
                            px(top, if (d as i32) % 2 == 0 { -1 } else { 0 })
                        }
                    }
                    Pattern::Log => {
                        // Bark: long vertical runs, broken here and there.
                        let col = h(x, 0, 5);
                        let broken = h(x, y / 5, 6) < 0.16;
                        let step = if col < 0.30 {
                            -1
                        } else if col > 0.82 {
                            1
                        } else {
                            0
                        };
                        px(base, if broken { step - 1 } else { step })
                    }
                    Pattern::Planks => {
                        let c = if side { base } else { top };
                        let row = y / 4;
                        let joint = (row * 7 + 3).rem_euclid(T);
                        if y % 4 == 0 {
                            px(c, -2)
                        } else if x == joint {
                            px(c, -1)
                        } else if y % 4 == 3 && h(x / 2, row, 2) > 0.5 {
                            px(c, 1)
                        } else {
                            px(c, if h(x / 3, row, 4) < 0.22 { -1 } else { 0 })
                        }
                    }
                    Pattern::Bricks => {
                        let c = if side { base } else { top };
                        let row = y / 4;
                        let shifted = x + (row % 2) * 4;
                        if y % 4 == 0 || shifted.rem_euclid(8) == 0 {
                            px(accent, 0)
                        } else {
                            let brick = hash01(seed, shifted.rem_euclid(T) / 8, row, 9);
                            px(
                                c,
                                if brick < 0.3 {
                                    -1
                                } else if brick > 0.8 {
                                    1
                                } else if y % 4 == 1 {
                                    -1
                                } else {
                                    0
                                },
                            )
                        }
                    }
                    Pattern::Ore => {
                        // Three nuggets: a 2 x 2 lump of the accent with a highlight, set
                        // in a rim of darker stone so it reads at a glance.
                        let c = if side { base } else { top };
                        let mut hit = None;
                        for i in 0..3 {
                            let cx = (hash01(seed, i, 0, 11) * T as f32) as i32;
                            let cy = (hash01(seed, i, 1, 11) * T as f32) as i32;
                            let wrap = |d: i32| (d + T / 2).rem_euclid(T) - T / 2;
                            let (dx, dy) = (wrap(x - cx), wrap(y - cy));
                            let inside = (0..2).contains(&dx) && (0..2).contains(&dy);
                            let rim = (-1..3).contains(&dx)
                                && (-1..3).contains(&dy)
                                && !((dx == -1 || dx == 2) && (dy == -1 || dy == 2));
                            if inside {
                                hit = Some(px(accent, if dx == 0 && dy == 1 { 1 } else { 0 }));
                            } else if rim && hit.is_none() {
                                hit = Some(px(c, -2));
                            }
                        }
                        hit.unwrap_or_else(|| stone(c, x, y))
                    }
                    Pattern::Water => {
                        let c = if side { base } else { top };
                        if y % 4 == 1 && h(x / 3, y, 2) > 0.55 {
                            px(c, 1)
                        } else if y % 4 == 3 && h(x / 4, y, 3) > 0.70 {
                            px(c, -1)
                        } else {
                            px(c, 0)
                        }
                    }
                    Pattern::Glass => {
                        let rim = x == 0 || y == 0 || x == T - 1 || y == T - 1;
                        if rim {
                            px(base, if x == 0 || y == T - 1 { 1 } else { 0 })
                        } else if (x == 3 && within(y, 10, 12)) || (x == 4 && y == 13) {
                            px(base, 2)
                        } else {
                            hole
                        }
                    }
                    Pattern::Tuft => {
                        // Five blades of different heights, leaning apart at the tips.
                        let blades = [(2, 9), (5, 12), (8, 10), (11, 13), (14, 8)];
                        let mut p = hole;
                        for (i, (bx, height)) in blades.iter().enumerate() {
                            let lean = if y > height / 2 {
                                (i as i32 % 3) - 1
                            } else {
                                0
                            };
                            if y < *height && x == bx + lean {
                                p = px(base, if y >= height - 2 { 1 } else { 0 });
                            }
                        }
                        p
                    }
                    Pattern::Flower => {
                        let bloom = within(x, 6, 9) && within(y, 8, 11);
                        let petal_edge = (x == 6 || x == 9) && (y == 8 || y == 11);
                        if bloom && !petal_edge {
                            px(accent, if x == 7 && y == 10 { 2 } else { 0 })
                        } else if x == 7 && y < 8 {
                            px(base, 0)
                        } else if (within(x, 5, 6) && y == 3) || (within(x, 8, 9) && y == 5) {
                            px(base, 1)
                        } else {
                            hole
                        }
                    }
                    Pattern::Mushroom => {
                        let cap = (y == 5 && within(x, 3, 11))
                            || (within(y, 6, 7) && within(x, 2, 12))
                            || (y == 8 && within(x, 3, 11))
                            || (y == 9 && within(x, 5, 9));
                        if cap {
                            let spot =
                                (x == 4 && y == 7) || (x == 9 && y == 8) || (x == 7 && y == 6);
                            px(if spot { accent } else { top }, if y >= 8 { 1 } else { 0 })
                        } else if within(x, 6, 8) && y < 5 {
                            px(base, if x == 6 { -1 } else { 0 })
                        } else {
                            hole
                        }
                    }
                    Pattern::Pebbles => {
                        let stones = [(3, 1, 2), (9, 1, 3), (13, 1, 1)];
                        let mut p = hole;
                        for (cx, cy, r) in stones {
                            let (dx, dy) = (x - cx, (y - cy) * 2);
                            if dx * dx + dy * dy <= r * r + 1 {
                                p = px(
                                    base,
                                    if dy < 0 && dx <= 0 {
                                        1
                                    } else if dy > 0 {
                                        -1
                                    } else {
                                        0
                                    },
                                );
                            }
                        }
                        p
                    }
                    Pattern::Torch => {
                        if within(x, 7, 8) && y < 10 {
                            px(base, if x == 7 { 0 } else { -1 })
                        } else if (within(x, 6, 9) && within(y, 10, 12))
                            || (within(x, 7, 8) && y == 13)
                            || (x == 7 && y == 14)
                        {
                            px(
                                top,
                                if within(x, 7, 8) && within(y, 10, 11) {
                                    2
                                } else {
                                    0
                                },
                            )
                        } else {
                            hole
                        }
                    }
                };
                if p.step == HOLE {
                    tile.push([0, 0, 0, 0]);
                    continue;
                }
                let c = ramp(p.color, p.step);
                tile.push([c[0], c[1], c[2], 255]);
            }
        }
        tile
    };
    (paint(false), paint(true))
}

/// Reads a 16 x 16 PNG tile from the last pack that ships it. Side tiles are stored bottom
/// row first, so an image drawn the usual way up is flipped.
fn png_tile(packs: &LoadedPacks, rel: &str, side: bool) -> Option<Tile> {
    for (dir, _) in packs.0.iter().rev() {
        let path = dir.join(rel);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let img = match Image::from_buffer(
            &bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::nearest(),
            RenderAssetUsages::default(),
        ) {
            Ok(img) => img,
            Err(e) => {
                error!("{}: {e}", path.display());
                continue;
            }
        };
        let data = img.data.as_ref()?;
        if img.width() != TILE || img.height() != TILE || data.len() != (TILE * TILE * 4) as usize {
            error!(
                "{}: block textures must be {TILE} x {TILE} RGBA",
                path.display()
            );
            continue;
        }
        let mut tile: Tile = data.chunks(4).map(|p| [p[0], p[1], p[2], 255]).collect();
        if side {
            let rows: Vec<Vec<[u8; 4]>> = tile.chunks(TILE as usize).map(|r| r.to_vec()).collect();
            tile = rows.into_iter().rev().flatten().collect();
        }
        return Some(tile);
    }
    None
}

/// Builds the atlas for every block, preferring pack PNGs over generated tiles.
pub fn atlas(blocks: &Registry<BlockDef>, packs: Option<&LoadedPacks>) -> (Image, Tiles) {
    let rows = (blocks.len() as u32 * 2).div_ceil(COLS).max(1);
    let size = UVec2::new(COLS * TILE, rows * TILE);
    let mut data = vec![0u8; (size.x * size.y * 4) as usize];
    let mut info = Tiles {
        size,
        water: blocks
            .iter()
            .map(|(_, b)| b.pattern == Pattern::Water)
            .collect(),
        glow: blocks.iter().map(|(_, b)| b.glow.clamp(0.0, 1.0)).collect(),
        light: blocks.iter().map(|(_, b)| b.light.min(15)).collect(),
        passes: blocks
            .iter()
            .map(|(_, b)| !b.solid || b.see_through())
            .collect(),
        cross: blocks
            .iter()
            .map(|(_, b)| b.shape == Shape::Cross)
            .collect(),
        see_through: blocks.iter().map(|(_, b)| b.see_through()).collect(),
        family: blocks
            .iter()
            .map(|(i, b)| {
                blocks
                    .iter()
                    .find(|(_, o)| o.color == b.color && o.top == b.top)
                    .map_or(i, |(first, _)| first)
            })
            .collect(),
        image: Image::default(),
    };
    for (i, def) in blocks.iter() {
        let (mut top, mut side) = draw(def);
        if let Some(packs) = packs {
            let both = png_tile(packs, &format!("textures/{}.png", def.id), false);
            if let Some(t) =
                png_tile(packs, &format!("textures/{}_top.png", def.id), false).or(both.clone())
            {
                top = t;
            }
            if let Some(t) = png_tile(packs, &format!("textures/{}_side.png", def.id), true)
                .or_else(|| png_tile(packs, &format!("textures/{}.png", def.id), true))
            {
                side = t;
            }
        }
        for (face, tile) in [top, side].iter().enumerate() {
            let o = info.origin(i, face as u32).as_uvec2();
            for y in 0..TILE {
                for x in 0..TILE {
                    let at = (((o.y + y) * size.x + o.x + x) * 4) as usize;
                    data[at..at + 4].copy_from_slice(&tile[(y * TILE + x) as usize]);
                }
            }
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::nearest();
    info.image = img.clone();
    (img, info)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(ron: &str) -> BlockDef {
        ron::from_str(ron).unwrap()
    }

    #[test]
    fn ramp_darkens_cool_and_lightens_warm() {
        let c = [120, 120, 120];
        let (dark, light) = (ramp(c, -2), ramp(c, 2));
        assert!(dark[0] < dark[2], "shadows lean blue: {dark:?}");
        assert!(light[0] > light[2], "highlights lean warm: {light:?}");
        assert!(dark[1] < c[1] && light[1] > c[1]);
        assert_eq!(ramp(c, 0), c);
        assert_eq!(ramp(c, 9), ramp(c, 2));
    }

    #[test]
    fn every_pattern_draws_full_opaque_tiles_with_some_variation() {
        for pattern in [
            "Grain", "Stone", "Sand", "Snow", "Grass", "Leaves", "Log", "Planks", "Bricks", "Ore",
            "Water",
        ] {
            let def = block(&format!(
                r#"(id: "b", color: (120, 90, 60), top: Some((90, 160, 70)), accent: Some((240, 200, 80)), pattern: {pattern})"#
            ));
            let (top, side) = draw(&def);
            for tile in [&top, &side] {
                assert_eq!(tile.len(), 256);
                assert!(tile.iter().all(|p| p[3] == 255));
                let first = tile[0];
                assert!(tile.iter().any(|p| *p != first), "{pattern} is flat");
            }
        }
        let flat = draw(&block(r#"(id: "b", color: (10, 20, 30), pattern: Smooth)"#));
        assert!(flat.0.iter().all(|p| *p == [10, 20, 30, 255]));
    }

    #[test]
    fn grass_sides_hang_a_lip_of_the_top_colour_over_soil() {
        let def = block(
            r#"(id: "grass", color: (124, 86, 52), top: Some((92, 168, 72)), pattern: Grass)"#,
        );
        let (_, side) = draw(&def);
        let green = |p: &[u8; 4]| p[1] > p[0];
        // The top row of the face (v = 15) is all grass, the bottom row all soil.
        assert!(side[15 * 16..].iter().all(green));
        assert!(side[..16].iter().all(|p| !green(p)));
    }

    #[test]
    fn blocks_do_not_overlap_in_the_atlas() {
        let mut blocks = Registry::<BlockDef>::default();
        blocks.insert(BlockDef::air());
        for i in 0..20 {
            blocks.insert(block(&format!(
                r#"(id: "b{i}", color: ({}, 0, 0), pattern: Smooth)"#,
                i * 10 + 10
            )));
        }
        let (img, tiles) = atlas(&blocks, None);
        assert_eq!(tiles.size, UVec2::new(256, 48));
        let data = img.data.as_ref().unwrap();
        for i in 1..=20u16 {
            for face in 0..2 {
                let o = tiles.origin(i, face).as_uvec2();
                // First and last texel of the cell carry this block's colour.
                for (x, y) in [(o.x, o.y), (o.x + 15, o.y + 15)] {
                    let at = ((y * 256 + x) * 4) as usize;
                    assert_eq!(data[at], (i as u32 * 10) as u8, "block {i} face {face}");
                }
            }
        }
        assert!(!tiles.water[1]);
    }

    #[test]
    fn ore_shares_a_family_and_its_rock_with_stone() {
        let mut blocks = Registry::<BlockDef>::default();
        blocks.insert(BlockDef::air());
        let stone = blocks.insert(block(
            r#"(id: "stone", color: (130, 130, 140), pattern: Stone)"#,
        ));
        let ore = blocks.insert(block(
            r#"(id: "ore", color: (130, 130, 140), accent: Some((250, 200, 60)), pattern: Ore)"#,
        ));
        let grass = blocks.insert(block(
            r#"(id: "grass", color: (130, 130, 140), top: Some((90, 160, 70)), pattern: Grass)"#,
        ));
        let (_, tiles) = atlas(&blocks, None);
        assert_eq!(tiles.family[ore as usize], tiles.family[stone as usize]);
        assert_ne!(tiles.family[grass as usize], tiles.family[stone as usize]);
        // Away from the nuggets the ore tile is the stone tile.
        let (s, _) = draw(blocks.at(stone));
        let (o, _) = draw(blocks.at(ore));
        let same = s.iter().zip(&o).filter(|(a, b)| a == b).count();
        assert!(same > 180, "only {same} of 256 texels match");
        assert!(same < 256);
    }
}
