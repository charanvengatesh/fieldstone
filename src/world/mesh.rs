//! Turns a chunk's blocks into one mesh. Only faces that look at the camera and touch air
//! are emitted. Each vertex is already in screen texels with its depth in z, so the depth
//! buffer sorts blocks, and anything standing among them, per pixel.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite_render::AlphaMode2d;

use super::iso::View;
use super::{AIR, CHUNK, HEIGHT};
use crate::core::{BlockDef, Registry, Rgb};

/// Brightness of the three visible faces: top, left, right.
const SHADE: [f32; 3] = [1.0, 0.8, 0.62];
/// Brightness added per block of height, so terraces read apart even where no side shows.
const RISE: f32 = 0.05;
/// The height that gets a block's plain colour.
const MID: f32 = 12.0;
/// How dark an outline texel is relative to its face.
const INK: u8 = 92;
/// Texels per side of one cell in the edge texture.
const CELL: u32 = 16;

/// Draw every block edge (the classic grid look) instead of only steps and block changes.
#[derive(Resource, Default)]
pub struct Grid(pub bool);

/// The material every chunk shares: white cells with dark borders, tinted per vertex.
#[derive(Resource)]
pub struct BlockMaterial(pub Handle<ColorMaterial>);

/// Linear colours per block for the top, left and right faces.
#[derive(Resource)]
pub struct Palette(Vec<[[f32; 4]; 3]>);

impl Palette {
    pub fn new(blocks: &Registry<BlockDef>) -> Self {
        let lit = |rgb: Rgb, shade: f32| {
            let c = Color::srgb_u8(rgb[0], rgb[1], rgb[2]).to_linear();
            [c.red * shade, c.green * shade, c.blue * shade, 1.0]
        };
        Self(
            blocks
                .iter()
                .map(|(_, b)| {
                    [
                        lit(b.top.unwrap_or(b.color), SHADE[0]),
                        lit(b.color, SHADE[1]),
                        lit(b.color, SHADE[2]),
                    ]
                })
                .collect(),
        )
    }
}

pub fn setup(
    blocks: Res<Registry<BlockDef>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut commands: Commands,
) {
    let texture = images.add(edge_texture());
    commands.insert_resource(BlockMaterial(materials.add(ColorMaterial {
        color: Color::WHITE,
        alpha_mode: AlphaMode2d::Opaque,
        texture: Some(texture),
        ..default()
    })));
    commands.insert_resource(Palette::new(&blocks));
}

/// A 4 x 4 sheet of face cells, one per edge mask. Bit 0 inks the v = 0 row, bit 1 the
/// u = 1 column, bit 2 the v = 1 row, bit 3 the u = 0 column.
pub fn edge_texture() -> Image {
    let size = CELL * 4;
    let mut data = vec![255u8; (size * size * 4) as usize];
    for mask in 0..16u32 {
        let (ox, oy) = ((mask % 4) * CELL, (mask / 4) * CELL);
        for y in 0..CELL {
            for x in 0..CELL {
                let inked = (mask & 1 != 0 && y == 0)
                    || (mask & 2 != 0 && x == CELL - 1)
                    || (mask & 4 != 0 && y == CELL - 1)
                    || (mask & 8 != 0 && x == 0);
                if inked {
                    let i = (((oy + y) * size + ox + x) * 4) as usize;
                    data[i..i + 3].fill(INK);
                }
            }
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::nearest();
    img
}

/// Vertex data for one chunk, in texels relative to the chunk origin.
#[derive(Default)]
pub struct Faces {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
}

impl Faces {
    pub fn len(&self) -> usize {
        self.positions.len() / 4
    }

    pub fn into_mesh(self) -> Mesh {
        let quads = self.len() as u32;
        let indices = (0..quads)
            .flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i))
            .collect();
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(indices))
    }
}

/// Builds the visible faces of chunk `pos`. `block` answers for any world position,
/// including cells in neighbouring chunks.
pub fn chunk_faces(
    pos: IVec2,
    view: View,
    grid: bool,
    palette: &Palette,
    block: impl Fn(IVec3) -> u16,
) -> Faces {
    let mut out = Faces::default();
    let origin = (pos * CHUNK).extend(0);
    let (left, right) = view.sides();
    // Normal, the four edge directions in corner order, and which shade to use.
    let tangent = |n: IVec3| IVec3::new(-n.y, n.x, 0);
    let faces = [
        (
            IVec3::Z,
            [IVec3::NEG_Y, IVec3::X, IVec3::Y, IVec3::NEG_X],
            0,
        ),
        (
            left,
            [IVec3::NEG_Z, tangent(left), IVec3::Z, -tangent(left)],
            1,
        ),
        (
            right,
            [IVec3::NEG_Z, tangent(right), IVec3::Z, -tangent(right)],
            2,
        ),
    ];
    for z in 0..HEIGHT {
        for y in 0..CHUNK {
            for x in 0..CHUNK {
                let local = IVec3::new(x, y, z);
                let p = origin + local;
                let id = block(p);
                if id == AIR {
                    continue;
                }
                for (normal, edges, shade) in faces {
                    if block(p + normal) != AIR {
                        continue;
                    }
                    let mut mask = 0u32;
                    for (bit, e) in edges.iter().enumerate() {
                        // The same face on the neighbouring block continues this surface
                        // only when that block is the same kind and is also exposed.
                        let q = p + *e;
                        let continues = block(q) == id && block(q + normal) == AIR;
                        if grid || !continues {
                            mask |= 1 << bit;
                        }
                    }
                    let lift = 1.0 + (z as f32 - MID) * RISE;
                    let color = palette
                        .0
                        .get(id as usize)
                        .map_or([1.0; 4], |c| c[shade])
                        .map(|v| (v * lift).min(1.0));
                    let corners = corners(local, normal);
                    let (u0, v0) = ((mask % 4) as f32 / 4.0, (mask / 4) as f32 / 4.0);
                    for (corner, uv) in
                        corners
                            .iter()
                            .zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
                    {
                        out.positions.push(view.project(*corner).to_array());
                        out.uvs.push([u0 + uv[0] / 4.0, v0 + uv[1] / 4.0]);
                        out.colors.push(color);
                    }
                }
            }
        }
    }
    out
}

/// The four corners of the face of block `p` with outward `normal`, ordered so that edge
/// `i` runs from corner `i` to corner `i + 1`.
fn corners(p: IVec3, normal: IVec3) -> [Vec3; 4] {
    let base = p.as_vec3();
    if normal == IVec3::Z {
        return [
            base + Vec3::new(0.0, 0.0, 1.0),
            base + Vec3::new(1.0, 0.0, 1.0),
            base + Vec3::new(1.0, 1.0, 1.0),
            base + Vec3::new(0.0, 1.0, 1.0),
        ];
    }
    let n = normal.as_vec3();
    let t = Vec3::new(-n.y, n.x, 0.0);
    let start = base + Vec3::splat(0.5) + n * 0.5 - t * 0.5 - Vec3::Z * 0.5;
    [start, start + t, start + t + Vec3::Z, start + Vec3::Z]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Registry;

    fn palette() -> Palette {
        let mut blocks = Registry::<BlockDef>::default();
        blocks.insert(BlockDef::air());
        blocks.insert(ron::from_str(r#"(id: "a", color: (200, 100, 50))"#).unwrap());
        blocks.insert(ron::from_str(r#"(id: "b", color: (10, 20, 30))"#).unwrap());
        Palette::new(&blocks)
    }

    #[test]
    fn a_lone_block_shows_three_outlined_faces() {
        let world = |p: IVec3| (p == IVec3::new(3, 4, 5)) as u16;
        for rot in 0..4 {
            let f = chunk_faces(IVec2::ZERO, View { rot }, false, &palette(), world);
            assert_eq!(f.len(), 3);
            // Every edge is a silhouette: all three faces use the fully inked cell.
            assert!(f.uvs.chunks(4).all(|q| q[0] == [0.75, 0.75]));
        }
    }

    #[test]
    fn a_flat_floor_is_one_clean_surface() {
        let floor = |p: IVec3| (p.z == 0) as u16;
        let f = chunk_faces(IVec2::ZERO, View::default(), false, &palette(), floor);
        // Top faces only: neighbours hide every side, including across chunk borders.
        assert_eq!(f.len(), (CHUNK * CHUNK) as usize);
        assert!(f.uvs.chunks(4).all(|q| q[0] == [0.0, 0.0]));
        let g = chunk_faces(IVec2::ZERO, View::default(), true, &palette(), floor);
        assert!(g.uvs.chunks(4).all(|q| q[0] == [0.75, 0.75]));
    }

    #[test]
    fn a_change_of_block_is_inked_on_both_sides() {
        // Kind 1 for x < 8, kind 2 from x = 8.
        let floor = |p: IVec3| {
            if p.z != 0 {
                0
            } else if p.x < 8 {
                1
            } else {
                2
            }
        };
        let f = chunk_faces(IVec2::ZERO, View::default(), false, &palette(), floor);
        let inked = f.uvs.chunks(4).filter(|q| q[0] != [0.0, 0.0]).count();
        assert_eq!(inked, 2 * CHUNK as usize);
    }

    #[test]
    fn faces_carry_true_depth() {
        let world = |p: IVec3| (p == IVec3::new(1, 1, 0)) as u16;
        let f = chunk_faces(IVec2::ZERO, View::default(), false, &palette(), world);
        // The top face spans depth 3 (far corner) to 5 (near corner).
        let top: Vec<f32> = f.positions[..4].iter().map(|p| p[2]).collect();
        assert_eq!(top, vec![3.0, 4.0, 5.0, 4.0]);
    }

    #[test]
    fn edge_texture_inks_the_requested_sides() {
        let img = edge_texture();
        let data = img.data.as_ref().unwrap();
        let px = |x: u32, y: u32| data[((y * CELL * 4 + x) * 4) as usize];
        // Mask 0 is clean, mask 1 inks its top row only.
        assert_eq!(px(0, 0), 255);
        assert_eq!(px(CELL + 3, 0), INK);
        assert_eq!(px(CELL + 3, 1), 255);
        // Mask 15 (last cell) inks all four borders.
        assert_eq!(px(3 * CELL, 3 * CELL + 5), INK);
        assert_eq!(px(4 * CELL - 1, 3 * CELL + 5), INK);
        assert_eq!(px(3 * CELL + 5, 4 * CELL - 1), INK);
        assert_eq!(px(3 * CELL + 5, 3 * CELL + 5), 255);
    }
}
