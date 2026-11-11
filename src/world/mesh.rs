//! Turns a chunk's blocks into one mesh of every face that touches air, in world space.
//! The view's projection matrix is the mesh's transform, which puts each vertex on screen
//! with its depth in z, so the depth buffer sorts blocks, and anything standing among
//! them, per pixel, and turning the view never rebuilds a mesh.
//!
//! Colour comes from the tile atlas; the vertex colour carries light. Light is the face's
//! angle to the sun, ambient occlusion in corners, and shadows cast by blocks between the
//! vertex and the sun, tinted cool in shade and warm in sun.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError,
};

use super::tiles::{TILE, Tiles, atlas};
use super::{AIR, CHUNK, HEIGHT};
use crate::core::packs::LoadedPacks;
use crate::core::time::{Daylight, GameTime, daylight};
use crate::core::{BlockDef, Registry, hash01};

/// Brightness of a face that looks straight up.
pub const TOP_SHADE: f32 = 1.0;
/// Brightness of a side face square to the sun's compass direction, and how much it gains
/// or loses turned fully toward or away from it.
const SIDE_SHADE: f32 = 0.70;
const SIDE_SWING: f32 = 0.20;
/// Brightness by how open a corner is: 0 is boxed in by two neighbours, 3 is clear.
pub const OCCLUSION: [f32; 4] = [0.42, 0.62, 0.84, 1.0];
/// Share of light a cast shadow removes.
pub const SHADOW: f32 = 0.52;
/// Brightness added per block of height, so terraces read apart even where no side shows.
const RISE: f32 = 0.04;
/// The height that gets plain brightness.
const MID: f32 = 12.0;
/// Direction to the sun, fixed in the world. It rises one block per unit, so shadows are
/// a block long per block of height: long enough that a thing above the ground always
/// shows its shadow below it.
pub const SUN: Vec3 = Vec3::new(-0.9, 0.45, 1.0);

/// Which lighting effects the mesher computes. Turning one off makes meshing cheaper.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quality {
    pub shadows: bool,
    pub occlusion: bool,
}

impl Default for Quality {
    fn default() -> Self {
        Self {
            shadows: true,
            occlusion: true,
        }
    }
}

/// Brightness of a face from its normal alone.
pub fn face_shade(normal: IVec3) -> f32 {
    if normal.z > 0 {
        return TOP_SHADE;
    }
    let toward = normal.as_vec3().truncate().dot(SUN.truncate().normalize());
    SIDE_SHADE + SIDE_SWING * toward
}
/// How many blocks of height a shadow ray climbs before giving up.
const SHADOW_REACH: i32 = 8;
/// Horizontal distance a block's shadow or occlusion can reach, for marking meshes dirty.
pub const LIGHT_REACH: i32 = 9;
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

/// What the block shader reads. It changes a few times a minute as the day moves, never
/// every frame: the clock comes from Bevy's view globals.
#[derive(ShaderType, Clone, Copy, Debug, PartialEq)]
pub struct BlockParams {
    /// x: unused, y and z: atlas size in texels, w: light bands.
    pub params: Vec4,
    /// x: daylight 0..1, y: time of day 0..1, z, w: unused.
    pub day: Vec4,
    /// The camera's focus (xyz) and, in `toward_eye`, the unit vector from it to the eye:
    /// a point's view depth past the focus is `dot(focus - p, toward_eye)`.
    pub focus: Vec4,
    pub toward_eye: Vec4,
    /// Colour of sunlight now.
    pub sun_color: Vec4,
    /// Colour of the light left in shade and at night.
    pub ambient_color: Vec4,
    /// Haze: rgb the colour of distance (the sky), w its full strength.
    pub haze: Vec4,
}

/// Depth behind the camera's focus over which the haze reaches full strength, in depth
/// units (blocks of ground toward the back of the screen).
pub const HAZE_REACH: f32 = 28.0;
/// Share of the sky colour mixed in at full haze.
pub const HAZE_STRENGTH: f32 = 0.22;

/// How much of the haze a world point gets: nothing at or in front of the focus plane,
/// full `HAZE_REACH` blocks behind it along the view axis.
pub fn haze_at(p: Vec3, focus: Vec3, toward_eye: Vec3) -> f32 {
    ((focus - p).dot(toward_eye) / HAZE_REACH).clamp(0.0, 1.0)
}

/// Mixes `color` toward the haze colour by `amount` of its strength.
pub fn hazed(color: Vec3, haze: Vec4, amount: f32) -> Vec3 {
    color.lerp(haze.truncate(), amount * haze.w)
}

/// Every material here is unlit and two-sided: no shadow maps, no prepass, no backface
/// culling (the mesher emits what is visible, and the figure's quads may wind either way).
fn unlit(descriptor: &mut RenderPipelineDescriptor) {
    descriptor.primitive.cull_mode = None;
}

/// The block material: the tile atlas lit by vertex colour, with outlines, banded light,
/// in `shaders/block.wgsl`. Mask, so cutout texels are dropped and depth still written.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct BlockArt {
    #[uniform(0)]
    pub block: BlockParams,
    #[texture(1)]
    #[sampler(2)]
    pub atlas: Handle<Image>,
}

impl Material for BlockArt {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/block.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Mask(0.5)
    }
    fn enable_shadows() -> bool {
        false
    }
    fn enable_prepass() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        unlit(descriptor);
        Ok(())
    }
}

/// The water material: the same inputs as [`BlockArt`], blended over what lies under the
/// surface, in `shaders/water.wgsl`. Drawn after the solid pass without writing depth.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct WaterArt {
    #[uniform(0)]
    pub block: BlockParams,
    #[texture(1)]
    #[sampler(2)]
    pub atlas: Handle<Image>,
}

impl Material for WaterArt {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/water.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    fn enable_shadows() -> bool {
        false
    }
    fn enable_prepass() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        unlit(descriptor);
        Ok(())
    }
}

/// Flat-coloured things: vertex colour times `tint`, optionally through a texture (the
/// crack overlay), in `shaders/flat.wgsl`. Used by the figure, particles, the highlight and
/// the ground shadow.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct FlatArt {
    #[uniform(0)]
    pub flat: FlatParams,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Option<Handle<Image>>,
    pub blend: bool,
}

#[derive(ShaderType, Clone, Copy, Debug, PartialEq)]
pub struct FlatParams {
    pub tint: Vec4,
    /// x: 1 when the texture is to be sampled, y: 1 to discard texels under half alpha.
    pub flags: Vec4,
}

impl FlatArt {
    pub fn plain(tint: Vec4, blend: bool) -> Self {
        Self {
            flat: FlatParams {
                tint,
                flags: Vec4::ZERO,
            },
            texture: None,
            blend,
        }
    }
}

impl Material for FlatArt {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/flat.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        if self.blend {
            AlphaMode::Blend
        } else {
            AlphaMode::Mask(0.5)
        }
    }
    fn enable_shadows() -> bool {
        false
    }
    fn enable_prepass() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        unlit(descriptor);
        Ok(())
    }
}

/// The silhouette material: the figure's own vertex colours as a flat tint, drawn only
/// where the depth buffer holds something nearer (`shaders/silhouette.wgsl`, depth test
/// turned round; 3D depth is reversed, nearer is greater, so `Less` keeps only covered
/// texels).
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct SilhouetteArt {
    #[uniform(0)]
    pub tint: Vec4,
}

impl Material for SilhouetteArt {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/silhouette.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
    fn enable_shadows() -> bool {
        false
    }
    fn enable_prepass() -> bool {
        false
    }
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        unlit(descriptor);
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(CompareFunction::Less);
        }
        Ok(())
    }
}

/// The material every chunk shares.
#[derive(Resource)]
pub struct BlockMaterial(pub Handle<BlockArt>);

/// The material every chunk's water shares.
#[derive(Resource)]
pub struct WaterMaterial(pub Handle<WaterArt>);

/// The tile atlas as an image, for anything drawn with plain materials: the crack overlay.
#[derive(Resource)]
pub struct AtlasImage(pub Handle<Image>);

pub fn setup(
    blocks: Res<Registry<BlockDef>>,
    packs: Option<Res<LoadedPacks>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<BlockArt>>,
    mut waters: ResMut<Assets<WaterArt>>,
    mut commands: Commands,
) {
    let (image, tiles) = atlas(&blocks, packs.as_deref());
    let params = Vec4::new(0.0, tiles.size.x as f32, tiles.size.y as f32, LEVELS);
    let block = BlockParams {
        params,
        day: Vec4::new(1.0, 0.5, 0.0, 0.0),
        focus: Vec4::ZERO,
        toward_eye: Vec4::new(0.0, 0.0, 1.0, 0.0),
        sun_color: Vec4::ONE,
        ambient_color: Vec4::ONE,
        haze: Vec4::new(0.36, 0.60, 0.92, HAZE_STRENGTH),
    };
    let atlas = images.add(image);
    commands.insert_resource(BlockMaterial(materials.add(BlockArt {
        block,
        atlas: atlas.clone(),
    })));
    commands.insert_resource(WaterMaterial(waters.add(WaterArt {
        block,
        atlas: atlas.clone(),
    })));
    commands.insert_resource(AtlasImage(atlas));
    commands.insert_resource(tiles);
}

/// Hands the shader the daylight, but only when it has moved by a visible step: writing a
/// material asset makes the renderer rebuild its bind group, so this stays quiet during
/// the long stretches where nothing changes.
pub fn tick(
    clock: Res<GameTime>,
    cam: Res<crate::cam::Cam>,
    handle: Option<Res<BlockMaterial>>,
    water: Option<Res<WaterMaterial>>,
    mut materials: ResMut<Assets<BlockArt>>,
    mut waters: ResMut<Assets<WaterArt>>,
) {
    let (Some(handle), Some(water)) = (handle, water) else {
        return;
    };
    let Some(current) = materials.get(&handle.0).map(|m| m.block) else {
        return;
    };
    let day = daylight(clock.frac());
    let sky = crate::core::time::sky_color(clock.frac());
    // The focus moves in whole blocks and the view direction in hundredths, so walking or
    // turning writes the material a few times a second and standing still never does.
    let wanted = BlockParams {
        params: current.params,
        day: Vec4::new(quantize(day.level), clock.frac(), 0.0, 0.0),
        focus: cam.focus.round().extend(0.0),
        toward_eye: cam
            .toward_eye()
            .map(|v| (v * 100.0).round() / 100.0)
            .extend(0.0),
        sun_color: day.sun.map(quantize).extend(1.0),
        ambient_color: day.ambient.map(quantize).extend(1.0),
        haze: sky.map(quantize).extend(HAZE_STRENGTH),
    };
    let same = |a: Vec4, b: Vec4| (a - b).abs().max_element() < 1e-6;
    if same(wanted.day.with_y(0.0), current.day.with_y(0.0))
        && same(wanted.focus, current.focus)
        && same(wanted.toward_eye, current.toward_eye)
        && same(wanted.sun_color, current.sun_color)
        && same(wanted.ambient_color, current.ambient_color)
        && same(wanted.haze, current.haze)
    {
        return;
    }
    if let Some(mut m) = materials.get_mut(&handle.0) {
        m.block = wanted;
    }
    if let Some(mut w) = waters.get_mut(&water.0) {
        w.block = wanted;
    }
}

/// Rounds to 1/255, the smallest step a colour on screen can take.
fn quantize(v: f32) -> f32 {
    (v * 255.0).round() / 255.0
}

/// Packs a face's outline mask, its water flag, whether it faces up, whether it lies under
/// water, and which of its edges are drops (the ground falls away past them, inked
/// heavier) into the vertex alpha.
pub fn pack_flags(mask: u32, water: bool, top: bool, submerged: bool, drop: u32) -> f32 {
    (mask + 16 * water as u32 + 32 * top as u32 + 64 * submerged as u32 + 128 * (drop & 15)) as f32
        / 2048.0
}

/// The inverse of [`pack_flags`]: `(outline mask, water, top, submerged, drop mask)`.
pub fn unpack_flags(alpha: f32) -> (u32, bool, bool, bool, u32) {
    let v = (alpha * 2048.0).round() as u32;
    (
        v % 16,
        (v / 16) % 2 == 1,
        (v / 32) % 2 == 1,
        (v / 64) % 2 == 1,
        (v / 128) % 16,
    )
}

/// How the ground under water is tinted: bluer and a little darker.
pub const SUBMERGED: Vec3 = Vec3::new(0.50, 0.68, 0.92);
/// Opacity of the water surface.
pub const WATER_ALPHA: f32 = 0.66;

/// Colour of a block's light.
pub const TORCH: Vec3 = Vec3::new(1.0, 0.80, 0.55);

/// Composes the light at a texel from the vertex channels and the time of day, as the
/// shader does: `channels` is (sun, block light, ambient). Returns linear rgb before
/// banding.
pub fn compose(channels: Vec3, day: &Daylight) -> Vec3 {
    let sun = Vec3::from(tint(channels.x)) * day.sun * day.level;
    let ambient = day.ambient * channels.z * (0.22 * (1.0 - day.level));
    let torch = TORCH * channels.y * 1.15;
    sun + ambient + torch
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

/// Vertex data for one chunk.
#[derive(Default, Clone)]
pub struct Faces {
    /// Block units relative to the chunk origin.
    pub positions: Vec<[f32; 3]>,
    /// Atlas coordinates 0..1.
    pub uvs: Vec<[f32; 2]>,
    /// Light channels: sun, block light, ambient; [`pack_flags`] in alpha. See [`compose`].
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
pub fn sunlit(point: Vec3, casts: impl Fn(IVec3) -> bool) -> bool {
    !shadowed(point, SUN, &casts)
}

/// The meshes of one chunk: the solid faces and, drawn after them with blending, the
/// water surfaces.
#[derive(Default)]
pub struct ChunkFaces {
    pub solid: Faces,
    pub water: Faces,
}

/// A quad about to be lit and pushed.
struct Quad {
    /// The cell the quad belongs to, in the world.
    p: IVec3,
    /// Direction the quad faces, for shade and for which cells it looks into. A cross quad
    /// looks up from its own cell.
    normal: IVec3,
    corners: [Vec3; 4],
    /// Outline mask by tile edge, after turning.
    mask: u32,
    /// Edges of a top face past which the ground falls away, by tile edge.
    drop: u32,
    tile: u32,
    rot: u32,
    flip: bool,
    /// A solid face against water: tinted under the surface.
    submerged: bool,
    cross: bool,
}

/// Builds every exposed face of chunk `pos` except undersides, which no view shows.
/// `block` answers for any world position, including cells in neighbouring chunks.
pub fn chunk_faces(
    pos: IVec2,
    grid: bool,
    quality: Quality,
    tiles: &Tiles,
    light: &super::light::Grid,
    block: impl Fn(IVec3) -> u16,
) -> ChunkFaces {
    let mut out = ChunkFaces::default();
    let origin = (pos * CHUNK).extend(0);
    let water = |b: u16| tiles.water.get(b as usize).copied().unwrap_or(false);
    let cross = |b: u16| tiles.cross.get(b as usize).copied().unwrap_or(false);
    let clear = |b: u16| b == AIR || tiles.see_through.get(b as usize).copied().unwrap_or(false);
    let family = |b: u16| tiles.family.get(b as usize).copied().unwrap_or(b);
    // Cubes block the sun and darken corners, leaves with gaps included; water and props
    // do not.
    let casts = |c: IVec3| {
        let b = block(c);
        b != AIR && !water(b) && !cross(b)
    };
    // Normal, the four edge directions in corner order, which tile.
    let side = |n: IVec3| {
        let t = IVec3::new(-n.y, n.x, 0);
        (n, [IVec3::NEG_Z, t, IVec3::Z, -t], 1)
    };
    let faces = [
        (
            IVec3::Z,
            [IVec3::NEG_Y, IVec3::X, IVec3::Y, IVec3::NEG_X],
            0,
        ),
        side(IVec3::X),
        side(IVec3::Y),
        side(IVec3::NEG_X),
        side(IVec3::NEG_Y),
    ];
    // A face shows when the cell it looks into lets it be seen: air, or something
    // see-through of another kind (water under water and leaves against leaves stay
    // closed).
    let shows = |id: u16, next: u16| next == AIR || (clear(next) && next != id);

    let mut quads: Vec<Quad> = Vec::new();
    for z in 0..HEIGHT {
        for y in 0..CHUNK {
            for x in 0..CHUNK {
                let local = IVec3::new(x, y, z);
                let p = origin + local;
                let id = block(p);
                if id == AIR {
                    continue;
                }
                if cross(id) {
                    let b = local.as_vec3();
                    for (a, c) in [
                        (b, b + Vec3::new(1.0, 1.0, 0.0)),
                        (b + Vec3::X, b + Vec3::Y),
                    ] {
                        quads.push(Quad {
                            p,
                            normal: IVec3::Z,
                            corners: [a, c, c + Vec3::Z, a + Vec3::Z],
                            mask: 0,
                            drop: 0,
                            tile: 1,
                            rot: 0,
                            flip: false,
                            submerged: false,
                            cross: true,
                        });
                    }
                    continue;
                }
                for (normal, edges, tile) in faces {
                    let next = block(p + normal);
                    if !shows(id, next) {
                        continue;
                    }
                    let mut mask = 0u32;
                    let mut drop = 0u32;
                    for (bit, e) in edges.iter().enumerate() {
                        // The same face on the neighbouring block continues this surface
                        // only when that block is of the same family and is also exposed.
                        let q = p + *e;
                        let beside = block(q);
                        let continues =
                            family(beside) == family(id) && shows(beside, block(q + normal));
                        if grid || !continues {
                            mask |= 1 << bit;
                        }
                        // Past a top face's edge into clear air the ground falls away: the
                        // strongest edge there is, inked heavier than a seam.
                        if normal == IVec3::Z && clear(beside) && !water(beside) {
                            drop |= 1 << bit;
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
                    quads.push(Quad {
                        p,
                        normal,
                        corners: corners(local, normal),
                        mask: tile_mask(mask, rot, flip),
                        drop: tile_mask(drop, rot, flip),
                        tile,
                        rot,
                        flip,
                        submerged: !water(id) && water(next),
                        cross: false,
                    });
                }
            }
        }
    }

    for q in quads {
        let id = block(q.p);
        let is_water = water(id);
        let target = if is_water {
            &mut out.water
        } else {
            &mut out.solid
        };
        let flags = pack_flags(
            q.mask,
            is_water,
            q.normal == IVec3::Z && !q.cross,
            q.submerged,
            q.drop,
        );
        let glow = tiles.glow.get(id as usize).copied().unwrap_or(0.0);
        let shade = if q.cross {
            // Upright, but a little under full top light so it sits in its cell.
            face_shade(IVec3::Z) * 0.92
        } else {
            face_shade(q.normal)
        };
        let lit = quality.shadows && q.normal.as_vec3().dot(SUN) > 0.0;
        let centre = q.corners.iter().sum::<Vec3>() / 4.0;
        let local = q.p - origin;
        let block_centre = local.as_vec3() + Vec3::splat(0.5);
        let lift = 1.0 + (local.z as f32 - MID) * RISE;
        let tile_origin = tiles.origin(id, q.tile);
        let mut lights = [0.0f32; 4];
        for (i, corner) in q.corners.iter().enumerate() {
            // The two neighbours beside this corner and the one diagonal to it, all in
            // the layer the face looks into. A cross quad reads its own cell.
            let cells = if q.cross {
                [q.p; 4]
            } else {
                let toward = (*corner - block_centre).signum().as_ivec3();
                let mut sides = [IVec3::ZERO; 2];
                let mut n = 0;
                for a in 0..3 {
                    if q.normal[a] == 0 {
                        sides[n][a] = toward[a];
                        n += 1;
                    }
                }
                [
                    q.p + q.normal,
                    q.p + q.normal + sides[0],
                    q.p + q.normal + sides[1],
                    q.p + q.normal + sides[0] + sides[1],
                ]
            };
            let mut open = 3;
            if quality.occlusion && !q.cross {
                let s1 = casts(cells[1]);
                let s2 = casts(cells[2]);
                let diagonal = casts(cells[3]);
                open = if s1 && s2 {
                    0
                } else {
                    3 - (s1 as usize + s2 as usize + diagonal as usize)
                };
            }
            // Block light: the brightest open cell beside the corner.
            let lamp = cells.iter().map(|c| light.at(*c)).max().unwrap_or(0) as f32
                / super::light::REACH as f32;
            // Shadow is sampled a quarter of the way in from the corner.
            let sample = origin.as_vec3() + corner.lerp(centre, 0.5) + q.normal.as_vec3() * 0.02;
            let shade_cast = lit && shadowed(sample, SUN, &casts);
            let ambient = shade * OCCLUSION[open] * lift;
            let sun = ambient * if shade_cast { 1.0 - SHADOW } else { 1.0 };
            lights[i] = sun.max(glow);
            let uv = (tile_origin
                + Vec2::splat(INSET)
                + tile_corner(i, q.rot, q.flip) * (TILE as f32 - 2.0 * INSET))
                / tiles.size.as_vec2();
            target.positions.push(corner.to_array());
            target.uvs.push(uv.to_array());
            target
                .colors
                .push([sun.max(glow), lamp.max(glow), ambient, flags]);
        }
        // Split along the brighter diagonal so a dark corner stays a corner.
        target
            .flips
            .push(lights[1] + lights[3] > lights[0] + lights[2]);
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
    use crate::cam::Cam;

    fn tiles() -> Tiles {
        let mut blocks = Registry::<BlockDef>::default();
        blocks.insert(BlockDef::air());
        blocks.insert(ron::from_str(r#"(id: "a", color: (200, 100, 50))"#).unwrap());
        blocks.insert(ron::from_str(r#"(id: "b", color: (10, 20, 30))"#).unwrap());
        blocks
            .insert(ron::from_str(r#"(id: "lamp", color: (250, 240, 200), glow: 0.95)"#).unwrap());
        // 4 water, 5 leaves with gaps, 6 a tuft.
        blocks.insert(
            ron::from_str(
                r#"(id: "water", color: (50, 110, 200), pattern: Water, walkable: false)"#,
            )
            .unwrap(),
        );
        blocks.insert(
            ron::from_str(r#"(id: "leaves", color: (60, 130, 60), pattern: Leaves, cutout: true)"#)
                .unwrap(),
        );
        blocks.insert(
            ron::from_str(r#"(id: "tuft", color: (90, 160, 60), pattern: Tuft, shape: Cross)"#)
                .unwrap(),
        );
        atlas(&blocks, None).1
    }

    fn both(world: impl Fn(IVec3) -> u16) -> ChunkFaces {
        chunk_faces(
            IVec2::ZERO,
            false,
            Quality::default(),
            &tiles(),
            &super::super::light::Grid::empty(),
            world,
        )
    }

    fn mesh(world: impl Fn(IVec3) -> u16) -> Faces {
        chunk_faces(
            IVec2::ZERO,
            false,
            Quality::default(),
            &tiles(),
            &super::super::light::Grid::empty(),
            world,
        )
        .solid
    }

    fn masks(f: &Faces) -> Vec<u32> {
        f.colors
            .chunks(4)
            .map(|q| unpack_flags(q[0][3]).0)
            .collect()
    }

    /// Sunlight channel of each corner of quad `q`.
    fn lights(f: &Faces, q: usize) -> Vec<f32> {
        f.colors[q * 4..q * 4 + 4].iter().map(|c| c[0]).collect()
    }

    #[test]
    fn flags_round_trip_and_follow_the_tile() {
        for mask in 0..16 {
            for water in [false, true] {
                for top in [false, true] {
                    for under in [false, true] {
                        for drop in [0, 5, 15] {
                            assert_eq!(
                                unpack_flags(pack_flags(mask, water, top, under, drop)),
                                (mask, water, top, under, drop)
                            );
                        }
                    }
                }
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
    fn a_lone_block_shows_five_outlined_faces_lit_by_the_sun() {
        let f = mesh(|p| (p == IVec3::new(3, 4, 5)) as u16);
        // The top and all four sides; never the underside.
        assert_eq!(f.len(), 5);
        assert_eq!(masks(&f), vec![15; 5]);
        // Faces come out as top, +x, +y, -x, -y. The sun stands toward -x and a little +y.
        let l: Vec<f32> = (0..5).map(|q| lights(&f, q)[0]).collect();
        assert!(l[0] > l[3], "top is brightest: {l:?}");
        assert!(l[3] > l[2] && l[2] > l[4] && l[4] > l[1], "{l:?}");
    }

    #[test]
    fn a_flat_floor_is_one_clean_evenly_lit_surface() {
        let floor = |p: IVec3| (p.z == 0) as u16;
        let t = tiles();
        let f = mesh(floor);
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
        let g = chunk_faces(
            IVec2::ZERO,
            true,
            Quality::default(),
            &t,
            &super::super::light::Grid::empty(),
            floor,
        )
        .solid;
        assert!(masks(&g).iter().all(|m| *m == 15));
    }

    #[test]
    fn a_change_of_block_is_inked_on_both_sides() {
        // Kind 1 for x < 8, kind 2 from x = 8.
        let f = mesh(|p| {
            if p.z != 0 {
                0
            } else if p.x < 8 {
                1
            } else {
                2
            }
        });
        let inked = masks(&f).iter().filter(|m| **m != 0).count();
        assert_eq!(inked, 2 * CHUNK as usize);
    }

    #[test]
    fn positions_are_in_world_space_and_the_view_gives_them_depth() {
        let f = mesh(|p| (p == IVec3::new(1, 1, 0)) as u16);
        // The top face is the unit square at z = 1.
        assert_eq!(
            f.positions[..4],
            [
                [1.0, 1.0, 1.0],
                [2.0, 1.0, 1.0],
                [2.0, 2.0, 1.0],
                [1.0, 2.0, 1.0]
            ]
        );
        // Through the classic camera the corner nearest the eye (2, 2) is nearest, the
        // far one (1, 1) farthest, the other two level with each other.
        let cam = Cam::quarter(0);
        let size = Vec2::new(1280.0, 800.0);
        let depth: Vec<f32> = f.positions[..4]
            .iter()
            .map(|p| cam.project(size, Vec3::from(*p)).z)
            .collect();
        assert!(depth[2] > depth[1] && depth[1] > depth[0], "{depth:?}");
        assert!((depth[1] - depth[3]).abs() < 1e-4, "{depth:?}");
    }

    /// The top face of the floor block at `(x, y)` in a mesh of a floor at z 0.
    fn floor_quad(f: &Faces, x: i32, y: i32) -> usize {
        let want = Vec3::new(x as f32, y as f32, 1.0);
        (0..f.len())
            .find(|q| Vec3::from(f.positions[q * 4]).distance(want) < 1e-3)
            .expect("floor face")
    }

    #[test]
    fn corners_against_a_wall_are_occluded() {
        // A floor with one block standing on it at (5, 5).
        let world = |p: IVec3| (p.z == 0 || p == IVec3::new(5, 5, 1)) as u16;
        let f = mesh(world);
        // The floor block at (6, 5) touches the standing block along its x = 6 edge:
        // corners 0 and 3 are in the crease, 1 and 2 are clear.
        let l = lights(&f, floor_quad(&f, 6, 5));
        assert!(l[0] < l[1] && l[3] < l[2], "{l:?}");
        // Far from it nothing is occluded.
        let far = lights(&f, floor_quad(&f, 12, 2));
        assert!(far.iter().all(|v| (*v - far[0]).abs() < 1e-6));
        assert!(far[0] > l[1] - 1e-6);
        // With occlusion off the crease is as bright as open ground (it lies sunward of
        // the block, so no shadow falls on it either).
        let plain = Quality {
            occlusion: false,
            ..default()
        };
        let g = chunk_faces(
            IVec2::ZERO,
            false,
            plain,
            &tiles(),
            &super::super::light::Grid::empty(),
            world,
        )
        .solid;
        let flat = lights(&g, floor_quad(&g, 4, 5));
        assert!(flat.iter().all(|v| (*v - far[0]).abs() < 1e-6), "{flat:?}");
    }

    #[test]
    fn a_pillar_casts_a_shadow_away_from_the_sun() {
        let world = |p: IVec3| (p.z == 0 || (p.x == 8 && p.y == 8 && p.z < 4)) as u16;
        let f = mesh(world);
        let open = lights(&f, floor_quad(&f, 2, 2))[0];
        // The sun is toward -x and +y, so the shadow lies toward +x and -y. A one-block
        // pillar throws a narrow shadow: it darkens part of the cells it crosses.
        let dark = lights(&f, floor_quad(&f, 10, 7));
        let shaded = dark.iter().filter(|v| **v < open * 0.7).count();
        assert!(shaded >= 2, "{dark:?} vs {open}");
        // The sunward side stays lit.
        let lit = lights(&f, floor_quad(&f, 6, 9));
        assert!(lit.iter().all(|v| (*v - open).abs() < 1e-6), "{lit:?}");
        // With shadows off the same cell is fully lit.
        let plain = Quality {
            shadows: false,
            ..default()
        };
        let g = chunk_faces(
            IVec2::ZERO,
            false,
            plain,
            &tiles(),
            &super::super::light::Grid::empty(),
            world,
        )
        .solid;
        let unshaded = lights(&g, floor_quad(&g, 10, 7));
        assert!(
            unshaded.iter().all(|v| (*v - open).abs() < 1e-6),
            "{unshaded:?}"
        );
    }

    /// Not a check: prints how long real chunks take to mesh at each quality, reading
    /// blocks from loaded chunks the way the game does.
    #[test]
    #[ignore]
    fn mesh_timing() {
        use crate::world::{Chunk, Map};
        let (wg, blocks) = crate::world::worldgen::tests::base_gen(42);
        let tiles = atlas(&blocks, None).1;
        let mut map = Map::default();
        for cy in -3..3 {
            for cx in -3..3 {
                let pos = IVec2::new(cx, cy);
                map.chunks.insert(
                    pos,
                    Chunk {
                        dirty: false,
                        ..Chunk::new(wg.chunk(pos), false)
                    },
                );
            }
        }
        for (name, quality) in [
            ("shadows and occlusion", Quality::default()),
            (
                "occlusion only",
                Quality {
                    shadows: false,
                    occlusion: true,
                },
            ),
            (
                "neither",
                Quality {
                    shadows: false,
                    occlusion: false,
                },
            ),
        ] {
            let started = std::time::Instant::now();
            let mut faces = 0;
            for cy in -2..2 {
                for cx in -2..2 {
                    let pos = IVec2::new(cx, cy);
                    faces += chunk_faces(
                        pos,
                        false,
                        quality,
                        &tiles,
                        &super::super::light::Grid::empty(),
                        |p| map.get(p).unwrap_or_else(|| wg.block(p)),
                    )
                    .solid
                    .len();
                }
            }
            println!(
                "{name}: {:.2} ms per chunk, {} faces per chunk",
                started.elapsed().as_secs_f32() * 1000.0 / 16.0,
                faces / 16
            );
        }
    }

    #[test]
    fn glowing_blocks_ignore_shade() {
        let f = mesh(|p| if p == IVec3::new(3, 3, 3) { 3 } else { 0 });
        // Even the face turned away from the sun is at least as bright as the glow.
        assert!((0..5).all(|q| lights(&f, q).iter().all(|v| *v >= 0.95)));
    }

    #[test]
    fn block_light_reaches_the_vertices_beside_a_lamp() {
        use crate::world::{Chunk, Map, empty_blocks, light};
        // A floor with a torch standing on it; block 3 (the lamp kind) emits 14.
        let mut map = Map::default();
        map.chunks
            .insert(IVec2::ZERO, Chunk::new(empty_blocks(), false));
        for y in 0..CHUNK {
            for x in 0..CHUNK {
                map.set(IVec3::new(x, y, 0), 1);
            }
        }
        map.set(IVec3::new(6, 6, 1), 3);
        let emits = [0u8, 0, 0, 14];
        let grid = light::flood(IVec2::ZERO, &map, &emits, |b| b == 0, |q| map.block(q));
        let f = chunk_faces(
            IVec2::ZERO,
            false,
            Quality::default(),
            &tiles(),
            &grid,
            |q| map.block(q),
        )
        .solid;
        let lamp = |q: usize| {
            f.colors[q * 4..q * 4 + 4]
                .iter()
                .map(|c| c[1])
                .fold(0.0, f32::max)
        };
        let near = lamp(floor_quad(&f, 7, 6));
        let far = lamp(floor_quad(&f, 14, 1));
        assert!(near > 0.8, "beside the torch: {near}");
        assert!(far < 0.3, "far from it: {far}");
    }

    #[test]
    fn light_composes_by_time_of_day() {
        let noon = daylight(0.5);
        let night = daylight(0.0);
        let open = Vec3::new(1.0, 0.0, 1.0);
        let lit_day = compose(open, &noon);
        let lit_night = compose(open, &night);
        assert!(lit_day.max_element() > 0.9);
        assert!(lit_night.max_element() < 0.35);
        // A torch matters at night and is drowned at noon.
        let torch = Vec3::new(1.0, 0.9, 1.0);
        assert!(compose(torch, &night).x > lit_night.x + 0.5);
        // The uniform only moves in visible steps.
        assert_eq!(quantize(0.5), 128.0 / 255.0);
        assert_eq!(quantize(0.50001), quantize(0.5));
    }

    #[test]
    fn water_goes_in_its_own_mesh_and_tints_the_bed_under_it() {
        // Sand (1) at z 0, water (4) at z 1 over the left half, air elsewhere.
        let f = both(|p| match (p.z, p.x < 8) {
            (0, _) => 1,
            (1, true) => 4,
            _ => 0,
        });
        assert!(f.water.len() > 0, "water surfaces");
        // Every water quad is flagged water; no solid quad is.
        assert!(f.water.colors.iter().all(|c| unpack_flags(c[3]).1));
        assert!(f.solid.colors.iter().all(|c| !unpack_flags(c[3]).1));
        // The sand under water is drawn (seen through it) and flagged submerged; dry sand
        // is not.
        let wet = floor_quad(&f.solid, 3, 3);
        let dry = floor_quad(&f.solid, 12, 3);
        assert!(
            unpack_flags(f.solid.colors[wet * 4][3]).3,
            "wet sand is submerged"
        );
        assert!(
            !unpack_flags(f.solid.colors[dry * 4][3]).3,
            "dry sand is not"
        );
        // Water tops only face air: no surfaces between water and water.
        let tops = f
            .water
            .positions
            .chunks(4)
            .filter(|q| q.iter().all(|p| (p[2] - 2.0).abs() < 1e-6))
            .count();
        assert_eq!(tops, 8 * 16);
    }

    #[test]
    fn faces_behind_leaves_are_drawn_and_leaves_do_not_touch_each_other() {
        // A stone column at x 0 with a leaf block beside it at x 1, and another leaf at x 2.
        let f = mesh(|p| match (p.x, p.y, p.z) {
            (0, 0, 0) => 1,
            (1, 0, 0) | (2, 0, 0) => 5,
            _ => 0,
        });
        // Stone's +x face (toward the leaves) is present: the leaves have gaps.
        let stone_px = f
            .positions
            .chunks(4)
            .any(|q| q.iter().all(|p| (p[0] - 1.0).abs() < 1e-6 && p[1] <= 1.0));
        assert!(stone_px, "stone shows through the leaves");
        // Between the two leaf blocks there is no face at x 2 from either side.
        let between = f
            .positions
            .chunks(4)
            .filter(|q| {
                q.iter()
                    .all(|p| (p[0] - 2.0).abs() < 1e-6 && p[1] <= 1.0 && p[2] <= 1.0)
            })
            .count();
        assert_eq!(between, 0, "leaves against leaves stay closed");
    }

    #[test]
    fn a_cross_prop_is_two_upright_quads_from_the_side_tile() {
        let f = mesh(|p| if p == IVec3::new(3, 3, 0) { 6 } else { 0 });
        assert_eq!(f.len(), 2);
        for q in 0..2 {
            let pos: Vec<Vec3> = (0..4).map(|k| Vec3::from(f.positions[q * 4 + k])).collect();
            // Bottom two corners at z 0, top two at z 1, running corner to corner of cell.
            assert!(pos[0].z == 0.0 && pos[1].z == 0.0 && pos[2].z == 1.0 && pos[3].z == 1.0);
            assert!((pos[1].truncate() - pos[0].truncate()).length() > 1.4);
            let (mask, water, top, _, _) = unpack_flags(f.colors[q * 4][3]);
            assert_eq!((mask, water, top), (0, false, false));
        }
        // From the side tile: its uv row is the second tile of the block.
        let t = tiles();
        let origin = t.origin(6, 1) / t.size.as_vec2();
        assert!((f.uvs[0][0] - origin.x).abs() < 0.01 && (f.uvs[0][1] - origin.y).abs() < 0.01);
    }
}
