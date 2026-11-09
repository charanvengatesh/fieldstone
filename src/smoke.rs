//! `--headless-smoke`: a scripted run without a window that exits non-zero when something
//! basic is broken. `--snapshot NAME` runs the same script and then draws what the camera
//! would see to `debug/NAME.png` with the software canvas.

use bevy::prelude::*;

use crate::core::{BlockDef, ItemDef, Registry};
use crate::debug::Canvas;
use crate::interact::{Cursor, within_reach};
use crate::inventory::Inventory;
use crate::player::{FIGURE, Player, closed, figure_texture};
use crate::world::iso::{BLOCK_H, View};
use crate::world::mesh::{Faces, Grid, Palette, chunk_faces, edge_texture};
use crate::world::worldgen::WorldGen;
use crate::world::{AIR, CHUNK, Focus, HEIGHT, Map};

#[derive(Resource)]
pub struct Smoke {
    frame: u32,
    snapshot: Option<String>,
    scale: f32,
    start: Vec3,
    /// The block the script breaks and puts back, and what it was.
    dug: Option<(IVec3, u16)>,
    failed: bool,
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
    focus: Res<Focus>,
    view: Res<View>,
    grid: Res<Grid>,
    wg: Res<WorldGen>,
    blocks: Res<Registry<BlockDef>>,
    palette: Option<Res<Palette>>,
    mut player: Single<(&Player, &mut Inventory)>,
    (mut broken, mut placed, mut exit): (
        MessageReader<crate::interact::BlockBroken>,
        MessageReader<crate::interact::BlockPlaced>,
        MessageWriter<AppExit>,
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
            let meshed = map.chunks.values().filter(|c| !c.dirty).count();
            if map.chunks.len() != 81 || meshed != 81 || map.entities.len() != 81 {
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
            let feet = player.body.pos.floor().as_ivec3();
            let on_ground = closed(&map, &blocks, feet - IVec3::Z) && !closed(&map, &blocks, feet);
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
                    format!("player is not standing on ground at {feet}"),
                );
            }
        }
        // Point at the ground a little way off and hold the button until it breaks.
        310 => {
            let feet = player.body.pos.floor().as_ivec3();
            let spot = (-3..=3)
                .flat_map(|dy| (-3..=3).map(move |dx| IVec3::new(dx, dy, 0)))
                .filter(|d| d.truncate().abs().max_element() == 2)
                .filter_map(|d| {
                    let top = map.top(feet.x + d.x, feet.y + d.y)?;
                    Some(IVec3::new(feet.x + d.x, feet.y + d.y, top))
                })
                .find(|b| {
                    let at = view
                        .project(b.as_vec3() + Vec3::new(0.3, 0.6, 1.0))
                        .truncate();
                    blocks.at(map.block(*b)).breakable
                        && within_reach(player.body.pos, *b)
                        && within_reach(player.body.pos, *b - view.sides().1)
                        // The hole needs a far wall to click when putting the block back.
                        && map.block(*b - view.sides().1) != AIR
                        && view
                            .pick(at, HEIGHT, |c| map.block(c) != AIR)
                            .is_some_and(|h| h.block == *b && h.normal == IVec3::Z)
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
                None => fail(
                    &mut smoke,
                    "no breakable block in view near the player".into(),
                ),
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
        560 => {
            if let (Some(name), Some(palette)) = (smoke.snapshot.clone(), palette) {
                let center = view.project(focus.0).truncate();
                let mut canvas =
                    Canvas::new(1280, 800, smoke.scale, center, Color::srgb_u8(24, 28, 40));
                let edges = edge_texture();
                for pos in map.chunks.keys() {
                    let faces = chunk_faces(*pos, *view, grid.0, &palette, |p| {
                        map.get(p).unwrap_or_else(|| wg.block(p))
                    });
                    let at = view.project((*pos * CHUNK).extend(0).as_vec3());
                    canvas.draw(at, &faces, &edges);
                }
                let at = view.project(player.body.pos.with_z(player.shown_z));
                canvas.draw(at + Vec3::Z * 0.05, &figure_faces(), &figure_texture());
                match canvas.save(&name) {
                    Ok(path) => info!("snapshot {}", path.display()),
                    Err(e) => fail(&mut smoke, format!("snapshot: {e}")),
                }
            }
            if smoke.failed {
                error!("smoke test FAILED");
                exit.write(AppExit::error());
            } else {
                info!("smoke test passed: world streams in, player walks, breaks and places");
                exit.write(AppExit::Success);
            }
        }
        _ => {}
    }
}

/// The player quad as the canvas wants it: the same corners and depths as the real mesh.
fn figure_faces() -> Faces {
    let (w, h) = (FIGURE.x as f32, FIGURE.y as f32);
    let top = h / BLOCK_H;
    Faces {
        positions: vec![
            [-w / 2.0, 0.0, 0.0],
            [w / 2.0, 0.0, 0.0],
            [w / 2.0, h, top],
            [-w / 2.0, h, top],
        ],
        uvs: vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
        colors: vec![[1.0; 4]; 4],
    }
}
