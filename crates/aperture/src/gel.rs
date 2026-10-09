//! Portal 2's gels: blobs that fly, and the paint they leave on surfaces.
//!
//! Paint lives on a grid laid over every flat surface: [`CELL`]-sized squares on each plane
//! (which way it faces, and where it is along that axis), each holding one paint. A splat of
//! gel paints the cells within its radius; painting over paint replaces it, and cleansing
//! water clears it.

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use crate::level::Paint;

/// The paint grid's cell size (metres).
pub const CELL: f32 = 0.25;
/// Surfaces are told apart along their normal to this (metres).
const PLANE_STEP: f32 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Splat {
    pub p: Vec3,
    /// The surface's normal (axis-aligned).
    pub n: Vec3,
    pub r: f32,
    pub paint: Paint,
}

/// Which way a surface faces: 0..6 for +X, -X, +Y, -Y, +Z, -Z.
fn face_of(n: Vec3) -> u8 {
    let a = n.abs();
    if a.x >= a.y && a.x >= a.z {
        if n.x >= 0.0 { 0 } else { 1 }
    } else if a.y >= a.z {
        if n.y >= 0.0 { 2 } else { 3 }
    } else if n.z >= 0.0 {
        4
    } else {
        5
    }
}

fn axes(face: u8) -> (usize, usize, usize) {
    // (normal axis, u axis, v axis)
    match face / 2 {
        0 => (0, 2, 1),
        1 => (1, 0, 2),
        _ => (2, 0, 1),
    }
}

/// A cell: face, the plane's place along the normal (in PLANE_STEPs), and its square.
pub type CellKey = (u8, i32, i32, i32);

#[derive(Clone, Debug, Default)]
pub struct GelMap {
    pub cells: HashMap<CellKey, Paint>,
    /// Bumped whenever the paint changes (for redrawing it).
    pub version: u64,
}

impl GelMap {
    pub fn clear(&mut self) {
        self.cells.clear();
        self.version += 1;
    }

    fn key(p: Vec3, n: Vec3) -> CellKey {
        let f = face_of(n);
        let (a, u, v) = axes(f);
        ((f), (p[a] / PLANE_STEP).round() as i32, (p[u] / CELL).floor() as i32, (p[v] / CELL).floor() as i32)
    }

    /// The middle of a cell, on its surface, and the surface's normal.
    pub fn cell_center(k: &CellKey) -> (Vec3, Vec3) {
        let (f, plane, i, j) = *k;
        let (a, u, v) = axes(f);
        let mut c = Vec3::ZERO;
        c[a] = plane as f32 * PLANE_STEP;
        c[u] = (i as f32 + 0.5) * CELL;
        c[v] = (j as f32 + 0.5) * CELL;
        let mut n = Vec3::ZERO;
        n[a] = if f % 2 == 0 { 1.0 } else { -1.0 };
        (c, n)
    }

    /// The in-plane axes of a cell's surface.
    pub fn cell_axes(k: &CellKey) -> (Vec3, Vec3) {
        let (_, u, v) = axes(k.0);
        let (mut uu, mut vv) = (Vec3::ZERO, Vec3::ZERO);
        uu[u] = 1.0;
        vv[v] = 1.0;
        (uu, vv)
    }

    /// Paint a splat: every cell whose middle is within its radius.
    pub fn add(&mut self, s: Splat) {
        let f = face_of(s.n);
        let (a, u, v) = axes(f);
        let plane = (s.p[a] / PLANE_STEP).round() as i32;
        let r = s.r;
        let (i0, i1) = (((s.p[u] - r) / CELL).floor() as i32, ((s.p[u] + r) / CELL).floor() as i32);
        let (j0, j1) = (((s.p[v] - r) / CELL).floor() as i32, ((s.p[v] + r) / CELL).floor() as i32);
        let mut changed = false;
        for i in i0..=i1 {
            for j in j0..=j1 {
                let cu = (i as f32 + 0.5) * CELL - s.p[u];
                let cv = (j as f32 + 0.5) * CELL - s.p[v];
                if cu * cu + cv * cv > r * r {
                    continue;
                }
                let k = (f, plane, i, j);
                if s.paint == Paint::Water {
                    changed |= self.cells.remove(&k).is_some();
                } else if self.cells.insert(k, s.paint) != Some(s.paint) {
                    changed = true;
                }
            }
        }
        if changed {
            self.version += 1;
        }
    }

    /// The paint at `p` on a surface facing `n`.
    pub fn paint_at(&self, p: Vec3, n: Vec3) -> Option<Paint> {
        if self.cells.is_empty() {
            return None;
        }
        let (f, plane, i, j) = Self::key(p, n);
        // The point may sit a little off the surface (a body resting on it, rounding).
        for dp in [0, -1, 1] {
            if let Some(&paint) = self.cells.get(&(f, plane + dp, i, j)) {
                return Some(paint);
            }
        }
        None
    }

    /// The gel at a point, for the movement.
    pub fn gel_at(&self, p: Vec3, n: Vec3) -> faith_move::Gel {
        self.paint_at(p, n).map(|p| p.gel()).unwrap_or_default()
    }

    /// Fill a rectangle: `c` its middle, `u` and `v` its in-plane axes, `half` its half sizes.
    pub fn fill(&mut self, c: Vec3, n: Vec3, u: Vec3, v: Vec3, half: Vec2, paint: Paint) {
        let step = CELL * 0.5;
        let nu = ((half.x * 2.0) / step).ceil().max(1.0) as i32;
        let nv = ((half.y * 2.0) / step).ceil().max(1.0) as i32;
        for i in 0..=nu {
            for j in 0..=nv {
                let a = -half.x + (i as f32 / nu as f32) * half.x * 2.0;
                let b = -half.y + (j as f32 / nv as f32) * half.y * 2.0;
                // Inset a hair so cells on the boundary belong to the rectangle.
                let p = c + u * a.clamp(-half.x + 0.01, half.x - 0.01) + v * b.clamp(-half.y + 0.01, half.y - 0.01);
                let k = Self::key(p, n);
                self.cells.insert(k, paint);
            }
        }
        self.version += 1;
    }
}

/// A blob of gel in flight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blob {
    pub p: Vec3,
    pub v: Vec3,
    pub paint: Paint,
    pub r: f32,
    pub age: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_paint_wins() {
        let mut g = GelMap::default();
        g.add(Splat { p: Vec3::ZERO, n: Vec3::Y, r: 1.0, paint: Paint::Bounce });
        assert_eq!(g.paint_at(Vec3::new(0.5, 0.0, 0.0), Vec3::Y), Some(Paint::Bounce));
        assert_eq!(g.paint_at(Vec3::new(0.5, 0.03, 0.0), Vec3::Y), Some(Paint::Bounce), "just above it");
        assert_eq!(g.paint_at(Vec3::new(0.5, 0.0, 0.0), Vec3::X), None, "wrong facing");
        assert_eq!(g.paint_at(Vec3::new(1.5, 0.0, 0.0), Vec3::Y), None, "outside");
        assert_eq!(g.paint_at(Vec3::new(0.5, 1.0, 0.0), Vec3::Y), None, "another floor");
        g.add(Splat { p: Vec3::new(0.4, 0.0, 0.0), n: Vec3::Y, r: 0.5, paint: Paint::Speed });
        assert_eq!(g.paint_at(Vec3::new(0.5, 0.0, 0.0), Vec3::Y), Some(Paint::Speed));
        g.add(Splat { p: Vec3::new(0.4, 0.0, 0.0), n: Vec3::Y, r: 0.5, paint: Paint::Water });
        assert_eq!(g.paint_at(Vec3::new(0.5, 0.0, 0.0), Vec3::Y), None, "washed off");
        assert_eq!(g.paint_at(Vec3::new(-0.8, 0.0, 0.0), Vec3::Y), Some(Paint::Bounce));
    }

    #[test]
    fn a_filled_rectangle_covers_its_corners() {
        let mut g = GelMap::default();
        g.fill(Vec3::new(0.0, 0.0, 0.0), Vec3::Y, Vec3::X, Vec3::Z, Vec2::new(2.0, 1.0), Paint::Speed);
        for p in [Vec3::new(-1.95, 0.0, -0.95), Vec3::new(1.95, 0.0, 0.95), Vec3::ZERO] {
            assert_eq!(g.paint_at(p, Vec3::Y), Some(Paint::Speed), "{p}");
        }
        assert_eq!(g.paint_at(Vec3::new(2.4, 0.0, 0.0), Vec3::Y), None);
    }
}
