//! Mirror's Edge's levels, as the game's own (see `portal_extract --port-me`): their meshes are
//! [`Kind::MeProp`](crate::doc::Kind::MeProp)s, drawn from the player's install, and what each
//! mesh collides with is kept in the level ([`MeShape`]), so the level plays (and its pieces
//! can be moved) without reading the game.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use glam::{Quat, Vec3, Vec4};
use serde::{Deserialize, Serialize};

use crate::brush::{polygon, Brush, Contents, Face};
use faith_move::Aabb;

use crate::level::{Mat, Slope, Solid};

/// Mirror's Edge's maps, in order: the persistent level, the game's file for it
/// (`levels/me/<file>.ron`), and what it's called.
pub const MAPS: &[(&str, &str, &str)] = &[
    ("Tutorial_p", "tutorial", "Mirror's Edge: Training"),
    ("TT_TutorialA01_p", "tt_tutorial_a01", "Mirror's Edge: Time trial, Training A1"),
    ("TT_TutorialA02_p", "tt_tutorial_a02", "Mirror's Edge: Time trial, Training A2"),
    ("TT_TutorialA03_p", "tt_tutorial_a03", "Mirror's Edge: Time trial, Training A3"),
];

/// What a Mirror's Edge mesh collides with, in its own space (ours: metres, before it's
/// placed): convex hulls, each its faces' planes (`n·p = d`, facing out: `(n, d)`), or, for a
/// mesh that has none, its triangles.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MeShape {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hulls: Vec<Vec<Vec4>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tris: Vec<[Vec3; 3]>,
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
