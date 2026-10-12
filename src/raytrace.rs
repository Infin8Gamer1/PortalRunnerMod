//! Ray-traced lighting, with Bevy's Solari: direct light from the sun and from everything that
//! glows (the ceiling panels, lit buttons, lenses...), and light bouncing between surfaces,
//! traced against the chamber on the GPU every frame. Nothing is baked: a cube carried under a
//! lamp shades the floor, and a red wall tints the light it bounces.
//!
//! It needs a GPU with hardware ray tracing, and it's the `ray_tracing` setting, switched on
//! the fly: Solari is always there (idle without a camera asking for it), and switching moves
//! every material between deferred (what Solari needs) and forward drawing, scales what glows
//! to physical units or back, and puts the cameras' traced light and the traced scene on or
//! off. Without it the game lights itself as before (lamps, sun shadows, an ambient fill).
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

use std::collections::HashMap;
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

/// The materials whose emission has been scaled to physical units (each only once): by how
/// much, and how exposure took it before (to put it back when the lighting's switched off).
#[derive(Resource, Default)]
pub struct ScaledEmissive(pub HashMap<AssetId<StandardMaterial>, (f32, f32)>);

static WANTED: AtomicBool = AtomicBool::new(false);
static SUPPORTED: AtomicBool = AtomicBool::new(false);

/// Whether the lighting is ray traced (it's wanted, and the GPU can).
pub fn active() -> bool {
    WANTED.load(Ordering::Relaxed) && SUPPORTED.load(Ordering::Relaxed)
}

/// The lighting follows the setting (`PORTAL_NORT=1` keeps it off).
fn follow_setting(settings: Res<crate::settings::Settings>) {
    if settings.is_changed() {
        WANTED.store(settings.ray_tracing >= 0.5 && std::env::var_os("PORTAL_NORT").is_none(), Ordering::Relaxed);
    }
}

/// The setting as shown.
pub fn label(v: f32) -> String {
    let on = v >= 0.5;
    if on && !SUPPORTED.load(Ordering::Relaxed) { "On (no GPU support)".into() } else if on { "On".into() } else { "Off".into() }
}

/// Add ray-traced lighting to the app (before the renderer starts): always, so it can be
/// switched on later; on from the start if it's `wanted`.
pub fn add(app: &mut App, wanted: bool) {
    WANTED.store(wanted, Ordering::Relaxed);
    app.add_plugins(SolariPlugins)
        .init_resource::<RtMeshes>()
        .init_resource::<ScaledEmissive>()
        .add_systems(Startup, check_support)
        .add_systems(Update, (follow_setting, forward_overlays, switch, (cameras, emissive_materials, sky_dome).run_if(active)).chain())
        .add_systems(PostUpdate, register_meshes.after(VisibilitySystems::VisibilityPropagate).run_if(active));
}

/// Whether the GPU has what the traced light needs; drawing as the lighting wants.
fn check_support(mut commands: Commands, device: Option<Res<RenderDevice>>) {
    let ok = device.is_some_and(|d| d.features().contains(SolariPlugins::required_wgpu_features()));
    SUPPORTED.store(ok, Ordering::Relaxed);
    if !ok && WANTED.load(Ordering::Relaxed) {
        warn!("Ray-traced lighting asked for, but this GPU can't: lighting the ordinary way");
    } else if active() {
        info!("Ray-traced lighting on");
    }
    // (Solari's plugin made drawing deferred: forward unless the light's traced.)
    if !active() {
        commands.insert_resource(DefaultOpaqueRendererMethod::forward());
    }
}

/// Ray-traced lighting switched on or off while running: materials drawn deferred (what
/// Solari needs) or forward, what glows scaled to physical units or back, the cameras' traced
/// light and the traced scene put on or taken off. Coming on, the cameras get theirs (from
/// `cameras`) the same frame, so nothing deferred meets a view that can't draw it; going off,
/// they keep theirs until every mesh draws forward again. (`cameras`, `emissive_materials`,
/// `sky_dome` and `register_meshes` put the rest back.)
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn switch(
    mut commands: Commands,
    mut was: Local<Option<bool>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut scaled: ResMut<ScaledEmissive>,
    player: Query<Entity, With<PlayerCamera>>,
    portals: Query<Entity, With<PortalCam>>,
    traced: Query<Entity, With<RaytracingMesh3d>>,
    domes: Query<Entity, With<SkyDome>>,
    mut drawn: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut redo: Local<u8>,
    mut off_in: Local<Option<u8>>,
) {
    // Each mesh's drawing is set up once, by how its material draws: for a few frames after a
    // switch, every mesh is set up again (as the materials are prepared again).
    if *redo > 0 {
        *redo -= 1;
        for mut m in &mut drawn {
            m.set_changed();
        }
    }
    // Going off: the cameras' traced light and deferred passes, once nothing draws deferred.
    if let Some(n) = off_in.as_mut() {
        if active() {
            *off_in = None;
        } else if *n > 0 {
            *n -= 1;
        } else {
            *off_in = None;
            // (The player's anti-aliasing is the graphics settings' business: they've put back
            // what they want.)
            for e in &player {
                commands.entity(e).remove::<(SolariLighting, DeferredPrepass)>().insert(CameraMainTextureUsages::default());
            }
            for e in &portals {
                commands.entity(e).remove::<(SolariLighting, DepthPrepass, DeferredPrepass, bevy::anti_alias::taa::TemporalAntiAliasing)>().insert(CameraMainTextureUsages::default());
            }
        }
    }
    let now = active();
    let Some(before) = *was else {
        // (At startup it's already as it should be.)
        *was = Some(now);
        return;
    };
    if before == now {
        return;
    }
    *was = Some(now);
    info!("Ray-traced lighting {}", if now { "on" } else { "off" });
    commands.insert_resource(if now { DefaultOpaqueRendererMethod::deferred() } else { DefaultOpaqueRendererMethod::forward() });
    // Every material prepared again, to draw the new way (marked changed: just looking at it
    // isn't), and every mesh set up again.
    let ids: Vec<_> = materials.ids().collect();
    for id in ids {
        if let Some(m) = materials.get_mut(id) {
            m.into_inner();
        }
    }
    *redo = 4;
    if now {
        return;
    }
    *off_in = Some(8);
    for (id, (scale, weight)) in scaled.0.drain() {
        if let Some(mut m) = materials.get_mut(id) {
            m.emissive = LinearRgba::from_vec3(m.emissive.to_vec3() / scale);
            m.emissive_exposure_weight = weight;
        }
    }
    for e in &traced {
        commands.entity(e).remove::<RaytracingMesh3d>();
    }
    for e in &domes {
        commands.entity(e).despawn();
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
/// draw forward (always: the lighting can be switched to deferred at any time).
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
    mut done: ResMut<ScaledEmissive>,
) {
    let done = &mut done.0;
    let lamp = mats.map(|m| m.lamp.id());
    let is_lamp = |id: AssetId<StandardMaterial>| Some(id) == lamp || lamps.as_ref().is_some_and(|l| l.0.contains(&id));
    let todo: Vec<_> = materials
        .iter()
        .filter(|(id, m)| !done.contains_key(id) && m.emissive.to_vec3() != Vec3::ZERO)
        .map(|(id, _)| id)
        .collect();
    for id in todo {
        // Bounce marks set their own glow every frame, already scaled.
        if scorches.as_ref().is_some_and(|s| s.0.contains(&id)) {
            done.insert(id, (1.0, 1.0));
            continue;
        }
        if let Some(mut m) = materials.get_mut(id) {
            let scale = EMISSIVE_SCALE * if is_lamp(id) { LAMP_BOOST } else { 1.0 };
            done.insert(id, (scale, m.emissive_exposure_weight));
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
    // (Built for this level, and there when it should be: switched off, it went.)
    if *built == Some(game.loads) && (game.sim.level.indoor || !domes.is_empty()) {
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

