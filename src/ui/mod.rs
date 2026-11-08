pub mod craft_menu;
pub mod hud;
pub mod inventory;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::art::ShapeAssets;
use crate::core::{ItemDef, Registry};
use crate::items::inventory::Stack;

pub const SLOT: f32 = 46.0;
pub const ICON: f32 = 34.0;
pub const PANEL_BG: Color = Color::srgba(0.06, 0.09, 0.07, 0.82);
pub const PANEL_BORDER: Color = Color::srgba(1.0, 1.0, 1.0, 0.14);
pub const SLOT_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.30);
pub const BORDER: Color = Color::srgba(1.0, 1.0, 1.0, 0.18);
pub const SELECTED: Color = Color::srgb(1.0, 0.88, 0.45);
pub const TEXT_DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.7);

/// A slot box showing one inventory index: an icon child and a count child.
#[derive(Component)]
pub struct SlotUi(pub usize);
#[derive(Component)]
pub struct Swatch;
#[derive(Component)]
pub struct Count;

/// Rounded translucent panel used for every HUD surface.
pub fn panel(node: Node) -> impl Bundle {
    (
        Node {
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(10.0)),
            ..node
        },
        BackgroundColor(PANEL_BG),
        BorderColor::all(PANEL_BORDER),
    )
}

pub fn spawn_slot(parent: &mut ChildSpawnerCommands, index: usize) -> Entity {
    parent
        .spawn((
            SlotUi(index),
            Node {
                width: Val::Px(SLOT),
                height: Val::Px(SLOT),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(7.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(SLOT_BG),
            BorderColor::all(BORDER),
        ))
        .with_children(|s| {
            s.spawn((
                Swatch,
                ImageNode::new(Handle::default()).with_color(Color::NONE),
                Node {
                    width: Val::Px(ICON),
                    height: Val::Px(ICON),
                    ..default()
                },
            ));
            s.spawn((
                Count,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(Color::WHITE),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(4.0),
                    bottom: Val::Px(2.0),
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
    assets: Res<'w, ShapeAssets>,
    boxes: Query<'w, 's, (&'static SlotUi, &'static mut BorderColor, &'static Children)>,
    swatches: Query<'w, 's, &'static mut ImageNode, With<Swatch>>,
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
                if let Ok(mut img) = self.swatches.get_mut(*kid) {
                    match stack.and_then(|s| self.assets.icons.get(&self.items.at(s.item).shape)) {
                        Some(h) => {
                            img.image = h.clone();
                            img.color = Color::WHITE;
                        }
                        None => img.color = Color::NONE,
                    }
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
                    hud::status,
                    hud::title,
                    hud::debug,
                    inventory::toggle,
                    inventory::paint,
                    craft_menu::toggle,
                    craft_menu::paint,
                ),
            );
    }
}
