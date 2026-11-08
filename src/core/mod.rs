pub mod defs;
pub mod packs;
pub mod registry;
pub mod save;
pub mod time;

use bevy::prelude::*;

pub use defs::*;
pub use registry::Registry;

/// Deterministic per-cell hash in 0..1, used for scatter and loot so worlds regenerate identically.
pub fn hash01(seed: u32, x: i32, y: i32, salt: u32) -> f32 {
    let mut h = seed ^ 0x9E37_79B9;
    for v in [x as u32, y as u32, salt] {
        h ^= v.wrapping_add(0x7F4A_7C15).wrapping_mul(0x8532_EBF5);
        h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
        h ^= h >> 12;
    }
    (h >> 8) as f32 / (1u32 << 24) as f32
}

pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<time::GameTime>()
            .add_systems(Update, time::tick);
    }
}
