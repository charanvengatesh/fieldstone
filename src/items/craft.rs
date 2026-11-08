//! Recipes: inputs from the inventory, optional station adjacency, output back in (or dropped).

use bevy::prelude::*;

use super::drops::SpawnDrop;
use super::inventory::Inventory;
use crate::core::{ItemDef, ObjectDef, RecipeDef, Registry};
use crate::player::Player;
use crate::stats::GainXp;
use crate::world::{ChunkMap, cell_of};

/// Cells around the player that count as "next to" a station.
const STATION_RANGE: i32 = 2;

#[derive(Message)]
pub struct CraftRequest(pub u16);

#[derive(Message)]
#[allow(dead_code)] // listeners arrive with stats and scripting
pub struct Crafted {
    pub recipe: u16,
}

/// Station tags reachable from the player's position.
pub fn nearby_stations<'a>(
    map: &'a ChunkMap,
    objects: &'a Registry<ObjectDef>,
    at: Vec2,
) -> impl Iterator<Item = &'a str> {
    let c = cell_of(at);
    (-STATION_RANGE..=STATION_RANGE).flat_map(move |dy| {
        (-STATION_RANGE..=STATION_RANGE).filter_map(move |dx| {
            let t = map.tile(c + IVec2::new(dx, dy))?;
            objects.at(t.object?).station.as_deref()
        })
    })
}

pub fn can_craft(
    r: &RecipeDef,
    inv: &Inventory,
    items: &Registry<ItemDef>,
    stations: &[&str],
) -> bool {
    r.station.as_deref().is_none_or(|s| stations.contains(&s))
        && r.inputs
            .iter()
            .all(|(id, n)| items.idx(id).is_some_and(|i| inv.count(i) >= *n))
}

pub fn craft(
    mut requests: MessageReader<CraftRequest>,
    recipes: Res<Registry<RecipeDef>>,
    items: Res<Registry<ItemDef>>,
    objects: Res<Registry<ObjectDef>>,
    map: Res<ChunkMap>,
    mut player: Single<(&Transform, &mut Inventory), With<Player>>,
    mut drops: MessageWriter<SpawnDrop>,
    mut crafted: MessageWriter<Crafted>,
    mut xp: MessageWriter<GainXp>,
) {
    let (tf, inv) = &mut *player;
    let pos = tf.translation.truncate();
    for req in requests.read() {
        let r = recipes.at(req.0);
        let stations: Vec<&str> = nearby_stations(&map, &objects, pos).collect();
        if !can_craft(r, inv, &items, &stations) {
            continue;
        }
        for (id, n) in &r.inputs {
            inv.remove(items.idx(id).unwrap(), *n);
        }
        let Some(out) = items.idx(&r.output.0) else {
            warn!("recipe '{}' outputs unknown item '{}'", r.id, r.output.0);
            continue;
        };
        let left = inv.add(out, r.output.1, &items);
        if left > 0 {
            drops.write(SpawnDrop {
                item: out,
                n: left,
                at: pos,
            });
        }
        crafted.write(Crafted { recipe: req.0 });
        xp.write(GainXp {
            amount: 2.0 * r.inputs.iter().map(|(_, n)| *n as f32).sum::<f32>(),
            skill: "crafting",
        });
    }
}
