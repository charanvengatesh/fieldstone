//! Fieldstone: a small, data-driven, moddable isometric block engine.
// Bevy systems take many parameters and nested tuples by design.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod cam;
mod camera;
mod core;
mod debug;
mod figure;
mod fx;
mod harness;
mod interact;
mod inventory;
mod player;
mod post;
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
    /// `--angle DEGREES`: start at a free view angle instead of a quarter turn.
    pub angle: Option<f32>,
    /// `--test [NAME]`: headless; run the packs' `tests/*.lua` (those whose file name
    /// contains NAME, when given) and exit with the result. `--bench` is `--test bench`.
    pub test: Option<Option<String>>,
    /// `--tilt NAME`: start at a tilt preset.
    pub tilt: Option<String>,
    /// `--time F`: start the day clock at this fraction (0.5 is noon, 0 midnight).
    pub time: Option<f32>,
    /// `--perspective`: a mild perspective instead of the parallel projection.
    pub perspective: bool,
    /// `--horizon BLOCKS`: the fog horizon (blocks behind the focus), for snapshots.
    pub horizon: Option<f32>,
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
        headless: args.iter().any(|a| {
            a == "--headless-smoke" || a == "--snapshot" || a == "--test" || a == "--bench"
        }),
        test: if args.iter().any(|a| a == "--bench") {
            Some(Some("bench".into()))
        } else if args.iter().any(|a| a == "--test") {
            Some(value("--test").filter(|v| !v.starts_with("--")).cloned())
        } else {
            None
        },
        tilt: value("--tilt").cloned(),
        time: value("--time").and_then(|v| v.parse().ok()),
        perspective: args.iter().any(|a| a == "--perspective"),
        horizon: value("--horizon").and_then(|v| v.parse().ok()),
        snapshot: value("--snapshot").cloned(),
        scale: value("--scale").and_then(|v| v.parse().ok()).unwrap_or(2.0),
        rot: value("--rot").and_then(|v| v.parse().ok()).unwrap_or(0),
        grid: args.iter().any(|a| a == "--grid"),
        angle: value("--angle").and_then(|v| v.parse().ok()),
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
        .init_asset::<Image>()
        .init_asset::<world::mesh::BlockArt>()
        .init_asset::<world::mesh::WaterArt>()
        .init_asset::<world::mesh::FlatArt>()
        .init_asset::<world::mesh::SilhouetteArt>();
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
        // Replaced each frame the daylight moves by `core::time::clear_color`.
        .insert_resource(ClearColor(Color::srgb(0.36, 0.60, 0.92)))
        .add_plugins((
            core::CorePlugin,
            world::WorldPlugin,
            player::PlayerPlugin,
            interact::InteractPlugin,
            fx::FxPlugin,
            harness::HarnessPlugin,
            script::ScriptPlugin,
        ))
        .insert_resource(world::mesh::Grid(options.grid))
        .add_systems(
            Update,
            (
                (core::save::save, core::save::finish)
                    .chain()
                    .after(interact::InteractSet),
                cam::follow
                    .after(player::PlayerSet)
                    .after(interact::InteractSet)
                    .after(world::ViewSet)
                    .after(world::PlaceSet)
                    .before(world::WorldSet),
            ),
        );
    if options.snapshot.is_some() {
        // Load what the 1280 x 800 picture will show, not what a window would.
        app.insert_resource(world::stream::Picture {
            size: Vec2::new(1280.0, 800.0) / options.scale.max(0.25),
        });
    }
    if let Some(horizon) = options.horizon {
        app.insert_resource(world::stream::Streaming {
            horizon: horizon.clamp(8.0, 200.0),
            horizon_pinned: true,
            ..default()
        });
    }
    if let Some(filter) = options.test.clone() {
        app.insert_resource(script::RunTests { filter });
    } else if options.headless {
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
            bevy::pbr::MaterialPlugin::<world::mesh::BlockArt>::default(),
            bevy::pbr::MaterialPlugin::<world::mesh::WaterArt>::default(),
            bevy::pbr::MaterialPlugin::<world::mesh::FlatArt>::default(),
            bevy::pbr::MaterialPlugin::<world::mesh::SilhouetteArt>::default(),
            camera::CameraPlugin,
            post::PostPlugin,
            debug::DebugPlugin,
            ui::UiPlugin,
        ))
        .add_systems(Startup, interact::spawn_highlight)
        .add_systems(Update, interact::show_highlight.in_set(world::PlaceSet));
    }
    // A failed self-test or test run is an error exit, so scripts and CI can see it.
    if app.run().is_error() {
        std::process::exit(1);
    }
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
    let mut cam = cam::Cam::quarter(options.rot);
    if let Some(degrees) = options.angle {
        // `--angle` counts from the classic quarter, as the 2D engine's did.
        cam.yaw = degrees.to_radians() + std::f32::consts::FRAC_PI_4;
        cam.target_yaw = cam.yaw;
    }
    if let Some(pitch) = options.tilt.as_deref().and_then(cam::Cam::pitch_named) {
        cam.pitch = pitch;
        cam.target_pitch = pitch;
    }
    if options.perspective {
        cam.perspective = true;
    }
    // Inserted here so a save or `--time` wins over the plugin's default.
    let mut clock = core::time::GameTime::default();
    let (start, pending) = match loaded {
        Some(l) => {
            info!("world seed {seed}, loaded {} edited chunks", l.chunks.len());
            map.parked = l
                .chunks
                .iter()
                .map(|(p, b)| (*p, world::Parked::pack(b)))
                .collect();
            cam = cam::Cam::quarter(l.rot);
            if let Some(yaw) = l.yaw {
                cam.yaw = yaw;
                cam.target_yaw = yaw;
            }
            if let Some(pitch) = l.pitch.or_else(|| cam::Cam::pitch_named(&l.tilt)) {
                cam.pitch = pitch;
                cam.target_pitch = pitch;
            }
            cam.perspective = l.perspective;
            if l.time > 0.0 {
                clock.secs = l.time;
            }
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
    if let Some(frac) = options.time {
        clock.set_frac(frac);
    }
    world.insert_resource(clock);
    world.insert_resource(wg);
    world.insert_resource(map);
    world.insert_resource(cam);
    world.insert_resource(world::Focus(start));
    world.insert_resource(pending);
    world.insert_resource(core::save::SavePath(path));
}
