//! Terrain generation from `worldgen.ron`: two fractal noise fields pick a height band and a
//! moisture swap, then scatter rules sprinkle objects deterministically per cell.

use bevy::prelude::*;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use super::chunk::{CHUNK, Chunk, Tile};
use crate::core::{ObjectDef, Registry, Scatter, TerrainDef, WorldGenDef, hash01};

#[derive(Resource)]
pub struct WorldGen {
    pub seed: u32,
    height: Fbm<Perlin>,
    moisture: Fbm<Perlin>,
    gate: Perlin,
    relief: Fbm<Perlin>,
    relief_scale: f64,
    ramps: f32,
    ramp: Option<u16>,
    height_scale: f64,
    moisture_scale: f64,
    /// Terrains that take relief (grass, meadow, forest, rock); sand and water stay flat.
    land: Vec<u16>,
    walkable: Vec<bool>,
    bands: Vec<(f64, u16)>,
    wet: Vec<(u16, u16, f64)>,
    scatter: Vec<ScatterIdx>,
}

struct ScatterIdx {
    object: u16,
    on: Vec<u16>,
    density: f32,
    noise: Option<(f64, f64)>,
    near: Option<Vec<u16>>,
    salt: u32,
}

impl WorldGen {
    pub fn new(
        seed: u32,
        def: &WorldGenDef,
        terrain: &Registry<TerrainDef>,
        objects: &Registry<ObjectDef>,
    ) -> Self {
        let t = |id: &str| {
            terrain
                .idx(id)
                .unwrap_or_else(|| panic!("worldgen: unknown terrain '{id}'"))
        };
        let o = |id: &str| {
            objects
                .idx(id)
                .unwrap_or_else(|| panic!("worldgen: unknown object '{id}'"))
        };
        Self {
            seed,
            height: Fbm::new(seed).set_octaves(5),
            moisture: Fbm::new(seed.wrapping_add(1)).set_octaves(3),
            gate: Perlin::new(seed.wrapping_add(2)),
            relief: Fbm::new(seed.wrapping_add(3)).set_octaves(2),
            relief_scale: def.relief_scale,
            ramps: def.ramps,
            ramp: objects.idx("ramp"),
            height_scale: def.height_scale,
            moisture_scale: def.moisture_scale,
            land: ["grass", "meadow", "forest", "rock", "tilled"]
                .iter()
                .filter_map(|id| terrain.idx(id))
                .collect(),
            walkable: terrain.iter().map(|(_, t)| t.walkable).collect(),
            bands: def.bands.iter().map(|(h, id)| (*h, t(id))).collect(),
            wet: def.wet.iter().map(|(a, b, m)| (t(a), t(b), *m)).collect(),
            scatter: def
                .scatter
                .iter()
                .enumerate()
                .map(|(i, s): (usize, &Scatter)| ScatterIdx {
                    object: o(&s.object),
                    on: s.on.iter().map(|id| t(id)).collect(),
                    density: s.density,
                    noise: s.noise,
                    near: s.near.as_ref().map(|v| v.iter().map(|id| t(id)).collect()),
                    salt: i as u32 + 100,
                })
                .collect(),
        }
    }

    /// Ground only, no scatter; cheap enough to call for neighbors.
    pub fn terrain(&self, cell: IVec2) -> u16 {
        let p = [cell.x as f64, cell.y as f64];
        let h = self
            .height
            .get([p[0] / self.height_scale, p[1] / self.height_scale]);
        let m = self
            .moisture
            .get([p[0] / self.moisture_scale, p[1] / self.moisture_scale]);
        let mut terrain = self.bands.last().map(|b| b.1).unwrap_or(0);
        for &(upper, id) in &self.bands {
            if h < upper {
                terrain = id;
                break;
            }
        }
        for &(from, to, thresh) in &self.wet {
            if terrain == from && m > thresh {
                terrain = to;
            }
        }
        terrain
    }

    /// Terrace level of a cell: land only, from slow relief noise in four steps.
    pub fn height_at(&self, cell: IVec2, terrain: u16, walkable: bool) -> u8 {
        if self.relief_scale <= 0.0 || !walkable || !self.land.contains(&terrain) {
            return 0;
        }
        let p = [
            cell.x as f64 / self.relief_scale,
            cell.y as f64 / self.relief_scale,
        ];
        let v = (self.relief.get(p) * 0.5 + 0.5).clamp(0.0, 0.999);
        (v * 4.0) as u8
    }

    pub fn tile(&self, cell: IVec2) -> Tile {
        let p = [cell.x as f64, cell.y as f64];
        let terrain = self.terrain(cell);
        let walkable = self.walkable[terrain as usize];
        let height = self.height_at(cell, terrain, walkable);
        // A cliff foot (the cell to the south is lower... i.e. this cell is lower than its
        // northern neighbor) may get a natural ramp instead of scatter.
        if let Some(ramp) = self.ramp {
            let north = cell + IVec2::Y;
            let nt = self.terrain(north);
            let nh = self.height_at(north, nt, self.walkable[nt as usize]);
            if walkable && nh > height && hash01(self.seed, cell.x, cell.y, 909) < self.ramps {
                return Tile {
                    terrain,
                    object: Some(ramp),
                    height,
                };
            }
        }
        let object = self.scatter.iter().find_map(|s| {
            if !s.on.contains(&terrain) || hash01(self.seed, cell.x, cell.y, s.salt) >= s.density {
                return None;
            }
            if let Some(near) = &s.near {
                let touching = (-1..=1).any(|dy| {
                    (-1..=1).any(|dx| {
                        (dx != 0 || dy != 0)
                            && near.contains(&self.terrain(cell + IVec2::new(dx, dy)))
                    })
                });
                if !touching {
                    return None;
                }
            }
            if let Some((scale, thresh)) = s.noise
                && self.gate.get([p[0] / scale + s.salt as f64, p[1] / scale]) <= thresh
            {
                return None;
            }
            Some(s.object)
        });
        Tile {
            terrain,
            object,
            height,
        }
    }

    pub fn chunk(&self, pos: IVec2) -> Chunk {
        let mut c = Chunk::filled(Tile::default());
        let origin = pos * CHUNK;
        for y in 0..CHUNK {
            for x in 0..CHUNK {
                c.tiles[(y * CHUNK + x) as usize] = self.tile(origin + IVec2::new(x, y));
            }
        }
        c
    }
}
