//! Mirror's Edge's own props, read from the player's install (nothing ships with the game):
//! the static meshes a level places with `Level::me_prop` (AC units, crates, vents,
//! billboards...), and the steel ladder on every ladder. Each mesh is read once and kept.
//! Without the game, levels look as they always did: a prop's stand-in box (if it has one),
//! and the ladders' plain rods.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use faith_move::Fixture;
use me_assets::{MaterialSlot, Packages, StaticMesh};

use crate::chamber::ChamberGeom;
use crate::me_viewmodel::roughness_image;
use crate::Game;

/// The ladder drawn on ladders, and how tall one piece of it is.
const LADDER: (&str, &str) = ("P_Generic", "S_SteelLadder_01b");
const LADDER_STEP: f32 = 2.56;

/// A zipline's cable and a swing pole's pipe: round, in the steel of Mirror's Edge's
/// ladders (the game has no meshes of its own for them: they're its levels' geometry).
const CABLE_RADIUS: f32 = 0.012;
const PIPE_RADIUS: f32 = 0.03;

/// The biggest a prop's textures are read (their mips that size, from the game's files): the
/// texture detail setting's (it's taken as a level's props are read).
static ME_TEXTURE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1024);

fn me_texture() -> u32 {
    ME_TEXTURE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Stands in for the ziplines and swing poles (see [`StandIn`]).
pub const LINES: usize = usize::MAX - 1;

/// Marks what stands in for a Mirror's Edge prop (its index in the level's `me_props`), for a
/// ladder (`usize::MAX`), or for a zipline or swing pole ([`LINES`]), hidden once the real
/// thing is drawn.
#[derive(Component)]
pub struct StandIn(pub usize);

/// A zipline's cable or a swing pole as straight pieces to draw: a cable that sags, along its
/// curve.
pub fn pieces(f: &Fixture) -> Vec<(Vec3, Vec3)> {
    match *f {
        Fixture::ZipLine { a, b, sag } if sag != 0.0 => {
            let pts = faith_move::Cable::new(a, b, sag).points(24);
            pts.windows(2).map(|w| (w[0], w[1])).collect()
        }
        Fixture::ZipLine { a, b, .. } | Fixture::SwingPole { a, b } => vec![(a, b)],
        _ => vec![],
    }
}

/// The game's packages (not thread-safe: they share what they've opened).
pub struct MePackages(Packages);

pub type Parts = Vec<(Handle<Mesh>, Handle<StandardMaterial>)>;

/// What a mesh's materials found: how many have a texture, how many only a plain colour, how
/// many have no material at all (a level's placement gives them one), of how many, and the
/// names of those with nothing.
#[derive(Clone, Default)]
pub struct MeLooks {
    pub textured: usize,
    pub plain: usize,
    pub unset: usize,
    pub total: usize,
    pub missing: Vec<String>,
    /// Its box (ours, from its origin).
    pub bounds: Option<(Vec3, Vec3)>,
}

#[derive(Resource, Default)]
pub struct MePropCache {
    meshes: HashMap<(&'static str, &'static str), Option<Parts>>,
    /// Meshes with some of their materials replaced (a placed mesh's own), by mesh and its
    /// replacements.
    overridden: HashMap<(&'static str, &'static str, Vec<&'static str>), Option<Parts>>,
    /// Materials whose meshes block traced light though they're drawn see-through (a
    /// building's glass).
    occluders: std::collections::HashSet<AssetId<StandardMaterial>>,
    /// Remastered materials (#101), by material and how many times their detail tiles.
    remastered: HashMap<(String, u32), Handle<StandardMaterial>>,
    /// The same, wound the other way round, for where a level mirrors a mesh (scales it by
    /// a negative amount an odd number of times): mirrored, its faces would face inwards.
    mirrored: HashMap<(&'static str, &'static str), Parts>,
    /// Each mesh's box (ours, from its origin), once it's been read.
    bounds: HashMap<(&'static str, &'static str), (Vec3, Vec3)>,
    /// What each mesh's materials found (`package.mesh`), once it's been read.
    looks: HashMap<String, MeLooks>,
    /// The level these were spawned for (Game::loads), and what of it got drawn.
    level: u64,
    drawn: Vec<bool>,
    ladders: bool,
    lines: bool,
}

pub fn setup(world: &mut World) {
    // (Textures go to the GPU block-compressed where it takes them.)
    if let Some(d) = world.get_resource::<bevy::render::renderer::RenderDevice>() {
        let bc = d.features().contains(bevy::render::render_resource::WgpuFeatures::TEXTURE_COMPRESSION_BC);
        crate::me_remaster::BC.store(bc, std::sync::atomic::Ordering::Relaxed);
    }
    world.insert_resource(MePropCache { level: u64::MAX, ..default() });
    let Some(cooked) = world.get_resource::<crate::me_viewmodel::MeArms>().map(|a| a.cooked_pc().to_path_buf()) else { return };
    // (Props collide with their meshes' own shapes, read from the same install.)
    aperture::me::set_install(&cooked);
    world.insert_non_send(MePackages(Packages::new(&cooked)));
}

/// Unreal's axes (X forward, Y right, Z up, centimetres) to ours (X right, Y up, Z back,
/// metres).
fn ue(p: [f32; 3]) -> [f32; 3] {
    [p[1] * 0.01, p[2] * 0.01, -p[0] * 0.01]
}

fn ue_dir(p: [f32; 3]) -> [f32; 3] {
    [p[1], p[2], -p[0]]
}

/// How a material's drawn, as the game has it: cut out (its clip value), see-through, or solid;
/// and from both sides.
fn drawn_as(slot: &MaterialSlot) -> (AlphaMode, bool) {
    use me_assets::Blend;
    match slot.blend {
        Blend::Opaque => (AlphaMode::Opaque, slot.two_sided),
        // (Cut-outs are fences and grates: seen from both sides.)
        Blend::Masked(clip) => (AlphaMode::Mask(clip.clamp(0.01, 0.99)), true),
        Blend::Translucent => (AlphaMode::Blend, true),
        Blend::Additive => (AlphaMode::Add, true),
    }
}

/// How many metres of the mesh one texture's width covers, over a section's triangles (its
/// area over its UVs' area): how big the texture is in the world.
fn metres_per_uv(mesh: &StaticMesh, first: usize, tris: usize, scale: Vec2) -> f32 {
    let (mut world, mut uv) = (0.0f64, 0.0f64);
    for t in 0..tris {
        let i = [mesh.indices[first + 3 * t] as usize, mesh.indices[first + 3 * t + 1] as usize, mesh.indices[first + 3 * t + 2] as usize];
        let p = i.map(|k| Vec3::from(ue(mesh.positions[k])));
        let q = i.map(|k| Vec2::from(mesh.uvs[k]) * scale);
        world += (p[1] - p[0]).cross(p[2] - p[0]).length() as f64 * 0.5;
        uv += ((q[1] - q[0]).perp_dot(q[2] - q[0])).abs() as f64 * 0.5;
    }
    if uv > 1e-9 { (world / uv).sqrt() as f32 } else { 1.0 }
}

/// A material that's an effect the game blends over its picture, not a surface: what's added
/// over it (light shafts, glows, fake bloom), see-through fog, haze, sky domes and the like,
/// and the fake shadows under cars. Traced, they'd be sheets of white or grey hanging in the air: they're left out.
fn effect(slot: &MaterialSlot) -> bool {
    const WORDS: [&str; 11] = ["fog", "bloom", "glow", "skydome", "lightshaft", "light_shaft", "trail", "haze", "blocker", "levelfx", "beam"];
    let name = slot.path.to_ascii_lowercase();
    // (A fake shadow under a car: the traced light casts its own.)
    let fake_shadow = ["dropshadow", "blobshadow", "fakeshadow"].iter().any(|w| name.contains(w));
    fake_shadow || slot.blend == me_assets::Blend::Additive || slot.blend == me_assets::Blend::Translucent && WORDS.iter().any(|w| name.contains(w))
}

/// The most a lamp's glow is (its colour's brightest channel): a few of the game's are far
/// brighter than its bloom ever let them look.
const GLOW_MOST: f32 = 12.0;

/// What a material glows, if it's a lamp's or a sign's (its emissive input, as the game has
/// it). Others' emissive inputs aren't glows: the far city's and its water's are how they're
/// lit (unlit, they'd glow like the sun), a wet floor's a sheen.
fn glow_of(slot: &MaterialSlot) -> Option<(Option<&me_assets::Rgba>, [f32; 3])> {
    const LAMPS: [&str; 10] = ["light", "lamp", "bulb", "neon", "sign", "screen", "monitor", "led_", "_on", "glow"];
    const NOT: [&str; 6] = ["b_vista", "vista", "water", "floor", "glass", "lightshaft"];
    let name = slot.path.to_ascii_lowercase();
    if !LAMPS.iter().any(|w| name.contains(w)) || NOT.iter().any(|w| name.contains(w)) {
        return None;
    }
    let (map, c) = slot.glow.as_ref()?;
    let most = c.iter().fold(0.0f32, |m, v| m.max(*v));
    let k = if most > GLOW_MOST { GLOW_MOST / most } else { 1.0 };
    Some((map.as_ref(), [c[0] * k, c[1] * k, c[2] * k]))
}

/// Makes a material glow as its lamp does (see [`glow_of`]).
fn light_up(slot: &MaterialSlot, mat: &Handle<StandardMaterial>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) {
    let Some((map, [r, g, b])) = glow_of(slot) else { return };
    let tex = map.map(|t| images.add(crate::me_remaster::plain(t, crate::me_remaster::Map::Color)));
    if let Some(mut m) = materials.get_mut(mat) {
        m.emissive = LinearRgba::rgb(r, g, b);
        m.emissive_texture = tex;
    }
}

fn build(
    mesh: &StaticMesh,
    slots: &[MaterialSlot],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    remastered: &mut HashMap<(String, u32), Handle<StandardMaterial>>,
    occluders: &mut std::collections::HashSet<AssetId<StandardMaterial>>,
) -> Parts {
    let positions: Vec<[f32; 3]> = mesh.positions.iter().map(|&p| ue(p)).collect();
    let normals: Vec<[f32; 3]> = mesh.normals.iter().map(|&n| ue_dir(n)).collect();
    // Our axes are Unreal's mirrored, which turns cross(normal, tangent) round: so the
    // bitangent's sign is the other way round too.
    let tangents: Vec<[f32; 4]> = mesh.tangents.iter().map(|&t| {
        let d = ue_dir([t[0], t[1], t[2]]);
        [d[0], d[1], d[2], -t[3]]
    }).collect();
    mesh.sections
        .iter()
        .zip(slots)
        .filter(|(s, slot)| s.triangles > 0 && !effect(slot))
        .map(|(s, slot)| {
            let first = s.first_index as usize;
            // Only the points its triangles use (a level's BSP is one mesh of many materials:
            // each with all of its points would be many times what's drawn).
            let mut remap: HashMap<u32, u32> = HashMap::new();
            let mut used: Vec<usize> = Vec::new();
            let indices: Vec<u32> = mesh.indices[first..first + 3 * s.triangles as usize]
                .iter()
                .map(|&i| {
                    *remap.entry(i).or_insert_with(|| {
                        used.push(i as usize);
                        used.len() as u32 - 1
                    })
                })
                .collect();
            let pick = |v: &[[f32; 3]]| -> Vec<[f32; 3]> { used.iter().map(|&i| v[i]).collect() };
            // (Its UVs as its material tiles them.)
            let scale = Vec2::from(slot.uv_scale);
            let uvs: Vec<[f32; 2]> = used.iter().map(|&i| (Vec2::from(mesh.uvs[i]) * scale).to_array()).collect();
            let m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pick(&positions))
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, pick(&normals))
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
                .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, used.iter().map(|&i| tangents[i]).collect::<Vec<_>>())
                .with_inserted_indices(Indices::U32(indices));
            // Cut-outs (fences, grilles) where the diffuse map's alpha says so.
            // How it's drawn: as the game's material has it (cut out, see-through, from both
            // sides).
            let (alpha_mode, double_sided) = drawn_as(slot);
            let cull_mode = if double_sided { None } else { Some(bevy::render::render_resource::Face::Back) };
            let glass = slot.blend == me_assets::Blend::Translucent;
            // Remastered (#101): what it's made of, its detail tiled at its real size (one
            // material for every mesh with it at that size).
            // (Once per material: its detail's tiling from the first mesh that has it.)
            if let Some(surface) = slot.diffuse.as_ref().and(crate::me_remaster::surface_of(&slot.path)) {
                let reps = crate::me_remaster::repeats(&surface, metres_per_uv(mesh, first, s.triangles as usize, scale));
                let key = (slot.path.clone(), 1);
                let mat = match remastered.get(&key) {
                    Some(m) => Some(m.clone()),
                    None => crate::me_remaster::bake_kept(slot, &surface, reps).map(|b| {
                        let color = images.add(b.color);
                        // (A backlit sign glows its colour.)
                        let glows = surface.glow > 0.0;
                        let m = materials.add(StandardMaterial {
                            emissive: if glows { LinearRgba::WHITE * surface.glow } else { LinearRgba::BLACK },
                            emissive_texture: glows.then(|| color.clone()),
                            base_color_texture: Some(color),
                            normal_map_texture: Some(images.add(b.normal)),
                            metallic_roughness_texture: Some(images.add(b.metal_rough)),
                            // (The map's: these scale it.)
                            perceptual_roughness: 1.0,
                            metallic: 1.0,
                            // (A mask from the colour map: opaque, its alpha isn't opacity.)
                            alpha_mode: if surface.mask { AlphaMode::Opaque } else { alpha_mode },
                            double_sided,
                            cull_mode,
                            ..default()
                        });
                        light_up(slot, &m, materials, images);
                        remastered.insert(key, m.clone());
                        m
                    }),
                };
                if let Some(mat) = mat {
                    return (meshes.add(m), mat);
                }
            }
            // Else the game's own maps, with mips, its white kept under real paint's (one
            // material for every mesh with it).
            let key = (slot.path.clone(), 0);
            if let Some(mat) = remastered.get(&key).filter(|_| !slot.path.is_empty()) {
                return (meshes.add(m), mat.clone());
            }
            let tex = slot.diffuse.as_ref().map(|t| images.add(crate::me_remaster::plain(&crate::me_remaster::tamed(t, slot.tint), crate::me_remaster::Map::Color)));
            let normal_map = slot.normal.as_ref().map(|t| images.add(crate::me_remaster::plain(t, crate::me_remaster::Map::Normal)));
            let rough = slot.specular.as_ref().map(|t| {
                let r = roughness_image(t);
                let px = r.data.clone().unwrap_or_default();
                images.add(crate::me_remaster::compressed(t.width, t.height, px, crate::me_remaster::Map::Data))
            });
            // Without a texture: its constant colour (glowing, if it's a light's), else grey.
            let plain = slot.color.map(|[r, g, b]| Color::linear_rgb(r, g, b));
            let mat = materials.add(StandardMaterial {
                base_color: if tex.is_some() { Color::WHITE } else { plain.unwrap_or(Color::srgb(0.55, 0.55, 0.56)) },
                emissive: match (plain, slot.emissive) {
                    (Some(c), true) => c.to_linear() * 4.0,
                    _ => LinearRgba::BLACK,
                },
                base_color_texture: tex,
                perceptual_roughness: if rough.is_some() { 1.0 } else { 0.8 },
                metallic_roughness_texture: rough,
                normal_map_texture: normal_map,
                alpha_mode,
                double_sided,
                cull_mode,
                reflectance: if glass { 0.5 } else { 0.3 },
                ..default()
            });
            // Glass: clear and smooth, its colour a tint (what it has of an opacity is its
            // alpha; else mostly clear). A building's (its windows) still shades what's behind it
            // in the traced light, as the rooms behind them would.
            if glass && slot.path.starts_with("B_") {
                occluders.insert(mat.id());
            }
            if glass {
                if let Some(mut g) = materials.get_mut(&mat) {
                    g.perceptual_roughness = 0.05;
                    g.metallic_roughness_texture = None;
                    let opaque = slot.diffuse.as_ref().is_none_or(|t| t.pixels.chunks_exact(4).all(|c| c[3] > 250));
                    g.base_color = g.base_color.with_alpha(if opaque { 0.25 } else { 1.0 });
                }
            }
            light_up(slot, &mat, materials, images);
            if !slot.path.is_empty() {
                remastered.insert(key, mat.clone());
            }
            (meshes.add(m), mat)
        })
        .collect()
}

fn get(
    key: (&'static str, &'static str),
    cache: &mut MePropCache,
    pkgs: &mut MePackages,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> Option<Parts> {
    if !cache.meshes.contains_key(&key) {
        let made = match me_assets::load_static_mesh(&mut pkgs.0, key.0, key.1, me_texture()) {
            Ok((mesh, slots)) => {
                let (mut lo, mut hi) = (Vec3::MAX, Vec3::MIN);
                for p in &mesh.positions {
                    let p = Vec3::from(ue(*p));
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
                if lo.x <= hi.x {
                    cache.bounds.insert(key, (lo, hi));
                }
                let mut looks = MeLooks { total: slots.len(), bounds: (lo.x <= hi.x).then_some((lo, hi)), ..default() };
                for s in &slots {
                    if s.diffuse.is_some() {
                        looks.textured += 1;
                    } else if s.color.is_some() {
                        looks.plain += 1;
                    } else if s.material == "None" {
                        looks.unset += 1;
                    } else {
                        looks.missing.push(s.material.clone());
                    }
                }
                cache.looks.insert(format!("{}.{}", key.0, key.1), looks);
                Some(build(&mesh, &slots, meshes, materials, images, &mut cache.remastered, &mut cache.occluders))
            }
            Err(e) => {
                warn!("Mirror's Edge prop {}.{}: {e}", key.0, key.1);
                None
            }
        };
        cache.meshes.insert(key, made);
    }
    cache.meshes.get(&key).cloned().flatten()
}

impl MePropCache {
    /// What a mesh's materials found, if it's been read.
    pub fn looks(&self, package: &str, mesh: &str) -> Option<&MeLooks> {
        self.looks.get(&format!("{package}.{mesh}"))
    }
}

/// A mesh's parts with some of its materials replaced (`overrides`, a section each; empty:
/// its own), read once.
fn get_with(
    key: (&'static str, &'static str),
    overrides: &[&'static str],
    cache: &mut MePropCache,
    pkgs: &mut MePackages,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> Option<Parts> {
    if overrides.iter().all(|o| o.is_empty()) {
        return get(key, cache, pkgs, meshes, materials, images);
    }
    let k = (key.0, key.1, overrides.to_vec());
    if !cache.overridden.contains_key(&k) {
        let names: Vec<String> = overrides.iter().map(|o| o.to_string()).collect();
        let made = match me_assets::load_static_mesh_with(&mut pkgs.0, key.0, key.1, &names, me_texture()) {
            Ok((mesh, slots)) => Some(build(&mesh, &slots, meshes, materials, images, &mut cache.remastered, &mut cache.occluders)),
            Err(e) => {
                warn!("Mirror's Edge prop {}.{} with its own materials: {e}", key.0, key.1);
                None
            }
        };
        cache.overridden.insert(k.clone(), made);
    }
    cache.overridden.get(&k).cloned().flatten()
}

/// A mesh's parts and its box (ours, from its origin), read once (the level editor's
/// thumbnails).
pub fn parts_and_bounds(
    key: (&'static str, &'static str),
    cache: &mut MePropCache,
    pkgs: &mut MePackages,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> Option<(Parts, (Vec3, Vec3))> {
    let parts = get(key, cache, pkgs, meshes, materials, images)?;
    Some((parts, *cache.bounds.get(&key)?))
}

/// A mesh's parts wound the other way round (its tangents' handedness too).
fn mirrored(key: (&'static str, &'static str), parts: &Parts, cache: &mut MePropCache, meshes: &mut Assets<Mesh>) -> Parts {
    if let Some(p) = cache.mirrored.get(&key) {
        return p.clone();
    }
    let flipped: Parts = parts
        .iter()
        .map(|(m, mat)| {
            let Some(mut mesh) = meshes.get(m).cloned() else { return (m.clone(), mat.clone()) };
            if let Some(Indices::U32(ix)) = mesh.indices_mut() {
                for t in ix.chunks_exact_mut(3) {
                    t.swap(1, 2);
                }
            }
            if let Some(bevy::mesh::VertexAttributeValues::Float32x4(ts)) = mesh.attribute_mut(Mesh::ATTRIBUTE_TANGENT) {
                for t in ts.iter_mut() {
                    t[3] = -t[3];
                }
            }
            (meshes.add(mesh), mat.clone())
        })
        .collect();
    cache.mirrored.insert(key, flipped.clone());
    flipped
}

fn place(commands: &mut Commands, parts: &Parts, tf: Transform, occluders: &std::collections::HashSet<AssetId<StandardMaterial>>) -> Entity {
    commands
        .spawn((ChamberGeom, crate::chamber::PlacedAt(tf.translation), tf, Visibility::default()))
        .with_children(|c| {
            for (m, mat) in parts {
                let mut e = c.spawn((Mesh3d(m.clone()), MeshMaterial3d(mat.clone()), Transform::default()));
                if occluders.contains(&mat.id()) {
                    e.insert(crate::raytrace::CastsTracedShadow);
                }
            }
        })
        .id()
}

/// A prop a Mirror's Edge mover carries (#109): which body (sim mover) it's part of, and
/// where it stands when that hasn't moved.
#[derive(Component)]
pub struct Carried {
    pub body: usize,
    pub base: Transform,
}

/// Carried props go where their movers are.
pub fn follow_movers(game: Res<Game>, mut carried: Query<(&Carried, &mut Transform)>) {
    let Some(me) = game.sim.me.as_ref() else { return };
    for (c, mut tf) in &mut carried {
        let Some((pivot, d, q)) = me.body(&game.sim.level, c.body) else { continue };
        let to = Transform { translation: pivot + d + q * (c.base.translation - pivot), rotation: q * c.base.rotation, scale: c.base.scale };
        if *tf != to {
            *tf = to;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_for_level(
    mut commands: Commands,
    game: Res<Game>,
    pkgs: Option<NonSendMut<MePackages>>,
    mut cache: ResMut<MePropCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut stand_ins: Query<(&StandIn, &mut Visibility)>,
    settings: Res<crate::settings::Settings>,
) {
    let Some(mut pkgs) = pkgs else { return };
    let cache = &mut *cache;
    if cache.level != game.loads {
        cache.level = game.loads;
        // (A different texture detail: what's been read is read again.)
        // (PORTAL_TEXTURE_DETAIL=0/1/2 in place of the setting: for captures.)
        let level_of = std::env::var("PORTAL_TEXTURE_DETAIL").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(settings.texture_detail);
        let detail = [256, 512, 1024][(level_of.round().max(0.0) as usize).min(2)];
        if ME_TEXTURE.swap(detail, std::sync::atomic::Ordering::Relaxed) != detail {
            cache.meshes.clear();
            cache.overridden.clear();
            cache.mirrored.clear();
            cache.remastered.clear();
        }
        let level = &game.sim.level;
        cache.drawn = vec![false; level.me_props.len()];
        let started = std::time::Instant::now();
        // (Which props movers carry.)
        let mut carried_by: HashMap<usize, usize> = HashMap::new();
        for mv in &level.me_movers {
            for &k in &mv.carries {
                carried_by.insert(k, mv.sim);
            }
        }
        for (k, p) in level.me_props.iter().enumerate() {
            if let Some(parts) = get_with((p.package, p.mesh), &p.materials, cache, &mut pkgs, &mut meshes, &mut materials, &mut images) {
                let parts = if p.scale.x * p.scale.y * p.scale.z < 0.0 { mirrored((p.package, p.mesh), &parts, cache, &mut meshes) } else { parts };
                let tf = Transform::from_translation(p.at).with_rotation(p.turn).with_scale(p.scale);
                let e = place(&mut commands, &parts, tf, &cache.occluders);
                if let Some(&body) = carried_by.get(&k) {
                    commands.entity(e).insert(Carried { body, base: tf });
                }
                cache.drawn[k] = true;
            }
        }
        if !level.me_props.is_empty() {
            info!("Mirror's Edge props: {} drawn in {:.1} s", level.me_props.len(), started.elapsed().as_secs_f32());
            if std::env::var_os("PORTAL_ME_MEMORY").is_some() {
                let tex: usize = images.iter().map(|(_, i)| i.data.as_ref().map_or(0, |d| d.len())).sum();
                let (verts, idx): (usize, usize) = meshes.iter().fold((0, 0), |(v, x), (_, m)| (v + m.count_vertices(), x + m.indices().map_or(0, |i| i.len())));
                info!("Mirror's Edge memory: {} images {} MB, {} meshes {} k vertices {} k indices", images.len(), tex >> 20, meshes.len(), verts / 1000, idx / 1000);
            }
        }
        // Ziplines and swing poles: round steel, the ladder's.
        cache.lines = level.me_art;
        let lines: Vec<(Vec3, Vec3, f32)> = level
            .fixtures
            .iter()
            .flat_map(|f| match *f {
                Fixture::ZipLine { .. } => pieces(f).into_iter().map(|(a, b)| (a, b, CABLE_RADIUS)).collect(),
                Fixture::SwingPole { a, b } if !level.me_art => vec![(a, b, PIPE_RADIUS)],
                _ => vec![],
            })
            .collect();
        if !lines.is_empty() {
            if let Some(steel) = get(LADDER, cache, &mut pkgs, &mut meshes, &mut materials, &mut images).and_then(|p| p.first().map(|(_, m)| m.clone())) {
                cache.lines = true;
                for (a, b, r) in lines {
                    let len = a.distance(b);
                    let rot = Quat::from_rotation_arc(Vec3::Y, (b - a) / len);
                    commands.spawn((ChamberGeom, Mesh3d(meshes.add(Cylinder::new(r, len).mesh().resolution(12))), MeshMaterial3d(steel.clone()), Transform::from_translation((a + b) * 0.5).with_rotation(rot)));
                }
            }
        }
        // The steel ladder, a piece at a time, up each ladder (drainpipes keep their pipe).
        // (A Mirror's Edge level's ladders are its own.)
        cache.ladders = level.me_art;
        let ladders: Vec<_> = level.fixtures.iter().filter_map(|f| if let Fixture::Ladder(l) = f { Some(*l) } else { None }).filter(|l| !l.pipe && !level.me_art).collect();
        if !ladders.is_empty() {
            if let Some(parts) = get(LADDER, cache, &mut pkgs, &mut meshes, &mut materials, &mut images) {
                cache.ladders = true;
                for l in ladders {
                    // Its rails are 4 cm behind its origin, along its +X: that's the way out.
                    let yaw = (-l.normal.z).atan2(l.normal.x);
                    let mut y = l.base.y;
                    while y < l.top + 0.9 {
                        let at = Vec3::new(l.base.x, y, l.base.z) + l.normal * 0.12;
                        place(&mut commands, &parts, Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)), &cache.occluders);
                        y += LADDER_STEP;
                    }
                }
            }
        }
    }
    for (s, mut v) in &mut stand_ins {
        let hide = match s.0 {
            usize::MAX => cache.ladders,
            LINES => cache.lines,
            k => cache.drawn.get(k).copied().unwrap_or(false),
        };
        if hide && *v != Visibility::Hidden {
            *v = Visibility::Hidden;
        }
    }
}
