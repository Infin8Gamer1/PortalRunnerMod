//! The test chambers, in play order, and Mirror's Edge's courses with portals.

use faith_move::{greybox, moves, rooftops, springboard};

use crate::level::Level;

mod arena;

pub use arena::arena;

/// Every map, in the order the game cycles through them.
pub fn all() -> Vec<(&'static str, fn() -> Level)> {
    vec![
        ("Test Arena", arena as fn() -> Level),
        ("Rooftops (ME)", || Level::from_greybox(&rooftops::rooftops())),
        ("Moves (ME)", || Level::from_greybox(&moves::moves())),
        ("Springboard (ME)", || Level::from_greybox(&springboard::springboard())),
        ("Training (ME)", || Level::from_greybox(&greybox::greybox())),
    ]
}
