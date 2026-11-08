//! Full inventory panel (E). Clicking a slot swaps it with the selected hotbar slot.

use bevy::prelude::*;

use super::*;
use crate::items::inventory::{HOTBAR, Inventory, SLOTS};
use crate::player::Player;

#[derive(Component)]
pub struct InventoryPanel;

pub fn spawn(mut commands: Commands) {
    commands
        .spawn((
            InventoryPanel,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0 + SLOT + 16.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-(SLOT + 4.0) * HOTBAR as f32 / 2.0 - 8.0)),
                padding: UiRect::all(Val::Px(8.0)),
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::px(HOTBAR as u16, SLOT),
                row_gap: Val::Px(4.0),
                column_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            for i in HOTBAR..SLOTS {
                let e = spawn_slot(p, i);
                p.commands().entity(e).observe(
                    move |_: On<PointerClick>, mut inv: Single<&mut Inventory, With<Player>>| {
                        let sel = inv.selected;
                        inv.slots.swap(i, sel);
                    },
                );
            }
        });
}

pub fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    mut vis: Single<&mut Visibility, With<InventoryPanel>>,
) {
    if keys.just_pressed(KeyCode::KeyE) {
        flip(&mut vis);
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
