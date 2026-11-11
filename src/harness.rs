//! Testing without a window. Scripts can inject input that the engine and other scripts
//! cannot tell from the real thing, fake the frame time and speed up the clock, and run
//! test functions as coroutines that wait for frames and check what happened.
//! `cargo run -- --test` loads every `tests/*.lua` in the packs and exits non-zero if any
//! check failed.

use bevy::input::InputSystems;
use bevy::input::gestures::RotationGesture;
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

use crate::interact::Cursor;

/// Input a script asked for, applied at the start of the next frame where the real input
/// systems would have put it, so every system sees it the way it sees a keyboard.
#[derive(Resource, Default)]
pub struct Injected {
    pub press: Vec<KeyCode>,
    pub release: Vec<KeyCode>,
    pub mouse_press: Vec<MouseButton>,
    pub mouse_release: Vec<MouseButton>,
    /// Pointer movement in screen pixels.
    pub motion: Vec2,
    /// Scroll in lines.
    pub scroll: f32,
    /// Trackpad twist in radians.
    pub twist: f32,
    /// Where the pointer is, in world texels, while a test holds it there.
    pub cursor: Option<Vec2>,
}

/// Measurements a test replaces: a frame time in milliseconds, as on a slow machine.
#[derive(Resource, Default)]
pub struct Fake {
    pub frame_ms: Option<f32>,
}

/// Gesture and pointer messages go in ahead of the input systems, which fold them into
/// the accumulated resources this frame.
pub fn inject_messages(
    mut injected: ResMut<Injected>,
    mut motion: MessageWriter<MouseMotion>,
    mut wheel: MessageWriter<MouseWheel>,
    mut twist: MessageWriter<RotationGesture>,
) {
    if injected.motion != Vec2::ZERO {
        motion.write(MouseMotion {
            delta: injected.motion,
        });
        injected.motion = Vec2::ZERO;
    }
    if injected.scroll != 0.0 {
        wheel.write(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: injected.scroll,
            window: Entity::PLACEHOLDER,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        injected.scroll = 0.0;
    }
    if injected.twist != 0.0 {
        twist.write(RotationGesture(injected.twist));
        injected.twist = 0.0;
    }
}

/// Buttons go in after the input systems have cleared last frame's edges, so a press is
/// "just pressed" for exactly one frame, like a real key.
pub fn inject_buttons(
    mut injected: ResMut<Injected>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut cursor: ResMut<Cursor>,
) {
    for k in injected.release.drain(..) {
        keys.release(k);
    }
    for k in injected.press.drain(..) {
        keys.press(k);
    }
    for b in injected.mouse_release.drain(..) {
        mouse.release(b);
    }
    for b in injected.mouse_press.drain(..) {
        mouse.press(b);
    }
    if let Some(at) = injected.cursor {
        cursor.0 = Some(at);
    }
}

/// Lets go of everything a test held, between tests.
pub fn release_all(world: &mut World) {
    world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
    world.resource_mut::<ButtonInput<MouseButton>>().reset_all();
    *world.resource_mut::<Injected>() = Injected::default();
    world.resource_mut::<Fake>().frame_ms = None;
    world
        .resource_mut::<Time<Virtual>>()
        .set_relative_speed(1.0);
    world.resource_mut::<crate::core::PointerCaptured>().0 = false;
}

pub struct HarnessPlugin;

impl Plugin for HarnessPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Injected>()
            .init_resource::<Fake>()
            .add_systems(
                PreUpdate,
                (
                    inject_messages.before(InputSystems),
                    inject_buttons.after(InputSystems),
                ),
            );
    }
}
