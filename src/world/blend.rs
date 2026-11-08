//! Smooth terrain field shared by the ground mesh, the edge layers and the water overlay. A
//! point's weight for a terrain is the kernel-weighted share of nearby tiles of that terrain,
//! so the 0.5 contour bends around tile corners instead of cutting them diagonally.
//!
//! The field is computed once per chunk rebuild ([`chunk_field`]) and handed to every mesh
//! builder; each vertex holds at most [`MAX_TERRAINS`] weights in a fixed array (no allocation).

use bevy::prelude::*;

use super::chunk::{CHUNK, TILES, Tile};

/// Kernel radius in cells. Larger rounds shores more and swallows single-tile features.
pub const RADIUS: f32 = 1.45;
/// Sub-quads per cell edge for the blended meshes.
pub const SUB: i32 = 4;
/// Vertices per dual cell.
pub const VERTS: usize = ((SUB + 1) * (SUB + 1)) as usize;
/// Distinct terrains a kernel can see at once (a 5x5 window could hold more, but in practice
/// four is rare; extras are folded into the nearest heavier entry).
pub const MAX_TERRAINS: usize = 6;

/// Terrain weights at one vertex, heaviest first, summing to 1.
#[derive(Clone, Copy, Debug, Default)]
pub struct Weights {
    pub n: u8,
    pub items: [(u16, f32); MAX_TERRAINS],
}

impl Weights {
    pub fn iter(&self) -> impl Iterator<Item = &(u16, f32)> {
        self.items[..self.n as usize].iter()
    }
    pub fn first(&self) -> (u16, f32) {
        self.items[0]
    }
    pub fn second(&self) -> Option<(u16, f32)> {
        (self.n > 1).then_some(self.items[1])
    }
    pub fn of(&self, terrain: u16) -> f32 {
        self.iter()
            .find(|(t, _)| *t == terrain)
            .map_or(0.0, |(_, w)| *w)
    }
    fn add(&mut self, terrain: u16, w: f32) {
        if let Some(e) = self.items[..self.n as usize]
            .iter_mut()
            .find(|(t, _)| *t == terrain)
        {
            e.1 += w;
        } else if (self.n as usize) < MAX_TERRAINS {
            self.items[self.n as usize] = (terrain, w);
            self.n += 1;
        } else {
            // Overflow: give the weight to the current heaviest so totals still sum to 1.
            self.items[0].1 += w;
        }
    }
    fn finish(&mut self, total: f32) {
        for e in &mut self.items[..self.n as usize] {
            e.1 /= total.max(1e-6);
        }
        self.items[..self.n as usize].sort_by(|a, b| b.1.total_cmp(&a.1).then(b.0.cmp(&a.0)));
    }
}

/// Resolves the terrain of any cell, preferring this chunk, then a neighbor, then clamping.
pub fn terrain_lookup<'a>(
    pos: IVec2,
    tiles: &'a [Tile; TILES],
    tile_at: &'a impl Fn(IVec2) -> Option<u16>,
) -> impl Fn(IVec2) -> u16 + 'a {
    let origin = pos * CHUNK;
    move |cell: IVec2| {
        let local = cell - origin;
        let inside = (0..CHUNK).contains(&local.x) && (0..CHUNK).contains(&local.y);
        if inside {
            tiles[(local.y * CHUNK + local.x) as usize].terrain
        } else {
            tile_at(cell).unwrap_or_else(|| {
                let l = local.clamp(IVec2::ZERO, IVec2::splat(CHUNK - 1));
                tiles[(l.y * CHUNK + l.x) as usize].terrain
            })
        }
    }
}

/// Kernel-weighted terrain shares at a point given in cell units (tile centers at integers).
pub fn weights_at(p: Vec2, terrain_of: &impl Fn(IVec2) -> u16) -> Weights {
    let mut acc = Weights::default();
    let r = RADIUS.ceil() as i32;
    let center = p.round().as_ivec2();
    let mut total = 0.0;
    for dy in -r..=r {
        for dx in -r..=r {
            let cell = center + IVec2::new(dx, dy);
            let d = (cell.as_vec2() - p).length();
            if d >= RADIUS {
                continue;
            }
            let k = 1.0 - d / RADIUS;
            let w = k * k * (3.0 - 2.0 * k);
            total += w;
            acc.add(terrain_of(cell), w);
        }
    }
    acc.finish(total);
    acc
}

/// The whole chunk's field: `TILES * VERTS` entries, dual cell by dual cell, row-major within
/// a cell (`sy * (SUB + 1) + sx`). Dual cell `(tx, ty)` spans tile centers `(tx, ty)` to
/// `(tx + 1, ty + 1)`.
pub fn chunk_field(
    pos: IVec2,
    tiles: &[Tile; TILES],
    tile_at: &impl Fn(IVec2) -> Option<u16>,
) -> Vec<Weights> {
    let origin = pos * CHUNK;
    let terrain_of = terrain_lookup(pos, tiles, tile_at);
    let mut out = Vec::with_capacity(TILES * VERTS);
    for ty in 0..CHUNK {
        for tx in 0..CHUNK {
            let cell = origin + IVec2::new(tx, ty);
            for sy in 0..=SUB {
                for sx in 0..=SUB {
                    let p =
                        cell.as_vec2() + Vec2::new(sx as f32 / SUB as f32, sy as f32 / SUB as f32);
                    out.push(weights_at(p, &terrain_of));
                }
            }
        }
    }
    out
}

/// Vertex positions and the shared index pattern for one dual cell, in chunk-local world units.
pub fn cell_vertex(tx: i32, ty: i32, sx: i32, sy: i32) -> [f32; 3] {
    use super::chunk::CELL;
    let (u, v) = (sx as f32 / SUB as f32, sy as f32 / SUB as f32);
    [
        (tx as f32 + 0.5 + u) * CELL,
        (ty as f32 + 0.5 + v) * CELL,
        0.0,
    ]
}

pub fn push_cell_indices(idx: &mut Vec<u32>, base: u32) {
    let wv = (SUB + 1) as u32;
    for sy in 0..SUB as u32 {
        for sx in 0..SUB as u32 {
            let i0 = base + sy * wv + sx;
            idx.extend([i0, i0 + 1, i0 + wv + 1, i0, i0 + wv + 1, i0 + wv]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_one_and_prefer_the_near_tile() {
        let field = |c: IVec2| if c.x >= 0 { 1 } else { 0 };
        let w = weights_at(Vec2::new(0.4, 0.0), &field);
        let sum: f32 = w.iter().map(|e| e.1).sum();
        assert!((sum - 1.0).abs() < 1e-4);
        assert_eq!(w.first().0, 1);
        let w = weights_at(Vec2::new(-0.5, 0.0), &field);
        assert!((w.first().1 - 0.5).abs() < 0.05);
    }

    #[test]
    fn chunk_field_has_one_entry_per_vertex_and_no_nan() {
        let tiles = [Tile::default(); TILES];
        let f = chunk_field(IVec2::ZERO, &tiles, &|_| None);
        assert_eq!(f.len(), TILES * VERTS);
        assert!(
            f.iter()
                .all(|w| w.n == 1 && (w.first().1 - 1.0).abs() < 1e-5)
        );
    }
}
