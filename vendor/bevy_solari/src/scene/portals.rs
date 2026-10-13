//! A linked pair of portals, for traced light to go through: the game sets it each frame
//! (none when there's no linked pair), and the scene bindings carry it to the shaders.

use bevy_ecs::resource::Resource;
use bevy_math::{Mat4, Vec2, Vec3};
use bevy_render::extract_resource::ExtractResource;

/// The linked portals: none, or a pair (each one's `to_other` takes it to the other).
#[derive(Resource, Clone, Default, ExtractResource)]
pub struct RaytracingPortals {
    pub portals: Vec<RaytracingPortal>,
}

/// A portal's opening (an ellipse `half_size` across its `right` and `up`), facing `normal`,
/// and the rigid transform that takes what goes into it to where it comes out of the other.
#[derive(Clone, Copy, Debug)]
pub struct RaytracingPortal {
    pub center: Vec3,
    pub normal: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub half_size: Vec2,
    pub to_other: Mat4,
}
