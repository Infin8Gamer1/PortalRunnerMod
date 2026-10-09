//! A plain white hall for trying things out (and for the tests): every surface takes
//! portals.

use crate::level::{Level, Mat};

pub fn arena() -> Level {
    let mut l = Level::new("Test Arena");
    l.intro = "A white room. Every surface takes portals.";
    l.room([-20.0, 0.0, -20.0], [20.0, 12.0, 20.0], 1.0, Mat::White, Mat::White, Mat::White);
    l.spawn([0.0, 0.0, 10.0], 0.0);
    for x in [-10.0, 10.0] {
        for z in [-10.0, 10.0] {
            l.lamp([x, 11.5, z]);
        }
    }
    l
}
