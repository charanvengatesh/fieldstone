//! The player, the camera that follows them, and the cursor in world space.

use bevy::input::gestures::PinchGesture;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::art::ShapeAssets;
use crate::combat::weapon::Cooldown;
use crate::core::settings::{Keys, RenderMode, Settings};
use crate::core::{CreatureDef, ItemDef, ObjectDef, Registry, ShapeDef, TerrainDef};
use crate::items::interact::{REACH, Target};
use crate::items::inventory::Inventory;
use crate::stats::{Creature, Progress, Stats};
use crate::world::{CELL, ChunkMap, PLAYER_R, blocker, cell_of, center_of, slide};

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
            scale: match settings.render {
                RenderMode::Pixel => 1.0 / settings.pixel_scale.clamp(1, 6) as f32,
                RenderMode::Vector => settings.zoom.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1),
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
    let id = creatures
        .idx("player")
        .expect("creatures.ron must define 'player'");
    let def = creatures.at(id);
    let e = crate::art::spawn_standing(
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
    mut toast: ResMut<crate::core::Toast>,
    mut stalled: Local<f32>,
    mut player: Single<(&mut Transform, &mut Stats), With<Player>>,
) {
    let (tf, stats) = &mut *player;
    let axis = |neg: &[KeyCode], pos: &[KeyCode]| {
        keys.pressed(&input, pos) as i8 as f32 - keys.pressed(&input, neg) as i8 as f32
    };
    let mut dir =
        Vec2::new(axis(&keys.left, &keys.right), axis(&keys.down, &keys.up)).normalize_or_zero();
    let pos = tf.translation.truncate();
    let mut walking_to_target = false;
    if dir != Vec2::ZERO {
        target.0 = None;
    } else if let Some(cell) = target.0 {
        let to = center_of(cell) - pos;
        if to.length() > REACH * 0.8 {
            dir = to.normalize_or_zero();
            walking_to_target = true;
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
    let solid = blocker(&map, &terrain, &objects, cell_of(pos));
    let mut p = pos;
    p = slide(p, delta.with_y(0.0), PLAYER_R, &solid);
    p = slide(p, delta.with_x(0.0), PLAYER_R, &solid);
    tf.translation = p.extend(tf.translation.z);
    // Auto-walk that makes no headway (water, wall, trunk) gives up instead of grinding.
    if walking_to_target && (p - pos).length() < 0.5 * time.delta_secs() * 60.0 {
        *stalled += time.delta_secs();
        if *stalled > 0.5 {
            target.0 = None;
            *stalled = 0.0;
            toast.show("Can't reach that from here.", 2.0);
        }
    } else {
        *stalled = 0.0;
    }
    // Walk bob: a little squash in step with movement.
    let moving = (p - pos).length() > 0.01;
    let wave = (time.elapsed_secs() * 16.0).sin() * 0.05;
    tf.scale = if moving {
        Vec3::new(1.0 + wave, 1.0 - wave, 1.0)
    } else {
        Vec3::ONE
    };
}

/// Faces the cursor. Vector mode rotates the whole body; pixel mode picks one of four frames
/// (right, up, left, down) and mirrors the hand to the facing side.
pub fn face_cursor(
    cursor: Res<Cursor>,
    assets: Res<ShapeAssets>,
    mut player: Single<(&mut Transform, Option<&mut Sprite>), With<Player>>,
    mut hand: Single<&mut Transform, (With<HeldItem>, Without<Player>)>,
) {
    let (tf, sprite) = &mut *player;
    let Some(world) = cursor.world else { return };
    let to = world - tf.translation.truncate();
    if to.length() <= 2.0 {
        return;
    }
    if !assets.pixel() {
        tf.rotation = Quat::from_rotation_z(to.to_angle());
        return;
    }
    tf.rotation = Quat::IDENTITY;
    // Quadrant: 0 right, 1 up, 2 left, 3 down, matching the generated rotation strip.
    let a = to.to_angle();
    let quadrant = (((a + std::f32::consts::FRAC_PI_4).rem_euclid(std::f32::consts::TAU))
        / std::f32::consts::FRAC_PI_2) as usize
        % 4;
    if let Some(sprite) = sprite
        && let Some(atlas) = &mut sprite.texture_atlas
    {
        atlas.index = quadrant;
    }
    let side = match quadrant {
        2 => -1.0,
        _ => 1.0,
    };
    hand.translation.x = CELL * 0.42 * side;
    hand.scale.x = side;
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

/// The camera follows the player; in pixel mode it snaps to the texel grid so sprites never
/// straddle screen pixels and shimmer.
pub fn follow(
    settings: Res<Settings>,
    player: Single<&Transform, (With<Player>, Without<Camera2d>)>,
    mut cam: Single<(&mut Transform, &Projection), With<Camera2d>>,
) {
    let (tf, proj) = &mut *cam;
    let mut p = player.translation.truncate();
    if settings.render == RenderMode::Pixel
        && let Projection::Orthographic(o) = proj
    {
        let step = o.scale.max(1e-4);
        p = (p / step).round() * step;
    }
    tf.translation = p.extend(tf.translation.z);
}

/// Pinch on a trackpad or `-` / `=` on the keyboard.
pub fn zoom(
    mut pinches: MessageReader<PinchGesture>,
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    time: Res<Time>,
    mut settings: ResMut<Settings>,
    mut pinch_acc: Local<f32>,
    mut proj: Single<&mut Projection, With<Camera2d>>,
) {
    let Projection::Orthographic(o) = &mut **proj else {
        return;
    };
    if settings.render == RenderMode::Pixel {
        // Whole pixel scales only; a pinch accumulates until it is worth a step.
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
            o.scale = 1.0 / settings.pixel_scale as f32;
        }
        return;
    }
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
