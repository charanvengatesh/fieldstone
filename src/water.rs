//! Animated water: a second per-chunk mesh over water terrain, drawn with a small shader that
//! ripples, glints and foams at the shoreline. The mesh is built on the same dual grid as the
//! terrain, carrying the water color in rgb and the water weight in alpha.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

use crate::world::blend::{SUB, VERTS, Weights, cell_vertex, push_cell_indices};
use crate::world::chunk::CHUNK;

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
        params: Vec4::new(0.0, 1.0, 0.035, 0.0),
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

/// Water overlay for one chunk, or `None` if the chunk touches no water. Alpha carries the
/// smooth water weight so the shore matches the ground layers.
pub fn water_mesh(
    field: &[Weights],
    is_water: impl Fn(u16) -> bool,
    color: impl Fn(u16) -> [f32; 4],
) -> Option<Mesh> {
    let mut positions = Vec::new();
    let mut cols = Vec::new();
    let mut idx = Vec::new();
    for ty in 0..CHUNK {
        for tx in 0..CHUNK {
            let cell_i = (ty * CHUNK + tx) as usize * VERTS;
            let cell = &field[cell_i..cell_i + VERTS];
            let wet_weight = |w: &Weights| -> (f32, [f32; 3]) {
                let mut weight = 0.0;
                let mut rgb = [0.0f32; 3];
                for (t, k) in w.iter() {
                    if is_water(*t) {
                        weight += k;
                        let c = color(*t);
                        for ch in 0..3 {
                            rgb[ch] += c[ch] * k;
                        }
                    }
                }
                if weight > 0.0 {
                    for ch in rgb.iter_mut() {
                        *ch /= weight;
                    }
                }
                (weight, rgb)
            };
            if cell.iter().all(|w| wet_weight(w).0 < 0.3) {
                continue;
            }
            let b = positions.len() as u32;
            for sy in 0..=SUB {
                for sx in 0..=SUB {
                    let (weight, rgb) = wet_weight(&cell[(sy * (SUB + 1) + sx) as usize]);
                    positions.push(cell_vertex(tx, ty, sx, sy));
                    cols.push([rgb[0], rgb[1], rgb[2], weight]);
                }
            }
            push_cell_indices(&mut idx, b);
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
