//! Using the held item on the world. Built for trackpads: a single click on an object sets a
//! mining target that keeps working (and walks you closer) until it breaks, you move, or you
//! click elsewhere. Holding the button still works. `F` does what right click does.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
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

pub const REACH: f32 = CELL * 6.0;
/// Damage per second from bare hands or a tool whose power exactly meets the hardness.
const BASE_DPS: f32 = 1.5;
/// Scroll distance (in pixels, lines count as 40) per hotbar step.
const SCROLL_STEP: f32 = 36.0;

/// Remaining hit points of objects currently being mined.
#[derive(Resource, Default)]
pub struct Damage(pub HashMap<IVec2, f32>);

/// Cell the player is working on after a click, if any.
#[derive(Resource, Default)]
pub struct Target(pub Option<IVec2>);

/// Fired when an object breaks; farming, scripts and stats listen.
#[derive(Message)]
pub struct ObjectBroken {
    pub cell: IVec2,
    pub object: u16,
}

#[derive(Message)]
pub struct ObjectPlaced {
    pub cell: IVec2,
    pub object: u16,
}

/// Right click or `F`.
pub fn use_pressed(mouse: &ButtonInput<MouseButton>, keys: &ButtonInput<KeyCode>) -> bool {
    mouse.just_pressed(MouseButton::Right) || keys.just_pressed(KeyCode::KeyF)
}

pub fn select_slot(
    keys: Res<ButtonInput<KeyCode>>,
    mut wheel: MessageReader<MouseWheel>,
    mut acc: Local<f32>,
    mut inv: Single<&mut Inventory, With<Player>>,
) {
    use KeyCode::*;
    const DIGITS: [KeyCode; HOTBAR] = [
        Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0,
    ];
    if let Some(i) = DIGITS.iter().position(|k| keys.just_pressed(*k)) {
        inv.selected = i;
    }
    // Trackpads emit many tiny pixel deltas; accumulate and step at a threshold.
    for w in wheel.read() {
        let scale = match w.unit {
            MouseScrollUnit::Line => 40.0,
            MouseScrollUnit::Pixel => 1.0,
        };
        *acc += (w.y + w.x) * scale;
    }
    while *acc <= -SCROLL_STEP {
        *acc += SCROLL_STEP;
        inv.selected = (inv.selected + 1) % HOTBAR;
    }
    while *acc >= SCROLL_STEP {
        *acc -= SCROLL_STEP;
        inv.selected = (inv.selected + HOTBAR - 1) % HOTBAR;
    }
}

pub fn in_reach(player: Vec2, cell: IVec2) -> bool {
    player.distance(center_of(cell)) <= REACH
}

/// Click an object to target it; click empty ground or press Escape to clear.
pub fn pick_target(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<Cursor>,
    map: Res<ChunkMap>,
    items: Res<Registry<ItemDef>>,
    inv: Single<&Inventory, With<Player>>,
    mut target: ResMut<Target>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        target.0 = None;
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    // Weapons and rods have their own click handling.
    if matches!(
        inv.held().map(|s| &items.at(s.item).usage),
        Some(Use::Weapon { .. } | Use::Rod { .. })
    ) {
        target.0 = None;
        return;
    }
    target.0 = cursor
        .cell
        .filter(|c| map.tile(*c).is_some_and(|t| t.object.is_some()));
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
    mut target: ResMut<Target>,
    player: Single<(&Transform, &Inventory, &Progress), With<Player>>,
    mut drops: MessageWriter<SpawnDrop>,
    mut broken: MessageWriter<ObjectBroken>,
    mut xp: MessageWriter<GainXp>,
    mut commands: Commands,
) {
    let (tf, inv, progress) = *player;
    let pos = tf.translation.truncate();
    let held_cell = cursor.cell.filter(|_| mouse.pressed(MouseButton::Left));
    let cell = match target.0.or(held_cell) {
        Some(c) if in_reach(pos, c) => c,
        _ => {
            damage.0.clear();
            return;
        }
    };
    let Some(obj) = map.tile(cell).and_then(|t| t.object) else {
        target.0 = None;
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
    target.0 = None;
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

/// Right click / `F`: eat the held food, or place the held object on the hovered cell.
pub fn place(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
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
    if !use_pressed(&mouse, &keys) {
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
    let Some(cell) = cursor.cell.filter(|c| in_reach(pos, *c)) else {
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
    let ground = terrain.at(tile.terrain);
    let def = objects.at(obj);
    let blocked = tile.object.is_some()
        || !ground.walkable
        || def
            .on
            .as_ref()
            .is_some_and(|allowed| !allowed.iter().any(|t| t == &ground.id))
        || (def.solid && overlapping_cells(pos, PLAYER_R + def.radius * CELL).any(|c| c == cell));
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
