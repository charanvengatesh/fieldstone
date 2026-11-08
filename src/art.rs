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
}

pub fn palette_color(palette: &Registry<ColorDef>, id: &str) -> Color {
    match palette.get(id) {
        Some(c) => Color::srgb_u8(c.rgb[0], c.rgb[1], c.rgb[2]),
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
        assets
            .colors
            .insert(c.id.clone(), Color::srgb_u8(c.rgb[0], c.rgb[1], c.rgb[2]));
        material(&c.id);
    }
    assets.unit_circle = meshes.add(Circle::new(1.0));
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

/// First color of a shape, used for UI swatches.
pub fn swatch(assets: &ShapeAssets, shapes: &Registry<ShapeDef>, shape: &str) -> Color {
    shapes
        .get(shape)
        .and_then(|s| s.parts.first())
        .map(|p| match p {
            Part::Circle { color, .. } | Part::Rect { color, .. } | Part::Poly { color, .. } => {
                color
            }
        })
        .and_then(|c| assets.colors.get(c).copied())
        .unwrap_or(Color::WHITE)
}

pub struct ArtPlugin;

impl Plugin for ArtPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShapeAssets>()
            .add_systems(PreStartup, build_shapes);
    }
}
