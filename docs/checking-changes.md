# Checking changes

## Tests

```
cargo test --workspace             # everything
cargo test -p aperture -p faith_move   # the mechanics, the chambers and the movement
```

- `faith_move`'s tests check Faith's movement against Mirror's Edge's numbers.
- `aperture`'s tests check the portals and puzzle pieces. Its solution tests (`solves.rs`, and
  one per built chamber) play each chamber from its start to its exit as Faith, so a change
  that makes one impossible is caught. Portal's chambers are played from their level files in
  `levels/portal/`, so these run without anything extracted.
- `aperture`'s `copies_are_the_committed_ones` checks that the converter still makes the
  committed `levels/portal/` files from the extracted maps (unless you've edited one). If it
  fails after a change to the converter, run `cargo run --release -p portal_extract --
  --port-maps` and commit the files it writes.

The repository isn't rustfmt-formatted: don't run `cargo fmt` over it.

## Screenshots

```
PORTAL_CAPTURE=shots PORTAL_SCRIPT=portals cargo run --release
```

The capture mode plays a scripted run and saves screenshots into the folder (`shots` here).
Each screenshot's line in the log says how long frames really take; the game's own clock is
fixed at 60 frames a second while capturing.

The scripts are in `src/capture.rs`. Some of them:

| Script | What it shoots |
|---|---|
| `tour` (the default) | Each chamber's start |
| `portals`, `recursion`, `fling`, `stations`, `gun` | The portals, views through them, a fling, the Proving Grounds' stations, the gun |
| `through`, `flingfilm` | A screenshot every frame running through a portal, and falling through a floor portal |
| `cubethrough`, `plate`, `goo`, `glass`, `pellets` | A cube held half through a portal, the faith plate, goo, observation windows, Chamber 06's pellet |
| `lighting`, `lighting3`, `rtswitch` | Lighting still, moving and outdoors; before and after ray tracing, and a portal's view against the world it shows; switching ray tracing |
| `p1` | Portal's chambers' starts (`PORTAL_FIRST_MAP` / `PORTAL_LAST_MAP` pick them; `PORTAL_TP="x,y,z,yaw,pitch;..."` adds places to look from) |
| `views` | Map `PORTAL_FIRST_MAP` from each place in `PORTAL_TP="x,y,z,yaw,pitch;..."` (where she'd stand, and which way to look): a spectator camera there, so nothing falls or drowns. `PORTAL_HOLD` frames before each shot, `PORTAL_SHOTS` shots of each; `PORTAL_TRACK=pellet` keeps the view on the pellet; `PORTAL_PLACE=1` puts her there instead |
| `spectate` | The spectator camera over Chamber 08's goo, following the pellet, then back to her. A spectator sees her as others do: her third-person body |
| `faith` | Faith on Training, in the sun and the shade: her first-person arms and legs, standing and through a stride, and her face from a spectator in front of her (`PORTAL_FAITH_SUN` / `PORTAL_FAITH_SHADE` = `x,y,z,yaw` move the spots) |
| `normaltest` | With `PORTAL_NORMAL_TEST=1`: a bump on Bevy's own tangents and one on the game's, lit from above: both lit on top means normal maps are read the right way up |
| `native`, `effects`, `rockets`, `rtx` | A few of Portal's chambers, their effects, escape_01's rocket turret, Portal with RTX's pieces |
| `chambersmenu`, `menu` | The menus |
| `editor`, `editormouse`, `editoreffects`, `nativeedit` | The level editor (start these with `PORTAL_EDIT=1`) |

Other variables:
- `PORTAL_MAP=<n>`: start on chamber n (its place in the list).
- `PORTAL_EDIT=1`: start in the level editor.
- `PORTAL_NOVSYNC=1`: uncap the frame rate.
- `PORTAL_NORT=1`: no ray-traced lighting for this run.
