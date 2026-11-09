//! Lua hooks for mods. Every pack's `scripts` load at startup; the engine fires named
//! callbacks (see `mods/README.md`) and exposes a few global functions. Script-side calls are
//! queued and applied by a Bevy system, so scripts never touch the ECS directly.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use bevy::prelude::*;
use bevy_mod_scripting::asset::Language;
use bevy_mod_scripting::prelude::*;

use crate::core::packs::LoadedPacks;
use crate::core::time::GameTime;
use crate::core::{CreatureDef, ItemDef, Registry, ShapeDef};
use crate::items::craft::Crafted;
use crate::items::drops::SpawnDrop;
use crate::items::interact::{ObjectBroken, ObjectPlaced, ObjectUsed};
use crate::player::Player;
use crate::stats::Died;
use crate::world::{CELL, center_of};

callback_labels!(
    OnObjectBroken => "on_object_broken",
    OnObjectPlaced => "on_object_placed",
    OnCraft => "on_craft",
    OnDeath => "on_death",
    OnDayStart => "on_day_start",
    OnObjectUsed => "on_object_used",
    OnTick => "on_tick",
);

/// What scripts asked for since the last frame.
enum Request {
    GiveItem(String, u32),
    SpawnCreature(String, f32, f32),
    ShowText(String, f32),
    Win(String),
    SetDayFrac(f32),
}

static QUEUE: Mutex<Vec<Request>> = Mutex::new(Vec::new());
static NIGHT: AtomicBool = AtomicBool::new(false);
/// Frame snapshot readable from scripts without touching the ECS.
static PLAYER_X: AtomicI32 = AtomicI32::new(0);
static PLAYER_Y: AtomicI32 = AtomicI32::new(0);
static DAY_FRAC: AtomicU32 = AtomicU32::new(0);
static JUST_PRESSED: Mutex<Vec<KeyCode>> = Mutex::new(Vec::new());

fn push(r: Request) {
    QUEUE.lock().unwrap().push(r);
}

/// Reads each pack's script files and hands them to the scripting runtime.
pub fn load_scripts(
    asset_server: Res<AssetServer>,
    packs: Res<LoadedPacks>,
    mut commands: Commands,
) {
    let mut handles = Vec::new();
    for (dir, pack) in &packs.0 {
        for rel in &pack.scripts {
            let path = dir.join(rel);
            match std::fs::read_to_string(&path) {
                Ok(src) => {
                    info!("script {}", path.display());
                    handles
                        .push(asset_server.add(ScriptAsset::new(src).with_language(Language::Lua)));
                }
                Err(e) => error!("{}: {e}", path.display()),
            }
        }
    }
    if !handles.is_empty() {
        commands.spawn(ScriptComponent(handles));
    }
}

/// Global functions available to every script.
pub fn register_api(world: &mut World) {
    NamespaceBuilder::<GlobalNamespace>::new_unregistered(world)
        .register("log", |msg: String| info!("[script] {msg}"))
        .register("give_item", |id: String, n: i64| {
            push(Request::GiveItem(id, n.max(0) as u32))
        })
        .register("spawn_creature", |id: String, x: i64, y: i64| {
            push(Request::SpawnCreature(id, x as f32, y as f32))
        })
        .register("is_night", || NIGHT.load(Ordering::Relaxed))
        .register("show_text", |msg: String, secs: f64| {
            push(Request::ShowText(msg, secs as f32))
        })
        .register("win", |msg: String| push(Request::Win(msg)))
        .register("set_day_frac", |f: f64| push(Request::SetDayFrac(f as f32)))
        .register("player_x", || PLAYER_X.load(Ordering::Relaxed) as i64)
        .register("player_y", || PLAYER_Y.load(Ordering::Relaxed) as i64)
        .register("day_frac", || {
            f32::from_bits(DAY_FRAC.load(Ordering::Relaxed)) as f64
        })
        .register("key_just_pressed", |name: String| {
            crate::core::settings::parse_key(&name)
                .is_some_and(|k| JUST_PRESSED.lock().unwrap().contains(&k))
        })
        .register("screenshot", |name: String| {
            crate::debug::push(crate::debug::Probe::Screenshot(name))
        })
        .register("dump_state", |name: String| {
            crate::debug::push(crate::debug::Probe::Dump(name))
        })
        .register("append_file", |name: String, line: String| {
            crate::debug::push(crate::debug::Probe::Append(name, line))
        });
}

/// Applies queued script requests.
pub fn apply_requests(
    items: Res<Registry<ItemDef>>,
    creatures: Res<Registry<CreatureDef>>,
    shapes: Res<Registry<ShapeDef>>,
    assets: Res<crate::art::ShapeAssets>,
    player: Single<&Transform, With<Player>>,
    mut toast: ResMut<crate::core::Toast>,
    mut ending: ResMut<crate::core::Ending>,
    mut clock: ResMut<GameTime>,
    mut drops: MessageWriter<SpawnDrop>,
    mut commands: Commands,
) {
    let queue: Vec<Request> = std::mem::take(&mut *QUEUE.lock().unwrap());
    for r in queue {
        match r {
            Request::GiveItem(id, n) => match items.idx(&id) {
                Some(item) => {
                    drops.write(SpawnDrop {
                        item,
                        n,
                        at: player.translation.truncate(),
                    });
                }
                None => warn!("script: unknown item '{id}'"),
            },
            Request::SpawnCreature(id, x, y) => match creatures.idx(&id) {
                Some(c) => {
                    let at = center_of(IVec2::new(x as i32, y as i32));
                    crate::combat::spawn::spawn_creature(
                        &mut commands,
                        &assets,
                        &shapes,
                        &creatures,
                        c,
                        at,
                    );
                }
                None => warn!("script: unknown creature '{id}'"),
            },
            Request::ShowText(msg, secs) => toast.show(msg, secs.max(0.5)),
            Request::Win(msg) => {
                info!("[script] win: {msg}");
                ending.0 = Some(msg);
            }
            Request::SetDayFrac(f) => {
                let day = clock.day() as f64;
                clock.secs = (day + f.clamp(0.0, 0.999) as f64) * crate::core::time::DAY_SECS;
            }
        }
    }
}

/// Turns engine messages into script callbacks.
pub fn bridge(
    clock: Res<GameTime>,
    time: Res<Time>,
    input: Res<ButtonInput<KeyCode>>,
    player: Option<Single<&Transform, With<Player>>>,
    objects: Res<Registry<crate::core::ObjectDef>>,
    recipes: Res<Registry<crate::core::RecipeDef>>,
    creatures: Res<Registry<CreatureDef>>,
    mut broken: MessageReader<ObjectBroken>,
    mut placed: MessageReader<ObjectPlaced>,
    mut crafted: MessageReader<Crafted>,
    mut died: MessageReader<Died>,
    mut used: MessageReader<ObjectUsed>,
    mut last_day: Local<Option<u32>>,
    mut out: MessageWriter<ScriptCallbackMessage>,
) {
    NIGHT.store(clock.day_frac() >= 0.5, Ordering::Relaxed);
    DAY_FRAC.store(clock.day_frac().to_bits(), Ordering::Relaxed);
    if let Some(p) = &player {
        let c = (p.translation.truncate() / CELL).round().as_ivec2();
        PLAYER_X.store(c.x, Ordering::Relaxed);
        PLAYER_Y.store(c.y, Ordering::Relaxed);
    }
    *JUST_PRESSED.lock().unwrap() = input.get_just_pressed().copied().collect();
    out.write(ScriptCallbackMessage::new_for_all_scripts(
        OnTick,
        vec![ScriptValue::Float(time.delta_secs_f64())],
    ));
    let s = |v: &str| ScriptValue::String(v.to_owned().into());
    let i = |v: i32| ScriptValue::Integer(v as i64);
    for b in broken.read() {
        out.write(ScriptCallbackMessage::new_for_all_scripts(
            OnObjectBroken,
            vec![i(b.cell.x), i(b.cell.y), s(objects.id_of(b.object))],
        ));
    }
    for p in placed.read() {
        out.write(ScriptCallbackMessage::new_for_all_scripts(
            OnObjectPlaced,
            vec![i(p.cell.x), i(p.cell.y), s(objects.id_of(p.object))],
        ));
    }
    for u in used.read() {
        out.write(ScriptCallbackMessage::new_for_all_scripts(
            OnObjectUsed,
            vec![
                i(u.cell.x),
                i(u.cell.y),
                s(objects.id_of(u.from)),
                s(objects.id_of(u.to)),
            ],
        ));
    }
    for c in crafted.read() {
        out.write(ScriptCallbackMessage::new_for_all_scripts(
            OnCraft,
            vec![s(recipes.id_of(c.recipe))],
        ));
    }
    for d in died.read() {
        let who = d.creature.map_or("player", |c| creatures.id_of(c));
        let cell = (d.at / CELL).round().as_ivec2();
        out.write(ScriptCallbackMessage::new_for_all_scripts(
            OnDeath,
            vec![s(who), i(cell.x), i(cell.y)],
        ));
    }
    let day = clock.day();
    if *last_day != Some(day) {
        if last_day.is_some() {
            out.write(ScriptCallbackMessage::new_for_all_scripts(
                OnDayStart,
                vec![i(day as i32)],
            ));
        }
        *last_day = Some(day);
    }
}

pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(BMSPlugin)
            .add_systems(Startup, (register_api, load_scripts).chain())
            .add_systems(
                Update,
                (
                    bridge,
                    event_handler::<OnObjectBroken, LuaScriptingPlugin>,
                    event_handler::<OnObjectPlaced, LuaScriptingPlugin>,
                    event_handler::<OnCraft, LuaScriptingPlugin>,
                    event_handler::<OnDeath, LuaScriptingPlugin>,
                    event_handler::<OnDayStart, LuaScriptingPlugin>,
                    event_handler::<OnObjectUsed, LuaScriptingPlugin>,
                    event_handler::<OnTick, LuaScriptingPlugin>,
                    apply_requests,
                )
                    .chain()
                    .in_set(ScriptSet),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScriptSet;
