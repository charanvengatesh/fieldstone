# debug

A probe, not content. Leave it enabled while testing; set `enabled: false` in `pack.ron` for
a clean game.

| Key | Does |
|-----|------|
| F9  | screenshot + world-state dump |
| F10 | toggle automatic snapshots every 5 s (40 per session) |
| F11 | dump only |

Files land in `debug/` next to `settings.ron` (the game folder when writable, otherwise the
per-user config directory). `events.log` records every hook the engine fires with the day,
time and player cell. A dump holds the clocks, render settings, player position and stats,
inventory, target and cursor, nearby creatures with their AI state, nearby chests, and three
ASCII maps (objects and terrain, heights, legends).

When something looks wrong: press F9 while looking at it, then hand over the matching
`NNN-f9.png` and `NNN-f9.txt` with a line about what you expected.

`FIELDSTONE_DEBUG_AUTO=5 cargo run` starts automatic capture at launch, for runs nobody is
watching; so does a file `debug/auto` holding the seconds. Screenshots read the window, so the
game must be on screen and active: on macOS a binary started from a background shell never
gets a presentable frame and saves black images. `open target/debug/fieldstone` (after
`cargo build`) launches it activated. Dumps and the event log work either way.
