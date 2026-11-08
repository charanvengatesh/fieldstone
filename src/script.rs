//! Lua hooks for mods. Every pack's `scripts` load at startup; the engine fires named
//! callbacks (see `mods/README.md`) and exposes a few global functions. Script-side calls are
//! queued and applied by a Bevy system, so scripts never touch the ECS directly.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;
use bevy_mod_scripting::asset::Language;
use bevy_mod_scripting::prelude::*;

use crate::core::packs::LoadedPacks;
use crate::core::time::GameTime;
use crate::core::{CreatureDef, ItemDef, Registry, ShapeDef};
use crate::items::craft::Crafted;
use crate::items::drops::SpawnDrop;
use crate::items::interact::{ObjectBroken, ObjectPlaced};
use crate::player::Player;
use crate::stats::Died;
use crate::world::{CELL, center_of};

callback_labels!(
    OnObjectBroken => "on_object_broken",
    OnObjectPlaced => "on_object_placed",
    OnCraft => "on_craft",
    OnDeath => "on_death",
    OnDayStart => "on_day_start",
);

/// What scripts asked for since the last frame.
enum Request {
    GiveItem(String, u32),
    SpawnCreature(String, f32, f32),
}

static QUEUE: Mutex<Vec<Request>> = Mutex::new(Vec::new());
static NIGHT: AtomicBool = AtomicBool::new(false);

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
        .register("is_night", || NIGHT.load(Ordering::Relaxed));
}

/// Applies queued script requests.
pub fn apply_requests(
    items: Res<Registry<ItemDef>>,
    creatures: Res<Registry<CreatureDef>>,
    shapes: Res<Registry<ShapeDef>>,
    assets: Res<crate::art::ShapeAssets>,
    player: Single<&Transform, With<Player>>,
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
        }
    }
}

/// Turns engine messages into script callbacks.
pub fn bridge(
    clock: Res<GameTime>,
    objects: Res<Registry<crate::core::ObjectDef>>,
    recipes: Res<Registry<crate::core::RecipeDef>>,
    creatures: Res<Registry<CreatureDef>>,
    mut broken: MessageReader<ObjectBroken>,
    mut placed: MessageReader<ObjectPlaced>,
    mut crafted: MessageReader<Crafted>,
    mut died: MessageReader<Died>,
    mut last_day: Local<Option<u32>>,
    mut out: MessageWriter<ScriptCallbackMessage>,
) {
    NIGHT.store(clock.day_frac() >= 0.5, Ordering::Relaxed);
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
                    apply_requests,
                )
                    .chain()
                    .in_set(ScriptSet),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScriptSet;
