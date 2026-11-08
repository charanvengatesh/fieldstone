//! Natural creature spawning from `creatures.ron` spawn tables, and despawning far away.

use bevy::prelude::*;

use super::ai::Ai;
use crate::art::ShapeAssets;
use crate::core::time::GameTime;
use crate::core::{CreatureDef, ObjectDef, Registry, Rng, ShapeDef, TerrainDef, TimeOfDay};
use crate::player::Player;
use crate::stats::{Creature, Stats};
use crate::world::{CELL, ChunkMap, PLAYER_R, cell_of, center_of, overlapping_cells};

const MAX_HOSTILE: usize = 18;
const MAX_PASSIVE: usize = 12;
const SPAWN_INTERVAL: f32 = 1.5;
/// Cells past the visible half-extent where creatures may appear, and where they vanish.
const MARGIN_IN: f32 = 3.0;
const MARGIN_OUT: f32 = 20.0;
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
    cam: Single<&Projection, With<Camera2d>>,
    mobs: Query<&Creature, Without<Player>>,
    mut commands: Commands,
) {
    timer.0 -= time.delta_secs();
    if timer.0 > 0.0 {
        return;
    }
    timer.0 = SPAWN_INTERVAL;
    let (mut hostile, mut passive) = (0, 0);
    for c in &mobs {
        if matches!(creatures.at(c.0).ai, crate::core::Behavior::Hostile { .. }) {
            hostile += 1;
        } else {
            passive += 1;
        }
    }
    let night = is_night(&clock);
    if rng.f32() > if night { DAY_CHANCE * 2.0 } else { DAY_CHANCE } {
        return;
    }
    let origin = player.translation.truncate();
    let half = view_half_extent(&cam);
    let ring = half.max_element() / CELL;
    let at = origin
        + Vec2::from_angle(rng.range(0.0, std::f32::consts::TAU))
            * CELL
            * rng.range(ring + MARGIN_IN, ring + MARGIN_IN + 6.0);
    let cell = cell_of(at);
    // Only off-screen candidates; the ring is round, the view is not.
    let rel = (center_of(cell) - origin).abs();
    if rel.x < half.x + CELL && rel.y < half.y + CELL {
        return;
    }
    let Some(tile) = map.tile(cell) else { return };
    if tile.object.is_some_and(|o| !objects.at(o).decor())
        || !terrain.at(tile.terrain).walkable
        || overlapping_cells(center_of(cell), PLAYER_R).any(|c| {
            map.tile(c)
                .is_none_or(|t| t.object.is_some_and(|o| objects.at(o).solid))
        })
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
            let is_hostile = matches!(c.ai, crate::core::Behavior::Hostile { .. });
            if (is_hostile && hostile >= MAX_HOSTILE) || (!is_hostile && passive >= MAX_PASSIVE) {
                return None;
            }
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
    map.lights.iter().any(|c| {
        map.tile(*c)
            .and_then(|t| t.object)
            .and_then(|o| objects.at(o).light)
            .is_some_and(|r| (*c - cell).as_vec2().length() <= r)
    })
}

pub fn despawn_far(
    player: Single<&Transform, With<Player>>,
    cam: Single<&Projection, With<Camera2d>>,
    mobs: Query<(Entity, &Transform), (With<Creature>, Without<Player>)>,
    mut commands: Commands,
) {
    let origin = player.translation.truncate();
    let limit = view_half_extent(&cam).max_element() + (MARGIN_IN + MARGIN_OUT) * CELL;
    for (e, tf) in &mobs {
        if tf.translation.truncate().distance(origin) > limit {
            commands.entity(e).despawn();
        }
    }
}

/// Half the visible world size, from the orthographic projection.
pub fn view_half_extent(proj: &Projection) -> Vec2 {
    match proj {
        Projection::Orthographic(o) => o.area.half_size(),
        _ => Vec2::splat(20.0 * CELL),
    }
}
