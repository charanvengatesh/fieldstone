//! Fieldstone: a small, data-driven, moddable isometric block engine.
// Bevy systems take many parameters and nested tuples by design.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]
// Temporary while the engine is rebuilt module by module; removed before 0.7.0.
#![allow(dead_code)]

mod core;
mod inventory;

use bevy::prelude::*;

/// Command-line choices: `--new` ignores an existing save, `--seed N` fixes the world seed,
/// `--headless-smoke` runs the scripted self-test without a window.
pub struct Options {
    pub fresh: bool,
    pub seed: Option<u32>,
    pub headless: bool,
}

fn main() {
    // Content and player files are found relative to the game folder, not wherever the
    // process was launched from (Finder, a zip, a terminal elsewhere).
    let root = core::paths::root();
    if let Err(e) = std::env::set_current_dir(&root) {
        eprintln!("could not enter {}: {e}", root.display());
    }
    // A panic in a double-clicked binary prints nowhere; leave a note next to the game.
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("{info}\n");
        eprintln!("{msg}");
        let _ = std::fs::write(core::paths::user_dir().join("fieldstone-crash.log"), msg);
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = Options {
        fresh: args.iter().any(|a| a == "--new"),
        seed: args
            .iter()
            .position(|a| a == "--seed")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok()),
        headless: args.iter().any(|a| a == "--headless-smoke"),
    };
    let mut app = App::new();
    if options.headless {
        app.add_plugins((
            MinimalPlugins.set(bevy::app::ScheduleRunnerPlugin::run_loop(
                std::time::Duration::from_millis(16),
            )),
            bevy::log::LogPlugin::default(),
            AssetPlugin::default(),
            bevy::input::InputPlugin,
            TransformPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<ColorMaterial>()
        .init_asset::<Image>();
    } else {
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Fieldstone".into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(ImagePlugin::default_nearest())
                // Assets live under the content root, wherever the process was started.
                .set(AssetPlugin {
                    file_path: core::paths::root()
                        .join("assets")
                        .to_string_lossy()
                        .into_owned(),
                    ..default()
                }),
        );
    }
    core::packs::load_all(&mut app);
    let settings = core::settings::load();
    app.insert_resource(core::settings::Keys::from_names(&settings.keys))
        .insert_resource(settings)
        .insert_resource(ClearColor(Color::srgb_u8(24, 28, 40)))
        .add_plugins(core::CorePlugin)
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Camera2d);
        });
    if options.headless {
        app.add_systems(
            Update,
            |mut frames: Local<u32>, mut exit: MessageWriter<AppExit>| {
                *frames += 1;
                if *frames == 60 {
                    info!("smoke test ran 60 frames");
                    exit.write(AppExit::Success);
                }
            },
        );
    }
    app.run();
}
