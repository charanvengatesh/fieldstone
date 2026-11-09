//! Save files are RON: seed, clock, player, inventory, and only the chunks that differ from
//! generation. Tiles are stored through a per-file id table so saves survive pack reordering.

use std::fs;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::settings::Keys;
use super::time::GameTime;
use super::{ItemDef, ObjectDef, Registry, TerrainDef};
use crate::items::container::Containers;
use crate::items::inventory::{Inventory, Stack};
use crate::player::Player;
use crate::stats::{Progress, Stats};
use crate::world::chunk::{TILES, Tile};
use crate::world::{ChunkMap, worldgen::WorldGen};

/// Set by the pause menu: `.0` save now, `.1` exit after saving.
#[derive(Resource, Default)]
pub struct SaveRequest(pub bool, pub bool);

/// Modified chunks that are currently unloaded (or were loaded from disk and not yet streamed in).
#[derive(Resource, Default)]
pub struct SavedChunks(pub HashMap<IVec2, [Tile; TILES]>);

#[derive(Serialize, Deserialize, Default)]
struct SaveFile {
    seed: u32,
    /// World units per cell when this file was written; positions are rescaled on load.
    #[serde(default = "legacy_cell")]
    cell_size: f32,
    time: GameTime,
    player: (f32, f32),
    inventory: Vec<Option<(String, u32)>>,
    #[serde(default)]
    hp: Option<f32>,
    #[serde(default)]
    food: Option<f32>,
    /// `(xp, level, skills)`.
    #[serde(default)]
    progress: Option<(f32, u32, Vec<(String, f32)>)>,
    /// Chest contents: cell and slots as `(item id, count)`.
    #[serde(default)]
    containers: Vec<((i32, i32), Vec<Option<(String, u32)>>)>,
    /// Bed respawn point.
    #[serde(default)]
    spawn: Option<(f32, f32)>,
    /// Ending message once the shrine is complete.
    #[serde(default)]
    ending: Option<String>,
    /// Crop timers: cell and game-time due.
    #[serde(default)]
    growth: Vec<((i32, i32), f64)>,
    /// String ids referenced by `chunks`; index 0 is reserved for "no object".
    ids: Vec<String>,
    /// Chunk coordinate and, per tile, `(terrain id index, object id index)`.
    chunks: Vec<((i32, i32), Vec<(u16, u16)>)>,
    /// Per saved chunk, the terrace heights (same order as `chunks`); empty for old saves.
    #[serde(default)]
    heights: Vec<Vec<u8>>,
}

pub fn save(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    mut request: ResMut<SaveRequest>,
    containers: Res<Containers>,
    spawn: Res<crate::stats::SpawnPoint>,
    ending: Res<super::Ending>,
    growth: Res<crate::farm::Growth>,
    mut exit: MessageWriter<AppExit>,
    wg: Res<WorldGen>,
    time: Res<GameTime>,
    map: Res<ChunkMap>,
    saved: Res<SavedChunks>,
    terrain: Res<Registry<TerrainDef>>,
    objects: Res<Registry<ObjectDef>>,
    items: Res<Registry<ItemDef>>,
    player: Single<(&Transform, &Inventory, &Stats, &Progress), With<Player>>,
) {
    if !keys.just(&input, &keys.save) && !request.0 {
        return;
    }
    request.0 = false;
    let (tf, inv, stats, progress) = *player;
    let mut ids: Vec<String> = vec![String::new()];
    let mut intern = |s: &str| -> u16 {
        match ids.iter().position(|i| i == s) {
            Some(i) => i as u16,
            None => {
                ids.push(s.to_owned());
                (ids.len() - 1) as u16
            }
        }
    };
    let mut chunks = Vec::new();
    let mut heights = Vec::new();
    let loaded = map
        .chunks
        .iter()
        .filter(|(_, c)| c.modified)
        .map(|(p, c)| (*p, &c.tiles));
    for (pos, tiles) in loaded.chain(saved.0.iter().map(|(p, t)| (*p, t))) {
        let encoded = tiles
            .iter()
            .map(|t| {
                (
                    intern(terrain.id_of(t.terrain)),
                    t.object.map_or(0, |o| intern(objects.id_of(o))),
                )
            })
            .collect();
        chunks.push(((pos.x, pos.y), encoded));
        heights.push(tiles.iter().map(|t| t.height).collect());
    }
    let file = SaveFile {
        seed: wg.seed,
        cell_size: crate::world::CELL,
        time: *time,
        player: (tf.translation.x, tf.translation.y),
        inventory: inv
            .slots
            .iter()
            .map(|s| s.map(|s| (items.id_of(s.item).to_owned(), s.n)))
            .collect(),
        hp: Some(stats.hp),
        food: Some(stats.food),
        progress: Some((progress.xp, progress.level, progress.skills.clone())),
        containers: containers
            .0
            .iter()
            .map(|(c, slots)| {
                (
                    (c.x, c.y),
                    slots
                        .iter()
                        .map(|s| s.map(|s| (items.id_of(s.item).to_owned(), s.n)))
                        .collect(),
                )
            })
            .collect(),
        spawn: Some((spawn.0.x, spawn.0.y)),
        ending: ending.0.clone(),
        growth: growth.0.iter().map(|(c, t)| ((c.x, c.y), *t)).collect(),
        ids,
        chunks,
        heights,
    };
    let path = super::paths::save_file();
    let result = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
        .map_err(|e| e.to_string())
        .and_then(|text| {
            fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::write(&path, text).map_err(|e| e.to_string())
        });
    match result {
        Ok(()) => info!("saved to {}", path.display()),
        Err(e) => error!("save failed: {e}"),
    }
    if request.1 {
        request.1 = false;
        exit.write(AppExit::Success);
    }
}

/// Seed and saved state read before the world is built, if a save exists.
pub struct Loaded {
    pub seed: u32,
    pub time: GameTime,
    pub player: Vec2,
    pub inventory: Vec<Option<(String, u32)>>,
    pub hp: Option<f32>,
    pub food: Option<f32>,
    pub progress: Option<(f32, u32, Vec<(String, f32)>)>,
    pub containers: Vec<((i32, i32), Vec<Option<(String, u32)>>)>,
    pub spawn: Option<Vec2>,
    pub ending: Option<String>,
    pub growth: Vec<(IVec2, f64)>,
    pub chunks: SavedChunks,
}

pub fn load(terrain: &Registry<TerrainDef>, objects: &Registry<ObjectDef>) -> Option<Loaded> {
    let path = super::paths::save_file();
    let text = fs::read_to_string(&path).ok()?;
    let file: SaveFile = match ron::from_str(&text) {
        Ok(f) => f,
        Err(e) => {
            error!("{}: {e}; starting a new world", path.display());
            return None;
        }
    };
    let fallback_terrain = 0;
    let mut chunks = HashMap::default();
    for (k, ((x, y), tiles)) in file.chunks.into_iter().enumerate() {
        let mut arr = [Tile::default(); TILES];
        if let Some(hs) = file.heights.get(k) {
            for (slot, h) in arr.iter_mut().zip(hs) {
                slot.height = *h;
            }
        }
        for (slot, (t, o)) in arr.iter_mut().zip(tiles) {
            let name = |i: u16| file.ids.get(i as usize).map(String::as_str).unwrap_or("");
            slot.terrain = terrain.idx(name(t)).unwrap_or(fallback_terrain);
            slot.object = (o != 0).then(|| objects.idx(name(o))).flatten();
        }
        chunks.insert(IVec2::new(x, y), arr);
    }
    info!("loaded {}", path.display());
    let scale = crate::world::CELL / file.cell_size;
    Some(Loaded {
        seed: file.seed,
        time: file.time,
        player: Vec2::new(file.player.0, file.player.1) * scale,
        inventory: file.inventory,
        hp: file.hp,
        food: file.food,
        progress: file.progress,
        containers: file.containers,
        spawn: file.spawn.map(|(x, y)| Vec2::new(x, y) * scale),
        ending: file.ending,
        growth: file
            .growth
            .into_iter()
            .map(|((x, y), t)| (IVec2::new(x, y), t))
            .collect(),
        chunks: SavedChunks(chunks),
    })
}

/// Applies the saved player position and inventory once the player exists.
pub fn restore_player(
    mut commands: Commands,
    pending: Option<Res<PendingPlayer>>,
    items: Res<Registry<ItemDef>>,
    mut containers: ResMut<Containers>,
    mut spawn_point: ResMut<crate::stats::SpawnPoint>,
    mut ending: ResMut<super::Ending>,
    mut growth: ResMut<crate::farm::Growth>,
    mut player: Single<(&mut Transform, &mut Inventory, &mut Stats, &mut Progress), With<Player>>,
) {
    let Some(p) = pending else {
        return;
    };
    let (tf, inv, stats, progress) = &mut *player;
    tf.translation = p.pos.extend(tf.translation.z);
    for (slot, saved) in inv.slots.iter_mut().zip(&p.inventory) {
        *slot = saved
            .as_ref()
            .and_then(|(id, n)| items.idx(id).map(|item| Stack { item, n: *n }));
    }
    if let Some((xp, level, skills)) = &p.progress {
        progress.xp = *xp;
        progress.level = *level;
        progress.skills = skills.clone();
        // Replay level bonuses so stats match the saved level.
        stats.max_hp += 5.0 * *level as f32;
        stats.attack += *level as f32;
    }
    stats.hp = p.hp.unwrap_or(stats.max_hp).min(stats.max_hp);
    stats.food = p.food.unwrap_or(crate::stats::FOOD_START);
    for ((x, y), slots) in &p.containers {
        containers.0.insert(
            IVec2::new(*x, *y),
            slots
                .iter()
                .map(|s| {
                    s.as_ref()
                        .and_then(|(id, n)| items.idx(id).map(|item| Stack { item, n: *n }))
                })
                .collect(),
        );
    }
    if let Some(sp) = p.spawn {
        spawn_point.0 = sp;
    }
    if p.ending.is_some() {
        ending.0 = p.ending.clone();
    }
    for (c, t) in &p.growth {
        growth.0.insert(*c, *t);
    }
    commands.remove_resource::<PendingPlayer>();
}

fn legacy_cell() -> f32 {
    25.0
}

#[derive(Resource)]
pub struct PendingPlayer {
    pub pos: Vec2,
    pub inventory: Vec<Option<(String, u32)>>,
    pub hp: Option<f32>,
    pub food: Option<f32>,
    pub progress: Option<(f32, u32, Vec<(String, f32)>)>,
    pub containers: Vec<((i32, i32), Vec<Option<(String, u32)>>)>,
    pub spawn: Option<Vec2>,
    pub ending: Option<String>,
    pub growth: Vec<(IVec2, f64)>,
}
