//! Smooth terrain field shared by the ground mesh and the water overlay. A point's weight for
//! a terrain is the kernel-weighted share of nearby tiles of that terrain, so the 0.5 contour
//! bends around tile corners instead of cutting them diagonally.

use bevy::prelude::*;

use super::chunk::{CHUNK, TILES, Tile};

/// Kernel radius in cells. Larger rounds shores more and swallows single-tile features.
pub const RADIUS: f32 = 1.45;
/// Sub-quads per cell edge for the blended meshes.
pub const SUB: i32 = 4;

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
/// Returns `(terrain, weight)` pairs summing to 1, heaviest first.
pub fn weights_at(p: Vec2, terrain_of: &impl Fn(IVec2) -> u16) -> Vec<(u16, f32)> {
    let mut acc: Vec<(u16, f32)> = Vec::with_capacity(6);
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
            let t = terrain_of(cell);
            match acc.iter_mut().find(|(id, _)| *id == t) {
                Some(e) => e.1 += w,
                None => acc.push((t, w)),
            }
        }
    }
    for e in &mut acc {
        e.1 /= total.max(1e-6);
    }
    acc.sort_by(|a, b| b.1.total_cmp(&a.1).then(b.0.cmp(&a.0)));
    acc
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
        assert_eq!(w[0].0, 1);
        let w = weights_at(Vec2::new(-0.5, 0.0), &field);
        // Exactly on the boundary: both sides within a hair of 0.5.
        assert!((w[0].1 - 0.5).abs() < 0.05);
    }
}
