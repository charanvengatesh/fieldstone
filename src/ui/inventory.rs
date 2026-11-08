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
                    move |_: On<PointerClick>,
                          open: Res<OpenContainer>,
                          map: Res<ChunkMap>,
                          objects: Res<Registry<ObjectDef>>,
                          mut containers: ResMut<Containers>,
                          mut inv: Single<&mut Inventory, With<Player>>| {
                        if super::container::push_to_open(
                            i,
                            &open,
                            &map,
                            &objects,
                            &mut containers,
                            &mut inv,
                        ) {
                            return;
                        }
                        let sel = inv.selected;
                        inv.slots.swap(i, sel);
                    },
                );
            }
        });
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
