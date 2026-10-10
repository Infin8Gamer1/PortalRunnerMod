//! Drawing portals: what you see through them, recursively.
//!
//! Each portal has a camera per recursion level. The level-1 camera for the blue portal sits
//! where the player's camera would be if it were carried through blue and out of orange,
//! renders the world into a screen-sized image, and blue's surface shows that image in screen
//! space, so it lines up exactly with the view. Things between that camera and orange's wall
//! (the wall itself) are cut away by an oblique near plane on orange's surface. Level 2 is
//! carried through twice, and so on; the deepest level's surfaces show only the portal's
//! colour.
//!
//! Render layers keep it straight: the world is layer 0, Faith's own body is
//! [`PLAYER_LAYER`] (the player camera only: through a portal she'd be a headless pair of
//! legs), and the portal surfaces each camera may see are [`SURFACE_LAYER`] + its level.

use bevy::asset::{load_internal_asset, uuid_handle, RenderAssetUsages};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraProjection, RenderTarget, SubCameraView};
use bevy::camera::primitives::Frustum;
use bevy::light::cluster::ClusterConfig;
use bevy::light::NotShadowCaster;
use bevy::math::Vec3A;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{DistanceFog, Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::window::PrimaryWindow;

use aperture::{Portal, PortalColor};

use crate::{Game, PlayerCamera};

/// Faith's legs and torso: drawn by the player camera only.
pub const PLAYER_LAYER: usize = 2;
/// Portal surfaces seen from recursion level `k` are on this layer + k (k = 0: the player's
/// camera).
pub const SURFACE_LAYER: usize = 4;
/// Most recursion levels there are cameras for.
pub const MAX_DEPTH: usize = 4;
/// How far in front of its wall a portal's surface (and its cameras' near plane) sits:
/// above the gel painted on walls (`chamber::GEL_MAX_LIFT`), so gel neither covers the
/// opening nor shows up flat across the view through it.
pub const SURFACE_LIFT: f32 = 0.012;

/// How deep the tunnel behind an opening goes: the eye can be this far through before
/// she's carried across (her eye leads her body).
const TUNNEL_DEPTH: f32 = 0.6;

const PORTAL_SHADER: Handle<Shader> = uuid_handle!("7d3b8a1e-2f4c-4f8e-9a51-6c2e0b9d4a17");

pub fn color_of(c: PortalColor) -> LinearRgba {
    match c {
        PortalColor::Blue => LinearRgba::rgb(0.06, 0.38, 1.0),
        PortalColor::Orange => LinearRgba::rgb(1.0, 0.36, 0.02),
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct PortalMaterial {
    #[uniform(0)]
    pub uniform: PortalUniform,
    #[texture(1)]
    #[sampler(2)]
    pub view: Option<Handle<Image>>,
}

#[derive(Clone, Copy, Debug, Default, bevy::render::render_resource::ShaderType)]
pub struct PortalUniform {
    pub color: Vec4,
    /// x linked, y time, z open (0..1), w mode (0 surface, 1 tunnel, 2 deepest).
    pub params: Vec4,
}

impl Material for PortalMaterial {
    fn fragment_shader() -> ShaderRef {
        PORTAL_SHADER.into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

/// A perspective projection whose near plane is a given plane (Eric Lengyel's oblique
/// near-plane clipping, for Bevy's reverse-Z infinite projection): everything on the far side
/// of the portal's surface is cut away. Culling and shadow cascades use the plain frustum.
#[derive(Debug, Clone)]
pub struct ObliqueProjection {
    pub fov: f32,
    pub aspect: f32,
    pub near: f32,
    /// The clip plane in view space: points with `plane.xyz . p + plane.w >= 0` are kept.
    pub plane: Option<Vec4>,
}

impl Default for ObliqueProjection {
    fn default() -> Self {
        ObliqueProjection { fov: 1.0, aspect: 16.0 / 9.0, near: 0.01, plane: None }
    }
}

impl ObliqueProjection {
    fn base(&self) -> PerspectiveProjection {
        PerspectiveProjection { fov: self.fov, aspect_ratio: self.aspect, near: self.near, far: 1000.0, ..default() }
    }
}

impl CameraProjection for ObliqueProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        let mut m = Mat4::perspective_infinite_reverse_rh(self.fov, self.aspect, self.near);
        let Some(c) = self.plane else { return m };
        // Row 2 (clip z) becomes row 3 (clip w = -z) minus s x the plane: depth is 1 on the
        // plane and falls towards 0 away from it; behind it, it's past the near plane. `s`
        // keeps every visible point's depth at or above 0.
        let ty = (self.fov * 0.5).tan();
        let tx = ty * self.aspect;
        let reach = c.x.abs() * tx + c.y.abs() * ty - c.z;
        let s = 1.0 / reach.max(1e-3);
        m.x_axis.z = -s * c.x;
        m.y_axis.z = -s * c.y;
        m.z_axis.z = -1.0 - s * c.z;
        m.w_axis.z = -s * c.w;
        m
    }

    fn get_clip_from_view_for_sub(&self, _sub_view: &SubCameraView) -> Mat4 {
        self.get_clip_from_view()
    }

    fn update(&mut self, width: f32, height: f32) {
        self.aspect = width / height.max(1.0);
    }

    fn far(&self) -> f32 {
        1000.0
    }

    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [Vec3A; 8] {
        self.base().get_frustum_corners(z_near, z_far)
    }

    fn compute_frustum(&self, camera_transform: &GlobalTransform) -> Frustum {
        self.base().compute_frustum(camera_transform)
    }
}

#[derive(Component)]
pub struct PortalCam {
    pub color: PortalColor,
    pub level: usize,
}

#[derive(Component)]
pub struct PortalSurface {
    pub color: PortalColor,
    pub level: usize,
}

/// The inside of the tunnel behind a portal: drawn (without the oval) only while the
/// camera is right at the surface, so stepping through never shows the inside of the wall.
#[derive(Component)]
pub struct PortalTunnel {
    pub color: PortalColor,
}

#[derive(Resource)]
pub struct PortalRender {
    /// Recursion levels drawn (1..=MAX_DEPTH).
    pub depth: usize,
    /// The portal views' resolution, as a fraction of the window's.
    pub scale: f32,
    size: UVec2,
    /// Per level (index 0 = level 1), per colour.
    images: Vec<[Handle<Image>; 2]>,
    /// Surface materials per colour per level (0..=MAX_DEPTH), and the tunnels'.
    surface_mats: [Vec<Handle<PortalMaterial>>; 2],
    tunnel_mats: [Handle<PortalMaterial>; 2],
}

pub struct PortalRenderPlugin;

impl Plugin for PortalRenderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, PORTAL_SHADER, "shaders/portal.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<PortalMaterial>::default()).add_systems(Startup, setup);
    }
}

fn target_image(size: UVec2) -> Image {
    // Linear HDR: the player's camera tonemaps the portal's view along with everything else.
    let mut img = Image::new_target_texture(size.x.max(1), size.y.max(1), TextureFormat::Rgba16Float, None);
    img.asset_usage = RenderAssetUsages::RENDER_WORLD;
    img
}

/// A quad in the portal's local frame (X right, Y up, facing +Z), `half` in size, with UVs
/// across it.
fn portal_quad(half: Vec2) -> Mesh {
    let (x, y) = (half.x, half.y);
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-x, -y, 0.0], [x, -y, 0.0], [x, y, 0.0], [-x, y, 0.0]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

/// The tunnel behind the opening, seen from inside: its four walls and back.
fn tunnel_mesh(half: Vec2, depth: f32) -> Mesh {
    let (x, y, d) = (half.x, half.y, -depth);
    let mut pos = Vec::new();
    let mut idx = Vec::new();
    // Each face's corners, anticlockwise seen from inside (the side that's drawn).
    let faces = [
        [[-x, -y, 0.0], [-x, -y, d], [-x, y, d], [-x, y, 0.0]],
        [[x, -y, d], [x, -y, 0.0], [x, y, 0.0], [x, y, d]],
        [[-x, y, 0.0], [-x, y, d], [x, y, d], [x, y, 0.0]],
        [[-x, -y, d], [-x, -y, 0.0], [x, -y, 0.0], [x, -y, d]],
        [[-x, -y, d], [x, -y, d], [x, y, d], [-x, y, d]],
    ];
    for f in faces {
        let b = pos.len() as u32;
        pos.extend_from_slice(&f);
        idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.5, 0.5]; n])
        .with_inserted_indices(Indices::U32(idx))
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<PortalMaterial>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let size = windows.single().map(|w| UVec2::new(w.physical_width(), w.physical_height())).unwrap_or(UVec2::new(1600, 900));
    let images_per_level: Vec<[Handle<Image>; 2]> = (0..MAX_DEPTH).map(|_| [images.add(target_image(size)), images.add(target_image(size))]).collect();

    for level in 1..=MAX_DEPTH {
        for c in PortalColor::BOTH {
            commands.spawn((
                PortalCam { color: c, level },
                Camera3d::default(),
                Camera {
                    // Deepest first, so each level's image is ready for the one above it.
                    order: -(level as isize) * 4 - c.index() as isize,
                    is_active: false,
                    ..default()
                },
                RenderTarget::from(images_per_level[level - 1][c.index()].clone()),
                Projection::custom(ObliqueProjection::default()),
                Transform::default(),
                cam_layers(level, true),
                // The oblique projection would confuse the light clusters' depth slices.
                ClusterConfig::Single,
            ));
        }
    }

    let quad = meshes.add(portal_quad(aperture::portal::PORTAL_HALF));
    let tunnel = meshes.add(tunnel_mesh(aperture::portal::PORTAL_HALF, TUNNEL_DEPTH));
    let mut surface_mats: [Vec<Handle<PortalMaterial>>; 2] = [Vec::new(), Vec::new()];
    for c in PortalColor::BOTH {
        let col = color_of(c);
        for level in 0..=MAX_DEPTH {
            let view = images_per_level.get(level).map(|l| l[c.index()].clone());
            let m = mats.add(PortalMaterial { uniform: PortalUniform { color: col.to_vec4(), params: Vec4::ZERO }, view });
            surface_mats[c.index()].push(m.clone());
            commands.spawn((
                PortalSurface { color: c, level },
                Mesh3d(quad.clone()),
                MeshMaterial3d(m),
                Transform::default(),
                Visibility::Hidden,
                RenderLayers::layer(SURFACE_LAYER + level),
                NotShadowCaster,
            ));
        }
    }
    let tunnel_mats = PortalColor::BOTH.map(|c| {
        mats.add(PortalMaterial {
            uniform: PortalUniform { color: color_of(c).to_vec4(), params: Vec4::new(1.0, 0.0, 1.0, 1.0) },
            view: Some(images_per_level[0][c.index()].clone()),
        })
    });
    for c in PortalColor::BOTH {
        commands.spawn((
            PortalTunnel { color: c },
            Mesh3d(tunnel.clone()),
            MeshMaterial3d(tunnel_mats[c.index()].clone()),
            Transform::default(),
            Visibility::Hidden,
            RenderLayers::layer(SURFACE_LAYER),
            NotShadowCaster,
        ));
    }
    commands.insert_resource(PortalRender { depth: 3, scale: 1.0, size, images: images_per_level, surface_mats, tunnel_mats });
}

/// What a portal camera draws: the world, the surfaces at its level, Faith's body, and her
/// copy through a portal (`through`).
fn cam_layers(level: usize, through: bool) -> RenderLayers {
    let layers = RenderLayers::from_layers(&[0, SURFACE_LAYER + level, crate::body3p::BODY_LAYER]);
    if through {
        layers.with(crate::body3p::THROUGH_LAYER)
    } else {
        layers
    }
}

/// Whether any of a portal's opening shows on a camera's screen.
fn on_screen(clip_from_world: Mat4, cam: Vec3, p: &Portal) -> bool {
    let dist = p.signed_distance(cam);
    // In the opening (or just through it, before she's carried across): it's all round.
    if dist < 0.5 && dist > -TUNNEL_DEPTH && p.in_rect(cam, -0.3) {
        return true;
    }
    if dist < -0.05 {
        return false;
    }
    let mut lo = Vec2::splat(f32::INFINITY);
    let mut hi = Vec2::splat(f32::NEG_INFINITY);
    for c in p.corners() {
        let q = clip_from_world * c.extend(1.0);
        if q.w <= 1e-4 {
            // Some of it is beside or behind the camera: assume it shows.
            return true;
        }
        let n = q.truncate().truncate() / q.w;
        lo = lo.min(n);
        hi = hi.max(n);
    }
    hi.x >= -1.0 && lo.x <= 1.0 && hi.y >= -1.0 && lo.y <= 1.0
}

/// Place the portal cameras and surfaces for this frame (after the player camera has moved).
#[allow(clippy::type_complexity)]
pub fn update(
    game: Res<Game>,
    time: Res<Time>,
    mut render: ResMut<PortalRender>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<PortalMaterial>>,
    main: Query<(&Transform, &Projection, Option<&DistanceFog>), (With<PlayerCamera>, Without<PortalCam>)>,
    mut cams: Query<(Entity, &PortalCam, &mut Camera, &mut Transform, &mut Projection, &mut RenderLayers, Option<&mut DistanceFog>), Without<PlayerCamera>>,
    mut surfaces: Query<(&PortalSurface, &mut Transform, &mut Visibility), (Without<PortalCam>, Without<PlayerCamera>, Without<PortalTunnel>)>,
    mut tunnels: Query<(&PortalTunnel, &mut Transform, &mut Visibility), (Without<PortalCam>, Without<PlayerCamera>, Without<PortalSurface>)>,
    mut commands: Commands,
) {
    let Ok((main_tf, main_proj, main_fog)) = main.single() else { return };
    let sim = &game.sim;
    let t = time.elapsed_secs();

    // Keep the images the size of the window (times the resolution setting).
    if let Ok(w) = windows.single() {
        let scale = render.scale.clamp(0.25, 1.0);
        let size = UVec2::new(((w.physical_width() as f32 * scale) as u32).max(1), ((w.physical_height() as f32 * scale) as u32).max(1));
        if size != render.size {
            render.size = size;
            for level in &render.images {
                for h in level {
                    if let Some(mut img) = images.get_mut(h) {
                        *img = target_image(size);
                    }
                }
            }
        }
    }

    let (fov, aspect, near) = match main_proj {
        Projection::Perspective(p) => (p.fov, p.aspect_ratio, p.near),
        _ => (1.0, 16.0 / 9.0, 0.01),
    };
    let base = PerspectiveProjection { fov, aspect_ratio: aspect, near, far: 1000.0, ..default() };
    let clip_of = |tf: &Transform| base.get_clip_from_view() * tf.to_matrix().inverse();
    let linked = sim.linked();
    let depth = render.depth.clamp(1, MAX_DEPTH);

    // Which levels are worth drawing, and where their cameras go.
    let mut active = [[false; MAX_DEPTH + 1]; 2];
    let mut cam_tf = [[Transform::default(); MAX_DEPTH + 1]; 2];
    for c in PortalColor::BOTH {
        cam_tf[c.index()][0] = *main_tf;
        active[c.index()][0] = true;
        let (Some(p), Some(link)) = (sim.portal(c), sim.link(c)) else { continue };
        let q = link.quat();
        for level in 1..=depth {
            let prev = cam_tf[c.index()][level - 1];
            if !active[c.index()][level - 1] || !on_screen(clip_of(&prev), prev.translation, &p) {
                break;
            }
            active[c.index()][level] = true;
            cam_tf[c.index()][level] = Transform { translation: link.point(prev.translation), rotation: q * prev.rotation, scale: Vec3::ONE };
        }
    }

    for (e, pc, mut cam, mut tf, mut proj, mut layers, fog) in &mut cams {
        let ci = pc.color.index();
        let on = linked && pc.level <= depth && active[ci][pc.level];
        if cam.is_active != on {
            cam.is_active = on;
        }
        if !on {
            continue;
        }
        // Her copy through the portal she's in stands where that portal's first camera is.
        let want = cam_layers(pc.level, !(pc.level == 1 && sim.player_env == Some(pc.color)));
        if *layers != want {
            *layers = want;
        }
        *tf = cam_tf[ci][pc.level];
        // Clip at the exit portal's surface (pushed out a hair so its own wall goes too).
        let exit = sim.portal(pc.color.other()).unwrap();
        let n = exit.normal;
        let plane_world = n.extend(-n.dot(exit.center + n * SURFACE_LIFT));
        let view_from_world = tf.to_matrix();
        let plane_view = view_from_world.transpose() * plane_world;
        if let Projection::Custom(custom) = &mut *proj {
            if let Some(o) = custom.get_mut::<ObliqueProjection>() {
                o.fov = fov;
                o.aspect = aspect;
                o.near = near;
                // With the camera already out in front of the exit (her eye through the
                // opening before she is), there's no wall between it and the view to cut away,
                // and an oblique near plane behind the camera would cut into the view instead.
                o.plane = (plane_view.w < -1e-4).then_some(plane_view);
            }
        }
        match (main_fog, fog) {
            (Some(mf), Some(mut f)) => *f = mf.clone(),
            (Some(mf), None) => {
                commands.entity(e).insert(mf.clone());
            }
            (None, Some(_)) => {
                commands.entity(e).remove::<DistanceFog>();
            }
            (None, None) => {}
        }
    }

    // Surfaces: on the portal, a hair in front of its wall (and of any gel on it).
    for (s, mut tf, mut vis) in &mut surfaces {
        let Some(p) = sim.portal(s.color) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let show = s.level <= depth;
        *vis = if show { Visibility::Inherited } else { Visibility::Hidden };
        tf.translation = p.center + p.normal * SURFACE_LIFT;
        tf.rotation = p.rotation();
        let open = (p.age / 0.25).clamp(0.0, 1.0);
        let open = 1.0 - (1.0 - open).powi(3);
        let deepest = s.level >= depth || !active[s.color.index()][s.level + 1];
        let mode = if deepest { 2.0 } else { 0.0 };
        if let Some(mut m) = mats.get_mut(&render.surface_mats[s.color.index()][s.level]) {
            m.uniform.params = Vec4::new(if linked { 1.0 } else { 0.0 }, t, open, mode);
            if deepest && linked {
                // The far end of a hall of portals: just the colour, but not the swirl of a
                // closed one.
                m.uniform.params.x = 0.0;
            }
        }
    }
    // The tunnel: only when the camera is in (or right at) the opening.
    for (tn, mut tf, mut vis) in &mut tunnels {
        let Some(p) = sim.portal(tn.color) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let l = p.to_local(main_tf.translation);
        let near_it = linked && l.z < 0.15 && l.z > 0.05 - TUNNEL_DEPTH && l.x.abs() < p.half.x && l.y.abs() < p.half.y;
        *vis = if near_it { Visibility::Inherited } else { Visibility::Hidden };
        tf.translation = p.center + p.normal * SURFACE_LIFT;
        tf.rotation = p.rotation();
        if let Some(mut m) = mats.get_mut(&render.tunnel_mats[tn.color.index()]) {
            m.uniform.params = Vec4::new(1.0, t, 1.0, 1.0);
        }
    }
}
