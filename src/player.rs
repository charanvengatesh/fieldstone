//! The player: a body moved by the keyboard along the screen axes, drawn as a small block
//! model (`crate::figure`) projected through the view like the terrain, so it turns and is
//! covered exactly as the blocks around it are.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPipeline};

use crate::core::settings::Keys;
use crate::core::time::{Daylight, GameTime, daylight};
use crate::core::{BlockDef, Registry};
use crate::figure;
use crate::inventory::Inventory;
use crate::world::iso::View;
use crate::world::light::{REACH, level_at};
use crate::world::mesh::{compose, sunlit};
use crate::world::physics::{Body, step};
use crate::world::tiles::Tiles;
use crate::world::{AIR, Focus, HEIGHT, Map};

/// Blocks per second.
const SPEED: f32 = 4.5;
/// Texels across and up one frame of the figure.
/// Frames of the stride: standing, then mid-step.
pub const FRAMES: usize = 2;
/// Depth nudge toward the viewer, so the figure wins against the face it stands on.
const DEPTH_BIAS: f32 = 0.05;

#[derive(Component)]
pub struct Player {
    pub body: Body,
    /// Height the figure is drawn at; eases toward the body so steps do not pop.
    pub shown_z: f32,
    /// Which way the figure faces on the ground, in world terms (one of eight).
    pub facing: Vec2,
    /// Distance walked, which drives the two-frame stride.
    pub stride: f32,
    /// Which frame of the figure is showing: 0 standing, 1 mid-step.
    pub frame: usize,
    /// Brightness the figure is drawn at; eases between sun and shade.
    pub lit: f32,
    /// Seconds spent standing still, for the idle breath.
    pub idle: f32,
    /// Highest point of the current fall, for the landing.
    pub fall_from: f32,
    /// Seconds of landing squash left.
    pub squash: f32,
}

/// The player landed after a fall of `drop` blocks.
#[derive(Message, Clone, Copy)]
pub struct Landed {
    pub pos: Vec3,
    pub drop: f32,
}

impl Player {
    pub fn at(pos: Vec3) -> Self {
        Self {
            body: Body::at(pos),
            shown_z: pos.z,
            facing: Vec2::new(1.0, -1.0).normalize(),
            stride: 0.0,
            frame: 0,
            lit: 1.0,
            idle: 0.0,
            fall_from: pos.z,
            squash: 0.0,
        }
    }

    /// Where the figure stands for drawing, and how it is scaled about its feet.
    pub fn placement(&self) -> Projected {
        Projected {
            origin: self.body.pos.with_z(self.shown_z),
            scale: figure_scale(self.idle, self.squash),
            bias: DEPTH_BIAS,
        }
    }
}

/// Something drawn in world units through the view's projection, as chunks are: its
/// mesh positions are blocks relative to `origin`. `stream::apply_view` gives it the view
/// matrix as its transform every frame, so a turn of the view costs nothing.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct Projected {
    pub origin: Vec3,
    /// Scale about the origin, in world axes.
    pub scale: Vec3,
    /// Depth added after projection, toward the viewer when positive.
    pub bias: f32,
}

impl Projected {
    pub fn at(origin: Vec3) -> Self {
        Self {
            origin,
            scale: Vec3::ONE,
            bias: 0.0,
        }
    }
}

/// The soft dark patch on the ground under the player.
#[derive(Component)]
pub struct GroundShadow;

/// Whether the figure shows as a flat shade through whatever hides it.
#[derive(Resource)]
pub struct Silhouette(pub bool);

impl Default for Silhouette {
    fn default() -> Self {
        Self(true)
    }
}

/// The silhouette's colour (rgb) and opacity.
pub const SILHOUETTE: Vec4 = Vec4::new(0.55, 0.65, 0.95, 0.55);
/// How far in front of the figure its silhouette sits, in depth units: past the model's
/// own near faces, so only blocks nearer than the whole figure bring it out.
pub const SILHOUETTE_LIFT: f32 = 0.8;

/// Marks the entity that draws the silhouette.
#[derive(Component)]
pub struct SilhouetteQuad;

/// The silhouette material: the figure's own vertex colours as a flat tint, drawn only
/// where the depth buffer holds something nearer (`shaders/silhouette.wgsl`, depth test
/// turned round).
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct SilhouetteArt {
    #[uniform(0)]
    pub tint: Vec4,
}

impl Material2d for SilhouetteArt {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "shaders/silhouette.wgsl".into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
    fn specialize(
        _pipeline: &Material2dPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // 2D depth is reversed (nearer is greater), so `Less` keeps only the texels that
        // something else covers.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(CompareFunction::Less);
        }
        Ok(())
    }
}

/// How wide the ground shadow is, in blocks.
const SHADOW_RADIUS: f32 = 0.34;
/// Brightness of the figure in cast shadow.
const SHADED: f32 = 0.66;
/// Blocks walked per frame of the stride.
const STEP: f32 = 0.42;

/// Whether a walker may not occupy `c`: a solid block, or air above something nothing may
/// stand on (water), however high.
pub fn closed(map: &Map, blocks: &Registry<BlockDef>, c: IVec3) -> bool {
    let b = map.block(c);
    if b != AIR {
        return blocks.at(b).blocks_movement();
    }
    for z in (0..c.z.min(HEIGHT)).rev() {
        let below = map.block(IVec3::new(c.x, c.y, z));
        if below != AIR {
            return !blocks.at(below).walkable;
        }
    }
    false
}

/// Texture coordinates of the figure quad for one frame of the strip.
pub fn spawn(
    focus: Res<Focus>,
    pending: Option<Res<crate::core::save::Pending>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut silhouettes: ResMut<Assets<SilhouetteArt>>,
    mut commands: Commands,
) {
    // A loaded save hands over what the player carried.
    let mut inventory = Inventory::default();
    if let Some(p) = pending {
        for (slot, saved) in inventory.slots.iter_mut().zip(&p.inventory) {
            *slot = *saved;
        }
        inventory.selected = p.selected.min(crate::inventory::HOTBAR - 1);
    }
    let player = Player::at(focus.0);
    let placement = player.placement();
    let mesh = meshes.add(figure::model(player.facing, player.frame).into_mesh());
    commands.spawn((
        player,
        inventory,
        placement,
        Mesh2d(mesh.clone()),
        MeshMaterial2d(materials.add(ColorMaterial {
            color: Color::WHITE,
            ..default()
        })),
        Transform::default(),
        NoFrustumCulling,
    ));
    // The same model again, drawn only where blocks cover it.
    commands.spawn((
        SilhouetteQuad,
        placement,
        Mesh2d(mesh),
        MeshMaterial2d(silhouettes.add(SilhouetteArt { tint: SILHOUETTE })),
        Transform::default(),
        NoFrustumCulling,
    ));
    // An eight-sided patch lying on the ground; its corners are set by `place`.
    let ring: Vec<u32> = (1..=8).flat_map(|i| [0, i, i % 8 + 1]).collect();
    let patch = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 9])
    .with_inserted_indices(Indices::U32(ring));
    commands.spawn((
        GroundShadow,
        Mesh2d(meshes.add(patch)),
        MeshMaterial2d(materials.add(ColorMaterial {
            color: Color::srgba(0.02, 0.02, 0.06, 0.34),
            alpha_mode: AlphaMode2d::Blend,
            ..default()
        })),
        Transform::default(),
    ));
}

/// WASD or the arrows walk along the screen axes, whichever way the view is turned.
pub fn movement(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    time: Res<Time>,
    view: Res<View>,
    map: Res<Map>,
    blocks: Res<Registry<BlockDef>>,
    mut focus: ResMut<Focus>,
    mut landed: MessageWriter<Landed>,
    mut player: Single<&mut Player>,
) {
    let axis = |neg: &[KeyCode], pos: &[KeyCode]| {
        keys.pressed(&input, pos) as i8 as f32 - keys.pressed(&input, neg) as i8 as f32
    };
    let screen = Vec2::new(axis(&keys.left, &keys.right), axis(&keys.down, &keys.up));
    player.facing = figure::facing_of(view.ground_dir(screen), player.facing);
    // Nothing moves until the ground under the player has streamed in; the focus still
    // follows them there, or it never would.
    if map
        .get(player.body.pos.floor().as_ivec3().with_z(0))
        .is_none()
    {
        focus.0 = player.body.pos.with_z(player.shown_z);
        return;
    }
    let wish = view.ground_dir(screen) * SPEED;
    let before = player.body.pos;
    let was_grounded = player.body.grounded;
    step(&mut player.body, wish, time.delta_secs(), &|c| {
        closed(&map, &blocks, c)
    });
    // Falls are measured from their highest point; a landing of a block or more squashes
    // the figure and raises dust.
    if !player.body.grounded {
        player.fall_from = player.fall_from.max(player.body.pos.z);
    } else if !was_grounded {
        let drop = player.fall_from - player.body.pos.z;
        if drop >= 1.0 {
            player.squash = SQUASH_SECS;
            landed.write(Landed {
                pos: player.body.pos,
                drop,
            });
        }
        player.fall_from = player.body.pos.z;
    } else {
        player.fall_from = player.body.pos.z;
    }
    player.squash = (player.squash - time.delta_secs()).max(0.0);
    let walked = (player.body.pos - before).truncate().length();
    if walked > 1e-4 && player.body.grounded {
        player.stride += walked;
        player.frame = (player.stride / STEP) as usize % FRAMES;
        player.idle = 0.0;
    } else {
        player.frame = 0;
        player.idle += time.delta_secs();
    }
    let target = player.body.pos.z;
    let ease = (time.delta_secs() * 18.0).min(1.0);
    player.shown_z += (target - player.shown_z) * ease;
    if (target - player.shown_z).abs() < 0.01 {
        player.shown_z = target;
    }
    focus.0 = player.body.pos.with_z(player.shown_z);
}

/// The figure's colour: lit by the sun (`lit` 1 in the open, less in shade) and by
/// block light `lamp` 0..15, mixed by the time of day like the blocks are.
pub fn figure_light(lit: f32, lamp: u8, day: &Daylight) -> Vec3 {
    compose(Vec3::new(lit, lamp as f32 / REACH as f32, lit), day)
}

/// Puts the figure, its ghost and its ground shadow where the body is, shows the right
/// frame of the stride, and dims the figure when something stands between it and the sun.
pub fn place(
    time: Res<Time>,
    clock: Res<GameTime>,
    view: Res<View>,
    map: Res<Map>,
    blocks: Res<Registry<BlockDef>>,
    tiles: Option<Res<Tiles>>,
    mut last_color: Local<Vec3>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut shown: Local<(Option<View>, Option<(Vec2, usize)>)>,
    mut player: Single<(
        &mut Player,
        &mut Projected,
        &Mesh2d,
        &MeshMaterial2d<ColorMaterial>,
    )>,
    mut shadow: Single<(&mut Transform, &Mesh2d), (With<GroundShadow>, Without<Player>)>,
) {
    let (player, projected, mesh, material) = &mut *player;
    let pos = player.body.pos.with_z(player.shown_z);
    let placement = player.placement();
    if **projected != placement {
        **projected = placement;
    }

    // The model is rebuilt when the facing or the stride frame changes.
    let pose = (player.facing, player.frame);
    if shown.1 != Some(pose) {
        shown.1 = Some(pose);
        if let Some(mut m) = meshes.get_mut(&mesh.0) {
            *m = figure::model(player.facing, player.frame).into_mesh();
        }
    }

    // Light: sampled at chest height, eased so walking under a tree fades.
    let solid = |c: IVec3| {
        let b = map.block(c);
        b != AIR && blocks.at(b).blocks_movement() && blocks.at(b).walkable
    };
    let target = if sunlit(pos + Vec3::Z * 0.9, solid) {
        1.0
    } else {
        SHADED
    };
    player.lit += (target - player.lit) * (time.delta_secs() * 8.0).min(1.0);
    let lamp = tiles
        .map(|t| {
            level_at(
                &map,
                &t.light,
                |b| t.passes.get(b as usize).copied().unwrap_or(false),
                (pos + Vec3::Z * 0.9).floor().as_ivec3(),
            )
        })
        .unwrap_or(0);
    let color = figure_light(player.lit, lamp, &daylight(clock.frac()));
    if (color - *last_color).abs().max_element() > 1e-3
        && let Some(mut m) = materials.get_mut(&material.0)
    {
        *last_color = color;
        m.color = Color::linear_rgb(color.x, color.y, color.z);
    }

    // The ground shadow lies on the first closed cell under the feet.
    let feet = player.body.pos.floor().as_ivec3();
    let ground = (0..8)
        .map(|d| feet.z - d)
        .find(|z| closed(&map, &blocks, IVec3::new(feet.x, feet.y, z - 1)))
        .unwrap_or(feet.z) as f32;
    let (shadow_tf, shadow_mesh) = &mut *shadow;
    shadow_tf.translation = view.project(pos.with_z(ground)) + Vec3::Z * 0.03;
    if shown.0 != Some(*view) {
        shown.0 = Some(*view);
        if let Some(mut m) = meshes.get_mut(&shadow_mesh.0) {
            let mut ring = vec![[0.0f32; 3]];
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let offset = Vec3::new(a.cos(), a.sin(), 0.0) * SHADOW_RADIUS;
                ring.push(view.project(offset).to_array());
            }
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, ring);
        }
    }
}

/// Puts the silhouette exactly where the figure is, and shows it only when wanted.
pub fn place_silhouette(
    on: Res<Silhouette>,
    player: Single<&Projected, With<Player>>,
    mut quad: Single<(&mut Projected, &mut Visibility), (With<SilhouetteQuad>, Without<Player>)>,
) {
    let (at, vis) = &mut *quad;
    let mut wanted_at = **player;
    wanted_at.bias += SILHOUETTE_LIFT;
    if **at != wanted_at {
        **at = wanted_at;
    }
    let wanted = if on.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if **vis != wanted {
        **vis = wanted;
    }
}

/// Seconds the landing squash lasts.
pub const SQUASH_SECS: f32 = 0.1;

/// How the figure is scaled about its feet: a landing squashes it wide and short for a
/// moment; standing still, it breathes a texel every 1.2 seconds.
pub fn figure_scale(idle: f32, squash: f32) -> Vec3 {
    if squash > 0.0 {
        return Vec3::new(1.12, 0.85, 1.0);
    }
    let breath = idle > 0.0 && (idle % 1.2) > 0.9;
    // One texel at the classic tilt (16 texels a block).
    let dip = 1.0 / (16.0 * figure::HEIGHT);
    if breath {
        Vec3::new(1.0, 1.0 - dip, 1.0)
    } else {
        Vec3::ONE
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerSet;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Landed>()
            .init_resource::<Silhouette>()
            .add_systems(Startup, spawn)
            .add_systems(
                Update,
                movement
                    .in_set(PlayerSet)
                    .before(crate::world::WorldSet)
                    .run_if(crate::core::not_paused),
            )
            // Drawn through this frame's view angle, not last frame's, or the figure
            // trails the world through every turn.
            .add_systems(
                Update,
                (place, place_silhouette)
                    .chain()
                    .in_set(crate::world::PlaceSet)
                    .run_if(crate::core::not_paused),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Chunk, empty_blocks};

    #[test]
    fn water_closes_the_whole_column_above_it() {
        let mut blocks = Registry::<BlockDef>::default();
        blocks.insert(BlockDef::air());
        let stone = blocks.insert(ron::from_str(r#"(id: "stone", color: (1, 1, 1))"#).unwrap());
        let water = blocks
            .insert(ron::from_str(r#"(id: "water", color: (1, 1, 1), walkable: false)"#).unwrap());
        let mut map = Map::default();
        map.chunks.insert(
            IVec2::ZERO,
            Chunk {
                dirty: false,
                ..Chunk::new(empty_blocks(), false)
            },
        );
        for x in 0..4 {
            map.set(IVec3::new(x, 0, 0), stone);
        }
        map.set(IVec3::new(2, 0, 1), water);
        // Solid blocks are closed, air above ground is open.
        assert!(closed(&map, &blocks, IVec3::new(0, 0, 0)));
        assert!(!closed(&map, &blocks, IVec3::new(0, 0, 1)));
        // Water and every cell above it are closed.
        assert!(closed(&map, &blocks, IVec3::new(2, 0, 1)));
        assert!(closed(&map, &blocks, IVec3::new(2, 0, 2)));
        assert!(closed(&map, &blocks, IVec3::new(2, 0, 9)));
        // A block laid on the water makes a bridge.
        map.set(IVec3::new(2, 0, 2), stone);
        assert!(!closed(&map, &blocks, IVec3::new(2, 0, 3)));
    }
}
