//! aperture: Portal's mechanics for faith_move.
//!
//! Portals and the portal gun, cubes and buttons, doors, emancipation grills, aerial faith
//! plates, hard light bridges, lasers and Portal 2's gels, as an engine-independent
//! simulation ([`Sim`]) that moves a faith_move [`Controller`](faith_move::Controller)
//! through portals. The test chambers are in [`levels`].

pub mod art;
pub mod brush;
pub mod collide;
pub mod doc;
pub mod geom;
pub mod gel;
pub mod io;
pub mod level;
pub mod levels;
pub mod me;
pub mod port;
pub mod portal;
pub mod rope;
pub mod sim;
pub mod source;

pub use level::{CubeKind, Gun, Inputs, Item, Launch, Level, Mat, MeProp, Paint, PlacedLight, Sig, SunLight};
pub use portal::{Link, Portal, PortalColor};
pub use sim::{Aim, Hit, HitWhat, MapTransition, Mask, Prop, Sim, SimEvent};

/// Movement tuning for Faith in a portal game: Mirror's Edge's, with a skid instead of a hard
/// clamp when a fling lands her faster than she can run, and no floor to fall out of (the
/// chamber's own kill height and goo do that).
/// The gravity Faith falls under (m/s²): Portal's (600 units/s²), not Mirror's Edge's
/// (16.4), so Portal's flings carry as they should. A level can set its own (its spawn's).
pub const FAITH_GRAVITY: f32 = 15.24;

pub fn tuning(hard_land_height: f32, lethal_fall_height: f32) -> faith_move::Tuning {
    faith_move::Tuning {
        gravity: FAITH_GRAVITY,
        overspeed_decel: Some(30.0),
        hard_land_height,
        lethal_fall_height,
        kill_y: None,
        respawn_on_death: true,
        // Source's stepsize is 18 units. Mirror's Edge's 35 units (0.35 m) stops on a
        // 16-unit stair (0.406 m).
        step_height: 18.0 * 0.0254,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod solves;
#[cfg(test)]
mod me_tests;
