//! In-game clock. One day is `DAY_SECS` real seconds; `day_frac` drives lighting, spawns, crops.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const DAY_SECS: f64 = 600.0;

#[derive(Resource, Default, Serialize, Deserialize, Clone, Copy, Debug)]
pub struct GameTime {
    pub secs: f64,
}

impl GameTime {
    /// 0.0 at dawn, 0.5 at dusk, wraps at 1.0.
    pub fn day_frac(&self) -> f32 {
        (self.secs / DAY_SECS).fract() as f32
    }
    pub fn day(&self) -> u32 {
        (self.secs / DAY_SECS) as u32
    }
}

pub fn tick(time: Res<Time>, mut clock: ResMut<GameTime>) {
    clock.secs += time.delta_secs_f64();
}
