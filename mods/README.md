# Mods

A mod is a folder in here with the same layout as `assets/packs/base/`. Every file is optional.

```
mods/my-mod/
  pack.ron        (name: "my-mod", format: 1, enabled: true, depends: ["base"], scripts: [])
  palette.ron     new or overridden colors
  shapes.ron      shape recipes (also the source of generated pixel art)
  sprites.ron     PNG strips per visual id: frames, fps, anchor
  tiles/<terrain>.png   4x4 sheet of 16 px tiles per terrain (and _0.._3 frames for water)
  terrain.ron     ground types
  objects.ron     trees, ores, walls, stations
  items.ron
  recipes.ron
  loot.ron
  creatures.ron   mobs and the player record
  fish.ron        what bites in each water type
  hints.ron       one-shot guidance lines
  worldgen.ron    replaces the generator config if present
```

Packs load in dependency order (`assets/packs/*` first, then `mods/*`). A record whose `id`
already exists replaces the earlier one, so a mod can retune a base item by redefining it.
Schemas are documented in `src/core/defs.rs`.

## Scripts

`scripts` in `pack.ron` lists Lua files (paths relative to the pack). Define any of these
functions and the engine calls them:

| Hook | Arguments |
|------|-----------|
| `on_script_loaded()` | |
| `on_object_broken(x, y, object_id)` | cell and the object that broke |
| `on_object_placed(x, y, object_id)` | |
| `on_craft(recipe_id)` | |
| `on_death(creature_id, x, y)` | `"player"` when the player dies |
| `on_day_start(day)` | fires at each dawn after the first |
| `on_object_used(x, y, from_id, to_id)` | an object with `use_swaps` was used (doors, shrine stages) |
| `on_tick(dt)` | every frame, `dt` in seconds |

Pixel art: a sprite PNG is a horizontal strip of equal frames; a tile sheet is a 4x4 grid where
tile index = corner mask (bit 0 bottom-left, 1 bottom-right, 2 top-left, 3 top-right). Anything
you do not provide is generated from the shape recipe, so a mod can start with shapes and swap
in PNGs one at a time.

Coordinates are cell indices (one cell is 16 world units). Scripts load once at startup; there is no
hot reload yet. `give_item` drops the items at the player's feet rather than inserting them.

Functions scripts can call:

| Function | Effect |
|----------|--------|
| `log(message)` | writes to the game log |
| `give_item(item_id, count)` | drops items at the player's feet |
| `spawn_creature(creature_id, x, y)` | spawns a creature on a cell |
| `is_night()` | `true` during the dark half of the day |
| `show_text(message, seconds)` | toast line at the top of the screen |
| `win(message)` | ends the game: eternal day and an ending card |
| `set_day_frac(f)` | moves the clock within the current day (0 dawn, 0.5 dusk) |
| `player_x()`, `player_y()` | the player's cell |
| `day_frac()` | 0 dawn .. 0.5 dusk .. 1 |
| `key_just_pressed(name)` | `true` the frame a key goes down; names as in `settings.ron` (`"F9"`, `"K"`) |
| `screenshot(name)` | saves `debug/<name>.png` |
| `dump_state(name)` | saves `debug/<name>.txt`: clocks, player, inventory, creatures, chests, ASCII maps |
| `append_file(name, line)` | appends a line to `debug/<name>` |

`format` is the pack format the mod was written for; the engine warns when a mod is newer than it
understands. Objects can declare `storage` (chest slots), `use_swaps` / `use_consumes` (doors,
multi-stage builds), `bed`, `radius` (hitbox), `on` (terrain restriction), `light`, `grows_into`.

See `mods/example-autumn/` for a data-only mod (eight palette overrides, nothing else) and
`mods/example-crystal/` for a complete mod: a new ore in the world generator, items,
recipes, a light source, and a script that reacts to mining and placing. `mods/debug/` is a
probe: F9 saves a screenshot and a world-state dump under `debug/` for bug reports.
