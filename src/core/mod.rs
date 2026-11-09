pub mod defs;
pub mod packs;
pub mod paths;
pub mod registry;
pub mod settings;

use bevy::prelude::*;

pub use defs::*;
pub use registry::Registry;

/// Deterministic hash of a cell in 0..1, the only source of randomness in world generation,
/// so a seed always regenerates the same world.
pub fn hash01(seed: u32, x: i32, y: i32, salt: u32) -> f32 {
    let mut h = seed ^ 0x9E37_79B9;
    for v in [x as u32, y as u32, salt] {
        h ^= v.wrapping_add(0x7F4A_7C15).wrapping_mul(0x8532_EBF5);
        h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
        h ^= h >> 12;
    }
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// True while the pointer is over any UI node; world click handlers stay out of the way.
#[derive(Resource, Default)]
pub struct UiHovered(pub bool);

/// Gameplay stops while true.
#[derive(Resource, Default)]
pub struct Paused(pub bool);

pub fn not_paused(paused: Res<Paused>) -> bool {
    !paused.0
}

/// A short line shown top-center for a few seconds.
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

pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Paused>()
            .init_resource::<UiHovered>()
            .init_resource::<Toast>()
            .add_systems(Update, toast_tick);
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
