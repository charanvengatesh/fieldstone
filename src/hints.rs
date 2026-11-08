//! First-minutes guidance from `hints.ron`: each hint fires once when its trigger holds.

use bevy::prelude::*;

use crate::core::time::GameTime;
use crate::core::{HintDef, HintWhen, ItemDef, ObjectDef, Registry, Toast};
use crate::items::interact::ObjectPlaced;
use crate::items::inventory::Inventory;
use crate::player::Player;
use crate::stats::Progress;

#[derive(Resource, Default)]
pub struct Shown(Vec<u16>);

pub fn run(
    time: Res<Time>,
    clock: Res<GameTime>,
    hints: Res<Registry<HintDef>>,
    items: Res<Registry<ItemDef>>,
    objects: Res<Registry<ObjectDef>>,
    mut placed: MessageReader<ObjectPlaced>,
    mut shown: ResMut<Shown>,
    mut toast: ResMut<Toast>,
    mut elapsed: Local<f32>,
    player: Single<(&Inventory, &Progress), With<Player>>,
) {
    *elapsed += time.delta_secs();
    let (inv, progress) = *player;
    let placed_now: Vec<u16> = placed.read().map(|p| p.object).collect();
    if toast.left > 0.0 {
        return;
    }
    for (i, h) in hints.iter() {
        if shown.0.contains(&i) {
            continue;
        }
        let fire = match &h.when {
            HintWhen::After(secs) => *elapsed >= *secs,
            HintWhen::HasItem(id) => items.idx(id).is_some_and(|it| inv.count(it) > 0),
            HintWhen::Placed(id) => objects.idx(id).is_some_and(|o| placed_now.contains(&o)),
            HintWhen::Night => clock.day_frac() >= 0.5,
            HintWhen::Day(n) => clock.day() >= *n,
            HintWhen::Level(n) => progress.level >= *n,
        };
        if fire {
            shown.0.push(i);
            toast.show(h.text.clone(), h.secs);
            return;
        }
    }
}

pub struct HintsPlugin;

impl Plugin for HintsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shown>()
            .add_systems(Update, run.run_if(crate::core::not_paused));
    }
}
