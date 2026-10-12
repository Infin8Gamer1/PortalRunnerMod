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
use crate::me_viewmodel::{image_from, normal_image, roughness_image};
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

fn build(mesh: &StaticMesh, slots: &[MaterialSlot], meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) -> Parts {
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
            let tex = slot.diffuse.as_ref().map(|t| images.add(image_from(t)));
            let normal_map = slot.normal.as_ref().map(|t| images.add(normal_image(t)));
            let rough = slot.specular.as_ref().map(|t| images.add(roughness_image(t)));
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
                Some(build(&mesh, &slots, meshes, materials, images))
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

fn place(commands: &mut Commands, parts: &Parts, tf: Transform) {
    commands.spawn((ChamberGeom, tf, Visibility::default())).with_children(|c| {
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
                place(&mut commands, &parts, Transform::from_translation(p.at).with_rotation(p.turn).with_scale(p.scale));
                cache.drawn[k] = true;
            }
        }
        // Ziplines and swing poles: round steel, the ladder's.
        cache.lines = false;
        let lines: Vec<(Vec3, Vec3, f32)> = level
            .fixtures
            .iter()
            .filter_map(|f| match *f {
                Fixture::ZipLine { a, b } => Some((a, b, CABLE_RADIUS)),
                Fixture::SwingPole { a, b } => Some((a, b, PIPE_RADIUS)),
                _ => None,
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
        cache.ladders = false;
        let ladders: Vec<_> = level.fixtures.iter().filter_map(|f| if let Fixture::Ladder(l) = f { Some(*l) } else { None }).filter(|l| !l.pipe).collect();
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
