//! The Bevy camera is a copy of [`Cam`]: each frame it takes the pose and projection `Cam`
//! computes for the current picture size. Zoom steps the magnification setting; the pinch
//! and `-` / `=` land here.

use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::input::gestures::PinchGesture;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::cam::Cam;
use crate::core::settings::{Keys, Settings};
use crate::interact::Cursor;
use crate::world::stream::Picture;

/// The world camera.
#[derive(Component)]
pub struct MainCamera;

pub fn setup(mut commands: Commands) {
    // No multisampling (it softens texel edges) and no tonemapping (the LUT kinds need
    // textures this build leaves out; the shaders write final colour).
    commands.spawn((
        Camera3d::default(),
        MainCamera,
        Projection::Orthographic(OrthographicProjection::default_3d()),
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
    ));
}

/// Puts the camera where [`Cam`] says. The eye is snapped so the focus lands on a whole
/// texel, which keeps texels from straddling pixels and shimmering as the camera eases.
pub fn follow(
    cam: Res<Cam>,
    picture: Res<Picture>,
    settings: Res<Settings>,
    mut cams: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
    mut last: Local<Option<(Vec2, f32, bool)>>,
) {
    let (right, up) = cam.axes();
    let p = cam.project(picture.size, cam.focus);
    let snap = Vec2::new(p.x - p.x.round(), p.y - p.y.round());
    let nudge = (right * snap.x + up * snap.y) / cam.zoom();
    let mut snapped = *cam;
    snapped.focus -= nudge;
    // Picture pixels per texel: the magnification over the picture's own coarseness.
    let magnify = cam.magnify / settings.picture_scale.clamp(1, 4) as f32;
    let (transform, projection) = snapped.pose(picture.size, magnify);
    // The projection depends on the picture, the magnification and the kind only; writing
    // it marks the camera changed, so it is written when one of those moved.
    let key = (picture.size, magnify, cam.perspective);
    let fresh = *last != Some(key);
    *last = Some(key);
    for (mut tf, mut proj) in &mut cams {
        if *tf != transform {
            *tf = transform;
        }
        if fresh {
            *proj = projection.clone();
        }
    }
}

/// The pointer as texels from the middle of the picture, x right, y up. A texel is
/// `pixel_scale` physical pixels; the window reports the pointer in logical ones.
pub fn track_cursor(
    settings: Res<Settings>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cursor: ResMut<Cursor>,
) {
    cursor.0 = window.cursor_position().map(|p| {
        let centre = Vec2::new(window.width(), window.height()) / 2.0;
        let d = (p - centre) * window.scale_factor();
        Vec2::new(d.x, -d.y) / settings.picture_scale.clamp(1, 4) as f32
    });
}

/// Pinch on a trackpad or `-` / `=`: whole magnifications only, a pinch accumulates until
/// it is worth a step. The camera eases to the new magnification through [`Cam`].
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
                track_cursor.before(crate::interact::InteractSet),
                zoom.before(crate::cam::follow),
                follow.after(crate::cam::follow),
            )
                .before(crate::world::WorldSet),
        );
    }
}
