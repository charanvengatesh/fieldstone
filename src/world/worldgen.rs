//! Terrain from `worldgen.ron`: a noise height field decides how tall each column is, the
//! band it falls in decides what it is made of, and low columns are flooded. Every block is
//! a pure function of the seed and its position, so chunks generate in any order.

use bevy::prelude::*;

use super::noise::fbm2;
use super::{AIR, Blocks, CHUNK, HEIGHT, empty_blocks, index};
use crate::core::{BlockDef, HeightDef, Registry, WorldGenDef};

#[derive(Resource)]
pub struct WorldGen {
    pub seed: u32,
    height: HeightDef,
    sea: i32,
    sea_block: u16,
    /// `(max surface height, surface block, block under it)`, lowest first.
    bands: Vec<(i32, u16, u16)>,
    under_depth: i32,
    base: u16,
    floor: u16,
}

impl WorldGen {
    pub fn new(seed: u32, def: &WorldGenDef, blocks: &Registry<BlockDef>) -> Self {
        let b = |id: &str| {
            blocks
                .idx(id)
                .unwrap_or_else(|| panic!("worldgen: unknown block '{id}'"))
        };
        assert!(!def.bands.is_empty(), "worldgen: at least one band");
        Self {
            seed,
            height: def.height,
            sea: def.sea,
            sea_block: b(&def.sea_block),
            bands: def
                .bands
                .iter()
                .map(|band| (band.max, b(&band.surface), b(&band.under)))
                .collect(),
            under_depth: def.under_depth,
            base: b(&def.base),
            floor: b(&def.floor),
        }
    }

    /// Height of the top ground block of a column, before flooding.
    pub fn surface(&self, x: i32, y: i32) -> i32 {
        let h = &self.height;
        let n = fbm2(self.seed, x as f64 / h.scale, y as f64 / h.scale, h.octaves);
        // Fractal noise bunches around the middle; stretch it so lowlands and peaks exist.
        let n = ((n - 0.5) * 1.8 + 0.5).clamp(0.0, 1.0);
        (h.low + ((h.high - h.low) as f32 * n).round() as i32).clamp(1, HEIGHT - 2)
    }

    fn column_block(&self, surface: i32, z: i32) -> u16 {
        if z == 0 {
            return self.floor;
        }
        if z > surface {
            return if z <= self.sea { self.sea_block } else { AIR };
        }
        let (_, top, under) = *self
            .bands
            .iter()
            .find(|band| surface <= band.0)
            .unwrap_or_else(|| self.bands.last().expect("bands"));
        if z == surface {
            top
        } else if z > surface - self.under_depth {
            under
        } else {
            self.base
        }
    }

    /// The generated block at any position.
    pub fn block(&self, p: IVec3) -> u16 {
        if !(0..HEIGHT).contains(&p.z) {
            return AIR;
        }
        self.column_block(self.surface(p.x, p.y), p.z)
    }

    pub fn chunk(&self, pos: IVec2) -> Blocks {
        let mut blocks = empty_blocks();
        let origin = pos * CHUNK;
        for y in 0..CHUNK {
            for x in 0..CHUNK {
                let (wx, wy) = (origin.x + x, origin.y + y);
                let surface = self.surface(wx, wy);
                for z in 0..HEIGHT {
                    blocks[index(IVec3::new(x, y, z))] = self.column_block(surface, z);
                }
            }
        }
        blocks
    }

    /// Where a new player stands: the nearest dry column to the origin with level ground
    /// around it.
    pub fn spawn(&self) -> Vec3 {
        let dry = |x: i32, y: i32| self.surface(x, y) > self.sea;
        for r in 0..200i32 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) != r || !dry(dx, dy) {
                        continue;
                    }
                    let h = self.surface(dx, dy);
                    let level = (-2..=2).all(|oy| {
                        (-2..=2).all(|ox| {
                            dry(dx + ox, dy + oy) && (self.surface(dx + ox, dy + oy) - h).abs() <= 1
                        })
                    });
                    if level {
                        return Vec3::new(dx as f32 + 0.5, dy as f32 + 0.5, h as f32 + 1.0);
                    }
                }
            }
        }
        Vec3::new(0.5, 0.5, self.surface(0, 0).max(self.sea) as f32 + 1.0)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::core::Def;

    /// Reads one base-pack file into a registry, with air first for blocks.
    pub fn load<T: Def>() -> Registry<T> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/packs/base/");
        let text = std::fs::read_to_string(format!("{path}{}", T::FILE)).unwrap();
        let mut reg = Registry::<T>::default();
        for d in ron::from_str::<Vec<T>>(&text).unwrap() {
            reg.insert(d);
        }
        reg
    }

    pub fn base_blocks() -> Registry<BlockDef> {
        let mut reg = Registry::<BlockDef>::default();
        reg.insert(BlockDef::air());
        for (_, b) in load::<BlockDef>().iter() {
            reg.insert(b.clone());
        }
        reg
    }

    pub fn base_gen(seed: u32) -> (WorldGen, Registry<BlockDef>) {
        let blocks = base_blocks();
        let def = load::<WorldGenDef>().iter().last().unwrap().1.clone();
        (WorldGen::new(seed, &def, &blocks), blocks)
    }

    #[test]
    fn same_seed_same_world() {
        let (a, _) = base_gen(42);
        let (b, _) = base_gen(42);
        let (c, _) = base_gen(43);
        let pos = IVec2::new(-3, 5);
        assert_eq!(a.chunk(pos)[..], b.chunk(pos)[..]);
        assert_ne!(a.chunk(pos)[..], c.chunk(pos)[..]);
        // The per-block query agrees with whole-chunk generation.
        let chunk = a.chunk(pos);
        for p in [
            IVec3::new(-48, 80, 0),
            IVec3::new(-33, 95, 12),
            IVec3::new(-40, 88, 9),
        ] {
            assert_eq!(a.block(p), chunk[index(p)]);
        }
    }

    #[test]
    fn columns_are_floor_then_ground_then_sea_or_air() {
        let (wg, blocks) = base_gen(42);
        let water = blocks.idx("water").unwrap();
        let bedrock = blocks.idx("bedrock").unwrap();
        let (mut wet, mut dry) = (0, 0);
        for y in -100..100 {
            for x in -100..100 {
                let h = wg.surface(x, y);
                assert_eq!(wg.block(IVec3::new(x, y, 0)), bedrock);
                for z in 1..=h {
                    let b = wg.block(IVec3::new(x, y, z));
                    assert!(b != AIR && b != water, "hole in the ground at {x},{y},{z}");
                }
                for z in h + 1..HEIGHT {
                    let b = wg.block(IVec3::new(x, y, z));
                    assert_eq!(b, if z <= wg.sea { water } else { AIR });
                }
                if h < wg.sea {
                    wet += 1;
                } else {
                    dry += 1;
                }
            }
        }
        assert!(wet > 400 && dry > 4000, "wet {wet}, dry {dry}");
    }

    #[test]
    fn spawn_is_on_dry_level_ground() {
        for seed in [1, 7, 42, 1000] {
            let (wg, _) = base_gen(seed);
            let s = wg.spawn();
            let (x, y) = (s.x.floor() as i32, s.y.floor() as i32);
            assert_eq!(s.z as i32, wg.surface(x, y) + 1);
            assert!(wg.surface(x, y) > wg.sea);
        }
    }
}
