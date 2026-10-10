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

/// Marks what stands in for a Mirror's Edge prop (its index in the level's `me_props`), or
/// for a ladder (`usize::MAX`), hidden once the real thing is drawn.
#[derive(Component)]
pub struct StandIn(pub usize);

/// The game's packages (not thread-safe: they share what they've opened).
pub struct MePackages(Packages);

type Parts = Vec<(Handle<Mesh>, Handle<StandardMaterial>)>;

#[derive(Resource, Default)]
pub struct MePropCache {
    meshes: HashMap<(&'static str, &'static str), Option<Parts>>,
    /// The level these were spawned for (Game::loads), and what of it got drawn.
    level: u64,
    drawn: Vec<bool>,
    ladders: bool,
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
            let mat = materials.add(StandardMaterial {
                base_color: if tex.is_some() { Color::WHITE } else { Color::srgb(0.55, 0.55, 0.56) },
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
    cache
        .meshes
        .entry(key)
        .or_insert_with(|| match me_assets::load_static_mesh(&mut pkgs.0, key.0, key.1, 512) {
            Ok((mesh, slots)) => Some(build(&mesh, &slots, meshes, materials, images)),
            Err(e) => {
                warn!("Mirror's Edge prop {}.{}: {e}", key.0, key.1);
                None
            }
        })
        .clone()
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
                place(&mut commands, &parts, Transform::from_translation(p.at).with_rotation(Quat::from_rotation_y(p.yaw)).with_scale(Vec3::splat(p.scale)));
                cache.drawn[k] = true;
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
        let hide = if s.0 == usize::MAX { cache.ladders } else { cache.drawn.get(s.0).copied().unwrap_or(false) };
        if hide && *v != Visibility::Hidden {
            *v = Visibility::Hidden;
        }
    }
}
