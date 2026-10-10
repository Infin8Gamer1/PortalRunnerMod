//! Faith as others would see her: Mirror's Edge's third-person body (`SK_TKY_Crim_Fixer`,
//! with `AS_F3P_Unarmed`), animated by its own copy of the animation driver from the same
//! movement, so you see yourself through portals: running, jumping, wallrunning, hanging.
//!
//! It draws on its own layer, which the portal cameras and the lights see but the player's
//! camera doesn't (that shows the first-person body), so it also casts her shadow. While
//! she's in a portal's opening a second copy stands at the far side, carried through it,
//! so the half of her that's already through shows there.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use me_assets::pose::section_indices;
use me_assets::{FaithArms, Skinner};

use faith_anim::Rig;

use crate::me_viewmodel::{candidates, image_from, normal_image, roughness_image};
use crate::Game;

/// The third-person body's render layer.
pub const BODY_LAYER: usize = 12;
/// Her copy through the portal she's in. Not drawn by that portal's own first view, whose
/// camera is her eye carried through the same way: it would be inside the copy's head.
pub const THROUGH_LAYER: usize = 13;

#[derive(Resource)]
pub struct Body3p {
    body: FaithArms,
    rig: Rig,
    skinner: Skinner,
    meshes: Vec<Handle<Mesh>>,
    /// Where she is, and her copy through the portal she's in.
    root: Entity,
    through: Entity,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    tangents: Vec<[f32; 4]>,
}

pub fn setup(
    mut commands: Commands,
    game: Res<Game>,
    arms: Option<Res<crate::me_viewmodel::MeArms>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Only alongside the first-person body (same install).
    let Some(arms) = arms else { return };
    let cooked = arms.cooked_pc().to_path_buf();
    let install = candidates().into_iter().find(|d| me_assets::cooked_pc(d).is_some_and(|c| c == cooked)).unwrap_or(cooked);
    let body = match FaithArms::load_body3p(&install, 1024) {
        Ok(b) => b,
        Err(e) => {
            warn!("Mirror's Edge third-person body: couldn't load ({e})");
            return;
        }
    };
    info!("Mirror's Edge third-person body: {} bones, {} vertices", body.mesh.bones.len(), body.mesh.vertices.len());
    let rig = Rig::new(&body, game.ctrl.yaw);
    let skinner = Skinner::new(&body.mesh);
    let (mut positions, mut normals, mut tangents) = (vec![], vec![], vec![]);
    skinner.skin_full(&body.mesh, &rig.driver.globals, Mat4::IDENTITY, &mut positions, &mut normals, Some(&mut tangents));
    let uvs: Vec<[f32; 2]> = body.mesh.vertices.iter().map(|v| v.uv).collect();
    let layer = RenderLayers::layer(BODY_LAYER);
    let root = commands.spawn((Transform::default(), Visibility::default(), layer.clone())).id();
    let through_layer = RenderLayers::layer(THROUGH_LAYER);
    let through = commands.spawn((Transform::default(), Visibility::Hidden, through_layer.clone())).id();
    let mut handles = vec![];
    for (k, sec) in body.mesh.sections.iter().enumerate() {
        let m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents.clone())
            .with_inserted_indices(Indices::U32(section_indices(&body.mesh, k)));
        let h = meshes.add(m);
        let slot = body.materials.get(sec.material as usize);
        let tex = slot.and_then(|s| s.diffuse.as_ref()).map(|t| images.add(image_from(t)));
        let normal_map = slot.and_then(|s| s.normal.as_ref()).map(|t| images.add(normal_image(t)));
        let rough = slot.and_then(|s| s.specular.as_ref()).map(|t| images.add(roughness_image(t)));
        let mat = materials.add(StandardMaterial {
            base_color: if tex.is_some() { Color::WHITE } else { Color::srgb(0.12, 0.12, 0.13) },
            base_color_texture: tex,
            perceptual_roughness: if rough.is_some() { 1.0 } else { 0.8 },
            metallic: 0.0,
            metallic_roughness_texture: rough,
            normal_map_texture: normal_map,
            reflectance: 0.3,
            ..default()
        });
        for (parent, layer) in [(root, &layer), (through, &through_layer)] {
            let e = commands.spawn((Mesh3d(h.clone()), MeshMaterial3d(mat.clone()), Transform::default(), layer.clone())).id();
            commands.entity(parent).add_child(e);
        }
        handles.push(h);
    }
    commands.insert_resource(Body3p { body, rig, skinner, meshes: handles, root, through, positions, normals, tangents });
}

pub fn animate(
    time: Res<Time>,
    game: Res<Game>,
    settings: Res<crate::settings::Settings>,
    body: Option<ResMut<Body3p>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut roots: Query<(&mut Transform, &mut Visibility)>,
) {
    let Some(mut b) = body else { return };
    let b = &mut *b;
    let on = settings.body3p > 0.5;
    if let Ok((_, mut v)) = roots.get_mut(b.root) {
        let want = if on { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    if !on {
        if let Ok((_, mut v)) = roots.get_mut(b.through) {
            *v = Visibility::Hidden;
        }
        return;
    }
    let dt = time.delta_secs().min(0.1);
    for e in &game.ctrl.events {
        if let faith_move::Event::Teleport { yaw_delta } = *e {
            b.rig.teleported(yaw_delta);
        }
    }
    let frame = b.rig.update(dt, &game.ctrl, &game.shot, &b.body);
    b.skinner.skin_full(&b.body.mesh, &b.rig.driver.globals, Mat4::IDENTITY, &mut b.positions, &mut b.normals, Some(&mut b.tangents));
    for h in &b.meshes {
        if let Some(mut m) = meshes.get_mut(h) {
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, b.positions.clone());
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, b.normals.clone());
            m.insert_attribute(Mesh::ATTRIBUTE_TANGENT, b.tangents.clone());
        }
    }
    // The whole body turns with the legs (one mesh; the upper body's twist is in the pose).
    let here = Transform::from_translation(frame.origin).with_rotation(frame.legs_rot);
    if let Ok((mut tf, _)) = roots.get_mut(b.root) {
        *tf = here;
    }
    // Her copy through the portal she's in.
    let link = game.sim.player_env.and_then(|c| game.sim.link(c));
    if let Ok((mut tf, mut v)) = roots.get_mut(b.through) {
        match link {
            Some(l) => {
                tf.translation = l.point(here.translation);
                tf.rotation = l.quat() * here.rotation;
                *v = Visibility::Inherited;
            }
            None => *v = Visibility::Hidden,
        }
    }
}
