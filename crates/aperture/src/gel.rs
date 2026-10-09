//! Portal 2's gels: blobs that fly, and the paint they leave on surfaces.
//!
//! Paint is a list of round splats, each flat on a surface (a point, the surface's normal, a
//! radius). The newest splat covering a point decides what's there, so painting over paint
//! replaces it, and cleansing water paints "nothing".

use std::collections::HashMap;

use glam::Vec3;

use crate::level::Paint;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Splat {
    pub p: Vec3,
    /// The surface's normal (axis-aligned).
    pub n: Vec3,
    pub r: f32,
    pub paint: Paint,
}

/// How far a point may be off a splat's surface and still be on it.
const ON_SURFACE: f32 = 0.12;
const CELL: f32 = 2.0;
/// Most splats kept; past it the oldest go.
const MAX_SPLATS: usize = 12000;

#[derive(Clone, Debug, Default)]
pub struct GelMap {
    pub splats: Vec<Splat>,
    /// Splat indices by grid cell (every cell a splat's disc touches).
    grid: HashMap<(i32, i32, i32), Vec<u32>>,
    /// Bumped whenever the paint changes (for redrawing it).
    pub version: u64,
}

fn cell_of(p: Vec3) -> (i32, i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32, (p.z / CELL).floor() as i32)
}

impl GelMap {
    pub fn clear(&mut self) {
        self.splats.clear();
        self.grid.clear();
        self.version += 1;
    }

    /// Paint a splat. A splat of the same paint almost on top of an existing one adds
    /// nothing.
    pub fn add(&mut self, s: Splat) {
        if self.covers_same(&s) {
            return;
        }
        if self.splats.len() >= MAX_SPLATS {
            let keep: Vec<Splat> = self.splats.split_off(MAX_SPLATS / 4);
            self.splats = keep;
            self.rebuild();
        }
        let i = self.splats.len() as u32;
        self.splats.push(s);
        self.index(i);
        self.version += 1;
    }

    fn covers_same(&self, s: &Splat) -> bool {
        // The newest splat at its centre is the same paint and reaches well past it.
        self.query(s.p, s.n).is_some_and(|(idx, paint)| paint == s.paint && {
            let o = &self.splats[idx];
            o.p.distance(s.p) + s.r * 0.55 < o.r
        })
    }

    fn index(&mut self, i: u32) {
        let s = self.splats[i as usize];
        let lo = cell_of(s.p - Vec3::splat(s.r));
        let hi = cell_of(s.p + Vec3::splat(s.r));
        for x in lo.0..=hi.0 {
            for y in lo.1..=hi.1 {
                for z in lo.2..=hi.2 {
                    self.grid.entry((x, y, z)).or_default().push(i);
                }
            }
        }
    }

    fn rebuild(&mut self) {
        self.grid.clear();
        for i in 0..self.splats.len() as u32 {
            self.index(i);
        }
        self.version += 1;
    }

    /// The newest splat covering `p` on a surface facing `n`: its index and paint.
    pub fn query(&self, p: Vec3, n: Vec3) -> Option<(usize, Paint)> {
        let ids = self.grid.get(&cell_of(p))?;
        ids.iter().rev().map(|&i| (i as usize, &self.splats[i as usize])).find_map(|(i, s)| {
            let d = p - s.p;
            let off = d.dot(s.n);
            let lateral = (d - s.n * off).length();
            (s.n.dot(n) > 0.9 && off.abs() < ON_SURFACE && lateral <= s.r).then_some((i, s.paint))
        })
    }

    /// The paint at `p` on a surface facing `n` (water counts as none).
    pub fn paint_at(&self, p: Vec3, n: Vec3) -> Option<Paint> {
        match self.query(p, n) {
            Some((_, Paint::Water)) | None => None,
            Some((_, paint)) => Some(paint),
        }
    }

    /// The gel at a point, for the movement.
    pub fn gel_at(&self, p: Vec3, n: Vec3) -> faith_move::Gel {
        self.paint_at(p, n).map(|p| p.gel()).unwrap_or_default()
    }

    /// Paint under a box's bottom face (anywhere under it), for whoever stands there.
    pub fn gel_under(&self, feet: Vec3, half: f32) -> faith_move::Gel {
        let mut best = faith_move::Gel::None;
        for (dx, dz) in [(0.0, 0.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let g = self.gel_at(feet + Vec3::new(dx * half, 0.0, dz * half), Vec3::Y);
            if g != faith_move::Gel::None {
                best = g;
                if dx == 0.0 {
                    break;
                }
            }
        }
        best
    }

    /// Fill a rectangle with splats: `c` its middle, `u` and `v` its in-plane axes, `half`
    /// its half sizes.
    pub fn fill(&mut self, c: Vec3, n: Vec3, u: Vec3, v: Vec3, half: glam::Vec2, paint: Paint) {
        let r = 0.55;
        let step = 0.6;
        let nu = ((half.x * 2.0) / step).ceil().max(1.0) as i32;
        let nv = ((half.y * 2.0) / step).ceil().max(1.0) as i32;
        for i in 0..=nu {
            for j in 0..=nv {
                let a = -half.x + (i as f32 / nu as f32) * half.x * 2.0;
                let b = -half.y + (j as f32 / nv as f32) * half.y * 2.0;
                let p = c + u * a + v * b;
                self.splats.push(Splat { p, n, r, paint });
            }
        }
        self.rebuild();
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
        assert_eq!(g.paint_at(Vec3::new(0.5, 0.0, 0.0), Vec3::X), None, "wrong facing");
        assert_eq!(g.paint_at(Vec3::new(1.5, 0.0, 0.0), Vec3::Y), None, "outside");
        g.add(Splat { p: Vec3::new(0.4, 0.0, 0.0), n: Vec3::Y, r: 0.5, paint: Paint::Speed });
        assert_eq!(g.paint_at(Vec3::new(0.5, 0.0, 0.0), Vec3::Y), Some(Paint::Speed));
        g.add(Splat { p: Vec3::new(0.4, 0.0, 0.0), n: Vec3::Y, r: 0.5, paint: Paint::Water });
        assert_eq!(g.paint_at(Vec3::new(0.5, 0.0, 0.0), Vec3::Y), None, "washed off");
        assert_eq!(g.paint_at(Vec3::new(-0.8, 0.0, 0.0), Vec3::Y), Some(Paint::Bounce));
    }
}
