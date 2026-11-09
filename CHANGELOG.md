# Changelog

## 0.7.0

A rebuild. The top-down survival game is retired to tag `v0.6.1`; this release is a small
isometric block engine meant to be extended by mods. Saves from 0.6 are ignored (they stay on
disk untouched).

### New
- Isometric view of a world made of blocks on an x, y, z grid, 32 levels tall. Dig down,
  build up. Four view rotations (Q / E).
- Exact occlusion from a depth buffer: every vertex carries `x + y + z`, so blocks and the
  player sort per pixel with no sorting code. A faint overlay keeps the player findable
  behind terrain.
- Flat-shaded art generated from one colour per block, with outlines on steps and material
  changes (G shows the full grid).
- Terrain from a height field with surface bands and a sea; trees, boulders and ore veins
  from `stamps.ron` and `features.ron`, which mods extend record by record.
- Physics with gravity and automatic one-block steps. Water cannot be crossed until bridged.
- Break, place, hotbar, bag with stack splitting, pause menu, save file with only edited
  chunks.
- Lua through `mlua` with a synchronous, sandboxed API: scripts read and write blocks and the
  inventory directly.
- Debug probe: F9 saves the window, a software-drawn copy of the view that works even when
  the window is hidden, and a text dump. `--snapshot` draws the world with no window at all.

### Removed
- Combat, creatures, hunger, crafting, farming, fishing, chests, the shrine quest, day and
  night, sound, the vector and pixel top-down renderers, shaders, shape recipes. They return
  as mods.
- Dependencies `noise` and `bevy_mod_scripting`; Bevy is built with 2D, UI and input
  features only.

## 0.6.1

Depth and building. The world reads as a 3/4 view with terraced land, and building gains
multi-tile pieces, connected walls, shelter and stack handling.

### Added
- Terraced elevation: land has heights 0 to 3 with generated cliff faces; a step is blocked
  unless a ramp bridges it. Natural ramps at cliff feet, craftable `ramp_wood`. Saves keep
  heights.
- Multi-tile objects (`size` in objects.ron): oak 2x2, bed 1x2, stone table 2x1. Placing
  checks the whole footprint; breaking any part breaks the whole.
- Connected walls (`connects` group): wood and stone walls join into runs, fences join and
  let arrows over (`low`).
- Shelter: an enclosed space counts as indoors, nothing spawns inside it, and a hint
  explains it the first night you are under a roof.
- Inventory handling: shift-click moves half a stack, right-click moves one, splitting
  into an empty slot, a Sort button on chests, hover tooltips for every item kind, and
  clicking a hotbar slot selects it.
- `structures.ron`: hand-drawn stamps (object, terrain, clear) placed by scatter rules with a
  `structure` field; storage objects in a stamp are stocked from a loot table on first
  generation. The base pack ships a `campsite` on meadow. New `stump` object.
- Creatures with four facing frames turn with their movement.

### Changed
- Sprites stand on their cell (feet anchors) and sort by screen y, so the player walks behind
  trees and in front of walls. Soft shadows survive pixelization. Standing art is taller.
- Higher terraces draw slightly brighter.

### Fixed
- The crystal example mod's worldgen override had fallen behind the base list (no oaks); a
  worldgen override replaces the whole record, so the file now mirrors base.

## 0.6.0

The first step of the pixel-art train. The game now renders as 16 px sprites and tiles by
default; the vector renderer stays available for one more release.

### Added
- Pixel renderer: `settings.ron` `render: Pixel | Vector` and `pixel_scale` (whole-number
  zoom, pinch or `-`/`=`); the camera snaps to the texel grid.
- `sprites.ron`: per-visual PNG strips with frames, fps and anchor; anything without art is
  rasterized from its shape at 16 px per cell with a one-texel outline. The player uses four
  generated facing frames.
- Dual-grid terrain tile sheets generated per terrain (`tiles/<terrain>.png` overrides),
  animated water frames.
- Hunger: a food bar that drains over the day; low food slows stamina and stops healing,
  starvation hurts. Food values on every edible; a campfire station cooks meat and fish.
- Creatures: deer (skittish) and bats (night, hills).

### Changed
- One cell is 16 world units (was 25). Saves record their cell size and rescale on load.
- Creature speeds rescaled to the new grid.

## 0.5.0

First public release: survival loop, combat and RPG stats, farming, fishing, Lua scripting
and example mods, shrine quest, animated water, procedural sound, pause menu and settings,
chests, bed and doors, release builds for macOS, Linux and Windows.
