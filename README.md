# Fieldstone

A small, data-driven, moddable top-down survival game in Rust and Bevy. Wander a procedurally
generated field, chop trees, mine ore, craft tools at a workbench, smelt at a furnace, and wall
off a base. Everything you see is a vector shape; everything you interact with is defined in RON.

## Controls

| Input | Action |
|-------|--------|
| WASD / arrow keys | Move |
| Shift | Sprint |
| Left click (hold) | Mine the object under the cursor with the held tool |
| Right click | Place the held block or station |
| 1-0, mouse wheel | Select hotbar slot |
| E | Inventory (click a slot to swap it with the selected hotbar slot) |
| C | Crafting menu (click a lit recipe to craft) |
| F5 | Save to `saves/world.ron` (loaded automatically next start) |
| F3 | Debug readout |

## Run

```sh
cargo run                                   # first build takes a few minutes (Bevy)
cargo run --features bevy/dynamic_linking   # faster rebuilds while iterating
cargo test                                  # registry, chunk math, collision, inventory tests
cargo run -- --headless-smoke               # 600 frames without a window, exits 0 if nothing panics
```

## Layout

- `src/core/` — content schemas (`defs.rs`), id registries, pack loader, clock, save/load
- `src/world/` — chunked tile storage, noise world generation, streaming and meshes, collision
- `src/art.rs` — shape recipes to `Mesh2d` entities; the only rendering code
- `src/player.rs` — input, movement, camera follow, cursor cell
- `src/items/` — inventory, drops, mining and placing, crafting
- `src/ui/` — hotbar, inventory panel, crafting panel, debug text
- `assets/packs/base/` — the base content pack: palette, shapes, terrain, objects, items, recipes, loot, worldgen
- `mods/` — drop additional packs here; see `mods/README.md`

## Content and mods

The engine knows about *kinds* of things (terrain, objects, items, recipes, loot tables, shapes,
colors, the generator config) and nothing concrete. A pack is a folder of RON files, one per kind;
`assets/packs/base` is just the first pack. Later packs override earlier records with the same id.
Schemas live in `src/core/defs.rs`.

## Roadmap

1. Survival loop: world, mining, inventory, crafting, building, saving (done)
2. Combat and RPG stats: health, stamina, weapons, creatures, experience, day/night
3. Farming, fishing, Lua scripting hooks, example mod
4. Polish: animation, particles, lighting, audio, menus
