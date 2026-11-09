//! Keeps the chunks around the player loaded and drawn. Terrain is one vertex-colored mesh per
//! chunk; objects are shape entities tracked in `ChunkMap::objects`.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::chunk::footprint;
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
/// Dirty chunks re-meshed per frame; the rest wait a frame or two.
const REBUILDS_PER_FRAME: usize = 2;

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
    for e in map.layers.remove(&pos).unwrap_or_default() {
        commands.entity(e).despawn();
    }
    let origin = pos * CHUNK;
    for y in 0..CHUNK {
        for x in 0..CHUNK {
            let cell = origin + IVec2::new(x, y);
            map.lights.remove(&cell);
            map.parts.remove(&cell);
            if let Some(e) = map.objects.remove(&cell) {
                commands.entity(e).despawn();
            }
        }
    }
}

/// Rebuilds meshes for dirty chunks and spawns missing object entities.
pub fn draw(
    player: Single<&Transform, With<Player>>,
    mut map: ResMut<ChunkMap>,
    terrain: Res<Registry<TerrainDef>>,
    objects: Res<Registry<ObjectDef>>,
    shapes: Res<Registry<ShapeDef>>,
    palette: Res<Registry<ColorDef>>,
    assets: Res<ShapeAssets>,
    material: Res<TerrainMaterial>,
    water: Option<Res<crate::water::WaterHandle>>,
    ground: Option<Res<crate::ground::GroundHandles>>,
    sheets: Res<super::tiles::TileSheets>,
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
    let focus = chunk_of(cell_of(player.translation.truncate()));
    let ChunkMap {
        chunks,
        entities,
        objects: obj_entities,
        water: water_entities,
        layers,
        lights,
        parts,
    } = &mut *map;
    let mut dirty: Vec<IVec2> = chunks
        .iter()
        .filter(|(_, c)| c.dirty)
        .map(|(p, _)| *p)
        .collect();
    // Rebuilding is the expensive part of streaming; spread it out, nearest chunk first.
    dirty.sort_by_key(|p| (*p - focus).abs().max_element());
    dirty.truncate(REBUILDS_PER_FRAME);
    for pos in dirty {
        chunks.get_mut(&pos).unwrap().dirty = false;
        let tiles = chunks[&pos].tiles;
        let neighbor = |c: IVec2| {
            chunks
                .get(&chunk_of(c))
                .map(|ch| ch.tiles[local_index(c)].terrain)
        };
        let field = super::blend::chunk_field(pos, &tiles, &neighbor);
        let pixel = assets.pixel() && !sheets.ground.is_empty();
        let origin_v = (center_of(pos * CHUNK) - CELL / 2.0).extend(0.0);
        for e in layers.remove(&pos).unwrap_or_default() {
            commands.entity(e).despawn();
        }
        // Pixel mode: tile sheets. The lowest terrain present fills every cell; the rest
        // draw their corner-mask tiles on top in priority order.
        let origin_cell = pos * CHUNK;
        let height_of = |x: i32, y: i32| -> u8 {
            let cell = origin_cell + IVec2::new(x, y);
            chunks
                .get(&chunk_of(cell))
                .map_or(0, |ch| ch.tiles[local_index(cell)].height)
        };
        let (mesh, pixel_material) = if pixel {
            let order = super::tiles::present(&field);
            let base = order[0];
            let mut spawned = Vec::new();
            for (k, &t) in order.iter().enumerate().skip(1) {
                let Some(m) = super::tiles::tile_mesh(&field, t, false, &height_of) else {
                    continue;
                };
                let (material, is_water) = sheet_material(&sheets, t);
                let mut e = commands.spawn((
                    Mesh2d(meshes.add(m)),
                    MeshMaterial2d(material),
                    Transform::from_translation(origin_v + Vec3::Z * (0.01 + 0.005 * k as f32)),
                ));
                if is_water {
                    e.insert(super::tiles::WaterLayer(t));
                }
                spawned.push(e.id());
            }
            if let (Some(cliff), Some(m)) = (&sheets.cliff, super::tiles::cliff_mesh(&height_of)) {
                // Cliff faces sit on the real tile grid (half a cell off the dual grid).
                let e = commands
                    .spawn((
                        Mesh2d(meshes.add(m)),
                        MeshMaterial2d(cliff.clone()),
                        Transform::from_translation(
                            (center_of(pos * CHUNK) - CELL / 2.0).extend(0.08),
                        ),
                    ))
                    .id();
                spawned.push(e);
            }
            if !spawned.is_empty() {
                layers.insert(pos, spawned);
            }
            let m =
                super::tiles::tile_mesh(&field, base, true, &height_of).expect("base tile mesh");
            let (material, is_water) = sheet_material(&sheets, base);
            (meshes.add(m), Some((material, is_water, base)))
        } else {
            (meshes.add(terrain_mesh(&field, &colors)), None)
        };
        if let Some(ground) = ground.as_ref().filter(|_| !pixel) {
            let spawned: Vec<Entity> = crate::ground::layer_meshes(&field, &colors)
                .into_iter()
                .enumerate()
                .map(|(k, (_, m))| {
                    commands
                        .spawn((
                            crate::ground::GroundLayer,
                            Mesh2d(meshes.add(m)),
                            MeshMaterial2d(ground.layer.clone()),
                            Transform::from_translation(
                                origin_v + Vec3::Z * (0.01 + 0.005 * k as f32),
                            ),
                        ))
                        .id()
                })
                .collect();
            if !spawned.is_empty() {
                layers.insert(pos, spawned);
            }
        }
        if let Some(water) = water.as_ref().filter(|_| !pixel) {
            let wm = crate::water::water_mesh(
                &field,
                |t| wet.get(t as usize).copied().unwrap_or(false),
                |t| {
                    colors
                        .get(t as usize)
                        .copied()
                        .unwrap_or([0.0, 0.0, 1.0, 1.0])
                },
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
        // Which material type the base uses depends on the mode (set once at startup).
        enum BaseMat {
            Color(Handle<ColorMaterial>),
            Ground(Handle<crate::ground::GroundMaterial>),
        }
        let base_material = match (&pixel_material, &ground) {
            (Some((m, _, _)), _) => BaseMat::Color(m.clone()),
            (None, Some(g)) => BaseMat::Ground(g.base.clone()),
            (None, None) => BaseMat::Color(material.0.clone()),
        };
        let base_entity = match entities.get(&pos) {
            Some(&e) => e,
            None => {
                let e = commands
                    .spawn((
                        ChunkMesh(pos),
                        Transform::from_translation(origin.extend(0.0)),
                    ))
                    .id();
                entities.insert(pos, e);
                e
            }
        };
        let mut ec = commands.entity(base_entity);
        ec.insert(Mesh2d(mesh));
        match base_material {
            BaseMat::Color(h) => {
                ec.insert(MeshMaterial2d(h));
            }
            BaseMat::Ground(h) => {
                ec.insert(MeshMaterial2d(h));
            }
        }
        match &pixel_material {
            Some((_, true, t)) => {
                ec.insert(super::tiles::WaterLayer(*t));
            }
            _ => {
                ec.remove::<super::tiles::WaterLayer>();
            }
        }
        for (cell, obj) in new_objects {
            let def = objects.at(obj);
            if def.is_part() {
                continue;
            }
            if def.light.is_some() {
                lights.insert(cell);
            }
            for c in footprint(cell, def.footprint()).skip(1) {
                parts.insert(c, cell);
            }
            let e = spawn_object_visual(
                &mut commands,
                &assets,
                &objects,
                &shapes,
                chunks,
                parts,
                cell,
                obj,
            );
            obj_entities.insert(cell, e);
        }
    }
}

/// Width of the anti-alias band between terrains in the opaque base mesh (the edge layers on
/// top do the real per-pixel cut; this just keeps the base from showing hard seams).
const EDGE: f32 = 0.06;

/// Opaque ground mesh: each vertex takes the heaviest terrain of the shared field with a thin
/// blend toward the runner-up.
fn terrain_mesh(field: &[super::blend::Weights], colors: &[[f32; 4]]) -> Mesh {
    use super::blend::{SUB, VERTS, cell_vertex, push_cell_indices};
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
    let mut positions = Vec::with_capacity(TILES * VERTS);
    let mut cols = Vec::with_capacity(TILES * VERTS);
    let mut idx = Vec::with_capacity(TILES * (SUB * SUB * 6) as usize);
    for ty in 0..CHUNK {
        for tx in 0..CHUNK {
            let cell_i = (ty * CHUNK + tx) as usize * VERTS;
            let b = positions.len() as u32;
            for sy in 0..=SUB {
                for sx in 0..=SUB {
                    let w = &field[cell_i + (sy * (SUB + 1) + sx) as usize];
                    let (top, tw) = w.first();
                    let a = color(top);
                    let c = match w.second() {
                        None => a,
                        Some((second, sw)) => {
                            let bcol = color(second);
                            let m = smooth((tw - sw) / (2.0 * EDGE) + 0.5);
                            [
                                bcol[0] + (a[0] - bcol[0]) * m,
                                bcol[1] + (a[1] - bcol[1]) * m,
                                bcol[2] + (a[2] - bcol[2]) * m,
                                1.0,
                            ]
                        }
                    };
                    positions.push(cell_vertex(tx, ty, sx, sy));
                    cols.push(c);
                }
            }
            push_cell_indices(&mut idx, b);
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

/// Removes whatever occupies `cell` (the whole footprint for multi-tile objects) and its
/// visual. Returns the removed object id. Neighbors that connect visually are redrawn by
/// the caller via [`refresh_neighbors`].
pub fn clear_object(map: &mut ChunkMap, cell: IVec2, commands: &mut Commands) -> Option<u16> {
    let anchor = map.anchor_of(cell);
    let old = map.tile(anchor)?.object?;
    // Footprint cells are whichever parts point at this anchor, plus the anchor itself.
    let cells: Vec<IVec2> = map
        .parts
        .iter()
        .filter(|(_, a)| **a == anchor)
        .map(|(c, _)| *c)
        .chain(std::iter::once(anchor))
        .collect();
    for c in cells {
        map.set_object(c, None);
        map.parts.remove(&c);
        map.lights.remove(&c);
        if let Some(e) = map.objects.remove(&c) {
            commands.entity(e).despawn();
        }
    }
    Some(old)
}

/// Sets an object anchored at `cell` (bottom-left of its footprint), fills the rest of the
/// footprint with parts, spawns its visual and redraws connecting neighbors.
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
    if def.light.is_some() {
        map.lights.insert(cell);
    }
    let part = objects.idx(crate::core::PART_ID);
    for c in footprint(cell, def.footprint()).skip(1) {
        clear_object(map, c, commands);
        map.set_object(c, part);
        map.parts.insert(c, cell);
    }
    let ChunkMap { chunks, parts, .. } = &mut *map;
    let e = spawn_object_visual(
        commands, assets, objects, shapes, chunks, parts, cell, object,
    );
    map.objects.insert(cell, e);
    refresh_neighbors(map, cell, objects, shapes, assets, commands);
}

/// Spawns the visual for an object anchored at `cell`: feet at the bottom-center of its
/// footprint, connect frame chosen by same-group neighbors.
pub fn spawn_object_visual(
    commands: &mut Commands,
    assets: &ShapeAssets,
    objects: &Registry<ObjectDef>,
    shapes: &Registry<ShapeDef>,
    chunks: &bevy::platform::collections::HashMap<IVec2, Chunk>,
    parts: &bevy::platform::collections::HashMap<IVec2, IVec2>,
    cell: IVec2,
    object: u16,
) -> Entity {
    let def = objects.at(object);
    let shape = shapes.idx(&def.shape);
    let e = crate::art::spawn_standing(
        commands,
        assets,
        shape,
        Transform::from_translation(object_pos(cell, def).extend(1.0)),
    );
    if let Some(group) = &def.connects {
        let same = |c: IVec2| {
            let a = parts.get(&c).copied().unwrap_or(c);
            chunks
                .get(&chunk_of(a))
                .and_then(|ch| ch.tiles[local_index(a)].object)
                .is_some_and(|o| objects.at(o).connects.as_deref() == Some(group.as_str()))
        };
        let mask = same(cell + IVec2::Y) as usize
            | (same(cell + IVec2::X) as usize) << 1
            | (same(cell - IVec2::Y) as usize) << 2
            | (same(cell - IVec2::X) as usize) << 3;
        commands.entity(e).insert(crate::art::ConnectFrame(mask));
    }
    e
}

/// Redraws the four neighbors of `cell` that belong to a connect group.
pub fn refresh_neighbors(
    map: &mut ChunkMap,
    cell: IVec2,
    objects: &Registry<ObjectDef>,
    shapes: &Registry<ShapeDef>,
    assets: &ShapeAssets,
    commands: &mut Commands,
) {
    for d in [IVec2::Y, IVec2::X, IVec2::NEG_Y, IVec2::NEG_X] {
        let n = map.anchor_of(cell + d);
        let Some(o) = map.tile(n).and_then(|t| t.object) else {
            continue;
        };
        if objects.at(o).connects.is_none() {
            continue;
        }
        if let Some(e) = map.objects.remove(&n) {
            commands.entity(e).despawn();
        }
        let ChunkMap { chunks, parts, .. } = &mut *map;
        let e = spawn_object_visual(commands, assets, objects, shapes, chunks, parts, n, o);
        map.objects.insert(n, e);
    }
}

/// Material for a terrain's tile sheet (first water frame for water) and whether it is water.
fn sheet_material(sheets: &super::tiles::TileSheets, t: u16) -> (Handle<ColorMaterial>, bool) {
    if let Some(frames) = sheets.water.get(&t) {
        return (frames[0].clone(), true);
    }
    (sheets.ground.get(&t).cloned().unwrap_or_default(), false)
}

/// Where an object draws: the cell center nudged by its `jitter`, deterministic per cell, so
/// forests and scattered decor stop lining up on the grid.
pub fn object_pos(cell: IVec2, def: &ObjectDef) -> Vec2 {
    let fp = def.footprint();
    let base = center_of(cell) + Vec2::new((fp.x - 1) as f32 * CELL / 2.0, 0.0);
    if def.jitter <= 0.0 {
        return base;
    }
    let dx = crate::core::hash01(5, cell.x, cell.y, 21) - 0.5;
    let dy = crate::core::hash01(5, cell.x, cell.y, 22) - 0.5;
    // Whole units so pixel art stays on the texel grid.
    (base + Vec2::new(dx, dy) * 2.0 * def.jitter * CELL).round()
}
