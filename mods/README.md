# Mods

A mod is a folder in here with the same layout as `assets/packs/base/`. Every file is optional.

```
mods/my-mod/
  pack.ron        (name: "my-mod", depends: ["base"], scripts: [])
  palette.ron     new or overridden colors
  shapes.ron      vector art recipes
  terrain.ron     ground types
  objects.ron     trees, ores, walls, stations
  items.ron
  recipes.ron
  loot.ron
  creatures.ron  mobs and the player record
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

Functions scripts can call:

| Function | Effect |
|----------|--------|
| `log(message)` | writes to the game log |
| `give_item(item_id, count)` | drops items at the player's feet |
| `spawn_creature(creature_id, x, y)` | spawns a creature on a cell |
| `is_night()` | `true` during the dark half of the day |

See `mods/example-crystal/` for a complete mod: a new ore in the world generator, items,
recipes, a light source, and a script that reacts to mining and placing.
