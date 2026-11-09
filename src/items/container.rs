//! Chests: per-cell slot lists kept outside the tile grid, opened by using the object.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::drops::SpawnDrop;
use super::drops::roll_stacks;
use super::interact::{ObjectBroken, REACH, in_reach};
use super::inventory::Stack;
use crate::core::{ItemDef, LootDef, ObjectDef, Registry};
use crate::player::Player;
use crate::world::chunk::{CHUNK, local_index};
use crate::world::worldgen::WorldGen;
use crate::world::{ChunkMap, center_of};

#[derive(Resource, Default)]
pub struct Containers(pub HashMap<IVec2, Vec<Option<Stack>>>);

/// The chest the player currently has open, if any.
#[derive(Resource, Default)]
pub struct OpenContainer(pub Option<IVec2>);

impl Containers {
    pub fn slots_mut(&mut self, cell: IVec2, size: u32) -> &mut Vec<Option<Stack>> {
        let v = self.0.entry(cell).or_default();
        if v.len() < size as usize {
            v.resize(size as usize, None);
        }
        v
    }
}

/// Fills storage objects stamped by structures the first time their chunk is generated.
/// Cells already in `Containers` (from a save, or an earlier visit) keep what they have.
pub fn stock_structures(
    mut map: ResMut<ChunkMap>,
    wg: Res<WorldGen>,
    objects: Res<Registry<ObjectDef>>,
    items: Res<Registry<ItemDef>>,
    loot: Res<Registry<LootDef>>,
    mut containers: ResMut<Containers>,
) {
    let fresh = std::mem::take(&mut map.fresh);
    for pos in fresh {
        let Some(chunk) = map.chunks.get(&pos) else {
            continue;
        };
        let origin = pos * CHUNK;
        for y in 0..CHUNK {
            for x in 0..CHUNK {
                let cell = origin + IVec2::new(x, y);
                let Some(obj) = chunk.tiles[local_index(cell)].object else {
                    continue;
                };
                let storage = objects.at(obj).storage;
                if storage == 0 || containers.0.contains_key(&cell) {
                    continue;
                }
                let Some(table) = wg.stamp_loot(cell) else {
                    continue;
                };
                let rolled = roll_stacks(loot.at(table), &items, cell, wg.seed, 4242);
                let slots = containers.slots_mut(cell, storage);
                for (slot, stack) in slots.iter_mut().zip(rolled) {
                    *slot = Some(stack);
                }
            }
        }
    }
}

/// Closes the chest when it breaks or the player wanders off; spills a broken chest's contents.
pub fn upkeep(
    mut broken: MessageReader<ObjectBroken>,
    objects: Res<Registry<ObjectDef>>,
    player: Single<&Transform, With<Player>>,
    mut containers: ResMut<Containers>,
    mut open: ResMut<OpenContainer>,
    mut drops: MessageWriter<SpawnDrop>,
) {
    for b in broken.read() {
        if objects.at(b.object).storage > 0
            && let Some(slots) = containers.0.remove(&b.cell)
        {
            for s in slots.into_iter().flatten() {
                drops.write(SpawnDrop {
                    item: s.item,
                    n: s.n,
                    at: center_of(b.cell),
                });
            }
        }
        if open.0 == Some(b.cell) {
            open.0 = None;
        }
    }
    if let Some(cell) = open.0
        && !in_reach(player.translation.truncate(), cell)
    {
        open.0 = None;
    }
    let _ = REACH;
}

/// Hides the inventory panel again when the chest that forced it open goes away.
pub fn follow_close(
    open: Res<OpenContainer>,
    mut was_open: Local<bool>,
    mut panels: Query<&mut Visibility, With<crate::ui::inventory::InventoryPanel>>,
) {
    let now = open.0.is_some();
    if *was_open && !now {
        for mut v in &mut panels {
            *v = Visibility::Hidden;
        }
    }
    *was_open = now;
}
