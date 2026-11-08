//! The field: grid geometry, placed stones, border, and cursor tracking.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::player::Player;

/// Side length of one grid cell in world units. Cells are centered on integer multiples of it.
pub const CELL: f32 = 25.0;
/// Largest cell index (per axis) still inside the field. The field is `2 * FIELD + 1` cells wide.
pub const FIELD: IVec2 = IVec2::new(242, 138);
/// Half-extent of the field in world units; the border bands start here.
pub const WORLD_HALF: Vec2 =
    Vec2::new((FIELD.x as f32 + 0.5) * CELL, (FIELD.y as f32 + 0.5) * CELL);
/// Player half-size.
pub const PLAYER_R: f32 = CELL * 0.3;

const BORDER: f32 = 1000.0;
const STONE: Color = Color::srgb(80.0 / 255.0, 60.0 / 255.0, 0.0);
const GRID: Color = Color::srgba(180.0 / 255.0, 180.0 / 255.0, 180.0 / 255.0, 50.0 / 255.0);
const BORDER_COLOR: Color = Color::srgba(25.0 / 255.0, 75.0 / 255.0, 25.0 / 255.0, 100.0 / 255.0);
/// Shrinks the player's box when testing overlap so merely touching a stone never counts.
const EPS: f32 = 1e-3;

/// Occupied cells and the sprite entity drawn in each.
#[derive(Resource, Default, Deref, DerefMut)]
pub struct Blocks(HashMap<IVec2, Entity>);

/// The cell under the mouse this frame, if the cursor is over the window.
#[derive(Resource, Default)]
pub struct Cursor(pub Option<IVec2>);

pub fn cell_of(p: Vec2) -> IVec2 {
    (p / CELL).round().as_ivec2()
}

pub fn center_of(c: IVec2) -> Vec2 {
    c.as_vec2() * CELL
}

/// Cells whose square strictly overlaps the player's box centered at `p` (at most four).
pub fn overlapping_cells(p: Vec2) -> impl Iterator<Item = IVec2> {
    let lo = cell_of(p - PLAYER_R + EPS);
    let hi = cell_of(p + PLAYER_R - EPS);
    (lo.x..=hi.x).flat_map(move |x| (lo.y..=hi.y).map(move |y| IVec2::new(x, y)))
}

/// Moves `p` by `delta` along a single axis, stopping flush against the first stone in the way.
pub fn slide(p: Vec2, delta: Vec2, blocks: &Blocks) -> Vec2 {
    if delta == Vec2::ZERO {
        return p;
    }
    let axis = if delta.x != 0.0 { Vec2::X } else { Vec2::Y };
    let dir = axis * delta.dot(axis).signum();
    let next = p + delta;
    let nearest = overlapping_cells(next)
        .filter(|c| blocks.contains_key(c))
        .min_by(|a, b| center_of(*a).dot(dir).total_cmp(&center_of(*b).dot(dir)));
    match nearest {
        None => next,
        Some(c) => next - dir * ((next - center_of(c)).dot(dir) + CELL / 2.0 + PLAYER_R),
    }
}

pub fn spawn_border(mut commands: Commands) {
    let span = 2.0 * (WORLD_HALF + BORDER);
    let edge = WORLD_HALF + BORDER / 2.0;
    let bands = [
        (Vec2::new(-edge.x, 0.0), Vec2::new(BORDER, span.y)),
        (Vec2::new(edge.x, 0.0), Vec2::new(BORDER, span.y)),
        (Vec2::new(0.0, -edge.y), Vec2::new(span.x, BORDER)),
        (Vec2::new(0.0, edge.y), Vec2::new(span.x, BORDER)),
    ];
    for (center, size) in bands {
        commands.spawn((
            Sprite::from_color(BORDER_COLOR, size),
            Transform::from_translation(center.extend(1.0)),
        ));
    }
}

/// Draws only the grid lines inside the camera's view, clipped to the field.
pub fn draw_grid(cam: Single<(&Projection, &Transform), With<Camera2d>>, mut gizmos: Gizmos) {
    let (proj, tf) = *cam;
    let Projection::Orthographic(ortho) = proj else {
        return;
    };
    let offset = tf.translation.truncate();
    let view = Rect::from_corners(ortho.area.min + offset, ortho.area.max + offset);
    let clip = Rect::from_corners(view.min.max(-WORLD_HALF), view.max.min(WORLD_HALF));
    // Line `i` sits on the boundary between cells `i` and `i + 1`.
    let lo = (view.min / CELL - 0.5).ceil().as_ivec2().max(-FIELD - 1);
    let hi = (view.max / CELL - 0.5).floor().as_ivec2().min(FIELD);
    for i in lo.x..=hi.x {
        let x = (i as f32 + 0.5) * CELL;
        gizmos.line_2d(Vec2::new(x, clip.min.y), Vec2::new(x, clip.max.y), GRID);
    }
    for j in lo.y..=hi.y {
        let y = (j as f32 + 0.5) * CELL;
        gizmos.line_2d(Vec2::new(clip.min.x, y), Vec2::new(clip.max.x, y), GRID);
    }
}

pub fn track_cursor(
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &Transform), With<Camera2d>>,
    mut cursor: ResMut<Cursor>,
) {
    let (camera, tf) = *cam;
    cursor.0 = window
        .cursor_position()
        .and_then(|p| {
            camera
                .viewport_to_world_2d(&GlobalTransform::from(*tf), p)
                .ok()
        })
        .map(cell_of);
}

/// Left click lays a stone under the cursor; right click removes one.
pub fn build(
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Res<Cursor>,
    player: Single<&Transform, With<Player>>,
    mut blocks: ResMut<Blocks>,
    mut commands: Commands,
) {
    let Some(cell) = cursor.0 else { return };
    if mouse.just_pressed(MouseButton::Left)
        && cell.abs().cmple(FIELD).all()
        && !blocks.contains_key(&cell)
        && !overlapping_cells(player.translation.truncate()).any(|c| c == cell)
    {
        let id = commands
            .spawn((
                Sprite::from_color(STONE, Vec2::splat(CELL)),
                Transform::from_translation(center_of(cell).extend(1.0)),
            ))
            .id();
        blocks.insert(cell, id);
    }
    if mouse.just_pressed(MouseButton::Right)
        && let Some(id) = blocks.remove(&cell)
    {
        commands.entity(id).despawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stones(cells: &[IVec2]) -> Blocks {
        Blocks(cells.iter().map(|&c| (c, Entity::PLACEHOLDER)).collect())
    }

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn cell_round_trip() {
        for c in [IVec2::ZERO, -FIELD, IVec2::new(7, -3)] {
            assert_eq!(cell_of(center_of(c)), c);
        }
        assert_eq!(cell_of(Vec2::new(12.4, -12.4)), IVec2::ZERO);
        assert_eq!(cell_of(Vec2::new(12.6, 0.0)), IVec2::X);
    }

    #[test]
    fn free_move_is_unchanged() {
        assert!(close(
            slide(Vec2::ZERO, Vec2::X * 4.0, &stones(&[])),
            Vec2::X * 4.0
        ));
    }

    #[test]
    fn stops_flush_against_stone_from_each_side() {
        let b = stones(&[IVec2::X]); // spans x in 12.5..37.5
        let wall = CELL / 2.0 + PLAYER_R;
        assert!(close(
            slide(Vec2::ZERO, Vec2::X * 10.0, &b),
            Vec2::new(CELL - wall, 0.0)
        ));
        assert!(close(
            slide(Vec2::new(50.0, 0.0), Vec2::X * -10.0, &b),
            Vec2::new(CELL + wall, 0.0)
        ));
        let b = stones(&[IVec2::ZERO]);
        assert!(close(
            slide(Vec2::new(0.0, 25.0), Vec2::Y * -10.0, &b),
            Vec2::new(0.0, wall)
        ));
        assert!(close(
            slide(Vec2::new(0.0, -25.0), Vec2::Y * 10.0, &b),
            Vec2::new(0.0, -wall)
        ));
    }

    #[test]
    fn slides_along_a_wall_when_touching() {
        let b = stones(&[IVec2::X]);
        let flush = Vec2::new(CELL / 2.0 - PLAYER_R, 0.0);
        assert!(close(
            slide(flush, Vec2::Y * 4.0, &b),
            flush + Vec2::Y * 4.0
        ));
    }

    #[test]
    fn overlap_covers_player_cells_only() {
        let cells: Vec<_> = overlapping_cells(Vec2::ZERO).collect();
        assert_eq!(cells, vec![IVec2::ZERO]);
        let cells: Vec<_> = overlapping_cells(Vec2::new(12.5, 12.5)).collect();
        assert_eq!(cells.len(), 4);
    }
}
