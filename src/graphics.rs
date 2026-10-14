//! The display and graphics settings, applied: window mode and size, VSync, a frame-rate
//! cap, brightness, anti-aliasing (TAA, or DLSS: on the player's camera and, matched to it,
//! the portal and viewmodel cameras), sharpening, HDR with bloom and auto exposure, and the
//! portal views' resolution. The lighting's traced (`raytrace`): it makes its own shadows and
//! occlusion, so there are no shadow maps or SSAO, and TAA (or DLSS) is what smooths its grain
//! over frames.

use std::time::{Duration, Instant};

use bevy::anti_alias::contrast_adaptive_sharpening::ContrastAdaptiveSharpening;
use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::camera::{CameraMainTextureUsages, Exposure, Hdr};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::math::cubic_splines::LinearSpline;
use bevy::post_process::auto_exposure::{AutoExposure, AutoExposureCompensationCurve, AutoExposurePlugin};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::camera::{MipBias, TemporalJitter};
use bevy::render::render_resource::TextureUsages;
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
/// DLSS settings, and how much of the window's resolution each renders at.
pub const DLSS_MODES: [(&str, f32); 6] = [("Off", 1.0), ("DLAA", 1.0), ("Quality", 0.667), ("Balanced", 0.58), ("Performance", 0.5), ("Ultra perf.", 0.333)];

/// Whether DLSS is usable: built in, and this GPU has it (set once the renderer's up). And
/// whether it has Ray Reconstruction, which with ray-traced lighting denoises it as well.
static DLSS_OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DLSS_RR_OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// DLSS Ray Reconstruction is in use: DLSS on, on a GPU that has it (and traces the light).
fn ray_reconstruction(settings: &Settings) -> bool {
    dlss_on(settings) && crate::raytrace::supported() && DLSS_RR_OK.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn dlss_label(v: f32) -> String {
    let name = DLSS_MODES[(v as usize).min(DLSS_MODES.len() - 1)].0;
    if v < 0.5 {
        name.to_string()
    } else if !cfg!(feature = "dlss") {
        format!("{name} (not built in)")
    } else if !DLSS_OK.load(std::sync::atomic::Ordering::Relaxed) {
        format!("{name} (no GPU support)")
    } else if crate::raytrace::supported() && DLSS_RR_OK.load(std::sync::atomic::Ordering::Relaxed) {
        format!("{name} + Ray Reconstruction")
    } else {
        name.to_string()
    }
}

/// The fraction of the window's resolution the scene renders at (DLSS upscales the rest).
pub fn render_scale(settings: &Settings) -> f32 {
    if dlss_on(settings) { DLSS_MODES[(settings.dlss as usize).min(DLSS_MODES.len() - 1)].1 } else { 1.0 }
}

fn dlss_on(settings: &Settings) -> bool {
    settings.dlss >= 0.5 && DLSS_OK.load(std::sync::atomic::Ordering::Relaxed)
}

/// Auto exposure: how far it may turn the picture down and up (stops), as Portal's maps let
/// theirs (env_tonemap_controller: 0.75x to 1.5x). (These and the reference are exact in
/// binary: Bevy's curve wants its segments' ends to meet exactly.)
const AUTO_EXPOSURE_RANGE: (f32, f32) = (-0.4375, 0.5625);
/// Mirror's Edge's levels: as far down, and up to 3 stops brighter, as its own eye adaptation
/// opens up indoors (its rooms are lit for that; traced, a room lit only by its lamps is far
/// darker than the sunlit roofs).
const ME_AUTO_EXPOSURE_RANGE: (f32, f32) = (-0.4375, 3.0);
/// The view's brightness (log2 of its average luminance, metered as Bevy does) that's left
/// as it is: a chamber as bright as Portal's first ones. Brighter is turned down, darker up.
const AUTO_EXPOSURE_REFERENCE: f32 = -1.625;
/// How fast it adjusts (stops a second).
const AUTO_EXPOSURE_SPEED: f32 = 1.0;

/// (PORTAL_AE_REF overrides the reference, for calibrating it.)
fn auto_exposure_reference() -> f32 {
    std::env::var("PORTAL_AE_REF").ok().and_then(|v| v.parse().ok()).unwrap_or(AUTO_EXPOSURE_REFERENCE)
}

/// The compensation for a metered brightness `l` (log2), as the curve Bevy's auto exposure
/// takes: it aims for `curve(l) - l` stops, which is the way back to `reference`, held within
/// `AUTO_EXPOSURE_RANGE`.
fn exposure_curve(reference: f32, (down, up): (f32, f32)) -> AutoExposureCompensationCurve {
    let points = [Vec2::new(-8.0, -8.0 + up), Vec2::new(reference - up, reference), Vec2::new(reference - down, reference), Vec2::new(8.0, 8.0 + down)];
    AutoExposureCompensationCurve::from_curve(LinearSpline::new(points)).expect("a valid exposure curve")
}

/// The overlay cameras drawn over the player's (the arms and the gun).
#[derive(Component)]
pub struct OverlayCamera;

pub struct GraphicsPlugin;

impl Plugin for GraphicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(AutoExposurePlugin).add_systems(Update, (apply, map_tone.after(apply))).add_systems(PostUpdate, carry_previous_view).add_systems(Last, frame_cap);
        #[cfg(feature = "dlss")]
        app.add_systems(
            PreUpdate,
            |ok: Option<Res<bevy::anti_alias::dlss::DlssSuperResolutionSupported>>, rr: Option<Res<bevy::anti_alias::dlss::DlssRayReconstructionSupported>>, mut told: Local<bool>| {
                DLSS_OK.store(ok.is_some(), std::sync::atomic::Ordering::Relaxed);
                DLSS_RR_OK.store(rr.is_some(), std::sync::atomic::Ordering::Relaxed);
                if !*told {
                    *told = true;
                    info!("DLSS: super resolution {}, ray reconstruction {}", if ok.is_some() { "available" } else { "not available" }, if rr.is_some() { "available" } else { "not available" });
                    if ok.is_none() {
                        warn!("DLSS didn't start: it needs an NVIDIA RTX GPU on Vulkan, and its libraries in $DLSS_SDK/lib (now {:?})", std::env::var_os("DLSS_SDK"));
                    }
                }
            },
        );
    }
}

/// What was last applied, to apply only what changed.
#[derive(Default)]
struct Applied {
    window: Option<(i32, i32, bool)>,
    cameras: Option<(i32, i32)>,
    sun: bool,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn apply(
    mut commands: Commands,
    settings: Res<Settings>,
    mut applied: Local<Applied>,
    mut windows: Query<(&mut Window, Option<&bevy::window::OnMonitor>), With<PrimaryWindow>>,
    main: Query<Entity, With<PlayerCamera>>,
    portals: Query<Entity, With<PortalCam>>,
    overlays: Query<Entity, (With<OverlayCamera>, Without<PlayerCamera>)>,
    new_cameras: Query<(), Added<Camera>>,
    mut sun: Query<&mut DirectionalLight, With<Sun>>,
    mut curves: ResMut<Assets<AutoExposureCompensationCurve>>,
    mut curve: Local<Option<Handle<AutoExposureCompensationCurve>>>,
) {
    // ---- the window
    let novsync = std::env::var_os("PORTAL_NOVSYNC").is_some();
    // (PORTAL_WINDOW_MODE=0/1/2 in place of the setting: for tests.)
    let mode = std::env::var("PORTAL_WINDOW_MODE").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(settings.window_mode);
    let window = (mode.round() as i32, settings.resolution.round() as i32, settings.vsync > 0.5 && !novsync);
    if applied.window != Some(window) {
        if let Ok((mut w, on)) = windows.single_mut() {
            let first = applied.window.is_none();
            // The monitor it's on, if that's known (asking for "the current one" panics when
            // the window system won't say, as on Wayland at times); else the first.
            let monitor = on.map(|m| MonitorSelection::Entity(m.0)).unwrap_or(MonitorSelection::Index(0));
            let mode = match window.0 {
                1 => WindowMode::BorderlessFullscreen(monitor),
                2 => WindowMode::Fullscreen(monitor, VideoModeSelection::Current),
                _ => WindowMode::Windowed,
            };
            // (Already that kind of window (it started as one): left as it is, not switched.)
            if std::mem::discriminant(&w.mode) != std::mem::discriminant(&mode) {
                w.mode = mode;
            }
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
    let sharpen = (settings.sharpening * 4.0).round() as i32;
    let ev = (settings.brightness * 4.0).round() as i32;
    let bloom = (settings.bloom * 10.0).round() as i32;
    let dlss = if dlss_on(&settings) { settings.dlss as i32 } else { 0 };
    let rr = ray_reconstruction(&settings);
    let auto = settings.auto_exposure > 0.5;
    let cams = (sharpen, ev * 100000 + bloom * 100 + dlss * 2 + rr as i32 + (auto as i32) * 10_000_000);
    if applied.cameras != Some(cams) || !new_cameras.is_empty() {
        applied.cameras = Some(cams);
        // Brighter is a lower exposure value.
        let exposure = Exposure { ev100: Exposure::EV100_BLENDER - settings.brightness };
        for e in &main {
            let mut ec = commands.entity(e);
            ec.remove::<(TemporalAntiAliasing, TemporalJitter, MipBias, ContrastAdaptiveSharpening, Bloom, AutoExposure)>();
            if auto {
                let curve = curve.get_or_insert_with(|| curves.add(exposure_curve(auto_exposure_reference(), AUTO_EXPOSURE_RANGE))).clone();
                ec.insert(AutoExposure { speed_brighten: AUTO_EXPOSURE_SPEED, speed_darken: AUTO_EXPOSURE_SPEED, compensation_curve: curve, ..default() });
            }
            // HDR, tonemapped once here (the portal views arrive linear), with bloom.
            ec.insert((Msaa::Off, exposure, Hdr, Tonemapping::TonyMcMapface));
            if settings.bloom > 0.0 {
                ec.insert(Bloom { intensity: 0.15 * settings.bloom, ..Bloom::NATURAL });
            }
            // DLSS does the anti-aliasing itself; else TAA, which also smooths the traced
            // light's grain over frames.
            if dlss == 0 {
                ec.insert(TemporalAntiAliasing::default());
            }
            #[cfg(feature = "dlss")]
            {
                use bevy::anti_alias::dlss::{Dlss, DlssPerfQualityMode, DlssRayReconstructionFeature};
                ec.remove::<(Dlss, Dlss<DlssRayReconstructionFeature>)>();
                if dlss > 0 {
                    let perf_quality_mode = match dlss {
                        1 => DlssPerfQualityMode::Dlaa,
                        2 => DlssPerfQualityMode::Quality,
                        3 => DlssPerfQualityMode::Balanced,
                        4 => DlssPerfQualityMode::Performance,
                        _ => DlssPerfQualityMode::UltraPerformance,
                    };
                    if rr {
                        // Upscales and denoises the traced light in one (in place of TAA).
                        ec.insert(Dlss::<DlssRayReconstructionFeature> { perf_quality_mode, reset: false, _phantom_data: Default::default() });
                    } else {
                        ec.insert(Dlss { perf_quality_mode, ..default() });
                    }
                }
            }
            if settings.sharpening > 0.01 {
                ec.insert(ContrastAdaptiveSharpening { enabled: true, sharpening_strength: settings.sharpening, denoise: false });
            }
        }
        // The portal views are drawn into the player's view before its post-processing, so its
        // TAA smooths them too. They render linear HDR (no tonemapping), which the player's
        // camera tonemaps.
        for e in &portals {
            commands.entity(e).insert((Msaa::Off, exposure, Hdr, Tonemapping::None));
        }
        // The arms and the gun draw over the player's picture, into the same HDR texture
        // (a camera with a different format or sampling would start from its own, black, one
        // and copy that over everything).
        // The same texture usages too: DLSS and the traced light both add storage to the
        // player's (DLSS out of sight, in the renderer), and a camera with other usages gets a
        // texture of its own, never cleared, laid over the picture.
        let usages = CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING);
        for e in &overlays {
            commands.entity(e).insert((Msaa::Off, exposure, Hdr, Tonemapping::TonyMcMapface, usages));
        }
    }

    // ---- the sun: no shadow maps (the traced light casts its own shadows; on a GPU that can't
    // trace, the plain lighting does without).
    if !applied.sun {
        applied.sun = true;
        for mut l in &mut sun {
            l.shadow_maps_enabled = false;
        }
    }
}

/// Portal's maps' tonemap controllers (env_tonemap_controller), as their logic sets them: how
/// far the auto exposure may turn the picture down and up (Source's scales: 0.75x is down 0.42
/// stops), how fast it adjusts (1: ours), and the bloom (Portal's chambers set 0.3: ours as it
/// is). What isn't set stays as ours.
#[allow(clippy::type_complexity)]
fn map_tone(
    game: Res<Game>,
    settings: Res<Settings>,
    mut cams: Query<(Option<&mut AutoExposure>, Option<&mut Bloom>), With<PlayerCamera>>,
    mut curves: ResMut<Assets<AutoExposureCompensationCurve>>,
    mut made: Local<std::collections::HashMap<(i32, i32), Handle<AutoExposureCompensationCurve>>>,
) {
    let get = |k: &str| game.sim.io.as_ref().and_then(|io| io.tonemap.get(k)).copied();
    // (In sixteenths of a stop, as the curve wants; and as far as it can go either way.)
    let sixteenths = |v: f32| (v * 16.0).round() as i32;
    let stops = |scale: f32| sixteenths(scale.max(1e-3).log2().clamp(-9.5, 6.0));
    let range = if game.sim.level.me_art { ME_AUTO_EXPOSURE_RANGE } else { AUTO_EXPOSURE_RANGE };
    let down = get("setautoexposuremin").map_or(sixteenths(range.0), stops);
    let up = get("setautoexposuremax").map_or(sixteenths(range.1), stops).max(down + 1);
    let speed = AUTO_EXPOSURE_SPEED * get("settonemaprate").map_or(1.0, |r| r.clamp(0.05, 4.0));
    let bloom = 0.15 * settings.bloom * get("setbloomscale").map_or(1.0, |b| (b / 0.3).clamp(0.0, 4.0));
    for (auto, glow) in &mut cams {
        if let Some(mut auto) = auto {
            let curve = made.entry((down, up)).or_insert_with(|| curves.add(exposure_curve(auto_exposure_reference(), (down as f32 / 16.0, up as f32 / 16.0)))).clone();
            if auto.compensation_curve != curve {
                auto.compensation_curve = curve;
            }
            if auto.speed_brighten != speed {
                auto.speed_brighten = speed;
                auto.speed_darken = speed;
            }
        }
        if let Some(mut glow) = glow {
            if glow.intensity != bloom {
                glow.intensity = bloom;
            }
        }
    }
}

/// Through a portal, last frame's view (what motion vectors measure from) is carried through
/// it too: what's on screen now was on screen a frame ago, seen through the portal, and TAA
/// and the traced light can carry on from it rather than start over.
fn carry_previous_view(game: Res<Game>, mut cams: Query<&mut bevy::core_pipeline::prepass::PreviousViewData, With<PlayerCamera>>) {
    for e in &game.sim.events {
        let aperture::SimEvent::Teleported { entered, .. } = *e else { continue };
        let Some(link) = game.sim.link(entered) else { continue };
        let through = Mat4::from_rotation_translation(link.quat(), link.point(Vec3::ZERO));
        let back = through.inverse();
        for mut prev in &mut cams {
            prev.view_from_world *= back;
            prev.clip_from_world *= back;
            prev.world_from_clip = through * prev.world_from_clip;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_auto_exposure_curve_builds() {
        exposure_curve(AUTO_EXPOSURE_REFERENCE, AUTO_EXPOSURE_RANGE);
        exposure_curve(-6.0, AUTO_EXPOSURE_RANGE);
        exposure_curve(5.0, AUTO_EXPOSURE_RANGE);
        // As far as a map's tonemap controller can take it, either way (`map_tone`).
        exposure_curve(AUTO_EXPOSURE_REFERENCE, (-9.5, -9.4375));
        exposure_curve(AUTO_EXPOSURE_REFERENCE, (6.0, 6.0625));
        exposure_curve(AUTO_EXPOSURE_REFERENCE, (-9.5, 6.0));
    }
}
