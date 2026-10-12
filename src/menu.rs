//! The menus: the title screen (with the chamber behind it, the camera drifting), the pause
//! menu (Esc, or Start on a pad), chamber select with best times, the settings in pages, and
//! the controls. Keyboard: arrows or WASD pick and change, Enter picks, Esc goes back. Pad:
//! the d-pad, A picks, B goes back. Or the mouse.

use bevy::app::AppExit;
use bevy::prelude::*;

use crate::fonts::UiFonts;
use crate::progress::Progress;
use crate::settings::{self, Page, Settings};
use crate::Game;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Title,
    Pause,
    /// The chambers' folders.
    Chambers,
    /// One folder's chambers.
    Folder(crate::LevelGroup),
    Settings(Page),
    Controls,
}

#[derive(Resource)]
pub struct Menu {
    pub open: bool,
    pub screen: Screen,
    /// At the title screen: nothing's been played yet (or Main Menu was picked).
    pub title: bool,
    selected: usize,
    /// Set when play starts or resumes; the cursor-lock system grabs the mouse and clears it.
    pub start: bool,
    /// The menu closed this frame: the key or button that closed it isn't also a jump.
    pub just_closed: bool,
    /// The screens to go back to.
    back: Vec<Screen>,
    dirty: bool,
    /// Open the level editor: on a new level (`true`) or the one being played.
    pub edit: Option<bool>,
}

impl Menu {
    /// Open at `screen`, from the game (Esc, F1).
    pub fn open_at(&mut self, screen: Screen) {
        self.open = true;
        self.title = screen == Screen::Title;
        self.go(screen, false);
    }

    fn go(&mut self, screen: Screen, remember: bool) {
        if remember && self.open {
            self.back.push(self.screen);
        } else if !remember {
            self.back.clear();
        }
        self.screen = screen;
        self.selected = 0;
        self.dirty = true;
    }

    /// Close, for the level editor (the mouse stays free).
    fn close_to_editor(&mut self, new: bool) {
        self.close();
        self.start = false;
        self.edit = Some(new);
    }

    fn close(&mut self) {
        self.open = false;
        self.just_closed = true;
        self.title = false;
        self.start = true;
        self.back.clear();
        self.dirty = true;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Play,
    Resume,
    Restart,
    Chambers,
    Settings,
    Controls,
    MainMenu,
    Edit,
    NewLevel,
    Quit,
    Back,
    Load(usize),
    Folder(crate::LevelGroup),
}

enum Item {
    Action(String, Action),
    /// A settings row.
    Setting(usize),
    /// The settings pages, side by side.
    Tabs,
    /// A chamber: its name, best time, and it loads it.
    Chamber(usize, String, Option<f32>),
}

#[derive(Component)]
pub struct MenuRoot;
#[derive(Component)]
pub struct MenuContent;
#[derive(Component)]
pub struct ItemRow(usize);
#[derive(Component)]
pub struct ItemValue(usize);
#[derive(Component, Clone, Copy)]
pub enum Click {
    Item(usize),
    Nudge(usize, f32),
    Tab(Page),
}

const RED: Color = Color::srgb(0.86, 0.10, 0.07);
const INK: Color = Color::srgb(0.95, 0.95, 0.96);
const DIM: Color = Color::srgba(0.95, 0.95, 0.96, 0.55);
const ROW: Color = Color::srgba(1.0, 1.0, 1.0, 0.0);
const ROW_SELECTED: Color = Color::srgba(1.0, 1.0, 1.0, 0.10);
const BTN: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);

pub const CONTROLS: &[(&str, &str)] = &[
    ("Move", "WASD  /  left stick"),
    ("Look", "Mouse  /  right stick"),
    ("Jump, vault, wallrun, climb", "Space  /  A, LB"),
    ("Crouch, slide, roll", "Shift or C  /  B, LT"),
    ("Blue portal", "Left mouse  /  RT"),
    ("Orange portal", "Right mouse  /  RB"),
    ("Pick up, drop, press", "E  /  X"),
    ("Kick", "F  /  R3"),
    ("Turn 180", "Q  /  Y"),
    ("Dodge", "A or D + Space"),
    ("Reaction Time", "Left Alt  /  L3"),
    ("Respawn at checkpoint", "R  /  Back"),
    ("Checkpoints", "1 - 6"),
    ("Restart chamber", "Backspace"),
    ("Next / previous chamber", "M / N"),
    ("Pause", "Esc  /  Start"),
    ("Debug readout", "F3"),
    ("Mirror's Edge body / simple arms", "F2"),
];

fn items(menu: &Menu, game: &Game, progress: &Progress) -> Vec<Item> {
    let a = |s: &str, act| Item::Action(s.to_string(), act);
    match menu.screen {
        Screen::Title => {
            let first = progress.best.is_empty() && progress.last.is_none();
            vec![
                a(if first { "Play" } else { "Continue" }, Action::Play),
                a("Chambers", Action::Chambers),
                a("New level", Action::NewLevel),
                a("Level editor", Action::Edit),
                a("Settings", Action::Settings),
                a("Controls", Action::Controls),
                a("Quit", Action::Quit),
            ]
        }
        Screen::Pause => vec![
            a("Resume", Action::Resume),
            a("Restart chamber", Action::Restart),
            a("Chambers", Action::Chambers),
            a("Edit this level", Action::Edit),
            a("New level", Action::NewLevel),
            a("Settings", Action::Settings),
            a("Controls", Action::Controls),
            a("Main menu", Action::MainMenu),
            a("Quit", Action::Quit),
        ],
        Screen::Chambers => {
            let mut v: Vec<Item> = crate::LevelGroup::MENU
                .iter()
                .map(|&g| {
                    let n = game.groups.iter().filter(|x| **x == g).count();
                    a(&format!("{}  ({n})", g.name()), Action::Folder(g))
                })
                .collect();
            v.push(a("Back", Action::Back));
            v
        }
        Screen::Folder(g) => {
            // (Their own names, without the folder's.)
            let short = |name: &str| name.strip_prefix("Portal native: ").or_else(|| name.strip_prefix("Portal: ")).unwrap_or(name).to_string();
            let mut v: Vec<Item> = game.levels.iter().enumerate().filter(|(i, _)| game.groups[*i] == g).map(|(i, (name, _))| Item::Chamber(i, short(name), progress.best_for(name))).collect();
            v.push(a("Back", Action::Back));
            v
        }
        Screen::Settings(page) => {
            let mut v = vec![Item::Tabs];
            v.extend(settings::rows_on(page).into_iter().map(Item::Setting));
            v.push(a("Back", Action::Back));
            v
        }
        Screen::Controls => vec![a("Back", Action::Back)],
    }
}

pub fn setup(mut commands: Commands) {
    // Screenshot capture and PORTAL_MAP go straight into the chamber.
    let skip = std::env::var_os("PORTAL_CAPTURE").is_some() || std::env::var_os("PORTAL_MAP").is_some();
    commands.spawn((
        MenuRoot,
        Node { position_type: PositionType::Absolute, width: percent(100.0), height: percent(100.0), ..default() },
        GlobalZIndex(10),
        if skip { Visibility::Hidden } else { Visibility::Visible },
    ));
    commands.insert_resource(Menu { open: !skip, screen: Screen::Title, title: !skip, selected: 0, start: false, just_closed: false, back: vec![], dirty: true, edit: None });
}

/// Rebuild the menu's contents when the screen changes.
fn build(commands: &mut Commands, root: Entity, fonts: &UiFonts, menu: &Menu, list: &[Item], settings: &Settings, game: &Game) {
    let content = commands
        .spawn((
            MenuContent,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                flex_direction: FlexDirection::Row,
                ..default()
            },
            // The title screen shows the chamber behind it; elsewhere it's dimmed.
            BackgroundColor(if menu.title { Color::NONE } else { Color::srgba(0.0, 0.0, 0.0, 0.45) }),
        ))
        .id();
    commands.entity(root).add_child(content);
    let panel = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                padding: UiRect::new(px(64.0), px(44.0), px(48.0), px(48.0)),
                row_gap: px(2.0),
                width: px(620.0),
                height: percent(100.0),
                ..default()
            },
            // A dark band down the left, under the text.
            BackgroundColor(Color::srgba(0.02, 0.02, 0.03, 0.74)),
        ))
        .id();
    commands.entity(content).add_child(panel);

    let heading = |commands: &mut Commands, panel: Entity| {
        let (big, small) = match menu.screen {
            Screen::Title => ("PORTAL RUNNER", "Mirror's Edge, with a portal gun"),
            Screen::Pause => ("PAUSED", game.levels[game.level_index].0.as_str()),
            Screen::Chambers => ("CHAMBERS", "Best times are saved"),
            Screen::Folder(g) => ("CHAMBERS", g.about()),
            Screen::Settings(_) => ("SETTINGS", "Saved as you change them"),
            Screen::Controls => ("CONTROLS", "Keyboard and mouse  /  gamepad"),
        };
        let size = if menu.screen == Screen::Title { 54.0 } else { 40.0 };
        let h = commands.spawn((Text::new(big), fonts.bold(size), TextColor(RED))).id();
        let s = commands.spawn((Text::new(small), fonts.regular(17.0), TextColor(DIM), Node { margin: UiRect::bottom(px(28.0)), ..default() })).id();
        commands.entity(panel).add_children(&[h, s]);
    };
    heading(commands, panel);

    if menu.screen == Screen::Controls {
        let grid = commands.spawn(Node { display: Display::Grid, grid_template_columns: vec![GridTrack::auto(), GridTrack::auto()], column_gap: px(28.0), row_gap: px(6.0), margin: UiRect::bottom(px(22.0)), ..default() }).id();
        for (what, keys) in CONTROLS {
            let a = commands.spawn((Text::new(*what), fonts.regular(16.0), TextColor(DIM))).id();
            let b = commands.spawn((Text::new(*keys), fonts.regular(16.0), TextColor(INK))).id();
            commands.entity(grid).add_children(&[a, b]);
        }
        commands.entity(panel).add_child(grid);
    }

    for (i, item) in list.iter().enumerate() {
        let row = commands
            .spawn((
                ItemRow(i),
                Click::Item(i),
                Button,
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(14.0), px(7.0)),
                    column_gap: px(10.0),
                    border: UiRect::left(px(3.0)),
                    ..default()
                },
                BackgroundColor(ROW),
                BorderColor::all(Color::NONE),
            ))
            .id();
        commands.entity(panel).add_child(row);
        match item {
            Item::Action(label, _) => {
                let t = commands.spawn((Text::new(label.clone()), fonts.bold(24.0), TextColor(INK))).id();
                commands.entity(row).add_child(t);
            }
            Item::Chamber(_, name, best) => {
                // (A long list, closer together.)
                if list.len() > 12 {
                    commands.entity(row).entry::<Node>().and_modify(|mut n| n.padding = UiRect::axes(px(14.0), px(3.0)));
                }
                let t = commands.spawn((Text::new(name.clone()), fonts.regular(if list.len() > 12 { 18.0 } else { 20.0 }), TextColor(INK), Node { flex_grow: 1.0, ..default() })).id();
                let b = commands
                    .spawn((Text::new(best.map(crate::hud::fmt_time).unwrap_or_else(|| "-".into())), fonts.regular(17.0), TextColor(if best.is_some() { INK } else { DIM })))
                    .id();
                commands.entity(row).add_children(&[t, b]);
            }
            Item::Setting(r) => {
                let t = commands.spawn((Text::new(settings::row_label(*r)), fonts.regular(19.0), TextColor(INK), Node { flex_grow: 1.0, ..default() })).id();
                let minus = small_button(commands, fonts, "-", Click::Nudge(*r, -1.0));
                let v = commands
                    .spawn((ItemValue(*r), Text::new(settings::row_value(*r, settings)), fonts.regular(18.0), TextColor(INK), Node { width: px(92.0), justify_content: JustifyContent::Center, ..default() }, TextLayout::justify(Justify::Center)))
                    .id();
                let plus = small_button(commands, fonts, "+", Click::Nudge(*r, 1.0));
                commands.entity(row).add_children(&[t, minus, v, plus]);
            }
            Item::Tabs => {
                let Screen::Settings(current) = menu.screen else { continue };
                for p in Page::ALL {
                    let on = p == current;
                    let tab = commands
                        .spawn((
                            Click::Tab(p),
                            Button,
                            Node { padding: UiRect::axes(px(12.0), px(5.0)), border: UiRect::bottom(px(2.0)), ..default() },
                            BorderColor::all(if on { RED } else { Color::NONE }),
                            BackgroundColor(Color::NONE),
                        ))
                        .with_child((Text::new(p.name()), fonts.bold(18.0), TextColor(if on { INK } else { DIM })))
                        .id();
                    commands.entity(row).add_child(tab);
                }
            }
        }
    }
    let hint = match menu.screen {
        Screen::Settings(_) => "Up / Down pick  -  Left / Right change (on the tabs: page)  -  Esc back",
        Screen::Title => "Up / Down pick  -  Enter choose",
        _ => "Up / Down pick  -  Enter choose  -  Esc back",
    };
    let h = commands.spawn((Text::new(hint), fonts.regular(14.0), TextColor(DIM), Node { margin: UiRect::top(px(24.0)), ..default() })).id();
    commands.entity(panel).add_child(h);
}

fn small_button(commands: &mut Commands, fonts: &UiFonts, label: &str, click: Click) -> Entity {
    commands
        .spawn((
            click,
            Button,
            Node { width: px(30.0), height: px(28.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BackgroundColor(BTN),
        ))
        .with_child((Text::new(label), fonts.bold(18.0), TextColor(INK)))
        .id()
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut menu: ResMut<Menu>,
    mut settings: ResMut<Settings>,
    mut game: ResMut<Game>,
    progress: Res<Progress>,
    fonts: Res<UiFonts>,
    clicks: Query<(&Interaction, &Click), Changed<Interaction>>,
    mut rows: Query<(&ItemRow, &mut BackgroundColor, &mut BorderColor)>,
    mut values: Query<(&ItemValue, &mut Text)>,
    mut root: Query<(Entity, &mut Visibility), With<MenuRoot>>,
    content: Query<Entity, With<MenuContent>>,
    mut exit: MessageWriter<AppExit>,
    capture: Option<Res<crate::capture::Capture>>,
) {
    menu.just_closed = false;
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let esc = keys.just_pressed(KeyCode::Escape) || pad(GamepadButton::East) && menu.open;
    let start_button = pad(GamepadButton::Start);

    if !menu.open {
        if capture.is_none() && (keys.just_pressed(KeyCode::Escape) || start_button) {
            menu.open_at(Screen::Pause);
        } else if keys.just_pressed(KeyCode::F1) {
            menu.open_at(Screen::Controls);
        }
    } else {
        let list = items(&menu, &game, &progress);
        let n = list.len();
        let mut activate = keys.just_pressed(KeyCode::Enter) || pad(GamepadButton::South);
        if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyS) || pad(GamepadButton::DPadDown) {
            menu.selected = (menu.selected + 1) % n;
        }
        if keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyW) || pad(GamepadButton::DPadUp) {
            menu.selected = (menu.selected + n - 1) % n;
        }
        let mut dir = 0.0;
        if keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::KeyA) || pad(GamepadButton::DPadLeft) {
            dir = -1.0;
        }
        if keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::KeyD) || pad(GamepadButton::DPadRight) {
            dir = 1.0;
        }
        let mut nudge: Option<(usize, f32)> = None;
        let mut tab: Option<Page> = None;
        for (interaction, click) in &clicks {
            match (interaction, *click) {
                (Interaction::Pressed, Click::Item(i)) => {
                    menu.selected = i;
                    // Clicking a setting's row (not its buttons) does nothing more.
                    activate |= !matches!(list.get(i), Some(Item::Setting(_)) | Some(Item::Tabs));
                }
                (Interaction::Hovered, Click::Item(i)) => menu.selected = i,
                (Interaction::Pressed, Click::Nudge(r, d)) => nudge = Some((r, d)),
                (Interaction::Pressed, Click::Tab(p)) => tab = Some(p),
                _ => {}
            }
        }
        let selected = menu.selected.min(n.saturating_sub(1));
        if dir != 0.0 {
            match list.get(selected) {
                Some(Item::Setting(r)) => nudge = Some((*r, dir)),
                Some(Item::Tabs) => {
                    if let Screen::Settings(p) = menu.screen {
                        let k = Page::ALL.iter().position(|q| *q == p).unwrap_or(0) as i32 + dir as i32;
                        tab = Some(Page::ALL[k.rem_euclid(Page::ALL.len() as i32) as usize]);
                    }
                }
                _ => {}
            }
        }
        if let Some((r, d)) = nudge {
            settings::nudge(r, &mut settings, d);
            settings.save();
        }
        if let Some(p) = tab {
            menu.screen = Screen::Settings(p);
            menu.dirty = true;
        }

        let action = if esc {
            Some(Action::Back)
        } else if start_button && !menu.title {
            Some(Action::Resume)
        } else if activate {
            match list.get(selected) {
                Some(Item::Action(_, a)) => Some(*a),
                Some(Item::Chamber(i, _, _)) => Some(Action::Load(*i)),
                _ => None,
            }
        } else {
            None
        };
        match action {
            Some(Action::Play) => {
                let last = progress.last.as_ref().and_then(|l| game.levels.iter().position(|(n, _)| n == l));
                if let Some(i) = last.filter(|i| *i != game.level_index) {
                    game.pending = Some(i);
                }
                menu.close();
            }
            Some(Action::Resume) => menu.close(),
            Some(Action::Restart) => {
                game.pending = Some(game.level_index);
                menu.close();
            }
            Some(Action::Load(i)) => {
                game.pending = Some(i);
                menu.close();
            }
            Some(Action::Chambers) => menu.go(Screen::Chambers, true),
            Some(Action::Folder(g)) => menu.go(Screen::Folder(g), true),
            Some(Action::Settings) => menu.go(Screen::Settings(Page::Audio), true),
            Some(Action::Controls) => menu.go(Screen::Controls, true),
            Some(Action::MainMenu) => {
                menu.title = true;
                menu.go(Screen::Title, false);
            }
            Some(Action::Edit) => menu.close_to_editor(false),
            Some(Action::NewLevel) => menu.close_to_editor(true),
            Some(Action::Quit) => {
                exit.write(AppExit::Success);
            }
            Some(Action::Back) => match menu.back.pop() {
                Some(s) => {
                    menu.screen = s;
                    menu.selected = 0;
                    menu.dirty = true;
                }
                // Nowhere to go back to: the title stays; anything else resumes.
                None if menu.title => {
                    if menu.screen != Screen::Title {
                        menu.go(Screen::Title, false);
                    }
                }
                None => menu.close(),
            },
            None => {}
        }
    }

    let Ok((root, mut vis)) = root.single_mut() else { return };
    let want = if menu.open { Visibility::Visible } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
    if menu.dirty {
        menu.dirty = false;
        for e in &content {
            commands.entity(e).despawn();
        }
        if menu.open {
            let list = items(&menu, &game, &progress);
            build(&mut commands, root, &fonts, &menu, &list, &settings, &game);
        }
        return;
    }
    for (row, mut bg, mut border) in &mut rows {
        let on = row.0 == menu.selected;
        *bg = BackgroundColor(if on { ROW_SELECTED } else { ROW });
        *border = BorderColor::all(if on { RED } else { Color::NONE });
    }
    if settings.is_changed() {
        for (v, mut text) in &mut values {
            **text = settings::row_value(v.0, &settings);
        }
    }
}

/// At the title screen, the camera drifts slowly round the chamber's start.
pub fn title_camera(time: Res<Time<Real>>, menu: Res<Menu>, game: Res<Game>, mut camera: Query<&mut Transform, With<crate::PlayerCamera>>) {
    if !(menu.open && menu.title) {
        return;
    }
    let Ok(mut tf) = camera.single_mut() else { return };
    let t = time.elapsed_secs();
    let cp = &game.sim.level.checkpoints[0];
    let yaw = cp.yaw + 0.55 * (t * 0.08).sin();
    let pitch = 0.06 + 0.05 * (t * 0.11).sin();
    tf.translation = cp.spawn + Vec3::Y * 1.75;
    tf.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
}
