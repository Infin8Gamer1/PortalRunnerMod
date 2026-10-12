//! Portal's maps made into level files ([`LevelDoc`]): the game's own copy of each map, to
//! play and to change in the level editor. Made from what `portal_extract` writes out for a
//! map: its brushes as they were built (`.brushes`), its entities (`.ents`), its props
//! (`.props`) and its overlays (`.overlays`).
//!
//! Each of the map's brushes is a brush object, each displacement and overlay one of its own,
//! and each entity an entity object: its class, keys and outputs as the map has them, its name
//! the object's, a brush entity's brushes its own. A static prop (not an entity in the map:
//! it's compiled in) is a `prop_static` entity, as in Hammer. What the extractor works out
//! from the game's files besides is kept in keys of its own: a sprite's texture width
//! (`_width`), how far a physics prop's lowest point is below its origin (`_bottom`). Built,
//! the copy makes the same level as the map does ([`crate::source`]), and is drawn the same.

use std::collections::HashMap;

use glam::{Vec3, Vec4};

use crate::brush::{Brush, Contents, Displacement, Face};
use crate::doc::{Kind, LevelDoc, Object};
use crate::io::Output;
use crate::source::{self, key};

/// Portal's maps, in order, with what the game calls them.
pub const MAPS: &[(&str, &str)] = &[
    ("testchmb_a_00", "Portal: Chambers 00-01"),
    ("testchmb_a_01", "Portal: Chambers 02-03"),
    ("testchmb_a_02", "Portal: Chambers 04-05"),
    ("testchmb_a_03", "Portal: Chambers 06-07"),
    ("testchmb_a_04", "Portal: Chamber 08"),
    ("testchmb_a_05", "Portal: Chamber 09"),
    ("testchmb_a_06", "Portal: Chamber 10"),
    ("testchmb_a_07", "Portal: Chambers 11-12"),
    ("testchmb_a_08", "Portal: Chamber 13"),
    ("testchmb_a_09", "Portal: Chamber 14"),
    ("testchmb_a_10", "Portal: Chamber 15"),
    ("testchmb_a_11", "Portal: Chamber 16"),
    ("testchmb_a_13", "Portal: Chamber 17"),
    ("testchmb_a_14", "Portal: Chamber 18"),
    ("testchmb_a_15", "Portal: Chamber 19"),
    ("escape_00", "Portal: Escape 1"),
    ("escape_01", "Portal: Escape 2"),
    ("escape_02", "Portal: Escape 3"),
];

/// What the game's own copy of a map is called (the map's own name is the imported map's).
pub fn native_title(title: &str) -> String {
    match title.strip_prefix("Portal: ") {
        Some(rest) => format!("Portal native: {rest}"),
        None => format!("{title} (native)"),
    }
}

/// A map's files, as `portal_extract` writes them.
pub struct MapFiles<'a> {
    pub brushes: &'a str,
    pub ents: &'a str,
    pub props: &'a str,
    pub overlays: &'a str,
}

/// A map's brushes file, read.
#[derive(Default)]
pub struct BrushFile {
    /// Each material's flags (as the mesh file's: 16, takes portals).
    pub materials: HashMap<String, u8>,
    /// Each brush, with its brush model (0: the world), where it is in the map.
    pub brushes: Vec<(usize, Brush)>,
    pub displacements: Vec<Displacement>,
}

/// Whether a material takes portals, by its flags (an unknown one does: Source's tool textures
/// say nothing).
fn takes_portals(materials: &HashMap<String, u8>, mat: &str) -> bool {
    materials.get(mat).is_none_or(|f| f & 16 != 0)
}

/// Whether a material can be seen through (translucent, alpha-tested, additive; glass).
pub fn see_through(materials: &HashMap<String, u8>, mat: &str) -> bool {
    mat.starts_with("glass/") || materials.get(mat).is_some_and(|f| f & (1 | 2 | 8) != 0)
}

fn floats(v: &str) -> Vec<f32> {
    v.split_whitespace().filter_map(|x| x.parse().ok()).collect()
}

/// Read a map's `.brushes` (see `portal_extract`'s `maps.rs`).
pub fn parse_brushes(text: &str) -> BrushFile {
    let mut out = BrushFile::default();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        match word {
            "mat" => {
                if let Some((flags, name)) = rest.split_once(' ') {
                    out.materials.insert(name.to_string(), flags.parse().unwrap_or(0));
                }
            }
            "brush" => {
                let v: Vec<i64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                let (model, contents) = (v.first().copied().unwrap_or(0) as usize, v.get(1).copied().unwrap_or(1) as i32);
                let mut faces = Vec::new();
                while let Some(side) = lines.next_if(|l| l.starts_with("side ")) {
                    let f: Vec<&str> = side.split_whitespace().collect();
                    if f.len() < 14 {
                        continue;
                    }
                    let n: Vec<f32> = f[1..13].iter().filter_map(|x| x.parse().ok()).collect();
                    if n.len() < 12 {
                        continue;
                    }
                    let mat = f[13..].join(" ");
                    faces.push(Face { n: Vec3::new(n[0], n[1], n[2]), d: n[3], u: Vec4::new(n[4], n[5], n[6], n[7]), v: Vec4::new(n[8], n[9], n[10], n[11]), noportal: false, see_through: false, mat });
                }
                if faces.len() >= 4 {
                    out.brushes.push((model, Brush { contents: Contents::from_bits(contents), faces }));
                }
            }
            "disp" => {
                let f: Vec<&str> = rest.split_whitespace().collect();
                let n: Vec<f32> = f.iter().take(21).filter_map(|x| x.parse().ok()).collect();
                let offsets = lines.next_if(|l| l.starts_with("offsets")).map(|l| floats(&l[7..])).unwrap_or_default();
                if n.len() < 21 || f.len() < 22 {
                    continue;
                }
                let corners = std::array::from_fn(|k| Vec3::new(n[1 + k * 3], n[2 + k * 3], n[3 + k * 3]));
                out.displacements.push(Displacement {
                    mat: f[21..].join(" "),
                    power: n[0] as u32,
                    corners,
                    u: Vec4::new(n[13], n[14], n[15], n[16]),
                    v: Vec4::new(n[17], n[18], n[19], n[20]),
                    offsets: offsets.chunks_exact(3).map(|c| Vec3::new(c[0], c[1], c[2])).collect(),
                    noportal: false,
                });
            }
            _ => {}
        }
    }
    let table = out.materials.clone();
    for (_, b) in &mut out.brushes {
        for f in &mut b.faces {
            f.noportal = !takes_portals(&table, &f.mat);
            f.see_through = see_through(&table, &f.mat);
        }
    }
    for d in &mut out.displacements {
        d.noportal = !takes_portals(&table, &d.mat);
    }
    out
}

/// Rounded to a hundred-thousandth (no noise in the file).
fn tidy(x: f32) -> f32 {
    let x = (x * 100_000.0).round() / 100_000.0;
    if x == 0.0 { 0.0 } else { x }
}

/// A normal without negative zeros (`-0.0` in the file).
fn unsigned_zeros(v: Vec3) -> Vec3 {
    v + Vec3::ZERO
}

fn tidy3(v: Vec3) -> Vec3 {
    Vec3::new(tidy(v.x), tidy(v.y), tidy(v.z))
}

fn tidy4(v: Vec4) -> Vec4 {
    Vec4::new(tidy(v.x), tidy(v.y), tidy(v.z), tidy(v.w))
}

/// A brush moved so that `at` is its origin, its numbers tidied.
fn relative(b: &Brush, at: Vec3) -> Brush {
    let mut b = b.moved(-at);
    for f in &mut b.faces {
        f.n = unsigned_zeros(f.n);
        f.d = tidy(f.d);
        f.u = tidy4(f.u);
        f.v = tidy4(f.v);
    }
    b
}

/// Where an object of something's box stands: its middle (to a tenth of a millimetre).
fn middle(lo: Vec3, hi: Vec3) -> Vec3 {
    ((lo + hi) * 0.5 * 10_000.0).round() / 10_000.0
}

/// A `.props` line, as the converter needs it.
struct PropLine {
    kind: String,
    model: String,
    /// Source's (the line's own).
    origin: Vec3,
    angles: Vec3,
    name: String,
    /// The rest: a physics prop's bottom; a sprite's size, colour...
    extra: Vec<f32>,
    used: bool,
}

fn parse_prop_lines(text: &str) -> Vec<PropLine> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('|').collect();
            if f.len() < 6 {
                return None;
            }
            let v = |s: &str| {
                let x = floats(s);
                Vec3::new(x.first().copied().unwrap_or(0.0), x.get(1).copied().unwrap_or(0.0), x.get(2).copied().unwrap_or(0.0))
            };
            let extra = if f[0] == "sprite" { floats(f[3]) } else { f.get(6).map(|b| floats(b)).unwrap_or_default() };
            Some(PropLine { kind: f[0].to_string(), model: f[1].to_string(), origin: v(f[2]), angles: if f[0] == "sprite" { Vec3::ZERO } else { v(f[3]) }, name: f[5].to_ascii_lowercase(), extra, used: false })
        })
        .collect()
}

/// Keys the converter makes into the object's own (its class, name, point and turn).
fn own_key(k: &str) -> bool {
    ["classname", "targetname", "origin", "angles", "hammerid"].iter().any(|o| k.eq_ignore_ascii_case(o))
}

/// Map `map` (what the game calls it: `title`) as a level file.
pub fn convert(map: &str, title: &str, files: &MapFiles) -> LevelDoc {
    let bf = parse_brushes(files.brushes);
    let ents = source::parse_entities(files.ents);
    let mut props = parse_prop_lines(files.props);
    let mut doc = LevelDoc::new(title);
    doc.map = Some(map.to_string());

    // The world's brushes, each where its box's middle is.
    let mut by_model: HashMap<usize, Vec<&Brush>> = HashMap::new();
    for (m, b) in &bf.brushes {
        if *m == 0 {
            let Some(bx) = b.bounds() else { continue };
            let at = middle(bx.min, bx.max);
            doc.objects.push(Object::new(Kind::Brush(relative(b, at)), at));
        } else {
            by_model.entry(*m).or_default().push(b);
        }
    }
    for d in &bf.displacements {
        let (lo, hi) = d.corners.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, z), c| (a.min(*c), z.max(*c)));
        let at = middle(lo, hi);
        let shift = |v: Vec4| tidy4(Vec4::from((v.truncate(), v.w + v.truncate().dot(at))));
        let rel = Displacement { corners: d.corners.map(|c| tidy3(c - at)), u: shift(d.u), v: shift(d.v), ..d.clone() };
        doc.objects.push(Object::new(Kind::Displacement(rel), at));
    }

    // Its entities, in order.
    for e in &ents {
        let class = key(e, "classname").unwrap_or("");
        if class.is_empty() || class == "worldspawn" {
            continue;
        }
        let origin_src = key(e, "origin").map(|v| {
            let x = floats(v);
            Vec3::new(x.first().copied().unwrap_or(0.0), x.get(1).copied().unwrap_or(0.0), x.get(2).copied().unwrap_or(0.0))
        });
        let model = key(e, "model").and_then(|m| m.strip_prefix('*')).and_then(|m| m.parse::<usize>().ok());
        let brushes = model.and_then(|m| by_model.get(&m)).cloned().unwrap_or_default();
        // Where it stands: its origin; a brush entity without one, its brushes' middle.
        let mut at = origin_src.map(source::to_ours).unwrap_or(Vec3::ZERO);
        if origin_src.is_none() && !brushes.is_empty() {
            let (lo, hi) = brushes.iter().filter_map(|b| b.bounds()).fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, z), b| (a.min(b.min), z.max(b.max)));
            if lo.x <= hi.x {
                at = middle(lo, hi);
            }
        }
        let mut keys: Vec<(String, String)> = Vec::new();
        let mut outputs = Vec::new();
        for (k, v) in e {
            if own_key(k) || (k.eq_ignore_ascii_case("model") && model.is_some()) {
                continue;
            }
            if k.starts_with("On") || k.starts_with("Out") {
                if let Some(o) = Output::parse(k, v) {
                    outputs.push(o);
                    continue;
                }
            }
            keys.push((k.clone(), v.clone()));
        }
        let name = key(e, "targetname").unwrap_or("").to_string();
        let lname = name.to_ascii_lowercase();
        // What the extractor worked out for it: a prop's or sprite's line.
        let near = |p: &PropLine| origin_src.is_some_and(|o| (p.origin - o).length() < 0.05);
        let line_kind = match class {
            "env_sprite" => Some("sprite"),
            c if c.starts_with("prop_physics") => Some("physics"),
            "prop_dynamic" | "prop_dynamic_override" | "prop_portal_stats_display" => Some("dynamic"),
            _ => None,
        };
        if let Some(kind) = line_kind {
            let found = props.iter().position(|p| !p.used && p.kind == kind && ((!lname.is_empty() && p.name == lname) || (lname.is_empty() && near(p))));
            if let Some(i) = found {
                let p = &mut props[i];
                p.used = true;
                // (Put on an attachment of what it's parented to: there.)
                if origin_src.is_some_and(|o| (p.origin - o).length() > 0.05) {
                    at = source::to_ours(p.origin);
                }
                match kind {
                    "physics" => {
                        if let Some(b) = p.extra.first() {
                            keys.push(("_bottom".into(), format!("{b}")));
                        }
                    }
                    "sprite" => {
                        let scale = key(e, "scale").and_then(|s| s.trim().parse::<f32>().ok()).filter(|s| *s > 0.0).unwrap_or(1.0);
                        if let Some(size) = p.extra.first() {
                            keys.push(("_width".into(), format!("{}", (size / scale).round())));
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut o = Object::new(Kind::Entity { class: class.to_string(), keys, brushes: brushes.iter().map(|b| relative(b, at)).collect() }, tidy3(at));
        o.name = name;
        o.outputs = outputs;
        if let Some(a) = key(e, "angles") {
            let a = floats(a);
            if a.len() >= 3 && (a[0] != 0.0 || a[1] != 0.0 || a[2] != 0.0) {
                o.set_rotation(source::source_turn(a[0], a[1], a[2]));
            }
        }
        doc.objects.push(o);
    }

    // Its static props, as prop_static entities.
    for p in props.iter().filter(|p| p.kind == "static") {
        let (model, skin) = crate::art::glb_model(&p.model);
        let mut keys = vec![("model".to_string(), model)];
        if skin > 0 {
            keys.push(("skin".into(), skin.to_string()));
        }
        let mut o = Object::new(Kind::Entity { class: "prop_static".into(), keys, brushes: Vec::new() }, tidy3(source::to_ours(p.origin)));
        if p.angles != Vec3::ZERO {
            o.set_rotation(source::source_turn(p.angles.x, p.angles.y, p.angles.z));
        }
        doc.objects.push(o);
    }

    // Its overlays.
    for ov in crate::art::parse_overlays(files.overlays) {
        let (lo, hi) = ov.corners.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, z), (c, _)| (a.min(*c), z.max(*c)));
        let at = middle(lo, hi);
        let mut o = Object::new(Kind::Overlay { mat: ov.mat, corners: ov.corners.map(|(c, _)| tidy3(c - at)), uv: ov.corners.map(|(_, uv)| [tidy(uv[0]), tidy(uv[1])]), normal: tidy3(ov.normal) }, at);
        o.name = ov.name;
        doc.objects.push(o);
    }

    // As the map plays: the gun she starts with, where falling out of it kills her.
    let l = doc.build_map("", &source::Scenes::new());
    doc.blue = l.gun.blue;
    doc.orange = l.gun.orange;
    doc.kill_y = l.kill_y;
    doc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{Item, Level};

    /// The extracted maps there are: (name, the files' text).
    fn maps() -> Vec<(String, [String; 6])> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(&dir) else { return out };
        let mut names: Vec<String> = rd.filter_map(|e| e.ok()?.file_name().into_string().ok()?.strip_suffix(".brushes").map(str::to_string)).collect();
        names.sort();
        for n in names {
            let read = |ext: &str| std::fs::read_to_string(dir.join(format!("{n}.{ext}"))).unwrap_or_default();
            out.push((n.clone(), [read("brushes"), read("ents"), read("props"), read("overlays"), read("solids"), String::new()]));
        }
        out
    }

    /// The boxes of `b` with none of `a` the same (its material, its corners within 2 mm),
    /// and those of `a` with none of `b`.
    fn unmatched(a: &[crate::level::Solid], b: &[crate::level::Solid]) -> Vec<(crate::level::Mat, faith_move::Aabb)> {
        let cell = |p: Vec3| ((p.x * 10.0).floor() as i32, (p.y * 10.0).floor() as i32, (p.z * 10.0).floor() as i32);
        let mut out = Vec::new();
        for (x, y) in [(a, b), (b, a)] {
            let mut grid: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
            for (i, s) in y.iter().enumerate() {
                grid.entry(cell(s.b.min)).or_default().push(i);
            }
            let mut used = vec![false; y.len()];
            for s in x {
                let c = cell(s.b.min);
                // (The closest: an angled brush's slices can be thinner than the tolerance.)
                let mut best: Option<(f32, usize)> = None;
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        for dz in -1..=1 {
                            for &i in grid.get(&(c.0 + dx, c.1 + dy, c.2 + dz)).into_iter().flatten() {
                                let t = &y[i];
                                let off = (t.b.min - s.b.min).abs().max_element().max((t.b.max - s.b.max).abs().max_element());
                                if !used[i] && t.mat == s.mat && off < 0.002 && best.is_none_or(|(o, _)| off < o) {
                                    best = Some((off, i));
                                }
                            }
                        }
                    }
                }
                let found = best.is_some();
                if let Some((_, i)) = best {
                    used[i] = true;
                }
                if !found {
                    out.push((s.mat, s.b));
                }
            }
        }
        out
    }

    fn item_kind(i: &Item) -> String {
        format!("{:?}", std::mem::discriminant(i))
    }

    /// The copies kept in the repository (`levels/portal/`) read, and build into a level with
    /// its logic and a start, without the extracted files.
    #[test]
    fn the_committed_copies_build() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../levels/portal");
        let mut n = 0;
        for (map, title) in MAPS {
            let Ok(text) = std::fs::read_to_string(dir.join(format!("{map}.ron"))) else { continue };
            let doc = LevelDoc::from_ron(&text).unwrap_or_else(|e| panic!("{map}: {e}"));
            assert_eq!(doc.map.as_deref(), Some(*map));
            assert_eq!(doc.name, native_title(title), "{map}");
            let l = doc.build();
            assert!(l.io.as_ref().is_some_and(|io| !io.ents.is_empty()), "{map}: no logic");
            assert!(!l.solids.is_empty() && !l.checkpoints.is_empty(), "{map}");
            assert!(l.art.as_ref().is_some_and(|a| !a.faces.is_empty()), "{map}: nothing drawn");
            n += 1;
        }
        assert_eq!(n, MAPS.len(), "a map's copy is missing from levels/portal");
    }

    /// Each extracted map's copy, saved and read back, makes the level the map makes: the same
    /// boxes, pieces, logic, lights and start.
    #[test]
    fn copies_make_the_maps_levels() {
        for (name, f) in maps() {
            let doc = convert(&name, "Copy", &MapFiles { brushes: &f[0], ents: &f[1], props: &f[2], overlays: &f[3] });
            let text = doc.to_ron();
            let back = LevelDoc::from_ron(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(back.objects.len(), doc.objects.len(), "{name}");
            let ours = back.build();
            let theirs: Level = source::level(&name, "Map", &f[4], &f[1]);
            // Boxes: the same ones (to 2 mm), give or take a few of an angled brush's slices.
            let unmatched = unmatched(&ours.solids, &theirs.solids);
            assert!(unmatched.len() * 200 <= theirs.solids.len().max(1), "{name}: {} boxes against {}: {} not matched ({:?}...)", ours.solids.len(), theirs.solids.len(), unmatched.len(), &unmatched[..unmatched.len().min(4)]);
            assert_eq!(ours.slopes.len(), theirs.slopes.len(), "{name}: slopes");
            // Pieces, in order, and where they are.
            let ki: Vec<String> = ours.items.iter().map(item_kind).collect();
            let kt: Vec<String> = theirs.items.iter().map(item_kind).collect();
            assert_eq!(ki, kt, "{name}: items");
            for (x, y) in ours.items.iter().zip(&theirs.items) {
                if let (Item::Mover(m), Item::Mover(n)) = (x, y) {
                    // (A template turns some as it puts them where they're made: the map slices an
                    // angled brush of one before it's turned, the copy after.)
                    assert_eq!(m.boxes.is_empty(), n.boxes.is_empty(), "{name}: mover {} boxes", m.model);
                    if m.boxes.len() != n.boxes.len() {
                        let (a, b) = (m.boxes.iter().fold(m.boxes[0].b, |x, s| x.union(&s.b)), n.boxes.iter().fold(n.boxes[0].b, |x, s| x.union(&s.b)));
                        assert!((a.min - b.min).length() < 0.05 && (a.max - b.max).length() < 0.05, "{name}: mover {} is {a:?}, not {b:?}", m.model);
                    }
                    assert_eq!(m.path.len(), n.path.len(), "{name}: mover {} path", m.model);
                    assert!((m.shift - n.shift).length() < 0.01, "{name}: mover {} shift", m.model);
                    assert_eq!(m.faces.len(), n.faces.len(), "{name}: mover {} faces", m.model);
                }
            }
            // The logic: the same entities (less the map's own world, and the static props it
            // has as entities), each its kind.
            let (li, lt) = (ours.io.as_ref().unwrap(), theirs.io.as_ref().unwrap());
            let ents = |io: &crate::io::Io| io.ents.iter().filter(|e| e.class != "worldspawn" && e.class != "prop_static").map(|e| (e.name.clone(), e.class.clone(), format!("{:?}", std::mem::discriminant(&e.kind)), e.outputs.len())).collect::<Vec<_>>();
            assert_eq!(ents(li), ents(lt), "{name}: logic");
            assert_eq!(li.signal_count(), lt.signal_count(), "{name}: signals");
            assert_eq!(ours.lights.len(), theirs.lights.len(), "{name}: lights");
            assert_eq!(ours.landmarks.len(), theirs.landmarks.len(), "{name}: landmarks");
            assert_eq!(ours.hidden_models.len(), theirs.hidden_models.len(), "{name}: hidden models");
            assert!((ours.checkpoints[0].spawn - theirs.checkpoints[0].spawn).length() < 0.01, "{name}: start");
            assert_eq!(ours.gun, theirs.gun, "{name}: gun");
            assert!((ours.kill_y - theirs.kill_y).abs() < 0.05, "{name}: kill height");
            // What's drawn: every prop, sprite and overlay the map has.
            let art = ours.art.as_ref().unwrap();
            let theirs_art = crate::art::MapArt::from_files(&name, &f[2], &f[3], &f[1]);
            assert_eq!(art.props.len(), theirs_art.props.len(), "{name}: props");
            assert_eq!(art.overlays.len(), crate::art::parse_overlays(&f[3]).len(), "{name}: overlays");
            assert_eq!(art.ropes.len(), crate::rope::hang(&source::parse_entities(&f[1])).len(), "{name}: ropes");
            assert!(!art.faces.is_empty(), "{name}: faces");
        }
    }
}
