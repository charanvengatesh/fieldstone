pub mod chunk;
pub mod collide;
pub mod stream;
pub mod worldgen;

use bevy::prelude::*;

pub use chunk::{CELL, ChunkMap, cell_of, center_of};
pub use collide::{PLAYER_R, overlapping_cells, slide};

use crate::core::{ObjectDef, Registry, TerrainDef};

/// Whether a cell blocks movement: unloaded, unwalkable terrain, or a solid object.
pub fn is_solid(
    map: &ChunkMap,
    terrain: &Registry<TerrainDef>,
    objects: &Registry<ObjectDef>,
    cell: IVec2,
) -> bool {
    match map.tile(cell) {
        None => true,
        Some(t) => !terrain.at(t.terrain).walkable || t.object.is_some_and(|o| objects.at(o).solid),
    }
}

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChunkMap>()
            .add_systems(Startup, stream::setup)
            .add_systems(
                Update,
                (stream::stream, stream::draw).chain().in_set(WorldSet),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldSet;
