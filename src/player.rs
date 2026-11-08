//! The player, the camera that follows them, and the cursor in world space.

use bevy::input::gestures::PinchGesture;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::art::ShapeAssets;
use crate::combat::weapon::Cooldown;
use crate::core::settings::{Keys, Settings};
use crate::core::{CreatureDef, ItemDef, ObjectDef, Registry, ShapeDef, TerrainDef};
use crate::items::interact::{REACH, Target};
use crate::items::inventory::Inventory;
use crate::stats::{Creature, Progress, Stats};
use crate::world::{CELL, ChunkMap, PLAYER_R, cell_of, center_of, slide, solid_half};

/// Stamina per second while sprinting.
const SPRINT_COST: f32 = 12.0;
const ZOOM_RANGE: (f32, f32) = (0.3, 1.1);

#[derive(Component)]
pub struct Player;

/// The held item drawn in the player's hand.
#[derive(Component)]
pub struct HeldItem(pub Option<u16>);

/// The mouse in world space this frame, if the cursor is over the window.
#[derive(Resource, Default)]
pub struct Cursor {
    pub cell: Option<IVec2>,
    pub world: Option<Vec2>,
}

pub fn spawn(
    mut commands: Commands,
    shapes: Res<Registry<ShapeDef>>,
    creatures: Res<Registry<CreatureDef>>,
    assets: Res<ShapeAssets>,
    settings: Res<Settings>,
) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: settings.zoom.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1),
            ..OrthographicProjection::default_2d()
        }),
    ));
    let id = creatures
        .idx("player")
        .expect("creatures.ron must define 'player'");
    let def = creatures.at(id);
    let e = crate::art::spawn_shape(
        &mut commands,
        &assets,
        shapes.idx(&def.shape),
        Transform::from_xyz(0.0, 0.0, 2.0),
    );
    let hand = commands
        .spawn((
            HeldItem(None),
            Transform::from_xyz(0.0, 0.0, 0.05),
            Visibility::default(),
        ))
        .id();
    commands.entity(e).add_child(hand).insert((
        Player,
        Creature(id),
        Stats::from(def.stats),
        Progress::default(),
        Cooldown::default(),
        Inventory::default(),
    ));
}

/// WASD / arrows to move, Shift to sprint. With no key held and a mining target out of reach,
/// the player walks toward it (trackpad flow: click a far tree and arrive). Each axis resolves
/// separately so the player stops flush against solids and slides along them.
pub fn movement(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    time: Res<Time>,
    map: Res<ChunkMap>,
    terrain: Res<Registry<TerrainDef>>,
    objects: Res<Registry<ObjectDef>>,
    mut target: ResMut<Target>,
    mut player: Single<(&mut Transform, &mut Stats), With<Player>>,
) {
    let (tf, stats) = &mut *player;
    let axis = |neg: &[KeyCode], pos: &[KeyCode]| {
        keys.pressed(&input, pos) as i8 as f32 - keys.pressed(&input, neg) as i8 as f32
    };
    let mut dir =
        Vec2::new(axis(&keys.left, &keys.right), axis(&keys.down, &keys.up)).normalize_or_zero();
    let pos = tf.translation.truncate();
    if dir != Vec2::ZERO {
        target.0 = None;
    } else if let Some(cell) = target.0 {
        let to = center_of(cell) - pos;
        if to.length() > REACH * 0.8 {
            dir = to.normalize_or_zero();
        }
    }
    let sprinting = keys.pressed(&input, &keys.sprint) && dir != Vec2::ZERO && stats.stamina > 0.0;
    let speed = if sprinting {
        stats.stamina = (stats.stamina - SPRINT_COST * time.delta_secs()).max(0.0);
        stats.speed * 2.0
    } else {
        stats.speed
    };
    let delta = dir * speed * time.delta_secs();
    let solid = |c| solid_half(&map, &terrain, &objects, c);
    let mut p = pos;
    p = slide(p, delta.with_y(0.0), PLAYER_R, solid);
    p = slide(p, delta.with_x(0.0), PLAYER_R, solid);
    tf.translation = p.extend(tf.translation.z);
    // Walk bob: a little squash in step with movement.
    let moving = (p - pos).length() > 0.01;
    let wave = (time.elapsed_secs() * 16.0).sin() * 0.05;
    tf.scale = if moving {
        Vec3::new(1.0 + wave, 1.0 - wave, 1.0)
    } else {
        Vec3::ONE
    };
}

/// Faces the cursor so the eyes and the held item point where the player is looking.
pub fn face_cursor(cursor: Res<Cursor>, mut tf: Single<&mut Transform, With<Player>>) {
    if let Some(world) = cursor.world {
        let to = world - tf.translation.truncate();
        if to.length() > 2.0 {
            tf.rotation = Quat::from_rotation_z(to.to_angle());
        }
    }
}

/// Redraws the item in hand when the selection changes.
pub fn show_held(
    inv: Single<&Inventory, (With<Player>, Changed<Inventory>)>,
    items: Res<Registry<ItemDef>>,
    shapes: Res<Registry<ShapeDef>>,
    assets: Res<ShapeAssets>,
    mut hand: Single<(Entity, &mut HeldItem)>,
    mut commands: Commands,
) {
    let (entity, held) = &mut *hand;
    let now = inv.held().map(|s| s.item);
    if held.0 == now {
        return;
    }
    held.0 = now;
    commands.entity(*entity).despawn_children();
    if let Some(item) = now {
        let shape = shapes.idx(&items.at(item).shape);
        let e = crate::art::spawn_shape(
            &mut commands,
            &assets,
            shape,
            Transform::from_xyz(CELL * 0.42, -CELL * 0.1, 0.0)
                .with_scale(Vec3::splat(0.55))
                .with_rotation(Quat::from_rotation_z(-0.6)),
        );
        commands.entity(*entity).add_child(e);
    }
}

pub fn follow(
    player: Single<&Transform, (With<Player>, Without<Camera2d>)>,
    mut cam: Single<&mut Transform, With<Camera2d>>,
) {
    cam.translation = player.translation.with_z(cam.translation.z);
}

/// Pinch on a trackpad or `-` / `=` on the keyboard.
pub fn zoom(
    mut pinches: MessageReader<PinchGesture>,
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    time: Res<Time>,
    mut settings: ResMut<Settings>,
    mut proj: Single<&mut Projection, With<Camera2d>>,
) {
    let Projection::Orthographic(o) = &mut **proj else {
        return;
    };
    let mut factor = 1.0;
    for p in pinches.read() {
        factor *= 1.0 - p.0;
    }
    if keys.pressed(&input, &keys.zoom_out) {
        factor *= 1.0 + 1.5 * time.delta_secs();
    }
    if keys.pressed(&input, &keys.zoom_in) {
        factor /= 1.0 + 1.5 * time.delta_secs();
    }
    if factor != 1.0 {
        o.scale = (o.scale * factor).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        settings.zoom = o.scale;
    }
}

pub fn track_cursor(
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &Transform), With<Camera2d>>,
    mut cursor: ResMut<Cursor>,
) {
    let (camera, tf) = *cam;
    cursor.world = window.cursor_position().and_then(|p| {
        camera
            .viewport_to_world_2d(&GlobalTransform::from(*tf), p)
            .ok()
    });
    cursor.cell = cursor.world.map(cell_of);
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Cursor>()
            .add_systems(Startup, spawn)
            .add_systems(
                Update,
                (movement, follow, zoom, track_cursor, face_cursor, show_held)
                    .chain()
                    .in_set(PlayerSet),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerSet;
