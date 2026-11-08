//! Fishing: cast a rod at water, wait for the bite, click inside the window.

use bevy::prelude::*;

use crate::core::time::GameTime;
use crate::core::{FishTableDef, ItemDef, Registry, Rng, TerrainDef, TimeOfDay, Use};
use crate::items::drops::SpawnDrop;
use crate::items::interact::in_reach;
use crate::items::inventory::Inventory;
use crate::player::{Cursor, Player};
use crate::stats::GainXp;
use crate::world::ChunkMap;

const BITE_WINDOW: f32 = 0.8;

#[derive(Component)]
pub struct Fishing {
    pub table: u16,
    pub power: u8,
    /// Seconds until the bite; negative while the bite window is open.
    pub until_bite: f32,
}

/// Status line for the HUD: `None` when idle.
#[derive(Resource, Default)]
pub struct FishingStatus(pub Option<&'static str>);

pub fn cast_and_catch(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    clock: Res<GameTime>,
    cursor: Res<Cursor>,
    mut rng: ResMut<Rng>,
    items: Res<Registry<ItemDef>>,
    terrain: Res<Registry<TerrainDef>>,
    tables: Res<Registry<FishTableDef>>,
    map: Res<ChunkMap>,
    mut status: ResMut<FishingStatus>,
    mut toast: ResMut<crate::core::Toast>,
    ui: Res<crate::core::UiHovered>,
    mut player: Single<(Entity, &Transform, &Inventory, Option<&mut Fishing>), With<Player>>,
    mut drops: MessageWriter<SpawnDrop>,
    mut xp: MessageWriter<GainXp>,
    mut commands: Commands,
) {
    let (entity, tf, inv, fishing) = &mut *player;
    let pos = tf.translation.truncate();
    let rod = match inv.held().map(|s| &items.at(s.item).usage) {
        Some(Use::Rod { power }) => Some(*power),
        _ => None,
    };
    let Some(power) = rod else {
        if fishing.is_some() {
            commands.entity(*entity).remove::<Fishing>();
        }
        status.0 = None;
        return;
    };
    match fishing {
        None => {
            status.0 = None;
            if !mouse.just_pressed(MouseButton::Left) || ui.0 {
                return;
            }
            let Some(cell) = cursor.cell.filter(|c| in_reach(pos, *c)) else {
                return;
            };
            let Some(table) = map
                .tile(cell)
                .and_then(|t| terrain.at(t.terrain).fish_table.as_deref())
                .and_then(|id| tables.idx(id))
            else {
                return;
            };
            commands.entity(*entity).insert(Fishing {
                table,
                power,
                until_bite: rng.range(2.0, 6.0),
            });
            status.0 = Some("fishing...");
        }
        Some(f) => {
            f.until_bite -= time.delta_secs();
            if f.until_bite > 0.0 {
                status.0 = Some("fishing...");
                return;
            }
            status.0 = Some("! bite !");
            let clicked = mouse.just_pressed(MouseButton::Left);
            if f.until_bite < -BITE_WINDOW || clicked {
                if clicked {
                    let night = clock.day_frac() >= 0.5;
                    let candidates: Vec<(u16, f32)> = tables
                        .at(f.table)
                        .entries
                        .iter()
                        .filter(|e| {
                            e.min_rod_power <= f.power
                                && match e.time {
                                    TimeOfDay::Any => true,
                                    TimeOfDay::Day => !night,
                                    TimeOfDay::Night => night,
                                }
                        })
                        .filter_map(|e| items.idx(&e.item).map(|i| (i, e.weight)))
                        .collect();
                    let total: f32 = candidates.iter().map(|c| c.1).sum();
                    if total <= 0.0 {
                        toast.show("Nothing bites. A stronger rod, or another hour.", 3.0);
                    }
                    let mut pick = rng.range(0.0, total);
                    for (item, w) in candidates {
                        pick -= w;
                        if pick <= 0.0 {
                            drops.write(SpawnDrop {
                                item,
                                n: 1,
                                at: pos,
                            });
                            xp.write(GainXp {
                                amount: 5.0,
                                skill: "fishing",
                            });
                            break;
                        }
                    }
                }
                commands.entity(*entity).remove::<Fishing>();
                status.0 = None;
            }
        }
    }
}

pub struct FishPlugin;

impl Plugin for FishPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FishingStatus>()
            .add_systems(Update, cast_and_catch.in_set(FishSet));
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FishSet;
