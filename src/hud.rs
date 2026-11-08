//! Top-left readout of the grid cell under the cursor.

use bevy::prelude::*;

use crate::world::Cursor;

#[derive(Component)]
pub struct CoordText;

pub fn spawn(mut commands: Commands) {
    commands.spawn((
        CoordText,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(24.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(20.0),
            ..default()
        },
    ));
}

pub fn update(cursor: Res<Cursor>, mut text: Single<&mut Text, With<CoordText>>) {
    text.0 = cursor
        .0
        .map_or_else(String::new, |c| format!("{}, {}", c.x, c.y));
}
