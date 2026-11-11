//! The camera: where it looks from (yaw and pitch about a focus point), how it maps the
//! world to the picture (orthographic by default, a perspective toggle), and how it
//! follows the player (a dead zone so small steps do not drag the picture, a lean toward
//! the pointer, a pan scripts can hold, a zoom that keeps the point under the pointer
//! still). Everything that turns a world position into a pixel or a pixel into a ray goes
//! through [`Cam::project`] and [`Cam::ray`], on the CPU, with the same matrices the GPU
//! camera gets from [`Cam::pose`]; the picker, the streamer, the follow logic and the
//! software canvas all use them.
//!
//! Picture coordinates are "texels": pixels of the render target, origin at the middle, x
//! right, y up. One block is [`TEXELS_PER_BLOCK`] texels across at the focus.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

use bevy::prelude::*;

use crate::core::PointerCaptured;
use crate::core::settings::Settings;
use crate::interact::Cursor;
use crate::world::Focus;

/// Texels across one block, at the focus plane.
pub const TEXELS_PER_BLOCK: f32 = 16.0;
/// How far behind the focus the eye sits: well clear of the world, inside the far plane.
const EYE_DISTANCE: f32 = 220.0;
/// Vertical field of view of the perspective projection.
const FOV: f32 = 35.0_f32.to_radians();
/// Half size of the box the player may move in before the camera follows, in texels.
pub const DEAD_ZONE: Vec2 = Vec2::new(20.0, 12.0);
/// Seconds a let-go pan holds still before drifting home.
const PAN_HOLD: f32 = 1.5;
/// How far the camera leans toward the pointer, as a share of the pointer's offset, and
/// the most it leans, in texels.
const PEEK_SHARE: f32 = 0.12;
const PEEK_MAX: Vec2 = Vec2::new(48.0, 28.0);

/// Pitch presets by name, in degrees above the ground: `classic` (30 degrees, the true
/// dimetric angle) draws ground diamonds twice as wide as they are tall.
pub const PITCHES: [(&str, f32); 4] = [
    ("low", 20.0),
    ("classic", 30.0),
    ("high", 45.0),
    ("top", 60.0),
];

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Cam {
    /// Compass direction the camera looks *from*, radians; 45 degrees is the classic
    /// view from the (+x, +y) side. Eased toward `target_yaw`.
    pub yaw: f32,
    pub target_yaw: f32,
    /// Angle above the ground plane, radians, eased toward `target_pitch`.
    pub pitch: f32,
    pub target_pitch: f32,
    pub perspective: bool,
    /// The world point in the middle of the picture.
    pub focus: Vec3,
    /// Screen pixels per texel: what is shown now (eases) and the resting value.
    pub magnify: f32,
    pub magnify_target: f32,
    /// Offset a script holds, in texels along the picture's axes; eases back when let go.
    pub pan: Vec2,
    pan_target: Vec2,
    pan_held: bool,
    /// Lean toward the pointer, in texels.
    pub peek: Vec2,
    /// The dead zone's anchor, a world point.
    settled: Vec3,
    pub peek_on: bool,
    pub dead_zone_on: bool,
    /// How fast a let-go pan drifts home, per second.
    pub pan_return: f32,
    pan_rest: f32,
    /// Pixel scale the magnification was last derived from.
    level: u32,
    /// Pointer offset to zoom about on the next update, if a script named one.
    zoom_about: Option<Vec2>,
    /// The player's offset from the anchor on the picture last frame, so a turn of the
    /// camera can keep it there.
    last_offset: Vec2,
    started: bool,
}

impl Default for Cam {
    fn default() -> Self {
        Self::quarter(0)
    }
}

impl Cam {
    /// The classic view from one of the four compass quarters, at the classic pitch.
    pub fn quarter(rot: u8) -> Self {
        let yaw = FRAC_PI_4 + (rot % 4) as f32 * FRAC_PI_2;
        let pitch = PITCHES[1].1.to_radians();
        Self {
            yaw,
            target_yaw: yaw,
            pitch,
            target_pitch: pitch,
            perspective: false,
            focus: Vec3::ZERO,
            magnify: 3.0,
            magnify_target: 3.0,
            pan: Vec2::ZERO,
            pan_target: Vec2::ZERO,
            pan_held: false,
            peek: Vec2::ZERO,
            settled: Vec3::ZERO,
            peek_on: true,
            dead_zone_on: true,
            pan_return: 0.8,
            pan_rest: 0.0,
            level: 0,
            zoom_about: None,
            last_offset: Vec2::ZERO,
            started: false,
        }
    }

    /// The compass quarter the camera is at or heading to, 0 to 3.
    pub fn rot(&self) -> u8 {
        ((self.target_yaw - FRAC_PI_4) / FRAC_PI_2)
            .round()
            .rem_euclid(4.0) as u8
    }

    pub fn turning(&self) -> bool {
        self.yaw != self.target_yaw || self.pitch != self.target_pitch
    }

    pub fn turn_by(&mut self, radians: f32) {
        self.target_yaw += radians;
    }

    /// Heads for the nearest yaw that is a multiple of `step` from the classic quarter.
    pub fn snap(&mut self, step: f32) {
        self.target_yaw = ((self.target_yaw - FRAC_PI_4) / step).round() * step + FRAC_PI_4;
    }

    pub fn set_pitch(&mut self, radians: f32) {
        self.target_pitch = radians.clamp(5.0_f32.to_radians(), 89.0_f32.to_radians());
    }

    /// The preset name nearest the pitch the camera is heading to.
    pub fn pitch_name(&self) -> &'static str {
        let deg = self.target_pitch.to_degrees();
        PITCHES
            .iter()
            .min_by(|a, b| (a.1 - deg).abs().total_cmp(&(b.1 - deg).abs()))
            .map(|p| p.0)
            .unwrap_or("classic")
    }

    pub fn pitch_named(name: &str) -> Option<f32> {
        PITCHES
            .iter()
            .find(|p| p.0 == name)
            .map(|p| p.1.to_radians())
    }

    /// The preset after the one the camera is nearest to.
    pub fn next_pitch(&self) -> f32 {
        let i = PITCHES
            .iter()
            .position(|p| p.0 == self.pitch_name())
            .unwrap_or(1);
        PITCHES[(i + 1) % PITCHES.len()].1.to_radians()
    }

    /// Moves yaw, pitch and magnification toward their targets; exact on arrival.
    pub fn ease(&mut self, dt: f32) {
        let k = (dt * 12.0).min(1.0);
        let gap = self.target_yaw - self.yaw;
        if gap.abs() < 0.002 {
            self.target_yaw = self.target_yaw.rem_euclid(TAU);
            self.yaw = self.target_yaw;
        } else {
            self.yaw += gap * k;
        }
        let gap = self.target_pitch - self.pitch;
        if gap.abs() < 0.001 {
            self.pitch = self.target_pitch;
        } else {
            self.pitch += gap * k;
        }
    }

    /// Unit vector from the focus toward the eye.
    pub fn toward_eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(cy * cp, sy * cp, sp)
    }

    /// The picture's right and up directions in the world.
    pub fn axes(&self) -> (Vec3, Vec3) {
        let back = self.toward_eye();
        let right = Vec3::Z.cross(back).normalize();
        let up = back.cross(right).normalize();
        (right, up)
    }

    /// Texels per block at the focus plane.
    pub fn zoom(&self) -> f32 {
        TEXELS_PER_BLOCK
    }

    /// The ground direction a picture-axis input moves along: `screen` (x right, y up)
    /// to a unit vector on the ground, so walking "up" goes away from the eye.
    pub fn ground_dir(&self, screen: Vec2) -> Vec2 {
        let (right, _) = self.axes();
        let right = right.truncate().normalize_or_zero();
        let away = -self.toward_eye().truncate().normalize_or_zero();
        (right * screen.x + away * screen.y).normalize_or_zero()
    }

    /// The world axes nearest the picture's "away" (up the screen) and "right"
    /// directions on the ground, as unit cell steps.
    pub fn sides(&self) -> (IVec3, IVec3) {
        let axis = |d: Vec2| {
            if d.x.abs() >= d.y.abs() {
                IVec3::new(d.x.signum() as i32, 0, 0)
            } else {
                IVec3::new(0, d.y.signum() as i32, 0)
            }
        };
        (
            axis(self.ground_dir(Vec2::Y)),
            axis(self.ground_dir(Vec2::X)),
        )
    }

    /// The first solid cell under a texel of a `target`-sized picture.
    pub fn pick(
        &self,
        target: Vec2,
        texel: Vec2,
        solid: impl Fn(IVec3) -> bool,
    ) -> Option<crate::world::Hit> {
        let (origin, dir) = self.ray(target, texel);
        crate::world::pick(origin, dir, 2.0 * EYE_DISTANCE + 200.0, solid)
    }

    /// A texel offset along the picture's axes as a displacement on the ground plane (the
    /// focus never leaves the player's height): sideways along the picture's right, up the
    /// picture along the ground away from the eye, stretched by the pitch.
    pub fn slide(&self, texels: Vec2) -> Vec3 {
        let (right, _) = self.axes();
        let away = -self.toward_eye().truncate().normalize_or_zero().extend(0.0);
        let stretch = 1.0 / self.pitch.sin().max(0.1);
        (right * texels.x + away * texels.y * stretch) / self.zoom()
    }

    /// Where the camera looks: the anchor plus the lean and the pan.
    pub fn center(&self) -> Vec3 {
        self.settled + self.slide(self.peek + self.pan)
    }

    /// In perspective, how far the eye must sit for the focus plane to show `target_h`
    /// texels at [`TEXELS_PER_BLOCK`] per block.
    fn perspective_distance(&self, target_h: f32) -> f32 {
        let half_height_blocks = target_h / self.zoom() / 2.0;
        (half_height_blocks / (FOV / 2.0).tan()).max(4.0)
    }

    /// The view matrix (world to camera space, looking down -z, up is world z).
    pub fn view_matrix(&self, target: Vec2) -> Mat4 {
        let eye = if self.perspective {
            self.focus + self.toward_eye() * self.perspective_distance(target.y)
        } else {
            self.focus + self.toward_eye() * EYE_DISTANCE
        };
        Transform::from_translation(eye)
            .looking_at(self.focus, Vec3::Z)
            .to_matrix()
            .inverse()
    }

    /// The projection for a render target `target` texels wide and high.
    pub fn projection_matrix(&self, target: Vec2) -> Mat4 {
        if self.perspective {
            bevy::math::proj::perspective(FOV, target.x / target.y, 0.1, 2000.0)
        } else {
            let (hw, hh) = (target.x / self.zoom() / 2.0, target.y / self.zoom() / 2.0);
            bevy::math::proj::orthographic(-hw, hw, -hh, hh, 0.1, 2000.0)
        }
    }

    /// A world point as texels from the middle of a `target`-sized picture (x right, y up)
    /// and its view depth, greater nearer the eye.
    pub fn project(&self, target: Vec2, p: Vec3) -> Vec3 {
        let view = self.view_matrix(target);
        let clip = self.projection_matrix(target) * view * p.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        let depth = (view * p.extend(1.0)).z;
        Vec3::new(ndc.x * target.x / 2.0, ndc.y * target.y / 2.0, depth)
    }

    /// The ray through a texel of the picture: origin and unit direction, away from the eye.
    pub fn ray(&self, target: Vec2, texel: Vec2) -> (Vec3, Vec3) {
        let inverse = (self.projection_matrix(target) * self.view_matrix(target)).inverse();
        let ndc = Vec2::new(texel.x / target.x * 2.0, texel.y / target.y * 2.0);
        let near = inverse * Vec4::new(ndc.x, ndc.y, 0.0, 1.0);
        let far = inverse * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let (near, far) = (near.truncate() / near.w, far.truncate() / far.w);
        (near, (far - near).normalize())
    }

    /// The world point at height `z` under a texel.
    pub fn unproject(&self, target: Vec2, texel: Vec2, z: f32) -> Vec3 {
        let (o, d) = self.ray(target, texel);
        if d.z.abs() < 1e-6 {
            return o.with_z(z);
        }
        o + d * ((z - o.z) / d.z)
    }

    /// Bevy's transform and projection for this camera on a `target`-sized picture. Until
    /// the picture is its own render target, the window is the picture at `magnify` window
    /// pixels per texel, which the orthographic scale takes in.
    pub fn pose(&self, target: Vec2) -> (Transform, Projection) {
        let eye = if self.perspective {
            self.focus + self.toward_eye() * self.perspective_distance(target.y)
        } else {
            self.focus + self.toward_eye() * EYE_DISTANCE
        };
        let transform = Transform::from_translation(eye).looking_at(self.focus, Vec3::Z);
        let projection = if self.perspective {
            Projection::Perspective(PerspectiveProjection {
                fov: FOV,
                near: 0.1,
                far: 2000.0,
                ..default()
            })
        } else {
            Projection::Orthographic(OrthographicProjection {
                scale: 1.0 / (self.zoom() * self.magnify.max(1e-3)),
                near: 0.1,
                far: 2000.0,
                ..OrthographicProjection::default_3d()
            })
        };
        (transform, projection)
    }

    /// Adds to the held pan this frame; it stays while something calls this every frame.
    pub fn pan_by(&mut self, delta: Vec2) {
        self.pan_target += delta;
        self.pan_held = true;
    }

    /// Asks the next update to zoom about this texel offset from the middle rather than
    /// the real pointer.
    pub fn zoom_about(&mut self, offset: Vec2) {
        self.zoom_about = Some(offset);
    }
}

fn ease2(from: Vec2, to: Vec2, dt: f32, rate: f32) -> Vec2 {
    let k = (dt * rate).min(1.0);
    let next = from + (to - from) * k;
    if next.distance(to) < 0.05 { to } else { next }
}

/// Follows the player: the dead zone, the lean, the pan, the zoom, and the eased
/// magnification. Picture-space quantities are texels; the focus comes out in the world.
pub fn follow(
    time: Res<Time<Real>>,
    focus: Res<Focus>,
    cursor: Res<Cursor>,
    captured: Res<PointerCaptured>,
    settings: Res<Settings>,
    picture: Res<crate::world::stream::Picture>,
    mut cam: ResMut<Cam>,
) {
    let dt = time.delta_secs();
    let target = picture.size;
    if !cam.started {
        cam.started = true;
        cam.settled = focus.0;
        cam.focus = focus.0;
        cam.level = settings.pixel_scale;
        cam.magnify = settings.pixel_scale.clamp(1, 6) as f32;
        cam.magnify_target = cam.magnify;
    }

    // Zoom: a new resting level eases the magnification, and the point under the pointer
    // stays where it is by shifting the pan.
    let level = settings.pixel_scale.clamp(1, 6);
    if level != cam.level {
        let new = level as f32;
        let about = cam
            .zoom_about
            .take()
            .or_else(|| cursor.0.filter(|_| !captured.0))
            .unwrap_or(Vec2::ZERO);
        // The pointer's texel offset shrinks by old/new; the pan takes the difference.
        let shift = about * (1.0 - cam.magnify_target / new);
        cam.pan += shift;
        cam.pan_target += shift;
        cam.pan_rest = 0.0;
        cam.magnify_target = new;
        cam.level = level;
    }
    cam.zoom_about = None;
    let m = cam.magnify + (cam.magnify_target - cam.magnify) * (dt * 14.0).min(1.0);
    cam.magnify = if (m - cam.magnify_target).abs() < 0.002 {
        cam.magnify_target
    } else {
        m
    };

    // While the camera turns, the player keeps their place on the picture: the anchor is
    // re-laid under them through the new axes before the dead zone is measured.
    if cam.turning() {
        let keep = cam.last_offset;
        let slide = cam.slide(keep);
        let moved = focus.0 - slide - cam.settled;
        cam.settled += moved;
        // The focus takes the same move at once; easing it would let the player drift.
        cam.focus += moved;
    }

    // Dead zone: the anchor only moves when the player leaves the box around it, measured
    // on the picture.
    if cam.dead_zone_on {
        let d = (cam.project(target, focus.0) - cam.project(target, cam.settled)).truncate();
        let excess = Vec2::new(
            (d.x.abs() - DEAD_ZONE.x).max(0.0) * d.x.signum(),
            (d.y.abs() - DEAD_ZONE.y).max(0.0) * d.y.signum(),
        );
        if excess != Vec2::ZERO {
            let slide = cam.slide(excess);
            cam.settled += slide;
        }
    } else {
        cam.settled = focus.0;
    }

    // Peek toward the pointer, unless a script has the pointer.
    let want_peek = match cursor.0 {
        Some(o) if cam.peek_on && !captured.0 => (o * PEEK_SHARE).clamp(-PEEK_MAX, PEEK_MAX),
        _ => Vec2::ZERO,
    };
    cam.peek = ease2(cam.peek, want_peek, dt, 8.0);

    // Pan: held by a script this frame, or drifting home after a rest.
    if cam.pan_held {
        cam.pan_rest = 0.0;
    } else {
        cam.pan_rest += dt;
        if cam.pan_rest > PAN_HOLD {
            let rate = cam.pan_return;
            cam.pan_target = ease2(cam.pan_target, Vec2::ZERO, dt, rate);
        }
    }
    cam.pan_held = false;
    cam.pan = ease2(cam.pan, cam.pan_target, dt, 10.0);

    cam.last_offset = (cam.project(target, focus.0) - cam.project(target, cam.settled)).truncate();

    let wanted = cam.center();
    let k = (dt * 10.0).min(1.0);
    let next = cam.focus + (wanted - cam.focus) * k;
    cam.focus = if next.distance(wanted) < 0.05 / cam.zoom() {
        wanted
    } else {
        next
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Vec2 = Vec2::new(1280.0, 800.0);

    #[test]
    fn projection_and_ray_round_trip_in_both_projections() {
        for perspective in [false, true] {
            let mut cam = Cam::quarter(1);
            cam.perspective = perspective;
            cam.focus = Vec3::new(10.0, -4.0, 12.0);
            cam.set_pitch(35.0_f32.to_radians());
            cam.pitch = cam.target_pitch;
            for p in [
                Vec3::new(12.5, -3.5, 13.0),
                Vec3::new(3.0, 2.0, 9.0),
                Vec3::new(10.0, -4.0, 12.0),
            ] {
                let s = cam.project(T, p);
                let (o, d) = cam.ray(T, s.truncate());
                // The ray passes through the point.
                let t = (p - o).dot(d);
                let miss = (o + d * t).distance(p);
                assert!(
                    miss < 1e-2,
                    "perspective {perspective}: {p} missed by {miss}"
                );
                // And unprojecting at the point's height lands on it.
                let back = cam.unproject(T, s.truncate(), p.z);
                assert!(back.distance(p) < 1e-2, "{back} vs {p}");
            }
            // The focus sits in the middle of the picture.
            let s = cam.project(T, cam.focus);
            assert!(s.truncate().length() < 1e-3, "{s}");
        }
    }

    #[test]
    fn a_block_is_sixteen_texels_across_and_the_classic_pitch_makes_two_to_one_diamonds() {
        let cam = Cam::quarter(0);
        // Along the picture's right axis a block spans TEXELS_PER_BLOCK texels.
        let (right, _) = cam.axes();
        let a = cam.project(T, Vec3::ZERO);
        let b = cam.project(T, right);
        assert!((b.x - a.x - TEXELS_PER_BLOCK).abs() < 1e-3, "{}", b.x - a.x);
        // A ground diamond: the x axis of the world runs diagonally; its vertical extent
        // on the picture is half its horizontal one at the classic pitch.
        let px = cam.project(T, Vec3::X) - a;
        let ratio = px.y.abs() / px.x.abs();
        assert!((ratio - 0.5).abs() < 0.01, "{ratio}");
        // Nearer things have greater depth.
        let near = cam.project(T, cam.toward_eye() * 3.0);
        assert!(near.z > a.z);
    }

    #[test]
    fn yaw_quarters_and_pitch_presets() {
        let mut cam = Cam::quarter(2);
        assert_eq!(cam.rot(), 2);
        cam.turn_by(FRAC_PI_2);
        assert_eq!(cam.rot(), 3);
        cam.turn_by(0.3);
        cam.snap(FRAC_PI_2);
        assert_eq!(cam.rot(), 3);
        assert_eq!(cam.pitch_name(), "classic");
        cam.set_pitch(Cam::pitch_named("top").unwrap());
        assert_eq!(cam.pitch_name(), "top");
        assert!((cam.next_pitch() - Cam::pitch_named("low").unwrap()).abs() < 1e-6);
        for _ in 0..200 {
            cam.ease(1.0 / 60.0);
        }
        assert!(!cam.turning());
    }

    fn world() -> World {
        let mut w = World::new();
        w.init_resource::<Time<Real>>();
        w.insert_resource(Focus(Vec3::new(0.5, 0.5, 10.0)));
        w.init_resource::<Cursor>();
        w.init_resource::<PointerCaptured>();
        w.insert_resource(Settings::default());
        w.init_resource::<crate::world::stream::Picture>();
        w.init_resource::<Cam>();
        w
    }

    fn run(w: &mut World, frames: usize) {
        let mut sys = IntoSystem::into_system(follow);
        sys.initialize(w);
        for _ in 0..frames {
            w.resource_mut::<Time<Real>>()
                .advance_by(std::time::Duration::from_millis(16));
            sys.run((), w).unwrap();
            sys.apply_deferred(w);
        }
    }

    fn centre_px(w: &World) -> Vec2 {
        let cam = w.resource::<Cam>();
        cam.project(T, cam.focus).truncate()
    }

    #[test]
    fn small_steps_stay_inside_the_dead_zone_and_big_ones_pull_the_camera() {
        let mut w = world();
        run(&mut w, 2);
        let start = w.resource::<Cam>().focus;
        w.resource_mut::<Focus>().0 += Vec3::new(0.3, 0.0, 0.0);
        run(&mut w, 30);
        assert_eq!(w.resource::<Cam>().focus, start);
        w.resource_mut::<Focus>().0 += Vec3::new(5.0, 0.0, 0.0);
        run(&mut w, 120);
        let moved = w.resource::<Cam>().focus - start;
        assert!(moved.x > 2.5, "{moved}");
    }

    #[test]
    fn the_camera_leans_toward_the_pointer_and_back() {
        let mut w = world();
        run(&mut w, 2);
        w.resource_mut::<Cursor>().0 = Some(Vec2::new(300.0, 0.0));
        run(&mut w, 90);
        let peek = w.resource::<Cam>().peek;
        assert!((peek.x - 36.0).abs() < 0.5, "{peek}");
        w.resource_mut::<PointerCaptured>().0 = true;
        run(&mut w, 90);
        assert_eq!(w.resource::<Cam>().peek, Vec2::ZERO);
        let _ = centre_px(&w);
    }

    #[test]
    fn a_held_pan_stays_and_a_released_one_drifts_home() {
        let mut w = world();
        run(&mut w, 2);
        for _ in 0..30 {
            w.resource_mut::<Cam>().pan_by(Vec2::new(2.0, 0.0));
            run(&mut w, 1);
        }
        assert!(w.resource::<Cam>().pan.x > 40.0);
        run(&mut w, 60);
        assert!(w.resource::<Cam>().pan.x > 40.0);
        run(&mut w, 700);
        assert_eq!(w.resource::<Cam>().pan, Vec2::ZERO);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_still() {
        let mut w = world();
        run(&mut w, 2);
        // The pointer sits 90 texels right of the middle at magnification 3; the world
        // point under it is what must stay put.
        let under_before = {
            let cam = w.resource::<Cam>();
            cam.unproject(T, Vec2::new(90.0, 0.0), 10.0)
        };
        w.resource_mut::<Cursor>().0 = Some(Vec2::new(90.0, 0.0));
        w.resource_mut::<Cam>().peek_on = false;
        w.resource_mut::<Cam>().pan_return = 0.0;
        w.resource_mut::<Settings>().pixel_scale = 6;
        run(&mut w, 60);
        let cam = w.resource::<Cam>();
        assert!((cam.magnify - 6.0).abs() < 1e-3);
        // Twice the magnification: the same screen pixel is 45 texels from the middle.
        let under_after = cam.unproject(T, Vec2::new(45.0, 0.0), 10.0);
        assert!(
            under_after.distance(under_before) < 0.1,
            "{under_after} vs {under_before}"
        );
    }
}
