//! Procedural sound. Every effect is synthesized into a tiny WAV at startup, so the game ships
//! no audio files; volume comes from `settings.ron`.

use std::sync::Arc;

use bevy::audio::{AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::core::settings::Settings;
use crate::core::time::GameTime;
use crate::items::craft::Crafted;
use crate::items::drops::PickedUp;
use crate::items::interact::{ObjectBroken, ObjectPlaced};
use crate::player::Player;
use crate::stats::{Damage, Died};

const RATE: u32 = 22_050;

#[derive(Clone, Copy)]
enum Wave {
    Sine,
    Square,
    Noise,
}

/// Renders a short sweep: `f0` to `f1` Hz over `secs`, exponential decay, soft attack.
fn synth(wave: Wave, f0: f32, f1: f32, secs: f32, gain: f32) -> Vec<i16> {
    let n = (RATE as f32 * secs) as usize;
    let mut phase = 0.0f32;
    let mut seed = 0x1234_5678u32;
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let f = f0 + (f1 - f0) * t;
            phase += f / RATE as f32;
            let env = (1.0 - t).powf(2.2) * (t * 40.0).min(1.0);
            let s = match wave {
                Wave::Sine => (phase * std::f32::consts::TAU).sin(),
                Wave::Square => {
                    if phase.fract() < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Noise => {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
                }
            };
            (s * env * gain * i16::MAX as f32 * 0.5) as i16
        })
        .collect()
}

fn mix(parts: &[Vec<i16>]) -> Vec<i16> {
    let len = parts.iter().map(Vec::len).max().unwrap_or(0);
    (0..len)
        .map(|i| {
            parts
                .iter()
                .map(|p| p.get(i).copied().unwrap_or(0) as i32)
                .sum::<i32>()
                .clamp(i16::MIN as i32, i16::MAX as i32) as i16
        })
        .collect()
}

fn wav(samples: &[i16]) -> AudioSource {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    AudioSource {
        bytes: Arc::from(bytes.into_boxed_slice()),
    }
}

#[derive(Resource)]
pub struct Sounds {
    pub hit: Handle<AudioSource>,
    pub break_: Handle<AudioSource>,
    pub pickup: Handle<AudioSource>,
    pub craft: Handle<AudioSource>,
    pub place: Handle<AudioSource>,
    pub hurt: Handle<AudioSource>,
    pub kill: Handle<AudioSource>,
    pub night: Handle<AudioSource>,
    pub dawn: Handle<AudioSource>,
}

pub fn setup(mut commands: Commands, mut audio: ResMut<Assets<AudioSource>>) {
    let mut add = |s: Vec<i16>| audio.add(wav(&s));
    commands.insert_resource(Sounds {
        hit: add(mix(&[
            synth(Wave::Noise, 0.0, 0.0, 0.06, 0.5),
            synth(Wave::Sine, 220.0, 140.0, 0.08, 0.5),
        ])),
        break_: add(mix(&[
            synth(Wave::Noise, 0.0, 0.0, 0.18, 0.6),
            synth(Wave::Sine, 160.0, 60.0, 0.22, 0.7),
        ])),
        pickup: add(mix(&[
            synth(Wave::Sine, 660.0, 990.0, 0.09, 0.35),
            synth(Wave::Sine, 1320.0, 1320.0, 0.05, 0.15),
        ])),
        craft: add(mix(&[
            synth(Wave::Square, 440.0, 440.0, 0.07, 0.18),
            synth(Wave::Square, 660.0, 660.0, 0.12, 0.18),
        ])),
        place: add(mix(&[
            synth(Wave::Sine, 300.0, 180.0, 0.07, 0.5),
            synth(Wave::Noise, 0.0, 0.0, 0.03, 0.3),
        ])),
        hurt: add(mix(&[
            synth(Wave::Square, 200.0, 90.0, 0.16, 0.35),
            synth(Wave::Noise, 0.0, 0.0, 0.08, 0.3),
        ])),
        kill: add(mix(&[
            synth(Wave::Sine, 500.0, 120.0, 0.25, 0.4),
            synth(Wave::Square, 250.0, 60.0, 0.2, 0.2),
        ])),
        night: add(mix(&[
            synth(Wave::Sine, 110.0, 82.0, 1.4, 0.35),
            synth(Wave::Sine, 165.0, 123.0, 1.4, 0.2),
        ])),
        dawn: add(mix(&[
            synth(Wave::Sine, 330.0, 440.0, 0.5, 0.25),
            synth(Wave::Sine, 495.0, 660.0, 0.7, 0.2),
        ])),
    });
}

fn play(commands: &mut Commands, handle: &Handle<AudioSource>, volume: f32) {
    commands.spawn((
        AudioPlayer::new(handle.clone()),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)),
    ));
}

pub fn cues(
    sounds: Res<Sounds>,
    settings: Res<Settings>,
    clock: Res<GameTime>,
    mut was_night: Local<Option<bool>>,
    mut broken: MessageReader<ObjectBroken>,
    mut placed: MessageReader<ObjectPlaced>,
    mut picked: MessageReader<PickedUp>,
    mut crafted: MessageReader<Crafted>,
    mut damage: MessageReader<Damage>,
    mut died: MessageReader<Died>,
    player: Single<Entity, With<Player>>,
    mut commands: Commands,
) {
    let v = settings.volume.clamp(0.0, 1.0);
    if v <= 0.0 {
        return;
    }
    let mut any = |h: &Handle<AudioSource>, count: usize, gain: f32| {
        if count > 0 {
            play(&mut commands, h, v * gain);
        }
    };
    any(&sounds.break_, broken.read().count(), 0.9);
    any(&sounds.place, placed.read().count(), 0.8);
    any(&sounds.pickup, picked.read().count(), 0.7);
    any(&sounds.craft, crafted.read().count(), 0.8);
    let (mut hurt, mut hit) = (0, 0);
    for d in damage.read() {
        if d.target == *player {
            hurt += 1;
        } else {
            hit += 1;
        }
    }
    any(&sounds.hurt, hurt, 0.9);
    any(&sounds.hit, hit, 0.8);
    any(
        &sounds.kill,
        died.read().filter(|d| d.creature.is_some()).count(),
        0.8,
    );
    let night = clock.day_frac() >= 0.5;
    if let Some(prev) = *was_night
        && prev != night
    {
        play(
            &mut commands,
            if night { &sounds.night } else { &sounds.dawn },
            v * 0.6,
        );
    }
    *was_night = Some(night);
}

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, cues);
    }
}
