//! The stone the player steers, and the camera that follows it.

use bevy::prelude::*;

use crate::world::{Blocks, PLAYER_R, WORLD_HALF, slide};

const WALK: f32 = 120.0;
const SPRINT: f32 = 240.0;
const PLAYER_COLOR: Color = Color::srgb(200.0 / 255.0, 200.0 / 255.0, 200.0 / 255.0);

#[derive(Component)]
pub struct Player;

pub fn spawn(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Player,
        Sprite::from_color(PLAYER_COLOR, Vec2::splat(2.0 * PLAYER_R)),
        Transform::from_xyz(0.0, 0.0, 2.0),
    ));
}

/// WASD / arrows to move, Shift to sprint. Resolves each axis separately so the player
/// stops flush against stones and slides along them, then clamps to the field.
pub fn movement(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    blocks: Res<Blocks>,
    mut tf: Single<&mut Transform, With<Player>>,
) {
    use KeyCode::*;
    let axis = |neg: [KeyCode; 2], pos: [KeyCode; 2]| {
        keys.any_pressed(pos) as i8 as f32 - keys.any_pressed(neg) as i8 as f32
    };
    let dir = Vec2::new(
        axis([KeyA, ArrowLeft], [KeyD, ArrowRight]),
        axis([KeyS, ArrowDown], [KeyW, ArrowUp]),
    )
    .normalize_or_zero();
    let speed = if keys.any_pressed([ShiftLeft, ShiftRight]) {
        SPRINT
    } else {
        WALK
    };
    let delta = dir * speed * time.delta_secs();

    let mut p = tf.translation.truncate();
    p = slide(p, delta.with_y(0.0), &blocks);
    p = slide(p, delta.with_x(0.0), &blocks);
    p = p.clamp(-WORLD_HALF + PLAYER_R, WORLD_HALF - PLAYER_R);
    tf.translation = p.extend(tf.translation.z);
}

pub fn follow(
    player: Single<&Transform, (With<Player>, Without<Camera2d>)>,
    mut cam: Single<&mut Transform, With<Camera2d>>,
) {
    cam.translation = player.translation.with_z(cam.translation.z);
}
