//! Day/night tint: a camera-sized quad whose alpha follows the clock.

use bevy::prelude::*;

use crate::core::time::GameTime;

const NIGHT: Color = Color::srgb(0.05, 0.07, 0.2);
const MAX_ALPHA: f32 = 0.62;

#[derive(Component)]
pub struct Overlay;

#[derive(Resource)]
pub struct OverlayMaterial(pub Handle<ColorMaterial>);

pub fn spawn(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    cam: Single<Entity, With<Camera2d>>,
) {
    let material = materials.add(ColorMaterial::from_color(NIGHT.with_alpha(0.0)));
    let quad = meshes.add(Rectangle::new(20_000.0, 20_000.0));
    let overlay = commands
        .spawn((
            Overlay,
            Mesh2d(quad),
            MeshMaterial2d(material.clone()),
            Transform::from_xyz(0.0, 0.0, 30.0),
        ))
        .id();
    commands.entity(*cam).add_child(overlay);
    commands.insert_resource(OverlayMaterial(material));
}

/// Darkness ramps up through dusk (0.4..0.6), holds at night, and fades at dawn (0.9..1.0).
pub fn darkness(frac: f32) -> f32 {
    let dusk = ((frac - 0.4) / 0.2).clamp(0.0, 1.0);
    let dawn = ((frac - 0.9) / 0.1).clamp(0.0, 1.0);
    let d = dusk * (1.0 - dawn);
    d * d * (3.0 - 2.0 * d)
}

pub fn update(
    clock: Res<GameTime>,
    overlay: Res<OverlayMaterial>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if let Some(mut m) = materials.get_mut(&overlay.0) {
        m.color = NIGHT.with_alpha(darkness(clock.day_frac()) * MAX_ALPHA);
    }
}

pub struct DaylightPlugin;

impl Plugin for DaylightPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn)
            .add_systems(Update, update);
    }
}

#[cfg(test)]
mod tests {
    use super::darkness;

    #[test]
    fn day_is_bright_and_night_is_dark() {
        assert_eq!(darkness(0.1), 0.0);
        assert_eq!(darkness(0.75), 1.0);
        assert!(darkness(0.5) > 0.3 && darkness(0.5) < 0.7);
        assert_eq!(darkness(0.999), darkness(0.999));
        assert!(darkness(0.95) < 0.6);
    }
}
