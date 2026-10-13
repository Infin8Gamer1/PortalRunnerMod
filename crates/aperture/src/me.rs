//! Mirror's Edge's levels, as the game's own (see `portal_extract --port-me`): their meshes are
//! [`Kind::MeProp`](crate::doc::Kind::MeProp)s, drawn from the player's install. A prop set to
//! (`own_shape`) collides with its mesh's own collision, read from the install with the mesh
//! ([`shape`]) and placed where the prop stands; without the game it collides as its box.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use glam::{Quat, Vec3, Vec4};
use me_assets::map::{self, Shape};

use crate::brush::{polygon, Brush, Contents, Face};
use faith_move::Aabb;

use crate::level::{Mat, Slope, Solid};

/// Mirror's Edge's maps, in order: the persistent level, the game's file for it
/// (`levels/me/<file>.ron`), and what it's called.
pub const MAPS: &[(&str, &str, &str)] = &[
    ("Tutorial_p", "tutorial", "Mirror's Edge: Training"),
    ("Edge_p", "edge", "Mirror's Edge: Edge"),
    ("Escape_p", "escape", "Mirror's Edge: Escape"),
    ("Stormdrain_p", "stormdrain", "Mirror's Edge: Stormdrain"),
    ("Cranes_p", "cranes", "Mirror's Edge: Cranes"),
    ("Subway_p", "subway", "Mirror's Edge: Subway"),
    ("Mall_p", "mall", "Mirror's Edge: Mall"),
    ("Factory_p", "factory", "Mirror's Edge: Factory"),
    ("Boat_p", "boat", "Mirror's Edge: Boat"),
    ("Convoy_p", "convoy", "Mirror's Edge: Convoy"),
    ("Scraper_p", "scraper", "Mirror's Edge: Scraper"),
    ("TT_TutorialA01_p", "tt_tutorial_a01", "Mirror's Edge: Time trial, Training A1"),
    ("TT_TutorialA02_p", "tt_tutorial_a02", "Mirror's Edge: Time trial, Training A2"),
    ("TT_TutorialA03_p", "tt_tutorial_a03", "Mirror's Edge: Time trial, Training A3"),
    ("TT_EdgeA01_p", "tt_edge_a01", "Mirror's Edge: Time trial, Edge A1"),
    ("TT_EscapeA01_p", "tt_escape_a01", "Mirror's Edge: Time trial, Escape A1"),
    ("TT_EscapeB01_p", "tt_escape_b01", "Mirror's Edge: Time trial, Escape B1"),
    ("TT_StormdrainA02_p", "tt_stormdrain_a02", "Mirror's Edge: Time trial, Stormdrain A2"),
    ("TT_StormdrainB01_p", "tt_stormdrain_b01", "Mirror's Edge: Time trial, Stormdrain B1"),
    ("TT_StormdrainB02_p", "tt_stormdrain_b02", "Mirror's Edge: Time trial, Stormdrain B2"),
    ("TT_StormdrainB03_p", "tt_stormdrain_b03", "Mirror's Edge: Time trial, Stormdrain B3"),
    ("TT_CranesA01_p", "tt_cranes_a01", "Mirror's Edge: Time trial, Cranes A1"),
    ("TT_CranesB01_p", "tt_cranes_b01", "Mirror's Edge: Time trial, Cranes B1"),
    ("TT_CranesB02_p", "tt_cranes_b02", "Mirror's Edge: Time trial, Cranes B2"),
    ("TT_CranesC01_p", "tt_cranes_c01", "Mirror's Edge: Time trial, Cranes C1"),
    ("TT_CranesD01_p", "tt_cranes_d01", "Mirror's Edge: Time trial, Cranes D1"),
    ("TT_MallA01_p", "tt_mall_a01", "Mirror's Edge: Time trial, Mall A1"),
    ("TT_FactoryA01_p", "tt_factory_a01", "Mirror's Edge: Time trial, Factory A1"),
    ("TT_ConvoyA01_p", "tt_convoy_a01", "Mirror's Edge: Time trial, Convoy A1"),
    ("TT_ConvoyA02_p", "tt_convoy_a02", "Mirror's Edge: Time trial, Convoy A2"),
    ("TT_ConvoyB01_p", "tt_convoy_b01", "Mirror's Edge: Time trial, Convoy B1"),
    ("TT_ConvoyB02_p", "tt_convoy_b02", "Mirror's Edge: Time trial, Convoy B2"),
    ("TT_ScraperA01_p", "tt_scraper_a01", "Mirror's Edge: Time trial, Scraper A1"),
    ("TT_ScraperB01_p", "tt_scraper_b01", "Mirror's Edge: Time trial, Scraper B1"),
];

/// What a Mirror's Edge mesh collides with, in its own space (ours: metres, before it's
/// placed): convex hulls, each its faces' planes (`n·p = d`, facing out: `(n, d)`), or, for a
/// mesh that has none, its triangles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeShape {
    pub hulls: Vec<Vec<Vec4>>,
    pub tris: Vec<[Vec3; 3]>,
}

/// Unreal's points (centimetres; X forward, Y right, Z up) as ours (metres; X right, Y up,
/// Z back).
pub fn ue(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[1], p[2], -p[0]) * 0.01
}

pub fn ue_dir(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[1], p[2], -p[0])
}

/// A plane, its normal made unit length.
pub fn plane(n: Vec3, d: f32) -> Vec4 {
    let len = n.length();
    (n / len).extend(d / len)
}

/// A box's planes: its centre `c`, axes and half sizes.
pub fn box_planes(c: Vec3, axes: [Vec3; 3], half: [f32; 3]) -> Vec<Vec4> {
    let mut out = vec![];
    for k in 0..3 {
        let a = axes[k].normalize();
        out.push(plane(a, a.dot(c) + half[k]));
        out.push(plane(-a, -a.dot(c) + half[k]));
    }
    out
}

impl MeShape {
    /// A mesh's collision as the game has it, as ours.
    pub fn from_game(c: &map::Collision) -> MeShape {
        let mut s = MeShape::default();
        for sh in &c.shapes {
            match sh {
                Shape::Hull { planes, .. } => {
                    if planes.len() >= 4 {
                        s.hulls.push(planes.iter().map(|p| plane(ue_dir([p[0], p[1], p[2]]), p[3] * 0.01)).collect());
                    }
                }
                Shape::Box { tm, size } => {
                    let axes = [ue_dir(tm[0]), ue_dir(tm[1]), ue_dir(tm[2])];
                    s.hulls.push(box_planes(ue(tm[3]), axes, [size[0] * 0.005, size[1] * 0.005, size[2] * 0.005]));
                }
                // (A capsule or a ball as the box round it.)
                Shape::Capsule { tm, radius, length } => {
                    let axes = [ue_dir(tm[0]), ue_dir(tm[1]), ue_dir(tm[2])];
                    let r = radius * 0.01;
                    s.hulls.push(box_planes(ue(tm[3]), axes, [r, r, length * 0.005 + r]));
                }
                Shape::Sphere { center, radius } => {
                    let r = radius * 0.01;
                    s.hulls.push(box_planes(ue(*center), [Vec3::X, Vec3::Y, Vec3::Z], [r, r, r]));
                }
            }
        }
        s.tris = c.tris.iter().map(|t| t.map(ue)).collect();
        s
    }

    pub fn is_empty(&self) -> bool {
        self.hulls.is_empty() && self.tris.is_empty()
    }
}

/// Where the game is and what's been read of it: each package's file, the packages open now
/// (the last few: a level's props come a package at a time), each mesh's shape.
#[derive(Default)]
struct Install {
    cooked: Option<PathBuf>,
    files: HashMap<String, PathBuf>,
    open: Vec<(String, Arc<me_assets::Package>)>,
    shapes: HashMap<String, Option<Arc<MeShape>>>,
}

static INSTALL: Mutex<Option<Install>> = Mutex::new(None);
const OPEN_MOST: usize = 3;

/// Read Mirror's Edge from here (its folder, or its `TdGame/CookedPC`), rather than where
/// [`find_install`] looks.
pub fn set_install(dir: &Path) {
    let cooked = me_assets::cooked_pc(dir);
    if let Ok(mut i) = INSTALL.lock() {
        if i.as_ref().is_some_and(|i| i.cooked == cooked) {
            return;
        }
        *i = Some(Install::at(cooked));
    }
}

/// Mirror's Edge's install: `FAITH_ME_DIR`, `me_path.txt` (here or at the repository's top), or
/// Steam's.
pub fn find_install() -> Option<PathBuf> {
    let mut tries: Vec<PathBuf> = std::env::var_os("FAITH_ME_DIR").map(PathBuf::from).into_iter().collect();
    for f in [PathBuf::from("me_path.txt"), Path::new(env!("CARGO_MANIFEST_DIR")).join("../../me_path.txt")] {
        if let Ok(s) = std::fs::read_to_string(f) {
            tries.push(PathBuf::from(s.trim()));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        for steam in [".local/share/Steam", ".steam/steam", ".var/app/com.valvesoftware.Steam/.local/share/Steam"] {
            tries.push(Path::new(&home).join(steam).join("steamapps/common/mirrors edge"));
        }
    }
    for s in [r"C:\Program Files (x86)\Steam\steamapps\common", r"C:\Program Files\Steam\steamapps\common"] {
        tries.push(Path::new(s).join("mirrors edge"));
    }
    tries.into_iter().find_map(|p| me_assets::cooked_pc(&p))
}

impl Install {
    fn at(cooked: Option<PathBuf>) -> Install {
        let mut files = HashMap::new();
        let mut stack: Vec<PathBuf> = cooked.iter().cloned().collect();
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    files.entry(stem.to_ascii_lowercase()).or_insert(p);
                }
            }
        }
        Install { cooked, files, ..Default::default() }
    }

    fn package(&mut self, name: &str) -> Option<Arc<me_assets::Package>> {
        let key = name.to_ascii_lowercase();
        if let Some(k) = self.open.iter().position(|(n, _)| *n == key) {
            let p = self.open.remove(k);
            self.open.push(p.clone());
            return Some(p.1);
        }
        let p = Arc::new(me_assets::Package::open(self.files.get(&key)?).ok()?);
        if self.open.len() >= OPEN_MOST {
            self.open.remove(0);
        }
        self.open.push((key, p.clone()));
        Some(p)
    }
}

/// What mesh `mesh` in package `package` collides with, read from the install (once), if the
/// game's there and the mesh has anything to collide with.
pub fn shape(package: &str, mesh: &str) -> Option<Arc<MeShape>> {
    let mut guard = INSTALL.lock().ok()?;
    let install = guard.get_or_insert_with(|| Install::at(find_install()));
    let key = format!("{package}.{mesh}");
    if let Some(s) = install.shapes.get(&key) {
        return s.clone();
    }
    let read = install.package(package).and_then(|pkg| {
        // (A level's BSP: its solid faces.)
        if mesh == me_assets::bsp::BSP {
            let tris = me_assets::bsp::Bsp::read(&pkg).ok()?.collision_tris();
            let s = MeShape { hulls: vec![], tris: tris.iter().map(|t| t.map(ue)).collect() };
            return (!s.is_empty()).then(|| Arc::new(s));
        }
        let i = pkg.exports.iter().position(|e| e.name == mesh && pkg.class_name(e) == "StaticMesh")?;
        let s = MeShape::from_game(&map::mesh_collision(&pkg, i));
        (!s.is_empty()).then(|| Arc::new(s))
    });
    install.shapes.insert(key, read.clone());
    read
}

/// Whether the game's there to read shapes from.
pub fn installed() -> bool {
    INSTALL.lock().ok().is_some_and(|mut g| g.get_or_insert_with(|| Install::at(find_install())).cooked.is_some())
}

/// How thick a triangle is to collide with (behind its face).
const SKIN: f32 = 0.05;

/// A hull's planes as a brush (nothing drawn on it).
pub fn hull_brush(planes: &[Vec4]) -> Brush {
    brush(planes.iter().copied())
}

fn brush(planes: impl IntoIterator<Item = Vec4>) -> Brush {
    let faces = planes
        .into_iter()
        .map(|p| Face { n: p.truncate(), d: p.w, mat: String::new(), u: Vec4::ZERO, v: Vec4::ZERO, noportal: false, see_through: false })
        .collect();
    Brush { contents: Contents::Solid, faces }
}

/// A triangle as a thin solid: its face, and `SKIN` behind it.
fn slab(t: &[Vec3; 3]) -> Option<Brush> {
    let n = (t[1] - t[0]).cross(t[2] - t[0]);
    if n.length_squared() < 1e-10 {
        return None;
    }
    let n = n.normalize();
    let d = n.dot(t[0]);
    let mut planes = vec![n.extend(d), (-n).extend(-(d - SKIN))];
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        let side = (b - a).cross(n);
        if side.length_squared() < 1e-12 {
            return None;
        }
        let side = side.normalize();
        planes.push(side.extend(side.dot(a)));
    }
    Some(brush(planes))
}

/// At most this many boxes for a slanted triangle (it's slices of it, merged).
const TRI_BOXES: usize = 8;
/// Slanted faces smaller than this (m²) don't take portals (none would fit).
const MIN_SLOPE: f32 = 0.3;

fn merged(boxes: Vec<Aabb>, most: usize) -> Vec<Aabb> {
    if boxes.len() <= most {
        return boxes;
    }
    let per = boxes.len().div_ceil(most);
    boxes.chunks(per).map(|c| c.iter().skip(1).fold(c[0], |a, b| Aabb::new(a.min.min(b.min), a.max.max(b.max)))).collect()
}

/// A box to keep: a real one (a shape that doesn't close makes boxes out to infinity).
fn real(b: &Aabb) -> bool {
    b.min.is_finite() && b.max.is_finite() && (b.max - b.min).max_element() < 1000.0
}

/// Placed shapes' solids, by what's placed where: rebuilding a level after moving one piece
/// (the editor does at every change) only works out that piece's again.
type Placed = Arc<(Vec<Solid>, Vec<Slope>)>;
static PLACED: Mutex<Option<HashMap<(String, [u32; 10], u8), Placed>>> = Mutex::new(None);
const PLACED_MOST: usize = 20_000;

/// [`MeShape::place`], kept: `key` names the shape (its mesh).
pub fn place_kept(key: &str, shape: &MeShape, at: Vec3, turn: Quat, scale: Vec3, mat: Mat, solids: &mut Vec<Solid>, slopes: &mut Vec<Slope>) {
    let bits = [at.x, at.y, at.z, turn.x, turn.y, turn.z, turn.w, scale.x, scale.y, scale.z].map(f32::to_bits);
    let k = (key.to_string(), bits, mat as u8);
    let found = PLACED.lock().ok().and_then(|m| m.as_ref().and_then(|m| m.get(&k).cloned()));
    let placed = found.unwrap_or_else(|| {
        let (mut s, mut sl) = (Vec::new(), Vec::new());
        shape.place(at, turn, scale, mat, &mut s, &mut sl);
        let p = Arc::new((s, sl));
        if let Ok(mut m) = PLACED.lock() {
            let m = m.get_or_insert_with(HashMap::new);
            if m.len() >= PLACED_MOST {
                m.clear();
            }
            m.insert(k, p.clone());
        }
        p
    });
    solids.extend_from_slice(&placed.0);
    slopes.extend_from_slice(&placed.1);
}

impl MeShape {
    /// Its solids where a mesh stands (`at`, turned `turn`, scaled `scale` along its own
    /// axes): boxes of `mat`, and the slanted faces that take portals.
    pub fn place(&self, at: Vec3, turn: Quat, scale: Vec3, mat: Mat, solids: &mut Vec<Solid>, slopes: &mut Vec<Slope>) {
        let place = |b: Brush| b.scaled(scale).turned(turn).moved(at);
        let slanted = |b: &Brush, faces: std::ops::Range<usize>, slopes: &mut Vec<Slope>| {
            if !mat.portalable() {
                return;
            }
            for i in faces {
                let f = &b.faces[i];
                if f.n.abs().max_element() > 0.999 {
                    continue;
                }
                let s = Slope { normal: f.n.normalize(), pts: polygon(&b.faces, i) };
                if s.pts.len() >= 3 && s.area() >= MIN_SLOPE {
                    slopes.push(s);
                }
            }
        };
        for h in &self.hulls {
            let b = place(brush(h.iter().copied()));
            solids.extend(b.boxes().into_iter().filter(real).map(|b| Solid { b, mat }));
            slanted(&b, 0..b.faces.len(), slopes);
        }
        for t in &self.tris {
            let Some(b) = slab(t) else { continue };
            let b = place(b);
            let boxes = if b.axial() { b.bounds().into_iter().collect() } else { merged(b.boxes(), TRI_BOXES) };
            solids.extend(boxes.into_iter().filter(real).map(|b| Solid { b, mat }));
            // (Its face, not its edges or back.)
            slanted(&b, 0..1, slopes);
        }
    }
}
