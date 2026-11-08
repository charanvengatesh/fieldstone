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

Scripting hooks (Lua) arrive in a later milestone; `scripts` in `pack.ron` is reserved for them.
