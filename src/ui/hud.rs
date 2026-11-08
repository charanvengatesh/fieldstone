//! Always-visible HUD: hotbar at the bottom, debug readout (F3) at the top left.

use bevy::prelude::*;

use super::*;
use crate::items::inventory::{HOTBAR, Inventory};
use crate::player::{Cursor, Player};
use crate::world::{ChunkMap, cell_of};

#[derive(Component)]
pub struct Hotbar;
#[derive(Component)]
pub struct DebugText;

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
