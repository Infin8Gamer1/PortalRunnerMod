# Portal Runner

Mirror's Edge's parkour with Portal's portal gun and test chambers, in Rust and Bevy.

Faith runs, vaults, wallruns, climbs and rolls exactly as she does in Mirror's Edge (the
movement is [Faith Runner](https://github.com/tnrjns/faith-runner)'s, a port of the game's own
code). She also carries the Aperture Science Handheld Portal Device: portals, momentum through
them, cubes, buttons, grills, faith plates, light bridges, lasers and Portal 2's gels. Play the
game's own chambers or Portal's, or make your own in the level editor.

A personal project, not for distribution. None of Portal's, Portal 2's or Mirror's Edge's
assets are in this repository: the game reads them from your own installs. What is here is
the layout of Portal's maps as the game's own level files (`levels/portal/`), naming the assets
they use.

## Quickstart

You need [Rust](https://rustup.rs), and your own copies of Portal, Portal 2 and Mirror's Edge
on Steam for the real look and sound (anything missing has a stand-in).

```
cargo run --release -p portal_extract            # once: copy Portal's assets into assets/extracted/
cargo run --release --no-default-features       # play (drop the flag once the DLSS SDK is set up)
```

Then **Play** on the title screen. WASD and the mouse move; left and right click shoot blue
and orange portals; Space jumps; E picks things up; F4 opens the level editor.

## Docs

- [Getting started](docs/getting-started.md): setup, the extractor's options, DLSS, ray-traced
  lighting
- [Controls](docs/controls.md)
- [Settings](docs/settings.md)
- [Chambers](docs/chambers.md): what there is to play, and how the portal rules work
- [Making levels](docs/levels.md): the level editor, and chambers in Rust
- [Checking changes](docs/checking-changes.md): tests and scripted screenshots
- [Code layout](docs/code-layout.md)
- [Faith Runner](docs/faith-runner.md): every move Faith makes, in detail

## Credits

The movement, Faith's body and her sounds are Faith Runner's
(https://github.com/tnrjns/faith-runner). Portal, Portal 2 and Mirror's Edge belong to Valve
and EA; this project only reads your own copies.
