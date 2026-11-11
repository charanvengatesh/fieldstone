//! The player: a body moved by the keyboard along the screen axes, drawn as an upright
//! quad whose vertices carry depth so blocks in front of it cover it correctly.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite_render::AlphaMode2d;

use crate::core::settings::Keys;
use crate::core::time::{Daylight, GameTime, daylight};
use crate::core::{BlockDef, Registry};
use crate::inventory::Inventory;
use crate::world::iso::{Tilt, View};
use crate::world::light::{REACH, level_at};
use crate::world::mesh::{compose, sunlit};
use crate::world::physics::{Body, step};
use crate::world::tiles::Tiles;
use crate::world::{AIR, Focus, HEIGHT, Map};

/// Blocks per second.
const SPEED: f32 = 4.5;
/// Texels across and up one frame of the figure.
pub const FIGURE: UVec2 = UVec2::new(16, 28);
/// Frames in the figure strip.
pub const FRAMES: usize = 2;
/// How far in front of its true depth the figure is drawn, so its feet never fight the
/// ground they stand on.
const DEPTH_BIAS: f32 = 0.05;

#[derive(Component)]
pub struct Player {
    pub body: Body,
    /// Height the figure is drawn at; eases toward the body so steps do not pop.
    pub shown_z: f32,
    /// -1 or 1: which way the figure faces on screen.
    pub facing: f32,
    /// Which way the figure is turned: sideways, away from the viewer or toward them.
    pub heading: Heading,
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

/// Which way the figure is turned, in screen terms.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Heading {
    #[default]
    Side,
    /// Walking up the screen: the back is to the viewer.
    Away,
    /// Walking down the screen: the face is to the viewer.
    Toward,
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
            facing: 1.0,
            heading: Heading::Side,
            stride: 0.0,
            frame: 0,
            lit: 1.0,
            idle: 0.0,
            fall_from: pos.z,
            squash: 0.0,
        }
    }

    /// Which column of the figure strip to show: three headings, each standing and
    /// stepping.
    pub fn strip_frame(&self) -> usize {
        let heading = match self.heading {
            Heading::Side => 0,
            Heading::Away => 1,
            Heading::Toward => 2,
        };
        heading * FRAMES + self.frame
    }
}

/// The soft dark patch on the ground under the player.
#[derive(Component)]
pub struct GroundShadow;

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
/// Columns in the figure strip: three headings of [`FRAMES`] each.
pub const STRIP: usize = 3 * FRAMES;

fn frame_uvs(frame: usize) -> Vec<[f32; 2]> {
    let (u0, u1) = (
        frame as f32 / STRIP as f32,
        (frame + 1) as f32 / STRIP as f32,
    );
    vec![[u0, 1.0], [u1, 1.0], [u1, 0.0], [u0, 0.0]]
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
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        figure_corners(1.0, Tilt::default()),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, frame_uvs(0))
    .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    commands.spawn((
        Player::at(focus.0),
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
    if screen.x != 0.0 {
        player.facing = screen.x.signum();
    }
    if screen != Vec2::ZERO {
        player.heading = if screen.x.abs() >= screen.y.abs() {
            Heading::Side
        } else if screen.y > 0.0 {
            Heading::Away
        } else {
            Heading::Toward
        };
    }
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
    compose(Vec3::new(lit, lamp as f32 / REACH as f32, lit), day, 1.0)
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
    mut shown: Local<(Option<View>, usize)>,
    mut player: Single<(
        &mut Player,
        &mut Transform,
        &Mesh2d,
        &MeshMaterial2d<ColorMaterial>,
    )>,
    mut shadow: Single<(&mut Transform, &Mesh2d), (With<GroundShadow>, Without<Player>)>,
) {
    let (player, tf, mesh, material) = &mut *player;
    let pos = player.body.pos.with_z(player.shown_z);
    let mut at = view.project(pos);
    at.z += DEPTH_BIAS;
    tf.translation = at;
    tf.scale = Vec3::new(player.facing, 1.0, 1.0) * figure_scale(player.idle, player.squash);

    // The stride frame and the heading.
    let frame = player.strip_frame();
    if shown.1 != frame {
        shown.1 = frame;
        if let Some(mut m) = meshes.get_mut(&mesh.0) {
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, frame_uvs(frame));
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
        let tilted = shown.0.is_none_or(|v| v.tilt != view.tilt);
        shown.0 = Some(*view);
        if tilted && let Some(mut m) = meshes.get_mut(&mesh.0) {
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, figure_corners(1.0, view.tilt));
        }
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

/// Seconds the landing squash lasts.
pub const SQUASH_SECS: f32 = 0.1;

/// How the figure is scaled about its feet: a landing squashes it wide and short for a
/// moment; standing still, it breathes a texel every 1.2 seconds.
pub fn figure_scale(idle: f32, squash: f32) -> Vec3 {
    if squash > 0.0 {
        return Vec3::new(1.12, 0.85, 1.0);
    }
    let breath = idle > 0.0 && (idle % 1.2) > 0.9;
    let dip = 1.0 / FIGURE.y as f32;
    if breath {
        Vec3::new(1.0, 1.0 - dip, 1.0)
    } else {
        Vec3::ONE
    }
}

/// The figure quad's corners in texels with depth: feet at the origin; a texel of height
/// is `1 / block_h` blocks up, which is `rise / block_h` nearer the viewer at this tilt.
pub fn figure_corners(facing: f32, tilt: Tilt) -> Vec<[f32; 3]> {
    let (w, h) = (FIGURE.x as f32 * facing.signum(), FIGURE.y as f32);
    let top = h / tilt.block_h * tilt.rise();
    vec![
        [-w / 2.0, 0.0, 0.0],
        [w / 2.0, 0.0, 0.0],
        [w / 2.0, h, top],
        [-w / 2.0, h, top],
    ]
}

/// The figure's quad as plain vertex data, for the software canvas: the same corners and
/// depths as the mesh, mirrored when facing left.
pub fn figure_faces(facing: f32, frame: usize, tilt: Tilt) -> crate::world::mesh::Faces {
    crate::world::mesh::Faces {
        positions: figure_corners(facing, tilt),
        uvs: frame_uvs(frame),
        colors: vec![[1.0; 4]; 4],
        flips: vec![false],
    }
}

/// The figure as a strip of [`STRIP`] frames: for each heading (sideways, back to the
/// viewer, facing the viewer) standing, then mid-step. Drawn here so the base pack ships no
/// images: hair, face, a tunic lit from the left, belt, trousers, boots, and one texel of
/// ink around the lot.
pub fn figure_texture() -> Image {
    const HAIR: [u8; 4] = [78, 50, 34, 255];
    const SKIN: [u8; 4] = [236, 192, 150, 255];
    const SKIN_DARK: [u8; 4] = [198, 148, 118, 255];
    const TUNIC: [u8; 4] = [62, 116, 198, 255];
    const TUNIC_LIGHT: [u8; 4] = [104, 156, 226, 255];
    const TUNIC_DARK: [u8; 4] = [44, 78, 152, 255];
    const BELT: [u8; 4] = [92, 62, 40, 255];
    const LEGS: [u8; 4] = [84, 76, 104, 255];
    const LEGS_DARK: [u8; 4] = [60, 54, 82, 255];
    const BOOT: [u8; 4] = [52, 40, 38, 255];
    const EYE: [u8; 4] = [40, 34, 48, 255];
    const INK: [u8; 4] = [26, 24, 36, 255];
    let (fw, h) = (FIGURE.x as i32, FIGURE.y as i32);
    let w = fw * STRIP as i32;
    let mut px = vec![[0u8; 4]; (w * h) as usize];
    for column in 0..STRIP as i32 {
        let ox = column * fw;
        let mut rect = |x0: i32, y0: i32, x1: i32, y1: i32, c: [u8; 4]| {
            for y in y0.max(0)..y1.min(h) {
                for x in x0.max(0)..x1.min(fw) {
                    px[(y * w + ox + x) as usize] = c;
                }
            }
        };
        let heading = column / FRAMES as i32;
        let stepping = column % FRAMES as i32 == 1;
        match heading {
            0 => {
                // Sideways, facing right: hair over the crown and down the back.
                rect(5, 1, 11, 4, HAIR);
                rect(5, 4, 7, 7, HAIR);
                rect(7, 4, 11, 9, SKIN);
                rect(6, 7, 7, 9, SKIN_DARK);
                rect(9, 6, 10, 7, EYE);
                rect(7, 9, 10, 10, SKIN_DARK);
            }
            1 => {
                // Back to the viewer: all hair, a bit of neck.
                rect(5, 1, 11, 8, HAIR);
                rect(5, 8, 11, 9, HAIR);
                rect(7, 9, 10, 10, SKIN_DARK);
            }
            _ => {
                // Facing the viewer: a fringe, two eyes, a mouth.
                rect(5, 1, 11, 4, HAIR);
                rect(5, 4, 6, 7, HAIR);
                rect(10, 4, 11, 7, HAIR);
                rect(6, 4, 10, 9, SKIN);
                rect(6, 8, 10, 9, SKIN_DARK);
                rect(6, 6, 7, 7, EYE);
                rect(9, 6, 10, 7, EYE);
                rect(7, 9, 10, 10, SKIN_DARK);
            }
        }
        // Tunic: lit on the left, shaded on the right, belted.
        rect(4, 10, 12, 19, TUNIC);
        rect(4, 10, 6, 15, TUNIC_LIGHT);
        rect(10, 10, 12, 19, TUNIC_DARK);
        rect(4, 17, 12, 18, BELT);
        if heading == 1 {
            // A seam down the back.
            rect(8, 10, 9, 17, TUNIC_DARK);
        }
        // Arms swing against the legs when stepping.
        let swing = if stepping { 1 } else { 0 };
        rect(2, 11 - swing, 4, 17 - swing, TUNIC_LIGHT);
        rect(2, 17 - swing, 4, 19 - swing, SKIN);
        rect(12, 11 + swing, 14, 17 + swing, TUNIC_DARK);
        rect(12, 17 + swing, 14, 19 + swing, SKIN_DARK);
        // Legs and boots: together when standing, apart mid-step.
        if stepping {
            rect(4, 19, 7, 25, LEGS);
            rect(3, 25, 7, 27, BOOT);
            rect(10, 19, 13, 26, LEGS_DARK);
            rect(10, 26, 14, 28, BOOT);
        } else {
            rect(5, 19, 8, 26, LEGS);
            rect(9, 19, 12, 26, LEGS_DARK);
            rect(4, 26, 8, 28, BOOT);
            rect(9, 26, 13, 28, BOOT);
        }
    }
    // One texel of ink around everything that was drawn, within each frame.
    let filled: Vec<bool> = px.iter().map(|p| p[3] != 0).collect();
    for y in 0..h {
        for x in 0..w {
            if filled[(y * w + x) as usize] {
                continue;
            }
            let touches = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                // Ink never crosses from one frame into the next.
                nx >= 0
                    && ny >= 0
                    && nx < w
                    && ny < h
                    && nx / fw == x / fw
                    && filled[(ny * w + nx) as usize]
            });
            if touches {
                px[(y * w + x) as usize] = INK;
            }
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
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
        app.add_message::<Landed>()
            .add_systems(Startup, spawn)
            .add_systems(
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

    #[test]
    fn figure_is_outlined_and_mostly_opaque() {
        let img = figure_texture();
        let data = img.data.as_ref().unwrap();
        let opaque = data.chunks(4).filter(|p| p[3] == 255).count();
        assert!(opaque > 900 && opaque < (FIGURE.x * FIGURE.y) as usize * STRIP);
        assert_eq!(img.width(), FIGURE.x * STRIP as u32);
        // The corners stay transparent.
        assert_eq!(data[3], 0);
    }
}
