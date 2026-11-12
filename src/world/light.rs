//! Block light: torches, crystals and lava light their surroundings. Light spreads from an
//! emitter through open cells, one level less per block, out to `REACH` blocks. A chunk's
//! grid is flooded from every emitter in it and its loaded neighbours, so light crosses
//! chunk borders; an emitter in a chunk that is not loaded yet counts once it is.

use std::collections::VecDeque;

use bevy::prelude::*;

use super::{AIR, CHUNK, HEIGHT, Map, Nearby, chunk_of};

/// The brightest emission, and how many blocks light travels from it.
pub const REACH: i32 = 15;

/// A chunk's light levels with a margin of `REACH` around it, so vertices on the border see
/// light from next door.
pub struct Grid {
    origin: IVec2,
    side: i32,
    levels: Vec<u8>,
}

impl Grid {
    /// A grid with no light at all.
    pub fn empty() -> Self {
        Self {
            origin: IVec2::ZERO,
            side: 0,
            levels: Vec::new(),
        }
    }

    /// Light at a world cell, 0 outside the grid or where nothing shines.
    pub fn at(&self, p: IVec3) -> u8 {
        let l = p.truncate() - self.origin;
        if l.x < 0 || l.y < 0 || l.x >= self.side || l.y >= self.side || !(0..HEIGHT).contains(&p.z)
        {
            return 0;
        }
        self.levels[((p.z * self.side + l.y) * self.side + l.x) as usize]
    }

    fn slot(&self, p: IVec3) -> Option<usize> {
        let l = p.truncate() - self.origin;
        (l.x >= 0 && l.y >= 0 && l.x < self.side && l.y < self.side && (0..HEIGHT).contains(&p.z))
            .then(|| ((p.z * self.side + l.y) * self.side + l.x) as usize)
    }
}

/// Floods the light grid for chunk `pos`. `emits` is each block kind's light level and
/// `passes` whether light travels through it; `block` answers for any position.
pub fn flood(
    pos: IVec2,
    nearby: &Nearby,
    emits: &[u8],
    passes: impl Fn(u16) -> bool,
    block: impl Fn(IVec3) -> u16,
) -> Grid {
    let side = CHUNK + 2 * REACH;
    let origin = pos * CHUNK - IVec2::splat(REACH);
    // Emitters come from the loaded chunks that overlap the grid, read straight from
    // their block arrays. Most chunks have none, and then there is nothing to allocate.
    let mut seeds: Vec<(IVec3, u8)> = Vec::new();
    if emits.iter().any(|e| *e > 0) {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let c = pos + IVec2::new(dx, dy);
                let Some(blocks) = nearby.chunk(c) else {
                    continue;
                };
                let base = (c * CHUNK).extend(0);
                for (i, b) in blocks.iter().enumerate() {
                    let e = emits.get(*b as usize).copied().unwrap_or(0);
                    if e > 0 {
                        let i = i as i32;
                        let local = IVec3::new(i % CHUNK, (i / CHUNK) % CHUNK, i / (CHUNK * CHUNK));
                        seeds.push((base + local, e));
                    }
                }
            }
        }
    }
    if seeds.is_empty() {
        return Grid::empty();
    }
    let mut grid = Grid {
        origin,
        side,
        levels: vec![0; (side * side * HEIGHT) as usize],
    };
    let mut queue: VecDeque<(IVec3, u8)> = VecDeque::new();
    for (p, e) in seeds {
        if let Some(i) = grid.slot(p) {
            grid.levels[i] = e;
            queue.push_back((p, e));
        }
    }
    while let Some((p, level)) = queue.pop_front() {
        if level <= 1 {
            continue;
        }
        for d in [
            IVec3::X,
            IVec3::NEG_X,
            IVec3::Y,
            IVec3::NEG_Y,
            IVec3::Z,
            IVec3::NEG_Z,
        ] {
            let q = p + d;
            let Some(i) = grid.slot(q) else {
                continue;
            };
            let next = level - 1;
            if grid.levels[i] >= next {
                continue;
            }
            let b = block(q);
            if b != AIR && !passes(b) {
                continue;
            }
            grid.levels[i] = next;
            queue.push_back((q, next));
        }
    }
    grid
}

/// Light at one cell, flooding its chunk on demand. For scripts and tests.
pub fn level_at(map: &Map, emits: &[u8], passes: impl Fn(u16) -> bool, p: IVec3) -> u8 {
    let c = chunk_of(p);
    flood(c, &Nearby::of(map, c), emits, passes, |q| map.block(q)).at(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Chunk;

    /// One loaded chunk with a floor at z 0; block 1 is stone, block 2 a torch.
    fn world() -> Map {
        let mut map = Map::default();
        let mut chunk = Chunk::new(crate::world::empty_blocks(), false);
        for i in 0..(CHUNK * CHUNK) as usize {
            chunk.blocks[i] = 1;
        }
        map.chunks.insert(IVec2::ZERO, chunk);
        map
    }

    const EMITS: [u8; 3] = [0, 0, 14];

    #[test]
    fn light_falls_one_per_block_and_stops_at_walls() {
        let mut map = world();
        map.set(IVec3::new(8, 8, 1), 2);
        let passes = |b: u16| b == 0;
        let grid = flood(
            IVec2::ZERO,
            &Nearby::of(&map, IVec2::ZERO),
            &EMITS,
            passes,
            |q| map.block(q),
        );
        assert_eq!(grid.at(IVec3::new(8, 8, 1)), 14);
        assert_eq!(grid.at(IVec3::new(9, 8, 1)), 13);
        assert_eq!(grid.at(IVec3::new(8, 8, 5)), 10);
        assert_eq!(grid.at(IVec3::new(8, 8, 15)), 0);
        // Light goes round a wall rather than through it.
        map.set(IVec3::new(10, 8, 1), 1);
        map.set(IVec3::new(10, 8, 2), 1);
        let grid = flood(
            IVec2::ZERO,
            &Nearby::of(&map, IVec2::ZERO),
            &EMITS,
            passes,
            |q| map.block(q),
        );
        assert_eq!(grid.at(IVec3::new(10, 8, 1)), 0);
        assert_eq!(
            grid.at(IVec3::new(11, 8, 1)),
            9,
            "round the wall: five steps instead of three"
        );
        // The floor is dark, and nothing shines where nothing is loaded.
        assert_eq!(grid.at(IVec3::new(8, 8, 0)), 0);
        assert_eq!(level_at(&map, &EMITS, passes, IVec3::new(40, 40, 1)), 0);
    }

    #[test]
    fn light_crosses_into_the_next_chunk() {
        let mut map = world();
        map.chunks
            .insert(IVec2::X, Chunk::new(crate::world::empty_blocks(), false));
        map.set(IVec3::new(15, 4, 1), 2);
        let grid = flood(
            IVec2::X,
            &Nearby::of(&map, IVec2::X),
            &EMITS,
            |b| b == 0,
            |q| map.block(q),
        );
        assert_eq!(grid.at(IVec3::new(16, 4, 1)), 13);
        assert_eq!(grid.at(IVec3::new(20, 4, 1)), 9);
    }
}
