//! Portal Runner: Mirror's Edge's movement with Portal's portal gun and puzzles.
//!
//! The movement is the `faith_move` crate (from Faith Runner) and the portals and puzzle
//! pieces are the `aperture` crate; this is the Bevy shell round them: input, rendering
//! (portals in `portal_render`, chambers in `chamber`, the gun in `gun`), sound, the HUD.

mod audio;
mod body3p;
mod capture;
mod chamber;
mod editor;
mod extracted;
mod fonts;
mod graphics;
mod grill_fx;
mod gun;
mod hud;
mod me_props;
mod me_viewmodel;
mod map_audio;
mod map_fx;
mod map_grade;
mod menu;
mod model_props;
mod placed_lights;
mod portal_audio;
mod portal_render;
mod progress;
mod raytrace;
mod props;
mod settings;
mod source_map;
mod speed_fx;
mod viewmodel;

use std::f32::consts::FRAC_PI_2;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use aperture::{Aim, Level, MapTransition, PortalColor, Sim, SimEvent};
use faith_move::greybox;
use faith_move::{CameraFx, Controller, Event as MoveEvent, Input as MoveInput, Shot, State as MoveState};

use crate::portal_render::PLAYER_LAYER;

fn main() {
    // DLSS finds its libraries through DLSS_SDK when it starts, not only when it's built: if
    // it isn't set now (a terminal that hasn't read the profile, a launcher), use where the SDK
    // was when this was built.
    #[cfg(feature = "dlss")]
    if std::env::var_os("DLSS_SDK").is_none() {
        if let Some(sdk) = option_env!("DLSS_SDK") {
            // SAFETY: first thing in main, before any other thread exists.
            unsafe { std::env::set_var("DLSS_SDK", sdk) };
        }
    }
    let mut app = App::new();
    // DLSS asks for an application id before the renderer starts (any fixed UUID).
    #[cfg(feature = "dlss")]
    app.insert_resource(bevy::anti_alias::dlss::DlssProjectId(bevy::asset::uuid::Uuid::from_u128(0x5f0e_9a1d_7c3b_4e62_a8d4_1b9c_e2f7_6031)));
    // The asset server reads the extracted models from `assets/` (next to `extracted/`).
    let asset_dir = extracted::find_root()
        .and_then(|r| r.parent().map(|p| p.to_path_buf()))
        .and_then(|p| std::fs::canonicalize(p).ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "assets".into());
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Portal Runner".into(),
                    resolution: (1600, 900).into(),
                    // PORTAL_NOVSYNC=1: uncapped, for measuring.
                    present_mode: if std::env::var_os("PORTAL_NOVSYNC").is_some() {
                        bevy::window::PresentMode::AutoNoVsync
                    } else {
                        bevy::window::PresentMode::AutoVsync
                    },
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin { file_path: asset_dir, ..default() }),
    )
    .add_plugins(portal_render::PortalRenderPlugin)
    .add_plugins(speed_fx::SpeedFxPlugin)
    .add_plugins(graphics::GraphicsPlugin)
    .add_plugins(editor::EditorPlugin)
    .add_systems(PostUpdate, props::aim_turret_guns.after(bevy::app::AnimationSystems).before(bevy::transform::TransformSystems::Propagate))
    .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
    .init_resource::<source_map::MapMats>()
    .init_resource::<source_map::Lamps>()
    .init_resource::<map_fx::FxMats>()
    .init_resource::<map_grade::Grades>()
    .insert_resource(ClearColor(Color::srgb(0.05, 0.05, 0.06)))
    .insert_resource(GlobalAmbientLight { color: Color::srgb(0.85, 0.92, 1.0), brightness: 400.0, affects_lightmapped_meshes: true })
    .add_systems(
        Startup,
        (
            extracted::setup,
            props::setup,
            fonts::setup,
            settings::setup,
            setup_world,
            editor::add_levels,
            chamber::setup,
            load_first_level,
            hud::setup,
            viewmodel::setup,
            me_viewmodel::setup,
            body3p::setup,
            me_props::setup,
            gun::setup,
            menu::setup,
            audio::setup,
            portal_audio::setup,
            map_audio::setup,
            map_fx::setup,
            capture::setup,
        )
            .chain(),
    );
    let game = (
        switch_level,
        play.run_if(editor::idle),
        speed_fx::update,
        (chamber::update, chamber::scroll_goo, (map_fx::sync, map_fx::update, map_fx::beams, map_fx::tag_cameras, map_fx::cameras, map_fx::screen, map_grade::update), source_map::sync, source_map::bulbs, source_map::move_models, source_map::move_bodies, source_map::perch_props, source_map::show_frames, source_map::show_skins, source_map::show_props, source_map::face_sprites, source_map::tag_prop_players, source_map::animate_props, grill_fx::update),
        me_props::spawn_for_level,
        model_props::spawn_for_level,
        placed_lights::spawn_for_level,
        (
            props::tag_parts,
            props::animate,
            props::tint_floor_buttons,
            props::tag_plate_players,
            props::animate_plates,
            props::tag_launcher_players,
            props::animate_launchers,
            props::animate_catchers,
            props::animate_turrets,
            chamber::scorches,
        )
            .chain(),
        viewmodel::animate,
        me_viewmodel::animate,
        body3p::animate,
        portal_roll.run_if(editor::idle),
        menu::title_camera,
        portal_render::update,
        gun::update,
        gun::tag_model,
        audio::play,
        portal_audio::play,
        map_audio::play,
        hud::update,
    )
        .chain();
    // Ray-traced lighting is chosen at startup (PORTAL_NORT=1 turns it off for this run).
    raytrace::add(&mut app, settings::Settings::load().ray_tracing > 0.5 && std::env::var_os("PORTAL_NORT").is_none());
    app.add_systems(Update, (capture::run.run_if(resource_exists::<capture::Capture>), menu::update.run_if(editor::idle), cursor_lock.run_if(editor::idle), game, editor::systems()).chain());
    app.run();
}

// ------------------------------------------------------------------ state

#[derive(Resource)]
pub struct Game {
    pub ctrl: Controller,
    pub sim: Sim,
    /// The chamber as Mirror's Edge's sounds see it (footstep surfaces, checkpoints).
    pub level: greybox::Level,
    pub levels: Vec<(String, source_map::LevelFn)>,
    /// Parallel to `levels`: the BSP name of a Portal map (or of the map a level file is the
    /// game's own copy of), or none for a built chamber.
    pub source_names: Vec<Option<String>>,
    /// Parallel to `levels`: the folder each is in (the chambers menu).
    pub groups: Vec<LevelGroup>,
    pub level_index: usize,
    /// Load this level next frame (chamber finished, or a key).
    pub pending: Option<usize>,
    /// Where to stand when `pending` is a map entered from the previous one.
    pub arrival: Option<MapTransition>,
    /// Counting down to the next chamber after finishing one.
    pub next_in: Option<f32>,
    pub locked: bool,
    /// The mouse was grabbed this frame: that click isn't a shot.
    pub just_locked: bool,
    /// The debug readout (F3).
    pub show_debug: bool,
    /// The Reaction Time hint has been shown in this chamber.
    pub reaction_hinted: bool,
    pub mouse_sens: f32,
    pub pad_sens: f32,
    /// Time in this chamber, and the best per chamber.
    pub timer: f32,
    pub last_time: Option<f32>,
    pub flash: Option<(String, f32)>,
    /// Seconds since the chamber started (its title card).
    pub intro: f32,
    /// Input supplied by a script instead of the keyboard (screenshot capture mode).
    pub scripted: Option<MoveInput>,
    pub scripted_fire: Option<PortalColor>,
    pub scripted_use: bool,
    pub fx: CameraFx,
    pub shot: Shot,
    pub look: faith_move::LookLimiter,
    pub reaction: faith_move::reaction::ReactionTime,
    /// Seconds since dying (for the fade).
    pub died: f32,
    /// Bumped each time a chamber is (re)loaded.
    pub loads: u64,
}

/// The chambers menu's folders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LevelGroup {
    /// Portal's maps: the game's own copies of them (level files in `levels/portal/`), to play
    /// and to edit.
    Portal,
    /// The game's own chambers and the level files made in the editor.
    Testing,
    /// The editor's own level (not in the menu).
    Editor,
}

impl LevelGroup {
    pub const MENU: [LevelGroup; 2] = [LevelGroup::Portal, LevelGroup::Testing];

    pub fn name(self) -> &'static str {
        match self {
            LevelGroup::Portal => "Portal 1 chambers",
            LevelGroup::Testing => "Testing chambers",
            LevelGroup::Editor => "Editor",
        }
    }

    /// What's under it, for its folder in the menu.
    pub fn about(self) -> &'static str {
        match self {
            LevelGroup::Portal => "Portal's chambers: open them in the editor to change them",
            LevelGroup::Testing => "The game's own chambers and the levels made in the editor",
            LevelGroup::Editor => "",
        }
    }
}

#[derive(Component)]
pub struct PlayerCamera;

#[derive(Component)]
pub struct Sun;

// ------------------------------------------------------------------ setup

fn setup_world(mut commands: Commands) {
    commands.insert_resource(progress::Progress::load());
    // The game's chambers (Portal's and the level files follow: `editor::add_levels`).
    let levels: Vec<(String, source_map::LevelFn)> = aperture::levels::all().into_iter().map(|(name, f)| (name.to_string(), std::sync::Arc::new(f) as source_map::LevelFn)).collect();
    let source_names = vec![None; levels.len()];
    let groups = vec![LevelGroup::Testing; levels.len()];
    let first = std::env::var("PORTAL_MAP").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0).min(levels.len() - 1);
    let level = (levels[first].1)();
    let cp = level.checkpoints[0].clone();
    let mut ctrl = Controller::new(aperture::tuning(5.3, 15.0), cp.spawn, cp.yaw);
    ctrl.state = MoveState::Ground;

    commands.spawn((
        Sun,
        DirectionalLight { illuminance: 11_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(30.0, 60.0, 20.0).looking_at(Vec3::new(0.0, 0.0, -60.0), Vec3::Y),
        // Lights the world, Faith's body and the first-person arms.
        RenderLayers::from_layers(&[0, PLAYER_LAYER, viewmodel::VIEWMODEL_LAYER, body3p::BODY_LAYER, body3p::THROUGH_LAYER]),
    ));
    commands.spawn((
        PlayerCamera,
        IsDefaultUiCamera,
        Camera3d::default(),
        // Near plane 1 cm: the legs render in the world and the camera sits right above them.
        Projection::Perspective(PerspectiveProjection { fov: 70f32.to_radians(), near: 0.01, ..default() }),
        Transform::from_translation(cp.spawn + Vec3::Y * 1.7),
        RenderLayers::from_layers(&[0, PLAYER_LAYER, portal_render::SURFACE_LAYER]),
    ));
    commands.insert_resource(Game {
        ctrl,
        sim: Sim::new(level.clone()),
        level: level.greybox(),
        levels,
        source_names,
        groups,
        level_index: first,
        arrival: None,
        pending: None,
        next_in: None,
        locked: false,
        just_locked: false,
        show_debug: false,
        reaction_hinted: false,
        mouse_sens: 0.0022,
        pad_sens: 3.2,
        timer: 0.0,
        last_time: None,
        flash: None,
        intro: 0.0,
        scripted: None,
        scripted_fire: None,
        scripted_use: false,
        fx: CameraFx::default(),
        shot: Shot::default(),
        look: Default::default(),
        reaction: Default::default(),
        died: 99.0,
        loads: 0,
    });
}

fn load_first_level(
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut meshes: ResMut<Assets<Mesh>>,
    mats: Res<chamber::ChamberMats>,
    mut pools: ResMut<chamber::Pools>,
    models: Res<props::PropModels>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    camera: Query<Entity, With<PlayerCamera>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    let g = &mut *game;
    g.loads += 1;
    chamber::spawn(&mut commands, &mut meshes, &mats, &g.sim, &mut pools, &models);
    light_for(&g.sim.level, &mut sun, camera.single().ok(), &mut commands, &mut clear, &mut ambient);
    g.flash = None;
}

/// Lighting for a chamber: indoors a dim sky, lamps and soft light from above; outdoors
/// Mirror's Edge's sun, sky and haze.
fn light_for(
    level: &Level,
    sun: &mut Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    camera: Option<Entity>,
    commands: &mut Commands,
    clear: &mut ClearColor,
    ambient: &mut GlobalAmbientLight,
) {
    if let Ok((mut l, mut tf)) = sun.single_mut() {
        if level.indoor {
            l.illuminance = 2500.0;
            l.shadow_maps_enabled = false;
            *tf = Transform::from_xyz(8.0, 30.0, 14.0).looking_at(Vec3::ZERO, Vec3::Y);
        } else {
            l.illuminance = 11_000.0;
            l.shadow_maps_enabled = !raytrace::active();
            *tf = Transform::from_xyz(30.0, 60.0, 20.0).looking_at(Vec3::new(0.0, 0.0, -60.0), Vec3::Y);
        }
        l.color = Color::WHITE;
        // A sun of the level's own (the level editor's).
        if let Some(s) = &level.sun {
            l.illuminance = s.illuminance;
            l.color = Color::srgb(s.color.x, s.color.y, s.color.z);
            let dir = s.dir.normalize_or(Vec3::NEG_Y);
            let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
            *tf = Transform::from_translation(-dir * 60.0).looking_to(dir, up);
        }
    }
    if level.indoor {
        clear.0 = Color::srgb(0.04, 0.04, 0.05);
        ambient.brightness = 520.0;
        ambient.color = Color::srgb(0.92, 0.95, 1.0);
    } else {
        clear.0 = Color::srgb(0.62, 0.78, 0.93);
        ambient.brightness = 650.0;
        ambient.color = Color::srgb(0.85, 0.92, 1.0);
    }
    if let Some((c, brightness)) = level.ambient {
        ambient.color = Color::srgb(c.x, c.y, c.z);
        ambient.brightness = brightness;
    }
    // PORTAL_DEBUG_VOID=1: where nothing's drawn shows magenta (a hole in a map).
    if std::env::var_os("PORTAL_DEBUG_VOID").is_some() {
        clear.0 = Color::srgb(1.0, 0.0, 1.0);
    }
    if let Some(cam) = camera {
        if level.indoor {
            commands.entity(cam).insert(DistanceFog {
                color: Color::srgb(0.05, 0.05, 0.06),
                falloff: FogFalloff::Linear { start: 60.0, end: 160.0 },
                ..default()
            });
        } else {
            commands.entity(cam).insert(DistanceFog {
                color: Color::srgb(0.70, 0.82, 0.94),
                directional_light_color: Color::srgba(1.0, 0.95, 0.85, 0.4),
                directional_light_exponent: 20.0,
                falloff: FogFalloff::Linear { start: 60.0, end: 260.0 },
            });
        }
    }
}

/// White tile with a soft darker border: a classic greybox grid (Mirror's Edge's maps).
pub fn grid_texture() -> Image {
    const N: u32 = 128;
    let mut data = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let edge = x.min(y).min(N - 1 - x).min(N - 1 - y);
            let half = (x == N / 2 || y == N / 2) as u8;
            let v: u8 = match edge {
                0 => 150,
                1 => 185,
                _ if half == 1 => 222,
                _ => 245,
            };
            data.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let mut img = Image::new(
        Extent3d { width: N, height: N, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

// ------------------------------------------------------------------ systems

fn cursor_lock(
    mut game: ResMut<Game>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    capture: Option<Res<capture::Capture>>,
    mut menu: ResMut<menu::Menu>,
) {
    if capture.is_some() {
        return;
    }
    let Ok(mut cursor) = cursor.single_mut() else { return };
    let focused = windows.single().map(|w| w.focused).unwrap_or(true);
    let start = std::mem::take(&mut menu.start);
    let want = if menu.open || !focused {
        false
    } else if start {
        true
    } else if keys.just_pressed(KeyCode::Escape) {
        false
    } else if mouse.just_pressed(MouseButton::Left) {
        true
    } else {
        game.locked
    };
    if want != game.locked {
        game.just_locked = want;
        game.locked = want;
        cursor.visible = !want;
        cursor.grab_mode = if want { CursorGrabMode::Locked } else { CursorGrabMode::None };
    }
}

/// Load another chamber: M next, N previous, Backspace restart; or the next one after
/// finishing.
#[allow(clippy::too_many_arguments)]
fn switch_level(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<Game>,
    mut meshes: ResMut<Assets<Mesh>>,
    mats: Res<chamber::ChamberMats>,
    mut pools: ResMut<chamber::Pools>,
    models: Res<props::PropModels>,
    geom: Query<Entity, With<chamber::ChamberGeom>>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    camera: Query<Entity, With<PlayerCamera>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    settings: Res<settings::Settings>,
    mut progress: ResMut<progress::Progress>,
    menu: Res<menu::Menu>,
    editor: Res<editor::Editor>,
) {
    let n = game.levels.len();
    let mut to = game.pending.take();
    let keys_on = !menu.open && !editor.active;
    let mut keyed = false;
    if keys_on && keys.just_pressed(KeyCode::KeyM) {
        to = Some((game.level_index + 1) % n);
        keyed = true;
    }
    if keys_on && keys.just_pressed(KeyCode::KeyN) {
        to = Some((game.level_index + n - 1) % n);
        keyed = true;
    }
    if keys_on && keys.just_pressed(KeyCode::Backspace) {
        to = Some(game.level_index);
        keyed = true;
    }
    if keyed {
        game.arrival = None;
    }
    let Some(i) = to else { return };
    let arrival = game.arrival.take();
    for e in &geom {
        commands.entity(e).despawn();
    }
    let g = &mut *game;
    g.level_index = i;
    let level = (g.levels[i].1)();
    g.sim = Sim::new(level.clone());
    g.sim.funnel = settings.funnel > 0.5;
    g.sim.arrived = arrival.is_some();
    g.level = level.greybox();
    g.loads += 1;
    chamber::spawn(&mut commands, &mut meshes, &mats, &g.sim, &mut pools, &models);
    light_for(&level, &mut sun, camera.single().ok(), &mut commands, &mut clear, &mut ambient);
    let cp = &level.checkpoints[0];
    let (spawn, yaw) = arrival.as_ref().and_then(|tr| tr.spawn_in(&level)).unwrap_or((cp.spawn, cp.yaw));
    g.ctrl.spawn = spawn;
    g.ctrl.spawn_yaw = yaw;
    g.ctrl.respawn();
    g.ctrl.state = MoveState::Ground;
    g.timer = 0.0;
    g.last_time = None;
    g.intro = 0.0;
    g.next_in = None;
    g.flash = None;
    g.reaction_hinted = false;
    let name = g.levels[i].0.to_string();
    // (Not the editor's level: it's gone next time.)
    if progress.last.as_ref() != Some(&name) && Some(i) != editor.slot() {
        progress.last = Some(name);
        progress.save();
    }
}

#[allow(clippy::too_many_arguments)]
fn play(
    mut game: ResMut<Game>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse: Res<ButtonInput<MouseButton>>,
    pads: Query<&Gamepad>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut camera: Query<(&mut Transform, &mut Projection), With<PlayerCamera>>,
    settings: Res<settings::Settings>,
    menu: Res<menu::Menu>,
    mut progress: ResMut<progress::Progress>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut render: ResMut<portal_render::PortalRender>,
    editor: Res<editor::Editor>,
) {
    let dt = time.delta_secs().min(0.05);
    let game = &mut *game;
    game.fx.settings.fov_base = settings.fov;
    game.ctrl.tuning.hard_land_height = settings.hard_land;
    game.ctrl.tuning.lethal_fall_height = settings.lethal_height();
    game.ctrl.tuning.skid_after_speed_gel = settings.gel_skid_delay;
    game.sim.funnel = settings.funnel > 0.5;
    render.depth = (settings.portal_depth.round() as usize).clamp(1, portal_render::MAX_DEPTH);
    render.scale = settings.portal_resolution * graphics::render_scale(&settings);

    // ---- gather input
    let mut input = MoveInput::default();
    let mut mv = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) { mv.y += 1.0; }
    if keys.pressed(KeyCode::KeyS) { mv.y -= 1.0; }
    if keys.pressed(KeyCode::KeyD) { mv.x += 1.0; }
    if keys.pressed(KeyCode::KeyA) { mv.x -= 1.0; }
    let crouch_keys = [KeyCode::ShiftLeft, KeyCode::KeyC, KeyCode::ControlLeft];
    input.jump_pressed = keys.just_pressed(KeyCode::Space);
    input.jump_held = keys.pressed(KeyCode::Space);
    input.crouch_pressed = crouch_keys.iter().any(|k| keys.just_pressed(*k));
    input.crouch_held = crouch_keys.iter().any(|k| keys.pressed(*k));
    input.turn_pressed = keys.just_pressed(KeyCode::KeyQ);
    // Kick / punch / barge is F (the mouse buttons are the portal gun).
    input.melee_pressed = keys.just_pressed(KeyCode::KeyF);
    let clicks = game.locked && !game.just_locked;
    let mut fire_blue = clicks && mouse.just_pressed(MouseButton::Left);
    let mut fire_orange = clicks && mouse.just_pressed(MouseButton::Right);
    let mut use_key = keys.just_pressed(KeyCode::KeyE);
    game.just_locked = false;
    if game.locked {
        let d = mouse_motion.delta;
        input.look = Vec2::new(-d.x, -d.y) * game.mouse_sens * settings.sensitivity;
    }
    let mut respawn = keys.just_pressed(KeyCode::KeyR);
    let mut reaction = keys.just_pressed(KeyCode::AltLeft);

    for pad in &pads {
        let l = pad.left_stick();
        if l.length() > 0.15 {
            mv += l;
        }
        let r = pad.right_stick();
        let r = if r.length() > 0.12 { r * r.length() } else { Vec2::ZERO };
        input.look += Vec2::new(-r.x, r.y) * game.pad_sens * settings.sensitivity * dt;
        let jump = [GamepadButton::South, GamepadButton::LeftTrigger];
        let crouch = [GamepadButton::East, GamepadButton::LeftTrigger2];
        input.jump_pressed |= jump.iter().any(|b| pad.just_pressed(*b));
        input.jump_held |= jump.iter().any(|b| pad.pressed(*b));
        input.crouch_pressed |= crouch.iter().any(|b| pad.just_pressed(*b));
        input.crouch_held |= crouch.iter().any(|b| pad.pressed(*b));
        input.turn_pressed |= pad.just_pressed(GamepadButton::North);
        input.melee_pressed |= pad.just_pressed(GamepadButton::RightThumb);
        fire_blue |= pad.just_pressed(GamepadButton::RightTrigger2);
        fire_orange |= pad.just_pressed(GamepadButton::RightTrigger);
        use_key |= pad.just_pressed(GamepadButton::West);
        reaction |= pad.just_pressed(GamepadButton::LeftThumb);
        respawn |= pad.just_pressed(GamepadButton::Select);
    }
    input.strafe_raw = mv.x.clamp(-1.0, 1.0);
    input.move_axis = mv.clamp_length_max(1.0);
    // The menu just closed: whatever closed it isn't also a move.
    if menu.open || menu.just_closed {
        input = MoveInput::default();
        fire_blue = false;
        fire_orange = false;
        use_key = false;
        respawn = false;
        reaction = false;
    }
    if let Some(scripted) = game.scripted {
        input = scripted;
        fire_blue = game.scripted_fire == Some(PortalColor::Blue);
        fire_orange = game.scripted_fire == Some(PortalColor::Orange);
        use_key = game.scripted_use;
        game.scripted_fire = None;
        game.scripted_use = false;
    }

    if keys.just_pressed(KeyCode::F3) {
        game.show_debug = !game.show_debug;
    }

    // ---- checkpoints
    let digits = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6];
    let mut goto = None;
    for (i, k) in digits.iter().enumerate() {
        if keys.just_pressed(*k) && !menu.open && i < game.sim.level.checkpoints.len() {
            goto = Some(i);
        }
    }
    if respawn || goto.is_some() {
        let g = &mut *game;
        g.sim.respawn_at(&mut g.ctrl, goto);
        if goto == Some(0) {
            g.timer = 0.0;
        }
    }

    // ---- simulate (frozen while the mouse is free, so you can alt-tab)
    game.sim.events.clear();
    let running = (game.locked || !pads.is_empty() || game.scripted.is_some()) && !menu.open;
    if running {
        let (was_full, was_on) = (game.reaction.energy >= 100.0, game.reaction.active);
        if reaction {
            game.reaction.attempt();
        }
        let speed = game.reaction.update(dt, game.ctrl.vel.length());
        if !was_full && game.reaction.energy >= 100.0 && !game.reaction_hinted {
            game.reaction_hinted = true;
            game.flash = Some(("Reaction Time ready - Left Alt".to_string(), 0.0));
        }
        if !was_on && game.reaction.active {
            game.flash = Some(("REACTION TIME".to_string(), 0.0));
        }
        virtual_time.set_relative_speed(speed);

        let g = &mut *game;
        g.sim.step_player(&mut g.ctrl, &input, dt);
        // Through a portal: everything that keeps its own copy of her yaw turns with her.
        for e in &g.ctrl.events {
            if let MoveEvent::Teleport { yaw_delta } = *e {
                g.fx.shift_yaw(yaw_delta);
                g.look.shift_yaw(yaw_delta);
            }
        }
        g.look.apply(&mut g.ctrl, dt);
        let aim = Aim::of(&g.ctrl);
        if (g.sim.held.is_some() || g.sim.held_turret.is_some()) && (fire_blue || fire_orange) {
            // Holding something, the trigger lets it go with a push.
            g.sim.throw(aim.dir, 4.0);
        } else if fire_blue {
            g.sim.fire(PortalColor::Blue, aim);
        } else if fire_orange {
            g.sim.fire(PortalColor::Orange, aim);
        }
        if use_key {
            g.sim.use_key(aim);
        }
        g.sim.step(dt, &mut g.ctrl);
        g.shot = g.fx.update(dt, &g.ctrl, &input);
        g.timer += dt;
        g.intro += dt;
    }

    for e in game.ctrl.events.clone() {
        if let Some(label) = flash_label(e) {
            game.flash = Some((label.to_string(), 0.0));
        }
    }
    game.died += dt;
    for e in game.sim.events.clone() {
        match e {
            SimEvent::PlayerDied => {
                game.died = 0.0;
                game.flash = Some(("RESPAWN".into(), 0.0));
            }
            SimEvent::Checkpoint { index } => {
                let name = game.sim.level.checkpoints[index].name;
                game.flash = Some((format!("CHECKPOINT - {name}"), 0.0));
            }
            SimEvent::PortalsCleared => game.flash = Some(("PORTALS CLEARED".into(), 0.0)),
            SimEvent::LevelComplete => {
                let t = game.timer;
                // (A test run in the editor isn't a time.)
                let pb = Some(game.level_index) != editor.slot() && progress.finished(&game.levels[game.level_index].0, t);
                progress.save();
                game.last_time = Some(t);
                game.flash = Some((format!("CHAMBER COMPLETE  {}{}", hud::fmt_time(t), if pb { "  - NEW BEST" } else { "" }), -2.0));
                // A Portal changelevel names its map. Switch without the three-second wait, so
                // the elevator's delayed door-open doesn't play on the map being left, and
                // stand in the next map's elevator rather than at its player start.
                // (The next map's own copy, from one of the game's copies.)
                let group = game.groups[game.level_index];
                let next = game.sim.transition.clone().and_then(|tr| {
                    let i = (0..game.levels.len()).find(|&i| game.source_names[i].as_deref() == Some(tr.map.as_str()) && game.groups[i] == group)?;
                    Some((i, tr))
                });
                match next {
                    Some((i, tr)) => {
                        game.arrival = Some(tr);
                        game.pending = Some(i);
                        game.next_in = None;
                    }
                    None => game.next_in = Some(3.0),
                }
            }
            _ => {}
        }
    }
    if let Some(t) = &mut game.next_in {
        *t -= dt;
        if *t <= 0.0 {
            game.next_in = None;
            game.pending = Some((game.level_index + 1) % game.levels.len());
        }
    }
    if let Some((_, t)) = &mut game.flash {
        *t += dt;
    }

    // ---- camera (the Mirror's Edge body, when loaded, places it again from its eye bone)
    let view = game.shot.view;
    if let Ok((mut tf, mut proj)) = camera.single_mut() {
        tf.translation = view.eye;
        tf.rotation = Quat::from_euler(EulerRot::YXZ, view.yaw, view.pitch.clamp(-FRAC_PI_2, FRAC_PI_2), -view.roll);
        if let Projection::Perspective(p) = &mut *proj {
            let aspect = windows.single().map(|w| w.width() / w.height().max(1.0)).unwrap_or(16.0 / 9.0);
            // Controller gives horizontal FOV; Bevy wants vertical.
            let h = view.fov_deg.to_radians();
            p.fov = 2.0 * ((h * 0.5).tan() / aspect).atan();
            p.aspect_ratio = aspect;
        }
    }
}

/// What's left of the roll from coming out of a portal at an angle, easing out.
fn portal_roll(game: Res<Game>, mut camera: Query<&mut Transform, With<PlayerCamera>>) {
    let roll = game.sim.roll;
    if roll.abs() < 1e-4 {
        return;
    }
    if let Ok(mut tf) = camera.single_mut() {
        let fwd = tf.rotation * Vec3::NEG_Z;
        tf.rotation = Quat::from_axis_angle(fwd, -roll) * tf.rotation;
    }
}

fn flash_label(e: MoveEvent) -> Option<&'static str> {
    Some(match e {
        MoveEvent::HardLand => "HARD LANDING - tap crouch just before you land",
        MoveEvent::Death => "RESPAWN",
        _ => return None,
    })
}
