//! Save files are RON: seed, clock, player, inventory, and only the chunks that differ from
//! generation. Tiles are stored through a per-file id table so saves survive pack reordering.

use std::fs;
use std::path::Path;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::time::GameTime;
use super::{ItemDef, ObjectDef, Registry, TerrainDef};
use crate::items::inventory::{Inventory, Stack};
use crate::player::Player;
use crate::stats::{Progress, Stats};
use crate::world::chunk::{TILES, Tile};
use crate::world::{ChunkMap, worldgen::WorldGen};

pub const SAVE_PATH: &str = "saves/world.ron";

/// Modified chunks that are currently unloaded (or were loaded from disk and not yet streamed in).
#[derive(Resource, Default)]
pub struct SavedChunks(pub HashMap<IVec2, [Tile; TILES]>);

#[derive(Serialize, Deserialize, Default)]
struct SaveFile {
    seed: u32,
    time: GameTime,
    player: (f32, f32),
    inventory: Vec<Option<(String, u32)>>,
    #[serde(default)]
    hp: Option<f32>,
    /// `(xp, level, skills)`.
    #[serde(default)]
    progress: Option<(f32, u32, Vec<(String, f32)>)>,
    /// String ids referenced by `chunks`; index 0 is reserved for "no object".
    ids: Vec<String>,
    /// Chunk coordinate and, per tile, `(terrain id index, object id index)`.
    chunks: Vec<((i32, i32), Vec<(u16, u16)>)>,
}

pub fn save(
    keys: Res<ButtonInput<KeyCode>>,
    wg: Res<WorldGen>,
    time: Res<GameTime>,
    map: Res<ChunkMap>,
    saved: Res<SavedChunks>,
    terrain: Res<Registry<TerrainDef>>,
    objects: Res<Registry<ObjectDef>>,
    items: Res<Registry<ItemDef>>,
    player: Single<(&Transform, &Inventory, &Stats, &Progress), With<Player>>,
) {
    if !keys.just_pressed(KeyCode::F5) {
        return;
    }
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
    }
    let file = SaveFile {
        seed: wg.seed,
        time: *time,
        player: (tf.translation.x, tf.translation.y),
        inventory: inv
            .slots
            .iter()
            .map(|s| s.map(|s| (items.id_of(s.item).to_owned(), s.n)))
            .collect(),
        hp: Some(stats.hp),
        progress: Some((progress.xp, progress.level, progress.skills.clone())),
        ids,
        chunks,
    };
    let result = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
        .map_err(|e| e.to_string())
        .and_then(|text| {
            fs::create_dir_all(Path::new(SAVE_PATH).parent().unwrap())
                .map_err(|e| e.to_string())?;
            fs::write(SAVE_PATH, text).map_err(|e| e.to_string())
        });
    match result {
        Ok(()) => info!("saved to {SAVE_PATH}"),
        Err(e) => error!("save failed: {e}"),
    }
}

/// Seed and saved state read before the world is built, if a save exists.
pub struct Loaded {
    pub seed: u32,
    pub time: GameTime,
    pub player: Vec2,
    pub inventory: Vec<Option<(String, u32)>>,
    pub hp: Option<f32>,
    pub progress: Option<(f32, u32, Vec<(String, f32)>)>,
    pub chunks: SavedChunks,
}

pub fn load(terrain: &Registry<TerrainDef>, objects: &Registry<ObjectDef>) -> Option<Loaded> {
    let text = fs::read_to_string(SAVE_PATH).ok()?;
    let file: SaveFile = match ron::from_str(&text) {
        Ok(f) => f,
        Err(e) => {
            error!("{SAVE_PATH}: {e}; starting a new world");
            return None;
        }
    };
    let fallback_terrain = 0;
    let mut chunks = HashMap::default();
    for ((x, y), tiles) in file.chunks {
        let mut arr = [Tile::default(); TILES];
        for (slot, (t, o)) in arr.iter_mut().zip(tiles) {
            let name = |i: u16| file.ids.get(i as usize).map(String::as_str).unwrap_or("");
            slot.terrain = terrain.idx(name(t)).unwrap_or(fallback_terrain);
            slot.object = (o != 0).then(|| objects.idx(name(o))).flatten();
        }
        chunks.insert(IVec2::new(x, y), arr);
    }
    info!("loaded {SAVE_PATH}");
    Some(Loaded {
        seed: file.seed,
        time: file.time,
        player: Vec2::new(file.player.0, file.player.1),
        inventory: file.inventory,
        hp: file.hp,
        progress: file.progress,
        chunks: SavedChunks(chunks),
    })
}

/// Applies the saved player position and inventory once the player exists.
pub fn restore_player(
    mut commands: Commands,
    pending: Option<Res<PendingPlayer>>,
    items: Res<Registry<ItemDef>>,
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
    commands.remove_resource::<PendingPlayer>();
}

#[derive(Resource)]
pub struct PendingPlayer {
    pub pos: Vec2,
    pub inventory: Vec<Option<(String, u32)>>,
    pub hp: Option<f32>,
    pub progress: Option<(f32, u32, Vec<(String, f32)>)>,
}
