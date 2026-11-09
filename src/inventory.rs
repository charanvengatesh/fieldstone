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
    use crate::core::ItemDef;

    fn items() -> Registry<ItemDef> {
        let mut r = Registry::default();
        r.insert(ron::from_str::<ItemDef>(r#"(id: "log", stack_max: 5)"#).unwrap());
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

/// Moves up to `n` of the stack at `src[i]` into `dst`, topping up matching stacks first and
/// then filling empty slots. Returns how many moved.
pub fn transfer(
    src: &mut [Option<Stack>],
    i: usize,
    dst: &mut [Option<Stack>],
    n: u32,
    items: &Registry<ItemDef>,
) -> u32 {
    let Some(stack) = src.get(i).copied().flatten() else {
        return 0;
    };
    let mut left = n.min(stack.n);
    let want = left;
    let max = items.at(stack.item).stack_max;
    for s in dst.iter_mut().flatten() {
        if s.item == stack.item && s.n < max && left > 0 {
            let take = left.min(max - s.n);
            s.n += take;
            left -= take;
        }
    }
    for slot in dst.iter_mut() {
        if left == 0 {
            break;
        }
        if slot.is_none() {
            let take = left.min(max);
            *slot = Some(Stack {
                item: stack.item,
                n: take,
            });
            left -= take;
        }
    }
    let moved = want - left;
    let remaining = stack.n - moved;
    src[i] = (remaining > 0).then_some(Stack {
        item: stack.item,
        n: remaining,
    });
    moved
}

/// Groups by item, merges stacks up to their max, and packs them to the front.
pub fn sort_slots(slots: &mut [Option<Stack>], items: &Registry<ItemDef>) {
    let mut counts: Vec<(u16, u32)> = Vec::new();
    for s in slots.iter().flatten() {
        match counts.iter_mut().find(|(id, _)| *id == s.item) {
            Some(e) => e.1 += s.n,
            None => counts.push((s.item, s.n)),
        }
    }
    counts.sort_by_key(|(id, _)| *id);
    let mut out: Vec<Option<Stack>> = Vec::with_capacity(slots.len());
    for (item, mut n) in counts {
        let max = items.at(item).stack_max.max(1);
        while n > 0 && out.len() < slots.len() {
            let take = n.min(max);
            out.push(Some(Stack { item, n: take }));
            n -= take;
        }
    }
    out.resize(slots.len(), None);
    slots.copy_from_slice(&out);
}

#[cfg(test)]
mod transfer_tests {
    use super::*;
    use crate::core::ItemDef;

    fn reg() -> Registry<ItemDef> {
        let mut r = Registry::default();
        r.insert(ron::from_str::<ItemDef>(r#"(id: "log", stack_max: 10)"#).unwrap());
        r
    }

    #[test]
    fn half_and_one_move_the_right_amounts() {
        let items = reg();
        let mut a = vec![Some(Stack { item: 0, n: 7 }), None];
        let mut b = vec![None, None];
        assert_eq!(transfer(&mut a, 0, &mut b, 3, &items), 3);
        assert_eq!(a[0].unwrap().n, 4);
        assert_eq!(b[0].unwrap().n, 3);
        assert_eq!(transfer(&mut a, 0, &mut b, 1, &items), 1);
        assert_eq!(b[0].unwrap().n, 4);
        assert_eq!(transfer(&mut a, 0, &mut b, 99, &items), 3);
        assert!(a[0].is_none());
    }

    #[test]
    fn sort_merges_and_packs() {
        let items = reg();
        let mut slots = vec![
            None,
            Some(Stack { item: 0, n: 6 }),
            None,
            Some(Stack { item: 0, n: 6 }),
        ];
        sort_slots(&mut slots, &items);
        assert_eq!(slots[0].unwrap().n, 10);
        assert_eq!(slots[1].unwrap().n, 2);
        assert!(slots[2].is_none());
    }
}
