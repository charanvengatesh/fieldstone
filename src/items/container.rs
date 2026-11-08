//! Chests: per-cell slot lists kept outside the tile grid, opened by using the object.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::drops::SpawnDrop;
use super::interact::{ObjectBroken, REACH, in_reach};
use super::inventory::Stack;
use crate::core::{ObjectDef, Registry};
use crate::player::Player;
use crate::world::center_of;

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
