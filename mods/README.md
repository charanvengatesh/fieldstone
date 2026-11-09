# Mods

A mod is a folder in here laid out like `assets/packs/base/`. Every file is optional.

```
mods/my-mod/
  pack.ron        (name: "my-mod", format: 2, enabled: true, depends: ["base"], scripts: [])
  blocks.ron      new or changed cubes
  items.ron       things that are not blocks (every block already gets its own item)
  stamps.ron      small 3D shapes: trees, boulders, huts
  features.ron    where stamps stand and where ore replaces stone
  worldgen.ron    the terrain itself: heights, sea level, surface bands
  scripts/*.lua   behaviour
```

Packs load in dependency order (`assets/packs/*`, then `mods/*`). A record whose `id` already
exists replaces the earlier one; a new `id` is added. So a mod retunes a base block by
redefining it and adds a tree by adding one `features.ron` record, without copying anything.
Schemas are documented in `src/core/defs.rs`. Set `enabled: false` to keep a mod on disk
without loading it.

Saves name blocks by id. Removing a mod turns its blocks to air and drops its items on the
next load, with a warning in the log; nothing else is lost.

## Blocks

```ron
(id: "crystal", name: "Crystal", color: (170, 126, 236), top: Some((208, 182, 255)),
    pattern: Bricks, accent: Some((232, 214, 255)), glow: 0.85, hp: 0.8)
```

A block's art is two 16 x 16 tiles, one for the top and one for the sides. The engine draws
them from `color`, `top` (top face, when it differs) and `accent` (a second colour) using
`pattern`:

| Pattern | Looks like |
|---------|-----------|
| `Grain` (default) | loose speckle: dirt, gravel |
| `Smooth` | one flat colour |
| `Stone` | quiet blotches and flecks |
| `Sand` | fine speckle with ripples |
| `Snow` | near white with sparse cool shade |
| `Grass` | tufted `top`; sides are `color` soil with a ragged lip of `top` |
| `Leaves` | clumps with dark gaps |
| `Log` | bark on the sides, rings in `top` on the ends |
| `Planks` | boards with seams |
| `Bricks` | courses with `accent` mortar |
| `Ore` | stone with nuggets of `accent` |
| `Water` | wave dashes; the shader makes them move |

To draw your own instead, put PNGs in the pack: `textures/<id>_top.png` and
`textures/<id>_side.png`, or one `textures/<id>.png` for both. Each is 16 x 16, drawn the
usual way up.

Other fields: `glow` (0..1, the least brightness the block ever has, so it stays lit in
shadow), `solid`, `walkable` (false for water: nothing stands on or over it until something
is built there), `hp` (seconds to break), `breakable`, `drops` (an item id; the block itself
when absent), `item` (false to make no item). Blocks with the same `color` and `top` are one
family and get no outline between them, which is how ore sits cleanly in stone.

## Stamps and features

A stamp is layers from the bottom up; each layer is rows read like a map, all the same size.
A space leaves the world alone. The middle column stands on the chosen cell.

```ron
(id: "tree",
    layers: [
        ["   ", " T ", "   "],
        ["lll", "lTl", "lll"],
    ],
    legend: { 'T': "log", 'l': "leaves" })
```

Features place things:

```ron
(id: "forest", rule: Surface(stamp: "tree", on: ["grass"], density: 0.16, noise: Some((28.0, 0.56))))
(id: "coal",   rule: Replace(block: "stone", with: "coal_ore", density: 0.8, noise: Some((5.0, 0.70)), z: (1, 20)))
```

`density` is the chance per eligible cell. `noise: Some((scale, threshold))` keeps a rule to
patches (groves, veins); leave it out for an even scatter. Generation is a pure function of
the seed, so the same seed with the same packs always gives the same world.

## Scripts

List Lua files in `pack.ron`. Each script has its own globals, so two mods can both define
`on_tick`. Define any of these and the engine calls them:

| Hook | When |
|------|------|
| `on_script_loaded()` | once, after the file ran |
| `on_tick(dt)` | every frame, `dt` in seconds |
| `on_block_broken(x, y, z, block_id)` | a block was broken by the player |
| `on_block_placed(x, y, z, block_id)` | a block was placed by the player |

Functions you can call. They act immediately: a `set_block` is visible to the next
`get_block`.

| Function | Does |
|----------|------|
| `log(text)` | writes to the game log |
| `show_text(text, seconds)` | a line at the top of the screen |
| `get_block(x, y, z)` | the block id there, `"air"` when empty or not loaded |
| `set_block(x, y, z, block_id)` | writes a block; `true` when something changed |
| `give_item(item_id, n)` | adds to the player's bag; returns how many did not fit |
| `take_item(item_id, n)` | removes exactly `n` or nothing; `true` on success |
| `count_item(item_id)` | how many the player holds |
| `sort_inventory()` | merges and orders the bag (not the hotbar) |
| `player_pos()` | `x, y, z` of the player's feet |
| `key_just_pressed(name)` | `true` the frame a key goes down; names as in `settings.ron` |
| `screenshot(name)`, `snapshot(name)`, `dump_state(name)`, `append_file(name, line)` | debug output under `debug/` |

Scripts run in a sandbox: `table`, `string`, `math` and `utf8` only. There is no `io`, `os` or
`require`. Scripts load once at startup; there is no hot reload yet.

## The two mods in this folder

- `example-crystal/` is a complete small mod to copy: two blocks (an ore and a glowing
  crystal), one feature rule, one script.
- `debug/` is a probe: F9 saves a window screenshot, a software snapshot and a text dump for
  bug reports. See its README.
