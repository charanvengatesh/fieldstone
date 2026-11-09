//! The isometric view. World space is a grid of unit blocks with z up; the screen is a 2:1
//! dimetric projection looking along `(-1, -1, -1)`. Because that direction is exact, a
//! point's distance toward the viewer is simply `x + y + z`, which every vertex carries as
//! its depth so the depth buffer resolves all occlusion.

use bevy::prelude::*;

/// Texels across a floor diamond.
pub const TILE_W: f32 = 32.0;
/// Texels down a floor diamond.
pub const TILE_H: f32 = 16.0;
/// Texels a block rises on screen.
pub const BLOCK_H: f32 = 16.0;

/// Which of the four diagonal directions the camera looks from.
#[derive(Resource, Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct View {
    pub rot: u8,
}

/// The block under a screen point and the face the view ray entered it through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub block: IVec3,
    pub normal: IVec3,
}

impl View {
    pub fn turned(self, steps: i32) -> View {
        View {
            rot: (self.rot as i32 + steps).rem_euclid(4) as u8,
        }
    }

    /// World space to view space: a quarter turn about z per `rot`.
    fn turn(self, p: Vec3) -> Vec3 {
        match self.rot % 4 {
            0 => p,
            1 => Vec3::new(p.y, -p.x, p.z),
            2 => Vec3::new(-p.x, -p.y, p.z),
            _ => Vec3::new(-p.y, p.x, p.z),
        }
    }

    fn unturn(self, p: Vec3) -> Vec3 {
        View {
            rot: (4 - self.rot % 4) % 4,
        }
        .turn(p)
    }

    /// Screen texels in x and y, depth toward the viewer in z. Linear, so it may be applied
    /// to offsets as well as positions.
    pub fn project(self, p: Vec3) -> Vec3 {
        let v = self.turn(p);
        Vec3::new(
            (v.x - v.y) * TILE_W / 2.0,
            -(v.x + v.y) * TILE_H / 2.0 + v.z * BLOCK_H,
            v.x + v.y + v.z,
        )
    }

    /// The world point at height `z` that lands on `screen`.
    pub fn unproject(self, screen: Vec2, z: f32) -> Vec3 {
        let diff = screen.x / (TILE_W / 2.0);
        let sum = (z * BLOCK_H - screen.y) / (TILE_H / 2.0);
        self.unturn(Vec3::new((sum + diff) / 2.0, (sum - diff) / 2.0, z))
    }

    /// World direction from any point straight toward the viewer.
    pub fn toward_viewer(self) -> Vec3 {
        self.unturn(Vec3::ONE)
    }

    /// The two side faces of a block the camera can see, as world normals: `(left, right)`
    /// on screen.
    pub fn sides(self) -> (IVec3, IVec3) {
        (
            self.unturn(Vec3::Y).round().as_ivec3(),
            self.unturn(Vec3::X).round().as_ivec3(),
        )
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
        let step = dir.signum().as_ivec3();
        // Distance along the ray to the next boundary on each axis; every component of
        // `dir` has magnitude 1, so distances are plain offsets.
        let mut next = Vec3::ZERO;
        for a in 0..3 {
            let frac = start[a] - cell[a] as f32;
            next[a] = if step[a] > 0 { 1.0 - frac } else { frac };
        }
        let mut normal = IVec3::Z;
        for _ in 0..(top * 3 + 8) {
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
            next[a] += 1.0;
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

    fn views() -> impl Iterator<Item = View> {
        (0..4).map(|rot| View { rot })
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
    fn pick_misses_when_nothing_is_there() {
        let view = View::default();
        assert_eq!(view.pick(Vec2::ZERO, 32, |_| false), None);
    }
}
