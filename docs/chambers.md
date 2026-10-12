# Chambers

**Chambers** in the title menu lists them all, with your best times. In the game, **M** and **N**
go to the next and previous one, and finishing a chamber moves on to the next.

## Testing chambers

The game's own chambers:

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
   the faith plate up for the redirection cube, and aim the laser into its catcher.
5. **Chamber 04: Gel Runner**: propulsion gel for a jump no one could make on foot, repulsion
   gel up a wall too tall to climb, and conversion gel to put a portal on a black floor.
6. **Chamber 05: Rooftop Run**: outside. Slide, wallrun, climb the water tower, then fling off
   it over the last gap. Roll the landing.
7. **Chamber 06: Unstationary Scaffold**: a high energy pellet bounces across the room. Put a
   portal where it hits and the other facing its catcher across the goo; the platform it
   powers carries you over.

Then Mirror's Edge's own courses from Faith Runner (Rooftops, Moves, Springboard, Training),
where every roof and wall takes portals. Levels made in the [level editor](levels.md) are
listed after them.

## Portal's chambers

Portal's test chambers 00-19 and the escape, as level files of the game's own in the
repository (`levels/portal/`). Each holds its map's layout (brushes, entities, props and
overlays) and names the assets it uses, which the extractor copies from your install.

They play as Portal's do:
- **Surfaces**: Portal's textures (and Portal with RTX's materials), white surfaces taking
  portals as in Portal, goo, and the map's props (doors, elevators, droppers, furniture).
- **Logic**: Source's entity I/O runs (relays, timers, counters, triggers), so buttons open
  doors, preset portals come and go, droppers make cubes, elevators ride, and the exit
  elevator ends the chamber.
- **Hazards**: pellets fly from their launchers into their catchers. Turrets watch for you,
  fire when they see you, and tip over when a cube hits them, you run into them, or you kick
  them. The escape's rocket turrets lock on and fire slow rockets: put a portal in front of
  one and send its rocket into the glass, the tubes or GLaDOS.
- **GLaDOS** speaks her lines on cue (her scenes, read from Portal's `scenes.image`).
- **Sound and effects**: each map's sounds play where they are, with its soundscapes'
  ambience. Fans spin, vents push you, and steam, sparks, mist and spotlights' beams show as
  the logic switches them. Security cameras follow you.
- **Physics props** (chairs, mugs, clipboards) can be knocked about and picked up. Portal
  detectors and bumpers work, and each map's colour correction and tonemapping are applied.

Each opens in the [level editor](levels.md), every brush, entity and wire of it there to
change.

Every chamber is played start to finish by a scripted test, so a change that makes one
impossible is caught (see [Checking changes](checking-changes.md)).

## How the portal rules work

- Portals go on **white** surfaces (and on white conversion gel). Black metal, glass, grating,
  red runner-vision surfaces, cubes and light bridges don't take them. A portal that hangs off
  an edge or overlaps the other is nudged to fit, as in Portal; if there's no room, it
  fizzles.
- Shots go through open portals and through grating, but not through glass or emancipation
  grills.
- Walking through a portal keeps your speed and turns it with you. Falling into a floor portal
  flings you out of the other one. Falling at a portal, you're steered into it (Portal's
  funnel; it can be switched off).
- Going through an emancipation grill closes your portals. Carrying a cube through one
  vaporises it (its dropper makes a new one).
- Cubes, gel, lasers and light bridges all go through portals too. A redirection cube sends a
  laser out of its front: turn it by turning while you hold it.
- Repulsion gel (blue) bounces you off floors and walls; hold crouch not to bounce. Propulsion
  gel (orange) lets you run at up to 17 m/s. Conversion gel (white) takes portals. Gel coats
  cubes too: a blue cube bounces.
- Faith falls at Portal's gravity (15.24 m/s²); a level can set its own. A fall from 5.3 m is a
  hard landing unless you roll, and one from 15 m kills. Both heights are
  [settings](settings.md), and lethal falls can be turned off. Bouncing on repulsion gel
  never counts as a landing, and a fall starts again at a portal's exit.
