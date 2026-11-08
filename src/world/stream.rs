//! Keeps the chunks around the player loaded and drawn. Terrain is one vertex-colored mesh per
//! chunk; objects are shape entities tracked in `ChunkMap::objects`.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::chunk::*;
use super::worldgen::WorldGen;
use crate::art::{ShapeAssets, palette_color};
use crate::core::save::SavedChunks;
use crate::core::{ColorDef, ObjectDef, Registry, ShapeDef, TerrainDef};
use crate::player::Player;

/// Chunks kept loaded on each side of the player's chunk.
const LOAD_RADIUS: i32 = 2;
const UNLOAD_RADIUS: i32 = 3;
const LOADS_PER_FRAME: usize = 2;

#[derive(Component)]
#[allow(dead_code)] // handy when inspecting entities
pub struct ChunkMesh(pub IVec2);

/// Shared white material; color comes from the vertices.
#[derive(Resource)]
pub struct TerrainMaterial(pub Handle<ColorMaterial>);

pub fn setup(mut commands: Commands, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.insert_resource(TerrainMaterial(
        materials.add(ColorMaterial::from_color(Color::WHITE)),
    ));
}

pub fn stream(
    player: Single<&Transform, With<Player>>,
    wg: Res<WorldGen>,
    mut map: ResMut<ChunkMap>,
    mut saved: ResMut<SavedChunks>,
    mut commands: Commands,
) {
    let center = chunk_of(cell_of(player.translation.truncate()));
    let far: Vec<IVec2> = map
        .chunks
        .keys()
        .filter(|p| (**p - center).abs().max_element() > UNLOAD_RADIUS)
        .copied()
        .collect();
    for pos in far {
        unload(&mut map, &mut saved, pos, &mut commands);
    }
    let mut loads = 0;
    for dy in -LOAD_RADIUS..=LOAD_RADIUS {
        for dx in -LOAD_RADIUS..=LOAD_RADIUS {
            let pos = center + IVec2::new(dx, dy);
            if map.chunks.contains_key(&pos) || loads >= LOADS_PER_FRAME {
                continue;
            }
            let mut chunk = wg.chunk(pos);
            if let Some(tiles) = saved.0.remove(&pos) {
                chunk.tiles = tiles;
                chunk.modified = true;
            }
            map.chunks.insert(pos, chunk);
            loads += 1;
        }
    }
}

fn unload(map: &mut ChunkMap, saved: &mut SavedChunks, pos: IVec2, commands: &mut Commands) {
    let Some(chunk) = map.chunks.remove(&pos) else {
        return;
    };
    if chunk.modified {
        saved.0.insert(pos, chunk.tiles);
    }
    if let Some(e) = map.entities.remove(&pos) {
        commands.entity(e).despawn();
    }
    let origin = pos * CHUNK;
    for y in 0..CHUNK {
        for x in 0..CHUNK {
            if let Some(e) = map.objects.remove(&(origin + IVec2::new(x, y))) {
                commands.entity(e).despawn();
            }
        }
    }
}

/// Rebuilds meshes for dirty chunks and spawns missing object entities.
pub fn draw(
    mut map: ResMut<ChunkMap>,
    terrain: Res<Registry<TerrainDef>>,
    objects: Res<Registry<ObjectDef>>,
    shapes: Res<Registry<ShapeDef>>,
    palette: Res<Registry<ColorDef>>,
    assets: Res<ShapeAssets>,
    material: Res<TerrainMaterial>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let colors: Vec<[f32; 4]> = terrain
        .iter()
        .map(|(_, t)| palette_color(&palette, &t.color).to_linear().to_f32_array())
        .collect();
    let ChunkMap {
        chunks,
        entities,
        objects: obj_entities,
    } = &mut *map;
    let dirty: Vec<IVec2> = chunks
        .iter()
        .filter(|(_, c)| c.dirty)
        .map(|(p, _)| *p)
        .collect();
    for pos in dirty {
        chunks.get_mut(&pos).unwrap().dirty = false;
        let tiles = chunks[&pos].tiles;
        let neighbor = |c: IVec2| {
            chunks
                .get(&chunk_of(c))
                .map(|ch| ch.tiles[local_index(c)].terrain)
        };
        let mesh = meshes.add(terrain_mesh(pos, &tiles, &colors, neighbor));
        let chunk = &tiles;
        let origin = center_of(pos * CHUNK) - CELL / 2.0;
        let new_objects: Vec<(IVec2, u16)> = (0..TILES)
            .filter_map(|i| {
                let cell = pos * CHUNK + IVec2::new(i as i32 % CHUNK, i as i32 / CHUNK);
                chunk[i]
                    .object
                    .filter(|_| !obj_entities.contains_key(&cell))
                    .map(|o| (cell, o))
            })
            .collect();
        match entities.get(&pos) {
            Some(&e) => {
                commands.entity(e).insert(Mesh2d(mesh));
            }
            None => {
                let e = commands
                    .spawn((
                        ChunkMesh(pos),
                        Mesh2d(mesh),
                        MeshMaterial2d(material.0.clone()),
                        Transform::from_translation(origin.extend(0.0)),
                    ))
                    .id();
                entities.insert(pos, e);
            }
        }
        for (cell, obj) in new_objects {
            let shape = shapes.idx(&objects.at(obj).shape);
            let e = crate::art::spawn_shape(
                &mut commands,
                &assets,
                shape,
                Transform::from_translation(center_of(cell).extend(1.0)),
            );
            obj_entities.insert(cell, e);
        }
    }
}

/// Terrain as a soft gradient: each vertex takes the average of the four tiles meeting at that
/// corner, with a little per-tile brightness jitter so flat areas do not read as a grid.
fn terrain_mesh(
    pos: IVec2,
    tiles: &[Tile; TILES],
    colors: &[[f32; 4]],
    tile_at: impl Fn(IVec2) -> Option<u16>,
) -> Mesh {
    let origin = pos * CHUNK;
    let color_of = |cell: IVec2| -> [f32; 4] {
        let local = cell - origin;
        let terrain = if (0..CHUNK).contains(&local.x) && (0..CHUNK).contains(&local.y) {
            Some(tiles[(local.y * CHUNK + local.x) as usize].terrain)
        } else {
            tile_at(cell)
        };
        let Some(t) = terrain else {
            return [0.0, 0.0, 0.0, 1.0];
        };
        let base = colors
            .get(t as usize)
            .copied()
            .unwrap_or([1.0, 0.0, 1.0, 1.0]);
        let jitter = 0.92 + 0.16 * crate::core::hash01(7, cell.x, cell.y, 3);
        [base[0] * jitter, base[1] * jitter, base[2] * jitter, 1.0]
    };
    // Corner color = mean of the four tiles touching it; corners are shared by (CHUNK+1)^2 points.
    let n = (CHUNK + 1) as usize;
    let mut corner = vec![[0.0f32; 4]; n * n];
    for cy in 0..=CHUNK {
        for cx in 0..=CHUNK {
            let mut acc = [0.0f32; 4];
            for (dx, dy) in [(-1, -1), (0, -1), (-1, 0), (0, 0)] {
                let c = color_of(origin + IVec2::new(cx + dx, cy + dy));
                for k in 0..4 {
                    acc[k] += c[k] * 0.25;
                }
            }
            corner[cy as usize * n + cx as usize] = acc;
        }
    }
    let mut positions = Vec::with_capacity(TILES * 5);
    let mut cols = Vec::with_capacity(TILES * 5);
    let mut idx = Vec::with_capacity(TILES * 12);
    for (i, _) in tiles.iter().enumerate() {
        let (tx, ty) = (i as i32 % CHUNK, i as i32 / CHUNK);
        let (x, y) = (tx as f32 * CELL, ty as f32 * CELL);
        let c = |cx: i32, cy: i32| corner[cy as usize * n + cx as usize];
        let center = color_of(origin + IVec2::new(tx, ty));
        let b = positions.len() as u32;
        positions.extend([
            [x, y, 0.0],
            [x + CELL, y, 0.0],
            [x + CELL, y + CELL, 0.0],
            [x, y + CELL, 0.0],
            [x + CELL / 2.0, y + CELL / 2.0, 0.0],
        ]);
        cols.extend([
            c(tx, ty),
            c(tx + 1, ty),
            c(tx + 1, ty + 1),
            c(tx, ty + 1),
            center,
        ]);
        idx.extend([
            b,
            b + 1,
            b + 4,
            b + 1,
            b + 2,
            b + 4,
            b + 2,
            b + 3,
            b + 4,
            b + 3,
            b,
            b + 4,
        ]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, cols)
    .with_inserted_indices(Indices::U32(idx))
}

/// Removes the object entity on a cell, if any, and clears the tile's object.
pub fn clear_object(map: &mut ChunkMap, cell: IVec2, commands: &mut Commands) -> Option<u16> {
    let old = map.set_object(cell, None)?.object;
    if let Some(e) = map.objects.remove(&cell) {
        commands.entity(e).despawn();
    }
    old
}

/// Sets an object on a cell and spawns its entity.
pub fn put_object(
    map: &mut ChunkMap,
    cell: IVec2,
    object: u16,
    objects: &Registry<ObjectDef>,
    shapes: &Registry<ShapeDef>,
    assets: &ShapeAssets,
    commands: &mut Commands,
) {
    clear_object(map, cell, commands);
    if map.set_object(cell, Some(object)).is_none() {
        return;
    }
    let shape = shapes.idx(&objects.at(object).shape);
    let e = crate::art::spawn_shape(
        commands,
        assets,
        shape,
        Transform::from_translation(center_of(cell).extend(1.0)),
    );
    map.objects.insert(cell, e);
}
