//! Fieldstone: a small, data-driven, moddable isometric block engine.
// Bevy systems take many parameters and nested tuples by design.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]
// Temporary while the engine is rebuilt module by module; removed before 0.7.0.
#![allow(dead_code)]

mod camera;
mod core;
mod debug;
mod interact;
mod inventory;
mod player;
mod smoke;
mod ui;
mod world;

use bevy::prelude::*;

/// Command-line choices: `--new` ignores an existing save, `--seed N` fixes the world seed,
/// `--headless-smoke` runs the scripted self-test without a window.
pub struct Options {
    pub fresh: bool,
    pub seed: Option<u32>,
    pub headless: bool,
    /// `--snapshot NAME`: headless, then a software picture of the world in `debug/NAME.png`.
    pub snapshot: Option<String>,
    /// Snapshot zoom in screen pixels per texel (`--scale N`), starting view rotation
    /// (`--rot N`) and the full grid (`--grid`).
    pub scale: f32,
    pub rot: u8,
    pub grid: bool,
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
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
    };
    let options = Options {
        fresh: args.iter().any(|a| a == "--new"),
        seed: value("--seed").and_then(|v| v.parse().ok()),
        headless: args
            .iter()
            .any(|a| a == "--headless-smoke" || a == "--snapshot"),
        snapshot: value("--snapshot").cloned(),
        scale: value("--scale").and_then(|v| v.parse().ok()).unwrap_or(2.0),
        rot: value("--rot").and_then(|v| v.parse().ok()).unwrap_or(0),
        grid: args.iter().any(|a| a == "--grid"),
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
    setup_world(&mut app, &options);
    let settings = core::settings::load();
    app.insert_resource(core::settings::Keys::from_names(&settings.keys))
        .insert_resource(settings)
        .insert_resource(ClearColor(Color::srgb_u8(24, 28, 40)))
        .add_plugins((
            core::CorePlugin,
            world::WorldPlugin,
            player::PlayerPlugin,
            interact::InteractPlugin,
        ))
        .insert_resource(world::iso::View {
            rot: options.rot % 4,
        })
        .insert_resource(world::mesh::Grid(options.grid));
    if options.headless {
        app.insert_resource(smoke::Smoke::new(options.snapshot.clone(), options.scale))
            .add_systems(Update, smoke::drive.before(player::PlayerSet));
    } else {
        app.add_plugins((camera::CameraPlugin, debug::DebugPlugin, ui::UiPlugin))
            .add_systems(Startup, interact::spawn_highlight)
            .add_systems(
                Update,
                (
                    interact::track_cursor.before(interact::InteractSet),
                    interact::show_highlight.after(interact::InteractSet),
                ),
            );
    }
    app.run();
}

/// Builds the generator from `worldgen.ron` and a seed, and points the camera at the spawn.
fn setup_world(app: &mut App, options: &Options) {
    let world = app.world_mut();
    let def = world
        .resource::<core::Registry<core::WorldGenDef>>()
        .iter()
        .last()
        .map(|(_, d)| d.clone())
        .expect("no worldgen.ron record; is assets/packs/base present?");
    let seed = options.seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(7, |d| d.as_secs() as u32)
    });
    let wg = world::worldgen::WorldGen::new(
        seed,
        &def,
        world.resource::<core::Registry<core::BlockDef>>(),
    );
    let spawn = wg.spawn();
    info!("world seed {seed}, spawn {spawn}");
    world.insert_resource(wg);
    world.insert_resource(world::Focus(spawn));
}
