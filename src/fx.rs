//! Visual effects: short-lived shapes (slashes, break particles) and night glows around lights.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::art::ShapeAssets;
use crate::core::time::GameTime;
use crate::core::{ObjectDef, Registry, Rng, ShapeDef};
use crate::daylight::darkness;
use crate::items::interact::{Damage, ObjectBroken, REACH};
use crate::player::{Cursor, Player};
use crate::world::chunk::{CHUNK, TILES};
use crate::world::{CELL, ChunkMap, center_of};

/// Shrinks to nothing over `total` seconds while drifting, then despawns.
#[derive(Component)]
pub struct Transient {
    pub left: f32,
    pub total: f32,
    pub velocity: Vec2,
    pub scale: f32,
}

/// Fired by melee attacks so a slash can be drawn.
#[derive(Message)]
pub struct Swing {
    pub origin: Vec2,
    pub dir: Vec2,
    pub range: f32,
}

#[derive(Component)]
pub struct Glow;

#[derive(Resource, Default)]
pub struct Glows(HashMap<IVec2, Entity>);

#[derive(Resource)]
pub struct GlowMaterial(Handle<ColorMaterial>);

const GLOW_COLOR: Color = Color::srgb(1.0, 0.82, 0.5);
const GLOW_ALPHA: f32 = 0.22;

pub fn setup(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(GlowMaterial(
        materials.add(ColorMaterial::from_color(GLOW_COLOR.with_alpha(0.0))),
    ));
}

pub fn tick(
    time: Res<Time>,
    mut q: Query<(Entity, &mut Transient, &mut Transform)>,
    mut commands: Commands,
) {
    for (e, mut t, mut tf) in &mut q {
        t.left -= time.delta_secs();
        if t.left <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let k = t.left / t.total;
        tf.translation += (t.velocity * time.delta_secs()).extend(0.0);
        tf.scale = Vec3::splat(t.scale * k);
    }
}

pub fn slash(
    mut swings: MessageReader<Swing>,
    shapes: Res<Registry<ShapeDef>>,
    assets: Res<ShapeAssets>,
    mut commands: Commands,
) {
    for s in swings.read() {
        let at = s.origin + s.dir * s.range * 0.55;
        let e = crate::art::spawn_shape(
            &mut commands,
            &assets,
            shapes.idx("slash"),
            Transform::from_translation(at.extend(2.5))
                .with_rotation(Quat::from_rotation_z(s.dir.to_angle())),
        );
        commands.entity(e).insert(Transient {
            left: 0.15,
            total: 0.15,
            velocity: s.dir * 60.0,
            scale: 1.0,
        });
    }
}

/// A burst of small circles in the broken object's color.
pub fn break_particles(
    mut broken: MessageReader<ObjectBroken>,
    objects: Res<Registry<ObjectDef>>,
    shapes: Res<Registry<ShapeDef>>,
    assets: Res<ShapeAssets>,
    mut rng: ResMut<Rng>,
    mut commands: Commands,
) {
    for b in broken.read() {
        let shape = &objects.at(b.object).shape;
        let color_id = shapes
            .get(shape)
            .and_then(|s| s.parts.first())
            .map(|p| match p {
                crate::core::Part::Circle { color, .. }
                | crate::core::Part::Rect { color, .. }
                | crate::core::Part::Poly { color, .. } => color.clone(),
            });
        let Some(material) = color_id.and_then(|c| assets.materials.get(&c).cloned()) else {
            continue;
        };
        let center = center_of(b.cell);
        for _ in 0..6 {
            let dir = Vec2::from_angle(rng.range(0.0, std::f32::consts::TAU));
            let r = rng.range(1.5, 3.5);
            commands.spawn((
                Mesh2d(assets.unit_circle.clone()),
                MeshMaterial2d(material.clone()),
                Transform::from_translation((center + dir * CELL * 0.2).extend(2.4))
                    .with_scale(Vec3::splat(r)),
                Transient {
                    left: 0.45,
                    total: 0.45,
                    velocity: dir * rng.range(40.0, 110.0),
                    scale: r,
                },
            ));
        }
    }
}

/// Keeps one glow circle over every light-giving object in loaded chunks; alpha follows night.
pub fn glows(
    clock: Res<GameTime>,
    map: Res<ChunkMap>,
    objects: Res<Registry<ObjectDef>>,
    assets: Res<ShapeAssets>,
    material: Res<GlowMaterial>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut glows: ResMut<Glows>,
    mut commands: Commands,
) {
    if let Some(mut m) = materials.get_mut(&material.0) {
        m.color = GLOW_COLOR.with_alpha(GLOW_ALPHA * darkness(clock.day_frac()));
    }
    let mut live: HashMap<IVec2, f32> = HashMap::default();
    for (pos, chunk) in &map.chunks {
        for i in 0..TILES {
            if let Some(r) = chunk.tiles[i].object.and_then(|o| objects.at(o).light) {
                let cell = pos * CHUNK + IVec2::new(i as i32 % CHUNK, i as i32 / CHUNK);
                live.insert(cell, r);
            }
        }
    }
    let stale: Vec<IVec2> = glows
        .0
        .keys()
        .filter(|c| !live.contains_key(*c))
        .copied()
        .collect();
    for c in stale {
        if let Some(e) = glows.0.remove(&c) {
            commands.entity(e).despawn();
        }
    }
    for (cell, r) in live {
        if glows.0.contains_key(&cell) {
            continue;
        }
        let e = commands
            .spawn((
                Glow,
                Mesh2d(assets.unit_circle.clone()),
                MeshMaterial2d(material.0.clone()),
                Transform::from_translation(center_of(cell).extend(11.0))
                    .with_scale(Vec3::splat(r * CELL)),
            ))
            .id();
        glows.0.insert(cell, e);
    }
}

/// Outline on the hovered cell (bright when in reach) and a shrinking bar while mining.
pub fn hover(
    cursor: Res<Cursor>,
    damage: Res<Damage>,
    map: Res<ChunkMap>,
    objects: Res<Registry<ObjectDef>>,
    player: Single<&Transform, With<Player>>,
    mut gizmos: Gizmos,
) {
    let Some(cell) = cursor.cell else { return };
    let center = center_of(cell);
    let near = player.translation.truncate().distance(center) <= REACH;
    let color = if near {
        Color::srgba(1.0, 1.0, 1.0, 0.9)
    } else {
        Color::srgba(1.0, 1.0, 1.0, 0.25)
    };
    gizmos.rect_2d(
        Isometry2d::from_translation(center),
        Vec2::splat(CELL - 2.0),
        color,
    );
    if let Some(hp) = damage.0.get(&cell)
        && let Some(def) = map.tile(cell).and_then(|t| t.object).map(|o| objects.at(o))
    {
        let frac = (hp / def.hp).clamp(0.0, 1.0);
        let y = center.y - CELL * 0.65;
        let half = CELL * 0.5;
        gizmos.line_2d(
            Vec2::new(center.x - half, y),
            Vec2::new(center.x - half + CELL * frac, y),
            Color::srgb(1.0, 0.85, 0.3),
        );
    }
}

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Glows>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (slash, break_particles, glows, tick, hover).in_set(FxSet),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FxSet;
