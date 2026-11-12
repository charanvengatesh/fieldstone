# Fieldstone

A small isometric block engine in Rust and Bevy, built to be modded. The world is cubes on a
grid: walk over them, break them, stack them. Everything concrete (which blocks exist, how the
land is shaped, what grows on it) is data in a pack, and behaviour is Lua. The base pack is
deliberately bare; features arrive as mods.

The earlier top-down survival game lives on at tag `v0.6.1`.

## Controls

| Input | Action |
|-------|--------|
| WASD / arrow keys | Walk along the screen axes |
| Space | Jump: clears one block, not two |
| Hold click | Break the block under the pointer (within five blocks) |
| Right click (two-finger click) | Place the held block against the face under the pointer |
| 1-0, two-finger scroll | Choose a hotbar slot |
| Tab or I | Bag. Click a slot to swap it with the held slot, shift-click splits half, right-click one |
| Q / E | Turn the view a quarter, smoothly |
| T | Next pitch preset: low (20), classic (30), high (45), top (60 degrees) |
| P | Perspective on or off (`mods/camera`) |
| F7 | Cycle the look: plain, outline, crt (`mods/shaders`, plain by default) |
| Shift + move the pointer | Pan away from the player; let go to drift back (`mods/camera`) |
| Alt + drag, middle-drag, or trackpad twist | Orbit freely: sideways turns, up and down pitches; letting go settles on the nearest quarter (`mods/camera`) |
| R | Back to the starting view (`mods/camera`) |
| Ctrl or Cmd + scroll | Zoom (`mods/camera`) |
| Pinch, or - / = | Zoom, eased, toward the pointer |
| G | Outline every block (the classic grid look) or only steps and material changes |
| Esc | Pause: resume, save, save and quit |
| F5 | Save to `saves/iso.ron`, loaded on the next start |
| F3 | Debug line: frame rate, position, chunks, faces, target |
| F4 / F6 | Performance line, then the line with a frame-time graph, then off; quality level by hand (`mods/performance`) |
| F9 | Debug capture (with `mods/debug` enabled): pictures and a text dump under `debug/` |

Keys are rebindable in `settings.ron`, written next to the executable on first run; it also
holds `pixel_scale` (the zoom, 1 to 6) and `picture_scale` (1 draws at the window's
resolution; 2 to 4 make the whole picture chunky).

A one-block hole hides its own floor behind its near walls. To fill it, click the far wall.
When blocks hide the player, a pale silhouette shows through them; the camera leans a
little toward the pointer and waits for the player to leave a small box before it follows.

## Run

```sh
cargo run                        # dev build; first build compiles Bevy and takes a few minutes
cargo run --release              # how it is meant to be played: a few times faster
cargo run -- --new               # ignore the save and start a fresh world
cargo run -- --seed 42           # fix the world seed
cargo test                       # unit tests
cargo run -- --headless-smoke    # no window: walk, break, place, save, verify; non-zero on failure
cargo run -- --test [NAME]       # no window: run the packs' tests/*.lua (whose file name contains NAME) flat out on a fixed 16 ms step; non-zero on failure
cargo run -- --bench             # --test bench: a glide, turns, a zoom and a save; prints one BENCH line, fails on a spike
cargo run --release --features profile   # Tracy spans for every system and the chunk build tasks
CANVAS_REF=name cargo test canvas_reference -- --ignored   # a fixed world through the software canvas, debug/canvas_ref_name.png, for pixel comparisons
cargo run -- --snapshot NAME     # the same script, then a software picture in debug/NAME.png
                                 #   options: --scale N  --rot 0..3  --angle DEGREES  --tilt NAME  --perspective  --time 0..1  --grid
cargo build --release
```

## How it works

- **Camera.** A real 3D camera (`src/cam.rs`) over Bevy's 3D pipeline: orthographic by
  default, perspective on a toggle, any yaw and pitch, with presets at the classic dimetric
  angles (30 degrees of pitch gives 2:1 ground diamonds). Chunk meshes are in world space;
  the depth buffer sorts blocks and the player per pixel, and turning the view rebuilds
  nothing. The same matrices run on the CPU for picking, streaming and headless snapshots.
- **World.** Chunks of 16 x 16 columns by 32 levels of block ids. Each block is a pure
  function of the seed and its position, so chunks generate in any order and a save stores
  only chunks that were edited. Only chunks that can appear on screen are loaded, and
  generation and meshing share a time budget per frame.
- **Meshing.** One mesh per chunk with every face that touches air. Colour comes from a
  tile atlas; the vertex colour carries light.
- **Art.** Each block's two 16 x 16 tiles are generated from its colours by a pattern
  (`src/world/tiles.rs`), or read from PNGs a pack ships.
- **Light.** The sun is fixed in the world. Face angle to it, ambient occlusion in corners
  and cast shadows are computed per vertex; `assets/shaders/block.wgsl` evaluates them per texel and bands them
  so shading stays pixel art. Shade is cool, sun is warm.
- **Physics.** A box that moves one axis at a time, falls, lands and jumps one block high.
- **Scripts.** Lua 5.4 through `mlua`, sandboxed to `table`, `string`, `math`, `utf8`. The
  engine's functions act on the world directly, so a script reads a block and sees the answer
  on the next line.

## Layout

- `src/core/` schemas (`defs.rs`), id registries, pack loader, settings, save file
- `src/world/` `iso.rs` projection and picking, block storage, `noise.rs`, `worldgen.rs`,
  `stamp.rs`, `tiles.rs` pixel-art tiles, `mesh.rs` faces and light, `stream.rs`, `physics.rs`
- `assets/shaders/block.wgsl` the block shader
- `src/player.rs`, `src/interact.rs`, `src/inventory.rs`, `src/look.rs` where the camera
  looks, `src/camera.rs`, `src/ui.rs`
- `src/script.rs` Lua hooks and API
- `src/debug.rs` screenshots, software snapshots, text dumps; `src/smoke.rs` the self-test;
  `src/harness.rs` injected input and fakes for Lua tests
- `assets/packs/base/` the base pack: `blocks.ron`, `worldgen.ron`, `stamps.ron`, `features.ron`
- `mods/` packs loaded after the base; see `mods/README.md`

## Releases

Tagged versions build for macOS (Apple Silicon), Linux and Windows on GitHub Actions; the zips
are on the Releases page. Unzip and run `fieldstone` from the folder; it finds `assets/` and
`mods/` beside itself.

## Roadmap

The engine stays small. Each of these returns as a mod, with at most one small generic
capability added to the engine and exposed to Lua: crafting, day and night, health and hunger,
creatures and combat, liquid and see-through blocks, a cutaway view for caves, sound, light
sources that light their surroundings.
