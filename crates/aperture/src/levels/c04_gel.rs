//! Chamber 04, Gel Runner: propulsion gel for a jump too long to make on foot, repulsion
//! gel up a wall too tall to climb, and conversion gel to put a portal where none would go.
//!
//! ```text
//!   side view:
//!                                                     [upper y 5]  goo   [far side y 5, black]
//!   spawn ===orange gel===  goo  [landing]  blue gel |wall          ~~~~   white gel patch
//!   z:  3        -6  -24  -26  -37         -50  -56 |-56   -60     -72   -76        -80
//! ```

use super::kit;
use crate::level::{Inputs, Level, Mat, Paint};

pub const GAP: (f32, f32) = (-37.0, -26.0);
pub const WALL_Z: f32 = -56.0;
pub const UPPER: f32 = 5.0;
pub const FAR_Z: f32 = -72.0;
pub const PATCH: [f32; 3] = [0.0, UPPER, -76.0];
pub const PANEL: [f32; 3] = [8.0, UPPER + 0.8, -58.0];

pub fn chamber04() -> Level {
    let mut l = Level::new("Chamber 04: Gel Runner");
    l.intro = "Propulsion. Repulsion. Conversion. Mind the goo.";
    l.room([-8.0, 0.0, -80.0], [8.0, 16.0, 6.0], 1.0, Mat::Black, Mat::Black, Mat::Black);
    l.spawn([0.0, 0.0, 3.0], 0.0);
    kit::lamps(&mut l, (-8.0, 8.0), (-80.0, 6.0), 16.0, 10.0);

    // ---- Propulsion: an orange runway, then a gap no one could jump on foot.
    l.paint(Paint::Speed, [-3.0, 0.0, -24.0], [3.0, 0.0, -6.0], [0.0, 1.0, 0.0]);
    l.gel_pipe(Paint::Speed, [0.0, 15.6, -8.0], [0.0, -1.0, 0.0], Inputs::always());
    l.cut([-8.0, -1.01, GAP.0], [8.0, 0.01, GAP.1]);
    l.solid(Mat::Black, [-9.0, -4.0, GAP.0 - 1.0], [9.0, -3.0, GAP.1 + 1.0]);
    l.solid(Mat::Black, [-9.0, -3.0, GAP.0 - 1.0], [9.0, -1.0, GAP.0]);
    l.solid(Mat::Black, [-9.0, -3.0, GAP.1], [9.0, -1.0, GAP.1 + 1.0]);
    l.goo([-8.0, -3.0, GAP.0], [8.0, -1.2, GAP.1]);
    l.checkpoint("Landing", [-8.0, 0.0, -50.0], [8.0, 3.0, GAP.0], [0.0, 0.0, -42.0], 0.0);

    // ---- Repulsion: blue gel before a wall too tall to climb.
    l.solid(Mat::Black, [-8.0, 0.0, -60.0], [8.0, UPPER, WALL_Z]);
    l.paint(Paint::Bounce, [-8.0, 0.0, WALL_Z + 6.0], [8.0, 0.0, WALL_Z], [0.0, 1.0, 0.0]);
    l.gel_pipe(Paint::Bounce, [0.0, 15.6, WALL_Z + 3.0], [0.0, -1.0, 0.0], Inputs::always());
    l.checkpoint("Upper level", [-8.0, UPPER, -60.0], [8.0, UPPER + 3.0, WALL_Z], [0.0, UPPER, -57.5], 0.0);

    // ---- Conversion: goo, and a black floor beyond that white gel makes take portals.
    l.goo([-8.0, 0.0, FAR_Z], [8.0, 1.0, -60.0]);
    l.solid(Mat::Black, [-8.0, 0.0, -80.0], [8.0, UPPER, FAR_Z]);
    l.paint(Paint::Portal, [PATCH[0] - 2.0, UPPER, PATCH[2] - 2.0], [PATCH[0] + 2.0, UPPER, PATCH[2] + 2.0], [0.0, 1.0, 0.0]);
    l.gel_pipe(Paint::Portal, [0.0, 15.6, PATCH[2]], [0.0, -1.0, 0.0], Inputs::always());
    // One white panel on this side for the other portal.
    l.repaint(Mat::White, [8.0, UPPER, -60.0], [9.0, UPPER + 4.0, WALL_Z]);
    kit::exit_room(&mut l, (-3.0, 3.0), (-87.0, -80.5), UPPER);
    l.cut([-1.5, UPPER, -81.01], [1.5, UPPER + 3.0, -79.99]);

    l.hint([-8.0, 0.0, -6.0], [8.0, 3.0, 6.0], "Orange gel: run. Keep going, and jump at the very edge.");
    l.hint([-8.0, 0.0, -50.0], [8.0, 3.0, GAP.0], "Blue gel bounces. Jump on it at the wall, climb as you rise.");
    l.hint([-8.0, UPPER, -60.0], [8.0, UPPER + 3.0, WALL_Z], "White gel takes portals. One on the white patch across the goo, one on the white panel here.");
    l
}
