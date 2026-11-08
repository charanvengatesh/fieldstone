//! Attacking with the held weapon: melee arcs and projectiles.

use bevy::prelude::*;

use crate::art::ShapeAssets;
use crate::core::{ItemDef, ObjectDef, Registry, ShapeDef, TerrainDef, Use};
use crate::fx::Swing;
use crate::items::inventory::Inventory;
use crate::player::{Cursor, Player};
use crate::stats::{Creature, Damage, Stats};
use crate::world::{CELL, ChunkMap, cell_of, center_of, solid_half};

const PROJECTILE_SPEED: f32 = 420.0;
const ARC_HALF_ANGLE: f32 = 1.1;
const SWING_STAMINA: f32 = 6.0;

#[derive(Component, Default)]
pub struct Cooldown(pub f32);

#[derive(Component)]
pub struct Projectile {
    pub dir: Vec2,
    pub damage: f32,
    pub left: f32,
}

/// Left click with a weapon held. Melee hits every creature in a cone toward the cursor;
/// ranged spawns a projectile. Mining still happens when the cursor is on an object.
pub fn attack(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    cursor: Res<Cursor>,
    items: Res<Registry<ItemDef>>,
    objects: Res<Registry<ObjectDef>>,
    shapes: Res<Registry<ShapeDef>>,
    assets: Res<ShapeAssets>,
    map: Res<ChunkMap>,
    mut player: Single<(&Transform, &Inventory, &mut Stats, &mut Cooldown), With<Player>>,
    mobs: Query<(Entity, &Transform), (With<Creature>, Without<Player>)>,
    mut damage: MessageWriter<Damage>,
    mut swings: MessageWriter<Swing>,
    ui: Res<crate::core::UiHovered>,
    mut commands: Commands,
) {
    let (tf, inv, stats, cd) = &mut *player;
    cd.0 = (cd.0 - time.delta_secs()).max(0.0);
    if !mouse.just_pressed(MouseButton::Left) || cd.0 > 0.0 || ui.0 {
        return;
    }
    let Some(Use::Weapon {
        damage: dmg,
        range,
        cooldown,
        projectile,
    }) = inv.held().map(|s| &items.at(s.item).usage)
    else {
        return;
    };
    let Some(target) = cursor.cell else { return };
    // A cursor on a real object means the player wants to mine it, not swing at it.
    if map
        .tile(target)
        .and_then(|t| t.object)
        .is_some_and(|o| !objects.at(o).decor())
        && projectile.is_none()
    {
        return;
    }
    if stats.stamina < SWING_STAMINA {
        return;
    }
    stats.stamina -= SWING_STAMINA;
    cd.0 = *cooldown;
    let origin = tf.translation.truncate();
    let dir = (center_of(target) - origin).normalize_or_zero();
    let total = dmg + stats.attack;
    match projectile {
        Some(shape) => {
            let e = crate::art::spawn_shape(
                &mut commands,
                &assets,
                shapes.idx(shape),
                Transform::from_translation((origin + dir * CELL * 0.5).extend(1.8))
                    .with_rotation(Quat::from_rotation_z(dir.to_angle())),
            );
            commands.entity(e).insert(Projectile {
                dir,
                damage: total,
                left: range * CELL,
            });
        }
        None => {
            swings.write(Swing {
                origin,
                dir,
                range: range * CELL,
            });
            for (e, mtf) in &mobs {
                let to = mtf.translation.truncate() - origin;
                if to.length() <= range * CELL
                    && dir.angle_to(to.normalize_or_zero()).abs() <= ARC_HALF_ANGLE
                {
                    damage.write(Damage {
                        target: e,
                        amount: total,
                    });
                }
            }
        }
    }
}

pub fn fly(
    time: Res<Time>,
    map: Res<ChunkMap>,
    terrain: Res<Registry<TerrainDef>>,
    objects: Res<Registry<ObjectDef>>,
    mut shots: Query<(Entity, &mut Transform, &mut Projectile)>,
    mobs: Query<(Entity, &Transform), (With<Creature>, Without<Projectile>, Without<Player>)>,
    mut damage: MessageWriter<Damage>,
    mut commands: Commands,
) {
    for (e, mut tf, mut p) in &mut shots {
        let step = p.dir * PROJECTILE_SPEED * time.delta_secs();
        tf.translation += step.extend(0.0);
        p.left -= step.length();
        let pos = tf.translation.truncate();
        let hit = mobs
            .iter()
            .find(|(_, m)| m.translation.truncate().distance(pos) < CELL * 0.45)
            .map(|(m, _)| m);
        if let Some(m) = hit {
            damage.write(Damage {
                target: m,
                amount: p.damage,
            });
        }
        let blocked = solid_half(&map, &terrain, &objects, cell_of(pos))
            .is_some_and(|h| (pos - center_of(cell_of(pos))).abs().max_element() < h);
        if hit.is_some() || p.left <= 0.0 || blocked {
            commands.entity(e).despawn();
        }
    }
}
