//! Structure stamps from `structures.ron`: a character grid resolved against the registries
//! once at startup, then queried per cell by the generator.

use bevy::prelude::*;

use crate::core::{ObjectDef, Registry, Stamp, StructureDef, TerrainDef};

/// One resolved cell of a stamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Object(u16),
    Terrain(u16),
    Clear,
}

#[derive(Clone, Debug)]
pub struct Structure {
    pub size: IVec2,
    /// Row-major, bottom row first, `None` where the stamp leaves the world alone.
    cells: Vec<Option<Cell>>,
}

impl Structure {
    /// Resolves ids and checks the grid. Row 0 of the def is the north edge.
    pub fn parse(
        def: &StructureDef,
        terrain: &Registry<TerrainDef>,
        objects: &Registry<ObjectDef>,
    ) -> Result<Structure, String> {
        let h = def.rows.len();
        let w = def.rows.first().map_or(0, |r| r.chars().count());
        if w == 0 || h == 0 {
            return Err(format!("structure '{}' is empty", def.id));
        }
        let mut cells = Vec::with_capacity(w * h);
        for row in def.rows.iter().rev() {
            if row.chars().count() != w {
                return Err(format!("structure '{}': rows differ in width", def.id));
            }
            for ch in row.chars() {
                if ch == ' ' {
                    cells.push(None);
                    continue;
                }
                let stamp = def.legend.get(&ch).ok_or_else(|| {
                    format!("structure '{}': '{ch}' is not in the legend", def.id)
                })?;
                cells.push(Some(match stamp {
                    Stamp::Object(id) => {
                        Cell::Object(objects.idx(id).ok_or_else(|| {
                            format!("structure '{}': unknown object '{id}'", def.id)
                        })?)
                    }
                    Stamp::Terrain(id) => Cell::Terrain(terrain.idx(id).ok_or_else(|| {
                        format!("structure '{}': unknown terrain '{id}'", def.id)
                    })?),
                    Stamp::Clear => Cell::Clear,
                }));
            }
        }
        Ok(Structure {
            size: IVec2::new(w as i32, h as i32),
            cells,
        })
    }

    /// The stamp at a footprint offset from the bottom-left anchor.
    pub fn at(&self, local: IVec2) -> Option<Cell> {
        if local.x < 0 || local.y < 0 || local.x >= self.size.x || local.y >= self.size.y {
            return None;
        }
        self.cells[(local.y * self.size.x + local.x) as usize]
    }

    /// Every footprint cell relative to the anchor.
    pub fn footprint(&self) -> impl Iterator<Item = IVec2> + '_ {
        (0..self.size.y).flat_map(move |y| (0..self.size.x).map(move |x| IVec2::new(x, y)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regs() -> (Registry<TerrainDef>, Registry<ObjectDef>) {
        let mut terrain = Registry::<TerrainDef>::default();
        terrain.insert(ron::from_str(r#"(id: "grass", color: "grass", walkable: true)"#).unwrap());
        terrain.insert(ron::from_str(r#"(id: "tilled", color: "soil", walkable: true)"#).unwrap());
        let mut objects = Registry::<ObjectDef>::default();
        objects.insert(ron::from_str(r#"(id: "chest", shape: "chest", storage: 4)"#).unwrap());
        objects.insert(ron::from_str(r#"(id: "wall", shape: "wall")"#).unwrap());
        (terrain, objects)
    }

    fn def(rows: &[&str]) -> StructureDef {
        ron::from_str(&format!(
            r#"(id: "hut", rows: [{}], legend: {{ '#': Object("wall"), 'c': Object("chest"), 't': Terrain("tilled"), '.': Clear }})"#,
            rows.iter().map(|r| format!("\"{r}\"")).collect::<Vec<_>>().join(", ")
        ))
        .unwrap()
    }

    #[test]
    fn rows_read_north_to_south() {
        let (t, o) = regs();
        let s = Structure::parse(&def(&["#c#", ". t"]), &t, &o).unwrap();
        assert_eq!(s.size, IVec2::new(3, 2));
        // Bottom row is the last listed.
        assert_eq!(s.at(IVec2::new(0, 0)), Some(Cell::Clear));
        assert_eq!(s.at(IVec2::new(1, 0)), None);
        assert_eq!(
            s.at(IVec2::new(2, 0)),
            Some(Cell::Terrain(t.idx("tilled").unwrap()))
        );
        assert_eq!(
            s.at(IVec2::new(1, 1)),
            Some(Cell::Object(o.idx("chest").unwrap()))
        );
        assert_eq!(s.at(IVec2::new(3, 1)), None);
        assert_eq!(s.footprint().count(), 6);
    }

    #[test]
    fn bad_grids_are_rejected() {
        let (t, o) = regs();
        assert!(Structure::parse(&def(&["##", "#"]), &t, &o).is_err());
        assert!(Structure::parse(&def(&["#x"]), &t, &o).is_err());
        assert!(Structure::parse(&def(&[]), &t, &o).is_err());
        let mut d = def(&["c"]);
        d.legend.insert('c', Stamp::Object("ghost".into()));
        assert!(Structure::parse(&d, &t, &o).is_err());
    }
}
