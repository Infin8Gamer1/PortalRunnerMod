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
}

pub fn setup(mut commands: Commands) {
    commands.insert_resource(PortalAudio::default());
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
    let one_shot = |commands: &mut Commands, h: Option<Handle<AudioSource>>, volume: f32, speed: f32| {
        if let Some(h) = h {
            if volume > 0.001 {
                commands.spawn((AudioPlayer::new(h), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)).with_speed(speed)));
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
            SimEvent::Checkpoint { .. } => {}
        }
    }
    for e in &game.ctrl.events {
        match *e {
            faith_move::Event::GelBounce { .. } => one_shot(&mut commands, ex.sound("gel_bounce"), 0.7 * fx, 1.0),
            faith_move::Event::Land { impact, .. } if impact > 14.0 => one_shot(&mut commands, pick(&ex, "hard_impact", n), 0.6 * fx, 1.0),
            _ => {}
        }
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
        let vol = v * fx;
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
