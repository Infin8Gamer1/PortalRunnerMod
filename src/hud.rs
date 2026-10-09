//! The HUD: Portal's crosshair (its two brackets fill in as each portal opens), Mirror's
//! Edge's speed readout and sprint bar, the chamber's title card, hints, and messages.

use bevy::prelude::*;

use aperture::PortalColor;

use crate::portal_render::color_of;
use crate::{settings, Game};

#[derive(Component)]
pub struct HudStats;
#[derive(Component)]
pub struct HudFlash;
#[derive(Component)]
pub struct HudHelp;
#[derive(Component)]
pub struct HudClickToPlay;
#[derive(Component)]
pub struct SprintBar;
#[derive(Component)]
pub struct HudHint;
#[derive(Component)]
pub struct HudTitle;
#[derive(Component)]
pub struct Bracket(PortalColor);
#[derive(Component)]
pub struct Overlay;

pub const HELP: &str = "\
WASD move  |  Mouse look  |  Space jump, vault, wall moves  |  Shift / C crouch, slide, roll
Left mouse BLUE portal  |  Right mouse ORANGE portal  |  E pick up, drop, press  |  F kick
Q 180 turn  |  A or D + Space dodge  |  Left Alt Reaction Time
R respawn  |  Backspace restart chamber  |  1-6 checkpoints  |  M / N next / previous map
F1 help  |  F2 Mirror's Edge body / procedural arms  |  Esc menu
Pad: RT blue, RB orange, X use, A/LB jump, B/LT crouch, Y turn, R3 kick";

pub fn fmt_time(t: f32) -> String {
    format!("{:02}:{:05.2}", (t / 60.0) as u32, t % 60.0)
}

pub fn setup(mut commands: Commands) {
    let font = |px_: f32| TextFont { font_size: px_.into(), ..default() };
    let ink = TextColor(Color::srgb(0.92, 0.93, 0.95));
    let shadow = TextShadow { offset: Vec2::new(1.5, 1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.85) };

    commands.spawn((
        HudStats,
        Text::new(""),
        font(16.0),
        ink.clone(),
        shadow,
        Node { position_type: PositionType::Absolute, top: px(14.0), left: px(16.0), ..default() },
    ));
    commands
        .spawn((
            Node { position_type: PositionType::Absolute, top: px(118.0), left: px(16.0), width: px(180.0), height: px(5.0), ..default() },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.15)),
        ))
        .with_children(|p| {
            p.spawn((SprintBar, Node { width: percent(0.0), height: percent(100.0), ..default() }, BackgroundColor(Color::srgb(0.86, 0.10, 0.07))));
        });

    // Portal's crosshair: a dot, and a bracket each side that fills when that portal is open.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: percent(50.0),
            top: percent(50.0),
            width: px(0.0),
            height: px(0.0),
            ..default()
        })
        .with_children(|c| {
            c.spawn((
                Node { position_type: PositionType::Absolute, left: px(-2.0), top: px(-2.0), width: px(4.0), height: px(4.0), ..default() },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
            ));
            for (color, left) in [(PortalColor::Blue, -22.0), (PortalColor::Orange, 12.0)] {
                c.spawn((
                    Bracket(color),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(left),
                        top: px(-14.0),
                        width: px(10.0),
                        height: px(28.0),
                        border: if color == PortalColor::Blue { UiRect::left(px(3.0)) } else { UiRect::right(px(3.0)) },
                        border_radius: BorderRadius::all(px(14.0)),
                        ..default()
                    },
                    BorderColor::all(Color::LinearRgba(color_of(color))),
                    BackgroundColor(Color::NONE),
                ));
            }
        });

    commands.spawn((
        HudFlash,
        Text::new(""),
        font(28.0),
        TextColor(Color::srgb(1.0, 0.55, 0.1)),
        shadow,
        TextLayout::justify(Justify::Center),
        Node { position_type: PositionType::Absolute, top: percent(58.0), width: percent(100.0), justify_content: JustifyContent::Center, ..default() },
    ));
    commands.spawn((
        HudHint,
        Text::new(""),
        font(18.0),
        ink.clone(),
        shadow,
        TextLayout::justify(Justify::Center),
        Node { position_type: PositionType::Absolute, top: percent(70.0), width: percent(100.0), justify_content: JustifyContent::Center, ..default() },
    ));
    commands.spawn((
        HudTitle,
        Text::new(""),
        font(40.0),
        ink.clone(),
        shadow,
        TextLayout::justify(Justify::Center),
        Node { position_type: PositionType::Absolute, top: percent(12.0), width: percent(100.0), justify_content: JustifyContent::Center, ..default() },
    ));
    commands.spawn((
        HudHelp,
        Text::new(HELP),
        font(13.0),
        ink.clone(),
        shadow,
        TextLayout::justify(Justify::Right),
        Node { position_type: PositionType::Absolute, top: px(14.0), right: px(16.0), ..default() },
    ));
    commands.spawn((
        HudClickToPlay,
        Text::new("CLICK TO PLAY"),
        font(40.0),
        TextColor(Color::srgb(1.0, 0.55, 0.1)),
        shadow,
        TextLayout::justify(Justify::Center),
        Node { position_type: PositionType::Absolute, top: percent(40.0), width: percent(100.0), justify_content: JustifyContent::Center, ..default() },
    ));
    // A full-screen tint: red while a laser burns, black fading in and out on death.
    commands.spawn((
        Overlay,
        Node { position_type: PositionType::Absolute, width: percent(100.0), height: percent(100.0), ..default() },
        BackgroundColor(Color::NONE),
        GlobalZIndex(-1),
    ));
}

#[allow(clippy::type_complexity)]
pub fn update(
    game: Res<Game>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    me: Option<Res<crate::me_viewmodel::MeArms>>,
    menu: Res<settings::Menu>,
    mut texts: ParamSet<(
        Query<&mut Text, With<HudStats>>,
        Query<(&mut Text, &mut TextColor), With<HudFlash>>,
        Query<&mut Text, With<HudHint>>,
        Query<(&mut Text, &mut TextColor), With<HudTitle>>,
    )>,
    mut vis: ParamSet<(Query<&mut Visibility, With<HudHelp>>, Query<&mut Visibility, With<HudClickToPlay>>)>,
    mut bar: Query<&mut Node, With<SprintBar>>,
    mut brackets: Query<(&Bracket, &mut BackgroundColor), Without<Overlay>>,
    mut overlay: Query<&mut BackgroundColor, With<Overlay>>,
) {
    let c = &game.ctrl;
    let sim = &game.sim;
    let speed = c.horizontal_speed();
    if let Ok(mut t) = texts.p0().single_mut() {
        let best = game.best[game.level_index].map(|b| format!("   Best {}", fmt_time(b))).unwrap_or_default();
        let anim = me.as_ref().filter(|m| m.active).map(|m| format!("   [{}]", m.current_anim())).unwrap_or_default();
        let cp = sim.level.checkpoints.get(sim.checkpoint).map(|c| c.name).unwrap_or("");
        t.0 = format!(
            "{}{}   {:.0} fps\n{:.1} m/s  ({:.0} km/h)\nTime {}{}\n{} - {}\nSprint",
            c.state.name(),
            anim,
            diagnostics.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0),
            speed,
            speed * 3.6,
            fmt_time(game.timer),
            best,
            game.levels[game.level_index].0,
            cp,
        );
    }
    if let Ok(mut n) = bar.single_mut() {
        n.width = percent(c.sprint_charge * 100.0);
    }
    if let Ok((mut t, mut color)) = texts.p1().single_mut() {
        match &game.flash {
            Some((s, age)) => {
                let a = (1.0 - (age.max(0.0) - 1.0) / 0.6).clamp(0.0, 1.0);
                t.0 = s.clone();
                color.0 = Color::srgba(1.0, 0.55, 0.1, a);
            }
            None => t.0.clear(),
        }
    }
    if let Ok(mut t) = texts.p2().single_mut() {
        t.0 = if menu.open { String::new() } else { sim.hint(c.feet).unwrap_or("").to_string() };
    }
    if let Ok((mut t, mut color)) = texts.p3().single_mut() {
        let a = (1.0 - (game.intro - 2.5) / 1.0).clamp(0.0, 1.0);
        if a > 0.0 && !menu.open {
            let intro = sim.level.intro;
            t.0 = if intro.is_empty() { sim.level.name.to_string() } else { format!("{}\n{}", sim.level.name, intro) };
            color.0 = Color::srgba(0.95, 0.95, 0.97, a);
        } else {
            t.0.clear();
        }
    }
    if let Ok(mut v) = vis.p0().single_mut() {
        *v = if game.show_help { Visibility::Inherited } else { Visibility::Hidden };
    }
    if let Ok(mut v) = vis.p1().single_mut() {
        *v = if game.locked || menu.open || game.scripted.is_some() { Visibility::Hidden } else { Visibility::Inherited };
    }
    for (b, mut bg) in &mut brackets {
        let open = sim.portal(b.0).is_some();
        let col = color_of(b.0);
        bg.0 = if open { Color::LinearRgba(LinearRgba { alpha: 0.75, ..col }) } else { Color::NONE };
    }
    if let Ok(mut bg) = overlay.single_mut() {
        let death = if game.died < 0.6 { 1.0 - game.died / 0.6 } else { 0.0 };
        let burn = (sim.burn / aperture::sim::LASER_KILL).clamp(0.0, 1.0);
        bg.0 = if death > 0.0 {
            Color::srgba(0.0, 0.0, 0.0, death)
        } else if burn > 0.0 {
            Color::srgba(0.9, 0.1, 0.05, burn * 0.45)
        } else {
            Color::NONE
        };
    }
}
