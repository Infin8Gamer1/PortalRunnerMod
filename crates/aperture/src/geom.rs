//! Rays and boxes.

use glam::Vec3;

use faith_move::Aabb;

/// Where a ray from `o` along `d` (any length; `t` is in units of `d`) first enters `b`, and
/// the face's outward normal. A ray starting inside doesn't hit it.
pub fn ray_aabb(o: Vec3, d: Vec3, b: &Aabb) -> Option<(f32, Vec3)> {
    let mut t_in = f32::NEG_INFINITY;
    let mut t_out = f32::INFINITY;
    let mut normal = Vec3::ZERO;
    for a in 0..3 {
        if d[a].abs() < 1e-12 {
            if o[a] < b.min[a] || o[a] > b.max[a] {
                return None;
            }
            continue;
        }
        let inv = 1.0 / d[a];
        let (t0, t1) = ((b.min[a] - o[a]) * inv, (b.max[a] - o[a]) * inv);
        let (near, far) = if t0 < t1 { (t0, t1) } else { (t1, t0) };
        if near > t_in {
            t_in = near;
            normal = Vec3::ZERO;
            normal[a] = -d[a].signum();
        }
        t_out = t_out.min(far);
    }
    if t_in > t_out || t_out < 0.0 || t_in < 0.0 {
        return None;
    }
    Some((t_in, normal))
}

/// Whether point `p` is inside `b` (faces included).
pub fn contains(b: &Aabb, p: Vec3) -> bool {
    p.cmpge(b.min).all() && p.cmple(b.max).all()
}

/// `a` with `cut` taken out of it: up to six boxes that together fill the rest.
pub fn subtract(a: &Aabb, cut: &Aabb) -> Vec<Aabb> {
    if !a.overlaps(cut) {
        return vec![*a];
    }
    let mut out = Vec::new();
    let mut rest = *a;
    for axis in 0..3 {
        if cut.min[axis] > rest.min[axis] {
            let mut piece = rest;
            piece.max[axis] = cut.min[axis];
            out.push(piece);
            rest.min[axis] = cut.min[axis];
        }
        if cut.max[axis] < rest.max[axis] {
            let mut piece = rest;
            piece.min[axis] = cut.max[axis];
            out.push(piece);
            rest.max[axis] = cut.max[axis];
        }
    }
    out.retain(|b| b.size().min_element() > 1e-5);
    out
}

/// The axis (unit, signed) closest to `v`.
pub fn snap_axis(v: Vec3) -> Vec3 {
    let a = v.abs();
    if a.x >= a.y && a.x >= a.z {
        Vec3::X * v.x.signum()
    } else if a.y >= a.z {
        Vec3::Y * v.y.signum()
    } else {
        Vec3::Z * v.z.signum()
    }
}

/// Closest distance between segment `a`-`b` and point `p`, and the point on the segment.
pub fn segment_point(a: Vec3, b: Vec3, p: Vec3) -> (f32, Vec3) {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 < 1e-12 { 0.0 } else { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) };
    let q = a + ab * t;
    (q.distance(p), q)
}

/// A small deterministic random number generator (xorshift), so the simulation replays the
/// same way in tests.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in 0..1.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in -1..1.
    pub fn signed(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_the_near_face() {
        let b = Aabb::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::ONE);
        let (t, n) = ray_aabb(Vec3::new(0.0, 0.0, 5.0), Vec3::NEG_Z, &b).unwrap();
        assert!((t - 4.0).abs() < 1e-5);
        assert_eq!(n, Vec3::Z);
        assert!(ray_aabb(Vec3::new(3.0, 0.0, 5.0), Vec3::NEG_Z, &b).is_none());
        assert!(ray_aabb(Vec3::ZERO, Vec3::NEG_Z, &b).is_none(), "starting inside");
    }

    #[test]
    fn subtracting_a_hole_leaves_the_rest() {
        let wall = Aabb::new(Vec3::new(-5.0, 0.0, -0.5), Vec3::new(5.0, 4.0, 0.0));
        let hole = Aabb::new(Vec3::new(-1.0, 1.0, -3.0), Vec3::new(1.0, 3.0, 0.01));
        let parts = subtract(&wall, &hole);
        let vol = |b: &Aabb| b.size().x * b.size().y * b.size().z;
        let total: f32 = parts.iter().map(vol).sum();
        assert!((total - (vol(&wall) - 2.0 * 2.0 * 0.5)).abs() < 1e-4, "{total}");
        assert!(parts.iter().all(|p| !p.overlaps(&hole)));
    }
}
