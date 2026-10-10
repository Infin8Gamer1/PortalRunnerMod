//! The test chambers, in play order, and Mirror's Edge's courses with portals.

use faith_move::{greybox, moves, rooftops, springboard};

use crate::level::Level;

mod arena;
pub mod c01_portals;
pub mod c02_momentum;
pub mod c03_hard_light;
pub mod c04_gel;
pub mod c05_rooftops;
pub mod c06_scaffold;
pub mod kit;

pub use arena::arena;
pub use c01_portals::chamber01;
pub use c02_momentum::chamber02;
pub use c03_hard_light::chamber03;
pub use c04_gel::chamber04;
pub use c05_rooftops::chamber05;
pub use c06_scaffold::chamber06;

/// Every map, in the order the game cycles through them.
pub fn all() -> Vec<(&'static str, fn() -> Level)> {
    vec![
        ("Proving Grounds", arena as fn() -> Level),
        ("Chamber 01", chamber01),
        ("Chamber 02", chamber02),
        ("Chamber 03", chamber03),
        ("Chamber 04", chamber04),
        ("Chamber 05", chamber05),
        ("Chamber 06", chamber06),
        ("Rooftops (ME)", || Level::from_greybox(&rooftops::rooftops())),
        ("Moves (ME)", || Level::from_greybox(&moves::moves())),
        ("Springboard (ME)", || Level::from_greybox(&springboard::springboard())),
        ("Training (ME)", || Level::from_greybox(&greybox::greybox())),
    ]
}
