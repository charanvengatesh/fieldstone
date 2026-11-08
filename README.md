# Fieldstone

Steer a stone across a green field and lay walls on the grid. A Rust/Bevy rewrite of the
original p5.js sketch (still in git history).

## Controls

| Input | Action |
|-------|--------|
| WASD / arrow keys | Move |
| Shift | Sprint |
| Left click | Lay a stone under the cursor |
| Right click | Remove the stone under the cursor |

The top-left readout shows the grid cell under the cursor.

## Run

```sh
cargo run                                   # first build takes a few minutes (Bevy)
cargo run --features bevy/dynamic_linking   # faster rebuilds while iterating
cargo test                                  # grid and collision unit tests
```

## Layout

- `src/main.rs` — app setup and system order
- `src/world.rs` — grid geometry, stones, border, cursor, collision (`slide`)
- `src/player.rs` — input, movement, camera follow
- `src/hud.rs` — coordinate readout
