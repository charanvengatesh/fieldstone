//! The player: a body moved by the keyboard along the screen axes, drawn as an upright
//! quad whose vertices carry depth so blocks in front of it cover it correctly.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::image::ImageSampler;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite::Anchor;
use bevy::sprite_render::AlphaMode2d;

use crate::core::settings::Keys;
use crate::core::{BlockDef, Registry};
use crate::inventory::Inventory;
use crate::world::iso::{BLOCK_H, View};
use crate::world::physics::{Body, step};
use crate::world::{AIR, Focus, HEIGHT, Map};

/// Blocks per second.
const SPEED: f32 = 4.5;
/// Texels across and up the figure.
pub const FIGURE: UVec2 = UVec2::new(16, 28);
/// How far in front of its true depth the figure is drawn, so its feet never fight the
/// ground they stand on.
const DEPTH_BIAS: f32 = 0.05;
/// The layer only the overlay camera draws.
pub const OVERLAY_LAYER: usize = 1;

#[derive(Component)]
pub struct Player {
    pub body: Body,
    /// Height the figure is drawn at; eases toward the body so steps do not pop.
    pub shown_z: f32,
    /// -1 or 1: which way the figure faces on screen.
    pub facing: f32,
}

/// A faint copy of the figure drawn over everything, so the player stays findable behind
/// terrain.
#[derive(Component)]
pub struct Ghost;

/// Whether a walker may not occupy `c`: a solid block, or air above something nothing may
/// stand on (water), however high.
pub fn closed(map: &Map, blocks: &Registry<BlockDef>, c: IVec3) -> bool {
    let b = map.block(c);
    if b != AIR {
        return blocks.at(b).solid;
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
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
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
    let image = images.add(figure_texture());
    let (w, h) = (FIGURE.x as f32, FIGURE.y as f32);
    // Feet at the origin; a point `y` texels up is `y / BLOCK_H` blocks nearer the viewer.
    let top = h / BLOCK_H;
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-w / 2.0, 0.0, 0.0],
            [w / 2.0, 0.0, 0.0],
            [w / 2.0, h, top],
            [-w / 2.0, h, top],
        ],
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
    )
    .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    commands.spawn((
        Player {
            body: Body::at(focus.0),
            shown_z: focus.0.z,
            facing: 1.0,
        },
        inventory,
        Mesh2d(meshes.add(mesh)),
        MeshMaterial2d(materials.add(ColorMaterial {
            color: Color::WHITE,
            alpha_mode: AlphaMode2d::Mask(0.5),
            texture: Some(image.clone()),
            ..default()
        })),
        Transform::default(),
    ));
    commands.spawn((
        Ghost,
        Sprite {
            image,
            color: Color::srgba(1.0, 1.0, 1.0, 0.3),
            ..default()
        },
        Anchor::BOTTOM_CENTER,
        RenderLayers::layer(OVERLAY_LAYER),
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
    mut player: Single<&mut Player>,
) {
    let axis = |neg: &[KeyCode], pos: &[KeyCode]| {
        keys.pressed(&input, pos) as i8 as f32 - keys.pressed(&input, neg) as i8 as f32
    };
    let screen = Vec2::new(axis(&keys.left, &keys.right), axis(&keys.down, &keys.up));
    if screen.x != 0.0 {
        player.facing = screen.x.signum();
    }
    // Nothing moves until the ground under the player has streamed in.
    if map
        .get(player.body.pos.floor().as_ivec3().with_z(0))
        .is_none()
    {
        return;
    }
    let wish = view.ground_dir(screen) * SPEED;
    step(&mut player.body, wish, time.delta_secs(), &|c| {
        closed(&map, &blocks, c)
    });
    let target = player.body.pos.z;
    let ease = (time.delta_secs() * 18.0).min(1.0);
    player.shown_z += (target - player.shown_z) * ease;
    if (target - player.shown_z).abs() < 0.01 {
        player.shown_z = target;
    }
    focus.0 = player.body.pos.with_z(player.shown_z);
}

/// Puts the figure and its ghost where the body is.
pub fn place(
    view: Res<View>,
    player: Single<(&Player, &mut Transform), Without<Ghost>>,
    mut ghost: Single<&mut Transform, With<Ghost>>,
) {
    let (player, mut tf) = player.into_inner();
    let mut at = view.project(player.body.pos.with_z(player.shown_z));
    at.z += DEPTH_BIAS;
    tf.translation = at;
    tf.scale = Vec3::new(player.facing, 1.0, 1.0);
    ghost.translation = at;
    ghost.scale = tf.scale;
}

/// The figure's quad as plain vertex data, for the software canvas: the same corners and
/// depths as the mesh, mirrored when facing left.
pub fn figure_faces(facing: f32) -> crate::world::mesh::Faces {
    let (w, h) = (FIGURE.x as f32 * facing.signum(), FIGURE.y as f32);
    let top = h / BLOCK_H;
    crate::world::mesh::Faces {
        positions: vec![
            [-w / 2.0, 0.0, 0.0],
            [w / 2.0, 0.0, 0.0],
            [w / 2.0, h, top],
            [-w / 2.0, h, top],
        ],
        uvs: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        colors: vec![[1.0; 4]; 4],
    }
}

/// A small outlined figure: head, tunic, legs. Generated so the base pack ships no images.
pub fn figure_texture() -> Image {
    const SKIN: [u8; 4] = [232, 190, 150, 255];
    const HAIR: [u8; 4] = [84, 56, 36, 255];
    const TUNIC: [u8; 4] = [58, 110, 190, 255];
    const TUNIC_DARK: [u8; 4] = [42, 82, 150, 255];
    const LEGS: [u8; 4] = [70, 62, 84, 255];
    const INK: [u8; 4] = [26, 24, 34, 255];
    let (w, h) = (FIGURE.x as i32, FIGURE.y as i32);
    let mut px = vec![[0u8; 4]; (w * h) as usize];
    let mut rect = |x0: i32, y0: i32, x1: i32, y1: i32, c: [u8; 4]| {
        for y in y0..y1 {
            for x in x0..x1 {
                px[(y * w + x) as usize] = c;
            }
        }
    };
    rect(5, 1, 11, 4, HAIR);
    rect(5, 4, 11, 9, SKIN);
    rect(4, 9, 12, 19, TUNIC);
    rect(9, 9, 12, 19, TUNIC_DARK);
    rect(2, 10, 4, 17, TUNIC);
    rect(12, 10, 14, 17, TUNIC_DARK);
    rect(5, 19, 8, 27, LEGS);
    rect(9, 19, 12, 27, LEGS);
    // One texel of ink around everything that was drawn.
    let filled: Vec<bool> = px.iter().map(|p| p[3] != 0).collect();
    for y in 0..h {
        for x in 0..w {
            if filled[(y * w + x) as usize] {
                continue;
            }
            let touches = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                nx >= 0 && ny >= 0 && nx < w && ny < h && filled[(ny * w + nx) as usize]
            });
            if touches {
                px[(y * w + x) as usize] = INK;
            }
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: FIGURE.x,
            height: FIGURE.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        px.into_iter().flatten().collect(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::nearest();
    img
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerSet;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn).add_systems(
            Update,
            (movement, place)
                .chain()
                .in_set(PlayerSet)
                .before(crate::world::WorldSet)
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
                blocks: empty_blocks(),
                dirty: false,
                modified: false,
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

    #[test]
    fn figure_is_outlined_and_mostly_opaque() {
        let img = figure_texture();
        let data = img.data.as_ref().unwrap();
        let opaque = data.chunks(4).filter(|p| p[3] == 255).count();
        assert!(opaque > 150 && opaque < (FIGURE.x * FIGURE.y) as usize);
        // The corners stay transparent.
        assert_eq!(data[3], 0);
    }
}
