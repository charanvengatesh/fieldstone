//! Natural creature spawning from `creatures.ron` spawn tables, and despawning far away.

use bevy::prelude::*;

use super::ai::Ai;
use crate::art::ShapeAssets;
use crate::core::time::GameTime;
use crate::core::{CreatureDef, ObjectDef, Registry, Rng, ShapeDef, TerrainDef, TimeOfDay};
use crate::player::Player;
use crate::stats::{Creature, Stats};
use crate::world::{CELL, ChunkMap, PLAYER_R, cell_of, center_of, overlapping_cells};

const MAX_MOBS: usize = 24;
const SPAWN_INTERVAL: f32 = 1.5;
const MIN_DIST: f32 = 9.0;
const MAX_DIST: f32 = 18.0;
const DESPAWN_DIST: f32 = 40.0;
/// Spawn attempts succeed at this rate by day; night is twice as busy.
const DAY_CHANCE: f32 = 0.25;

#[derive(Resource, Default)]
pub struct SpawnTimer(pub f32);

pub fn is_night(t: &GameTime) -> bool {
    t.day_frac() >= 0.5
}

pub fn spawn_creature(
    commands: &mut Commands,
    assets: &ShapeAssets,
    shapes: &Registry<ShapeDef>,
    creatures: &Registry<CreatureDef>,
    id: u16,
    at: Vec2,
) -> Entity {
    let def = creatures.at(id);
    let e = crate::art::spawn_shape(
        commands,
        assets,
        shapes.idx(&def.shape),
        Transform::from_translation(at.extend(1.9)),
    );
    commands
        .entity(e)
        .insert((Creature(id), Stats::from(def.stats), Ai::default()))
        .id()
}

pub fn natural_spawns(
    time: Res<Time>,
    clock: Res<GameTime>,
    mut timer: ResMut<SpawnTimer>,
    mut rng: ResMut<Rng>,
    creatures: Res<Registry<CreatureDef>>,
    terrain: Res<Registry<TerrainDef>>,
    objects: Res<Registry<ObjectDef>>,
    shapes: Res<Registry<ShapeDef>>,
    assets: Res<ShapeAssets>,
    map: Res<ChunkMap>,
    player: Single<&Transform, With<Player>>,
    mobs: Query<&Transform, (With<Creature>, Without<Player>)>,
    mut commands: Commands,
) {
    timer.0 -= time.delta_secs();
    if timer.0 > 0.0 || mobs.iter().count() >= MAX_MOBS {
        return;
    }
    timer.0 = SPAWN_INTERVAL;
    let night = is_night(&clock);
    if rng.f32() > if night { DAY_CHANCE * 2.0 } else { DAY_CHANCE } {
        return;
    }
    let origin = player.translation.truncate();
    let at = origin
        + Vec2::from_angle(rng.range(0.0, std::f32::consts::TAU))
            * CELL
            * rng.range(MIN_DIST, MAX_DIST);
    let cell = cell_of(at);
    let Some(tile) = map.tile(cell) else { return };
    if tile.object.is_some()
        || !terrain.at(tile.terrain).walkable
        || overlapping_cells(at, PLAYER_R).any(|c| map.tile(c).is_none_or(|t| t.object.is_some()))
    {
        return;
    }
    if lit(&map, &objects, cell) {
        return;
    }
    let ground = terrain.id_of(tile.terrain);
    let candidates: Vec<(u16, f32)> = creatures
        .iter()
        .filter_map(|(i, c)| {
            let s = c.spawn.as_ref()?;
            let time_ok = match s.time {
                TimeOfDay::Any => true,
                TimeOfDay::Day => !night,
                TimeOfDay::Night => night,
            };
            (time_ok && s.on.iter().any(|t| t == ground)).then_some((i, s.weight))
        })
        .collect();
    let total: f32 = candidates.iter().map(|c| c.1).sum();
    if total <= 0.0 {
        return;
    }
    let mut pick = rng.range(0.0, total);
    for (id, w) in candidates {
        pick -= w;
        if pick <= 0.0 {
            spawn_creature(
                &mut commands,
                &assets,
                &shapes,
                &creatures,
                id,
                center_of(cell),
            );
            return;
        }
    }
}

/// Whether any light-giving object covers `cell`.
pub fn lit(map: &ChunkMap, objects: &Registry<ObjectDef>, cell: IVec2) -> bool {
    let max_r = objects
        .iter()
        .filter_map(|(_, o)| o.light)
        .fold(0.0f32, f32::max)
        .ceil() as i32;
    (-max_r..=max_r).any(|dy| {
        (-max_r..=max_r).any(|dx| {
            let c = cell + IVec2::new(dx, dy);
            map.tile(c)
                .and_then(|t| t.object)
                .and_then(|o| objects.at(o).light)
                .is_some_and(|r| (c - cell).as_vec2().length() <= r)
        })
    })
}

pub fn despawn_far(
    player: Single<&Transform, With<Player>>,
    mobs: Query<(Entity, &Transform), (With<Creature>, Without<Player>)>,
    mut commands: Commands,
) {
    let origin = player.translation.truncate();
    for (e, tf) in &mobs {
        if tf.translation.truncate().distance(origin) > DESPAWN_DIST * CELL {
            commands.entity(e).despawn();
        }
    }
}
