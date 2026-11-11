//! The player: a body moved by the keyboard along the picture's axes, drawn as a small
//! block model (`crate::figure`) in the world like any other mesh.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::MeshMaterial3d;
use bevy::prelude::*;

use crate::cam::Cam;
use crate::core::settings::Keys;
use crate::core::time::{Daylight, GameTime, daylight};
use crate::core::{BlockDef, Registry};
use crate::figure;
use crate::inventory::Inventory;
use crate::world::light::{REACH, level_at};
use crate::world::mesh::{FlatArt, SilhouetteArt, compose, sunlit};
use crate::world::physics::{Body, jump, step};
use crate::world::tiles::Tiles;
use crate::world::{AIR, Focus, HEIGHT, Map};

/// Blocks per second.
const SPEED: f32 = 4.5;

#[derive(Component)]
pub struct Player {
    pub body: Body,
    /// Height the figure is drawn at; eases toward the body so steps do not pop.
    pub shown_z: f32,
    /// Which way the figure faces on the ground, in world terms (one of eight).
    pub facing: Vec2,
    /// Distance walked since standing still, which drives the walk cycle.
    pub stride: f32,
    /// Where the limbs are in the walk cycle, -1 to 1; 0 standing.
    pub swing: f32,
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
            swing: 0.0,
            lit: 1.0,
            idle: 0.0,
            fall_from: pos.z,
            squash: 0.0,
        }
    }

    /// Where the figure stands, which way it faces, and how it is scaled about its feet.
    pub fn transform(&self) -> Transform {
        // The model is built facing +y; turn it to the facing.
        let angle = self.facing.y.atan2(self.facing.x) - std::f32::consts::FRAC_PI_2;
        Transform {
            translation: self.body.pos.with_z(self.shown_z),
            rotation: Quat::from_rotation_z(angle),
            scale: figure_scale(self.idle, self.squash),
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

/// Marks the entity that draws the silhouette.
#[derive(Component)]
pub struct SilhouetteQuad;

/// How far toward the eye the silhouette sits: half the model's thickness and a hair, so
/// the figure's own near faces (its back faces are culled) do not bring it out, while a
/// block in the very next cell does. Moving along the view axis changes nothing on an
/// orthographic picture but the depth.
pub fn silhouette_lift(_cam: &Cam) -> f32 {
    0.42
}

/// The silhouette's transform for a figure at `figure`.
pub fn silhouette_transform(cam: &Cam, figure: &Transform) -> Transform {
    let mut t = *figure;
    t.translation += cam.toward_eye() * silhouette_lift(cam);
    t
}

/// How wide the ground shadow is, in blocks.
const SHADOW_RADIUS: f32 = 0.34;
/// Brightness of the figure in cast shadow.
const SHADED: f32 = 0.66;

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

pub fn spawn(
    focus: Res<Focus>,
    pending: Option<Res<crate::core::save::Pending>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut flats: ResMut<Assets<FlatArt>>,
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
    let transform = player.transform();
    let mesh = meshes.add(figure::model(Vec2::Y, player.swing).into_mesh());
    commands.spawn((
        player,
        inventory,
        Mesh3d(mesh.clone()),
        MeshMaterial3d(flats.add(FlatArt::plain(Vec4::ONE, false))),
        transform,
    ));
    // The same model again, drawn only where blocks cover it.
    commands.spawn((
        SilhouetteQuad,
        Mesh3d(mesh),
        MeshMaterial3d(silhouettes.add(SilhouetteArt { tint: SILHOUETTE })),
        transform,
    ));
    // A soft dark disc lying on the ground under the feet.
    commands.spawn((
        GroundShadow,
        Mesh3d(meshes.add(disc(SHADOW_RADIUS))),
        MeshMaterial3d(flats.add(FlatArt::plain(Vec4::new(0.02, 0.02, 0.06, 0.34), true))),
        Transform::default(),
    ));
}

/// An eight-sided disc of `radius` lying flat at z 0, with the attributes the flat
/// material wants.
fn disc(radius: f32) -> Mesh {
    let mut positions = vec![[0.0f32, 0.0, 0.0]];
    let mut uvs = vec![[0.5f32, 0.5]];
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::TAU / 8.0;
        positions.push([a.cos() * radius, a.sin() * radius, 0.0]);
        uvs.push([0.5 + a.cos() * 0.5, 0.5 + a.sin() * 0.5]);
    }
    let ring: Vec<u32> = (1..=8).flat_map(|i| [0, i, i % 8 + 1]).collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32; 4]; 9])
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 0.0, 1.0]; 9])
    .with_inserted_indices(Indices::U32(ring))
}

/// WASD or the arrows walk along the screen axes, whichever way the view is turned.
pub fn movement(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<Keys>,
    time: Res<Time>,
    cam: Res<Cam>,
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
    player.facing = figure::facing_of(cam.ground_dir(screen), player.facing);
    // Nothing moves until the ground under the player has streamed in; the focus still
    // follows them there, or it never would.
    if map
        .get(player.body.pos.floor().as_ivec3().with_z(0))
        .is_none()
    {
        focus.0 = player.body.pos.with_z(player.shown_z);
        return;
    }
    let wish = cam.ground_dir(screen) * SPEED;
    let before = player.body.pos;
    let was_grounded = player.body.grounded;
    if keys.just(&input, &keys.jump) {
        jump(&mut player.body);
    }
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
    // The walk cycle follows the distance walked; standing still, the limbs ease back
    // together.
    let walked = (player.body.pos - before).truncate().length();
    if walked > 1e-4 && player.body.grounded {
        player.stride += walked;
        player.swing = figure::swing_at(player.stride);
        player.idle = 0.0;
    } else {
        player.swing += (0.0 - player.swing) * (time.delta_secs() * 14.0).min(1.0);
        if player.swing.abs() < 0.02 {
            player.swing = 0.0;
            player.stride = 0.0;
        }
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
    map: Res<Map>,
    blocks: Res<Registry<BlockDef>>,
    tiles: Option<Res<Tiles>>,
    mut last_color: Local<Vec3>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut flats: ResMut<Assets<FlatArt>>,
    mut shown: Local<Option<i32>>,
    mut player: Single<(
        &mut Player,
        &mut Transform,
        &Mesh3d,
        &MeshMaterial3d<FlatArt>,
    )>,
    mut shadow: Single<&mut Transform, (With<GroundShadow>, Without<Player>)>,
) {
    let (player, tf, mesh, material) = &mut *player;
    let pos = player.body.pos.with_z(player.shown_z);
    let wanted = player.transform();
    if **tf != wanted {
        **tf = wanted;
    }

    // The model is rebuilt when the walk cycle moves, in steps of a thirty-second of the
    // swing, so a still figure costs nothing. Facing is the transform's turn.
    let step = (player.swing * 32.0).round() as i32;
    if *shown != Some(step) {
        *shown = Some(step);
        if let Some(mut m) = meshes.get_mut(&mesh.0) {
            *m = figure::model(Vec2::Y, step as f32 / 32.0).into_mesh();
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
        && let Some(mut m) = flats.get_mut(&material.0)
    {
        *last_color = color;
        m.flat.tint = color.extend(1.0);
    }

    // The ground shadow lies on the first closed cell under the feet.
    let feet = player.body.pos.floor().as_ivec3();
    let ground = (0..8)
        .map(|d| feet.z - d)
        .find(|z| closed(&map, &blocks, IVec3::new(feet.x, feet.y, z - 1)))
        .unwrap_or(feet.z) as f32;
    shadow.translation = player.body.pos.with_z(ground + 0.02);
}

/// Puts the silhouette exactly where the figure is, and shows it only when wanted.
pub fn place_silhouette(
    on: Res<Silhouette>,
    cam: Res<Cam>,
    player: Single<&Transform, With<Player>>,
    mut quad: Single<(&mut Transform, &mut Visibility), (With<SilhouetteQuad>, Without<Player>)>,
) {
    let (tf, vis) = &mut *quad;
    let wanted_tf = silhouette_transform(&cam, &player);
    if **tf != wanted_tf {
        **tf = wanted_tf;
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
