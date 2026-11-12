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
| `Glass` | a rim and two glints around a hole (use with `cutout: true`) |
| `Tuft`, `Flower`, `Mushroom`, `Pebbles`, `Torch` | upright sprites for `shape: Cross` props: blades in `color`; a stem with a bloom in `accent`; a stalk in `color` under a cap in `top` with `accent` spots; stones in `color`; a stick in `color` with a flame in `top` |

To draw your own instead, put PNGs in the pack: `textures/<id>_top.png` and
`textures/<id>_side.png`, or one `textures/<id>.png` for both. Each is 16 x 16, drawn the
usual way up.

Other fields: `shape: Cross` (two crossed upright quads drawn from the side tile instead
of a cube: plants, torches; never blocks the way, breaks in a touch, lets light through),
`cutout: true` (texels with alpha under one half are holes, as `Leaves` and `Glass` draw
them; the faces behind the block are drawn and light passes), `light` (1 to 15: the block
is a light source and lights the air around it, one level less per block, through anything
that is not a solid cube), `glow` (0..1, the least brightness the block's own texels ever
have, so a lamp looks lit in shadow), `solid`, `walkable` (false for water and lava: nothing stands on or over them until
something is built there; water is drawn see-through, blended over its bed), `hp` (seconds to break), `breakable`, `drops` (an item id; the block itself
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
| `key_just_pressed(name)`, `key_pressed(name)` | the frame a key goes down; while it is held. Names as in `settings.ron` |
| `mouse_pressed("left" / "right" / "middle")` | while a mouse button is held |
| `mouse_delta()` | `dx, dy` the pointer moved this frame, in screen pixels |
| `scroll_delta()` | lines scrolled this frame |
| `rotate_gesture()` | radians of trackpad twist this frame |
| `capture_pointer(on)` | while on, the pointer breaks and places nothing (for drags of your own) |
| `window_focused()` | `false` while the game is in the background, where it idles at ten ticks a second |
| `view_angle()`, `view_target()` | degrees the view is at, and is heading to |
| `turn_view(degrees)` | turn smoothly from wherever the view is heading |
| `set_view_angle(degrees)` | put the view at an angle at once |
| `snap_view(step)` | head for the nearest multiple of `step` degrees (default 90) |
| `zoom()`, `set_zoom(n)`, `zoom_at(n, x, y)` | screen pixels per texel, 1 to 6; `zoom_at` keeps the point `x, y` texels from the middle still |
| `tilt()`, `set_tilt(name)` | the pitch preset the camera is nearest to or heading for: `low`, `classic`, `high`, `top` |
| `pitch()`, `set_pitch(degrees)` | the camera's angle above the ground, 5 to 89 degrees |
| `time_of_day()`, `set_time_of_day(f)` | the day clock as a fraction: 0 midnight, 0.25 dawn, 0.5 noon, 0.75 dusk; a day is 12 minutes |
| `light_at(x, y, z)` | block light at a cell, 0 to 15 |
| `pan_view(dx, dy)`, `camera_pan()`, `camera_center()` | hold a pan in texels (call every frame), read the pan, read the world point the camera looks at (`x, y, z`) |
| `get_setting(name)`, `set_setting(name, value)` | `shadows`, `occlusion`, `grid`, `see_through` (the silhouette through blocks), `peek`, `dead_zone`, `perspective` (booleans), `mesh_budget_ms` (main-thread milliseconds a frame for taking finished chunks in), `builds` (chunks generating or meshing on the compute pool at once, 1 to 16), `view_margin`, `horizon` (blocks behind the focus where the fog is whole and loading stops), `spike_ms` (the frame-work line `perf().spikes` counts against, default 12; numbers) |
| `perf()` | a table: `fps`, `frame_ms`, `worst_ms` (the longest recent frame), `work_ms` (the main thread's own work this frame, without the wait for the display), `spikes` (frames whose work went past `spike_ms`), `spike_ms`, `chunks`, `faces`, `mesh_bytes`, `stream_ms` (choosing what to load), `mesh_ms` (main-thread time taking finished chunks in), `build_ms` (compute-pool time of the chunks that landed this frame), `uploads`, `inflight`, `meshed` |
| `perf_history()` | the last 240 frames' `work_ms`, oldest first |
| `perf_reset()` | forget the spikes and the peak; a measurement starts now |
| `show_perf_graph(on)` | the frame-time graph under the HUD lines (240 columns of `work_ms`, lines at 8.3 and 16.7 ms) |
| `set_hud(key, text)` | keep a line on screen, top right; empty text removes it |
| `screenshot(name)`, `snapshot(name)`, `dump_state(name)`, `append_file(name, line)` | debug output under `debug/` |

Scripts run in a sandbox: `table`, `string`, `math` and `utf8` only. There is no `io`, `os` or
`require`. Scripts load once at startup; there is no hot reload yet.

## Testing a mod

Put `tests/*.lua` in a pack and run `cargo run -- --test`. Test files are scripts like any
other (they may define hooks), plus:

```lua
test("Q turns a quarter", function()
    local start = view_target()
    press_key("Q") wait(1) release_key("Q")
    expect(near(view_target(), start + 90, 0.01), "Q asks for a quarter turn")
    expect(wait_until(function() return not view_turning() end, 120), "the turn finishes")
end)
```

| Function | Does |
|----------|------|
| `test(name, fn)` | registers a test; `fn` runs as a coroutine, one test at a time, nothing held between tests |
| `wait(frames)`, `wait_until(check, frames)` | pause the test; `wait_until` returns whether `check` came true |
| `expect(ok, message)` | records a failure; the run exits non-zero if any |
| `near(a, b, tolerance)` | numeric closeness |
| `press_key`, `release_key`, `press_mouse`, `release_mouse` | held from the next frame; names as in `settings.ron`, buttons `"left"`, `"right"`, `"middle"` |
| `move_mouse(dx, dy)`, `scroll(lines)`, `twist(radians)` | pointer motion, wheel, trackpad rotation |
| `aim_at(x, y, z)`, `aim_screen(x, y)`, `clear_aim()` | point at a world position, or at a screen offset from the middle in texels |
| `screen_of(x, y, z)` | where a world position lands on screen, as an offset from the middle in texels |
| `teleport(x, y, z)` | move the player's feet |
| `set_time_scale(s)` | run the game clock faster (hooks see a bigger `dt`) |
| `simulate_frame_ms(ms)` | make `perf()` read as on a machine with that frame time; `nil` stops |
| `frame()`, `view_turning()`, `hud_text(key)`, `selected_slot()`, `is_grounded()`, `target()` | readouts; `target()` is a table `x, y, z, block, nx, ny, nz` or `nil` |
| `particles()`, `player_heading()` | live particles; which way the figure is turned: `left`, `right`, `away`, `toward` |
| `shader_passes()`, `set_shader(id, on)`, `set_shader_param(id, i, value)` | the picture's passes (`id`, `order`, `enabled`); turn one on or off; set one of its sixteen numbers (`i` 1 to 16). Empty headless |

Injected input is indistinguishable from a player's to the engine and to other scripts,
so a test can drive another mod's controls. `mods/debug/tests/` covers the engine, the
camera mod and the performance mod; copy one to start.

## The mods in this folder

- `camera/` adds free orbit to the engine's quarter turns: Alt-drag, middle-drag or a
  trackpad twist, settling on the nearest quarter view; Space-drag pans; R resets; Ctrl or
  Cmd + scroll zooms; C toggles a compass that also names the tilt. Its constants (drag speed, snap step) are at the top of the
  script.
- `performance/` shows the numbers (F4) and adapts quality to the frame rate, or lets you
  set it (F6). Its levels and thresholds are a table at the top of the script.
- `shaders/` holds optional finishing passes, off by default: an ink outline and a CRT
  look, `F7` to cycle. Its README shows how to write a pass: a WGSL fragment shader listed
  in `shaders.ron` with an order and sixteen numbers, run over the picture in turn.
- `example-crystal/` is a complete small mod to copy: two blocks (an ore and a glowing
  crystal), one feature rule, one script.
- `debug/` is a probe: F9 saves a window screenshot, a software snapshot and a text dump for
  bug reports. See its README.
