//! The sky: one quad behind the world, sized to the viewport and following the camera,
//! coloured by the time of day with a sun, a moon and stars (`shaders/sky.wgsl`).

use std::f32::consts::TAU;

use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::sprite_render::{Material2d, MeshMaterial2d};

use crate::camera::MainCamera;
use crate::core::settings::Settings;
use crate::core::time::{GameTime, daylight, sun_height};
use crate::world::stream::Viewport;

/// How far behind the camera's depth the sky sits; the visible range is 1000 either way.
const BEHIND: f32 = 900.0;

#[derive(ShaderType, Clone, Copy, Debug)]
pub struct SkyParams {
    /// Colour at the top of the screen.
    pub zenith: Vec4,
    /// Colour at the bottom.
    pub horizon: Vec4,
    /// Sun: x, y on screen 0..1, z radius in screen fractions, w daylight 0..1.
    pub sun: Vec4,
    /// Moon: x, y on screen, z radius, w how visible the stars are 0..1.
    pub moon: Vec4,
    /// x: screen pixels per texel, y: viewport width over height, z: seconds, w: bands.
    pub params: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct SkyArt {
    #[uniform(0)]
    pub sky: SkyParams,
}

impl Material2d for SkyArt {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/sky.wgsl".into()
    }
}

#[derive(Component)]
pub struct Sky;

/// Zenith and horizon colours for a time of day.
pub fn sky_colors(frac: f32) -> (Vec3, Vec3) {
    let h = sun_height(frac);
    let smooth = |lo: f32, hi: f32, v: f32| {
        let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let night_z = Vec3::new(0.04, 0.05, 0.12);
    let night_h = Vec3::new(0.10, 0.11, 0.22);
    let dusk_z = Vec3::new(0.22, 0.20, 0.42);
    let dusk_h = Vec3::new(0.96, 0.52, 0.30);
    let day_z = Vec3::new(0.36, 0.60, 0.92);
    let day_h = Vec3::new(0.78, 0.88, 0.98);
    // Night to dusk as the sun nears the horizon, dusk to day as it climbs.
    let up = smooth(-0.25, 0.0, h);
    let high = smooth(0.0, 0.4, h);
    let zenith = night_z.lerp(dusk_z, up).lerp(day_z, high);
    let horizon = night_h.lerp(dusk_h, up).lerp(day_h, high);
    (zenith, horizon)
}

/// Where the sun (or, half a day on, the moon) sits on screen: rises on the right, sets
/// on the left.
pub fn disc_position(frac: f32) -> Vec2 {
    let a = (frac - 0.25) * TAU;
    Vec2::new(0.5 + 0.42 * a.cos(), 0.22 + 0.62 * a.sin())
}

pub fn params(frac: f32, pixel_scale: f32, aspect: f32, secs: f32) -> SkyParams {
    let (zenith, horizon) = sky_colors(frac);
    let day = daylight(frac);
    let sun = disc_position(frac);
    let moon = disc_position(frac + 0.5);
    let stars = 1.0 - ((day.level - 0.12) / 0.3).clamp(0.0, 1.0);
    SkyParams {
        zenith: zenith.extend(1.0),
        horizon: horizon.extend(1.0),
        sun: Vec4::new(sun.x, sun.y, 0.045, day.level),
        moon: Vec4::new(moon.x, moon.y, 0.03, stars),
        params: Vec4::new(pixel_scale, aspect, secs, 8.0),
    }
}

pub fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyArt>>,
) {
    commands.spawn((
        Sky,
        Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial2d(materials.add(SkyArt {
            sky: params(0.5, 3.0, 16.0 / 9.0, 0.0),
        })),
        Transform::from_xyz(0.0, 0.0, -BEHIND),
    ));
}

/// Keeps the quad over the whole viewport, just inside the far end of the depth range,
/// and its colours at the current time.
pub fn follow(
    clock: Res<GameTime>,
    time: Res<Time>,
    settings: Res<Settings>,
    viewport: Res<Viewport>,
    cams: Query<&Transform, (With<MainCamera>, Without<Sky>)>,
    mut skies: Query<(&mut Transform, &MeshMaterial2d<SkyArt>), With<Sky>>,
    mut materials: ResMut<Assets<SkyArt>>,
) {
    let Ok(cam) = cams.single() else { return };
    let size = viewport.half * 2.0;
    for (mut tf, handle) in &mut skies {
        tf.translation = Vec3::new(
            cam.translation.x,
            cam.translation.y,
            cam.translation.z - BEHIND,
        );
        tf.scale = Vec3::new(size.x, size.y, 1.0);
        if let Some(mut m) = materials.get_mut(&handle.0) {
            m.sky = params(
                clock.frac(),
                settings.pixel_scale.clamp(1, 6) as f32,
                size.x / size.y.max(1.0),
                time.elapsed_secs(),
            );
        }
    }
}

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy::sprite_render::Material2dPlugin::<SkyArt>::default())
            .add_systems(Startup, setup)
            .add_systems(Update, follow.after(crate::camera::follow));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sky_is_blue_by_day_and_dark_by_night() {
        let (z, h) = sky_colors(0.5);
        assert!(z.z > z.x && h.max_element() > 0.7, "day: {z} {h}");
        let (z, h) = sky_colors(0.0);
        assert!(
            z.max_element() < 0.15 && h.max_element() < 0.25,
            "night: {z} {h}"
        );
        let (_, h) = sky_colors(0.75);
        assert!(h.x > h.z, "dusk horizon is warm: {h}");
    }

    #[test]
    fn the_sun_crosses_from_right_to_left() {
        assert!(disc_position(0.25).x > 0.9);
        assert!((disc_position(0.5).x - 0.5).abs() < 1e-4);
        assert!(disc_position(0.75).x < 0.1);
        assert!(disc_position(0.5).y > disc_position(0.25).y);
    }
}
