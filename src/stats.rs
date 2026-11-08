//! Health, stamina, damage, death, and experience. Shared by the player and every creature.

use bevy::prelude::*;

use crate::core::{CreatureDef, ItemDef, LootDef, Registry, StatsDef};
use crate::items::drops::{SpawnDrop, roll_loot};
use crate::player::Player;
use crate::world::worldgen::WorldGen;

pub const STAMINA_REGEN: f32 = 8.0;
pub const HP_REGEN: f32 = 0.5;

#[derive(Component, Debug, Clone, Copy)]
pub struct Stats {
    pub hp: f32,
    pub max_hp: f32,
    pub stamina: f32,
    pub max_stamina: f32,
    pub attack: f32,
    pub defense: f32,
    pub speed: f32,
}

impl From<StatsDef> for Stats {
    fn from(d: StatsDef) -> Self {
        Self {
            hp: d.hp,
            max_hp: d.hp,
            stamina: d.stamina,
            max_stamina: d.stamina,
            attack: d.attack,
            defense: d.defense,
            speed: d.speed,
        }
    }
}

/// Which `creatures.ron` record an entity was spawned from.
#[derive(Component, Clone, Copy)]
pub struct Creature(pub u16);

/// Experience and level. Skills are named buckets of xp (`mining`, `combat`, `crafting`).
#[derive(Component, Default, Debug, Clone)]
pub struct Progress {
    pub xp: f32,
    pub level: u32,
    pub skills: Vec<(String, f32)>,
}

impl Progress {
    pub fn next_level_xp(&self) -> f32 {
        100.0 * ((self.level + 1) as f32).powf(1.5)
    }
    pub fn skill(&self, name: &str) -> f32 {
        self.skills
            .iter()
            .find(|(n, _)| n == name)
            .map_or(0.0, |(_, x)| *x)
    }
    /// Skill level: 1 per 50 xp, diminishing.
    pub fn skill_level(&self, name: &str) -> f32 {
        (self.skill(name) / 50.0).sqrt()
    }
}

/// Apply `amount` of raw damage to `target`; defense is subtracted, minimum 1.
#[derive(Message, Clone, Copy)]
pub struct Damage {
    pub target: Entity,
    pub amount: f32,
}

/// An entity's hp reached zero this frame.
#[derive(Message, Clone, Copy)]
pub struct Died {
    pub entity: Entity,
    pub at: Vec2,
    /// `None` for the player.
    pub creature: Option<u16>,
}

/// Experience granted to the player.
#[derive(Message, Clone)]
pub struct GainXp {
    pub amount: f32,
    pub skill: &'static str,
}

/// Brief whitening on hit.
#[derive(Component)]
pub struct HitFlash(pub f32);

pub fn apply_damage(
    mut damage: MessageReader<Damage>,
    mut died: MessageWriter<Died>,
    mut targets: Query<(&mut Stats, &Transform, Option<&Creature>, Has<Player>)>,
    mut commands: Commands,
) {
    for d in damage.read() {
        let Ok((mut stats, tf, creature, is_player)) = targets.get_mut(d.target) else {
            continue;
        };
        if stats.hp <= 0.0 {
            continue;
        }
        stats.hp -= (d.amount - stats.defense).max(1.0);
        commands.entity(d.target).insert(HitFlash(0.15));
        if stats.hp <= 0.0 {
            died.write(Died {
                entity: d.target,
                at: tf.translation.truncate(),
                creature: creature.filter(|_| !is_player).map(|c| c.0),
            });
        }
    }
}

pub fn regen(time: Res<Time>, mut q: Query<&mut Stats>) {
    for mut s in &mut q {
        if s.hp > 0.0 {
            s.stamina = (s.stamina + STAMINA_REGEN * time.delta_secs()).min(s.max_stamina);
            s.hp = (s.hp + HP_REGEN * time.delta_secs()).min(s.max_hp);
        }
    }
}

/// Dead creatures drop loot and grant xp; the dead player respawns at the world spawn.
pub fn on_death(
    mut died: MessageReader<Died>,
    creatures: Res<Registry<CreatureDef>>,
    loot: Res<Registry<LootDef>>,
    items: Res<Registry<ItemDef>>,
    wg: Res<WorldGen>,
    spawn: Res<SpawnPoint>,
    mobs: Query<&Creature, Without<Player>>,
    mut player: Single<(&mut Transform, &mut Stats), With<Player>>,
    mut drops: MessageWriter<SpawnDrop>,
    mut xp: MessageWriter<GainXp>,
    mut commands: Commands,
) {
    for d in died.read() {
        if let Ok(c) = mobs.get(d.entity) {
            let def = creatures.at(c.0);
            if let Some(table) = def.drops.as_deref().and_then(|id| loot.get(id)) {
                roll_loot(
                    table,
                    &items,
                    d.at,
                    wg.seed,
                    d.entity.to_bits() as u32,
                    &mut drops,
                );
            }
            xp.write(GainXp {
                amount: def.xp,
                skill: "combat",
            });
            commands.entity(d.entity).despawn();
        } else {
            let (tf, stats) = &mut *player;
            info!("you died; respawning");
            tf.translation = spawn.0.extend(tf.translation.z);
            stats.hp = stats.max_hp;
            stats.stamina = stats.max_stamina;
        }
    }
}

pub fn gain_xp(
    mut xp: MessageReader<GainXp>,
    mut player: Single<(&mut Progress, &mut Stats), With<Player>>,
) {
    let (prog, stats) = &mut *player;
    for g in xp.read() {
        prog.xp += g.amount;
        match prog.skills.iter_mut().find(|(n, _)| n == g.skill) {
            Some((_, x)) => *x += g.amount,
            None => prog.skills.push((g.skill.to_owned(), g.amount)),
        }
        while prog.xp >= prog.next_level_xp() {
            prog.xp -= prog.next_level_xp();
            prog.level += 1;
            stats.max_hp += 5.0;
            stats.hp = stats.max_hp;
            stats.attack += 1.0;
            info!("level {}", prog.level);
        }
    }
}

/// Scales a hit entity up briefly, then back.
pub fn hit_flash(
    time: Res<Time>,
    mut q: Query<(Entity, &mut HitFlash, &mut Transform)>,
    mut commands: Commands,
) {
    for (e, mut flash, mut tf) in &mut q {
        flash.0 -= time.delta_secs();
        tf.scale = Vec3::splat(if flash.0 > 0.0 { 1.25 } else { 1.0 });
        if flash.0 <= 0.0 {
            commands.entity(e).remove::<HitFlash>();
        }
    }
}

/// Where the player appears on a new world and after dying.
#[derive(Resource, Default)]
pub struct SpawnPoint(pub Vec2);

pub struct StatsPlugin;

impl Plugin for StatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpawnPoint>()
            .add_message::<Damage>()
            .add_message::<Died>()
            .add_message::<GainXp>()
            .add_systems(
                Update,
                (apply_damage, on_death, gain_xp, regen, hit_flash)
                    .chain()
                    .in_set(StatsSet),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct StatsSet;
