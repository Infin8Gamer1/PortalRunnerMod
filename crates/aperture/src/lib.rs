//! aperture: Portal's mechanics for faith_move.
//!
//! Portals and the portal gun, cubes and buttons, doors, emancipation grills, aerial faith
//! plates, hard light bridges, lasers and Portal 2's gels, as an engine-independent
//! simulation ([`Sim`]) that moves a faith_move [`Controller`](faith_move::Controller)
//! through portals. The test chambers are in [`levels`].

pub mod collide;
pub mod geom;
pub mod gel;
pub mod level;
pub mod levels;
pub mod portal;
pub mod sim;

pub use level::{CubeKind, Gun, Inputs, Item, Level, Mat, Paint, Sig};
pub use portal::{Link, Portal, PortalColor};
pub use sim::{Aim, Hit, HitWhat, Mask, Prop, Sim, SimEvent};

/// Movement tuning for Faith in a portal game: Mirror's Edge's, with a skid instead of a hard
/// clamp when a fling lands her faster than she can run, and no floor to fall out of (the
/// chamber's own kill height and goo do that).
pub fn tuning(hard_land_height: f32, lethal_fall_height: f32) -> faith_move::Tuning {
    faith_move::Tuning {
        overspeed_decel: Some(30.0),
        hard_land_height,
        lethal_fall_height,
        kill_y: None,
        respawn_on_death: true,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;
