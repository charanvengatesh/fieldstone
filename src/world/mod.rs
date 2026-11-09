//! Block storage. The world is an unbounded plane of chunks, each `CHUNK x CHUNK` columns
//! of `HEIGHT` blocks. A block is a `u16` index into the block registry; 0 is air.

pub mod iso;
pub mod mesh;
pub mod noise;
pub mod physics;
pub mod stream;
pub mod worldgen;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

pub const CHUNK: i32 = 16;
pub const HEIGHT: i32 = 32;
pub const VOLUME: usize = (CHUNK * CHUNK * HEIGHT) as usize;
pub const AIR: u16 = 0;

pub type Blocks = Box<[u16; VOLUME]>;

pub fn empty_blocks() -> Blocks {
    vec![AIR; VOLUME]
        .into_boxed_slice()
        .try_into()
        .expect("volume")
}

pub struct Chunk {
    pub blocks: Blocks,
    /// The mesh no longer matches the blocks.
    pub dirty: bool,
    /// Differs from generation output, so it belongs in the save file.
    pub modified: bool,
}

/// Loaded chunks, the entity drawing each, and edited chunks that are currently unloaded.
#[derive(Resource, Default)]
pub struct Map {
    pub chunks: HashMap<IVec2, Chunk>,
    pub entities: HashMap<IVec2, Entity>,
    pub parked: HashMap<IVec2, Blocks>,
}

/// What the camera centres on, in block units.
#[derive(Resource, Default)]
pub struct Focus(pub Vec3);

pub fn chunk_of(p: IVec3) -> IVec2 {
    p.truncate().div_euclid(IVec2::splat(CHUNK))
}

/// Index of a block inside its chunk. `p.z` must be in `0..HEIGHT`.
pub fn index(p: IVec3) -> usize {
    let l = p.truncate().rem_euclid(IVec2::splat(CHUNK));
    ((p.z * CHUNK + l.y) * CHUNK + l.x) as usize
}

impl Map {
    /// The block at `p`, or `None` above or below the world or in an unloaded chunk.
    pub fn get(&self, p: IVec3) -> Option<u16> {
        if !(0..HEIGHT).contains(&p.z) {
            return None;
        }
        let c = chunk_of(p);
        match self.chunks.get(&c) {
            Some(chunk) => Some(chunk.blocks[index(p)]),
            None => self.parked.get(&c).map(|b| b[index(p)]),
        }
    }

    /// Like [`Map::get`] with air for anything unknown.
    pub fn block(&self, p: IVec3) -> u16 {
        self.get(p).unwrap_or(AIR)
    }

    /// Writes a block in a loaded chunk and marks every mesh that shows it. Returns whether
    /// anything changed.
    pub fn set(&mut self, p: IVec3, id: u16) -> bool {
        if !(0..HEIGHT).contains(&p.z) {
            return false;
        }
        let c = chunk_of(p);
        let Some(chunk) = self.chunks.get_mut(&c) else {
            return false;
        };
        let i = index(p);
        if chunk.blocks[i] == id {
            return false;
        }
        chunk.blocks[i] = id;
        chunk.dirty = true;
        chunk.modified = true;
        // A block on a chunk border changes the faces and outlines of the chunk beside it.
        let l = p.truncate().rem_euclid(IVec2::splat(CHUNK));
        for (on_edge, d) in [
            (l.x == 0, IVec2::NEG_X),
            (l.x == CHUNK - 1, IVec2::X),
            (l.y == 0, IVec2::NEG_Y),
            (l.y == CHUNK - 1, IVec2::Y),
        ] {
            if on_edge && let Some(n) = self.chunks.get_mut(&(c + d)) {
                n.dirty = true;
            }
        }
        true
    }

    /// Height of the highest non-air block in a loaded column.
    pub fn top(&self, x: i32, y: i32) -> Option<i32> {
        self.get(IVec3::new(x, y, 0))?;
        (0..HEIGHT)
            .rev()
            .find(|z| self.block(IVec3::new(x, y, *z)) != AIR)
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldSet;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Map>()
            .init_resource::<Focus>()
            .init_resource::<iso::View>()
            .init_resource::<mesh::Grid>()
            .add_systems(Startup, mesh::setup)
            .add_systems(
                Update,
                (stream::stream, stream::view_changed, stream::remesh)
                    .chain()
                    .in_set(WorldSet),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_with(chunks: &[IVec2]) -> Map {
        let mut map = Map::default();
        for c in chunks {
            map.chunks.insert(
                *c,
                Chunk {
                    blocks: empty_blocks(),
                    dirty: false,
                    modified: false,
                },
            );
        }
        map
    }

    #[test]
    fn negative_cells_map_into_their_chunk() {
        assert_eq!(chunk_of(IVec3::new(-1, 16, 3)), IVec2::new(-1, 1));
        assert_eq!(index(IVec3::new(-1, 16, 0)), 15);
        assert_eq!(index(IVec3::new(0, 0, 1)), (CHUNK * CHUNK) as usize);
    }

    #[test]
    fn set_marks_the_chunk_and_its_border_neighbour() {
        let mut map = map_with(&[IVec2::ZERO, IVec2::X, IVec2::Y]);
        assert!(map.set(IVec3::new(15, 4, 2), 5));
        assert!(!map.set(IVec3::new(15, 4, 2), 5));
        assert_eq!(map.block(IVec3::new(15, 4, 2)), 5);
        assert!(map.chunks[&IVec2::ZERO].modified);
        assert!(map.chunks[&IVec2::X].dirty);
        assert!(!map.chunks[&IVec2::Y].dirty);
        assert!(!map.chunks[&IVec2::X].modified);
        // Outside the world or the loaded area nothing is written.
        assert!(!map.set(IVec3::new(0, 0, HEIGHT), 1));
        assert!(!map.set(IVec3::new(-40, 0, 0), 1));
        assert_eq!(map.top(15, 4), Some(2));
        assert_eq!(map.top(0, 0), None);
    }
}
