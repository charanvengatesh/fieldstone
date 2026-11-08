//! Crafting panel (C): one row per recipe with its icon, lit when craftable, click to craft.

use bevy::prelude::*;

use super::*;
use crate::core::settings::Keys;
use crate::core::{ObjectDef, RecipeDef};
use crate::items::craft::{CraftRequest, can_craft, nearby_stations};
use crate::items::inventory::Inventory;
use crate::player::Player;
use crate::world::ChunkMap;

#[derive(Component)]
pub struct CraftPanel;
#[derive(Component)]
pub struct RecipeRow(pub u16);

const ON: Color = Color::srgb(0.95, 1.0, 0.9);
const OFF: Color = Color::srgba(1.0, 1.0, 1.0, 0.32);

pub fn spawn(
    mut commands: Commands,
    recipes: Res<Registry<RecipeDef>>,
    items: Res<Registry<ItemDef>>,
    assets: Res<ShapeAssets>,
) {
    commands
        .spawn((
            CraftPanel,
            panel(Node {
                position_type: PositionType::Absolute,
                top: Val::Px(16.0),
                right: Val::Px(16.0),
                padding: UiRect::all(Val::Px(10.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                max_height: Val::Percent(85.0),
                overflow: Overflow::scroll_y(),
                ..default()
            }),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("Crafting"),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(TEXT_DIM),
                Node {
                    margin: UiRect::bottom(Val::Px(6.0)),
                    ..default()
                },
            ));
            for (i, r) in recipes.iter() {
                let name = |id: &str| items.get(id).map_or(id.to_owned(), |it| it.name.clone());
                let inputs: Vec<String> = r
                    .inputs
                    .iter()
                    .map(|(id, n)| format!("{n} {}", name(id)))
                    .collect();
                let title = if r.output.1 > 1 {
                    format!("{} ×{}", name(&r.output.0), r.output.1)
                } else {
                    name(&r.output.0)
                };
                let detail = format!(
                    "{}{}",
                    inputs.join(", "),
                    r.station
                        .as_deref()
                        .map_or(String::new(), |s| format!("   @ {s}"))
                );
                let icon = items
                    .get(&r.output.0)
                    .and_then(|it| assets.icons.get(&it.shape))
                    .cloned()
                    .unwrap_or_default();
                p.spawn((
                    RecipeRow(i),
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(8.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|row| {
                    row.spawn((
                        ImageNode::new(icon),
                        Node {
                            width: Val::Px(26.0),
                            height: Val::Px(26.0),
                            ..default()
                        },
                    ));
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|col| {
                        col.spawn((
                            Text::new(title),
                            TextFont {
                                font_size: FontSize::Px(14.0),
                                ..default()
                            },
                            TextColor(OFF),
                        ));
                        col.spawn((
                            Text::new(detail),
                            TextFont {
                                font_size: FontSize::Px(11.0),
                                ..default()
                            },
                            TextColor(OFF),
                        ));
                    });
                })
                .observe(
                    move |_: On<PointerClick>, mut w: MessageWriter<CraftRequest>| {
                        w.write(CraftRequest(i));
                    },
                );
            }
        });
}

pub fn toggle(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    mut vis: Single<&mut Visibility, With<CraftPanel>>,
) {
    if keys.just(&input, &keys.craft) {
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
    rows: Query<(&RecipeRow, &Children)>,
    mut images: Query<&mut ImageNode>,
    cols: Query<&Children, Without<RecipeRow>>,
    mut colors: Query<&mut TextColor>,
) {
    if **panel == Visibility::Hidden {
        return;
    }
    let (tf, inv) = *player;
    let stations: Vec<&str> = nearby_stations(&map, &objects, tf.translation.truncate()).collect();
    for (row, children) in &rows {
        let ok = can_craft(recipes.at(row.0), inv, &items, &stations);
        for child in children {
            if let Ok(mut img) = images.get_mut(*child) {
                img.color = if ok {
                    Color::WHITE
                } else {
                    Color::srgba(1.0, 1.0, 1.0, 0.35)
                };
            }
            if let Ok(texts) = cols.get(*child) {
                for (k, t) in texts.iter().enumerate() {
                    if let Ok(mut c) = colors.get_mut(t) {
                        c.0 = if ok {
                            if k == 0 { ON } else { TEXT_DIM }
                        } else {
                            OFF
                        };
                    }
                }
            }
        }
    }
}
