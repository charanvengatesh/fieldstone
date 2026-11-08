# Fieldstone

A small, data-driven, moddable top-down survival game in Rust and Bevy. Wander a procedurally
generated field, chop trees, mine ore, craft tools at a workbench, smelt at a furnace, and wall
off a base. Everything you see is a vector shape; everything you interact with is defined in RON.

## Controls

Built for a trackpad first: nothing needs holding or a right button.

| Input | Action |
|-------|--------|
| WASD / arrow keys | Move |
| Shift | Sprint (uses stamina) |
| Click an object | Target it: the player walks within reach and works it until it breaks. Click empty ground or press Esc to stop. Holding the button also works |
| Click with a weapon | Swing or shoot toward the cursor. Switch to a tool or empty slot to mine |
| Q | Drop the held stack |
| Click water with a rod | Cast; click again on the bite |
| Click grass with a hoe | Till it |
| F or right click | Use the held item (place, plant, eat), or use the object under the cursor: open a chest, open a door, sleep in a bed, feed the shrine |
| Esc | Pause menu: resume, save, save and quit |
| 1-0, two-finger scroll | Select hotbar slot |
| Pinch, or - / = | Zoom |
| E | Inventory (click a slot to swap it with the selected hotbar slot) |
| C | Crafting menu (click a lit recipe to craft) |
| F5 | Save to `saves/world.ron` (loaded automatically next start) |
| F3 | Debug readout |

Every key is rebindable in `settings.ron`, written next to the executable on first run.

## Goal

Survive, build, and end the night: craft a Sun Shrine (stone and gold at a workbench), forge
three Sun Cores (gold, iron and coal at a furnace), and feed them to the shrine. The quest is a
Lua script in the base pack, so a mod can replace it.

## Run

```sh
cargo run                                   # first build takes a few minutes (Bevy)
cargo run --features bevy/dynamic_linking   # faster rebuilds while iterating
cargo test                                  # registry, chunk math, collision, inventory tests
cargo run -- --headless-smoke               # no window: scripted input mines an object and places a wall, exits non-zero on failure
cargo run -- --new                          # ignore the save file and start a fresh world
cargo run -- --seed 42                      # fix the world seed
cargo build --release                       # optimized binary in target/release/
```

## Layout

- `src/core/` — content schemas (`defs.rs`), id registries, pack loader, clock, save/load
- `src/world/` — chunked tile storage, noise world generation, streaming and meshes, collision
- `src/art.rs` — shape recipes to `Mesh2d` entities; the only rendering code
- `src/player.rs` — input, movement, camera follow, cursor cell
- `src/items/` — inventory, drops, mining and placing, crafting
- `src/stats.rs` — health, stamina, damage, death, experience and skills
- `src/combat/` — weapons (melee arcs, projectiles), creature AI, natural spawning
- `src/daylight.rs` — day/night tint
- `src/farm.rs` — tilling and staged crop growth
- `src/fish.rs` — casting, bites, fish tables
- `src/script.rs` — Lua hooks and script-callable functions (`bevy_mod_scripting`)
- `src/fx.rs` — slashes, break particles, glints, night glows around lights
- `src/water.rs` + `assets/shaders/water.wgsl` — animated water overlay
- `src/sound.rs` — procedural sound effects
- `src/hints.rs` — one-shot guidance from `hints.ron`
- `src/ui/` — hotbar, inventory panel, crafting panel, debug text
- `assets/packs/base/` — the base content pack: palette, shapes, terrain, objects, items, recipes, loot, creatures, fish, hints, worldgen, scripts
- `mods/` — additional packs; `example-crystal` (new ore, items, Lua) and `example-autumn` (palette-only recolor); see `mods/README.md`

## Content and mods

The engine knows about *kinds* of things (terrain, objects, items, recipes, loot tables, shapes,
colors, the generator config) and nothing concrete. A pack is a folder of RON files, one per kind;
`assets/packs/base` is just the first pack. Later packs override earlier records with the same id.
Schemas live in `src/core/defs.rs`.

## Releases

Tagging `v*` builds macOS, Linux and Windows zips through GitHub Actions and attaches them to a
release. Each zip holds the binary plus `assets/` and `mods/`; run it from inside the folder.

## Roadmap

- v0.5 (this): pause and settings, chests, bed and doors, shrine quest, water shader, sound,
  hints, release builds, pack format version, two example mods
- v0.6: hunger, multi-tile structures, stack splitting, crop timer persistence, more biomes
