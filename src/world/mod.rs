pub mod chunk;
pub mod collide;
pub mod stream;
pub mod worldgen;

use bevy::prelude::*;

pub use chunk::{CELL, ChunkMap, cell_of, center_of};
pub use collide::{PLAYER_R, overlapping_cells, slide};

use crate::core::{ObjectDef, Registry, TerrainDef};

/// Half-size of the collision box on a cell, or `None` if it is passable. Unloaded chunks and
/// unwalkable terrain fill the whole cell; objects use their own `radius`.
pub fn solid_half(
    map: &ChunkMap,
    terrain: &Registry<TerrainDef>,
    objects: &Registry<ObjectDef>,
    cell: IVec2,
) -> Option<f32> {
    let Some(t) = map.tile(cell) else {
        return Some(CELL / 2.0);
    };
    if !terrain.at(t.terrain).walkable {
        return Some(CELL / 2.0);
    }
    let o = objects.at(t.object?);
    (o.solid && o.radius > 0.0).then_some(o.radius * CELL)
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
