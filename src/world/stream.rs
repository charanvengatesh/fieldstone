//! Keeps the chunks the camera can see loaded and their meshes current, within a time
//! budget per frame, and hands each mesh the view's projection as its transform.

use std::time::Instant;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::math::Affine3A;
use bevy::prelude::*;

use super::iso::View;
use super::mesh::{BlockMaterial, Grid, Quality, chunk_faces};
use super::tiles::Tiles;
use super::worldgen::WorldGen;
use super::{CHUNK, Chunk, Focus, HEIGHT, Map, chunk_of};

/// What the camera shows: half its width and height in texels.
#[derive(Resource, Clone, Copy)]
pub struct Viewport {
    pub half: Vec2,
}

impl Default for Viewport {
    fn default() -> Self {
        // A 1280 x 720 window at three pixels per texel.
        Self {
            half: Vec2::new(214.0, 120.0),
        }
    }
}

/// How eagerly the world streams in.
#[derive(Resource, Clone, Copy)]
pub struct Streaming {
    /// Milliseconds per frame spent generating and meshing chunks. At least one chunk is
    /// always processed, so the world never stalls.
    pub budget_ms: f32,
    /// Texels beyond the screen edge that are kept loaded, so walking never shows a gap.
    pub margin: f32,
}

impl Default for Streaming {
    fn default() -> Self {
        Self {
            budget_ms: 3.0,
            margin: 96.0,
        }
    }
}

/// Texels beyond the load margin a chunk may drift before it is unloaded, so a chunk on
/// the edge does not load and unload as the camera wobbles.
const KEEP_EXTRA: f32 = 192.0;

/// Numbers the debug line and scripts read.
#[derive(Resource, Default, Clone, Copy)]
pub struct Perf {
    /// Frames per second, smoothed.
    pub fps: f32,
    /// Milliseconds per frame, smoothed.
    pub frame_ms: f32,
    pub chunks: usize,
    /// Block faces across all loaded meshes.
    pub quads: usize,
    /// Milliseconds spent meshing in the last frame that meshed anything.
    pub mesh_ms: f32,
    /// Chunk meshes built since the game started.
    pub meshed: u64,
}

/// Marks the entity that draws a chunk.
#[derive(Component)]
pub struct ChunkMesh(pub IVec2);

/// Chunks whose blocks could appear within `reach` texels of the middle of the screen,
/// nearest the focus first.
pub fn visible_chunks(view: View, focus: Vec3, reach: Vec2) -> Vec<IVec2> {
    let centre = view.project(focus).truncate();
    let (lo, hi) = (centre - reach, centre + reach);
    // Every world column that can show on screen lies between where the screen's corners
    // land on the bottom and the top of the world.
    let mut min = IVec2::MAX;
    let mut max = IVec2::MIN;
    for corner in [lo, hi, Vec2::new(lo.x, hi.y), Vec2::new(hi.x, lo.y)] {
        for z in [0.0, HEIGHT as f32] {
            let c = chunk_of(view.unproject(corner, z).floor().as_ivec3());
            min = min.min(c);
            max = max.max(c);
        }
    }
    let home = chunk_of(focus.floor().as_ivec3());
    let mut out: Vec<IVec2> = (min.y..=max.y)
        .flat_map(|y| (min.x..=max.x).map(move |x| IVec2::new(x, y)))
        .filter(|c| {
            // The chunk's box on screen against the screen rectangle.
            let base = (*c * CHUNK).extend(0).as_vec3();
            let (mut a, mut b) = (Vec2::MAX, Vec2::MIN);
            for dx in [0, CHUNK] {
                for dy in [0, CHUNK] {
                    for dz in [0, HEIGHT] {
                        let p = view
                            .project(base + IVec3::new(dx, dy, dz).as_vec3())
                            .truncate();
                        a = a.min(p);
                        b = b.max(p);
                    }
                }
            }
            a.x <= hi.x && b.x >= lo.x && a.y <= hi.y && b.y >= lo.y
        })
        .collect();
    // The ground under the player first, the horizon last.
    out.sort_by_key(|c| (*c - home).length_squared());
    out
}

pub fn stream(
    focus: Res<Focus>,
    view: Res<View>,
    viewport: Res<Viewport>,
    streaming: Res<Streaming>,
    wg: Res<WorldGen>,
    mut map: ResMut<Map>,
    mut commands: Commands,
) {
    let started = Instant::now();
    let reach = viewport.half + Vec2::splat(streaming.margin);
    let keep = visible_chunks(*view, focus.0, reach + Vec2::splat(KEEP_EXTRA));
    let far: Vec<IVec2> = map
        .chunks
        .keys()
        .filter(|p| !keep.contains(p))
        .copied()
        .collect();
    for pos in far {
        let chunk = map.chunks.remove(&pos).expect("listed above");
        if chunk.modified {
            map.parked.insert(pos, chunk.blocks);
        }
        if let Some(e) = map.entities.remove(&pos) {
            commands.entity(e).despawn();
        }
    }
    // Generation gets the smaller share of the frame's budget; meshing gets the rest.
    let budget = streaming.budget_ms * 0.4;
    for (n, pos) in visible_chunks(*view, focus.0, reach)
        .into_iter()
        .filter(|p| !map.chunks.contains_key(p))
        .collect::<Vec<_>>()
        .into_iter()
        .enumerate()
    {
        if n > 0 && started.elapsed().as_secs_f32() * 1000.0 > budget {
            break;
        }
        let (blocks, modified) = match map.parked.remove(&pos) {
            Some(blocks) => (blocks, true),
            None => (wg.chunk(pos), false),
        };
        map.chunks.insert(pos, Chunk::new(blocks, modified));
    }
}

/// `Q` / `E` turn the view a quarter; `G` toggles the full block grid.
pub fn view_keys(
    input: Res<ButtonInput<KeyCode>>,
    keys: Res<crate::core::settings::Keys>,
    mut view: ResMut<View>,
    mut grid: ResMut<Grid>,
) {
    if keys.just(&input, &keys.rotate_left) {
        view.turn_by(std::f32::consts::FRAC_PI_2);
    }
    if keys.just(&input, &keys.rotate_right) {
        view.turn_by(-std::f32::consts::FRAC_PI_2);
    }
    if keys.just(&input, &keys.grid) {
        grid.0 = !grid.0;
    }
    if keys.just(&input, &keys.tilt) {
        let next = view.target_tilt.next();
        view.set_tilt(next);
    }
}

/// Moves the view toward the angle it was asked for.
pub fn ease_view(time: Res<Time<Real>>, mut view: ResMut<View>) {
    if view.turning() {
        let dt = time.delta_secs();
        view.ease(dt);
    }
}

/// A new outline mode or lighting quality changes every mesh.
pub fn settings_changed(grid: Res<Grid>, quality: Res<Quality>, mut map: ResMut<Map>) {
    if grid.is_changed() || quality.is_changed() {
        for chunk in map.chunks.values_mut() {
            chunk.dirty = true;
        }
    }
}

pub fn remesh(
    focus: Res<Focus>,
    grid: Res<Grid>,
    quality: Res<Quality>,
    streaming: Res<Streaming>,
    wg: Res<WorldGen>,
    tiles: Option<Res<Tiles>>,
    material: Option<Res<BlockMaterial>>,
    mut perf: ResMut<Perf>,
    mut map: ResMut<Map>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let (Some(tiles), Some(material)) = (tiles, material) else {
        return;
    };
    let center = chunk_of(focus.0.floor().as_ivec3());
    let mut dirty: Vec<IVec2> = map
        .chunks
        .iter()
        .filter(|(_, c)| c.dirty)
        .map(|(p, _)| *p)
        .collect();
    if dirty.is_empty() {
        perf.mesh_ms = 0.0;
        return;
    }
    dirty.sort_by_key(|p| (*p - center).length_squared());
    let started = Instant::now();
    let budget = streaming.budget_ms * 0.6;
    for (n, pos) in dirty.into_iter().enumerate() {
        if n > 0 && started.elapsed().as_secs_f32() * 1000.0 > budget {
            break;
        }
        // Cells outside the loaded area are answered by the generator, so a chunk's border
        // faces are right the first time and loading a neighbour never forces a rebuild.
        let faces = chunk_faces(pos, grid.0, *quality, &tiles, |p| {
            map.get(p).unwrap_or_else(|| wg.block(p))
        });
        let quads = faces.len();
        let old = map.entities.remove(&pos);
        if quads == 0 {
            if let Some(e) = old {
                commands.entity(e).despawn();
            }
        } else {
            let mesh = Mesh2d(meshes.add(faces.into_mesh()));
            let e = match old {
                Some(e) => {
                    commands.entity(e).insert(mesh);
                    e
                }
                None => commands
                    .spawn((
                        ChunkMesh(pos),
                        mesh,
                        MeshMaterial2d(material.0.clone()),
                        Transform::default(),
                        // Loaded chunks are on screen by construction, so culling would
                        // only cost time, and its bounds are not meant for sheared
                        // transforms.
                        NoFrustumCulling,
                    ))
                    .id(),
            };
            map.entities.insert(pos, e);
        }
        let chunk = map.chunks.get_mut(&pos).expect("listed above");
        chunk.dirty = false;
        chunk.quads = quads as u32;
        chunk.builds += 1;
        perf.meshed += 1;
    }
    perf.mesh_ms = started.elapsed().as_secs_f32() * 1000.0;
}

/// Gives every chunk mesh the view's projection as its transform. The mesh is in world
/// space, so this is all a turn of the view costs. It runs after transform propagation,
/// which would otherwise overwrite a matrix that is not a plain move, turn and scale.
pub fn apply_view(view: Res<View>, mut chunks: Query<(&ChunkMesh, &mut GlobalTransform)>) {
    let matrix = view.matrix();
    for (chunk, mut transform) in &mut chunks {
        let origin = (chunk.0 * CHUNK).extend(0).as_vec3();
        let wanted = GlobalTransform::from(Affine3A::from_mat3_translation(
            matrix,
            view.project(origin),
        ));
        if *transform != wanted {
            *transform = wanted;
        }
    }
}

/// Keeps the smoothed frame time and the world totals current.
pub fn measure(
    time: Res<Time<Real>>,
    fake: Res<crate::harness::Fake>,
    map: Res<Map>,
    mut perf: ResMut<Perf>,
) {
    let ms = fake.frame_ms.unwrap_or(time.delta_secs() * 1000.0);
    if ms > 0.0 {
        perf.frame_ms = if perf.frame_ms == 0.0 {
            ms
        } else {
            perf.frame_ms + (ms - perf.frame_ms) * 0.05
        };
        perf.fps = 1000.0 / perf.frame_ms;
    }
    perf.chunks = map.chunks.len();
    perf.quads = map.chunks.values().map(|c| c.quads as usize).sum();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_chunks_that_can_reach_the_screen_are_wanted() {
        let focus = Vec3::new(8.0, 8.0, 12.0);
        let reach = Viewport::default().half + Vec2::splat(Streaming::default().margin);
        for rot in 0..4 {
            let view = View::quarter(rot);
            let near = visible_chunks(view, focus, reach);
            // The chunk under the focus comes first, and far fewer than the old fixed
            // nine by nine square are needed.
            assert_eq!(near[0], IVec2::ZERO, "rot {rot}");
            assert!(near.len() >= 6 && near.len() < 40, "{} chunks", near.len());
            // A bigger screen needs a superset.
            let wide = visible_chunks(view, focus, reach * 3.0);
            assert!(wide.len() > near.len());
            assert!(near.iter().all(|c| wide.contains(c)));
            // Whatever is on screen at ground level is covered.
            let centre = view.project(focus).truncate();
            for corner in [reach, -reach, Vec2::new(reach.x, -reach.y)] {
                let ground = view.unproject(centre + corner * 0.99, 12.0);
                assert!(near.contains(&chunk_of(ground.floor().as_ivec3())));
            }
        }
    }

    #[test]
    fn every_point_on_screen_at_any_height_is_in_a_wanted_chunk() {
        let focus = Vec3::new(-6.5, -10.5, 12.0);
        let reach = Viewport::default().half + Vec2::splat(Streaming::default().margin);
        for degrees in [0.0f32, 30.0, 45.0, 90.0, 137.0, 200.0, 315.0] {
            let angle = degrees.to_radians();
            let view = View {
                angle,
                target: angle,
                ..default()
            };
            let wanted = visible_chunks(view, focus, reach);
            let centre = view.project(focus).truncate();
            for sx in -10..=10 {
                for sy in -10..=10 {
                    let screen = centre + reach * Vec2::new(sx as f32, sy as f32) / 10.0 * 0.999;
                    for z in 0..HEIGHT {
                        let p = view.unproject(screen, z as f32 + 0.5);
                        assert!(
                            wanted.contains(&chunk_of(p.floor().as_ivec3())),
                            "{degrees} degrees: {p} is on screen but its chunk is not wanted"
                        );
                    }
                }
            }
            println!("{degrees} degrees: {} chunks wanted", wanted.len());
        }
    }

    #[test]
    fn a_turning_view_wants_chunks_too() {
        let view = View {
            angle: 0.7,
            target: 1.0,
            ..default()
        };
        let near = visible_chunks(view, Vec3::new(-40.0, 100.0, 10.0), Vec2::new(300.0, 200.0));
        assert_eq!(near[0], IVec2::new(-3, 6));
        assert!(near.len() > 4);
    }
}
