//! Pieces chambers are built from.

use crate::level::{Inputs, Level, Mat};

/// Ceiling lamps over a rectangle, `every` metres apart, just under the ceiling at `y`.
pub fn lamps(l: &mut Level, x: (f32, f32), z: (f32, f32), y: f32, every: f32) {
    let nx = ((x.1 - x.0) / every).round().max(1.0) as i32;
    let nz = ((z.1 - z.0) / every).round().max(1.0) as i32;
    for i in 0..nx {
        for j in 0..nz {
            let px = x.0 + (i as f32 + 0.5) * (x.1 - x.0) / nx as f32;
            let pz = z.0 + (j as f32 + 0.5) * (z.1 - z.0) / nz as f32;
            l.lamp([px, y - 0.05, pz]);
        }
    }
}

/// The way out: a small room (inside `x`, `z`, floor at `y`) with the exit marker at its
/// back. Cut the doorway into it yourself.
pub fn exit_room(l: &mut Level, x: (f32, f32), z: (f32, f32), y: f32) {
    l.room([x.0, y, z.0], [x.1, y + 3.5, z.1], 0.5, Mat::Black, Mat::Black, Mat::Black);
    let (cx, cz) = ((x.0 + x.1) * 0.5, (z.0 + z.1) * 0.5);
    l.exit([cx - 1.0, y, cz - 1.0], [cx + 1.0, y + 3.0, cz + 1.0]);
    l.lamp([cx, y + 3.4, cz]);
}

/// A doorway through a wall that runs along X (its faces at `z0` and `z1`), centred on
/// `x`, `w` wide and `h` high from `y`: cut, with an emancipation grill across it.
pub fn grill_door_x(l: &mut Level, x: f32, y: f32, z0: f32, z1: f32, w: f32, h: f32) {
    l.cut([x - w * 0.5, y, z0.min(z1) - 0.01], [x + w * 0.5, y + h, z0.max(z1) + 0.01]);
    let zm = (z0 + z1) * 0.5;
    l.fizzler([x - w * 0.5, y, zm - 0.05], [x + w * 0.5, y + h, zm + 0.05], Inputs::always());
}

/// The same for a wall that runs along Z (faces at `x0`, `x1`), centred on `z`.
pub fn grill_door_z(l: &mut Level, z: f32, y: f32, x0: f32, x1: f32, w: f32, h: f32) {
    l.cut([x0.min(x1) - 0.01, y, z - w * 0.5], [x0.max(x1) + 0.01, y + h, z + w * 0.5]);
    let xm = (x0 + x1) * 0.5;
    l.fizzler([xm - 0.05, y, z - w * 0.5], [xm + 0.05, y + h, z + w * 0.5], Inputs::always());
}
