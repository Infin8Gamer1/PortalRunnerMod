//! Portals: an oval opening on a flat surface, and the transform that carries anything going
//! into one out of the other.
//!
//! A portal's frame: `right`, `up` and `normal` (out of the surface, towards whoever looks at
//! it). Seen from the front, `right` is to the viewer's right: `right = up x normal`. Going
//! in through one portal's front you come out of the other's front, turned half round about
//! its up: local (x, y, z) in one becomes (-x, y, -z) in the other.

use glam::{Mat3, Quat, Vec2, Vec3};

use faith_move::Aabb;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortalColor {
    Blue,
    Orange,
}

impl PortalColor {
    pub const BOTH: [PortalColor; 2] = [PortalColor::Blue, PortalColor::Orange];

    pub fn index(self) -> usize {
        match self {
            PortalColor::Blue => 0,
            PortalColor::Orange => 1,
        }
    }

    pub fn other(self) -> PortalColor {
        match self {
            PortalColor::Blue => PortalColor::Orange,
            PortalColor::Orange => PortalColor::Blue,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PortalColor::Blue => "blue",
            PortalColor::Orange => "orange",
        }
    }
}

/// Half the portal's width and height (metres). Portal's are 64 x 112 units on a 72-unit
/// player; scaled to Faith's 1.8 m that is 1.6 x 2.8 m. A little smaller reads better here.
pub const PORTAL_HALF: Vec2 = Vec2::new(0.7, 1.2);

/// Portal's own size, in Portal's maps (64 x 112 units): what their walls, and the portals
/// the maps open themselves (centred 56 units off the floor), are made for. Smaller, a
/// preset portal's bottom edge stands off the floor and a cube can't be pushed through it.
pub const PORTAL_HALF_SOURCE: Vec2 = Vec2::new(32.0 * 0.0254, 56.0 * 0.0254);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Portal {
    pub center: Vec3,
    /// Out of the surface (axis-aligned: portals go on box faces).
    pub normal: Vec3,
    /// Up along the surface (axis-aligned).
    pub up: Vec3,
    pub half: Vec2,
    /// Seconds since it opened (for the opening animation).
    pub age: f32,
}

impl Portal {
    pub fn new(center: Vec3, normal: Vec3, up: Vec3) -> Self {
        Self::sized(center, normal, up, PORTAL_HALF)
    }

    /// One `half` in size.
    pub fn sized(center: Vec3, normal: Vec3, up: Vec3, half: Vec2) -> Self {
        Portal { center, normal, up, half, age: 0.0 }
    }

    pub fn right(&self) -> Vec3 {
        self.up.cross(self.normal)
    }

    /// Columns right, up, normal: local to world.
    pub fn basis(&self) -> Mat3 {
        Mat3::from_cols(self.right(), self.up, self.normal)
    }

    /// Rotation taking local axes (X right, Y up, Z out of the surface) to world.
    pub fn rotation(&self) -> Quat {
        Quat::from_mat3(&self.basis())
    }

    pub fn to_local(&self, p: Vec3) -> Vec3 {
        let d = p - self.center;
        Vec3::new(d.dot(self.right()), d.dot(self.up), d.dot(self.normal))
    }

    pub fn from_local(&self, l: Vec3) -> Vec3 {
        self.center + self.right() * l.x + self.up * l.y + self.normal * l.z
    }

    /// How far `p` is in front of the surface (negative: behind it).
    pub fn signed_distance(&self, p: Vec3) -> f32 {
        (p - self.center).dot(self.normal)
    }

    /// Whether a point on (or near) the surface plane is inside the oval.
    pub fn in_oval(&self, p: Vec3) -> bool {
        let l = self.to_local(p);
        (l.x / self.half.x).powi(2) + (l.y / self.half.y).powi(2) <= 1.0
    }

    /// Whether a point's projection onto the surface is inside the rectangle round the oval,
    /// shrunk by `inset` (negative grows it).
    pub fn in_rect(&self, p: Vec3, inset: f32) -> bool {
        let l = self.to_local(p);
        l.x.abs() <= self.half.x - inset && l.y.abs() <= self.half.y - inset
    }

    /// The box covering the opening, from `back` behind the surface to `front` in front of it.
    pub fn slab(&self, back: f32, front: f32) -> Aabb {
        let r = self.right() * self.half.x;
        let u = self.up * self.half.y;
        let a = self.center + r + u - self.normal * back;
        let b = self.center - r - u + self.normal * front;
        Aabb::new(a.min(b), a.max(b))
    }

    /// The four corners of the rectangle round the oval, on the surface.
    pub fn corners(&self) -> [Vec3; 4] {
        let r = self.right() * self.half.x;
        let u = self.up * self.half.y;
        [self.center - r - u, self.center + r - u, self.center + r + u, self.center - r + u]
    }
}

/// What carries things from portal `from` out of portal `to`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Link {
    pub from: Portal,
    pub to: Portal,
    /// Rotation from `from`'s side to `to`'s.
    pub rot: Mat3,
}

impl Link {
    pub fn new(from: Portal, to: Portal) -> Self {
        let flip = Mat3::from_diagonal(Vec3::new(-1.0, 1.0, -1.0));
        let rot = to.basis() * flip * from.basis().transpose();
        Link { from, to, rot }
    }

    pub fn point(&self, p: Vec3) -> Vec3 {
        self.to.center + self.rot * (p - self.from.center)
    }

    pub fn dir(&self, v: Vec3) -> Vec3 {
        self.rot * v
    }

    pub fn quat(&self) -> Quat {
        Quat::from_mat3(&self.rot)
    }
}

/// Yaw and pitch (faith_move's: yaw 0 looks down -Z, positive yaw turns left) of a view
/// direction.
pub fn yaw_pitch(dir: Vec3) -> (f32, f32) {
    let d = dir.normalize_or_zero();
    let pitch = d.y.clamp(-1.0, 1.0).asin();
    let yaw = (-d.x).atan2(-d.z);
    (yaw, pitch)
}

/// The view direction for a yaw and pitch.
pub fn view_dir(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(-yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos())
}

/// Where a view ends up through a portal: its new yaw and pitch, and how far it is left
/// rolled (radians, positive = right side down) once the view is set upright again.
/// Looking nearly straight up or down after the portal, the yaw comes from where the top of
/// the view now points.
pub fn carry_view(link: &Link, yaw: f32, pitch: f32) -> (f32, f32, f32) {
    let q = Quat::from_euler(glam::EulerRot::YXZ, yaw, pitch, 0.0);
    let fwd = q * Vec3::NEG_Z;
    let up = q * Vec3::Y;
    let f2 = link.dir(fwd);
    let u2 = link.dir(up);
    let (mut yaw2, pitch2) = yaw_pitch(f2);
    if f2.y.abs() > 0.98 {
        // Straight up or down: the top of the view (or its bottom, looking up) says which
        // way is ahead.
        let ahead = if f2.y < 0.0 { u2 } else { -u2 };
        let h = Vec3::new(ahead.x, 0.0, ahead.z);
        if h.length_squared() > 1e-6 {
            yaw2 = (-h.x).atan2(-h.z);
        }
    }
    // The roll left over: the carried view's up against the upright view's up.
    let upright = Quat::from_euler(glam::EulerRot::YXZ, yaw2, pitch2, 0.0);
    let up_upright = upright * Vec3::Y;
    let right_upright = upright * Vec3::X;
    let roll = (-u2.dot(right_upright)).atan2(u2.dot(up_upright));
    (yaw2, pitch2, roll)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-4
    }

    #[test]
    fn walking_into_a_wall_portal_comes_out_of_the_other_facing_away() {
        // Blue on a wall facing +Z at z = 0; orange on a wall facing +X at x = 10.
        let blue = Portal::new(Vec3::new(0.0, 1.2, 0.0), Vec3::Z, Vec3::Y);
        let orange = Portal::new(Vec3::new(10.0, 1.2, 5.0), Vec3::X, Vec3::Y);
        let l = Link::new(blue, orange);
        // Moving into blue (-Z) comes out of orange moving +X.
        assert!(close(l.dir(Vec3::NEG_Z), Vec3::X));
        // A point 0.5 m behind blue is 0.5 m in front of orange.
        assert!(close(l.point(Vec3::new(0.0, 1.2, -0.5)), Vec3::new(10.5, 1.2, 5.0)));
        // A point to blue's right (+X seen from the front) comes out to orange's left.
        let p = l.point(Vec3::new(0.3, 1.2, 0.0));
        assert!(close(p, orange.center - orange.right() * 0.3), "{p}");
        // And back again.
        let back = Link::new(orange, blue);
        assert!(close(back.point(l.point(Vec3::new(0.2, 0.7, -0.4))), Vec3::new(0.2, 0.7, -0.4)));
    }

    #[test]
    fn falling_into_a_floor_portal_flings_out_of_a_wall() {
        // Blue on the floor, its up pointing -Z (the way she faces); orange on a wall facing +X.
        let blue = Portal::new(Vec3::ZERO, Vec3::Y, Vec3::NEG_Z);
        let orange = Portal::new(Vec3::new(0.0, 1.2, 20.0), Vec3::X, Vec3::Y);
        let l = Link::new(blue, orange);
        assert!(close(l.dir(Vec3::NEG_Y * 10.0), Vec3::X * 10.0));
        // Looking down and ahead (-Z) into the floor: out of the wall looking out, upright.
        let (yaw, pitch, roll) = carry_view(&l, 0.0, -1.2);
        let d = view_dir(yaw, pitch);
        assert!(d.x > 0.9, "{d}");
        assert!(roll.abs() < 0.05, "roll {roll}");
    }

    #[test]
    fn view_round_trip() {
        for (yaw, pitch) in [(0.3f32, 0.2f32), (-2.0, -0.7), (3.0, 1.0)] {
            let (y, p) = yaw_pitch(view_dir(yaw, pitch));
            assert!(close(view_dir(y, p), view_dir(yaw, pitch)));
        }
    }
}
