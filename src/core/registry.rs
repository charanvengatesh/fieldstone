//! Id-keyed content tables. Indices are stable for the life of the process, so hot paths
//! (tiles) store `u16` indices while data files and saves use string ids.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::defs::Def;

#[derive(Resource)]
pub struct Registry<T> {
    items: Vec<T>,
    index: HashMap<String, u16>,
}

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            index: HashMap::default(),
        }
    }
}

impl<T: Def> Registry<T> {
    /// Inserts or replaces by id. Replacing keeps the original index so references stay valid.
    pub fn insert(&mut self, def: T) -> u16 {
        if let Some(&i) = self.index.get(def.id()) {
            self.items[i as usize] = def;
            return i;
        }
        let i = u16::try_from(self.items.len()).expect("more than 65535 defs of one kind");
        self.index.insert(def.id().to_owned(), i);
        self.items.push(def);
        i
    }

    pub fn idx(&self, id: &str) -> Option<u16> {
        self.index.get(id).copied()
    }

    pub fn get(&self, id: &str) -> Option<&T> {
        self.idx(id).map(|i| &self.items[i as usize])
    }

    pub fn at(&self, i: u16) -> &T {
        &self.items[i as usize]
    }

    pub fn id_of(&self, i: u16) -> &str {
        self.items[i as usize].id()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u16, &T)> {
        self.items.iter().enumerate().map(|(i, t)| (i as u16, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::defs::ColorDef;

    #[test]
    fn override_keeps_index() {
        let mut r = Registry::<ColorDef>::default();
        let a = r.insert(ColorDef {
            id: "a".into(),
            rgb: [1, 1, 1],
            a: 255,
        });
        r.insert(ColorDef {
            id: "b".into(),
            rgb: [2, 2, 2],
            a: 255,
        });
        let a2 = r.insert(ColorDef {
            id: "a".into(),
            rgb: [9, 9, 9],
            a: 255,
        });
        assert_eq!(a, a2);
        assert_eq!(r.len(), 2);
        assert_eq!(r.get("a").unwrap().rgb, [9, 9, 9]);
        assert_eq!(r.id_of(1), "b");
    }
}
