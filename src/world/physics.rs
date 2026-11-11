//! Movement for anything that walks: a box that slides along blocks one axis at a time,
//! falls when nothing is under it, and steps up a single block without being asked.

use bevy::prelude::*;

/// Half the width of a walker.
pub const RADIUS: f32 = 0.3;
/// Height of a walker.
pub const TALL: f32 = 1.6;
const GRAVITY: f32 = 32.0;
const MAX_FALL: f32 = 24.0;
const EPS: f32 = 1e-3;
/// Longest step simulated at once, so a fall never crosses a whole block in one go.
pub const MAX_DT: f32 = 1.0 / 30.0;

/// `pos` is the centre of the feet.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub pos: Vec3,
    /// Downward speed in blocks per second.
    pub fall: f32,
    pub grounded: bool,
}

impl Body {
    pub fn at(pos: Vec3) -> Self {
        Self {
            pos,
            fall: 0.0,
            grounded: false,
        }
    }
}

/// Whether a walker standing with its feet at `pos` overlaps no blocked cell.
pub fn fits(pos: Vec3, blocked: &impl Fn(IVec3) -> bool) -> bool {
    let lo = Vec3::new(pos.x - RADIUS + EPS, pos.y - RADIUS + EPS, pos.z + EPS)
        .floor()
        .as_ivec3();
    let hi = Vec3::new(
        pos.x + RADIUS - EPS,
        pos.y + RADIUS - EPS,
        pos.z + TALL - EPS,
    )
    .floor()
    .as_ivec3();
    for z in lo.z..=hi.z {
        for y in lo.y..=hi.y {
            for x in lo.x..=hi.x {
                if blocked(IVec3::new(x, y, z)) {
                    return false;
                }
            }
        }
    }
    true
}

/// Advances `body` by `dt` seconds with a wished ground velocity in blocks per second.
pub fn step(body: &mut Body, wish: Vec2, dt: f32, blocked: &impl Fn(IVec3) -> bool) {
    let dt = dt.min(MAX_DT);
    for axis in 0..2 {
        let d = wish[axis] * dt;
        if d == 0.0 {
            continue;
        }
        let mut next = body.pos;
        next[axis] += d;
        if fits(next, blocked) {
            body.pos = next;
            continue;
        }
        // Stop flush against the boundary that was hit; a step up takes a jump.
        let dir = d.signum();
        let edge = body.pos[axis] + dir * RADIUS;
        let wall = if dir > 0.0 {
            (edge - 2.0 * EPS).floor() + 1.0
        } else {
            (edge + 2.0 * EPS).ceil() - 1.0
        };
        let mut flush = body.pos;
        flush[axis] = wall - dir * (RADIUS + EPS);
        if (flush[axis] - body.pos[axis]) * dir > 0.0 && fits(flush, blocked) {
            body.pos = flush;
        }
    }
    body.fall = (body.fall + GRAVITY * dt).min(MAX_FALL);
    let mut next = body.pos;
    next.z -= body.fall * dt;
    if fits(next, blocked) {
        body.pos = next;
        body.grounded = false;
    } else if body.fall < 0.0 {
        // Rising into a ceiling: stop under it and start falling.
        body.pos.z = (next.z + TALL).floor() - TALL - 2.0 * EPS;
        body.fall = 0.0;
        body.grounded = false;
    } else {
        // Land on top of the layer that stopped the fall.
        body.pos.z = next.z.floor() + 1.0;
        body.fall = 0.0;
        body.grounded = true;
    }
}

/// Upward speed a jump starts with: enough to clear a block with a little to spare.
pub const JUMP: f32 = 9.5;

/// Starts a jump if the body stands on something.
pub fn jump(body: &mut Body) -> bool {
    if !body.grounded {
        return false;
    }
    body.fall = -JUMP;
    body.grounded = false;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `secs` of simulation at 60 steps per second.
    fn run(body: &mut Body, wish: Vec2, secs: f32, world: &impl Fn(IVec3) -> bool) {
        for _ in 0..(secs * 60.0) as u32 {
            step(body, wish, 1.0 / 60.0, world);
        }
    }

    fn standing(x: f32, y: f32, z: f32, world: &impl Fn(IVec3) -> bool) -> Body {
        let mut b = Body::at(Vec3::new(x, y, z));
        run(&mut b, Vec2::ZERO, 0.5, world);
        assert!(b.grounded);
        b
    }

    #[test]
    fn walks_on_flat_ground() {
        let floor = |c: IVec3| c.z < 1;
        let mut b = standing(0.5, 0.5, 1.0, &floor);
        run(&mut b, Vec2::new(4.0, 0.0), 1.0, &floor);
        assert!((b.pos.x - 4.5).abs() < 0.05, "{}", b.pos);
        assert_eq!(b.pos.z, 1.0);
        assert!(b.grounded);
    }

    #[test]
    fn falls_and_lands_on_the_surface() {
        let floor = |c: IVec3| c.z < 3;
        let mut b = Body::at(Vec3::new(0.5, 0.5, 9.4));
        run(&mut b, Vec2::ZERO, 2.0, &floor);
        assert_eq!(b.pos.z, 3.0);
        assert!(b.grounded && b.fall == 0.0);
    }

    #[test]
    fn a_two_high_wall_stops_the_walker_flush() {
        let world = |c: IVec3| c.z < 1 || (c.x == 3 && c.z < 3);
        for (start, dir, want) in [(0.5, 4.0, 3.0 - RADIUS), (6.5, -4.0, 4.0 + RADIUS)] {
            let mut b = standing(start, 0.5, 1.0, &world);
            run(&mut b, Vec2::new(dir, 0.0), 2.0, &world);
            assert!((b.pos.x - want).abs() < 0.01, "{}", b.pos);
            assert_eq!(b.pos.z, 1.0);
        }
    }

    #[test]
    fn a_single_block_stops_the_walker_until_they_jump() {
        let world = |c: IVec3| c.z < 1 || (c.x >= 3 && c.z < 2);
        let mut b = standing(0.5, 0.5, 1.0, &world);
        run(&mut b, Vec2::new(4.0, 0.0), 1.5, &world);
        assert!(b.pos.x < 3.0, "{}", b.pos);
        assert_eq!(b.pos.z, 1.0);
        // A jump while walking clears it; a jump in the air does nothing.
        assert!(jump(&mut b));
        assert!(!jump(&mut b));
        run(&mut b, Vec2::new(4.0, 0.0), 1.0, &world);
        assert!(b.pos.x > 3.5, "{}", b.pos);
        assert_eq!(b.pos.z, 2.0);
        assert!(b.grounded);
    }

    #[test]
    fn a_jump_clears_one_block_and_not_two() {
        let world = |c: IVec3| c.z < 1;
        let mut b = standing(0.5, 0.5, 1.0, &world);
        jump(&mut b);
        let mut peak: f32 = 0.0;
        for _ in 0..60 {
            step(&mut b, Vec2::ZERO, 1.0 / 60.0, &world);
            peak = peak.max(b.pos.z);
        }
        assert!(peak > 2.1 && peak < 2.9, "peak {peak}");
        assert!(b.grounded && b.pos.z == 1.0);
    }

    #[test]
    fn a_ceiling_stops_a_jump_short() {
        // A ceiling at z 3: the head (1.6 tall) hits it on the way up.
        let world = |c: IVec3| c.z < 1 || c.z == 3;
        let mut b = standing(0.5, 0.5, 1.0, &world);
        jump(&mut b);
        let mut peak: f32 = 0.0;
        for _ in 0..60 {
            step(&mut b, Vec2::ZERO, 1.0 / 60.0, &world);
            peak = peak.max(b.pos.z);
        }
        assert!(peak <= 3.0 - TALL + 1e-3, "peak {peak}");
        assert!(b.grounded && b.pos.z == 1.0);
    }

    #[test]
    fn walking_off_a_ledge_drops_to_the_lower_level() {
        let world = |c: IVec3| c.z < 1 || (c.x < 3 && c.z < 4);
        let mut b = standing(0.5, 0.5, 4.0, &world);
        run(&mut b, Vec2::new(4.0, 0.0), 2.0, &world);
        assert!(b.pos.x > 3.5);
        assert_eq!(b.pos.z, 1.0);
    }

    #[test]
    fn slides_along_a_wall_when_moving_diagonally() {
        let world = |c: IVec3| c.z < 1 || (c.x == 3 && c.z < 3);
        let mut b = standing(2.5, 0.5, 1.0, &world);
        run(&mut b, Vec2::new(4.0, 4.0), 1.0, &world);
        assert!(b.pos.x < 3.0 && b.pos.y > 4.0, "{}", b.pos);
    }
}
