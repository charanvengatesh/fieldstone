pub mod blend;
pub mod chunk;
pub mod collide;
pub mod stream;
pub mod tiles;
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

/// Whether a mover standing in `from` may step into `to` as far as height goes: same level,
/// or either cell holds a ramp. Unloaded cells count as level 0.
pub fn passable(map: &ChunkMap, objects: &Registry<ObjectDef>, from: IVec2, to: IVec2) -> bool {
    let (a, b) = (map.height(from), map.height(to));
    if a == b {
        return true;
    }
    let is_ramp = |c: IVec2| {
        map.tile(c)
            .and_then(|t| t.object)
            .is_some_and(|o| objects.id_of(o) == "ramp")
    };
    a.abs_diff(b) == 1 && (is_ramp(from) || is_ramp(to))
}

/// Collision closure for a mover currently in `here`: solid boxes plus height steps.
pub fn blocker<'a>(
    map: &'a ChunkMap,
    terrain: &'a Registry<TerrainDef>,
    objects: &'a Registry<ObjectDef>,
    here: IVec2,
) -> impl Fn(IVec2) -> Option<f32> + 'a {
    move |c: IVec2| {
        solid_half(map, terrain, objects, c)
            .or_else(|| (!passable(map, objects, here, c)).then_some(CELL / 2.0))
    }
}

/// Whether a straight line between two points crosses no solid box (sampled every ~8 units).
pub fn clear_line(
    map: &ChunkMap,
    terrain: &Registry<TerrainDef>,
    objects: &Registry<ObjectDef>,
    from: Vec2,
    to: Vec2,
) -> bool {
    let steps = ((to - from).length() / 8.0).ceil().max(1.0) as i32;
    (1..steps).all(|i| {
        let p = from.lerp(to, i as f32 / steps as f32);
        let cell = cell_of(p);
        solid_half(map, terrain, objects, cell)
            .is_none_or(|h| (p - center_of(cell)).abs().max_element() >= h)
    })
}

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChunkMap>()
            .init_resource::<tiles::TileSheets>()
            .init_resource::<tiles::WaterClock>()
            .add_systems(
                PreStartup,
                tiles::build_sheets.after(crate::art::build_shapes),
            )
            .add_systems(Update, tiles::animate_water)
            .add_systems(Startup, stream::setup)
            .add_systems(
                Update,
                (stream::stream, stream::draw).chain().in_set(WorldSet),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldSet;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ObjectDef;
    use chunk::{Chunk, Tile, local_index};

    fn world(heights: &[(IVec2, u8)], ramps: &[IVec2]) -> (ChunkMap, Registry<ObjectDef>) {
        let mut objects = Registry::<ObjectDef>::default();
        let ramp: ObjectDef = ron::from_str(r#"(id: "ramp", shape: "ramp", solid: false)"#).unwrap();
        let ramp_id = objects.insert(ramp);
        let mut chunk = Chunk::filled(Tile::default());
        for (c, h) in heights {
            chunk.tiles[local_index(*c)].height = *h;
        }
        for c in ramps {
            chunk.tiles[local_index(*c)].object = Some(ramp_id);
        }
        let mut map = ChunkMap::default();
        map.chunks.insert(IVec2::ZERO, chunk);
        (map, objects)
    }

    #[test]
    fn steps_need_a_ramp() {
        let a = IVec2::new(2, 2);
        let up = IVec2::new(2, 3);
        let (map, objects) = world(&[(up, 1)], &[]);
        assert!(!passable(&map, &objects, a, up));
        assert!(passable(&map, &objects, a, IVec2::new(3, 2)));
        let (map, objects) = world(&[(up, 1)], &[a]);
        assert!(passable(&map, &objects, a, up));
        assert!(passable(&map, &objects, up, a));
        // Two levels at once never pass, ramp or not.
        let (map, objects) = world(&[(up, 2)], &[a]);
        assert!(!passable(&map, &objects, a, up));
    }
}
