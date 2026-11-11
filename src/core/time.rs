//! The game clock. A day lasts `DAY_SECS` of play; `frac` runs from midnight at 0 through
//! dawn at a quarter, noon at a half and dusk at three quarters. The clock stops while the
//! game is paused and is kept in the save file.

use std::f32::consts::TAU;

use bevy::prelude::*;

/// Real seconds in one game day.
pub const DAY_SECS: f64 = 720.0;
/// Where a new game starts: mid-morning.
pub const START_FRAC: f64 = 0.38;

#[derive(Resource, Clone, Copy, Debug)]
pub struct GameTime {
    pub secs: f64,
}

impl Default for GameTime {
    fn default() -> Self {
        Self {
            secs: START_FRAC * DAY_SECS,
        }
    }
}

impl GameTime {
    /// Time of day in 0..1.
    pub fn frac(&self) -> f32 {
        (self.secs / DAY_SECS).rem_euclid(1.0) as f32
    }

    pub fn set_frac(&mut self, frac: f32) {
        let day = (self.secs / DAY_SECS).floor();
        self.secs = (day + frac.rem_euclid(1.0) as f64) * DAY_SECS;
    }

    pub fn day(&self) -> u32 {
        (self.secs / DAY_SECS).floor().max(0.0) as u32
    }
}

/// Height of the sun, -1 at midnight to 1 at noon, 0 at dawn and dusk.
pub fn sun_height(frac: f32) -> f32 {
    ((frac - 0.25) * TAU).sin()
}

/// How the world is lit at a time of day.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Daylight {
    /// Share of full sun, 0.12 at night to 1 at noon.
    pub level: f32,
    /// Colour of sunlight: warm when the sun is low, white at noon.
    pub sun: Vec3,
    /// Colour of the light that remains in shade and at night.
    pub ambient: Vec3,
}

fn smooth(lo: f32, hi: f32, v: f32) -> f32 {
    let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Daylight level at the dead of night.
pub const NIGHT: f32 = 0.06;

pub fn daylight(frac: f32) -> Daylight {
    let h = sun_height(frac);
    let level = NIGHT + (1.0 - NIGHT) * smooth(-0.12, 0.35, h);
    // Moonlight is blue, dusk and dawn are orange, the day is white.
    let sun = Vec3::new(0.55, 0.66, 1.0)
        .lerp(Vec3::new(1.0, 0.56, 0.30), smooth(-0.22, -0.04, h))
        .lerp(Vec3::new(1.0, 0.98, 0.93), smooth(0.02, 0.45, h));
    let ambient =
        Vec3::new(0.22, 0.30, 0.70).lerp(Vec3::new(0.80, 0.88, 1.08), smooth(-0.2, 0.3, h));
    Daylight {
        level,
        sun,
        ambient,
    }
}

/// The colour behind the world at a time of day: a day-sky blue down to a deep night.
pub fn sky_color(frac: f32) -> Vec3 {
    let h = sun_height(frac);
    let night = Vec3::new(0.06, 0.07, 0.16);
    let dusk = Vec3::new(0.34, 0.26, 0.42);
    let day = Vec3::new(0.36, 0.60, 0.92);
    night
        .lerp(dusk, smooth(-0.25, 0.0, h))
        .lerp(day, smooth(0.0, 0.4, h))
}

/// Keeps the clear colour at the sky colour, written only when it moves a visible step.
pub fn clear_color(clock: Res<GameTime>, mut clear: ResMut<ClearColor>) {
    let c = sky_color(clock.frac());
    let now = clear.0.to_linear();
    let moved = (Vec3::new(now.red, now.green, now.blue) - c)
        .abs()
        .max_element()
        > 1.0 / 255.0;
    if moved {
        clear.0 = Color::linear_rgb(c.x, c.y, c.z);
    }
}

pub fn tick(time: Res<Time>, mut clock: ResMut<GameTime>) {
    clock.secs += time.delta_secs_f64();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_wraps_and_keeps_its_day() {
        let mut t = GameTime::default();
        assert!((t.frac() - START_FRAC as f32).abs() < 1e-5);
        t.secs += DAY_SECS * 1.5;
        assert_eq!(t.day(), 1);
        t.set_frac(0.75);
        assert_eq!(t.day(), 1);
        assert!((t.frac() - 0.75).abs() < 1e-5);
    }

    #[test]
    fn noon_is_bright_white_and_midnight_dim_and_blue() {
        let noon = daylight(0.5);
        let midnight = daylight(0.0);
        let dusk = daylight(0.75);
        assert!((noon.level - 1.0).abs() < 1e-3);
        assert!((midnight.level - NIGHT).abs() < 1e-3);
        assert!(
            midnight.sun.z > midnight.sun.x,
            "moonlight is blue: {}",
            midnight.sun
        );
        let (day, dark) = (sky_color(0.5), sky_color(0.0));
        assert!(day.z > day.x && day.max_element() > 0.8, "day sky: {day}");
        assert!(dark.max_element() < 0.2, "night sky: {dark}");
        assert!(dusk.level > midnight.level && dusk.level < noon.level);
        assert!(dusk.sun.x > dusk.sun.z, "dusk sun is warm: {}", dusk.sun);
        assert!(midnight.ambient.z > midnight.ambient.x, "night is blue");
        // The curve is continuous around the clock: a 1/400 day step (under two seconds)
        // never jumps more than a twentieth of the range.
        let mut prev = daylight(0.0).level;
        for i in 1..=400 {
            let l = daylight(i as f32 / 400.0).level;
            assert!((l - prev).abs() < 0.05, "step {i}: {prev} to {l}");
            prev = l;
        }
    }
}
