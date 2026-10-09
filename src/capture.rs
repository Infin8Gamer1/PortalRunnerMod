//! Screenshot capture, for checking the game without a person at the keyboard:
//! `PORTAL_CAPTURE=<dir> PORTAL_SCRIPT=<name> cargo run --release` plays a scripted run,
//! saves PNGs into `<dir>` along the way, and quits.

use bevy::prelude::*;
use bevy::render::view::window::screenshot::{save_to_disk, Screenshot};

use aperture::PortalColor;
use faith_move::{Input as MoveInput, State as MoveState};

use crate::Game;

#[derive(Resource)]
pub struct Capture {
    dir: String,
    script: String,
    frame: u32,
    step: usize,
    wait: u32,
}

pub fn setup(mut commands: Commands, mut game: ResMut<Game>) {
    if let Ok(dir) = std::env::var("PORTAL_CAPTURE") {
        let _ = std::fs::create_dir_all(&dir);
        let script = std::env::var("PORTAL_SCRIPT").unwrap_or_else(|_| "tour".into());
        game.locked = true;
        game.show_help = false;
        // Every frame is 1/60 s of game time, however fast it renders: scripts count frames.
        commands.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)));
        commands.insert_resource(Capture { dir, script, frame: 0, step: 0, wait: 0 });
    }
}

/// One scripted action.
enum Act {
    /// Wait this many frames.
    Wait(u32),
    /// Look this way (degrees: yaw 0 = -Z, positive turns left; pitch positive up).
    Look(f32, f32),
    /// Hold this input for some frames.
    Hold(MoveInput, u32),
    Fire(PortalColor),
    Use,
    Shot(&'static str),
    /// Load map `n`.
    Map(usize),
    /// Put her at a point.
    Teleport(Vec3),
    /// Open the settings menu.
    Menu,
}

fn fwd() -> MoveInput {
    MoveInput { move_axis: Vec2::new(0.0, 1.0), ..default() }
}

fn script(name: &str) -> Vec<Act> {
    use Act::*;
    match name {
        // A wall portal pair in the arena: look through one, walk through it.
        "portals" => vec![
            Map(0),
            Wait(60),
            Teleport(Vec3::new(0.0, 0.0, -12.0)),
            Wait(30),
            Look(0.0, -8.0),
            Wait(5),
            Fire(PortalColor::Blue),
            Wait(10),
            Look(90.0, -4.0),
            Wait(5),
            Fire(PortalColor::Orange),
            Wait(20),
            Shot("01_orange_portal"),
            Look(0.0, 0.0),
            Wait(30),
            Shot("02_blue_portal_view"),
            Look(-25.0, -5.0),
            Wait(10),
            Shot("03_angle"),
            Look(0.0, 0.0),
            Hold(fwd(), 60),
            Shot("04_walking_in"),
            Hold(fwd(), 70),
            Shot("05_through"),
            Look(180.0, 0.0),
            Wait(20),
            Shot("06_look_back"),
            // Close up: step into blue slowly.
            Teleport(Vec3::new(0.0, 0.0, -17.5)),
            Look(0.0, 0.0),
            Wait(20),
            Shot("07_close"),
            Hold(MoveInput { move_axis: Vec2::new(0.0, 0.35), ..default() }, 25),
            Shot("08_closer"),
            Hold(MoveInput { move_axis: Vec2::new(0.0, 0.35), ..default() }, 12),
            Shot("09_in_it"),
            Hold(MoveInput { move_axis: Vec2::new(0.0, 0.35), ..default() }, 12),
            Shot("10_out"),
        ],
        // Chamber 04's conversion gel: a portal on it, seen from across the goo and up close.
        "gelportal" => vec![
            Map(4),
            Wait(60),
            Teleport(Vec3::new(0.0, 5.0, -58.5)),
            Wait(30),
            Look(0.0, -5.6),
            Wait(5),
            Fire(PortalColor::Blue),
            Wait(10),
            Look(-90.0, -5.0),
            Wait(5),
            Fire(PortalColor::Orange),
            Wait(30),
            Look(0.0, -5.6),
            Wait(10),
            Shot("01_gel_portal_far"),
            Teleport(Vec3::new(0.0, 5.0, -72.5)),
            Look(0.0, -35.0),
            Wait(30),
            Shot("02_gel_portal_near"),
            Look(-90.0, -5.0),
            Teleport(Vec3::new(5.5, 5.0, -58.0)),
            Wait(30),
            Shot("03_orange_view"),
        ],
        // Picking up a cube, holding it, and firing while holding it.
        "pickup" => vec![
            Map(0),
            Wait(60),
            Teleport(Vec3::new(18.0, 0.0, 19.6)),
            Look(0.0, -40.0),
            Wait(60),
            Use,
            Wait(4),
            Shot("p1"),
            Wait(6),
            Shot("p2"),
            Wait(6),
            Shot("p3"),
            Wait(10),
            Shot("p4"),
            Look(0.0, -5.0),
            Wait(30),
            Shot("p5_holding"),
            Fire(PortalColor::Blue),
            Wait(2),
            Shot("p6_fire"),
        ],
        // Speed: sprinting, then far past it on Chamber 04's propulsion gel.
        "speed" => vec![
            Map(4),
            Wait(60),
            Look(0.0, -2.0),
            Hold(fwd(), 60),
            Shot("01_running"),
            Hold(fwd(), 70),
            Shot("02_sprinting"),
            Hold(fwd(), 45),
            Shot("03_on_gel"),
        ],
        "menu" => vec![Map(1), Wait(30), Shot("before_menu"), Menu, Wait(10), Shot("menu")],
        // The gun, standing and looking about.
        "gun" => vec![Map(0), Wait(90), Look(0.0, 0.0), Wait(10), Shot("gun_ahead"), Look(0.0, -40.0), Wait(10), Shot("gun_down"), Fire(PortalColor::Blue), Wait(3), Shot("gun_fire")],
        // The Proving Grounds' stations.
        "stations" => vec![
            Map(0),
            Wait(60),
            Teleport(Vec3::new(14.0, 0.0, 9.0)),
            Look(-35.0, -12.0),
            Wait(20),
            Shot("s01_laser"),
            // The redirection cube from its dropper, into the beam facing the catcher.
            Teleport(Vec3::new(18.0, 0.0, 19.6)),
            Look(0.0, -40.0),
            Wait(20),
            Use,
            Wait(20),
            Shot("s02_holding"),
            Teleport(Vec3::new(17.0, 0.0, 2.0)),
            Look(180.0, -20.0),
            Wait(40),
            Shot("s03_in_beam"),
            Use,
            Wait(30),
            Teleport(Vec3::new(20.0, 0.0, -2.0)),
            Look(150.0, -8.0),
            Wait(20),
            Shot("s04_catcher"),
            Teleport(Vec3::new(14.0, 0.0, -6.0)),
            Look(0.0, -15.0),
            Wait(20),
            Shot("s05_bridge"),
            // Gel.
            Teleport(Vec3::new(-5.0, 0.0, -5.6)),
            Look(0.0, -45.0),
            Wait(10),
            Use,
            Teleport(Vec3::new(1.0, 0.0, 2.4)),
            Look(0.0, -45.0),
            Wait(10),
            Use,
            Teleport(Vec3::new(4.0, 0.0, -14.6)),
            Look(0.0, -45.0),
            Wait(10),
            Use,
            Wait(200),
            Teleport(Vec3::new(-2.0, 0.0, 6.0)),
            Look(0.0, -10.0),
            Wait(10),
            Shot("s06_gel"),
            // The faith plate.
            Teleport(Vec3::new(-14.0, 0.0, -1.5)),
            Look(0.0, 0.0),
            Hold(fwd(), 40),
            Shot("s07_plate_flight"),
            Wait(60),
            Look(180.0, -25.0),
            Wait(10),
            Shot("s08_platform"),
        ],
        // Two portals facing each other: the hall of mirrors.
        "recursion" => vec![
            Map(0),
            Wait(30),
            Teleport(Vec3::new(-20.0, 0.0, -1.0)),
            Look(90.0, -3.0),
            Wait(5),
            Fire(PortalColor::Blue),
            Wait(5),
            Look(-90.0, -1.6),
            Wait(5),
            Fire(PortalColor::Orange),
            Wait(5),
            Teleport(Vec3::new(-14.0, 0.0, -1.0)),
            Look(90.0, 0.0),
            Wait(30),
            Shot("r01_hall"),
            Look(70.0, 0.0),
            Wait(10),
            Shot("r02_hall_angle"),
        ],
        // Chamber 02's fling.
        "fling" => vec![
            Map(2),
            Wait(30),
            Teleport(Vec3::new(0.0, 12.0, 0.6)),
            Wait(10),
            Look(0.0, -62.0),
            Wait(5),
            Fire(PortalColor::Blue),
            Wait(20),
            Shot("f01_aim"),
            Look(0.0, -20.0),
            Hold(fwd(), 6),
            Hold(MoveInput { move_axis: Vec2::new(0.0, 1.0), jump_pressed: true, jump_held: true, ..default() }, 1),
            Hold(MoveInput::default(), 16),
            Shot("f02_falling"),
            Hold(MoveInput::default(), 22),
            Shot("f03_falling_more"),
            Hold(MoveInput::default(), 14),
            Shot("f04_out"),
            Hold(MoveInput::default(), 20),
            Shot("f05_flying"),
            Hold(MoveInput::default(), 60),
            Shot("f06_landed"),
        ],
        // Everything in a row: a tour of each map's start.
        _ => {
            let mut v = Vec::new();
            for i in 0..aperture::levels::all().len() {
                v.extend([Map(i), Wait(90), Shot(Box::leak(format!("map{i:02}").into_boxed_str()))]);
            }
            v
        }
    }
}

pub fn run(
    mut commands: Commands,
    mut cap: ResMut<Capture>,
    mut game: ResMut<Game>,
    mut exit: MessageWriter<AppExit>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    mut menu: ResMut<crate::settings::Menu>,
) {
    let fps = diagnostics.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
    cap.frame += 1;
    if cap.frame < 20 {
        return;
    }
    let acts = script(&cap.script);
    game.scripted = Some(MoveInput::default());
    if cap.wait > 0 {
        cap.wait -= 1;
        if let Some(Act::Hold(i, _)) = acts.get(cap.step) {
            game.scripted = Some(*i);
        }
        if cap.wait == 0 {
            cap.step += 1;
        }
        return;
    }
    let Some(act) = acts.get(cap.step) else {
        // Give the last screenshot time to be written.
        cap.step += 1;
        if cap.step > acts.len() + 60 {
            println!("capture: done");
            exit.write(AppExit::Success);
        }
        return;
    };
    match act {
        Act::Wait(n) => {
            cap.wait = *n;
            return;
        }
        Act::Hold(i, n) => {
            game.scripted = Some(*i);
            cap.wait = *n;
            return;
        }
        Act::Look(yaw, pitch) => {
            game.ctrl.yaw = yaw.to_radians();
            game.ctrl.pitch = pitch.to_radians();
        }
        Act::Fire(c) => game.scripted_fire = Some(*c),
        Act::Use => game.scripted_use = true,
        Act::Shot(name) => {
            let path = format!("{}/{name}.png", cap.dir);
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            println!(
                "capture: {name}  fps {fps:.0}  feet {} state {} portals {:?} held {:?} props {:?}",
                game.ctrl.feet,
                game.ctrl.state.name(),
                game.sim.portals.iter().map(|p| p.map(|p| p.center)).collect::<Vec<_>>(),
                game.sim.held,
                game.sim.props.iter().filter(|p| p.alive).map(|p| p.pos).collect::<Vec<_>>()
            );
        }
        Act::Map(i) => {
            game.pending = Some(*i);
        }
        Act::Menu => menu.open = true,
        Act::Teleport(p) => {
            // What she's holding comes too.
            let d = *p - game.ctrl.feet;
            if let Some(h) = game.sim.held {
                game.sim.props[h].pos += d;
            }
            game.ctrl.feet = *p;
            game.ctrl.state = MoveState::Air;
        }
    }
    cap.step += 1;
}
