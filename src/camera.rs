//! The camera looks at [`Focus`] through the current [`View`]. Zoom is a whole number of
//! screen pixels per texel so block edges stay crisp.

use bevy::input::gestures::PinchGesture;
use bevy::prelude::*;

use crate::core::settings::{Keys, Settings};
use crate::look::Look;
use crate::world::stream::Viewport;

/// The world camera.
#[derive(Component)]
pub struct MainCamera;

pub fn setup(settings: Res<Settings>, mut commands: Commands) {
    let projection = || {
        Projection::Orthographic(OrthographicProjection {
            scale: 1.0 / settings.pixel_scale.clamp(1, 6) as f32,
            ..OrthographicProjection::default_2d()
        })
    };
    // No multisampling: it softens texel edges and lets edge pixels sample the tile next
    // door in the atlas.
    commands.spawn((Camera2d, MainCamera, projection(), Msaa::Off));
}

/// Puts the camera where [`Look`] says, at the focus depth so the visible depth range
/// (camera z plus or minus 1000) always surrounds what is on screen, snapped to whole
/// screen pixels so texels never straddle them and shimmer.
pub fn follow(
    look: Res<Look>,
    mut viewport: ResMut<Viewport>,
    mut cams: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
) {
    for (mut tf, mut proj) in &mut cams {
        if let Projection::Orthographic(o) = &mut *proj {
            if (o.scale - look.scale).abs() > 1e-6 {
                o.scale = look.scale;
            }
            let step = o.scale.max(1e-4);
            let half = o.area.half_size();
            if half.x > 1.0 && viewport.half != half {
                viewport.half = half;
            }
            tf.translation = Vec3::new(
                (look.center.x / step).round() * step,
                (look.center.y / step).round() * step,
                look.depth,
            );
        }
    }
}

/// Pinch on a trackpad or `-` / `=`: whole pixel scales only, a pinch accumulates until it
/// is worth a step. The camera eases to the new scale through [`Look`].
pub fn zoom(
    mut pinches: MessageReader<PinchGesture>,
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    mut settings: ResMut<Settings>,
    mut pinch_acc: Local<f32>,
) {
    let mut delta = 0i32;
    for p in pinches.read() {
        *pinch_acc += p.0;
    }
    while *pinch_acc > 0.25 {
        *pinch_acc -= 0.25;
        delta += 1;
    }
    while *pinch_acc < -0.25 {
        *pinch_acc += 0.25;
        delta -= 1;
    }
    if keys.just(&input, &keys.zoom_in) {
        delta += 1;
    }
    if keys.just(&input, &keys.zoom_out) {
        delta -= 1;
    }
    if delta != 0 {
        settings.pixel_scale = (settings.pixel_scale as i32 + delta).clamp(1, 6) as u32;
    }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        // In the background the game ticks ten times a second instead of flat out.
        app.insert_resource(bevy::winit::WinitSettings {
            focused_mode: bevy::winit::UpdateMode::Continuous,
            unfocused_mode: bevy::winit::UpdateMode::reactive_low_power(
                std::time::Duration::from_millis(100),
            ),
        })
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                zoom.before(crate::look::update),
                follow.after(crate::look::update),
            )
                .before(crate::world::WorldSet),
        );
    }
}
