//! Fieldstone: a small, data-driven, moddable top-down survival game.
// Bevy systems take many parameters and nested tuples by design.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod art;
mod combat;
mod core;
mod daylight;
mod farm;
mod fish;
mod fx;
mod ground;
mod hints;
mod items;
mod player;
mod script;
mod smoke;
mod sound;
mod stats;
mod ui;
mod water;
mod world;

use bevy::prelude::*;

use crate::core::save::{PendingPlayer, SavedChunks};
use crate::core::time::GameTime;
use crate::core::{ObjectDef, Registry, TerrainDef, WorldGenDef};
use crate::world::worldgen::WorldGen;

fn main() {
    // Content and player files are found relative to the game folder, not wherever the
    // process was launched from (Finder, a zip, a terminal elsewhere).
    let root = core::paths::root();
    if let Err(e) = std::env::set_current_dir(&root) {
        eprintln!("could not enter {}: {e}", root.display());
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let headless = args.iter().any(|a| a == "--headless-smoke");
    let options = Options {
        fresh: args.iter().any(|a| a == "--new"),
        seed: args
            .iter()
            .position(|a| a == "--seed")
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok()),
    };
    let mut app = App::new();
    if headless {
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
        .init_resource::<smoke::Smoke>()
        .add_systems(Update, smoke::drive.before(player::PlayerSet));
    } else {
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Fieldstone".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((
            ui::UiPlugin,
            daylight::DaylightPlugin,
            fx::FxPlugin,
            water::WaterPlugin,
            ground::GroundPlugin,
            sound::SoundPlugin,
            hints::HintsPlugin,
        ));
    }
    core::packs::load_all(&mut app);
    setup_world(&mut app, &options);
    let settings = core::settings::load();
    app.insert_resource(core::settings::Keys::from_names(&settings.keys))
        .insert_resource(settings)
        .init_resource::<core::save::SaveRequest>();
    app.insert_resource(ClearColor(Color::srgb_u8(40, 80, 160)))
        .add_plugins((
            core::CorePlugin,
            art::ArtPlugin,
            world::WorldPlugin,
            player::PlayerPlugin,
            items::ItemsPlugin,
            combat::CombatPlugin,
            farm::FarmPlugin,
            fish::FishPlugin,
            stats::StatsPlugin,
            script::ScriptPlugin,
        ))
        .configure_sets(
            Update,
            (
                player::PlayerSet,
                world::WorldSet,
                items::ItemsSet,
                combat::CombatSet,
                farm::FarmSet,
                fish::FishSet,
                stats::StatsSet,
                script::ScriptSet,
            )
                .chain()
                .run_if(core::not_paused),
        )
        .add_systems(Update, (core::save::save, core::save::restore_player))
        .run();
}

/// Command-line choices: `--new` ignores an existing save, `--seed N` fixes the world seed.
struct Options {
    fresh: bool,
    seed: Option<u32>,
}

/// Builds the generator from `worldgen.ron` and either a save file or a fresh seed.
fn setup_world(app: &mut App, options: &Options) {
    let world = app.world_mut();
    for kind in [
        "terrain", "objects", "items", "shapes", "palette", "worldgen",
    ] {
        let _ = kind;
    }
    let terrain = world.get_resource_or_init::<Registry<TerrainDef>>().len();
    if terrain == 0 {
        panic!("no terrain defined; is assets/packs/base present?");
    }
    world.get_resource_or_init::<Registry<ObjectDef>>();
    world.get_resource_or_init::<Registry<core::HintDef>>();
    let gen_def = world
        .get_resource_or_init::<Registry<WorldGenDef>>()
        .iter()
        .last()
        .map(|(_, d)| d.clone())
        .expect("worldgen.ron must define one record");
    let terrain = world.resource::<Registry<TerrainDef>>();
    let objects = world.resource::<Registry<ObjectDef>>();
    let loaded = if options.fresh {
        None
    } else {
        core::save::load(terrain, objects)
    };
    if options.seed.is_some() && loaded.is_some() {
        warn!("--seed ignored: a save exists; use --new to start a fresh world with that seed");
    }
    let seed = loaded
        .as_ref()
        .map(|l| l.seed)
        .or(options.seed)
        .unwrap_or_else(|| {
            None::<&core::save::Loaded>.map_or_else(
                || {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(7, |d| d.as_secs() as u32)
                },
                |l| l.seed,
            )
        });
    let wg = WorldGen::new(seed, &gen_def, terrain, objects);
    let spawn = spawn_point(&wg, terrain, objects);
    let pending = match &loaded {
        Some(l) => PendingPlayer {
            pos: l.player,
            inventory: l.inventory.clone(),
            hp: l.hp,
            progress: l.progress.clone(),
            containers: l.containers.clone(),
            spawn: l.spawn,
            ending: l.ending.clone(),
            growth: l.growth.clone(),
        },
        None => PendingPlayer {
            pos: spawn,
            inventory: vec![],
            hp: None,
            progress: None,
            containers: vec![],
            spawn: None,
            ending: None,
            growth: vec![],
        },
    };
    let (time, chunks) = loaded.map_or((GameTime::default(), SavedChunks::default()), |l| {
        (l.time, l.chunks)
    });
    info!("world seed {seed}");
    app.insert_resource(wg)
        .insert_resource(stats::SpawnPoint(spawn))
        .insert_resource(time)
        .insert_resource(chunks)
        .insert_resource(pending);
}

/// A good place to start: open land (not sand, not rock), trees close by, and enough
/// connected ground to walk around on. Searched in growing rings from the origin.
fn spawn_point(
    wg: &WorldGen,
    terrain: &Registry<TerrainDef>,
    objects: &Registry<ObjectDef>,
) -> Vec2 {
    let land: Vec<u16> = ["grass", "meadow", "forest"]
        .iter()
        .filter_map(|id| terrain.idx(id))
        .collect();
    let tree = objects.idx("tree");
    let open = |c: IVec2| {
        let t = wg.tile(c);
        terrain.at(t.terrain).walkable && t.object.is_none_or(|o| !objects.at(o).solid)
    };
    let mut fallback = None;
    for r in 0..400i32 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue;
                }
                let c = IVec2::new(dx, dy);
                let t = wg.tile(c);
                if !terrain.at(t.terrain).walkable || t.object.is_some() {
                    continue;
                }
                fallback.get_or_insert(c);
                if !land.contains(&t.terrain) {
                    continue;
                }
                let trees = (-10..=10)
                    .flat_map(|y| (-10..=10).map(move |x| c + IVec2::new(x, y)))
                    .filter(|n| wg.tile(*n).object == tree && tree.is_some())
                    .count();
                if trees < 4 {
                    continue;
                }
                if connected_area(c, 200, &open) >= 200 {
                    return world::center_of(c);
                }
            }
        }
    }
    world::center_of(fallback.unwrap_or(IVec2::ZERO))
}

/// Flood fill over open cells, stopping once `cap` cells are reached.
fn connected_area(start: IVec2, cap: usize, open: &impl Fn(IVec2) -> bool) -> usize {
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![start];
    while let Some(c) = stack.pop() {
        if seen.len() >= cap || !seen.insert(c) || !open(c) {
            continue;
        }
        for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            if !seen.contains(&(c + d)) {
                stack.push(c + d);
            }
        }
    }
    seen.len()
}
