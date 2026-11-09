//! Chest panel. Shows while a chest is open; clicking a chest slot pulls the stack into the
//! inventory, clicking an inventory slot while a chest is open pushes it in.

use bevy::prelude::*;

use super::*;
use crate::core::ObjectDef;
use crate::items::container::{Containers, OpenContainer};
use crate::items::inventory::{HOTBAR, Inventory};
use crate::player::Player;
use crate::world::ChunkMap;

#[derive(Component)]
pub struct ContainerPanel;
#[derive(Component)]
pub struct ContainerSlot(pub usize);

/// Largest chest any object defines; the panel is built once with that many boxes.
pub const MAX_SLOTS: usize = 30;

pub fn spawn(mut commands: Commands) {
    let width = (SLOT + 6.0) * HOTBAR as f32 + 10.0;
    commands
        .spawn((
            ContainerPanel,
            panel(Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0 + SLOT + 44.0 + 3.0 * (SLOT + 6.0) + 24.0),
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
            for i in 0..MAX_SLOTS {
                let e = spawn_slot_from(p, i, SlotSource::Chest);
                p.commands().entity(e).insert(ContainerSlot(i)).observe(
                    move |click: On<PointerClick>,
                          input: Res<ButtonInput<KeyCode>>,
                          open: Res<OpenContainer>,
                          items: Res<Registry<ItemDef>>,
                          mut containers: ResMut<Containers>,
                          mut inv: Single<&mut Inventory, With<Player>>| {
                        let Some(cell) = open.0 else { return };
                        let Some(slots) = containers.0.get_mut(&cell) else {
                            return;
                        };
                        let amount = super::inventory::click_amount(
                            &click,
                            &input,
                            slots.get(i).copied().flatten(),
                        );
                        crate::items::inventory::transfer(slots, i, &mut inv.slots, amount, &items);
                    },
                );
            }
        });
}

#[derive(Component)]
pub struct SortButton;

pub fn spawn_sort(mut commands: Commands) {
    let width = (SLOT + 6.0) * HOTBAR as f32 + 10.0;
    commands
        .spawn((
            SortButton,
            panel(Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(
                    12.0 + SLOT + 44.0 + 3.0 * (SLOT + 6.0) + 24.0 + 2.0 * (SLOT + 6.0) + 18.0,
                ),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(width / 2.0 - 70.0)),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                ..default()
            }),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("Sort chest"),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
            ));
        })
        .observe(
            |_: On<PointerClick>,
             open: Res<OpenContainer>,
             items: Res<Registry<ItemDef>>,
             mut containers: ResMut<Containers>| {
                if let Some(cell) = open.0
                    && let Some(slots) = containers.0.get_mut(&cell)
                {
                    crate::items::inventory::sort_slots(slots, &items);
                }
            },
        );
}

pub fn paint(
    open: Res<OpenContainer>,
    map: Res<ChunkMap>,
    objects: Res<Registry<ObjectDef>>,
    mut containers: ResMut<Containers>,
    mut panel: Single<(&mut Visibility, &Children), With<ContainerPanel>>,
    mut boxes: Query<(&ContainerSlot, &mut Node)>,
    mut inventory_vis: Single<
        &mut Visibility,
        (
            With<super::inventory::InventoryPanel>,
            Without<ContainerPanel>,
            Without<SortButton>,
        ),
    >,
    mut sort_vis: Single<
        &mut Visibility,
        (
            With<SortButton>,
            Without<ContainerPanel>,
            Without<super::inventory::InventoryPanel>,
        ),
    >,
    mut painter: SlotPainter,
) {
    let (vis, children) = &mut *panel;
    let Some(cell) = open.0 else {
        **vis = Visibility::Hidden;
        **sort_vis = Visibility::Hidden;
        return;
    };
    let size = map
        .tile(cell)
        .and_then(|t| t.object)
        .map_or(0, |o| objects.at(o).storage)
        .min(MAX_SLOTS as u32);
    if size == 0 {
        **vis = Visibility::Hidden;
        **sort_vis = Visibility::Hidden;
        return;
    }
    **vis = Visibility::Visible;
    **sort_vis = Visibility::Visible;
    **inventory_vis = Visibility::Visible;
    for (slot, mut node) in &mut boxes {
        node.display = if slot.0 < size as usize {
            Display::Flex
        } else {
            Display::None
        };
    }
    let slots = containers.slots_mut(cell, size).clone();
    painter.paint(children, &slots, None);
}
