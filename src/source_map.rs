//! Portal's maps, as the game's own copies of them (level files in `levels/portal/`, listed
//! after the game's chambers): drawn here from their faces (a mesh per brush model and
//! material) with Portal's textures from `assets/extracted/`, and Portal with RTX's maps where
//! it has them. Light panels glow (under ray tracing they light the chamber); the map's light
//! entities are lights for the ordinary path.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

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
const MAPS: &[(&str, &str)] = aperture::port::MAPS;

/// GLaDOS's scenes (`maps/scenes.txt`), shared by every map and every level file with
/// Portal's entities.
pub fn scenes() -> Arc<aperture::source::Scenes> {
    static SCENES: OnceLock<Arc<aperture::source::Scenes>> = OnceLock::new();
    SCENES
        .get_or_init(|| {
            let text = crate::extracted::find_root().and_then(|r| std::fs::read_to_string(r.join("maps").join("scenes.txt")).ok()).unwrap_or_default();
            Arc::new(aperture::source::parse_scenes(&text))
        })
        .clone()
}

/// Each of Portal's materials' flags (`maps/materials.txt`: 1 translucent, 2 alphatest, 4
/// glows, 8 additive, 16 takes portals, 32 textured), for what the maps draw.
pub fn material_flags() -> &'static HashMap<String, u8> {
    static FLAGS: OnceLock<HashMap<String, u8>> = OnceLock::new();
    FLAGS.get_or_init(|| {
        let text = crate::extracted::find_root().and_then(|r| std::fs::read_to_string(r.join("maps").join("materials.txt")).ok()).unwrap_or_default();
        text.lines().filter_map(|l| l.split_once(' ')).filter_map(|(f, n)| Some((n.to_string(), f.parse().ok()?))).collect()
    })
}

/// The game's own copies of Portal's maps (`levels/portal/`, made by `portal_extract
/// --port-maps`), in the maps' order: each one's file and map.
pub fn native_files() -> Vec<(PathBuf, String)> {
    let dir = crate::editor::levels_dir().join("portal");
    MAPS.iter().map(|(name, _)| (dir.join(format!("{name}.ron")), name.to_string())).filter(|(p, _)| p.is_file()).collect()
}

/// The game's own copies of Mirror's Edge's maps (`levels/me/`, made by `portal_extract
/// --port-me`), in the maps' order: each one's file and name (its file's).
pub fn me_files() -> Vec<(PathBuf, String)> {
    let dir = crate::editor::levels_dir().join("me");
    aperture::me::MAPS.iter().map(|(_, file, _)| (dir.join(format!("{file}.ron")), file.to_string())).filter(|(p, _)| p.is_file()).collect()
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
    /// Its model's name (for the sounds its animations make).
    model: String,
    played: u32,
    /// Seconds into an open-then-close fire cycle (a pellet launcher's mouth).
    cycle: Option<f32>,
    /// Which clip that cycle is showing: 1 open, 2 close.
    showing: u8,
    node: Option<AnimationNodeIndex>,
}

/// Materials made for maps' textures, kept from map to map.
#[derive(Resource, Default)]
pub struct MapMats(HashMap<String, Handle<StandardMaterial>>);

/// Materials that light the chambers, as the chambers' ceiling lamps do (ray tracing gives
/// them the lamps' extra boost): Portal's light panels.
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

/// The material for one of a map's textures.
fn material(name: &str, flags: u8, extracted: &Extracted, images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>, m: &ChamberMats, lamps: &mut Lamps) -> Handle<StandardMaterial> {
    if name.starts_with("glass/") {
        return if name.contains("frosted") { m.frosted.clone() } else { m.glass.clone() };
    }
    let (translucent, alphatest, glows, additive) = (flags & 1 != 0, flags & 2 != 0, flags & 4 != 0, flags & 8 != 0);
    let tex = |n: &str, srgb: bool| if srgb { extracted.texture(n) } else { extracted.texture_data(n) };
    let base_img = tex(&format!("map/{name}"), true);
    let see_through = translucent || alphatest || additive;
    // What glows of it: Portal with RTX's emissive map where it has one; else, on an opaque
    // $selfillum material, where its alpha says (a gradient wall glows along its strip, not
    // all over); else all of it.
    let glow = if !glows {
        None
    } else if let Some(t) = tex(&format!("map/{name}_emissive"), true) {
        Some(images.add(t))
    } else {
        base_img.as_ref().filter(|_| !see_through).and_then(glow_mask).map(|t| images.add(t))
    };
    let base = base_img.map(|t| images.add(t));
    let normal = tex(&format!("map/{name}_normal"), false).map(|t| images.add(t));
    let mr = tex(&format!("map/{name}_mr"), false).map(|t| images.add(t));
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
        emissive_texture: if glows { glow.or(base) } else { None },
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
    // Only Portal's light textures light its rooms (they're what its lights.rad lists); a sign,
    // a hazard stripe, a wall's glowing strip shows but lights nothing much.
    if glows && name.starts_with("lights/") {
        lamps.0.insert(h.id());
    }
    h
}

/// A $selfillum texture's glow: its colour where its alpha says. None if it's opaque all
/// over (then all of it glows).
fn glow_mask(img: &Image) -> Option<Image> {
    let data = img.data.as_ref()?;
    if img.texture_descriptor.format.block_copy_size(None) != Some(4) || data.chunks_exact(4).all(|p| p[3] == 255) {
        return None;
    }
    let mut out = img.clone();
    for p in out.data.as_mut()?.chunks_exact_mut(4) {
        let a = p[3] as u32;
        for c in &mut p[..3] {
            *c = (*c as u32 * a / 255) as u8;
        }
        p[3] = 255;
    }
    Some(out)
}

/// A map's faces to draw: (brush model, material, its flags, the mesh), from its brushes.
fn face_meshes(art: &aperture::art::MapArt) -> Vec<(u32, String, u8, Mesh)> {
    let flags = material_flags();
    art.faces
        .iter()
        .map(|g| {
            let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, g.pos.clone())
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, g.nrm.clone())
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, g.uv.clone())
                .with_inserted_indices(Indices::U32(g.idx.clone()));
            // (A material no map has: drawn as one with a texture, if it's been extracted.)
            (g.model as u32, g.mat.clone(), flags.get(&g.mat).copied().unwrap_or(32), mesh)
        })
        .collect()
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
    mut scaled: Option<ResMut<crate::raytrace::ScaledEmissive>>,
) {
    if *drawn == Some(game.loads) {
        return;
    }
    let Some(m) = m else { return };
    // A material of its own made from a shared one (cached from an earlier map) whose glow the
    // traced light has scaled already: it's scaled as that one is, not again.
    let mut same_glow = |from: &Handle<StandardMaterial>, to: &Handle<StandardMaterial>| {
        if let Some(s) = scaled.as_mut() {
            if let Some(v) = s.0.get(&from.id()).copied() {
                s.0.insert(to.id(), v);
            }
        }
    };
    *drawn = Some(game.loads);
    let level = &game.sim.level;
    let Some(art) = level.art.clone() else { return };
    let name = level.source.clone().unwrap_or_default();
    if extracted.root.is_none() {
        return;
    }
    let mut frames: HashMap<String, Vec<Handle<Image>>> = HashMap::new();
    for (model, mat, flags, mut mesh) in face_meshes(&art) {
        if level.hidden_models.contains(&(model as usize)) {
            continue;
        }
        // Goo is drawn as the level's goo.
        if mat == "nature/hazard_liquid" {
            continue;
        }
        if mesh.generate_tangents().is_err() {
            warn!("{name}: no tangents for {mat}");
        }
        let shared = match cache.0.get(&mat) {
            Some(h) => h.clone(),
            None => {
                let h = material(&mat, flags, &extracted, &mut images, &mut materials, &m, &mut lamps);
                cache.0.insert(mat.clone(), h.clone());
                h
            }
        };
        // A texture the logic switches between frames (a door's state panel: X, then the check)
        // on a brush of its own: its own material, so each panel follows its own door.
        let mut handle = shared.clone();
        let mut switched = None;
        if model != 0 {
            let frames = frames.entry(mat.clone()).or_insert_with(|| (1..64).map_while(|k| extracted.texture(&format!("map/{mat}_frame{k}"))).map(|t| images.add(t)).collect::<Vec<_>>()).clone();
            if let Some(own) = (!frames.is_empty()).then(|| materials.get(&handle).cloned()).flatten() {
                let first = own.base_color_texture.clone();
                handle = materials.add(own);
                same_glow(&shared, &handle);
                if lamps.0.contains(&shared.id()) {
                    lamps.0.insert(handle.id());
                }
                switched = first.map(|f| TextureFrames { key: FrameKey::Model(model as usize), frames: std::iter::once(f).chain(frames).collect(), shown: 0 });
            }
        }
        let mut e = commands.spawn((ChamberGeom, Mesh3d(meshes.add(mesh)), MeshMaterial3d(handle), Transform::default()));
        if let Some(f) = switched {
            e.insert(f);
        }
        if flags & (1 | 8) != 0 || mat.starts_with("glass/") {
            e.insert(NotShadowCaster);
        }
        if model != 0 {
            e.insert(MapModel(model as usize));
        }
    }
    // Its props: models where the map puts them, riding with what they're parented to. (The
    // logic's names, lower case.)
    let of_kind = |kind: aperture::io::Kind| -> HashMap<String, usize> {
        game.sim.io.iter().flat_map(|io| io.ents.iter().enumerate()).filter(|(_, e)| e.kind == kind && !e.name.is_empty()).map(|(i, e)| (e.name.to_ascii_lowercase(), i)).collect()
    };
    let animated = of_kind(aperture::io::Kind::Animated);
    let movers: HashMap<String, usize> = game
        .sim
        .io
        .iter()
        .flat_map(|io| io.ents.iter())
        .filter_map(|e| match e.kind {
            aperture::io::Kind::Mover { index, .. } if !e.name.is_empty() => Some((e.name.to_ascii_lowercase(), index)),
            _ => None,
        })
        .collect();
    let sprites = of_kind(aperture::io::Kind::Sprite);
    // Physics props knocked about: the sim's prop each one's art is.
    let bodies: HashMap<usize, (u32, Vec3, Quat)> = level.bodies.iter().zip(&game.sim.bodies).map(|(b, id)| (b.art, (*id, b.origin, b.turn))).collect();
    let mut cabin_lamp: Option<(Handle<Mesh>, Handle<StandardMaterial>)> = None;
    let mut sprite_quad: Option<Handle<Mesh>> = None;
    let (mut grill_mats, mut grill_seed) = (crate::grill_fx::Mats::default(), 1u32);
    for (pi, p) in art.props.iter().enumerate() {
        let mover = movers.get(&p.parent).copied();
        let shift = mover.and_then(|m| game.sim.movers.get(m)).map_or(Vec3::ZERO, |m| m.def.shift);
        match &p.kind {
            // An emancipation grill's particles.
            aperture::art::PropKind::Particles { .. } => {
                if let Some(kind) = crate::grill_fx::Kind::of(&p.model) {
                    let seed = grill_seed;
                    grill_seed += 1;
                    crate::grill_fx::spawn(&mut commands, &mut meshes, &mut materials, &mut images, &extracted, &mut grill_mats, &game.sim, kind, p.at, p.turn, seed);
                }
            }
            // A glow sprite: a quad facing the camera, glowing additively in its colour.
            aperture::art::PropKind::Sprite { size, color, alpha } => {
                let Some(tex) = extracted.texture(&format!("map/{}", p.model)) else { continue };
                let at = p.at + shift;
                let k = alpha * SPRITE_GLOW;
                let mat = materials.add(StandardMaterial {
                    base_color: Color::linear_rgb(color.x * k, color.y * k, color.z * k),
                    base_color_texture: Some(images.add(tex)),
                    alpha_mode: AlphaMode::Add,
                    unlit: true,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                });
                let ent = sprites.get(&p.name).map(|&e| (e, Handle::default()));
                commands.spawn((
                    ChamberGeom,
                    MapProp { base: at, turn: Quat::IDENTITY, mover, animated: ent, model: p.model.clone(), skin: 0 },
                    MapSprite,
                    Mesh3d(sprite_quad.get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0))).clone()),
                    MeshMaterial3d(mat),
                    Transform::from_translation(at).with_scale(Vec3::splat(*size)),
                    Visibility::default(),
                    NotShadowCaster,
                ));
            }
            // (Effects and cameras are `map_fx`'s.)
            aperture::art::PropKind::Spotlight { .. } | aperture::art::PropKind::Steam { .. } | aperture::art::PropKind::Sparks { .. } | aperture::art::PropKind::Beam { .. } | aperture::art::PropKind::Camera => {}
            kind => {
                let Some(path) = extracted.model(&p.model) else { continue };
                let at = p.at + shift;
                let animated = animated.get(&p.name).map(|&e| (e, assets.load(path.clone())));
                let (model, skin) = match p.model.rsplit_once("_skin").and_then(|(m, k)| Some((m, k.parse::<u32>().ok()?))) {
                    Some((m, k)) => (m.to_string(), k),
                    None => (p.model.clone(), 0),
                };
                let turn = p.turn;
                // A caught pellet in its catcher (Portal's `combineball`, sprite cards): drawn as
                // the game's own pellets are, a glowing ball, shown when the logic enables it.
                if p.model.ends_with("effects/combineball") {
                    commands
                        .spawn((
                            ChamberGeom,
                            MapProp { base: at, turn, mover, animated: animated.clone(), model: model.clone(), skin },
                            Visibility::default(),
                            Transform::from_translation(at).with_scale(Vec3::splat(aperture::sim::PELLET_RADIUS)),
                            PointLight { intensity: crate::chamber::PELLET_LIGHT, range: 9.0, radius: 0.1, color: Color::srgb(1.0, 0.5, 0.15), shadow_maps_enabled: false, ..default() },
                        ))
                        .with_children(|c| crate::chamber::pellet_ball(c, &m));
                    continue;
                }
                // A physics prop that's a body of the sim's: drawn where that is.
                if let Some(&(id, origin, turn0)) = bodies.get(&pi) {
                    commands.spawn((ChamberGeom, crate::chamber::PlacedAt(p.at), BodyProp { id, origin, turn0 }, WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))), Visibility::default(), Transform::from_translation(at).with_rotation(turn)));
                    continue;
                }
                // A loose prop on one of the map's cubes stands on ours.
                let perched = match kind {
                    aperture::art::PropKind::Physics { bottom, .. } => aperture::source::Perched::find(&game.sim, at, *bottom),
                    _ => None,
                };
                let at = perched.map_or(at, |q| q.at);
                let mut spawned = commands.spawn((
                    ChamberGeom,
                    crate::chamber::PlacedAt(p.at),
                    MapProp { base: at, turn, mover, animated, model, skin },
                    WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
                    Visibility::default(),
                    Transform::from_translation(at).with_rotation(turn),
                ));
                if let Some(q) = perched {
                    spawned.insert(Perch(q));
                }
                // The shaft lights stay where they are, so the car goes dark as it leaves them,
                // and its doors shut them out. A light on the car rides with it (`move_models`
                // moves this prop): a lamp under its roof (the model's origin is its floor's
                // middle, its roof 3.6 m up). Ray tracing lights by what glows, so the lamp
                // glows; the ordinary path has the point light too.
                if p.model.contains("round_elevator_body") {
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
        }
    }
    // Its cables, hung as Portal hangs them (what hangs from them: Chamber 16's cage lights).
    if !art.ropes.is_empty() {
        let cable = materials.add(StandardMaterial { base_color: Color::srgb(0.03, 0.03, 0.03), perceptual_roughness: 0.6, ..default() });
        commands.spawn((ChamberGeom, Mesh3d(meshes.add(cable_mesh(&art.ropes))), MeshMaterial3d(cable), Transform::default(), NotShadowCaster));
    }
    // Its overlays (decals: indicator light strips, signs, stains), each a quad just off its
    // surface. A named one has its own material, so its strip follows its own door.
    let mut overlay_mats: HashMap<String, Handle<StandardMaterial>> = HashMap::new();
    for o in &art.overlays {
        let (mat, target, n) = (o.mat.as_str(), o.name.clone(), o.normal);
        let flags = o.flags.or_else(|| material_flags().get(mat).copied()).unwrap_or(32 | 1);
        let lift = n * OVERLAY_LIFT;
        let pos: Vec<[f32; 3]> = o.corners.iter().map(|(c, _)| (*c + lift).to_array()).collect();
        let uv: Vec<[f32; 2]> = o.corners.iter().map(|(_, uv)| *uv).collect();
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
                let h = materials.add(own);
                same_glow(&base, &h);
                h
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
    for l in lights.iter().take(MAX_LIGHTS) {
        let color = Color::linear_rgb(l.color.x, l.color.y, l.color.z);
        let intensity = l.brightness * LUMENS_PER_BRIGHTNESS;
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

/// How many sides a cable has round.
const CABLE_SIDES: usize = 6;

/// The map's cables as one mesh: a thin tube along each.
fn cable_mesh(ropes: &[aperture::rope::Rope]) -> Mesh {
    let (mut pos, mut nrm, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for r in ropes {
        let n = r.points.len();
        if n < 2 {
            continue;
        }
        let base = pos.len() as u32;
        let mut along = 0.0;
        for (i, &p) in r.points.iter().enumerate() {
            let ahead = (r.points[(i + 1).min(n - 1)] - r.points[i.saturating_sub(1)]).normalize_or(Vec3::X);
            let side = ahead.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
            let up = side.cross(ahead);
            if i > 0 {
                along += p.distance(r.points[i - 1]);
            }
            for k in 0..=CABLE_SIDES {
                let a = k as f32 / CABLE_SIDES as f32 * std::f32::consts::TAU;
                let out = side * a.cos() + up * a.sin();
                pos.push((p + out * r.radius).to_array());
                nrm.push(out.to_array());
                uv.push([k as f32 / CABLE_SIDES as f32, along]);
            }
        }
        let ring = (CABLE_SIDES + 1) as u32;
        for i in 0..n as u32 - 1 {
            for k in 0..CABLE_SIDES as u32 {
                let (a, b) = (base + i * ring + k, base + (i + 1) * ring + k);
                idx.extend([a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx));
    let _ = mesh.generate_tangents();
    mesh
}

/// A physics prop the sim knocks about: its prop's id, where the model's origin is from the
/// prop's middle (as it was turned to start with), and that turn.
#[derive(Component)]
pub struct BodyProp {
    id: u32,
    origin: Vec3,
    turn0: Quat,
}

/// Physics props go where the sim has them (turned as it's turned them), gone when they are.
pub fn move_bodies(game: Res<Game>, mut q: Query<(&BodyProp, &mut Transform, &mut Visibility)>) {
    for (b, mut tf, mut vis) in &mut q {
        let p = game.sim.props.iter().find(|p| p.id == b.id && p.alive);
        let want = if p.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if let Some(p) = p {
            // (Its turn in the sim is from as it stood.)
            tf.translation = p.pos + p.rot * b.origin;
            tf.rotation = p.rot * b.turn0;
        }
    }
}

/// A map light's bulb (see [`bulbs`]).
#[derive(Component)]
pub struct Bulb;

/// Ray tracing lights by what glows, not by lights: under it, a bulb where each of the map's
/// brighter lights is, in the traced scene only, not drawn (Portal's lights are often fill
/// lights in mid-air). Made when a map loads or the lighting's switched to traced, gone when
/// it's switched back.
pub fn bulbs(
    mut commands: Commands,
    game: Res<Game>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<Bulb>>,
    mut built: Local<Option<Option<u64>>>,
) {
    let want = crate::raytrace::active().then_some(game.loads);
    if *built == Some(want) {
        return;
    }
    *built = Some(want);
    for e in &existing {
        commands.entity(e).despawn();
    }
    if want.is_none() {
        return;
    }
    let mut lights = game.sim.level.lights.clone();
    lights.sort_by(|a, b| b.brightness.total_cmp(&a.brightness));
    let mut bulb = None;
    for l in lights.iter().take(MAX_LIGHTS).filter(|l| l.brightness >= BULB_MIN_BRIGHTNESS) {
        let bulb = bulb
            .get_or_insert_with(|| {
                let mut m = Sphere::new(BULB_RADIUS).mesh().ico(1).unwrap();
                crate::raytrace::make_ready(&mut m);
                meshes.add(m)
            })
            .clone();
        let glow = materials.add(StandardMaterial { base_color: Color::BLACK, emissive: LinearRgba::from_vec3(l.color * l.brightness * BULB_GLOW), ..default() });
        commands.spawn((ChamberGeom, Bulb, RaytracingMesh3d(bulb), MeshMaterial3d(glow), Transform::from_translation(l.at)));
    }
}

/// Brush models (and props parented to them) follow their movers: doors slide, elevators
/// ride, walls come and go.
pub fn move_models(game: Res<Game>, mut q: Query<(&MapModel, &mut Transform, &mut Visibility)>, mut props: Query<(&MapProp, &mut Transform, Has<MapSprite>, &mut Visibility), Without<MapModel>>) {
    let movers = &game.sim.movers;
    for (p, mut tf, sprite, mut vis) in &mut props {
        let Some(mv) = p.mover.and_then(|m| movers.get(m)) else { continue };
        // (Gone with what it rides: a rail cart at the end of its track.)
        if !mv.present && *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        } else if mv.present && *vis == Visibility::Hidden && p.animated.is_none() {
            *vis = Visibility::Inherited;
        }
        let (at, turn) = match (mv.def.swing, &mv.def.spin) {
            (Some(sw), _) => {
                let r = sw.rotation(mv.at.x);
                (mv.carried + sw.pivot + r * (p.base - sw.pivot), r * p.turn)
            }
            // On a fan: round with it.
            (None, Some(spin)) => {
                let r = Quat::from_axis_angle(spin.axis, mv.spin_angle);
                (mv.offset() + spin.pivot + r * (p.base - spin.pivot), r * p.turn)
            }
            (None, None) => (p.base + mv.offset(), p.turn),
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
        let want = match (mv.def.swing, &mv.def.spin) {
            // A swinging door turns about its hinge.
            (Some(sw), _) => {
                let r = sw.rotation(mv.at.x);
                Transform { translation: mv.carried + sw.pivot - r * sw.pivot, rotation: r, ..default() }
            }
            // A fan spins about its middle.
            (None, Some(spin)) => {
                let r = Quat::from_axis_angle(spin.axis, mv.spin_angle);
                Transform { translation: mv.offset() + spin.pivot - r * spin.pivot, rotation: r, ..default() }
            }
            (None, None) => Transform::from_translation(mv.offset()),
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

/// A loose prop standing on one of the map's cubes (see [`aperture::source::Perched`]).
#[derive(Component)]
pub struct Perch(aperture::source::Perched);

/// Loose props ride the cubes they're on, and drop off them.
pub fn perch_props(game: Res<Game>, time: Res<Time>, mut props: Query<(&mut Perch, &mut Transform)>) {
    let dt = time.delta_secs().min(0.1);
    for (mut p, mut tf) in &mut props {
        p.0.step(&game.sim, dt);
        if tf.translation != p.0.at {
            tf.translation = p.0.at;
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
            if let Ok(MapProp { animated: Some((ent, gltf)), model, .. }) = props.get(cur) {
                commands.entity(e).insert(MapPropPlayer { ent: *ent, gltf: gltf.clone(), model: model.clone(), played: 0, cycle: None, showing: 0, node: None });
                break;
            }
            let Ok(p) = parents.get(cur) else { break };
            cur = p.parent();
        }
    }
}

/// A pellet launcher's model (Portal's `combine_ball_launcher`). (Not known by its clips:
/// the round elevator's doors have the same open, close and idle_closed, and told to close,
/// they'd fire as a launcher does, opening first, #58.)
fn is_pellet_launcher(model: &str) -> bool {
    model.ends_with("/combine_ball_launcher")
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
    mut players: Query<(Entity, &mut MapPropPlayer, &mut AnimationPlayer, &GlobalTransform)>,
    mut clock: Local<Option<f32>>,
    mut sounds: ResMut<crate::portal_audio::PropSounds>,
) {
    let dt = match *clock {
        Some(t) => (game.sim.time - t).max(0.0),
        None => 0.0,
    };
    *clock = Some(game.sim.time);
    let Some(io) = &game.sim.io else { return };
    for (e, mut pp, mut player, tf) in &mut players {
        let Some(gltf) = gltfs.get(&pp.gltf) else { continue };
        if let Some((name, n)) = io.animations.get(&pp.ent) {
            if *n != pp.played {
                pp.played = *n;
                if name == "close" && is_pellet_launcher(&pp.model) {
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
                        let len = clips.get(clip).map_or(1.0, |c| c.duration());
                        sounds.animation(&pp.model, name, game.sim.time, len, tf.translation());
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
