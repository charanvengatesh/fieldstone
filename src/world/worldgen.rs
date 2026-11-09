//! Terrain generation from `worldgen.ron`: two fractal noise fields pick a height band and a
//! moisture swap, then scatter rules sprinkle objects deterministically per cell.

use bevy::prelude::*;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use super::chunk::{CHUNK, Chunk, Tile};
use super::structure::{Cell as StampCell, Structure};
use crate::core::{
    LootDef, ObjectDef, Registry, Scatter, StructureDef, TerrainDef, WorldGenDef, hash01,
};

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
    stamps: Vec<StampRule>,
}

struct ScatterIdx {
    object: u16,
    on: Vec<u16>,
    density: f32,
    noise: Option<(f64, f64)>,
    near: Option<Vec<u16>>,
    salt: u32,
}

/// A scatter rule that stamps a structure instead of one object.
struct StampRule {
    structure: Structure,
    on: Vec<u16>,
    density: f32,
    salt: u32,
    loot: Option<u16>,
}

impl WorldGen {
    pub fn new(
        seed: u32,
        def: &WorldGenDef,
        terrain: &Registry<TerrainDef>,
        objects: &Registry<ObjectDef>,
        structures: &Registry<StructureDef>,
        loot: &Registry<LootDef>,
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
            stamps: def
                .scatter
                .iter()
                .enumerate()
                .filter_map(|(i, s): (usize, &Scatter)| {
                    let id = s.structure.as_ref()?;
                    let sdef = structures
                        .get(id)
                        .unwrap_or_else(|| panic!("worldgen: unknown structure '{id}'"));
                    let structure = Structure::parse(sdef, terrain, objects)
                        .unwrap_or_else(|e| panic!("worldgen: {e}"));
                    let loot = sdef.loot.as_ref().map(|l| {
                        loot.idx(l)
                            .unwrap_or_else(|| panic!("structure '{id}': unknown loot '{l}'"))
                    });
                    Some(StampRule {
                        structure,
                        on: s.on.iter().map(|id| t(id)).collect(),
                        density: s.density,
                        salt: i as u32 + 100,
                        loot,
                    })
                })
                .collect(),
            scatter: def
                .scatter
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    if s.structure.is_none() && s.object.is_empty() {
                        panic!("worldgen: a scatter rule names neither an object nor a structure");
                    }
                    s.structure.is_none()
                })
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
        if let Some((rule, local)) = self.stamp_at(cell) {
            match rule.structure.at(local) {
                Some(StampCell::Object(o)) => {
                    return Tile {
                        terrain,
                        object: Some(o),
                        height,
                    };
                }
                Some(StampCell::Terrain(t)) => {
                    return Tile {
                        terrain: t,
                        object: None,
                        height,
                    };
                }
                Some(StampCell::Clear) => {
                    return Tile {
                        terrain,
                        object: None,
                        height,
                    };
                }
                None => {}
            }
        }
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

    /// The structure covering a cell, if a stamp rule fires on an anchor whose footprint
    /// includes it, with the cell's offset from that anchor.
    fn stamp_at(&self, cell: IVec2) -> Option<(&StampRule, IVec2)> {
        self.stamps.iter().find_map(|rule| {
            rule.structure.footprint().find_map(|local| {
                let anchor = cell - local;
                (hash01(self.seed, anchor.x, anchor.y, rule.salt) < rule.density
                    && self.anchor_fits(rule, anchor))
                .then_some((rule, local))
            })
        })
    }

    /// A stamp needs its anchor on an allowed terrain and the whole footprint walkable, on
    /// land and at one height, so no cliff or shore cuts through it.
    fn anchor_fits(&self, rule: &StampRule, anchor: IVec2) -> bool {
        let terrain = self.terrain(anchor);
        if !rule.on.contains(&terrain) {
            return false;
        }
        let h0 = self.height_at(anchor, terrain, self.walkable[terrain as usize]);
        rule.structure.footprint().all(|local| {
            let c = anchor + local;
            let t = self.terrain(c);
            let walkable = self.walkable[t as usize];
            walkable && self.land.contains(&t) && self.height_at(c, t, walkable) == h0
        })
    }

    /// The loot table a stamped storage object on this cell starts with.
    pub fn stamp_loot(&self, cell: IVec2) -> Option<u16> {
        let (rule, local) = self.stamp_at(cell)?;
        matches!(rule.structure.at(local), Some(StampCell::Object(_)))
            .then_some(rule.loot)
            .flatten()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Def;

    fn load<T: Def>() -> Registry<T> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/packs/base/");
        let text = std::fs::read_to_string(format!("{path}{}", T::FILE)).unwrap();
        let mut reg = Registry::<T>::default();
        for d in ron::from_str::<Vec<T>>(&text).unwrap() {
            reg.insert(d);
        }
        reg
    }

    #[test]
    fn base_pack_stamps_campsites_on_flat_meadow() {
        let terrain = load::<TerrainDef>();
        let objects = load::<ObjectDef>();
        let structures = load::<StructureDef>();
        let loot = load::<LootDef>();
        let def = load::<WorldGenDef>().iter().last().unwrap().1.clone();
        let wg = WorldGen::new(42, &def, &terrain, &objects, &structures, &loot);
        let chest = objects.idx("chest").unwrap();
        let campfire = objects.idx("campfire").unwrap();
        let mut chests = 0;
        for y in -200..200 {
            for x in -200..200 {
                let c = IVec2::new(x, y);
                let t = wg.tile(c);
                if t.object == Some(chest) {
                    chests += 1;
                    assert!(
                        wg.stamp_loot(c).is_some(),
                        "stamped chest without loot at {c}"
                    );
                    // The campfire sits two cells west and on the same terrace.
                    let f = wg.tile(c - IVec2::new(2, 0));
                    assert_eq!(f.object, Some(campfire));
                    assert_eq!(f.height, t.height);
                } else if t.object.is_none() {
                    assert!(wg.stamp_loot(c).is_none(), "loot on a bare cell at {c}");
                }
            }
        }
        println!("campsites: {chests}");
        assert!(chests > 0, "no campsite in a 400x400 area");
    }
}
