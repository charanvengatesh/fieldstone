//! Crafting panel (C): one row per recipe, lit when craftable, click to craft.

use bevy::prelude::*;

use super::*;
use crate::core::{ObjectDef, RecipeDef};
use crate::items::craft::{CraftRequest, can_craft, nearby_stations};
use crate::items::inventory::Inventory;
use crate::player::Player;
use crate::world::ChunkMap;

#[derive(Component)]
pub struct CraftPanel;
#[derive(Component)]
pub struct RecipeRow(pub u16);

const ON: Color = Color::srgb(0.9, 1.0, 0.85);
const OFF: Color = Color::srgba(1.0, 1.0, 1.0, 0.35);

pub fn spawn(
    mut commands: Commands,
    recipes: Res<Registry<RecipeDef>>,
    items: Res<Registry<ItemDef>>,
) {
    commands
        .spawn((
            CraftPanel,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(40.0),
                right: Val::Px(12.0),
                padding: UiRect::all(Val::Px(8.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            for (i, r) in recipes.iter() {
                let name = |id: &str| items.get(id).map_or(id.to_owned(), |it| it.name.clone());
                let inputs: Vec<String> = r
                    .inputs
                    .iter()
                    .map(|(id, n)| format!("{n} {}", name(id)))
                    .collect();
                let label = format!(
                    "{} x{}  <  {}{}",
                    name(&r.output.0),
                    r.output.1,
                    inputs.join(", "),
                    r.station
                        .as_deref()
                        .map_or(String::new(), |s| format!("  @{s}"))
                );
                p.spawn((
                    RecipeRow(i),
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(15.0),
                        ..default()
                    },
                    TextColor(OFF),
                    Node {
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                        ..default()
                    },
                ))
                .observe(
                    move |_: On<PointerClick>, mut w: MessageWriter<CraftRequest>| {
                        w.write(CraftRequest(i));
                    },
                );
            }
        });
}

pub fn toggle(keys: Res<ButtonInput<KeyCode>>, mut vis: Single<&mut Visibility, With<CraftPanel>>) {
    if keys.just_pressed(KeyCode::KeyC) {
        flip(&mut vis);
    }
}

pub fn paint(
    panel: Single<&Visibility, With<CraftPanel>>,
    player: Single<(&Transform, &Inventory), With<Player>>,
    recipes: Res<Registry<RecipeDef>>,
    items: Res<Registry<ItemDef>>,
    objects: Res<Registry<ObjectDef>>,
    map: Res<ChunkMap>,
    mut rows: Query<(&RecipeRow, &mut TextColor)>,
) {
    if **panel == Visibility::Hidden {
        return;
    }
    let (tf, inv) = *player;
    let stations: Vec<&str> = nearby_stations(&map, &objects, tf.translation.truncate()).collect();
    for (row, mut color) in &mut rows {
        color.0 = if can_craft(recipes.at(row.0), inv, &items, &stations) {
            ON
        } else {
            OFF
        };
    }
}
