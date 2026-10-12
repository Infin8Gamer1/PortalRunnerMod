//! Pictures of the props for the browser, made when their tiles first show: each prop is put
//! on a render layer of its own, framed by a camera drawing into a small image, read back, and
//! kept (and saved under `thumbs/` in the extracted folder, for next time).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bevy::app::Propagate;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::core_pipeline::prepass::{DeferredPrepass, DepthPrepass};
use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::render::view::window::screenshot::{Screenshot, ScreenshotCaptured};

use super::library::{Library, Source};
use crate::me_props::{MePackages, MePropCache};

/// The render layer props are drawn on for their pictures (nothing else is).
pub const LAYER: usize = 20;
const SIZE: u32 = 144;
const FOV: f32 = 0.5;

#[derive(Resource, Default)]
pub struct Thumbs {
    /// The pictures made, by prop.
    pub ready: HashMap<usize, Handle<Image>>,
    /// The props whose tiles are showing (the browser fills this each frame).
    pub wanted: Vec<usize>,
    /// The props whose pictures couldn't be made.
    pub failed: HashSet<usize>,
    job: Option<Job>,
    target: Handle<Image>,
    camera: Option<Entity>,
    /// Where the read-back picture arrives.
    captured: Arc<Mutex<Option<Image>>>,
}

struct Job {
    prop: usize,
    root: Entity,
    bounds: (Vec3, Vec3),
    frames: u32,
    /// Frames drawn since it was ready, and whether it's been read back.
    drawn: u32,
    shot: bool,
}

/// Where a prop's picture is kept.
fn cached(lib: &Library, i: usize) -> Option<PathBuf> {
    let root = crate::extracted::find_root()?;
    let p = &lib.props[i];
    Some(match &p.source {
        Source::Model(path) => root.join("thumbs").join(path.trim_end_matches(".glb")).with_extension("png"),
        Source::Me(package, mesh) => root.join("thumbs/me").join(package).join(format!("{mesh}.png")),
    })
}

/// Where a Mirror's Edge prop's box is kept (so it's known before its mesh is read again).
fn cached_bounds(lib: &Library, i: usize) -> Option<PathBuf> {
    cached(lib, i).map(|p| p.with_extension("bounds"))
}

pub fn setup(mut commands: Commands, mut thumbs: ResMut<Thumbs>, mut images: ResMut<Assets<Image>>) {
    let mut target = Image::new_target_texture(SIZE, SIZE, TextureFormat::Rgba8UnormSrgb, None);
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    thumbs.target = images.add(target);
    let layer = RenderLayers::layer(LAYER);
    thumbs.camera = Some(
        commands
            .spawn((
                Name::new("Editor: thumbnail camera"),
                Camera3d::default(),
                Camera { order: -200, is_active: false, clear_color: ClearColorConfig::Custom(Color::srgb(0.13, 0.13, 0.14)), ..default() },
                RenderTarget::Image(thumbs.target.clone().into()),
                Projection::Perspective(PerspectiveProjection { fov: FOV, near: 0.01, ..default() }),
                Msaa::Off,
                layer.clone(),
            ))
            .id(),
    );
    // A key light over the camera's shoulder and a dimmer one from the other side.
    commands.spawn((DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(1.0, 2.0, -1.5).looking_at(Vec3::ZERO, Vec3::Y), layer.clone()));
    commands.spawn((DirectionalLight { illuminance: 3000.0, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(-1.5, 0.5, 1.0).looking_at(Vec3::ZERO, Vec3::Y), layer));
}

/// Load pictures made before; make one more (a prop at a time) for what's showing.
#[allow(clippy::too_many_arguments)]
pub fn run(
    mut commands: Commands,
    mut thumbs: ResMut<Thumbs>,
    mut lib: ResMut<Library>,
    mut images: ResMut<Assets<Image>>,
    assets: Res<AssetServer>,
    mut cams: Query<(&mut Camera, &mut Transform)>,
    scenes: Query<&WorldAssetRoot>,
    children: Query<&Children>,
    mut pkgs: Option<NonSendMut<MePackages>>,
    mut me_cache: ResMut<MePropCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let thumbs = &mut *thumbs;
    let Some(cam) = thumbs.camera else { return };
    let wanted = std::mem::take(&mut thumbs.wanted);
    // ---- the picture under way
    if let Some(job) = &mut thumbs.job {
        job.frames += 1;
        let ready = match scenes.get(job.root) {
            // A model: once it's loaded, and spawned (it has children).
            Ok(root) => assets.is_loaded_with_dependencies(&root.0) && children.get(job.root).is_ok_and(|c| !c.is_empty()),
            Err(_) => true,
        };
        if job.shot {
            if let Some(img) = thumbs.captured.lock().ok().and_then(|mut c| c.take()) {
                // (As a plain picture: the one read back is set up as a render target's copy.)
                if let Ok(rgba) = img.try_into_dynamic().map(|d| d.to_rgba8()) {
                    if let Some(path) = cached(&lib, job.prop) {
                        let _ = std::fs::create_dir_all(path.parent().unwrap());
                        let _ = rgba.save(&path);
                    }
                    let mut img = Image::from_dynamic(rgba.into(), true, RenderAssetUsages::default());
                    img.sampler = ImageSampler::linear();
                    thumbs.ready.insert(job.prop, images.add(img));
                } else {
                    thumbs.failed.insert(job.prop);
                }
                commands.entity(job.root).despawn();
                if let Ok((mut c, _)) = cams.get_mut(cam) {
                    c.is_active = false;
                }
                thumbs.job = None;
            } else if job.frames > 900 {
                thumbs.failed.insert(job.prop);
                commands.entity(job.root).despawn();
                thumbs.job = None;
            }
        } else if ready {
            if job.drawn == 0 {
                // Framed: from the front, a little above and to the side.
                let (lo, hi) = job.bounds;
                let c = (lo + hi) * 0.5;
                let r = ((hi - lo).length() * 0.5).max(0.05);
                let dir = Vec3::new(0.75, 0.55, -1.2).normalize();
                if let Ok((mut camera, mut tf)) = cams.get_mut(cam) {
                    *tf = Transform::from_translation(c + dir * (r / (FOV * 0.5).sin()) * 1.02).looking_at(c, Vec3::Y);
                    camera.is_active = true;
                }
                // (Ray-traced lighting draws everything deferred: so must this.)
                if crate::raytrace::active() {
                    commands.entity(cam).insert((DepthPrepass, DeferredPrepass));
                }
            }
            job.drawn += 1;
            // A few frames for its textures to arrive, then read it back.
            if job.drawn == 6 {
                let slot = thumbs.captured.clone();
                commands.spawn(Screenshot::image(thumbs.target.clone())).observe(move |cap: On<ScreenshotCaptured>| {
                    if let Ok(mut s) = slot.lock() {
                        *s = Some(cap.image.clone());
                    }
                });
                job.shot = true;
            }
        } else if job.frames > 900 {
            thumbs.failed.insert(job.prop);
            commands.entity(job.root).despawn();
            thumbs.job = None;
        }
        return;
    }
    // ---- pictures made before (a few a frame), then the next to make
    let mut loaded = 0;
    for &i in &wanted {
        if thumbs.ready.contains_key(&i) || thumbs.failed.contains(&i) || i >= lib.props.len() {
            continue;
        }
        if let Some(path) = cached(&lib, i).filter(|p| p.is_file()) {
            if loaded >= 12 {
                continue;
            }
            loaded += 1;
            if lib.props[i].bounds.is_none() {
                lib.props[i].bounds = cached_bounds(&lib, i).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| read_bounds(&t));
            }
            match std::fs::read(&path).ok().and_then(|b| Image::from_buffer(&b, ImageType::Extension("png"), CompressedImageFormats::NONE, true, ImageSampler::linear(), RenderAssetUsages::default()).ok()) {
                Some(img) => {
                    thumbs.ready.insert(i, images.add(img));
                }
                None => {
                    thumbs.failed.insert(i);
                }
            }
            continue;
        }
        // Make this one.
        let layer = Propagate(RenderLayers::layer(LAYER));
        let root = match &lib.props[i].source {
            Source::Model(path) => {
                let Some(bounds) = lib.props[i].bounds else {
                    thumbs.failed.insert(i);
                    continue;
                };
                let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(format!("extracted/{path}")));
                (commands.spawn((Name::new("Editor: thumbnail"), WorldAssetRoot(scene), layer, Transform::default(), Visibility::default())).id(), bounds)
            }
            Source::Me(package, mesh) => {
                let Some(pkgs) = pkgs.as_deref_mut() else {
                    thumbs.failed.insert(i);
                    break;
                };
                match crate::me_props::parts_and_bounds((package, mesh), &mut me_cache, pkgs, &mut meshes, &mut materials, &mut images) {
                    Some((parts, bounds)) => {
                        lib.props[i].bounds = Some(bounds);
                        if let Some(p) = cached_bounds(&lib, i) {
                            let _ = std::fs::create_dir_all(p.parent().unwrap());
                            let _ = std::fs::write(p, format!("{} {} {} {} {} {}", bounds.0.x, bounds.0.y, bounds.0.z, bounds.1.x, bounds.1.y, bounds.1.z));
                        }
                        let root = commands
                            .spawn((Name::new("Editor: thumbnail"), layer, Transform::default(), Visibility::default()))
                            .with_children(|c| {
                                for (m, mat) in parts {
                                    c.spawn((Mesh3d(m), MeshMaterial3d(mat), Transform::default()));
                                }
                            })
                            .id();
                        (root, bounds)
                    }
                    None => {
                        thumbs.failed.insert(i);
                        continue;
                    }
                }
            }
        };
        thumbs.job = Some(Job { prop: i, root: root.0, bounds: root.1, frames: 0, drawn: 0, shot: false });
        break;
    }
}

fn read_bounds(t: &str) -> Option<(Vec3, Vec3)> {
    let n: Vec<f32> = t.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    (n.len() == 6).then(|| (Vec3::new(n[0], n[1], n[2]), Vec3::new(n[3], n[4], n[5])))
}
