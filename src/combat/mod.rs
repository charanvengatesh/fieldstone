pub mod ai;
pub mod spawn;
pub mod weapon;

use bevy::prelude::*;

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<spawn::SpawnTimer>()
            .add_message::<crate::fx::Swing>()
            .add_systems(
                Update,
                (
                    weapon::attack,
                    weapon::fly,
                    ai::think,
                    spawn::natural_spawns,
                    spawn::despawn_far,
                )
                    .chain()
                    .in_set(CombatSet),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct CombatSet;
