# Changelog

## 1.2.0

The engine made fast: less work on the main thread, a third of the mesh bytes, a
profiler one flag away, and a harness that runs in seconds. Nothing visual changed; the
canvas reference and the GPU picture were compared at each step.

### New
- Instrumentation. `Perf` measures the main thread's own work per frame (`work_ms`, the
  schedule from `First` to `Last`, without the wait for the display), keeps 240 frames of
  it, counts `spikes` past `spike_ms` (default 12) and sums `mesh_bytes`. F4 cycles the
  numbers, the numbers with a frame-time graph, off. Lua: `perf()` fields, `perf_history`,
  `perf_reset`, `show_perf_graph`. `cargo run --release --features profile` opens Tracy
  spans for every system and the chunk tasks (generate, flood, faces, upload).
- `--bench` (`--test bench`): a glide across chunk boundaries, two turns, a zoom and a
  save, one line of numbers per phase, and a failure on any frame past the spike line.
  `--test NAME` runs the test files whose name contains NAME.
- `CANVAS_REF=name cargo test canvas_reference -- --ignored` draws a fixed world from a
  fixed camera with the software canvas, for pixel comparisons across changes.
- Startup is logged: packs, world, atlas, icons and the first frame, in milliseconds.

### Changed
- Chunk meshes are packed: one `Uint32x3` attribute a vertex (12 bytes, from 36) that
  `shaders/block_vertex.wgsl` unpacks, u16 indices, GPU-only. The tile's turn, the
  edges and the grid are found per cell in the fragment shaders from the block cell's
  hash (bit for bit the Rust `hash01`), so a quad may cover many cells: faces of one
  surface merge into runs and rectangles wherever the light interpolates to the same
  values the single cells had (`Quality.merge`, default on; Lua `merge`). The canvas
  mirror shares the helpers (`tile_turn`, `rotate_texel`, `border_edges`).
- The Lua engine functions are created once at VM setup and reach the world through a
  per-frame handle, instead of 70 closures and 70 string formats every frame.
- The player's block light comes from the grid its chunk's last build flooded instead
  of copying nine chunks and scanning them every frame. The HUD, the target highlight,
  the camera projection, the pause menu and the tooltip are written only when they
  change; the camera and performance mods write their HUD lines only on change.
  Particles share one material per chip colour.
- Worldgen remembers surface heights and whole columns (shared by the mesh tasks), so a
  mesh at the edge of the loaded world costs 1.4 ms instead of 6.7. Unloaded edited
  chunks are kept as runs (a few KB instead of 16). Saves serialise and write on the IO
  pool.
- `--test` runs flat out on a fixed 16 ms step: the whole harness takes about 3 s
  instead of 45. `builds` defaults to two per compute thread (2 to 8).
- Release profile: fat LTO.

### Measured (Apple M3 Pro)
- Binary 40.1 MB (1.1.0: 44.2 MB), fat LTO. Startup to the first frame: 379 ms windowed,
  of which packs 4 ms, atlas 1 ms, icons 1 ms; the rest is the window and the GPU.
- Release, windowed, orthographic, 170 chunks loaded (81 k faces, 4.6 MB of meshes):
  main-thread work per frame over an 8 s walk averages 0.54 ms, worst 2.1 ms, no frame
  past 12 ms, at `pixel_scale` 3 and 1 alike. (The display was asleep for these runs,
  which puts the app in its 10 Hz background mode, so wall-clock frame times were not
  re-measured; 1.1.0 measured 9.2 ms worst at the 120 Hz cap.)
- Faces in the harness world 26 410 to 19 493 (-26 %); mesh bytes 4.2 MB to 1.1 MB (-74 %:
  a third from merging, the rest from packing). Base-gen forest chunk: 660 to 521 faces,
  mesher 0.56 to 0.62 ms; with no neighbour loaded 6.7 to 1.4 ms (the column cache).
- `--bench` (debug, headless, fixed step): every phase under 1 ms of work a frame on
  average; the gate is 25 ms. Headless startup to the first frame: 8 ms.
- The whole harness (36 scenarios, three seeds): about 3 s a seed, from 45.

## 1.1.0

A smooth orthographic view. The parallel projection sees far more ground than the
perspective one (at 30 degrees a picture 30 blocks tall covers 60 blocks of ground and the
world's full height), and the streaming that kept up with it ran on the main thread.

### New
- A fog horizon. The haze is now fog that reaches the sky colour `horizon` blocks behind
  the focus along the view axis (default 40), and no chunk wholly past it is loaded: the far
  edge of the world fades out instead of popping. Lua `set_setting("horizon", blocks)`;
  the performance pack sets it with each level (48 / 40 / 32 / 24); `--horizon N` pins it
  for snapshots.
- Chunks are generated and meshed on Bevy's compute pool. The main thread only takes
  finished chunks in (`mesh_budget_ms` now caps that), `builds` (default 4) is how many
  run at once, and an edit during a build sends the chunk round again. Meshing waits for
  a chunk's wanted neighbours where it can: a missing neighbour is answered cell by cell
  by the generator, which is many times slower than reading a loaded chunk.
- `perf()` and the F4 line report `worst_ms` (the longest recent frame), `stream_ms`,
  `build_ms`, `uploads` and `inflight`; `dump_state` writes the same line with the view.
- Harness scenarios compare the orthographic and perspective views' chunks and streaming
  cost, and check a near horizon loads less.

### Changed
- The camera builds its view and projection once a frame (`Cam::lens`); streaming computes
  the visible set once, for both the keep and load reaches, and reuses it until the focus
  chunk, the view angle (to half a degree), the projection, the picture or the reach
  changes. Before, every chunk corner rebuilt both matrices, several thousand times a frame.

### Measured (Apple M3 Pro, release build, orthographic, pixel_scale 3)
- An eight-second walk east across chunk boundaries, same window and seed: worst frame
  18.5 ms on 1.0.1, 9.2 ms now; both average 8.3 ms, the display's 120 Hz cap.
  225 chunks loaded before, 180 now (the horizon at the performance pack's full level, 48).
- A settled frame's streaming pass: 0.004 ms orthographic, 0.007 ms perspective (the
  harness, debug build), down from a full visible-set computation every frame.
- Meshing one chunk with real content, full quality: 0.7 ms with its neighbours loaded,
  6.7 ms without (the generator answers each border cell), which is why meshing waits for
  neighbours. `cargo test build_timing -- --ignored --nocapture` compares the main thread
  against the pool.

## 1.0.1

### Changed
- The picture is drawn at the window's own resolution again; `pixel_scale` is the
  magnification, as it was before 1.0. A new `picture_scale` setting (default 1, up to 4)
  draws the whole picture coarser for anyone who wants the chunky look.
- With no shader pass on and `picture_scale` 1, the world camera draws straight to the
  window: no picture, no copy. The chain switches itself on the moment a pass is enabled.
  (Where a pass lives, base or mod, makes no difference to its cost.)
- The camera's dead zone and lean scale with the magnification, so they feel the same at
  every zoom.

## 1.0.0

The 3D camera and the shader chain, measured and settled. See the two alphas below for
what changed; this release fixes the picture's resize and records the cost.

### Changed
- The dithered palette pass is gone; it made the picture worse. `mods/shaders` keeps the
  outline and CRT passes, off by default, so the picture ships as the engine draws it.

### Fixed
- Changing the zoom (or the window) remade the picture in place, which made the renderer
  copy a texture without `COPY_SRC` and draw into a depth texture a frame behind. New
  pictures are made a frame early and swapped in once the renderer holds them.

### Measured (Apple M3 Pro, 2560 x 1440, release build)
- Binary 42.1 MB (0.9.8: 35.9 MB; `bevy_pbr`'s embedded shaders).
- 8.3 ms a frame with 55 chunks loaded and two passes on, which is the display's 120 Hz
  cap; the frame is not the limit.
- Mesher per chunk unchanged.

## 1.0.0-alpha.2

The picture and the shader chain.

### New
- The world camera draws into a picture of `window / pixel_scale` texels (`src/post.rs`);
  a compositor camera blows it up to the window with nearest sampling and hosts the UI at
  the window's own resolution. Zoom steps the picture's size.
- Pack shader passes: `shaders.ron` lists WGSL fragment shaders with an order and sixteen
  numbers; each runs over the picture in turn through one material whose pipeline key is
  the pass's shader. Bindings in `assets/shaders/blit.wgsl`. Lua `shader_passes()`,
  `set_shader(id, on)`, `set_shader_param(id, i, value)`.
- `mods/shaders`: optional `outline` (ink where brightness or hue jumps) and `crt`
  passes, both off by default; `F7` cycles plain, outline, crt.

## 1.0.0-alpha.1

The picture is drawn by a 3D camera. Depth was hard to read in the fixed isometric
projection, so the world now goes through Bevy's 3D pipeline with an orthographic camera
(perspective on a toggle) while keeping the baked lighting and the pixel look.

### Changed
- `src/cam.rs` owns the camera: yaw, pitch (presets `low` 20, `classic` 30, `high` 45,
  `top` 60 degrees; 30 is the true dimetric angle for 2:1 diamonds), a world-point focus,
  and the follow logic from `look.rs` (dead zone, lean, pan, zoom to the pointer, all in
  texels on the picture). `Cam::project` and `Cam::ray` are the only projection code; the
  picker (`world::pick`), the streamer, the harness and the software canvas use them, so
  headless snapshots still show what the GPU shows.
- Chunks, the figure, its silhouette, particles, the highlight and the ground shadow are
  ordinary `Mesh3d`s with plain transforms. Materials are `bevy_pbr::Material`s: unlit,
  two-sided, no shadow maps or prepass; shaders bind at group 3 and read Bevy's globals at
  group 0 binding 11. `iso.rs`, `look.rs`, `Projected` and `apply_view` are gone.
- The silhouette culls its back faces and sits 0.42 blocks toward the eye, so a block in
  the next cell brings it out and the figure never silhouettes itself.
- Haze is measured along the view axis from the camera's focus.
- `mods/camera`: a vertical Alt-drag pitches the camera; `P` toggles perspective; the
  compass names the pitch and the projection.
- Lua: `pitch()`, `set_pitch(degrees)`, `set_setting("perspective", bool)`;
  `camera_center()` returns the focus in world blocks; `view_angle()` is normalised to
  0..360. The save file keeps yaw, pitch and the projection (old files still load).
- `--perspective` for snapshots. `Picture` (the render target size in texels) replaces
  `Viewport`.
- Cargo feature `bevy_pbr` (no LUTs, no post-process, no glTF).

### Known
- The picture is still the window at `pixel_scale` window pixels per texel; the pixel
  render target and the shader chain come in alpha.2.

## 0.9.8

Depth cues, since "further back" and "higher up" land on the same pixel in this projection.

### Changed
- Shadows: the sun sits lower (a block of shadow per block of height, was two thirds)
  and a cast shadow takes 52 percent of the light (was 40). Boxed-in corners are darker
  (`OCCLUSION` 0.42 at the tightest), so walls meet the ground with a contact band.
- Height grading: 4 percent brighter per block of height (was 2.5), so terraces read
  apart.
- Drop edges: where a top face's edge gives onto clear air the ink is two texels deep and
  darker than a seam between materials (`pack_flags` carries a drop mask; flags are now
  scaled by 2048).
- Haze: beyond the camera's focus, blocks fade toward the sky colour, full at 28 depth
  units (`HAZE_REACH`, `HAZE_STRENGTH` 0.22). The focus depth rides in the block uniform
  in whole blocks, so it is written a few times a second while walking and never at rest.
- Harness: `00_arena.lua` flattens a pad around the spawn before the scenarios run; the
  zoom-to-pointer scenario builds its stone toward the viewer, above the terrain.

## 0.9.7

### New
- Jump: Space (rebindable as `jump`) while standing. It clears one block and not two, a
  ceiling stops it short, and a press in the air does nothing. The automatic step-up is
  gone: a one-block step takes a jump.

### Changed
- The walk is a continuous cycle driven by the distance walked: legs pivot at the hip,
  arms at the shoulder, with a small bob, easing back together on stopping. The
  two-pose stride is gone.
- `mods/camera` pans on Shift-drag now that Space jumps.

### Fixed
- The figure's face showed through the back of its head: the figure's material defaulted
  to blending, which writes no depth. It is opaque now.
- The figure silhouetted itself when it turned: the silhouette's depth lift was a constant
  smaller than the model's own depth span at some tilts and facings. The lift is now
  measured from the model each frame, just past its farthest point.

## 0.9.6

### Changed
- The player is a small block model now (`src/figure.rs`: head, body, arms, legs as boxes
  in world units, coloured per face and shaded by the terrain's sun) projected through
  the view exactly as chunks are, via a shared `Projected` component that
  `stream::apply_view` drives. It turns with the view, foreshortens with the tilt, faces
  the way it walks (eight facings) and is covered by blocks per vertex. The flat sprite
  strip is gone, and with it the figure texture.
- The figure, its silhouette and shadow, the target highlight and particles are placed in
  a `PlaceSet` that runs after the view settles for the frame, so nothing trails the
  world through a turn by a frame any more.
- The silhouette is the same model with its depth lifted past its own near faces
  (`SILHOUETTE_LIFT`), drawn from vertex colour; `shaders/silhouette.wgsl` no longer
  samples a texture.

## 0.9.5

### Fixed
- Turning the view (Q, E, R, the orbit drag, a tilt change) swung the camera around
  before it settled. The projection pivots on the world origin, so the player's point on
  screen moved and the camera chased it through the dead zone and its easing. The camera
  now takes that swing in full each frame, so the world turns about the player, and the
  view settles (`ViewSet`) before the camera reads it rather than a frame after.

## 0.9.4

Trim. After 0.9.0 to 0.9.3 the base had grown past what an engine needs; this release
removes rather than adds. The line the engine draws: rendering and simulation in Rust,
content and behaviour in packs.

### Removed
- The sky quad, sun, moon and stars: with the camera on the player you never saw them.
  The clear colour follows the clock instead (`core::time::sky_color`).
- Bloom and the HDR target (`bevy_post_process` feature gone). Glowing texels clamp to
  white.
- Cloud shadows, and with them the per-fragment noise and the inverse view matrix.
- The see-through dither, its uniforms and the screen-space Bayer code on both sides.
- Leaf holes: `leaves` is a solid tile again (the cutout path stays for `glass`).

### Changed
- When blocks hide the player, a pale silhouette of the figure shows through them
  (`SilhouetteArt`, `shaders/silhouette.wgsl`): the same quad drawn in the blend pass
  with the depth test turned round, so it appears only where something covers it. The
  `see_through` setting toggles it. The canvas mirrors it (`Canvas::draw_behind`).
- The block and water materials are no longer written every frame: time comes from
  Bevy's view globals, and the daylight uniform changes only when a channel moves a
  visible step. The renderer stops rebuilding those bind groups each frame.
- Fewer props on the ground (tufts, flowers, bluebells, mushrooms, pebbles at roughly
  a third of their densities).
- The meshing budget per frame defaults to 2 ms (was 3); the performance mod's levels
  step down from there.
- `[profile.dev] opt-level = 2` for the game crate: an incremental rebuild still takes
  about two seconds, and the dev build runs closer to release. `cargo run --release` is
  how it is meant to be played.
- Measured: the mesher costs the same as 0.9.1 per chunk (0.95 ms with shadows and
  occlusion, 0.40 ms without, dev profile).

## 0.9.3

Feel: cracks, chips and dust, a figure that turns, and a glow around lights.

### New
- Cracks: the target highlight shows a crack tile from the atlas once a block starts to
  wear, deepening through four stages (`Tiles::crack_uv`, drawn by `crack_tile`).
- Particles (`src/fx.rs`): breaking a block throws eight chips coloured from its tile;
  landing after a fall of a block or more raises dust. They fall, settle on the ground and
  fade in 0.6 s; at most 200 live. Lua `particles()`.
- The figure turns with the walk: sideways, back to the viewer, facing the viewer, each
  standing and stepping (a six-column strip). Standing still it breathes a texel every
  1.2 s; a landing squashes it for a tenth of a second. Lua `player_heading()`.
- Bloom: the camera renders HDR and blooms only what is brighter than white, so torches,
  lava and crystal glow a little while everything else stays crisp pixels. Needs the
  `bevy_post_process` feature; tonemapping stays linear.
- Lua `screen_of(x, y, z)`; harness scenarios for chips, dust and the heading; the
  zoom-to-pointer scenario builds its own target block so it no longer depends on what
  the terrain around the spawn looks like.

## 0.9.2

World: see-through water, leaves with gaps, glass, and small plants and stones.

### New
- Transparent water: each chunk's water surfaces are a second mesh (`WaterArt`,
  `shaders/water.wgsl`) blended over what lies under them. The bed is drawn, tinted
  blue and darker, and the shore gets foam along the water's edge instead of ink.
- Cutouts: `cutout: true` on a block makes alpha holes in its tiles real. Leaves have
  gaps, `glass` is a rimmed pane. Faces behind a cutout are drawn and light passes through.
- Props: `shape: Cross` blocks are two upright quads from the side tile, lit as tops, never
  in the way and gone in a touch. Patterns `Tuft`, `Flower`, `Mushroom`, `Pebbles`,
  `Torch`. Base scatters tufts, flowers, bluebells, mushrooms in the shade and pebbles on
  sand and stone; the torch is a prop now.
- Harness scenarios: walking through props, standing over water, a snapshot with glass and
  props; unit tests for the water split, the submerged flag, cutout culling and cross quads.

### Changed
- `Tiles` carries `cross` and `see_through` per block; `BlockDef::blocks_movement` is what
  physics asks (solid cubes only). Vertex flags gained a `submerged` bit.
- Only cubes (leaves included) cast shadows and darken corners; water and props do not.

## 0.9.1

Light: block light, a day that passes, a sky, cloud shadows.

### New
- Light sources: `light: 1..15` on a block makes it shine; light floods through open cells
  a level less per block and across chunk borders. Base adds `torch` (14) and `lava` (12);
  the example crystal lights at 9. Lua `light_at(x, y, z)`.
- Day and night: a 12 minute day, saved with the game and frozen while paused. Sunlight
  dims and warms toward dusk, night is blue moonlight plus whatever torches are about,
  while cast shadows fade with the sun. Lua `time_of_day()`, `set_time_of_day(f)`;
  `--time F` for snapshots.
- Sky: a gradient behind the world in dithered bands, with a sun by day, a crescent moon
  and stars by night.
- Cloud shadows drift over the tops of things by day, by world position, so they stay put
  when the view turns.
- Harness scenarios for the torch flood, the clock and a torch-at-night snapshot; unit
  tests for the flood, the daylight curve and the sky colours.

### Changed
- Vertex colour is now light channels (sun, block light, ambient) mixed in the shader by
  the time of day; `debug.rs` mirrors the same mix, so snapshots match the window.
- The player figure takes the daylight and nearby torches too.
- Harness tests that depended on the view rotation or on landing over water now measure
  which way they walk and move to dry ground.

## 0.9.0

Camera: see through what hides you, tilt the view, look around, zoom to the pointer.

### New
- See-through: blocks between the camera and the player dissolve into a pixel dither, so
  you are never lost behind a cliff or under a tree. The dither fits the figure and only
  takes what is nearer than the player. Replaces the ghost overlay (and its second camera).
  `set_setting("see_through", false)` turns it off.
- Tilt presets: `T` cycles low, classic, high and top as a smooth motion. The projection's
  own view direction defines depth at every tilt, so occlusion stays exact in between.
  Saved with the game; `--tilt NAME` for snapshots; Lua `tilt()`, `set_tilt(name)`.
- Look: the camera leaves the player alone inside a small dead zone, leans a little toward
  the pointer, and can be panned by a script. `mods/camera` pans while Space is held and
  the pointer moves; let go and it drifts home after a moment. Lua `pan_view`,
  `camera_pan`, `camera_center`, `set_setting("peek" / "dead_zone", bool)`.
- Zoom eases between levels and keeps the point under the pointer still (Ctrl or Cmd +
  scroll, pinch, `-` / `=`). Lua `zoom_at(level, x, y)`, `aim_screen(x, y)`.
- Harness scenarios for tilt, pan, peek and zoom-to-pointer; look logic has its own unit
  tests.

### Changed
- The compass line also names the tilt.

## 0.8.1

The debug mod becomes a test harness: scripts can play the game.

### New
- Lua can inject input the engine and other scripts cannot tell from a player's:
  `press_key`, `release_key`, `press_mouse`, `release_mouse`, `move_mouse`, `scroll`, `twist`
  (trackpad rotation), `aim_at(x, y, z)`, `clear_aim`. It lands at the start of the next
  frame where the real input systems put theirs, so "just pressed" lasts one frame.
- Fakes: `simulate_frame_ms(ms)` makes the performance numbers read as on a slow machine;
  `set_time_scale(s)` speeds the game clock; `teleport(x, y, z)`.
- Readouts: `frame`, `view_turning`, `hud_text(key)`, `selected_slot`, `is_grounded`,
  `target()`.
- `cargo run -- --test` loads every `tests/*.lua` in the packs. A file calls
  `test(name, function)`; the function runs as a coroutine and may `wait(frames)`,
  `wait_until(check, frames)` and `expect(condition, message)`. Tests run one at a time with
  nothing held between them. The process exits non-zero if any check failed, and CI runs it.
- `mods/debug/tests/`: sixteen scenarios covering injected keys and scrolling, Q and R,
  Alt-drag orbit, trackpad twist, Ctrl-scroll zoom, the compass, F4, F6, adaptive quality
  on a pretend 25 fps machine and its recovery, how much cheaper the lower quality levels
  rebuild the world, breaking and placing through the pointer, streaming after a long
  teleport, and walls versus steps.

### Fixed
- After a teleport the world never streamed in under the player: movement waited for the
  ground, and the camera focus (which drives streaming) only moved once movement ran.
- Q and E only turned the view in windowed mode.
- R in the camera mod returned to world north, not to the view the game started with.
- A failed self-test exited with status 0.
- An engine function fetched before a coroutine `wait` was dead afterwards; scripts now call
  permanent wrappers.

### Measured (dev build)
- Rebuilding about 24 loaded chunks: 38 ms at full quality, 21 to 23 ms with shadows and
  corner shading off. The GPU side of a frame is not measured headlessly.

## 0.8.0

Rotation and performance, as engine capabilities plus two mods that drive them.

### New
- The view is a continuous angle. Q / E turn it a quarter as a smooth motion, and any
  angle in between is a valid view.
- Chunk meshes are built in world space with every exposed side, and the view's projection
  is their transform. Turning the view rebuilds nothing; the smoke test checks that.
- The sun is fixed in the world, so turning the view shows the lit and the shaded sides of
  things instead of relighting them.
- Streaming loads only chunks that can appear on screen (about 20 at the default zoom,
  where 0.7 always loaded 81), at any zoom and angle, within a time budget per frame.
- In the background the game ticks ten times a second instead of flat out.
- Lua: `key_pressed`, `mouse_pressed`, `mouse_delta`, `scroll_delta`, `rotate_gesture`,
  `capture_pointer`, `window_focused`, `view_angle`, `view_target`, `turn_view`,
  `set_view_angle`, `snap_view`, `zoom`, `set_zoom`, `get_setting`, `set_setting`, `perf`,
  `set_hud`.
- `mods/camera`: hold Alt or the middle button and drag, or twist on a trackpad, to orbit
  freely; letting go settles on the nearest quarter view. R resets, Ctrl or Cmd + scroll
  zooms, C toggles a compass line.
- `mods/performance`: F4 shows frame rate, frame time, chunks and faces. When the frame
  rate stays low it steps quality down (smaller load margin, smaller mesh budget, no cast
  shadows, no corner shading) and back up when it recovers. F6 sets the level by hand.
- `--angle DEGREES` starts at a free view angle, for snapshots.

### Changed
- F3 shows smoothed frame rate, frame time, chunk and face counts and mesh time.
- The interface repaints slots only when the inventory or a panel changed.
- Chunk meshes are not frustum culled: they are on screen by construction, and the
  culler's bounds are wrong for a projection transform.

### Measured (dev build, Apple M3 Pro)
- Chunks loaded at the default zoom: 81 before, 17 to 23 now.
- Meshing one chunk: 0.66 ms with shadows and corner shading, 0.43 ms without shadows,
  0.37 ms with neither.

## 0.7.1

The art pass: pixel-art blocks, real lighting, a block shader.

### New
- Every block has two generated 16 x 16 pixel-art tiles (top and sides) drawn from its
  colours by a `pattern`: grass with a ragged lip over soil, bark and growth rings, blotchy
  stone, ore nuggets, rippled sand, leaves, waves. Colours come from hue-shifted ramps:
  shadows lean cool, highlights warm. Tiles turn and mirror from block to block so they do
  not repeat as a grid.
- A pack can replace any tile with `textures/<id>_top.png` and `textures/<id>_side.png`.
- Lighting: ambient occlusion darkens creases and corners, blocks cast shadows away from the
  sun, shade is tinted cool and sunlight warm. `glow` keeps a block bright in shadow.
- `assets/shaders/block.wgsl`: light is evaluated at each texel's centre and quantised into
  bands, so shading stays pixel art instead of smooth gradients; outlines darken whole
  texels; water rows slide and glint.
- The player is a shaded two-frame figure that dims under trees and has a soft shadow on
  the ground.
- Item icons are drawn from each block's own tiles.

### Changed
- No outline is drawn between blocks of the same colour family, so ore sits in stone
  without a grid around it.
- Software snapshots mirror the shader, so `--snapshot` and F9's `-soft` picture show the
  same lighting as the game.
- Fewer boulders.

## 0.7.0

A rebuild. The top-down survival game is retired to tag `v0.6.1`; this release is a small
isometric block engine meant to be extended by mods. Saves from 0.6 are ignored (they stay on
disk untouched).

### New
- Isometric view of a world made of blocks on an x, y, z grid, 32 levels tall. Dig down,
  build up. Four view rotations (Q / E).
- Exact occlusion from a depth buffer: every vertex carries `x + y + z`, so blocks and the
  player sort per pixel with no sorting code. A faint overlay keeps the player findable
  behind terrain.
- Flat-shaded art generated from one colour per block, with outlines on steps and material
  changes (G shows the full grid).
- Terrain from a height field with surface bands and a sea; trees, boulders and ore veins
  from `stamps.ron` and `features.ron`, which mods extend record by record.
- Physics with gravity and automatic one-block steps. Water cannot be crossed until bridged.
- Break, place, hotbar, bag with stack splitting, pause menu, save file with only edited
  chunks.
- Lua through `mlua` with a synchronous, sandboxed API: scripts read and write blocks and the
  inventory directly.
- Debug probe: F9 saves the window, a software-drawn copy of the view that works even when
  the window is hidden, and a text dump. `--snapshot` draws the world with no window at all.

### Removed
- Combat, creatures, hunger, crafting, farming, fishing, chests, the shrine quest, day and
  night, sound, the vector and pixel top-down renderers, shaders, shape recipes. They return
  as mods.
- Dependencies `noise` and `bevy_mod_scripting`; Bevy is built with 2D, UI and input
  features only.

## 0.6.1

Depth and building. The world reads as a 3/4 view with terraced land, and building gains
multi-tile pieces, connected walls, shelter and stack handling.

### Added
- Terraced elevation: land has heights 0 to 3 with generated cliff faces; a step is blocked
  unless a ramp bridges it. Natural ramps at cliff feet, craftable `ramp_wood`. Saves keep
  heights.
- Multi-tile objects (`size` in objects.ron): oak 2x2, bed 1x2, stone table 2x1. Placing
  checks the whole footprint; breaking any part breaks the whole.
- Connected walls (`connects` group): wood and stone walls join into runs, fences join and
  let arrows over (`low`).
- Shelter: an enclosed space counts as indoors, nothing spawns inside it, and a hint
  explains it the first night you are under a roof.
- Inventory handling: shift-click moves half a stack, right-click moves one, splitting
  into an empty slot, a Sort button on chests, hover tooltips for every item kind, and
  clicking a hotbar slot selects it.
- `structures.ron`: hand-drawn stamps (object, terrain, clear) placed by scatter rules with a
  `structure` field; storage objects in a stamp are stocked from a loot table on first
  generation. The base pack ships a `campsite` on meadow. New `stump` object.
- Creatures with four facing frames turn with their movement.

### Changed
- Sprites stand on their cell (feet anchors) and sort by screen y, so the player walks behind
  trees and in front of walls. Soft shadows survive pixelization. Standing art is taller.
- Higher terraces draw slightly brighter.

### Fixed
- The crystal example mod's worldgen override had fallen behind the base list (no oaks); a
  worldgen override replaces the whole record, so the file now mirrors base.

## 0.6.0

The first step of the pixel-art train. The game now renders as 16 px sprites and tiles by
default; the vector renderer stays available for one more release.

### Added
- Pixel renderer: `settings.ron` `render: Pixel | Vector` and `pixel_scale` (whole-number
  zoom, pinch or `-`/`=`); the camera snaps to the texel grid.
- `sprites.ron`: per-visual PNG strips with frames, fps and anchor; anything without art is
  rasterized from its shape at 16 px per cell with a one-texel outline. The player uses four
  generated facing frames.
- Dual-grid terrain tile sheets generated per terrain (`tiles/<terrain>.png` overrides),
  animated water frames.
- Hunger: a food bar that drains over the day; low food slows stamina and stops healing,
  starvation hurts. Food values on every edible; a campfire station cooks meat and fish.
- Creatures: deer (skittish) and bats (night, hills).

### Changed
- One cell is 16 world units (was 25). Saves record their cell size and rescale on load.
- Creature speeds rescaled to the new grid.

## 0.5.0

First public release: survival loop, combat and RPG stats, farming, fishing, Lua scripting
and example mods, shrine quest, animated water, procedural sound, pause menu and settings,
chests, bed and doors, release builds for macOS, Linux and Windows.
