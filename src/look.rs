//! Where the camera looks, in screen texels: the player with a dead zone so small steps do
//! not drag the screen, a lean toward the pointer, a pan scripts can hold, and a zoom that
//! eases and keeps the point under the pointer still. Runs without a window too, so
//! tests can read it; `camera.rs` only copies the result onto the camera.

use bevy::prelude::*;

use crate::core::PointerCaptured;
use crate::core::settings::Settings;
use crate::interact::Cursor;
use crate::world::Focus;
use crate::world::iso::View;

/// Half size of the box the player may move in before the camera follows.
pub const DEAD_ZONE: Vec2 = Vec2::new(20.0, 12.0);
/// Seconds a let-go pan holds still before drifting home.
const PAN_HOLD: f32 = 1.5;
/// How far the camera leans toward the pointer, as a share of the pointer's offset, and
/// the most it leans.
const PEEK_SHARE: f32 = 0.12;
const PEEK_MAX: Vec2 = Vec2::new(48.0, 28.0);

#[derive(Resource)]
pub struct Look {
    /// Where the camera is, in texels, and the depth it sits at.
    pub center: Vec2,
    pub depth: f32,
    /// Screen pixels per texel: what is shown now (eases) and the resting value.
    pub scale: f32,
    pub scale_target: f32,
    /// Offset a script holds; eases back to nothing when let go.
    pub pan: Vec2,
    pan_target: Vec2,
    pan_held: bool,
    /// Lean toward the pointer.
    pub peek: Vec2,
    /// The dead zone's anchor.
    settled: Vec2,
    pub peek_on: bool,
    pub dead_zone_on: bool,
    /// How fast a let-go pan drifts home, per second; slow, so a zoom toward the pointer
    /// lingers while you look.
    pub pan_return: f32,
    /// Seconds since the pan was last pushed; it only drifts home after a rest.
    pan_rest: f32,
    /// Pixel scale the targets were last derived from.
    level: u32,
    /// Pointer offset to zoom about on the next update, if a script named one.
    zoom_about: Option<Vec2>,
    /// Where the focus projected last frame and the view it projected through, to tell a
    /// turn of the view from a walk.
    last_focus: Vec2,
    last_view: Option<View>,
    started: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            center: Vec2::ZERO,
            depth: 0.0,
            scale: 1.0 / 3.0,
            scale_target: 1.0 / 3.0,
            pan: Vec2::ZERO,
            pan_target: Vec2::ZERO,
            pan_held: false,
            peek: Vec2::ZERO,
            settled: Vec2::ZERO,
            peek_on: true,
            dead_zone_on: true,
            pan_return: 0.8,
            pan_rest: 0.0,
            level: 0,
            zoom_about: None,
            last_focus: Vec2::ZERO,
            last_view: None,
            started: false,
        }
    }
}

impl Look {
    /// Adds to the held pan this frame; it stays while something calls this every frame.
    pub fn pan_by(&mut self, delta: Vec2) {
        self.pan_target += delta;
        self.pan_held = true;
    }

    /// Asks the next update to zoom about this pointer offset (texels from the centre)
    /// rather than the real pointer.
    pub fn zoom_about(&mut self, offset: Vec2) {
        self.zoom_about = Some(offset);
    }

    /// Texels per screen pixel is `scale`; this is the offset of a screen point from the
    /// middle of the screen, in texels.
    pub fn pointer_offset(&self, cursor: &Cursor) -> Option<Vec2> {
        cursor.0.map(|p| p - self.center)
    }
}

fn ease(from: Vec2, to: Vec2, dt: f32, rate: f32) -> Vec2 {
    let k = (dt * rate).min(1.0);
    let next = from + (to - from) * k;
    if next.distance(to) < 0.05 { to } else { next }
}

pub fn update(
    time: Res<Time<Real>>,
    focus: Res<Focus>,
    view: Res<View>,
    cursor: Res<Cursor>,
    captured: Res<PointerCaptured>,
    settings: Res<Settings>,
    mut look: ResMut<Look>,
) {
    let dt = time.delta_secs();
    let p = view.project(focus.0);
    look.depth = p.z;
    if !look.started {
        look.started = true;
        look.settled = p.truncate();
        look.center = p.truncate();
        look.last_focus = p.truncate();
        look.level = settings.pixel_scale;
        look.scale = 1.0 / settings.pixel_scale.clamp(1, 6) as f32;
        look.scale_target = look.scale;
    }

    // While the view turns or tilts, the focus swings across the screen (the projection
    // pivots on the world origin, not on the player). The camera takes that swing in full
    // and at once, so the world turns about the player instead of the camera chasing them
    // round through the dead zone.
    let view_moved = look
        .last_view
        .is_some_and(|v| v.angle != view.angle || v.tilt != view.tilt);
    if view_moved {
        let swing = p.truncate() - look.last_focus;
        look.settled += swing;
        look.center += swing;
    }
    look.last_focus = p.truncate();
    look.last_view = Some(*view);

    // Zoom: a new resting level eases the scale, and the point under the pointer stays
    // where it is by shifting the pan.
    let level = settings.pixel_scale.clamp(1, 6);
    if level != look.level {
        let new_scale = 1.0 / level as f32;
        let about = look
            .zoom_about
            .take()
            .or_else(|| look.pointer_offset(&cursor).filter(|_| !captured.0))
            .unwrap_or(Vec2::ZERO);
        let shift = about * (1.0 - new_scale / look.scale_target);
        look.pan += shift;
        look.pan_target += shift;
        look.pan_rest = 0.0;
        look.scale_target = new_scale;
        look.level = level;
    }
    look.zoom_about = None;
    let s = look.scale + (look.scale_target - look.scale) * (dt * 14.0).min(1.0);
    look.scale = if (s - look.scale_target).abs() < 0.002 {
        look.scale_target
    } else {
        s
    };

    // Dead zone: the anchor only moves when the player leaves the box around it.
    if look.dead_zone_on {
        let d = p.truncate() - look.settled;
        let excess = Vec2::new(
            (d.x.abs() - DEAD_ZONE.x).max(0.0) * d.x.signum(),
            (d.y.abs() - DEAD_ZONE.y).max(0.0) * d.y.signum(),
        );
        look.settled += excess;
    } else {
        look.settled = p.truncate();
    }

    // Peek toward the pointer, unless a script has the pointer.
    let want_peek = match look.pointer_offset(&cursor) {
        Some(o) if look.peek_on && !captured.0 => (o * PEEK_SHARE).clamp(-PEEK_MAX, PEEK_MAX),
        _ => Vec2::ZERO,
    };
    look.peek = ease(look.peek, want_peek, dt, 8.0);

    // Pan: held by a script this frame, or drifting home.
    if look.pan_held {
        look.pan_rest = 0.0;
    } else {
        look.pan_rest += dt;
        if look.pan_rest > PAN_HOLD {
            let rate = look.pan_return;
            look.pan_target = ease(look.pan_target, Vec2::ZERO, dt, rate);
        }
    }
    look.pan_held = false;
    look.pan = ease(look.pan, look.pan_target, dt, 10.0);

    let target = look.settled + look.peek + look.pan;
    look.center = ease(look.center, target, dt, 10.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        let mut w = World::new();
        w.init_resource::<Time<Real>>();
        w.insert_resource(Focus(Vec3::new(0.5, 0.5, 10.0)));
        w.init_resource::<View>();
        w.init_resource::<Cursor>();
        w.init_resource::<PointerCaptured>();
        w.insert_resource(Settings::default());
        w.init_resource::<Look>();
        w
    }

    /// Runs `update` for `frames` at 60 per second.
    fn run(w: &mut World, frames: usize) {
        let mut sys = IntoSystem::into_system(update);
        sys.initialize(w);
        for _ in 0..frames {
            w.resource_mut::<Time<Real>>()
                .advance_by(std::time::Duration::from_millis(16));
            sys.run((), w).unwrap();
            sys.apply_deferred(w);
        }
    }

    #[test]
    fn a_turn_of_the_view_pivots_on_the_player() {
        let mut w = world();
        w.insert_resource(Focus(Vec3::new(40.5, -12.5, 10.0)));
        run(&mut w, 30);
        let offset = |w: &World| {
            let p = w
                .resource::<View>()
                .project(w.resource::<Focus>().0)
                .truncate();
            w.resource::<Look>().center - p
        };
        let before = offset(&w);
        w.resource_mut::<View>()
            .turn_by(std::f32::consts::FRAC_PI_2);
        for _ in 0..40 {
            // The view eases a step, then the camera reacts in the same frame order as
            // the game: ease_view runs before look::update.
            w.resource_mut::<View>().ease(1.0 / 60.0);
            run(&mut w, 1);
            let now = offset(&w);
            assert!(
                now.distance(before) < 0.5,
                "the player drifted on screen mid-turn: {before} -> {now}"
            );
        }
    }

    #[test]
    fn small_steps_stay_inside_the_dead_zone_and_big_ones_pull_the_camera() {
        let mut w = world();
        run(&mut w, 2);
        let start = w.resource::<Look>().center;
        // Half a block is well inside the box.
        w.resource_mut::<Focus>().0 += Vec3::new(0.3, 0.0, 0.0);
        run(&mut w, 30);
        assert_eq!(w.resource::<Look>().center, start);
        // Five blocks is not.
        w.resource_mut::<Focus>().0 += Vec3::new(5.0, 0.0, 0.0);
        run(&mut w, 120);
        let moved = w.resource::<Look>().center - start;
        assert!(moved.x > 40.0, "{moved}");
    }

    #[test]
    fn the_camera_leans_toward_the_pointer_and_back() {
        let mut w = world();
        run(&mut w, 2);
        let start = w.resource::<Look>().center;
        w.resource_mut::<Cursor>().0 = Some(start + Vec2::new(300.0, 0.0));
        run(&mut w, 90);
        // The lean shortens the pointer's offset from the centre as it goes, so it settles
        // at share * (300 - lean).
        let peek = w.resource::<Look>().peek;
        assert!((peek.x - 36.0 / 1.12).abs() < 0.5, "{peek}");
        // A captured pointer does not lean.
        w.resource_mut::<PointerCaptured>().0 = true;
        run(&mut w, 90);
        assert_eq!(w.resource::<Look>().peek, Vec2::ZERO);
    }

    #[test]
    fn a_held_pan_stays_and_a_released_one_drifts_home() {
        let mut w = world();
        run(&mut w, 2);
        for _ in 0..30 {
            w.resource_mut::<Look>().pan_by(Vec2::new(2.0, 0.0));
            run(&mut w, 1);
        }
        assert!(w.resource::<Look>().pan.x > 40.0);
        // It holds for a moment, then drifts home.
        run(&mut w, 60);
        assert!(w.resource::<Look>().pan.x > 40.0);
        run(&mut w, 700);
        assert_eq!(w.resource::<Look>().pan, Vec2::ZERO);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer_still() {
        let mut w = world();
        run(&mut w, 2);
        let centre = w.resource::<Look>().center;
        // The pointer sits 90 texels right of the middle at scale 1/3.
        w.resource_mut::<Cursor>().0 = Some(centre + Vec2::new(90.0, 0.0));
        w.resource_mut::<Look>().peek_on = false;
        w.resource_mut::<Look>().pan_return = 0.0;
        let under_before = centre + Vec2::new(90.0, 0.0);
        w.resource_mut::<Settings>().pixel_scale = 6;
        run(&mut w, 60);
        let look = w.resource::<Look>();
        assert!((look.scale - 1.0 / 6.0).abs() < 1e-4);
        // The same screen pixel now covers half the texels, so the pointer's texel is
        // 45 texels from the new centre.
        let under_after = look.center + Vec2::new(45.0, 0.0);
        assert!(
            under_after.distance(under_before) < 1.0,
            "{under_after} vs {under_before}"
        );
    }
}
