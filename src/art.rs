//! Vector rendering. A shape id from `shapes.ron` becomes a parent entity with one `Mesh2d`
//! child per part. Meshes and materials are built once per shape and shared.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use crate::core::{ColorDef, Part, Registry, ShapeDef};
use crate::world::chunk::CELL;

pub struct PartAssets {
    pub mesh: Handle<Mesh>,
    pub material: Handle<ColorMaterial>,
    pub transform: Transform,
}

#[derive(Resource, Default)]
pub struct ShapeAssets {
    pub shapes: HashMap<u16, Vec<PartAssets>>,
    pub colors: HashMap<String, Color>,
    /// One shared material per palette color, for effects that need a colored primitive.
    pub materials: HashMap<String, Handle<ColorMaterial>>,
    /// Unit circle (radius 1) for particles and glows; scale the transform.
    pub unit_circle: Handle<Mesh>,
    /// Unit square for dashes and glints.
    pub unit_rect: Handle<Mesh>,
    /// Rasterized icon per shape id, for UI.
    pub icons: HashMap<String, Handle<Image>>,
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
    shapes: Res<Registry<ShapeDef>>,
    palette: Res<Registry<ColorDef>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: ResMut<ShapeAssets>,
) {
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
    for (_, shape) in shapes.iter() {
        let icon = rasterize(&shape.parts, &assets.colors, ICON_SIZE);
        assets.icons.insert(shape.id.clone(), images.add(icon));
    }
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
                            // Source-over blend.
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
            // Regular polygon: inside if the distance along every edge normal is within the apothem.
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

/// Spawns a shape as children of a fresh entity at `transform`. Unknown shapes get an empty parent.
pub fn spawn_shape(
    commands: &mut Commands,
    assets: &ShapeAssets,
    shape: Option<u16>,
    transform: Transform,
) -> Entity {
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

pub struct ArtPlugin;

impl Plugin for ArtPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShapeAssets>()
            .add_systems(PreStartup, build_shapes);
    }
}
