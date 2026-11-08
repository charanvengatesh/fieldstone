//! Axis-separated AABB movement against a grid of solid cells.

use bevy::prelude::*;

use super::chunk::{CELL, cell_of, center_of};

/// Player half-size.
pub const PLAYER_R: f32 = CELL * 0.3;
/// Shrinks the moving box when testing overlap so merely touching a cell never counts.
const EPS: f32 = 1e-3;

/// Cells whose square strictly overlaps a box of half-size `r` centered at `p`.
pub fn overlapping_cells(p: Vec2, r: f32) -> impl Iterator<Item = IVec2> {
    let lo = cell_of(p - r + EPS);
    let hi = cell_of(p + r - EPS);
    (lo.x..=hi.x).flat_map(move |x| (lo.y..=hi.y).map(move |y| IVec2::new(x, y)))
}

/// Moves `p` by `delta` along a single axis, stopping flush against the first solid cell.
pub fn slide(p: Vec2, delta: Vec2, r: f32, is_solid: impl Fn(IVec2) -> bool) -> Vec2 {
    if delta == Vec2::ZERO {
        return p;
    }
    let axis = if delta.x != 0.0 { Vec2::X } else { Vec2::Y };
    let dir = axis * delta.dot(axis).signum();
    let next = p + delta;
    let nearest = overlapping_cells(next, r)
        .filter(|&c| is_solid(c))
        .min_by(|a, b| center_of(*a).dot(dir).total_cmp(&center_of(*b).dot(dir)));
    match nearest {
        None => next,
        Some(c) => next - dir * ((next - center_of(c)).dot(dir) + CELL / 2.0 + r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-4
    }

    fn solid(cells: &'static [IVec2]) -> impl Fn(IVec2) -> bool {
        move |c| cells.contains(&c)
    }

    #[test]
    fn free_move_is_unchanged() {
        let got = slide(Vec2::ZERO, Vec2::X * 4.0, PLAYER_R, |_| false);
        assert!(close(got, Vec2::X * 4.0));
    }

    #[test]
    fn stops_flush_against_cell_from_each_side() {
        let wall = CELL / 2.0 + PLAYER_R;
        let b = solid(&[IVec2::X]);
        assert!(close(
            slide(Vec2::ZERO, Vec2::X * 10.0, PLAYER_R, &b),
            Vec2::new(CELL - wall, 0.0)
        ));
        assert!(close(
            slide(Vec2::new(50.0, 0.0), Vec2::X * -10.0, PLAYER_R, &b),
            Vec2::new(CELL + wall, 0.0)
        ));
        let b = solid(&[IVec2::ZERO]);
        assert!(close(
            slide(Vec2::new(0.0, 25.0), Vec2::Y * -10.0, PLAYER_R, &b),
            Vec2::new(0.0, wall)
        ));
        assert!(close(
            slide(Vec2::new(0.0, -25.0), Vec2::Y * 10.0, PLAYER_R, &b),
            Vec2::new(0.0, -wall)
        ));
    }

    #[test]
    fn slides_along_a_wall_when_touching() {
        let flush = Vec2::new(CELL / 2.0 - PLAYER_R, 0.0);
        let got = slide(flush, Vec2::Y * 4.0, PLAYER_R, solid(&[IVec2::X]));
        assert!(close(got, flush + Vec2::Y * 4.0));
    }

    #[test]
    fn overlap_covers_box_cells_only() {
        assert_eq!(
            overlapping_cells(Vec2::ZERO, PLAYER_R).collect::<Vec<_>>(),
            vec![IVec2::ZERO]
        );
        assert_eq!(overlapping_cells(Vec2::splat(12.5), PLAYER_R).count(), 4);
    }
}
