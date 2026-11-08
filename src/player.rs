//! The player, the camera that follows them, and the cell under the cursor.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::art::ShapeAssets;
use crate::combat::weapon::Cooldown;
use crate::core::{CreatureDef, ObjectDef, Registry, ShapeDef, TerrainDef};
use crate::items::inventory::Inventory;
use crate::stats::{Creature, Progress, Stats};

use crate::world::{ChunkMap, PLAYER_R, cell_of, is_solid, slide};

/// Stamina per second while sprinting.
const SPRINT_COST: f32 = 12.0;
pub const ZOOM: f32 = 0.6;

#[derive(Component)]
pub struct Player;

/// The cell under the mouse this frame, if the cursor is over the window.
#[derive(Resource, Default)]
pub struct Cursor(pub Option<IVec2>);

pub fn spawn(
    mut commands: Commands,
    shapes: Res<Registry<ShapeDef>>,
    creatures: Res<Registry<CreatureDef>>,
    assets: Res<ShapeAssets>,
) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: ZOOM,
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
    commands.entity(e).insert((
        Player,
        Creature(id),
        Stats::from(def.stats),
        Progress::default(),
        Cooldown::default(),
        Inventory::default(),
    ));
}

/// WASD / arrows to move, Shift to sprint. Each axis resolves separately so the player stops
/// flush against solids and slides along them.
pub fn movement(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    map: Res<ChunkMap>,
    terrain: Res<Registry<TerrainDef>>,
    objects: Res<Registry<ObjectDef>>,
    mut player: Single<(&mut Transform, &mut Stats), With<Player>>,
) {
    use KeyCode::*;
    let (tf, stats) = &mut *player;
    let axis = |neg: [KeyCode; 2], pos: [KeyCode; 2]| {
        keys.any_pressed(pos) as i8 as f32 - keys.any_pressed(neg) as i8 as f32
    };
    let dir = Vec2::new(
        axis([KeyA, ArrowLeft], [KeyD, ArrowRight]),
        axis([KeyS, ArrowDown], [KeyW, ArrowUp]),
    )
    .normalize_or_zero();
    let sprinting =
        keys.any_pressed([ShiftLeft, ShiftRight]) && dir != Vec2::ZERO && stats.stamina > 0.0;
    let speed = if sprinting {
        stats.stamina = (stats.stamina - SPRINT_COST * time.delta_secs()).max(0.0);
        stats.speed * 2.0
    } else {
        stats.speed
    };
    let delta = dir * speed * time.delta_secs();
    let solid = |c| is_solid(&map, &terrain, &objects, c);
    let mut p = tf.translation.truncate();
    p = slide(p, delta.with_y(0.0), PLAYER_R, solid);
    p = slide(p, delta.with_x(0.0), PLAYER_R, solid);
    tf.translation = p.extend(tf.translation.z);
}

pub fn follow(
    player: Single<&Transform, (With<Player>, Without<Camera2d>)>,
    mut cam: Single<&mut Transform, With<Camera2d>>,
) {
    cam.translation = player.translation.with_z(cam.translation.z);
}

pub fn track_cursor(
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &Transform), With<Camera2d>>,
    mut cursor: ResMut<Cursor>,
) {
    let (camera, tf) = *cam;
    cursor.0 = window
        .cursor_position()
        .and_then(|p| {
            camera
                .viewport_to_world_2d(&GlobalTransform::from(*tf), p)
                .ok()
        })
        .map(cell_of);
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Cursor>()
            .add_systems(Startup, spawn)
            .add_systems(
                Update,
                (movement, follow, track_cursor).chain().in_set(PlayerSet),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerSet;
