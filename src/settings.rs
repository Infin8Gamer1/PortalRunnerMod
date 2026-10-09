//! The settings: what they are, their ranges and pages in the menu (`menu.rs` draws
//! them), and saving them to `settings.txt` next to where you run the game.

use bevy::prelude::*;

const FILE: &str = "settings.txt";

#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Settings {
    /// 0..=1 each.
    pub master: f32,
    pub music: f32,
    pub effects: f32,
    /// Multiplier on the base mouse/pad sensitivity.
    pub sensitivity: f32,
    /// Horizontal field of view in degrees (Mirror's Edge uses 90).
    pub fov: f32,
    /// Falls this high (m) are a hard landing unless you roll (Mirror's Edge: 5.3).
    pub hard_land: f32,
    /// Falls this high (m) kill (Mirror's Edge: 10). At the top of the range: off.
    pub lethal_fall: f32,
    /// How many portals deep you can see through portals (1-4).
    pub portal_depth: f32,
    /// Steer into portals you fall at (Portal's funnel): 1 on, 0 off.
    pub funnel: f32,
    /// Draw Portal's gun in Faith's view: 1 on, 0 off (she isn't holding it, so it floats
    /// over Mirror's Edge's arm animations; off by default).
    pub gun_model: f32,
    /// How much the view widens when going fast (1 normal, 0 off).
    pub fov_kick: f32,
    /// Streaks round the screen's edges when going fast: 1 on, 0 off.
    pub speed_lines: f32,
    /// Off propulsion gel, seconds before landing-speed skids start (keeping the gel's speed
    /// until then).
    pub gel_skid_delay: f32,
    /// Show the chamber timer and best time (a time trial): 1 on, 0 off.
    pub show_timer: f32,
    // ---- display and graphics (graphics.rs applies them)
    /// 0 windowed, 1 borderless fullscreen, 2 fullscreen.
    pub window_mode: f32,
    /// Index into `graphics::RESOLUTIONS` (windowed).
    pub resolution: f32,
    pub vsync: f32,
    /// Index into `graphics::FPS_CAPS`.
    pub fps_cap: f32,
    /// Exposure offset in stops (+ brighter).
    pub brightness: f32,
    /// `graphics::AA_MODES`.
    pub anti_aliasing: f32,
    /// Contrast-adaptive sharpening strength (0 off).
    pub sharpening: f32,
    /// The portal views' resolution, as a fraction of the window's.
    pub portal_resolution: f32,
    /// `graphics::SHADOWS`.
    pub shadows: f32,
}

/// The volume sliders are perceptual: each step is the same number of decibels, and the
/// bottom of a slider is this far below the top (then silence at 0).
pub const VOLUME_RANGE_DB: f32 = 30.0;
/// The loudness of everything at full master volume. The games' sounds are mastered hot,
/// so full volume sits well below unity, leaving headroom for many sounds at once.
pub const OUTPUT_TRIM: f32 = 0.4;
/// Bumped when the meaning of the volume sliders changes, so old saved values are reset.
const AUDIO_VERSION: f32 = 2.0;

/// The gain for a volume slider at `s` (0..=1).
pub fn slider_gain(s: f32) -> f32 {
    if s <= 0.001 {
        0.0
    } else {
        10f32.powf((s.min(1.0) - 1.0) * VOLUME_RANGE_DB / 20.0)
    }
}

/// A lethal fall height at this or above means no fall kills.
pub const LETHAL_OFF: f32 = 40.0;

impl Settings {
    /// The lethal fall height for the movement (huge when it's off).
    pub fn lethal_height(&self) -> f32 {
        if self.lethal_fall >= LETHAL_OFF { 1.0e6 } else { self.lethal_fall }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            master: 0.8,
            music: 0.8,
            effects: 1.0,
            sensitivity: 1.0,
            fov: 90.0,
            hard_land: 5.3,
            lethal_fall: 10.0,
            portal_depth: 3.0,
            funnel: 1.0,
            gun_model: 0.0,
            fov_kick: 1.0,
            speed_lines: 1.0,
            gel_skid_delay: 1.5,
            show_timer: 0.0,
            window_mode: 0.0,
            resolution: 1.0,
            vsync: 1.0,
            fps_cap: 0.0,
            brightness: 0.0,
            anti_aliasing: 3.0,
            sharpening: 0.0,
            portal_resolution: 1.0,
            shadows: 2.0,
        }
    }
}

impl Settings {
    pub fn music_gain(&self) -> f32 {
        OUTPUT_TRIM * slider_gain(self.master) * slider_gain(self.music)
    }

    pub fn effects_gain(&self) -> f32 {
        OUTPUT_TRIM * slider_gain(self.master) * slider_gain(self.effects)
    }

    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let mut audio_version = 1.0;
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let Ok(v) = v.trim().parse::<f32>() else { continue };
            let k = k.trim();
            if k == "audio" {
                audio_version = v;
            } else if let Some(r) = ROWS.iter().find(|r| r.key == k) {
                r.set(&mut s, v);
            }
        }
        // Volumes saved for the old linear sliders mean something else now.
        if audio_version < AUDIO_VERSION {
            let d = Settings::default();
            (s.master, s.music, s.effects) = (d.master, d.music, d.effects);
        }
        for r in &ROWS {
            let v = r.get(&s).clamp(r.min, r.max);
            r.set(&mut s, v);
        }
        s
    }

    pub fn to_text(&self) -> String {
        let mut t = format!("# Portal Runner settings\naudio = {AUDIO_VERSION}\n");
        for r in &ROWS {
            t += &format!("{} = {}\n", r.key, r.get(self));
        }
        t
    }

    pub fn load() -> Settings {
        std::fs::read_to_string(FILE).map(|t| Settings::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Err(e) = std::fs::write(FILE, self.to_text()) {
            warn!("couldn't save {FILE}: {e}");
        }
    }
}

/// The menu's pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Audio,
    Controls,
    Gameplay,
    Display,
    Graphics,
}

struct Row {
    /// Its name in the settings file.
    key: &'static str,
    page: Page,
    label: &'static str,
    min: f32,
    max: f32,
    step: f32,
    get: fn(&Settings) -> f32,
    put: fn(&mut Settings, f32),
    show: fn(f32) -> String,
}

impl Row {
    fn get(&self, s: &Settings) -> f32 {
        (self.get)(s)
    }
    fn set(&self, s: &mut Settings, v: f32) {
        (self.put)(s, v)
    }
    fn nudge(&self, s: &mut Settings, dir: f32) {
        let v = self.get(s) + self.step * dir;
        // Snap to the step so repeated presses land on round numbers.
        let v = ((v / self.step).round() * self.step).clamp(self.min, self.max);
        self.set(s, v);
    }
}

fn pct(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

fn on_off(v: f32) -> String {
    if v > 0.5 { "On".to_string() } else { "Off".to_string() }
}

const ROWS: [Row; 23] = [
    Row { key: "master", page: Page::Audio, label: "Master volume", min: 0.0, max: 1.0, step: 0.05, get: |s| s.master, put: |s, v| s.master = v, show: pct },
    Row { key: "music", page: Page::Audio, label: "Music", min: 0.0, max: 1.0, step: 0.05, get: |s| s.music, put: |s, v| s.music = v, show: pct },
    Row { key: "effects", page: Page::Audio, label: "Effects", min: 0.0, max: 1.0, step: 0.05, get: |s| s.effects, put: |s, v| s.effects = v, show: pct },
    Row {
        key: "sensitivity",
        page: Page::Controls,
        label: "Mouse sensitivity",
        min: 0.1,
        max: 3.0,
        step: 0.05,
        get: |s| s.sensitivity,
        put: |s, v| s.sensitivity = v,
        show: |v| format!("{v:.2}x"),
    },
    Row { key: "fov", page: Page::Controls, label: "Field of view", min: 70.0, max: 110.0, step: 1.0, get: |s| s.fov, put: |s, v| s.fov = v, show: |v| format!("{v:.0} deg") },
    Row {
        key: "hard_land",
        page: Page::Gameplay,
        label: "Hard landing from",
        min: 2.0,
        max: 30.0,
        step: 0.5,
        get: |s| s.hard_land,
        put: |s, v| s.hard_land = v,
        show: |v| format!("{v:.1} m"),
    },
    Row {
        key: "lethal_fall",
        page: Page::Gameplay,
        label: "Lethal fall from",
        min: 5.0,
        max: LETHAL_OFF,
        step: 1.0,
        get: |s| s.lethal_fall,
        put: |s, v| s.lethal_fall = v,
        show: |v| if v >= LETHAL_OFF { "Off".to_string() } else { format!("{v:.0} m") },
    },
    Row {
        key: "portal_depth",
        page: Page::Gameplay,
        label: "Portal recursion",
        min: 1.0,
        max: 4.0,
        step: 1.0,
        get: |s| s.portal_depth,
        put: |s, v| s.portal_depth = v,
        show: |v| format!("{v:.0}"),
    },
    Row { key: "funnel", page: Page::Gameplay, label: "Portal funnel", min: 0.0, max: 1.0, step: 1.0, get: |s| s.funnel, put: |s, v| s.funnel = v, show: on_off },
    Row {
        key: "window_mode",
        page: Page::Display,
        label: "Window",
        min: 0.0,
        max: 2.0,
        step: 1.0,
        get: |s| s.window_mode,
        put: |s, v| s.window_mode = v,
        show: |v| crate::graphics::WINDOW_MODES[v as usize].to_string(),
    },
    Row {
        key: "resolution",
        page: Page::Display,
        label: "Window size",
        min: 0.0,
        max: (crate::graphics::RESOLUTIONS.len() - 1) as f32,
        step: 1.0,
        get: |s| s.resolution,
        put: |s, v| s.resolution = v,
        show: |v| {
            let (x, y) = crate::graphics::RESOLUTIONS[v as usize];
            format!("{x} x {y}")
        },
    },
    Row { key: "vsync", page: Page::Display, label: "VSync", min: 0.0, max: 1.0, step: 1.0, get: |s| s.vsync, put: |s, v| s.vsync = v, show: on_off },
    Row {
        key: "fps_cap",
        page: Page::Display,
        label: "Frame rate cap",
        min: 0.0,
        max: (crate::graphics::FPS_CAPS.len() - 1) as f32,
        step: 1.0,
        get: |s| s.fps_cap,
        put: |s, v| s.fps_cap = v,
        show: |v| match crate::graphics::FPS_CAPS[v as usize] {
            0 => "Off".to_string(),
            n => format!("{n} fps"),
        },
    },
    Row {
        key: "brightness",
        page: Page::Display,
        label: "Brightness",
        min: -2.0,
        max: 2.0,
        step: 0.25,
        get: |s| s.brightness,
        put: |s, v| s.brightness = v,
        show: |v| format!("{v:+.2}"),
    },
    Row {
        key: "anti_aliasing",
        page: Page::Graphics,
        label: "Anti-aliasing",
        min: 0.0,
        max: (crate::graphics::AA_MODES.len() - 1) as f32,
        step: 1.0,
        get: |s| s.anti_aliasing,
        put: |s, v| s.anti_aliasing = v,
        show: |v| crate::graphics::AA_MODES[v as usize].to_string(),
    },
    Row {
        key: "sharpening",
        page: Page::Graphics,
        label: "Sharpening",
        min: 0.0,
        max: 1.0,
        step: 0.1,
        get: |s| s.sharpening,
        put: |s, v| s.sharpening = v,
        show: |v| if v <= 0.0 { "Off".to_string() } else { pct(v) },
    },
    Row {
        key: "portal_resolution",
        page: Page::Graphics,
        label: "Portal view resolution",
        min: 0.5,
        max: 1.0,
        step: 0.25,
        get: |s| s.portal_resolution,
        put: |s, v| s.portal_resolution = v,
        show: pct,
    },
    Row {
        key: "shadows",
        page: Page::Graphics,
        label: "Shadows",
        min: 0.0,
        max: (crate::graphics::SHADOWS.len() - 1) as f32,
        step: 1.0,
        get: |s| s.shadows,
        put: |s, v| s.shadows = v,
        show: |v| crate::graphics::SHADOWS[v as usize].to_string(),
    },
    Row { key: "fov_kick", page: Page::Display, label: "Speed FOV increase", min: 0.0, max: 1.5, step: 0.25, get: |s| s.fov_kick, put: |s, v| s.fov_kick = v, show: |v| if v <= 0.0 { "Off".to_string() } else { pct(v) } },
    Row { key: "speed_lines", page: Page::Display, label: "Speed lines", min: 0.0, max: 1.0, step: 1.0, get: |s| s.speed_lines, put: |s, v| s.speed_lines = v, show: on_off },
    Row {
        key: "gel_skid_delay",
        page: Page::Gameplay,
        label: "Skid delay off speed gel",
        min: 0.0,
        max: 5.0,
        step: 0.25,
        get: |s| s.gel_skid_delay,
        put: |s, v| s.gel_skid_delay = v,
        show: |v| format!("{v:.2} s"),
    },
    Row { key: "show_timer", page: Page::Gameplay, label: "Chamber timer", min: 0.0, max: 1.0, step: 1.0, get: |s| s.show_timer, put: |s, v| s.show_timer = v, show: on_off },
    Row { key: "gun_model", page: Page::Display, label: "Portal gun model", min: 0.0, max: 1.0, step: 1.0, get: |s| s.gun_model, put: |s, v| s.gun_model = v, show: on_off },
];

pub fn setup(mut commands: Commands) {
    commands.insert_resource(Settings::load());
}

/// The rows on a page of the settings menu (indices for the functions below).
pub fn rows_on(page: Page) -> Vec<usize> {
    (0..ROWS.len()).filter(|&i| ROWS[i].page == page).collect()
}

pub fn row_label(i: usize) -> &'static str {
    ROWS[i].label
}

pub fn row_value(i: usize, s: &Settings) -> String {
    (ROWS[i].show)(ROWS[i].get(s))
}

/// Step a row up (`dir` 1) or down (-1).
pub fn nudge(i: usize, s: &mut Settings, dir: f32) {
    ROWS[i].nudge(s, dir);
}

impl Page {
    pub const ALL: [Page; 5] = [Page::Audio, Page::Controls, Page::Gameplay, Page::Display, Page::Graphics];

    pub fn name(self) -> &'static str {
        match self {
            Page::Audio => "Audio",
            Page::Controls => "Controls",
            Page::Gameplay => "Gameplay",
            Page::Display => "Display",
            Page::Graphics => "Graphics",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_clamps() {
        let s = Settings { master: 0.35, music: 0.0, effects: 0.9, sensitivity: 1.25, fov: 100.0, hard_land: 6.5, lethal_fall: 14.0, portal_depth: 2.0, funnel: 0.0, gun_model: 1.0, fov_kick: 0.5, speed_lines: 0.0, gel_skid_delay: 2.25,
            show_timer: 1.0,
            window_mode: 1.0,
            resolution: 3.0,
            vsync: 0.0,
            fps_cap: 2.0,
            brightness: 0.5,
            anti_aliasing: 4.0,
            sharpening: 0.3,
            portal_resolution: 0.75,
            shadows: 3.0,
        };
        assert_eq!(Settings::parse(&s.to_text()), s);
        let wild = Settings::parse("audio = 2\nmaster = 7\nfov = 10\nnonsense\nmusic = x");
        assert_eq!(wild.master, 1.0);
        assert_eq!(wild.fov, 70.0);
        assert_eq!(wild.music, Settings::default().music);
    }

    #[test]
    fn every_row_has_its_own_key() {
        for (i, a) in ROWS.iter().enumerate() {
            assert!(ROWS[i + 1..].iter().all(|b| b.key != a.key), "{} twice", a.key);
            let d = Settings::default();
            assert!((a.min..=a.max).contains(&a.get(&d)), "{}'s default is out of range", a.key);
        }
    }

    #[test]
    fn old_linear_volumes_are_reset() {
        let old = Settings::parse("master = 0.2\nmusic = 0.3\nfov = 100");
        assert_eq!(old.master, Settings::default().master);
        assert_eq!(old.music, Settings::default().music);
        assert_eq!(old.fov, 100.0);
    }

    #[test]
    fn volume_is_perceptual() {
        assert_eq!(slider_gain(0.0), 0.0);
        assert!((slider_gain(1.0) - 1.0).abs() < 1e-6);
        // Each tenth of the slider is the same step in decibels.
        let r1 = slider_gain(0.9) / slider_gain(0.8);
        let r2 = slider_gain(0.4) / slider_gain(0.3);
        assert!((r1 - r2).abs() < 1e-4);
        // The defaults land about where the old linear sliders were comfortable (~20%).
        let fx = Settings::default().effects_gain();
        assert!((0.15..0.25).contains(&fx), "{fx}");
    }

    #[test]
    fn nudges_snap_to_steps() {
        let mut s = Settings { master: 0.33, ..default() };
        ROWS[0].nudge(&mut s, 1.0);
        assert!((s.master - 0.40).abs() < 1e-5, "{}", s.master);
        for _ in 0..30 {
            ROWS[0].nudge(&mut s, -1.0);
        }
        assert_eq!(s.master, 0.0);
    }
}
