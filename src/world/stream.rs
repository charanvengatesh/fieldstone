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
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if let Some(n) = map.chunks.get_mut(&(pos + IVec2::new(dx, dy))) {
                        n.dirty = true;
                    }
                }
            }
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

/// Sub-quads per tile edge.
const SUB: i32 = 4;
/// How far the blend between terrains reaches into a tile, in tiles (0.5 = fully blended).
const BLEND: f32 = 0.22;

/// Terrain with a narrow blended seam and fine grain. Each tile keeps its own color except
/// inside `BLEND` of an edge, where it eases toward the four-tile corner average, so shores
/// curve without the whole field turning to mush. Cells past a chunk edge use the neighbor
/// chunk when loaded and clamp otherwise (the neighbor re-meshes this chunk when it arrives).
fn terrain_mesh(
    pos: IVec2,
    tiles: &[Tile; TILES],
    colors: &[[f32; 4]],
    tile_at: impl Fn(IVec2) -> Option<u16>,
) -> Mesh {
    let origin = pos * CHUNK;
    let base = |cell: IVec2| -> [f32; 4] {
        let local = cell - origin;
        let inside = (0..CHUNK).contains(&local.x) && (0..CHUNK).contains(&local.y);
        let terrain = if inside {
            tiles[(local.y * CHUNK + local.x) as usize].terrain
        } else {
            tile_at(cell).unwrap_or_else(|| {
                let l = local.clamp(IVec2::ZERO, IVec2::splat(CHUNK - 1));
                tiles[(l.y * CHUNK + l.x) as usize].terrain
            })
        };
        colors
            .get(terrain as usize)
            .copied()
            .unwrap_or([1.0, 0.0, 1.0, 1.0])
    };
    let n = (CHUNK + 1) as usize;
    let mut corner = vec![[0.0f32; 4]; n * n];
    for cy in 0..=CHUNK {
        for cx in 0..=CHUNK {
            let mut acc = [0.0f32; 4];
            for (dx, dy) in [(-1, -1), (0, -1), (-1, 0), (0, 0)] {
                let c = base(origin + IVec2::new(cx + dx, cy + dy));
                for k in 0..3 {
                    acc[k] += c[k] * 0.25;
                }
            }
            corner[cy as usize * n + cx as usize] = [acc[0], acc[1], acc[2], 1.0];
        }
    }
    let lerp4 = |a: [f32; 4], b: [f32; 4], t: f32| {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
            1.0,
        ]
    };
    let smooth = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    let verts_per_tile = ((SUB + 1) * (SUB + 1)) as usize;
    let mut positions = Vec::with_capacity(TILES * verts_per_tile);
    let mut cols = Vec::with_capacity(TILES * verts_per_tile);
    let mut idx = Vec::with_capacity(TILES * (SUB * SUB * 6) as usize);
    for ty in 0..CHUNK {
        for tx in 0..CHUNK {
            let cell = origin + IVec2::new(tx, ty);
            let own = base(cell);
            let c00 = corner[ty as usize * n + tx as usize];
            let c10 = corner[ty as usize * n + tx as usize + 1];
            let c01 = corner[(ty + 1) as usize * n + tx as usize];
            let c11 = corner[(ty + 1) as usize * n + tx as usize + 1];
            let b = positions.len() as u32;
            for sy in 0..=SUB {
                for sx in 0..=SUB {
                    let (u, v) = (sx as f32 / SUB as f32, sy as f32 / SUB as f32);
                    positions.push([(tx as f32 + u) * CELL, (ty as f32 + v) * CELL, 0.0]);
                    let seam = lerp4(lerp4(c00, c10, u), lerp4(c01, c11, u), v);
                    // Distance to the nearest tile edge, 0 at the edge, 0.5 at the center.
                    let edge = (0.5 - (u - 0.5).abs()).min(0.5 - (v - 0.5).abs());
                    let w = 1.0 - smooth(edge / BLEND);
                    let mut c = lerp4(own, seam, w);
                    // Fine grain: a per-vertex speckle so flat ground has some tooth.
                    let g = 0.965
                        + 0.07
                            * crate::core::hash01(
                                11,
                                cell.x * (SUB + 1) + sx,
                                cell.y * (SUB + 1) + sy,
                                5,
                            );
                    for channel in c.iter_mut().take(3) {
                        *channel *= g;
                    }
                    cols.push(c);
                }
            }
            let w = (SUB + 1) as u32;
            for sy in 0..SUB as u32 {
                for sx in 0..SUB as u32 {
                    let i0 = b + sy * w + sx;
                    idx.extend([i0, i0 + 1, i0 + w + 1, i0, i0 + w + 1, i0 + w]);
                }
            }
        }
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
