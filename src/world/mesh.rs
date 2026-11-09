//! Turns a chunk's blocks into one mesh. Only faces that look at the camera and touch air
//! are emitted. Each vertex is already in screen texels with its depth in z, so the depth
//! buffer sorts blocks, and anything standing among them, per pixel.
//!
//! Colour comes from the tile atlas; the vertex colour carries light. Light is the face's
//! angle to the sun, ambient occlusion in corners, and shadows cast by blocks between the
//! vertex and the sun, tinted cool in shade and warm in sun.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::sprite_render::{AlphaMode2d, Material2d};

use super::iso::View;
use super::tiles::{TILE, Tiles, atlas};
use super::{AIR, CHUNK, HEIGHT};
use crate::core::packs::LoadedPacks;
use crate::core::{BlockDef, Registry, hash01};

/// Brightness of the three visible faces: top, left, right.
pub const SHADE: [f32; 3] = [1.0, 0.80, 0.60];
/// Brightness by how open a corner is: 0 is boxed in by two neighbours, 3 is clear.
pub const OCCLUSION: [f32; 4] = [0.52, 0.70, 0.86, 1.0];
/// Share of light a cast shadow removes.
pub const SHADOW: f32 = 0.40;
/// Brightness added per block of height, so terraces read apart even where no side shows.
const RISE: f32 = 0.025;
/// The height that gets plain brightness.
const MID: f32 = 12.0;
/// Direction to the sun in view space: behind and to the left, so the right face is the
/// dark one and shadows fall to the right on screen. It rises one block per unit.
const SUN: Vec3 = Vec3::new(-0.6, 0.3, 1.0);
/// How many blocks of height a shadow ray climbs before giving up.
const SHADOW_REACH: i32 = 8;
/// Horizontal distance a block's shadow or occlusion can reach, for marking meshes dirty.
pub const LIGHT_REACH: i32 = 6;
/// Colour of light in full shade and in full sun.
const SHADE_TINT: Vec3 = Vec3::new(0.80, 0.88, 1.08);
const SUN_TINT: Vec3 = Vec3::new(1.06, 1.0, 0.90);
/// Number of bands the shader quantises light into.
pub const LEVELS: f32 = 10.0;
/// How far, in texels, UVs stay inside their tile so sampling never bleeds.
const INSET: f32 = 0.01;

/// Draw every block edge (the classic grid look) instead of only steps and block changes.
#[derive(Resource, Default)]
pub struct Grid(pub bool);

/// The block material: the tile atlas lit by vertex colour, with outlines, banded light
/// and moving water done in `shaders/block.wgsl`.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct BlockArt {
    /// x: seconds, y and z: atlas size in texels, w: light bands.
    #[uniform(0)]
    pub params: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub atlas: Handle<Image>,
}

impl Material2d for BlockArt {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/block.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Opaque
    }
}

/// The material every chunk shares.
#[derive(Resource)]
pub struct BlockMaterial(pub Handle<BlockArt>);

pub fn setup(
    blocks: Res<Registry<BlockDef>>,
    packs: Option<Res<LoadedPacks>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<BlockArt>>,
    mut commands: Commands,
) {
    let (image, tiles) = atlas(&blocks, packs.as_deref());
    let params = Vec4::new(0.0, tiles.size.x as f32, tiles.size.y as f32, LEVELS);
    commands.insert_resource(BlockMaterial(materials.add(BlockArt {
        params,
        atlas: images.add(image),
    })));
    commands.insert_resource(tiles);
}

/// Feeds the clock to the shader for moving water.
pub fn tick(
    time: Res<Time>,
    handle: Option<Res<BlockMaterial>>,
    mut materials: ResMut<Assets<BlockArt>>,
) {
    if let Some(handle) = handle
        && let Some(mut m) = materials.get_mut(&handle.0)
    {
        m.params.x = time.elapsed_secs();
    }
}

/// Packs a face's outline mask and its water flag into the vertex alpha.
pub fn pack_flags(mask: u32, water: bool) -> f32 {
    (mask + 16 * water as u32) as f32 / 32.0
}

/// The inverse of [`pack_flags`]: `(outline mask, water)`.
pub fn unpack_flags(alpha: f32) -> (u32, bool) {
    let v = (alpha * 32.0).round() as u32;
    (v % 16, v >= 16)
}

/// Re-expresses an outline mask given per geometric edge as a mask per tile edge, for a
/// tile turned `rot` quarter turns or mirrored. Bit 0 is the `v = 0` row, bit 1 the `u = 1`
/// column, bit 2 the `v = 1` row, bit 3 the `u = 0` column.
pub fn tile_mask(mask: u32, rot: u32, flip: bool) -> u32 {
    (0..4)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| {
            let edge = if flip {
                [0, 3, 2, 1][i as usize]
            } else {
                (i + rot) % 4
            };
            1 << edge
        })
        .sum()
}

/// Tile coordinates 0..1 of corner `i` for a tile turned `rot` quarter turns or mirrored.
fn tile_corner(i: usize, rot: u32, flip: bool) -> Vec2 {
    const BASE: [Vec2; 4] = [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y];
    if flip {
        let c = BASE[i];
        Vec2::new(1.0 - c.x, c.y)
    } else {
        BASE[(i + rot as usize) % 4]
    }
}

/// Light colour for a brightness: cool when dim, warm when bright.
pub fn tint(light: f32) -> [f32; 3] {
    let t = ((light - 0.45) / 0.55).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    (SHADE_TINT.lerp(SUN_TINT, t) * light).to_array()
}

/// Vertex data for one chunk, in texels relative to the chunk origin.
#[derive(Default)]
pub struct Faces {
    pub positions: Vec<[f32; 3]>,
    /// Atlas coordinates 0..1.
    pub uvs: Vec<[f32; 2]>,
    /// Light in rgb, [`pack_flags`] in alpha.
    pub colors: Vec<[f32; 4]>,
    /// Per quad: split along corners 1-3 instead of 0-2, so light interpolates evenly.
    pub flips: Vec<bool>,
}

impl Faces {
    pub fn len(&self) -> usize {
        self.positions.len() / 4
    }

    /// The two triangles of quad `q` as corner numbers.
    pub fn triangles(&self, q: usize) -> [[usize; 3]; 2] {
        if self.flips.get(q).copied().unwrap_or(false) {
            [[1, 2, 3], [1, 3, 0]]
        } else {
            [[0, 1, 2], [0, 2, 3]]
        }
    }

    pub fn into_mesh(self) -> Mesh {
        let indices = (0..self.len())
            .flat_map(|q| {
                self.triangles(q)
                    .into_iter()
                    .flatten()
                    .map(move |i| (q * 4 + i) as u32)
            })
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

/// Whether something stands between `from` and the sun.
fn shadowed(from: Vec3, sun: Vec3, casts: &impl Fn(IVec3) -> bool) -> bool {
    for i in 1..=SHADOW_REACH * 2 {
        let q = from + sun * (i as f32 * 0.5);
        if q.z >= HEIGHT as f32 {
            return false;
        }
        if casts(q.floor().as_ivec3()) {
            return true;
        }
    }
    false
}

/// Whether the sun reaches `point`, for lighting things that are not blocks.
pub fn sunlit(point: Vec3, view: View, casts: impl Fn(IVec3) -> bool) -> bool {
    !shadowed(point, view.to_world(SUN), &casts)
}

/// Builds the visible faces of chunk `pos`. `block` answers for any world position,
/// including cells in neighbouring chunks.
pub fn chunk_faces(
    pos: IVec2,
    view: View,
    grid: bool,
    tiles: &Tiles,
    block: impl Fn(IVec3) -> u16,
) -> Faces {
    let mut out = Faces::default();
    let origin = (pos * CHUNK).extend(0);
    let (left, right) = view.sides();
    let sun = view.to_world(SUN);
    let water = |b: u16| tiles.water.get(b as usize).copied().unwrap_or(false);
    let family = |b: u16| tiles.family.get(b as usize).copied().unwrap_or(b);
    // Anything but air and water blocks light.
    let casts = |c: IVec3| {
        let b = block(c);
        b != AIR && !water(b)
    };
    // Normal, the four edge directions in corner order, which shade, which tile.
    let tangent = |n: IVec3| IVec3::new(-n.y, n.x, 0);
    let faces = [
        (
            IVec3::Z,
            [IVec3::NEG_Y, IVec3::X, IVec3::Y, IVec3::NEG_X],
            0,
            0,
        ),
        (
            left,
            [IVec3::NEG_Z, tangent(left), IVec3::Z, -tangent(left)],
            1,
            1,
        ),
        (
            right,
            [IVec3::NEG_Z, tangent(right), IVec3::Z, -tangent(right)],
            2,
            1,
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
                for (normal, edges, shade, tile) in faces {
                    if block(p + normal) != AIR {
                        continue;
                    }
                    let mut mask = 0u32;
                    for (bit, e) in edges.iter().enumerate() {
                        // The same face on the neighbouring block continues this surface
                        // only when that block is of the same family and is also exposed.
                        let q = p + *e;
                        let continues = family(block(q)) == family(id) && block(q + normal) == AIR;
                        if grid || !continues {
                            mask |= 1 << bit;
                        }
                    }
                    // Tops turn by quarters and sides mirror, so one tile does not repeat
                    // as an obvious grid.
                    let pick = hash01(0x5EED, p.x.wrapping_add(p.z * 131), p.y, tile);
                    let (rot, flip) = if water(id) {
                        // Waves must line up from block to block.
                        (0, false)
                    } else if tile == 0 {
                        ((pick * 4.0) as u32 % 4, false)
                    } else {
                        (0, pick < 0.5)
                    };
                    let flags = pack_flags(tile_mask(mask, rot, flip), water(id));
                    let glow = tiles.glow.get(id as usize).copied().unwrap_or(0.0);
                    let lit = normal.as_vec3().dot(sun) > 0.0;
                    let corners = corners(local, normal);
                    let centre = corners.iter().sum::<Vec3>() / 4.0;
                    let block_centre = local.as_vec3() + Vec3::splat(0.5);
                    let lift = 1.0 + (z as f32 - MID) * RISE;
                    let tile_origin = tiles.origin(id, tile);
                    let mut lights = [0.0f32; 4];
                    for (i, corner) in corners.iter().enumerate() {
                        // The two neighbours beside this corner and the one diagonal to
                        // it, all in the layer the face looks into.
                        let toward = (*corner - block_centre).signum().as_ivec3();
                        let mut sides = [IVec3::ZERO; 2];
                        let mut n = 0;
                        for a in 0..3 {
                            if normal[a] == 0 {
                                sides[n][a] = toward[a];
                                n += 1;
                            }
                        }
                        let (s1, s2) = (casts(p + normal + sides[0]), casts(p + normal + sides[1]));
                        let diagonal = casts(p + normal + sides[0] + sides[1]);
                        let open = if s1 && s2 {
                            0
                        } else {
                            3 - (s1 as usize + s2 as usize + diagonal as usize)
                        };
                        // Shadow is sampled a quarter of the way in from the corner.
                        let sample =
                            origin.as_vec3() + corner.lerp(centre, 0.5) + normal.as_vec3() * 0.02;
                        let shade_cast = lit && shadowed(sample, sun, &casts);
                        let light = SHADE[shade]
                            * OCCLUSION[open]
                            * lift
                            * if shade_cast { 1.0 - SHADOW } else { 1.0 };
                        lights[i] = light.max(glow);
                        let c = tint(lights[i]);
                        let uv = (tile_origin
                            + Vec2::splat(INSET)
                            + tile_corner(i, rot, flip) * (TILE as f32 - 2.0 * INSET))
                            / tiles.size.as_vec2();
                        out.positions.push(view.project(*corner).to_array());
                        out.uvs.push(uv.to_array());
                        out.colors.push([c[0], c[1], c[2], flags]);
                    }
                    // Split along the brighter diagonal so a dark corner stays a corner.
                    out.flips
                        .push(lights[1] + lights[3] > lights[0] + lights[2]);
                }
            }
        }
    }
    out
}

/// The four corners of the face of block `p` with outward `normal`, ordered so that edge
/// `i` runs from corner `i` to corner `i + 1`.
pub fn corners(p: IVec3, normal: IVec3) -> [Vec3; 4] {
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

    fn tiles() -> Tiles {
        let mut blocks = Registry::<BlockDef>::default();
        blocks.insert(BlockDef::air());
        blocks.insert(ron::from_str(r#"(id: "a", color: (200, 100, 50))"#).unwrap());
        blocks.insert(ron::from_str(r#"(id: "b", color: (10, 20, 30))"#).unwrap());
        blocks
            .insert(ron::from_str(r#"(id: "lamp", color: (250, 240, 200), glow: 0.95)"#).unwrap());
        atlas(&blocks, None).1
    }

    fn masks(f: &Faces) -> Vec<u32> {
        f.colors
            .chunks(4)
            .map(|q| unpack_flags(q[0][3]).0)
            .collect()
    }

    /// Brightness of each corner of quad `q`, with the tint taken back out.
    fn lights(f: &Faces, q: usize) -> Vec<f32> {
        f.colors[q * 4..q * 4 + 4]
            .iter()
            .map(|c| c[0].max(c[1]).max(c[2]))
            .collect()
    }

    #[test]
    fn flags_round_trip_and_follow_the_tile() {
        for mask in 0..16 {
            for water in [false, true] {
                assert_eq!(unpack_flags(pack_flags(mask, water)), (mask, water));
            }
        }
        // A quarter turn moves each edge one place on; a mirror swaps left and right.
        assert_eq!(tile_mask(0b0001, 1, false), 0b0010);
        assert_eq!(tile_mask(0b1000, 1, false), 0b0001);
        assert_eq!(tile_mask(0b0010, 0, true), 0b1000);
        assert_eq!(tile_mask(0b0101, 0, true), 0b0101);
        assert_eq!(tile_mask(0b1111, 3, false), 0b1111);
    }

    #[test]
    fn a_lone_block_shows_three_outlined_faces() {
        let world = |p: IVec3| (p == IVec3::new(3, 4, 5)) as u16;
        for rot in 0..4 {
            let f = chunk_faces(IVec2::ZERO, View { rot }, false, &tiles(), world);
            assert_eq!(f.len(), 3);
            // Every edge is a silhouette.
            assert_eq!(masks(&f), vec![15, 15, 15]);
            // Top, left, right get darker in that order.
            let (t, l, r) = (lights(&f, 0)[0], lights(&f, 1)[0], lights(&f, 2)[0]);
            assert!(t > l && l > r, "{t} {l} {r}");
        }
    }

    #[test]
    fn a_flat_floor_is_one_clean_evenly_lit_surface() {
        let floor = |p: IVec3| (p.z == 0) as u16;
        let t = tiles();
        let f = chunk_faces(IVec2::ZERO, View::default(), false, &t, floor);
        // Top faces only: neighbours hide every side, including across chunk borders.
        assert_eq!(f.len(), (CHUNK * CHUNK) as usize);
        assert!(masks(&f).iter().all(|m| *m == 0));
        let first = f.colors[0];
        assert!(f.colors.iter().all(|c| *c == first));
        // Every corner samples inside block 1's top tile.
        let o = t.origin(1, 0) / t.size.as_vec2();
        let span = TILE as f32 / t.size.as_vec2();
        assert!(f.uvs.iter().all(|uv| {
            uv[0] > o.x && uv[0] < o.x + span.x && uv[1] > o.y && uv[1] < o.y + span.y
        }));
        let g = chunk_faces(IVec2::ZERO, View::default(), true, &t, floor);
        assert!(masks(&g).iter().all(|m| *m == 15));
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
        let f = chunk_faces(IVec2::ZERO, View::default(), false, &tiles(), floor);
        let inked = masks(&f).iter().filter(|m| **m != 0).count();
        assert_eq!(inked, 2 * CHUNK as usize);
    }

    #[test]
    fn faces_carry_true_depth() {
        let world = |p: IVec3| (p == IVec3::new(1, 1, 0)) as u16;
        let f = chunk_faces(IVec2::ZERO, View::default(), false, &tiles(), world);
        // The top face spans depth 3 (far corner) to 5 (near corner).
        let top: Vec<f32> = f.positions[..4].iter().map(|p| p[2]).collect();
        assert_eq!(top, vec![3.0, 4.0, 5.0, 4.0]);
    }

    /// The top face of the floor block at `(x, y)` in a mesh of a floor at z 0.
    fn floor_quad(f: &Faces, x: i32, y: i32) -> usize {
        let want = View::default().project(Vec3::new(x as f32, y as f32, 1.0));
        (0..f.len())
            .find(|q| Vec3::from(f.positions[q * 4]).distance(want) < 1e-3)
            .expect("floor face")
    }

    #[test]
    fn corners_against_a_wall_are_occluded() {
        // A floor with one block standing on it at (5, 5).
        let world = |p: IVec3| (p.z == 0 || p == IVec3::new(5, 5, 1)) as u16;
        let f = chunk_faces(IVec2::ZERO, View::default(), false, &tiles(), world);
        // The floor block at (6, 5) touches the standing block along its x = 6 edge:
        // corners 0 and 3 are in the crease, 1 and 2 are clear.
        let l = lights(&f, floor_quad(&f, 6, 5));
        assert!(l[0] < l[1] && l[3] < l[2], "{l:?}");
        // Far from it nothing is occluded.
        let far = lights(&f, floor_quad(&f, 12, 2));
        assert!(far.iter().all(|v| (*v - far[0]).abs() < 1e-6));
        assert!(far[0] > l[1] - 1e-6);
    }

    #[test]
    fn a_pillar_casts_a_shadow_away_from_the_sun() {
        let world = |p: IVec3| (p.z == 0 || (p.x == 8 && p.y == 8 && p.z < 4)) as u16;
        let f = chunk_faces(IVec2::ZERO, View::default(), false, &tiles(), world);
        let open = lights(&f, floor_quad(&f, 2, 2))[0];
        // The sun is toward -x and +y, so the shadow lies toward +x and -y. A one-block
        // pillar throws a narrow shadow: it darkens part of the cells it crosses.
        let dark = lights(&f, floor_quad(&f, 10, 7));
        let shaded = dark.iter().filter(|v| **v < open * 0.7).count();
        assert!(shaded >= 2, "{dark:?} vs {open}");
        // The sunward side stays lit.
        let lit = lights(&f, floor_quad(&f, 6, 9));
        assert!(lit.iter().all(|v| (*v - open).abs() < 1e-6), "{lit:?}");
    }

    #[test]
    fn glowing_blocks_ignore_shade() {
        let world = |p: IVec3| if p == IVec3::new(3, 3, 3) { 3 } else { 0 };
        let f = chunk_faces(IVec2::ZERO, View::default(), false, &tiles(), world);
        // Even the dark right face is at least as bright as the glow.
        assert!(lights(&f, 2).iter().all(|v| *v >= 0.95));
    }
}
