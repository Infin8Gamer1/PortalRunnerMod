//! Chamber 03, Hard Light: a light bridge runs the wrong way, across the room. Send it
//! through your portals to span the goo, ride the faith plate up for the redirection cube,
//! and turn the laser into its catcher to open the way out.
//!
//! ```text
//!   z -32 +--[catcher]------[door]-+   exit beyond
//!         |                        |
//!         | balcony  laser <-------|  (laser along z -24, from the right wall)
//!         | (cube)    plate        |
//!   z -20 +~~~~~~~~~~~~~~~~~~~~~~~~+
//!         |~~~~~~~~ goo ~~~~~~~~~~~|
//!   z  -6 +~~~~~~~~~~~~~~~~~~~~~~~~+
//!         |emitter=====bridge=====>|  (y 0.3, along z -2)
//!   z  +6 +-------- spawn ---------+
//!       x -10                    x +10
//! ```

use super::kit;
use crate::level::{CubeKind, Inputs, Level, Mat};

pub const BRIDGE_Z: f32 = -2.0;
pub const BRIDGE_Y: f32 = 0.3;
pub const BALCONY_TOP: f32 = 5.0;
pub const CUBE: [f32; 3] = [-8.0, BALCONY_TOP + 0.5, -22.5];
pub const LASER_Z: f32 = -24.0;
pub const CATCHER: [f32; 3] = [-4.0, 0.45, -31.99];
pub const PLATE: [f32; 3] = [2.0, 0.0, -21.5];

pub fn chamber03() -> Level {
    let mut l = Level::new("Chamber 03: Hard Light");
    l.intro = "Hard light bridges go where you send them. So do lasers.";
    l.room([-10.0, 0.0, -32.0], [10.0, 12.0, 6.0], 1.0, Mat::White, Mat::White, Mat::Black);
    l.spawn([0.0, 0.0, 3.0], 0.0);
    kit::lamps(&mut l, (-10.0, 10.0), (-32.0, 6.0), 12.0, 9.5);

    // The bridge, running across the start along z -2.
    l.bridge([-9.99, BRIDGE_Y, BRIDGE_Z], [1.0, 0.0, 0.0], Inputs::always());

    // The goo pit (its walls black: nothing to portal down there).
    l.cut([-10.0, -1.01, -20.0], [10.0, 0.01, -6.0]);
    l.solid(Mat::Black, [-11.0, -4.0, -21.0], [11.0, -3.0, -5.0]);
    l.solid(Mat::Black, [-11.0, -3.0, -21.0], [11.0, -1.0, -20.0]);
    l.solid(Mat::Black, [-11.0, -3.0, -6.0], [11.0, -1.0, -5.0]);
    l.goo([-10.0, -3.0, -20.0], [10.0, -1.2, -6.0]);
    l.repaint(Mat::Black, [-11.0, 0.0, -20.0], [11.0, 12.0, -6.0]);
    // The far side takes no portals either: the bridge is the way across.
    l.repaint(Mat::Black, [-11.0, -1.0, -33.0], [11.0, 13.0, -20.0]);

    // The far side: a balcony with the redirection cube, a faith plate up to it.
    l.solid(Mat::Black, [-10.0, 0.0, -26.0], [-6.0, BALCONY_TOP, -20.0]);
    l.cube(CubeKind::Reflector, CUBE);
    l.faith_plate(PLATE, [-8.0, BALCONY_TOP, -24.0], 2.0);
    l.checkpoint("Far side", [-6.0, 0.0, -32.0], [10.0, 3.0, -20.0], [6.0, 0.0, -22.0], 0.0);

    // The laser along z -24, into the balcony's side; the catcher on the back wall.
    l.laser([9.99, 0.45, LASER_Z], [-1.0, 0.0, 0.0], Inputs::always());
    l.catcher(0, CATCHER, [0.0, 0.0, 1.0]);
    l.cut([4.0, 0.0, -33.01], [7.0, 3.0, -31.99]);
    l.door(Inputs::on(0), [4.0, 0.0, -32.6], [7.0, 3.0, -32.4]);
    l.wire(0, &[[-4.0, 0.03, -31.97], [-3.0, 0.03, -31.97], [3.9, 0.03, -31.97]]);
    kit::exit_room(&mut l, (2.5, 8.5), (-39.0, -33.0), 0.0);
    // An observation room over the far side, looking down on the goo.
    kit::observation_room(&mut l, kit::Along::X, -3.0, 7.5, -32.0, -33.0, 5.0, 2.2, 3.5, false);
    l.cut([4.0, 0.0, -33.6], [7.0, 3.0, -32.4]);

    l.hint([-10.0, 0.0, -6.0], [10.0, 4.0, 6.0], "The bridge goes into a portal where it meets the wall. Point the other portal across the goo.");
    l.hint([-6.0, 0.0, -32.0], [10.0, 4.0, -20.0], "Take the faith plate up for the redirection cube. Put it in the laser, facing the catcher.");
    l
}
