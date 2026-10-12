//! Portal's maps' own sounds, from the library `portal_extract` writes (`sounds/lib/`, with
//! the soundscripts and soundscapes beside it): each `ambient_generic` playing and stopping as
//! the map's logic says, heard from where it is (or everywhere), how far it carries as its
//! soundscript (or its radius) has it; the area's ambience (env_soundscape), the nearest one
//! in sight within its radius, looping and now and then a sound at random; fans humming as
//! they spin. Files are read the first time they're played.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume};
use bevy::prelude::*;

use aperture::io::Kind;

use crate::{Game, PlayerCamera};

/// A soundscript: its files (one at random), volume, pitch (1: as recorded) and soundlevel.
pub struct Script {
    pub volume: f32,
    pub pitch: (f32, f32),
    pub soundlevel: f32,
    pub waves: Vec<String>,
}

/// A file of the library: how long it is, whether it loops, its loudness correction.
pub struct LibFile {
    pub secs: f32,
    pub loops: bool,
    pub gain: f32,
}

/// One of a soundscape's rules.
#[derive(Clone, Debug)]
pub enum Rule {
    Loop { volume: f32, pitch: f32, wave: String },
    Random { every: (f32, f32), volume: (f32, f32), pitch: (f32, f32), waves: Vec<String> },
    Sub { volume: f32, name: String },
}

/// The library, as read at startup.
#[derive(Resource, Default)]
pub struct SoundLib {
    root: Option<PathBuf>,
    /// By lower-case name.
    pub scripts: HashMap<String, Script>,
    /// By path (under `sound/`).
    pub files: HashMap<String, LibFile>,
    pub scapes: HashMap<String, Vec<Rule>>,
    cache: HashMap<String, Option<Handle<AudioSource>>>,
}

/// A sound found: its file, how loud (with its loudness correction), its pitch, how far it
/// carries (its soundlevel's distance at which it's at full volume, metres) and whether it
/// loops.
pub struct Found {
    pub handle: Handle<AudioSource>,
    pub volume: f32,
    pub pitch: f32,
    pub near: f32,
    pub loops: bool,
}

/// Source's soundlevel as the distance (metres) within which a sound's at full volume
/// (snd_refdist 36 units at snd_refdb 60 dB), beyond which it falls off with distance.
pub fn near_distance(soundlevel: f32) -> f32 {
    if soundlevel <= 0.0 {
        return f32::INFINITY;
    }
    36.0 * 0.0254 * 10f32.powf((soundlevel - 60.0) / 20.0)
}

fn floats(s: &str) -> Vec<f32> {
    s.split([' ', ',']).filter_map(|x| x.trim().parse().ok()).collect()
}

impl SoundLib {
    pub fn load() -> SoundLib {
        let Some(root) = crate::extracted::find_root() else { return SoundLib::default() };
        let dir = root.join("sounds");
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap_or_default();
        let mut lib = SoundLib { root: Some(root.clone()), ..default() };
        for l in read("lib/index.txt").lines() {
            let f: Vec<&str> = l.split('|').collect();
            if f.len() < 4 {
                continue;
            }
            let db: f32 = f[3].parse().unwrap_or(-20.0);
            lib.files.insert(f[0].to_string(), LibFile { secs: f[1].parse().unwrap_or(0.0), loops: f[2] == "1", gain: crate::extracted::normalising_gain(db) });
        }
        for l in read("scripts.txt").lines() {
            let f: Vec<&str> = l.split('|').collect();
            if f.len() < 5 {
                continue;
            }
            let p = floats(f[2]);
            let waves: Vec<String> = f[4].split(';').filter(|w| !w.is_empty()).map(str::to_string).collect();
            lib.scripts.insert(f[0].to_ascii_lowercase(), Script { volume: f[1].parse().unwrap_or(1.0), pitch: (p.first().copied().unwrap_or(100.0) / 100.0, p.get(1).copied().unwrap_or(100.0) / 100.0), soundlevel: f[3].parse().unwrap_or(75.0), waves });
        }
        let mut scape: Option<(String, Vec<Rule>)> = None;
        for l in read("soundscapes.txt").lines() {
            let mut it = l.split_whitespace();
            match it.next() {
                Some("scape") => {
                    if let Some((n, r)) = scape.take() {
                        lib.scapes.insert(n, r);
                    }
                    scape = it.next().map(|n| (n.to_string(), Vec::new()));
                }
                Some("loop") => {
                    let v: Vec<&str> = it.collect();
                    if let (Some(rules), [vol, pitch, wave]) = (scape.as_mut().map(|s| &mut s.1), v.as_slice()) {
                        rules.push(Rule::Loop { volume: vol.parse().unwrap_or(1.0), pitch: pitch.parse::<f32>().unwrap_or(100.0) / 100.0, wave: wave.to_string() });
                    }
                }
                Some("random") => {
                    let v: Vec<&str> = it.collect();
                    if let (Some(rules), [t0, t1, v0, v1, p0, p1, waves]) = (scape.as_mut().map(|s| &mut s.1), v.as_slice()) {
                        let n = |s: &str| s.parse::<f32>().unwrap_or(1.0);
                        rules.push(Rule::Random { every: (n(t0), n(t1)), volume: (n(v0), n(v1)), pitch: (n(p0) / 100.0, n(p1) / 100.0), waves: waves.split(';').map(str::to_string).collect() });
                    }
                }
                Some("sub") => {
                    let v: Vec<&str> = it.collect();
                    if let (Some(rules), [vol, name]) = (scape.as_mut().map(|s| &mut s.1), v.as_slice()) {
                        rules.push(Rule::Sub { volume: vol.parse().unwrap_or(1.0), name: name.to_string() });
                    }
                }
                _ => {}
            }
        }
        if let Some((n, r)) = scape {
            lib.scapes.insert(n, r);
        }
        if !lib.files.is_empty() {
            info!("Sound library: {} files, {} soundscripts, {} soundscapes", lib.files.len(), lib.scripts.len(), lib.scapes.len());
        }
        lib
    }

    /// A file of the library, read the first time.
    pub fn file(&mut self, path: &str, sources: &mut Assets<AudioSource>) -> Option<Handle<AudioSource>> {
        if let Some(h) = self.cache.get(path) {
            return h.clone();
        }
        let bytes = self.root.as_ref().and_then(|r| std::fs::read(r.join("sounds").join("lib").join(path)).ok());
        // (Only what decodes: a WAVE's RIFF header, an MP3's tag or frame.)
        let ok = bytes.as_ref().is_some_and(|b| (path.ends_with(".wav") && b.len() > 44 && b.starts_with(b"RIFF")) || (path.ends_with(".mp3") && b.len() > 4));
        let h = bytes.filter(|_| ok).map(|b| sources.add(AudioSource { bytes: Arc::from(b.into_boxed_slice()) }));
        self.cache.insert(path.to_string(), h.clone());
        h
    }

    /// A sound by its soundscript's name, or its file's path (`k` picks among a soundscript's
    /// files, and its pitch in its range).
    pub fn find(&mut self, name: &str, k: u32, sources: &mut Assets<AudioSource>) -> Option<Found> {
        let lower = name.trim().to_ascii_lowercase().replace('\\', "/");
        let (wave, volume, pitch, soundlevel) = match self.scripts.get(&lower) {
            Some(s) if !s.waves.is_empty() => {
                let w = s.waves[k as usize % s.waves.len()].clone();
                let f = (k.wrapping_mul(2_654_435_761) >> 16) as f32 / 65_536.0;
                (w, s.volume, s.pitch.0 + (s.pitch.1 - s.pitch.0) * f, s.soundlevel)
            }
            _ => (lower.trim_start_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_').to_string(), 1.0, 1.0, 75.0),
        };
        let handle = self.file(&wave, sources)?;
        let f = self.files.get(&wave);
        Some(Found { handle, volume: volume * f.map_or(1.0, |f| f.gain), pitch, near: near_distance(soundlevel), loops: f.is_some_and(|f| f.loops) })
    }

    /// Every sound there is to place (the editor's browser): the soundscripts whose files are
    /// all there, then the files.
    pub fn listing(&self) -> Vec<String> {
        let mut scripts: Vec<String> = self.scripts.iter().filter(|(_, s)| s.waves.iter().all(|w| self.files.contains_key(w))).map(|(n, _)| n.clone()).collect();
        scripts.sort();
        let mut files: Vec<String> = self.files.keys().cloned().collect();
        files.sort();
        scripts.extend(files);
        scripts
    }
}

pub fn setup(mut commands: Commands) {
    commands.insert_resource(SoundLib::load());
}

/// A sound playing for an `ambient_generic`: which of its starts, its file's volume, how far
/// it carries at full volume, whether it loops, when it started.
struct Emitter {
    entity: Entity,
    starts: u32,
    volume: f32,
    near: f32,
    loops: bool,
    since: f32,
}

/// What's playing.
#[derive(Default)]
pub struct State {
    loads: u64,
    emitters: HashMap<usize, Emitter>,
    /// Each sound's last start that's been played (a one-off that's finished isn't played
    /// again till it's started again).
    played: HashMap<usize, u32>,
    /// The soundscape heard (its name), its loops, its random sounds (when each is next).
    scape: Option<String>,
    loops: Vec<Entity>,
    randoms: Vec<(f32, Rule, f32)>,
    /// Each fan's (mover's) hum.
    fans: HashMap<usize, Entity>,
    time: f32,
    look: f32,
    n: u32,
}

/// How a sound fades with distance: full within `near`, then as one over distance, gone by
/// `reach` (fading over its last fifth).
fn falloff(d: f32, near: f32, reach: f32) -> f32 {
    let g = if d <= near { 1.0 } else { near / d };
    let edge = ((reach - d) / (reach * 0.2).max(0.01)).clamp(0.0, 1.0);
    (g * edge).clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
pub fn play(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    mut lib: ResMut<SoundLib>,
    mut sources: ResMut<Assets<AudioSource>>,
    settings: Res<crate::settings::Settings>,
    camera: Query<&GlobalTransform, With<PlayerCamera>>,
    mut sinks: Query<&mut AudioSink>,
    players: Query<(), With<AudioPlayer>>,
    mut st: Local<State>,
    editor: Res<crate::editor::Editor>,
) {
    let st = &mut *st;
    // A new level: what was playing stops.
    if st.loads != game.loads || editor.active {
        for e in st.emitters.drain().map(|(_, e)| e.entity).chain(st.loops.drain(..)).chain(st.fans.drain().map(|(_, e)| e)) {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.despawn();
            }
        }
        st.scape = None;
        st.randoms.clear();
        st.played.clear();
        st.loads = game.loads;
        if editor.active {
            return;
        }
    }
    if lib.files.is_empty() {
        return;
    }
    let Some(io) = game.sim.io.as_ref() else { return };
    let dt = time.delta_secs();
    st.time += dt;
    let fx = settings.effects_gain();
    let ear = camera.single().map(|t| t.translation()).unwrap_or(game.ctrl.feet);
    let sim = &game.sim;
    let mover_offset = |m: Option<usize>| m.and_then(|m| sim.movers.get(m)).map_or(Vec3::ZERO, |m| m.offset());

    // ---- each ambient_generic, as the logic has it: started (again) when its start count goes
    // up, a looping one stopped when it's stopped (a one-off plays out)
    for (e, ent) in io.ents.iter().enumerate() {
        let Kind::Sound { at, sound, reach, everywhere, looped, mover, .. } = &ent.kind else { continue };
        let Some(state) = io.sounds.get(&e) else { continue };
        let at = *at + mover_offset(*mover);
        let gain = |near: f32| if *everywhere { 1.0 } else { falloff(at.distance(ear), near.min(*reach), *reach) };
        // A one-off that's finished (its player gone, a moment after it was started).
        if st.emitters.get(&e).is_some_and(|em| st.time - em.since > 0.5 && !players.contains(em.entity)) {
            st.emitters.remove(&e);
        }
        let stale = st.emitters.get(&e).is_some_and(|em| em.starts != state.starts || (!state.playing && em.loops));
        if stale {
            let em = st.emitters.remove(&e).unwrap();
            if let Ok(mut ec) = commands.get_entity(em.entity) {
                ec.despawn();
            }
        }
        if state.playing && !st.emitters.contains_key(&e) && st.played.get(&e) != Some(&state.starts) {
            st.played.insert(e, state.starts);
            st.n = st.n.wrapping_add(1);
            let Some(found) = lib.find(sound, st.n, &mut sources) else { continue };
            let loops = *looped && found.loops;
            let v = found.volume * state.volume * gain(found.near) * fx;
            let ps = if loops { PlaybackSettings::LOOP } else { PlaybackSettings::DESPAWN };
            let entity = commands.spawn((AudioPlayer::new(found.handle), ps.with_volume(Volume::Linear(v)).with_speed(found.pitch * state.pitch))).id();
            st.emitters.insert(e, Emitter { entity, starts: state.starts, volume: found.volume, near: found.near, loops, since: st.time });
        } else if let Some(em) = st.emitters.get(&e) {
            if let Ok(mut s) = sinks.get_mut(em.entity) {
                s.set_volume(Volume::Linear(em.volume * state.volume * gain(em.near) * fx));
            }
        }
    }

    // ---- the soundscape: the nearest on, in sight and in reach (looked for now and then);
    // none found, the last one stays
    st.look -= dt;
    if st.look <= 0.0 {
        st.look = 0.3;
        let mut best: Option<(f32, &str)> = None;
        for ent in &io.ents {
            let Kind::Soundscape { at, radius, scape, enabled: true } = &ent.kind else { continue };
            let d = at.distance(ear);
            if d > *radius || best.is_some_and(|(b, _)| d >= b) {
                continue;
            }
            let dir = (*at - ear).normalize_or_zero();
            let (_, hit) = sim.trace(ear, dir, d, aperture::Mask::SHOT, 0);
            if hit.is_none_or(|h| h.t >= d - 0.05) {
                best = Some((d, scape.as_str()));
            }
        }
        if let Some((_, name)) = best {
            if st.scape.as_deref() != Some(name) {
                st.scape = Some(name.to_string());
                for e in st.loops.drain(..) {
                    if let Ok(mut ec) = commands.get_entity(e) {
                        ec.despawn();
                    }
                }
                st.randoms.clear();
                // Its rules (and those of what it plays), each at its volume.
                let mut rules: Vec<(Rule, f32)> = Vec::new();
                let mut todo = vec![(name.to_string(), 1.0, 0)];
                while let Some((n, vol, depth)) = todo.pop() {
                    for r in lib.scapes.get(&n).cloned().unwrap_or_default() {
                        match r {
                            Rule::Sub { volume, name } if depth < 4 => todo.push((name, vol * volume, depth + 1)),
                            Rule::Sub { .. } => {}
                            r => rules.push((r, vol)),
                        }
                    }
                }
                for (r, vol) in rules {
                    match r {
                        Rule::Loop { volume, pitch, wave } => {
                            st.n = st.n.wrapping_add(1);
                            if let Some(found) = lib.find(&wave, st.n, &mut sources) {
                                let v = found.volume * volume * vol * fx * SCAPE_GAIN;
                                st.loops.push(commands.spawn((AudioPlayer::new(found.handle), PlaybackSettings::LOOP.with_volume(Volume::Linear(v)).with_speed(pitch))).id());
                            }
                        }
                        r @ Rule::Random { .. } => {
                            let first = st.time + 1.0;
                            st.randoms.push((first, r, vol));
                        }
                        Rule::Sub { .. } => {}
                    }
                }
            }
        }
    }
    let now = st.time;
    for k in 0..st.randoms.len() {
        if st.randoms[k].0 > now {
            continue;
        }
        st.n = st.n.wrapping_add(1);
        let n = st.n;
        let f = |lo: f32, hi: f32, salt: u32| lo + (hi - lo) * ((n.wrapping_mul(2_654_435_761).wrapping_add(salt.wrapping_mul(40_503)) >> 8) as f32 / 16_777_216.0);
        let (_, Rule::Random { every, volume, pitch, waves }, vol) = st.randoms[k].clone() else { continue };
        st.randoms[k].0 = now + f(every.0, every.1, 1).max(0.2);
        let Some(w) = waves.get(n as usize % waves.len().max(1)) else { continue };
        if let Some(found) = lib.find(w, n, &mut sources) {
            let v = found.volume * f(volume.0, volume.1, 2) * vol * fx * SCAPE_GAIN;
            commands.spawn((AudioPlayer::new(found.handle), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(v)).with_speed(f(pitch.0, pitch.1, 3))));
        }
    }

    // ---- fans: their hum, louder and higher as they spin up
    for (i, m) in sim.movers.iter().enumerate() {
        let Some(spin) = &m.def.spin else { continue };
        if spin.sound.is_empty() {
            continue;
        }
        let frac = (m.spin_rate / spin.speed.abs().max(1e-3)).abs().min(1.0);
        let at = spin.pivot + m.offset();
        match st.fans.get(&i).copied() {
            Some(e) if frac < 0.01 => {
                if let Ok(mut ec) = commands.get_entity(e) {
                    ec.despawn();
                }
                st.fans.remove(&i);
            }
            Some(e) => {
                if let Ok(mut s) = sinks.get_mut(e) {
                    s.set_volume(Volume::Linear(frac * falloff(at.distance(ear), 3.0, 25.0) * fx));
                    s.set_speed(0.5 + 0.5 * frac);
                }
            }
            None if frac >= 0.01 => {
                if let Some(found) = lib.find(&spin.sound, i as u32, &mut sources) {
                    let e = commands.spawn((AudioPlayer::new(found.handle), PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)))).id();
                    st.fans.insert(i, e);
                }
            }
            None => {}
        }
    }
}

/// How loud an area's ambience is.
const SCAPE_GAIN: f32 = 0.7;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_fall_off_as_source_has_them() {
        // 75 dB: full within about 5 m.
        let near = near_distance(75.0);
        assert!((near - 5.14).abs() < 0.05, "{near}");
        assert_eq!(falloff(2.0, near, 30.0), 1.0);
        assert!((falloff(10.28, near, 30.0) - 0.5).abs() < 0.01);
        assert_eq!(falloff(31.0, near, 30.0), 0.0);
    }
}
