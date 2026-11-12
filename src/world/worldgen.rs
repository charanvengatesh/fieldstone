//! Terrain from `worldgen.ron` and `features.ron`: a noise height field decides how tall
//! each column is, the band it falls in decides what it is made of, low columns are
//! flooded, then features swap ore into the ground and stand stamps on the surface. Every
//! block is a pure function of the seed and its position, so chunks generate in any order.

use bevy::prelude::*;

use super::noise::{fbm2, value3};
use super::stamp::Stamp;
use std::sync::{Arc, Mutex};

use bevy::platform::collections::HashMap;

use super::{AIR, Blocks, CHUNK, HEIGHT, empty_blocks, index};
use crate::core::{BlockDef, FeatureDef, HeightDef, Registry, Rule, StampDef, WorldGenDef, hash01};

#[derive(Resource, Clone)]
pub struct WorldGen {
    pub seed: u32,
    /// Columns answered one block at a time (`block`, for cells outside loaded chunks)
    /// and surface heights, remembered: shared by every clone, so the mesh tasks and the
    /// main thread fill one cache. Cleared when it grows past `CACHE_CAP`.
    cache: Arc<Mutex<Cache>>,
    height: HeightDef,
    sea: i32,
    sea_block: u16,
    /// `(max surface height, surface block, block under it)`, lowest first.
    bands: Vec<(i32, u16, u16)>,
    under_depth: i32,
    base: u16,
    floor: u16,
    veins: Vec<Vein>,
    scatters: Vec<Scatter>,
    spawn_on: Vec<u16>,
}

/// Entries either cache may hold before it starts over.
const CACHE_CAP: usize = 65_536;

#[derive(Default)]
struct Cache {
    heights: HashMap<IVec2, i32>,
    columns: HashMap<IVec2, Arc<[u16; HEIGHT as usize]>>,
}

/// A `Replace` rule, resolved.
#[derive(Clone)]
struct Vein {
    block: u16,
    with: u16,
    density: f32,
    noise: Option<(f64, f32)>,
    z: (i32, i32),
    salt: u32,
}

/// A `Surface` rule, resolved.
#[derive(Clone)]
struct Scatter {
    stamp: Stamp,
    on: Vec<u16>,
    density: f32,
    noise: Option<(f64, f32)>,
    salt: u32,
}

impl WorldGen {
    pub fn new(
        seed: u32,
        def: &WorldGenDef,
        blocks: &Registry<BlockDef>,
        features: &Registry<FeatureDef>,
        stamps: &Registry<StampDef>,
    ) -> Self {
        let b = |id: &str| {
            blocks
                .idx(id)
                .unwrap_or_else(|| panic!("worldgen: unknown block '{id}'"))
        };
        assert!(!def.bands.is_empty(), "worldgen: at least one band");
        let (mut veins, mut scatters) = (Vec::new(), Vec::new());
        for (i, f) in features.iter() {
            // Salts come from the id, so adding a mod never reshuffles existing features.
            let salt =
                f.id.bytes()
                    .fold(0x811C_9DC5u32, |h, c| {
                        (h ^ c as u32).wrapping_mul(0x0100_0193)
                    })
                    .wrapping_add(i as u32);
            match &f.rule {
                Rule::Replace {
                    block,
                    with,
                    density,
                    noise,
                    z,
                } => veins.push(Vein {
                    block: b(block),
                    with: b(with),
                    density: *density,
                    noise: *noise,
                    z: *z,
                    salt,
                }),
                Rule::Surface {
                    stamp,
                    on,
                    density,
                    noise,
                } => {
                    let def = stamps
                        .get(stamp)
                        .unwrap_or_else(|| panic!("feature '{}': unknown stamp '{stamp}'", f.id));
                    scatters.push(Scatter {
                        stamp: Stamp::parse(def, blocks).unwrap_or_else(|e| panic!("{e}")),
                        on: on.iter().map(|id| b(id)).collect(),
                        density: *density,
                        noise: *noise,
                        salt,
                    });
                }
            }
        }
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
            veins,
            scatters,
            spawn_on: def.spawn_on.iter().map(|id| b(id)).collect(),
            cache: Arc::new(Mutex::new(Cache::default())),
        }
    }

    /// The surface height of a column, from the cache when it has been asked before.
    fn surface_cached(&self, at: IVec2) -> i32 {
        if let Some(h) = self.cache.lock().unwrap().heights.get(&at) {
            return *h;
        }
        let h = self.surface(at.x, at.y);
        let mut cache = self.cache.lock().unwrap();
        if cache.heights.len() >= CACHE_CAP {
            cache.heights.clear();
        }
        cache.heights.insert(at, h);
        h
    }

    /// Every block of one column, stamps included, as `block` would answer them.
    fn column(&self, at: IVec2) -> Arc<[u16; HEIGHT as usize]> {
        if let Some(c) = self.cache.lock().unwrap().columns.get(&at) {
            return c.clone();
        }
        let surface = self.surface_cached(at);
        let mut col = [AIR; HEIGHT as usize];
        for (z, cell) in col.iter_mut().enumerate() {
            let p = at.extend(z as i32);
            *cell = match self.ground(p, surface) {
                AIR => self.stamped(p).unwrap_or(AIR),
                block => block,
            };
        }
        let col: Arc<[u16; HEIGHT as usize]> = Arc::new(col);
        let mut cache = self.cache.lock().unwrap();
        if cache.columns.len() >= CACHE_CAP {
            cache.columns.clear();
        }
        cache.columns.insert(at, col.clone());
        col
    }

    /// Height of the top ground block of a column, before flooding.
    pub fn surface(&self, x: i32, y: i32) -> i32 {
        let h = &self.height;
        let n = fbm2(self.seed, x as f64 / h.scale, y as f64 / h.scale, h.octaves);
        // Fractal noise bunches around the middle; stretch it so lowlands and peaks exist.
        let n = ((n - 0.5) * 1.8 + 0.5).clamp(0.0, 1.0);
        (h.low + ((h.high - h.low) as f32 * n).round() as i32).clamp(1, HEIGHT - 2)
    }

    /// Plain terrain: floor, ground, sea, air.
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

    /// Terrain with ore veins swapped in.
    fn ground(&self, p: IVec3, surface: i32) -> u16 {
        let block = self.column_block(surface, p.z);
        for v in &self.veins {
            if block != v.block || p.z < v.z.0 || p.z > v.z.1 {
                continue;
            }
            if hash01(self.seed, p.x, p.y, v.salt.wrapping_add(p.z as u32 * 7919)) >= v.density {
                continue;
            }
            if let Some((scale, threshold)) = v.noise {
                let q = [p.x as f64 / scale, p.y as f64 / scale, p.z as f64 / scale];
                if value3(self.seed ^ v.salt, q) <= threshold {
                    continue;
                }
            }
            return v.with;
        }
        block
    }

    /// The height a stamp of rule `s` stands on at column `at`, if one is placed there.
    fn stands(&self, s: &Scatter, at: IVec2) -> Option<i32> {
        if hash01(self.seed, at.x, at.y, s.salt) >= s.density {
            return None;
        }
        if let Some((scale, threshold)) = s.noise {
            let q = [at.x as f64 / scale, at.y as f64 / scale, 0.5];
            if value3(self.seed ^ s.salt, q) <= threshold {
                return None;
            }
        }
        let surface = self.surface_cached(at);
        // Dry land only, and only on the surfaces the rule names.
        (surface >= self.sea && s.on.contains(&self.column_block(surface, surface)))
            .then_some(surface)
    }

    /// The stamp block at `p`, if any stamp reaches it. Rules apply in registry order and
    /// columns in ascending order, so overlapping stamps resolve the same way everywhere.
    fn stamped(&self, p: IVec3) -> Option<u16> {
        self.scatters.iter().find_map(|s| {
            let (size, c) = (s.stamp.size, s.stamp.center());
            // Columns whose stamp footprint covers `p`.
            let lo = p.truncate() + c - size.truncate() + IVec2::ONE;
            let hi = p.truncate() + c;
            (lo.y..=hi.y)
                .flat_map(|y| (lo.x..=hi.x).map(move |x| IVec2::new(x, y)))
                .find_map(|at| {
                    let surface = self.stands(s, at)?;
                    s.stamp.at((p.truncate() - at).extend(p.z - surface - 1))
                })
        })
    }

    /// The generated block at any position. Whole columns are worked out and remembered,
    /// since a caller that asks for one cell (the mesher at the edge of the loaded world)
    /// asks for its neighbours next.
    pub fn block(&self, p: IVec3) -> u16 {
        if !(0..HEIGHT).contains(&p.z) {
            return AIR;
        }
        self.column(p.truncate())[p.z as usize]
    }

    /// The generated block at any position, worked out from scratch: what `block` must
    /// agree with.
    #[cfg(test)]
    pub fn block_uncached(&self, p: IVec3) -> u16 {
        if !(0..HEIGHT).contains(&p.z) {
            return AIR;
        }
        match self.ground(p, self.surface(p.x, p.y)) {
            AIR => self.stamped(p).unwrap_or(AIR),
            block => block,
        }
    }

    pub fn chunk(&self, pos: IVec2) -> Blocks {
        let mut blocks = empty_blocks();
        let origin = pos * CHUNK;
        for y in 0..CHUNK {
            for x in 0..CHUNK {
                let (wx, wy) = (origin.x + x, origin.y + y);
                let surface = self.surface_cached(IVec2::new(wx, wy));
                for z in 0..HEIGHT {
                    blocks[index(IVec3::new(x, y, z))] =
                        self.ground(IVec3::new(wx, wy, z), surface);
                }
            }
        }
        // Stamps, including ones standing in neighbouring chunks that reach into this one.
        // A cell keeps the first stamp to claim it, as `stamped` does.
        let mut claimed = vec![false; blocks.len()];
        for s in &self.scatters {
            let (size, c) = (s.stamp.size, s.stamp.center());
            let lo = origin + c - size.truncate() + IVec2::ONE;
            let hi = origin + IVec2::splat(CHUNK - 1) + c;
            for ay in lo.y..=hi.y {
                for ax in lo.x..=hi.x {
                    let at = IVec2::new(ax, ay);
                    let Some(surface) = self.stands(s, at) else {
                        continue;
                    };
                    for dz in 0..size.z {
                        for dy in -c.y..size.y - c.y {
                            for dx in -c.x..size.x - c.x {
                                let p = IVec3::new(ax + dx, ay + dy, surface + 1 + dz);
                                let inside = p.z < HEIGHT
                                    && (p.truncate() - origin).cmpge(IVec2::ZERO).all()
                                    && (p.truncate() - origin).cmplt(IVec2::splat(CHUNK)).all();
                                if !inside {
                                    continue;
                                }
                                let i = index(p);
                                if let Some(block) = s.stamp.at(IVec3::new(dx, dy, dz))
                                    && blocks[i] == AIR
                                    && !claimed[i]
                                {
                                    blocks[i] = block;
                                    claimed[i] = true;
                                }
                            }
                        }
                    }
                }
            }
        }
        blocks
    }

    /// Where a new player stands: the nearest dry column to the origin with level, clear
    /// ground around it.
    pub fn spawn(&self) -> Vec3 {
        self.spawn_where(|x, y| {
            let h = self.surface(x, y);
            self.spawn_on.is_empty() || self.spawn_on.contains(&self.column_block(h, h))
        })
        .or_else(|| self.spawn_where(|_, _| true))
        .unwrap_or_else(|| Vec3::new(0.5, 0.5, self.surface(0, 0).max(self.sea) as f32 + 1.0))
    }

    fn spawn_where(&self, wanted: impl Fn(i32, i32) -> bool) -> Option<Vec3> {
        let dry = |x: i32, y: i32| self.surface(x, y) > self.sea;
        for r in 0..200i32 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) != r || !dry(dx, dy) || !wanted(dx, dy) {
                        continue;
                    }
                    let h = self.surface(dx, dy);
                    let level = (-2..=2).all(|oy| {
                        (-2..=2).all(|ox| {
                            let (x, y) = (dx + ox, dy + oy);
                            dry(x, y)
                                && (self.surface(x, y) - h).abs() <= 1
                                // No tree or boulder in the way.
                                && (1..=3).all(|up| {
                                    self.block(IVec3::new(x, y, self.surface(x, y) + up)) == AIR
                                })
                        })
                    });
                    if level {
                        return Some(Vec3::new(dx as f32 + 0.5, dy as f32 + 0.5, h as f32 + 1.0));
                    }
                }
            }
        }
        None
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
        let (features, stamps) = (load::<FeatureDef>(), load::<StampDef>());
        (
            WorldGen::new(seed, &def, &blocks, &features, &stamps),
            blocks,
        )
    }

    #[test]
    fn same_seed_same_world() {
        let (a, _) = base_gen(42);
        let (b, _) = base_gen(42);
        let (c, _) = base_gen(43);
        let pos = IVec2::new(-3, 5);
        assert_eq!(a.chunk(pos)[..], b.chunk(pos)[..]);
        assert_ne!(a.chunk(pos)[..], c.chunk(pos)[..]);
    }

    #[test]
    fn cached_columns_agree_with_fresh_blocks() {
        let (wg, _) = base_gen(42);
        for (x, y) in [(0, 0), (17, -3), (-40, 25), (3, 3)] {
            for z in 0..HEIGHT {
                let p = IVec3::new(x, y, z);
                assert_eq!(wg.block(p), wg.block_uncached(p), "{p}");
                // Twice: the second answer comes from the cache.
                assert_eq!(wg.block(p), wg.block_uncached(p), "{p}");
            }
        }
    }

    #[test]
    fn single_blocks_agree_with_whole_chunks() {
        // Covers stamps that straddle chunk borders: both paths must place the same blocks.
        let (wg, blocks) = base_gen(42);
        let log = blocks.idx("log").unwrap();
        let mut logs = 0;
        for cy in -3..3 {
            for cx in -3..3 {
                let pos = IVec2::new(cx, cy);
                let chunk = wg.chunk(pos);
                for z in 0..HEIGHT {
                    for y in 0..CHUNK {
                        for x in 0..CHUNK {
                            let p = (pos * CHUNK).extend(0) + IVec3::new(x, y, z);
                            assert_eq!(wg.block(p), chunk[index(p)], "at {p}");
                            logs += (chunk[index(p)] == log) as u32;
                        }
                    }
                }
            }
        }
        assert!(logs > 20, "only {logs} log blocks in 36 chunks");
    }

    #[test]
    fn ore_stays_inside_stone_and_below_its_ceiling() {
        let (wg, blocks) = base_gen(42);
        let coal = blocks.idx("coal_ore").unwrap();
        let mut found = 0;
        for y in -60..60 {
            for x in -60..60 {
                let h = wg.surface(x, y);
                for z in 0..HEIGHT {
                    if wg.block(IVec3::new(x, y, z)) == coal {
                        found += 1;
                        assert!(z <= h && (1..=20).contains(&z), "coal at {x},{y},{z}");
                    }
                }
            }
        }
        assert!(found > 50, "only {found} coal blocks");
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
                // Above the ground: sea up to sea level, and never water higher than that.
                for z in h + 1..HEIGHT {
                    let b = wg.block(IVec3::new(x, y, z));
                    assert_eq!(b == water, z <= wg.sea, "at {x},{y},{z}");
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
            // Head room: nothing generated where the player stands.
            assert_eq!(wg.block(IVec3::new(x, y, s.z as i32)), AIR);
            assert_eq!(wg.block(IVec3::new(x, y, s.z as i32 + 1)), AIR);
        }
    }
}

#[cfg(test)]
mod scout {
    use super::tests::base_gen;
    use super::*;

    /// Not a check: prints which seeds start near water, for picking snapshot seeds.
    #[test]
    #[ignore]
    fn seeds_near_water() {
        for seed in 1..60 {
            let (wg, blocks) = base_gen(seed);
            let water = blocks.idx("water").unwrap();
            let s = wg.spawn().floor().as_ivec3();
            let near = (1..14).find(|r| {
                (-*r..=*r).any(|d| {
                    [(d, -*r), (d, *r), (-*r, d), (*r, d)]
                        .iter()
                        .any(|(dx, dy)| wg.block(IVec3::new(s.x + dx, s.y + dy, wg.sea)) == water)
                })
            });
            if let Some(r) = near {
                println!("seed {seed}: water {r} blocks from spawn {s}");
            }
        }
    }
}
