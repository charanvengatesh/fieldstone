//! Scripted input for `--headless-smoke`: mines the nearest breakable object, then places a
//! wall, and reports whether each worked. Exits non-zero on failure.

use bevy::prelude::*;

use crate::core::{ItemDef, ObjectDef, Registry};
use crate::items::inventory::Inventory;
use crate::player::{Cursor, Player};
use crate::world::{ChunkMap, cell_of};

#[derive(Resource, Default)]
pub struct Smoke {
    frame: u32,
    target: Option<IVec2>,
    place_at: Option<IVec2>,
    failed: bool,
}

pub fn drive(
    mut smoke: ResMut<Smoke>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut cursor: ResMut<Cursor>,
    map: Res<ChunkMap>,
    objects: Res<Registry<ObjectDef>>,
    items: Res<Registry<ItemDef>>,
    mut player: Single<(&Transform, &mut Inventory), With<Player>>,
    mut exit: MessageWriter<AppExit>,
) {
    smoke.frame += 1;
    let (tf, inv) = &mut *player;
    let here = cell_of(tf.translation.truncate());
    match smoke.frame {
        // Chunks are loaded by now; pick a bare-hands object within reach.
        60 => {
            let found = (-4..=4)
                .flat_map(|dy| (-4..=4).map(move |dx| here + IVec2::new(dx, dy)))
                .find(|c| {
                    map.tile(*c).and_then(|t| t.object).is_some_and(|o| {
                        objects.at(o).hardness == 0
                            && objects.at(o).tool.is_none()
                            && objects.at(o).hp <= 3.0
                    })
                });
            match found {
                Some(c) => {
                    info!(
                        "smoke: mining {} at {c}",
                        objects.id_of(map.tile(c).unwrap().object.unwrap())
                    );
                    smoke.target = Some(c);
                    cursor.cell = Some(c);
                    cursor.world = Some(crate::world::center_of(c));
                    mouse.press(MouseButton::Left);
                }
                None => {
                    error!("smoke: no breakable object near spawn");
                    smoke.failed = true;
                }
            }
        }
        61..=400 => {
            if let Some(c) = smoke.target
                && map.tile(c).is_some_and(|t| t.object.is_none())
            {
                info!(
                    "smoke: mined after {} frames, inventory holds {} stacks",
                    smoke.frame - 60,
                    inv.slots.iter().flatten().count()
                );
                mouse.release(MouseButton::Left);
                smoke.target = None;
            }
        }
        401 => {
            if smoke.target.is_some() {
                error!("smoke: object never broke");
                smoke.failed = true;
            }
            mouse.release(MouseButton::Left);
            if let Some(wall) = items.idx("wall_wood") {
                inv.slots[0] = Some(crate::items::inventory::Stack { item: wall, n: 1 });
                inv.selected = 0;
            }
            let spot = (-3..=3)
                .flat_map(|dy| (-3..=3).map(move |dx| here + IVec2::new(dx, dy)))
                .find(|c| {
                    (*c - here).abs().max_element() >= 2
                        && map.tile(*c).is_some_and(|t| t.object.is_none())
                });
            smoke.place_at = spot;
            cursor.cell = spot;
            mouse.press(MouseButton::Right);
        }
        402 => mouse.release(MouseButton::Right),
        420 => {
            let placed = smoke
                .place_at
                .is_some_and(|c| map.tile(c).is_some_and(|t| t.object.is_some()));
            if placed {
                info!("smoke: placed wall at {:?}", smoke.place_at);
            } else {
                error!("smoke: wall was not placed");
                smoke.failed = true;
            }
        }
        600 => {
            if smoke.failed {
                error!("smoke test FAILED");
                exit.write(AppExit::error());
            } else {
                info!("smoke test ran 600 frames: mining and placing work");
                exit.write(AppExit::Success);
            }
        }
        _ => {}
    }
}
