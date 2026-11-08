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
| Click with a weapon | Swing or shoot toward the cursor |
| Click water with a rod | Cast; click again on the bite |
| Click grass with a hoe | Till it |
| F or right click | Use the held item: place a block or station, plant seeds, eat |
| 1-0, two-finger scroll | Select hotbar slot |
| Pinch, or - / = | Zoom |
| E | Inventory (click a slot to swap it with the selected hotbar slot) |
| C | Crafting menu (click a lit recipe to craft) |
| F5 | Save to `saves/world.ron` (loaded automatically next start) |
| F3 | Debug readout |

## Run

```sh
cargo run                                   # first build takes a few minutes (Bevy)
cargo run --features bevy/dynamic_linking   # faster rebuilds while iterating
cargo test                                  # registry, chunk math, collision, inventory tests
cargo run -- --headless-smoke               # no window: scripted input mines an object and places a wall, exits non-zero on failure
cargo run -- --new                          # ignore the save file and start a fresh world
cargo run -- --seed 42                      # fix the world seed
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
- `src/fx.rs` — slashes, break particles, night glows around lights
- `src/ui/` — hotbar, inventory panel, crafting panel, debug text
- `assets/packs/base/` — the base content pack: palette, shapes, terrain, objects, items, recipes, loot, creatures, fish, worldgen
- `mods/` — drop additional packs here; see `mods/README.md`

## Content and mods

The engine knows about *kinds* of things (terrain, objects, items, recipes, loot tables, shapes,
colors, the generator config) and nothing concrete. A pack is a folder of RON files, one per kind;
`assets/packs/base` is just the first pack. Later packs override earlier records with the same id.
Schemas live in `src/core/defs.rs`.

## Roadmap

1. Survival loop: world, mining, inventory, crafting, building, saving (done)
2. Combat and RPG stats: health, stamina, weapons, creatures, experience, day/night (done)
3. Farming, fishing, Lua scripting hooks, example mod (done; see `mods/example-crystal`)
4. Polish: slash and particle effects, torch glows, title card, CLI flags (done); audio and
   rebindable keys are still open
