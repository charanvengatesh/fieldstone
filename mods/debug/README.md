# debug

A probe, not content. Leave it enabled while testing; set `enabled: false` in `pack.ron` for
a clean game.

| Key | Does |
|-----|------|
| F9  | window screenshot + software snapshot + world-state dump |
| F10 | toggle automatic capture every 5 s (40 per session) |
| F11 | dump only |

Files land in `debug/` next to `settings.ron` (the game folder when writable, otherwise the
per-user config directory). `events.log` records every hook the engine fires with the time
and the player's cell. A dump holds the seed, view, chunk counts, player position, inventory,
target, and two maps around the player: the surface block and its height.

Two kinds of picture are saved. `NNN-f9.png` is the real window; it is black when the window
is hidden or the app is in the background. `NNN-f9-soft.png` is the same view drawn on the
CPU from the same mesh data; it always works, but shows no interface.

When something looks wrong: press F9 while looking at it, then hand over the number with a
line about what you expected.

For a run nobody is watching, `FIELDSTONE_DEBUG_AUTO=5 cargo run` (or a file `debug/auto`
holding the seconds) captures from launch without any script. Without a window at all,
`cargo run -- --snapshot NAME [--seed N] [--scale N] [--rot N] [--grid]` runs the scripted
self-test and writes `debug/NAME.png`.

## Tests

`tests/*.lua` in this folder are the engine's own scenarios, run headlessly by
`cargo run -- --test` (and by CI): injected keys and scrolling, the camera mod's orbit,
twist, reset and zoom, the performance mod's overlay, hand levels and adaptive behaviour on a
pretend slow machine, breaking and placing, streaming after a teleport, walls and steps. The
functions they use are listed in `mods/README.md` under Testing a mod.
