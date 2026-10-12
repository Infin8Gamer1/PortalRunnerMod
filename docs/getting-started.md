# Getting started

## What you need

- **Rust** (https://rustup.rs).
- For the real look and sound, your own copies of **Portal**, **Portal 2** and **Mirror's
  Edge** (Steam). Each is optional: anything missing has a stand-in. **Portal with RTX** is
  used too if it's installed.
- For **DLSS** (built in by default), NVIDIA's DLSS SDK: see [DLSS](#dlss) below, or build
  without it.

None of the games' assets are in this repository. The game reads Mirror's Edge's files from
your install as it starts, and copies what it uses from Portal and Portal 2 out of your
installs with the extractor.

## 1. Extract Portal's assets

```
cargo run --release -p portal_extract
```

This writes Portal's and Portal 2's textures, models, sounds and maps' assets into
`assets/extracted/` (it's ignored by Git). Run it once, and again after pulling changes that
add models.

It finds the games in the usual Steam folders on Linux and Windows. Otherwise, tell it where
they are:

| Option | What it does |
|---|---|
| `--portal <dir>`, `--portal2 <dir>`, `--portal-rtx <dir>` | Where each game is |
| `--out <dir>` | Write somewhere other than `assets/extracted/` |
| `--no-props` | Skip the props library: every prop model of both games (about 1,700, 2 GB), for the level editor's props browser |
| `--only-props` | Make only the props library |
| `--check-props` | List the props library's materials that have no texture, and why |
| `--no-sounds` / `--sounds` | Skip Portal's sound library, or write it again |
| `--port-maps` | Make Portal's chambers (`levels/portal/`) again from the extracted maps, keeping any you've edited |
| `--inspect <model.mdl>...` | Describe models (parts, bones, size, animations) instead of extracting |

The puzzle pieces (buttons, cube sockets, cubes, droppers, faith plates, doors, grill and
laser emitters, catchers, relays, light bridge emitters) use Portal 2's models once they're
extracted. If Portal with RTX is installed, its remaster is used where it has one: its
physically based materials for walls and floors, its toxic water for goo, and Portal's own
cube, buttons, dropper, round door and grill emitters in place of Portal 2's. (Portal 2's
own mechanics, such as lasers, light bridges, faith plates, gel and the reflection cube, have
no Portal version and stay Portal 2's.)

**Mirror's Edge** is found in the usual Steam folders too, including under Proton. Otherwise,
set `FAITH_ME_DIR` to where it's installed, or put that path in `me_path.txt`.

## 2. Run the game

```
cargo run --release
```

Without the DLSS SDK, add `--no-default-features`: the game is the same, without DLSS.

It opens on the title screen:
- **Continue** (or **Play**) picks up at the last chamber you were in.
- **Chambers** lists every chamber with your best times, in two folders: Portal's chambers,
  and the testing chambers (the game's own, and the levels made in the editor).
- **Settings** and **Controls** are there too.

In the game, **Esc** (or Start) pauses. Progress is saved to `progress.txt`, and settings to
`settings.txt`, in the folder you run the game from.

See [Controls](controls.md) for the keys, [Chambers](chambers.md) for what there is to play,
and [Making levels](levels.md) for the level editor (**F4**).

## Lighting

On GPUs with hardware ray tracing (NVIDIA RTX, AMD RX 6000 and later, on Vulkan), lighting is
ray traced with Bevy's Solari. The sun, the ceiling panels and everything that glows light the
chambers directly and by bouncing, every frame, through portal views too. Nothing is baked.

Other GPUs light the chambers the ordinary way. So does turning off **Settings > Graphics >
Ray-traced lighting** (which switches straight away), or `PORTAL_NORT=1` for one run.

## DLSS

DLSS (NVIDIA RTX, Vulkan) is built in by default. Building it needs NVIDIA's DLSS SDK, which
you download yourself under NVIDIA's licence. It must be version 310.5.3 (what Bevy 0.19's
DLSS is built against), and its libraries are in Git LFS:

```
git clone --branch v310.5.3 --depth 1 https://github.com/NVIDIA/DLSS.git
cd DLSS && git lfs pull
```

Then:
- Set `DLSS_SDK` to that folder. It's also where DLSS finds its libraries when the game
  starts; if it isn't set then, the game uses the path it was built with.
- Set `VULKAN_SDK` to where the Vulkan headers are: `/usr` on Linux with the distribution's
  `vulkan-headers`, or the Vulkan SDK on Windows.
- Have clang installed.

For example, in your shell profile:

```
export DLSS_SDK=$HOME/Documents/Git/DLSS VULKAN_SDK=/usr
```

On a GPU that has it, DLSS is on from the start (Quality). **Settings > Graphics > DLSS**
offers DLAA, Quality, Balanced, Performance and Ultra performance, or off; it takes the place
of the anti-aliasing setting, and the portal views render at its lower resolution too. With
ray-traced lighting it's **DLSS Ray Reconstruction**, which denoises the traced light as it
upscales (no grain, even moving fast). The log says which DLSS features the GPU has, and the
setting says when one is missing.

Frame times on an RTX 4080 at 1440p, ray traced, standing still / with a pair of portals in
view:

| DLSS | Frame time (ms) |
|---|---|
| Off (TAA) | 5.4 / 13.3 |
| DLAA | 11.7 / 19.1 |
| Quality | 6.2 / 10.7 |
| Balanced | 5.2 / 8.9 |
| Performance | 4.1 / 7.5 |
