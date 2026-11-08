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

/// Sub-quads per dual cell edge.
const SUB: i32 = 4;
/// Width of the anti-alias band between terrains, in weight units (0.5 is the boundary).
const EDGE: f32 = 0.10;

/// Terrain drawn on the dual grid. Each dual cell spans the centers of four neighboring tiles;
/// inside it, every terrain among those four gets a bilinear weight and the heaviest one wins,
/// with a thin blend at the 0.5 contour. The contour of a bilinear field is a curve, so shores
/// round off and diagonals cut cleanly instead of stepping. Grain is hashed on a shared
/// lattice so it is continuous across cells and chunks.
fn terrain_mesh(
    pos: IVec2,
    tiles: &[Tile; TILES],
    colors: &[[f32; 4]],
    tile_at: impl Fn(IVec2) -> Option<u16>,
) -> Mesh {
    let origin = pos * CHUNK;
    let terrain_of = |cell: IVec2| -> u16 {
        let local = cell - origin;
        let inside = (0..CHUNK).contains(&local.x) && (0..CHUNK).contains(&local.y);
        if inside {
            tiles[(local.y * CHUNK + local.x) as usize].terrain
        } else {
            tile_at(cell).unwrap_or_else(|| {
                let l = local.clamp(IVec2::ZERO, IVec2::splat(CHUNK - 1));
                tiles[(l.y * CHUNK + l.x) as usize].terrain
            })
        }
    };
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
            // Corner terrains of this dual cell: the tile itself and its +x, +y, +xy neighbors.
            let t = [
                terrain_of(cell),
                terrain_of(cell + IVec2::X),
                terrain_of(cell + IVec2::Y),
                terrain_of(cell + IVec2::ONE),
            ];
            let b = positions.len() as u32;
            for sy in 0..=SUB {
                for sx in 0..=SUB {
                    let (u, v) = (sx as f32 / SUB as f32, sy as f32 / SUB as f32);
                    let w = [(1.0 - u) * (1.0 - v), u * (1.0 - v), (1.0 - u) * v, u * v];
                    // Weight per distinct terrain; higher terrain index wins exact ties.
                    let mut best: (u16, f32) = (t[0], -1.0);
                    let mut second: (u16, f32) = (t[0], -1.0);
                    for k in 0..4 {
                        if (0..k).any(|m| t[m] == t[k]) {
                            continue;
                        }
                        let weight: f32 =
                            (0..4).filter(|&m| t[m] == t[k]).map(|m| w[m]).sum::<f32>()
                                + t[k] as f32 * 1e-4;
                        if weight > best.1 {
                            second = best;
                            best = (t[k], weight);
                        } else if weight > second.1 {
                            second = (t[k], weight);
                        }
                    }
                    let a = color(best.0);
                    let mut c = if second.1 < 0.0 {
                        a
                    } else {
                        let bcol = color(second.0);
                        let m = smooth((best.1 - second.1) / (2.0 * EDGE) + 0.5);
                        [
                            bcol[0] + (a[0] - bcol[0]) * m,
                            bcol[1] + (a[1] - bcol[1]) * m,
                            bcol[2] + (a[2] - bcol[2]) * m,
                            1.0,
                        ]
                    };
                    let g = 0.97
                        + 0.06 * crate::core::hash01(11, cell.x * SUB + sx, cell.y * SUB + sy, 5);
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
    let shape = shapes.idx(&objects.at(object).shape);
    let e = crate::art::spawn_shape(
        commands,
        assets,
        shape,
        Transform::from_translation(center_of(cell).extend(1.0)),
    );
    map.objects.insert(cell, e);
}
