//! Lua for mods. Every pack's `scripts` load into one Lua state, each in its own
//! environment so their hook functions never collide. Once per frame the engine exposes
//! its functions and calls the hooks; the functions act on the world directly, so a script
//! reads a block and sees the answer on the next line.
//!
//! The Lua standard library is cut down to `table`, `string`, `math` and `utf8`: scripts
//! have no file or process access beyond `append_file`, which stays inside `debug/`.

use std::cell::RefCell;
use std::collections::HashSet;

use bevy::prelude::*;
use mlua::{Function, IntoLuaMulti, Lua, LuaOptions, StdLib, Table};

use crate::core::packs::LoadedPacks;
use crate::core::settings::parse_key;
use crate::core::{BlockDef, ItemDef, Registry, Toast};
use crate::interact::{BlockBroken, BlockPlaced};
use crate::inventory::{HOTBAR, Inventory, sort_slots};
use crate::player::Player;
use crate::world::Map;

/// Things that happened since the scripts last ran.
#[derive(Resource, Default)]
pub struct ScriptEvents(Vec<Event>);

enum Event {
    Broken(IVec3, u16),
    Placed(IVec3, u16),
}

struct Script {
    name: String,
    env: Table,
}

/// The Lua state and the scripts in it. Not `Send`, so it lives as a non-send resource and
/// only [`run`] touches it.
pub struct Vm {
    lua: Lua,
    scripts: Vec<Script>,
    /// `(name, source)` waiting to be executed on the next run.
    pending: Vec<(String, String)>,
    /// `script:hook` pairs that already failed, so an error is logged once, not every frame.
    reported: HashSet<String>,
}

impl Vm {
    pub fn new(sources: Vec<(String, String)>) -> Self {
        let libs = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8;
        Self {
            lua: Lua::new_with(libs, LuaOptions::default()).expect("lua state"),
            scripts: Vec::new(),
            pending: sources,
            reported: HashSet::new(),
        }
    }
}

/// Reads each pack's script files.
pub fn setup(world: &mut World) {
    let mut sources = Vec::new();
    for (dir, pack) in &world.resource::<LoadedPacks>().0 {
        for rel in &pack.scripts {
            let path = dir.join(rel);
            match std::fs::read_to_string(&path) {
                Ok(src) => {
                    info!("script {}", path.display());
                    sources.push((format!("{}/{rel}", pack.name), src));
                }
                Err(e) => error!("{}: {e}", path.display()),
            }
        }
    }
    world.insert_non_send(Vm::new(sources));
}

pub fn collect(
    mut broken: MessageReader<BlockBroken>,
    mut placed: MessageReader<BlockPlaced>,
    mut out: ResMut<ScriptEvents>,
) {
    out.0
        .extend(broken.read().map(|b| Event::Broken(b.at, b.block)));
    out.0
        .extend(placed.read().map(|b| Event::Placed(b.at, b.block)));
}

/// Calls `hook` in one script if it defines it; a failure is logged once.
fn call(script: &Script, hook: &str, args: impl IntoLuaMulti, reported: &mut HashSet<String>) {
    let Ok(Some(f)) = script.env.get::<Option<Function>>(hook) else {
        return;
    };
    if let Err(e) = f.call::<()>(args)
        && reported.insert(format!("{}:{hook}", script.name))
    {
        error!("script {} in {hook}: {e}", script.name);
    }
}

fn cell(x: f64, y: f64, z: f64) -> IVec3 {
    IVec3::new(x.floor() as i32, y.floor() as i32, z.floor() as i32)
}

/// Exposes the engine to Lua, loads any waiting scripts, and fires this frame's hooks.
pub fn run(world: &mut World) {
    let Some(vm) = world.remove_non_send::<Vm>() else {
        return;
    };
    let Vm {
        lua,
        mut scripts,
        mut pending,
        mut reported,
    } = vm;
    let events = std::mem::take(&mut world.resource_mut::<ScriptEvents>().0);
    let dt = world.resource::<Time>().delta_secs_f64();
    let names: Vec<(IVec3, String, bool)> = {
        let blocks = world.resource::<Registry<BlockDef>>();
        events
            .iter()
            .map(|e| match e {
                Event::Broken(at, b) => (*at, blocks.id_of(*b).to_owned(), true),
                Event::Placed(at, b) => (*at, blocks.id_of(*b).to_owned(), false),
            })
            .collect()
    };
    let shared = RefCell::new(&mut *world);
    let result = lua.scope(|scope| {
        let g = lua.globals();
        g.set(
            "log",
            scope.create_function(|_, msg: String| {
                info!("[script] {msg}");
                Ok(())
            })?,
        )?;
        g.set(
            "show_text",
            scope.create_function(|_, (msg, secs): (String, Option<f32>)| {
                let mut w = shared.borrow_mut();
                w.resource_mut::<Toast>()
                    .show(msg, secs.unwrap_or(3.0).max(0.5));
                Ok(())
            })?,
        )?;
        g.set(
            "get_block",
            scope.create_function(|_, (x, y, z): (f64, f64, f64)| {
                let w = shared.borrow();
                let id = w.resource::<Map>().block(cell(x, y, z));
                Ok(w.resource::<Registry<BlockDef>>().id_of(id).to_owned())
            })?,
        )?;
        g.set(
            "set_block",
            scope.create_function(|_, (x, y, z, id): (f64, f64, f64, String)| {
                let mut w = shared.borrow_mut();
                let Some(block) = w.resource::<Registry<BlockDef>>().idx(&id) else {
                    warn!("script: unknown block '{id}'");
                    return Ok(false);
                };
                Ok(w.resource_mut::<Map>().set(cell(x, y, z), block))
            })?,
        )?;
        g.set(
            "give_item",
            scope.create_function(|_, (id, n): (String, Option<u32>)| {
                let mut w = shared.borrow_mut();
                let n = n.unwrap_or(1);
                let left = w.resource_scope(|world: &mut World, items: Mut<Registry<ItemDef>>| {
                    let Some(item) = items.idx(&id) else {
                        warn!("script: unknown item '{id}'");
                        return n;
                    };
                    world
                        .query_filtered::<&mut Inventory, With<Player>>()
                        .iter_mut(world)
                        .next()
                        .map_or(n, |mut inv| inv.add(item, n, &items))
                });
                Ok(left)
            })?,
        )?;
        g.set(
            "count_item",
            scope.create_function(|_, id: String| {
                let mut w = shared.borrow_mut();
                let world: &mut World = &mut w;
                let Some(item) = world.resource::<Registry<ItemDef>>().idx(&id) else {
                    return Ok(0);
                };
                Ok(world
                    .query_filtered::<&Inventory, With<Player>>()
                    .iter(world)
                    .next()
                    .map_or(0, |inv| inv.count(item)))
            })?,
        )?;
        g.set(
            "take_item",
            scope.create_function(|_, (id, n): (String, Option<u32>)| {
                let mut w = shared.borrow_mut();
                let world: &mut World = &mut w;
                let Some(item) = world.resource::<Registry<ItemDef>>().idx(&id) else {
                    return Ok(false);
                };
                Ok(world
                    .query_filtered::<&mut Inventory, With<Player>>()
                    .iter_mut(world)
                    .next()
                    .is_some_and(|mut inv| inv.remove(item, n.unwrap_or(1))))
            })?,
        )?;
        g.set(
            "sort_inventory",
            scope.create_function(|_, ()| {
                let mut w = shared.borrow_mut();
                w.resource_scope(|world: &mut World, items: Mut<Registry<ItemDef>>| {
                    if let Some(mut inv) = world
                        .query_filtered::<&mut Inventory, With<Player>>()
                        .iter_mut(world)
                        .next()
                    {
                        // The hotbar is the player's own arrangement; only the bag is sorted.
                        sort_slots(&mut inv.slots[HOTBAR..], &items);
                    }
                });
                Ok(())
            })?,
        )?;
        g.set(
            "player_pos",
            scope.create_function(|_, ()| {
                let mut w = shared.borrow_mut();
                let world: &mut World = &mut w;
                let p = world
                    .query::<&Player>()
                    .iter(world)
                    .next()
                    .map_or(Vec3::ZERO, |p| p.body.pos);
                Ok((p.x, p.y, p.z))
            })?,
        )?;
        g.set(
            "key_just_pressed",
            scope.create_function(|_, name: String| {
                let w = shared.borrow();
                Ok(parse_key(&name)
                    .is_some_and(|k| w.resource::<ButtonInput<KeyCode>>().just_pressed(k)))
            })?,
        )?;
        g.set(
            "screenshot",
            scope.create_function(|_, name: String| {
                crate::debug::screenshot(&mut shared.borrow_mut(), &name);
                Ok(())
            })?,
        )?;
        g.set(
            "snapshot",
            scope.create_function(|_, name: String| {
                Ok(crate::debug::snapshot(&mut shared.borrow_mut(), &name, 2.0).is_ok())
            })?,
        )?;
        g.set(
            "dump_state",
            scope.create_function(|_, name: String| {
                crate::debug::dump(&mut shared.borrow_mut(), &name);
                Ok(())
            })?,
        )?;
        g.set(
            "append_file",
            scope.create_function(|_, (name, line): (String, String)| {
                crate::debug::append(&name, &line);
                Ok(())
            })?,
        )?;

        for (name, src) in pending.drain(..) {
            // Each script sees the globals through its own table, so two mods can both
            // define `on_tick`.
            let env = lua.create_table()?;
            let meta = lua.create_table()?;
            meta.set("__index", g.clone())?;
            env.set_metatable(Some(meta))?;
            match lua
                .load(&src)
                .set_name(&name)
                .set_environment(env.clone())
                .exec()
            {
                Ok(()) => {
                    let script = Script { name, env };
                    call(&script, "on_script_loaded", (), &mut reported);
                    scripts.push(script);
                }
                Err(e) => error!("script {name}: {e}"),
            }
        }
        for script in &scripts {
            call(script, "on_tick", dt, &mut reported);
        }
        for (at, id, broken) in &names {
            let hook = if *broken {
                "on_block_broken"
            } else {
                "on_block_placed"
            };
            for script in &scripts {
                call(script, hook, (at.x, at.y, at.z, id.as_str()), &mut reported);
            }
        }
        Ok(())
    });
    if let Err(e) = result {
        error!("script runtime: {e}");
    }
    world.insert_non_send(Vm {
        lua,
        scripts,
        pending,
        reported,
    });
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScriptSet;

pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptEvents>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (collect, run)
                    .chain()
                    .in_set(ScriptSet)
                    .after(crate::interact::InteractSet)
                    .before(crate::world::WorldSet),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Chunk, empty_blocks};

    /// A world with one empty chunk, a stone block kind, and a player.
    fn world(scripts: &[(&str, &str)]) -> World {
        let mut world = World::new();
        let mut blocks = Registry::<BlockDef>::default();
        blocks.insert(BlockDef::air());
        blocks.insert(ron::from_str(r#"(id: "stone", color: (1, 2, 3))"#).unwrap());
        let mut items = Registry::<ItemDef>::default();
        items.insert(ron::from_str(r#"(id: "stone")"#).unwrap());
        let mut map = Map::default();
        map.chunks.insert(
            IVec2::ZERO,
            Chunk {
                blocks: empty_blocks(),
                dirty: false,
                modified: false,
            },
        );
        world.insert_resource(blocks);
        world.insert_resource(items);
        world.insert_resource(map);
        world.init_resource::<Toast>();
        world.init_resource::<Time>();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<ScriptEvents>();
        world.spawn((Player::at(Vec3::new(1.5, 2.5, 3.0)), Inventory::default()));
        world.insert_non_send(Vm::new(
            scripts
                .iter()
                .map(|(n, s)| (n.to_string(), s.to_string()))
                .collect(),
        ));
        world
    }

    fn stone_count(world: &mut World) -> u32 {
        world.query::<&Inventory>().single(world).unwrap().count(0)
    }

    #[test]
    fn scripts_read_and_write_the_world_at_once() {
        let mut world = world(&[(
            "a.lua",
            r#"
            function on_script_loaded()
                set_block(1, 2, 3, "stone")
            end
            function on_tick(dt)
                -- The write above is visible here, in the same frame it was made.
                if get_block(1, 2, 3) == "stone" then
                    give_item("stone", 5)
                    set_block(1, 2, 3, "air")
                    -- Taking is all or nothing.
                    if take_item("stone", 9) or not take_item("stone", 3) then
                        error("take_item is wrong")
                    end
                    if count_item("stone") ~= 2 then
                        error("count_item is wrong")
                    end
                    sort_inventory()
                end
            end
            function on_block_broken(x, y, z, id)
                local px, py, pz = player_pos()
                show_text(id .. " at " .. x .. "," .. y .. "," .. z .. " from " .. px, 2)
            end
            "#,
        )]);
        run(&mut world);
        assert_eq!(world.resource::<Map>().block(IVec3::new(1, 2, 3)), 0);
        assert_eq!(stone_count(&mut world), 2);
        // A second frame does not repeat the load hook or the gift.
        world
            .resource_mut::<ScriptEvents>()
            .0
            .push(Event::Broken(IVec3::new(4, 5, 6), 1));
        run(&mut world);
        assert_eq!(stone_count(&mut world), 2);
        assert_eq!(world.resource::<Toast>().text, "stone at 4,5,6 from 1.5");
    }

    #[test]
    fn scripts_are_isolated_and_sandboxed() {
        let mut world = world(&[
            ("a.lua", r#"function on_tick() give_item("stone", 1) end"#),
            ("b.lua", r#"function on_tick() give_item("stone", 10) end"#),
            (
                "c.lua",
                r#"if io == nil and os == nil and require == nil then give_item("stone", 100) end"#,
            ),
            ("broken.lua", "this is not lua"),
            ("d.lua", r#"function on_tick() error("boom") end"#),
        ]);
        run(&mut world);
        // Both on_tick hooks ran, the sandbox check passed, and bad scripts hurt nobody.
        assert_eq!(stone_count(&mut world), 111);
        run(&mut world);
        assert_eq!(stone_count(&mut world), 122);
    }
}
