# Changelog

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
