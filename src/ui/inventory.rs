//! Full inventory panel (E). Clicking a slot swaps it with the selected hotbar slot.

use bevy::prelude::*;

use super::*;
use crate::core::ObjectDef;
use crate::core::settings::Keys;
use crate::items::container::{Containers, OpenContainer};
use crate::items::inventory::{HOTBAR, Inventory, SLOTS};
use crate::player::Player;
use crate::world::ChunkMap;

#[derive(Component)]
pub struct InventoryPanel;

pub fn spawn(mut commands: Commands) {
    let width = (SLOT + 6.0) * HOTBAR as f32 + 10.0;
    commands
        .spawn((
            InventoryPanel,
            panel(Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0 + SLOT + 44.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-width / 2.0)),
                padding: UiRect::all(Val::Px(5.0)),
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::px(HOTBAR as u16, SLOT),
                row_gap: Val::Px(6.0),
                column_gap: Val::Px(6.0),
                ..default()
            }),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            for i in HOTBAR..SLOTS {
                let e = spawn_slot(p, i);
                p.commands().entity(e).observe(
                    move |click: On<PointerClick>,
                          input: Res<ButtonInput<KeyCode>>,
                          open: Res<OpenContainer>,
                          map: Res<ChunkMap>,
                          objects: Res<Registry<ObjectDef>>,
                          items: Res<Registry<ItemDef>>,
                          mut containers: ResMut<Containers>,
                          mut inv: Single<&mut Inventory, With<Player>>| {
                        let amount = click_amount(&click, &input, inv.slots[i]);
                        // With a chest open, clicks push into it.
                        if let Some(cell) = open.0
                            && let Some(size) = map
                                .tile(cell)
                                .and_then(|t| t.object)
                                .map(|o| objects.at(o).storage)
                                .filter(|s| *s > 0)
                        {
                            let chest = containers.slots_mut(cell, size);
                            crate::items::inventory::transfer(
                                &mut inv.slots,
                                i,
                                chest,
                                amount,
                                &items,
                            );
                            return;
                        }
                        let sel = inv.selected;
                        if amount == u32::MAX {
                            inv.slots.swap(i, sel);
                        } else {
                            // Split part of the stack into the first empty slot.
                            let mut slots = std::mem::take(&mut inv.slots);
                            let mut dst = vec![None; slots.len()];
                            // Only empty slots receive the split so nothing merges back.
                            for (k, s) in slots.iter().enumerate() {
                                if s.is_some() {
                                    dst[k] = Some(crate::items::inventory::Stack {
                                        item: u16::MAX,
                                        n: u32::MAX,
                                    });
                                }
                            }
                            let moved = crate::items::inventory::transfer(
                                &mut slots, i, &mut dst, amount, &items,
                            );
                            if moved > 0 {
                                for (k, d) in dst.into_iter().enumerate() {
                                    if let Some(st) = d
                                        && st.item != u16::MAX
                                    {
                                        slots[k] = Some(st);
                                    }
                                }
                            }
                            inv.slots = slots;
                        }
                    },
                );
            }
        });
}

/// Whole stack on a plain click, half with shift, one with the secondary button.
pub fn click_amount(
    click: &On<PointerClick>,
    input: &ButtonInput<KeyCode>,
    stack: Option<crate::items::inventory::Stack>,
) -> u32 {
    let n = stack.map_or(0, |s| s.n);
    if click.button == bevy::picking::pointer::PointerButton::Secondary {
        1
    } else if input.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        n.div_ceil(2).max(1)
    } else {
        u32::MAX
    }
}

pub fn toggle(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    mut open: ResMut<OpenContainer>,
    mut vis: Single<&mut Visibility, With<InventoryPanel>>,
) {
    if keys.just(&input, &keys.inventory) {
        flip(&mut vis);
        if **vis == Visibility::Hidden {
            open.0 = None;
        }
    }
}

pub fn paint(
    inv: Single<&Inventory, With<Player>>,
    panel: Single<(&Visibility, &Children), With<InventoryPanel>>,
    mut painter: SlotPainter,
) {
    let (vis, children) = *panel;
    if *vis != Visibility::Hidden {
        painter.paint(children, &inv.slots, None);
    }
}
