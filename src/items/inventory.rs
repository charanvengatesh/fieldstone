//! Slot-based inventory. The first `HOTBAR` slots are the hotbar; `selected` indexes it.

use bevy::prelude::*;

use crate::core::{ItemDef, Registry};

pub const HOTBAR: usize = 10;
pub const SLOTS: usize = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stack {
    pub item: u16,
    pub n: u32,
}

#[derive(Component, Debug)]
pub struct Inventory {
    pub slots: Vec<Option<Stack>>,
    pub selected: usize,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: vec![None; SLOTS],
            selected: 0,
        }
    }
}

impl Inventory {
    pub fn held(&self) -> Option<Stack> {
        self.slots[self.selected]
    }

    pub fn count(&self, item: u16) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|s| s.item == item)
            .map(|s| s.n)
            .sum()
    }

    /// Adds as much as fits, topping up existing stacks first. Returns what did not fit.
    pub fn add(&mut self, item: u16, mut n: u32, items: &Registry<ItemDef>) -> u32 {
        let max = items.at(item).stack_max;
        for s in self
            .slots
            .iter_mut()
            .flatten()
            .filter(|s| s.item == item && s.n < max)
        {
            let take = n.min(max - s.n);
            s.n += take;
            n -= take;
            if n == 0 {
                return 0;
            }
        }
        for slot in self.slots.iter_mut().filter(|s| s.is_none()) {
            let take = n.min(max);
            *slot = Some(Stack { item, n: take });
            n -= take;
            if n == 0 {
                return 0;
            }
        }
        n
    }

    /// Removes exactly `n` of `item`, or nothing if there are not enough.
    pub fn remove(&mut self, item: u16, mut n: u32) -> bool {
        if self.count(item) < n {
            return false;
        }
        for slot in self.slots.iter_mut().rev() {
            if let Some(s) = slot
                && s.item == item
            {
                let take = n.min(s.n);
                s.n -= take;
                n -= take;
                if s.n == 0 {
                    *slot = None;
                }
                if n == 0 {
                    break;
                }
            }
        }
        true
    }

    /// Takes one from the selected slot.
    pub fn consume_held(&mut self) {
        if let Some(s) = &mut self.slots[self.selected] {
            s.n -= 1;
            if s.n == 0 {
                self.slots[self.selected] = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ItemDef, Use};

    fn items() -> Registry<ItemDef> {
        let mut r = Registry::default();
        r.insert(ItemDef {
            id: "log".into(),
            name: "Log".into(),
            shape: "log".into(),
            stack_max: 5,
            tags: vec![],
            usage: Use::None,
        });
        r
    }

    #[test]
    fn stacks_fill_then_overflow() {
        let reg = items();
        let mut inv = Inventory {
            slots: vec![None; 2],
            selected: 0,
        };
        assert_eq!(inv.add(0, 7, &reg), 0);
        assert_eq!(
            inv.slots,
            vec![Some(Stack { item: 0, n: 5 }), Some(Stack { item: 0, n: 2 })]
        );
        assert_eq!(inv.add(0, 4, &reg), 1);
        assert_eq!(inv.count(0), 10);
    }

    #[test]
    fn remove_is_all_or_nothing() {
        let reg = items();
        let mut inv = Inventory::default();
        inv.add(0, 7, &reg);
        assert!(!inv.remove(0, 8));
        assert_eq!(inv.count(0), 7);
        assert!(inv.remove(0, 6));
        assert_eq!(inv.count(0), 1);
    }
}
