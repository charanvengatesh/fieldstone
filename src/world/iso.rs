//! The isometric view. World space is a grid of unit blocks with z up; the screen is a
//! dimetric projection: floor diamonds `TILE_W` wide and `tile_h` tall, blocks `block_h`
//! tall, about any turn. The projection's own view direction is `(1, 1, tile_h / block_h)`
//! in view space, so a point's distance toward the viewer is `x + y + z * tile_h / block_h`,
//! which every vertex carries as its depth so the depth buffer resolves all occlusion at
//! any tilt. The classic 2:1 view has `tile_h = block_h = 16`, where depth is `x + y + z`.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

/// Texels across a floor diamond.
pub const TILE_W: f32 = 32.0;
/// Texels down a floor diamond.
pub const TILE_H: f32 = 16.0;
/// Texels a block rises on screen.
pub const BLOCK_H: f32 = 16.0;

/// How steeply the camera looks down: the height of a floor diamond and of a block on
/// screen, in texels. Presets keep pixel art crisp; the motion between them passes through
/// in-between values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tilt {
    pub tile_h: f32,
    pub block_h: f32,
}

impl Tilt {
    /// Presets by name: `low`, `classic`, `high`, `top`.
    pub const PRESETS: [(&'static str, Tilt); 4] = [
        (
            "low",
            Tilt {
                tile_h: 12.0,
                block_h: 18.0,
            },
        ),
        (
            "classic",
            Tilt {
                tile_h: TILE_H,
                block_h: BLOCK_H,
            },
        ),
        (
            "high",
            Tilt {
                tile_h: 24.0,
                block_h: 12.0,
            },
        ),
        (
            "top",
            Tilt {
                tile_h: 32.0,
                block_h: 6.0,
            },
        ),
    ];

    pub fn named(name: &str) -> Option<Tilt> {
        Self::PRESETS
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, t)| *t)
    }

    /// The preset this tilt is, or the nearest one.
    pub fn name(self) -> &'static str {
        Self::PRESETS
            .iter()
            .min_by(|a, b| {
                let da = (a.1.tile_h - self.tile_h).abs() + (a.1.block_h - self.block_h).abs();
                let db = (b.1.tile_h - self.tile_h).abs() + (b.1.block_h - self.block_h).abs();
                da.total_cmp(&db)
            })
            .map_or("classic", |(n, _)| n)
    }

    /// The preset after this one, round robin.
    pub fn next(self) -> Tilt {
        let i = Self::PRESETS
            .iter()
            .position(|(n, _)| *n == self.name())
            .unwrap_or(1);
        Self::PRESETS[(i + 1) % Self::PRESETS.len()].1
    }

    /// Depth gained per block of height: the projection's view direction rises this much.
    pub fn rise(self) -> f32 {
        self.tile_h / self.block_h
    }
}

impl Default for Tilt {
    fn default() -> Self {
        Tilt {
            tile_h: TILE_H,
            block_h: BLOCK_H,
        }
    }
}

/// Where the camera looks from: a turn about the vertical axis, in radians, and a tilt.
/// A quarter turn is one of the four classic diagonal views. `angle` and `tilt` are what
/// is drawn now; they ease toward their targets, so a turn or a tilt is a motion rather
/// than a cut.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub angle: f32,
    pub target: f32,
    pub tilt: Tilt,
    pub target_tilt: Tilt,
}

impl Default for View {
    fn default() -> Self {
        View {
            angle: 0.0,
            target: 0.0,
            tilt: Tilt::default(),
            target_tilt: Tilt::default(),
        }
    }
}

/// The block under a screen point and the face the view ray entered it through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub block: IVec3,
    pub normal: IVec3,
}

impl View {
    /// One of the four diagonal views, at rest, at the classic tilt.
    pub fn quarter(rot: u8) -> View {
        let angle = (rot % 4) as f32 * FRAC_PI_2;
        View {
            angle,
            target: angle,
            ..default()
        }
    }

    /// Asks for a tilt, as a motion.
    pub fn set_tilt(&mut self, tilt: Tilt) {
        self.target_tilt = tilt;
    }

    /// The quarter view this one is at or heading to.
    pub fn rot(self) -> u8 {
        (self.target / FRAC_PI_2).round().rem_euclid(4.0) as u8
    }

    /// Whether the view is still moving toward its target angle or tilt.
    pub fn turning(self) -> bool {
        self.angle != self.target || self.tilt != self.target_tilt
    }

    /// Asks for a turn by `radians` from wherever the view is heading.
    pub fn turn_by(&mut self, radians: f32) {
        self.target += radians;
    }

    /// Heads for the nearest multiple of `step` radians.
    pub fn snap(&mut self, step: f32) {
        self.target = (self.target / step).round() * step;
    }

    /// Moves `angle` and `tilt` toward their targets; exact on arrival, and the angle
    /// stays within one turn.
    pub fn ease(&mut self, dt: f32) {
        let k = (dt * 12.0).min(1.0);
        let gap = self.target - self.angle;
        if gap.abs() < 0.002 {
            self.target = self.target.rem_euclid(TAU);
            self.angle = self.target;
        } else {
            self.angle += gap * k;
        }
        let (dh, db) = (
            self.target_tilt.tile_h - self.tilt.tile_h,
            self.target_tilt.block_h - self.tilt.block_h,
        );
        if dh.abs() + db.abs() < 0.05 {
            self.tilt = self.target_tilt;
        } else {
            self.tilt.tile_h += dh * k;
            self.tilt.block_h += db * k;
        }
    }

    /// Sine and cosine of the angle, exact at quarter turns so the classic views stay on
    /// whole texels.
    fn sin_cos(self) -> (f32, f32) {
        let (s, c) = self.angle.sin_cos();
        let clean = |v: f32| {
            if v.abs() < 1e-6 {
                0.0
            } else if (v.abs() - 1.0).abs() < 1e-6 {
                v.signum()
            } else {
                v
            }
        };
        (clean(s), clean(c))
    }

    /// World space to view space: a turn about z.
    fn turn(self, p: Vec3) -> Vec3 {
        let (s, c) = self.sin_cos();
        Vec3::new(p.x * c + p.y * s, -p.x * s + p.y * c, p.z)
    }

    fn unturn(self, p: Vec3) -> Vec3 {
        let (s, c) = self.sin_cos();
        Vec3::new(p.x * c - p.y * s, p.x * s + p.y * c, p.z)
    }

    /// Screen texels in x and y, depth toward the viewer in z. Linear, so it may be applied
    /// to offsets as well as positions.
    pub fn project(self, p: Vec3) -> Vec3 {
        let v = self.turn(p);
        Vec3::new(
            (v.x - v.y) * TILE_W / 2.0,
            -(v.x + v.y) * self.tilt.tile_h / 2.0 + v.z * self.tilt.block_h,
            v.x + v.y + v.z * self.tilt.rise(),
        )
    }

    /// [`View::project`] as a matrix: a mesh in world space drawn with this as its
    /// transform lands on screen, so turning the view never rebuilds a mesh.
    pub fn matrix(self) -> Mat3 {
        Mat3::from_cols(
            self.project(Vec3::X),
            self.project(Vec3::Y),
            self.project(Vec3::Z),
        )
    }

    /// The world point at height `z` that lands on `screen`.
    pub fn unproject(self, screen: Vec2, z: f32) -> Vec3 {
        let diff = screen.x / (TILE_W / 2.0);
        let sum = (z * self.tilt.block_h - screen.y) / (self.tilt.tile_h / 2.0);
        self.unturn(Vec3::new((sum + diff) / 2.0, (sum - diff) / 2.0, z))
    }

    /// World direction from any point straight toward the viewer.
    pub fn toward_viewer(self) -> Vec3 {
        self.unturn(Vec3::new(1.0, 1.0, self.tilt.rise()))
    }

    /// The two side faces of a block that face the camera most, as world normals:
    /// `(left, right)` on screen.
    pub fn sides(self) -> (IVec3, IVec3) {
        let axis = |v: Vec3| {
            let d = self.unturn(v).truncate();
            if d.x.abs() >= d.y.abs() {
                IVec3::new(d.x.signum() as i32, 0, 0)
            } else {
                IVec3::new(0, d.y.signum() as i32, 0)
            }
        };
        (axis(Vec3::Y), axis(Vec3::X))
    }

    /// The ground direction that moves a walker along a screen direction (x right, y up).
    pub fn ground_dir(self, screen: Vec2) -> Vec2 {
        let v = Vec3::new(screen.x - screen.y, -screen.x - screen.y, 0.0);
        self.unturn(v).truncate().normalize_or_zero()
    }

    /// Walks the view ray under `screen` from height `top` downward and returns the first
    /// cell for which `solid` holds.
    pub fn pick(self, screen: Vec2, top: i32, solid: impl Fn(IVec3) -> bool) -> Option<Hit> {
        let dir = -self.toward_viewer();
        let start = self.unproject(screen, top as f32) + dir * 1e-4;
        let mut cell = start.floor().as_ivec3();
        let mut step = IVec3::ZERO;
        // Ray distance to the next boundary on each axis, and between boundaries. An axis
        // the ray runs along never comes up.
        let mut next = Vec3::splat(f32::INFINITY);
        let mut gap = Vec3::splat(f32::INFINITY);
        for a in 0..3 {
            if dir[a].abs() < 1e-6 {
                continue;
            }
            step[a] = dir[a].signum() as i32;
            let frac = start[a] - cell[a] as f32;
            let ahead = if step[a] > 0 { 1.0 - frac } else { frac };
            next[a] = ahead / dir[a].abs();
            gap[a] = 1.0 / dir[a].abs();
        }
        let mut normal = IVec3::Z;
        for _ in 0..(top * 4 + 8) {
            if cell.z < 0 {
                return None;
            }
            if cell.z < top && solid(cell) {
                return Some(Hit {
                    block: cell,
                    normal,
                });
            }
            let a = if next.x <= next.y && next.x <= next.z {
                0
            } else if next.y <= next.z {
                1
            } else {
                2
            };
            cell[a] += step[a];
            next[a] += gap[a];
            normal = IVec3::ZERO;
            normal[a] = -step[a];
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POINTS: [Vec3; 5] = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(3.25, -7.5, 4.0),
        Vec3::new(-12.75, 9.125, 11.5),
        Vec3::new(100.5, 200.25, 31.0),
        Vec3::new(-0.5, -0.5, 0.25),
    ];

    /// The four quarter views and three in-between angles.
    fn views() -> impl Iterator<Item = View> {
        (0..4)
            .map(View::quarter)
            .chain([0.3, 1.0, 4.0].map(|angle| View {
                angle,
                target: angle,
                ..default()
            }))
    }

    #[test]
    fn unproject_inverts_project() {
        for view in views() {
            for p in POINTS {
                let s = view.project(p);
                let back = view.unproject(s.truncate(), p.z);
                assert!((back - p).length() < 1e-3, "{view:?} {p} -> {back}");
            }
        }
    }

    #[test]
    fn nearer_points_share_a_pixel_and_have_more_depth() {
        for view in views() {
            for p in POINTS {
                let near = p + view.toward_viewer() * 2.5;
                let (a, b) = (view.project(p), view.project(near));
                assert!((a.truncate() - b.truncate()).length() < 1e-3);
                assert!(b.z > a.z + 1.0);
            }
        }
    }

    #[test]
    fn default_view_matches_the_classic_layout() {
        let v = View::default();
        // +x runs down-right, +y down-left, +z straight up.
        assert_eq!(v.project(Vec3::X).truncate(), Vec2::new(16.0, -8.0));
        assert_eq!(v.project(Vec3::Y).truncate(), Vec2::new(-16.0, -8.0));
        assert_eq!(v.project(Vec3::Z).truncate(), Vec2::new(0.0, 16.0));
        assert_eq!(v.sides(), (IVec3::Y, IVec3::X));
        // Pressing "up" walks away from the viewer.
        let up = v.ground_dir(Vec2::Y);
        assert!(up.x < 0.0 && up.y < 0.0);
    }

    /// A floor up to z 3 and a pillar of two blocks at (2, 3).
    fn solid(c: IVec3) -> bool {
        c.z < 4 || (c.x == 2 && c.y == 3 && c.z < 6)
    }

    #[test]
    fn pick_finds_the_face_under_the_cursor() {
        for view in views() {
            let at = |p: Vec3| view.pick(view.project(p).truncate(), 32, solid);
            // Top of the pillar, off-centre so the ray avoids edges.
            let hit = at(Vec3::new(2.3, 3.6, 6.0)).unwrap();
            assert_eq!((hit.block, hit.normal), (IVec3::new(2, 3, 5), IVec3::Z));
            // The floor away from the pillar.
            let hit = at(Vec3::new(10.3, 10.6, 4.0)).unwrap();
            assert_eq!((hit.block, hit.normal), (IVec3::new(10, 10, 3), IVec3::Z));
            // Both visible sides of the pillar's upper block.
            let (left, right) = view.sides();
            for n in [left, right] {
                let t = Vec3::new(-n.y as f32, n.x as f32, 0.0);
                let p = Vec3::new(2.5, 3.5, 5.5) + n.as_vec3() * 0.5 + t * 0.2 + Vec3::Z * 0.1;
                let hit = at(p).unwrap();
                assert_eq!(
                    (hit.block, hit.normal),
                    (IVec3::new(2, 3, 5), n),
                    "{view:?}"
                );
            }
        }
    }

    #[test]
    fn the_matrix_is_the_projection() {
        for view in views() {
            for p in POINTS {
                assert!((view.matrix() * p - view.project(p)).length() < 1e-2);
            }
        }
    }

    #[test]
    fn turns_ease_onto_their_target_exactly() {
        let mut view = View::default();
        view.turn_by(FRAC_PI_2);
        assert!(view.turning());
        assert_eq!(view.rot(), 1);
        for _ in 0..200 {
            view.ease(1.0 / 60.0);
        }
        assert_eq!(view, View::quarter(1));
        // A quarter view lands on whole texels, like the default one.
        assert_eq!(view.project(Vec3::X).truncate(), Vec2::new(16.0, 8.0));
        // Turning past a full circle wraps, and a free angle snaps to the nearest quarter.
        view.turn_by(TAU);
        for _ in 0..200 {
            view.ease(1.0 / 60.0);
        }
        assert_eq!(view.rot(), 1);
        assert!(view.angle < TAU);
        let mut free = View {
            angle: 2.9,
            target: 2.9,
            ..default()
        };
        free.snap(FRAC_PI_2);
        assert_eq!(free.rot(), 2);
    }

    #[test]
    fn tilts_cycle_and_ease() {
        assert_eq!(Tilt::default().name(), "classic");
        assert_eq!(Tilt::default().next().name(), "high");
        assert_eq!(Tilt::named("top").unwrap().next().name(), "low");
        assert_eq!(Tilt::named("nope"), None);
        let mut view = View::default();
        view.set_tilt(Tilt::named("top").unwrap());
        assert!(view.turning());
        for _ in 0..200 {
            view.ease(1.0 / 60.0);
        }
        assert!(!view.turning());
        assert_eq!(view.tilt, Tilt::named("top").unwrap());
        // From the top the floor diamond is a square and a block hardly rises.
        assert_eq!(view.project(Vec3::X).truncate(), Vec2::new(16.0, -16.0));
        assert_eq!(view.project(Vec3::Z).truncate(), Vec2::new(0.0, 6.0));
    }

    #[test]
    fn pick_misses_when_nothing_is_there() {
        let view = View::default();
        assert_eq!(view.pick(Vec2::ZERO, 32, |_| false), None);
    }
}
