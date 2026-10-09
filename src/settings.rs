//! The launch menu: volume (master / music / effects), mouse sensitivity, field
//! of view, Mirror's Edge's fall heights (hard landing, lethal), and the portals'
//! recursion depth and funnel. It opens when the game starts and again on Esc, and saves
//! to `settings.txt` next to where you run the game.
//!
//! Keyboard: ↑/↓ pick a row, ←/→ change it, Enter plays. Or click the − / +
//! buttons and Play.

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
    Video,
}

struct Row {
    /// Its name in the settings file.
    key: &'static str,
    #[allow(dead_code)] // for the menu's pages
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

const ROWS: [Row; 12] = [
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
    Row { key: "fov_kick", page: Page::Video, label: "Speed FOV increase", min: 0.0, max: 1.5, step: 0.25, get: |s| s.fov_kick, put: |s, v| s.fov_kick = v, show: |v| if v <= 0.0 { "Off".to_string() } else { pct(v) } },
    Row { key: "speed_lines", page: Page::Video, label: "Speed lines", min: 0.0, max: 1.0, step: 1.0, get: |s| s.speed_lines, put: |s, v| s.speed_lines = v, show: on_off },
    Row { key: "gun_model", page: Page::Gameplay, label: "Portal gun model", min: 0.0, max: 1.0, step: 1.0, get: |s| s.gun_model, put: |s, v| s.gun_model = v, show: on_off },
];

#[derive(Resource)]
pub struct Menu {
    pub open: bool,
    selected: usize,
    /// Set when Play is chosen; the cursor-lock system grabs the mouse and clears it.
    pub start: bool,
}

#[derive(Component)]
pub struct MenuRoot;
#[derive(Component)]
pub struct ValueText(usize);
#[derive(Component)]
pub struct RowNode(usize);
#[derive(Component, Clone, Copy)]
pub enum MenuButton {
    Nudge(usize, f32),
    Play,
}

const PANEL: Color = Color::srgba(0.04, 0.04, 0.05, 0.86);
const RED: Color = Color::srgb(0.86, 0.10, 0.07);
const ROW_IDLE: Color = Color::srgba(1.0, 1.0, 1.0, 0.04);
const ROW_SELECTED: Color = Color::srgba(0.86, 0.10, 0.07, 0.35);
const BTN: Color = Color::srgba(1.0, 1.0, 1.0, 0.10);
const BTN_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.22);

pub fn setup(mut commands: Commands) {
    let settings = Settings::load();
    // Screenshot capture runs straight through without the menu.
    let open = std::env::var_os("PORTAL_CAPTURE").is_none();
    let font = |px_: f32| TextFont { font_size: px_.into(), ..default() };
    let white = TextColor(Color::srgb(0.95, 0.95, 0.95));
    let dim = TextColor(Color::srgba(0.95, 0.95, 0.95, 0.55));

    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            GlobalZIndex(10),
            if open { Visibility::Visible } else { Visibility::Hidden },
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(28.0)),
                    row_gap: px(8.0),
                    width: px(520.0),
                    ..default()
                },
                BackgroundColor(PANEL),
            ))
            .with_children(|panel| {
                panel.spawn((Text::new("PORTAL RUNNER"), font(34.0), TextColor(RED)));
                panel.spawn((Text::new("Settings"), font(18.0), dim.clone(), Node { margin: UiRect::bottom(px(12.0)), ..default() }));
                for (i, r) in ROWS.iter().enumerate() {
                    panel
                        .spawn((
                            RowNode(i),
                            Node {
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                padding: UiRect::axes(px(12.0), px(6.0)),
                                column_gap: px(10.0),
                                ..default()
                            },
                            BackgroundColor(ROW_IDLE),
                        ))
                        .with_children(|row| {
                            row.spawn((Text::new(r.label), font(19.0), white.clone(), Node { flex_grow: 1.0, ..default() }));
                            row.spawn(button(MenuButton::Nudge(i, -1.0))).with_children(|b| {
                                b.spawn((Text::new("-"), font(22.0), white.clone()));
                            });
                            row.spawn((
                                ValueText(i),
                                Text::new((r.show)(r.get(&settings))),
                                font(19.0),
                                white.clone(),
                                Node { width: px(70.0), justify_content: JustifyContent::Center, ..default() },
                            ));
                            row.spawn(button(MenuButton::Nudge(i, 1.0))).with_children(|b| {
                                b.spawn((Text::new("+"), font(22.0), white.clone()));
                            });
                        });
                }
                panel
                    .spawn((
                        MenuButton::Play,
                        Button,
                        Node {
                            margin: UiRect::top(px(16.0)),
                            padding: UiRect::axes(px(16.0), px(10.0)),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BackgroundColor(RED),
                    ))
                    .with_children(|b| {
                        b.spawn((Text::new("PLAY"), font(22.0), white.clone()));
                    });
                panel.spawn((
                    Text::new("Up/Down pick  -  Left/Right change  -  Enter play  -  Esc opens this again"),
                    font(14.0),
                    dim.clone(),
                    Node { margin: UiRect::top(px(10.0)), ..default() },
                ));
            });
        });

    commands.insert_resource(Menu { open, selected: 0, start: false });
    commands.insert_resource(settings);
}

fn button(kind: MenuButton) -> impl Bundle {
    (
        kind,
        Button,
        Node { width: px(34.0), height: px(30.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
        BackgroundColor(BTN),
    )
}

#[allow(clippy::type_complexity)]
pub fn update(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<Menu>,
    mut settings: ResMut<Settings>,
    mut buttons: Query<(&Interaction, &MenuButton, &mut BackgroundColor), (Changed<Interaction>, Without<RowNode>)>,
    mut rows: Query<(&RowNode, &mut BackgroundColor), Without<MenuButton>>,
    mut values: Query<(&ValueText, &mut Text)>,
    mut root: Query<&mut Visibility, With<MenuRoot>>,
) {
    if !menu.open {
        if keys.just_pressed(KeyCode::Escape) {
            menu.open = true;
        }
    } else {
        let before = settings.clone();
        let n = ROWS.len();
        if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) {
            menu.selected = (menu.selected + 1) % n;
        }
        if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) {
            menu.selected = (menu.selected + n - 1) % n;
        }
        for (key, dir) in [(KeyCode::ArrowLeft, -1.0), (KeyCode::KeyA, -1.0), (KeyCode::ArrowRight, 1.0), (KeyCode::KeyD, 1.0)] {
            if keys.just_pressed(key) {
                ROWS[menu.selected].nudge(&mut settings, dir);
            }
        }
        let mut play = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space);
        for (interaction, kind, mut bg) in &mut buttons {
            let play_button = matches!(kind, MenuButton::Play);
            match interaction {
                Interaction::Pressed => match *kind {
                    MenuButton::Nudge(i, dir) => {
                        menu.selected = i;
                        ROWS[i].nudge(&mut settings, dir);
                    }
                    MenuButton::Play => play = true,
                },
                Interaction::Hovered => *bg = BackgroundColor(if play_button { RED.lighter(0.08) } else { BTN_HOVER }),
                Interaction::None => *bg = BackgroundColor(if play_button { RED } else { BTN }),
            }
        }
        if *settings != before {
            settings.save();
        }
        if play {
            menu.open = false;
            menu.start = true;
            settings.save();
        }
    }

    for (row, mut bg) in &mut rows {
        *bg = BackgroundColor(if row.0 == menu.selected { ROW_SELECTED } else { ROW_IDLE });
    }
    if settings.is_changed() {
        for (v, mut text) in &mut values {
            let r = &ROWS[v.0];
            **text = (r.show)(r.get(&settings));
        }
    }
    for mut vis in &mut root {
        let want = if menu.open { Visibility::Visible } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_clamps() {
        let s = Settings { master: 0.35, music: 0.0, effects: 0.9, sensitivity: 1.25, fov: 100.0, hard_land: 6.5, lethal_fall: 14.0, portal_depth: 2.0, funnel: 0.0, gun_model: 1.0, fov_kick: 0.5, speed_lines: 0.0 };
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
