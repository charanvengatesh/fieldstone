pub mod defs;
pub mod packs;
pub mod paths;
pub mod registry;
pub mod save;
pub mod settings;
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

/// Small xorshift generator for gameplay randomness (spawns, wander). World generation does
/// not use it; that stays deterministic through [`hash01`].
#[derive(Resource)]
pub struct Rng(pub u64);

impl Default for Rng {
    fn default() -> Self {
        Self(0x9E37_79B9_7F4A_7C15)
    }
}

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Uniform in 0..1.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
}

/// True while the pointer is over any UI node; world click handlers stay out of the way.
#[derive(Resource, Default)]
pub struct UiHovered(pub bool);

/// Gameplay stops while true; the clock and every gameplay set are gated on it.
#[derive(Resource, Default)]
pub struct Paused(pub bool);

pub fn not_paused(paused: Res<Paused>) -> bool {
    !paused.0
}

/// A short line shown top-center for a few seconds (bed, shrine, scripts).
#[derive(Resource, Default)]
pub struct Toast {
    pub text: String,
    pub left: f32,
}

impl Toast {
    pub fn show(&mut self, text: impl Into<String>, secs: f32) {
        self.text = text.into();
        self.left = secs;
    }
}

/// Set when the shrine (or a script) ends the game: day locks at noon, a card shows.
#[derive(Resource, Default)]
pub struct Ending(pub Option<String>);

pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<time::GameTime>()
            .init_resource::<Rng>()
            .init_resource::<Paused>()
            .init_resource::<UiHovered>()
            .init_resource::<Toast>()
            .init_resource::<Ending>()
            .add_systems(Update, (time::tick, toast_tick).run_if(not_paused));
    }
}

pub fn toast_tick(time: Res<Time>, mut toast: ResMut<Toast>) {
    if toast.left > 0.0 {
        toast.left -= time.delta_secs();
        if toast.left <= 0.0 {
            toast.text.clear();
        }
    }
}
