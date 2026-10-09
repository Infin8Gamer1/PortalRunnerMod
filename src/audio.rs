//! Mirror's Edge's own sounds, read from the player's install at startup
//! (A_Material_Footstep, A_Material_Handstep, A_Character_Female_01,
//! A_Character_Effects, A_Bodyfalls, A_Ambience_Wind, A_M_TimeTrial).
//!
//! Which sound plays when is decided by `faith_anim::sound::Director`: the animations' own footstep, handstep, cloth and breath cues, landings, rolls,
//! slides and the rush of wind at speed. The rooftop ambience and the time-trial music are the
//! app's own.
//!
//! Without the game's sound packages everything here stays silent.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;
use faith_anim::sound::{wanted_cues, CueInfo, Director, SoundCmd};
use faith_move::State as MoveState;
use me_assets::{Packages, SoundBank};

use crate::me_viewmodel::{candidates, MeArms};
use crate::Game;
use faith_move::greybox::Surface;

/// What the feet (or, for handsteps, the hands) are touching.
fn surface_under(c: &faith_move::Controller, level: &faith_move::greybox::Level, hand: bool) -> Surface {
    let root = c.root();
    let fwd = Vec3::new(-c.yaw.sin(), 0.0, -c.yaw.cos());
    let probe = match c.state {
        MoveState::WallRun { normal, .. } | MoveState::WallClimb { normal, .. } | MoveState::WallClimbTurned { normal, .. } => {
            Some((root + Vec3::Y * 1.0 - normal * 0.45, 0.6))
        }
        MoveState::LedgeHang { ledge_y, .. } => Some((Vec3::new(root.x, ledge_y, root.z) + fwd * 0.35, 0.6)),
        _ if hand => Some((root + Vec3::Y * 1.1 + fwd * 0.5, 0.9)),
        _ => None,
    };
    probe
        .and_then(|(p, r)| level.surface_near(p, r))
        .or_else(|| level.surface_near(root - Vec3::Y * 0.02, 0.3))
        .unwrap_or(Surface::Concrete)
}

#[derive(Resource)]
pub struct MeAudio {
    director: Director,
    /// Each cue's variations as Bevy audio (lower-case path).
    sources: HashMap<String, Vec<Handle<AudioSource>>>,
    /// The looping sounds playing (by the director's id).
    loops: HashMap<u64, Entity>,
    ambient: Option<Entity>,
    music: Option<Entity>,
    /// Master × effects volume from the settings menu.
    fx_gain: f32,
}

impl MeAudio {
    /// Carry out one of the director's commands.
    fn run(&mut self, commands: &mut Commands, sinks: &mut Query<&mut AudioSink>, cmd: SoundCmd) -> Option<Entity> {
        match cmd {
            SoundCmd::Play { id, cue, variant, volume, speed, looping } => {
                let source = self.sources.get(&cue)?.get(variant)?.clone();
                let settings = if looping { PlaybackSettings::LOOP } else { PlaybackSettings::DESPAWN };
                let e = commands
                    .spawn((AudioPlayer::new(source), settings.with_volume(Volume::Linear(volume * self.fx_gain)).with_speed(speed)))
                    .id();
                if looping {
                    self.loops.insert(id, e);
                }
                Some(e)
            }
            SoundCmd::Stop(id) => {
                if let Some(e) = self.loops.remove(&id) {
                    if let Ok(mut ec) = commands.get_entity(e) {
                        ec.despawn();
                    }
                }
                None
            }
            SoundCmd::Volume(id, v) => {
                if let Some(&e) = self.loops.get(&id) {
                    if let Ok(mut s) = sinks.get_mut(e) {
                        s.set_volume(Volume::Linear(v * self.fx_gain));
                    }
                }
                None
            }
        }
    }

    fn has(&self, cue: &str) -> bool {
        self.director.has(cue)
    }
}

pub fn setup(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>, arms: Option<Res<MeArms>>) {
    let cooked = match &arms {
        Some(a) => Some(a.cooked_pc().to_path_buf()),
        None => candidates().iter().find_map(|d| me_assets::cooked_pc(d)),
    };
    let Some(cooked) = cooked else { return };
    let mut bank = SoundBank::new(Packages::new(&cooked));

    let mut wanted = wanted_cues(arms.as_ref().map(|a| a.arms()));
    wanted.extend(["A_Ambience_Wind.Wind.Wind".to_string(), "A_M_TimeTrial.Cues.Music".to_string()]);

    let mut infos = HashMap::new();
    let mut handles = HashMap::new();
    for path in wanted {
        let key = path.to_ascii_lowercase();
        if infos.contains_key(&key) {
            continue;
        }
        let Some(cue) = bank.cue(&path) else { continue };
        let h: Vec<Handle<AudioSource>> = cue.waves.iter().map(|w| sources.add(AudioSource { bytes: Arc::from(w.ogg.as_slice()) })).collect();
        infos.insert(key.clone(), CueInfo { variants: h.len(), volume: cue.volume, pitch: cue.pitch, looping: cue.looping });
        handles.insert(key, h);
    }
    if infos.is_empty() {
        info!("Mirror's Edge sound packages not found; no audio");
        return;
    }
    info!("Mirror's Edge audio: {} cues loaded", infos.len());
    let mut audio = MeAudio { director: Director::new(infos), sources: handles, loops: HashMap::new(), ambient: None, music: None, fx_gain: 1.0 };
    let ambient = audio.director.play("A_Ambience_Wind.Wind.Wind", 0.0);
    if let Some(SoundCmd::Play { cue, variant, speed, .. }) = ambient {
        if let Some(src) = audio.sources.get(&cue).and_then(|v| v.get(variant)).cloned() {
            audio.ambient = Some(commands.spawn((AudioPlayer::new(src), PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)).with_speed(speed))).id());
        }
    }
    commands.insert_resource(audio);
}

pub fn play(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    audio: Option<ResMut<MeAudio>>,
    arms: Option<Res<MeArms>>,
    mut sinks: Query<&mut AudioSink>,
    settings: Res<crate::settings::Settings>,
) {
    let Some(mut audio) = audio else { return };
    audio.fx_gain = settings.effects_gain();
    let music_gain = settings.music_gain();
    // Loops that are already playing follow the sliders right away.
    // Rooftop wind outdoors only (test chambers have their own hum).
    let outdoors = !game.sim.level.indoor;
    if let Some(e) = audio.ambient {
        if let Ok(mut s) = sinks.get_mut(e) {
            s.set_volume(Volume::Linear(if outdoors { 0.35 * audio.fx_gain } else { 0.0 }));
        }
    }
    if let Some(e) = audio.music {
        if let Ok(mut s) = sinks.get_mut(e) {
            s.set_volume(Volume::Linear(0.55 * music_gain));
        }
    }
    let dt = time.delta_secs().min(0.1);
    let c = &game.ctrl;

    let anim_driven = arms.as_ref().is_some_and(|a| a.active);
    let notifies = arms.as_ref().filter(|a| a.active).map(|a| a.notifies().to_vec()).unwrap_or_default();
    let mut out = vec![];
    audio.director.update(dt, c, &notifies, anim_driven, game.shot.step_phase, &|hand| surface_under(c, &game.level, hand), &mut out);
    for cmd in out {
        audio.run(&mut commands, &mut sinks, cmd);
    }

    // ---- Mirror's Edge's time-trial music out on the roofs
    match (outdoors, audio.music) {
        (true, None) => {
            if audio.has("A_M_TimeTrial.Cues.Music") {
                if let Some(cmd) = audio.director.play("A_M_TimeTrial.Cues.Music", 0.0) {
                    audio.music = audio.run(&mut commands, &mut sinks, cmd);
                }
            }
        }
        (false, Some(e)) => {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.despawn();
            }
            audio.music = None;
        }
        _ => {}
    }
}
