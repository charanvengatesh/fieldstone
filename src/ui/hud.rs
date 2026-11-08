//! Always-visible HUD: hotbar at the bottom, debug readout (F3) at the top left.

use bevy::prelude::*;

use super::*;
use crate::items::inventory::{HOTBAR, Inventory};
use crate::player::{Cursor, Player};
use crate::stats::{Progress, Stats};
use crate::world::{ChunkMap, cell_of};

#[derive(Component)]
pub struct Hotbar;
#[derive(Component)]
pub struct DebugText;
#[derive(Component)]
pub struct HpBar;
#[derive(Component)]
pub struct StaminaBar;
#[derive(Component)]
pub struct LevelText;
#[derive(Component)]
pub struct StatusText;

const BAR_W: f32 = 180.0;

fn bar(parent: &mut ChildSpawnerCommands, marker: impl Component, color: Color) {
    parent
        .spawn((
            Node {
                width: Val::Px(BAR_W),
                height: Val::Px(10.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(SLOT_BG),
            BorderColor::all(BORDER),
        ))
        .with_children(|b| {
            b.spawn((
                marker,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(color),
            ));
        });
}

pub fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Hotbar,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(12.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-(SLOT + 4.0) * HOTBAR as f32 / 2.0)),
                column_gap: Val::Px(4.0),
                ..default()
            },
        ))
        .with_children(|p| {
            for i in 0..HOTBAR {
                spawn_slot(p, i);
            }
        });
    commands
        .spawn((Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            bottom: Val::Px(12.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        },))
        .with_children(|p| {
            p.spawn((
                LevelText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
            ));
            bar(p, HpBar, Color::srgb(0.85, 0.25, 0.3));
            bar(p, StaminaBar, Color::srgb(0.35, 0.75, 0.3));
        });
    commands.spawn((
        StatusText,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0 + SLOT + 12.0),
            left: Val::Percent(50.0),
            margin: UiRect::left(Val::Px(-40.0)),
            ..default()
        },
    ));
    commands.spawn((
        DebugText,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
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
    hotbar: Single<&Children, With<Hotbar>>,
    mut painter: SlotPainter,
) {
    painter.paint(&hotbar, &inv.slots, Some(inv.selected));
}

pub fn bars(
    player: Single<(&Stats, &Progress), With<Player>>,
    mut hp: Single<&mut Node, (With<HpBar>, Without<StaminaBar>)>,
    mut stamina: Single<&mut Node, (With<StaminaBar>, Without<HpBar>)>,
    mut level: Single<&mut Text, With<LevelText>>,
) {
    let (s, p) = *player;
    hp.width = Val::Percent((s.hp / s.max_hp * 100.0).clamp(0.0, 100.0));
    stamina.width = Val::Percent((s.stamina / s.max_stamina.max(1.0) * 100.0).clamp(0.0, 100.0));
    level.0 = format!("Lv {}  {:.0}/{:.0} xp", p.level, p.xp, p.next_level_xp());
}

pub fn status(
    fishing: Res<crate::fish::FishingStatus>,
    mut text: Single<&mut Text, With<StatusText>>,
) {
    text.0 = fishing.0.unwrap_or("").to_owned();
}

pub fn debug(
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<Cursor>,
    map: Res<ChunkMap>,
    time: Res<Time>,
    clock: Res<crate::core::time::GameTime>,
    player: Single<&Transform, With<Player>>,
    mut text: Single<(&mut Text, &mut Visibility), With<DebugText>>,
) {
    let (text, vis) = &mut *text;
    if keys.just_pressed(KeyCode::F3) {
        flip(vis);
    }
    if **vis == Visibility::Hidden {
        return;
    }
    let p = cell_of(player.translation.truncate());
    let c = cursor.0.map_or("-".into(), |c| format!("{}, {}", c.x, c.y));
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
