//! The level editor (F4): fly round a level, drag boxes and puzzle pieces into it from the
//! content browser, move, turn and size them with the gizmo, wire them up in the details panel,
//! and play it (F5 or F4) straight away with the real simulation.
//!
//! What's edited is an [`aperture::doc::LevelDoc`], saved as RON under `levels/`; those files
//! are listed after the game's chambers. The editor's level is one more entry in the game's
//! list, made from the document each time it's loaded, so the view is always the game's own
//! drawing of it: every change reloads it (cheap for a chamber). Opening the editor in a
//! built chamber starts from a copy of it.

pub mod catalog;
mod ui;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraMainTextureUsages, CameraOutputMode, Exposure, Hdr};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::gizmos::transform_gizmo::{TransformGizmoCamera, TransformGizmoFocus, TransformGizmoMeshMarker, TransformGizmoMode, TransformGizmoPlugin, TransformGizmoSettings, TransformGizmoState, TransformGizmoSystems};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::material::OpaqueRendererMethod;
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use bevy_egui::{egui, EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext};

use aperture::doc::{Kind, LevelDoc, Object};
use faith_move::Aabb;

use crate::{menu, Game, PlayerCamera};

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((EguiPlugin::default(), TransformGizmoPlugin))
            .insert_resource(EguiGlobalSettings { auto_create_primary_context: false, ..default() })
            .insert_resource(TransformGizmoSettings { snap_translate: Some(0.25), snap_rotate: Some(15f32.to_radians()), snap_scale: Some(0.25), ..default() })
            .init_resource::<Editor>()
            .configure_sets(PostUpdate, TransformGizmoSystems.run_if(editing))
            .add_systems(Startup, setup)
            .add_systems(Update, (gizmo_cameras, forward_gizmos))
            .add_systems(EguiPrimaryContextPass, ui::draw.run_if(editing));
    }
}

/// The editor's systems, run after the game's each frame (the camera is the editor's).
pub fn systems() -> impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, ()> {
    (toggle, fly, interact, rebuild, overlays, hide_game_views).chain()
}

pub fn editing(ed: Res<Editor>) -> bool {
    ed.active
}

/// The game is being played, not edited.
pub fn idle(ed: Res<Editor>) -> bool {
    !ed.active
}

/// Where the camera is and how fast it flies.
pub struct FlyCam {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
}

#[derive(Resource)]
pub struct Editor {
    pub active: bool,
    pub doc: LevelDoc,
    /// The file it was opened from or saved to.
    pub path: Option<PathBuf>,
    pub selected: Option<usize>,
    undo: Vec<LevelDoc>,
    redo: Vec<LevelDoc>,
    pub unsaved: bool,
    /// Reload the level from the document.
    rebuild: bool,
    /// What the editor's level entry builds.
    shared: Arc<RwLock<LevelDoc>>,
    /// The editor's entry in the game's level list.
    slot: Option<usize>,
    /// The level files in the game's list: (its index, the file).
    pub files: Vec<(usize, PathBuf)>,
    /// A document has been opened (the first F4 opens the level being played).
    started: bool,
    /// Open the editor next frame (`PORTAL_EDIT`).
    enter_next: bool,
    pub cam: FlyCam,
    /// Looking round with the right button held (it began over the view, not a panel).
    looking: bool,
    pub grid: f32,
    pub status: String,
    /// The catalog entry being dragged out of the content browser.
    pub drop: Option<usize>,
    /// Where it would land.
    ghost: Vec<Object>,
    /// The next click in the view sets the selected faith plate's target.
    pub pick_target: bool,
    /// The document before the gizmo drag under way.
    before_drag: Option<LevelDoc>,
    /// The document before the details panel's edit under way (one undo step per edit).
    pub before_edit: Option<LevelDoc>,
    /// The name to save under.
    pub save_name: String,
    /// Where the panels are (in the window's logical pixels), for drops.
    pub panels: Vec<egui::Rect>,
    /// The proxy the gizmo moves.
    handle: Option<Entity>,
    ui_camera: Option<Entity>,
    ui_sink: Option<Entity>,
}

impl Default for Editor {
    fn default() -> Self {
        let doc = LevelDoc::starter("Untitled");
        Editor {
            active: false,
            shared: Arc::new(RwLock::new(doc.clone())),
            save_name: doc.name.clone(),
            doc,
            path: None,
            selected: None,
            undo: Vec::new(),
            redo: Vec::new(),
            unsaved: false,
            rebuild: false,
            slot: None,
            files: Vec::new(),
            started: false,
            enter_next: false,
            cam: FlyCam { pos: Vec3::new(0.0, 3.0, 4.0), yaw: 0.0, pitch: -0.2, speed: 8.0 },
            looking: false,
            grid: 0.25,
            status: String::new(),
            drop: None,
            ghost: Vec::new(),
            pick_target: false,
            before_drag: None,
            before_edit: None,
            panels: Vec::new(),
            handle: None,
            ui_camera: None,
            ui_sink: None,
        }
    }
}

impl Editor {
    /// Record the document as it was before a change (one undo step), and show the change.
    pub fn commit(&mut self, before: LevelDoc) {
        if before == self.doc {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.unsaved = true;
        self.rebuild = true;
    }

    /// Change the document (one undo step).
    pub fn change(&mut self, f: impl FnOnce(&mut LevelDoc)) {
        let before = self.doc.clone();
        f(&mut self.doc);
        self.commit(before);
    }

    pub fn undo(&mut self) {
        if let Some(d) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, d));
            self.after_history();
        }
    }

    pub fn redo(&mut self) {
        if let Some(d) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, d));
            self.after_history();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn after_history(&mut self) {
        self.unsaved = true;
        self.rebuild = true;
        if self.selected.is_some_and(|i| i >= self.doc.objects.len()) {
            self.selected = None;
        }
    }

    /// Start on a new document.
    pub fn open(&mut self, doc: LevelDoc, path: Option<PathBuf>) {
        self.save_name = doc.name.clone();
        self.doc = doc;
        self.path = path;
        self.selected = None;
        self.undo.clear();
        self.redo.clear();
        self.unsaved = false;
        self.rebuild = true;
        self.started = true;
        if let Some(s) = self.doc.objects.iter().find(|o| o.kind == Kind::Spawn) {
            self.cam = FlyCam { pos: s.at + Vec3::new(0.0, 2.5, 0.0), yaw: s.yaw.to_radians(), pitch: -0.3, speed: self.cam.speed };
        }
    }

    /// Place a catalog entry where the middle of the view meets the level.
    pub fn place_in_view(&mut self, entry: usize) {
        let rot = Quat::from_euler(EulerRot::YXZ, self.cam.yaw, self.cam.pitch, 0.0);
        let look = rot * Vec3::NEG_Z;
        let (p, n) = surface(&self.doc, self.cam.pos, look);
        self.add(entry, p, n, look);
    }

    /// Place a catalog entry at `p` on a surface facing `n`, seen looking along `look`.
    pub fn add(&mut self, entry: usize, p: Vec3, n: Vec3, look: Vec3) {
        let e = &catalog::CATALOG[entry];
        let objs = catalog::place(e, &self.doc, p, n, look, self.grid);
        let count = objs.len();
        self.change(|doc| doc.objects.extend(objs));
        self.selected = Some(self.doc.objects.len() - 1);
        self.status = format!("Placed {}{}", e.name, if count > 1 { format!(" ({count} objects)") } else { String::new() });
    }

    fn over_panels(&self, window: &Window) -> bool {
        window.cursor_position().is_some_and(|c| self.panels.iter().any(|r| r.contains(egui::pos2(c.x, c.y))))
    }

    pub fn delete_selected(&mut self) {
        if let Some(i) = self.selected.take() {
            self.change(|d| {
                d.objects.remove(i);
            });
        }
    }

    pub fn duplicate_selected(&mut self) {
        if let Some(i) = self.selected {
            let mut o = self.doc.objects[i].clone();
            o.at.x += self.grid.max(0.25) * 4.0;
            self.change(|d| d.objects.push(o));
            self.selected = Some(self.doc.objects.len() - 1);
        }
    }

    /// Save the document: to its file, or a new one named after it in `levels/`.
    pub fn save(&mut self, game: &mut Game, as_new: bool) {
        if as_new || self.path.is_none() {
            self.doc.name = self.save_name.trim().to_string();
            if self.doc.name.is_empty() {
                self.doc.name = "Untitled".into();
            }
            self.path = Some(new_file(&self.doc.name));
        }
        let Some(path) = self.path.clone() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&path, self.doc.to_ron()) {
            Ok(()) => {
                self.unsaved = false;
                self.status = format!("Saved {}", path.display());
                if !self.files.iter().any(|(_, p)| *p == path) {
                    let i = game.levels.len();
                    game.levels.push((self.doc.name.clone(), file_level(path.clone())));
                    game.source_names.push(None);
                    self.files.push((i, path));
                }
            }
            Err(e) => self.status = format!("Couldn't save {}: {e}", path.display()),
        }
    }

    /// Edit level `i` of the game's list: its file, or a copy of a built chamber.
    pub fn open_level(&mut self, game: &Game, i: usize) {
        if let Some((_, path)) = self.files.iter().find(|(j, _)| *j == i) {
            match read_doc(path) {
                Ok(d) => self.open(d, Some(path.clone())),
                Err(e) => self.status = e,
            }
        } else if game.source_names.get(i).is_some_and(|s| s.is_some()) {
            self.status = "Portal's own maps can't be edited (their geometry is the map's): starting a new level".into();
            self.open(LevelDoc::starter("Untitled"), None);
        } else {
            let (mut d, skipped) = LevelDoc::from_level(&(game.levels[i].1)());
            d.name = format!("{} (copy)", d.name);
            self.open(d, None);
            self.unsaved = true;
            self.status = if skipped > 0 { format!("Imported {} ({skipped} things the editor can't show were left out)", game.levels[i].0) } else { format!("Imported {}", game.levels[i].0) };
        }
    }

    fn enter(&mut self, game: &mut Game, menu: &mut menu::Menu) {
        if self.slot.is_none() {
            let shared = self.shared.clone();
            self.slot = Some(game.levels.len());
            game.levels.push(("Editor".into(), Arc::new(move || shared.read().map(|d| d.build()).unwrap_or_else(|_| LevelDoc::starter("Untitled").build()))));
            game.source_names.push(None);
        }
        // The level being played, unless that's the editor's own or there's unsaved work.
        if !self.started || (Some(game.level_index) != self.slot && !self.unsaved) {
            let i = game.level_index;
            self.open_level(game, i);
            // Start where she's standing (in the level that's open, not a Portal map's).
            if game.source_names[i].is_none() {
                let v = game.shot.view;
                self.cam = FlyCam { pos: v.eye, yaw: v.yaw, pitch: v.pitch, speed: self.cam.speed };
            }
        }
        self.active = true;
        self.rebuild = true;
        game.locked = false;
        menu.open = false;
        menu.title = false;
    }

    /// The editor's entry in the game's level list, once it has one.
    pub fn slot(&self) -> Option<usize> {
        self.slot
    }

    /// Play the level from its start.
    pub fn play(&mut self, menu: &mut menu::Menu) {
        self.active = false;
        self.rebuild = true;
        self.drop = None;
        self.pick_target = false;
        menu.start = true;
    }
}

/// Where level files are: `PORTAL_LEVELS`, else `levels/` in the source tree.
pub fn levels_dir() -> PathBuf {
    if let Ok(p) = std::env::var("PORTAL_LEVELS") {
        return PathBuf::from(p);
    }
    let here = PathBuf::from("levels");
    if here.is_dir() {
        return here;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("levels")
}

/// The level files, by name.
pub fn level_files() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(levels_dir()).map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "ron")).collect()).unwrap_or_default();
    v.sort();
    v
}

fn new_file(name: &str) -> PathBuf {
    let slug: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    let slug = slug.trim_matches('_').to_string();
    let slug = if slug.is_empty() { "level".to_string() } else { slug };
    let dir = levels_dir();
    let mut path = dir.join(format!("{slug}.ron"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{slug}_{n}.ron"));
        n += 1;
    }
    path
}

pub fn read_doc(path: &Path) -> Result<LevelDoc, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    LevelDoc::from_ron(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// A level made from a file each time it's loaded (so saving shows on the next load).
fn file_level(path: PathBuf) -> crate::source_map::LevelFn {
    Arc::new(move || match read_doc(&path) {
        Ok(d) => d.build(),
        Err(e) => {
            error!("{e}");
            LevelDoc::starter("Broken level file").build()
        }
    })
}

/// The level files, after the game's chambers and Portal's maps (startup).
pub fn add_levels(mut game: ResMut<Game>, mut ed: ResMut<Editor>) {
    let before = game.levels.len();
    for path in level_files() {
        let name = read_doc(&path).map(|d| d.name).unwrap_or_else(|_| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        info!("Level {}: {} ({})", game.levels.len(), name, path.display());
        ed.files.push((game.levels.len(), path.clone()));
        game.levels.push((name, file_level(path)));
        game.source_names.push(None);
    }
    // PORTAL_MAP=<n> can start on a level file too.
    if let Some(i) = std::env::var("PORTAL_MAP").ok().and_then(|v| v.parse::<usize>().ok()).filter(|i| (before..game.levels.len()).contains(i)) {
        game.pending = Some(i);
    }
    // PORTAL_EDIT=1: start in the editor.
    ed.enter_next = std::env::var_os("PORTAL_EDIT").is_some();
}

/// The proxy the gizmo moves, the camera egui draws on, and one for the game's own UI to
/// hide on while editing.
fn setup(mut commands: Commands, mut ed: ResMut<Editor>) {
    ed.handle = Some(commands.spawn((Name::new("Editor handle"), Transform::default(), Visibility::Hidden)).id());
    ed.ui_camera = Some(
        commands
            .spawn((
                Name::new("Editor UI camera"),
                PrimaryEguiContext,
                Camera2d,
                RenderLayers::none(),
                Camera {
                    order: 100,
                    is_active: false,
                    output_mode: CameraOutputMode::Write { blend_state: Some(BlendState::ALPHA_BLENDING), clear_color: ClearColorConfig::None },
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
            ))
            .id(),
    );
    ed.ui_sink = Some(commands.spawn((Name::new("Editor: game UI hidden"), Camera2d, RenderLayers::none(), Camera { order: -100, is_active: false, ..default() })).id());
}

/// The gizmo's own overlay camera is set up like the arms' and the gun's: the player camera's
/// main texture (HDR, MSAA, usages) so it draws over the picture rather than replacing it, no
/// clearing, and only while editing.
#[allow(clippy::type_complexity)]
fn gizmo_cameras(
    mut commands: Commands,
    ed: Res<Editor>,
    player: Query<(Entity, Ref<Msaa>, Option<&CameraMainTextureUsages>, Option<&Exposure>, Has<TransformGizmoCamera>), With<PlayerCamera>>,
    mut cams: Query<(Entity, &RenderLayers, &mut Camera, Has<crate::graphics::OverlayCamera>), (With<Camera3d>, Without<PlayerCamera>)>,
) {
    let Ok((player, msaa, usages, exposure, marked)) = player.single() else { return };
    if !marked {
        commands.entity(player).insert(TransformGizmoCamera);
    }
    for (e, layers, mut cam, tagged) in &mut cams {
        if *layers != RenderLayers::layer(15) {
            continue;
        }
        if !tagged || msaa.is_changed() {
            let mut ec = commands.entity(e);
            ec.insert((crate::graphics::OverlayCamera, *msaa, Hdr, Tonemapping::TonyMcMapface, usages.cloned().unwrap_or_default()));
            if let Some(x) = exposure {
                ec.insert(*x);
            }
            cam.clear_color = ClearColorConfig::None;
        }
        if cam.is_active != ed.active {
            cam.is_active = ed.active;
        }
    }
}

/// The gizmo's materials draw forward (with ray-traced lighting everything else is deferred,
/// which an overlay camera doesn't draw).
fn forward_gizmos(q: Query<&MeshMaterial3d<StandardMaterial>, Added<TransformGizmoMeshMarker>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    for m in &q {
        if let Some(mut mat) = materials.get_mut(&m.0) {
            mat.opaque_render_method = OpaqueRendererMethod::Forward;
        }
    }
}

fn typing(contexts: &mut EguiContexts) -> bool {
    contexts.ctx_mut().is_ok_and(|c| c.egui_wants_keyboard_input())
}

fn over_ui(contexts: &mut EguiContexts) -> bool {
    contexts.ctx_mut().is_ok_and(|c| c.is_pointer_over_egui() || c.egui_is_using_pointer())
}

/// F4: into the editor, or out to play what's being edited; F5: play.
fn toggle(mut ed: ResMut<Editor>, keys: Res<ButtonInput<KeyCode>>, mut game: ResMut<Game>, mut menu: ResMut<menu::Menu>, mut contexts: EguiContexts) {
    if std::mem::take(&mut ed.enter_next) {
        ed.enter(&mut game, &mut menu);
        return;
    }
    if ed.active && typing(&mut contexts) {
        return;
    }
    if keys.just_pressed(KeyCode::F4) {
        if ed.active {
            ed.play(&mut menu);
        } else if !menu.open || menu.title {
            ed.enter(&mut game, &mut menu);
        }
    } else if ed.active && keys.just_pressed(KeyCode::F5) {
        ed.play(&mut menu);
    }
}

/// Fly the camera: hold the right button to look round, with WASD to move, Q/E down and up,
/// Shift faster, the wheel for speed (or, not looking, to move in and out).
#[allow(clippy::too_many_arguments)]
fn fly(
    mut ed: ResMut<Editor>,
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut camera: Query<(&mut Transform, &mut Projection), With<PlayerCamera>>,
    mut contexts: EguiContexts,
    gizmo: Res<TransformGizmoState>,
) {
    if !ed.active {
        ed.looking = false;
        return;
    }
    let over = over_ui(&mut contexts);
    let dt = time.delta_secs().min(0.1);
    ed.looking = mouse.pressed(MouseButton::Right) && (ed.looking || (mouse.just_pressed(MouseButton::Right) && !over));
    if let Ok(mut c) = cursor.single_mut() {
        let (grab, visible) = if ed.looking { (CursorGrabMode::Locked, false) } else { (CursorGrabMode::None, true) };
        // (The gizmo confines the cursor itself while dragging.)
        if !gizmo.active && (c.grab_mode != grab || c.visible != visible) {
            c.grab_mode = grab;
            c.visible = visible;
        }
    }
    let looking = ed.looking;
    let cam = &mut ed.cam;
    if looking {
        cam.yaw -= motion.delta.x * 0.0025;
        cam.pitch = (cam.pitch - motion.delta.y * 0.0025).clamp(-1.55, 1.55);
        if scroll.delta.y != 0.0 {
            cam.speed = (cam.speed * 1.2f32.powf(scroll.delta.y.signum())).clamp(0.5, 80.0);
        }
    }
    let rot = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    let typing = typing(&mut contexts);
    if ed.looking || (!typing && !keys.pressed(KeyCode::ControlLeft)) {
        let mut mv = Vec3::ZERO;
        let held = |k: KeyCode| keys.pressed(k);
        // WASD only while looking (they're the gizmo's keys otherwise); the arrows always.
        if ed.looking {
            if held(KeyCode::KeyW) { mv.z -= 1.0; }
            if held(KeyCode::KeyS) { mv.z += 1.0; }
            if held(KeyCode::KeyA) { mv.x -= 1.0; }
            if held(KeyCode::KeyD) { mv.x += 1.0; }
            if held(KeyCode::KeyQ) { mv.y -= 1.0; }
            if held(KeyCode::KeyE) { mv.y += 1.0; }
        }
        if held(KeyCode::ArrowUp) { mv.z -= 1.0; }
        if held(KeyCode::ArrowDown) { mv.z += 1.0; }
        if held(KeyCode::ArrowLeft) { mv.x -= 1.0; }
        if held(KeyCode::ArrowRight) { mv.x += 1.0; }
        if held(KeyCode::PageUp) { mv.y += 1.0; }
        if held(KeyCode::PageDown) { mv.y -= 1.0; }
        let fast = if held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight) { 3.0 } else { 1.0 };
        let world = rot * Vec3::new(mv.x, 0.0, mv.z) + Vec3::Y * mv.y;
        let cam = &mut ed.cam;
        cam.pos += world.normalize_or_zero() * cam.speed * fast * dt;
    }
    if !ed.looking && !over && scroll.delta.y != 0.0 {
        let cam = &mut ed.cam;
        cam.pos += rot * Vec3::NEG_Z * scroll.delta.y * cam.speed.sqrt() * 0.5;
    }
    let cam = &ed.cam;
    if let Ok((mut tf, mut proj)) = camera.single_mut() {
        tf.translation = cam.pos;
        tf.rotation = rot;
        if let Projection::Perspective(p) = &mut *proj {
            p.fov = FOV;
            if let Ok(w) = windows.single() {
                p.aspect_ratio = w.width() / w.height().max(1.0);
            }
        }
    }
}

/// The editor camera's vertical field of view.
const FOV: f32 = 1.0;

/// The ray under the cursor.
fn cursor_ray(window: &Window, cam: &FlyCam) -> Option<(Vec3, Vec3)> {
    let c = window.cursor_position()?;
    let (w, h) = (window.width().max(1.0), window.height().max(1.0));
    let ndc = Vec2::new(2.0 * c.x / w - 1.0, 1.0 - 2.0 * c.y / h);
    let t = (FOV * 0.5).tan();
    let rot = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    Some((cam.pos, (rot * Vec3::new(ndc.x * t * w / h, ndc.y * t, -1.0)).normalize()))
}

/// Where a ray enters a box (not from inside it), and the face's normal.
fn ray_box(o: Vec3, d: Vec3, b: &Aabb) -> Option<(f32, Vec3)> {
    if (o.cmpge(b.min) & o.cmple(b.max)).all() {
        return None;
    }
    let mut near = f32::NEG_INFINITY;
    let mut far = f32::INFINITY;
    let mut n = Vec3::ZERO;
    for i in 0..3 {
        if d[i].abs() < 1e-8 {
            if o[i] < b.min[i] || o[i] > b.max[i] {
                return None;
            }
            continue;
        }
        let (t1, t2) = ((b.min[i] - o[i]) / d[i], (b.max[i] - o[i]) / d[i]);
        let (lo, hi) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        if lo > near {
            near = lo;
            n = Vec3::ZERO;
            n[i] = -d[i].signum();
        }
        far = far.min(hi);
    }
    (near <= far && near > 0.0).then_some((near, n))
}

/// The object a ray meets first.
fn pick(doc: &LevelDoc, o: Vec3, d: Vec3) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for (i, obj) in doc.objects.iter().enumerate() {
        if let Some((t, _)) = ray_box(o, d, &obj.bounds()) {
            // Pieces win over the walls round them by a little.
            let t = if obj.kind.is_box() { t + 0.05 } else { t };
            if best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t, i));
            }
        }
    }
    best.map(|(_, i)| i)
}

/// Where a ray meets the level's surfaces (its blocks, else the ground), and the normal there.
fn surface(doc: &LevelDoc, o: Vec3, d: Vec3) -> (Vec3, Vec3) {
    let mut best: Option<(f32, Vec3)> = None;
    for obj in &doc.objects {
        if !matches!(obj.kind, Kind::Block { .. } | Kind::Platform { .. } | Kind::MeProp { solid: Some(_), .. }) {
            continue;
        }
        if let Some((t, n)) = ray_box(o, d, &obj.bounds()) {
            if best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t, n));
            }
        }
    }
    if let Some((t, n)) = best {
        return (o + d * t, n);
    }
    if d.y < -1e-3 && o.y > 0.0 {
        return (o + d * (-o.y / d.y), Vec3::Y);
    }
    (o + d * 8.0, Vec3::Y)
}

/// Clicks, drops from the browser, the gizmo and the keys.
#[allow(clippy::too_many_arguments)]
fn interact(
    mut commands: Commands,
    mut ed: ResMut<Editor>,
    mut game: ResMut<Game>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut contexts: EguiContexts,
    gizmo: Res<TransformGizmoState>,
    mut settings: ResMut<TransformGizmoSettings>,
    mut handles: Query<&mut Transform>,
) {
    if !ed.active {
        if let Some(h) = ed.handle {
            commands.entity(h).remove::<TransformGizmoFocus>();
        }
        return;
    }
    let over = over_ui(&mut contexts);
    let typing = typing(&mut contexts);
    let Ok(window) = windows.single() else { return };
    let ray = cursor_ray(window, &ed.cam);
    let look = Quat::from_euler(EulerRot::YXZ, ed.cam.yaw, ed.cam.pitch, 0.0) * Vec3::NEG_Z;
    settings.snap_translate = (ed.grid > 0.0).then_some(ed.grid);
    settings.snap_scale = (ed.grid > 0.0).then_some(ed.grid);

    // ---- dragging something in from the browser
    // (egui holds the pointer while a tile's dragged: what matters is whether it's over a panel.)
    ed.ghost.clear();
    if let Some(entry) = ed.drop {
        if let Some((o, d)) = ray.filter(|_| !ed.over_panels(window)) {
            let (p, n) = surface(&ed.doc, o, d);
            if mouse.pressed(MouseButton::Left) {
                ed.ghost = catalog::place(&catalog::CATALOG[entry], &ed.doc, p, n, look, ed.grid);
            } else {
                ed.add(entry, p, n, look);
            }
        }
        if !mouse.pressed(MouseButton::Left) {
            ed.drop = None;
        }
    }

    // ---- the gizmo: the selection follows it while it's dragged
    let Some(h) = ed.handle else { return };
    if gizmo.active {
        if ed.before_drag.is_none() {
            ed.before_drag = Some(ed.doc.clone());
        }
        if let (Some(i), Ok(tf)) = (ed.selected, handles.get(h)) {
            let mode = settings.mode;
            let tf = *tf;
            if let Some(obj) = ed.doc.objects.get_mut(i) {
                match mode {
                    TransformGizmoMode::Translate => obj.at = tf.translation,
                    TransformGizmoMode::Scale if obj.kind.is_box() => {
                        // (Sized about its middle, as the gizmo scales.)
                        obj.size = tf.scale.abs().max(Vec3::splat(0.05));
                    }
                    TransformGizmoMode::Rotate if obj.kind.turns().is_some() => {
                        let (yaw, pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
                        obj.yaw = yaw.to_degrees();
                        if obj.kind.turns() == Some(true) {
                            obj.pitch = pitch.to_degrees();
                        }
                    }
                    _ => {}
                }
            }
        }
    } else if let Some(before) = ed.before_drag.take() {
        ed.commit(before);
    }
    if !gizmo.active {
        match ed.selected.and_then(|i| ed.doc.objects.get(i)) {
            Some(obj) => {
                if let Ok(mut tf) = handles.get_mut(h) {
                    tf.translation = obj.at;
                    tf.rotation = if obj.kind.turns().is_some() { obj.rotation() } else { Quat::IDENTITY };
                    tf.scale = if obj.kind.is_box() { obj.size } else { Vec3::ONE };
                }
                commands.entity(h).insert(TransformGizmoFocus);
            }
            None => {
                commands.entity(h).remove::<TransformGizmoFocus>();
            }
        }
    }

    // ---- clicking in the view
    if mouse.just_pressed(MouseButton::Left) && !over && ed.drop.is_none() && gizmo.hovered_axis.is_none() && !gizmo.active {
        if let Some((o, d)) = ray {
            if ed.pick_target {
                ed.pick_target = false;
                let (p, _) = surface(&ed.doc, o, d);
                if let Some(i) = ed.selected {
                    ed.change(|doc| {
                        if let Some(Kind::FaithPlate { target, .. }) = doc.objects.get_mut(i).map(|o| &mut o.kind) {
                            *target = p;
                        }
                    });
                }
            } else {
                ed.selected = pick(&ed.doc, o, d);
            }
        }
    }

    // ---- keys
    if typing || ed.looking {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if ctrl {
        if keys.just_pressed(KeyCode::KeyZ) {
            if shift { ed.redo() } else { ed.undo() }
        }
        if keys.just_pressed(KeyCode::KeyY) {
            ed.redo();
        }
        if keys.just_pressed(KeyCode::KeyD) {
            ed.duplicate_selected();
        }
        if keys.just_pressed(KeyCode::KeyS) {
            ed.save(&mut game, false);
        }
        return;
    }
    if keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace) {
        ed.delete_selected();
    }
    if keys.just_pressed(KeyCode::KeyW) {
        settings.mode = TransformGizmoMode::Translate;
    }
    if keys.just_pressed(KeyCode::KeyE) {
        settings.mode = TransformGizmoMode::Rotate;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        settings.mode = TransformGizmoMode::Scale;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        if let Some(obj) = ed.selected.and_then(|i| ed.doc.objects.get(i)) {
            let b = obj.bounds();
            let r = (b.size().length() * 0.5).max(1.0);
            let rot = Quat::from_euler(EulerRot::YXZ, ed.cam.yaw, ed.cam.pitch, 0.0);
            ed.cam.pos = b.center() - rot * Vec3::NEG_Z * (r * 2.2 + 1.0);
        }
    }
    if keys.just_pressed(KeyCode::Escape) {
        ed.selected = None;
        ed.pick_target = false;
    }
}

/// Reload the level from the document.
fn rebuild(mut ed: ResMut<Editor>, mut game: ResMut<Game>) {
    if !ed.rebuild {
        return;
    }
    ed.rebuild = false;
    if let Ok(mut s) = ed.shared.write() {
        *s = ed.doc.clone();
    }
    game.pending = ed.slot;
}

/// What the game doesn't draw (the start, triggers, cuts, wires), the selection, signals
/// between pieces, and where a dragged piece would land.
fn overlays(ed: Res<Editor>, mut gizmos: Gizmos) {
    if !ed.active {
        return;
    }
    let cuboid = |g: &mut Gizmos, b: &Aabb, c: Color| g.cube(Transform::from_translation(b.center()).with_scale(b.size()), c);
    let arrow = |g: &mut Gizmos, from: Vec3, dir: Vec3, len: f32, c: Color| {
        g.arrow(from, from + dir * len, c);
    };
    // A faint grid on the ground round the camera.
    let at = Vec3::new((ed.cam.pos.x / 1.0).round(), 0.002, (ed.cam.pos.z / 1.0).round());
    gizmos.grid(Isometry3d::new(at, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), UVec2::splat(40), Vec2::splat(1.0), Color::srgba(1.0, 1.0, 1.0, 0.12));
    for (i, o) in ed.doc.objects.iter().enumerate() {
        let b = o.bounds();
        match &o.kind {
            Kind::Spawn => {
                cuboid(&mut gizmos, &b, Color::srgb(0.3, 0.9, 0.45));
                arrow(&mut gizmos, o.at + Vec3::Y * 0.05, o.dir(), 1.2, Color::srgb(0.3, 0.9, 0.45));
            }
            Kind::Checkpoint { spawn, .. } => {
                cuboid(&mut gizmos, &b, Color::srgba(0.3, 0.8, 0.9, 0.7));
                arrow(&mut gizmos, o.at + *spawn + Vec3::Y * 0.05, o.dir(), 1.0, Color::srgb(0.3, 0.8, 0.9));
            }
            Kind::Exit => cuboid(&mut gizmos, &b, Color::srgba(0.3, 0.95, 0.35, 0.8)),
            Kind::Hint { .. } => cuboid(&mut gizmos, &b, Color::srgba(0.7, 0.45, 0.95, 0.7)),
            Kind::Cut => cuboid(&mut gizmos, &b, Color::srgba(0.95, 0.3, 0.3, 0.8)),
            Kind::Lamp => {
                gizmos.sphere(Isometry3d::from_translation(o.at), 0.2, Color::srgba(1.0, 0.95, 0.7, 0.4));
            }
            Kind::Wire { points, .. } => {
                let pts: Vec<Vec3> = std::iter::once(o.at).chain(points.iter().map(|p| o.at + *p)).collect();
                gizmos.linestrip(pts, Color::srgba(0.4, 0.6, 1.0, 0.8));
            }
            Kind::Platform { travel, .. } => {
                let end = Aabb::new(b.min + *travel, b.max + *travel);
                cuboid(&mut gizmos, &end, Color::srgba(0.6, 0.6, 0.7, 0.5));
                gizmos.line(b.center(), end.center(), Color::srgba(0.6, 0.6, 0.7, 0.5));
            }
            Kind::FaithPlate { target, apex, .. } if ed.selected == Some(i) => {
                // The arc it throws along.
                let top = o.at.y.max(target.y) + apex;
                let pts: Vec<Vec3> = (0..=24)
                    .map(|k| {
                        let t = k as f32 / 24.0;
                        let p = o.at.lerp(*target, t);
                        let y = if t < 0.5 { o.at.y + (top - o.at.y) * (1.0 - (1.0 - 2.0 * t).powi(2)) } else { target.y + (top - target.y) * (1.0 - (2.0 * t - 1.0).powi(2)) };
                        Vec3::new(p.x, y, p.z)
                    })
                    .collect();
                gizmos.linestrip(pts, Color::srgb(1.0, 0.85, 0.2));
                gizmos.sphere(Isometry3d::from_translation(*target), 0.3, Color::srgb(1.0, 0.85, 0.2));
            }
            _ => {}
        }
        if o.kind.turns() == Some(true) {
            arrow(&mut gizmos, o.at, o.dir(), 0.8, Color::srgba(1.0, 1.0, 1.0, 0.5));
        }
    }
    // The selection, and what it's wired to.
    if let Some(sel) = ed.selected.and_then(|i| ed.doc.objects.get(i)) {
        let b = sel.bounds();
        let grow = Aabb::new(b.min - 0.02, b.max + 0.02);
        cuboid(&mut gizmos, &grow, Color::srgb(1.0, 0.75, 0.1));
        let sigs: Vec<usize> = sel.kind.drives().into_iter().chain(sel.kind.inputs().map(|i| i.all.clone()).unwrap_or_default()).collect();
        for o in &ed.doc.objects {
            let linked = o.kind.drives().is_some_and(|s| sigs.contains(&s)) || o.kind.inputs().is_some_and(|i| i.all.iter().any(|s| sigs.contains(s)));
            if linked && !std::ptr::eq(o, sel) {
                gizmos.line(sel.bounds().center(), o.bounds().center(), Color::srgb(0.3, 0.7, 1.0));
                cuboid(&mut gizmos, &o.bounds(), Color::srgba(0.3, 0.7, 1.0, 0.6));
            }
        }
    }
    for g in &ed.ghost {
        cuboid(&mut gizmos, &g.bounds(), Color::srgba(1.0, 1.0, 1.0, 0.8));
    }
}

/// While editing: the first-person arms and gun aren't drawn, nor the game's HUD (its UI
/// draws on a camera that's off).
#[allow(clippy::type_complexity)]
fn hide_game_views(
    mut commands: Commands,
    ed: Res<Editor>,
    mut was: Local<bool>,
    mut overlays: Query<&mut Camera, Or<(With<crate::viewmodel::ViewmodelCamera>, With<crate::gun::GunCamera>)>>,
    player: Query<Entity, With<PlayerCamera>>,
    mut ui_cam: Query<&mut Camera, (Without<crate::viewmodel::ViewmodelCamera>, Without<crate::gun::GunCamera>, Without<PlayerCamera>)>,
) {
    if ed.active {
        for mut c in &mut overlays {
            if c.is_active {
                c.is_active = false;
            }
        }
    }
    if ed.active == *was {
        return;
    }
    *was = ed.active;
    let (Some(sink), Some(ui)) = (ed.ui_sink, ed.ui_camera) else { return };
    if let Ok(mut c) = ui_cam.get_mut(ui) {
        c.is_active = ed.active;
    }
    for p in &player {
        if ed.active {
            commands.entity(p).remove::<IsDefaultUiCamera>();
            commands.entity(sink).insert(IsDefaultUiCamera);
        } else {
            commands.entity(sink).remove::<IsDefaultUiCamera>();
            commands.entity(p).insert(IsDefaultUiCamera);
        }
    }
    if !ed.active {
        // The arms come back (the gun's camera follows the gun itself).
        for mut c in &mut overlays {
            c.is_active = true;
        }
    }
}


#[cfg(test)]
mod tests {
    use super::catalog::{find, place, CATALOG};
    use super::*;

    #[test]
    fn rays_meet_boxes_from_outside_only() {
        let b = Aabb::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        let (t, n) = ray_box(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z, &b).unwrap();
        assert!((t - 4.0).abs() < 1e-5);
        assert_eq!(n, Vec3::Z);
        assert!(ray_box(Vec3::ZERO, Vec3::NEG_Z, &b).is_none(), "from inside");
        assert!(ray_box(Vec3::new(0.0, 0.0, 5.0), Vec3::Z, &b).is_none(), "behind");
        assert!(ray_box(Vec3::new(3.0, 0.0, 5.0), Vec3::NEG_Z, &b).is_none(), "beside");
    }

    #[test]
    fn the_cursor_finds_the_floor_and_the_piece_on_it() {
        let mut doc = LevelDoc::starter("T");
        // Looking down at the floor from inside the room.
        let (p, n) = surface(&doc, Vec3::new(0.0, 3.0, -8.0), Vec3::NEG_Y);
        assert!((p - Vec3::new(0.0, 0.0, -8.0)).length() < 1e-4, "{p}");
        assert_eq!(n, Vec3::Y);
        // At the back wall.
        let (p, n) = surface(&doc, Vec3::new(0.0, 2.0, -8.0), Vec3::NEG_Z);
        assert!((p.z + 16.0).abs() < 1e-4 && n == Vec3::Z, "{p} {n}");
        // A button there is picked over the floor round it.
        let objs = place(&CATALOG[find("Floor button")], &doc, Vec3::new(0.0, 0.0, -8.0), Vec3::Y, Vec3::NEG_Z, 0.25);
        doc.objects.extend(objs);
        let hit = pick(&doc, Vec3::new(0.0, 3.0, -8.0), Vec3::NEG_Y).unwrap();
        assert!(matches!(doc.objects[hit].kind, Kind::FloorButton { .. }));
    }

    #[test]
    fn pieces_land_on_what_they_are_dropped_on() {
        let mut doc = LevelDoc::starter("T");
        let look = Vec3::NEG_Z;
        // A block rests on the floor, its corner on the grid.
        let o = &place(&CATALOG[find("White panels")], &doc, Vec3::new(0.1, 0.0, -5.1), Vec3::Y, look, 0.5)[0];
        let b = o.bounds();
        assert!(b.min.y.abs() < 1e-5 && (b.min.x / 0.5).fract().abs() < 1e-5, "{b:?}");
        // A laser on a wall faces out of it.
        let o = &place(&CATALOG[find("Laser emitter")], &doc, Vec3::new(-6.0, 2.0, -8.0), Vec3::X, look, 0.25)[0];
        assert!((o.dir() - Vec3::X).length() < 1e-4, "{}", o.dir());
        // A cube sits on the floor.
        let o = &place(&CATALOG[find("Storage cube")], &doc, Vec3::new(1.0, 0.0, -4.0), Vec3::Y, look, 0.25)[0];
        assert!((o.at.y - 0.45).abs() < 1e-5);
        // A button gets a signal of its own, and a door after it listens to it.
        doc.objects.extend(place(&CATALOG[find("Floor button")], &doc, Vec3::new(0.0, 0.0, -4.0), Vec3::Y, look, 0.25));
        doc.objects.extend(place(&CATALOG[find("Floor button")], &doc, Vec3::new(3.0, 0.0, -4.0), Vec3::Y, look, 0.25));
        assert_eq!(doc.objects.last().unwrap().kind.drives(), Some(1));
        let door = &place(&CATALOG[find("Door")], &doc, Vec3::new(0.0, 0.0, -15.0), Vec3::Y, look, 0.25)[0];
        assert_eq!(door.kind.inputs().unwrap().all, vec![1]);
        assert!(door.bounds().min.y.abs() < 1e-5);
        // A room is its six sides and a lamp.
        assert_eq!(place(&CATALOG[find("Room")], &doc, Vec3::new(30.0, 0.0, 0.0), Vec3::Y, look, 0.25).len(), 7);
    }

    #[test]
    fn every_catalog_entry_builds() {
        let doc = LevelDoc::starter("T");
        let mut d = doc.clone();
        for e in CATALOG {
            d.objects.extend(place(e, &doc, Vec3::new(0.0, 0.0, -8.0), Vec3::Y, Vec3::NEG_Z, 0.25));
        }
        let text = d.to_ron();
        assert_eq!(LevelDoc::from_ron(&text).unwrap(), d);
        let l = d.build();
        assert!(!l.items.is_empty());
    }
}
