//! `--headless-smoke`: a scripted run without a window that exits non-zero when something
//! basic is broken. `--snapshot NAME` runs the same script and then draws what the camera
//! would see to `debug/NAME.png` with the software canvas.

use bevy::prelude::*;

use crate::core::{BlockDef, Registry};
use crate::debug::Canvas;
use crate::player::{FIGURE, Player, closed, figure_texture};
use crate::world::iso::{BLOCK_H, View};
use crate::world::mesh::{Faces, Grid, Palette, chunk_faces, edge_texture};
use crate::world::worldgen::WorldGen;
use crate::world::{CHUNK, Focus, Map};

#[derive(Resource)]
pub struct Smoke {
    frame: u32,
    snapshot: Option<String>,
    scale: f32,
    start: Vec3,
    failed: bool,
}

impl Smoke {
    pub fn new(snapshot: Option<String>, scale: f32) -> Self {
        Self {
            frame: 0,
            snapshot,
            scale,
            start: Vec3::ZERO,
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
    map: Res<Map>,
    focus: Res<Focus>,
    view: Res<View>,
    grid: Res<Grid>,
    wg: Res<WorldGen>,
    blocks: Res<Registry<BlockDef>>,
    palette: Option<Res<Palette>>,
    player: Single<&Player>,
    mut exit: MessageWriter<AppExit>,
) {
    smoke.frame += 1;
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
        320 => {
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
                info!("smoke test passed: world streams in, player lands and walks");
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
