//! The editor's panels (egui): the toolbar, the content browser, the outliner and the details.

use std::collections::HashMap;

use bevy::gizmos::transform_gizmo::{TransformGizmoMode, TransformGizmoSettings};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiTextureHandle};

use aperture::doc::{Kind, LevelDoc};
use aperture::{CubeKind, Inputs, Mat, Paint, PortalColor};

use super::catalog::{Category, CATALOG};
use super::Editor;
use crate::chamber::ChamberMats;
use crate::{menu, Game};

const MATS: [Mat; 11] = [Mat::White, Mat::Black, Mat::Glass, Mat::Frosted, Mat::Grate, Mat::Runner, Mat::Prop, Mat::Roof, Mat::Wall, Mat::Concrete, Mat::Clip];
const CUBES: [CubeKind; 3] = [CubeKind::Storage, CubeKind::Companion, CubeKind::Reflector];
const PAINTS: [Paint; 4] = [Paint::Bounce, Paint::Speed, Paint::Portal, Paint::Water];
const GRIDS: [f32; 6] = [0.0, 0.125, 0.25, 0.5, 1.0, 2.0];

/// The browser's tab and search, the outliner's filter.
#[derive(Default)]
pub struct UiState {
    category: Option<usize>,
    search: String,
    filter: String,
    /// The selection the outliner last showed (to scroll to a new one).
    shown: Option<usize>,
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
) -> Result {
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

    // ---- toolbar
    let top = egui::Panel::top("editor_toolbar").show(&mut root, |ui| {
        ui.horizontal(|ui| {
            toolbar(ui, ed, &mut game, &mut menu, &mut settings);
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
        content(ui, ed, &mut state, &thumbs);
    });
    ed.panels.push(left.response.rect);

    // ---- outliner and details
    let right = egui::Panel::right("editor_details").resizable(true).default_size(330.0).size_range(220.0..=700.0).show(&mut root, |ui| {
        outliner(ui, ed, &mut state);
        ui.separator();
        egui::ScrollArea::vertical().id_salt("details").show(ui, |ui| {
            details(ui, ed);
        });
    });
    ed.panels.push(right.response.rect);

    // One undo step per edit in the panels: committed once it's let go of.
    let busy = ctx.egui_is_using_pointer() || ctx.memory(|m| m.focused().is_some());
    if !busy {
        if let Some(before) = ed.before_edit.take() {
            ed.commit(before);
        }
    }
    Ok(())
}

fn toolbar(ui: &mut egui::Ui, ed: &mut Editor, game: &mut Game, menu: &mut menu::Menu, settings: &mut TransformGizmoSettings) {
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

fn content(ui: &mut egui::Ui, ed: &mut Editor, state: &mut UiState, thumbs: &HashMap<Mat, Thumb>) {
    ui.heading("Content");
    ui.horizontal_wrapped(|ui| {
        if ui.selectable_label(state.category.is_none(), "All").clicked() {
            state.category = None;
        }
        for (i, c) in Category::ALL.iter().enumerate() {
            if ui.selectable_label(state.category == Some(i), c.name()).clicked() {
                state.category = Some(i);
            }
        }
    });
    ui.add(egui::TextEdit::singleline(&mut state.search).hint_text("Search"));
    ui.weak("Drag into the view. Double-click: in front of you.");
    ui.separator();
    let search = state.search.to_lowercase();
    egui::ScrollArea::vertical().id_salt("content").show(ui, |ui| {
        for (ci, cat) in Category::ALL.iter().enumerate() {
            if state.category.is_some_and(|c| c != ci) {
                continue;
            }
            let entries: Vec<usize> = (0..CATALOG.len()).filter(|&i| CATALOG[i].category == *cat && (search.is_empty() || CATALOG[i].name.to_lowercase().contains(&search))).collect();
            if entries.is_empty() {
                continue;
            }
            ui.label(egui::RichText::new(cat.name()).strong());
            // (Rows by hand: a drag source doesn't wrap in a wrapping layout.)
            let per_row = ((ui.available_width() + 6.0) / 78.0).floor().max(1.0) as usize;
            for row in entries.chunks(per_row) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for &i in row {
                        let r = ui.dnd_drag_source(egui::Id::new(("catalog", i)), i, |ui| tile(ui, i, thumbs)).response;
                        if r.dragged() {
                            ed.drop = Some(i);
                        }
                        if r.double_clicked() {
                            ed.place_in_view(i);
                        }
                        r.on_hover_text(CATALOG[i].name);
                    }
                });
            }
            ui.add_space(6.0);
        }
    });
}

/// A content browser tile: the material, or a coloured glyph, with the name under it.
fn tile(ui: &mut egui::Ui, i: usize, thumbs: &HashMap<Mat, Thumb>) {
    let e = &CATALOG[i];
    let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 88.0), egui::Sense::hover());
    let p = ui.painter();
    let img = egui::Rect::from_min_size(rect.min, egui::vec2(72.0, 64.0));
    p.rect_filled(rect, 4.0, egui::Color32::from_gray(38));
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
    ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text("Filter"));
    let filter = state.filter.to_lowercase();
    let scroll_to = ed.selected != state.shown;
    state.shown = ed.selected;
    egui::ScrollArea::vertical().id_salt("outliner").max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
        let mut pick = None;
        for (i, o) in ed.doc.objects.iter().enumerate() {
            let label = o.kind.label();
            if !filter.is_empty() && !label.to_lowercase().contains(&filter) {
                continue;
            }
            let r = ui.selectable_label(ed.selected == Some(i), format!("{i:>3}  {label}"));
            if r.clicked() {
                pick = Some(i);
            }
            if scroll_to && ed.selected == Some(i) {
                r.scroll_to_me(Some(egui::Align::Center));
            }
        }
        if let Some(i) = pick {
            ed.selected = Some(i);
            state.shown = Some(i);
        }
    });
}

fn details(ui: &mut egui::Ui, ed: &mut Editor) {
    let before = ed.doc.clone();
    let newest = ed.doc.objects.iter().rev().find_map(|o| o.kind.drives()).unwrap_or(0);
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
                    if let Some(any) = o.kind.turns() {
                        ui.label("Yaw");
                        ui.add(egui::DragValue::new(&mut o.yaw).speed(1.0).suffix("°"));
                        ui.end_row();
                        if any {
                            ui.label("Pitch");
                            ui.add(egui::DragValue::new(&mut o.pitch).speed(1.0).range(-90.0..=90.0).suffix("°"));
                            ui.end_row();
                        }
                    }
                    pick_target = kind_fields(ui, &mut o.kind, newest);
                });
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
    if ed.doc != before && ed.before_edit.is_none() {
        ed.before_edit = Some(before);
    }
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
        Kind::MeProp { package, mesh, scale, solid } => {
            ui.label("Package");
            ui.text_edit_singleline(package);
            ui.end_row();
            ui.label("Mesh");
            ui.text_edit_singleline(mesh);
            ui.end_row();
            ui.label("Scale");
            ui.add(egui::DragValue::new(scale).speed(0.01).range(0.05..=20.0));
            ui.end_row();
            ui.label("Solid");
            let mut on = solid.is_some();
            if ui.checkbox(&mut on, "").changed() {
                *solid = on.then_some((Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 1.0, 0.5)));
            }
            ui.end_row();
            if let Some((lo, hi)) = solid {
                ui.label("Box from");
                vec3(ui, lo, 0.02);
                ui.end_row();
                ui.label("to");
                vec3(ui, hi, 0.02);
                ui.end_row();
            }
        }
        Kind::Cut | Kind::Spawn | Kind::Exit | Kind::Lamp | Kind::Goo | Kind::Turret => {}
    }
    pick
}
