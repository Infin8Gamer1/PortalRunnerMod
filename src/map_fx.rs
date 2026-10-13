//! What's drawn of Portal's maps besides their faces and props (`source_map`): steam, sparks,
//! particle systems other than the grills' (the elevator shafts' beams, fire, dust drawn
//! into a broken tube, neurotoxin), spotlights' beams, beams between two points, security
//! cameras that turn to watch her (and fall when a portal's opened under them), and the logic's
//! shakes and fades of the view.
//!
//! Particles are worked out from the simulation's clock each frame, as the grills' are
//! (`grill_fx`): each one's whole life is known from when it's born, so nothing is simulated.
//! They're drawn as one mesh of quads per system, facing the camera. The logic shows and hides
//! each (its `hidden`, by entity); one with no name shows as it starts.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use aperture::art::PropKind;

use crate::chamber::ChamberGeom;
use crate::{Game, PlayerCamera};

/// What a particle system looks like and does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Look {
    /// Puffs blown along its way (env_steam): metres a second, how far, sizes, how many a
    /// second, colour, opacity.
    Steam { speed: f32, length: f32, start: f32, end: f32, rate: f32, color: Vec3, alpha: f32 },
    /// Bursts of sparks (env_spark), at most `delay` seconds apart.
    Sparks { magnitude: f32, delay: f32 },
    /// A column of light motes rising (an elevator shaft's beam; GLaDOS's chamber's beams).
    Column { radius: f32, height: f32, color: Vec3 },
    /// Flames.
    Fire { size: f32 },
    /// Dust drawn into its middle (a broken tube's suck).
    Suck { radius: f32 },
    /// A spreading haze (the escape's neurotoxin).
    Haze { radius: f32, color: Vec3 },
}

impl Look {
    /// The look of a particle system by its effect's name, if it's one that's drawn.
    fn of_effect(effect: &str) -> Option<Look> {
        let e = effect.to_ascii_lowercase();
        Some(match e.as_str() {
            "elevator_beam" => Look::Column { radius: 0.7, height: 7.0, color: Vec3::new(0.75, 0.85, 1.0) },
            "glados_beam" => Look::Column { radius: 0.25, height: 12.0, color: Vec3::new(1.0, 0.9, 0.7) },
            "broken_tube_suck" => Look::Suck { radius: 1.8 },
            e if e.starts_with("fire") => Look::Fire { size: if e.contains("large") { 1.2 } else { 0.6 } },
            e if e.starts_with("neurotoxins") => Look::Haze { radius: 6.0, color: Vec3::new(0.55, 0.75, 0.35) },
            _ => return None,
        })
    }

    /// Born a second, and how long each lives.
    fn rate_life(self) -> (f32, f32) {
        match self {
            Look::Steam { speed, length, rate, .. } => (rate.clamp(1.0, 80.0), (length / speed.max(0.1)).clamp(0.1, 6.0)),
            Look::Sparks { .. } => (0.0, 0.6),
            Look::Column { height, .. } => ((height * 3.0).clamp(4.0, 40.0), 5.0),
            Look::Fire { size } => (40.0 * size, 0.8),
            Look::Suck { .. } => (40.0, 1.2),
            Look::Haze { radius, .. } => ((radius * 2.0).clamp(4.0, 30.0), 8.0),
        }
    }

    /// Blended over what's behind (smoke, steam), else adding light (sparks, fire, motes).
    fn blended(self) -> bool {
        matches!(self, Look::Steam { .. } | Look::Suck { .. } | Look::Haze { .. })
    }
}

/// The particle effects drawn (`info_particle_system`'s effect_name: the editor offers these;
/// a grill's are drawn as the grill).
pub const EFFECTS: [&str; 7] = ["elevator_beam", "glados_beam", "broken_tube_suck", "fire_large_02", "fire_incinerator_door", "neurotoxins_step1", "neurotoxins_step3"];

/// A particle system where the map has it.
#[derive(Component)]
pub struct Fx {
    look: Look,
    at: Vec3,
    /// Its axes: along (the way it faces), and two across.
    axes: [Vec3; 3],
    /// Its entity in the logic (it shows as that says), or how it starts.
    ent: Option<usize>,
    start_on: bool,
    mover: Option<usize>,
    seed: u32,
}

/// A beam (a spotlight's, or env_beam's): shown as the logic says.
#[derive(Component)]
pub struct Shown {
    ent: Option<usize>,
    start_on: bool,
}

/// A security camera: its root, and where it is.
#[derive(Component)]
pub struct Camera {
    at: Vec3,
    turn: Quat,
    ent: Option<usize>,
    /// How far it's fallen (knocked down), and how fast.
    fall: f32,
    speed: f32,
    floor: f32,
    /// Its aim now (smoothed): yaw and pitch.
    aim: Vec2,
}

/// A security camera's turning part: its yaw bone or its pitch bone, its rest pose, and the
/// camera (its root entity).
#[derive(Component)]
pub struct CameraPart {
    pitch: bool,
    rest: Quat,
    root: Entity,
}

fn rand(seed: u32, k: u32, n: u32) -> f32 {
    let mut h = seed.wrapping_mul(0x9E37_79B9) ^ k.wrapping_mul(0x85EB_CA6B) ^ n.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_4D2B);
    h ^= h >> 15;
    (h >> 8) as f32 / 16_777_216.0
}

/// A soft round puff, white, its alpha falling off to its edge (made once).
fn puff(images: &mut Assets<Image>) -> Handle<Image> {
    let n = 64u32;
    let mut data = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let d = Vec2::new(x as f32 + 0.5 - n as f32 / 2.0, y as f32 + 0.5 - n as f32 / 2.0).length() / (n as f32 / 2.0);
            let a = (1.0 - d).clamp(0.0, 1.0).powf(1.6);
            data.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
        }
    }
    images.add(Image::new(
        bevy::render::render_resource::Extent3d { width: n, height: n, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        data,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ))
}

/// The materials, made once: blended puffs, added light.
#[derive(Resource, Default)]
pub struct FxMats {
    blend: Option<Handle<StandardMaterial>>,
    add: Option<Handle<StandardMaterial>>,
}

/// Put the map's effects and cameras where it has them (as a map's drawn).
#[allow(clippy::too_many_arguments)]
pub fn sync(
    mut commands: Commands,
    game: Res<Game>,
    assets: Res<AssetServer>,
    extracted: Res<crate::extracted::Extracted>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<FxMats>,
    mut drawn: Local<Option<u64>>,
) {
    if *drawn == Some(game.loads) {
        return;
    }
    *drawn = Some(game.loads);
    let Some(art) = game.sim.level.art.clone() else { return };
    let tex = mats.blend.as_ref().and_then(|m| materials.get(m)).and_then(|m| m.base_color_texture.clone()).unwrap_or_else(|| puff(&mut images));
    let blend = mats.blend.get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(tex.clone()), alpha_mode: AlphaMode::Blend, unlit: true, double_sided: true, cull_mode: None, ..default() })).clone();
    let add = mats.add.get_or_insert_with(|| materials.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(tex.clone()), alpha_mode: AlphaMode::Add, unlit: true, double_sided: true, cull_mode: None, ..default() })).clone();
    let io = game.sim.io.as_ref();
    let by_name = |n: &str| io.and_then(|io| io.ents.iter().position(|e| !n.is_empty() && e.name.eq_ignore_ascii_case(n)));
    let movers: std::collections::HashMap<String, usize> = io
        .map(|io| io.ents.iter().filter_map(|e| if let aperture::io::Kind::Mover { index, .. } = e.kind { (!e.name.is_empty()).then(|| (e.name.to_ascii_lowercase(), index)) } else { None }).collect())
        .unwrap_or_default();
    let mut seed = 7u32;
    for p in &art.props {
        let ent = by_name(&p.name);
        let mover = movers.get(&p.parent).copied();
        let fwd = p.turn * Vec3::NEG_Z;
        let (a, b) = fwd.any_orthonormal_pair();
        let look = match &p.kind {
            PropKind::Steam { speed, length, start, end, rate, color, alpha, .. } => Some(Look::Steam { speed: *speed, length: *length, start: *start, end: *end, rate: *rate, color: *color, alpha: *alpha }),
            PropKind::Sparks { magnitude, delay, .. } => Some(Look::Sparks { magnitude: *magnitude, delay: *delay }),
            PropKind::Particles { .. } => Look::of_effect(&p.model),
            _ => None,
        };
        if let Some(look) = look {
            seed += 1;
            let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
            // (Never empty: Bevy's mesh buffers can't hold nothing. An unseen triangle.)
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 3]);
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; 3]);
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 3]);
            mesh.insert_indices(Indices::U32(vec![0, 1, 2]));
            commands.spawn((
                ChamberGeom,
                Fx { look, at: p.at, axes: [fwd, a, b], ent, start_on: p.kind.starts_on(), mover, seed },
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(if look.blended() { blend.clone() } else { add.clone() }),
                Transform::default(),
                Visibility::Hidden,
                NoFrustumCulling,
                NotShadowCaster,
            ));
            continue;
        }
        match &p.kind {
            // A spotlight: a light along its way, and its beam, a cone fading out to its end.
            PropKind::Spotlight { length, width, color, start_on, light } => {
                let c = color.min(Vec3::splat(4.0));
                let beam = beam_fan(*length, *width);
                let mat = materials.add(StandardMaterial { base_color: Color::linear_rgb(c.x * 0.35, c.y * 0.35, c.z * 0.35), alpha_mode: AlphaMode::Add, unlit: true, double_sided: true, cull_mode: None, ..default() });
                let up = if fwd.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
                commands
                    .spawn((ChamberGeom, Shown { ent, start_on: *start_on }, Transform::from_translation(p.at).looking_to(fwd, up), Visibility::Hidden))
                    .with_children(|ch| {
                        ch.spawn((FacesEye, Mesh3d(meshes.add(beam)), MeshMaterial3d(mat), Transform::default(), NotShadowCaster));
                        if !*light {
                            return;
                        }
                        ch.spawn(SpotLight { intensity: 60_000.0 * c.length().max(0.2), color: Color::linear_rgb(c.x, c.y, c.z), range: length * 1.5, radius: 0.0, outer_angle: (width / length.max(0.1)).atan().clamp(0.05, 1.2), inner_angle: (width / length.max(0.1)).atan().clamp(0.05, 1.2) * 0.6, shadow_maps_enabled: false, ..default() });
                    });
            }
            // A beam between two points: a flat strip, turned about its line to face the eye.
            PropKind::Beam { to, width, color, start_on } => {
                let len = p.at.distance(*to);
                if len < 0.01 {
                    continue;
                }
                let mut strip = Mesh::from(Rectangle::new(width.max(0.02), len));
                strip.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32, 1.0, 1.0, 1.0]; 4]);
                let mat = materials.add(StandardMaterial { base_color: Color::linear_rgb(color.x * 2.0, color.y * 2.0, color.z * 2.0), base_color_texture: extracted.texture(&format!("map/{}", p.model.trim_end_matches(".vmt"))).map(|t| images.add(t)), alpha_mode: AlphaMode::Add, unlit: true, double_sided: true, cull_mode: None, ..default() });
                commands.spawn((ChamberGeom, Shown { ent, start_on: *start_on }, BeamBetween(p.at, *to), Mesh3d(meshes.add(strip)), MeshMaterial3d(mat), Transform::from_translation((p.at + *to) * 0.5), Visibility::Hidden, NotShadowCaster));
            }
            // A security camera: its model, its parts tagged as its scene spawns.
            PropKind::Camera => {
                let Some(path) = extracted.model(&p.model) else { continue };
                commands.spawn((
                    ChamberGeom,
                    Camera { at: p.at, turn: p.turn, ent, fall: 0.0, speed: 0.0, floor: floor_under(&game.sim, p.at), aim: Vec2::ZERO },
                    WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
                    Transform::from_translation(p.at).with_rotation(p.turn),
                    Visibility::default(),
                ));
            }
            _ => {}
        }
    }
}

/// A beam's ends.
#[derive(Component)]
pub struct BeamBetween(Vec3, Vec3);

/// Where the floor under a point is (the top of the highest box under it).
fn floor_under(sim: &aperture::Sim, p: Vec3) -> f32 {
    sim.statics.iter().filter(|s| s.b.min.x <= p.x && s.b.max.x >= p.x && s.b.min.z <= p.z && s.b.max.z >= p.z && s.b.max.y <= p.y).map(|s| s.b.max.y).fold(p.y - 20.0, f32::max)
}

/// A spotlight's beam: a flat fan along -Z (lying in its X), `length` long, `width` across at
/// its end, its brightness in its vertices' colour.
fn beam_fan(length: f32, width: f32) -> Mesh {
    // (Across it: bright down its middle, nothing at its sides; along it: from its lamp, a
    // little way in to full, then fading out to its end. Turned about its line to face the eye,
    // it's a cone of light in the air with no edge to see.)
    const ACROSS: [f32; 7] = [-1.0, -0.66, -0.33, 0.0, 0.33, 0.66, 1.0];
    let rows = 12;
    let start = width * 0.15;
    let (mut pos, mut nrm, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for r in 0..=rows {
        let t = r as f32 / rows as f32;
        let half = (start + (width - start) * t) * 0.5;
        let along = (t / 0.08).min(1.0) * (1.0 - t) * (1.0 - t);
        for x in ACROSS {
            let b = (1.0 - x * x) * (1.0 - x * x) * along;
            pos.push([x * half, 0.0, -length * t]);
            nrm.push([0.0, 1.0, 0.0]);
            col.push([b, b, b, 1.0]);
        }
    }
    let n = ACROSS.len() as u32;
    for r in 0..rows as u32 {
        for c in 0..n - 1 {
            let (a, b) = (r * n + c, (r + 1) * n + c);
            idx.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(idx))
}

/// A spotlight's beam (a child of the light, along its -Z): turned about its line to face the
/// eye.
#[derive(Component)]
pub struct FacesEye;

/// Whether the logic has something shown: its entity not hidden (or, one the logic can't
/// name, as it starts).
fn shown(game: &Game, ent: Option<usize>, start_on: bool) -> bool {
    match (ent, game.sim.io.as_ref()) {
        (Some(e), Some(io)) => !io.hidden.contains(&e),
        _ => start_on,
    }
}

/// One particle: where, how big, its colour and opacity, and (a spark) the way it streaks.
struct Particle {
    at: Vec3,
    size: f32,
    color: Vec4,
    streak: Option<Vec3>,
}

/// A system's live particles at time `t`.
fn particles(fx: &Fx, t: f32) -> Vec<Particle> {
    let (rate, life) = fx.look.rate_life();
    let [fwd, a, b] = fx.axes;
    let mut out = Vec::new();
    let r = |k: u32, n: u32| rand(fx.seed, k, n);
    if let Look::Sparks { magnitude, delay } = fx.look {
        // A burst in each slot of time, somewhere in it.
        let slot = delay.max(0.4);
        let now = (t / slot).floor() as i64;
        for s in [now - 1, now] {
            if s < 0 {
                continue;
            }
            let born = s as f32 * slot + r(s as u32, 0) * slot * 0.8;
            let age = t - born;
            if !(0.0..life).contains(&age) {
                continue;
            }
            let n = (magnitude.clamp(1.0, 8.0) * 5.0) as u32;
            for k in 0..n {
                let dir = (fwd * 1.5 + a * (r(s as u32, k * 3 + 1) - 0.5) * 2.0 + b * (r(s as u32, k * 3 + 2) - 0.5) * 2.0).normalize_or(fwd);
                let v = dir * (2.0 + 2.5 * r(s as u32, k * 3 + 3)) * magnitude.sqrt().max(0.7);
                let g = Vec3::Y * -9.8;
                let at = fx.at + v * age + g * 0.5 * age * age;
                let vel = v + g * age;
                let f = 1.0 - age / life;
                out.push(Particle { at, size: 0.025, color: Vec4::new(1.0, 0.7 + 0.3 * f, 0.3 * f, f) * 3.0, streak: Some(vel * 0.03) });
            }
        }
        return out;
    }
    let first = ((t - life) * rate).ceil().max(0.0) as u32;
    let last = (t * rate).floor() as u32;
    for k in first..=last {
        let born = k as f32 / rate;
        let age = t - born;
        if !(0.0..life).contains(&age) {
            continue;
        }
        let f = age / life;
        // (Faded in and out.)
        let fade = (f * 6.0).min(1.0) * (1.0 - f).min(1.0);
        let (u, v, w) = (r(k, 1) - 0.5, r(k, 2) - 0.5, r(k, 3));
        let p = match fx.look {
            Look::Steam { speed, start, end, color, alpha, .. } => {
                let spread = speed * 0.25 * age;
                Particle { at: fx.at + fwd * speed * age + (a * u + b * v) * spread, size: start + (end - start) * f, color: Vec4::new(color.x, color.y, color.z, alpha * 0.45 * fade * (1.0 - f)), streak: None }
            }
            Look::Column { radius, height, color } => {
                let ang = u * std::f32::consts::TAU + age * 0.3;
                Particle { at: fx.at + Vec3::new(ang.cos(), 0.0, ang.sin()) * radius * (0.3 + 0.7 * w) + Vec3::Y * (v + 0.5 + f * 0.3) * height, size: 0.05, color: Vec4::new(color.x, color.y, color.z, 1.0) * 0.6 * fade, streak: None }
            }
            Look::Fire { size } => Particle { at: fx.at + (a * u + b * v) * size * 0.5 + Vec3::Y * size * 1.5 * f, size: size * (0.6 - 0.45 * f), color: Vec4::new(1.0, 0.55 - 0.3 * f, 0.15, 1.0) * 1.5 * fade, streak: None },
            Look::Suck { radius } => {
                let ang = u * std::f32::consts::TAU + f * 2.0;
                let d = radius * (1.0 - f);
                Particle { at: fx.at + Vec3::new(ang.cos() * d, v * radius * (1.0 - f), ang.sin() * d), size: 0.08, color: Vec4::new(0.8, 0.8, 0.75, 0.5 * fade), streak: None }
            }
            Look::Haze { radius, color } => Particle { at: fx.at + Vec3::new(u, v * 0.3, w - 0.5) * radius * 2.0 + Vec3::Y * f * 0.5, size: 2.5 + 2.0 * f, color: Vec4::new(color.x, color.y, color.z, 0.12 * fade), streak: None },
            Look::Sparks { .. } => continue,
        };
        out.push(p);
    }
    out
}

/// Each system's particles now, facing the camera; shown as the logic says.
pub fn update(game: Res<Game>, camera: Query<&GlobalTransform, With<PlayerCamera>>, mut fx: Query<(&Fx, &Mesh3d, &mut Visibility)>, mut meshes: ResMut<Assets<Mesh>>) {
    let Ok(cam) = camera.single() else { return };
    let (right, up) = (cam.right().as_vec3(), cam.up().as_vec3());
    let t = game.sim.time;
    for (f, mesh, mut vis) in &mut fx {
        // (Far off: not worth the quads, and not shown, or the last ones would hang there.)
        let near = f.at.distance(cam.translation()) <= 60.0;
        let on = near && shown(&game, f.ent, f.start_on) && f.mover.and_then(|m| game.sim.movers.get(m)).is_none_or(|m| m.present);
        let want = if on { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if !on {
            continue;
        }
        let shift = f.mover.and_then(|m| game.sim.movers.get(m)).map_or(Vec3::ZERO, |m| m.offset());
        let parts = particles(f, t);
        let (mut pos, mut nrm, mut uv, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let n = -cam.forward().as_vec3();
        for p in parts {
            let base = pos.len() as u32;
            let c = p.at + shift;
            let (x, y) = match p.streak {
                // (A streak along its way, thin across it.)
                Some(s) => (s.cross(n).normalize_or(right) * p.size, s * 0.5 + s.normalize_or(up) * p.size),
                None => (right * p.size * 0.5, up * p.size * 0.5),
            };
            for (k, q) in [c - x - y, c + x - y, c + x + y, c - x + y].into_iter().enumerate() {
                pos.push(q.to_array());
                nrm.push(n.to_array());
                uv.push([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]][k]);
                col.push(p.color.to_array());
            }
            idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        if idx.is_empty() {
            pos = vec![[0.0; 3]; 3];
            nrm = vec![n.to_array(); 3];
            uv = vec![[0.0; 2]; 3];
            col = vec![[0.0; 4]; 3];
            idx = vec![0, 1, 2];
        }
        if let Some(mut m) = meshes.get_mut(&mesh.0) {
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
            m.insert_indices(Indices::U32(idx));
        }
    }
}

/// Beams and spotlights shown as the logic says; a beam turned about its line to face the eye.
#[allow(clippy::type_complexity)]
pub fn beams(game: Res<Game>, camera: Query<&GlobalTransform, With<PlayerCamera>>, mut q: Query<(&Shown, Option<&BeamBetween>, &mut Transform, &mut Visibility), Without<FacesEye>>, mut fans: Query<(&ChildOf, &mut Transform), With<FacesEye>>, lamps: Query<&GlobalTransform, With<Shown>>) {
    let eye = camera.single().map(|c| c.translation()).unwrap_or(Vec3::ZERO);
    for (parent, mut tf) in &mut fans {
        let Ok(lamp) = lamps.get(parent.parent()) else { continue };
        let e = lamp.affine().inverse().transform_point3(eye);
        if Vec2::new(e.x, e.y).length_squared() > 1e-6 {
            tf.rotation = Quat::from_rotation_z((-e.x).atan2(e.y));
        }
    }
    for (s, between, mut tf, mut vis) in &mut q {
        let want = if shown(&game, s.ent, s.start_on) { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if let Some(BeamBetween(a, b)) = between {
            let along = (*b - *a).normalize_or(Vec3::Y);
            let to_eye = (eye - (*a + *b) * 0.5).normalize_or(Vec3::Z);
            let side = along.cross(to_eye).normalize_or(Vec3::X);
            let face = side.cross(along);
            tf.rotation = Quat::from_mat3(&Mat3::from_cols(side, along, face));
        }
    }
}

/// Tag a security camera's turning parts (its yaw and pitch bones) as its scene spawns.
pub fn tag_cameras(mut commands: Commands, added: Query<(Entity, &Name, &Transform), Added<Name>>, parents: Query<&ChildOf>, roots: Query<(), With<Camera>>) {
    for (e, name, tf) in &added {
        let pitch = match name.as_str() {
            "LR_Rot1" => false,
            "UD_Rot1" => true,
            _ => continue,
        };
        let mut cur = e;
        for _ in 0..10 {
            let Ok(p) = parents.get(cur) else { break };
            cur = p.parent();
            if roots.contains(cur) {
                commands.entity(e).insert(CameraPart { pitch, rest: tf.rotation, root: cur });
                break;
            }
        }
    }
}

/// How far a security camera turns (radians): either way, and up or down.
const CAMERA_YAW: f32 = 1.3;
const CAMERA_PITCH: f32 = 0.7;

/// Security cameras watch her (turning their parts to her eye, a little behind), and fall
/// once a portal's knocked them down.
pub fn cameras(time: Res<Time>, game: Res<Game>, mut cams: Query<(Entity, &mut Camera, &mut Transform), Without<CameraPart>>, mut parts: Query<(&CameraPart, &mut Transform, &ChildOf)>, globals: Query<&GlobalTransform>) {
    let dt = time.delta_secs();
    let eye = game.ctrl.view().eye;
    for (e, mut c, mut tf) in &mut cams {
        let knocked = c.ent.is_some_and(|k| game.sim.io.as_ref().is_some_and(|io| io.knocked.contains(&k)));
        if knocked {
            // Down it goes, tipping over as it falls, to rest on the floor.
            if c.at.y - c.fall > c.floor + 0.15 {
                c.speed += 9.8 * dt;
                c.fall = (c.fall + c.speed * dt).min(c.at.y - c.floor - 0.15);
            }
            let tip = (c.fall / 0.6).min(1.0) * std::f32::consts::FRAC_PI_2;
            tf.translation = c.at - Vec3::Y * c.fall;
            tf.rotation = Quat::from_rotation_x(tip) * c.turn;
            continue;
        }
        // Where she is from it, in its model's frame (it faces -Z).
        let local = c.turn.inverse() * (eye - c.at);
        let yaw = (-local.x).atan2(-local.z).clamp(-CAMERA_YAW, CAMERA_YAW);
        let pitch = (local.y / Vec2::new(local.x, local.z).length().max(1e-3)).atan().clamp(-CAMERA_PITCH, CAMERA_PITCH);
        let k = 1.0 - (-dt * 3.0).exp();
        let was = c.aim;
        c.aim = was + (Vec2::new(yaw, pitch) - was) * k;
        let aim = c.aim;
        for (part, mut ptf, parent) in &mut parts {
            if part.root != e {
                continue;
            }
            // A turn in the model's frame (`d`), as the part's own: through its parent's frame
            // as it is now (the pitch part's parent is the yaw part, turned already).
            let (Ok(root), Ok(par)) = (globals.get(e), globals.get(parent.parent())) else { continue };
            let p = root.rotation().inverse() * par.rotation();
            let yawed = Quat::from_rotation_y(aim.x);
            let d = if part.pitch { Quat::from_axis_angle(yawed * Vec3::X, aim.y) } else { yawed };
            ptf.rotation = p.inverse() * d * p * part.rest;
        }
    }
}

/// The logic's shakes (felt within their radius, for their duration) and fades of the screen.
#[derive(Default)]
pub struct ScreenState {
    loads: u64,
    shake: u32,
    shaking: Vec<(f32, f32, f32)>,
    fade: u32,
    fading: Option<(aperture::io::Fade, f32)>,
}

/// The fade over the screen.
#[derive(Component)]
pub struct FadeOverlay;

pub fn setup(mut commands: Commands) {
    commands.spawn((FadeOverlay, Node { position_type: PositionType::Absolute, width: percent(100.0), height: percent(100.0), ..default() }, BackgroundColor(Color::NONE), GlobalZIndex(5), Pickable::IGNORE));
}

pub fn screen(time: Res<Time>, mut game: ResMut<Game>, mut st: Local<ScreenState>, mut overlay: Query<&mut BackgroundColor, With<FadeOverlay>>) {
    let dt = time.delta_secs();
    if st.loads != game.loads {
        // (A new level: what was under way is over; what the logic did before isn't news.)
        st.loads = game.loads;
        st.shaking.clear();
        st.fading = None;
        st.shake = 0;
        st.fade = 0;
    }
    let ear = game.ctrl.view().eye;
    let (shakes, fades) = match game.sim.io.as_ref() {
        Some(io) => (io.shakes.clone(), io.fades.clone()),
        None => (Vec::new(), Vec::new()),
    };
    let seen = st.shake;
    for s in shakes.iter().filter(|s| s.n > seen) {
        st.shake = s.n;
        if s.amplitude <= 0.0 {
            st.shaking.clear();
        } else if s.at.distance(ear) <= s.radius {
            st.shaking.push((s.duration, s.duration, (s.amplitude / 12.0).clamp(0.05, 0.6)));
        }
    }
    let seen = st.fade;
    for f in fades.iter().filter(|f| f.n > seen) {
        st.fade = f.n;
        st.fading = Some((*f, 0.0));
    }
    // Shaking: trauma kept up, easing off over each one's duration.
    let mut keep = 0.0f32;
    for (left, total, amount) in &mut st.shaking {
        *left -= dt;
        keep = keep.max(*amount * (*left / total.max(0.01)).max(0.0));
    }
    st.shaking.retain(|s| s.0 > 0.0);
    if keep > 0.0 {
        game.fx.add_trauma(keep * dt * 6.0);
    }
    // Fading: to its colour over its duration, held, then back; or from it.
    let alpha = match &mut st.fading {
        Some((f, t)) => {
            *t += dt;
            let k = (*t / f.duration.max(0.01)).min(1.0);
            let a = if f.from {
                1.0 - k
            } else if *t <= f.duration + f.hold || f.stays {
                k
            } else {
                1.0 - ((*t - f.duration - f.hold) / 0.5).min(1.0)
            };
            Some((f.color, a * f.alpha))
        }
        None => None,
    };
    if alpha.is_some_and(|(_, a)| a <= 0.0) && st.fading.as_ref().is_some_and(|(f, t)| *t > f.duration + f.hold + 0.5 || f.from && *t > f.duration) {
        st.fading = None;
    }
    if let Ok(mut bg) = overlay.single_mut() {
        let want = alpha.map_or(Color::NONE, |(c, a)| Color::srgba(c.x, c.y, c.z, a.clamp(0.0, 1.0)));
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_offered_effect_is_drawn() {
        for e in EFFECTS {
            assert!(Look::of_effect(e).is_some(), "{e}");
        }
    }

    #[test]
    fn effects_by_name() {
        // (Portal's water mist isn't drawn: as in Portal with RTX, the goo's clear.)
        assert_eq!(Look::of_effect("water_mist_1024_512"), None);
        assert!(matches!(Look::of_effect("elevator_beam"), Some(Look::Column { .. })));
        assert!(matches!(Look::of_effect("fire_large_02"), Some(Look::Fire { .. })));
        assert_eq!(Look::of_effect("finale_fx"), None);
    }

    #[test]
    fn steam_keeps_a_jet_of_puffs() {
        let fx = Fx { look: Look::Steam { speed: 3.0, length: 2.0, start: 0.25, end: 0.6, rate: 26.0, color: Vec3::ONE, alpha: 1.0 }, at: Vec3::ZERO, axes: [Vec3::Y, Vec3::X, Vec3::Z], ent: None, start_on: true, mover: None, seed: 1 };
        let p = particles(&fx, 10.0);
        // A life of 2/3 s at 26 a second.
        assert!((16..=19).contains(&p.len()), "{}", p.len());
        assert!(p.iter().all(|q| q.at.y >= 0.0 && q.at.y <= 2.1));
    }
}
