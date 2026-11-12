//! Keeps the chunks the camera can see loaded and their meshes current, within a time
//! budget per frame.

use std::sync::Arc;
use std::time::Instant;

use bevy::camera::primitives::Aabb;
use bevy::mesh::Indices;
use bevy::pbr::{Material, MeshMaterial3d};
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};

use super::mesh::{BlockMaterial, ChunkFaces, Faces, Grid, Quality, WaterMaterial, chunk_faces};
use super::tiles::Tiles;
use super::worldgen::WorldGen;
use super::{Blocks, CHUNK, Chunk, Focus, HEIGHT, Map, Nearby, Parked, chunk_of, light};
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
    /// Milliseconds per frame the main thread spends taking finished chunks in (making
    /// their meshes). At least one chunk is always taken, so the world never stalls.
    pub budget_ms: f32,
    /// Chunks generating or meshing on the compute pool at once.
    pub builds: usize,
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
            // Two tasks per compute thread keeps the pool busy while results are taken in.
            builds: AsyncComputeTaskPool::try_get().map_or(4, |p| (p.thread_num() * 2).clamp(2, 8)),
            margin: 96.0,
            horizon: 40.0,
            horizon_pinned: false,
        }
    }
}

/// Texels beyond the load margin a chunk may drift before it is unloaded, so a chunk on
/// the edge does not load and unload as the camera wobbles.
const KEEP_EXTRA: f32 = 192.0;

/// Frames of main-thread work the graph remembers.
pub const HISTORY: usize = 240;

/// Numbers the debug line and scripts read.
#[derive(Resource, Clone, Copy)]
pub struct Perf {
    /// Frames per second, smoothed.
    pub fps: f32,
    /// Milliseconds per frame, smoothed.
    pub frame_ms: f32,
    /// The longest recent frame in milliseconds: a peak that settles back toward the
    /// average over a couple of seconds, so a hitch stays readable.
    pub worst_ms: f32,
    /// Main-thread milliseconds this frame spent in the game's own schedule (`First` to
    /// `Last`): the work, without the wait for the display or the headless runner's sleep.
    pub work_ms: f32,
    /// The last `HISTORY` frames' `work_ms`, oldest first from `head`.
    pub history: [f32; HISTORY],
    pub head: usize,
    /// Frames whose work went past `spike_ms` since the game started (or `perf_reset`).
    pub spikes: u32,
    pub spike_ms: f32,
    /// Bytes of chunk mesh data on the GPU.
    pub mesh_bytes: usize,
    pub chunks: usize,
    /// Block faces across all loaded meshes.
    pub quads: usize,
    /// Main-thread milliseconds taking finished chunks in, in the last frame that took any.
    pub mesh_ms: f32,
    /// Milliseconds the compute pool spent on the chunks that finished this frame.
    pub build_ms: f32,
    /// Main-thread milliseconds this frame choosing what to load and unload.
    pub stream_ms: f32,
    /// Chunk meshes made this frame.
    pub uploads: usize,
    /// Chunks generating or meshing on the compute pool right now.
    pub inflight: usize,
    /// Chunk meshes built since the game started.
    pub meshed: u64,
}

impl Default for Perf {
    fn default() -> Self {
        Self {
            fps: 0.0,
            frame_ms: 0.0,
            worst_ms: 0.0,
            work_ms: 0.0,
            history: [0.0; HISTORY],
            head: 0,
            spikes: 0,
            spike_ms: 12.0,
            mesh_bytes: 0,
            chunks: 0,
            quads: 0,
            mesh_ms: 0.0,
            build_ms: 0.0,
            stream_ms: 0.0,
            uploads: 0,
            inflight: 0,
            meshed: 0,
        }
    }
}

impl Perf {
    /// Forgets the spikes and the peak, for a measurement that starts now.
    pub fn reset(&mut self) {
        self.spikes = 0;
        self.worst_ms = 0.0;
        self.history = [0.0; HISTORY];
        self.head = 0;
    }
}

/// When the frame's work began; `begin_frame` sets it in `First`, `end_frame` reads it
/// in `Last`.
#[derive(Resource, Default)]
pub struct FrameClock(Option<Instant>);

pub fn begin_frame(mut clock: ResMut<FrameClock>) {
    clock.0 = Some(Instant::now());
}

pub fn end_frame(clock: Res<FrameClock>, mut perf: ResMut<Perf>) {
    let Some(started) = clock.0 else {
        return;
    };
    let ms = started.elapsed().as_secs_f32() * 1000.0;
    perf.work_ms = ms;
    let head = perf.head;
    perf.history[head] = ms;
    perf.head = (head + 1) % HISTORY;
    if ms > perf.spike_ms {
        perf.spikes += 1;
    }
}

/// What a task hands back, and how long it took in milliseconds.
pub enum Built {
    /// A chunk's blocks, fresh from the generator.
    Generated(Blocks, f32),
    /// A loaded chunk's faces and the block light they were lit by.
    Meshed(Box<(ChunkFaces, light::Grid)>, f32),
}

/// Chunks being generated or meshed on the compute pool, and the shared inputs the tasks
/// read. Generation and meshing are separate tasks: a chunk is meshed once it is loaded,
/// ideally after its neighbours are too, since cells outside the loaded area are answered
/// by the generator one at a time, which is far slower than reading a chunk's array.
#[derive(Resource, Default)]
pub struct Builds {
    worldgen: Option<Arc<WorldGen>>,
    tiles: Option<Arc<Tiles>>,
    pub inflight: HashMap<IVec2, Task<Built>>,
}

impl Builds {
    fn worldgen(&mut self, wg: &WorldGen) -> Arc<WorldGen> {
        self.worldgen
            .get_or_insert_with(|| Arc::new(wg.clone()))
            .clone()
    }

    /// Starts generating chunk `pos`.
    fn generate(&mut self, pos: IVec2, wg: &WorldGen) {
        let worldgen = self.worldgen(wg);
        let pool = AsyncComputeTaskPool::get_or_init(TaskPool::new);
        let task = pool.spawn(async move {
            let _span = info_span!("generate").entered();
            let started = Instant::now();
            let blocks = worldgen.chunk(pos);
            Built::Generated(blocks, started.elapsed().as_secs_f32() * 1000.0)
        });
        self.inflight.insert(pos, task);
    }

    /// Starts meshing loaded chunk `pos` from a snapshot of the map around it.
    #[allow(clippy::too_many_arguments)]
    fn mesh(
        &mut self,
        pos: IVec2,
        map: &Map,
        wg: &WorldGen,
        tiles: &Tiles,
        tiles_changed: bool,
        grid: bool,
        quality: Quality,
    ) {
        let worldgen = self.worldgen(wg);
        if tiles_changed || self.tiles.is_none() {
            self.tiles = Some(Arc::new(tiles.clone()));
        }
        let tiles = self.tiles.clone().expect("set above");
        let nearby = Nearby::of(map, pos);
        let pool = AsyncComputeTaskPool::get_or_init(TaskPool::new);
        let task = pool.spawn(async move {
            let _span = info_span!("mesh").entered();
            let started = Instant::now();
            let (faces, lamps) = build(pos, &nearby, &worldgen, &tiles, grid, quality);
            Built::Meshed(
                Box::new((faces, lamps)),
                started.elapsed().as_secs_f32() * 1000.0,
            )
        });
        self.inflight.insert(pos, task);
    }
}

/// The faces of chunk `pos` from the blocks around it, and the light grid they were lit
/// by. Cells outside the snapshot are answered by the generator, so a chunk's border faces
/// are right the first time and loading a neighbour never forces a rebuild.
pub fn build(
    pos: IVec2,
    nearby: &Nearby,
    wg: &WorldGen,
    tiles: &Tiles,
    grid: bool,
    quality: Quality,
) -> (ChunkFaces, light::Grid) {
    let block = |p: IVec3| nearby.get(p).unwrap_or_else(|| wg.block(p));
    let lamps = {
        let _span = info_span!("flood").entered();
        light::flood(
            pos,
            nearby,
            &tiles.light,
            |b| tiles.passes[b as usize],
            block,
        )
    };
    let _span = info_span!("faces").entered();
    let faces = chunk_faces(pos, grid, quality, tiles, &lamps, block);
    (faces, lamps)
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

#[allow(clippy::too_many_arguments)]
pub fn stream(
    focus: Res<Focus>,
    cam: Res<Cam>,
    picture: Res<Picture>,
    streaming: Res<Streaming>,
    wg: Res<WorldGen>,
    mut visible: ResMut<Visible>,
    mut builds: ResMut<Builds>,
    mut perf: ResMut<Perf>,
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
            map.parked.insert(pos, Parked::pack(&chunk.blocks));
        }
        if let Some(e) = map.entities.remove(&pos) {
            commands.entity(e).despawn();
        }
        if let Some(e) = map.waters.remove(&pos) {
            commands.entity(e).despawn();
        }
    }
    // Chunks to load, nearest first. Edited chunks come back from the park at once (their
    // blocks are kept, only the mesh is missing); new ground is generated on the pool. A
    // generation that is no longer wanted is dropped when it lands.
    let missing: Vec<IVec2> = visible
        .wanted
        .iter()
        .copied()
        .filter(|p| !map.chunks.contains_key(p) && !builds.inflight.contains_key(p))
        .collect();
    for pos in missing {
        if let Some(parked) = map.parked.remove(&pos) {
            map.chunks.insert(pos, Chunk::new(parked.unpack(), true));
            continue;
        }
        if builds.inflight.len() >= streaming.builds.max(1) {
            break;
        }
        builds.generate(pos, &wg);
    }
    perf.stream_ms = started.elapsed().as_secs_f32() * 1000.0;
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

#[allow(clippy::too_many_arguments)]
pub fn remesh(
    focus: Res<Focus>,
    grid: Res<Grid>,
    quality: Res<Quality>,
    streaming: Res<Streaming>,
    wg: Res<WorldGen>,
    tiles: Option<Res<Tiles>>,
    material: Option<Res<BlockMaterial>>,
    water: Option<Res<WaterMaterial>>,
    visible: Res<Visible>,
    mut builds: ResMut<Builds>,
    mut perf: ResMut<Perf>,
    mut map: ResMut<Map>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let (Some(tiles), Some(material), Some(water)) = (tiles, material, water) else {
        return;
    };
    let center = chunk_of(focus.0.floor().as_ivec3());
    // Meshes for dirty chunks: those with every wanted neighbour loaded first (a missing
    // neighbour is answered cell by cell by the generator, many times slower), then by
    // distance. A chunk's dirty flag clears when its build starts; an edit during the
    // build sets it again and it goes round once more.
    let wanted: HashSet<IVec2> = visible.wanted.iter().copied().collect();
    let frontier = |p: IVec2| {
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                let n = p + IVec2::new(dx, dy);
                wanted.contains(&n) && !map.chunks.contains_key(&n)
            })
        })
    };
    let mut dirty: Vec<(bool, i32, IVec2)> = map
        .chunks
        .iter()
        .filter(|(p, c)| c.dirty && !builds.inflight.contains_key(*p))
        .map(|(p, _)| (frontier(*p), (*p - center).length_squared(), *p))
        .collect();
    dirty.sort_unstable_by_key(|(frontier, d, _)| (*frontier, *d));
    let tiles_changed = tiles.is_changed();
    for (_, _, pos) in dirty {
        if builds.inflight.len() >= streaming.builds.max(1) {
            break;
        }
        builds.mesh(pos, &map, &wg, &tiles, tiles_changed, grid.0, *quality);
        map.chunks.get_mut(&pos).expect("listed above").dirty = false;
    }
    // Finished tasks, nearest first, within the budget for making meshes.
    let mut done: Vec<IVec2> = builds
        .inflight
        .iter_mut()
        .filter(|(_, t)| t.is_finished())
        .map(|(p, _)| *p)
        .collect();
    perf.uploads = 0;
    perf.build_ms = 0.0;
    perf.inflight = builds.inflight.len();
    if done.is_empty() {
        perf.mesh_ms = 0.0;
        return;
    }
    done.sort_by_key(|p| (*p - center).length_squared());
    let started = Instant::now();
    for (n, pos) in done.into_iter().enumerate() {
        if n > 0 && started.elapsed().as_secs_f32() * 1000.0 > streaming.budget_ms {
            break;
        }
        let task = builds.inflight.remove(&pos).expect("listed above");
        let Some(built) = block_on(poll_once(task)) else {
            continue;
        };
        let _span = info_span!("upload").entered();
        let faces = match built {
            // The chunk arrives dirty and is meshed next, unless it is no longer wanted
            // or came in by another road (the park) meanwhile.
            Built::Generated(blocks, ms) => {
                perf.build_ms += ms;
                if !map.chunks.contains_key(&pos) && visible.keep.contains(&pos) {
                    map.chunks.insert(pos, Chunk::new(blocks, false));
                }
                continue;
            }
            // The chunk may have been unloaded while its mesh was built.
            Built::Meshed(built, ms) => {
                perf.build_ms += ms;
                let Some(chunk) = map.chunks.get_mut(&pos) else {
                    continue;
                };
                let (faces, lamps) = *built;
                chunk.light = lamps;
                faces
            }
        };
        let quads = faces.solid.len() + faces.water.len();
        // Each mesh keeps or loses its entity on its own.
        let old = map.entities.remove(&pos);
        let mut bytes = 0;
        if let Some((e, b)) = place(
            faces.solid,
            old,
            &material.0,
            pos,
            &mut meshes,
            &mut commands,
        ) {
            map.entities.insert(pos, e);
            bytes += b;
        }
        let old = map.waters.remove(&pos);
        if let Some((e, b)) = place(faces.water, old, &water.0, pos, &mut meshes, &mut commands) {
            map.waters.insert(pos, e);
            bytes += b;
        }
        let chunk = map.chunks.get_mut(&pos).expect("present above");
        chunk.quads = quads as u32;
        chunk.bytes = bytes as u32;
        chunk.builds += 1;
        perf.meshed += 1;
        perf.uploads += 1;
    }
    perf.inflight = builds.inflight.len();
    perf.mesh_ms = started.elapsed().as_secs_f32() * 1000.0;
}

/// Puts `faces` on `old` or a new entity with `material`, or despawns `old` when there is
/// nothing to draw. Returns the entity that now draws the chunk and the mesh's bytes.
fn place<M: Material>(
    faces: Faces,
    old: Option<Entity>,
    material: &Handle<M>,
    pos: IVec2,
    meshes: &mut Assets<Mesh>,
    commands: &mut Commands,
) -> Option<(Entity, usize)> {
    if faces.len() == 0 {
        if let Some(e) = old {
            commands.entity(e).despawn();
        }
        return None;
    }
    let mesh = faces.into_block_mesh();
    let bytes = mesh_bytes(&mesh);
    let mesh = Mesh3d(meshes.add(mesh));
    let entity = match old {
        Some(e) => {
            commands.entity(e).insert(mesh);
            e
        }
        // A packed mesh has no float positions for Bevy to measure, so the chunk's box is
        // given by hand and frustum culling keeps working.
        None => commands
            .spawn((
                mesh,
                MeshMaterial3d(material.clone()),
                Transform::from_translation((pos * CHUNK).extend(0).as_vec3()),
                Aabb::from_min_max(
                    Vec3::ZERO,
                    Vec3::new(CHUNK as f32, CHUNK as f32, HEIGHT as f32),
                ),
            ))
            .id(),
    };
    Some((entity, bytes))
}

/// Vertex and index bytes of a mesh as the GPU holds them.
pub fn mesh_bytes(mesh: &Mesh) -> usize {
    let vertices = mesh.count_vertices() * mesh.get_vertex_size() as usize;
    let indices = match mesh.indices() {
        Some(Indices::U16(i)) => i.len() * 2,
        Some(Indices::U32(i)) => i.len() * 4,
        None => 0,
    };
    vertices + indices
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
        let settle = (ms / 2000.0).min(1.0);
        perf.worst_ms = ms.max(perf.worst_ms - (perf.worst_ms - perf.frame_ms) * settle);
    }
    perf.chunks = map.chunks.len();
    perf.quads = map.chunks.values().map(|c| c.quads as usize).sum();
    perf.mesh_bytes = map.chunks.values().map(|c| c.bytes as usize).sum();
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

#[cfg(test)]
mod build_timing {
    use super::*;

    /// `cargo test build_timing -- --ignored --nocapture`: one chunk build on the main
    /// thread against the same build on the compute pool, four at a time.
    #[test]
    #[ignore]
    fn build_timing() {
        let (wg, blocks) = crate::world::worldgen::tests::base_gen(42);
        let tiles = crate::world::tiles::atlas(&blocks, None).1;
        let mut map = Map::default();
        for cy in -3..3 {
            for cx in -3..3 {
                let pos = IVec2::new(cx, cy);
                map.chunks.insert(pos, Chunk::new(wg.chunk(pos), false));
            }
        }
        let quality = Quality::default();
        let started = Instant::now();
        for cy in -2..2 {
            for cx in -2..2 {
                let pos = IVec2::new(cx, cy);
                let nearby = Nearby::of(&map, pos);
                std::hint::black_box(build(pos, &nearby, &wg, &tiles, false, quality).0);
            }
        }
        println!(
            "main thread: {:.2} ms per chunk",
            started.elapsed().as_secs_f32() * 1000.0 / 16.0
        );
        // The same chunks with no neighbour loaded: every border cell is the generator's.
        let started = Instant::now();
        for cy in -2..2 {
            for cx in -2..2 {
                let pos = IVec2::new(cx, cy);
                let mut lone = Map::default();
                lone.chunks.insert(pos, Chunk::new(wg.chunk(pos), false));
                let nearby = Nearby::of(&lone, pos);
                std::hint::black_box(build(pos, &nearby, &wg, &tiles, false, quality).0);
            }
        }
        println!(
            "no neighbours: {:.2} ms per chunk",
            started.elapsed().as_secs_f32() * 1000.0 / 16.0
        );
        let pool = AsyncComputeTaskPool::get_or_init(TaskPool::new);
        let wg = Arc::new(wg);
        let tiles = Arc::new(tiles);
        let started = Instant::now();
        let mut tasks = Vec::new();
        for cy in -2..2 {
            for cx in -2..2 {
                let pos = IVec2::new(cx, cy);
                let nearby = Nearby::of(&map, pos);
                let (wg, tiles) = (wg.clone(), tiles.clone());
                tasks.push(pool.spawn(async move {
                    let t = Instant::now();
                    std::hint::black_box(build(pos, &nearby, &wg, &tiles, false, quality).0);
                    t.elapsed().as_secs_f32() * 1000.0
                }));
            }
        }
        let mut task_ms = 0.0;
        for t in tasks {
            task_ms += block_on(t);
        }
        println!(
            "pool ({} threads): {:.2} ms wall per chunk, {:.2} ms task time per chunk",
            pool.thread_num(),
            started.elapsed().as_secs_f32() * 1000.0 / 16.0,
            task_ms / 16.0
        );
    }
}
