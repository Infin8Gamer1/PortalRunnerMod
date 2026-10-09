//! Portal Runner: Mirror's Edge's movement with Portal's portal gun and puzzles.
//!
//! The movement is the `faith_move` crate (from Faith Runner) and the portals and puzzle
//! pieces are the `aperture` crate; this is the Bevy shell round them: input, rendering
//! (portals in `portal_render`, chambers in `chamber`, the gun in `gun`), sound, the HUD.

mod audio;
mod capture;
mod chamber;
mod extracted;
mod gun;
mod hud;
mod me_viewmodel;
mod portal_audio;
mod portal_render;
mod settings;
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

use aperture::{Aim, Level, PortalColor, Sim, SimEvent};
use faith_move::greybox;
use faith_move::{CameraFx, Controller, Event as MoveEvent, Input as MoveInput, Shot, State as MoveState};

use crate::portal_render::PLAYER_LAYER;

fn main() {
    let mut app = App::new();
    // The asset server reads the extracted models from `assets/` (next to `extracted/`).
    let asset_dir = extracted::find_root()
        .and_then(|r| r.parent().map(|p| p.to_path_buf()))
        .and_then(|p| std::fs::canonicalize(p).ok())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "assets".into());
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window { title: "Portal Runner".into(), resolution: (1600, 900).into(), ..default() }),
                ..default()
            })
            .set(AssetPlugin { file_path: asset_dir, ..default() }),
    )
    .add_plugins(portal_render::PortalRenderPlugin)
    .insert_resource(ClearColor(Color::srgb(0.05, 0.05, 0.06)))
    .insert_resource(GlobalAmbientLight { color: Color::srgb(0.85, 0.92, 1.0), brightness: 400.0, affects_lightmapped_meshes: true })
    .add_systems(
        Startup,
        (
            extracted::setup,
            setup_world,
            chamber::setup,
            load_first_level,
            hud::setup,
            viewmodel::setup,
            me_viewmodel::setup,
            gun::setup,
            settings::setup,
            audio::setup,
            portal_audio::setup,
            capture::setup,
        )
            .chain(),
    );
    let game = (
        switch_level,
        play,
        chamber::update,
        viewmodel::animate,
        me_viewmodel::animate,
        portal_roll,
        portal_render::update,
        gun::update,
        gun::tag_model,
        audio::play,
        portal_audio::play,
        hud::update,
    )
        .chain();
    app.add_systems(Update, (capture::run.run_if(resource_exists::<capture::Capture>), settings::update, cursor_lock, game).chain());
    app.run();
}

// ------------------------------------------------------------------ state

#[derive(Resource)]
pub struct Game {
    pub ctrl: Controller,
    pub sim: Sim,
    /// The chamber as Mirror's Edge's sounds see it (footstep surfaces, checkpoints).
    pub level: greybox::Level,
    pub levels: Vec<(&'static str, fn() -> Level)>,
    pub level_index: usize,
    /// Load this level next frame (chamber finished, or a key).
    pub pending: Option<usize>,
    /// Counting down to the next chamber after finishing one.
    pub next_in: Option<f32>,
    pub locked: bool,
    /// The mouse was grabbed this frame: that click isn't a shot.
    pub just_locked: bool,
    pub show_help: bool,
    pub mouse_sens: f32,
    pub pad_sens: f32,
    /// Time in this chamber, and the best per chamber.
    pub timer: f32,
    pub best: Vec<Option<f32>>,
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
}

#[derive(Component)]
pub struct PlayerCamera;

#[derive(Component)]
pub struct Sun;

// ------------------------------------------------------------------ setup

fn setup_world(mut commands: Commands) {
    let levels = aperture::levels::all();
    let first = std::env::var("PORTAL_MAP").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(0).min(levels.len() - 1);
    let level = (levels[first].1)();
    let cp = level.checkpoints[0].clone();
    let mut ctrl = Controller::new(aperture::tuning(5.3, 10.0), cp.spawn, cp.yaw);
    ctrl.state = MoveState::Ground;

    commands.spawn((
        Sun,
        DirectionalLight { illuminance: 11_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(30.0, 60.0, 20.0).looking_at(Vec3::new(0.0, 0.0, -60.0), Vec3::Y),
        // Lights the world, Faith's body and the first-person arms.
        RenderLayers::from_layers(&[0, PLAYER_LAYER, viewmodel::VIEWMODEL_LAYER]),
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
    let n = levels.len();
    commands.insert_resource(Game {
        ctrl,
        sim: Sim::new(level.clone()),
        level: level.greybox(),
        levels,
        level_index: first,
        pending: None,
        next_in: None,
        locked: false,
        just_locked: false,
        show_help: true,
        mouse_sens: 0.0022,
        pad_sens: 3.2,
        timer: 0.0,
        best: vec![None; n],
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
    });
}

fn load_first_level(
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut meshes: ResMut<Assets<Mesh>>,
    mats: Res<chamber::ChamberMats>,
    mut pools: ResMut<chamber::Pools>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    camera: Query<Entity, With<PlayerCamera>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    let g = &mut *game;
    chamber::spawn(&mut commands, &mut meshes, &mats, &g.sim, &mut pools);
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
            l.shadow_maps_enabled = true;
            *tf = Transform::from_xyz(30.0, 60.0, 20.0).looking_at(Vec3::new(0.0, 0.0, -60.0), Vec3::Y);
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
    mut menu: ResMut<settings::Menu>,
) {
    if capture.is_some() {
        return;
    }
    let Ok(mut cursor) = cursor.single_mut() else { return };
    let focused = windows.single().map(|w| w.focused).unwrap_or(true);
    let start = std::mem::take(&mut menu.start);
    let want = if menu.open || keys.just_pressed(KeyCode::Escape) || !focused {
        false
    } else if start || mouse.just_pressed(MouseButton::Left) {
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
    geom: Query<Entity, With<chamber::ChamberGeom>>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    camera: Query<Entity, With<PlayerCamera>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    settings: Res<settings::Settings>,
) {
    let n = game.levels.len();
    let mut to = game.pending.take();
    if keys.just_pressed(KeyCode::KeyM) {
        to = Some((game.level_index + 1) % n);
    }
    if keys.just_pressed(KeyCode::KeyN) {
        to = Some((game.level_index + n - 1) % n);
    }
    if keys.just_pressed(KeyCode::Backspace) {
        to = Some(game.level_index);
    }
    let Some(i) = to else { return };
    for e in &geom {
        commands.entity(e).despawn();
    }
    let g = &mut *game;
    g.level_index = i;
    let level = (g.levels[i].1)();
    g.sim = Sim::new(level.clone());
    g.sim.funnel = settings.funnel > 0.5;
    g.level = level.greybox();
    chamber::spawn(&mut commands, &mut meshes, &mats, &g.sim, &mut pools);
    light_for(&level, &mut sun, camera.single().ok(), &mut commands, &mut clear, &mut ambient);
    let cp = &level.checkpoints[0];
    g.ctrl.spawn = cp.spawn;
    g.ctrl.spawn_yaw = cp.yaw;
    g.ctrl.respawn();
    g.ctrl.state = MoveState::Ground;
    g.timer = 0.0;
    g.last_time = None;
    g.intro = 0.0;
    g.next_in = None;
    g.flash = None;
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
    menu: Res<settings::Menu>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut render: ResMut<portal_render::PortalRender>,
) {
    let dt = time.delta_secs().min(0.05);
    let game = &mut *game;
    game.fx.settings.fov_base = settings.fov;
    game.ctrl.tuning.hard_land_height = settings.hard_land;
    game.ctrl.tuning.lethal_fall_height = settings.lethal_height();
    game.sim.funnel = settings.funnel > 0.5;
    render.depth = (settings.portal_depth.round() as usize).clamp(1, portal_render::MAX_DEPTH);

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
    if let Some(scripted) = game.scripted {
        input = scripted;
        fire_blue = game.scripted_fire == Some(PortalColor::Blue);
        fire_orange = game.scripted_fire == Some(PortalColor::Orange);
        use_key = game.scripted_use;
        game.scripted_fire = None;
        game.scripted_use = false;
    }

    if keys.just_pressed(KeyCode::F1) {
        game.show_help = !game.show_help;
    }

    // ---- checkpoints
    let digits = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6];
    let mut goto = None;
    for (i, k) in digits.iter().enumerate() {
        if keys.just_pressed(*k) && i < game.sim.level.checkpoints.len() {
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
        if !was_full && game.reaction.energy >= 100.0 {
            game.flash = Some(("REACTION TIME READY - Left Alt".to_string(), 0.0));
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
        if g.sim.held.is_some() && (fire_blue || fire_orange) {
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
        g.sim.step(dt, &g.ctrl);
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
                let li = game.level_index;
                let pb = game.best[li].is_none_or(|b| t < b);
                if pb {
                    game.best[li] = Some(t);
                }
                game.last_time = Some(t);
                game.flash = Some((format!("CHAMBER COMPLETE  {}{}", hud::fmt_time(t), if pb { "  - NEW BEST" } else { "" }), -2.0));
                game.next_in = Some(3.0);
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
