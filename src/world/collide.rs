//! Axis-separated AABB movement against a grid of solid cells. Each solid cell reports its own
//! half-size so a tree blocks only at the trunk while a wall fills the cell.

use bevy::prelude::*;

use super::chunk::{CELL, cell_of, center_of};

/// Player half-size.
pub const PLAYER_R: f32 = CELL * 0.26;

/// Cells whose square strictly overlaps a box of half-size `r` centered at `p`.
pub fn overlapping_cells(p: Vec2, r: f32) -> impl Iterator<Item = IVec2> {
    const EPS: f32 = 1e-3;
    let lo = cell_of(p - r + EPS);
    let hi = cell_of(p + r - EPS);
    (lo.x..=hi.x).flat_map(move |x| (lo.y..=hi.y).map(move |y| IVec2::new(x, y)))
}

/// Moves `p` by `delta` along a single axis, stopping flush against the first solid box.
/// `solid(cell)` returns the half-size of the cell's collision box, or `None` if passable.
pub fn slide(p: Vec2, delta: Vec2, r: f32, solid: impl Fn(IVec2) -> Option<f32>) -> Vec2 {
    if delta == Vec2::ZERO {
        return p;
    }
    let axis = if delta.x != 0.0 { Vec2::X } else { Vec2::Y };
    let dir = axis * delta.dot(axis).signum();
    let next = p + delta;
    let home = cell_of(next);
    let mut best: Option<(f32, Vec2, f32)> = None;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let cell = home + IVec2::new(dx, dy);
            let Some(h) = solid(cell) else { continue };
            let c = center_of(cell);
            let d = (next - c).abs();
            if d.x >= r + h || d.y >= r + h {
                continue;
            }
            // Only boxes the move actually entered count; ones we already overlap sideways
            // are handled by the other axis.
            let before = (p - c).abs();
            if before.x < r + h && before.y < r + h {
                continue;
            }
            let along = c.dot(dir);
            if best.is_none_or(|(a, _, _)| along < a) {
                best = Some((along, c, h));
            }
        }
    }
    match best {
        None => next,
        Some((_, c, h)) => next - dir * ((next - c).dot(dir) + h + r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-4
    }

    fn full(cells: &'static [IVec2]) -> impl Fn(IVec2) -> Option<f32> {
        move |c| cells.contains(&c).then_some(CELL / 2.0)
    }

    #[test]
    fn free_move_is_unchanged() {
        let got = slide(Vec2::ZERO, Vec2::X * 4.0, PLAYER_R, |_| None);
        assert!(close(got, Vec2::X * 4.0));
    }

    #[test]
    fn stops_flush_against_cell_from_each_side() {
        let wall = CELL / 2.0 + PLAYER_R;
        let b = full(&[IVec2::X]);
        assert!(close(
            slide(Vec2::ZERO, Vec2::X * 10.0, PLAYER_R, &b),
            Vec2::new(CELL - wall, 0.0)
        ));
        assert!(close(
            slide(Vec2::new(2.0 * CELL, 0.0), Vec2::X * -10.0, PLAYER_R, &b),
            Vec2::new(CELL + wall, 0.0)
        ));
        let b = full(&[IVec2::ZERO]);
        assert!(close(
            slide(Vec2::new(0.0, CELL), Vec2::Y * -10.0, PLAYER_R, &b),
            Vec2::new(0.0, wall)
        ));
        assert!(close(
            slide(Vec2::new(0.0, -CELL), Vec2::Y * 10.0, PLAYER_R, &b),
            Vec2::new(0.0, -wall)
        ));
    }

    #[test]
    fn slides_along_a_wall_when_touching() {
        let flush = Vec2::new(CELL / 2.0 - PLAYER_R, 0.0);
        let got = slide(flush, Vec2::Y * 4.0, PLAYER_R, full(&[IVec2::X]));
        assert!(close(got, flush + Vec2::Y * 4.0));
    }

    #[test]
    fn small_hitbox_lets_player_pass_beside_it() {
        // A trunk of half-size 0.2 cells in cell (1,0): walking along y at x = 0.4 cells misses it.
        let trunk = |c: IVec2| (c == IVec2::X).then_some(CELL * 0.2);
        let start = Vec2::new(CELL * 0.45, -CELL);
        let got = slide(start, Vec2::Y * 50.0, PLAYER_R, trunk);
        assert!(close(got, start + Vec2::Y * 50.0));
        // Walking straight at it along x stops at the trunk edge.
        let got = slide(Vec2::ZERO, Vec2::X * 20.0, PLAYER_R, trunk);
        assert!(close(got, Vec2::new(CELL - CELL * 0.2 - PLAYER_R, 0.0)));
    }

    #[test]
    fn overlap_covers_box_cells_only() {
        assert_eq!(
            overlapping_cells(Vec2::ZERO, PLAYER_R).collect::<Vec<_>>(),
            vec![IVec2::ZERO]
        );
        assert_eq!(
            overlapping_cells(Vec2::splat(CELL / 2.0), PLAYER_R).count(),
            4
        );
    }
}
