//! Always-visible HUD: hotbar with the held item's name, vitals, status line, title card,
//! and the debug readout (F3).

use bevy::prelude::*;

use super::*;
use crate::core::settings::Keys;
use crate::core::{Ending, Toast};
use crate::items::inventory::{HOTBAR, Inventory};
use crate::player::{Cursor, Player};
use crate::stats::{Progress, Stats};
use crate::world::{ChunkMap, cell_of};

#[derive(Component)]
pub struct Hotbar;
#[derive(Component)]
pub struct HeldName;
#[derive(Component)]
pub struct DebugText;
#[derive(Component)]
pub struct HpBar;
#[derive(Component)]
pub struct StaminaBar;
#[derive(Component)]
pub struct FoodBar;
#[derive(Component)]
pub struct LevelText;
#[derive(Component)]
pub struct StatusText;
#[derive(Component)]
pub struct TitleCard(pub f32);
#[derive(Component)]
pub struct ToastText;
#[derive(Component)]
pub struct EndingCard;

const BAR_W: f32 = 170.0;
const HOTBAR_W: f32 = (SLOT + 6.0) * HOTBAR as f32 + 10.0;

fn bar(parent: &mut ChildSpawnerCommands, marker: impl Component, color: Color) {
    parent
        .spawn((
            Node {
                width: Val::Px(BAR_W),
                height: Val::Px(9.0),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(SLOT_BG),
        ))
        .with_children(|b| {
            b.spawn((
                marker,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    border_radius: BorderRadius::all(Val::Px(5.0)),
                    ..default()
                },
                BackgroundColor(color),
            ));
        });
}

fn small(text: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

pub fn spawn(mut commands: Commands) {
    // Hotbar panel with the held item's name above it.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            left: Val::Percent(50.0),
            margin: UiRect::left(Val::Px(-HOTBAR_W / 2.0)),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|col| {
            col.spawn((HeldName, small("", 14.0, TEXT_DIM)));
            col.spawn((
                Hotbar,
                panel(Node {
                    padding: UiRect::all(Val::Px(5.0)),
                    column_gap: Val::Px(6.0),
                    ..default()
                }),
            ))
            .with_children(|p| {
                for i in 0..HOTBAR {
                    spawn_slot(p, i);
                }
            });
        });
    // Vitals, bottom left, above the hotbar's height so narrow windows never overlap.
    commands
        .spawn(panel(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            bottom: Val::Px(12.0 + SLOT + 40.0),
            padding: UiRect::all(Val::Px(10.0)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(5.0),
            ..default()
        }))
        .with_children(|p| {
            p.spawn((LevelText, small("", 13.0, TEXT_DIM)));
            bar(p, HpBar, Color::srgb(0.88, 0.30, 0.34));
            bar(p, StaminaBar, Color::srgb(0.40, 0.78, 0.36));
            bar(p, FoodBar, Color::srgb(0.92, 0.66, 0.28));
        });
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0 + SLOT + 48.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|p| {
            p.spawn((StatusText, small("", 18.0, Color::WHITE)));
        });
    commands
        .spawn((
            TitleCard(6.0),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(28.0),
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(20.0), Val::Px(0.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(8.0),
                ..default()
            },
        ))
        .with_children(|p| {
            p.spawn(small("Fieldstone", 48.0, Color::WHITE));
            p.spawn(small(
                "WASD move   ·   click a tree or rock to work it   ·   F or right-click to use",
                14.0,
                TEXT_DIM,
            ));
            p.spawn(small(
                "E inventory   ·   C craft   ·   scroll or 1-0 hotbar   ·   pinch to zoom   ·   F5 save",
                14.0,
                TEXT_DIM,
            ));
        });
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(18.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(Val::Px(20.0), Val::Px(0.0)),
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                ToastText,
                small("", 18.0, Color::WHITE),
                Node {
                    max_width: Val::Px(620.0),
                    ..default()
                },
            ));
        });
    commands
        .spawn((
            EndingCard,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(5),
        ))
        .with_children(|root| {
            root.spawn(panel(Node {
                padding: UiRect::all(Val::Px(28.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(10.0),
                max_width: Val::Px(560.0),
                ..default()
            }))
            .with_children(|p| {
                p.spawn(small("The field is at peace", 32.0, Color::WHITE));
                p.spawn(small("", 15.0, TEXT_DIM));
                p.spawn(small("Keep playing, or Esc to quit.", 12.0, TEXT_DIM));
            });
        });
    commands.spawn((
        DebugText,
        small("", 15.0, Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            left: Val::Px(10.0),
            ..default()
        },
        Visibility::Hidden,
    ));
}

pub fn update(
    inv: Single<&Inventory, With<Player>>,
    items: Res<Registry<ItemDef>>,
    hotbar: Single<&Children, With<Hotbar>>,
    mut name: Single<&mut Text, (With<HeldName>, Without<Count>)>,
    mut painter: SlotPainter,
) {
    painter.paint(&hotbar, &inv.slots, Some(inv.selected));
    name.0 = inv.held().map_or(String::new(), |s| {
        let it = items.at(s.item);
        if s.n > 1 {
            format!("{} ×{}", it.name, s.n)
        } else {
            it.name.clone()
        }
    });
}

pub fn bars(
    player: Single<(&Stats, &Progress), With<Player>>,
    mut hp: Single<&mut Node, (With<HpBar>, Without<StaminaBar>, Without<FoodBar>)>,
    mut stamina: Single<&mut Node, (With<StaminaBar>, Without<HpBar>, Without<FoodBar>)>,
    mut food: Single<&mut Node, (With<FoodBar>, Without<HpBar>, Without<StaminaBar>)>,
    mut level: Single<&mut Text, With<LevelText>>,
) {
    let (s, p) = *player;
    food.width = Val::Percent((s.food / crate::stats::FOOD_MAX * 100.0).clamp(0.0, 100.0));
    hp.width = Val::Percent((s.hp / s.max_hp * 100.0).clamp(0.0, 100.0));
    stamina.width = Val::Percent((s.stamina / s.max_stamina.max(1.0) * 100.0).clamp(0.0, 100.0));
    level.0 = format!("Lv {}   {:.0} / {:.0} xp", p.level, p.xp, p.next_level_xp());
}

pub fn title(
    time: Res<Time>,
    mut card: Single<(Entity, &mut TitleCard, &Children)>,
    mut colors: Query<&mut TextColor>,
    mut commands: Commands,
) {
    let (entity, title, children) = &mut *card;
    title.0 -= time.delta_secs();
    if title.0 <= 0.0 {
        commands.entity(*entity).despawn();
        return;
    }
    let alpha = title.0.min(1.0);
    for child in children.iter() {
        if let Ok(mut c) = colors.get_mut(child) {
            c.0 =
                c.0.with_alpha(alpha * if c.0.alpha() < 0.9 { 0.7 } else { 1.0 });
        }
    }
}

pub fn toast(toast: Res<Toast>, mut text: Single<&mut Text, With<ToastText>>) {
    text.0 = toast.text.clone();
}

pub fn ending(
    ending: Res<Ending>,
    mut card: Single<(&mut Visibility, &Children), With<EndingCard>>,
    panels: Query<&Children, Without<EndingCard>>,
    mut texts: Query<&mut Text, Without<ToastText>>,
) {
    let (vis, children) = &mut *card;
    let Some(message) = &ending.0 else {
        **vis = Visibility::Hidden;
        return;
    };
    **vis = Visibility::Visible;
    for panel in children.iter() {
        if let Ok(kids) = panels.get(panel)
            && let Some(mid) = kids.get(1)
            && let Ok(mut t) = texts.get_mut(*mid)
        {
            t.0 = message.clone();
        }
    }
}

pub fn status(
    fishing: Res<crate::fish::FishingStatus>,
    mut text: Single<&mut Text, With<StatusText>>,
) {
    text.0 = fishing.0.unwrap_or("").to_owned();
}

pub fn debug(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    cursor: Res<Cursor>,
    map: Res<ChunkMap>,
    time: Res<Time>,
    clock: Res<crate::core::time::GameTime>,
    player: Single<&Transform, With<Player>>,
    mut text: Single<(&mut Text, &mut Visibility), With<DebugText>>,
) {
    let (text, vis) = &mut *text;
    if keys.just(&input, &keys.debug) {
        flip(vis);
    }
    if **vis == Visibility::Hidden {
        return;
    }
    let p = cell_of(player.translation.truncate());
    let c = cursor
        .cell
        .map_or("-".into(), |c| format!("{}, {}", c.x, c.y));
    text.0 = format!(
        "player {}, {}\ncursor {c}\nchunks {}  objects {}\nday {} {:.0}%  fps {:.0}",
        p.x,
        p.y,
        map.chunks.len(),
        map.objects.len(),
        clock.day(),
        clock.day_frac() * 100.0,
        1.0 / time.delta_secs().max(1e-6)
    );
}
