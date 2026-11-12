//! The save file: seed, player, inventory, and only the chunks that differ from what the
//! generator would make. Blocks are written as runs through a per-file id table, so a save
//! survives packs being added, removed or reordered.

use std::fs;
use std::path::{Path, PathBuf};

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use serde::{Deserialize, Serialize};

use super::settings::Keys;
use super::time::GameTime;
use super::{BlockDef, ItemDef, Registry, SaveRequest, Toast};
use crate::cam::Cam;
use crate::inventory::{Inventory, Stack};
use crate::player::Player;
use crate::world::worldgen::WorldGen;
use crate::world::{AIR, Blocks, Map, VOLUME, empty_blocks};

pub const VERSION: u32 = 2;

/// Where this run reads and writes its save.
#[derive(Resource)]
pub struct SavePath(pub PathBuf);

#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct SaveFile {
    pub version: u32,
    pub seed: u32,
    pub player: (f32, f32, f32),
    pub rot: u8,
    /// Tilt preset name from before 1.0; read as a pitch preset when `pitch` is absent.
    #[serde(default)]
    pub tilt: String,
    /// Camera yaw and pitch in radians, and the projection; older files have none.
    #[serde(default)]
    pub yaw: Option<f32>,
    #[serde(default)]
    pub pitch: Option<f32>,
    #[serde(default)]
    pub perspective: bool,
    /// Game clock in seconds; older files have none and start at the default morning.
    #[serde(default)]
    pub time: f64,
    pub selected: usize,
    /// Slots as `(item id, count)`.
    pub inventory: Vec<Option<(String, u32)>>,
    /// Block ids referenced by `chunks`.
    pub ids: Vec<String>,
    /// Chunk coordinate and its blocks as `(index into ids, run length)`.
    pub chunks: Vec<((i32, i32), Vec<(u16, u32)>)>,
}

/// A save resolved against the current registries.
pub struct Loaded {
    pub seed: u32,
    pub player: Vec3,
    pub rot: u8,
    pub tilt: String,
    pub yaw: Option<f32>,
    pub pitch: Option<f32>,
    pub perspective: bool,
    pub time: f64,
    pub selected: usize,
    pub inventory: Vec<Option<Stack>>,
    pub chunks: HashMap<IVec2, Blocks>,
}

/// What a loaded save hands the player when it spawns.
#[derive(Resource, Default)]
pub struct Pending {
    pub inventory: Vec<Option<Stack>>,
    pub selected: usize,
}

/// Collapses a chunk into `(value, run length)` pairs.
pub fn encode(blocks: &[u16], mut map: impl FnMut(u16) -> u16) -> Vec<(u16, u32)> {
    let mut runs: Vec<(u16, u32)> = Vec::new();
    for b in blocks {
        let v = map(*b);
        match runs.last_mut() {
            Some((last, n)) if *last == v => *n += 1,
            _ => runs.push((v, 1)),
        }
    }
    runs
}

/// Expands runs back into a chunk; `None` when they do not add up to exactly one chunk.
pub fn decode(runs: &[(u16, u32)], mut map: impl FnMut(u16) -> u16) -> Option<Blocks> {
    if runs.iter().map(|(_, n)| *n as usize).sum::<usize>() != VOLUME {
        return None;
    }
    let mut blocks = empty_blocks();
    let mut i = 0;
    for (v, n) in runs {
        let v = map(*v);
        blocks[i..i + *n as usize].fill(v);
        i += *n as usize;
    }
    Some(blocks)
}

/// Builds the file from live state. `chunks` yields every edited chunk.
pub fn pack<'a>(
    seed: u32,
    player: Vec3,
    cam: &Cam,
    time: f64,
    inv: &Inventory,
    chunks: impl Iterator<Item = (IVec2, &'a Blocks)>,
    blocks: &Registry<BlockDef>,
    items: &Registry<ItemDef>,
) -> SaveFile {
    let mut ids: Vec<String> = Vec::new();
    let mut table: HashMap<u16, u16> = HashMap::default();
    let mut chunks: Vec<((i32, i32), Vec<(u16, u32)>)> = chunks
        .map(|(pos, data)| {
            let runs = encode(&data[..], |b| {
                *table.entry(b).or_insert_with(|| {
                    ids.push(blocks.id_of(b).to_owned());
                    (ids.len() - 1) as u16
                })
            });
            ((pos.x, pos.y), runs)
        })
        .collect();
    chunks.sort_by_key(|(pos, _)| *pos);
    SaveFile {
        version: VERSION,
        seed,
        player: (player.x, player.y, player.z),
        rot: cam.rot(),
        tilt: cam.pitch_name().to_owned(),
        yaw: Some(cam.target_yaw),
        pitch: Some(cam.target_pitch),
        perspective: cam.perspective,
        time,
        selected: inv.selected,
        inventory: inv
            .slots
            .iter()
            .map(|s| s.map(|s| (items.id_of(s.item).to_owned(), s.n)))
            .collect(),
        ids,
        chunks,
    }
}

/// Resolves a file against the current registries. Unknown blocks become air and unknown
/// items are dropped, each with a warning, so removing a mod never bricks a save.
pub fn unpack(
    file: &SaveFile,
    blocks: &Registry<BlockDef>,
    items: &Registry<ItemDef>,
) -> Result<Loaded, String> {
    if file.version != VERSION {
        return Err(format!("save version {} is not {VERSION}", file.version));
    }
    let table: Vec<u16> = file
        .ids
        .iter()
        .map(|id| {
            blocks.idx(id).unwrap_or_else(|| {
                warn!("save: unknown block '{id}' becomes air");
                AIR
            })
        })
        .collect();
    let mut chunks = HashMap::default();
    for (pos, runs) in &file.chunks {
        let data = decode(runs, |i| table.get(i as usize).copied().unwrap_or(AIR))
            .ok_or_else(|| format!("chunk {pos:?} is malformed"))?;
        chunks.insert(IVec2::new(pos.0, pos.1), data);
    }
    Ok(Loaded {
        seed: file.seed,
        player: Vec3::new(file.player.0, file.player.1, file.player.2),
        rot: file.rot % 4,
        tilt: file.tilt.clone(),
        yaw: file.yaw,
        pitch: file.pitch,
        perspective: file.perspective,
        time: file.time,
        selected: file.selected,
        inventory: file
            .inventory
            .iter()
            .map(|slot| {
                let (id, n) = slot.as_ref()?;
                match items.idx(id) {
                    Some(item) => Some(Stack { item, n: *n }),
                    None => {
                        warn!("save: unknown item '{id}' dropped");
                        None
                    }
                }
            })
            .collect(),
        chunks,
    })
}

pub fn load(path: &Path, blocks: &Registry<BlockDef>, items: &Registry<ItemDef>) -> Option<Loaded> {
    let text = fs::read_to_string(path).ok()?;
    let file: SaveFile = match ron::from_str(&text) {
        Ok(f) => f,
        Err(e) => {
            error!("{}: {e}; starting a new world", path.display());
            return None;
        }
    };
    match unpack(&file, blocks, items) {
        Ok(l) => Some(l),
        Err(e) => {
            error!("{}: {e}; starting a new world", path.display());
            None
        }
    }
}

/// The write in flight, if any: the file is packed on the main thread (it reads the
/// map), then serialised and written on the IO pool, so a big save does not hold a frame.
#[derive(Resource, Default)]
pub struct Saving {
    task: Option<Task<Result<(usize, PathBuf), String>>>,
    /// Quit once the write lands.
    quit: bool,
}

/// Starts a save when the save key is pressed or the pause menu asks.
#[allow(clippy::too_many_arguments)]
pub fn save(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    path: Res<SavePath>,
    wg: Res<WorldGen>,
    cam: Res<Cam>,
    clock: Res<GameTime>,
    map: Res<Map>,
    blocks: Res<Registry<BlockDef>>,
    items: Res<Registry<ItemDef>>,
    player: Single<(&Player, &Inventory)>,
    mut request: ResMut<SaveRequest>,
    mut saving: ResMut<Saving>,
) {
    if !keys.just(&input, &keys.save) && !request.save {
        return;
    }
    if saving.task.is_some() {
        // One at a time; the request stands until the write in flight lands.
        request.save = true;
        return;
    }
    request.save = false;
    if request.quit {
        saving.quit = true;
    }
    let (player, inv) = *player;
    let parked: Vec<(IVec2, Blocks)> = map.parked.iter().map(|(p, b)| (*p, b.unpack())).collect();
    let edited = map
        .chunks
        .iter()
        .filter(|(_, c)| c.modified)
        .map(|(p, c)| (*p, &c.blocks))
        .chain(parked.iter().map(|(p, b)| (*p, b)));
    let file = pack(
        wg.seed,
        player.body.pos,
        &cam,
        clock.secs,
        inv,
        edited,
        &blocks,
        &items,
    );
    let path = path.0.clone();
    saving.task = Some(IoTaskPool::get().spawn(async move {
        let chunks = file.chunks.len();
        let text = ron::ser::to_string(&file).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        fs::write(&path, text).map_err(|e| e.to_string())?;
        Ok((chunks, path))
    }));
}

/// Reports a finished write, and exits if the save was the way out.
pub fn finish(
    mut saving: ResMut<Saving>,
    mut toast: ResMut<Toast>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(task) = saving.task.as_mut() else {
        return;
    };
    if !task.is_finished() {
        return;
    }
    let task = saving.task.take().expect("checked above");
    match block_on(poll_once(task)) {
        Some(Ok((chunks, path))) => {
            info!("saved {chunks} chunks to {}", path.display());
            toast.show("Saved.", 2.0);
        }
        Some(Err(e)) => {
            error!("could not save: {e}");
            toast.show("Could not save; see the log.", 4.0);
        }
        None => {}
    }
    if saving.quit {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regs(order: &[&str]) -> (Registry<BlockDef>, Registry<ItemDef>) {
        let mut blocks = Registry::<BlockDef>::default();
        blocks.insert(BlockDef::air());
        let mut items = Registry::<ItemDef>::default();
        for id in order {
            blocks.insert(ron::from_str(&format!(r#"(id: "{id}", color: (1, 2, 3))"#)).unwrap());
            items.insert(ron::from_str(&format!(r#"(id: "{id}")"#)).unwrap());
        }
        (blocks, items)
    }

    #[test]
    fn runs_round_trip() {
        let mut blocks = empty_blocks();
        blocks[..300].fill(2);
        blocks[300..305].fill(7);
        blocks[VOLUME - 1] = 9;
        let runs = encode(&blocks[..], |b| b);
        assert_eq!(runs.len(), 4);
        assert_eq!(decode(&runs, |b| b).unwrap()[..], blocks[..]);
        assert!(decode(&runs[..3], |b| b).is_none());
    }

    #[test]
    fn a_save_survives_a_different_registry_order() {
        let (blocks, items) = regs(&["stone", "dirt"]);
        let (stone, dirt) = (blocks.idx("stone").unwrap(), blocks.idx("dirt").unwrap());
        let mut chunk = empty_blocks();
        chunk[..512].fill(stone);
        chunk[512..600].fill(dirt);
        let mut inv = Inventory::default();
        inv.add(items.idx("dirt").unwrap(), 12, &items);
        inv.selected = 3;
        let file = pack(
            77,
            Vec3::new(1.5, -2.5, 9.0),
            &{
                let mut c = Cam::quarter(2);
                c.set_pitch(Cam::pitch_named("high").unwrap());
                c.perspective = true;
                c
            },
            1234.5,
            &inv,
            [(IVec2::new(-1, 4), &chunk)].into_iter(),
            &blocks,
            &items,
        );
        // Through text, as on disk.
        let text = ron::ser::to_string(&file).unwrap();
        let file: SaveFile = ron::from_str(&text).unwrap();
        // The next run loads packs in another order and has lost nothing.
        let (blocks2, items2) = regs(&["dirt", "sand", "stone"]);
        let loaded = unpack(&file, &blocks2, &items2).unwrap();
        assert_eq!((loaded.seed, loaded.rot, loaded.selected), (77, 2, 3));
        assert_eq!(loaded.tilt, "high");
        assert!(loaded.perspective && loaded.pitch.is_some() && loaded.yaw.is_some());
        assert_eq!(loaded.time, 1234.5);
        assert_eq!(loaded.player, Vec3::new(1.5, -2.5, 9.0));
        let got = &loaded.chunks[&IVec2::new(-1, 4)];
        assert_eq!(got[0], blocks2.idx("stone").unwrap());
        assert_eq!(got[550], blocks2.idx("dirt").unwrap());
        assert_eq!(got[700], AIR);
        assert_eq!(
            loaded.inventory[0],
            Some(Stack {
                item: items2.idx("dirt").unwrap(),
                n: 12
            })
        );
    }

    #[test]
    fn missing_content_degrades_instead_of_failing() {
        let (blocks, items) = regs(&["stone", "crystal"]);
        let mut chunk = empty_blocks();
        chunk[..10].fill(blocks.idx("crystal").unwrap());
        let mut inv = Inventory::default();
        inv.add(items.idx("crystal").unwrap(), 1, &items);
        let file = pack(
            1,
            Vec3::ZERO,
            &Cam::quarter(0),
            0.0,
            &inv,
            [(IVec2::ZERO, &chunk)].into_iter(),
            &blocks,
            &items,
        );
        let (blocks2, items2) = regs(&["stone"]);
        let loaded = unpack(&file, &blocks2, &items2).unwrap();
        assert_eq!(loaded.chunks[&IVec2::ZERO][0], AIR);
        assert_eq!(loaded.inventory[0], None);
        // A file from another version is refused.
        let old = SaveFile {
            version: 1,
            ..Default::default()
        };
        assert!(unpack(&old, &blocks2, &items2).is_err());
    }
}
