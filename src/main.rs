//! Fieldstone: steer a stone across a green field and lay walls on the grid.

mod hud;
mod player;
mod world;

use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Fieldstone".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(
            75.0 / 255.0,
            150.0 / 255.0,
            50.0 / 255.0,
        )))
        .init_resource::<world::Blocks>()
        .init_resource::<world::Cursor>()
        .add_systems(Startup, (player::spawn, world::spawn_border, hud::spawn))
        .add_systems(
            Update,
            (
                player::movement,
                player::follow,
                world::track_cursor,
                world::build,
                hud::update,
                world::draw_grid,
            )
                .chain(),
        )
        .run();
}
