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

/// Which way a wall runs: along X (its faces are at fixed z) or along Z (fixed x).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Along {
    X,
    Z,
}

/// An observation room behind a chamber wall, as Portal has over its chambers: a window `w`
/// wide and `h` high, its sill at `y` and its middle at `c` along the wall, through a wall
/// whose faces are at `face` (the chamber's side) and `back`; a dark room `depth` deep behind,
/// lit. Clear glass, or `frosted`.
#[allow(clippy::too_many_arguments)]
pub fn observation_room(l: &mut Level, along: Along, c: f32, y: f32, face: f32, back: f32, w: f32, h: f32, depth: f32, frosted: bool) {
    // (u along the wall, v up, d through it.)
    let at = |u: f32, v: f32, d: f32| match along {
        Along::X => [u, v, d],
        Along::Z => [d, v, u],
    };
    let n = (back - face).signum();
    let (u0, u1) = (c - w * 0.5, c + w * 0.5);
    let far = back + n * depth;
    let (floor, ceiling) = (y - 1.0, y + h + 0.8);
    l.room(at(u0 - 0.5, floor, back.min(far)), at(u1 + 0.5, ceiling, back.max(far)), 0.3, Mat::Black, Mat::Black, Mat::Black);
    l.cut(at(u0, y, face - n * 0.01), at(u1, y + h, back + n * 0.31));
    let mid = (face + back) * 0.5;
    l.solid(if frosted { Mat::Frosted } else { Mat::Glass }, at(u0, y, mid - 0.015), at(u1, y + h, mid + 0.015));
    l.lamp(at(c, ceiling, back + n * depth * 0.5));
}
