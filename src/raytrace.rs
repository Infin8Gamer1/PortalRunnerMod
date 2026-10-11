//! Ray-traced lighting, with Bevy's Solari: direct light from the sun and from everything that
//! glows (the ceiling panels, lit buttons, lenses...), and light bouncing between surfaces,
//! traced against the chamber on the GPU every frame. Nothing is baked: a cube carried under a
//! lamp shades the floor, and a red wall tints the light it bounces.
//!
//! It needs a GPU with hardware ray tracing, and it's chosen at startup (the `ray_tracing`
//! setting): Solari draws everything deferred, which can't be switched on the fly. Without it
//! the game lights itself as before (lamps, sun shadows, an ambient fill).
//!
//! What it takes:
//! - Every lit, opaque mesh in the world is put in the ray-traced scene ([`RaytracingMesh3d`])
//!   while it's visible. Solari wants positions, normals, UVs and tangents, and 32-bit indices:
//!   meshes without are given them (or copied, if they carry anything more).
//! - Solari lights by emission in physical units (exposed like the sun), where the ordinary
//!   path shows emissive colours as they are: emissive materials are scaled to match.
//! - Outdoors, a sky dome lights from all round (Solari has no sky of its own), with a hole
//!   where the sun is so it doesn't shade it.
//! - The player's camera and the first portal views trace; deeper portal views are lit the
//!   ordinary (deferred) way, and the first-person arms and gun draw forward over the top.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::camera::visibility::{RenderLayers, VisibilitySystems};
use bevy::camera::CameraMainTextureUsages;
use bevy::core_pipeline::prepass::{DeferredPrepass, DepthPrepass};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::material::OpaqueRendererMethod;
use bevy::pbr::DefaultOpaqueRendererMethod;
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;
use bevy::render::renderer::RenderDevice;
use bevy::render::view::Msaa;
use bevy::solari::prelude::{RaytracingMesh3d, SolariLighting, SolariPlugins};

use crate::body3p::{BODY_LAYER, THROUGH_LAYER};
use crate::portal_render::PortalCam;
use crate::{Game, PlayerCamera, Sun};

/// Portal views this deep (and shallower) trace their light; deeper ones, a small part of the
/// screen, are lit the ordinary way.
pub const TRACED_DEPTH: usize = 1;
/// Emission, from what the ordinary path shows on screen to physical units: the inverse of
/// the camera's exposure at the default brightness (`Exposure::EV100_BLENDER`).
pub const EMISSIVE_SCALE: f32 = 1000.0;
/// The ceiling panels, on top of that: they light the chambers.
pub const LAMP_BOOST: f32 = 30.0;
/// The sky, as it shows on screen (scaled to nits like every emissive colour): the clear
/// colour outdoors.
const SKY: Vec3 = Vec3::new(0.34, 0.57, 0.85);
const SKY_RADIUS: f32 = 900.0;
/// The hole round the sun (radians from its direction, to a triangle's centre: wide enough to
/// take the triangle the sun is in).
const SUN_HOLE: f32 = 0.1;

static WANTED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether the lighting is ray traced (asked for at startup, and the GPU can).
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// The setting as shown: a change waits for a restart.
pub fn label(v: f32) -> String {
    let on = v >= 0.5;
    let name = if on { "On" } else { "Off" };
    if on != WANTED.load(Ordering::Relaxed) {
        format!("{name} (after restart)")
    } else if on && !active() {
        format!("{name} (no GPU support)")
    } else {
        name.to_string()
    }
}

/// Add ray-traced lighting to the app, if it's wanted (before the renderer starts).
pub fn add(app: &mut App, wanted: bool) {
    WANTED.store(wanted, Ordering::Relaxed);
    if !wanted {
        return;
    }
    app.add_plugins(SolariPlugins)
        .init_resource::<RtMeshes>()
        .add_systems(Startup, check_support)
        .add_systems(Update, (cameras, forward_overlays, emissive_materials, sky_dome).run_if(active))
        .add_systems(PostUpdate, register_meshes.after(VisibilitySystems::VisibilityPropagate).run_if(active));
}

/// The plugins only start if the GPU has what they need; if not, back to forward rendering.
fn check_support(mut commands: Commands, device: Option<Res<RenderDevice>>) {
    let ok = device.is_some_and(|d| d.features().contains(SolariPlugins::required_wgpu_features()));
    ACTIVE.store(ok, Ordering::Relaxed);
    if ok {
        info!("Ray-traced lighting on");
    } else {
        warn!("Ray-traced lighting asked for, but this GPU can't: lighting the ordinary way");
        commands.insert_resource(DefaultOpaqueRendererMethod::forward());
    }
}

/// Solari on the player's camera and the first portal views; a deferred prepass on the
/// deeper ones (deferred materials draw nowhere else). Through a portal there's no reset of
/// the traced light's history: most of it is in world space (still good), and what's kept per
/// pixel is checked against the new view anyway. (The overlay cameras' texture usages are
/// matched to the player's in `graphics`.)
#[allow(clippy::type_complexity)]
fn cameras(
    mut commands: Commands,
    player: Query<Entity, (With<PlayerCamera>, Without<SolariLighting>)>,
    portals: Query<(Entity, &PortalCam), (Without<SolariLighting>, Without<DeferredPrepass>)>,
) {
    let usages = CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING);
    let traced = (SolariLighting::default(), usages, Msaa::Off);
    for e in &player {
        commands.entity(e).insert(traced.clone());
    }
    for (e, pc) in &portals {
        if pc.level <= TRACED_DEPTH {
            // Their own TAA: the player's can't settle what's seen through a portal (its
            // motion is the portal's, not what's in it), and it's what smooths traced light.
            commands.entity(e).insert((traced.clone(), bevy::anti_alias::taa::TemporalAntiAliasing::default()));
        } else {
            commands.entity(e).insert((DepthPrepass, DeferredPrepass, Msaa::Off));
        }
    }
}

/// The arms and the gun are drawn by overlay cameras with no deferred pass: their materials
/// draw forward.
fn forward_overlays(
    q: Query<(&MeshMaterial3d<StandardMaterial>, &RenderLayers), Added<MeshMaterial3d<StandardMaterial>>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let overlay = RenderLayers::from_layers(&[crate::viewmodel::VIEWMODEL_LAYER, crate::gun::GUN_LAYER]);
    for (m, layers) in &q {
        if layers.intersects(&overlay) && !layers.intersects(&RenderLayers::layer(0)) {
            if materials.get(&m.0).is_some_and(|mat| mat.opaque_render_method != OpaqueRendererMethod::Forward) {
                if let Some(mut mat) = materials.get_mut(&m.0) {
                    mat.opaque_render_method = OpaqueRendererMethod::Forward;
                }
            }
        }
    }
}

/// Emission in physical units, exposed (so the ordinary path, in the deeper portal views,
/// shows it the same).
fn emissive_materials(
    mut materials: ResMut<Assets<StandardMaterial>>,
    mats: Option<Res<crate::chamber::ChamberMats>>,
    lamps: Option<Res<crate::source_map::Lamps>>,
    scorches: Option<Res<crate::chamber::ScorchMats>>,
    mut done: Local<HashSet<AssetId<StandardMaterial>>>,
) {
    let lamp = mats.map(|m| m.lamp.id());
    let is_lamp = |id: AssetId<StandardMaterial>| Some(id) == lamp || lamps.as_ref().is_some_and(|l| l.0.contains(&id));
    let todo: Vec<_> = materials
        .iter()
        .filter(|(id, m)| !done.contains(id) && m.emissive.to_vec3() != Vec3::ZERO)
        .map(|(id, _)| id)
        .collect();
    for id in todo {
        done.insert(id);
        // Bounce marks set their own glow every frame, already scaled.
        if scorches.as_ref().is_some_and(|s| s.0.contains(&id)) {
            continue;
        }
        if let Some(mut m) = materials.get_mut(id) {
            let scale = EMISSIVE_SCALE * if is_lamp(id) { LAMP_BOOST } else { 1.0 };
            m.emissive = LinearRgba::from_vec3(m.emissive.to_vec3() * scale);
            m.emissive_exposure_weight = 1.0;
        }
    }
}

/// Marks the sky dome.
#[derive(Component)]
struct SkyDome;

/// Outdoors, a sky to light by: a dome of emissive triangles, open round the sun.
fn sky_dome(
    mut commands: Commands,
    game: Res<Game>,
    sun: Query<&GlobalTransform, With<Sun>>,
    domes: Query<Entity, With<SkyDome>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut built: Local<Option<u64>>,
) {
    if *built == Some(game.loads) {
        return;
    }
    // The sun is placed with the level; wait for its transform to follow.
    let Ok(sun) = sun.single() else { return };
    *built = Some(game.loads);
    for e in &domes {
        commands.entity(e).despawn();
    }
    if game.sim.level.indoor {
        return;
    }
    let to_sun = sun.back().as_vec3();
    let material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::from_vec3(SKY),
        cull_mode: None,
        ..default()
    });
    commands.spawn((SkyDome, Mesh3d(meshes.add(dome_mesh(to_sun))), MeshMaterial3d(material), Transform::default(), NotShadowCaster));
}

/// An inward-facing sphere round the world, without the triangles near `to_sun`.
fn dome_mesh(to_sun: Vec3) -> Mesh {
    let sphere = Sphere::new(SKY_RADIUS).mesh().ico(3).unwrap();
    let pos: Vec<Vec3> = match sphere.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(p)) => p.iter().map(|&p| Vec3::from(p)).collect(),
        _ => vec![],
    };
    let idx: Vec<u32> = sphere.indices().map(|i| i.iter().map(|i| i as u32).collect()).unwrap_or_default();
    let mut out_pos = vec![];
    for t in idx.chunks_exact(3) {
        let (a, b, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
        if ((a + b + c) / 3.0).normalize().angle_between(to_sun) < SUN_HOLE {
            continue;
        }
        // Wound the other way: seen from inside.
        out_pos.extend([a, c, b]);
    }
    let n = out_pos.len();
    let normals: Vec<[f32; 3]> = out_pos.iter().map(|p| (-p.normalize()).to_array()).collect();
    Mesh::new(PrimitiveTopology::TriangleList, default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, out_pos.iter().map(|p| p.to_array()).collect::<Vec<_>>())
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0f32, 0.0, 0.0, 1.0]; n])
        .with_inserted_indices(Indices::U32((0..n as u32).collect()))
}

/// A forward-drawn mesh that's in the traced scene anyway, to cast its shadow and show in
/// reflections (Faith's body: skinned on the CPU every frame, it has no motion for traced
/// light to follow, which would leave it grainy, so it's lit the ordinary way).
#[derive(Component)]
pub struct CastsTracedShadow;

/// Each mesh's ray-tracing version: itself (once it has what Solari needs), a copy without
/// the attributes Solari can't take, or none.
#[derive(Resource, Default)]
struct RtMeshes(HashMap<AssetId<Mesh>, Option<Handle<Mesh>>>);

const RT_ATTRIBUTES: [bevy::mesh::MeshVertexAttributeId; 4] = [Mesh::ATTRIBUTE_POSITION.id, Mesh::ATTRIBUTE_NORMAL.id, Mesh::ATTRIBUTE_UV_0.id, Mesh::ATTRIBUTE_TANGENT.id];

/// Give a mesh what Solari needs: UVs, tangents, 32-bit indices. False if it can't be.
pub fn make_ready(mesh: &mut Mesh) -> bool {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList || !mesh.contains_attribute(Mesh::ATTRIBUTE_POSITION) || !mesh.contains_attribute(Mesh::ATTRIBUTE_NORMAL) {
        return false;
    }
    let n = mesh.count_vertices();
    if mesh.indices().is_none() {
        mesh.insert_indices(Indices::U32((0..n as u32).collect()));
    }
    if let Some(Indices::U16(i)) = mesh.indices() {
        let i: Vec<u32> = i.iter().map(|&i| i as u32).collect();
        mesh.insert_indices(Indices::U32(i));
    }
    if !mesh.contains_attribute(Mesh::ATTRIBUTE_UV_0) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; n]);
    }
    if !mesh.contains_attribute(Mesh::ATTRIBUTE_TANGENT) && mesh.generate_tangents().is_err() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0f32, 0.0, 0.0, 1.0]; n]);
    }
    true
}

fn is_ready(mesh: &Mesh) -> bool {
    mesh.primitive_topology() == PrimitiveTopology::TriangleList
        && matches!(mesh.indices(), Some(Indices::U32(_)))
        && mesh.attributes().count() == RT_ATTRIBUTES.len()
        && RT_ATTRIBUTES.iter().all(|&a| mesh.contains_attribute(a))
}

fn rt_version(id: AssetId<Mesh>, handle: &Handle<Mesh>, meshes: &mut Assets<Mesh>) -> Option<Handle<Mesh>> {
    let mesh = meshes.get(id)?;
    if is_ready(mesh) {
        return Some(handle.clone());
    }
    let extra = mesh.attributes().any(|(a, _)| !RT_ATTRIBUTES.contains(&a.id));
    if !extra {
        let mut mesh = meshes.get_mut(id)?;
        return make_ready(&mut mesh).then(|| handle.clone());
    }
    // Copy just what Solari takes (the original keeps the rest for drawing).
    let mut copy = Mesh::new(mesh.primitive_topology(), default());
    for (a, v) in mesh.attributes() {
        if RT_ATTRIBUTES.contains(&a.id) {
            copy.insert_attribute(*a, v.clone());
        }
    }
    if let Some(i) = mesh.indices() {
        copy.insert_indices(i.clone());
    }
    make_ready(&mut copy).then(|| meshes.add(copy))
}

/// Keep the ray-traced scene to what's drawn: lit, opaque meshes in the world, while visible.
#[allow(clippy::type_complexity)]
fn register_meshes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
    mut events: MessageReader<AssetEvent<Mesh>>,
    mut rt: ResMut<RtMeshes>,
    q: Query<(Entity, &Mesh3d, &MeshMaterial3d<StandardMaterial>, &InheritedVisibility, Option<&RenderLayers>, Option<&RaytracingMesh3d>, Has<CastsTracedShadow>)>,
) {
    for e in events.read() {
        if let AssetEvent::Modified { id } | AssetEvent::Removed { id } = e {
            rt.0.remove(id);
        }
    }
    let world = RenderLayers::from_layers(&[0, BODY_LAYER, THROUGH_LAYER]);
    for (e, mesh, mat, visible, layers, current, occluder) in &q {
        // (Forward-drawn materials keep the ordinary lights, and stay out, unless they're to
        // cast shadows all the same.)
        let lit = materials.get(&mat.0).is_some_and(|m| m.alpha_mode == AlphaMode::Opaque && !m.unlit && (occluder || m.opaque_render_method != OpaqueRendererMethod::Forward));
        let want = if visible.get() && lit && layers.is_none_or(|l| l.intersects(&world)) {
            let id = mesh.0.id();
            // Not loaded yet: next time.
            if !rt.0.contains_key(&id) && meshes.get(id).is_some() {
                let v = rt_version(id, &mesh.0, &mut meshes);
                rt.0.insert(id, v);
            }
            rt.0.get(&id).cloned().flatten()
        } else {
            None
        };
        match (want, current) {
            (Some(h), Some(c)) if c.0 == h => {}
            (Some(h), _) => {
                commands.entity(e).insert(RaytracingMesh3d(h));
            }
            (None, Some(_)) => {
                commands.entity(e).remove::<RaytracingMesh3d>();
            }
            (None, None) => {}
        }
    }
}
