//! Chamber 05, Rooftop Run: out on the roofs, Mirror's Edge style, with Aperture's panels
//! bolted on. Wallrun the billboard across the first gap; put blue on the white back of the
//! far billboard; climb the water tower; put orange on the white patch below it and drop
//! (a fall that would kill you) into it, to fly over the last gap. Roll as you land.
//!
//! ```text
//!   side view:                  tower top (6)
//!   roof A (0)                    |#|
//!   spawn  AC  pipe   gap 1   roof B (-2)  [patch]  [billboard]|   gap 2     roof C (-4)  finish
//!   z: 4   -5  -14    -24..-34       -46            -57  -62      -72            -98
//! ```

use glam::Vec3;

use faith_move::climb::Ladder;
use faith_move::Fixture;

use crate::level::{Level, Mat};

pub const ROOF_B: f32 = -2.0;
pub const ROOF_C: f32 = -4.0;
pub const TOWER_TOP: f32 = 6.0;
pub const BILLBOARD_Z: f32 = -57.0;
pub const RUN_WALL_X: f32 = 5.6;

pub fn chamber05() -> Level {
    let mut l = Level::new("Chamber 05: Rooftop Run");
    l.intro = "Outside the facility. The panels came with you.";
    l.indoor = false;
    l.kill_y = -18.0;

    // Roof A, with an AC unit to vault and a pipe to slide under.
    l.solid(Mat::Concrete, [-6.0, -20.0, -24.0], [6.0, 0.0, 8.0]);
    l.spawn([0.0, 0.0, 4.0], 0.0);
    l.solid(Mat::Prop, [-1.5, 0.0, -6.0], [1.5, 1.0, -5.0]);
    l.solid(Mat::Runner, [-6.0, 1.3, -14.0], [6.0, 1.6, -13.6]);

    // Gap 1, and the red billboard along it to wallrun.
    l.solid(Mat::Runner, [RUN_WALL_X, -2.0, -36.0], [6.0, 6.0, -22.0]);

    // Roof B: the water tower (a ladder up its side), a white patch below it, and at the far
    // end a billboard whose back is white.
    l.solid(Mat::Concrete, [-6.0, -20.0, -62.0], [6.0, ROOF_B, -34.0]);
    l.checkpoint("Roof B", [-6.0, ROOF_B, -44.0], [6.0, ROOF_B + 3.0, -34.0], [0.0, ROOF_B, -38.0], 0.0);
    l.solid(Mat::Concrete, [-6.0, ROOF_B, -48.0], [-2.0, TOWER_TOP, -44.0]);
    // The ladder's on the side facing back up the roof, away from the jump.
    l.fixtures.push(Fixture::Ladder(Ladder { base: Vec3::new(-4.0, ROOF_B, -44.0), top: TOWER_TOP, normal: Vec3::Z, pipe: false, exit: true }));
    l.repaint(Mat::White, [-1.0, ROOF_B - 0.5, -50.0], [3.0, ROOF_B, -42.0]);
    l.solid(Mat::Concrete, [-4.0, ROOF_B, BILLBOARD_Z], [4.0, 1.0, BILLBOARD_Z + 0.4]);
    l.solid(Mat::White, [-4.0, 1.0, BILLBOARD_Z], [4.0, 7.0, BILLBOARD_Z + 0.4]);
    l.checkpoint("Tower", [-6.0, TOWER_TOP, -48.0], [-2.0, TOWER_TOP + 3.0, -44.0], [-4.0, TOWER_TOP, -46.0], -90.0);

    // Gap 2, too wide to cross on foot, and roof C far below.
    l.solid(Mat::Concrete, [-10.0, -20.0, -110.0], [10.0, ROOF_C, -72.0]);
    l.solid(Mat::Prop, [-8.0, ROOF_C, -106.0], [8.0, ROOF_C + 2.5, -104.0]);
    l.checkpoint("Roof C", [-10.0, ROOF_C, -104.0], [10.0, ROOF_C + 3.0, -72.0], [0.0, ROOF_C, -90.0], 0.0);
    l.exit([-3.0, ROOF_C, -100.0], [3.0, ROOF_C + 3.0, -96.0]);

    l.hint([-6.0, 0.0, -24.0], [6.0, 3.0, 8.0], "Vault the unit, slide under the pipe, run the red billboard over the gap.");
    l.hint([-6.0, ROOF_B, -62.0], [6.0, ROOF_B + 3.0, -50.0], "The billboard's back is white. Blue there. Then up the water tower.");
    l.hint([-6.0, TOWER_TOP, -48.0], [-2.0, TOWER_TOP + 3.0, -44.0], "Orange on the white patch below. Jump. Crouch just before you land to roll.");
    l
}
