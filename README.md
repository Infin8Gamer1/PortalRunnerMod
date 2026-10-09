# Portal Runner

Mirror's Edge's parkour with Portal's portal gun and test chambers, in Rust and Bevy.

Faith runs, vaults, wallruns, climbs and rolls exactly as she does in Mirror's Edge (the
movement is [Faith Runner](https://github.com/tnrjns/faith-runner)'s `faith_move`, a port of
the game's own code). On top of that she carries the Aperture Science Handheld Portal Device:
blue and orange portals, momentum through them, cubes, buttons, doors, emancipation grills,
aerial faith plates, hard light bridges, lasers and Portal 2's gels.

A personal project, not for distribution. Nothing from Portal, Portal 2 or Mirror's Edge is in
this repository: the game reads Mirror's Edge's files from your install at startup, and copies
what it uses from Portal and Portal 2 out of your installs with the extractor below.

## Run it

You need Rust (https://rustup.rs) and, for the real look and sound, your own copies of Portal,
Portal 2 and Mirror's Edge (Steam). Everything has a stand-in if a game is missing.

```
cargo run --release -p portal_extract   # once: Portal/Portal 2 sounds, textures, models -> assets/extracted/
cargo run --release
```

The extractor finds the games in the usual Steam folders (Linux and Windows); otherwise pass
`--portal <dir>` and `--portal2 <dir>`. Run it again after pulling changes that add models: the
puzzle pieces (buttons, cube sockets, pedestal buttons, cubes, droppers, chamber doors, grill
emitters, laser emitters, catchers and relays, light bridge emitters) use Portal 2's models
when they're extracted, and stand-ins when they aren't. `--inspect <model.mdl>...` describes
models (parts, bones, size) instead of extracting. Mirror's Edge is found the same way, or set
`FAITH_ME_DIR` (or put its path in `me_path.txt`).

It opens on the title screen: **Continue** (or **Play**) picks up at the last chamber you were
in, **Chambers** lists them all with your best times, and **Settings** and **Controls** are
there too. In game, **Esc** (or Start) pauses. Progress is saved to `progress.txt`.

## Controls

| Action | Keyboard / mouse | Controller |
|---|---|---|
| Move / look | WASD / mouse | Left stick / right stick |
| Blue portal / orange portal | Left mouse / right mouse | RT / RB |
| Pick up, drop, press a button | E | X |
| Jump, vault, wallrun, wallclimb, pull up | Space | A or LB |
| Crouch, slide, roll (tap just before landing) | Shift, C or Ctrl | B or LT |
| Kick, punch, barge a door | F | R3 |
| 180° turn | Q | Y |
| Dodge | A or D + Space | |
| Reaction Time (slow motion) | Left Alt | L3 |
| Respawn at checkpoint / checkpoint 1-6 | R / 1-6 | Select |
| Restart chamber / next map / previous map | Backspace / M / N | |
| Pause menu / controls | Esc / F1 | Start |
| Debug readout (state, speed, fps) / Mirror's Edge body on/off | F3 / F2 | |

Holding a cube, either trigger lets go of it with a push. Menus: arrows or WASD, Enter, Esc;
the d-pad, A and B; or the mouse.

## The chambers

M and N cycle through them; finishing one moves on to the next.

1. **Proving Grounds**: a sandbox with a station for everything: a cube dropper, button and
   door; a faith plate up to a platform (and a zipline back down); gel pipes switched by
   pedestal buttons; a laser to redirect into a catcher, which turns on a light bridge over a
   goo pit; a wall to wallrun.
2. **Chamber 01: Portals**: the cube is on a ledge you can't climb. Portals up, cube down,
   button, door, an emancipation grill.
3. **Chamber 02: Momentum**: a blue-only gun and a fixed orange. Drop off the tower (the fall
   alone would kill you) into a blue portal and fling out of orange over the goo; then wallrun
   the last gap.
4. **Chamber 03: Hard Light**: send the light bridge through your portals across the goo, take
   the faith plate up for the redirection cube, aim the laser into its catcher.
5. **Chamber 04: Gel Runner**: propulsion gel for a jump no one could make on foot, repulsion
   gel up a wall too tall to climb, conversion gel to put a portal on a black floor.
6. **Chamber 05: Rooftop Run**: outside. Slide, wallrun, climb the water tower, then a fling off
   it over the last gap. Roll the landing.

Then Mirror's Edge's own courses from Faith Runner (Rooftops, Moves, Springboard, Training),
where every roof and wall takes portals.

Each chamber is beaten start to finish by a scripted test (`cargo test -p aperture`), so a
change that makes one impossible is caught.

## How the portal rules work

- Portals go on **white** surfaces (and on white conversion gel). Black metal, glass, grating,
  red runner-vision surfaces, cubes and light bridges don't take them. A portal that hangs off
  an edge or overlaps the other is nudged to fit, as in Portal; if there's no room, it fizzles.
- Shots go through open portals, through grating, and not through glass or emancipation grills.
- Walking through a portal keeps your speed and turns it with you; falling into a floor portal
  flings you out of the other one. Falling at a portal, you're steered into it (Portal's
  funnel; it can be switched off).
- Going through an emancipation grill closes your portals; carrying a cube through one
  vaporises it (its dropper makes a new one).
- Cubes, gel, lasers and light bridges all go through portals too. A redirection cube sends a
  laser out of its front: turn it by turning while you hold it.
- Repulsion gel (blue) bounces you, off floors and walls; hold crouch not to bounce. Propulsion
  gel (orange) lets you run at up to 17 m/s. Conversion gel (white) takes portals. Gel coats
  cubes too: a blue cube bounces.
- Falls follow Mirror's Edge: a hard landing (unless you roll) from 5.3 m, death from 10 m. Both
  heights are in the settings (the lethal one can be turned off). Bouncing on repulsion gel
  never counts as a landing, and the fall restarts at a portal's exit.

## Settings

In pages: **Audio** (master, music, effects), **Controls** (mouse sensitivity, field of view),
**Gameplay** (**Hard landing from** and **Lethal fall from**, metres, Mirror's Edge's 5.3 and
10, lethal can be Off; **Portal recursion**, how many portals deep you can see, 1-4; **Portal
funnel**; **Skid delay off speed gel**; **Chamber timer**), **Display** (window: windowed,
borderless or fullscreen; window size; VSync; a frame-rate cap; brightness; **Speed FOV
increase**, **Speed lines**, **Portal gun model**) and **Graphics** (anti-aliasing: off, FXAA,
SMAA, MSAA 4x or TAA; sharpening; the portal views' resolution; shadow quality). They apply at
once and are saved to `settings.txt` where you run the game. (`PORTAL_NOVSYNC=1` still forces
VSync off.)

## Making levels

Levels are Rust code: see [docs/levels.md](docs/levels.md) for the builder, every piece you
can place, and how to give a level its own solution test.

## Layout

```
crates/faith_move/      the movement (Faith Runner): engine-independent, glam only
crates/faith_anim/      Mirror's Edge's animation driver for Faith's body (Faith Runner)
crates/me_assets/       Mirror's Edge package readers (Faith Runner)
crates/aperture/        portals and the puzzle pieces, engine-independent:
  portal.rs               portals and the transform through them
  collide.rs              collision through portals (hole and sleeve per body)
  sim.rs                  the running chamber: gun, Faith through portals, cubes, buttons,
                          doors, grills, plates, bridges, lasers, gel
  gel.rs                  the paint grid and gel blobs
  level.rs                the level description and its builder
  levels/                 the chambers (kit.rs: shared pieces)
  tests.rs                the mechanics, and each chamber beaten by script
crates/portal_extract/  copies sounds, textures (VTF -> PNG) and models (MDL -> GLB) out of
                        your Portal and Portal 2 VPKs
src/main.rs             the Bevy app: input, the frame's order, level loading, lighting
src/portal_render.rs    recursive portal views (oblique near plane), portal surfaces
src/chamber.rs          drawing a chamber and its pieces
src/gun.rs              shot tracers, sparks, the hold beam (and the optional gun model)
src/hud.rs              crosshair, hints, messages, timer, debug readout
src/menu.rs             title, pause, chamber select, settings pages, controls
src/speed_fx.rs         speed FOV kick, speed lines, skid dust
src/graphics.rs         applying the display and graphics settings, the frame-rate cap
src/fonts.rs            the UI font (Inter from the system, if it's there)
src/progress.rs         best times and the last chamber, saved
src/portal_audio.rs     Portal's sounds
src/audio.rs            Mirror's Edge's sounds (Faith Runner)
src/me_viewmodel.rs     Faith's Mirror's Edge body (Faith Runner)
src/settings.rs         the settings and their pages
src/capture.rs          scripted screenshot runs (see below)
docs/faith-runner.md    Faith Runner's own README: every move, in detail
```

## Checking changes

```
cargo test -p aperture -p faith_move          # mechanics, chambers, movement
PORTAL_CAPTURE=shots PORTAL_SCRIPT=portals cargo run --release
```

The capture mode plays a scripted run and saves screenshots into the folder: scripts are
`portals`, `stations`, `recursion`, `fling`, `gun` and `tour` (each map's start), in
`src/capture.rs`. `PORTAL_MAP=<n>` starts on map n; `PORTAL_NOVSYNC=1` uncaps the frame rate.

## Credits

The movement, Faith's body and her sounds are Faith Runner's
(https://github.com/tnrjns/faith-runner). Portal, Portal 2 and Mirror's Edge belong to Valve
and EA; this project only reads your own copies.
