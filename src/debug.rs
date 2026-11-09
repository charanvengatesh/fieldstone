//! Debug probes for scripts: screenshots, world-state dumps and text logs written under
//! `debug/` in the user directory. Nothing here runs unless a script asks for it; the
//! `mods/debug` pack is the stock client.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::PrimaryWindow;

use crate::combat::ai::Ai;
use crate::core::settings::Settings;
use crate::core::time::GameTime;
use crate::core::{CreatureDef, ItemDef, ObjectDef, Registry, TerrainDef};
use crate::items::container::{Containers, OpenContainer};
use crate::items::interact::Target;
use crate::items::inventory::{Inventory, Stack};
use crate::player::{Cursor, HeldItem, Player};
use crate::stats::{Creature, Progress, Stats};
use crate::world::worldgen::WorldGen;
use crate::world::{ChunkMap, cell_of, chunk::chunk_of};

/// What scripts asked the probe for since the last frame.
pub enum Probe {
    Screenshot(String),
    Dump(String),
    Append(String, String),
}

static QUEUE: Mutex<Vec<Probe>> = Mutex::new(Vec::new());

pub fn push(p: Probe) {
    QUEUE.lock().unwrap().push(p);
}

/// `debug/` under the user directory, created on first use.
pub fn dir() -> PathBuf {
    let d = crate::core::paths::user_dir().join("debug");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Keeps file names inside `debug/`: one path segment of safe characters.
pub fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim_start_matches('.').to_owned();
    if s.is_empty() { "probe".to_owned() } else { s }
}

/// Exclusive so one system can read everything the report needs.
pub fn run(world: &mut World) {
    let queue: Vec<Probe> = std::mem::take(&mut *QUEUE.lock().unwrap());
    for p in queue {
        match p {
            Probe::Screenshot(name) => {
                let path = dir().join(format!("{}.png", safe_name(&name)));
                if world
                    .query_filtered::<Entity, With<PrimaryWindow>>()
                    .iter(world)
                    .next()
                    .is_none()
                {
                    warn!("debug: no window to screenshot");
                    continue;
                }
                // Reads the window's swapchain: the window must be on screen and the app
                // active. A game launched from a background shell on macOS never renders a
                // presentable frame and saves black images; launch it with `open` instead.
                info!("debug: screenshot {}", path.display());
                world
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
            }
            Probe::Dump(name) => {
                let path = dir().join(format!("{}.txt", safe_name(&name)));
                let text = report(world);
                match std::fs::write(&path, text) {
                    Ok(()) => info!("debug: dump {}", path.display()),
                    Err(e) => error!("debug: {}: {e}", path.display()),
                }
            }
            Probe::Append(name, line) => {
                let path = dir().join(safe_name(&name));
                use std::io::Write;
                let r = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .and_then(|mut f| writeln!(f, "{line}"));
                if let Err(e) = r {
                    error!("debug: {}: {e}", path.display());
                }
            }
        }
    }
}

const HALF_W: i32 = 20;
const HALF_H: i32 = 10;

/// A plain-text picture of the game right now: clocks, player, inventory, creatures,
/// chests and three ASCII maps around the player.
pub fn report(world: &mut World) -> String {
    // Queries first (they need `&mut World`), then plain resource reads.
    let window = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .map(|w| (w.width(), w.height(), w.scale_factor()));
    let camera = world
        .query_filtered::<&Projection, With<Camera2d>>()
        .iter(world)
        .next()
        .and_then(|p| match p {
            Projection::Orthographic(o) => Some((o.scale, o.area.size())),
            _ => None,
        });
    struct PlayerSnap {
        pos: Vec2,
        stats: [f32; 8],
        level: u32,
        xp: f32,
        next_xp: f32,
        skills: Vec<(String, f32)>,
        slots: Vec<Option<(u16, u32)>>,
        selected: usize,
        held: Option<u16>,
    }
    let held = world
        .query::<&HeldItem>()
        .iter(world)
        .next()
        .and_then(|h| h.0);
    let player = world
        .query_filtered::<(&Transform, &Stats, &Progress, &Inventory), With<Player>>()
        .iter(world)
        .next()
        .map(|(tf, s, p, inv)| PlayerSnap {
            pos: tf.translation.truncate(),
            stats: [
                s.hp,
                s.max_hp,
                s.stamina,
                s.max_stamina,
                s.food,
                s.attack,
                s.defense,
                s.speed,
            ],
            level: p.level,
            xp: p.xp,
            next_xp: p.next_level_xp(),
            skills: p.skills.clone(),
            slots: inv
                .slots
                .iter()
                .map(|s| s.as_ref().map(|s| (s.item, s.n)))
                .collect(),
            selected: inv.selected,
            held,
        });
    let creatures_snap: Vec<(Vec2, f32, f32, u16, Option<[f32; 5]>)> = world
        .query_filtered::<(&Transform, &Stats, &Creature, Option<&Ai>), Without<Player>>()
        .iter(world)
        .map(|(t, s, c, ai)| {
            (
                t.translation.truncate(),
                s.hp,
                s.max_hp,
                c.0,
                ai.map(|a| [a.dir.x, a.dir.y, a.timer, a.bite, a.calm]),
            )
        })
        .collect();
    let entities = world.query::<Entity>().iter(world).count();

    let mut out = String::new();
    let clock = world.resource::<GameTime>();
    let time = world.resource::<Time>();
    let settings = world.resource::<Settings>();
    let map = world.resource::<ChunkMap>();
    let terrain = world.resource::<Registry<TerrainDef>>();
    let objects = world.resource::<Registry<ObjectDef>>();
    let items = world.resource::<Registry<ItemDef>>();
    let creatures = world.resource::<Registry<CreatureDef>>();
    let containers = world.resource::<Containers>();
    let open = world.resource::<OpenContainer>();
    let target = world.resource::<Target>();
    let cursor = world.resource::<Cursor>();
    let seed = world.get_resource::<WorldGen>().map(|w| w.seed);
    let packs = world
        .get_resource::<crate::core::packs::LoadedPacks>()
        .map(|p| {
            p.0.iter()
                .map(|(_, m)| m.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let _ = writeln!(out, "fieldstone {} debug dump", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        out,
        "seed {:?}  day {}  frac {:.3} ({})  frame dt {:.1} ms  entities {}  chunks {}",
        seed,
        clock.day(),
        clock.day_frac(),
        if clock.day_frac() >= 0.5 {
            "night"
        } else {
            "day"
        },
        time.delta_secs() * 1000.0,
        entities,
        map.chunks.len(),
    );
    let _ = writeln!(
        out,
        "render {:?}  pixel_scale {}  zoom {}  packs {packs}",
        settings.render, settings.pixel_scale, settings.zoom
    );
    if let Some((w, h, sf)) = window {
        let _ = writeln!(out, "window {w}x{h} logical, scale factor {sf}");
    }
    if let Some((scale, size)) = camera {
        let _ = writeln!(out, "camera scale {scale}  view {size:?}");
    }

    let Some(pl) = player else {
        out.push_str("no player\n");
        return out;
    };
    let pos = pl.pos;
    let cell = cell_of(pos);
    let under = map.tile(cell);
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "player at {:.1},{:.1}  cell {}  chunk {}  height {}  terrain {}  indoors {}",
        pos.x,
        pos.y,
        cell,
        chunk_of(cell),
        under.map_or(0, |t| t.height),
        under.map_or("?", |t| terrain.id_of(t.terrain)),
        crate::world::indoors(map, terrain, objects, cell),
    );
    let [
        hp,
        max_hp,
        stamina,
        max_stamina,
        food,
        attack,
        defense,
        speed,
    ] = pl.stats;
    let _ = writeln!(
        out,
        "hp {hp:.1}/{max_hp:.0}  stamina {stamina:.1}/{max_stamina:.0}  food {food:.1}  atk {attack}  def {defense}  speed {speed}"
    );
    let skills: Vec<String> = pl
        .skills
        .iter()
        .map(|(k, v)| format!("{k} {v:.1}"))
        .collect();
    let _ = writeln!(
        out,
        "level {}  xp {:.1}/{:.0}  skills [{}]",
        pl.level,
        pl.xp,
        pl.next_xp,
        skills.join(", ")
    );
    let name = |s: &Option<Stack>| {
        s.as_ref().map_or("-".to_owned(), |s| {
            format!("{} x{}", items.id_of(s.item), s.n)
        })
    };
    let _ = writeln!(
        out,
        "held {:?}  selected slot {}",
        pl.held.map(|i| items.id_of(i)),
        pl.selected
    );
    for (i, s) in pl.slots.iter().enumerate() {
        if let Some((item, n)) = s {
            let _ = writeln!(out, "  slot {i:2}: {} x{n}", items.id_of(*item));
        }
    }
    let object_at = |c: Option<IVec2>| {
        c.and_then(|c| map.tile(c))
            .and_then(|t| t.object)
            .map(|o| objects.id_of(o))
    };
    let _ = writeln!(
        out,
        "target {:?} -> {:?}   cursor cell {:?} -> {:?}   open chest {:?}",
        target.0,
        object_at(target.0),
        cursor.cell,
        object_at(cursor.cell),
        open.0
    );

    // Creatures nearby.
    let mut near: Vec<(f32, String)> = creatures_snap
        .iter()
        .filter_map(|(p, hp, max_hp, kind, ai)| {
            let d = p.distance(pos) / crate::world::CELL;
            (d <= 24.0).then(|| {
                let ai = ai.map_or(String::new(), |[dx, dy, timer, bite, calm]| {
                    format!("  dir {dx:.2},{dy:.2} timer {timer:.1} bite {bite:.1} calm {calm:.1}")
                });
                (
                    d,
                    format!(
                        "{:10} cell {:>14}  hp {hp:.1}/{max_hp:.0}  dist {d:.1}{ai}",
                        creatures.id_of(*kind),
                        cell_of(*p).to_string(),
                    ),
                )
            })
        })
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let _ = writeln!(out, "\ncreatures within 24 cells: {}", near.len());
    for (_, line) in near {
        let _ = writeln!(out, "  {line}");
    }

    // Chests nearby.
    let mut chests: Vec<(IVec2, &Vec<Option<Stack>>)> = containers
        .0
        .iter()
        .filter(|(c, _)| (**c - cell).abs().max_element() <= HALF_W)
        .map(|(c, s)| (*c, s))
        .collect();
    chests.sort_by_key(|(c, _)| (c.y, c.x));
    let _ = writeln!(out, "\nchests within {HALF_W} cells: {}", chests.len());
    for (c, slots) in chests {
        let full: Vec<String> = slots.iter().filter(|s| s.is_some()).map(&name).collect();
        let _ = writeln!(out, "  {c}: [{}]", full.join(", "));
    }

    // Maps. Object glyphs are assigned in id order among the objects actually on the map,
    // so a legend never repeats a letter.
    let mut creature_cells: Vec<(i32, i32)> = creatures_snap
        .iter()
        .map(|(p, ..)| {
            let c = cell_of(*p);
            (c.x, c.y)
        })
        .collect();
    creature_cells.sort_unstable();
    let mut seen: Vec<u16> = Vec::new();
    for dy in -HALF_H..=HALF_H {
        for dx in -HALF_W..=HALF_W {
            if let Some(o) = map.tile(cell + IVec2::new(dx, dy)).and_then(|t| t.object)
                && !objects.at(o).is_part()
                && !seen.contains(&o)
            {
                seen.push(o);
            }
        }
    }
    seen.sort_unstable();
    let letters = b"abcdefghijklmnopqrstuvwxyz";
    let caps = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let terrain_glyph = |t: u16| letters[t as usize % letters.len()] as char;
    let object_glyph = |o: u16| {
        seen.iter()
            .position(|s| *s == o)
            .map_or('?', |i| caps[i % caps.len()] as char)
    };
    let _ = writeln!(
        out,
        "\nmap {}x{} around the player (north up): objects as capitals, terrain lowercase, @ player, * creature, + cursor, # footprint part, ? unloaded",
        HALF_W * 2 + 1,
        HALF_H * 2 + 1
    );
    let mut heights = String::new();
    for dy in (-HALF_H..=HALF_H).rev() {
        let mut row = String::new();
        let mut hrow = String::new();
        for dx in -HALF_W..=HALF_W {
            let c = cell + IVec2::new(dx, dy);
            let t = map.tile(c);
            let ch = if c == cell {
                '@'
            } else if creature_cells.binary_search(&(c.x, c.y)).is_ok() {
                '*'
            } else if cursor.cell == Some(c) {
                '+'
            } else {
                match t {
                    None => '?',
                    Some(t) => match t.object {
                        Some(o) if !objects.at(o).is_part() => object_glyph(o),
                        Some(_) => '#',
                        None => terrain_glyph(t.terrain),
                    },
                }
            };
            row.push(ch);
            hrow.push(t.map_or('?', |t| char::from(b'0' + t.height)));
        }
        let _ = writeln!(out, "{row}");
        let _ = writeln!(heights, "{hrow}");
    }
    let _ = writeln!(out, "\nheights:\n{heights}");
    let _ = writeln!(out, "terrain legend:");
    for (i, t) in terrain.iter() {
        let _ = writeln!(out, "  {} {}", terrain_glyph(i), t.id);
    }
    let _ = writeln!(out, "object legend (ids on the map):");
    for o in &seen {
        let _ = writeln!(out, "  {} {}", object_glyph(*o), objects.id_of(*o));
    }
    out
}

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, run.after(crate::script::ScriptSet));
    }
}
