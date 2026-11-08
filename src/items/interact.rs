//! Using the held item on the world: mining objects (left click held) and placing (right click).

use bevy::input::mouse::MouseWheel;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::drops::{SpawnDrop, roll_loot};
use super::inventory::{HOTBAR, Inventory};
use crate::art::ShapeAssets;
use crate::core::{ItemDef, LootDef, ObjectDef, Registry, ShapeDef, TerrainDef, Use};
use crate::player::{Cursor, Player};
use crate::stats::{GainXp, Progress, Stats};
use crate::world::stream::{clear_object, put_object};
use crate::world::worldgen::WorldGen;
use crate::world::{CELL, ChunkMap, PLAYER_R, center_of, overlapping_cells};

pub const REACH: f32 = CELL * 4.0;
/// Damage per second from bare hands or a tool whose power exactly meets the hardness.
const BASE_DPS: f32 = 1.0;

/// Remaining hit points of objects currently being mined.
#[derive(Resource, Default)]
pub struct Damage(pub HashMap<IVec2, f32>);

/// Fired when an object breaks; farming, scripts and stats listen.
#[derive(Message)]
#[allow(dead_code)] // listeners arrive with farming, stats and scripting
pub struct ObjectBroken {
    pub cell: IVec2,
    pub object: u16,
}

#[derive(Message)]
#[allow(dead_code)]
pub struct ObjectPlaced {
    pub cell: IVec2,
    pub object: u16,
}

pub fn select_slot(
    keys: Res<ButtonInput<KeyCode>>,
    mut wheel: MessageReader<MouseWheel>,
    mut inv: Single<&mut Inventory, With<Player>>,
) {
    use KeyCode::*;
    const DIGITS: [KeyCode; HOTBAR] = [
        Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0,
    ];
    if let Some(i) = DIGITS.iter().position(|k| keys.just_pressed(*k)) {
        inv.selected = i;
    }
    for w in wheel.read() {
        let step = if w.y < 0.0 { 1 } else { HOTBAR - 1 };
        inv.selected = (inv.selected + step) % HOTBAR;
    }
}

fn in_reach(player: Vec2, cell: IVec2) -> bool {
    player.distance(center_of(cell)) <= REACH
}

pub fn mine(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    cursor: Res<Cursor>,
    wg: Res<WorldGen>,
    items: Res<Registry<ItemDef>>,
    objects: Res<Registry<ObjectDef>>,
    loot: Res<Registry<LootDef>>,
    mut map: ResMut<ChunkMap>,
    mut damage: ResMut<Damage>,
    player: Single<(&Transform, &Inventory, &Progress), With<Player>>,
    mut drops: MessageWriter<SpawnDrop>,
    mut broken: MessageWriter<ObjectBroken>,
    mut xp: MessageWriter<GainXp>,
    mut commands: Commands,
) {
    let (tf, inv, progress) = *player;
    let pos = tf.translation.truncate();
    let target = cursor
        .0
        .filter(|c| mouse.pressed(MouseButton::Left) && in_reach(pos, *c));
    let Some(cell) = target else {
        damage.0.clear();
        return;
    };
    let Some(obj) = map.tile(cell).and_then(|t| t.object) else {
        return;
    };
    let def = objects.at(obj);
    let (tag, power) = match inv.held().map(|s| &items.at(s.item).usage) {
        Some(Use::Tool { tag, power }) => (Some(tag.as_str()), *power),
        _ => (None, 0),
    };
    let right_tool = def.tool.as_deref().is_none_or(|t| Some(t) == tag);
    if !right_tool || power < def.hardness {
        return;
    }
    let dps = BASE_DPS
        * (1 + power - def.hardness) as f32
        * (1.0 + 0.15 * progress.skill_level("mining"));
    let hp = damage.0.entry(cell).or_insert(def.hp);
    *hp -= dps * time.delta_secs();
    if *hp > 0.0 {
        return;
    }
    damage.0.remove(&cell);
    clear_object(&mut map, cell, &mut commands);
    if let Some(table) = def.drops.as_deref().and_then(|id| loot.get(id)) {
        roll_loot(table, &items, center_of(cell), wg.seed, 1, &mut drops);
    }
    broken.write(ObjectBroken { cell, object: obj });
    xp.write(GainXp {
        amount: def.xp,
        skill: "mining",
    });
}

pub fn place(
    mouse: Res<ButtonInput<MouseButton>>,
    cursor: Res<Cursor>,
    items: Res<Registry<ItemDef>>,
    objects: Res<Registry<ObjectDef>>,
    terrain: Res<Registry<TerrainDef>>,
    shapes: Res<Registry<ShapeDef>>,
    assets: Res<ShapeAssets>,
    mut map: ResMut<ChunkMap>,
    mut player: Single<(&Transform, &mut Inventory, &mut Stats), With<Player>>,
    mut placed: MessageWriter<ObjectPlaced>,
    mut commands: Commands,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    let (tf, inv, stats) = &mut *player;
    let pos = tf.translation.truncate();
    let Some(held) = inv.held() else { return };
    if let Use::Eat { hp, stamina } = items.at(held.item).usage {
        stats.hp = (stats.hp + hp).min(stats.max_hp);
        stats.stamina = (stats.stamina + stamina).min(stats.max_stamina);
        inv.consume_held();
        return;
    }
    let Some(cell) = cursor.0.filter(|c| in_reach(pos, *c)) else {
        return;
    };
    let Use::Place(id) = &items.at(held.item).usage else {
        return;
    };
    let Some(obj) = objects.idx(id) else {
        warn!(
            "item '{}' places unknown object '{id}'",
            items.id_of(held.item)
        );
        return;
    };
    let Some(tile) = map.tile(cell) else { return };
    let blocked = tile.object.is_some()
        || !terrain.at(tile.terrain).walkable
        || (objects.at(obj).solid && overlapping_cells(pos, PLAYER_R).any(|c| c == cell));
    if blocked {
        return;
    }
    put_object(
        &mut map,
        cell,
        obj,
        &objects,
        &shapes,
        &assets,
        &mut commands,
    );
    inv.consume_held();
    placed.write(ObjectPlaced { cell, object: obj });
}
