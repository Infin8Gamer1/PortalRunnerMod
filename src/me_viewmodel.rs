//! Faith's real first-person body, read from the player's own Mirror's Edge
//! install at startup (nothing from the game ships with this project).
//!
//! It works the way the game does: the body (arms from `SK_UpperBody`, legs
//! from `SK_LowerBody`, one skeleton) stands in the world, turned with the
//! player's yaw, and the camera *is* the skeleton's `CameraJoint` bone plus
//! the player's look pitch. So head bob, wallrun tilt, landing dips and the
//! roll come from the animations, the hands land where the animation puts
//! them (on the ledge lip when hanging), and looking down shows your legs.
//!
//! The arms render on their own layer with depth cleared, so they never clip
//! into walls; the legs render in the world.
//!
//! Where it looks for the game, in order: the `FAITH_ME_DIR` environment
//! variable, a `me_path.txt` file in the working directory, then common
//! install folders. If none has the game files, the procedural arms stay on.
//! F2 switches between the two.

use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use faith_move::CameraFxSettings;
use me_assets::pose::section_indices;
use me_assets::{FaithArms, MaterialSlot, SkeletalMesh, Skinner};

use faith_anim::{Driver, Rig};
use crate::viewmodel::{Arm, ViewmodelCamera, VIEWMODEL_LAYER};
use crate::portal_render::PLAYER_LAYER;
use crate::{Game, PlayerCamera};

/// TdPawn.Model1pFOV in DefaultGame.ini.
const MODEL_1P_FOV_DEG: f32 = 100.0;

const DEFAULT_PATHS: [&str; 6] = [
    r"C:\Games\Mirror's Edge",
    r"C:\Program Files (x86)\Steam\steamapps\common\mirrors edge",
    r"C:\Program Files\Steam\steamapps\common\mirrors edge",
    r"C:\Program Files\EA Games\Mirror's Edge",
    r"C:\Program Files (x86)\EA Games\Mirror's Edge",
    r"C:\Program Files (x86)\Origin Games\Mirror's Edge",
];

/// One skinned mesh (arms or legs): a root entity placed at the body each
/// frame, with one child per material section sharing the skinned vertices.
struct Part {
    skinner: Skinner,
    meshes: Vec<Handle<Mesh>>,
    root: Entity,
    /// The section entities (their render layer switches with the move's depth group).
    sections: Vec<Entity>,
    legs: bool,
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    tangents: Vec<[f32; 4]>,
}

#[derive(Resource)]
pub struct MeArms {
    arms: FaithArms,
    /// The animation, body placement and camera (faith_anim, shared with other hosts).
    rig: Rig,
    parts: Vec<Part>,
    pub active: bool,
    /// Scratch: the arms' morph deltas this frame.
    morph: Vec<Vec3>,
}

impl MeArms {
    pub fn current_anim(&self) -> &str {
        self.rig.driver.current()
    }

    /// Sound cues the animations hit this frame.
    pub fn notifies(&self) -> &[me_assets::anim::Notify] {
        self.rig.driver.notifies()
    }

    /// Faith's body and animations as loaded.
    pub fn arms(&self) -> &FaithArms {
        &self.arms
    }

    /// Where the game files were loaded from.
    pub fn cooked_pc(&self) -> &std::path::Path {
        &self.arms.cooked_pc
    }
}

pub fn candidates() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(p) = std::env::var_os("FAITH_ME_DIR") {
        v.push(PathBuf::from(p));
    }
    if let Ok(s) = std::fs::read_to_string("me_path.txt") {
        let s = s.trim();
        if !s.is_empty() {
            v.push(PathBuf::from(s));
        }
    }
    v.extend(DEFAULT_PATHS.iter().map(PathBuf::from));
    // Steam on Linux (and Steam Deck): the Windows build under Proton.
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for steam in [".local/share/Steam", ".steam/steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"] {
            v.push(home.join(steam).join("steamapps/common/mirrors edge"));
        }
    }
    v
}

pub(crate) fn image_from(t: &me_assets::Rgba) -> Image {
    image_with(t, t.pixels.clone(), TextureFormat::Rgba8UnormSrgb)
}

/// Normal maps hold vectors, not colours: no sRGB curve.
pub(crate) fn normal_image(t: &me_assets::Rgba) -> Image {
    image_with(t, t.pixels.clone(), TextureFormat::Rgba8Unorm)
}

/// The game's specular map as Bevy's metallic/roughness texture: shinier
/// where the spec map is bright (G = roughness, B = metallic = 0).
pub(crate) fn roughness_image(t: &me_assets::Rgba) -> Image {
    let px = t
        .pixels
        .chunks_exact(4)
        .flat_map(|c| {
            let spec = (c[0] as f32 * 0.299 + c[1] as f32 * 0.587 + c[2] as f32 * 0.114) / 255.0;
            let rough = (0.92 - 0.55 * spec).clamp(0.3, 0.95);
            [255, (rough * 255.0) as u8, 0, 255]
        })
        .collect();
    image_with(t, px, TextureFormat::Rgba8Unorm)
}

fn image_with(t: &me_assets::Rgba, pixels: Vec<u8>, format: TextureFormat) -> Image {
    let mut img = Image::new(
        Extent3d { width: t.width, height: t.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        pixels,
        format,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

#[allow(clippy::too_many_arguments)]
fn spawn_part(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    mesh: &SkeletalMesh,
    slots: &[MaterialSlot],
    globals: &[Mat4],
    legs: bool,
) -> Part {
    let skinner = Skinner::new(mesh);
    let (mut positions, mut normals, mut tangents) = (vec![], vec![], vec![]);
    skinner.skin_full(mesh, globals, Mat4::IDENTITY, &mut positions, &mut normals, Some(&mut tangents));
    let uvs: Vec<[f32; 2]> = mesh.vertices.iter().map(|v| v.uv).collect();
    // The legs draw in the world, but only for her own camera (not through portals).
    let layer = if legs { RenderLayers::layer(PLAYER_LAYER) } else { RenderLayers::layer(VIEWMODEL_LAYER) };
    let root = commands.spawn((Transform::default(), Visibility::default(), layer.clone())).id();
    let mut handles = vec![];
    let mut sections = vec![];
    // Every section, the lower body's torso (Faith's top) included: it's what you see when you
    // look down. Hiding it left the open waist of the trousers in view.
    for (k, sec) in mesh.sections.iter().enumerate() {
        let m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, tangents.clone())
            .with_inserted_indices(Indices::U32(section_indices(mesh, k)));
        let h = meshes.add(m);
        let slot = slots.get(sec.material as usize);
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
            // Skinned on the CPU every frame (no motion for traced light to follow): lit the
            // ordinary way, as the arms' overlay is anyway.
            opaque_render_method: bevy::material::OpaqueRendererMethod::Forward,
            ..default()
        });
        let e = commands.spawn((Mesh3d(h.clone()), MeshMaterial3d(mat), Transform::default(), layer.clone(), NotShadowCaster)).id();
        commands.entity(root).add_child(e);
        handles.push(h);
        sections.push(e);
    }
    Part { skinner, meshes: handles, root, sections, legs, positions, normals, tangents }
}

pub fn setup(
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let mut last_err = None;
    let mut loaded = None;
    for dir in candidates() {
        if me_assets::cooked_pc(&dir).is_none() {
            continue;
        }
        match FaithArms::load(&dir, 1024) {
            Ok(a) => {
                loaded = Some((a, dir));
                break;
            }
            Err(e) => last_err = Some(format!("{}: {e}", dir.display())),
        }
    }
    let Some((arms, dir)) = loaded else {
        match last_err {
            Some(e) => warn!("Mirror's Edge body: couldn't load ({e}); using procedural arms"),
            None => info!("Mirror's Edge install not found; using procedural arms (set FAITH_ME_DIR or me_path.txt)"),
        }
        return;
    };
    info!("Mirror's Edge body loaded from {} (legs: {})", dir.display(), arms.legs.is_some());

    let rig = Rig::new(&arms, game.ctrl.yaw);
    let driver: &Driver = &rig.driver;
    let mut parts = vec![spawn_part(
        &mut commands, &mut meshes, &mut materials, &mut images, &arms.mesh, &arms.materials, &driver.globals, false,
    )];
    if let Some((legs, slots)) = &arms.legs {
        parts.push(spawn_part(&mut commands, &mut meshes, &mut materials, &mut images, legs, slots, &driver.globals, true));
    }

    // Let the move timings follow the animations they play.
    rig.tune(&arms, &mut game.ctrl.tuning);
    game.fx.settings = CameraFxSettings::animation_driven();

    commands.insert_resource(MeArms { arms, rig, parts, active: true, morph: vec![] });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn animate(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<Game>,
    me: Option<ResMut<MeArms>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut main_cam: Query<(&mut Transform, &Projection), (With<PlayerCamera>, Without<ViewmodelCamera>)>,
    mut vm_cam: Query<(&mut Transform, &mut Projection), (With<ViewmodelCamera>, Without<PlayerCamera>)>,
    mut proc_arms: Query<&mut Visibility, With<Arm>>,
    mut roots: Query<(&mut Transform, &mut Visibility), (Without<Arm>, Without<PlayerCamera>, Without<ViewmodelCamera>)>,
    mut layers: Query<&mut RenderLayers, Without<ViewmodelCamera>>,
) {
    let Some(mut me) = me else { return };
    let me = &mut *me;
    if keys.just_pressed(KeyCode::F2) {
        me.active = !me.active;
        game.fx.settings = if me.active { CameraFxSettings::animation_driven() } else { CameraFxSettings::default() };
    }
    for mut v in &mut proc_arms {
        *v = if me.active { Visibility::Hidden } else { Visibility::Inherited };
    }
    for p in &me.parts {
        if let Ok((_, mut v)) = roots.get_mut(p.root) {
            *v = if me.active { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
    if !me.active {
        return;
    }

    let dt = time.delta_secs().min(0.1);

    if keys.just_pressed(KeyCode::KeyG) {
        me.rig.driver.play_idle(&game.ctrl, &me.arms);
    }
    // Through a portal: the body turns with the view at once.
    for e in &game.ctrl.events {
        if let faith_move::Event::Teleport { yaw_delta } = *e {
            me.rig.teleported(yaw_delta);
        }
    }
    // The body, its placement and the camera (faith_anim::Rig, as the game places them).
    let frame = me.rig.update(dt, &game.ctrl, &game.shot, &me.arms);
    let c = &game.ctrl;

    // TdMove.FirstPersonDPG: the body normally draws over the world (SDPG_Foreground, our
    // viewmodel layer); swinging and pipe climbing use SDPG_Intermediate, depth-tested against
    // the world, so the bar hides the fingers wrapped round it.
    let arms_layer = if frame.intermediate { RenderLayers::layer(PLAYER_LAYER) } else { RenderLayers::layer(VIEWMODEL_LAYER) };
    for p in me.parts.iter().filter(|p| !p.legs) {
        for &e in &p.sections {
            if let Ok(mut l) = layers.get_mut(e) {
                if *l != arms_layer {
                    *l = arms_layer.clone();
                }
            }
        }
    }

    for p in &mut me.parts {
        let mesh = if p.legs { &me.arms.legs.as_ref().unwrap().0 } else { &me.arms.mesh };
        if p.legs || me.arms.morphs.is_empty() {
            p.skinner.skin_full(mesh, &me.rig.driver.globals, Mat4::IDENTITY, &mut p.positions, &mut p.normals, Some(&mut p.tangents));
        } else {
            // The forearm twist morphs (TdPlayerPawn.Init1pArms), weighted by each roll bone's
            // twist: Blend90 rolling one way, Blend90m the other, full at 90 degrees. Without
            // them the skinning pinches the forearm where it twists.
            let morph = &mut me.morph;
            me.rig.driver.forearm_morphs(&me.arms, morph);
            p.skinner.skin_morphed(mesh, &me.rig.driver.globals, Mat4::IDENTITY, morph, &mut p.positions, &mut p.normals, Some(&mut p.tangents));
        }
        for h in &p.meshes {
            if let Some(mut m) = meshes.get_mut(h) {
                m.insert_attribute(Mesh::ATTRIBUTE_POSITION, p.positions.clone());
                m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, p.normals.clone());
                m.insert_attribute(Mesh::ATTRIBUTE_TANGENT, p.tangents.clone());
            }
        }
        if let Ok((mut tf, _)) = roots.get_mut(p.root) {
            tf.translation = frame.origin;
            tf.rotation = if p.legs { frame.legs_rot } else { frame.body_rot };
        }
    }

    // ---- the camera is the camera bone, plus look pitch and screen shake
    let (cam_pos, cam_rot) = (frame.cam_pos, frame.cam_rot);

    if let Ok((mut tf, proj)) = main_cam.single_mut() {
        // (A spectator camera has the view: the hands stay with it, drawn or not.)
        if game.spectate.is_none() {
            tf.translation = cam_pos;
            tf.rotation = cam_rot;
        }
        if let Ok((mut vt, mut vproj)) = vm_cam.single_mut() {
            *vt = *tf;
            // Mirror's Edge renders the first-person body at its own FOV,
            // TdPawn.Model1pFOV = 100° (the world uses 90°), which keeps hands
            // at the edge of view (a palm on the wall while wallrunning) on screen.
            let mut p = proj.clone();
            if let Projection::Perspective(pp) = &mut p {
                pp.near = 0.01;
                // Looking down, the arms layer blends to the world's FOV so
                // the torso (arms layer) and legs (world) line up at the waist.
                let world_h = 2.0 * ((pp.fov * 0.5).tan() * pp.aspect_ratio).atan();
                let t = ((-c.pitch - 0.35) / 0.55).clamp(0.0, 1.0);
                let t = t * t * (3.0 - 2.0 * t);
                let h = MODEL_1P_FOV_DEG.to_radians() * (1.0 - t) + world_h * t;
                pp.fov = 2.0 * ((h * 0.5).tan() / pp.aspect_ratio.max(0.1)).atan();
            }
            *vproj = p;
        }
    }
}
