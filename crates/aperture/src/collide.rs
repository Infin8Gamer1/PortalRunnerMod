//! Collision through portals.
//!
//! A portal is a hole in its wall, but only for someone going in through its front: walk
//! round the back of a thin wall and it's still solid. So each body (Faith, every cube)
//! remembers which portal, if any, it is going through (its *environment*, as Portal calls
//! it). While it is, it collides with a copy of the level that has the hole cut out of the
//! wall behind the opening and a sleeve round the tunnel (so it can't slip sideways into the
//! wall, or into a room behind a thin one). Everyone else collides with the plain level.
//!
//! A body enters a portal's environment when its centre is in front of the portal, within
//! the opening's outline, and close enough to reach it this frame; it leaves when its centre
//! wanders out of that column. It goes through when its centre (Faith: her eye) crosses the
//! surface inside the opening.

use glam::Vec3;

use faith_move::{Aabb, BoxWorld, Fixture, Gel, SweepHit, World};

use crate::geom;
use crate::gel::GelMap;
use crate::portal::{Portal, PortalColor};

/// How deep the tunnel behind a portal goes (deeper than Faith is tall, so falling into a
/// floor portal feet first her eye gets through before her feet reach its bottom).
pub const TUNNEL_DEPTH: f32 = 3.0;
const SLEEVE: f32 = 0.15;

/// The level's still boxes with the hole for portal `p` cut out, and the sleeve round it.
pub fn carve(statics: &[Aabb], p: &Portal) -> BoxWorld {
    let hole = p.slab(TUNNEL_DEPTH, 0.005);
    let mut boxes = Vec::with_capacity(statics.len() + 4);
    for b in statics {
        // (A tilted face's hole is the box round its tunnel: only what's behind the face is
        // cut, not the room in front of it.)
        if p.axial() || p.signed_distance(b.center()) < 0.0 {
            boxes.extend(geom::subtract(b, &hole));
        } else {
            boxes.push(*b);
        }
    }
    // The sleeve: four walls round the hole, from the surface back (corners overlapping).
    // (Not on a tilted face: boxes square to the world would close the tunnel.)
    if !p.axial() {
        return BoxWorld::indexed(boxes, Vec::new(), 4.0);
    }
    let (hx, hy) = (p.half.x, p.half.y);
    let (ox, oy) = (hx + SLEEVE, hy + SLEEVE);
    for (lo, hi) in [
        (Vec3::new(hx, -oy, -TUNNEL_DEPTH), Vec3::new(ox, oy, 0.0)),
        (Vec3::new(-ox, -oy, -TUNNEL_DEPTH), Vec3::new(-hx, oy, 0.0)),
        (Vec3::new(-ox, hy, -TUNNEL_DEPTH), Vec3::new(ox, oy, 0.0)),
        (Vec3::new(-ox, -oy, -TUNNEL_DEPTH), Vec3::new(ox, -hy, 0.0)),
    ] {
        boxes.push(local_box(p, lo, hi));
    }
    BoxWorld::indexed(boxes, Vec::new(), 4.0)
}

/// The world box of a box given in a portal's local frame (on a tilted face, the box round
/// it).
pub fn local_box(p: &Portal, lo: Vec3, hi: Vec3) -> Aabb {
    let corners = [0, 1, 2, 3, 4, 5, 6, 7].map(|k| p.from_local(Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z })));
    Aabb::new(corners.iter().fold(Vec3::splat(f32::MAX), |a, c| a.min(*c)), corners.iter().fold(Vec3::splat(f32::MIN), |a, c| a.max(*c)))
}

/// Which portal a body is going through, if any; `linked` says whether both portals are
/// open (a lone portal is just a mark on the wall).
pub fn update_env(env: &mut Option<PortalColor>, portals: &[Option<Portal>; 2], linked: bool, center: Vec3, half: Vec3, vel: Vec3, dt: f32) {
    if !linked {
        *env = None;
        return;
    }
    if let Some(c) = *env {
        let Some(p) = portals[c.index()] else {
            *env = None;
            return;
        };
        let l = p.to_local(center);
        let reach = reach(&p, half, vel, dt);
        if l.x.abs() > p.half.x + 0.1 || l.y.abs() > p.half.y + 0.1 || l.z < -TUNNEL_DEPTH || l.z > reach + 0.5 {
            *env = None;
        } else {
            return;
        }
    }
    let mut best: Option<(f32, PortalColor)> = None;
    for c in PortalColor::BOTH {
        let Some(p) = portals[c.index()] else { continue };
        let l = p.to_local(center);
        if l.x.abs() <= p.half.x && l.y.abs() <= p.half.y && l.z >= -0.02 && l.z <= reach(&p, half, vel, dt) && best.is_none_or(|(d, _)| l.z < d) {
            best = Some((l.z, c));
        }
    }
    *env = best.map(|(_, c)| c);
}

/// How far in front of a portal a body's centre may be and still reach it this frame.
fn reach(p: &Portal, half: Vec3, vel: Vec3, dt: f32) -> f32 {
    half.dot(p.normal.abs()) + vel.dot(p.normal).abs() * dt * 3.0 + 0.4
}

/// The world as one body sees it: the level (plain, or with its portal's hole), whatever
/// moves (doors, bridges, cubes), and the paint.
pub struct BodyWorld<'a> {
    pub still: &'a BoxWorld,
    pub moving: BoxWorld,
    pub gel: &'a GelMap,
    pub fixtures: &'a [Fixture],
}

impl World for BodyWorld<'_> {
    fn sweep(&self, half: Vec3, start: Vec3, delta: Vec3) -> Option<SweepHit> {
        match (self.still.sweep(half, start, delta), self.moving.sweep(half, start, delta)) {
            (Some(a), Some(b)) => Some(if b.t < a.t { b } else { a }),
            (a, b) => a.or(b),
        }
    }

    fn overlaps(&self, region: &Aabb) -> bool {
        self.still.overlaps(region) || self.moving.overlaps(region)
    }

    fn fixtures(&self) -> &[Fixture] {
        self.fixtures
    }

    fn gel(&self, point: Vec3, normal: Vec3) -> Gel {
        self.gel.gel_at(point, normal)
    }
}

/// Move a box (centre `c`, half size `half`) along one axis, stopping at the first thing in
/// the way. Returns the normal it hit, if it did.
pub fn move_box(world: &dyn World, half: Vec3, c: &mut Vec3, axis: usize, d: f32) -> Option<Vec3> {
    if d == 0.0 {
        return None;
    }
    let mut delta = Vec3::ZERO;
    delta[axis] = d;
    match world.sweep(half, *c, delta) {
        Some(h) => {
            c[axis] += d * h.t;
            Some(h.normal)
        }
        None => {
            c[axis] += d;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wall_portal_is_a_hole_only_through_its_opening() {
        // A wall 0.3 m thick facing +Z, a portal on it.
        let wall = Aabb::new(Vec3::new(-10.0, 0.0, -0.3), Vec3::new(10.0, 5.0, 0.0));
        let floor = Aabb::new(Vec3::new(-10.0, -1.0, -10.0), Vec3::new(10.0, 0.0, 10.0));
        let p = Portal::new(Vec3::new(0.0, 1.2, 0.0), Vec3::Z, Vec3::Y);
        let w = carve(&[wall, floor], &p);
        // A body in the opening goes straight in.
        let half = Vec3::new(0.3, 0.9, 0.3);
        let c = Vec3::new(0.0, 0.91, 1.0);
        assert!(w.sweep(half, c, Vec3::new(0.0, 0.0, -2.5)).is_none());
        // Off to the side, the wall is still there.
        assert!(w.sweep(half, Vec3::new(3.0, 0.91, 1.0), Vec3::new(0.0, 0.0, -2.5)).is_some());
        // Inside the tunnel, the sleeve stops it sliding sideways.
        assert!(w.sweep(half, Vec3::new(0.0, 0.91, -1.0), Vec3::new(2.0, 0.0, 0.0)).is_some());
        // The floor carries on under it.
        assert!(w.sweep(half, Vec3::new(0.0, 0.91, -1.0), Vec3::new(0.0, -0.5, 0.0)).is_some());
    }

    #[test]
    fn the_environment_needs_the_front() {
        let p = Portal::new(Vec3::new(0.0, 1.2, 0.0), Vec3::Z, Vec3::Y);
        let portals = [Some(p), Some(Portal::new(Vec3::new(20.0, 1.2, 0.0), Vec3::Z, Vec3::Y))];
        let half = Vec3::new(0.3, 0.9, 0.3);
        let mut env = None;
        // Behind the wall: no.
        update_env(&mut env, &portals, true, Vec3::new(0.0, 0.9, -0.6), half, Vec3::ZERO, 1.0 / 60.0);
        assert_eq!(env, None);
        // In front, inside the outline: yes, and it stays while she's in the tunnel.
        update_env(&mut env, &portals, true, Vec3::new(0.0, 0.9, 0.6), half, Vec3::ZERO, 1.0 / 60.0);
        assert_eq!(env, Some(PortalColor::Blue));
        update_env(&mut env, &portals, true, Vec3::new(0.0, 0.9, -0.6), half, Vec3::ZERO, 1.0 / 60.0);
        assert_eq!(env, Some(PortalColor::Blue));
        // Not linked: nothing.
        update_env(&mut env, &portals, false, Vec3::new(0.0, 0.9, 0.6), half, Vec3::ZERO, 1.0 / 60.0);
        assert_eq!(env, None);
    }
}
