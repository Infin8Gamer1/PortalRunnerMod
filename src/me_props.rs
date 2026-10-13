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

/// How many metres of the mesh one texture's width covers, over a section's triangles (its
/// area over its UVs' area): how big the texture is in the world.
fn metres_per_uv(mesh: &StaticMesh, first: usize, tris: usize) -> f32 {
    let (mut world, mut uv) = (0.0f64, 0.0f64);
    for t in 0..tris {
        let i = [mesh.indices[first + 3 * t] as usize, mesh.indices[first + 3 * t + 1] as usize, mesh.indices[first + 3 * t + 2] as usize];
        let p = i.map(|k| Vec3::from(ue(mesh.positions[k])));
        let q = i.map(|k| Vec2::from(mesh.uvs[k]));
        world += (p[1] - p[0]).cross(p[2] - p[0]).length() as f64 * 0.5;
        uv += ((q[1] - q[0]).perp_dot(q[2] - q[0])).abs() as f64 * 0.5;
    }
    if uv > 1e-9 { (world / uv).sqrt() as f32 } else { 1.0 }
}

fn build(mesh: &StaticMesh, slots: &[MaterialSlot], meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>, remastered: &mut HashMap<(String, u32), Handle<StandardMaterial>>) -> Parts {
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
        .filter(|(s, _)| s.triangles > 0)
        .map(|(s, slot)| {
            let first = s.first_index as usize;
            let indices = mesh.indices[first..first + 3 * s.triangles as usize].to_vec();
            let m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
                .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone())
                .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals.clone())
                .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, mesh.uvs.clone())
                .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents.clone())
                .with_inserted_indices(Indices::U32(indices));
            // Cut-outs (fences, grilles) where the diffuse map's alpha says so.
            let cutout = slot.diffuse.as_ref().is_some_and(|t| t.pixels.chunks_exact(4).filter(|c| c[3] < 128).count() * 20 > t.pixels.len() / 4);
            // Remastered (#101): what it's made of, its detail tiled at its real size (one
            // material for every mesh with it at that size).
            // (Once per material: its detail's tiling from the first mesh that has it.)
            if let Some(surface) = slot.diffuse.as_ref().and(crate::me_remaster::surface_of(&slot.path)) {
                let reps = crate::me_remaster::repeats(&surface, metres_per_uv(mesh, first, s.triangles as usize));
                let key = (slot.path.clone(), 1);
                let mat = match remastered.get(&key) {
                    Some(m) => Some(m.clone()),
                    None => crate::me_remaster::bake(slot, &surface, reps).map(|b| {
                        let m = materials.add(StandardMaterial {
                            base_color_texture: Some(images.add(b.color)),
                            normal_map_texture: Some(images.add(b.normal)),
                            metallic_roughness_texture: Some(images.add(b.metal_rough)),
                            // (The map's: these scale it.)
                            perceptual_roughness: 1.0,
                            metallic: 1.0,
                            alpha_mode: if cutout { AlphaMode::Mask(0.5) } else { AlphaMode::Opaque },
                            double_sided: cutout,
                            cull_mode: if cutout { None } else { Some(bevy::render::render_resource::Face::Back) },
                            ..default()
                        });
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
            let tex = slot.diffuse.as_ref().map(|t| images.add(crate::me_remaster::plain(&crate::me_remaster::tamed(t), true)));
            let normal_map = slot.normal.as_ref().map(|t| images.add(crate::me_remaster::plain(t, false)));
            let rough = slot.specular.as_ref().map(|t| {
                let r = roughness_image(t);
                let px = r.data.clone().unwrap_or_default();
                images.add(crate::me_remaster::with_mips(t.width, t.height, px, false))
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
                alpha_mode: if cutout { AlphaMode::Mask(0.5) } else { AlphaMode::Opaque },
                double_sided: cutout,
                cull_mode: if cutout { None } else { Some(bevy::render::render_resource::Face::Back) },
                reflectance: 0.3,
                ..default()
            });
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
        let made = match me_assets::load_static_mesh(&mut pkgs.0, key.0, key.1, 512) {
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
                Some(build(&mesh, &slots, meshes, materials, images, &mut cache.remastered))
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

fn place(commands: &mut Commands, parts: &Parts, tf: Transform) {
    commands.spawn((ChamberGeom, crate::chamber::PlacedAt(tf.translation), tf, Visibility::default())).with_children(|c| {
        for (m, mat) in parts {
            c.spawn((Mesh3d(m.clone()), MeshMaterial3d(mat.clone()), Transform::default()));
        }
    });
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
) {
    let Some(mut pkgs) = pkgs else { return };
    let cache = &mut *cache;
    if cache.level != game.loads {
        cache.level = game.loads;
        let level = &game.sim.level;
        cache.drawn = vec![false; level.me_props.len()];
        for (k, p) in level.me_props.iter().enumerate() {
            if let Some(parts) = get((p.package, p.mesh), cache, &mut pkgs, &mut meshes, &mut materials, &mut images) {
                let parts = if p.scale.x * p.scale.y * p.scale.z < 0.0 { mirrored((p.package, p.mesh), &parts, cache, &mut meshes) } else { parts };
                place(&mut commands, &parts, Transform::from_translation(p.at).with_rotation(p.turn).with_scale(p.scale));
                cache.drawn[k] = true;
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
                        place(&mut commands, &parts, Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)));
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
