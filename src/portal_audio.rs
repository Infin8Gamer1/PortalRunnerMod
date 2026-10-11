//! Portal's sounds (from your own copies of Portal and Portal 2, see `extracted`): the gun,
//! portals opening and fizzling, going through them, buttons, doors, cubes, faith plates,
//! lasers, bridges and gel, plus the hum of a test chamber and a little music.

use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume};
use bevy::prelude::*;

use aperture::{Paint, PortalColor, SimEvent};

use crate::extracted::Extracted;
use crate::{Game, PlayerCamera};

#[derive(Resource, Default)]
pub struct PortalAudio {
    /// Looping sounds by name, and the entity playing each.
    loops: Vec<(&'static str, Entity)>,
    /// The chamber's music (None outdoors, where Mirror's Edge's plays), once chosen.
    music: Option<Option<Entity>>,
    level: usize,
    count: u64,
    /// The last time a cube hit something (so a pile of cubes doesn't machine-gun).
    last_hit: f32,
    time: f32,
    /// When each sound last started, so one that fires many times a frame (gel splats,
    /// cubes piling up) plays once rather than stacking into a roar.
    recent: std::collections::HashMap<AssetId<AudioSource>, f32>,
    /// GLaDOS's last line played: (the level load it was in, its count).
    voice: (u64, u32),
    /// Turret shots heard (every other one plays).
    shots: u64,
}

/// The same sound doesn't start again within this long (s).
const RETRIGGER: f32 = 0.06;

/// Sounds a map prop's animation makes, as its model has them (Source's AE_CL_PLAYSOUND events:
/// a sound at a point through the clip, and where one's stopped), due at the simulation's time.
#[derive(Resource, Default)]
pub struct PropSounds(Vec<PropSound>);

struct PropSound {
    due: f32,
    sound: &'static str,
    at: Vec3,
    volume: f32,
    /// When it's cut off, if it is.
    stop: Option<f32>,
}

/// A sound playing that's cut off at a time (the simulation's).
#[derive(Component)]
pub struct StopAt(f32);

/// Each model's animations' sounds: (clip, sound, volume, when, stopped when; times are
/// fractions of the clip).
const ANIMATION_SOUNDS: &[(&str, &[(&str, &str, f32, f32, Option<f32>)])] = &[
    // Portal's round elevator doors: the doors moving, and the thunk as they meet or part.
    ("props/round_elevator_doors", &[
        ("open", "elevator_door_move", 0.8, 0.105, Some(0.947)),
        ("open", "elevator_door_stop", 0.8, 0.789, None),
        ("open", "elevator_door_stop", 0.8, 0.842, None),
        ("close", "elevator_door_move", 0.8, 0.105, Some(0.947)),
        ("close", "elevator_door_stop", 0.8, 0.368, None),
    ]),
    // A cube dropper's cover.
    ("props/box_dropper_cover", &[("open", "dropper_cover_open", 0.7, 0.105, None), ("close", "dropper_cover_close", 0.7, 0.105, None)]),
];

impl PropSounds {
    /// Model `model` starts clip `clip` (`len` s long) at `now`, at `at`.
    pub fn animation(&mut self, model: &str, clip: &str, now: f32, len: f32, at: Vec3) {
        for (m, events) in ANIMATION_SOUNDS {
            if !model.ends_with(m) {
                continue;
            }
            for &(_, sound, volume, when, stop) in events.iter().filter(|e| e.0 == clip) {
                self.0.push(PropSound { due: now + when * len, sound, at, volume, stop: stop.map(|s| now + s * len) });
            }
        }
    }
}

pub fn setup(mut commands: Commands) {
    commands.insert_resource(PortalAudio::default());
    commands.insert_resource(PropSounds::default());
}

fn pick(ex: &Extracted, prefix: &str, n: u64) -> Option<Handle<AudioSource>> {
    let v = ex.sounds_like(prefix);
    if v.is_empty() {
        return None;
    }
    Some(v[(n as usize) % v.len()].clone())
}

#[allow(clippy::too_many_arguments)]
pub fn play(
    mut commands: Commands,
    time: Res<Time>,
    game: Res<Game>,
    ex: Res<Extracted>,
    mut st: ResMut<PortalAudio>,
    settings: Res<crate::settings::Settings>,
    mut sinks: Query<&mut AudioSink>,
    camera: Query<&GlobalTransform, With<PlayerCamera>>,
    mut prop_sounds: ResMut<PropSounds>,
    stopping: Query<(Entity, &StopAt)>,
) {
    if ex.sounds.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    st.time += dt;
    let fx = settings.effects_gain();
    let ear = camera.single().map(|t| t.translation()).unwrap_or(game.ctrl.feet);
    let sim = &game.sim;
    // Distance falloff by hand (the listener moves through portals).
    let at_volume = |p: Vec3, v: f32| v * fx * (1.0 / (1.0 + (p.distance(ear) / 8.0).powi(2))).max(0.08);
    let mut plays: Vec<(Handle<AudioSource>, f32, f32)> = vec![];
    let mut one_shot = |_: &mut Commands, h: Option<Handle<AudioSource>>, volume: f32, speed: f32| {
        if let Some(h) = h {
            if volume > 0.001 {
                plays.push((h, volume, speed));
            }
        }
    };
    let n = {
        st.count += 1;
        st.count
    };
    for e in &sim.events {
        match *e {
            SimEvent::Shot { color, .. } => {
                let name = if color == PortalColor::Blue { "gun_blue" } else { "gun_orange" };
                one_shot(&mut commands, ex.sound(name), 0.55 * fx, 1.0);
            }
            SimEvent::PortalOpened { at, .. } => one_shot(&mut commands, pick(&ex, "portal_open", n), at_volume(at, 0.8), 1.0),
            SimEvent::PortalFailed { at, .. } => one_shot(&mut commands, ex.sound("portal_invalid"), at_volume(at, 0.8), 1.0),
            SimEvent::PortalsCleared => one_shot(&mut commands, ex.sound("portal_fizzle"), 0.7 * fx, 1.0),
            SimEvent::PortalFizzled { at, .. } => one_shot(&mut commands, ex.sound("portal_fizzle"), at_volume(at, 0.7), 1.0),
            SimEvent::Teleported { .. } => {
                one_shot(&mut commands, pick(&ex, "portal_enter", n), 0.5 * fx, 1.0);
                if game.ctrl.vel.length() > 12.0 {
                    one_shot(&mut commands, ex.sound("fling_whoosh"), 0.6 * fx, 1.0);
                }
            }
            SimEvent::PropTeleported { at, .. } => one_shot(&mut commands, pick(&ex, "portal_exit", n), at_volume(at, 0.4), 1.1),
            SimEvent::ButtonDown { at } => one_shot(&mut commands, ex.sound("button_down"), at_volume(at, 0.8), 1.0),
            SimEvent::ButtonUp { at } => one_shot(&mut commands, ex.sound("button_up"), at_volume(at, 0.7), 1.0),
            SimEvent::PedestalPressed { at } => one_shot(&mut commands, ex.sound("switch_press"), at_volume(at, 0.8), 1.0),
            SimEvent::DoorOpening { at } => one_shot(&mut commands, ex.sound("door_open"), at_volume(at, 0.7), 1.0),
            SimEvent::DoorClosing { at } => one_shot(&mut commands, ex.sound("door_close"), at_volume(at, 0.7), 1.0),
            SimEvent::CubeDropped { at } => one_shot(&mut commands, ex.sound("dropper_open"), at_volume(at, 0.8), 1.0),
            SimEvent::CubeFizzled { at } => one_shot(&mut commands, ex.sound("cube_fizzle"), at_volume(at, 0.8), 1.0),
            SimEvent::CubeLanded { at, speed } => {
                if st.time - st.last_hit > 0.08 {
                    st.last_hit = st.time;
                    let bouncy = sim.props.iter().any(|p| p.pos.distance(at) < 0.1 && p.coat == Some(Paint::Bounce));
                    let h = if bouncy { pick(&ex, "cube_bounce", n) } else { pick(&ex, "cube_hit", n) };
                    one_shot(&mut commands, h, at_volume(at, (speed / 10.0).clamp(0.2, 1.0)), 1.0);
                }
            }
            SimEvent::PickUp { .. } => one_shot(&mut commands, ex.sound("pickup"), 0.5 * fx, 1.0),
            SimEvent::Drop { .. } => one_shot(&mut commands, ex.sound("drop"), 0.5 * fx, 1.0),
            SimEvent::PlateLaunch { at } => {
                one_shot(&mut commands, ex.sound("plate_launch"), at_volume(at, 1.0), 1.2);
                one_shot(&mut commands, ex.sound("fling_whoosh"), 0.6 * fx, 1.0);
            }
            SimEvent::GelSplat { at, .. } => one_shot(&mut commands, pick(&ex, "gel_splat", n), at_volume(at, 0.35), 1.0),
            SimEvent::CatcherOn { at } => one_shot(&mut commands, ex.sound("catcher_on"), at_volume(at, 0.9), 1.0),
            // Turrets: their voice ("There you are", "Searching", "Critical error"...), and
            // their guns (every other shot: they fire ten a second).
            SimEvent::TurretSpotted { at } => {
                one_shot(&mut commands, pick(&ex, "turret_deploy_", n), at_volume(at, 0.6), 1.0);
                one_shot(&mut commands, pick(&ex, "turret_active_", n), at_volume(at, 0.9), 1.0);
            }
            SimEvent::TurretSearching { at } => one_shot(&mut commands, pick(&ex, "turret_search_", n), at_volume(at, 0.9), 1.0),
            // (Every other shot, counted by shots: the frame count went in step with them and
            // could leave every shot on an odd frame, silent.)
            SimEvent::TurretShot { from, .. } => {
                st.shots += 1;
                if st.shots % 2 == 0 {
                    one_shot(&mut commands, pick(&ex, "turret_shoot_", st.shots / 2), at_volume(from, 0.35), 1.0);
                }
            }
            SimEvent::TurretTipped { at } => one_shot(&mut commands, pick(&ex, "turret_tipped_", n), at_volume(at, 0.9), 1.0),
            // Picked up, it protests; put down, it looks about again.
            SimEvent::TurretPickedUp { at } => one_shot(&mut commands, pick(&ex, "turret_pickup_", n), at_volume(at, 0.9), 1.0),
            SimEvent::TurretPutDown { at } => one_shot(&mut commands, pick(&ex, "turret_autosearch_", n), at_volume(at, 0.9), 1.0),
            SimEvent::TurretRetired { at } => {
                one_shot(&mut commands, pick(&ex, "turret_retire_", n), at_volume(at, 0.9), 1.0);
                one_shot(&mut commands, ex.sound("turret_die"), at_volume(at, 0.5), 1.0);
            }
            SimEvent::PelletLaunched { at } => one_shot(&mut commands, ex.sound("pellet_launch"), at_volume(at, 0.6), 1.0),
            SimEvent::PelletBounced { at, .. } => one_shot(&mut commands, pick(&ex, "pellet_bounce", n), at_volume(at, 0.7), 1.0),
            SimEvent::PelletGone { at, caught } => one_shot(&mut commands, ex.sound(if caught { "pellet_caught" } else { "pellet_gone" }), at_volume(at, 0.8), 1.0),
            // The escape's rocket turret: its beep as it locks on, the launch, the blast.
            SimEvent::RocketLocking { at } => one_shot(&mut commands, ex.sound("rocket_locking"), at_volume(at, 0.8), 1.0),
            SimEvent::RocketFired { at } => one_shot(&mut commands, ex.sound("rocket_fire"), at_volume(at, 1.0), 1.0),
            SimEvent::RocketExploded { at, .. } => one_shot(&mut commands, pick(&ex, "rocket_explode", n), at_volume(at, 1.0), 1.0),
            SimEvent::CatcherOff { at } => one_shot(&mut commands, ex.sound("catcher_off"), at_volume(at, 0.7), 1.0),
            SimEvent::LaserBurn => {
                if n % 20 == 0 {
                    one_shot(&mut commands, pick(&ex, "laser_burn", n / 20), 0.6 * fx, 1.0);
                }
            }
            SimEvent::LevelComplete => {
                one_shot(&mut commands, ex.sound("chamber_solved"), 0.8 * fx, 1.0);
                one_shot(&mut commands, ex.sound("elevator_chime"), 0.7 * fx, 1.0);
            }
            SimEvent::PlayerDied => one_shot(&mut commands, ex.sound("chamber_fail"), 0.6 * fx, 1.0),
            SimEvent::Checkpoint { .. } | SimEvent::MoverAt { .. } | SimEvent::MoverUsed { .. } => {}
        }
    }
    // GLaDOS, over the speakers (not from anywhere in particular).
    if let Some(io) = &sim.io {
        let heard = if st.voice.0 == game.loads { st.voice.1 } else { 0 };
        for (k, sound) in io.voice.iter().filter(|(k, _)| *k > heard) {
            one_shot(&mut commands, ex.sound(sound), 1.0 * fx, 1.0);
            st.voice = (game.loads, *k);
        }
    }
    for e in &game.ctrl.events {
        match *e {
            faith_move::Event::GelBounce { .. } => one_shot(&mut commands, ex.sound("gel_bounce"), 0.7 * fx, 1.0),
            faith_move::Event::Land { impact, .. } if impact > 14.0 => one_shot(&mut commands, pick(&ex, "hard_impact", n), 0.6 * fx, 1.0),
            _ => {}
        }
    }

    // Props' animations' sounds, as they come due; and cut off where they're stopped. (What's
    // far off in time is from before a map loaded and its clock started again.)
    let now = sim.time;
    for (e, s) in &stopping {
        if now >= s.0 || s.0 - now > 30.0 {
            commands.entity(e).despawn();
        }
    }
    let (due, later): (Vec<PropSound>, Vec<PropSound>) = std::mem::take(&mut prop_sounds.0).into_iter().partition(|s| s.due <= now);
    prop_sounds.0 = later.into_iter().filter(|s| s.due < now + 30.0).collect();
    for s in due {
        let Some(h) = ex.sound(s.sound) else { continue };
        let v = at_volume(s.at, s.volume) * ex.gain(&h);
        let mut e = commands.spawn((AudioPlayer::new(h), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(v))));
        if let Some(t) = s.stop {
            e.insert(StopAt(t));
        }
    }

    for (h, volume, speed) in plays {
        let now = st.time;
        if st.recent.get(&h.id()).is_some_and(|t| now - t < RETRIGGER) {
            continue;
        }
        st.recent.insert(h.id(), now);
        let v = volume * ex.gain(&h);
        commands.spawn((AudioPlayer::new(h), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(v)).with_speed(speed)));
    }

    // ---- loops: their volume follows what's near.
    let indoor = sim.level.indoor;
    let near_portal = sim.portals.iter().flatten().map(|p| p.center.distance(ear)).fold(f32::INFINITY, f32::min);
    let laser_near = sim.lasers.iter().filter(|l| l.on).flat_map(|l| l.path.iter()).map(|(a, b)| aperture::geom::segment_point(*a, *b, ear).0).fold(f32::INFINITY, f32::min);
    let bridge_near = sim.bridges.iter().flat_map(|b| b.segs.iter()).map(|s| aperture::geom::segment_point(s.a, s.b, ear).0).fold(f32::INFINITY, f32::min);
    let pipe_near = sim.dispensers.iter().filter(|d| d.on).map(|d| d.at.distance(ear)).fold(f32::INFINITY, f32::min);
    let fall = if matches!(game.ctrl.state, faith_move::State::Air) { ((game.ctrl.vel.length() - 10.0) / 15.0).clamp(0.0, 1.0) } else { 0.0 };
    let near = |d: f32, r: f32| (1.0 - d / r).clamp(0.0, 1.0).powi(2);
    let wants: [(&'static str, f32); 7] = [
        ("room_hum", if indoor { 0.05 } else { 0.0 }),
        ("portal_ambient", near(near_portal, 6.0) * 0.5),
        ("laser_hum", near(laser_near, 5.0) * 0.4),
        ("bridge_hum", near(bridge_near, 6.0) * 0.35),
        ("gel_pipe", near(pipe_near, 10.0) * 0.5),
        ("hold_loop", if sim.held.is_some() { 0.25 } else { 0.0 }),
        ("fall_wind", fall * 0.6),
    ];
    for (name, v) in wants {
        let vol = v * fx * ex.sound(name).map(|h| ex.gain(&h)).unwrap_or(1.0);
        let existing = st.loops.iter().find(|(n, _)| *n == name).map(|(_, e)| *e);
        match existing {
            Some(e) => {
                if let Ok(mut s) = sinks.get_mut(e) {
                    s.set_volume(Volume::Linear(vol));
                }
            }
            None => {
                if let Some(h) = ex.sound(name) {
                    let e = commands.spawn((AudioPlayer::new(h), PlaybackSettings::LOOP.with_volume(Volume::Linear(vol)))).id();
                    st.loops.push((name, e));
                }
            }
        }
    }

    // ---- chamber music: quiet, a track per chamber, none outdoors.
    let music_gain = settings.music_gain();
    if st.level != game.level_index || st.music.is_none() {
        st.level = game.level_index;
        if let Some(Some(e)) = st.music.take() {
            if let Ok(mut ec) = commands.get_entity(e) {
                ec.despawn();
            }
        }
        let tracks = ex.sounds_like("music_chamber");
        st.music = Some(if indoor && !tracks.is_empty() {
            let k = game.level_index % tracks.len();
            Some(commands.spawn((AudioPlayer::new(tracks[k].clone()), PlaybackSettings::LOOP.with_volume(Volume::Linear(0.25 * music_gain)))).id())
        } else {
            // Outdoors Mirror's Edge's own music plays (audio.rs).
            None
        });
    }
    if let Some(Some(e)) = st.music {
        if let Ok(mut s) = sinks.get_mut(e) {
            s.set_volume(Volume::Linear(0.25 * music_gain));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevator_doors_sound_as_they_move() {
        let mut s = PropSounds::default();
        s.animation("map/props/round_elevator_doors", "close", 10.0, 2.0, Vec3::ZERO);
        let got: Vec<(&str, f32, Option<f32>)> = s.0.iter().map(|p| (p.sound, p.due, p.stop)).collect();
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].0 == "elevator_door_move" && (got[0].1 - 10.21).abs() < 1e-3 && got[0].2.is_some_and(|t| (t - 11.894).abs() < 1e-3), "{got:?}");
        assert!(got[1].0 == "elevator_door_stop" && (got[1].1 - 10.736).abs() < 1e-3, "{got:?}");
        // Other models' clips make none.
        s.animation("map/props/round_elevator_body", "close", 10.0, 2.0, Vec3::ZERO);
        assert_eq!(s.0.len(), 2);
    }
}
