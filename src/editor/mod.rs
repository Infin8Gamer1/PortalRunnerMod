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
pub mod library;
mod thumbs;
pub mod ui;

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

use aperture::doc::{Kind, LevelDoc, Object, Turns};
use faith_move::Aabb;

use library::Library;

use crate::{menu, Game, PlayerCamera};

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((EguiPlugin::default(), TransformGizmoPlugin))
            .insert_resource(EguiGlobalSettings { auto_create_primary_context: false, ..default() })
            .insert_resource(TransformGizmoSettings { snap_translate: Some(0.25), snap_rotate: Some(15f32.to_radians()), snap_scale: Some(0.25), ..default() })
            .add_plugins(bevy::app::HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate))
            .init_gizmo_group::<OnTop>()
            .init_gizmo_group::<EditorLines>()
            .init_resource::<Editor>()
            .init_resource::<Library>()
            .init_resource::<thumbs::Thumbs>()
            .configure_sets(PostUpdate, TransformGizmoSystems.run_if(editing))
            .add_systems(Startup, (setup, thumbs::setup))
            .add_systems(Update, (forward_gizmos, library::load, thumbs::run, unlit, me_bounds.run_if(editing)))
            .add_systems(EguiPrimaryContextPass, ui::draw.run_if(editing));
    }
}

/// The editor's systems, run after the game's each frame (the camera is the editor's).
pub fn systems() -> impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, ()> {
    (toggle, fly, interact, rebuild, overlays, hide_game_views, gizmo_cameras, apply_view).chain()
}

/// The editor's lines (on a layer of their own, so they show without the level's meshes).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct EditorLines;

/// Lines drawn over everything (the end handle).
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct OnTop;

/// The render layer the editor's lines are on.
const LINES_LAYER: usize = 21;

/// What the view shows (the View menu).
pub struct View {
    /// The level as the game draws it.
    pub meshes: bool,
    /// Outlines of what Faith and cubes collide with (the built level's boxes).
    pub collision: bool,
    pub grid: bool,
    /// What the game doesn't draw: the start, triggers, cuts, wires, lines along things.
    pub markers: bool,
    /// Every signal's links (not just the selection's).
    pub all_links: bool,
    /// Everything at its own colour, unlit.
    pub unlit: bool,
    pub fog: bool,
    /// Faith, standing at the start.
    pub body: bool,
}

impl Default for View {
    fn default() -> Self {
        View { meshes: true, collision: false, grid: true, markers: true, all_links: false, unlit: false, fog: true, body: true }
    }
}

/// Something from the content browser: a catalog entry, or a prop from the library.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    Entry(usize),
    Prop(usize),
}

/// Dragging the end handle: along an axis (0-2), or across the view (3).
struct EndDrag {
    axis: usize,
    /// Where the end was, and where along the axis (or the point on the view's plane) the
    /// drag took hold.
    start: Vec3,
    grab: Vec3,
    before: LevelDoc,
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
    /// What the view shows.
    pub view: View,
    /// The player camera's layers before the editor changed them.
    saved_layers: Option<RenderLayers>,
    /// What's being dragged out of the content browser.
    pub drop: Option<Pick>,
    /// What was clicked in the content browser: the next click in the view places it.
    pub armed: Option<Pick>,
    /// The end handle's axis under the cursor, and its drag.
    end_hover: Option<usize>,
    end_drag: Option<EndDrag>,
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
    /// Where each content browser tile is (for the capture scripts to find).
    pub tiles: Vec<(Pick, egui::Rect)>,
    /// The content browser's tab (none: all but the props), and the props' game and folder.
    pub tab: Option<catalog::Category>,
    pub prop_game: usize,
    pub prop_folder: Option<String>,
    /// The proxy the gizmo moves.
    handle: Option<Entity>,
    ui_camera: Option<Entity>,
    ui_sink: Option<Entity>,
    /// Goes up with each change to the document (for what's worked out from it to know).
    pub version: u64,
    /// What the material picker's open for.
    pub picking: Option<MatTarget>,
    /// The sound (object) the sound browser's open for.
    pub sound_picking: Option<usize>,
}

/// What the material picker gives a material to: an object, and one of its faces (none: all
/// of a brush's drawn faces, or a displacement's or overlay's own).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MatTarget {
    pub obj: usize,
    pub face: Option<usize>,
}

/// The undo history keeps at most this many objects, all its steps together.
const UNDO_OBJECTS: usize = 300_000;

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
            view: View::default(),
            saved_layers: None,
            drop: None,
            armed: None,
            end_hover: None,
            end_drag: None,
            ghost: Vec::new(),
            pick_target: false,
            before_drag: None,
            before_edit: None,
            panels: Vec::new(),
            tiles: Vec::new(),
            tab: None,
            prop_game: 0,
            prop_folder: None,
            handle: None,
            ui_camera: None,
            ui_sink: None,
            version: 0,
            picking: None,
            sound_picking: None,
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
        // (At most 200 steps, fewer of a big level: each is the whole level as it was.)
        let keep = (UNDO_OBJECTS / self.doc.objects.len().max(1)).clamp(10, 200);
        if self.undo.len() > keep {
            let extra = self.undo.len() - keep;
            self.undo.drain(..extra);
        }
        self.version += 1;
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
        self.version += 1;
        if self.selected.is_some_and(|i| i >= self.doc.objects.len()) {
            self.selected = None;
        }
    }

    /// Start on a new document.
    pub fn open(&mut self, doc: LevelDoc, path: Option<PathBuf>) {
        self.save_name = doc.name.clone();
        self.doc = doc;
        self.version += 1;
        self.path = path;
        self.selected = None;
        self.undo.clear();
        self.redo.clear();
        self.unsaved = false;
        self.rebuild = true;
        self.started = true;
        // Looking out from the start (a Portal map's: its player start).
        let start = self.doc.objects.iter().find(|o| o.kind == Kind::Spawn).or_else(|| self.doc.objects.iter().find(|o| matches!(&o.kind, Kind::Entity { class, .. } if class == "info_player_start")));
        if let Some(s) = start {
            let yaw = if s.kind == Kind::Spawn { s.yaw.to_radians() } else { (-s.dir().x).atan2(-s.dir().z) };
            self.cam = FlyCam { pos: s.at + Vec3::new(0.0, 2.5, 0.0), yaw, pitch: -0.3, speed: self.cam.speed };
        }
    }

    /// Place something where the middle of the view meets the level.
    pub fn place_in_view(&mut self, lib: &Library, pick: Pick) {
        let rot = Quat::from_euler(EulerRot::YXZ, self.cam.yaw, self.cam.pitch, 0.0);
        let look = rot * Vec3::NEG_Z;
        let (p, n) = surface(&self.doc, self.cam.pos, look);
        self.add(lib, pick, p, n, look);
    }

    /// Place something at `p` on a surface facing `n`, seen looking along `look`.
    pub fn add(&mut self, lib: &Library, pick: Pick, p: Vec3, n: Vec3, look: Vec3) {
        let objs = place(&self.doc, lib, pick, p, n, look, self.grid);
        let count = objs.len();
        if count == 0 {
            return;
        }
        self.change(|doc| doc.objects.extend(objs));
        self.selected = Some(self.doc.objects.len() - 1);
        let name = match pick {
            Pick::Entry(e) => catalog::CATALOG[e].name.to_string(),
            Pick::Prop(i) => lib.props[i].name.clone(),
        };
        self.status = format!("Placed {name}{}", if count > 1 { format!(" ({count} objects)") } else { String::new() });
    }

    /// Place a catalog entry (the capture scripts).
    pub fn add_entry(&mut self, entry: usize, p: Vec3, n: Vec3, look: Vec3) {
        self.add(&Library::default(), Pick::Entry(entry), p, n, look);
    }

    /// Where the end handle's arrow along `axis` (0-2; 3: its ring) is on the screen, if the
    /// selection has one (the capture scripts).
    pub fn end_handle_on_screen(&self, window: &Window, axis: usize) -> Option<Vec2> {
        let e = self.selected.and_then(|i| self.doc.objects.get(i)).and_then(end_of)?;
        let s = handle_size(&self.cam, e);
        let p = if axis < 3 { e + [Vec3::X, Vec3::Y, Vec3::Z][axis] * s * 0.7 } else { e };
        to_screen(&self.cam, window, p)
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
                    game.source_names.push(self.doc.map.clone());
                    game.groups.push(crate::LevelGroup::Testing);
                    self.files.push((i, path));
                }
            }
            Err(e) => self.status = format!("Couldn't save {}: {e}", path.display()),
        }
    }

    /// Edit level `i` of the game's list: its file, or a copy of a built chamber. One of
    /// Portal's maps as imported: the game's own copy of it (a level file), if there is one.
    /// Whether what's open is that level (or its copy).
    pub fn open_level(&mut self, game: &Game, i: usize) -> bool {
        let i = if game.groups.get(i) == Some(&crate::LevelGroup::PortalImported) {
            match (0..game.levels.len()).find(|&j| game.groups[j] == crate::LevelGroup::PortalNative && game.source_names[j] == game.source_names[i]) {
                Some(j) => j,
                None => {
                    self.status = "No copy of this map to edit in levels/portal (cargo run --release -p portal_extract -- --port-maps makes it). Starting a new level".into();
                    self.open(LevelDoc::starter("Untitled"), None);
                    return false;
                }
            }
        } else {
            i
        };
        if let Some(path) = self.files.iter().find(|(j, _)| *j == i).map(|(_, p)| p.clone()) {
            let path = &path;
            let started = std::time::Instant::now();
            match read_doc(path) {
                Ok(d) => {
                    let n = d.objects.len();
                    self.open(d, Some(path.clone()));
                    self.status = format!("Opened {} ({n} objects, {:.1} s)", path.display(), started.elapsed().as_secs_f32());
                }
                Err(e) => {
                    self.status = e;
                    return false;
                }
            }
        } else {
            let (mut d, skipped) = LevelDoc::from_level(&(game.levels[i].1)());
            d.name = format!("{} (copy)", d.name);
            self.open(d, None);
            self.unsaved = true;
            self.status = if skipped > 0 { format!("Imported {} ({skipped} things the editor can't show were left out)", game.levels[i].0) } else { format!("Imported {}", game.levels[i].0) };
        }
        true
    }

    fn enter(&mut self, game: &mut Game, menu: &mut menu::Menu) {
        if self.slot.is_none() {
            let shared = self.shared.clone();
            self.slot = Some(game.levels.len());
            game.levels.push(("Editor".into(), Arc::new(move || shared.read().map(|d| d.build_with(&crate::source_map::scenes())).unwrap_or_else(|_| LevelDoc::starter("Untitled").build()))));
            game.source_names.push(None);
            game.groups.push(crate::LevelGroup::Editor);
        }
        // The level being played, unless that's the editor's own or there's unsaved work.
        if !self.started || (Some(game.level_index) != self.slot && !self.unsaved) {
            let i = game.level_index;
            // Start where she's standing (in the level that's open).
            if self.open_level(game, i) {
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

    /// Reload the level from the document (after changing it directly).
    pub fn rebuild_now(&mut self) {
        self.rebuild = true;
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
        Ok(d) => d.build_with(&crate::source_map::scenes()),
        Err(e) => {
            error!("{e}");
            LevelDoc::starter("Broken level file").build()
        }
    })
}

/// A level file's name, without reading all of it (the game's copies of Portal's maps are
/// big): its `name:` line.
fn doc_name(path: &Path) -> String {
    use std::io::BufRead;
    let named = std::fs::File::open(path).ok().and_then(|f| {
        std::io::BufReader::new(f).lines().take(8).map_while(Result::ok).find_map(|l| {
            let rest = l.trim().strip_prefix("name:")?.trim().trim_end_matches(',');
            let quoted = rest.strip_prefix('"')?.strip_suffix('"')?;
            Some(quoted.replace("\\\"", "\"").replace("\\\\", "\\"))
        })
    });
    named.unwrap_or_else(|| read_doc(path).map(|d| d.name).unwrap_or_else(|_| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()))
}

/// The game's copies of Portal's maps and the level files, after the game's chambers and
/// Portal's maps (startup).
pub fn add_levels(mut game: ResMut<Game>, mut ed: ResMut<Editor>) {
    let before = game.levels.len();
    for (path, map) in crate::source_map::native_files() {
        let name = doc_name(&path);
        ed.files.push((game.levels.len(), path.clone()));
        game.levels.push((name, file_level(path)));
        game.source_names.push(Some(map));
        game.groups.push(crate::LevelGroup::PortalNative);
    }
    for path in level_files() {
        let name = doc_name(&path);
        info!("Level {}: {} ({})", game.levels.len(), name, path.display());
        ed.files.push((game.levels.len(), path.clone()));
        game.levels.push((name, file_level(path)));
        game.source_names.push(None);
        game.groups.push(crate::LevelGroup::Testing);
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
fn setup(mut commands: Commands, mut ed: ResMut<Editor>, mut store: ResMut<GizmoConfigStore>) {
    let (on_top, _) = store.config_mut::<OnTop>();
    on_top.depth_bias = -1.0;
    on_top.line.width = 3.0;
    on_top.render_layers = RenderLayers::layer(LINES_LAYER);
    let (lines, _) = store.config_mut::<EditorLines>();
    lines.render_layers = RenderLayers::layer(LINES_LAYER);
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
    player: Query<(Entity, Ref<Msaa>, Option<Ref<CameraMainTextureUsages>>, Option<&Exposure>, Has<TransformGizmoCamera>, &Projection), With<PlayerCamera>>,
    mut cams: Query<(Entity, &RenderLayers, &mut Camera, &mut Projection, Has<crate::graphics::OverlayCamera>), (With<Camera3d>, Without<PlayerCamera>)>,
) {
    let Ok((player, msaa, usages, exposure, marked, proj)) = player.single() else { return };
    if !marked {
        commands.entity(player).insert(TransformGizmoCamera);
    }
    for (e, layers, mut cam, mut gizmo_proj, tagged) in &mut cams {
        if *layers != RenderLayers::layer(15) {
            continue;
        }
        // The same view as the player's camera, or the gizmo drifts off what it's on.
        if ed.active {
            *gizmo_proj = proj.clone();
        }
        if !tagged || msaa.is_changed() || usages.as_ref().is_some_and(|u| u.is_changed()) {
            let mut ec = commands.entity(e);
            ec.insert((crate::graphics::OverlayCamera, *msaa, Hdr, Tonemapping::TonyMcMapface, usages.as_deref().cloned().unwrap_or_default()));
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

/// Whether the pointer is over a panel, or a menu or popup open over the view. (Not egui's own
/// `is_pointer_over_egui`: the panels are on its background layer, which it doesn't count.)
fn over_ui(ed: &Editor, window: &Window, contexts: &mut EguiContexts) -> bool {
    let Some(c) = window.cursor_position() else { return false };
    let pos = egui::pos2(c.x, c.y);
    // (Tooltips aside: a tile being dragged is drawn on their layer.)
    ed.over_panels(window) || contexts.ctx_mut().is_ok_and(|ctx| ctx.layer_id_at(pos).is_some_and(|l| !matches!(l.order, egui::Order::Background | egui::Order::Tooltip)))
}

/// F4: into the editor, or out to play what's being edited; F5: play.
fn toggle(mut ed: ResMut<Editor>, keys: Res<ButtonInput<KeyCode>>, mut game: ResMut<Game>, mut menu: ResMut<menu::Menu>, mut contexts: EguiContexts) {
    if std::mem::take(&mut ed.enter_next) {
        ed.enter(&mut game, &mut menu);
        return;
    }
    // From the menu: the editor, on this level or a new one.
    if let Some(new) = menu.edit.take() {
        let unsaved = ed.started && ed.unsaved;
        ed.enter(&mut game, &mut menu);
        if new {
            if unsaved {
                ed.status = "There are unsaved changes: save them, then File > New room".into();
            } else {
                ed.open(LevelDoc::starter("Untitled"), None);
                ed.status = "A new level: drag things in from Content, then File > Save".into();
            }
        }
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
    let over = windows.single().is_ok_and(|w| over_ui(&ed, w, &mut contexts));
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

/// Whether it's a volume that's only there to be walked into (a trigger: none of its faces
/// drawn), not something to see.
fn volume(kind: &Kind) -> bool {
    match kind {
        Kind::Entity { brushes, .. } => !brushes.is_empty() && brushes.iter().all(|b| b.faces.iter().all(|f| aperture::brush::tool_material(&f.mat))),
        _ => false,
    }
}

/// The object a ray meets first.
fn pick(doc: &LevelDoc, o: Vec3, d: Vec3) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for (i, obj) in doc.objects.iter().enumerate() {
        if let Some((t, _)) = ray_box(o, d, &obj.bounds()) {
            // Pieces win over the walls round them by a little; what's seen over a volume
            // (a trigger) that's round it.
            let t = if volume(&obj.kind) { t + 1000.0 } else if obj.kind.is_box() || matches!(obj.kind, Kind::Brush(_)) { t + 0.05 } else { t };
            if best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t, i));
            }
        }
    }
    best.map(|(_, i)| i)
}

/// What something makes where it's dropped; a ladder's foot goes down to the floor.
fn place(doc: &LevelDoc, lib: &Library, pick: Pick, p: Vec3, n: Vec3, look: Vec3, grid: f32) -> Vec<Object> {
    let entry = match pick {
        Pick::Entry(e) => e,
        Pick::Prop(i) if i < lib.props.len() => return vec![lib.object(i, catalog::snap_along(p, n, grid), n, look)],
        Pick::Prop(_) => return Vec::new(),
    };
    let mut objs = catalog::place(&catalog::CATALOG[entry], doc, p, n, look, grid);
    for o in &mut objs {
        if let Kind::Ladder { .. } = o.kind {
            let (floor, up) = surface(doc, o.at + o.dir() * 0.3, Vec3::NEG_Y);
            if up.y > 0.5 {
                o.at.y = floor.y;
            }
        }
    }
    objs
}

/// Where a ray meets the level's surfaces (its blocks, else the ground), and the normal there.
fn surface(doc: &LevelDoc, o: Vec3, d: Vec3) -> (Vec3, Vec3) {
    let mut best: Option<(f32, Vec3)> = None;
    for obj in &doc.objects {
        if !matches!(&obj.kind, Kind::Block { .. } | Kind::Platform { .. } | Kind::MeProp { bounds: Some(_), solid: true, .. } | Kind::Model { bounds: Some(_), solid: true, .. } | Kind::Brush(_)) {
            continue;
        }
        let Some((t, n)) = ray_box(o, d, &obj.bounds()) else { continue };
        if best.is_some_and(|(bt, _)| t >= bt) {
            continue;
        }
        // (An angled brush: its box. Whether a brush is solid only for those the ray meets
        // first: working it out takes a while, and a map has thousands.)
        if let Kind::Brush(b) = &obj.kind {
            if !matches!(b.collision(), aperture::brush::Collision::Solid(_)) {
                continue;
            }
        }
        best = Some((t, n));
    }
    if let Some((t, n)) = best {
        return (o + d * t, n);
    }
    if d.y < -1e-3 && o.y > 0.0 {
        return (o + d * (-o.y / d.y), Vec3::Y);
    }
    (o + d * 8.0, Vec3::Y)
}

/// The other end of something with two: a zipline's, a swing pole's, a beam's, where a faith
/// plate lands, where a platform travels to.
fn end_of(o: &Object) -> Option<Vec3> {
    match &o.kind {
        Kind::ZipLine { to } | Kind::SwingPole { to } | Kind::Beam { to, .. } => Some(o.at + *to),
        Kind::FaithPlate { target, .. } => Some(*target),
        Kind::Platform { travel, .. } => Some(o.at + *travel),
        _ => None,
    }
}

fn set_end(o: &mut Object, e: Vec3) {
    let at = o.at;
    match &mut o.kind {
        Kind::ZipLine { to } | Kind::SwingPole { to } | Kind::Beam { to, .. } => *to = e - at,
        Kind::FaithPlate { target, .. } => *target = e,
        Kind::Platform { travel, .. } => *travel = e - at,
        _ => {}
    }
}

/// How long the end handle's arrows are at `p` (they keep their size on screen).
fn handle_size(cam: &FlyCam, p: Vec3) -> f32 {
    (p - cam.pos).length() * 0.12
}

/// Where a point is on the screen (logical pixels), if it's in front of the camera.
fn to_screen(cam: &FlyCam, window: &Window, p: Vec3) -> Option<Vec2> {
    let rot = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    let v = rot.inverse() * (p - cam.pos);
    if v.z > -0.01 {
        return None;
    }
    let (w, h) = (window.width(), window.height());
    let t = (FOV * 0.5).tan();
    let ndc = Vec2::new(v.x / (-v.z * t * w / h), v.y / (-v.z * t));
    Some(Vec2::new((ndc.x + 1.0) * 0.5 * w, (1.0 - ndc.y) * 0.5 * h))
}

/// The end handle's part under the cursor: an arrow (0-2), or the ring in its middle (3).
fn end_hover(cam: &FlyCam, window: &Window, e: Vec3, cursor: Vec2) -> Option<usize> {
    let s = handle_size(cam, e);
    let centre = to_screen(cam, window, e)?;
    if centre.distance(cursor) < 10.0 {
        return Some(3);
    }
    let mut best = None;
    for (k, axis) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().enumerate() {
        let (Some(a), Some(b)) = (to_screen(cam, window, e + axis * s * 0.2), to_screen(cam, window, e + axis * s)) else { continue };
        let ab = b - a;
        let t = ((cursor - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let d = cursor.distance(a + ab * t);
        if d < 12.0 && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, k));
        }
    }
    best.map(|(_, k)| k)
}

/// The plane a drag of the end handle moves on: along an axis, facing the camera as much as
/// it can; across the view, facing it.
fn end_plane(cam: &FlyCam, axis: usize) -> Vec3 {
    let fwd = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0) * Vec3::NEG_Z;
    match axis {
        3 => fwd,
        k => {
            let a = [Vec3::X, Vec3::Y, Vec3::Z][k];
            (fwd - a * fwd.dot(a)).normalize_or(Vec3::Y)
        }
    }
}

/// Where a ray meets the drag's plane through `at`.
fn end_grab(cam: &FlyCam, axis: usize, at: Vec3, o: Vec3, d: Vec3) -> Option<Vec3> {
    let n = end_plane(cam, axis);
    let dn = d.dot(n);
    if dn.abs() < 1e-5 {
        return None;
    }
    let t = (at - o).dot(n) / dn;
    (t > 0.0).then(|| o + d * t)
}

/// Where the end is now, dragged from `start` (taken hold of at `grab`), snapped to the grid.
fn end_moved(cam: &FlyCam, axis: usize, start: Vec3, grab: Vec3, o: Vec3, d: Vec3, grid: f32) -> Option<Vec3> {
    let p = end_grab(cam, axis, start, o, d)?;
    let snap = |v: f32| if grid > 0.0 { (v / grid).round() * grid } else { v };
    Some(match axis {
        3 => {
            let m = p - grab;
            start + Vec3::new(snap(m.x), snap(m.y), snap(m.z))
        }
        k => {
            let a = [Vec3::X, Vec3::Y, Vec3::Z][k];
            start + a * snap((p - grab).dot(a))
        }
    })
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
    lib: Res<Library>,
) {
    if !ed.active {
        if let Some(h) = ed.handle {
            commands.entity(h).remove::<TransformGizmoFocus>();
        }
        return;
    }
    let Ok(window) = windows.single() else { return };
    let over = over_ui(&ed, window, &mut contexts);
    let typing = typing(&mut contexts);
    let ray = cursor_ray(window, &ed.cam);
    let look = Quat::from_euler(EulerRot::YXZ, ed.cam.yaw, ed.cam.pitch, 0.0) * Vec3::NEG_Z;
    settings.snap_translate = (ed.grid > 0.0).then_some(ed.grid);
    settings.snap_scale = (ed.grid > 0.0).then_some(ed.grid);

    // ---- dragging something in from the browser
    // (egui holds the pointer while a tile's dragged: what matters is whether it's over a panel.)
    ed.ghost.clear();
    let mut placed = false;
    if let Some(pick) = ed.drop {
        if let Some((o, d)) = ray.filter(|_| !ed.over_panels(window)) {
            let (p, n) = surface(&ed.doc, o, d);
            if mouse.pressed(MouseButton::Left) {
                ed.ghost = place(&ed.doc, &lib, pick, p, n, look, ed.grid);
            } else {
                ed.add(&lib, pick, p, n, look);
            }
        }
        if !mouse.pressed(MouseButton::Left) {
            ed.drop = None;
        }
    } else if let Some(pick) = ed.armed {
        // A tile clicked: it follows the cursor, and a click in the view puts it there.
        if let Some((o, d)) = ray.filter(|_| !over) {
            let (p, n) = surface(&ed.doc, o, d);
            if mouse.just_pressed(MouseButton::Left) && gizmo.hovered_axis.is_none() {
                ed.add(&lib, pick, p, n, look);
                placed = true;
                // Shift: place more of them.
                if !(keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)) {
                    ed.armed = None;
                }
            } else {
                ed.ghost = place(&ed.doc, &lib, pick, p, n, look, ed.grid);
            }
        }
    }

    // ---- the end handle (a zipline's other end, a faith plate's landing...)
    let end = ed.selected.and_then(|i| ed.doc.objects.get(i)).and_then(end_of);
    if let Some(drag) = &ed.end_drag {
        let (axis, start, grab) = (drag.axis, drag.start, drag.grab);
        if mouse.pressed(MouseButton::Left) {
            if let (Some(i), Some((o, d))) = (ed.selected, ray) {
                if let Some(moved) = end_moved(&ed.cam, axis, start, grab, o, d, ed.grid) {
                    if let Some(obj) = ed.doc.objects.get_mut(i) {
                        set_end(obj, moved);
                    }
                }
            }
        } else if let Some(drag) = ed.end_drag.take() {
            ed.commit(drag.before);
        }
    } else {
        ed.end_hover = None;
        if let (Some(e), Some(c)) = (end, window.cursor_position()) {
            if !over && !ed.looking && gizmo.hovered_axis.is_none() && !gizmo.active && ed.drop.is_none() && ed.armed.is_none() {
                ed.end_hover = end_hover(&ed.cam, window, e, c);
            }
        }
        if let (Some(axis), Some(e), Some((o, d))) = (ed.end_hover, end, ray) {
            if mouse.just_pressed(MouseButton::Left) {
                if let Some(grab) = end_grab(&ed.cam, axis, e, o, d) {
                    ed.end_drag = Some(EndDrag { axis, start: e, grab, before: ed.doc.clone() });
                }
            }
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
            if i < ed.doc.objects.len() {
                let obj = &mut ed.doc.objects[i];
                match mode {
                    TransformGizmoMode::Translate => obj.at = tf.translation,
                    TransformGizmoMode::Scale if obj.kind.is_box() => {
                        // (Sized about its middle, as the gizmo scales.)
                        obj.size = tf.scale.abs().max(Vec3::splat(0.05));
                    }
                    TransformGizmoMode::Scale => {
                        // (A brush, or a brush entity's brushes: scaled from as they were when the
                        // drag began, about the object's point.)
                        let k = tf.scale.abs().max(Vec3::splat(0.01));
                        let was = ed.before_drag.as_ref().and_then(|d| d.objects.get(i)).map(|o| o.kind.clone());
                        let obj = &mut ed.doc.objects[i];
                        match (&mut obj.kind, was) {
                            (Kind::MeProp { scale, .. } | Kind::Model { scale, .. }, _) => *scale = k,
                            (Kind::Brush(b), Some(Kind::Brush(orig))) => *b = orig.scaled(k),
                            (Kind::Entity { brushes, .. }, Some(Kind::Entity { brushes: orig, .. })) if brushes.len() == orig.len() => {
                                for (b, o) in brushes.iter_mut().zip(&orig) {
                                    *b = o.scaled(k);
                                }
                            }
                            _ => {}
                        }
                    }
                    TransformGizmoMode::Rotate => match obj.kind.turns() {
                        Some(Turns::Free) => obj.set_rotation(tf.rotation),
                        Some(turns) => {
                            let (yaw, pitch, _) = tf.rotation.to_euler(EulerRot::YXZ);
                            obj.yaw = yaw.to_degrees();
                            if turns == Turns::Aim {
                                obj.pitch = pitch.to_degrees();
                            }
                        }
                        None => {}
                    },
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
                    // (A box is sized along the world's axes, whichever way it faces: scaled
                    // unturned, or its handles would size it along the wrong ones. So is a brush
                    // entity's brushes: they don't turn with it.)
                    let world_sized = obj.kind.is_box() || matches!(&obj.kind, Kind::Entity { brushes, .. } if !brushes.is_empty());
                    let sized = world_sized && settings.mode == TransformGizmoMode::Scale;
                    tf.rotation = if obj.kind.turns().is_some() && !sized { obj.rotation() } else { Quat::IDENTITY };
                    tf.scale = match &obj.kind {
                        _ if obj.kind.is_box() => obj.size,
                        Kind::MeProp { scale, .. } | Kind::Model { scale, .. } => *scale,
                        _ => Vec3::ONE,
                    };
                }
                commands.entity(h).insert(TransformGizmoFocus);
            }
            None => {
                commands.entity(h).remove::<TransformGizmoFocus>();
            }
        }
    }

    // ---- clicking in the view
    let on_end = ed.end_hover.is_some() || ed.end_drag.is_some();
    if mouse.just_pressed(MouseButton::Left) && !over && ed.drop.is_none() && ed.armed.is_none() && !placed && !on_end && gizmo.hovered_axis.is_none() && !gizmo.active {
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

    // ---- keys (not while something's being dragged: an undo or a delete then would leave
    // the drag holding the wrong document)
    if typing || ed.looking || gizmo.active || ed.end_drag.is_some() {
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
        if ed.armed.is_some() {
            ed.armed = None;
        } else {
            ed.selected = None;
        }
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
fn overlays(ed: Res<Editor>, game: Res<Game>, mut gizmos: Gizmos<EditorLines>, mut on_top: Gizmos<OnTop>) {
    if !ed.active {
        return;
    }
    let view = &ed.view;
    // The end handle: arrows to drag the other end along, a ring to drag it about.
    if let Some(e) = ed.selected.and_then(|i| ed.doc.objects.get(i)).and_then(end_of) {
        use bevy::gizmos::transform_gizmo::{COLOR_X, COLOR_Y, COLOR_Z};
        let s = handle_size(&ed.cam, e);
        let active = ed.end_drag.as_ref().map(|d| d.axis).or(ed.end_hover);
        for (k, (axis, c)) in [(Vec3::X, COLOR_X), (Vec3::Y, COLOR_Y), (Vec3::Z, COLOR_Z)].into_iter().enumerate() {
            let c = if active == Some(k) { Color::WHITE } else { c };
            on_top.arrow(e + axis * s * 0.2, e + axis * s, c);
        }
        let rot = Quat::from_euler(EulerRot::YXZ, ed.cam.yaw, ed.cam.pitch, 0.0);
        on_top.circle(Isometry3d::new(e, rot), s * 0.1, if active == Some(3) { Color::WHITE } else { Color::srgb(1.0, 0.85, 0.2) });
    }
    let cuboid = |g: &mut Gizmos<EditorLines>, b: &Aabb, c: Color| g.cube(Transform::from_translation(b.center()).with_scale(b.size()), c);
    let arrow = |g: &mut Gizmos<EditorLines>, from: Vec3, dir: Vec3, len: f32, c: Color| {
        g.arrow(from, from + dir * len, c);
    };
    // A faint grid on the ground round the camera.
    let at = Vec3::new((ed.cam.pos.x / 1.0).round(), 0.002, (ed.cam.pos.z / 1.0).round());
    if view.grid {
        gizmos.grid(Isometry3d::new(at, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), UVec2::splat(40), Vec2::splat(1.0), Color::srgba(1.0, 1.0, 1.0, 0.12));
    }
    // What Faith and cubes collide with, coloured by what it is: blue takes portals, orange
    // doesn't, cyan is glass, yellow grating, magenta clip.
    if view.collision {
        for s in &game.sim.statics {
            let c = match s.mat {
                aperture::Mat::Glass | aperture::Mat::Frosted => Color::srgb(0.3, 0.9, 1.0),
                aperture::Mat::Grate => Color::srgb(1.0, 0.9, 0.2),
                aperture::Mat::Clip => Color::srgb(1.0, 0.3, 0.9),
                m if m.portalable() => Color::srgb(0.35, 0.6, 1.0),
                _ => Color::srgb(1.0, 0.55, 0.2),
            };
            cuboid(&mut gizmos, &s.b, c);
        }
    }
    // Every signal's links: from each source to each piece listening to it.
    if view.all_links {
        for src in ed.doc.objects.iter() {
            let Some(sig) = src.kind.drives() else { continue };
            for o in &ed.doc.objects {
                if o.kind.inputs().is_some_and(|i| i.all.contains(&sig)) {
                    gizmos.line(src.bounds().center(), o.bounds().center(), Color::srgba(0.3, 0.7, 1.0, 0.7));
                }
            }
        }
    }
    for (i, o) in ed.doc.objects.iter().enumerate().filter(|_| view.markers) {
        // (Only what has a box drawn works it out: a Portal map's thousands of brushes don't.)
        let b = if matches!(o.kind, Kind::Brush(_) | Kind::Displacement(_) | Kind::Overlay { .. } | Kind::Entity { .. } | Kind::Block { .. } | Kind::Decor { .. }) { Aabb::from_center_size(o.at, Vec3::ZERO) } else { o.bounds() };
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
            Kind::ZipLine { to } => gizmos.line(o.at, o.at + *to, Color::srgb(0.95, 0.65, 0.2)),
            Kind::SwingPole { to } | Kind::Beam { to, .. } => gizmos.line(o.at, o.at + *to, Color::srgba(0.9, 0.9, 0.95, 0.7)),
            Kind::Ladder { height, .. } => {
                let side = Vec3::Y.cross(o.dir()) * 0.25;
                for s in [side, -side] {
                    gizmos.line(o.at + s, o.at + s + Vec3::Y * *height, Color::srgb(0.9, 0.35, 0.25));
                }
                arrow(&mut gizmos, o.at + Vec3::Y * 0.5, o.dir(), 0.6, Color::srgb(0.9, 0.35, 0.25));
            }
            Kind::BarbedWire | Kind::SoftPad { solid: false } => cuboid(&mut gizmos, &b, Color::srgba(0.8, 0.8, 0.8, 0.5)),
            // Lights in their own colour: a point light's bulb (and, selected, how far it
            // reaches), a spot light's cone, the sun's way.
            Kind::PointLight { color, range, .. } => {
                let c = Color::srgb(color.x, color.y, color.z);
                gizmos.sphere(Isometry3d::from_translation(o.at), 0.15, c);
                if ed.selected == Some(i) {
                    gizmos.sphere(Isometry3d::from_translation(o.at), *range, c.with_alpha(0.25));
                }
            }
            Kind::SpotLight { color, range, angle, .. } => {
                let c = Color::srgb(color.x, color.y, color.z);
                let reach = if ed.selected == Some(i) { *range } else { range.min(2.0) };
                let (dir, rot) = (o.dir(), o.rotation());
                let r = reach * angle.clamp(1.0, 89.0).to_radians().tan();
                let end = o.at + dir * reach;
                gizmos.circle(Isometry3d::new(end, rot), r, c);
                for k in 0..8 {
                    let a = k as f32 * std::f32::consts::TAU / 8.0;
                    gizmos.line(o.at, end + rot * Vec3::new(a.cos() * r, a.sin() * r, 0.0), c.with_alpha(0.6));
                }
                gizmos.sphere(Isometry3d::from_translation(o.at), 0.12, c);
            }
            Kind::Sun { color, .. } => {
                let c = Color::srgb(color.x, color.y, color.z);
                gizmos.circle(Isometry3d::new(o.at, o.rotation()), 0.4, c);
                arrow(&mut gizmos, o.at, o.dir(), 2.5, c);
            }
            Kind::Platform { travel, .. } => {
                let end = Aabb::new(b.min + *travel, b.max + *travel);
                cuboid(&mut gizmos, &end, Color::srgba(0.6, 0.6, 0.7, 0.5));
                gizmos.line(b.center(), end.center(), Color::srgba(0.6, 0.6, 0.7, 0.5));
            }
            // Portal's entities: a volume's outline (a trigger's orange), a point entity's
            // mark in its kind's colour (props and brushes are drawn as they are).
            Kind::Entity { class, brushes, .. } => {
                if !brushes.is_empty() {
                    if volume(&o.kind) {
                        let c = if class.starts_with("trigger_") { Color::srgba(1.0, 0.55, 0.15, 0.7) } else { Color::srgba(0.75, 0.75, 0.8, 0.5) };
                        cuboid(&mut gizmos, &o.bounds(), c);
                    }
                } else if let Some(c) = entity_color(class) {
                    gizmos.cube(Transform::from_translation(o.at).with_scale(Vec3::splat(0.25)), c);
                }
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
        if o.kind.turns() == Some(Turns::Aim) {
            arrow(&mut gizmos, o.at, o.dir(), 0.8, Color::srgba(1.0, 1.0, 1.0, 0.5));
        }
    }
    // The selection, and what it's wired to.
    if let Some(sel) = ed.selected.and_then(|i| ed.doc.objects.get(i)) {
        let b = sel.bounds();
        let grow = Aabb::new(b.min - 0.02, b.max + 0.02);
        cuboid(&mut gizmos, &grow, Color::srgb(1.0, 0.75, 0.1));
        // A Portal entity's outputs (green, to what they name) and what names it (cyan).
        let name = sel.name.to_ascii_lowercase();
        let targets: Vec<String> = sel.outputs.iter().map(|out| out.target.to_ascii_lowercase()).collect();
        if !targets.is_empty() || !name.is_empty() {
            let from = b.center();
            for o in &ed.doc.objects {
                if std::ptr::eq(o, sel) {
                    continue;
                }
                // (Compared as they are: no copies of thousands of names a frame.)
                if !o.name.is_empty() && targets.iter().any(|t| t.eq_ignore_ascii_case(&o.name)) {
                    gizmos.arrow(from, o.bounds().center(), Color::srgb(0.3, 1.0, 0.4));
                }
                if !name.is_empty() && o.outputs.iter().any(|out| out.target.eq_ignore_ascii_case(&name)) {
                    gizmos.arrow(o.bounds().center(), from, Color::srgb(0.3, 0.85, 1.0));
                }
            }
        }
        let sigs: Vec<usize> = sel.kind.drives().into_iter().chain(sel.kind.inputs().map(|i| i.all.clone()).unwrap_or_default()).collect();
        for o in &ed.doc.objects {
            let linked = o.kind.drives().is_some_and(|s| sigs.contains(&s)) || o.kind.inputs().is_some_and(|i| i.all.iter().any(|s| sigs.contains(s)));
            if linked && !std::ptr::eq(o, sel) {
                gizmos.line(sel.bounds().center(), o.bounds().center(), Color::srgb(0.3, 0.7, 1.0));
                cuboid(&mut gizmos, &o.bounds(), Color::srgba(0.3, 0.7, 1.0, 0.6));
            }
        }
    }
    // (Bright: it's often over white panels.)
    let green = Color::srgb(0.2, 1.0, 0.35);
    for g in &ed.ghost {
        let b = g.bounds();
        cuboid(&mut gizmos, &b, green);
        let (lo, hi) = (b.min, Vec3::new(b.max.x, b.min.y, b.max.z));
        gizmos.line(lo, hi, green);
        gizmos.line(Vec3::new(lo.x, lo.y, hi.z), Vec3::new(hi.x, lo.y, lo.z), green);
        if g.kind.turns() == Some(Turns::Aim) {
            arrow(&mut gizmos, g.at, g.dir(), 0.8, green);
        }
    }
}

/// A point entity's mark's colour, by its kind (none: it's drawn, a prop).
fn entity_color(class: &str) -> Option<Color> {
    Some(if class.starts_with("prop_") || class == "env_sprite" || class == "move_rope" || class == "keyframe_rope" {
        return None;
    } else if class.starts_with("logic_") || class.starts_with("math_") || class.starts_with("point_") || class == "env_entity_maker" {
        Color::srgb(1.0, 0.85, 0.2)
    } else if class.starts_with("light") {
        Color::srgb(1.0, 0.95, 0.75)
    } else if class.starts_with("info_") || class.starts_with("path_") {
        Color::srgb(0.3, 0.9, 0.45)
    } else if class.starts_with("npc_") || class.starts_with("weapon_") {
        Color::srgb(1.0, 0.35, 0.3)
    } else if class.starts_with("ambient_") || class.starts_with("env_soundscape") {
        Color::srgb(0.6, 0.5, 1.0)
    } else {
        Color::srgb(0.4, 0.85, 1.0)
    })
}

/// The View menu's choices: what the camera draws (the level's meshes, Faith, the editor's
/// lines) and fog. Leaving the editor puts the camera back as it was.
#[allow(clippy::type_complexity)]
fn apply_view(
    mut commands: Commands,
    mut ed: ResMut<Editor>,
    mut cams: Query<(Entity, &mut RenderLayers, Has<bevy::pbr::DistanceFog>), With<PlayerCamera>>,
    mut taa: Query<&mut bevy::anti_alias::taa::TemporalAntiAliasing, With<PlayerCamera>>,
    #[cfg(feature = "dlss")] mut dlss: Query<&mut bevy::anti_alias::dlss::Dlss, With<PlayerCamera>>,
    #[cfg(feature = "dlss")] mut dlss_rr: Query<&mut bevy::anti_alias::dlss::Dlss<bevy::anti_alias::dlss::DlssRayReconstructionFeature>, With<PlayerCamera>>,
    mut fogged: Local<bool>,
) {
    let Ok((cam, mut layers, fog)) = cams.single_mut() else { return };
    // Without the level's meshes nothing's drawn where they were: the temporal smoothing's
    // history (the denoiser's, with traced light) would hold their ghosts there for good.
    if ed.active && !ed.view.meshes {
        for mut t in &mut taa {
            t.reset = true;
        }
        #[cfg(feature = "dlss")]
        {
            for mut d in &mut dlss {
                d.reset = true;
            }
            for mut d in &mut dlss_rr {
                d.reset = true;
            }
        }
    }
    if !ed.active {
        if let Some(saved) = ed.saved_layers.take() {
            *layers = saved;
        }
        return;
    }
    let saved = ed.saved_layers.get_or_insert_with(|| layers.clone()).clone();
    let mut want = RenderLayers::layer(LINES_LAYER);
    if ed.view.meshes {
        want = want.union(&saved.clone().without(crate::portal_render::PLAYER_LAYER));
    }
    if ed.view.body {
        want = want.with(crate::portal_render::PLAYER_LAYER);
    }
    if *layers != want {
        *layers = want;
    }
    // Fog off: taken off the camera; back on, the level's lighting puts it back.
    if !ed.view.fog && fog {
        commands.entity(cam).remove::<bevy::pbr::DistanceFog>();
        *fogged = false;
    } else if ed.view.fog && !*fogged {
        *fogged = true;
        if !fog {
            ed.rebuild = true;
        }
    }
}

/// Mirror's Edge props whose box isn't known yet (placed before their mesh was read, or
/// imported without a block): it is once the level's drawn them.
fn me_bounds(mut ed: ResMut<Editor>, cache: Option<Res<crate::me_props::MePropCache>>) {
    let Some(cache) = cache else { return };
    let missing = ed.doc.objects.iter().any(|o| matches!(o.kind, Kind::MeProp { bounds: None, .. }));
    if !missing {
        return;
    }
    // (Found out, not an edit: no undo step.)
    for o in &mut ed.doc.objects {
        if let Kind::MeProp { package, mesh, bounds: b @ None, .. } = &mut o.kind {
            *b = cache.looks(package, mesh).and_then(|l| l.bounds);
        }
    }
}

/// How a material was before the unlit view: its colour, glow (and its map, how exposure
/// takes it, whether ray tracing had scaled it).
struct Lit {
    base: Color,
    emissive: LinearRgba,
    emissive_texture: Option<Handle<Image>>,
    weight: f32,
    scaled: Option<(f32, f32)>,
}

/// The unlit view: every material shows its own colour, as glow alone (no colour lit by
/// anything: ray-traced lighting draws a material marked unlit black). As it was after.
fn unlit(
    ed: Res<Editor>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    mut scaled: Option<ResMut<crate::raytrace::ScaledEmissive>>,
    mut saved: Local<std::collections::HashMap<AssetId<StandardMaterial>, Lit>>,
    mut on: Local<bool>,
) {
    let want = ed.active && ed.view.unlit;
    let added: Vec<AssetId<StandardMaterial>> = events.read().filter_map(|e| if let AssetEvent::Added { id } = e { Some(*id) } else { None }).collect();
    let rt = crate::raytrace::active();
    let mut flatten = |id: AssetId<StandardMaterial>, materials: &mut Assets<StandardMaterial>, saved: &mut std::collections::HashMap<AssetId<StandardMaterial>, Lit>| {
        if saved.contains_key(&id) {
            return;
        }
        let Some(mut m) = materials.get_mut(id) else { return };
        // (The editor's own lines and handles are unlit already.)
        if m.unlit {
            return;
        }
        let was_scaled = scaled.as_ref().and_then(|s| s.0.get(&id).copied());
        saved.insert(id, Lit { base: m.base_color, emissive: m.emissive, emissive_texture: m.emissive_texture.clone(), weight: m.emissive_exposure_weight, scaled: was_scaled });
        let weight = m.emissive_exposure_weight;
        let c = m.base_color.to_linear();
        // Its colour as glow: in physical units when the light's traced (as every glow is then).
        let scale = if rt { crate::raytrace::EMISSIVE_SCALE } else { 1.0 };
        m.emissive = LinearRgba::rgb(c.red * scale, c.green * scale, c.blue * scale);
        m.emissive_texture = m.base_color_texture.clone();
        m.emissive_exposure_weight = if rt { 1.0 } else { 0.0 };
        m.base_color = Color::linear_rgba(0.0, 0.0, 0.0, c.alpha);
        // (Marked as scaled already, so the traced light doesn't scale it again; with the
        // lighting switched off meanwhile, it's unscaled like any other.)
        if let (true, Some(s)) = (rt, scaled.as_mut()) {
            s.0.entry(id).or_insert((scale, weight));
        }
    };
    if want && !*on {
        *on = true;
        let ids: Vec<_> = materials.ids().collect();
        for id in ids {
            flatten(id, &mut materials, &mut saved);
        }
    } else if want {
        // What's made while it's on (a level rebuilt, props loaded).
        for id in added {
            flatten(id, &mut materials, &mut saved);
        }
    } else if *on {
        *on = false;
        for (id, lit) in saved.drain() {
            if let Some(mut m) = materials.get_mut(id) {
                m.base_color = lit.base;
                m.emissive = lit.emissive;
                m.emissive_texture = lit.emissive_texture;
                m.emissive_exposure_weight = lit.weight;
            }
            // As the traced light had it (or hadn't).
            if let Some(s) = scaled.as_mut() {
                match lit.scaled {
                    Some(v) => {
                        s.0.insert(id, v);
                    }
                    None => {
                        s.0.remove(&id);
                    }
                }
            }
        }
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
    fn props_rest_on_what_they_are_dropped_on() {
        let mut lib = Library::default();
        lib.props.push(library::Prop { name: "crate".into(), game: 1, folder: "props".into(), source: library::Source::Model("props/portal2/props/crate.glb".into()), bounds: Some((Vec3::new(-0.5, -0.2, -0.3), Vec3::new(0.5, 1.0, 0.3))), looks: None });
        // On the floor: its bottom on it, facing the camera.
        let o = lib.object(0, Vec3::new(0.0, 0.0, -5.0), Vec3::Y, Vec3::NEG_Z);
        assert!(o.bounds().min.y.abs() < 1e-4, "{:?}", o.bounds());
        assert!((o.dir() - Vec3::Z).length() < 1e-4, "{}", o.dir());
        // On a wall facing +X: its back against it.
        let o = lib.object(0, Vec3::new(-6.0, 1.0, -5.0), Vec3::X, Vec3::NEG_X);
        assert!((o.bounds().min.x + 6.0).abs() < 1e-4, "{:?}", o.bounds());
        // It builds solid, its block where it is, and reads back.
        let mut doc = LevelDoc::starter("T");
        doc.objects.push(o);
        let l = doc.build();
        assert_eq!(l.models.len(), 1);
        assert_eq!(l.models[0].solid, Some(doc.objects.last().unwrap().bounds()));
        assert!(l.solids.iter().any(|s| Some(s.b) == l.models[0].solid));
        assert_eq!(LevelDoc::from_ron(&doc.to_ron()).unwrap(), doc);
    }

    #[test]
    fn the_end_handle_drags_along_its_axis() {
        let cam = FlyCam { pos: Vec3::new(0.0, 2.0, 5.0), yaw: 0.0, pitch: 0.0, speed: 8.0 };
        let end = Vec3::new(0.0, 2.0, -5.0);
        // Taking hold of the X arrow, then moving the cursor 1.3 m to the right: 1.25 on a
        // quarter-metre grid, along X only.
        let at = |x: f32| (cam.pos, (Vec3::new(x, 2.0, -5.0) - cam.pos).normalize());
        let (o, d) = at(0.3);
        let grab = end_grab(&cam, 0, end, o, d).unwrap();
        let (o, d) = at(1.6);
        let moved = end_moved(&cam, 0, end, grab, o, d, 0.25).unwrap();
        assert!((moved - Vec3::new(1.25, 2.0, -5.0)).length() < 1e-4, "{moved}");
        let mut zip = Object::new(Kind::ZipLine { to: Vec3::new(0.0, -2.0, -10.0) }, Vec3::new(0.0, 4.0, 5.0));
        set_end(&mut zip, moved);
        assert_eq!(end_of(&zip), Some(moved));
        assert_eq!(zip.at, Vec3::new(0.0, 4.0, 5.0), "the start stays");
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
        // Portal's volumes, sounds and effects among them, as the logic has them, and it runs.
        assert_eq!(l.volumes.len(), 5, "push, hurt, detector, bumper, no-portal");
        assert!(l.art.as_ref().is_some_and(|a| a.props.iter().any(|p| p.kind == aperture::art::PropKind::Camera)));
        let mut sim = aperture::Sim::new(l);
        let mut c = faith_move::Controller::new(aperture::tuning(5.3, 15.0), Vec3::ZERO, 0.0);
        for _ in 0..30 {
            sim.step(1.0 / 60.0, &mut c);
        }
        let io = sim.io.as_ref().expect("its logic");
        assert!(io.ents.iter().any(|e| matches!(e.kind, aperture::io::Kind::Sound { .. })));
        assert!(io.ents.iter().any(|e| matches!(e.kind, aperture::io::Kind::Correction { .. })));
        assert!(sim.movers.iter().any(|m| m.def.spin.is_some()), "the fan");
    }
}
