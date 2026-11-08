//! Crisp terrain edges. The base chunk mesh (heaviest terrain per vertex) is drawn opaque,
//! then one translucent layer per terrain present whose vertex alpha is that terrain's smooth
//! field weight; `ground.wgsl` cuts each layer at 0.5 per pixel. Higher-priority terrains
//! (later in `terrain.ron`) draw on top.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

use crate::world::blend::{SUB, VERTS, Weights, cell_vertex, push_cell_indices};
use crate::world::chunk::CHUNK;

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

/// One layer mesh per terrain that appears anywhere in the chunk's field, in priority order.
/// Cells where the terrain's weight never rises above a trace are skipped so layers stay small.
pub fn layer_meshes(field: &[Weights], colors: &[[f32; 4]]) -> Vec<(u16, Mesh)> {
    let mut present: Vec<u16> = Vec::new();
    for w in field {
        for (t, _) in w.iter() {
            if !present.contains(t) {
                present.push(*t);
            }
        }
    }
    if present.len() < 2 {
        return Vec::new();
    }
    present.sort_unstable();
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
                let cell_i = (ty * CHUNK + tx) as usize * VERTS;
                let cell = &field[cell_i..cell_i + VERTS];
                if cell.iter().all(|w| w.of(t) < 0.03) {
                    continue;
                }
                let b = positions.len() as u32;
                for sy in 0..=SUB {
                    for sx in 0..=SUB {
                        positions.push(cell_vertex(tx, ty, sx, sy));
                        let w = cell[(sy * (SUB + 1) + sx) as usize].of(t);
                        cols.push([color[0], color[1], color[2], w]);
                    }
                }
                push_cell_indices(&mut idx, b);
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
