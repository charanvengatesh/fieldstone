//! Keeps the chunks the camera can see loaded and their meshes current, within a time
//! budget per frame.

use std::time::Instant;

use bevy::pbr::{Material, MeshMaterial3d};
use bevy::prelude::*;

use super::mesh::{BlockMaterial, Faces, Grid, Quality, WaterMaterial, chunk_faces};
use super::tiles::Tiles;
use super::worldgen::WorldGen;
use super::{CHUNK, Chunk, Focus, HEIGHT, Map, chunk_of, light};
use crate::cam::{Cam, Lens};

/// The picture's size in texels: the render target the camera draws into.
#[derive(Resource, Clone, Copy)]
pub struct Picture {
    pub size: Vec2,
}

impl Default for Picture {
    fn default() -> Self {
        // A 1280 x 720 window at three pixels per texel; the window and snapshots set
        // their own.
        Self {
            size: Vec2::new(427.0, 240.0),
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
    /// Blocks behind the focus, along the view axis, at which the fog is whole and past
    /// which no chunk is loaded.
    pub horizon: f32,
    /// `--horizon` on the command line holds the horizon against scripts (the performance
    /// pack sets it with each quality level), so a snapshot shows the asked-for fog.
    pub horizon_pinned: bool,
}

impl Default for Streaming {
    fn default() -> Self {
        Self {
            budget_ms: 2.0,
            margin: 96.0,
            horizon: 40.0,
            horizon_pinned: false,
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

/// Chunks whose blocks could appear within `reach` texels of the middle of the picture
/// seen through `lens` and within `horizon` blocks behind the focus, nearest `home` first.
#[cfg(test)]
pub fn visible_chunks(lens: &Lens, reach: Vec2, home: Vec3, horizon: f32) -> Vec<IVec2> {
    visible_sets(lens, reach, 0.0, home, horizon).1
}

/// How many blocks past the horizon a loaded chunk is kept before it is dropped, so a
/// step back and forth does not reload it.
const HORIZON_EXTRA: f32 = 12.0;

/// Two nested visible sets from one pass: the chunks within `reach + extra` texels and
/// `horizon + HORIZON_EXTRA` blocks behind the focus (to keep loaded) and, among them,
/// those within `reach` and `horizon` (to load). Both nearest `home` first. The chunk
/// under `home` and its ring are always in both.
pub fn visible_sets(
    lens: &Lens,
    reach: Vec2,
    extra: f32,
    home: Vec3,
    horizon: f32,
) -> (Vec<IVec2>, Vec<IVec2>) {
    let outer = reach + Vec2::splat(extra);
    let (lo, hi) = (-outer, outer);
    // Every world column that can show on screen lies between where the screen's corners
    // land on the bottom and the top of the world.
    let mut min = IVec2::MAX;
    let mut max = IVec2::MIN;
    for corner in [lo, hi, Vec2::new(lo.x, hi.y), Vec2::new(hi.x, lo.y)] {
        for z in [0.0, HEIGHT as f32] {
            let c = chunk_of(lens.unproject(corner, z).floor().as_ivec3());
            min = min.min(c);
            max = max.max(c);
        }
    }
    // A perspective camera's near ground can lie outside those corners; the focus chunk
    // and its ring are always kept.
    let home_chunk = chunk_of(home.floor().as_ivec3());
    min = min.min(home_chunk - IVec2::ONE);
    max = max.max(home_chunk + IVec2::ONE);
    let mut keep: Vec<IVec2> = Vec::new();
    let mut wanted: Vec<IVec2> = Vec::new();
    for y in min.y..=max.y {
        for x in min.x..=max.x {
            let c = IVec2::new(x, y);
            if (c - home_chunk).abs().max_element() <= 1 {
                keep.push(c);
                wanted.push(c);
                continue;
            }
            // The chunk's box on screen against the two screen rectangles, and its
            // nearest corner against the horizon.
            let base = (c * CHUNK).extend(0).as_vec3();
            let (mut a, mut b) = (Vec2::MAX, Vec2::MIN);
            let mut nearest = f32::MAX;
            for dx in [0, CHUNK] {
                for dy in [0, CHUNK] {
                    for dz in [0, HEIGHT] {
                        let corner = base + IVec3::new(dx, dy, dz).as_vec3();
                        let p = lens.project(corner).truncate();
                        a = a.min(p);
                        b = b.max(p);
                        nearest = nearest.min(lens.behind(corner));
                    }
                }
            }
            if a.x <= hi.x
                && b.x >= lo.x
                && a.y <= hi.y
                && b.y >= lo.y
                && nearest <= horizon + HORIZON_EXTRA
            {
                keep.push(c);
                if a.x <= reach.x
                    && b.x >= -reach.x
                    && a.y <= reach.y
                    && b.y >= -reach.y
                    && nearest <= horizon
                {
                    wanted.push(c);
                }
            }
        }
    }
    // The ground under the player first, the horizon last.
    keep.sort_by_key(|c| (*c - home_chunk).length_squared());
    wanted.sort_by_key(|c| (*c - home_chunk).length_squared());
    (keep, wanted)
}

/// The visible sets from the last frame they were computed on, and what they were
/// computed for; an ordinary frame reuses them.
#[derive(Resource, Default)]
pub struct Visible {
    key: Option<(IVec2, i32, i32, bool, UVec2, UVec2, i32)>,
    pub keep: Vec<IVec2>,
    pub wanted: Vec<IVec2>,
}

impl Visible {
    /// Recomputes the sets when the camera, the picture or the focus chunk moved; the
    /// angles count in half degrees.
    pub fn refresh(
        &mut self,
        cam: &Cam,
        picture: Vec2,
        reach: Vec2,
        extra: f32,
        home: Vec3,
        horizon: f32,
    ) {
        let key = (
            chunk_of(home.floor().as_ivec3()),
            (cam.yaw.to_degrees() * 2.0).round() as i32,
            (cam.pitch.to_degrees() * 2.0).round() as i32,
            cam.perspective,
            picture.as_uvec2(),
            reach.as_uvec2(),
            horizon.round() as i32,
        );
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        let lens = cam.lens(picture);
        let (keep, wanted) = visible_sets(&lens, reach, extra, home, horizon);
        self.keep = keep;
        self.wanted = wanted;
    }
}

pub fn stream(
    focus: Res<Focus>,
    cam: Res<Cam>,
    picture: Res<Picture>,
    streaming: Res<Streaming>,
    wg: Res<WorldGen>,
    mut visible: ResMut<Visible>,
    mut map: ResMut<Map>,
    mut commands: Commands,
) {
    let started = Instant::now();
    let reach = picture.size / 2.0 + Vec2::splat(streaming.margin);
    visible.refresh(
        &cam,
        picture.size,
        reach,
        KEEP_EXTRA,
        focus.0,
        streaming.horizon,
    );
    let keep = &visible.keep;
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
        if let Some(e) = map.waters.remove(&pos) {
            commands.entity(e).despawn();
        }
    }
    // Generation gets the smaller share of the frame's budget; meshing gets the rest.
    let budget = streaming.budget_ms * 0.4;
    for (n, pos) in visible
        .wanted
        .iter()
        .copied()
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
    mut cam: ResMut<Cam>,
    mut grid: ResMut<Grid>,
) {
    if keys.just(&input, &keys.rotate_left) {
        cam.turn_by(std::f32::consts::FRAC_PI_2);
    }
    if keys.just(&input, &keys.rotate_right) {
        cam.turn_by(-std::f32::consts::FRAC_PI_2);
    }
    if keys.just(&input, &keys.grid) {
        grid.0 = !grid.0;
    }
    if keys.just(&input, &keys.tilt) {
        let next = cam.next_pitch();
        cam.set_pitch(next);
    }
}

/// Moves the camera toward the angles it was asked for.
pub fn ease_view(time: Res<Time<Real>>, mut cam: ResMut<Cam>) {
    if cam.turning() {
        let dt = time.delta_secs();
        cam.ease(dt);
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
    water: Option<Res<WaterMaterial>>,
    mut perf: ResMut<Perf>,
    mut map: ResMut<Map>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let (Some(tiles), Some(material), Some(water)) = (tiles, material, water) else {
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
        let lamps = light::flood(
            pos,
            &map,
            &tiles.light,
            |b| tiles.passes[b as usize],
            |p| map.get(p).unwrap_or_else(|| wg.block(p)),
        );
        let faces = chunk_faces(pos, grid.0, *quality, &tiles, &lamps, |p| {
            map.get(p).unwrap_or_else(|| wg.block(p))
        });
        let quads = faces.solid.len() + faces.water.len();
        // Each mesh keeps or loses its entity on its own.
        let old = map.entities.remove(&pos);
        if let Some(e) = place(
            faces.solid,
            old,
            &material.0,
            pos,
            &mut meshes,
            &mut commands,
        ) {
            map.entities.insert(pos, e);
        }
        let old = map.waters.remove(&pos);
        if let Some(e) = place(faces.water, old, &water.0, pos, &mut meshes, &mut commands) {
            map.waters.insert(pos, e);
        }
        let chunk = map.chunks.get_mut(&pos).expect("listed above");
        chunk.dirty = false;
        chunk.quads = quads as u32;
        chunk.builds += 1;
        perf.meshed += 1;
    }
    perf.mesh_ms = started.elapsed().as_secs_f32() * 1000.0;
}

/// Puts `faces` on `old` or a new entity with `material`, or despawns `old` when there is
/// nothing to draw. Returns the entity that now draws the chunk, if any.
fn place<M: Material>(
    faces: Faces,
    old: Option<Entity>,
    material: &Handle<M>,
    pos: IVec2,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
) -> Option<Entity> {
    if faces.len() == 0 {
        if let Some(e) = old {
            commands.entity(e).despawn();
        }
        return None;
    }
    let mesh = Mesh3d(meshes.add(faces.into_mesh()));
    Some(match old {
        Some(e) => {
            commands.entity(e).insert(mesh);
            e
        }
        None => commands
            .spawn((
                mesh,
                MeshMaterial3d(material.clone()),
                Transform::from_translation((pos * CHUNK).extend(0).as_vec3()),
            ))
            .id(),
    })
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

    const T: Vec2 = Vec2::new(1280.0, 800.0);

    #[test]
    fn only_chunks_that_can_reach_the_screen_are_wanted() {
        let focus = Vec3::new(8.0, 8.0, 12.0);
        let reach = T / 2.0 + Vec2::splat(Streaming::default().margin);
        for rot in 0..4 {
            let mut cam = Cam::quarter(rot);
            cam.focus = focus;
            let near = visible_chunks(&cam.lens(T), reach, focus, f32::INFINITY);
            assert_eq!(near[0], IVec2::ZERO, "rot {rot}");
            // A 1280 x 800 picture at sixteen texels a block is eighty blocks wide.
            assert!(near.len() >= 6 && near.len() < 160, "{} chunks", near.len());
            let wide = visible_chunks(&cam.lens(T * 3.0), reach * 3.0, focus, f32::INFINITY);
            assert!(wide.len() > near.len());
            assert!(near.iter().all(|c| wide.contains(c)));
            for corner in [reach, -reach, Vec2::new(reach.x, -reach.y)] {
                let ground = cam.unproject(T, corner * 0.99, 12.0);
                assert!(near.contains(&chunk_of(ground.floor().as_ivec3())));
            }
        }
    }

    #[test]
    fn the_horizon_drops_far_chunks_and_keeps_near_ones() {
        let focus = Vec3::new(8.0, 8.0, 12.0);
        let reach = T / 2.0 + Vec2::splat(Streaming::default().margin);
        let mut cam = Cam::quarter(0);
        cam.focus = focus;
        let lens = cam.lens(T);
        let all = visible_chunks(&lens, reach, focus, f32::INFINITY);
        let near = visible_chunks(&lens, reach, focus, 24.0);
        assert!(near.len() < all.len(), "{} vs {}", near.len(), all.len());
        // Nothing within the horizon is dropped, nothing past it is kept.
        for c in &all {
            let base = (*c * CHUNK).extend(0).as_vec3();
            let nearest = (0..8)
                .map(|k| {
                    let d = IVec3::new(k & 1, (k >> 1) & 1, (k >> 2) & 1);
                    lens.behind(base + (d * IVec3::new(CHUNK, CHUNK, HEIGHT)).as_vec3())
                })
                .fold(f32::MAX, f32::min);
            let ring = (*c - chunk_of(focus.floor().as_ivec3()))
                .abs()
                .max_element()
                <= 1;
            assert_eq!(
                near.contains(c),
                nearest <= 24.0 || ring,
                "{c} at {nearest}"
            );
        }
        // The cache only recomputes when something it depends on changes.
        let mut v = Visible::default();
        v.refresh(&cam, T, reach, 0.0, focus, 24.0);
        let first = v.wanted.clone();
        cam.yaw += 0.001;
        v.refresh(&cam, T, reach, 0.0, focus, 24.0);
        assert_eq!(v.wanted, first);
        v.refresh(&cam, T, reach, 0.0, focus, 64.0);
        assert!(v.wanted.len() > first.len());
    }

    #[test]
    fn every_point_on_screen_at_any_height_is_in_a_wanted_chunk() {
        let focus = Vec3::new(-6.5, -10.5, 12.0);
        let reach = T / 2.0 + Vec2::splat(Streaming::default().margin);
        for degrees in [45.0f32, 75.0, 90.0, 137.0, 200.0, 315.0] {
            for (perspective, pitch) in [(false, 30.0f32), (true, 30.0), (false, 60.0)] {
                let mut cam = Cam::quarter(0);
                cam.yaw = degrees.to_radians();
                cam.target_yaw = cam.yaw;
                cam.pitch = pitch.to_radians();
                cam.target_pitch = cam.pitch;
                cam.perspective = perspective;
                cam.focus = focus;
                let wanted = visible_chunks(&cam.lens(T), reach, focus, f32::INFINITY);
                for sx in -10..=10 {
                    for sy in -10..=10 {
                        let screen = reach * Vec2::new(sx as f32, sy as f32) / 10.0 * 0.999;
                        for z in 0..HEIGHT {
                            let p = cam.unproject(T, screen, z as f32 + 0.5);
                            assert!(
                                wanted.contains(&chunk_of(p.floor().as_ivec3())),
                                "{degrees} degrees, perspective {perspective}, pitch {pitch}: {p} is on screen but its chunk is not wanted"
                            );
                        }
                    }
                }
            }
        }
    }
}
