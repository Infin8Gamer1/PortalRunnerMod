//! Brushes: convex solids bounded by planes, each face with a material and how its texture
//! lies on it, as Portal's maps are built (Hammer's brushes, kept in the map's BSP). The
//! level editor keeps a Portal map's brushes as they were built ([`crate::port`]), so they can
//! be moved and retextured; here they're made into faces to draw and boxes to collide with,
//! as `portal_extract` makes them for the map itself: an axis-aligned brush is its box, an
//! angled one slices of it (steps along a ramp, posts along a slanted wall), and an angled face
//! that takes portals keeps its true plane (a [`Slope`]).

use glam::{DVec3, Vec3, Vec4};
use serde::{Deserialize, Serialize};

use faith_move::Aabb;

use crate::level::{Mat, Slope};

/// What's in a brush (Source's contents).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Contents {
    /// Solid: white where most of its faces take portals, else black.
    Solid,
    /// Glass (a window): solid, see-through.
    Window,
    /// Grating: solid to people and cubes, not to shots and lasers.
    Grate,
    /// Deadly goo.
    Goo,
    /// Water (it stops nothing).
    Water,
    /// An invisible wall that only stops Faith and cubes.
    Clip,
    /// Nothing solid (a trigger's volume, an areaportal).
    Empty,
}

impl Contents {
    /// From Source's contents bits, as `portal_extract` reads a brush.
    pub fn from_bits(c: i32) -> Contents {
        if c & 0x10 != 0 {
            Contents::Goo
        } else if c & 0x20 != 0 {
            Contents::Water
        } else if c & 0x2 != 0 {
            Contents::Window
        } else if c & 0x8 != 0 {
            Contents::Grate
        } else if c & 0x1 != 0 {
            Contents::Solid
        } else if c & 0x10000 != 0 {
            Contents::Clip
        } else {
            Contents::Empty
        }
    }
}

/// One face of a brush.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    /// Its plane, facing out of the brush: the points `p` with `p·n = d` (from the brush's
    /// object's point).
    pub n: Vec3,
    pub d: f32,
    /// Its material (Portal's: `metal/metalwall048b`; `tools/...` aren't drawn).
    pub mat: String,
    /// Where its texture lies: a point `p` on it (from the brush's object's point) shows the
    /// texture at `(p·u.xyz + u.w, p·v.xyz + v.w)`, in the texture's widths and heights.
    pub u: Vec4,
    pub v: Vec4,
    /// Its material doesn't take portals.
    #[serde(default, skip_serializing_if = "is_false")]
    pub noportal: bool,
    /// Its material can be seen through (glass, a grate): what's behind it shows.
    #[serde(default, skip_serializing_if = "is_false")]
    pub see_through: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A convex solid: the space behind every one of its faces.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Brush {
    pub contents: Contents,
    pub faces: Vec<Face>,
}

/// Whether a material isn't drawn: Source's tool textures (nodraw, clip, trigger, the sky).
pub fn tool_material(name: &str) -> bool {
    name.starts_with("tools/") || name.contains("toolsskybox")
}

/// Angled brushes are cut into slices at most this thick, across the way their angled sides
/// face (as `portal_extract`'s)...
const SLICE: f32 = 0.25;
/// ...and thin enough that a slope climbs no more than this in one...
const SLICE_RISE: f32 = 0.03;
/// ...but no more than this many.
const MAX_SLICES: usize = 160;
/// How close a corner must be to a plane to be on it (metres).
const ON: f32 = 1e-3;

/// A face's corners as drawn: the polygon its plane makes inside the others, counter-clockwise
/// seen from its front. (Worked out in double precision: a big square cut down to size.)
pub fn polygon(faces: &[Face], i: usize) -> Vec<Vec3> {
    let f = &faces[i];
    let n = f.n.as_dvec3().normalize_or_zero();
    if n == DVec3::ZERO {
        return Vec::new();
    }
    // A big square on the plane...
    let (a, b) = n.any_orthonormal_pair();
    let c = n * f.d as f64;
    let r = 1.0e5;
    let mut poly = vec![c + (a + b) * r, c + (b - a) * r, c - (a + b) * r, c + (a - b) * r];
    // (Counter-clockwise from the front: n·((p1 - p0) x (p2 - p0)) > 0.)
    if (poly[1] - poly[0]).cross(poly[2] - poly[0]).dot(n) < 0.0 {
        poly.reverse();
    }
    // ...cut by every other plane, keeping what's behind it.
    for (j, g) in faces.iter().enumerate() {
        if j == i || poly.is_empty() {
            continue;
        }
        let gn = g.n.as_dvec3();
        // (The same plane, once more: nothing to cut.)
        if gn.dot(n) > 0.99999 && (g.d - f.d).abs() < ON {
            continue;
        }
        poly = clip(&poly, gn, g.d as f64);
    }
    let mut out: Vec<Vec3> = poly.iter().map(|p| p.as_vec3()).collect();
    // Corners on top of each other (where faces meet along an edge) are one.
    out.dedup_by(|p, q| p.distance(*q) < 1e-5);
    if out.len() > 1 && out[0].distance(out[out.len() - 1]) < 1e-5 {
        out.pop();
    }
    if out.len() < 3 { Vec::new() } else { out }
}

/// What's behind the plane `p·n = d` of a polygon.
fn clip(poly: &[DVec3], n: DVec3, d: f64) -> Vec<DVec3> {
    let on = ON as f64;
    let mut out = Vec::with_capacity(poly.len() + 1);
    for k in 0..poly.len() {
        let (p, q) = (poly[k], poly[(k + 1) % poly.len()]);
        let (dp, dq) = (p.dot(n) - d, q.dot(n) - d);
        if dp <= on {
            out.push(p);
        }
        if (dp > on && dq < -on) || (dp < -on && dq > on) {
            out.push(p + (q - p) * (dp / (dp - dq)));
        }
    }
    out
}

impl Brush {
    /// Each face's polygon (empty where a face doesn't show: it's outside the others).
    pub fn polygons(&self) -> Vec<Vec<Vec3>> {
        (0..self.faces.len()).map(|i| polygon(&self.faces, i)).collect()
    }

    /// Whether every face is square to the axes.
    pub fn axial(&self) -> bool {
        self.faces.iter().all(|f| {
            let a = f.n.abs();
            a.max_element() > 0.99999
        })
    }

    /// Its box: quickly, when every face is square to the axes (each axis' two faces), else
    /// from its corners.
    pub fn bounds(&self) -> Option<Aabb> {
        if self.axial() {
            let (mut lo, mut hi) = (Vec3::splat(f32::MIN), Vec3::splat(f32::MAX));
            for f in &self.faces {
                let a = f.n.abs();
                let k = if a.x >= a.y && a.x >= a.z { 0 } else if a.y >= a.z { 1 } else { 2 };
                let s = f.n[k].signum();
                if s > 0.0 {
                    hi[k] = hi[k].min(f.d);
                } else {
                    lo[k] = lo[k].max(-f.d);
                }
            }
            return (lo.cmplt(hi).all()).then(|| Aabb::new(lo, hi));
        }
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for p in self.polygons().into_iter().flatten() {
            lo = lo.min(p);
            hi = hi.max(p);
        }
        (lo.cmplt(hi).all()).then(|| Aabb::new(lo, hi))
    }

    /// The same brush moved by `by` (its textures moving with it).
    pub fn moved(&self, by: Vec3) -> Brush {
        let faces = self
            .faces
            .iter()
            .map(|f| Face { d: f.d + f.n.dot(by), u: Vec4::from((f.u.truncate(), f.u.w - f.u.truncate().dot(by))), v: Vec4::from((f.v.truncate(), f.v.w - f.v.truncate().dot(by))), ..f.clone() })
            .collect();
        Brush { contents: self.contents, faces }
    }

    /// The same brush turned by `q` about the origin (its textures turning with it).
    pub fn turned(&self, q: glam::Quat) -> Brush {
        let faces = self.faces.iter().map(|f| Face { n: q * f.n, u: Vec4::from((q * f.u.truncate(), f.u.w)), v: Vec4::from((q * f.v.truncate(), f.v.w)), ..f.clone() }).collect();
        Brush { contents: self.contents, faces }
    }

    /// The same brush scaled by `s` along each axis about the origin (its textures keep their
    /// size: they don't stretch with it).
    pub fn scaled(&self, s: Vec3) -> Brush {
        let faces = self
            .faces
            .iter()
            .map(|f| {
                // A plane through p (p·n = d) goes through p*s: its normal n/s, scaled to unit.
                let m = f.n / s;
                let len = m.length().max(1e-9);
                Face { n: m / len, d: f.d / len, ..f.clone() }
            })
            .collect();
        Brush { contents: self.contents, faces }
    }

    /// Whether it hides what's inside it from every side: solid, and none of its faces seen
    /// through.
    pub fn opaque(&self) -> bool {
        self.contents == Contents::Solid && !self.faces.iter().any(|f| f.see_through)
    }

    /// Whether every point is inside it (or on it).
    pub fn holds(&self, pts: &[Vec3]) -> bool {
        self.faces.iter().all(|f| pts.iter().all(|p| p.dot(f.n) <= f.d + ON))
    }

    /// What it is to collide with: its material for the boxes ([`Mat`]), goo, or nothing. As
    /// `portal_extract` has it: a solid brush is glass if any face is, white if all its faces
    /// are nodraw (portals stick to what isn't marked otherwise) or at least half of its drawn
    /// faces take portals, else black.
    pub fn collision(&self) -> Collision {
        match self.contents {
            Contents::Goo => Collision::Goo,
            Contents::Water | Contents::Empty => Collision::None,
            Contents::Window => Collision::Solid(Mat::Glass),
            Contents::Grate => Collision::Solid(Mat::Grate),
            Contents::Clip => Collision::Solid(Mat::Clip),
            Contents::Solid => {
                let drawn: Vec<&Face> = self.faces.iter().filter(|f| !tool_material(&f.mat)).collect();
                let nodraw = !self.faces.is_empty() && self.faces.iter().all(|f| f.mat == "tools/toolsnodraw");
                if nodraw {
                    Collision::Solid(Mat::White)
                } else if drawn.iter().any(|f| f.mat.starts_with("glass/")) {
                    Collision::Solid(Mat::Glass)
                } else {
                    let yes = drawn.iter().filter(|f| !f.noportal).count();
                    Collision::Solid(if yes > 0 && yes * 2 >= drawn.len() { Mat::White } else { Mat::Black })
                }
            }
        }
    }

    /// Its boxes to collide with: its box when it's square to the axes, slices of it when
    /// it isn't (as `portal_extract`'s `brush_boxes`).
    pub fn boxes(&self) -> Vec<Aabb> {
        let polys = self.polygons();
        let corners: Vec<Vec3> = polys.iter().flatten().copied().collect();
        if corners.len() < 4 {
            return Vec::new();
        }
        let lo = corners.iter().fold(Vec3::splat(f32::MAX), |m, p| m.min(*p));
        let hi = corners.iter().fold(Vec3::splat(f32::MIN), |m, p| m.max(*p));
        if self.axial() {
            return vec![Aabb::new(lo, hi)];
        }
        // Edges: each polygon's.
        let mut edges = Vec::new();
        for p in &polys {
            for k in 0..p.len() {
                edges.push((p[k], p[(k + 1) % p.len()]));
            }
        }
        // Across the way its angled sides face, on the ground (a ramp becomes a stair, a
        // diagonal wall a row of posts). (Source's X is our -Z, its Y our -X: the axis it
        // picks first, a tie included, is our Z.)
        let angled: Vec<Vec3> = self.faces.iter().map(|f| f.n).filter(|n| n.abs().max_element() <= 0.99999).collect();
        let across = angled.iter().fold(Vec3::ZERO, |m, n| m + n.abs());
        let axis = if across.z >= across.x - 1e-3 { 2 } else { 0 };
        let size = hi - lo;
        let len = size[axis];
        // (Only for something you'd walk on, or under: a wall that leans has no steps to keep
        // small.)
        let sloped = angled.iter().any(|n| n.y.abs() > 0.5 && n[axis].abs() > 0.05);
        let by_rise = if sloped { (size.y / SLICE_RISE).ceil() as usize } else { 1 };
        let n = ((len / SLICE).ceil() as usize).max(by_rise).clamp(1, MAX_SLICES);
        let mut out = Vec::new();
        for k in 0..n {
            let (s0, s1) = (lo[axis] + len * k as f32 / n as f32, lo[axis] + len * (k + 1) as f32 / n as f32);
            let (mut a, mut z) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            let mut take = |p: Vec3| {
                a = a.min(p);
                z = z.max(p);
            };
            for p in &corners {
                if p[axis] >= s0 - 1e-4 && p[axis] <= s1 + 1e-4 {
                    take(*p);
                }
            }
            for &(p, q) in &edges {
                for s in [s0, s1] {
                    let (u, v) = (p[axis] - s, q[axis] - s);
                    if (u < 0.0) != (v < 0.0) && (u - v).abs() > 1e-7 {
                        take(p + (q - p) * (u / (u - v)));
                    }
                }
            }
            if a.x <= z.x {
                a[axis] = s0;
                z[axis] = s1;
                out.push(Aabb::new(a, z));
            }
        }
        out
    }

    /// Its faces that take portals at a slant, kept as they are (their boxes only step along
    /// them): of a still brush, the ones not square to the axes; of a brush entity's (`any`),
    /// every one (it may swing round). Only a white or black brush's.
    pub fn slopes(&self, any: bool) -> Vec<Slope> {
        if !matches!(self.collision(), Collision::Solid(Mat::White | Mat::Black)) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (i, f) in self.faces.iter().enumerate() {
            if f.noportal || tool_material(&f.mat) || (!any && f.n.abs().max_element() > 0.999) {
                continue;
            }
            let pts = polygon(&self.faces, i);
            if pts.len() >= 3 {
                out.push(Slope { normal: f.n.normalize(), pts });
            }
        }
        out
    }
}

/// What a brush is to collide with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Collision {
    Solid(Mat),
    Goo,
    None,
}

/// A displacement: a grid over a flat four-cornered face, each point pushed out (a dirt
/// floor's mounds). Its corners go round from the one its grid starts at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Displacement {
    pub mat: String,
    /// The grid has 2^power + 1 points a side.
    pub power: u32,
    /// Its flat face's corners (from its object's point).
    pub corners: [Vec3; 4],
    /// How its texture lies, as a brush face's.
    pub u: Vec4,
    pub v: Vec4,
    /// Each point's push off the flat face: rows from the first corner towards the second,
    /// each from the first corner's side to the fourth's.
    pub offsets: Vec<Vec3>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub noportal: bool,
}

impl Displacement {
    /// Points a side.
    pub fn side(&self) -> usize {
        (1usize << self.power.min(4)) + 1
    }

    /// Its grid: each point, and the flat point under it.
    pub fn grid(&self) -> Vec<(Vec3, Vec3)> {
        let side = self.side();
        let last = (side - 1) as f32;
        let c = self.corners;
        let mut out = Vec::with_capacity(side * side);
        for i in 0..side {
            let (a, b) = (c[0] + (c[1] - c[0]) * (i as f32 / last), c[3] + (c[2] - c[3]) * (i as f32 / last));
            for j in 0..side {
                let flat = a + (b - a) * (j as f32 / last);
                out.push((flat + self.offsets.get(i * side + j).copied().unwrap_or(Vec3::ZERO), flat));
            }
        }
        out
    }

    /// The way its flat face faces (up, for a floor).
    pub fn normal(&self) -> Vec3 {
        let c = self.corners;
        let n = (c[1] - c[0]).cross(c[3] - c[0]).normalize_or_zero();
        // (Its corners can go round either way: it faces away from its pushes, or up.)
        let push: Vec3 = self.offsets.iter().copied().sum();
        if push.dot(n) < 0.0 || (push == Vec3::ZERO && n.y < 0.0) { -n } else { n }
    }

    /// A floor's boxes, as `portal_extract` makes them: a box a grid cell, up to the cell's
    /// highest corner, so a walk follows its mounds. None for a wall or ceiling.
    pub fn boxes(&self) -> Vec<Aabb> {
        let side = self.side();
        let q = self.grid();
        let up = (q[side].1 - q[0].1).cross(q[1].1 - q[0].1).normalize_or_zero();
        if up.y.abs() < 0.7 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for i in 0..side - 1 {
            for j in 0..side - 1 {
                let cell = [q[i * side + j], q[(i + 1) * side + j], q[(i + 1) * side + j + 1], q[i * side + j + 1]];
                let lo = cell.iter().fold(Vec3::splat(f32::MAX), |m, (p, flat)| m.min(*p).min(*flat));
                let hi = cell.iter().fold(Vec3::splat(f32::MIN), |m, (p, _)| m.max(*p));
                if hi.y - lo.y < 0.01 {
                    continue;
                }
                out.push(Aabb::new(lo, hi));
            }
        }
        out
    }
}

/// A box brush: six faces of one material, its texture `scale` widths a metre across them
/// (upright on the walls).
pub fn box_brush(b: &Aabb, mat: &str, contents: Contents, scale: f32, noportal: bool) -> Brush {
    let mut faces = Vec::new();
    for (n, d) in [(Vec3::X, b.max.x), (Vec3::NEG_X, -b.min.x), (Vec3::Y, b.max.y), (Vec3::NEG_Y, -b.min.y), (Vec3::Z, b.max.z), (Vec3::NEG_Z, -b.min.z)] {
        // Its texture: across the face, upright on walls.
        let (u, v) = if n.y.abs() > 0.5 { (Vec3::X, Vec3::Z) } else { (n.cross(Vec3::Y).normalize() * -1.0, Vec3::NEG_Y) };
        faces.push(Face { n, d, mat: mat.to_string(), u: Vec4::from((u * scale, 0.0)), v: Vec4::from((v * scale, 0.0)), noportal, see_through: mat.starts_with("glass/") });
    }
    Brush { contents, faces }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> Brush {
        box_brush(&Aabb::new(Vec3::new(-1.0, 0.0, -2.0), Vec3::new(1.0, 3.0, 2.0)), "metal/x", Contents::Solid, 0.25, false)
    }

    #[test]
    fn a_box_brush_is_its_box() {
        let b = cube();
        assert!(b.axial());
        let bb = b.bounds().unwrap();
        assert_eq!((bb.min, bb.max), (Vec3::new(-1.0, 0.0, -2.0), Vec3::new(1.0, 3.0, 2.0)));
        assert_eq!(b.boxes().len(), 1);
        for (i, p) in b.polygons().iter().enumerate() {
            assert_eq!(p.len(), 4, "face {i}");
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize();
            assert!((n - b.faces[i].n).length() < 1e-5, "face {i} wound {n}");
        }
        let moved = b.moved(Vec3::new(5.0, 1.0, 0.0)).bounds().unwrap();
        assert!((moved.min - Vec3::new(4.0, 1.0, -2.0)).length() < 1e-5);
        let scaled = b.scaled(Vec3::new(2.0, 1.0, 0.5)).bounds().unwrap();
        assert!((scaled.max - Vec3::new(2.0, 3.0, 1.0)).length() < 1e-5);
    }

    #[test]
    fn a_ramp_is_cut_into_steps_and_keeps_its_slope() {
        // A wedge: floor, back, sides, and a slope rising towards -Z.
        let mut b = cube();
        b.faces.retain(|f| f.n != Vec3::Y && f.n != Vec3::Z);
        let n = Vec3::new(0.0, 4.0, 3.0).normalize();
        b.faces.push(Face { n, d: n.dot(Vec3::new(0.0, 0.0, 2.0)), mat: "metal/x".into(), u: Vec4::ZERO, v: Vec4::ZERO, noportal: false, see_through: false });
        let boxes = b.boxes();
        assert!(boxes.len() > 10);
        // The steps climb towards -Z.
        let first = boxes.iter().min_by(|a, b| a.min.z.total_cmp(&b.min.z)).unwrap();
        let last = boxes.iter().max_by(|a, b| a.min.z.total_cmp(&b.min.z)).unwrap();
        assert!(first.max.y > last.max.y);
        // White: its slope's kept.
        assert_eq!(b.collision(), Collision::Solid(Mat::White));
        assert_eq!(b.slopes(false).len(), 1);
    }

    #[test]
    fn collision_follows_the_faces() {
        let mut b = cube();
        for f in &mut b.faces {
            f.noportal = true;
        }
        assert_eq!(b.collision(), Collision::Solid(Mat::Black));
        for f in &mut b.faces {
            f.mat = "tools/toolsnodraw".into();
        }
        assert_eq!(b.collision(), Collision::Solid(Mat::White));
        b.faces[0].mat = "glass/glasswindow".into();
        assert_eq!(b.collision(), Collision::Solid(Mat::Glass));
    }
}
