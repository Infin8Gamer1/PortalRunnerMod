# Code layout

```
crates/faith_move/      the movement (Faith Runner): engine-independent, glam only
crates/faith_anim/      Mirror's Edge's animation driver for Faith's body (Faith Runner)
crates/me_assets/       Mirror's Edge package readers (Faith Runner)
crates/aperture/        portals and the puzzle pieces, engine-independent:
  portal.rs               portals and the transform through them
  collide.rs              collision through portals (hole and sleeve per body)
  sim.rs                  the running chamber: gun, Faith through portals, cubes, buttons,
                          doors, grills, plates, bridges, lasers, gel, volumes, physics props
  gel.rs                  the paint grid and gel blobs
  level.rs                the level description and its builder
  levels/                 the game's own chambers (kit.rs: shared pieces)
  doc.rs                  a level as a file (the editor's), Portal's brushes and entities
                          in it too
  brush.rs                brushes: faces to draw and boxes to collide with
  source.rs               Source's entities as a level's pieces and logic
  io.rs                   their logic: Source's entity I/O
  port.rs                 Portal's maps made into level files (levels/portal/)
  art.rs                  what's drawn of a Portal chamber: faces, props, overlays, cables
  tests.rs, solves.rs     the mechanics, and each chamber played through by script
crates/portal_extract/  copies sounds, textures (VTF -> PNG) and models (MDL -> GLB, with
                        their animations) out of your Portal and Portal 2 VPKs, and Portal
                        with RTX's remastered materials and models (remix.rs, usd.rs);
                        Portal's maps (bsp.rs: brushes, entities; maps.rs), GLaDOS's scenes
                        (scenes.rs), its sounds as a library with their soundscripts and
                        soundscapes (sounds.rs), its colour correction tables, and the maps
                        as level files (--port-maps)
src/main.rs             the Bevy app: input, the frame's order, level loading, lighting
src/portal_render.rs    recursive portal views (oblique near plane), portal surfaces
src/chamber.rs          drawing a chamber and its pieces
src/source_map.rs       Portal's chambers: drawing their faces and props (moving with their
                        doors and elevators), their lights
src/editor/             the level editor (catalog.rs: what the content browser offers)
src/gun.rs              shot tracers, sparks, the hold beam (and the optional gun model)
src/hud.rs              crosshair, hints, messages, timer, debug readout
src/menu.rs             title, pause, chamber select, settings pages, controls
src/speed_fx.rs         speed FOV kick, speed lines, skid dust
src/graphics.rs         applying the display and graphics settings, the frame-rate cap
src/raytrace.rs         ray-traced lighting (Solari): the traced scene, emissive lights, sky
src/fonts.rs            the UI font (Inter from the system, if it's there)
src/progress.rs         best times and the last chamber, saved
src/portal_audio.rs     Portal's sounds
src/map_audio.rs        Portal's chambers' own sounds and soundscapes; the sound library
src/map_fx.rs           Portal's chambers' particles, steam, sparks, beams, cameras, shakes,
                        fades
src/map_grade.rs        Portal's chambers' colour correction
src/audio.rs            Mirror's Edge's sounds (Faith Runner)
src/me_viewmodel.rs     Faith's Mirror's Edge body (Faith Runner)
src/body3p.rs           her third-person body, seen through portals (and her shadow)
src/props.rs            Portal 2's (and Portal with RTX's) models for the puzzle pieces
src/me_props.rs         Mirror's Edge's props and ladders, from the install
src/settings.rs         the settings and their pages
src/capture.rs          scripted screenshot runs (see checking-changes.md)
levels/                 level files: the editor's, and Portal's chambers (levels/portal/)
docs/                   these docs; faith-runner.md is Faith Runner's own README
vendor/bevy_solari/     Bevy's ray tracer with one crash fix (see vendor/README.md)
```
