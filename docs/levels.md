# Making levels

There are two ways: the level editor in the game, and Rust code (how the game's own chambers
are made, each with a test that plays it through).

## The level editor

**F4** in the game, or **Level editor** in the main or pause menu, opens the editor
(`PORTAL_EDIT=1` starts in it). The first time, it opens on a copy of the chamber you're in
(one of Portal's chambers: below). **New level** in the menus starts a new
one: a white room with lamps and a start.

- **Content** (left): the materials, puzzle pieces, lasers and bridges, gel, gameplay pieces
  (start, checkpoints, exit, hints, goo, platforms, portals, indicator wires), lights (point
  and spot lights, a sun, ceiling lamps), parkour
  (ziplines, swing poles, balance beams, ladders, drainpipes, barge doors, barbed wire, soft
  landings), and **Props**: every prop model of Portal and Portal 2 (extracted into `props/`)
  and every static mesh in Mirror's Edge's prop packages (from your install), by game and
  folder, with pictures made the first time their tiles show (kept under `thumbs/`). A prop
  lands with its front to the camera, resting on the surface, and is solid (a block round it)
  unless you untick that. Props turn about all three axes and scale along each (the details
  panel, or the gizmo: **R** scales along the prop's own axes; the toolbar's World/Local
  switches what moving and turning go along). The details panel says how many of a prop's
  materials have a texture (or, for glass and plain-coloured materials, a colour). Drag one into the view, or click it and then click in
  the view (Shift-click to place several; Esc stops): it lands on the surface under the cursor
  (a block rests on it, a laser or catcher faces out of it, a door stands across the way you're
  looking, a ladder's foot goes down to the floor), snapped to the grid, and a green outline
  shows where before you let go. Double-click puts it where the middle of the view is.
- **The view**: hold the right mouse button to look round, with WASD to fly, Q/E down and up,
  Shift faster, the wheel for speed (without the button, the wheel moves in and out). Click
  to select; **W/E/R** move, rotate and scale it with the gizmo (scale sizes boxes). **F**
  focuses on it, **Del** deletes it, **Ctrl+D** duplicates it, **Ctrl+Z / Ctrl+Y** undo and
  redo. Something with a second point (a zipline's or swing pole's other end, a beam's, where a
  faith plate lands, where a platform travels to) gets a second, smaller handle there: drag its
  arrows to move that end along an axis, or its ring to move it about. Lines show what the
  selection is wired to; outlines show what the game doesn't draw
  (the start, checkpoints, the exit, hints, cuts, a faith plate's arc when it's selected).
- **Lights**: a point light shines all round, a spot light in a cone the way it faces (turn it
  with the gizmo); each has a colour, an intensity in lumens (the ceiling lamps are 900,000), a
  range, a size (bigger is softer) and shadows on or off. A sun replaces the level's (its
  colour, brightness in lux, and the way it faces: where it stands doesn't matter). The
  level's settings (nothing selected) can set its own ambient (fill) light. With ray-traced
  lighting, which lights by what glows, each light is also a glowing bulb only the traced
  light sees (a spot light's in a short black tube that shapes its cone).
- **View** (toolbar): the level's meshes, collision (the boxes Faith and cubes stop against,
  coloured by what they are), markers, every signal's links, the grid, Faith, fog; lit or
  unlit; and ray-traced lighting, on or off straight away (the game's setting too).
- **Outliner** and **details** (right): every object; the selected one's position, size,
  facing and settings (material, signal, what turns it on, cube kind, gel...). With nothing
  selected, the level's name, intro line, indoors or out, and which portals the gun shoots.
- **Play** (**F5**, or **F4**) plays the level from its start with the real simulation; **F4**
  comes back to the editor.
- **File**: a new room or an empty level, open a level file or one of Portal's
  chambers, import any of the game's chambers (as its boxes and pieces, after the cuts), save
  (**Ctrl+S**).
- **Outliner**: filter by name or label, or show only brushes, entities or the rest (a Portal
  map has thousands of objects).

### Portal's chambers

Each of Portal's maps is a level file in the repository (`levels/portal/`, made by
`portal_extract -- --port-maps` from the extracted map), in the Chambers menu's **Portal 1
chambers** folder and under **File > Open a Portal chamber**. It's made the way the map was
built, and plays as Portal's does (the solution tests play every chamber on it):

- **Brushes**: each of the map's brushes, a convex solid whose every face has one of Portal's
  materials (where its texture lies goes with it: move a brush and its texture moves too).
  The details panel shows its contents (solid, window, grate, goo, water, clip, empty), what
  it collides as (white where at least half its drawn faces take portals), and each face's
  material: **Pick…** opens Portal's materials, with pictures. Scale it with the gizmo
  (**R**); its textures keep their size. Faces with Source's tool textures (nodraw, clip,
  trigger) aren't drawn; nor is a face inside another solid brush.
- **Entities**: each of the map's entities, Source's: its class, its name, its keys, and its
  **outputs** (when it fires an event, an input sent after a delay to the entities of a name:
  `OnTrigger` → `door.Open`). The details panel lists them all to change, and what fires at
  it (click one to go to it); in the view, lines show what the selection's outputs reach
  (green) and what fires at it (cyan). A brush entity (a door, a trigger, a wall that comes
  and goes) has its brushes: they move with it. Triggers show as orange outlines, logic as
  small yellow marks. Static props are `prop_static` entities.
- **Displacements** (the escape's dirt mounds) and **overlays** (decals: signs, indicator
  strips; named, the logic switches their frames) are objects too.

**Portal brushes** and **Portal logic** in the content browser make new ones: brushes of
Portal's white panels, concrete, black metal, glass, light panels and tool textures, and
relays, timers, map-start, counters, cases, triggers, sliding doors and toggled walls (brush
entities), GLaDOS scenes, Portal's lights, landmarks, level changes, the portal gun, targets,
fans (`func_rotating`: they spin up and down, and hum), push and hurt volumes, portal detectors
(their outputs fire as a portal's put in them), portal bumpers (which nudge a portal off their
edge) and no-portal volumes, and security cameras (which follow her, and fall when a portal's
opened under them).

**Portal sound & effects** has the rest: sounds (`ambient_generic`: looping or once, heard from
where it is or everywhere, started and stopped by the logic), soundscapes (an area's ambience,
the nearest in sight), particles (the elevator shafts' beams, fire, neurotoxin), steam,
sparks, spotlights' beams, beams between two named targets, screen shakes and fades, colour
correction (one of Portal's tables, weighed in near it or everywhere, faded in and out by the
logic), tonemap controllers (the logic sets the auto exposure's range and rate, and the bloom)
and decals. A sound's message has a **…** beside it: the sound browser, with Portal's
soundscripts and files and Mirror's Edge's cues (named `me:<package>.<group>.<cue>`, read from
its install), to search, listen to (▶) and pick. A soundscape, a particle effect and a colour
correction table have menus of what the game has.

A Portal model from the props browser can be made a physics prop (**Knock it about** in its
details): a `prop_physics` with its model's hull, that falls, is knocked about and can be
picked up as a cube can (one too big to lift stays as it is).

The files hold the maps' layouts, not their assets: materials, models and scenes are named,
and drawn from the extracted folder. Saving one writes its file in `levels/portal/` (commit it
to keep the change). `--port-maps` remakes them from the extracted maps (when the
converter's improved) but keeps one that's been changed (`levels/portal/written.txt` knows each
as it was written); delete it to have it made again. A chamber's level change leads to the next
map's.

### Mirror's Edge's levels

`portal_extract -- --port-me` makes Mirror's Edge's maps into level files (`levels/me/`), read
from your install: the map and the sublevels it streams in. They're in the Chambers menu's
**Mirror's Edge levels** folder, and open in the editor as any level file does (so far the
Training map and its three time trials):

- Its meshes are **Mirror's Edge props**, standing, turned and scaled as the map has them (one
  mirrored is drawn wound the other way round), drawn from the install.
- Each prop collides with its mesh's own collision (**Collision: the mesh's own**), read from
  the game with the mesh: its convex hulls, or for a mesh that has none, its triangles, placed
  wherever the prop is moved (and not drawn as boxes). Nothing of it is in the file; without
  the game a prop collides as its box. Meshes that only have triangles (the city's buildings)
  are solid only within 40 m of where she can go; the sky, the sun's flare and the effects'
  meshes (light shafts) are left out. Mirror's Edge props added from the browser collide the
  same way.
- Its blocking volumes are **hulls** (clip: convex, invisible, only she and cubes bump into
  them); its barbed wire, ladders and drainpipes, zipline, swing poles and balance beam are
  ours, placed where the map's are (a ladder's foot found on the wall behind it, a beam on top
  of what it is).
- Its sun is its brightest directional light; she starts at its start.

As with Portal's, `--port-me` keeps a file that's been changed (`levels/me/written.txt`).

What the browser offers is the list in `src/editor/catalog.rs`: a name, a category, how it sits
on what it's dropped on, a default size, and the object it makes. Adding a kind of thing the
level format doesn't have yet means a new `Kind` in `aperture::doc` (and how it builds into a
`Level`), its fields in the details panel (`src/editor/ui.rs`), then its catalog entry.

New sources (buttons, catchers, relays) get a signal of their own, and a door, bridge or
platform placed after one listens to it: drop a button, then a door, and the button opens it.

Levels are saved as RON in `levels/` (`PORTAL_LEVELS` to put them elsewhere): a list of objects,
an object a line, each a kind, where it is (a box's centre, a piece's point, as the builder
methods below take), a box's size, and which way it faces (degrees); a Portal entity's name and
outputs. They're read when they're loaded, so a saved change shows the next time the level
loads. `aperture::doc` has the format; `aperture::brush` brushes.

```ron
(
    name: "My chamber",
    objects: [
        (kind: Block(mat: White), at: (0.0, -0.5, -8.0), size: (14.0, 1.0, 18.0)),
        (kind: Spawn, at: (0.0, 0.0, -2.0)),
        (kind: FloorButton(sig: 0, cube_only: false), at: (-3.0, 0.0, -6.0)),
        (kind: Door(inputs: (all: [0])), at: (0.0, 1.5, -16.1), size: (3.0, 3.0, 0.2)),
        (kind: Laser(inputs: ()), at: (-6.0, 2.0, -12.0), yaw: -90.0),
    ],
)
```

## In Rust

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
- Gravity is Portal's, 15.24 m/s² (600 units/s²), so Portal's flings carry as they should. A
  level can set its own on its **Spawn** (Gravity, in its details); one of Portal's maps on
  its `info_player_start`, as a `gravity` key in units/s². Falls of 5.3 m are hard landings and
  15 m kill, by default (players can change both).
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
| `me_prop(package, mesh, at, yaw)` | A Mirror's Edge static mesh (`"P_Ventilation", "S_ACUnit_05"`), drawn from the player's install; no collision. |
| `me_prop_solid(package, mesh, at, yaw, a, b)` | The same, with a `Prop` solid `a`-`b` that blocks there (and is drawn instead when the game isn't installed). `cargo run -p me_assets --example list_exports -- <package> StaticMesh` lists a package's meshes; `--example mesh_sizes` gives their sizes. |

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
