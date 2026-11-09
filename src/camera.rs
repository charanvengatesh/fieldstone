//! The camera looks at [`Focus`] through the current [`View`]. Zoom is a whole number of
//! screen pixels per texel so block edges stay crisp.

use bevy::camera::visibility::RenderLayers;
use bevy::input::gestures::PinchGesture;
use bevy::prelude::*;

use crate::core::settings::{Keys, Settings};
use crate::world::Focus;
use crate::world::iso::View;
use crate::world::mesh::Grid;

/// Both cameras: the world camera and the overlay drawn on top of it.
#[derive(Component)]
pub struct MainCamera;

pub fn setup(settings: Res<Settings>, mut commands: Commands) {
    let projection = || {
        Projection::Orthographic(OrthographicProjection {
            scale: 1.0 / settings.pixel_scale.clamp(1, 6) as f32,
            ..OrthographicProjection::default_2d()
        })
    };
    commands.spawn((Camera2d, MainCamera, projection()));
    // The overlay has its own depth buffer, so what it draws is never hidden by blocks.
    commands.spawn((
        Camera2d,
        MainCamera,
        projection(),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(crate::player::OVERLAY_LAYER),
    ));
}

/// Centres the camera on the focus. Its z is the focus depth, so the visible depth range
/// (camera z plus or minus 1000) always surrounds what is on screen.
pub fn follow(
    focus: Res<Focus>,
    view: Res<View>,
    mut cams: Query<(&mut Transform, &Projection), With<MainCamera>>,
) {
    for (mut tf, proj) in &mut cams {
        let mut p = view.project(focus.0);
        if let Projection::Orthographic(o) = proj {
            // Snap to whole screen pixels so texels never straddle them and shimmer.
            let step = o.scale.max(1e-4);
            p.x = (p.x / step).round() * step;
            p.y = (p.y / step).round() * step;
        }
        tf.translation = p;
    }
}

/// Pinch on a trackpad or `-` / `=`: whole pixel scales only, a pinch accumulates until it
/// is worth a step.
pub fn zoom(
    mut pinches: MessageReader<PinchGesture>,
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    mut settings: ResMut<Settings>,
    mut pinch_acc: Local<f32>,
    mut cams: Query<&mut Projection, With<MainCamera>>,
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
        for mut proj in &mut cams {
            if let Projection::Orthographic(o) = &mut *proj {
                o.scale = 1.0 / settings.pixel_scale as f32;
            }
        }
    }
}

/// `Q` / `E` turn the view a quarter; `G` toggles the full block grid.
pub fn view_keys(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    mut view: ResMut<View>,
    mut grid: ResMut<Grid>,
) {
    if keys.just(&input, &keys.rotate_left) {
        *view = view.turned(1);
    }
    if keys.just(&input, &keys.rotate_right) {
        *view = view.turned(-1);
    }
    if keys.just(&input, &keys.grid) {
        grid.0 = !grid.0;
    }
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(
            Update,
            (view_keys, zoom, follow)
                .chain()
                .after(crate::player::PlayerSet)
                .before(crate::world::WorldSet),
        );
    }
}
