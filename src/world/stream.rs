//! Keeps chunks around the focus loaded and their meshes current.

use bevy::prelude::*;

use super::iso::View;
use super::mesh::{BlockMaterial, Grid, Palette, chunk_faces};
use super::worldgen::WorldGen;
use super::{CHUNK, Chunk, Focus, Map, chunk_of};

/// Chunks within this many chunks of the focus are loaded.
const LOAD_RADIUS: i32 = 4;
const UNLOAD_RADIUS: i32 = 6;
const LOADS_PER_FRAME: usize = 6;
const MESHES_PER_FRAME: usize = 4;

pub fn stream(focus: Res<Focus>, wg: Res<WorldGen>, mut map: ResMut<Map>, mut commands: Commands) {
    let center = chunk_of(focus.0.floor().as_ivec3());
    let far: Vec<IVec2> = map
        .chunks
        .keys()
        .filter(|p| (**p - center).abs().max_element() > UNLOAD_RADIUS)
        .copied()
        .collect();
    for pos in far {
        let chunk = map.chunks.remove(&pos).expect("listed above");
        if chunk.modified {
            map.parked.insert(pos, chunk.blocks);
        }
        if let Some(e) = map.entities.remove(&pos) {
            commands.entity(e).despawn();
        }
    }
    // Nearest first, so the ground under the player appears before the horizon.
    let mut wanted: Vec<IVec2> = (-LOAD_RADIUS..=LOAD_RADIUS)
        .flat_map(|dy| (-LOAD_RADIUS..=LOAD_RADIUS).map(move |dx| center + IVec2::new(dx, dy)))
        .filter(|p| !map.chunks.contains_key(p))
        .collect();
    wanted.sort_by_key(|p| (*p - center).length_squared());
    for pos in wanted.into_iter().take(LOADS_PER_FRAME) {
        let (blocks, modified) = match map.parked.remove(&pos) {
            Some(blocks) => (blocks, true),
            None => (wg.chunk(pos), false),
        };
        map.chunks.insert(
            pos,
            Chunk {
                blocks,
                dirty: true,
                modified,
            },
        );
    }
}

/// A new camera direction or outline mode changes every mesh.
pub fn view_changed(view: Res<View>, grid: Res<Grid>, mut map: ResMut<Map>) {
    if view.is_changed() || grid.is_changed() {
        for chunk in map.chunks.values_mut() {
            chunk.dirty = true;
        }
    }
}

pub fn remesh(
    focus: Res<Focus>,
    view: Res<View>,
    grid: Res<Grid>,
    wg: Res<WorldGen>,
    palette: Option<Res<Palette>>,
    material: Option<Res<BlockMaterial>>,
    mut map: ResMut<Map>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let (Some(palette), Some(material)) = (palette, material) else {
        return;
    };
    let center = chunk_of(focus.0.floor().as_ivec3());
    let mut dirty: Vec<IVec2> = map
        .chunks
        .iter()
        .filter(|(_, c)| c.dirty)
        .map(|(p, _)| *p)
        .collect();
    dirty.sort_by_key(|p| (*p - center).length_squared());
    dirty.truncate(MESHES_PER_FRAME);
    for pos in dirty {
        // Cells outside the loaded area are answered by the generator, so a chunk's border
        // faces are right the first time and loading a neighbour never forces a rebuild.
        let faces = chunk_faces(pos, *view, grid.0, &palette, |p| {
            map.get(p).unwrap_or_else(|| wg.block(p))
        });
        let at = view.project((pos * CHUNK).extend(0).as_vec3());
        let transform = Transform::from_translation(at);
        let old = map.entities.remove(&pos);
        if faces.len() == 0 {
            if let Some(e) = old {
                commands.entity(e).despawn();
            }
        } else {
            let mesh = Mesh2d(meshes.add(faces.into_mesh()));
            let e = match old {
                Some(e) => {
                    commands.entity(e).insert((mesh, transform));
                    e
                }
                None => commands
                    .spawn((mesh, MeshMaterial2d(material.0.clone()), transform))
                    .id(),
            };
            map.entities.insert(pos, e);
        }
        map.chunks.get_mut(&pos).expect("listed above").dirty = false;
    }
}
