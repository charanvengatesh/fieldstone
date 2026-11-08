//! Farming: hoe grass into tilled soil, plant crop objects, let them grow stage by stage.
//! Crops are ordinary objects chained with `grows_into`; harvesting is ordinary mining.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::art::ShapeAssets;
use crate::core::time::GameTime;
use crate::core::{ItemDef, ObjectDef, Registry, ShapeDef, TerrainDef, Use};
use crate::items::interact::in_reach;
use crate::items::inventory::Inventory;
use crate::player::{Cursor, Player};
use crate::world::stream::{clear_object, put_object};
use crate::world::{ChunkMap, chunk::chunk_of};

/// Game-time at which each growing crop advances a stage.
#[derive(Resource, Default)]
pub struct Growth(pub HashMap<IVec2, f64>);

/// Left click with a hoe on bare, tillable ground turns it over.
pub fn till(
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Res<Cursor>,
    items: Res<Registry<ItemDef>>,
    terrain: Res<Registry<TerrainDef>>,
    objects: Res<Registry<ObjectDef>>,
    mut map: ResMut<ChunkMap>,
    ui: Res<crate::core::UiHovered>,
    player: Single<(&Transform, &Inventory), With<Player>>,
    mut commands: Commands,
) {
    if !mouse.just_pressed(MouseButton::Left) || ui.0 {
        return;
    }
    let (tf, inv) = *player;
    let Some(cell) = cursor
        .cell
        .filter(|c| in_reach(tf.translation.truncate(), *c))
    else {
        return;
    };
    let is_hoe = matches!(inv.held().map(|s| &items.at(s.item).usage), Some(Use::Tool { tag, .. }) if tag == "hoe");
    if !is_hoe {
        return;
    }
    let Some(tile) = map.tile(cell) else { return };
    if tile.object.is_some_and(|o| !objects.at(o).decor()) {
        return;
    }
    if let Some(next) = terrain
        .at(tile.terrain)
        .till
        .as_deref()
        .and_then(|id| terrain.idx(id))
    {
        // Tufts and flowers are turned under.
        clear_object(&mut map, cell, &mut commands);
        map.set(
            cell,
            crate::world::chunk::Tile {
                terrain: next,
                object: None,
            },
        );
    }
}

/// Starts timers for crops that have none (freshly planted or just streamed in) and advances
/// stages whose time has come. Runs a few times a second; the work is tiny.
pub fn grow(
    clock: Res<GameTime>,
    objects: Res<Registry<ObjectDef>>,
    terrain: Res<Registry<TerrainDef>>,
    shapes: Res<Registry<ShapeDef>>,
    assets: Res<ShapeAssets>,
    mut growth: ResMut<Growth>,
    mut map: ResMut<ChunkMap>,
    mut commands: Commands,
    mut acc: Local<f32>,
    time: Res<Time>,
) {
    *acc += time.delta_secs();
    if *acc < 0.25 {
        return;
    }
    *acc = 0.0;
    let now = clock.secs;
    // Discover crops without a timer.
    let mut fresh = Vec::new();
    for (pos, chunk) in &map.chunks {
        for (i, t) in chunk.tiles.iter().enumerate() {
            let Some((_, secs)) = t.object.and_then(|o| objects.at(o).grows_into.as_ref()) else {
                continue;
            };
            let cell = pos * crate::world::chunk::CHUNK
                + IVec2::new(
                    i as i32 % crate::world::chunk::CHUNK,
                    i as i32 / crate::world::chunk::CHUNK,
                );
            if !growth.0.contains_key(&cell) {
                let watered = (-1..=1).any(|dy| {
                    (-1..=1).any(|dx| {
                        map.tile(cell + IVec2::new(dx, dy))
                            .is_some_and(|t| terrain.at(t.terrain).fish_table.is_some())
                    })
                });
                fresh.push((cell, now + *secs as f64 / if watered { 2.0 } else { 1.0 }));
            }
        }
    }
    growth.0.extend(fresh);
    // Advance due crops; drop timers for cells whose crop is gone or unloaded.
    let due: Vec<IVec2> = growth
        .0
        .iter()
        .filter(|(c, t)| **t <= now || !map.chunks.contains_key(&chunk_of(**c)))
        .map(|(c, _)| *c)
        .collect();
    for cell in due {
        growth.0.remove(&cell);
        let Some(obj) = map.tile(cell).and_then(|t| t.object) else {
            continue;
        };
        if let Some(next) = objects
            .at(obj)
            .grows_into
            .as_ref()
            .and_then(|(id, _)| objects.idx(id))
        {
            put_object(
                &mut map,
                cell,
                next,
                &objects,
                &shapes,
                &assets,
                &mut commands,
            );
        }
    }
}

pub struct FarmPlugin;

impl Plugin for FarmPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Growth>()
            .add_systems(Update, (till, grow).in_set(FarmSet));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FarmSet;
