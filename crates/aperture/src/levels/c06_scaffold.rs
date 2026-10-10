//! Chamber 06, Unstationary Scaffold: a high energy pellet bounces across the room. Send it
//! through your portals into the catcher on the far side of the goo, and the platform it
//! powers carries you over.
//!
//! ```text
//!   z -44 +-------[exit]-------+
//!         | catcher       ( )  |   orange here, on the right wall, low: into the catcher
//!   z -30 |~~~~~~~~~~~~~~~~~~~~|
//!         |~~~~ goo  [plat] ~~~|   the platform shuttles over the goo once it's powered
//!   z -10 |~~~~~~~~~~~~~~~~~~~~|
//!         | launcher -> pellet |   the pellet crosses at head height; blue where it hits
//!   z  +6 +------ spawn -------+
//!       x -10                x +10
//! ```

use super::kit;
use crate::level::{Gun, Inputs, Level, Mat};

pub const LAUNCHER: [f32; 3] = [-9.5, 6.0, -4.0];
pub const CATCHER: [f32; 3] = [-9.55, 1.2, -36.0];
pub const PLATFORM: ([f32; 3], [f32; 3]) = ([-1.5, -0.3, -13.0], [1.5, 0.0, -10.0]);
pub const TRAVEL: [f32; 3] = [0.0, 0.0, -17.0];

pub fn chamber06() -> Level {
    let mut l = Level::new("Chamber 06: Unstationary Scaffold");
    l.intro = "The Aperture Science High Energy Pellet. Do not touch it. Do put it where it's wanted.";
    l.gun = Gun::BOTH;
    l.room([-10.0, 0.0, -44.0], [10.0, 12.0, 6.0], 1.0, Mat::White, Mat::White, Mat::Black);
    l.repaint(Mat::Black, [-10.0, 0.0, 6.0], [10.0, 12.0, 7.0]);
    l.spawn([0.0, 0.0, 3.0], 0.0);
    kit::lamps(&mut l, (-10.0, 10.0), (-44.0, 6.0), 12.0, 10.0);
    kit::observation_room(&mut l, kit::Along::Z, -20.0, 7.0, -10.0, -11.0, 6.0, 2.0, 3.5, true);

    // The pellet, launched across the room from the left wall at head height and a half (until
    // one's caught).
    l.pellet_launcher(LAUNCHER, [1.0, 0.0, 0.0], Inputs::off(0));

    // The goo, and its pit's walls (black: nothing to portal down there).
    l.cut([-10.0, -1.01, -30.0], [10.0, 0.01, -10.0]);
    l.solid(Mat::Black, [-11.0, -4.0, -31.0], [11.0, -3.0, -9.0]);
    l.solid(Mat::Black, [-11.0, -3.0, -31.0], [11.0, -1.0, -30.0]);
    l.solid(Mat::Black, [-11.0, -3.0, -10.0], [11.0, -1.0, -9.0]);
    l.goo([-10.0, -3.0, -30.0], [10.0, -1.2, -10.0]);
    l.repaint(Mat::Black, [-11.0, 0.0, -30.0], [11.0, 12.0, -10.0]);

    // The catcher on the far side, low on the left wall; the platform it powers.
    l.pellet_catcher(0, CATCHER, [1.0, 0.0, 0.0]);
    l.platform(PLATFORM.0, PLATFORM.1, TRAVEL, 3.0, Inputs::on(0));
    l.wire(0, &[[-9.97, 0.6, -36.0], [-9.97, 0.03, -36.0], [-9.97, 0.03, -31.0], [0.0, 0.03, -31.0], [0.0, 0.03, -30.03]]);

    // The way out, in the back wall.
    kit::exit_room(&mut l, (-3.0, 3.0), (-50.5, -45.5), 0.0);
    l.cut([-1.5, 0.0, -45.51], [1.5, 3.0, -43.99]);

    l.hint([-10.0, 0.0, -10.0], [10.0, 4.0, 6.0], "The pellet bounces until it finds its catcher. A portal where it hits, the other facing the catcher.");
    l.hint([-10.0, 0.0, -44.0], [10.0, 4.0, -30.0], "The platform runs while the catcher's lit.");
    l
}
