//! Chamber 01, Portals: the cube is up on a ledge you can't climb. Put a portal on the wall
//! above it and one down here, walk through, and bring the cube back to the button.
//!
//! ```text
//!   z -18 +-------[door]-------+     corridor, grill, exit beyond
//!         |  ledge    button   |
//!         | (6 m,     (3,-12)  |
//!         |  black)            |
//!         | cube               |     the wall above the ledge (x -7) takes portals
//!         |                    |
//!   z  +4 +------ spawn -------+
//!       x -7                 x +7
//! ```

use super::kit;
use crate::level::{CubeKind, Gun, Inputs, Level, Mat};

/// Where things are, for the solution test.
pub const LEDGE_TOP: f32 = 6.0;
pub const LEDGE_X: (f32, f32) = (-7.0, -4.0);
pub const LEDGE_Z: (f32, f32) = (-16.0, -9.0);
pub const CUBE: [f32; 3] = [-5.5, LEDGE_TOP + 0.5, -10.2];
pub const BUTTON: [f32; 3] = [3.0, 0.0, -12.0];

pub fn chamber01() -> Level {
    let mut l = Level::new("Chamber 01: Portals");
    l.intro = "Speedy thing goes in, speedy thing comes out. First, the basics.";
    l.gun = Gun::BOTH;
    l.room([-7.0, 0.0, -18.0], [7.0, 10.0, 4.0], 1.0, Mat::White, Mat::White, Mat::Black);
    // The front wall behind the spawn: black, so the first portals go where they're meant to.
    l.repaint(Mat::Black, [-7.0, 0.0, 4.0], [7.0, 10.0, 5.0]);
    l.spawn([0.0, 0.0, 1.5], 0.0);
    kit::lamps(&mut l, (-7.0, 7.0), (-18.0, 4.0), 10.0, 7.0);

    // The ledge: black sides (no portals), too tall to climb.
    l.solid(Mat::Black, [LEDGE_X.0, 0.0, LEDGE_Z.0], [LEDGE_X.1, LEDGE_TOP, LEDGE_Z.1]);
    l.cube(CubeKind::Storage, CUBE);
    l.checkpoint("Ledge", [LEDGE_X.0, LEDGE_TOP, LEDGE_Z.0], [LEDGE_X.1, LEDGE_TOP + 3.0, LEDGE_Z.1], [-5.5, LEDGE_TOP, -13.0], -90.0);

    // The button and the door it opens.
    l.button(0, BUTTON);
    l.cut([-1.5, 0.0, -19.01], [1.5, 3.0, -17.99]);
    l.door(Inputs::on(0), [-1.5, 0.0, -18.6], [1.5, 3.0, -18.4]);
    l.wire(0, &[[3.0, 0.03, -12.95], [3.0, 0.03, -17.0], [1.6, 0.03, -17.0], [1.6, 0.03, -17.98]]);

    // Beyond: a corridor with an emancipation grill, then the exit.
    l.room([-2.0, 0.0, -27.0], [2.0, 3.5, -19.0], 0.5, Mat::Black, Mat::Black, Mat::Black);
    l.cut([-1.5, 0.0, -19.6], [1.5, 3.0, -18.39]);
    l.fizzler([-2.0, 0.0, -23.05], [2.0, 3.5, -22.95], Inputs::always());
    l.lamp([0.0, 3.45, -21.0]);
    kit::exit_room(&mut l, (-3.0, 3.0), (-33.0, -27.5), 0.0);
    l.cut([-1.5, 0.0, -28.01], [1.5, 3.0, -26.49]);

    l.hint([-7.0, 0.0, -4.0], [7.0, 4.0, 4.0], "Left mouse: blue portal. Right mouse: orange portal. Walk through one, come out of the other.");
    l.hint([0.0, 0.0, -18.0], [7.0, 4.0, -4.0], "The cube's up on the ledge. A portal on the white wall above it, one down here.");
    l.hint([LEDGE_X.0, LEDGE_TOP, LEDGE_Z.0], [LEDGE_X.1, LEDGE_TOP + 3.0, LEDGE_Z.1], "E picks the cube up. Shoot a portal low on the wall to walk back, or roll off the ledge (crouch as you land).");
    l.hint([-2.0, 0.0, -27.0], [2.0, 3.5, -19.0], "Emancipation grills close your portals and vaporise cubes.");
    l
}
