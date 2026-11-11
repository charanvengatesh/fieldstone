//! The player as a small block model: head, body, arms and legs as boxes in world units
//! (blocks), coloured per face and shaded by the same sun as the terrain. It is projected
//! through the view like a chunk is, so it turns, foreshortens and occludes exactly as the
//! blocks around it do. The model faces `facing` (a world direction on the ground) and
//! takes a stride `frame`; the base pack ships no image for it.

use std::f32::consts::FRAC_PI_4;

use bevy::prelude::*;

use crate::world::mesh::{Faces, SUN, TOP_SHADE};

/// Height of the model in blocks; the body it stands in is [`crate::world::physics::TALL`].
pub const HEIGHT: f32 = 1.64;

const HAIR: [f32; 3] = [0.31, 0.20, 0.13];
const SKIN: [f32; 3] = [0.93, 0.75, 0.59];
const EYE: [f32; 3] = [0.16, 0.13, 0.19];
const TUNIC: [f32; 3] = [0.24, 0.45, 0.78];
const TUNIC_DARK: [f32; 3] = [0.17, 0.31, 0.60];
const BELT: [f32; 3] = [0.36, 0.24, 0.16];
const LEGS: [f32; 3] = [0.33, 0.30, 0.41];
const LEGS_DARK: [f32; 3] = [0.24, 0.21, 0.32];
const BOOT: [f32; 3] = [0.20, 0.16, 0.15];

/// Colours of one box: top, front (toward the facing), back, left, right.
#[derive(Clone, Copy)]
struct Paint {
    top: [f32; 3],
    front: [f32; 3],
    back: [f32; 3],
    left: [f32; 3],
    right: [f32; 3],
}

impl Paint {
    const fn plain(c: [f32; 3]) -> Self {
        Self {
            top: c,
            front: c,
            back: c,
            left: c,
            right: c,
        }
    }
}

/// How bright a face with outward normal `n` (in world space) is under the fixed sun:
/// tops full, sides by how much they face the sun, as the terrain's sides are.
fn shade(n: Vec3) -> f32 {
    if n.z > 0.5 {
        return TOP_SHADE;
    }
    let toward = n.truncate().dot(SUN.truncate().normalize());
    0.70 + 0.20 * toward
}

/// Turns a model-space point (x right, y forward, z up, feet at the origin) into the
/// world frame of a figure facing `facing`.
fn turn(p: Vec3, facing: Vec2) -> Vec3 {
    // Facing +y in model space maps onto `facing`; x maps onto its right-hand side.
    let right = Vec2::new(facing.y, -facing.x);
    let xy = right * p.x + facing * p.y;
    Vec3::new(xy.x, xy.y, p.z)
}

/// Pushes the five visible faces of a box (no underside) with corners `lo..hi` in model
/// space, turned to `facing`, into `out`.
fn push_box(out: &mut Faces, lo: Vec3, hi: Vec3, paint: Paint, facing: Vec2) {
    let c = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    // Each face: four corners counter-clockwise seen from outside, its model normal, its
    // colour.
    let faces: [([Vec3; 4], Vec3, [f32; 3]); 5] = [
        (
            [
                c(lo.x, lo.y, hi.z),
                c(hi.x, lo.y, hi.z),
                c(hi.x, hi.y, hi.z),
                c(lo.x, hi.y, hi.z),
            ],
            Vec3::Z,
            paint.top,
        ),
        (
            [
                c(hi.x, hi.y, lo.z),
                c(lo.x, hi.y, lo.z),
                c(lo.x, hi.y, hi.z),
                c(hi.x, hi.y, hi.z),
            ],
            Vec3::Y,
            paint.front,
        ),
        (
            [
                c(lo.x, lo.y, lo.z),
                c(hi.x, lo.y, lo.z),
                c(hi.x, lo.y, hi.z),
                c(lo.x, lo.y, hi.z),
            ],
            Vec3::NEG_Y,
            paint.back,
        ),
        (
            [
                c(lo.x, hi.y, lo.z),
                c(lo.x, lo.y, lo.z),
                c(lo.x, lo.y, hi.z),
                c(lo.x, hi.y, hi.z),
            ],
            Vec3::NEG_X,
            paint.left,
        ),
        (
            [
                c(hi.x, lo.y, lo.z),
                c(hi.x, hi.y, lo.z),
                c(hi.x, hi.y, hi.z),
                c(hi.x, lo.y, hi.z),
            ],
            Vec3::X,
            paint.right,
        ),
    ];
    for (corners, normal, colour) in faces {
        let n = turn(normal, facing);
        let s = shade(n);
        for p in corners {
            out.positions.push(turn(p, facing).to_array());
            out.uvs.push([0.0, 0.0]);
            out.colors
                .push([colour[0] * s, colour[1] * s, colour[2] * s, 1.0]);
        }
        out.flips.push(false);
    }
}

/// A thin square stuck a hair in front of a face, for eyes and the like.
fn push_decal(
    out: &mut Faces,
    centre: Vec3,
    normal: Vec3,
    half: Vec2,
    colour: [f32; 3],
    facing: Vec2,
) {
    let up = Vec3::Z * half.y;
    let side = normal.cross(Vec3::Z).normalize() * half.x;
    let o = centre + normal * 0.004;
    let corners = [o - side - up, o + side - up, o + side + up, o - side + up];
    let s = shade(turn(normal, facing));
    for p in corners {
        out.positions.push(turn(p, facing).to_array());
        out.uvs.push([0.0, 0.0]);
        out.colors
            .push([colour[0] * s, colour[1] * s, colour[2] * s, 1.0]);
    }
    out.flips.push(false);
}

/// Builds the model facing `facing` (any unit direction on the ground; zero faces +y) at
/// stride `frame` 0 (standing) or 1 (mid-step). Positions are in blocks with the feet at
/// the origin.
pub fn model(facing: Vec2, frame: usize) -> Faces {
    let facing = if facing.length_squared() < 1e-6 {
        Vec2::Y
    } else {
        facing.normalize()
    };
    let mut out = Faces::default();
    let stepping = frame % 2 == 1;
    let swing = if stepping { 0.14 } else { 0.0 };

    // Legs with boots; one forward, one back when stepping.
    for (x, fwd, paint) in [(-0.12, swing, LEGS), (0.12, -swing, LEGS_DARK)] {
        push_box(
            &mut out,
            Vec3::new(x - 0.1, fwd - 0.11, 0.0),
            Vec3::new(x + 0.1, fwd + 0.11, 0.14),
            Paint::plain(BOOT),
            facing,
        );
        push_box(
            &mut out,
            Vec3::new(x - 0.1, fwd - 0.11, 0.14),
            Vec3::new(x + 0.1, fwd + 0.11, 0.66),
            Paint::plain(paint),
            facing,
        );
    }
    // Body: the tunic, darker at the back, belted at the hips.
    push_box(
        &mut out,
        Vec3::new(-0.25, -0.15, 0.66),
        Vec3::new(0.25, 0.15, 0.74),
        Paint::plain(BELT),
        facing,
    );
    push_box(
        &mut out,
        Vec3::new(-0.25, -0.15, 0.74),
        Vec3::new(0.25, 0.15, 1.20),
        Paint {
            top: TUNIC,
            front: TUNIC,
            back: TUNIC_DARK,
            left: TUNIC,
            right: TUNIC_DARK,
        },
        facing,
    );
    // Arms, swinging against the legs.
    for (x, fwd, c) in [(-0.33, -swing, TUNIC), (0.33, swing, TUNIC_DARK)] {
        push_box(
            &mut out,
            Vec3::new(x - 0.07, fwd - 0.08, 0.84),
            Vec3::new(x + 0.07, fwd + 0.08, 1.18),
            Paint::plain(c),
            facing,
        );
        push_box(
            &mut out,
            Vec3::new(x - 0.07, fwd - 0.08, 0.70),
            Vec3::new(x + 0.07, fwd + 0.08, 0.84),
            Paint::plain(SKIN),
            facing,
        );
    }
    // Head: skin in front, hair everywhere else, with a fringe, two eyes and a mouth.
    push_box(
        &mut out,
        Vec3::new(-0.22, -0.22, 1.20),
        Vec3::new(0.22, 0.22, HEIGHT),
        Paint {
            top: HAIR,
            front: SKIN,
            back: HAIR,
            left: HAIR,
            right: HAIR,
        },
        facing,
    );
    let front = Vec3::new(0.0, 0.22, 0.0);
    push_decal(
        &mut out,
        front + Vec3::Z * 1.58,
        Vec3::Y,
        Vec2::new(0.22, 0.06),
        HAIR,
        facing,
    );
    for x in [-0.09, 0.09] {
        push_decal(
            &mut out,
            front + Vec3::new(x, 0.0, 1.44),
            Vec3::Y,
            Vec2::new(0.035, 0.035),
            EYE,
            facing,
        );
    }
    push_decal(
        &mut out,
        front + Vec3::Z * 1.29,
        Vec3::Y,
        Vec2::new(0.07, 0.02),
        [0.78, 0.55, 0.45],
        facing,
    );
    out
}

/// Quantises a walk direction to one of eight facings so the model does not jitter as the
/// stick wobbles, and keeps the last facing when standing still.
pub fn facing_of(dir: Vec2, last: Vec2) -> Vec2 {
    if dir.length_squared() < 1e-6 {
        return last;
    }
    let a = (dir.y.atan2(dir.x) / FRAC_PI_4).round() * FRAC_PI_4;
    Vec2::new(a.cos(), a.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_stands_on_its_feet_and_turns_with_the_facing() {
        let f = model(Vec2::Y, 0);
        assert!(f.len() > 30);
        let zs: Vec<f32> = f.positions.iter().map(|p| p[2]).collect();
        assert!(zs.iter().cloned().fold(f32::MAX, f32::min) >= 0.0);
        assert!((zs.iter().cloned().fold(0.0, f32::max) - HEIGHT).abs() < 1e-5);
        // Facing +x, the eyes (the darkest texels) sit on the +x side of the head.
        let g = model(Vec2::X, 0);
        let eye = (0..g.colors.len())
            .find(|k| {
                let (c, p) = (g.colors[*k], g.positions[*k]);
                p[2] > 1.35 && p[2] < 1.55 && c[0] < 0.2
            })
            .expect("an eye");
        assert!(g.positions[eye][0] > 0.2, "{:?}", g.positions[eye]);
        // A step moves the two legs apart along the facing.
        let h = model(Vec2::Y, 1);
        let ys: Vec<f32> = h.positions[..8].iter().map(|p| p[1]).collect();
        let yt: Vec<f32> = f.positions[..8].iter().map(|p| p[1]).collect();
        assert!(ys != yt);
    }

    #[test]
    fn facings_snap_to_eighths() {
        let f = facing_of(Vec2::new(1.0, 0.1), Vec2::Y);
        assert!((f - Vec2::X).length() < 1e-5);
        let d = facing_of(Vec2::new(0.7, 0.8), Vec2::Y);
        assert!((d - Vec2::splat(FRAC_PI_4.cos())).length() < 1e-5);
        assert_eq!(facing_of(Vec2::ZERO, Vec2::NEG_X), Vec2::NEG_X);
    }
}
