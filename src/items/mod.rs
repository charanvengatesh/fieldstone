pub mod craft;
pub mod drops;
pub mod interact;
pub mod inventory;

use bevy::prelude::*;

pub struct ItemsPlugin;

impl Plugin for ItemsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<interact::Damage>()
            .add_message::<drops::SpawnDrop>()
            .add_message::<interact::ObjectBroken>()
            .add_message::<interact::ObjectPlaced>()
            .add_message::<craft::CraftRequest>()
            .add_message::<craft::Crafted>()
            .add_systems(
                Update,
                (
                    interact::select_slot,
                    interact::mine,
                    interact::place,
                    craft::craft,
                    drops::spawn,
                    drops::collect,
                )
                    .chain()
                    .in_set(ItemsSet),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemsSet;
