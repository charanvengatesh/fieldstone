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
    if let Some(e) = map.water.remove(&pos) {
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
    water: Option<Res<crate::water::WaterHandle>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let colors: Vec<[f32; 4]> = terrain
        .iter()
        .map(|(_, t)| palette_color(&palette, &t.color).to_linear().to_f32_array())
        .collect();
    let wet: Vec<bool> = terrain
        .iter()
        .map(|(_, t)| t.fish_table.is_some())
        .collect();
    let ChunkMap {
        chunks,
        entities,
        objects: obj_entities,
        water: water_entities,
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
        if let Some(water) = &water {
            let wm = crate::water::water_mesh(
                pos,
                &tiles,
                |t| wet.get(t as usize).copied().unwrap_or(false),
                |t| {
                    colors
                        .get(t as usize)
                        .copied()
                        .unwrap_or([0.0, 0.0, 1.0, 1.0])
                },
                neighbor,
            );
            match (wm, water_entities.get(&pos)) {
                (Some(m), Some(&e)) => {
                    commands.entity(e).insert(Mesh2d(meshes.add(m)));
                }
                (Some(m), None) => {
                    let e = commands
                        .spawn((
                            crate::water::WaterMesh,
                            Mesh2d(meshes.add(m)),
                            MeshMaterial2d(water.0.clone()),
                            Transform::from_translation(
                                (center_of(pos * CHUNK) - CELL / 2.0).extend(0.3),
                            ),
                        ))
                        .id();
                    water_entities.insert(pos, e);
                }
                (None, Some(_)) => {
                    if let Some(e) = water_entities.remove(&pos) {
                        commands.entity(e).despawn();
                    }
                }
                (None, None) => {}
            }
        }
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
            let def = objects.at(obj);
            let shape = shapes.idx(&def.shape);
            let e = crate::art::spawn_shape(
                &mut commands,
                &assets,
                shape,
                Transform::from_translation(object_pos(cell, def).extend(1.0)),
            );
            obj_entities.insert(cell, e);
        }
    }
}

/// Width of the anti-alias band between terrains, in weight units (0.5 is the boundary).
const EDGE: f32 = 0.06;

/// Ground mesh on the dual grid: each cell spans four tile centers and is tessellated into
/// `SUB x SUB` quads whose vertices take the heaviest terrain of the smooth field in
/// [`super::blend`], with a thin blend at the contour. Grain is hashed on a shared lattice so
/// it is continuous across cells and chunks.
fn terrain_mesh(
    pos: IVec2,
    tiles: &[Tile; TILES],
    colors: &[[f32; 4]],
    tile_at: impl Fn(IVec2) -> Option<u16>,
) -> Mesh {
    use super::blend::{SUB, terrain_lookup, weights_at};
    let origin = pos * CHUNK;
    let terrain_of = terrain_lookup(pos, tiles, &tile_at);
    let color = |t: u16| {
        colors
            .get(t as usize)
            .copied()
            .unwrap_or([1.0, 0.0, 1.0, 1.0])
    };
    let smooth = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    let verts_per_cell = ((SUB + 1) * (SUB + 1)) as usize;
    let mut positions = Vec::with_capacity(TILES * verts_per_cell);
    let mut cols = Vec::with_capacity(TILES * verts_per_cell);
    let mut idx = Vec::with_capacity(TILES * (SUB * SUB * 6) as usize);
    for ty in 0..CHUNK {
        for tx in 0..CHUNK {
            let cell = origin + IVec2::new(tx, ty);
            let b = positions.len() as u32;
            for sy in 0..=SUB {
                for sx in 0..=SUB {
                    let (u, v) = (sx as f32 / SUB as f32, sy as f32 / SUB as f32);
                    let p = cell.as_vec2() + Vec2::new(u, v);
                    let w = weights_at(p, &terrain_of);
                    let a = color(w[0].0);
                    let mut c = match w.get(1) {
                        None => a,
                        Some(second) => {
                            let bcol = color(second.0);
                            let m = smooth((w[0].1 - second.1) / (2.0 * EDGE) + 0.5);
                            [
                                bcol[0] + (a[0] - bcol[0]) * m,
                                bcol[1] + (a[1] - bcol[1]) * m,
                                bcol[2] + (a[2] - bcol[2]) * m,
                                1.0,
                            ]
                        }
                    };
                    let g = 0.975
                        + 0.05 * crate::core::hash01(11, cell.x * SUB + sx, cell.y * SUB + sy, 5);
                    for channel in c.iter_mut().take(3) {
                        *channel *= g;
                    }
                    positions.push([
                        (tx as f32 + 0.5 + u) * CELL,
                        (ty as f32 + 0.5 + v) * CELL,
                        0.0,
                    ]);
                    cols.push(c);
                }
            }
            let wv = (SUB + 1) as u32;
            for sy in 0..SUB as u32 {
                for sx in 0..SUB as u32 {
                    let i0 = b + sy * wv + sx;
                    idx.extend([i0, i0 + 1, i0 + wv + 1, i0, i0 + wv + 1, i0 + wv]);
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
    let def = objects.at(object);
    let shape = shapes.idx(&def.shape);
    let e = crate::art::spawn_shape(
        commands,
        assets,
        shape,
        Transform::from_translation(object_pos(cell, def).extend(1.0)),
    );
    map.objects.insert(cell, e);
}

/// Where an object draws: the cell center nudged by its `jitter`, deterministic per cell, so
/// forests and scattered decor stop lining up on the grid.
pub fn object_pos(cell: IVec2, def: &ObjectDef) -> Vec2 {
    if def.jitter <= 0.0 {
        return center_of(cell);
    }
    let dx = crate::core::hash01(5, cell.x, cell.y, 21) - 0.5;
    let dy = crate::core::hash01(5, cell.x, cell.y, 22) - 0.5;
    center_of(cell) + Vec2::new(dx, dy) * 2.0 * def.jitter * CELL
}
