//! Proving Grounds: one big white hall with a station for every piece, to try things out.
//!
//! ```text
//!            back wall (black in the middle; white gel turns it white)
//!   +------------------------------------------------+  z -24
//!   | platform (6 m)  |  gel pipes  |   goo pit       |
//!   |  pedestal       |   B  O  W   |  light bridge   |
//!   |                 |             |  (laser turns   |
//!   |  faith plate    | pedestals   |   it on)        |
//!   |                 |             |                 |
//!   |                 |  wallrun    |  laser -> cube  |
//!   |  button -> door |  wall       |   -> catcher    |
//!   |  dropper        |  spawn      |  dropper        |
//!   +------------------------------------------------+  z +24
//!  x -24                                            x +24
//! ```

use faith_move::Fixture;
use glam::Vec3;

use super::kit;
use crate::level::{CubeKind, Inputs, Level, Mat, Paint};

pub fn arena() -> Level {
    let mut l = Level::new("Proving Grounds");
    l.intro = "Every piece of the facility, to try out. Esc for the menu, F1 for the controls.";
    l.room([-24.0, 0.0, -24.0], [24.0, 12.0, 24.0], 1.0, Mat::White, Mat::White, Mat::White);
    l.repaint(Mat::Black, [-8.0, 0.0, -25.0], [8.0, 12.0, -24.0]);
    l.spawn([0.0, 0.0, 18.0], 0.0);
    kit::lamps(&mut l, (-24.0, 24.0), (-24.0, 24.0), 12.0, 12.0);
    l.hint([-6.0, 0.0, 12.0], [6.0, 4.0, 24.0], "Left mouse: blue portal   Right mouse: orange portal   E: pick up / use");

    // ---- A cube, a button, a door (left, front). The door leads out.
    l.dropper(CubeKind::Storage, [-16.0, 5.0, 17.0], Inputs::always());
    l.button(0, [-16.0, 0.0, 8.0]);
    // A cube-only button beside it (Faith standing on it does nothing).
    l.cube_button(7, [-11.5, 0.0, 8.0]);
    l.cut([-25.01, 0.0, 9.0], [-23.99, 3.0, 12.0]);
    l.door(Inputs::on(0), [-24.6, 0.0, 9.0], [-24.4, 3.0, 12.0]);
    l.wire(0, &[[-16.0, 0.03, 9.0], [-16.0, 0.03, 10.5], [-24.0, 0.03, 10.5]]);
    kit::exit_room(&mut l, (-31.0, -25.0), (7.0, 14.0), 0.0);
    l.hint([-22.0, 0.0, 4.0], [-10.0, 4.0, 20.0], "A cube on the button opens the door. E picks a cube up and puts it down.");

    // ---- An aerial faith plate up onto a platform (left, back).
    l.solid(Mat::Black, [-24.0, 0.0, -24.0], [-10.0, 6.0, -14.0]);
    l.faith_plate([-14.0, 0.0, -4.0], [-16.0, 6.0, -18.0], 2.5);
    l.checkpoint("Platform", [-24.0, 6.0, -24.0], [-10.0, 9.0, -14.0], [-14.0, 6.0, -16.0], 180.0);
    l.hint([-18.0, 0.0, -8.0], [-10.0, 3.0, 0.0], "Step onto the Aerial Faith Plate.");
    // A zipline down from it.
    l.fixtures.push(Fixture::ZipLine { a: Vec3::new(-11.0, 8.2, -15.0), b: Vec3::new(-2.0, 3.0, 2.0) });

    // ---- Gel pipes in the ceiling (middle, back), each switched by a pedestal button.
    l.gel_pipe(Paint::Bounce, [-5.0, 11.6, -12.0], [0.0, -1.0, 0.0], Inputs::on(4));
    l.gel_pipe(Paint::Speed, [1.0, 11.6, -4.0], [0.0, -1.0, 0.0], Inputs::on(5));
    l.gel_pipe(Paint::Portal, [0.0, 8.0, -20.0], [0.0, -0.2, -1.0], Inputs::on(6));
    l.pedestal(4, [-5.0, 0.0, -7.0], 0.0);
    l.pedestal(5, [1.0, 0.0, 1.0], 0.0);
    l.pedestal(6, [4.0, 0.0, -16.0], 0.0);
    l.wire(4, &[[-5.0, 0.03, -7.0], [-5.0, 0.03, -10.5]]);
    l.wire(5, &[[1.0, 0.03, 1.0], [1.0, 0.03, -2.5]]);
    l.hint([-8.0, 0.0, -18.0], [6.0, 4.0, 2.0], "Use the buttons to start the gel: blue bounces, orange runs, white takes portals.");
    // Some blue gel to start with, under the platform's edge.
    l.paint(Paint::Bounce, [-9.5, 0.0, -13.5], [-6.5, 0.0, -10.5], [0.0, 1.0, 0.0]);

    // ---- A goo pit with a light bridge over it (right, back): the laser below turns it on.
    l.cut([6.0, -1.01, -22.0], [22.0, 0.01, -12.0]);
    l.solid(Mat::Black, [5.0, -4.0, -23.0], [23.0, -3.0, -11.0]);
    for (a, b) in [
        ([5.0, -3.0, -23.0], [6.0, -1.0, -11.0]),
        ([22.0, -3.0, -23.0], [23.0, -1.0, -11.0]),
        ([6.0, -3.0, -23.0], [22.0, -1.0, -22.0]),
        ([6.0, -3.0, -12.0], [22.0, -1.0, -11.0]),
    ] {
        l.solid(Mat::Black, a, b);
    }
    l.goo([6.0, -3.0, -22.0], [22.0, -1.2, -12.0]);
    l.bridge([14.0, 0.0, -23.99], [0.0, 0.0, 1.0], Inputs::on(2));
    l.hint([6.0, 0.0, -11.0], [22.0, 3.0, -6.0], "The light bridge comes on when the laser reaches its catcher.");

    // ---- A laser, a redirection cube, a relay and a catcher (right, front).
    l.laser([23.99, 0.45, 4.0], [-1.0, 0.0, 0.0], Inputs::always());
    l.solid(Mat::Black, [8.0, 0.0, 2.0], [10.0, 4.0, 6.0]);
    l.dropper(CubeKind::Reflector, [18.0, 5.0, 17.0], Inputs::always());
    l.relay(3, [17.0, 0.0, 8.0]);
    l.catcher(2, [17.0, 0.45, 12.0], [0.0, 0.0, -1.0]);
    l.hint([10.0, 0.0, 0.0], [24.0, 4.0, 16.0], "Carry the redirection cube into the laser and turn it towards the catcher.");

    // ---- Mirror's Edge: a red wall to wallrun, a ledge to climb.
    l.solid(Mat::Runner, [-4.0, 0.0, 4.0], [-3.6, 4.0, 12.0]);
    l.solid(Mat::Runner, [4.0, 0.0, 10.0], [7.0, 1.2, 12.0]);
    l
}
