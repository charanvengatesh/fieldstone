//! Tile storage. The world is an unbounded grid of `CHUNK x CHUNK` chunks keyed by chunk
//! coordinate; a cell's chunk is `cell.div_euclid(CHUNK)`.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

pub const CHUNK: i32 = 32;
pub const TILES: usize = (CHUNK * CHUNK) as usize;
/// World units per cell. One texel of 16 px pixel art is one world unit.
pub const CELL: f32 = 16.0;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Tile {
    pub terrain: u16,
    pub object: Option<u16>,
    /// Terrace level 0..3; cliffs stand between different heights.
    pub height: u8,
}

pub struct Chunk {
    pub tiles: [Tile; TILES],
    /// Terrain changed since the mesh was last built.
    pub dirty: bool,
    /// Differs from generation output, so it belongs in the save file.
    pub modified: bool,
}

impl Chunk {
    pub fn filled(tile: Tile) -> Self {
        Self {
            tiles: [tile; TILES],
            dirty: true,
            modified: false,
        }
    }
}

/// Loaded chunks plus the entity drawing each one.
#[derive(Resource, Default)]
pub struct ChunkMap {
    pub chunks: HashMap<IVec2, Chunk>,
    pub entities: HashMap<IVec2, Entity>,
    /// Entity drawing the object on a cell, if the cell holds one and its chunk is loaded.
    pub objects: HashMap<IVec2, Entity>,
    /// Water overlay entity per chunk, when the chunk touches water.
    pub water: HashMap<IVec2, Entity>,
    /// Per-terrain edge layers per chunk (see `ground.rs`).
    pub layers: HashMap<IVec2, Vec<Entity>>,
    /// Cells holding a light-giving object, kept current by `stream`.
    pub lights: bevy::platform::collections::HashSet<IVec2>,
}

pub fn chunk_of(cell: IVec2) -> IVec2 {
    cell.div_euclid(IVec2::splat(CHUNK))
}

pub fn local_index(cell: IVec2) -> usize {
    let l = cell.rem_euclid(IVec2::splat(CHUNK));
    (l.y * CHUNK + l.x) as usize
}

pub fn cell_of(p: Vec2) -> IVec2 {
    (p / CELL).round().as_ivec2()
}

pub fn center_of(c: IVec2) -> Vec2 {
    c.as_vec2() * CELL
}

impl ChunkMap {
    pub fn tile(&self, cell: IVec2) -> Option<Tile> {
        self.chunks
            .get(&chunk_of(cell))
            .map(|c| c.tiles[local_index(cell)])
    }

    /// Height of a loaded cell; unloaded cells report 0 so edges of the world stay flat.
    pub fn height(&self, cell: IVec2) -> u8 {
        self.tile(cell).map_or(0, |t| t.height)
    }

    /// Returns the previous tile if the chunk is loaded.
    pub fn set(&mut self, cell: IVec2, tile: Tile) -> Option<Tile> {
        let c = self.chunks.get_mut(&chunk_of(cell))?;
        let slot = &mut c.tiles[local_index(cell)];
        let old = *slot;
        if old != tile {
            c.modified = true;
            c.dirty |= old.terrain != tile.terrain;
            *slot = tile;
        }
        Some(old)
    }

    pub fn set_object(&mut self, cell: IVec2, object: Option<u16>) -> Option<Tile> {
        let t = self.tile(cell)?;
        self.set(cell, Tile { object, ..t })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_math_handles_negatives() {
        assert_eq!(chunk_of(IVec2::new(0, 0)), IVec2::ZERO);
        assert_eq!(chunk_of(IVec2::new(-1, 31)), IVec2::new(-1, 0));
        assert_eq!(chunk_of(IVec2::new(-32, 32)), IVec2::new(-1, 1));
        assert_eq!(local_index(IVec2::new(-1, 0)), 31);
        assert_eq!(local_index(IVec2::new(32, 33)), CHUNK as usize);
    }

    #[test]
    fn cell_round_trip() {
        for c in [IVec2::ZERO, IVec2::new(-300, 17)] {
            assert_eq!(cell_of(center_of(c)), c);
        }
        assert_eq!(cell_of(Vec2::new(CELL * 0.49, -CELL * 0.49)), IVec2::ZERO);
    }
}
