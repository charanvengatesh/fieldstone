//! Small things that make the world feel touched: chips fly when a block breaks, dust
//! rises when the player lands. Particles are 2 x 2 texel sprites coloured from the
//! block's own tile, moved in world space under gravity and projected each frame; the
//! sprite pipeline depth-tests them against the blocks without writing depth.

use bevy::prelude::*;

use crate::core::{BlockDef, Registry, hash01};
use crate::interact::BlockBroken;
use crate::player::{Landed, closed};
use crate::world::Map;
use crate::world::iso::View;
use crate::world::tiles::{TILE, Tiles};

/// Live particles at most; past this, bursts are skipped rather than queued.
pub const CAP: usize = 200;
const LIFE: f32 = 0.6;
const GRAVITY: f32 = 22.0;
/// Depth nudge so a chip resting on a face is not hidden by it.
const DEPTH_BIAS: f32 = 0.04;

#[derive(Component)]
pub struct Particle {
    /// World position in blocks.
    pub pos: Vec3,
    /// Blocks per second.
    pub vel: Vec3,
    pub life: f32,
}

/// Colours a burst takes: a few texels of the block's top tile.
pub fn chip_colors(block: u16, tiles: &Tiles, n: usize) -> Vec<Color> {
    let Some(data) = tiles.image.data.as_ref() else {
        return vec![Color::WHITE; n];
    };
    let o = tiles.origin(block, 0).as_uvec2();
    (0..n)
        .map(|i| {
            let x = (hash01(0xC01, block as i32, i as i32, 1) * TILE as f32) as u32;
            let y = (hash01(0xC01, block as i32, i as i32, 2) * TILE as f32) as u32;
            let at = (((o.y + y) * tiles.size.x + o.x + x) * 4) as usize;
            Color::srgb_u8(data[at], data[at + 1], data[at + 2])
        })
        .collect()
}

/// The velocities of a burst of `n` chips from a block: outward and up, each its own way.
pub fn burst(seed: i32, n: usize, up: f32) -> Vec<Vec3> {
    (0..n)
        .map(|i| {
            let a = hash01(0xB0B, seed, i as i32, 1) * std::f32::consts::TAU;
            let r = 1.5 + hash01(0xB0B, seed, i as i32, 2) * 2.0;
            Vec3::new(
                a.cos() * r,
                a.sin() * r,
                up + hash01(0xB0B, seed, i as i32, 3) * 3.0,
            )
        })
        .collect()
}

fn spawn_burst(
    commands: &mut Commands,
    origin: Vec3,
    colors: &[Color],
    vels: &[Vec3],
    room: &mut usize,
) {
    for (color, vel) in colors.iter().zip(vels) {
        if *room == 0 {
            return;
        }
        *room -= 1;
        commands.spawn((
            Particle {
                pos: origin,
                vel: *vel,
                life: LIFE,
            },
            Sprite::from_color(*color, Vec2::splat(2.0)),
            Transform::from_translation(Vec3::new(0.0, 0.0, -1e5)),
        ));
    }
}

/// Chips from broken blocks and dust from landings.
pub fn emit(
    mut broken: MessageReader<BlockBroken>,
    mut landed: MessageReader<Landed>,
    tiles: Option<Res<Tiles>>,
    map: Res<Map>,
    live: Query<(), With<Particle>>,
    mut commands: Commands,
) {
    let mut room = CAP.saturating_sub(live.iter().count());
    for b in broken.read() {
        let colors = match tiles.as_deref() {
            Some(t) => chip_colors(b.block, t, 8),
            None => vec![Color::WHITE; 8],
        };
        let seed =
            b.at.x
                .wrapping_mul(73)
                .wrapping_add(b.at.y.wrapping_mul(151))
                ^ b.at.z;
        let vels = burst(seed, 8, 3.0);
        spawn_burst(
            &mut commands,
            b.at.as_vec3() + Vec3::splat(0.5),
            &colors,
            &vels,
            &mut room,
        );
    }
    for l in landed.read() {
        // Dust takes the colour of the ground, lightened, and stays low.
        let under = l.pos.floor().as_ivec3() - IVec3::Z;
        let ground = map.block(under);
        let colors: Vec<Color> = match tiles.as_deref() {
            Some(t) => chip_colors(ground, t, 6)
                .into_iter()
                .map(|c| c.mix(&Color::WHITE, 0.35))
                .collect(),
            None => vec![Color::WHITE; 6],
        };
        let n = if l.drop >= 2.0 { 6 } else { 4 };
        let vels = burst(under.x ^ under.y, n, 0.6);
        spawn_burst(
            &mut commands,
            l.pos + Vec3::Z * 0.1,
            &colors[..n],
            &vels,
            &mut room,
        );
    }
}

/// Moves every particle, lands it on the ground, and projects it to the screen.
pub fn tick(
    time: Res<Time>,
    view: Res<View>,
    map: Res<Map>,
    blocks: Res<Registry<BlockDef>>,
    mut particles: Query<(Entity, &mut Particle, &mut Transform)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs().min(0.05);
    for (e, mut p, mut tf) in &mut particles {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        p.vel.z -= GRAVITY * dt;
        let vel = p.vel;
        let next = p.pos + vel * dt;
        if closed(&map, &blocks, next.floor().as_ivec3()) {
            // Stop on whatever was hit and slide a little.
            p.vel = Vec3::new(vel.x * 0.5, vel.y * 0.5, 0.0);
            if vel.z < 0.0 {
                p.pos.z = next.z.floor() + 1.0 + 1e-3;
            }
        } else {
            p.pos = next;
        }
        let at = view.project(p.pos);
        tf.translation = at + Vec3::Z * DEPTH_BIAS;
    }
}

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (emit, tick)
                .chain()
                .after(crate::interact::InteractSet)
                .before(crate::world::WorldSet)
                .run_if(crate::core::not_paused),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_spreads_out_and_up() {
        let v = burst(7, 8, 3.0);
        assert_eq!(v.len(), 8);
        assert!(v.iter().all(|v| v.z >= 3.0 && v.truncate().length() >= 1.5));
        // Not all the same way.
        let mean: Vec2 = v.iter().map(|v| v.truncate().normalize()).sum::<Vec2>() / 8.0;
        assert!(mean.length() < 0.6, "{mean}");
    }
}
