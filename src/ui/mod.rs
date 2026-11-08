pub mod craft_menu;
pub mod hud;
pub mod inventory;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::art::{ShapeAssets, swatch};
use crate::core::{ItemDef, Registry, ShapeDef};
use crate::items::inventory::Stack;

pub const SLOT: f32 = 44.0;
pub const PANEL_BG: Color = Color::srgba(0.05, 0.08, 0.05, 0.85);
pub const SLOT_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.35);
pub const BORDER: Color = Color::srgba(1.0, 1.0, 1.0, 0.25);
pub const SELECTED: Color = Color::srgb(1.0, 0.9, 0.4);

/// A slot box showing one inventory index: a swatch child and a count child.
#[derive(Component)]
pub struct SlotUi(pub usize);
#[derive(Component)]
pub struct Swatch;
#[derive(Component)]
pub struct Count;

pub fn slot_node() -> Node {
    Node {
        width: Val::Px(SLOT),
        height: Val::Px(SLOT),
        border: UiRect::all(Val::Px(2.0)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

pub fn spawn_slot(parent: &mut ChildSpawnerCommands, index: usize) -> Entity {
    parent
        .spawn((
            SlotUi(index),
            slot_node(),
            BackgroundColor(SLOT_BG),
            BorderColor::all(BORDER),
        ))
        .with_children(|s| {
            s.spawn((
                Swatch,
                Node {
                    width: Val::Px(SLOT * 0.55),
                    height: Val::Px(SLOT * 0.55),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ));
            s.spawn((
                Count,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(3.0),
                    bottom: Val::Px(1.0),
                    ..default()
                },
            ));
        })
        .id()
}

/// Everything needed to paint slot boxes. Shared by the hotbar and the inventory panel.
#[derive(SystemParam)]
pub struct SlotPainter<'w, 's> {
    items: Res<'w, Registry<ItemDef>>,
    shapes: Res<'w, Registry<ShapeDef>>,
    assets: Res<'w, ShapeAssets>,
    boxes: Query<'w, 's, (&'static SlotUi, &'static mut BorderColor, &'static Children)>,
    swatches: Query<'w, 's, &'static mut BackgroundColor, With<Swatch>>,
    counts: Query<'w, 's, &'static mut Text, With<Count>>,
}

impl SlotPainter<'_, '_> {
    /// Paints the slot boxes among `children` from `slots`, highlighting `selected` if given.
    pub fn paint(&mut self, children: &Children, slots: &[Option<Stack>], selected: Option<usize>) {
        for child in children {
            let Ok((slot, mut border, kids)) = self.boxes.get_mut(*child) else {
                continue;
            };
            let stack = slots.get(slot.0).copied().flatten();
            *border = BorderColor::all(if Some(slot.0) == selected {
                SELECTED
            } else {
                BORDER
            });
            for kid in kids {
                if let Ok(mut bg) = self.swatches.get_mut(*kid) {
                    bg.0 = stack.map_or(Color::NONE, |s| {
                        swatch(&self.assets, &self.shapes, &self.items.at(s.item).shape)
                    });
                }
                if let Ok(mut text) = self.counts.get_mut(*kid) {
                    text.0 = stack
                        .filter(|s| s.n > 1)
                        .map_or(String::new(), |s| s.n.to_string());
                }
            }
        }
    }
}

pub fn flip(vis: &mut Visibility) {
    *vis = if *vis == Visibility::Hidden {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (hud::spawn, inventory::spawn, craft_menu::spawn))
            .add_systems(
                Update,
                (
                    hud::update,
                    hud::bars,
                    hud::debug,
                    inventory::toggle,
                    inventory::paint,
                    craft_menu::toggle,
                    craft_menu::paint,
                ),
            );
    }
}
