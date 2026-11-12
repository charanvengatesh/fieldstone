//! Block storage. The world is an unbounded plane of chunks, each `CHUNK x CHUNK` columns
//! of `HEIGHT` blocks. A block is a `u16` index into the block registry; 0 is air.

pub mod light;
pub mod mesh;
pub mod noise;
pub mod physics;
pub mod stamp;
pub mod stream;
pub mod tiles;
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
    /// Faces in the current mesh.
    pub quads: u32,
    /// How many times this chunk's mesh has been built since it was loaded.
    pub builds: u32,
}

impl Chunk {
    /// A freshly loaded chunk, waiting for its first mesh.
    pub fn new(blocks: Blocks, modified: bool) -> Self {
        Self {
            blocks,
            dirty: true,
            modified,
            quads: 0,
            builds: 0,
        }
    }
}

/// What a ray through the world hit: the cell and the face it came in by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub block: IVec3,
    pub normal: IVec3,
}

/// Walks a ray from `origin` along unit `dir` through the cells for up to `reach` blocks
/// and returns the first cell for which `solid` holds, with the face it entered by.
pub fn pick(origin: Vec3, dir: Vec3, reach: f32, solid: impl Fn(IVec3) -> bool) -> Option<Hit> {
    let start = origin + dir * 1e-4;
    let mut cell = start.floor().as_ivec3();
    let mut step = IVec3::ZERO;
    // Ray distance to the next boundary on each axis, and between boundaries. An axis the
    // ray runs along never comes up.
    let mut next = Vec3::splat(f32::INFINITY);
    let mut gap = Vec3::splat(f32::INFINITY);
    for a in 0..3 {
        if dir[a].abs() < 1e-6 {
            continue;
        }
        step[a] = dir[a].signum() as i32;
        let frac = start[a] - cell[a] as f32;
        let ahead = if step[a] > 0 { 1.0 - frac } else { frac };
        next[a] = ahead / dir[a].abs();
        gap[a] = 1.0 / dir[a].abs();
    }
    let mut normal = -IVec3::new(step.x, 0, 0);
    if normal == IVec3::ZERO {
        normal = IVec3::Z;
    }
    let mut travelled = 0.0;
    while travelled <= reach {
        if (0..HEIGHT).contains(&cell.z) && solid(cell) {
            return Some(Hit {
                block: cell,
                normal,
            });
        }
        // Below the world nothing is ever hit; above it the ray may still come down.
        if cell.z < 0 && dir.z <= 0.0 {
            return None;
        }
        let a = if next.x <= next.y && next.x <= next.z {
            0
        } else if next.y <= next.z {
            1
        } else {
            2
        };
        travelled = next[a];
        cell[a] += step[a];
        next[a] += gap[a];
        normal = IVec3::ZERO;
        normal[a] = -step[a];
    }
    None
}

/// Loaded chunks, the entity drawing each, and edited chunks that are currently unloaded.
#[derive(Resource, Default)]
pub struct Map {
    pub chunks: HashMap<IVec2, Chunk>,
    pub entities: HashMap<IVec2, Entity>,
    /// The entity drawing each chunk's water, where it has any.
    pub waters: HashMap<IVec2, Entity>,
    pub parked: HashMap<IVec2, Blocks>,
}

/// A chunk's blocks and its eight neighbours', copied out of the map so a build can run off
/// the main thread. A cell that is not loaded (nor parked) is `None`.
pub struct Nearby {
    pos: IVec2,
    cells: [Option<Blocks>; 9],
}

impl Nearby {
    pub fn of(map: &Map, pos: IVec2) -> Self {
        let mut cells: [Option<Blocks>; 9] = Default::default();
        for dy in -1..=1 {
            for dx in -1..=1 {
                let c = pos + IVec2::new(dx, dy);
                let blocks = map
                    .chunks
                    .get(&c)
                    .map(|ch| &ch.blocks)
                    .or_else(|| map.parked.get(&c));
                cells[((dy + 1) * 3 + dx + 1) as usize] = blocks.cloned();
            }
        }
        Self { pos, cells }
    }

    pub fn chunk(&self, c: IVec2) -> Option<&Blocks> {
        let d = c - self.pos;
        if d.abs().max_element() > 1 {
            return None;
        }
        self.cells[((d.y + 1) * 3 + d.x + 1) as usize].as_ref()
    }

    /// The block at `p`, or `None` outside the nine chunks or the world's height.
    pub fn get(&self, p: IVec3) -> Option<u16> {
        if !(0..HEIGHT).contains(&p.z) {
            return None;
        }
        self.chunk(chunk_of(p)).map(|b| b[index(p)])
    }
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
        // A block changes the light on faces up to `LIGHT_REACH` away, which may be in the
        // chunks beside this one.
        let r = mesh::LIGHT_REACH;
        for dy in [-r, 0, r] {
            for dx in [-r, 0, r] {
                let n = chunk_of(p + IVec3::new(dx, dy, 0));
                if n != c
                    && let Some(chunk) = self.chunks.get_mut(&n)
                {
                    chunk.dirty = true;
                }
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

/// Where the view angle and tilt for this frame are decided: after scripts and input,
/// before the camera follows and the world streams.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewSet;

/// Everything that projects a world position to the screen for this frame: the figure,
/// its silhouette and shadow, the highlight, particles. After [`ViewSet`], before the
/// camera follows.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlaceSet;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Map>()
            .init_resource::<Focus>()
            .init_resource::<crate::cam::Cam>()
            .init_resource::<mesh::Grid>()
            .init_resource::<mesh::Quality>()
            .init_resource::<stream::Picture>()
            .init_resource::<stream::Visible>()
            .init_resource::<stream::Streaming>()
            .init_resource::<stream::Perf>()
            .init_resource::<stream::Builds>()
            .add_systems(Startup, mesh::setup)
            // The view settles before the camera reads it, so the camera and the world
            // agree on the angle within a frame; scripts (the camera mod) get their say
            // first.
            .add_systems(
                Update,
                (stream::view_keys, stream::ease_view)
                    .chain()
                    .in_set(ViewSet)
                    .after(crate::script::ScriptSet)
                    .before(WorldSet),
            )
            .configure_sets(
                Update,
                PlaceSet
                    .after(ViewSet)
                    .after(crate::player::PlayerSet)
                    .after(crate::interact::InteractSet)
                    .before(WorldSet),
            )
            .add_systems(
                Update,
                (
                    mesh::tick,
                    stream::measure,
                    stream::stream,
                    stream::settings_changed,
                    stream::remesh,
                )
                    .chain()
                    .in_set(WorldSet),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ray_finds_the_first_solid_cell_and_the_face_it_entered() {
        // A floor at z 0 and a post at (3, 3, 1..3).
        let solid = |c: IVec3| c.z == 0 || (c.x == 3 && c.y == 3 && c.z <= 3);
        // Straight down onto the floor.
        let hit = pick(Vec3::new(1.5, 1.5, 10.0), Vec3::NEG_Z, 20.0, solid).unwrap();
        assert_eq!(hit.block, IVec3::new(1, 1, 0));
        assert_eq!(hit.normal, IVec3::Z);
        // Sideways into the post.
        let hit = pick(Vec3::new(0.5, 3.5, 2.5), Vec3::X, 20.0, solid).unwrap();
        assert_eq!(hit.block, IVec3::new(3, 3, 2));
        assert_eq!(hit.normal, IVec3::NEG_X);
        // A slanted ray from above: the post's top.
        let dir = Vec3::new(1.0, 1.0, -1.0).normalize();
        let hit = pick(Vec3::new(1.5, 1.5, 6.0), dir, 20.0, solid).unwrap();
        assert_eq!(hit.block, IVec3::new(3, 3, 3));
        assert_eq!(hit.normal, IVec3::Z);
        // Out of reach, or upward into nothing.
        assert!(pick(Vec3::new(0.5, 3.5, 2.5), Vec3::X, 1.0, solid).is_none());
        assert!(pick(Vec3::new(0.5, 0.5, 2.5), Vec3::Z, 20.0, solid).is_none());
    }

    fn map_with(chunks: &[IVec2]) -> Map {
        let mut map = Map::default();
        for c in chunks {
            map.chunks.insert(
                *c,
                Chunk {
                    dirty: false,
                    ..Chunk::new(empty_blocks(), false)
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
