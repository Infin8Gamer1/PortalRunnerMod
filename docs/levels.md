# Making levels

A level is a function returning an `aperture::Level`, built with its builder methods. Add a
file under `crates/aperture/src/levels/`, write the function, and list it in
`levels::all()` (in `levels/mod.rs`): the game cycles through that list in order.

```rust
use super::kit;
use crate::level::{CubeKind, Inputs, Level, Mat};

pub fn chamber06() -> Level {
    let mut l = Level::new("Chamber 06: Example");
    l.intro = "Shown under the name when the chamber starts.";
    // A white room 12 x 6 x 20 m inside, walls 1 m thick.
    l.room([-6.0, 0.0, -20.0], [6.0, 6.0, 0.0], 1.0, Mat::White, Mat::White, Mat::Black);
    l.spawn([0.0, 0.0, -2.0], 0.0);
    kit::lamps(&mut l, (-6.0, 6.0), (-20.0, 0.0), 6.0, 6.0);

    l.cube(CubeKind::Storage, [3.0, 0.5, -6.0]);
    l.button(0, [-3.0, 0.0, -10.0]);
    l.cut([-1.5, 0.0, -21.01], [1.5, 3.0, -19.99]);          // a doorway in the back wall
    l.door(Inputs::on(0), [-1.5, 0.0, -20.6], [1.5, 3.0, -20.4]);
    kit::exit_room(&mut l, (-3.0, 3.0), (-27.0, -21.0), 0.0);
    l.cut([-1.5, 0.0, -21.6], [1.5, 3.0, -20.4]);             // through the exit room's wall too
    l
}
```

## Conventions

- Metres, Y up. Faith's yaw 0 looks down **-Z**; 90° looks down -X (yaw turns left). `spawn`
  and `checkpoint` take degrees.
- Faith is 1.8 m tall and 0.6 m wide (1.22 m crouched). She jumps about 1.2 m, sprints at
  7.2 m/s and long-jumps about 6 m. She vaults things up to 1.48 m, wallclimbs walls about
  3.3 m high, and steps up 35 cm without jumping. Gaps you mean to be impossible on foot should
  be 9 m or more; walls, 5 m or more.
- Gravity is 16.4 m/s² (Mirror's Edge's). Falls of 5.3 m are hard landings and 10 m kill, by
  default (players can change both).
- A portal is 1.4 x 2.4 m. Walls meant to take one need at least that much flat white, and a
  wall portal you walk into needs its white to reach the floor (the portal's bottom sits on
  it).
- A cube is 0.9 m. A floor button is 1.9 m across (its top 12 cm up).

## Building geometry

| Method | What it does |
|---|---|
| `room(a, b, t, floor, walls, ceiling)` | A closed box room: the inside from `a` to `b`, walls `t` thick. |
| `solid(mat, a, b)` | A box from corner `a` to corner `b`. |
| `cut(a, b)` | Remove a box from every solid: doorways, windows, pits. Order matters: it cuts what's there already. |
| `repaint(mat, a, b)` | Change the material of every solid inside a box (a strip of black across a white wall). |
| `decor(mat, a, b)` | A painted box with no collision. |
| `lamp(at)` / `kit::lamps(...)` | Ceiling lights (indoors). |

Materials (`Mat`): **White** (takes portals), **Black** (doesn't), **Glass** (see-through,
blocks shots and lasers), **Grate** (blocks people and cubes; shots and lasers pass),
**Runner** (Mirror's Edge red), **Prop**, **Concrete** (rooftop, no portals), and **Roof** /
**Wall** (Mirror's Edge concrete that takes portals). Up-facing white and black surfaces get
floor tiles automatically.

Set `l.indoor = false` for an outdoor level (sun, sky, Mirror's Edge music) and `l.kill_y`
for how far down falling out of the world kills.

## Puzzle pieces

| Method | |
|---|---|
| `button(sig, at)` / `cube_button(sig, at)` | Floor button on the floor point `at`; drives signal `sig` while a cube (or Faith, unless cube-only) is on it. |
| `pedestal(sig, at, time)` | A button on a post; E presses it. On for `time` seconds (0: toggles). |
| `door(inputs, a, b)` | Open while its inputs are on. Slides apart along its longer side. |
| `cube(kind, centre)` | A cube. `CubeKind::Storage`, `Companion`, `Reflector` (redirects lasers). |
| `dropper(kind, centre, inputs)` | Drops a cube when its inputs come on (always on: at the start), and a new one when that one's destroyed. |
| `fizzler(a, b, inputs)` | Emancipation grill (a thin box). |
| `faith_plate(at, target, apex)` | Throws whoever steps on it to land on `target`, `apex` metres above the higher end. |
| `bridge(at, dir, inputs)` | Hard light bridge from the emitter face `at` along `dir` (walkable side up). |
| `laser(at, dir, inputs)` | Laser from `at` along `dir`. |
| `catcher(sig, at, facing)` | Laser catcher; its lens at `at` facing `facing`. On while lit. |
| `relay(sig, at)` | Laser relay standing on the floor; on while a laser passes through it. |
| `gel_pipe(paint, at, dir, inputs)` | Pours `Paint::Bounce`, `Speed`, `Portal` (conversion) or `Water` (cleans). |
| `paint(paint, a, b, normal)` | Gel already painted on a flat region (`a`-`b` flat along `normal`). |
| `goo(a, b)` | Deadly. Cubes that fall in are lost (droppers replace them). |
| `portal(color, centre, normal, up)` | A portal placed by the level (pair with `l.gun = Gun { blue: true, orange: false }` for Portal 1's early chambers). |
| `exit(a, b)` / `kit::exit_room(...)` | Walking in finishes the chamber. |
| `checkpoint(name, a, b, at, yaw)` | Walking into `a`-`b` makes `at` the respawn point. |
| `hint(a, b, text)` | Text shown while Faith stands in `a`-`b`. |
| `wire(sig, points)` | Indicator lights along a path (they light up with the signal). |
| `l.fixtures.push(Fixture::...)` | Mirror's Edge's ziplines, swing poles, balance beams, ladders. |

**Signals** are numbered wires (`Sig`, from 0). Sources (buttons, pedestals, catchers, relays)
drive one; listeners take `Inputs`: `Inputs::always()`, `Inputs::on(s)`, `Inputs::all(&[a, b])`
(all on), `Inputs::off(s)` (on while `s` is off).

## A solution test

Every chamber has a test in `crates/aperture/src/tests.rs` that plays it through: it walks
Faith about, aims, fires, picks things up and goes through portals using the real movement
and simulation, so a later change that breaks the chamber fails the test. The helpers there
(`walk`, `aim`, `walk_through`, `settle`, `run`) keep them short; see
`chamber01_can_be_solved`. Run them with `cargo test -p aperture`.

The test `every_chamber_builds` also checks every level's spawn isn't inside a wall.

To look at a level, `PORTAL_MAP=<index> cargo run --release` starts on it, and
`PORTAL_CAPTURE=shots PORTAL_SCRIPT=tour` photographs every map's start.
