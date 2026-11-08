//! Visuals. Every drawable thing names a visual id (the `shape` fields). In **vector** mode an
//! id becomes a parent entity with one `Mesh2d` child per shape part. In **pixel** mode it
//! becomes a `Sprite`: a PNG from `sprites.ron` when given, otherwise the shape rasterized at
//! 16 px per cell with a one-texel ink outline. Both are built once at startup and shared.

use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::sprite::Anchor;

use crate::core::packs::LoadedPacks;
use crate::core::settings::{RenderMode, Settings};
use crate::core::{ColorDef, Part, Registry, ShapeDef, SpriteDef};
use crate::world::chunk::CELL;

/// Texels per cell for generated pixel art.
pub const PX: f32 = 16.0;

pub struct PartAssets {
    pub mesh: Handle<Mesh>,
    pub material: Handle<ColorMaterial>,
    pub transform: Transform,
}

/// A ready-to-spawn pixel sprite.
#[derive(Clone)]
pub struct SpriteVisual {
    pub image: Handle<Image>,
    /// One frame, in texels (= world units).
    pub frame: Vec2,
    pub frames: u32,
    pub fps: f32,
    pub layout: Option<Handle<TextureAtlasLayout>>,
    /// Bevy anchor: (0, 0) is the center, range -0.5..0.5.
    pub anchor: Vec2,
}

#[derive(Resource, Default)]
pub struct ShapeAssets {
    pub mode: RenderMode,
    pub shapes: HashMap<u16, Vec<PartAssets>>,
    pub sprites: HashMap<u16, SpriteVisual>,
    pub colors: HashMap<String, Color>,
    /// One shared material per palette color, for effects that need a colored primitive.
    pub materials: HashMap<String, Handle<ColorMaterial>>,
    /// Unit circle (radius 1) for particles and glows; scale the transform.
    pub unit_circle: Handle<Mesh>,
    /// Unit square for dashes and glints.
    pub unit_rect: Handle<Mesh>,
    /// Icon per visual id, for UI.
    pub icons: HashMap<String, Handle<Image>>,
}

impl ShapeAssets {
    pub fn pixel(&self) -> bool {
        self.mode == RenderMode::Pixel
    }
}

/// Loops a sprite strip at `fps`. Facing-driven strips (fps 0) are stepped by their owner.
#[derive(Component)]
pub struct Animated {
    pub fps: f32,
    pub frames: u32,
    pub t: f32,
}

pub fn palette_color(palette: &Registry<ColorDef>, id: &str) -> Color {
    match palette.get(id) {
        Some(c) => Color::srgba_u8(c.rgb[0], c.rgb[1], c.rgb[2], c.a),
        None => {
            warn!("unknown palette color '{id}'");
            Color::srgb(1.0, 0.0, 1.0)
        }
    }
}

pub fn build_shapes(
    settings: Res<Settings>,
    shapes: Res<Registry<ShapeDef>>,
    sprites: Option<Res<Registry<SpriteDef>>>,
    palette: Res<Registry<ColorDef>>,
    packs: Res<LoadedPacks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
    mut assets: ResMut<ShapeAssets>,
) {
    assets.mode = settings.render;
    let mut mats: HashMap<String, Handle<ColorMaterial>> = HashMap::default();
    let mut material = |id: &str| {
        mats.entry(id.to_owned())
            .or_insert_with(|| {
                materials.add(ColorMaterial::from_color(palette_color(&palette, id)))
            })
            .clone()
    };
    for (_, c) in palette.iter() {
        assets.colors.insert(
            c.id.clone(),
            Color::srgba_u8(c.rgb[0], c.rgb[1], c.rgb[2], c.a),
        );
        material(&c.id);
    }
    assets.unit_circle = meshes.add(Circle::new(1.0));
    assets.unit_rect = meshes.add(Rectangle::new(1.0, 1.0));
    for (i, shape) in shapes.iter() {
        let parts = shape
            .parts
            .iter()
            .enumerate()
            .map(|(z, part)| {
                let (mesh, color, at, rot) = match part {
                    Part::Circle { r, color, at } => {
                        (meshes.add(Circle::new(r * CELL)), color, at, 0.0)
                    }
                    Part::Rect {
                        w,
                        h,
                        color,
                        at,
                        rot,
                    } => (
                        meshes.add(Rectangle::new(w * CELL, h * CELL)),
                        color,
                        at,
                        *rot,
                    ),
                    Part::Poly {
                        sides,
                        r,
                        color,
                        at,
                        rot,
                    } => (
                        meshes.add(RegularPolygon::new(r * CELL, *sides)),
                        color,
                        at,
                        *rot,
                    ),
                };
                PartAssets {
                    mesh,
                    material: material(color),
                    transform: Transform::from_xyz(at[0] * CELL, at[1] * CELL, 0.01 * z as f32)
                        .with_rotation(Quat::from_rotation_z(rot.to_radians())),
                }
            })
            .collect();
        assets.shapes.insert(i, parts);
    }
    assets.materials = mats;

    // Pixel sprites: a PNG from sprites.ron when given, otherwise the rasterized shape.
    let ink = assets
        .colors
        .get("ink")
        .copied()
        .unwrap_or(Color::srgb_u8(24, 30, 26));
    for (i, shape) in shapes.iter() {
        let def = sprites.as_ref().and_then(|r| r.get(&shape.id));
        let frames = def.map_or(1, |d| d.frames.max(1));
        let fps = def.map_or(0.0, |d| d.fps);
        let anchor = def.map_or(Vec2::ZERO, |d| {
            Vec2::new(d.anchor[0] - 0.5, d.anchor[1] - 0.5)
        });
        let loaded = def
            .and_then(|d| d.image.as_deref())
            .and_then(|rel| load_png(&packs, rel));
        let (image, frame) = match loaded {
            Some(img) => {
                let size = img.size().as_vec2();
                (img, Vec2::new(size.x / frames as f32, size.y))
            }
            None => {
                let rotate = def.is_some_and(|d| d.rotate && d.frames > 1);
                pixel_strip(
                    &shape.parts,
                    &assets.colors,
                    ink,
                    if rotate { frames } else { 1 },
                )
            }
        };
        let frames = if loaded_or_rotated(def) { frames } else { 1 };
        let layout = (frames > 1).then(|| {
            layouts.add(TextureAtlasLayout::from_grid(
                frame.as_uvec2(),
                frames,
                1,
                None,
                None,
            ))
        });
        let handle = images.add(image);
        assets.icons.insert(shape.id.clone(), handle.clone());
        assets.sprites.insert(
            i,
            SpriteVisual {
                image: handle,
                frame,
                frames,
                fps,
                layout,
                anchor,
            },
        );
    }
    // Vector mode keeps the larger supersampled icons.
    if assets.mode == RenderMode::Vector {
        for (_, shape) in shapes.iter() {
            let icon = rasterize(&shape.parts, &assets.colors, ICON_SIZE);
            assets.icons.insert(shape.id.clone(), images.add(icon));
        }
    }
}

fn loaded_or_rotated(def: Option<&SpriteDef>) -> bool {
    def.is_some_and(|d| d.image.is_some() || d.rotate)
}

/// Reads a PNG relative to whichever pack directory holds it (first match wins).
pub fn load_png(packs: &LoadedPacks, rel: &str) -> Option<Image> {
    for (dir, _) in packs.0.iter().rev() {
        let path = dir.join(rel);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        match Image::from_buffer(
            &bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::nearest(),
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        ) {
            Ok(img) => return Some(img),
            Err(e) => error!("{}: {e}", path.display()),
        }
    }
    debug!("image '{rel}' not found in any pack");
    None
}

/// Half-extent of a shape in cells (how far any part reaches from the origin).
fn extent(parts: &[Part]) -> f32 {
    parts
        .iter()
        .map(|p| match p {
            Part::Circle { r, at, .. } => Vec2::from(*at).abs().max_element() + r,
            Part::Rect { w, h, at, .. } => Vec2::from(*at).abs().max_element() + w.max(*h) * 0.71,
            Part::Poly { r, at, .. } => Vec2::from(*at).abs().max_element() + r,
        })
        .fold(0.3, f32::max)
}

/// Hard-pixel raster of a shape at 16 px per cell, as a horizontal strip of `frames` rotations,
/// with a one-texel ink outline around every opaque region. Returns the image and frame size.
pub fn pixel_strip(
    parts: &[Part],
    colors: &HashMap<String, Color>,
    ink: Color,
    frames: u32,
) -> (Image, Vec2) {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let half = extent(parts) * PX + 1.0;
    let size = ((half * 2.0).ceil() as u32).max(4).div_ceil(2) * 2;
    let span = size as f32 / PX;
    let resolved: Vec<([f32; 4], Part)> = parts
        .iter()
        .map(|p| {
            let id = match p {
                Part::Circle { color, .. }
                | Part::Rect { color, .. }
                | Part::Poly { color, .. } => color,
            };
            let c = colors
                .get(id)
                .copied()
                .unwrap_or(Color::srgb(1.0, 0.0, 1.0))
                .to_srgba();
            ([c.red, c.green, c.blue, c.alpha], p.clone())
        })
        .collect();
    let ink = ink.to_srgba();
    let w = size * frames;
    let mut data = vec![0u8; (w * size * 4) as usize];
    for f in 0..frames {
        let angle = -(f as f32) * std::f32::consts::TAU / frames as f32;
        let rot = Vec2::from_angle(angle);
        let mut frame = vec![[0.0f32; 4]; (size * size) as usize];
        for py in 0..size {
            for px in 0..size {
                let u = (px as f32 + 0.5) / size as f32;
                let v = (py as f32 + 0.5) / size as f32;
                let p = rot.rotate(Vec2::new((u - 0.5) * span, (0.5 - v) * span));
                let mut sample = [0.0f32; 4];
                for (col, part) in &resolved {
                    if inside(part, p) {
                        let a = col[3];
                        for k in 0..3 {
                            sample[k] = col[k] * a + sample[k] * (1.0 - a);
                        }
                        sample[3] = a + sample[3] * (1.0 - a);
                    }
                }
                // Hard pixels: anything half-covered or more is solid.
                if sample[3] >= 0.5 {
                    sample[3] = 1.0;
                } else {
                    sample = [0.0; 4];
                }
                frame[(py * size + px) as usize] = sample;
            }
        }
        // Outline: transparent texels touching a solid one become ink.
        let solid = |x: i32, y: i32| {
            x >= 0
                && y >= 0
                && x < size as i32
                && y < size as i32
                && frame[(y as u32 * size + x as u32) as usize][3] > 0.5
        };
        for py in 0..size as i32 {
            for px in 0..size as i32 {
                let i = (py as u32 * size + px as u32) as usize;
                let mut out = frame[i];
                if out[3] < 0.5
                    && (solid(px - 1, py)
                        || solid(px + 1, py)
                        || solid(px, py - 1)
                        || solid(px, py + 1))
                {
                    out = [ink.red, ink.green, ink.blue, 1.0];
                }
                let dst = ((py as u32 * w + f * size + px as u32) * 4) as usize;
                for k in 0..4 {
                    data[dst + k] = (out[k] * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: w,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    (image, Vec2::splat(size as f32))
}

pub const ICON_SIZE: u32 = 48;

/// Draws a shape into a small RGBA image with 3x supersampling. Shapes are authored in cell
/// units; the icon shows the range -0.55..0.55 so a one-cell object fills the square.
pub fn rasterize(parts: &[Part], colors: &HashMap<String, Color>, size: u32) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const SS: u32 = 3;
    let span = 1.1;
    let resolved: Vec<([f32; 4], &Part)> = parts
        .iter()
        .map(|p| {
            let id = match p {
                Part::Circle { color, .. }
                | Part::Rect { color, .. }
                | Part::Poly { color, .. } => color,
            };
            let c = colors
                .get(id)
                .copied()
                .unwrap_or(Color::srgb(1.0, 0.0, 1.0))
                .to_srgba();
            ([c.red, c.green, c.blue, c.alpha], p)
        })
        .collect();
    let mut data = vec![0u8; (size * size * 4) as usize];
    for py in 0..size {
        for px in 0..size {
            let mut acc = [0.0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let v = (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    let x = (u - 0.5) * span;
                    let y = (0.5 - v) * span;
                    let mut sample = [0.0f32; 4];
                    for (col, part) in &resolved {
                        if inside(part, Vec2::new(x, y)) {
                            let a = col[3];
                            for k in 0..3 {
                                sample[k] = col[k] * a + sample[k] * (1.0 - a);
                            }
                            sample[3] = a + sample[3] * (1.0 - a);
                        }
                    }
                    for k in 0..4 {
                        acc[k] += sample[k];
                    }
                }
            }
            let n = (SS * SS) as f32;
            let i = ((py * size + px) * 4) as usize;
            for k in 0..4 {
                data[i + k] = (acc[k] / n * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn inside(part: &Part, p: Vec2) -> bool {
    match part {
        Part::Circle { r, at, .. } => (p - Vec2::from(*at)).length() <= *r,
        Part::Rect { w, h, at, rot, .. } => {
            let q = Vec2::from_angle(-rot.to_radians()).rotate(p - Vec2::from(*at));
            q.x.abs() <= w / 2.0 && q.y.abs() <= h / 2.0
        }
        Part::Poly {
            sides, r, at, rot, ..
        } => {
            let q = Vec2::from_angle(-rot.to_radians()).rotate(p - Vec2::from(*at));
            let n = (*sides).max(3) as f32;
            let apothem = r * (std::f32::consts::PI / n).cos();
            (0..*sides).all(|i| {
                let angle = std::f32::consts::PI / n + i as f32 * std::f32::consts::TAU / n;
                q.dot(Vec2::from_angle(angle)) <= apothem
            })
        }
    }
}

/// Spawns a visual at `transform`: a sprite in pixel mode, shape parts in vector mode.
/// Unknown ids get an empty entity so callers can still attach components.
pub fn spawn_shape(
    commands: &mut Commands,
    assets: &ShapeAssets,
    shape: Option<u16>,
    transform: Transform,
) -> Entity {
    if assets.pixel() {
        if let Some(v) = shape.and_then(|s| assets.sprites.get(&s)) {
            let mut sprite = Sprite::from_image(v.image.clone());
            sprite.custom_size = Some(v.frame);
            if let Some(layout) = &v.layout {
                sprite.texture_atlas = Some(TextureAtlas {
                    layout: layout.clone(),
                    index: 0,
                });
            }
            let mut e = commands.spawn((sprite, Anchor(v.anchor), transform));
            if v.fps > 0.0 && v.frames > 1 {
                e.insert(Animated {
                    fps: v.fps,
                    frames: v.frames,
                    t: 0.0,
                });
            }
            return e.id();
        }
        return commands.spawn((transform, Visibility::default())).id();
    }
    let mut parent = commands.spawn((transform, Visibility::default()));
    if let Some(parts) = shape.and_then(|s| assets.shapes.get(&s)) {
        parent.with_children(|p| {
            for part in parts {
                p.spawn((
                    Mesh2d(part.mesh.clone()),
                    MeshMaterial2d(part.material.clone()),
                    part.transform,
                ));
            }
        });
    }
    parent.id()
}

pub fn animate(time: Res<Time>, mut q: Query<(&mut Animated, &mut Sprite)>) {
    for (mut a, mut s) in &mut q {
        a.t += time.delta_secs() * a.fps;
        if let Some(atlas) = &mut s.texture_atlas {
            atlas.index = (a.t as u32 % a.frames.max(1)) as usize;
        }
    }
}

pub struct ArtPlugin;

impl Plugin for ArtPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShapeAssets>()
            .add_systems(PreStartup, build_shapes)
            .add_systems(Update, animate);
    }
}
