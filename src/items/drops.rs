//! Items lying in the world. They bob, drift toward a nearby player, and get picked up.

use bevy::prelude::*;

use super::inventory::Inventory;
use crate::art::ShapeAssets;
use crate::core::{ItemDef, LootDef, Registry, ShapeDef, hash01};
use crate::player::Player;
use crate::world::CELL;

const MAGNET: f32 = CELL * 1.5;
const PICKUP: f32 = CELL * 0.4;
const DROP_SCALE: f32 = 0.5;
/// Seconds before a forgotten drop fades away.
const LIFETIME: f32 = 300.0;

#[derive(Component)]
pub struct Drop {
    pub item: u16,
    pub n: u32,
    pub age: f32,
}

/// Request to spawn items at a world position. Sent by mining, crafting overflow, and scripts.
#[derive(Message)]
pub struct SpawnDrop {
    pub item: u16,
    pub n: u32,
    pub at: Vec2,
}

/// A stack (or part of one) entered the inventory.
#[derive(Message)]
#[allow(dead_code)] // item and count are for scripts and stats later
pub struct PickedUp {
    pub item: u16,
    pub n: u32,
}

/// Rolls a loot table into drop messages. `salt` keeps rolls deterministic per cell and hit.
pub fn roll_loot(
    table: &LootDef,
    items: &Registry<ItemDef>,
    at: Vec2,
    seed: u32,
    salt: u32,
    out: &mut MessageWriter<SpawnDrop>,
) {
    for (i, e) in table.entries.iter().enumerate() {
        let cell = (at / CELL).round().as_ivec2();
        let r = hash01(seed, cell.x, cell.y, salt.wrapping_add(i as u32 * 7919));
        if r >= e.chance {
            continue;
        }
        let Some(item) = items.idx(&e.item) else {
            warn!("loot '{}' names unknown item '{}'", table.id, e.item);
            continue;
        };
        let span = e.max.saturating_sub(e.min) + 1;
        let n = e.min + ((r / e.chance) * span as f32) as u32 % span;
        if n > 0 {
            out.write(SpawnDrop { item, n, at });
        }
    }
}

pub fn spawn(
    mut reader: MessageReader<SpawnDrop>,
    items: Res<Registry<ItemDef>>,
    shapes: Res<Registry<ShapeDef>>,
    assets: Res<ShapeAssets>,
    mut commands: Commands,
) {
    for (i, d) in reader.read().enumerate() {
        let jitter = Vec2::from_angle(i as f32 * 2.4) * CELL * 0.2;
        let scale = if assets.pixel() { 0.75 } else { DROP_SCALE };
        let tf =
            Transform::from_translation((d.at + jitter).extend(1.5)).with_scale(Vec3::splat(scale));
        let e = crate::art::spawn_shape(
            &mut commands,
            &assets,
            shapes.idx(&items.at(d.item).shape),
            tf,
        );
        commands.entity(e).insert(Drop {
            item: d.item,
            n: d.n,
            age: 0.0,
        });
    }
}

pub fn collect(
    time: Res<Time>,
    items: Res<Registry<ItemDef>>,
    mut player: Single<(&Transform, &mut Inventory), With<Player>>,
    mut drops: Query<(Entity, &mut Transform, &mut Drop), Without<Player>>,
    mut picked: MessageWriter<PickedUp>,
    mut commands: Commands,
) {
    let (ptf, inv) = &mut *player;
    let target = ptf.translation.truncate();
    for (e, mut tf, mut d) in &mut drops {
        d.age += time.delta_secs();
        if d.age > LIFETIME {
            commands.entity(e).despawn();
            continue;
        }
        let p = tf.translation.truncate();
        let dist = p.distance(target);
        // Short grace period so a freshly dropped stack is visible before it flies in.
        if d.age > 0.3 && dist < MAGNET && d.age > 1.5
            || (d.age > 0.3 && dist < MAGNET && d.age <= 1.5 && dist > CELL * 2.0)
        {
            let step = (p - target).normalize_or_zero() * -(400.0 * time.delta_secs());
            tf.translation += step.extend(0.0);
        }
        tf.translation.z = 1.5 + (d.age * 3.0).sin() * 0.01;
        if dist < PICKUP {
            let before = d.n;
            d.n = inv.add(d.item, d.n, &items);
            if before > d.n {
                picked.write(PickedUp {
                    item: d.item,
                    n: before - d.n,
                });
            }
            if d.n == 0 {
                commands.entity(e).despawn();
            }
        }
    }
}
