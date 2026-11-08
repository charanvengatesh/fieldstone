//! Animated water: a second per-chunk mesh over water terrain, drawn with a small shader that
//! ripples, glints and foams at the shoreline. The mesh is built on the same dual grid as the
//! terrain, carrying the water color in rgb and the water weight in alpha.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

use crate::world::chunk::{CELL, CHUNK, TILES, Tile};

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct WaterMaterial {
    /// x: seconds, y: ripple strength, z: foam width in weight units.
    #[uniform(0)]
    pub params: Vec4,
}

impl Material2d for WaterMaterial {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/water.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// The one shared water material.
#[derive(Resource)]
pub struct WaterHandle(pub Handle<WaterMaterial>);

#[derive(Component)]
pub struct WaterMesh;

pub fn setup(mut commands: Commands, mut materials: ResMut<Assets<WaterMaterial>>) {
    commands.insert_resource(WaterHandle(materials.add(WaterMaterial {
        params: Vec4::new(0.0, 1.0, 0.07, 0.0),
    })));
}

pub fn tick(
    time: Res<Time>,
    handle: Res<WaterHandle>,
    mut materials: ResMut<Assets<WaterMaterial>>,
) {
    if let Some(mut m) = materials.get_mut(&handle.0) {
        m.params.x = time.elapsed_secs();
    }
}

const SUB: i32 = 3;

/// Water overlay for one chunk, or `None` if the chunk touches no water. `is_water(terrain)`
/// and `color(terrain)` describe terrains; `tile_at` resolves cells outside the chunk.
pub fn water_mesh(
    pos: IVec2,
    tiles: &[Tile; TILES],
    is_water: impl Fn(u16) -> bool,
    color: impl Fn(u16) -> [f32; 4],
    tile_at: impl Fn(IVec2) -> Option<u16>,
) -> Option<Mesh> {
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
    let mut positions = Vec::new();
    let mut cols = Vec::new();
    let mut idx = Vec::new();
    for ty in 0..CHUNK {
        for tx in 0..CHUNK {
            let cell = origin + IVec2::new(tx, ty);
            let t = [
                terrain_of(cell),
                terrain_of(cell + IVec2::X),
                terrain_of(cell + IVec2::Y),
                terrain_of(cell + IVec2::ONE),
            ];
            let wet = [
                is_water(t[0]),
                is_water(t[1]),
                is_water(t[2]),
                is_water(t[3]),
            ];
            if !wet.iter().any(|w| *w) {
                continue;
            }
            // Average water color among the wet corners (deep and shallow blend naturally).
            let mut base = [0.0f32; 4];
            let mut count = 0.0;
            for k in 0..4 {
                if wet[k] {
                    let c = color(t[k]);
                    for ch in 0..3 {
                        base[ch] += c[ch];
                    }
                    count += 1.0;
                }
            }
            for ch in base.iter_mut().take(3) {
                *ch /= count;
            }
            let b = positions.len() as u32;
            for sy in 0..=SUB {
                for sx in 0..=SUB {
                    let (u, v) = (sx as f32 / SUB as f32, sy as f32 / SUB as f32);
                    let w = [(1.0 - u) * (1.0 - v), u * (1.0 - v), (1.0 - u) * v, u * v];
                    let weight: f32 = (0..4).filter(|&k| wet[k]).map(|k| w[k]).sum();
                    positions.push([
                        (tx as f32 + 0.5 + u) * CELL,
                        (ty as f32 + 0.5 + v) * CELL,
                        0.0,
                    ]);
                    cols.push([base[0], base[1], base[2], weight]);
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
        return None;
    }
    Some(
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, cols)
        .with_inserted_indices(Indices::U32(idx)),
    )
}

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Material2dPlugin::<WaterMaterial>::default())
            .add_systems(Startup, setup)
            .add_systems(Update, tick);
    }
}
