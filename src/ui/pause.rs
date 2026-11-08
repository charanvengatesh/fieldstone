//! Pause menu (Esc): resume, save, quit. Pausing also freezes virtual time so effects stop.

use bevy::prelude::*;

use super::*;
use crate::core::save::SaveRequest;
use crate::core::settings::{Keys, Settings};
use crate::core::{Paused, Toast};

#[derive(Component)]
pub struct PauseMenu;

#[derive(Component, Clone, Copy)]
enum Button {
    Resume,
    Save,
    Quit,
}

pub fn spawn(mut commands: Commands) {
    commands
        .spawn((
            PauseMenu,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            Visibility::Hidden,
            GlobalZIndex(10),
        ))
        .with_children(|root| {
            root.spawn(panel(Node {
                padding: UiRect::all(Val::Px(24.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(10.0),
                min_width: Val::Px(260.0),
                ..default()
            }))
            .with_children(|p| {
                p.spawn((
                    Text::new("Paused"),
                    TextFont {
                        font_size: FontSize::Px(30.0),
                        ..default()
                    },
                    Node {
                        margin: UiRect::bottom(Val::Px(8.0)),
                        ..default()
                    },
                ));
                for (b, label) in [
                    (Button::Resume, "Resume"),
                    (Button::Save, "Save game"),
                    (Button::Quit, "Save and quit"),
                ] {
                    p.spawn((
                        b,
                        Node {
                            width: Val::Percent(100.0),
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::all(Val::Px(7.0)),
                            ..default()
                        },
                        BackgroundColor(SLOT_BG),
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            Text::new(label),
                            TextFont {
                                font_size: FontSize::Px(16.0),
                                ..default()
                            },
                        ));
                    })
                    .observe(
                        move |_: On<PointerClick>,
                              mut paused: ResMut<Paused>,
                              mut time: ResMut<Time<Virtual>>,
                              mut save: ResMut<SaveRequest>,
                              settings: Res<Settings>,
                              mut toast: ResMut<Toast>,
                              mut exit: MessageWriter<AppExit>| {
                            match b {
                                Button::Resume => {
                                    paused.0 = false;
                                    time.unpause();
                                }
                                Button::Save => {
                                    save.0 = true;
                                    toast.show("Saved.", 2.0);
                                }
                                Button::Quit => {
                                    save.0 = true;
                                    save.1 = true;
                                    crate::core::settings::save(&settings);
                                    exit.write(AppExit::Success);
                                }
                            }
                        },
                    );
                }
                p.spawn((
                    Text::new("Keys and zoom live in settings.ron"),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(TEXT_DIM),
                ));
            });
        });
}

pub fn toggle(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    settings: Res<Settings>,
    mut paused: ResMut<Paused>,
    mut time: ResMut<Time<Virtual>>,
    mut vis: Single<&mut Visibility, With<PauseMenu>>,
) {
    if keys.just(&input, &keys.pause) {
        paused.0 = !paused.0;
        if paused.0 {
            time.pause();
            crate::core::settings::save(&settings);
        } else {
            time.unpause();
        }
    }
    **vis = if paused.0 {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}
