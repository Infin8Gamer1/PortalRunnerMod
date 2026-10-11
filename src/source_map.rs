//! Portal's own maps, extracted by `portal_extract` into `assets/extracted/maps/`: listed
//! after the game's chambers, made into levels by `aperture::source`, and drawn here from their
//! faces (a mesh per brush model and material) with Portal's textures, and Portal with RTX's
//! maps where it has them. Light panels glow (under ray tracing they light the chamber); the
//! map's light entities are lights for the ordinary path.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::solari::prelude::RaytracingMesh3d;

use aperture::Level;

use crate::chamber::{ChamberGeom, ChamberMats};
use crate::extracted::Extracted;
use crate::Game;

/// A level to play: made when it's loaded.
pub type LevelFn = Arc<dyn Fn() -> Level + Send + Sync>;

/// Portal's maps, in order, with what the game calls them.
const MAPS: &[(&str, &str)] = &[
    ("testchmb_a_00", "Portal: Chambers 00-01"),
    ("testchmb_a_01", "Portal: Chambers 02-03"),
    ("testchmb_a_02", "Portal: Chambers 04-05"),
    ("testchmb_a_03", "Portal: Chambers 06-07"),
    ("testchmb_a_04", "Portal: Chamber 08"),
    ("testchmb_a_05", "Portal: Chamber 09"),
    ("testchmb_a_06", "Portal: Chamber 10"),
    ("testchmb_a_07", "Portal: Chambers 11-12"),
    ("testchmb_a_08", "Portal: Chamber 13"),
    ("testchmb_a_09", "Portal: Chamber 14"),
    ("testchmb_a_10", "Portal: Chamber 15"),
    ("testchmb_a_11", "Portal: Chamber 16"),
    ("testchmb_a_13", "Portal: Chamber 17"),
    ("testchmb_a_14", "Portal: Chamber 18"),
    ("testchmb_a_15", "Portal: Chamber 19"),
    ("escape_00", "Portal: Escape 1"),
    ("escape_01", "Portal: Escape 2"),
    ("escape_02", "Portal: Escape 3"),
];

fn available(dir: &Path) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
    MAPS.iter().copied().filter(move |(name, _)| dir.join(format!("{name}.mesh")).is_file())
}

/// BSP names of the extracted maps, in the same order as [`levels`].
pub fn map_names(root: Option<&Path>) -> Vec<String> {
    let Some(dir) = root.map(|r| r.join("maps")) else { return Vec::new() };
    available(&dir).map(|(name, _)| name.to_string()).collect()
}

/// The extracted maps there are, as levels.
pub fn levels(root: Option<&Path>) -> Vec<(String, LevelFn)> {
    let Some(dir) = root.map(|r| r.join("maps")) else { return Vec::new() };
    // GLaDOS's scenes, shared by every map.
    let scenes = Arc::new(aperture::source::parse_scenes(&std::fs::read_to_string(dir.join("scenes.txt")).unwrap_or_default()));
    available(&dir)
        .map(|(name, title)| {
            let dir: PathBuf = dir.clone();
            let scenes = scenes.clone();
            let f: LevelFn = Arc::new(move || {
                let read = |ext: &str| std::fs::read_to_string(dir.join(format!("{name}.{ext}"))).unwrap_or_default();
                aperture::source::level_with_scenes(name, title, &read("solids"), &read("ents"), &scenes)
            });
            (title.to_string(), f)
        })
        .collect()
}

/// A brush model's faces (one material's): they move with the sim's mover for that model.
#[derive(Component)]
pub struct MapModel(pub usize);

/// A brush's texture with frames the map's logic picks between (env_texturetoggle): frame 0
/// first.
#[derive(Component)]
pub struct TextureFrames {
    key: FrameKey,
    frames: Vec<Handle<Image>>,
    shown: u32,
}

/// What picks a switched texture's frame: a brush model's number, or (an overlay's) a name.
enum FrameKey {
    Model(usize),
    Name(String),
}

/// Show the frame the logic picked on each switched texture.
pub fn show_frames(game: Res<Game>, mut q: Query<(&mut TextureFrames, &MeshMaterial3d<StandardMaterial>)>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let Some(io) = &game.sim.io else { return };
    for (mut f, mat) in &mut q {
        let picked = match &f.key {
            FrameKey::Model(m) => io.frames.get(m),
            FrameKey::Name(n) => io.named_frames.get(n),
        };
        let want = picked.copied().unwrap_or(0) % f.frames.len() as u32;
        if want == f.shown {
            continue;
        }
        f.shown = want;
        let Some(mut m) = materials.get_mut(&mat.0) else { continue };
        let tex = f.frames[want as usize].clone();
        if m.emissive_texture.is_some() {
            m.emissive_texture = Some(tex.clone());
        }
        m.base_color_texture = Some(tex);
    }
}

/// One of the map's props: where it stands, and the mover it rides with (its parent).
#[derive(Component)]
pub struct MapProp {
    base: Vec3,
    /// How the map turns it (riding a swinging door, it turns further).
    turn: Quat,
    mover: Option<usize>,
    /// Its entity in the map's logic, and its model (for its animations), if it animates.
    animated: Option<(usize, Handle<bevy::gltf::Gltf>)>,
    /// Its model's name without the skin (`map/props_animsigns/signage_num00`), and the skin
    /// it shows: the logic can switch it (a chamber sign flickering on).
    model: String,
    skin: u32,
}

/// A map prop's animation player (on a node inside its model): its entity in the logic, and
/// the last animation it started.
#[derive(Component)]
pub struct MapPropPlayer {
    ent: usize,
    gltf: Handle<bevy::gltf::Gltf>,
    played: u32,
    /// Seconds into an open-then-close fire cycle (a pellet launcher's mouth).
    cycle: Option<f32>,
    /// Which clip that cycle is showing: 1 open, 2 close.
    showing: u8,
    node: Option<AnimationNodeIndex>,
}

/// Source's angles (pitch, yaw, roll; degrees) as a turn in our space.
fn source_rotation(a: Vec3) -> Quat {
    let r = Quat::from_rotation_z(a.y.to_radians()) * Quat::from_rotation_y(a.x.to_radians()) * Quat::from_rotation_x(a.z.to_radians());
    let to_ours = Quat::from_mat3(&Mat3::from_cols(Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y));
    to_ours * r * to_ours.inverse()
}

/// Materials made for maps' textures, kept from map to map.
#[derive(Resource, Default)]
pub struct MapMats(HashMap<String, Handle<StandardMaterial>>);

/// Materials that light the chambers, as the chambers' ceiling lamps do (ray tracing gives
/// them the lamps' extra boost).
#[derive(Resource, Default)]
pub struct Lamps(pub HashSet<AssetId<StandardMaterial>>);

/// How bright a light panel glows (as the chambers' lamps do).
const PANEL_GLOW: f32 = 6.0;
/// How far an overlay sits off its surface (metres).
const OVERLAY_LIFT: f32 = 0.003;
/// How bright a lit sign glows: enough to read in a dim room, not so much its colours (a
/// door's blue X, its orange check) burn out to white.
const SIGN_GLOW: f32 = 0.25;
/// A Portal light's brightness to our lumens.
const LUMENS_PER_BRIGHTNESS: f32 = 2500.0;
/// Under ray tracing, each light is a small glowing bulb: its radius, and how brightly it
/// glows for each unit of Portal's brightness (about one ceiling lamp's light for an ordinary
/// 200).
const BULB_RADIUS: f32 = 0.1;
const BULB_GLOW: f32 = 6.0;
/// At most this many of a map's lights are lights here.
const MAX_LIGHTS: usize = 96;
/// Dimmer than this, a light is not also a bulb.
const BULB_MIN_BRIGHTNESS: f32 = 80.0;
/// How brightly an elevator's cabin lamp glows.
const CABIN_GLOW: f32 = 25.0;

struct Group {
    /// The brush model it's part of (0: the world).
    model: u32,
    material: usize,
    mesh: Mesh,
}

struct MeshFile {
    /// (name, flags: 1 translucent, 2 alphatest, 4 glows, 8 additive, 16 takes portals, 32
    /// textured).
    materials: Vec<(String, u8)>,
    groups: Vec<Group>,
}

fn read_mesh(bytes: &[u8]) -> Option<MeshFile> {
    if bytes.get(..8)? != b"PRMESH1\0" {
        return None;
    }
    let mut at = 8;
    let u32_at = |at: &mut usize| -> Option<u32> {
        let v = u32::from_le_bytes(bytes.get(*at..*at + 4)?.try_into().ok()?);
        *at += 4;
        Some(v)
    };
    let n = u32_at(&mut at)? as usize;
    let mut materials = Vec::with_capacity(n);
    for _ in 0..n {
        let len = u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?) as usize;
        at += 2;
        let name = String::from_utf8_lossy(bytes.get(at..at + len)?).into_owned();
        at += len;
        let flags = *bytes.get(at)?;
        at += 1;
        materials.push((name, flags));
    }
    let ng = u32_at(&mut at)? as usize;
    let mut groups = Vec::with_capacity(ng);
    for _ in 0..ng {
        let model = u32_at(&mut at)?;
        let material = u32_at(&mut at)? as usize;
        let nv = u32_at(&mut at)? as usize;
        let floats: &[u8] = bytes.get(at..at + nv * 32)?;
        at += nv * 32;
        let f = |i: usize| f32::from_le_bytes(floats[i * 4..i * 4 + 4].try_into().unwrap());
        let pos: Vec<[f32; 3]> = (0..nv).map(|v| [f(v * 8), f(v * 8 + 1), f(v * 8 + 2)]).collect();
        let nrm: Vec<[f32; 3]> = (0..nv).map(|v| [f(v * 8 + 3), f(v * 8 + 4), f(v * 8 + 5)]).collect();
        let uv: Vec<[f32; 2]> = (0..nv).map(|v| [f(v * 8 + 6), f(v * 8 + 7)]).collect();
        let ni = u32_at(&mut at)? as usize;
        let idx: Vec<u32> = bytes.get(at..at + ni * 4)?.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        at += ni * 4;
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_indices(Indices::U32(idx));
        groups.push(Group { model, material, mesh });
    }
    Some(MeshFile { materials, groups })
}

/// The material for one of a map's textures.
fn material(name: &str, flags: u8, extracted: &Extracted, images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>, m: &ChamberMats, lamps: &mut Lamps) -> Handle<StandardMaterial> {
    if name.starts_with("glass/") {
        return if name.contains("frosted") { m.frosted.clone() } else { m.glass.clone() };
    }
    let (translucent, alphatest, glows, additive) = (flags & 1 != 0, flags & 2 != 0, flags & 4 != 0, flags & 8 != 0);
    let tex = |n: &str, srgb: bool| if srgb { extracted.texture(n) } else { extracted.texture_data(n) };
    let base = tex(&format!("map/{name}"), true).map(|t| images.add(t));
    let normal = tex(&format!("map/{name}_normal"), false).map(|t| images.add(t));
    let mr = tex(&format!("map/{name}_mr"), false).map(|t| images.add(t));
    let see_through = translucent || alphatest || additive;
    let mat = StandardMaterial {
        base_color: if base.is_some() { Color::WHITE } else { Color::srgb(0.6, 0.6, 0.62) },
        base_color_texture: base.clone(),
        normal_map_texture: normal,
        perceptual_roughness: if mr.is_some() { 1.0 } else { 0.75 },
        metallic: if mr.is_some() { 1.0 } else { 0.0 },
        metallic_roughness_texture: mr,
        emissive: if glows {
            let k = if name.starts_with("signage/") { SIGN_GLOW } else { PANEL_GLOW };
            LinearRgba::rgb(k, k, k)
        } else {
            LinearRgba::BLACK
        },
        emissive_texture: if glows { base } else { None },
        alpha_mode: if additive {
            AlphaMode::Add
        } else if translucent {
            AlphaMode::Blend
        } else if alphatest {
            AlphaMode::Mask(0.5)
        } else {
            AlphaMode::Opaque
        },
        double_sided: see_through,
        cull_mode: if see_through { None } else { Some(bevy::render::render_resource::Face::Back) },
        ..default()
    };
    let h = materials.add(mat);
    if glows {
        lamps.0.insert(h.id());
    }
    h
}

/// Draw the map when one's loaded (after the chamber's geometry is cleared and spawned).
#[allow(clippy::too_many_arguments)]
pub fn sync(
    mut commands: Commands,
    game: Res<Game>,
    assets: Res<AssetServer>,
    extracted: Res<Extracted>,
    m: Option<Res<ChamberMats>>,
    mut cache: ResMut<MapMats>,
    mut lamps: ResMut<Lamps>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut drawn: Local<Option<u64>>,
) {
    if *drawn == Some(game.loads) {
        return;
    }
    let Some(m) = m else { return };
    *drawn = Some(game.loads);
    let level = &game.sim.level;
    let Some(name) = level.source.as_deref() else { return };
    let Some(root) = extracted.root.as_ref() else { return };
    let path = root.join("maps").join(format!("{name}.mesh"));
    let Some(file) = std::fs::read(&path).ok().and_then(|b| read_mesh(&b)) else {
        warn!("{}: can't read", path.display());
        return;
    };
    let mut handles = Vec::with_capacity(file.materials.len());
    let mut frames: HashMap<String, Vec<Handle<Image>>> = HashMap::new();
    for (mat, flags) in &file.materials {
        let h = match cache.0.get(mat) {
            Some(h) => h.clone(),
            None => {
                let h = material(mat, *flags, &extracted, &mut images, &mut materials, &m, &mut lamps);
                cache.0.insert(mat.clone(), h.clone());
                h
            }
        };
        handles.push(h);
    }
    for g in file.groups {
        let (mat, flags) = &file.materials[g.material];
        if level.hidden_models.contains(&(g.model as usize)) {
            continue;
        }
        // Goo is drawn as the level's goo.
        if mat == "nature/hazard_liquid" {
            continue;
        }
        let mut mesh = g.mesh;
        if mesh.generate_tangents().is_err() {
            warn!("{name}: no tangents for {mat}");
        }
        // A texture the logic switches between frames (a door's state panel: X, then the check)
        // on a brush of its own: its own material, so each panel follows its own door.
        let mut handle = handles[g.material].clone();
        let mut switched = None;
        if g.model != 0 {
            let frames = frames.entry(mat.clone()).or_insert_with(|| (1..64).map_while(|k| extracted.texture(&format!("map/{mat}_frame{k}"))).map(|t| images.add(t)).collect::<Vec<_>>()).clone();
            if let Some(own) = (!frames.is_empty()).then(|| materials.get(&handle).cloned()).flatten() {
                let first = own.base_color_texture.clone();
                handle = materials.add(own);
                if lamps.0.contains(&handles[g.material].id()) {
                    lamps.0.insert(handle.id());
                }
                switched = first.map(|f| TextureFrames { key: FrameKey::Model(g.model as usize), frames: std::iter::once(f).chain(frames).collect(), shown: 0 });
            }
        }
        let mut e = commands.spawn((ChamberGeom, Mesh3d(meshes.add(mesh)), MeshMaterial3d(handle), Transform::default()));
        if let Some(f) = switched {
            e.insert(f);
        }
        if flags & (1 | 8) != 0 || mat.starts_with("glass/") {
            e.insert(NotShadowCaster);
        }
        if g.model != 0 {
            e.insert(MapModel(g.model as usize));
        }
    }
    // Its props: models where the map puts them, riding with what they're parented to.
    let text = std::fs::read_to_string(root.join("maps").join(format!("{name}.props"))).unwrap_or_default();
    let animated: HashMap<&str, usize> = game
        .sim
        .io
        .iter()
        .flat_map(|io| io.ents.iter().enumerate())
        .filter_map(|(i, e)| (e.kind == aperture::io::Kind::Animated).then_some((e.name.as_str(), i)))
        .collect();
    let movers: HashMap<&str, usize> = game
        .sim
        .io
        .iter()
        .flat_map(|io| io.ents.iter())
        .filter_map(|e| match e.kind {
            aperture::io::Kind::Mover { index, .. } if !e.name.is_empty() => Some((e.name.as_str(), index)),
            _ => None,
        })
        .collect();
    let floats = |v: &str| -> Vec3 {
        let f: Vec<f32> = v.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        Vec3::new(f.first().copied().unwrap_or(0.0), f.get(1).copied().unwrap_or(0.0), f.get(2).copied().unwrap_or(0.0))
    };
    let mut cabin_lamp: Option<(Handle<Mesh>, Handle<StandardMaterial>)> = None;
    let mut sprite_quad: Option<Handle<Mesh>> = None;
    let sprites: HashMap<&str, usize> = game
        .sim
        .io
        .iter()
        .flat_map(|io| io.ents.iter().enumerate())
        .filter_map(|(i, e)| (e.kind == aperture::io::Kind::Sprite).then_some((e.name.as_str(), i)))
        .collect();
    for line in text.lines() {
        let f: Vec<&str> = line.split('|').collect();
        if f.len() < 6 {
            continue;
        }
        // A glow sprite: a quad facing the camera, glowing additively in its colour.
        if f[0] == "sprite" {
            let v: Vec<f32> = f[3].split_whitespace().filter_map(|x| x.parse().ok()).collect();
            let Some(tex) = extracted.texture(&format!("map/{}", f[1])) else { continue };
            let (size, rgb, alpha) = (v.first().copied().unwrap_or(64.0) * 0.0254, Vec3::new(v.get(1).copied().unwrap_or(255.0), v.get(2).copied().unwrap_or(255.0), v.get(3).copied().unwrap_or(255.0)) / 255.0, v.get(4).copied().unwrap_or(255.0) / 255.0);
            let mover = movers.get(f[4]).copied();
            let at = aperture::source::to_ours(floats(f[2])) + mover.and_then(|m| game.sim.movers.get(m)).map_or(Vec3::ZERO, |m| m.def.shift);
            let mat = materials.add(StandardMaterial {
                base_color: Color::linear_rgb(rgb.x * alpha * SPRITE_GLOW, rgb.y * alpha * SPRITE_GLOW, rgb.z * alpha * SPRITE_GLOW),
                base_color_texture: Some(images.add(tex)),
                alpha_mode: AlphaMode::Add,
                unlit: true,
                double_sided: true,
                cull_mode: None,
                ..default()
            });
            let ent = sprites.get(f[5]).map(|&e| (e, Handle::default()));
            commands.spawn((
                ChamberGeom,
                MapProp { base: at, turn: Quat::IDENTITY, mover, animated: ent, model: f[1].to_string(), skin: 0 },
                MapSprite,
                Mesh3d(sprite_quad.get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0))).clone()),
                MeshMaterial3d(mat),
                Transform::from_translation(at).with_scale(Vec3::splat(size)),
                Visibility::default(),
                NotShadowCaster,
            ));
            continue;
        }
        let Some(path) = extracted.model(f[1]) else { continue };
        let mover = movers.get(f[4]).copied();
        let shift = mover.and_then(|m| game.sim.movers.get(m)).map_or(Vec3::ZERO, |m| m.def.shift);
        let at = aperture::source::to_ours(floats(f[2])) + shift;
        let animated = animated.get(f[5]).map(|&e| (e, assets.load(path.clone())));
        let (model, skin) = match f[1].rsplit_once("_skin").and_then(|(m, k)| Some((m, k.parse::<u32>().ok()?))) {
            Some((m, k)) => (m.to_string(), k),
            None => (f[1].to_string(), 0),
        };
        let turn = source_rotation(floats(f[3]));
        // A caught pellet in its catcher (Portal's `combineball`, sprite cards): drawn as the
        // game's own pellets are, a glowing ball, shown when the logic enables it.
        if f[1].ends_with("effects/combineball") {
            commands
                .spawn((
                    ChamberGeom,
                    MapProp { base: at, turn, mover, animated: animated.clone(), model: model.clone(), skin },
                    Visibility::default(),
                    Transform::from_translation(at).with_scale(Vec3::splat(aperture::sim::PELLET_RADIUS)),
                    PointLight { intensity: 40_000.0, range: 7.0, radius: 0.15, color: Color::srgb(1.0, 0.42, 0.05), shadow_maps_enabled: false, ..default() },
                ))
                .with_children(|c| {
                    c.spawn((Mesh3d(m.pellet_mesh.clone()), MeshMaterial3d(m.pellet.clone()), Transform::default(), NotShadowCaster));
                    c.spawn((Mesh3d(m.pellet_mesh.clone()), MeshMaterial3d(m.pellet_core.clone()), Transform::from_scale(Vec3::splat(0.42)), NotShadowCaster));
                });
            continue;
        }
        let mut spawned = commands.spawn((
            ChamberGeom,
            MapProp { base: at, turn, mover, animated, model, skin },
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
            Visibility::default(),
            Transform::from_translation(at).with_rotation(turn),
        ));
        // The shaft lights stay where they are, so the car goes dark as it leaves them, and
        // its doors shut them out. A light on the car rides with it (`move_models` moves this
        // prop): a lamp under its roof (the model's origin is its floor's middle, its roof 3.6 m
        // up). Ray tracing lights by what glows, so the lamp glows; the ordinary path has the
        // point light too.
        if f[1].contains("round_elevator_body") {
            let lamp = cabin_lamp.get_or_insert_with(|| {
                let warm = LinearRgba::rgb(1.0, 0.93, 0.82) * CABIN_GLOW;
                (meshes.add(Cylinder::new(0.42, 0.03)), materials.add(StandardMaterial { base_color: Color::WHITE, emissive: warm, ..default() }))
            });
            let lamp = lamp.clone();
            spawned.with_children(|c| {
                c.spawn((
                    PointLight { intensity: 80_000.0, color: Color::srgb(1.0, 0.95, 0.88), range: 4.5, radius: 0.0, shadow_maps_enabled: false, ..default() },
                    Transform::from_xyz(0.0, 1.45, 0.0),
                ));
                c.spawn((Mesh3d(lamp.0), MeshMaterial3d(lamp.1), Transform::from_xyz(0.0, 3.3, 0.0), NotShadowCaster));
            });
        }
    }
    // Its overlays (decals: indicator light strips, signs, stains), each a quad just off its
    // surface. A named one has its own material, so its strip follows its own door.
    let text = std::fs::read_to_string(root.join("maps").join(format!("{name}.overlays"))).unwrap_or_default();
    let mut overlay_mats: HashMap<String, Handle<StandardMaterial>> = HashMap::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('|').collect();
        if f.len() < 5 {
            continue;
        }
        let (mat, flags, target) = (f[0], f[1].parse::<u8>().unwrap_or(0), f[2].to_ascii_lowercase());
        let v: Vec<f32> = f[3].split_whitespace().filter_map(|x| x.parse().ok()).collect();
        let n = floats(f[4]);
        if v.len() < 20 || n == Vec3::ZERO {
            continue;
        }
        let lift = n * OVERLAY_LIFT;
        let pos: Vec<[f32; 3]> = (0..4).map(|k| (Vec3::new(v[k * 5], v[k * 5 + 1], v[k * 5 + 2]) + lift).to_array()).collect();
        let uv: Vec<[f32; 2]> = (0..4).map(|k| [v[k * 5 + 3], v[k * 5 + 4]]).collect();
        // (Wound to face the way it faces.)
        let (a, b, c) = (Vec3::from(pos[0]), Vec3::from(pos[1]), Vec3::from(pos[2]));
        let idx = if (b - a).cross(c - a).dot(n) >= 0.0 { vec![0, 1, 2, 0, 2, 3] } else { vec![0, 2, 1, 0, 3, 2] };
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![n.to_array(); 4])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_indices(Indices::U32(idx));
        let _ = mesh.generate_tangents();
        let key = format!("{mat}|{target}");
        let handle = overlay_mats
            .entry(key)
            .or_insert_with(|| {
                let base = material(mat, flags, &extracted, &mut images, &mut materials, &m, &mut lamps);
                let mut own = materials.get(&base).cloned().unwrap_or_default();
                // A decal: blended over what it's on (or darkening it: DecalModulate), in front.
                own.alpha_mode = if flags & 64 != 0 { AlphaMode::Multiply } else if flags & 1 != 0 { AlphaMode::Blend } else if flags & 2 != 0 { AlphaMode::Mask(0.5) } else { own.alpha_mode };
                own.depth_bias = 1.0;
                materials.add(own)
            })
            .clone();
        let frames: Vec<Handle<Image>> = if target.is_empty() { Vec::new() } else { (1..64).map_while(|k| extracted.texture(&format!("map/{mat}_frame{k}"))).map(|t| images.add(t)).collect() };
        let mut e = commands.spawn((ChamberGeom, Mesh3d(meshes.add(mesh)), MeshMaterial3d(handle.clone()), Transform::default(), NotShadowCaster));
        if let (false, Some(first)) = (frames.is_empty(), materials.get(&handle).and_then(|m| m.base_color_texture.clone())) {
            e.insert(TextureFrames { key: FrameKey::Name(target.clone()), frames: std::iter::once(first).chain(frames).collect(), shown: 0 });
        }
    }
    // The map's lights, brightest first.
    let mut lights = level.lights.clone();
    lights.sort_by(|a, b| b.brightness.total_cmp(&a.brightness));
    let bulb = crate::raytrace::active().then(|| {
        let mut m = Sphere::new(BULB_RADIUS).mesh().ico(1).unwrap();
        crate::raytrace::make_ready(&mut m);
        meshes.add(m)
    });
    for l in lights.iter().take(MAX_LIGHTS) {
        let color = Color::linear_rgb(l.color.x, l.color.y, l.color.z);
        let intensity = l.brightness * LUMENS_PER_BRIGHTNESS;
        // Ray tracing lights by what glows, not by lights: a bulb where each one is. It's in
        // the traced scene only, not drawn: Portal's lights are often fill lights in mid-air.
        if let Some(bulb) = &bulb {
            if l.brightness >= BULB_MIN_BRIGHTNESS {
                let glow = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::from_vec3(l.color * l.brightness * BULB_GLOW), ..default() });
                commands.spawn((ChamberGeom, RaytracingMesh3d(bulb.clone()), MeshMaterial3d(glow), Transform::from_translation(l.at)));
            }
        }
        match l.spot {
            Some((dir, cone)) => {
                commands.spawn((
                    ChamberGeom,
                    SpotLight { intensity, color, range: 25.0, radius: 0.0, outer_angle: cone.min(1.5), inner_angle: cone.min(1.5) * 0.7, shadow_maps_enabled: false, ..default() },
                    Transform::from_translation(l.at).looking_to(dir, if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y }),
                ));
            }
            None => {
                commands.spawn((ChamberGeom, PointLight { intensity, color, range: 20.0, radius: 0.0, shadow_maps_enabled: false, ..default() }, Transform::from_translation(l.at)));
            }
        }
    }
}

/// Brush models (and props parented to them) follow their movers: doors slide, elevators
/// ride, walls come and go.
pub fn move_models(game: Res<Game>, mut q: Query<(&MapModel, &mut Transform, &mut Visibility)>, mut props: Query<(&MapProp, &mut Transform, Has<MapSprite>), Without<MapModel>>) {
    let movers = &game.sim.movers;
    for (p, mut tf, sprite) in &mut props {
        let Some(mv) = p.mover.and_then(|m| movers.get(m)) else { continue };
        let (at, turn) = match mv.def.swing {
            Some(sw) => {
                let r = sw.rotation(mv.at.x);
                (mv.carried + sw.pivot + r * (p.base - sw.pivot), r * p.turn)
            }
            None => (p.base + mv.offset(), p.turn),
        };
        // (A sprite turns to the camera, not with what it rides.)
        let turn = if sprite { tf.rotation } else { turn };
        if tf.translation != at || tf.rotation != turn {
            tf.translation = at;
            tf.rotation = turn;
        }
    }
    for (m, mut tf, mut vis) in &mut q {
        let Some(mv) = movers.iter().find(|v| v.def.model == m.0) else { continue };
        let want = match mv.def.swing {
            // A swinging door turns about its hinge.
            Some(sw) => {
                let r = sw.rotation(mv.at.x);
                Transform { translation: mv.carried + sw.pivot - r * sw.pivot, rotation: r, ..default() }
            }
            None => Transform::from_translation(mv.offset()),
        };
        if tf.translation != want.translation || tf.rotation != want.rotation {
            tf.translation = want.translation;
            tf.rotation = want.rotation;
        }
        let want = if mv.present { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
}

/// One of a map's glow sprites (env_sprite): it turns to face the camera.
#[derive(Component)]
pub struct MapSprite;

/// How bright a sprite glows over its colour (additive, so the tonemap lifts it to a glow).
const SPRITE_GLOW: f32 = 1.5;

/// Sprites face the camera.
pub fn face_sprites(camera: Query<&GlobalTransform, With<crate::PlayerCamera>>, mut sprites: Query<&mut Transform, With<MapSprite>>) {
    let Ok(cam) = camera.single() else { return };
    let turn = cam.compute_transform().rotation;
    for mut tf in &mut sprites {
        if tf.rotation != turn {
            tf.rotation = turn;
        }
    }
}

/// A prop the map's logic has disabled (or that starts so) isn't shown.
pub fn show_props(game: Res<Game>, mut props: Query<(&MapProp, &mut Visibility)>) {
    let Some(io) = &game.sim.io else { return };
    for (p, mut vis) in &mut props {
        let Some((e, _)) = p.animated.as_ref() else { continue };
        let want = if io.hidden.contains(e) { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want {
            *vis = want;
        }
    }
}

/// A prop shows the skin the map's logic last gave it (its model for that skin).
pub fn show_skins(game: Res<Game>, assets: Res<AssetServer>, extracted: Res<Extracted>, mut props: Query<(&mut MapProp, &mut WorldAssetRoot)>) {
    let Some(io) = &game.sim.io else { return };
    if io.skins.is_empty() {
        return;
    }
    for (mut p, mut root) in &mut props {
        let Some(&want) = p.animated.as_ref().and_then(|(e, _)| io.skins.get(e)) else { continue };
        if want == p.skin {
            continue;
        }
        p.skin = want;
        let name = if want > 0 { format!("{}_skin{want}", p.model) } else { p.model.clone() };
        if let Some(path) = extracted.model(&name) {
            root.0 = assets.load(GltfAssetLabel::Scene(0).from_asset(path));
        }
    }
}

/// Give each animated prop's player what it needs to play the map's animations.
pub fn tag_prop_players(mut commands: Commands, added: Query<Entity, Added<AnimationPlayer>>, parents: Query<&ChildOf>, props: Query<&MapProp>) {
    for e in &added {
        let mut cur = e;
        for _ in 0..8 {
            if let Ok(MapProp { animated: Some((ent, gltf)), .. }) = props.get(cur) {
                commands.entity(e).insert(MapPropPlayer { ent: *ent, gltf: gltf.clone(), played: 0, cycle: None, showing: 0, node: None });
                break;
            }
            let Ok(p) = parents.get(cur) else { break };
            cur = p.parent();
        }
    }
}

/// A pellet launcher's model: shut at rest, with an open clip and a close clip, and no
/// `opened` (trap housings ask for that).
fn is_pellet_launcher(gltf: &bevy::gltf::Gltf) -> bool {
    let n = &gltf.named_animations;
    n.contains_key("idle_closed") && n.contains_key("open") && n.contains_key("close") && !n.contains_key("opened")
}

fn clip_duration(clips: &Assets<AnimationClip>, gltf: &bevy::gltf::Gltf, name: &str) -> Option<f32> {
    gltf.named_animations.get(name).and_then(|h| clips.get(h)).map(|c| c.duration().max(1e-3))
}

/// Put `name` on the player, paused, so the simulation can seek it.
fn seek_clip(
    commands: &mut Commands,
    graphs: &mut Assets<AnimationGraph>,
    player: &mut AnimationPlayer,
    entity: Entity,
    gltf: &bevy::gltf::Gltf,
    name: &str,
) -> Option<AnimationNodeIndex> {
    let clip = gltf.named_animations.get(name)?.clone();
    let (graph, node) = AnimationGraph::from_clip(clip);
    commands.entity(entity).insert(AnimationGraphHandle(graphs.add(graph)));
    player.stop_all();
    player.start(node).pause();
    Some(node)
}

/// Play what the map's logic last told each prop to (once, holding its last frame). A pellet
/// launcher told to `close` (that's all the map sends when it fires) plays open, then close.
pub fn animate_props(
    mut commands: Commands,
    game: Res<Game>,
    gltfs: Res<Assets<bevy::gltf::Gltf>>,
    clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut players: Query<(Entity, &mut MapPropPlayer, &mut AnimationPlayer)>,
    mut clock: Local<Option<f32>>,
) {
    let dt = match *clock {
        Some(t) => (game.sim.time - t).max(0.0),
        None => 0.0,
    };
    *clock = Some(game.sim.time);
    let Some(io) = &game.sim.io else { return };
    for (e, mut pp, mut player) in &mut players {
        let Some(gltf) = gltfs.get(&pp.gltf) else { continue };
        if let Some((name, n)) = io.animations.get(&pp.ent) {
            if *n != pp.played {
                pp.played = *n;
                if name == "close" && is_pellet_launcher(gltf) {
                    pp.cycle = Some(0.0);
                    pp.showing = 0;
                } else {
                    pp.cycle = None;
                    // A clip the model hasn't got: Portal's catcher is told `open` and `opened`
                    // and has only `close`, `closed` and `idle`: its close backwards, then idle.
                    let n = &gltf.named_animations;
                    let (clip, backwards) = match name.as_str() {
                        k if n.contains_key(k) => (n.get(k), false),
                        "open" => (n.get("close"), true),
                        "opened" => (n.get("idle"), false),
                        _ => (None, false),
                    };
                    if let Some(clip) = clip {
                        let (graph, node) = AnimationGraph::from_clip(clip.clone());
                        commands.entity(e).insert(AnimationGraphHandle(graphs.add(graph)));
                        player.stop_all();
                        let active = player.start(node);
                        if backwards {
                            let len = clips.get(clip).map_or(1.0, |c| c.duration());
                            active.set_speed(-1.0).seek_to(len);
                        }
                        pp.node = Some(node);
                    }
                }
            }
        }
        let Some(since) = pp.cycle else { continue };
        let since = since + dt;
        let open = clip_duration(&clips, gltf, "open").unwrap_or(0.97);
        let close = clip_duration(&clips, gltf, "close").unwrap_or(0.97);
        let (opening, t) = crate::props::launcher_mouth(since, open, close);
        let want = if opening { 1 } else { 2 };
        if pp.showing != want {
            let name = if opening { "open" } else { "close" };
            if let Some(node) = seek_clip(&mut commands, &mut graphs, &mut player, e, gltf, name) {
                pp.node = Some(node);
                pp.showing = want;
            }
        }
        if let Some(node) = pp.node {
            if let Some(active) = player.animation_mut(node) {
                active.set_seek_time(t);
            }
        }
        pp.cycle = (since < open + close).then_some(since);
    }
}
