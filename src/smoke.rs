//! `--headless-smoke`: a scripted run without a window that exits non-zero when something
//! basic is broken. `--snapshot NAME` runs the same script and then draws what the camera
//! would see to `debug/NAME.png` with the software canvas.

use bevy::prelude::*;

use crate::core::{BlockDef, ItemDef, Registry};
use crate::interact::{Cursor, within_reach};
use crate::inventory::Inventory;
use crate::player::{Player, closed};
use crate::world::iso::View;
use crate::world::physics::fits;
use crate::world::worldgen::WorldGen;
use crate::world::{AIR, HEIGHT, Map};

#[derive(Resource)]
pub struct Smoke {
    frame: u32,
    snapshot: Option<String>,
    scale: f32,
    start: Vec3,
    /// The block the script breaks and puts back, and what it was.
    dug: Option<(IVec3, u16)>,
    failed: bool,
    done: bool,
    /// Meshes built and the quarter view before the turn test.
    meshed: u64,
    rot: u8,
    loaded: Vec<(IVec2, u32)>,
}

impl Smoke {
    pub fn new(snapshot: Option<String>, scale: f32) -> Self {
        Self {
            frame: 0,
            snapshot,
            scale,
            start: Vec3::ZERO,
            dug: None,
            failed: false,
            done: false,
            meshed: 0,
            rot: 0,
            loaded: Vec::new(),
        }
    }
}

fn fail(smoke: &mut Smoke, why: String) {
    error!("smoke: {why}");
    smoke.failed = true;
}

pub fn drive(
    mut smoke: ResMut<Smoke>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut cursor: ResMut<Cursor>,
    items: Res<Registry<ItemDef>>,
    map: Res<Map>,
    mut view: ResMut<View>,
    perf: Res<crate::world::stream::Perf>,
    wg: Res<WorldGen>,
    blocks: Res<Registry<BlockDef>>,
    path: Res<crate::core::save::SavePath>,
    mut request: ResMut<crate::core::SaveRequest>,
    mut player: Single<(&Player, &mut Inventory)>,
    (mut broken, mut placed): (
        MessageReader<crate::interact::BlockBroken>,
        MessageReader<crate::interact::BlockPlaced>,
    ),
) {
    let (player, inv) = &mut *player;
    smoke.frame += 1;
    for b in broken.read() {
        info!(
            "smoke: [{}] broken {} at {}",
            smoke.frame,
            blocks.id_of(b.block),
            b.at
        );
    }
    for b in placed.read() {
        info!(
            "smoke: [{}] placed {} at {}",
            smoke.frame,
            blocks.id_of(b.block),
            b.at
        );
    }

    match smoke.frame {
        // The world has streamed in and the player has settled on the ground.
        120 => {
            // Every chunk the camera can see is loaded, meshed and drawn.
            let meshed = map.chunks.values().filter(|c| !c.dirty).count();
            let drawn = map.chunks.values().filter(|c| c.quads > 0).count();
            info!(
                "smoke: {} chunks loaded, {meshed} meshed, {} drawn",
                map.chunks.len(),
                map.entities.len()
            );
            if map.chunks.len() < 6 || meshed != map.chunks.len() || map.entities.len() != drawn {
                fail(
                    &mut smoke,
                    format!(
                        "{} chunks, {meshed} meshed, {} drawn",
                        map.chunks.len(),
                        map.entities.len()
                    ),
                );
            }
            if !player.body.grounded {
                fail(
                    &mut smoke,
                    format!("player never landed: {}", player.body.pos),
                );
            }
            smoke.start = player.body.pos;
            keys.press(KeyCode::KeyD);
        }
        // Two seconds of walking right, then a moment to settle after any drop.
        240 => keys.release(KeyCode::KeyD),
        300 => {
            let moved = (player.body.pos - smoke.start).truncate().length();
            // Standing means the box is free where it is and blocked just below; the
            // centre may overhang a ledge.
            let wall = |c: IVec3| closed(&map, &blocks, c);
            let pos = player.body.pos;
            let on_ground = fits(pos, &wall) && !fits(pos - Vec3::Z * 0.1, &wall);
            info!(
                "smoke: walked {moved:.1} blocks to {}, grounded {}",
                player.body.pos, player.body.grounded
            );
            if moved < 1.0 {
                fail(
                    &mut smoke,
                    format!("player did not move ({moved:.2} blocks)"),
                );
            }
            if !player.body.grounded || !on_ground {
                fail(
                    &mut smoke,
                    format!("player is not standing on ground at {pos}"),
                );
            }
        }
        // Point at the ground a little way off and hold the button until it breaks.
        310 => {
            let feet = player.body.pos.floor().as_ivec3();
            let spot = (-3..=3)
                .flat_map(|dy| (-3..=3).map(move |dx| IVec3::new(dx, dy, 0)))
                .filter(|d| (1..=3).contains(&d.truncate().abs().max_element()))
                .filter_map(|d| {
                    let top = map.top(feet.x + d.x, feet.y + d.y)?;
                    Some(IVec3::new(feet.x + d.x, feet.y + d.y, top))
                })
                .find(|b| {
                    let (left, right) = view.sides();
                    let top = view
                        .project(b.as_vec3() + Vec3::new(0.3, 0.6, 1.0))
                        .truncate();
                    // Where the far wall of the hole will be once the block is gone.
                    let wall = view
                        .project(
                            b.as_vec3() + Vec3::splat(0.5)
                                - right.as_vec3() * 0.5
                                - left.as_vec3() * 0.3
                                + Vec3::Z * 0.1,
                        )
                        .truncate();
                    blocks.at(map.block(*b)).breakable
                        && within_reach(player.body.pos, *b)
                        && within_reach(player.body.pos, *b - right)
                        && view
                            .pick(top, HEIGHT, |c| map.block(c) != AIR)
                            .is_some_and(|h| h.block == *b && h.normal == IVec3::Z)
                        // With the block dug out, its far wall must be in plain view so
                        // it can be clicked to put the block back.
                        && view
                            .pick(wall, HEIGHT, |c| c != *b && map.block(c) != AIR)
                            .is_some_and(|h| h.block == *b - right && h.normal == right)
                });
            match spot {
                Some(b) => {
                    info!("smoke: breaking {} at {b}", blocks.id_of(map.block(b)));
                    smoke.dug = Some((b, map.block(b)));
                    cursor.0 = Some(
                        view.project(b.as_vec3() + Vec3::new(0.3, 0.6, 1.0))
                            .truncate(),
                    );
                    mouse.press(MouseButton::Left);
                }
                // Terrain dependent: on some ground no nearby block has both a clear top
                // and a clear far wall. That is the script's limit, not a fault.
                None => warn!("smoke: no suitable block near the player; dig steps skipped"),
            }
        }
        // Let go the moment it breaks, so the hole stays one block deep.
        311..=519 => {
            if smoke.dug.is_some_and(|(b, _)| map.block(b) == AIR) {
                mouse.release(MouseButton::Left);
            }
        }
        520 => {
            mouse.release(MouseButton::Left);
            if let Some((b, id)) = smoke.dug {
                let def = blocks.at(id);
                let item = items.idx(def.drops.as_deref().unwrap_or(&def.id));
                let got = item.map_or(0, |i| inv.count(i));
                if map.block(b) != AIR || got != 1 {
                    fail(
                        &mut smoke,
                        format!("block at {b} is {}, inventory holds {got}", map.block(b)),
                    );
                } else {
                    info!(
                        "smoke: broke it, holding {got} {}",
                        items.id_of(item.unwrap())
                    );
                }
                // Put it back. The floor of a one-block hole is hidden behind its near
                // walls, so aim at the far wall: the face that looks into the hole.
                let (left, right) = view.sides();
                let centre = b.as_vec3() + Vec3::splat(0.5);
                let wall = centre - right.as_vec3() * 0.5 - left.as_vec3() * 0.3 + Vec3::Z * 0.1;
                let at = view.project(wall).truncate();
                let hit = view.pick(at, HEIGHT, |c| map.block(c) != AIR);
                if hit.map(|h| (h.block, h.normal)) != Some((b - right, right)) {
                    fail(
                        &mut smoke,
                        format!("far wall of the hole not pickable: {hit:?}"),
                    );
                }
                inv.selected = inv.slots.iter().position(|s| s.is_some()).unwrap_or(0);
                cursor.0 = Some(at);
                mouse.press(MouseButton::Right);
            }
        }
        530 => mouse.release(MouseButton::Right),
        540 => {
            if let Some((b, id)) = smoke.dug {
                let def = blocks.at(id);
                let want = blocks.idx(def.drops.as_deref().unwrap_or(&def.id));
                if Some(map.block(b)) != want || inv.slots.iter().any(|s| s.is_some()) {
                    fail(
                        &mut smoke,
                        format!("after placing, {b} holds {}", blocks.id_of(map.block(b))),
                    );
                } else {
                    info!("smoke: placed {} back at {b}", blocks.id_of(map.block(b)));
                }
            }
        }
        // Save, then read the file back and compare it with the live world.
        545 => request.save = true,
        555 => {
            let loaded = crate::core::save::load(&path.0, &blocks, &items);
            match (loaded, smoke.dug) {
                (Some(l), Some((b, _))) => {
                    let chunk = crate::world::chunk_of(b);
                    let same = l
                        .chunks
                        .get(&chunk)
                        .zip(map.chunks.get(&chunk))
                        .is_some_and(|(saved, live)| saved[..] == live.blocks[..]);
                    if !same || l.seed != wg.seed || l.player.distance(player.body.pos) > 0.01 {
                        fail(
                            &mut smoke,
                            format!("save does not match the world (chunk {chunk})"),
                        );
                    } else {
                        info!("smoke: save round trip matches ({} chunks)", l.chunks.len());
                    }
                }
                (None, _) => fail(&mut smoke, format!("no save at {}", path.0.display())),
                // Nothing was dug: that failure is already reported.
                (Some(_), None) => {}
            }
        }
        // Turn the view a quarter. Meshes are in world space, so no loaded chunk may be
        // rebuilt; only chunks the turn brings into view are built.
        556 => {
            smoke.loaded = map.chunks.iter().map(|(p, c)| (*p, c.builds)).collect();
            smoke.meshed = perf.meshed;
            smoke.rot = view.rot();
            view.turn_by(std::f32::consts::FRAC_PI_2);
        }
        640 => {
            if view.turning() || view.rot() != (smoke.rot + 1) % 4 {
                fail(
                    &mut smoke,
                    format!("the view did not finish its turn: {:?}", *view),
                );
            } else {
                // Chunks that were loaded before the turn must still be on their old mesh.
                let rebuilt = smoke
                    .loaded
                    .iter()
                    .filter(|(p, builds)| map.chunks.get(p).is_some_and(|c| c.builds != *builds))
                    .count();
                let built = perf.meshed - smoke.meshed;
                if rebuilt > 0 {
                    fail(
                        &mut smoke,
                        format!("the turn rebuilt {rebuilt} loaded chunks"),
                    );
                } else {
                    info!("smoke: turned a quarter; {built} chunks came into view, none rebuilt");
                }
            }
        }
        650 => smoke.done = true,
        _ => {}
    }
}

/// Runs once the script is over: draws the snapshot if one was asked for, then exits.
pub fn finish(world: &mut World) {
    let smoke = world.resource::<Smoke>();
    if !smoke.done {
        return;
    }
    let (failed, snapshot, scale) = (smoke.failed, smoke.snapshot.clone(), smoke.scale);
    let mut failed = failed;
    if let Some(name) = snapshot {
        match crate::debug::snapshot(world, &name, scale) {
            Ok(path) => info!("snapshot {}", path.display()),
            Err(e) => {
                error!("smoke: snapshot: {e}");
                failed = true;
            }
        }
    }
    if failed {
        error!("smoke test FAILED");
        world.write_message(AppExit::error());
    } else {
        let dug = world.resource::<Smoke>().dug.is_some();
        info!(
            "smoke test passed: world streams in, player walks{}",
            if dug {
                ", breaks, places, saves"
            } else {
                " (break, place and save steps skipped)"
            }
        );
        world.write_message(AppExit::Success);
    }
    world.resource_mut::<Smoke>().done = false;
}
