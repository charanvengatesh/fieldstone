//! Fieldstone: a small, data-driven, moddable isometric block engine.
// Bevy systems take many parameters and nested tuples by design.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod camera;
mod core;
mod debug;
mod interact;
mod inventory;
mod player;
mod script;
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
        .init_asset::<Image>()
        .init_asset::<world::mesh::BlockArt>();
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
            script::ScriptPlugin,
        ))
        .insert_resource(world::mesh::Grid(options.grid))
        .add_systems(Update, core::save::save.after(interact::InteractSet));
    if options.headless {
        app.insert_resource(smoke::Smoke::new(options.snapshot.clone(), options.scale))
            .add_systems(
                Update,
                (
                    smoke::drive.before(player::PlayerSet),
                    smoke::finish.after(core::save::save),
                ),
            );
    } else {
        app.add_plugins((
            bevy::sprite_render::Material2dPlugin::<world::mesh::BlockArt>::default(),
            camera::CameraPlugin,
            debug::DebugPlugin,
            ui::UiPlugin,
        ))
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

/// Builds the generator from `worldgen.ron` and either the save file or a fresh seed.
fn setup_world(app: &mut App, options: &Options) {
    // The scripted run keeps its own file so it never touches a real save.
    let path = if options.headless {
        core::paths::user_dir().join("saves").join("smoke.ron")
    } else {
        core::paths::save_file()
    };
    let world = app.world_mut();
    let def = world
        .resource::<core::Registry<core::WorldGenDef>>()
        .iter()
        .last()
        .map(|(_, d)| d.clone())
        .expect("no worldgen.ron record; is assets/packs/base present?");
    let blocks = world.resource::<core::Registry<core::BlockDef>>();
    let items = world.resource::<core::Registry<core::ItemDef>>();
    let loaded = if options.fresh || options.headless {
        None
    } else {
        core::save::load(&path, blocks, items)
    };
    if options.seed.is_some() && loaded.is_some() {
        warn!("--seed ignored: a save exists; use --new to start a fresh world with that seed");
    }
    let seed = loaded
        .as_ref()
        .map(|l| l.seed)
        .or(options.seed)
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(7, |d| d.as_secs() as u32)
        });
    let wg = world::worldgen::WorldGen::new(
        seed,
        &def,
        blocks,
        world.resource::<core::Registry<core::FeatureDef>>(),
        world.resource::<core::Registry<core::StampDef>>(),
    );
    let mut map = world::Map::default();
    let mut view = world::iso::View {
        rot: options.rot % 4,
    };
    let (start, pending) = match loaded {
        Some(l) => {
            info!("world seed {seed}, loaded {} edited chunks", l.chunks.len());
            map.parked = l.chunks;
            view.rot = l.rot;
            (
                l.player,
                core::save::Pending {
                    inventory: l.inventory,
                    selected: l.selected,
                },
            )
        }
        None => {
            let spawn = wg.spawn();
            info!("world seed {seed}, spawn {spawn}");
            (spawn, core::save::Pending::default())
        }
    };
    world.insert_resource(wg);
    world.insert_resource(map);
    world.insert_resource(view);
    world.insert_resource(world::Focus(start));
    world.insert_resource(pending);
    world.insert_resource(core::save::SavePath(path));
}
