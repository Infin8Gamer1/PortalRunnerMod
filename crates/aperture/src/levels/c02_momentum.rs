//! Chamber 02, Momentum: speedy thing goes in, speedy thing comes out. The gun only fires
//! blue; orange is fixed high on the tower's face. Drop off the tower into a blue portal
//! on the floor below (the fall alone would kill you), fly out of orange over the goo, then
//! wallrun the red wall across the last gap.
//!
//! ```text
//!   side view:                       orange (y 8)
//!   spawn (y 12) [tower]|o
//!                       |  \_ fling __
//!                [drop zone y 0]      \__
//!                       goo     [landing y 2]   goo (wallrun)  [exit y 2]
//!   z:            10    0   -8  -14          -35           -44        -50
//! ```

use super::kit;
use crate::level::{Gun, Level, Mat};
use crate::portal::PortalColor;

pub const TOWER_TOP: f32 = 12.0;
pub const LANDING_TOP: f32 = 2.0;
pub const WALL_X: f32 = 7.6;

pub fn chamber02() -> Level {
    let mut l = Level::new("Chamber 02: Momentum");
    l.intro = "In Aperture Science, momentum is conserved. So is the fall: don't miss.";
    l.gun = Gun { blue: true, orange: false };
    l.room([-8.0, -4.0, -50.0], [8.0, 18.0, 10.0], 1.0, Mat::Black, Mat::Black, Mat::Black);
    l.kill_y = -6.0;
    kit::lamps(&mut l, (-8.0, 8.0), (-50.0, 10.0), 18.0, 10.0);

    // The tower, its face black but for the fixed orange portal.
    l.solid(Mat::Black, [-8.0, -4.0, 0.0], [8.0, TOWER_TOP, 10.0]);
    l.portal(PortalColor::Orange, [0.0, 8.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]);
    l.spawn([0.0, TOWER_TOP, 6.0], 0.0);

    // The drop zone: white floor for the blue portal.
    l.solid(Mat::White, [-8.0, -4.0, -8.0], [8.0, 0.0, 0.0]);
    l.goo([-8.0, -4.0, -14.0], [8.0, -1.5, -8.0]);

    // Where the fling lands.
    l.solid(Mat::Black, [-8.0, -4.0, -35.0], [8.0, LANDING_TOP, -14.0]);
    l.checkpoint("Landing", [-8.0, LANDING_TOP, -35.0], [8.0, LANDING_TOP + 3.0, -14.0], [0.0, LANDING_TOP, -24.0], 0.0);

    // The last gap, and the red wall along it.
    l.goo([-8.0, -4.0, -44.0], [8.0, -1.5, -35.0]);
    l.solid(Mat::Runner, [WALL_X, LANDING_TOP, -46.0], [8.0, LANDING_TOP + 5.0, -33.0]);
    l.solid(Mat::Black, [-8.0, -4.0, -50.0], [8.0, LANDING_TOP, -44.0]);
    kit::exit_room(&mut l, (-3.0, 3.0), (-57.0, -50.5), LANDING_TOP);
    l.cut([-1.5, LANDING_TOP, -51.01], [1.5, LANDING_TOP + 3.0, -49.99]);

    l.hint([-8.0, TOWER_TOP, 0.0], [8.0, TOWER_TOP + 3.0, 10.0], "Blue portal on the white floor below, then jump. You'll come out of orange.");
    l.hint([-8.0, LANDING_TOP, -35.0], [8.0, LANDING_TOP + 3.0, -14.0], "Sprint along the red wall, jump at it, and run along it across the gap.");
    l
}
