//! The HUD, kept clean: Portal's crosshair (its two brackets fill in as each portal
//! opens), the chamber's title card, hints, short messages, and the laser and death tints.
//! The chamber timer shows if it's turned on in the settings; F3 shows a debug readout
//! (movement state, animation, speed, frame rate, sprint).

use bevy::prelude::*;

use aperture::PortalColor;

use crate::fonts::UiFonts;
use crate::menu::Menu;
use crate::portal_render::color_of;
use crate::progress::Progress;
use crate::{settings, Game};

/// The HUD's top-level nodes that `update` doesn't show and hide itself (a spectator camera
/// hides them).
#[derive(Component)]
pub struct HudRoot;
#[derive(Component)]
pub struct HudDebug;
#[derive(Component)]
pub struct HudDebugText;
#[derive(Component)]
pub struct HudFlash;
#[derive(Component)]
pub struct HudClickToPlay;
#[derive(Component)]
pub struct SprintBar;
#[derive(Component)]
pub struct HudHint;
#[derive(Component)]
pub struct HudHintText;
#[derive(Component)]
pub struct HudTitle;
#[derive(Component)]
pub struct HudTitleName;
#[derive(Component)]
pub struct HudTitleIntro;
#[derive(Component)]
pub struct HudTimer;
#[derive(Component)]
pub struct Bracket(PortalColor);
#[derive(Component)]
pub struct Overlay;
/// The crosshair's root (hidden in the menus).
#[derive(Component)]
pub struct Crosshair;

pub fn fmt_time(t: f32) -> String {
    format!("{:02}:{:05.2}", (t / 60.0) as u32, t % 60.0)
}

const INK: Color = Color::srgb(0.95, 0.95, 0.96);
const ORANGE: Color = Color::srgb(1.0, 0.55, 0.1);

fn shadow() -> TextShadow {
    TextShadow { offset: Vec2::new(1.0, 1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.75) }
}

pub fn setup(mut commands: Commands, fonts: Res<UiFonts>) {
    // ---- the debug readout (F3)
    commands
        .spawn((
            HudDebug,
            Node { position_type: PositionType::Absolute, top: px(12.0), left: px(14.0), flex_direction: FlexDirection::Column, row_gap: px(6.0), ..default() },
            Visibility::Hidden,
        ))
        .with_children(|d| {
            d.spawn((HudDebugText, Text::new(""), fonts.regular(14.0), TextColor(INK), shadow()));
            d.spawn((Node { width: px(160.0), height: px(4.0), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.15)))).with_children(|p| {
                p.spawn((SprintBar, Node { width: percent(0.0), height: percent(100.0), ..default() }, BackgroundColor(Color::srgb(0.86, 0.10, 0.07))));
            });
        });

    // ---- the timer (top right, when on)
    commands.spawn((
        HudTimer,
        Text::new(""),
        fonts.bold(20.0),
        TextColor(INK),
        shadow(),
        TextLayout::justify(Justify::Right),
        Node { position_type: PositionType::Absolute, top: px(16.0), right: px(20.0), ..default() },
    ));

    // ---- Portal's crosshair: a dot, and a bracket each side that fills when that portal is open.
    commands
        .spawn((Crosshair, Node { position_type: PositionType::Absolute, left: percent(50.0), top: percent(50.0), width: px(0.0), height: px(0.0), ..default() }))
        .with_children(|c| {
            c.spawn((
                Node { position_type: PositionType::Absolute, left: px(-2.0), top: px(-2.0), width: px(4.0), height: px(4.0), border_radius: BorderRadius::all(px(2.0)), ..default() },
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

    // ---- messages: just under the crosshair
    commands.spawn((
        HudRoot,
        HudFlash,
        Text::new(""),
        fonts.bold(22.0),
        TextColor(ORANGE),
        shadow(),
        TextLayout::justify(Justify::Center),
        Node { position_type: PositionType::Absolute, top: percent(57.0), width: percent(100.0), justify_content: JustifyContent::Center, ..default() },
    ));

    // ---- hints: low, on a soft dark band
    commands
        .spawn((HudRoot, Node { position_type: PositionType::Absolute, bottom: percent(14.0), width: percent(100.0), justify_content: JustifyContent::Center, ..default() }))
        .with_children(|r| {
            r.spawn((
                HudHint,
                Node { padding: UiRect::axes(px(16.0), px(8.0)), max_width: px(900.0), border_radius: BorderRadius::all(px(6.0)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                Visibility::Hidden,
            ))
            .with_child((HudHintText, Text::new(""), fonts.regular(17.0), TextColor(INK), TextLayout::justify(Justify::Center)));
        });

    // ---- the chamber's title card
    commands
        .spawn((
            HudRoot,
            HudTitle,
            Node { position_type: PositionType::Absolute, top: percent(14.0), width: percent(100.0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(6.0), ..default() },
        ))
        .with_children(|t| {
            t.spawn((HudTitleName, Text::new(""), fonts.bold(42.0), TextColor(INK), shadow(), TextLayout::justify(Justify::Center)));
            t.spawn((HudTitleIntro, Text::new(""), fonts.regular(19.0), TextColor(INK), shadow(), TextLayout::justify(Justify::Center)));
        });

    commands.spawn((
        HudClickToPlay,
        Text::new("Click to resume"),
        fonts.regular(18.0),
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
        shadow(),
        TextLayout::justify(Justify::Center),
        Node { position_type: PositionType::Absolute, top: percent(62.0), width: percent(100.0), justify_content: JustifyContent::Center, ..default() },
    ));
    // A full-screen tint: red while a laser burns, black fading in and out on death.
    commands.spawn((
        Overlay,
        Node { position_type: PositionType::Absolute, width: percent(100.0), height: percent(100.0), ..default() },
        BackgroundColor(Color::NONE),
        GlobalZIndex(-1),
    ));
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update(
    game: Res<Game>,
    progress: Res<Progress>,
    settings: Res<settings::Settings>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    me: Option<Res<crate::me_viewmodel::MeArms>>,
    menu: Res<Menu>,
    pads: Query<&Gamepad>,
    mut texts: ParamSet<(
        Query<&mut Text, With<HudDebugText>>,
        Query<(&mut Text, &mut TextColor), With<HudFlash>>,
        Query<&mut Text, With<HudHintText>>,
        Query<(&mut Text, &mut TextColor), With<HudTitleName>>,
        Query<(&mut Text, &mut TextColor), With<HudTitleIntro>>,
        Query<&mut Text, With<HudTimer>>,
    )>,
    mut vis: ParamSet<(
        Query<&mut Visibility, With<HudDebug>>,
        Query<&mut Visibility, With<HudClickToPlay>>,
        Query<&mut Visibility, With<HudHint>>,
        Query<&mut Visibility, With<HudTimer>>,
        Query<&mut Visibility, With<Crosshair>>,
    )>,
    mut bar: Query<&mut Node, With<SprintBar>>,
    mut brackets: Query<(&Bracket, &mut BackgroundColor), Without<Overlay>>,
    mut overlay: Query<&mut BackgroundColor, With<Overlay>>,
) {
    let c = &game.ctrl;
    let sim = &game.sim;
    // (A spectator camera hides the HUD, unless it's asked to show it.)
    let playing = !menu.open && game.spectate.is_none_or(|s| s.hud);
    let speed = c.horizontal_speed();
    let name = game.levels[game.level_index].0.as_str();

    if let Ok(mut v) = vis.p4().single_mut() {
        *v = if playing { Visibility::Inherited } else { Visibility::Hidden };
    }
    if let Ok(mut v) = vis.p0().single_mut() {
        *v = if game.show_debug && playing { Visibility::Inherited } else { Visibility::Hidden };
    }
    if game.show_debug {
        if let Ok(mut t) = texts.p0().single_mut() {
            let anim = me.as_ref().filter(|m| m.active).map(|m| format!("   [{}]", m.current_anim())).unwrap_or_default();
            let cp = sim.level.checkpoints.get(sim.checkpoint).map(|c| c.name).unwrap_or("");
            t.0 = format!(
                "{}{}   {:.0} fps\n{:.1} m/s  ({:.0} km/h)\n{} - {}",
                c.state.name(),
                anim,
                diagnostics.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0),
                speed,
                speed * 3.6,
                name,
                cp,
            );
        }
        if let Ok(mut n) = bar.single_mut() {
            n.width = percent(c.sprint_charge * 100.0);
        }
    }

    // (A course is timed whatever the setting: it's a time trial.)
    let course = sim.level.course;
    let timer_on = (settings.show_timer > 0.5 || course) && playing;
    if let Ok(mut v) = vis.p3().single_mut() {
        *v = if timer_on { Visibility::Inherited } else { Visibility::Hidden };
    }
    if timer_on {
        if let Ok(mut t) = texts.p5().single_mut() {
            let best = progress.best_for(name).map(|b| format!("\nBest {}", fmt_time(b))).unwrap_or_default();
            // A course's checkpoints passed of how many, and the last split.
            let count = if course {
                let of = sim.level.checkpoints.len().saturating_sub(1);
                let split = game.splits.last().map(|s| format!("  ({})", fmt_time(*s))).unwrap_or_default();
                format!("\nCheckpoint {} / {of}{split}", sim.checkpoint)
            } else {
                String::new()
            };
            t.0 = format!("{}{count}{}", fmt_time(game.timer), best);
        }
    }

    if let Ok((mut t, mut color)) = texts.p1().single_mut() {
        match &game.flash {
            Some((s, age)) if playing => {
                let a = (1.0 - (age.max(0.0) - 1.0) / 0.6).clamp(0.0, 1.0);
                t.0 = s.clone();
                color.0 = ORANGE.with_alpha(a);
            }
            _ => t.0.clear(),
        }
    }
    let hint = if playing { sim.hint(c.feet).unwrap_or("") } else { "" };
    if let Ok(mut t) = texts.p2().single_mut() {
        if t.0 != hint {
            t.0 = hint.to_string();
        }
    }
    if let Ok(mut v) = vis.p2().single_mut() {
        *v = if hint.is_empty() { Visibility::Hidden } else { Visibility::Inherited };
    }

    let a = if playing { (1.0 - (game.intro - 2.5) / 1.0).clamp(0.0, 1.0) } else { 0.0 };
    if let Ok((mut t, mut color)) = texts.p3().single_mut() {
        t.0 = if a > 0.0 { sim.level.name.to_string() } else { String::new() };
        color.0 = INK.with_alpha(a);
    }
    if let Ok((mut t, mut color)) = texts.p4().single_mut() {
        t.0 = if a > 0.0 { sim.level.intro.to_string() } else { String::new() };
        color.0 = INK.with_alpha(a * 0.85);
    }

    if let Ok(mut v) = vis.p1().single_mut() {
        let waiting = !game.locked && playing && game.scripted.is_none() && pads.is_empty();
        *v = if waiting { Visibility::Inherited } else { Visibility::Hidden };
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
        } else if burn > 0.0 || sim.hurt > 0.0 {
            Color::srgba(0.9, 0.1, 0.05, burn.max(sim.hurt) * 0.45)
        } else {
            Color::NONE
        };
    }
}
