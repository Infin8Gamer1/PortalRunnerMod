//! The display and graphics settings, applied: window mode and size, VSync, a frame-rate
//! cap, brightness, anti-aliasing (on the player's camera and, matched to it, the portal and
//! viewmodel cameras), sharpening, the portal views' resolution and the sun's shadows.

use std::time::{Duration, Instant};

use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::Smaa;
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::camera::Exposure;
use bevy::core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass};
use bevy::light::DirectionalLightShadowMap;
use bevy::prelude::*;
use bevy::render::camera::{MipBias, TemporalJitter};
use bevy::render::view::Msaa;
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, VideoModeSelection, WindowMode};

use crate::portal_render::PortalCam;
use crate::settings::Settings;
use crate::{Game, PlayerCamera, Sun};

/// Window sizes offered in windowed mode.
pub const RESOLUTIONS: [(u32, u32); 6] = [(1280, 720), (1600, 900), (1920, 1080), (2560, 1440), (3440, 1440), (3840, 2160)];
/// Frame-rate caps offered (0: none).
pub const FPS_CAPS: [u32; 7] = [0, 30, 60, 120, 144, 165, 240];
pub const WINDOW_MODES: [&str; 3] = ["Windowed", "Borderless", "Fullscreen"];
pub const AA_MODES: [&str; 5] = ["Off", "FXAA", "SMAA", "MSAA 4x", "TAA"];
pub const SHADOWS: [&str; 4] = ["Off", "Low", "Medium", "High"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aa {
    Off,
    Fxaa,
    Smaa,
    Msaa,
    Taa,
}

impl Aa {
    pub fn from_setting(v: f32) -> Aa {
        match v.round() as i32 {
            1 => Aa::Fxaa,
            2 => Aa::Smaa,
            3 => Aa::Msaa,
            4 => Aa::Taa,
            _ => Aa::Off,
        }
    }
}

/// The overlay cameras drawn over the player's (the arms and the gun).
#[derive(Component)]
pub struct OverlayCamera;

pub struct GraphicsPlugin;

impl Plugin for GraphicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (apply, reset_taa_on_teleport)).add_systems(Last, frame_cap);
    }
}

/// What was last applied, to apply only what changed.
#[derive(Default)]
struct Applied {
    window: Option<(i32, i32, bool)>,
    cameras: Option<(Aa, i32, i32)>,
    shadows: Option<(i32, bool)>,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn apply(
    mut commands: Commands,
    settings: Res<Settings>,
    game: Res<Game>,
    mut applied: Local<Applied>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    main: Query<Entity, With<PlayerCamera>>,
    portals: Query<Entity, With<PortalCam>>,
    overlays: Query<Entity, (With<OverlayCamera>, Without<PlayerCamera>)>,
    new_cameras: Query<(), Added<Camera>>,
    mut sun: Query<&mut DirectionalLight, With<Sun>>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
) {
    // ---- the window
    let novsync = std::env::var_os("PORTAL_NOVSYNC").is_some();
    let window = (settings.window_mode.round() as i32, settings.resolution.round() as i32, settings.vsync > 0.5 && !novsync);
    if applied.window != Some(window) {
        if let Ok(mut w) = windows.single_mut() {
            let first = applied.window.is_none();
            w.mode = match window.0 {
                1 => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                2 => WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current),
                _ => WindowMode::Windowed,
            };
            // The size is only ours to set when windowed (and not at startup when it's the
            // default, so a window manager's own placement stands).
            if window.0 == 0 && (!first || window.1 != 1) {
                let (x, y) = RESOLUTIONS[window.1.clamp(0, RESOLUTIONS.len() as i32 - 1) as usize];
                w.resolution.set(x as f32, y as f32);
            }
            w.present_mode = if window.2 { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
            applied.window = Some(window);
        }
    }

    // ---- the cameras
    let aa = Aa::from_setting(settings.anti_aliasing);
    let sharpen = (settings.sharpening * 4.0).round() as i32;
    let ev = (settings.brightness * 4.0).round() as i32;
    let cams = (aa, sharpen, ev);
    if applied.cameras != Some(cams) || !new_cameras.is_empty() {
        applied.cameras = Some(cams);
        // Brighter is a lower exposure value.
        let exposure = Exposure { ev100: Exposure::EV100_BLENDER - settings.brightness };
        for e in &main {
            let mut ec = commands.entity(e);
            ec.remove::<(Fxaa, Smaa, TemporalAntiAliasing, TemporalJitter, MipBias, DepthPrepass, MotionVectorPrepass, ContrastAdaptiveSharpening)>();
            ec.insert((if aa == Aa::Msaa { Msaa::Sample4 } else { Msaa::Off }, exposure));
            match aa {
                Aa::Fxaa => {
                    ec.insert(Fxaa::default());
                }
                Aa::Smaa => {
                    ec.insert(Smaa::default());
                }
                Aa::Taa => {
                    ec.insert(TemporalAntiAliasing::default());
                }
                Aa::Off | Aa::Msaa => {}
            }
            if settings.sharpening > 0.01 {
                ec.insert(ContrastAdaptiveSharpening { enabled: true, sharpening_strength: settings.sharpening, denoise: false });
            }
        }
        // The portal views are drawn into the player's view before its post-processing, so
        // FXAA, SMAA and TAA smooth them too; with MSAA they need their own.
        for e in &portals {
            commands.entity(e).insert((if aa == Aa::Msaa { Msaa::Sample4 } else { Msaa::Off }, exposure));
        }
        // The arms and the gun draw over the finished picture: always multisampled.
        for e in &overlays {
            commands.entity(e).insert((Msaa::Sample4, exposure));
        }
    }

    // ---- the sun's shadows (outdoors; indoors the lamps light the chambers)
    let quality = settings.shadows.round() as i32;
    let outdoor = !game.sim.level.indoor;
    if applied.shadows != Some((quality, outdoor)) {
        applied.shadows = Some((quality, outdoor));
        shadow_map.size = match quality {
            1 => 1024,
            3 => 4096,
            _ => 2048,
        };
        for mut l in &mut sun {
            l.shadow_maps_enabled = outdoor && quality > 0;
        }
    }
}

/// Through a portal the view jumps: TAA's history would smear the old view over the new.
fn reset_taa_on_teleport(game: Res<Game>, mut taa: Query<&mut TemporalAntiAliasing, With<PlayerCamera>>) {
    if game.sim.events.iter().any(|e| matches!(e, aperture::SimEvent::Teleported { .. })) {
        for mut t in &mut taa {
            t.reset = true;
        }
    }
}

/// Hold each frame to the frame-rate cap, if there is one.
fn frame_cap(settings: Res<Settings>, mut last: Local<Option<Instant>>) {
    let cap = FPS_CAPS[(settings.fps_cap.round() as usize).min(FPS_CAPS.len() - 1)];
    let now = Instant::now();
    if cap > 0 {
        if let Some(prev) = *last {
            let target = Duration::from_secs_f64(1.0 / cap as f64);
            let due = prev + target;
            if due > now {
                // Sleep most of the way, then spin the last bit (sleep overshoots).
                let left = due - now;
                if left > Duration::from_millis(2) {
                    std::thread::sleep(left - Duration::from_millis(1));
                }
                while Instant::now() < due {
                    std::hint::spin_loop();
                }
                *last = Some(due);
                return;
            }
        }
    }
    *last = Some(now);
}
