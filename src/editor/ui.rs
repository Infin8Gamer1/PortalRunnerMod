//! The editor's panels (egui): the toolbar, the content browser, the outliner and the details.

use std::collections::{HashMap, HashSet};

use bevy::gizmos::transform_gizmo::{TransformGizmoMode, TransformGizmoSettings};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiTextureHandle};

use aperture::brush::{tool_material, Collision, Contents};
use aperture::doc::{Kind, LevelDoc, Object, Turns};
use aperture::io::Output;
use aperture::{CubeKind, Inputs, Mat, Paint, PortalColor};

use super::catalog::{Category, CATALOG};
use super::library::{Library, GAMES};
use super::thumbs::Thumbs;
use super::{Editor, Pick};
use crate::chamber::ChamberMats;
use crate::{menu, Game};

const MATS: [Mat; 11] = [Mat::White, Mat::Black, Mat::Glass, Mat::Frosted, Mat::Grate, Mat::Runner, Mat::Prop, Mat::Roof, Mat::Wall, Mat::Concrete, Mat::Clip];
const CUBES: [CubeKind; 3] = [CubeKind::Storage, CubeKind::Companion, CubeKind::Reflector];
const PAINTS: [Paint; 4] = [Paint::Bounce, Paint::Speed, Paint::Portal, Paint::Water];
const GRIDS: [f32; 6] = [0.0, 0.125, 0.25, 0.5, 1.0, 2.0];
const CONTENTS: [Contents; 7] = [Contents::Solid, Contents::Window, Contents::Grate, Contents::Goo, Contents::Water, Contents::Clip, Contents::Empty];

/// Which objects the outliner lists.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Shown {
    #[default]
    All,
    /// Brushes, displacements and overlays.
    Brushes,
    /// Portal's entities.
    Entities,
    /// Everything else: the game's own pieces, props, lights.
    Pieces,
}

use super::MatTarget;

/// The browser's search, the outliner's filter.
#[derive(Default)]
pub struct UiState {
    search: String,
    filter: String,
    shown_kinds: Shown,
    /// The selection the outliner last showed (to scroll to a new one).
    shown: Option<usize>,
    /// The outliner's rows (object, label), and what they were made for.
    rows: Vec<(usize, String)>,
    rows_key: Option<(String, Shown, u64, usize)>,
    /// What the props browser lists (and for what).
    prop_list: Vec<usize>,
    prop_key: Option<(usize, Option<String>, String, usize)>,
    /// The names in the level (for outputs' targets), and when they were gathered.
    names: Vec<String>,
    names_version: Option<u64>,
    /// The material picker's search, and the materials it lists.
    mat_search: String,
    mat_list: Vec<String>,
    mat_key: Option<String>,
    /// Materials whose pictures are wanted (shown this frame), and their pictures.
    mat_wanted: Vec<String>,
    mat_tex: HashMap<String, Option<egui::TextureId>>,
}

/// A material's look for its tile: its texture, else its colour.
pub enum Thumb {
    Image(egui::TextureId),
    Color(egui::Color32),
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut contexts: EguiContexts,
    mut ed: ResMut<Editor>,
    mut game: ResMut<Game>,
    mut menu: ResMut<menu::Menu>,
    mut settings: ResMut<TransformGizmoSettings>,
    mats: Option<Res<ChamberMats>>,
    materials: Res<Assets<StandardMaterial>>,
    mut thumbs: Local<HashMap<Mat, Thumb>>,
    mut state: Local<UiState>,
    lib: Res<Library>,
    me_cache: Option<Res<crate::me_props::MePropCache>>,
    mut game_settings: ResMut<crate::settings::Settings>,
    mut prop_thumbs: ResMut<Thumbs>,
    mut prop_textures: Local<HashMap<usize, egui::TextureId>>,
    (extracted, mut images): (Res<crate::extracted::Extracted>, ResMut<Assets<Image>>),
) -> Result {
    // The props' pictures made since last time, for egui.
    for (i, h) in &prop_thumbs.ready {
        if !prop_textures.contains_key(i) {
            prop_textures.insert(*i, contexts.add_image(EguiTextureHandle::Strong(h.clone())));
        }
    }
    // Pictures of Portal's materials shown last frame (a few a frame: they're read from disk).
    let wanted: Vec<String> = std::mem::take(&mut state.mat_wanted).into_iter().filter(|n| !state.mat_tex.contains_key(n)).take(6).collect();
    for name in wanted {
        let tex = extracted.texture(&format!("map/{name}")).map(|img| contexts.add_image(EguiTextureHandle::Strong(images.add(img))));
        state.mat_tex.insert(name, tex);
    }
    if let Some(m) = &mats {
        for mat in MATS {
            if thumbs.contains_key(&mat) {
                continue;
            }
            let Some(sm) = materials.get(&crate::chamber::mat_for(m, mat)) else { continue };
            let thumb = match &sm.base_color_texture {
                Some(tex) => Thumb::Image(contexts.add_image(EguiTextureHandle::Strong(tex.clone()))),
                None => {
                    let c = sm.base_color.to_srgba();
                    Thumb::Color(egui::Color32::from_rgb((c.red * 255.0) as u8, (c.green * 255.0) as u8, (c.blue * 255.0) as u8))
                }
            };
            thumbs.insert(mat, thumb);
        }
    }
    let ctx = contexts.ctx_mut()?.clone();
    let ed = &mut *ed;
    let mut root = egui::Ui::new(ctx.clone(), "editor".into(), egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.viewport_rect()));
    ed.panels.clear();
    ed.tiles.clear();

    // ---- toolbar
    let top = egui::Panel::top("editor_toolbar").show(&mut root, |ui| {
        ui.horizontal(|ui| {
            toolbar(ui, ed, &mut game, &mut menu, &mut settings, &mut game_settings);
        });
    });
    ed.panels.push(top.response.rect);

    // ---- status line
    let bottom = egui::Panel::bottom("editor_status").show(&mut root, |ui| {
        ui.horizontal(|ui| {
            ui.weak("RMB + WASD/QE fly · wheel speed · W/E/R move/rotate/scale · drag from Content · Del delete · Ctrl+D duplicate · Ctrl+Z/Y undo · F focus · F5 or F4 play");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(&ed.status);
            });
        });
    });
    ed.panels.push(bottom.response.rect);

    // ---- content browser
    let left = egui::Panel::left("editor_content").resizable(true).default_size(330.0).size_range(200.0..=700.0).show(&mut root, |ui| {
        let Thumbs { wanted, failed, .. } = &mut *prop_thumbs;
        let mut props = PropThumbs { textures: &prop_textures, wanted, failed };
        content(ui, ed, &mut state, &thumbs, &lib, &mut props);
    });
    ed.panels.push(left.response.rect);

    // ---- outliner and details
    let right = egui::Panel::right("editor_details").resizable(true).default_size(330.0).size_range(220.0..=700.0).show(&mut root, |ui| {
        outliner(ui, ed, &mut state);
        ui.separator();
        egui::ScrollArea::vertical().id_salt("details").show(ui, |ui| {
            let looks = ed.selected.and_then(|i| ed.doc.objects.get(i)).and_then(|o| materials_of(&o.kind, &lib, me_cache.as_deref()));
            details(ui, ed, looks.as_ref(), &mut state);
        });
    });
    ed.panels.push(right.response.rect);

    // ---- the material picker
    if let Some(target) = ed.picking {
        let mut open = true;
        let window = egui::Window::new("Portal's materials").open(&mut open).default_size([460.0, 520.0]).default_pos(ctx.viewport_rect().center() - egui::vec2(230.0, 260.0)).show(&ctx, |ui| material_picker(ui, ed, &mut state, target));
        if let Some(w) = window {
            ed.panels.push(w.response.rect);
        }
        if !open || ed.selected != Some(target.obj) {
            ed.picking = None;
        }
    }

    // One undo step per edit in the panels: committed once it's let go of.
    let busy = ctx.egui_is_using_pointer() || ctx.memory(|m| m.focused().is_some());
    if !busy {
        if let Some(before) = ed.before_edit.take() {
            ed.commit(before);
        }
    }
    Ok(())
}

fn toolbar(ui: &mut egui::Ui, ed: &mut Editor, game: &mut Game, menu: &mut menu::Menu, settings: &mut TransformGizmoSettings, game_settings: &mut crate::settings::Settings) {
    ui.menu_button("File", |ui| {
        if ed.unsaved {
            ui.colored_label(egui::Color32::from_rgb(240, 180, 60), "Unsaved changes are lost by New/Open/Import");
            ui.separator();
        }
        if ui.button("New room").clicked() {
            ed.open(LevelDoc::starter("Untitled"), None);
            ui.close();
        }
        if ui.button("New, empty").clicked() {
            let mut d = LevelDoc::new("Untitled");
            d.objects.push(aperture::doc::Object::new(Kind::Spawn, Vec3::ZERO));
            ed.open(d, None);
            ui.close();
        }
        ui.menu_button("Open", |ui| {
            let files = super::level_files();
            if files.is_empty() {
                ui.weak(format!("No levels in {}", super::levels_dir().display()));
            }
            for p in files {
                let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                if ui.button(name).clicked() {
                    match super::read_doc(&p) {
                        Ok(d) => {
                            ed.open(d, Some(p.clone()));
                            ed.status = format!("Opened {}", p.display());
                        }
                        Err(e) => ed.status = e,
                    }
                    ui.close();
                }
            }
        });
        ui.menu_button("Open a Portal chamber", |ui| {
            let files = crate::source_map::native_files(crate::extracted::find_root().as_deref());
            if files.is_empty() {
                ui.weak("None yet: run the extractor (cargo run --release -p portal_extract)");
            }
            for (p, map) in files {
                let title = aperture::port::MAPS.iter().find(|(m, _)| *m == map).map(|(_, t)| aperture::port::native_title(t)).unwrap_or(map);
                if ui.button(title).clicked() {
                    match super::read_doc(&p) {
                        Ok(d) => {
                            let n = d.objects.len();
                            ed.open(d, Some(p.clone()));
                            ed.status = format!("Opened {} ({n} objects)", p.display());
                        }
                        Err(e) => ed.status = e,
                    }
                    ui.close();
                }
            }
        });
        ui.menu_button("Import a chamber", |ui| {
            for i in 0..game.levels.len() {
                let custom = ed.files.iter().any(|(j, _)| *j == i);
                if game.source_names[i].is_some() || custom || Some(i) == ed.slot {
                    continue;
                }
                if ui.button(&game.levels[i].0).clicked() {
                    ed.open_level(game, i);
                    ui.close();
                }
            }
        });
        ui.separator();
        if ui.button("Save   Ctrl+S").clicked() {
            ed.save(game, false);
            ui.close();
        }
        if ui.button("Save as a new level").clicked() {
            ed.save(game, true);
            ui.close();
        }
    });
    ui.menu_button("View", |ui| {
        let v = &mut ed.view;
        ui.checkbox(&mut v.meshes, "Visual meshes").on_hover_text("The level as the game draws it");
        ui.checkbox(&mut v.collision, "Collision").on_hover_text("What Faith and cubes collide with: blue takes portals, orange doesn't, cyan glass, yellow grating, magenta clip");
        ui.checkbox(&mut v.markers, "Markers").on_hover_text("The start, checkpoints, the exit, hints, cuts, wires, which way things point");
        ui.checkbox(&mut v.all_links, "All signal links").on_hover_text("Lines from every button, catcher and relay to what it drives (the selection's always show)");
        ui.checkbox(&mut v.grid, "Grid");
        ui.checkbox(&mut v.body, "Faith");
        ui.checkbox(&mut v.fog, "Fog");
        ui.separator();
        ui.label("Lighting");
        ui.radio_value(&mut v.unlit, false, "Lit");
        ui.radio_value(&mut v.unlit, true, "Unlit").on_hover_text("Everything at its own colour (textures as they are)");
        ui.separator();
        // (Not while unlit: that view takes the materials' glow over, as switching does.)
        let mut rt = game_settings.ray_tracing >= 0.5;
        let r = ui.add_enabled(!v.unlit, egui::Checkbox::new(&mut rt, format!("Ray-traced lighting: {}", crate::raytrace::label(game_settings.ray_tracing))));
        let r = if v.unlit { r.on_disabled_hover_text("Switch to Lit first") } else { r.on_hover_text("Switched straight away (the game's setting too)") };
        if r.changed() {
            game_settings.ray_tracing = if rt { 1.0 } else { 0.0 };
            game_settings.save();
        }
    });
    ui.separator();
    if ui.add_enabled(ed.can_undo(), egui::Button::new("⟲ Undo")).clicked() {
        ed.undo();
    }
    if ui.add_enabled(ed.can_redo(), egui::Button::new("⟳ Redo")).clicked() {
        ed.redo();
    }
    ui.separator();
    ui.selectable_value(&mut settings.mode, TransformGizmoMode::Translate, "➡ Move (W)");
    ui.selectable_value(&mut settings.mode, TransformGizmoMode::Rotate, "⟳ Rotate (E)");
    ui.selectable_value(&mut settings.mode, TransformGizmoMode::Scale, "🔲 Scale (R)");
    // Along the world's axes, or the selection's own (scaling is always its own).
    let local = settings.space == bevy::gizmos::transform_gizmo::TransformGizmoSpace::Local;
    if ui.selectable_label(local, if local { "Local" } else { "World" }).on_hover_text("Move and rotate along the world's axes, or the selection's own").clicked() {
        settings.space = if local { bevy::gizmos::transform_gizmo::TransformGizmoSpace::World } else { bevy::gizmos::transform_gizmo::TransformGizmoSpace::Local };
    }
    ui.separator();
    egui::ComboBox::from_id_salt("grid").width(70.0).selected_text(grid_name(ed.grid)).show_ui(ui, |ui| {
        for g in GRIDS {
            ui.selectable_value(&mut ed.grid, g, grid_name(g));
        }
    });
    ui.label("snap");
    ui.separator();
    if ui.button(egui::RichText::new("▶ Play").strong().color(egui::Color32::from_rgb(120, 230, 120))).on_hover_text("F5 (F4 comes back)").clicked() {
        ed.play(menu);
    }
    ui.separator();
    let file = ed.path.as_ref().and_then(|p| p.file_name()).map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "not saved yet".into());
    ui.label(egui::RichText::new(format!("{}{}", ed.doc.name, if ed.unsaved { " *" } else { "" })).strong());
    ui.weak(file);
}

fn grid_name(g: f32) -> String {
    if g <= 0.0 { "off".into() } else { format!("{g} m") }
}

fn content(ui: &mut egui::Ui, ed: &mut Editor, state: &mut UiState, thumbs: &HashMap<Mat, Thumb>, lib: &Library, props: &mut PropThumbs) {
    ui.heading("Content");
    ui.horizontal_wrapped(|ui| {
        if ui.selectable_label(ed.tab.is_none(), "All").clicked() {
            ed.tab = None;
        }
        for c in Category::ALL {
            if ui.selectable_label(ed.tab == Some(c), c.name()).clicked() {
                ed.tab = Some(c);
            }
        }
    });
    ui.add(egui::TextEdit::singleline(&mut state.search).hint_text("Search"));
    ui.weak("Drag into the view, or click and then click in the view. Double-click: in front of you.");
    ui.separator();
    let panel = ui.clip_rect();
    if ed.tab == Some(Category::Props) {
        props_browser(ui, ed, state, lib, props, panel);
        return;
    }
    let search = state.search.to_lowercase();
    egui::ScrollArea::vertical().id_salt("content").show(ui, |ui| {
        for cat in Category::ALL.iter() {
            if ed.tab.is_some_and(|c| c != *cat) {
                continue;
            }
            let entries: Vec<usize> = (0..CATALOG.len()).filter(|&i| CATALOG[i].category == *cat && (search.is_empty() || CATALOG[i].name.to_lowercase().contains(&search))).collect();
            if entries.is_empty() {
                continue;
            }
            ui.label(egui::RichText::new(cat.name()).strong());
            // (Rows by hand: a drag source doesn't wrap in a wrapping layout.)
            let per_row = ((ui.available_width() + 6.0) / (TILE.x + 6.0)).floor().max(1.0) as usize;
            for row in entries.chunks(per_row) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for &i in row {
                        let pick = Pick::Entry(i);
                        let armed = ed.armed == Some(pick);
                        let (rect, r) = ui.allocate_exact_size(TILE, egui::Sense::click_and_drag());
                        tile(ui.painter(), rect, i, thumbs, armed);
                        tile_input(ui, ed, lib, pick, &r, CATALOG[i].name, panel, |p, rect| tile(p, rect, i, thumbs, false));
                        ed.tiles.push((pick, r.rect));
                    }
                });
            }
            ui.add_space(6.0);
        }
        if ed.tab.is_none() {
            ui.weak("Portal's, Portal 2's and Mirror's Edge's props are under Props.");
        }
    });
}

/// The props' pictures as egui knows them, and the ones the browser is showing.
pub struct PropThumbs<'a> {
    pub textures: &'a HashMap<usize, egui::TextureId>,
    pub wanted: &'a mut Vec<usize>,
    /// The ones whose pictures couldn't be made.
    pub failed: &'a HashSet<usize>,
}

/// Every prop, by game and folder: only the rows on screen are laid out (and only their
/// pictures asked for).
fn props_browser(ui: &mut egui::Ui, ed: &mut Editor, state: &mut UiState, lib: &Library, props: &mut PropThumbs, panel: egui::Rect) {
    if !lib.loaded {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Reading the props...");
        });
        return;
    }
    ui.horizontal(|ui| {
        for (g, name) in GAMES.iter().enumerate() {
            if ui.selectable_label(ed.prop_game == g, *name).clicked() && ed.prop_game != g {
                ed.prop_game = g;
                ed.prop_folder = None;
            }
        }
    });
    let folders = &lib.folders[ed.prop_game];
    egui::ComboBox::from_id_salt("prop_folder").width(ui.available_width() - 8.0).selected_text(ed.prop_folder.clone().unwrap_or_else(|| "All folders".into())).show_ui(ui, |ui| {
        ui.selectable_value(&mut ed.prop_folder, None, "All folders");
        for f in folders {
            ui.selectable_value(&mut ed.prop_folder, Some(f.clone()), f);
        }
    });
    // What's listed, worked out again only when what picks it changes.
    let key = (ed.prop_game, ed.prop_folder.clone(), state.search.to_lowercase(), lib.props.len());
    if state.prop_key.as_ref() != Some(&key) {
        state.prop_list = (0..lib.props.len())
            .filter(|&i| {
                let p = &lib.props[i];
                p.game == key.0 && key.1.as_ref().is_none_or(|f| p.folder == *f) && (key.2.is_empty() || p.name.to_lowercase().contains(&key.2) || p.folder.to_lowercase().contains(&key.2))
            })
            .collect();
        state.prop_key = Some(key);
    }
    if state.prop_list.is_empty() {
        ui.weak(match ed.prop_game {
            2 => "No Mirror's Edge props: the game wasn't found (set FAITH_ME_DIR or me_path.txt).",
            _ if lib.props.iter().all(|p| p.game == 2) => "No Portal props yet: run cargo run --release -p portal_extract -- --only-props",
            _ => "Nothing matches.",
        });
        return;
    }
    ui.weak(format!("{} props", state.prop_list.len()));
    let per_row = ((ui.available_width() + 6.0) / (TILE.x + 6.0)).floor().max(1.0) as usize;
    let rows = state.prop_list.len().div_ceil(per_row);
    let list = &state.prop_list;
    egui::ScrollArea::vertical().id_salt("props").show_rows(ui, TILE.y + 6.0, rows, |ui, range| {
        for row in range {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for &i in list.iter().skip(row * per_row).take(per_row) {
                    let pick = Pick::Prop(i);
                    let armed = ed.armed == Some(pick);
                    let tex = props.textures.get(&i).copied();
                    let name = &lib.props[i].name;
                    let failed = tex.is_none() && props.failed.contains(&i);
                    let (rect, r) = ui.allocate_exact_size(TILE, egui::Sense::click_and_drag());
                    prop_tile(ui.painter(), rect, name, tex, armed, failed);
                    if tex.is_none() {
                        props.wanted.push(i);
                    }
                    ed.tiles.push((pick, r.rect));
                    let r = r.on_hover_text(format!("{name}\n{}", lib.props[i].folder));
                    tile_input(ui, ed, lib, pick, &r, name, panel, |p, rect| prop_tile(p, rect, name, tex, false, failed));
                }
            });
        }
    });
}

/// A tile's clicks and drags: dragged into the view, clicked and then the view clicked, or
/// double-clicked (in front of the camera). `paint` draws it following the cursor.
#[allow(clippy::too_many_arguments)]
fn tile_input(ui: &egui::Ui, ed: &mut Editor, lib: &Library, pick: Pick, r: &egui::Response, name: &str, panel: egui::Rect, paint: impl Fn(&egui::Painter, egui::Rect)) {
    let armed = ed.armed == Some(pick);
    if r.dragged() {
        ed.drop = Some(pick);
        ed.armed = None;
        // Over the panel, the tile goes with the cursor; over the view, the outline of where it
        // would land shows instead.
        if let Some(p) = ui.ctx().pointer_latest_pos().filter(|p| panel.contains(*p)) {
            paint(&ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("dragged tile"))), egui::Rect::from_center_size(p, TILE));
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if r.double_clicked() {
        ed.armed = None;
        ed.place_in_view(lib, pick);
    } else if r.clicked() {
        ed.armed = if armed { None } else { Some(pick) };
        if ed.armed.is_some() {
            ed.status = format!("Click in the view to place {name} (Shift: more than one, Esc: stop)");
        }
    }
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// A prop's tile: its picture (a placeholder until it's made), its name under it.
fn prop_tile(p: &egui::Painter, rect: egui::Rect, name: &str, tex: Option<egui::TextureId>, armed: bool, failed: bool) {
    let img = egui::Rect::from_min_size(rect.min, egui::vec2(TILE.x, TILE.x));
    p.rect_filled(rect, 4.0, egui::Color32::from_gray(38));
    match tex {
        Some(id) => {
            p.image(id, img.shrink(3.0), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        None => {
            p.rect_filled(img.shrink(3.0), 3.0, egui::Color32::from_gray(30));
            p.text(img.center(), egui::Align2::CENTER_CENTER, if failed { "?" } else { "…" }, egui::FontId::proportional(20.0), egui::Color32::from_gray(90));
        }
    }
    if armed {
        p.rect_stroke(rect, 4.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 190, 30)), egui::StrokeKind::Inside);
    }
    let label = egui::Rect::from_min_max(egui::pos2(rect.min.x + 2.0, img.max.y), egui::pos2(rect.max.x - 2.0, rect.max.y));
    let galley = p.layout(name.to_string(), egui::FontId::proportional(10.0), egui::Color32::from_gray(210), label.width());
    p.with_clip_rect(label).galley(egui::pos2(label.center().x - galley.size().x * 0.5, label.min.y + 1.0), galley, egui::Color32::WHITE);
}

const TILE: egui::Vec2 = egui::vec2(76.0, 102.0);

/// A content browser tile: the material, or a coloured glyph, with the name under it.
fn tile(p: &egui::Painter, rect: egui::Rect, i: usize, thumbs: &HashMap<Mat, Thumb>, armed: bool) {
    let e = &CATALOG[i];
    let img = egui::Rect::from_min_size(rect.min, egui::vec2(TILE.x, TILE.x));
    p.rect_filled(rect, 4.0, egui::Color32::from_gray(38));
    if armed {
        p.rect_stroke(rect, 4.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 190, 30)), egui::StrokeKind::Inside);
    }
    let [r, g, b] = e.color;
    match e.mat.and_then(|m| thumbs.get(&m)) {
        Some(Thumb::Image(id)) => {
            p.image(*id, img.shrink(4.0), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        Some(Thumb::Color(c)) => {
            p.rect_filled(img.shrink(4.0), 3.0, *c);
        }
        None => {
            p.rect_filled(img.shrink(4.0), 3.0, egui::Color32::from_rgb(r / 3, g / 3, b / 3));
            p.text(img.center(), egui::Align2::CENTER_CENTER, e.glyph, egui::FontId::proportional(if e.glyph.chars().count() > 1 { 16.0 } else { 30.0 }), egui::Color32::from_rgb(r, g, b));
        }
    }
    let label = egui::Rect::from_min_max(egui::pos2(rect.min.x + 2.0, img.max.y), egui::pos2(rect.max.x - 2.0, rect.max.y));
    let galley = p.layout(e.name.to_string(), egui::FontId::proportional(11.0), egui::Color32::from_gray(220), label.width());
    p.galley(egui::pos2(label.center().x - galley.size().x * 0.5, label.min.y + 2.0), galley, egui::Color32::WHITE);
}

fn outliner(ui: &mut egui::Ui, ed: &mut Editor, state: &mut UiState) {
    ui.heading("Outliner");
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text("Filter (label or name)").desired_width(ui.available_width() - 100.0));
        egui::ComboBox::from_id_salt("shown").width(84.0).selected_text(format!("{:?}", state.shown_kinds)).show_ui(ui, |ui| {
            for k in [Shown::All, Shown::Brushes, Shown::Entities, Shown::Pieces] {
                ui.selectable_value(&mut state.shown_kinds, k, format!("{k:?}"));
            }
        });
    });
    // The rows, made again only when the level or the filter changes (a Portal map has
    // thousands of objects).
    let key = (state.filter.to_lowercase(), state.shown_kinds, ed.version, ed.doc.objects.len());
    if state.rows_key.as_ref() != Some(&key) {
        state.rows = ed
            .doc
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| match state.shown_kinds {
                Shown::All => true,
                Shown::Brushes => matches!(o.kind, Kind::Brush(_) | Kind::Displacement(_) | Kind::Overlay { .. }),
                Shown::Entities => matches!(o.kind, Kind::Entity { .. }),
                Shown::Pieces => !o.kind.is_map_part(),
            })
            .map(|(i, o)| (i, if o.name.is_empty() { o.kind.label() } else { format!("{}  \"{}\"", o.kind.label(), o.name) }))
            .filter(|(_, label)| key.0.is_empty() || label.to_lowercase().contains(&key.0))
            .collect();
        state.rows_key = Some(key);
    }
    let scroll_to = ed.selected != state.shown;
    state.shown = ed.selected;
    let row_h = ui.text_style_height(&egui::TextStyle::Button) + ui.spacing().item_spacing.y;
    let mut area = egui::ScrollArea::vertical().id_salt("outliner").max_height(240.0).auto_shrink([false, true]);
    if scroll_to {
        if let Some(k) = ed.selected.and_then(|s| state.rows.iter().position(|(i, _)| *i == s)) {
            area = area.vertical_scroll_offset((k as f32 * row_h - 100.0).max(0.0));
        }
    }
    let mut pick = None;
    area.show_rows(ui, row_h, state.rows.len(), |ui, range| {
        for (i, label) in &state.rows[range] {
            if ui.selectable_label(ed.selected == Some(*i), format!("{i:>4}  {label}")).clicked() {
                pick = Some(*i);
            }
        }
    });
    if let Some(i) = pick {
        ed.selected = Some(i);
        state.shown = Some(i);
    }
    ui.weak(format!("{} of {} objects", state.rows.len(), ed.doc.objects.len()));
}

/// What a prop's materials found: a line to show, whether some have nothing, and which.
struct Looks {
    text: String,
    lacking: bool,
    which: String,
}

fn materials_of(kind: &Kind, lib: &Library, me: Option<&crate::me_props::MePropCache>) -> Option<Looks> {
    match kind {
        Kind::Model { path, .. } => Some(match lib.by_path.get(path).and_then(|&i| lib.props[i].looks) {
            Some((t, n)) => Looks { text: format!("{t} of {n} textured (or glass)"), lacking: t < n, which: String::new() },
            None => Looks { text: "not known (run the extractor again)".into(), lacking: false, which: String::new() },
        }),
        Kind::MeProp { package, mesh, .. } => {
            let Some(l) = me.and_then(|c| c.looks(package, mesh)) else {
                return Some(Looks { text: "not read yet".into(), lacking: false, which: String::new() });
            };
            let mut text = format!("{} of {} textured", l.textured, l.total);
            if l.plain > 0 {
                text += &format!(", {} a plain colour", l.plain);
            }
            if l.unset > 0 {
                text += &format!(", {} unset (a level gives them one)", l.unset);
            }
            if !l.missing.is_empty() {
                text += &format!(", {} without", l.missing.len());
            }
            Some(Looks { text, lacking: !l.missing.is_empty(), which: l.missing.join(", ") })
        }
        _ => None,
    }
}

fn details(ui: &mut egui::Ui, ed: &mut Editor, looks: Option<&Looks>, state: &mut UiState) {
    // What it was before this frame's edits: the level's own fields and the selected object
    // (not the whole level each frame: a Portal map's is big).
    let objects = std::mem::take(&mut ed.doc.objects);
    let head = ed.doc.clone();
    ed.doc.objects = objects;
    let sel = ed.selected;
    let was: Option<Object> = sel.and_then(|i| ed.doc.objects.get(i).cloned());
    let newest = ed.doc.objects.iter().rev().find_map(|o| o.kind.drives()).unwrap_or(0);
    // The names in the level (outputs name them), gathered again when it changes.
    if state.names_version != Some(ed.version) {
        let mut names: Vec<String> = ed.doc.objects.iter().filter(|o| !o.name.is_empty()).map(|o| o.name.clone()).collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        state.names = names;
        state.names_version = Some(ed.version);
    }
    // What fires at the selection: (the object, its event, the input).
    let incoming: Vec<(usize, String, String)> = match sel.and_then(|i| ed.doc.objects.get(i)).map(|o| o.name.clone()).filter(|n| !n.is_empty()) {
        Some(name) => ed.doc.objects.iter().enumerate().flat_map(|(k, o)| o.outputs.iter().filter(|out| out.target.eq_ignore_ascii_case(&name)).map(move |out| (k, out.event.clone(), out.input.clone()))).collect(),
        None => Vec::new(),
    };
    let mut picking = None;
    let incoming_labels: Vec<String> = incoming.iter().map(|(k, e, inp)| {
        let o = &ed.doc.objects[*k];
        format!("{} {}: {e}, {inp}", o.kind.label(), o.name)
    }).collect();
    let mut go_to = None;
    match ed.selected {
        None => {
            ui.heading("Level");
            egui::Grid::new("level").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                ui.label("Name");
                if ui.text_edit_singleline(&mut ed.save_name).changed() {
                    ed.doc.name = ed.save_name.clone();
                }
                ui.end_row();
                ui.label("Intro");
                ui.add(egui::TextEdit::multiline(&mut ed.doc.intro).desired_rows(2));
                ui.end_row();
                ui.label("Indoors");
                ui.checkbox(&mut ed.doc.indoor, "").on_hover_text("Off: sun, sky and Mirror's Edge's music");
                ui.end_row();
                ui.label("Kill below");
                ui.add(egui::DragValue::new(&mut ed.doc.kill_y).speed(0.25).suffix(" m"));
                ui.end_row();
                ui.label("Portal gun");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut ed.doc.blue, "blue");
                    ui.checkbox(&mut ed.doc.orange, "orange");
                });
                ui.end_row();
                if let Some(map) = &ed.doc.map {
                    ui.label("Portal map");
                    ui.label(map).on_hover_text("The game's own copy of this map: its level changes lead to the next map's copy");
                    ui.end_row();
                }
                ui.label("Ambient light");
                ui.vertical(|ui| {
                    let mut own = ed.doc.ambient.is_some();
                    if ui.checkbox(&mut own, "the level's own").on_hover_text("The fill light everywhere (else as indoors or outdoors has it)").changed() {
                        ed.doc.ambient = own.then_some(aperture::doc::Ambient { color: Vec3::new(0.92, 0.95, 1.0), brightness: if ed.doc.indoor { 520.0 } else { 650.0 } });
                    }
                    if let Some(a) = &mut ed.doc.ambient {
                        ui.horizontal(|ui| {
                            color_field(ui, &mut a.color);
                            ui.add(egui::DragValue::new(&mut a.brightness).speed(5.0).range(0.0..=20_000.0)).on_hover_text("Brightness (the game's indoors is 520)");
                        });
                    }
                });
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.weak(format!("{} objects. Click one in the view or the outliner to edit it.", ed.doc.objects.len()));
        }
        Some(i) if i < ed.doc.objects.len() => {
            let mut delete = false;
            let mut duplicate = false;
            let mut pick_target = false;
            {
                let o = &mut ed.doc.objects[i];
                ui.heading(o.kind.label());
                egui::Grid::new("object").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label(if o.kind.is_box() { "Centre" } else { "Position" });
                    vec3(ui, &mut o.at, 0.05);
                    ui.end_row();
                    if o.kind.is_box() {
                        ui.label("Size");
                        vec3(ui, &mut o.size, 0.05);
                        o.size = o.size.max(Vec3::splat(0.01));
                        ui.end_row();
                    }
                    match o.kind.turns() {
                        // About each axis (applied Y, then X, then Z).
                        Some(Turns::Free) => {
                            ui.label("Rotation");
                            ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(&mut o.pitch).speed(1.0).prefix("x ").suffix("°"));
                                ui.add(egui::DragValue::new(&mut o.yaw).speed(1.0).prefix("y ").suffix("°"));
                                ui.add(egui::DragValue::new(&mut o.roll).speed(1.0).prefix("z ").suffix("°"));
                            });
                            ui.end_row();
                        }
                        Some(turns) => {
                            ui.label("Yaw");
                            ui.add(egui::DragValue::new(&mut o.yaw).speed(1.0).suffix("°"));
                            ui.end_row();
                            if turns == Turns::Aim {
                                ui.label("Pitch");
                                ui.add(egui::DragValue::new(&mut o.pitch).speed(1.0).range(-90.0..=90.0).suffix("°"));
                                ui.end_row();
                            }
                        }
                        None => {}
                    }
                    if matches!(o.kind, Kind::Entity { .. } | Kind::Overlay { .. }) {
                        ui.label("Name");
                        ui.add(egui::TextEdit::singleline(&mut o.name).hint_text("what the logic calls it"));
                        ui.end_row();
                    }
                    map_fields(ui, o, i, state, &mut picking);
                    pick_target = kind_fields(ui, &mut o.kind, newest);
                    if let Some(l) = looks {
                        ui.label("Materials");
                        let r = if l.lacking { ui.colored_label(egui::Color32::from_rgb(240, 180, 60), &l.text) } else { ui.label(&l.text) };
                        if !l.which.is_empty() {
                            r.on_hover_text(format!("Without a texture or colour: {}", l.which));
                        }
                        ui.end_row();
                    }
                });
                if let Kind::Entity { keys, .. } = &mut o.kind {
                    entity_keys(ui, keys);
                }
                if matches!(o.kind, Kind::Entity { .. }) {
                    outputs(ui, &mut o.outputs, &state.names);
                    if !incoming_labels.is_empty() {
                        egui::CollapsingHeader::new(format!("Fired at it ({})", incoming_labels.len())).default_open(true).show(ui, |ui| {
                            for (k, label) in incoming_labels.iter().enumerate() {
                                if ui.link(label).on_hover_text("Select it").clicked() {
                                    go_to = Some(incoming[k].0);
                                }
                            }
                        });
                    }
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                duplicate = ui.button("Duplicate").clicked();
                delete = ui.button("Delete").clicked();
            });
            if let Some(s) = ed.doc.objects[i].kind.drives() {
                let n = ed.doc.objects.iter().filter(|o| o.kind.inputs().is_some_and(|inp| inp.all.contains(&s))).count();
                ui.weak(format!("Drives signal {s}: {n} piece(s) listen (lines in the view)."));
            }
            if pick_target {
                ed.pick_target = true;
                ed.status = "Click where the faith plate should land".into();
            }
            if duplicate {
                ed.duplicate_selected();
                return;
            }
            if delete {
                ed.delete_selected();
                return;
            }
        }
        Some(_) => ed.selected = None,
    }
    // Changed this frame: the level as it was, for its undo step (one copy, as the edit starts).
    let objects = std::mem::take(&mut ed.doc.objects);
    let head_changed = ed.doc != head;
    ed.doc.objects = objects;
    let obj_changed = sel.is_some_and(|i| ed.doc.objects.get(i) != was.as_ref());
    if (head_changed || obj_changed) && ed.before_edit.is_none() {
        let mut before = LevelDoc { objects: ed.doc.objects.clone(), ..head };
        if let (Some(i), Some(w)) = (sel, was) {
            if i < before.objects.len() {
                before.objects[i] = w;
            }
        }
        ed.before_edit = Some(before);
    }
    if let Some(k) = go_to {
        ed.selected = Some(k);
    }
    if picking.is_some() {
        ed.picking = picking;
    }
}

/// A material's name and its picture (if it has one), small.
fn material_row(ui: &mut egui::Ui, state: &mut UiState, mat: &str, size: f32) {
    match state.mat_tex.get(mat) {
        Some(Some(id)) => {
            ui.add(egui::Image::new((*id, egui::vec2(size, size))));
        }
        Some(None) => {
            ui.add_sized([size, size], egui::Label::new("·"));
        }
        None => {
            state.mat_wanted.push(mat.to_string());
            ui.add_sized([size, size], egui::Label::new("…"));
        }
    }
    ui.label(mat.rsplit('/').next().unwrap_or(mat)).on_hover_text(mat);
}

/// A brush's, displacement's or overlay's fields: what it is and its materials.
fn map_fields(ui: &mut egui::Ui, o: &mut Object, i: usize, state: &mut UiState, picking: &mut Option<MatTarget>) {
    match &mut o.kind {
        Kind::Brush(b) => {
            ui.label("Contents");
            combo(ui, "contents", &mut b.contents, &CONTENTS);
            ui.end_row();
            ui.label("Collides as");
            ui.label(match b.collision() {
                Collision::Solid(m) if m.portalable() => "solid: takes portals".to_string(),
                Collision::Solid(m) => format!("solid: {m:?}, no portals"),
                Collision::Goo => "deadly goo".to_string(),
                Collision::None => "nothing".to_string(),
            })
            .on_hover_text("Its contents, and what most of its drawn faces' materials take");
            ui.end_row();
            if let Some(bx) = b.bounds() {
                let s = bx.size();
                ui.label("Size");
                ui.label(format!("{:.2} x {:.2} x {:.2} m", s.x, s.y, s.z)).on_hover_text("Scale it with the gizmo (R)");
                ui.end_row();
            }
            ui.label("Materials");
            if ui.button("Pick for every face…").on_hover_text("Every drawn face (not its nodraw ones)").clicked() {
                *picking = Some(MatTarget { obj: i, face: None });
            }
            ui.end_row();
            for (k, f) in b.faces.iter().enumerate() {
                let n = f.n;
                let side = [(Vec3::Y, "top"), (Vec3::NEG_Y, "bottom"), (Vec3::X, "+x"), (Vec3::NEG_X, "-x"), (Vec3::Z, "+z"), (Vec3::NEG_Z, "-z")].iter().find(|(a, _)| a.dot(n) > 0.99).map_or("slant", |(_, s)| s);
                ui.label(format!("  face {k} ({side})"));
                ui.horizontal(|ui| {
                    material_row(ui, state, &f.mat, 18.0);
                    if ui.small_button("…").on_hover_text("Pick its material").clicked() {
                        *picking = Some(MatTarget { obj: i, face: Some(k) });
                    }
                    if !tool_material(&f.mat) && f.noportal {
                        ui.weak("no portals");
                    }
                });
                ui.end_row();
            }
        }
        Kind::Displacement(d) => {
            ui.label("Material");
            ui.horizontal(|ui| {
                let mat = d.mat.clone();
                material_row(ui, state, &mat, 18.0);
                if ui.small_button("…").clicked() {
                    *picking = Some(MatTarget { obj: i, face: None });
                }
            });
            ui.end_row();
            ui.label("Grid");
            ui.label(format!("{0} x {0} points", d.side()));
            ui.end_row();
        }
        Kind::Overlay { mat, .. } => {
            ui.label("Material");
            ui.horizontal(|ui| {
                let m = mat.clone();
                material_row(ui, state, &m, 18.0);
                if ui.small_button("…").clicked() {
                    *picking = Some(MatTarget { obj: i, face: None });
                }
            });
            ui.end_row();
        }
        Kind::Entity { class, brushes, .. } => {
            ui.label("Class");
            ui.add(egui::TextEdit::singleline(class).hint_text("Source's: logic_relay, trigger_once..."));
            ui.end_row();
            if !brushes.is_empty() {
                ui.label("Brushes");
                ui.label(format!("{} (they move with it)", brushes.len()));
                ui.end_row();
            }
        }
        _ => {}
    }
}

/// A Portal entity's keys (Source's): each one's value, to change; more to add.
fn entity_keys(ui: &mut egui::Ui, keys: &mut Vec<(String, String)>) {
    egui::CollapsingHeader::new(format!("Keys ({})", keys.len())).default_open(true).show(ui, |ui| {
        let mut remove = None;
        for (k, (key, value)) in keys.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add_sized([120.0, 18.0], egui::TextEdit::singleline(key));
                ui.add_sized([(ui.available_width() - 28.0).max(60.0), 18.0], egui::TextEdit::singleline(value));
                if ui.small_button("x").on_hover_text("Remove it").clicked() {
                    remove = Some(k);
                }
            });
        }
        if let Some(k) = remove {
            keys.remove(k);
        }
        if ui.small_button("+ key").clicked() {
            keys.push((String::new(), String::new()));
        }
    });
}

/// A Portal entity's outputs: when it fires an event, an input sent (after a delay, so many
/// times) to the entities of a name.
fn outputs(ui: &mut egui::Ui, outs: &mut Vec<Output>, names: &[String]) {
    egui::CollapsingHeader::new(format!("Outputs ({})", outs.len())).default_open(true).show(ui, |ui| {
        let mut remove = None;
        for (k, out) in outs.iter_mut().enumerate() {
            ui.push_id(k, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut out.event).desired_width(90.0).hint_text("OnTrigger"));
                    ui.label("to");
                    ui.add(egui::TextEdit::singleline(&mut out.target).desired_width(100.0).hint_text("target"));
                    ui.menu_button("…", |ui| {
                        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                            for n in names {
                                if ui.button(n).clicked() {
                                    out.target = n.clone();
                                    ui.close();
                                }
                            }
                        });
                    })
                    .response
                    .on_hover_text("The level's names");
                    if ui.small_button("x").on_hover_text("Remove it").clicked() {
                        remove = Some(k);
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.add(egui::TextEdit::singleline(&mut out.input).desired_width(80.0).hint_text("Input"));
                    ui.add(egui::TextEdit::singleline(&mut out.param).desired_width(60.0).hint_text("value"));
                    ui.add(egui::DragValue::new(&mut out.delay).speed(0.05).range(0.0..=600.0).prefix("after ").suffix(" s"));
                    ui.add(egui::DragValue::new(&mut out.times).speed(0.1).range(-1..=99).prefix("× ")).on_hover_text("How many times (-1: every time)");
                });
            });
            ui.add_space(2.0);
        }
        if let Some(k) = remove {
            outs.remove(k);
        }
        if ui.small_button("+ output").clicked() {
            outs.push(Output { event: "OnTrigger".into(), target: String::new(), input: "Trigger".into(), param: String::new(), delay: 0.0, times: -1 });
        }
    });
}

/// Portal's materials, to give the face (or faces) being picked for.
fn material_picker(ui: &mut egui::Ui, ed: &mut Editor, state: &mut UiState, target: MatTarget) {
    let flags = crate::source_map::material_flags();
    ui.add(egui::TextEdit::singleline(&mut state.mat_search).hint_text("Search").desired_width(f32::INFINITY));
    let key = state.mat_search.to_lowercase();
    if state.mat_key.as_ref() != Some(&key) {
        let mut list: Vec<String> = flags.keys().filter(|m| key.is_empty() || m.contains(&key)).cloned().collect();
        list.extend(["tools/toolsnodraw", "tools/toolsplayerclip", "tools/toolstrigger", "tools/toolsinvisible"].iter().map(|t| t.to_string()).filter(|t| key.is_empty() || t.contains(&key)));
        list.sort();
        state.mat_list = list;
        state.mat_key = Some(key);
    }
    if flags.is_empty() {
        ui.weak("No materials: run the extractor (cargo run --release -p portal_extract)");
    }
    let tile = egui::vec2(96.0, 118.0);
    let per_row = ((ui.available_width() + 6.0) / (tile.x + 6.0)).floor().max(1.0) as usize;
    let rows = state.mat_list.len().div_ceil(per_row);
    let mut chosen = None;
    let list = state.mat_list.clone();
    egui::ScrollArea::vertical().id_salt("materials").show_rows(ui, tile.y + 6.0, rows, |ui, range| {
        for row in range {
            ui.horizontal(|ui| {
                for m in list.iter().skip(row * per_row).take(per_row) {
                    let (rect, r) = ui.allocate_exact_size(tile, egui::Sense::click());
                    let p = ui.painter();
                    p.rect_filled(rect, 4.0, if r.hovered() { egui::Color32::from_gray(60) } else { egui::Color32::from_gray(38) });
                    let img = egui::Rect::from_min_size(rect.min, egui::vec2(tile.x, tile.x)).shrink(4.0);
                    match state.mat_tex.get(m) {
                        Some(Some(id)) => {
                            p.image(*id, img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                        }
                        got => {
                            if got.is_none() {
                                state.mat_wanted.push(m.clone());
                            }
                            p.rect_filled(img, 3.0, egui::Color32::from_gray(28));
                            p.text(img.center(), egui::Align2::CENTER_CENTER, if m.starts_with("tools/") { "tool" } else { "…" }, egui::FontId::proportional(14.0), egui::Color32::from_gray(110));
                        }
                    }
                    let label = egui::Rect::from_min_max(egui::pos2(rect.min.x + 2.0, img.max.y + 2.0), rect.max);
                    let galley = p.layout(m.rsplit('/').next().unwrap_or(m).to_string(), egui::FontId::proportional(10.0), egui::Color32::from_gray(210), label.width());
                    p.with_clip_rect(label).galley(label.min, galley, egui::Color32::WHITE);
                    if r.on_hover_text(m.as_str()).clicked() {
                        chosen = Some(m.clone());
                    }
                }
            });
        }
    });
    let Some(mat) = chosen else { return };
    // (Whether it takes portals, as the maps' materials say; a tool texture says nothing.)
    let noportal = !tool_material(&mat) && flags.get(&mat).is_some_and(|f| f & 16 == 0);
    let see_through = aperture::port::see_through(flags, &mat);
    ed.change(|d| {
        let Some(o) = d.objects.get_mut(target.obj) else { return };
        match &mut o.kind {
            Kind::Brush(b) => {
                let all_tools = b.faces.iter().all(|f| tool_material(&f.mat));
                for (k, f) in b.faces.iter_mut().enumerate() {
                    let take = match target.face {
                        Some(j) => j == k,
                        None => all_tools || !tool_material(&f.mat),
                    };
                    if take {
                        f.mat = mat.clone();
                        f.noportal = noportal;
                        f.see_through = see_through;
                    }
                }
            }
            Kind::Displacement(dp) => {
                dp.mat = mat.clone();
                dp.noportal = noportal;
            }
            Kind::Overlay { mat: m, .. } => *m = mat.clone(),
            _ => {}
        }
    });
    ed.status = format!("Material: {mat}");
}

fn vec3(ui: &mut egui::Ui, v: &mut Vec3, speed: f64) {
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut v.x).speed(speed).prefix("x ").max_decimals(3));
        ui.add(egui::DragValue::new(&mut v.y).speed(speed).prefix("y ").max_decimals(3));
        ui.add(egui::DragValue::new(&mut v.z).speed(speed).prefix("z ").max_decimals(3));
    });
}

fn combo<T: PartialEq + Copy + std::fmt::Debug>(ui: &mut egui::Ui, id: &str, value: &mut T, options: &[T]) {
    egui::ComboBox::from_id_salt(id).selected_text(format!("{value:?}")).show_ui(ui, |ui| {
        for o in options {
            ui.selectable_value(value, *o, format!("{o:?}"));
        }
    });
}

fn signal(ui: &mut egui::Ui, sig: &mut usize) {
    ui.label("Drives signal");
    ui.add(egui::DragValue::new(sig).speed(0.1).range(0..=99));
    ui.end_row();
}

/// What turns it on: the signals that must all be on (none: always), or inverted.
fn inputs(ui: &mut egui::Ui, inp: &mut Inputs, newest: usize) {
    ui.label("On when");
    ui.vertical(|ui| {
        if inp.all.is_empty() {
            ui.weak("always on");
        }
        let mut remove = None;
        for (k, s) in inp.all.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label("signal");
                ui.add(egui::DragValue::new(s).speed(0.1).range(0..=99));
                if ui.small_button("✕").clicked() {
                    remove = Some(k);
                }
            });
        }
        if let Some(k) = remove {
            inp.all.remove(k);
        }
        ui.horizontal(|ui| {
            if ui.small_button("+ signal").clicked() {
                inp.all.push(newest);
            }
            if !inp.all.is_empty() {
                ui.checkbox(&mut inp.invert, "inverted").on_hover_text("On while they're off");
            }
        });
    });
    ui.end_row();
}

/// A colour (sRGB, 0-1), picked.
fn color_field(ui: &mut egui::Ui, color: &mut Vec3) {
    let mut c = color.to_array();
    if ui.color_edit_button_rgb(&mut c).changed() {
        *color = Vec3::from(c);
    }
}

/// A light's colour, power, reach, size and shadows.
fn light_fields(ui: &mut egui::Ui, color: &mut Vec3, intensity: &mut f32, range: &mut f32, radius: &mut f32, shadows: &mut bool) {
    ui.label("Colour");
    color_field(ui, color);
    ui.end_row();
    ui.label("Intensity");
    let speed = (*intensity * 0.01).max(10.0);
    ui.add(egui::DragValue::new(intensity).speed(speed).range(0.0..=100_000_000.0).suffix(" lm")).on_hover_text("Luminous power: the ceiling lamps are 900,000");
    ui.end_row();
    ui.label("Range");
    ui.add(egui::DragValue::new(range).speed(0.1).range(0.5..=500.0).suffix(" m")).on_hover_text("How far it reaches");
    ui.end_row();
    ui.label("Size");
    ui.add(egui::DragValue::new(radius).speed(0.01).range(0.0..=5.0).suffix(" m")).on_hover_text("The light's own radius: bigger is softer");
    ui.end_row();
    ui.label("Shadows");
    ui.checkbox(shadows, "").on_hover_text("Casts shadows (with ray-traced lighting, everything does)");
    ui.end_row();
}

/// A prop's scale, and whether it's solid (a block the size of its box).
fn prop_fields(ui: &mut egui::Ui, scale: &mut Vec3, bounds: &Option<(Vec3, Vec3)>, solid: &mut bool) {
    ui.label("Scale");
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (axis, v) in ["x ", "y ", "z "].into_iter().zip([&mut scale.x, &mut scale.y, &mut scale.z]) {
                ui.add(egui::DragValue::new(v).speed(0.01).range(0.01..=50.0).prefix(axis));
            }
        });
        // All three at once (from the largest).
        let mut all = scale.max_element();
        if ui.add(egui::DragValue::new(&mut all).speed(0.01).range(0.01..=50.0).prefix("all ")).changed() {
            *scale = Vec3::splat(all);
        }
    });
    ui.end_row();
    ui.label("Solid");
    ui.add_enabled(bounds.is_some(), egui::Checkbox::without_text(solid)).on_hover_text("A block the size of its box stops Faith and cubes");
    ui.end_row();
    if let Some((lo, hi)) = bounds {
        let size = (*hi - *lo) * *scale;
        ui.label("Size");
        ui.label(format!("{:.2} x {:.2} x {:.2} m", size.x, size.y, size.z));
        ui.end_row();
    }
}

/// The fields of what it is; true if the faith plate's target is to be picked in the view.
fn kind_fields(ui: &mut egui::Ui, kind: &mut Kind, newest: usize) -> bool {
    let mut pick = false;
    match kind {
        Kind::Block { mat } | Kind::Decor { mat } => {
            ui.label("Material");
            combo(ui, "mat", mat, &MATS);
            ui.end_row();
        }
        Kind::Checkpoint { name, spawn } => {
            ui.label("Name");
            ui.text_edit_singleline(name);
            ui.end_row();
            ui.label("Respawn (from centre)");
            vec3(ui, spawn, 0.05);
            ui.end_row();
        }
        Kind::Hint { text } => {
            ui.label("Text");
            ui.add(egui::TextEdit::multiline(text).desired_rows(2));
            ui.end_row();
        }
        Kind::FloorButton { sig, cube_only } => {
            signal(ui, sig);
            ui.label("Cubes only");
            ui.checkbox(cube_only, "");
            ui.end_row();
        }
        Kind::Pedestal { sig, time } => {
            signal(ui, sig);
            ui.label("On for");
            ui.add(egui::DragValue::new(time).speed(0.1).range(0.0..=120.0).suffix(" s")).on_hover_text("0: it toggles");
            ui.end_row();
        }
        Kind::Catcher { sig } | Kind::Relay { sig } | Kind::PelletCatcher { sig } => signal(ui, sig),
        Kind::Door { inputs: inp } | Kind::Fizzler { inputs: inp } | Kind::Bridge { inputs: inp } | Kind::Laser { inputs: inp } => inputs(ui, inp, newest),
        Kind::Cube { cube } => {
            ui.label("Cube");
            combo(ui, "cube", cube, &CUBES);
            ui.end_row();
        }
        Kind::Dropper { cube, inputs: inp } => {
            ui.label("Cube");
            combo(ui, "cube", cube, &CUBES);
            ui.end_row();
            inputs(ui, inp, newest);
        }
        Kind::FaithPlate { target, apex, inputs: inp } => {
            ui.label("Lands on");
            ui.vertical(|ui| {
                vec3(ui, target, 0.05);
                pick = ui.button("Pick in the view").clicked();
            });
            ui.end_row();
            ui.label("Apex");
            ui.add(egui::DragValue::new(apex).speed(0.05).range(0.2..=30.0).suffix(" m")).on_hover_text("How far above the higher end the arc peaks");
            ui.end_row();
            inputs(ui, inp, newest);
        }
        Kind::GelPipe { paint, inputs: inp, rate, speed } => {
            ui.label("Gel");
            combo(ui, "paint", paint, &PAINTS);
            ui.end_row();
            ui.label("Rate");
            ui.add(egui::DragValue::new(rate).speed(0.5).range(1.0..=120.0).suffix(" /s"));
            ui.end_row();
            ui.label("Speed");
            ui.add(egui::DragValue::new(speed).speed(0.1).range(0.5..=30.0).suffix(" m/s"));
            ui.end_row();
            inputs(ui, inp, newest);
        }
        Kind::Painted { paint, normal } => {
            ui.label("Gel");
            combo(ui, "paint", paint, &PAINTS[..3]);
            ui.end_row();
            ui.label("Facing");
            let axes = [Vec3::Y, Vec3::NEG_Y, Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z];
            let names = ["up", "down", "+x", "-x", "+z", "-z"];
            let cur = axes.iter().position(|a| a.dot(*normal) > 0.9).map(|k| names[k]).unwrap_or("?");
            egui::ComboBox::from_id_salt("normal").selected_text(cur).show_ui(ui, |ui| {
                for (a, n) in axes.iter().zip(names) {
                    if ui.selectable_label(a.dot(*normal) > 0.9, n).clicked() {
                        *normal = *a;
                    }
                }
            });
            ui.end_row();
        }
        Kind::PelletLauncher { inputs: inp, life } => {
            ui.label("Pellet lives");
            ui.add(egui::DragValue::new(life).speed(0.1).range(0.5..=1000.0).suffix(" s"));
            ui.end_row();
            inputs(ui, inp, newest);
        }
        Kind::Platform { travel, speed, inputs: inp } => {
            ui.label("Travels");
            vec3(ui, travel, 0.05);
            ui.end_row();
            ui.label("Speed");
            ui.add(egui::DragValue::new(speed).speed(0.05).range(0.1..=30.0).suffix(" m/s"));
            ui.end_row();
            inputs(ui, inp, newest);
        }
        Kind::Portal { color } => {
            ui.label("Colour");
            combo(ui, "portal", color, &[PortalColor::Blue, PortalColor::Orange]);
            ui.end_row();
        }
        Kind::Wire { sig, points } => {
            ui.label("Lit by signal");
            ui.add(egui::DragValue::new(sig).speed(0.1).range(0..=99));
            ui.end_row();
            ui.label("Then to");
            ui.vertical(|ui| {
                let mut remove = None;
                for (k, p) in points.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        vec3(ui, p, 0.05);
                        if ui.small_button("✕").clicked() {
                            remove = Some(k);
                        }
                    });
                }
                if let Some(k) = remove {
                    points.remove(k);
                }
                if ui.small_button("+ point").clicked() {
                    let last = points.last().copied().unwrap_or(Vec3::ZERO);
                    points.push(last + Vec3::new(0.0, 0.0, -2.0));
                }
            });
            ui.end_row();
        }
        Kind::MeProp { package, mesh, scale, bounds, solid } => {
            ui.label("Mirror's Edge");
            ui.label(format!("{package} / {mesh}"));
            ui.end_row();
            prop_fields(ui, scale, bounds, solid);
        }
        Kind::Model { path, scale, bounds, solid } => {
            ui.label("Model");
            ui.label(path.as_str());
            ui.end_row();
            prop_fields(ui, scale, bounds, solid);
        }
        Kind::PointLight { color, intensity, range, radius, shadows } => light_fields(ui, color, intensity, range, radius, shadows),
        Kind::SpotLight { color, intensity, range, radius, angle, soft, shadows } => {
            light_fields(ui, color, intensity, range, radius, shadows);
            ui.label("Cone");
            ui.add(egui::DragValue::new(angle).speed(0.5).range(1.0..=89.0).suffix("°")).on_hover_text("How far out from its middle it shines");
            ui.end_row();
            ui.label("Soft edge");
            ui.add(egui::Slider::new(soft, 0.0..=1.0)).on_hover_text("How much of the cone fades out to its edge");
            ui.end_row();
        }
        Kind::Sun { color, illuminance } => {
            ui.label("Colour");
            color_field(ui, color);
            ui.end_row();
            ui.label("Brightness");
            let speed = (*illuminance * 0.01).max(1.0);
            ui.add(egui::DragValue::new(illuminance).speed(speed).range(0.0..=200_000.0).suffix(" lux")).on_hover_text("Indoors the game's is 2,500; outdoors, 11,000. It's the level's only sun: where it's placed doesn't matter, which way it faces does");
            ui.end_row();
        }
        Kind::ZipLine { to } | Kind::SwingPole { to } => {
            ui.label("To (from here)");
            vec3(ui, to, 0.05);
            ui.end_row();
        }
        Kind::Beam { to, solid } => {
            ui.label("Along (from here)");
            vec3(ui, to, 0.05);
            ui.end_row();
            ui.label("Its own beam");
            ui.checkbox(solid, "").on_hover_text("Off: it runs along the top of a block");
            ui.end_row();
        }
        Kind::SoftPad { solid } => {
            ui.label("Its own pad");
            ui.checkbox(solid, "").on_hover_text("Off: it marks the top of a block as soft");
            ui.end_row();
        }
        Kind::Ladder { height, pipe, exit } => {
            ui.label("Height");
            ui.add(egui::DragValue::new(height).speed(0.05).range(0.5..=60.0).suffix(" m"));
            ui.end_row();
            ui.label("Drainpipe");
            ui.checkbox(pipe, "");
            ui.end_row();
            ui.label("Climb out at the top");
            ui.checkbox(exit, "");
            ui.end_row();
        }
        Kind::Cut | Kind::Spawn | Kind::Exit | Kind::Lamp | Kind::Goo | Kind::Turret | Kind::BargeDoor | Kind::BarbedWire => {}
        // (Their fields are `map_fields`' and the entity panels'.)
        Kind::Brush(_) | Kind::Displacement(_) | Kind::Overlay { .. } | Kind::Entity { .. } => {}
    }
    pick
}
