//! 3D stamps from `stamps.ron`, resolved against the block registry once at startup.

use bevy::prelude::*;

use crate::core::{BlockDef, Registry, StampDef};

#[derive(Clone, Debug)]
pub struct Stamp {
    /// Width (x), depth (y) and height (z) in blocks.
    pub size: IVec3,
    /// Row-major per layer, lowest y first, bottom layer first; `None` leaves the world alone.
    cells: Vec<Option<u16>>,
}

impl Stamp {
    pub fn parse(def: &StampDef, blocks: &Registry<BlockDef>) -> Result<Stamp, String> {
        let err = |why: String| format!("stamp '{}': {why}", def.id);
        let h = def.layers.len();
        let d = def.layers.first().map_or(0, |l| l.len());
        let w = def
            .layers
            .first()
            .and_then(|l| l.first())
            .map_or(0, |r| r.chars().count());
        if w == 0 || d == 0 || h == 0 {
            return Err(err("is empty".into()));
        }
        let mut cells = Vec::with_capacity(w * d * h);
        for layer in &def.layers {
            if layer.len() != d {
                return Err(err("layers differ in depth".into()));
            }
            // The first row written is the highest y.
            for row in layer.iter().rev() {
                if row.chars().count() != w {
                    return Err(err("rows differ in width".into()));
                }
                for ch in row.chars() {
                    if ch == ' ' {
                        cells.push(None);
                        continue;
                    }
                    let id = def
                        .legend
                        .get(&ch)
                        .ok_or_else(|| err(format!("'{ch}' is not in the legend")))?;
                    let block = blocks
                        .idx(id)
                        .ok_or_else(|| err(format!("unknown block '{id}'")))?;
                    cells.push(Some(block));
                }
            }
        }
        Ok(Stamp {
            size: IVec3::new(w as i32, d as i32, h as i32),
            cells,
        })
    }

    /// The column of the footprint that sits on the placement cell.
    pub fn center(&self) -> IVec2 {
        self.size.truncate() / 2
    }

    /// The block at `offset` from the placement cell, with z counted from the first layer.
    pub fn at(&self, offset: IVec3) -> Option<u16> {
        let l = (offset.truncate() + self.center()).extend(offset.z);
        if l.cmplt(IVec3::ZERO).any() || l.cmpge(self.size).any() {
            return None;
        }
        self.cells[((l.z * self.size.y + l.y) * self.size.x + l.x) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks() -> Registry<BlockDef> {
        let mut r = Registry::<BlockDef>::default();
        r.insert(BlockDef::air());
        r.insert(ron::from_str(r#"(id: "log", color: (1, 1, 1))"#).unwrap());
        r.insert(ron::from_str(r#"(id: "leaves", color: (1, 1, 1))"#).unwrap());
        r
    }

    fn def(layers: &str) -> StampDef {
        ron::from_str(&format!(
            r#"(id: "t", layers: {layers}, legend: {{ 'T': "log", 'l': "leaves" }})"#
        ))
        .unwrap()
    }

    #[test]
    fn layers_stack_and_rows_read_like_a_map() {
        let s = Stamp::parse(
            &def(r#"[[" T ", "   ", "   "], ["lll", "lTl", "  l"]]"#),
            &blocks(),
        )
        .unwrap();
        assert_eq!(s.size, IVec3::new(3, 3, 2));
        // Bottom layer: the trunk is in the first row written, which is the highest y.
        assert_eq!(s.at(IVec3::new(0, 1, 0)), Some(1));
        assert_eq!(s.at(IVec3::new(0, 0, 0)), None);
        // Top layer: trunk in the middle, leaves around, a gap at low y on the left.
        assert_eq!(s.at(IVec3::new(0, 0, 1)), Some(1));
        assert_eq!(s.at(IVec3::new(1, -1, 1)), Some(2));
        assert_eq!(s.at(IVec3::new(-1, -1, 1)), None);
        assert_eq!(s.at(IVec3::new(-1, 1, 1)), Some(2));
        // Outside the footprint or above the top.
        assert_eq!(s.at(IVec3::new(2, 0, 0)), None);
        assert_eq!(s.at(IVec3::new(0, 0, 2)), None);
    }

    #[test]
    fn bad_stamps_are_rejected() {
        let b = blocks();
        assert!(Stamp::parse(&def("[]"), &b).is_err());
        assert!(Stamp::parse(&def(r#"[["TT", "T"]]"#), &b).is_err());
        assert!(Stamp::parse(&def(r#"[["T"], ["T", "T"]]"#), &b).is_err());
        assert!(Stamp::parse(&def(r#"[["x"]]"#), &b).is_err());
        let mut d = def(r#"[["T"]]"#);
        d.legend.insert('T', "ghost".into());
        assert!(Stamp::parse(&d, &b).is_err());
    }
}
