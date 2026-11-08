//! Crisp terrain edges. The base chunk mesh (heaviest terrain per vertex) is drawn opaque,
//! then one translucent layer per terrain present whose vertex alpha is that terrain's smooth
//! field weight; `ground.wgsl` cuts each layer at 0.5 per pixel. Higher-priority terrains
//! (later in `terrain.ron`) draw on top.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

use crate::world::blend::{SUB, terrain_lookup, weights_at};
use crate::world::chunk::{CELL, CHUNK, TILES, Tile};

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct GroundMaterial {
    /// x: grain strength, y: 1 for the opaque base layer.
    #[uniform(0)]
    pub params: Vec4,
}

impl Material2d for GroundMaterial {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/ground.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

#[derive(Resource)]
pub struct GroundHandles {
    pub base: Handle<GroundMaterial>,
    pub layer: Handle<GroundMaterial>,
}

#[derive(Component)]
pub struct GroundLayer;

pub fn setup(mut commands: Commands, mut materials: ResMut<Assets<GroundMaterial>>) {
    commands.insert_resource(GroundHandles {
        base: materials.add(GroundMaterial {
            params: Vec4::new(1.0, 1.0, 0.0, 0.0),
        }),
        layer: materials.add(GroundMaterial {
            params: Vec4::new(1.0, 0.0, 0.0, 0.0),
        }),
    });
}

/// One layer mesh per terrain that appears anywhere near this chunk, in priority order.
/// Each returns `(terrain, mesh)`; cells where the terrain's weight never rises above a
/// trace are skipped so layers stay small.
pub fn layer_meshes(
    pos: IVec2,
    tiles: &[Tile; TILES],
    colors: &[[f32; 4]],
    tile_at: impl Fn(IVec2) -> Option<u16>,
) -> Vec<(u16, Mesh)> {
    let origin = pos * CHUNK;
    let terrain_of = terrain_lookup(pos, tiles, &tile_at);
    // Terrains present in the chunk plus a one-cell margin.
    let mut present: Vec<u16> = Vec::new();
    for y in -1..=CHUNK {
        for x in -1..=CHUNK {
            let t = terrain_of(origin + IVec2::new(x, y));
            if !present.contains(&t) {
                present.push(t);
            }
        }
    }
    if present.len() < 2 {
        return Vec::new();
    }
    present.sort_unstable();
    // Cache the field once per vertex, then split into layers.
    let n = ((SUB + 1) * (SUB + 1)) as usize;
    let mut field: Vec<Vec<(u16, f32)>> = Vec::with_capacity(TILES * n);
    for ty in 0..CHUNK {
        for tx in 0..CHUNK {
            let cell = origin + IVec2::new(tx, ty);
            for sy in 0..=SUB {
                for sx in 0..=SUB {
                    let p =
                        cell.as_vec2() + Vec2::new(sx as f32 / SUB as f32, sy as f32 / SUB as f32);
                    field.push(weights_at(p, &terrain_of));
                }
            }
        }
    }
    let mut out = Vec::new();
    for &t in &present {
        let color = colors
            .get(t as usize)
            .copied()
            .unwrap_or([1.0, 0.0, 1.0, 1.0]);
        let mut positions = Vec::new();
        let mut cols = Vec::new();
        let mut idx = Vec::new();
        for ty in 0..CHUNK {
            for tx in 0..CHUNK {
                let cell_i = (ty * CHUNK + tx) as usize * n;
                let weights: Vec<f32> = (0..n)
                    .map(|k| {
                        field[cell_i + k]
                            .iter()
                            .find(|(id, _)| *id == t)
                            .map_or(0.0, |e| e.1)
                    })
                    .collect();
                if weights.iter().all(|w| *w < 0.03) {
                    continue;
                }
                let b = positions.len() as u32;
                for sy in 0..=SUB {
                    for sx in 0..=SUB {
                        let (u, v) = (sx as f32 / SUB as f32, sy as f32 / SUB as f32);
                        positions.push([
                            (tx as f32 + 0.5 + u) * CELL,
                            (ty as f32 + 0.5 + v) * CELL,
                            0.0,
                        ]);
                        cols.push([
                            color[0],
                            color[1],
                            color[2],
                            weights[(sy * (SUB + 1) + sx) as usize],
                        ]);
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
        if positions.is_empty() {
            continue;
        }
        out.push((
            t,
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, cols)
            .with_inserted_indices(Indices::U32(idx)),
        ));
    }
    out
}

pub struct GroundPlugin;

impl Plugin for GroundPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Material2dPlugin::<GroundMaterial>::default())
            .add_systems(Startup, setup);
    }
}
