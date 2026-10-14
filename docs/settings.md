# Settings

**Settings** in the title or pause menu. Changes apply at once (except where noted) and are
saved to `settings.txt` in the folder you run the game from.

## Audio

Master, music and effects volume.

## Controls

Mouse sensitivity and field of view.

## Gameplay

- **Hard landing from** and **Lethal fall from** (metres): a fall from this high is a hard
  landing (unless you roll), or kills. The defaults are 5.3 m (Mirror's Edge's) and 15 m.
  Lethal falls can be turned off.
- **Portal recursion**: how many portals deep you can see through portals, 1-4.
- **Portal funnel**: steers you into a portal you're falling at, as Portal does.
- **Skid delay off speed gel**: how long after leaving propulsion gel before you skid off
  the extra speed.
- **Chamber timer**: shows the time in the current chamber.

## Display

- Window: windowed, borderless or fullscreen, and its size.
- VSync, and a frame-rate cap. (`PORTAL_NOVSYNC=1` forces VSync off for one run.)
- Brightness.
- **Speed FOV increase** and **Speed lines**: the view widens, and lines streak, when you're
  going fast.
- **Faith's body in portals**: her Mirror's Edge body, seen through portals.
- **Portal gun model**: draws Portal's gun in Faith's view. Off by default (she isn't holding
  it, so it floats over Mirror's Edge's arm animations).

## Graphics

- **DLSS**: see [Getting started](getting-started.md#dlss). Off (or without DLSS), TAA smooths
  the picture: it's also what smooths the traced light's grain over frames.
- Sharpening, and the portal views' resolution.
- **Auto exposure**, **Bloom**.
- **Texture detail**: Mirror's Edge's textures, at most 256, 512 or 1024 square.

The lighting is always ray traced (see [Getting started](getting-started.md#lighting)): it
makes its own shadows and occlusion, so there are no shadow, ambient occlusion or other
anti-aliasing settings.
