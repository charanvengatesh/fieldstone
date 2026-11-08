//! Creature behavior: wander, chase, bite.

use bevy::prelude::*;

use crate::core::{Behavior, CreatureDef, ObjectDef, Registry, Rng, TerrainDef};
use crate::player::Player;
use crate::stats::{Creature, Damage, Stats};
use crate::world::{CELL, ChunkMap, PLAYER_R, slide, solid_half};

const BITE_RANGE: f32 = CELL * 0.9;
const BITE_COOLDOWN: f32 = 1.0;

#[derive(Component, Default)]
pub struct Ai {
    pub dir: Vec2,
    /// Seconds until the next wander decision.
    pub timer: f32,
    pub bite: f32,
    /// Seconds of ignoring the player (after a respawn nearby).
    pub calm: f32,
}

pub fn think(
    time: Res<Time>,
    mut rng: ResMut<Rng>,
    creatures: Res<Registry<CreatureDef>>,
    map: Res<ChunkMap>,
    terrain: Res<Registry<TerrainDef>>,
    objects: Res<Registry<ObjectDef>>,
    player: Single<(Entity, &Transform), With<Player>>,
    mut mobs: Query<(&Creature, &Stats, &mut Ai, &mut Transform), Without<Player>>,
    mut calm: MessageReader<crate::stats::CalmHostiles>,
    mut damage: MessageWriter<Damage>,
) {
    let (player_entity, ptf) = *player;
    let calms: Vec<crate::stats::CalmHostiles> = calm.read().copied().collect();
    let target = ptf.translation.truncate();
    let dt = time.delta_secs();
    let solid = |c| solid_half(&map, &terrain, &objects, c);
    for (c, stats, mut ai, mut tf) in &mut mobs {
        let def = creatures.at(c.0);
        let pos = tf.translation.truncate();
        let to_player = target - pos;
        ai.timer -= dt;
        ai.bite -= dt;
        ai.calm -= dt;
        for c in &calms {
            if pos.distance(c.around) <= c.radius {
                ai.calm = crate::stats::RESPAWN_GRACE;
                ai.dir = (pos - c.around).normalize_or_zero();
            }
        }
        let hostile = ai.calm <= 0.0
            && matches!(def.ai, Behavior::Hostile { aggro } if to_player.length() <= aggro * CELL);
        let speed = if hostile {
            if to_player.length() <= BITE_RANGE {
                ai.dir = Vec2::ZERO;
                if ai.bite <= 0.0 {
                    ai.bite = BITE_COOLDOWN;
                    damage.write(Damage {
                        target: player_entity,
                        amount: stats.attack,
                    });
                }
            } else {
                ai.dir = to_player.normalize_or_zero();
            }
            stats.speed
        } else {
            if ai.timer <= 0.0 {
                ai.timer = rng.range(1.0, 3.0);
                ai.dir = if rng.f32() < 0.6 {
                    Vec2::ZERO
                } else {
                    Vec2::from_angle(rng.range(0.0, std::f32::consts::TAU))
                };
            }
            stats.speed * 0.5
        };
        let delta = ai.dir * speed * dt;
        let mut p = pos;
        p = slide(p, delta.with_y(0.0), PLAYER_R, solid);
        p = slide(p, delta.with_x(0.0), PLAYER_R, solid);
        // Bumped into something while wandering: pick a new direction sooner.
        if !hostile && (p - pos).length() < delta.length() * 0.5 {
            ai.timer = 0.0;
        }
        tf.translation = p.extend(tf.translation.z);
    }
}
