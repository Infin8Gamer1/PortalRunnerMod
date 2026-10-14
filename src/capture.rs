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
    /// Hold the script (real time) while new shaders compile: at the start, and after a map
    /// loads. Frames run far faster than 60 a second here, so frame counts alone can shoot
    /// before the renderer's ready (a black frame).
    hold_until: Option<std::time::Instant>,
    /// Real time per frame (smoothed, ms): the clock the game runs on here is fixed.
    real_ms: f32,
    last: Option<std::time::Instant>,
    /// Keep her here (a view set up to look at something), till another map loads.
    hold: Option<Vec3>,
}

pub fn setup(mut commands: Commands, mut game: ResMut<Game>) {
    if let Ok(dir) = std::env::var("PORTAL_CAPTURE") {
        let _ = std::fs::create_dir_all(&dir);
        let script = std::env::var("PORTAL_SCRIPT").unwrap_or_else(|_| "tour".into());
        game.locked = true;
        // Every frame is 1/60 s of game time, however fast it renders: scripts count frames.
        commands.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)));
        commands.insert_resource(Capture { dir, script, frame: 0, step: 0, wait: 0, hold_until: Some(std::time::Instant::now() + std::time::Duration::from_secs(3)), real_ms: 0.0, last: None, hold: None });
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
    /// Throw her at this velocity.
    Launch(Vec3),
    /// A spectator camera, its eye here, looking this way (degrees, as `Look`): nothing
    /// touches it, and she stands still where she is as the chamber runs on. `Look` turns it.
    Spectate(Vec3, f32, f32),
    /// The spectator camera keeps looking at the first live pellet.
    TrackPellet,
    /// Back to her view.
    Unspectate,
    /// Put a portal here (centre, facing, up).
    Portal(PortalColor, Vec3, Vec3, Vec3),
    /// Open a menu screen.
    Menu(crate::menu::Screen),
    /// Hold this input for some frames, with a screenshot every frame (name_00, name_01...).
    Film(&'static str, MoveInput, u32),
    /// Wait until a live pellet has reached this x, or this many frames pass.
    WaitPelletX(f32, u32),
    /// Wait until a live pellet's within this far of a point, or this many frames pass.
    WaitPelletNear(Vec3, f32, u32),
    /// Put the first cube here, still.
    PlaceCube(Vec3),
    /// Send a live pellet (the first) from here at this velocity.
    SendPellet(Vec3, Vec3),
    /// Hang at the `k`th of `of` of the map's lights, spread through its list (they're inside
    /// its rooms), a little under it, still.
    AtLight(usize, usize),
    /// Stand off the `n`th cube (1.6 m along +X, or -X if that's in a wall), looking at it.
    ViewCube(usize),
    /// Switch every rocket turret on.
    RocketsOn,
    /// Retire every turret (so she can stand in their sight).
    TurretsOff,
    /// Do something in the level editor.
    Edit(fn(&mut crate::editor::Editor, &mut crate::menu::Menu)),
    /// Work the mouse, as if by hand.
    Mouse(Hand),
    /// Ray-traced lighting on or off (the setting, not saved).
    RayTracing(bool),
    /// Change the settings.
    Set(fn(&mut crate::settings::Settings)),
    /// Load one of Portal's chambers (the game's copy of the map).
    Chamber(crate::LevelGroup, &'static str),
    /// Stand off the `n`th of the map's effects of a kind (`camera`, `spotlight`, `steam`,
    /// `sparks`, `beam`, or a particle effect's name), `d` metres from it, looking at it
    /// (from beside it, or a little below one that's high up).
    ViewArt(&'static str, usize, f32),
    /// Send each of the map's entities of a class an input (and parameter).
    Send(&'static str, &'static str, &'static str),
}

/// What the mouse does.
#[derive(Clone, Copy)]
enum Hand {
    /// Go to the middle of the editor's content browser tile with this name.
    ToTile(&'static str),
    /// Go to the `n`th prop tile showing.
    ToProp(usize),
    /// Go to the selection's end handle: an arrow (0-2), or its ring (3).
    ToEnd(usize),
    /// Go here, as a fraction of the window's size.
    To(f32, f32),
    Press,
    Release,
    /// Roll the wheel.
    Scroll(f32),
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
        // A cube held half through a wall portal: the half that's through shows at the
        // other portal (and through the first one), not the cube's inside.
        "cubethrough" => vec![
            Map(0),
            Wait(60),
            Teleport(Vec3::new(18.0, 0.0, 19.6)),
            Look(0.0, -40.0),
            Wait(60),
            Use,
            Wait(30),
            Portal(PortalColor::Blue, Vec3::new(24.0, 1.2, 18.0), Vec3::NEG_X, Vec3::Y),
            Portal(PortalColor::Orange, Vec3::new(-24.0, 1.2, 14.0), Vec3::X, Vec3::Y),
            Teleport(Vec3::new(21.0, 0.0, 18.0)),
            Look(-90.0, -10.0),
            Wait(40),
            Film("c", MoveInput { move_axis: Vec2::new(0.0, 0.35), ..default() }, 56),
            Look(-75.0, -10.0),
            Wait(3),
            Shot("c_side"),
            Look(-105.0, -10.0),
            Wait(3),
            Shot("c_other_side"),
        ],
        // Lighting: held still (the traced light settles), moving, turned round; indoors and
        // out.
        "lighting" => vec![
            Map(0),
            Wait(60),
            Teleport(Vec3::new(0.0, 0.0, 10.0)),
            Look(0.0, 5.0),
            Wait(150),
            Shot("l01_still"),
            Hold(fwd(), 30),
            Shot("l02_moving"),
            Look(150.0, 10.0),
            Wait(3),
            Shot("l03_turned"),
            Wait(150),
            Shot("l04_turned_settled"),
            Look(150.0, -70.0),
            Wait(60),
            Shot("l04b_looking_down"),
            Map(1),
            Wait(150),
            Shot("l05_chamber01"),
            Map(5),
            Wait(150),
            Shot("l06_outdoor"),
            Look(160.0, 10.0),
            Wait(150),
            Shot("l07_outdoor_back"),
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
        // Landing far faster than she can run: the skid.
        "skid" => vec![
            Map(0),
            Wait(60),
            Teleport(Vec3::new(-4.0, 0.6, 16.0)),
            Look(90.0, -4.0),
            Wait(2),
            Launch(Vec3::new(-18.0, 1.0, 0.0)),
            Hold(fwd(), 14),
            Shot("k1"),
            Hold(fwd(), 5),
            Shot("k2"),
            Look(90.0, -50.0),
            Wait(1),
            Shot("k2_down"),
            Look(90.0, -4.0),
            Hold(fwd(), 5),
            Shot("k3"),
            Hold(fwd(), 5),
            Shot("k4"),
            Hold(fwd(), 8),
            Shot("k5"),
            Hold(fwd(), 12),
            Shot("k6"),
        ],
        // Each of Portal's models up close, in the Proving Grounds.
        "props" => vec![
            Map(0),
            Wait(90),
            Teleport(Vec3::new(-16.0, 0.0, 4.0)),
            Look(180.0, -30.0),
            Wait(20),
            Shot("p01_button"),
            Teleport(Vec3::new(-11.5, 0.0, 4.5)),
            Look(180.0, -32.0),
            Wait(20),
            Shot("p01b_cube_button"),
            Teleport(Vec3::new(-19.5, 0.0, 10.5)),
            Look(90.0, 0.0),
            Wait(20),
            Shot("p02_door"),
            Teleport(Vec3::new(-16.0, 0.0, 13.0)),
            Look(180.0, 40.0),
            Wait(20),
            Shot("p03_dropper"),
            Teleport(Vec3::new(-14.0, 0.0, 0.5)),
            Look(0.0, -35.0),
            Wait(20),
            Shot("p04_plate"),
            Teleport(Vec3::new(-5.0, 0.0, -4.8)),
            Look(0.0, -20.0),
            Wait(20),
            Shot("p05_pedestal"),
            // Off to the side of the beam (standing in it burns).
            Teleport(Vec3::new(21.5, 0.0, 6.2)),
            Look(-48.0, -22.0),
            Wait(20),
            Shot("p06_laser"),
            Teleport(Vec3::new(17.0, 0.0, 9.5)),
            Look(180.0, -20.0),
            Wait(20),
            Shot("p07_catcher"),
            Teleport(Vec3::new(14.5, 0.0, 8.0)),
            Look(-90.0, -15.0),
            Wait(20),
            Shot("p08_relay"),
            Teleport(Vec3::new(14.0, 0.0, -10.0)),
            Look(0.0, -5.0),
            Wait(20),
            Shot("p09_bridge"),
            Map(1),
            Wait(60),
            Shot("p10_chamber01"),
            Teleport(Vec3::new(0.0, 0.0, -20.5)),
            Look(0.0, -5.0),
            Wait(20),
            Shot("p11_fizzler"),
            Map(3),
            Wait(60),
            Teleport(Vec3::new(-6.0, 0.0, -4.0)),
            Look(115.0, -18.0),
            Wait(20),
            Shot("p12_bridge_emitter"),
        ],
        // Close-ups of Portal with RTX's remastered pieces (#17), under the ray-traced light.
        "rtx" => vec![
            Map(0),
            Wait(240),
            // The storage cube the dropper let go of.
            Teleport(Vec3::new(-16.0, 0.0, 15.2)),
            Look(180.0, -38.0),
            Wait(30),
            Shot("r01_storage_cube"),
            Teleport(Vec3::new(-5.0, 0.0, -5.6)),
            Look(0.0, -25.0),
            Wait(30),
            Shot("r02_pedestal"),
            // The floor button, up, then held down by the cube.
            Teleport(Vec3::new(-16.0, 0.0, 5.4)),
            Look(180.0, -35.0),
            Wait(30),
            Shot("r03_floor_button"),
            // Standing on its edge (still pressing it), looking across the pad.
            Teleport(Vec3::new(-16.0, 0.0, 8.75)),
            Look(0.0, -45.0),
            Wait(30),
            Shot("r04_floor_button_pressed"),
            // The door, shut, then open while she stands on its button.
            Teleport(Vec3::new(-19.5, 0.0, 10.5)),
            Look(90.0, 0.0),
            Wait(30),
            Shot("r05_door"),
            Teleport(Vec3::new(-16.0, 0.0, 8.3)),
            Look(90.0, -5.0),
            Wait(90),
            Shot("r06_door_open"),
            // The dropper's iris from below.
            Teleport(Vec3::new(-16.0, 0.0, 19.5)),
            Look(0.0, 55.0),
            Wait(20),
            Shot("r07_dropper"),
            // Chamber 01's grill.
            Map(1),
            Wait(60),
            Teleport(Vec3::new(0.0, 0.0, -20.5)),
            Look(0.0, -5.0),
            Wait(30),
            Shot("r08_fizzler"),
        ],
        // The faith plate at rest from two sides, then (from beside it, after it's thrown her)
        // its paddle snapping up, holding, and settling back into its pit.
        // Goo (#18): Chamber 03's pit, from its edge and looking along it, then the
        // Proving Grounds' pit.
        "goo" => vec![
            Map(3),
            Wait(240),
            Teleport(Vec3::new(-4.0, 0.0, -4.5)),
            Look(-20.0, -30.0),
            Wait(60),
            Shot("goo_a_edge"),
            Teleport(Vec3::new(8.0, 0.0, -4.0)),
            Look(30.0, -12.0),
            Wait(60),
            Shot("goo_b_along"),
            Map(0),
            Wait(240),
            Teleport(Vec3::new(14.0, 0.0, -10.5)),
            Look(0.0, -35.0),
            Wait(60),
            Shot("goo_c_arena"),
        ],
        // Observation windows (#19): Chamber 01's, then Chamber 03's over the goo.
        "glass" => vec![
            Map(1),
            Wait(240),
            Teleport(Vec3::new(1.0, 0.0, -10.0)),
            Look(-90.0, 28.0),
            Wait(60),
            Shot("glass_a_c01"),
            Map(3),
            Wait(240),
            Teleport(Vec3::new(0.0, 0.0, -24.0)),
            Look(10.0, 30.0),
            Wait(60),
            Shot("glass_b_c03_below"),
            Teleport(Vec3::new(2.0, 0.0, 3.0)),
            Look(5.0, 8.0),
            Wait(60),
            Shot("glass_c_c03_across"),
        ],
        // Portal's own maps (#20): each one's start, four ways round. PORTAL_FIRST_MAP and
        // PORTAL_LAST_MAP pick which (indexes into the map list; Portal's come after the game's).
        "p1" => {
            let first: usize = std::env::var("PORTAL_FIRST_MAP").ok().and_then(|v| v.parse().ok()).unwrap_or(aperture::levels::all().len());
            let last: usize = std::env::var("PORTAL_LAST_MAP").ok().and_then(|v| v.parse().ok()).unwrap_or(first);
            let mut v = Vec::new();
            for i in first..=last {
                v.extend([Map(i), Wait(300)]);
                for (k, yaw) in [0.0f32, 90.0, 180.0, 270.0].into_iter().enumerate() {
                    v.extend([Look(yaw, 0.0), Wait(45), Shot(Box::leak(format!("p1_{i:02}_{k}").into_boxed_str()))]);
                }
                // PORTAL_TP="x,y,z,yaw,pitch;...": more places to look from.
                for (k, p) in std::env::var("PORTAL_TP").unwrap_or_default().split(';').enumerate() {
                    let f: Vec<f32> = p.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    if f.len() == 5 {
                        v.extend([Teleport(Vec3::new(f[0], f[1], f[2])), Look(f[3], f[4]), Wait(60), Shot(Box::leak(format!("p1_{i:02}_tp{k}").into_boxed_str()))]);
                    }
                }
            }
            v
        }
        // Portal's stairs walk (#39): Chamber 02's steel stair (testchmb_a_01), down and back
        // up. PORTAL_FIRST_MAP picks the map index (Portal's maps come after the game's).
        "stairs" => {
            let first: usize = std::env::var("PORTAL_FIRST_MAP").ok().and_then(|v| v.parse().ok()).unwrap_or(aperture::levels::all().len() + 1);
            vec![
                Map(first),
                Wait(200),
                Teleport(Vec3::new(-22.7, 14.64, 20.0)),
                Look(180.0, -25.0),
                Wait(30),
                Shot("stairs_a_top"),
                Hold(fwd(), 40),
                Shot("stairs_b_halfway"),
                Hold(fwd(), 50),
                Shot("stairs_c_bottom"),
                Look(0.0, 10.0),
                Wait(10),
                Shot("stairs_d_looking_up"),
                Hold(fwd(), 45),
                Shot("stairs_e_climbing"),
                Hold(fwd(), 60),
                Shot("stairs_f_top"),
            ]
        }
        // Portal's Chamber 00 (testchmb_a_00), its signs as the logic drives them: the number
        // sign flickering on once GLaDOS has dimmed the lights (#35), the vault's clock, and the
        // door state panel, X until a cube's on its button, then the check (#30).
        "signs" => {
            let first = aperture::levels::all().len();
            vec![
                Map(first),
                Wait(60),
                Look(-152.0, 33.0),
                Wait(5),
                Shot("signs_a_number_off"),
                // (PORTAL_SIGNS_QUICK skips the minute GLaDOS takes, for the panel alone.)
                Wait(if std::env::var("PORTAL_SIGNS_QUICK").is_ok() { 1 } else { 3780 }),
                Shot("signs_b_number_on"),
                Look(180.0, 26.0),
                Wait(5),
                Shot("signs_c_clock"),
                Teleport(Vec3::new(24.28, 3.7, 18.5)),
                Look(180.0, 18.0),
                Wait(30),
                Shot("signs_d_panel_x"),
                // The indicator strip on the floor, from the button to the door (#44).
                Look(180.0, -40.0),
                Wait(5),
                Shot("signs_d_strip_off"),
                Look(180.0, 18.0),
                PlaceCube(Vec3::new(23.58, 4.4, 13.0)),
                Wait(150),
                Shot("signs_e_panel_check"),
                Look(180.0, -40.0),
                Wait(5),
                Shot("signs_e_strip_on"),
            ]
        }
        // A swinging door (#31): Chamber 18's (testchmb_a_14) pellet launcher turns its hatch
        // as it fires, seen from the start platform.
        "swing" => {
            let map = aperture::levels::all().len() + 13;
            let mut v = vec![Map(map), Wait(200), Teleport(Vec3::new(-28.45, 29.3, -37.39)), Wait(2)];
            // (In the air over the goo, beside the hatch: put back before each shot.)
            for k in 0..10 {
                v.extend([Teleport(Vec3::new(-10.5, 30.6, -40.5)), Launch(Vec3::ZERO), Look(42.0, 8.0), Wait(1), Shot(Box::leak(format!("swing_{k}").into_boxed_str())), Wait(10)]);
            }
            v
        }
        // Chamber 02's (testchmb_a_01) portal gun on its turning pedestal: it fires, turns a
        // quarter round, fires again and turns back.
        "pedestal" => {
            let map = aperture::levels::all().len() + 1;
            let mut v = vec![Map(map), Wait(100), Teleport(Vec3::new(-4.06, -3.25, 0.4)), Look(0.0, -12.0), Wait(10)];
            for k in 0..8 {
                v.extend([Shot(Box::leak(format!("pedestal_{k}").into_boxed_str())), Wait(40)]);
            }
            // Up to it: hers. Then a shot of her own.
            v.extend([Teleport(Vec3::new(-3.5, -3.25, -1.9)), Look(0.0, -12.0), Wait(20), Shot("pedestal_taken"), Look(90.0, 0.0), Wait(5), Fire(PortalColor::Blue), Wait(20), Shot("pedestal_fired")]);
            v
        }
        // Inside Chamber 00's exit elevator (testchmb_a_00), doors open, closing, and on the
        // ride up (#34): the cabin should stay lit.
        "elevator" => {
            let map = aperture::levels::all().len();
            let mut v = vec![Map(map), Wait(100), Teleport(Vec3::new(6.5, 15.1, 9.75)), Look(90.0, -5.0), Wait(30), Shot("elevator_a_in")];
            for k in 0..6 {
                v.extend([Wait(150), Shot(Box::leak(format!("elevator_b{k}").into_boxed_str()))]);
                if k == 2 {
                    // Its roof, and the lamp under it.
                    v.extend([Look(-90.0, 55.0), Wait(3), Shot("elevator_c_up"), Look(90.0, -5.0)]);
                }
            }
            v
        }
        // Chamber 00's exit elevator doors (testchmb_a_00, #58): opened as she comes up to
        // them, then closing behind her once she's in, once, frame by frame.
        "elevatordoors" => {
            let map = aperture::levels::all().len();
            let mut v = vec![Map(map), Wait(100), Teleport(Vec3::new(6.5, 15.1, 19.0)), Look(0.0, -5.0), Wait(120), Shot("elevatordoors_a_open"), Teleport(Vec3::new(6.5, 15.1, 9.3)), Look(180.0, -5.0)];
            for k in 0..16 {
                v.extend([Wait(6), Shot(Box::leak(format!("elevatordoors_b{k:02}").into_boxed_str()))]);
            }
            v
        }
        // Chamber 13's exit elevator (testchmb_a_08, #59): its car is the stats-screen one
        // (prop_portal_stats_display), the round elevator's body.
        "car13" => {
            let map = aperture::levels::all().len() + 8;
            let mut v = vec![Map(map), Wait(150), Teleport(Vec3::new(0.0, 3.8, -26.4)), Launch(Vec3::ZERO)];
            for (k, yaw) in [0.0, 90.0, 180.0, 270.0].into_iter().enumerate() {
                v.extend([Look(yaw, 5.0), Wait(20), Shot(Box::leak(format!("car13_{k}").into_boxed_str()))]);
            }
            v.extend([Teleport(Vec3::new(0.0, 3.8, -21.0)), Look(180.0, 0.0), Wait(20), Shot("car13_outside")]);
            v
        }
        // Chamber 15's tall emancipation grills (testchmb_a_10, #60): their emitter posts, of
        // Portal's size, stacked up each side.
        "grills15" => {
            let map = aperture::levels::all().len() + 10;
            let mut v = vec![Map(map), Wait(150)];
            let views: [(&str, Vec3, f32, f32); 4] = [
                ("grills15_a_section2", Vec3::new(54.4, -9.7, 8.0), 0.0, 12.0),
                ("grills15_b_section2_west", Vec3::new(50.5, -9.7, 3.0), 35.0, 15.0),
                ("grills15_c_section2_east", Vec3::new(58.5, -9.7, 3.0), -35.0, 15.0),
                ("grills15_d_middle", Vec3::new(16.2, -3.2, 9.0), 0.0, 20.0),
            ];
            for (name, at, yaw, pitch) in views {
                v.extend([Teleport(at), Launch(Vec3::ZERO), Look(yaw, pitch), Wait(30), Shot(name)]);
            }
            v
        }
        // Pellet pieces in Portal's maps up close, the camera held where it's put:
        // PORTAL_FIRST_MAP and PORTAL_TP ("x,y,z,yaw,pitch;...") as for `p1`, PORTAL_WAIT frames
        // into the map first (default 200).
        // Does light go through portals, ray traced? Chamber 08's pellet runs along its west
        // wall: orange on that wall beside it, blue across the room on the east wall, and a
        // spectator camera looking at blue, a shot every 8 frames as the pellet passes
        // orange. PORTAL_NOPORTALS=1: the same without them, to tell the difference.
        "portallight" => {
            use crate::LevelGroup::Portal as N;
            let mut v = vec![Chamber(N, "testchmb_a_04"), Wait(90)];
            if std::env::var_os("PORTAL_NOPORTALS").is_none() {
                v.push(Portal(PortalColor::Orange, Vec3::new(-9.75, 1.56, -6.0), Vec3::X, Vec3::Y));
                v.push(Portal(PortalColor::Blue, Vec3::new(3.04, 1.56, -6.0), Vec3::NEG_X, Vec3::Y));
            }
            v.push(Spectate(Vec3::new(-1.5, 1.8, -6.0), -90.0, 0.0));
            for k in 0..16 {
                v.extend([Wait(8), Shot(Box::leak(format!("light_{k:02}").into_boxed_str()))]);
            }
            v
        }
        // Light through portals, as Portal with RTX's screenshot of Chamber 08 has it: the
        // map's own orange on the west wall with the pellet passing in front of it, blue on the
        // way in's west wall, and a spectator camera by the entrance elevator looking across
        // the floor at blue. A shot with the pellet far off, then a shot every 6 frames as it
        // passes orange. PORTAL_NOPORTALS=1: the same without the portals; PORTAL_PELLET_GLOW=n
        // makes the pellet n times as bright.
        "portallight08" => {
            use crate::LevelGroup::Portal as N;
            let orange = Vec3::new(-9.648, 1.434, 1.644);
            // (Auto exposure off: one frame's brightness against another's is the light's.)
            let mut v = vec![Set(|s| s.auto_exposure = 0.0), Chamber(N, "testchmb_a_04"), Wait(60)];
            // (PORTAL_NODLSS=1: without DLSS, TAA instead: to tell what its history does.)
            if std::env::var_os("PORTAL_NODLSS").is_some() {
                v.insert(0, Set(|s| s.dlss = 0.0));
            }
            if std::env::var_os("PORTAL_NOPORTALS").is_none() {
                v.push(Portal(PortalColor::Orange, orange, Vec3::X, Vec3::Y));
                v.push(Portal(PortalColor::Blue, Vec3::new(-6.5024, 1.45, 18.5), Vec3::X, Vec3::Y));
            }
            v.extend([Spectate(Vec3::new(-0.5, 1.5, 19.3), 70.0, -14.0), WaitPelletNear(Vec3::new(-8.08, 1.56, 12.0), 3.0, 900), Wait(20), Shot("light08_far"), WaitPelletNear(orange, 5.0, 900)]);
            // (PORTAL_PELLET_STILL=1: the pellet held still in front of orange, and shots of
            // the light settling.)
            let still = std::env::var_os("PORTAL_PELLET_STILL").is_some();
            for k in 0..10 {
                if still {
                    v.push(SendPellet(Vec3::new(-8.08, 1.56, orange.z), Vec3::ZERO));
                }
                v.extend([Wait(if still { 20 } else { 6 }), Shot(Box::leak(format!("light08_{k:02}").into_boxed_str()))]);
            }
            v
        }
        // Lamps' light through portals (#94): orange on the east wall of Chamber 08's way in
        // (lit white by the lamps under its floor), blue where the map's own orange goes, on
        // the goo room's west wall, and a spectator camera on blue from across the goo; shots
        // as the light settles. PORTAL_NOPORTALS=1: the same without the portals.
        "portallamp" => {
            use crate::LevelGroup::Portal as N;
            let mut v = vec![Set(|s| s.auto_exposure = 0.0), Chamber(N, "testchmb_a_04"), Wait(60)];
            if std::env::var_os("PORTAL_NOPORTALS").is_none() {
                v.push(Portal(PortalColor::Orange, Vec3::new(0.0, 1.45, 16.5), Vec3::NEG_X, Vec3::Y));
                v.push(Portal(PortalColor::Blue, Vec3::new(-9.648, 1.434, 1.644), Vec3::X, Vec3::Y));
            }
            v.push(Spectate(Vec3::new(-4.5, 2.1, -1.0), 70.0, -5.0));
            for k in 0..4 {
                v.extend([Wait(60), Shot(Box::leak(format!("lamp_{k}").into_boxed_str()))]);
            }
            v
        }
        // A Mirror's Edge level (#100, PORTAL_ME_LEVEL: its file, `tutorial` unless said):
        // where she starts, looking round, then from above.
        "melevel" => {
            let file: &'static str = Box::leak(std::env::var("PORTAL_ME_LEVEL").unwrap_or_else(|_| "tutorial".into()).into_boxed_str());
            let mut v = vec![Chamber(crate::LevelGroup::MirrorsEdge, file), Wait(120)];
            for (k, yaw) in [0.0, 90.0, 180.0, 270.0].into_iter().enumerate() {
                v.extend([Look(yaw, -5.0), Wait(40), Shot(["me_0", "me_1", "me_2", "me_3"][k])]);
            }
            // (PORTAL_ME_VIEW=x,y,z,yaw,pitch: a spectator's view too.)
            if let Some(f) = std::env::var("PORTAL_ME_VIEW").ok().map(|s| s.split(',').filter_map(|x| x.trim().parse::<f32>().ok()).collect::<Vec<_>>()).filter(|f| f.len() == 5) {
                let wait = std::env::var("PORTAL_ME_WAIT").ok().and_then(|w| w.parse().ok()).unwrap_or(90);
                v.extend([Spectate(Vec3::new(f[0], f[1], f[2]), f[3], f[4]), Wait(wait)]);
            }
            v.extend([Wait(60), Shot("me_4")]);
            v
        }
        // Sunlight through portals (#94): on Rooftops, orange on the start roof facing the sky,
        // its top to the south so the sun going down and north into it comes out of blue going
        // down; blue low on the north face of the roof's back wall, over the floor that wall
        // shades, so what lights that floor came through the portals. A spectator camera up the
        // roof looking back at the wall. PORTAL_NOPORTALS=1: the same without the portals.
        "portalsun" => {
            let mut v = vec![Set(|s| s.auto_exposure = 0.0), Map(7), Wait(90)];
            if std::env::var_os("PORTAL_NOPORTALS").is_none() {
                v.push(Portal(PortalColor::Orange, Vec3::new(0.0, 0.0, -2.0), Vec3::Y, Vec3::Z));
                v.push(Portal(PortalColor::Blue, Vec3::new(-1.5, 1.1, 5.5), Vec3::NEG_Z, Vec3::Y));
            }
            v.push(Spectate(Vec3::new(1.0, 4.5, -9.0), 180.0, -22.0));
            for k in 0..4 {
                v.extend([Wait(60), Shot(Box::leak(format!("sun_{k}").into_boxed_str()))]);
            }
            v
        }
        // The spectator camera (#90): over Chamber 08's goo, where she'd drown, for as long as
        // it likes, following the pellet as it bounces; then back to her, where she stood.
        "spectate" => {
            use crate::LevelGroup::Portal as N;
            let mut v = vec![Chamber(N, "testchmb_a_04"), Wait(90), Spectate(Vec3::new(-4.0, 1.2, 2.0), 90.0, -10.0), TrackPellet];
            for k in 0..8 {
                v.extend([Wait(25), Shot(["spectate_0", "spectate_1", "spectate_2", "spectate_3", "spectate_4", "spectate_5", "spectate_6", "spectate_7"][k])]);
            }
            v.extend([Unspectate, Wait(10), Shot("spectate_back")]);
            v
        }
        "views" => {
            // (Her eye's height standing.)
            let t = faith_move::Tuning::default();
            let eye = t.stand_height - t.eye_from_top;
            let map: usize = std::env::var("PORTAL_FIRST_MAP").ok().and_then(|v| v.parse().ok()).unwrap_or(aperture::levels::all().len());
            let wait: u32 = std::env::var("PORTAL_WAIT").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
            // (PORTAL_HOLD: frames held at each view before its shot, for the traced light to
            // settle; PORTAL_SHOTS: shots of each, that many frames apart.)
            let hold: u32 = std::env::var("PORTAL_HOLD").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
            let shots: u32 = std::env::var("PORTAL_SHOTS").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
            // (Each view from a spectator camera, its eye where hers would be standing there:
            // nothing falls or dies. PORTAL_PLACE=1 puts her there instead. PORTAL_TRACK=pellet
            // keeps the view on the first live pellet.)
            let place = std::env::var_os("PORTAL_PLACE").is_some();
            let track = std::env::var("PORTAL_TRACK").is_ok_and(|t| t == "pellet");
            let mut v = vec![Map(map), Wait(wait)];
            for (k, p) in std::env::var("PORTAL_TP").unwrap_or_default().split(';').enumerate() {
                let f: Vec<f32> = p.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if f.len() == 5 {
                    let at = Vec3::new(f[0], f[1], f[2]);
                    if place {
                        v.extend([Teleport(at), Launch(Vec3::ZERO), Look(f[3], f[4])]);
                    } else {
                        v.push(Spectate(at + Vec3::Y * eye, f[3], f[4]));
                        if track {
                            v.push(TrackPellet);
                        }
                    }
                    for j in 0..shots {
                        let name = if shots == 1 { format!("view_{k}") } else { format!("view_{k}_{j}") };
                        v.extend([Wait(hold), Shot(Box::leak(name.into_boxed_str()))]);
                    }
                }
            }
            v
        }
        // Chamber 15's section 2 (testchmb_a_10, #61), as Portal's walkthrough (and the play
        // harness) has it: the pellet out across the room off the plate in front of its
        // launcher; orange over the lift, blue where it bounces, and it comes back through the
        // grill on a new line; past the grill, blue where it bounces on the far wall and orange
        // on the ceiling over the receptacle, and down into the receptacle it goes.
        "pellet15" => {
            let map = aperture::levels::all().len() + 10;
            let mut v = vec![Map(map), Wait(120), Teleport(Vec3::new(54.0, -9.75, 2.0)), Launch(Vec3::ZERO), Look(-15.0, 22.0), Wait(45), Shot("pellet15_a_out_across")];
            v.extend([
                Portal(PortalColor::Orange, Vec3::new(58.06, -5.06, 6.5), Vec3::NEG_Z, Vec3::Y),
                Portal(PortalColor::Blue, Vec3::new(50.54, -5.02, 6.5), Vec3::NEG_Z, Vec3::Y),
                Look(165.0, 15.0),
                Wait(40),
                Shot("pellet15_b_portals"),
                Wait(150),
                Teleport(Vec3::new(59.5, -9.75, -5.0)),
                Launch(Vec3::ZERO),
                Portal(PortalColor::Blue, Vec3::new(58.06, -5.03, -12.8), Vec3::Z, Vec3::Y),
                Portal(PortalColor::Orange, Vec3::new(58.37, -3.25, -8.06), Vec3::NEG_Y, Vec3::NEG_Z),
                Look(20.0, 10.0),
                Wait(20),
                Shot("pellet15_c_far_side"),
            ]);
            for k in 0..4 {
                v.extend([Wait(45), Shot(Box::leak(format!("pellet15_d{k}").into_boxed_str()))]);
            }
            v
        }
        // Chamber 15's moving platforms (testchmb_a_10): a pellet into section 3's catcher
        // starts them, and rail carts come along the track one after another.
        "rails15" => {
            let map = aperture::levels::all().len() + 10;
            // (The camera held over the pit the carts cross, looking along their track.)
            let look = |v: &mut Vec<Act>, name: &'static str| v.extend([Teleport(Vec3::new(-1.6, -3.0, 3.0)), Launch(Vec3::ZERO), Look(30.0, -45.0), Wait(2), Shot(name)]);
            let mut v = vec![Map(map), Wait(60)];
            look(&mut v, "rails15_a_before");
            v.push(SendPellet(Vec3::new(17.86, -1.86, 15.6), Vec3::new(0.0, 0.0, 6.0)));
            for k in 0..8 {
                v.push(Wait(240));
                look(&mut v, Box::leak(format!("rails15_b{k}").into_boxed_str()));
            }
            v
        }
        // Chamber 09's dropper (testchmb_a_05, #54/#55): shut on the next cube once it's
        // dropped the first; the first into the grill, and it opens, drops the next and shuts.
        "dropper" => {
            let map = aperture::levels::all().len() + 5;
            let look = |v: &mut Vec<Act>, name: &'static str| v.extend([Teleport(Vec3::new(-0.8, 0.0, 9.0)), Launch(Vec3::ZERO), Look(0.0, 32.0), Wait(2), Shot(name)]);
            let mut v = vec![Map(map), Wait(600)];
            look(&mut v, "dropper_a_shut");
            v.extend([PlaceCube(Vec3::new(-3.6, 1.0, -1.63)), Wait(150)]);
            look(&mut v, "dropper_b_open");
            v.push(Wait(30));
            look(&mut v, "dropper_c_dropping");
            v.push(Wait(240));
            look(&mut v, "dropper_d_shut_again");
            v
        }
        // Chamber 06's catcher (testchmb_a_03, in the floor where its maker builds it): empty
        // and open, then a pellet sent in: it closes on the pellet, which stays glowing in it
        // (#23).
        "catcher" => {
            let map = aperture::levels::all().len() + 3;
            let side = |v: &mut Vec<Act>, name: &'static str| v.extend([Teleport(Vec3::new(0.2, -3.66, 2.6)), Launch(Vec3::ZERO), Look(-50.0, -30.0), Wait(2), Shot(name)]);
            let mut v = vec![Map(map), Wait(200)];
            side(&mut v, "catcher_a_empty");
            v.push(SendPellet(Vec3::new(3.24, -1.2, 0.01), Vec3::new(0.0, -6.0, 0.0)));
            v.push(Wait(30));
            side(&mut v, "catcher_b_caught");
            v.push(Wait(90));
            side(&mut v, "catcher_c_closing");
            v.push(Wait(150));
            side(&mut v, "catcher_d_closed");
            v
        }
        // The escape's rocket turret (escape_01, #24), switched on, seen from beyond the glass
        // in front of it: its sight on her, the rocket, the glass going.
        "rockets" => {
            let map = aperture::levels::all().len() + 16;
            let mut v = vec![Map(map), Wait(150), Teleport(Vec3::new(-14.74, -6.0, -0.6)), Look(180.0, 0.0), Wait(30), Shot("rocket_a_off"), RocketsOn, Wait(20), Shot("rocket_b_sight")];
            for k in 0..48 {
                v.extend([Wait(5), Shot(Box::leak(format!("rocket_c{k:02}").into_boxed_str()))]);
            }
            v
        }
        // Portals on tilted faces (#47): Chamber 16's diagonal wall (testchmb_a_11), a portal
        // on it, a floor portal behind her, and through; then Chamber 18's 45° face
        // (testchmb_a_14), from in front of it.
        "tilted" => {
            let map = aperture::levels::all().len();
            let mut v = vec![Map(map + 11), Wait(150), TurretsOff, Teleport(Vec3::new(-4.38, 0.0, -19.0)), Look(45.0, 0.0), Wait(10), Fire(PortalColor::Blue), Wait(30), Shot("tilted_a_wall")];
            v.extend([Look(225.0, -70.0), Wait(5), Fire(PortalColor::Orange), Wait(30), Look(45.0, 0.0), Wait(10), Shot("tilted_b_linked")]);
            v.extend([Hold(fwd(), 80), Wait(10), Shot("tilted_c_out"), Look(225.0, -30.0), Wait(5), Shot("tilted_d_behind")]);
            v.extend([Map(map + 13), Wait(150), TurretsOff, Teleport(Vec3::new(-17.0, 79.4, 30.9)), Launch(Vec3::ZERO), Look(90.0, -45.0), Fire(PortalColor::Blue), Wait(1), Shot("tilted_e_ramp")]);
            v
        }
        // Chamber 14's fling (testchmb_a_09, #21): blue high on the far wall over the cube's
        // shelf, orange on the sunken stairs (on the nodraw brush over them); off the ledge
        // into orange and out over the shelf.
        "fling14" => {
            let map = aperture::levels::all().len() + 9;
            let jump = MoveInput { move_axis: Vec2::new(0.0, 1.0), jump_pressed: true, jump_held: true, ..default() };
            let mut v = vec![Map(map), Wait(200), TurretsOff];
            v.extend([
                Portal(PortalColor::Blue, Vec3::new(32.512, 14.607, -27.416), Vec3::NEG_X, Vec3::Y),
                Portal(PortalColor::Orange, Vec3::new(11.2, -3.2258, -13.8), Vec3::Y, Vec3::NEG_X),
                Teleport(Vec3::new(21.5, 6.6, -13.8)),
                Launch(Vec3::ZERO),
                Look(90.0, -25.0),
                Wait(20),
                Shot("fling14_a_edge"),
                // (Walking up to the drop she'd stop and look over it, as in Mirror's Edge: a
                // run and a jump takes her off.)
                Hold(fwd(), 30),
                Hold(jump, 3),
            ]);
            for k in 0..20 {
                v.extend([Hold(fwd(), 5), Shot(Box::leak(format!("fling14_b{k:02}").into_boxed_str()))]);
            }
            v.extend([Wait(60), Look(90.0, 0.0), Wait(5), Shot("fling14_c_shelf")]);
            v
        }
        // Chamber 10's last room (testchmb_a_06, #21): in, the pillars slide out over the pit
        // and orange opens on its floor; blue on the lower pillar's end; dropped into orange,
        // out over the pit onto the first ledge.
        "fling10" => {
            let map = aperture::levels::all().len() + 6;
            let mut v = vec![Map(map), Wait(200), TurretsOff, Teleport(Vec3::new(-3.25, 3.4, -6.0)), Launch(Vec3::ZERO), Wait(30), Teleport(Vec3::new(-3.25, 3.4, -12.5)), Look(0.0, 25.0), Wait(420), Shot("fling10_a_pillars")];
            v.extend([
                Portal(PortalColor::Blue, Vec3::new(-3.25, 11.4, -8.128), Vec3::NEG_Z, Vec3::Y),
                Wait(10),
                Shot("fling10_b_portal"),
                Teleport(Vec3::new(-3.24, 2.0, -15.0)),
                Launch(Vec3::new(0.0, -6.0, 0.0)),
                Look(0.0, -70.0),
            ]);
            for k in 0..16 {
                v.extend([Wait(5), Shot(Box::leak(format!("fling10_c{k:02}").into_boxed_str()))]);
            }
            v.extend([Wait(60), Look(180.0, 10.0), Wait(5), Shot("fling10_d_ledge")]);
            v
        }
        // Chamber 16's turret 3 (testchmb_a_11, #21): orange on the floor near her, blue opened
        // on the floor under the turret: it drops through and topples out beside orange.
        // Chamber 16's training turret (testchmb_a_11, #63/#65/#66): in front of it, searching
        // (laser sweeping), spotting her (wings out), firing, and its laser following her as she
        // steps aside.
        "turretfire" => {
            let map = aperture::levels::all().len() + 11;
            vec![
                Map(map),
                Wait(200),
                Teleport(Vec3::new(17.07, 0.05, -5.5)),
                Launch(Vec3::ZERO),
                Look(0.0, -6.0),
                Wait(3),
                Shot("turretfire_a_searching"),
                Wait(36),
                Shot("turretfire_b_deploying"),
                Wait(20),
                Shot("turretfire_c_firing"),
                Teleport(Vec3::new(18.6, 0.05, -6.2)),
                Launch(Vec3::ZERO),
                Look(18.0, -6.0),
                Wait(14),
                Shot("turretfire_d_tracking"),
            ]
        }
        // Picking Chamber 16's training turret up from behind (#62), carrying it, setting it
        // down.
        "turretcarry" => {
            let map = aperture::levels::all().len() + 11;
            vec![
                Map(map),
                Wait(200),
                Teleport(Vec3::new(17.07, 0.05, -12.0)),
                Launch(Vec3::ZERO),
                Look(180.0, -30.0),
                Wait(20),
                Shot("turretcarry_a_behind"),
                Use,
                Wait(20),
                Look(90.0, -15.0),
                Wait(30),
                Shot("turretcarry_b_carried"),
                Use,
                Wait(30),
                Look(90.0, -25.0),
                Wait(5),
                Shot("turretcarry_c_put_down"),
            ]
        }
        // Chamber 16's training turret knocked over (running into it from behind), seen lying
        // on the floor from the side.
        "turrettipped" => {
            let map = aperture::levels::all().len() + 11;
            vec![
                Map(map),
                Wait(200),
                Teleport(Vec3::new(17.07, 0.05, -15.0)),
                Launch(Vec3::ZERO),
                Look(180.0, 0.0),
                Wait(5),
                Hold(MoveInput { move_axis: Vec2::new(0.0, 1.0), ..default() }, 70),
                Wait(240),
                Teleport(Vec3::new(20.0, 0.05, -9.8)),
                Launch(Vec3::ZERO),
                Look(90.0, -18.0),
                Wait(5),
                Shot("turrettipped_side"),
            ]
        }
        "turret16" => {
            let map = aperture::levels::all().len() + 11;
            let mut v = vec![Map(map), Wait(200), TurretsOff, Teleport(Vec3::new(-2.4, 0.1, -8.0)), Launch(Vec3::ZERO), Look(0.0, -18.0), Wait(30)];
            v.extend([
                Portal(PortalColor::Orange, Vec3::new(-2.4, 0.0, -13.0), Vec3::Y, Vec3::NEG_Z),
                Wait(5),
                Shot("turret16_a_before"),
                Portal(PortalColor::Blue, Vec3::new(9.75, 0.0, -11.98), Vec3::Y, Vec3::X),
                Wait(3),
                Shot("turret16_b_dropped"),
                Wait(40),
                Shot("turret16_c_lying"),
            ]);
            v
        }
        // Chamber 16 (testchmb_a_11): the cage lights in Rattmann's den hanging from their
        // cables (#67), the radio, mug and tools on their cube (#72), and the stacked cubes
        // against the den's panel (#68).
        "den16" => {
            let map = aperture::levels::all().len() + 11;
            vec![
                Map(map),
                Wait(200),
                TurretsOff,
                Teleport(Vec3::new(-21.1, 0.1, 13.2)),
                Launch(Vec3::ZERO),
                Look(-114.0, 40.0),
                Wait(30),
                Shot("den16_a_lights"),
                Look(150.0, 25.0),
                Wait(10),
                Shot("den16_b_lights_back"),
                Teleport(Vec3::new(2.44, 0.1, -8.6)),
                Launch(Vec3::ZERO),
                Look(180.0, -20.0),
                Wait(30),
                Shot("den16_c_desk_cube"),
                Teleport(Vec3::new(-17.5, 0.1, 12.8)),
                Launch(Vec3::ZERO),
                Look(10.0, -12.0),
                Wait(30),
                Shot("den16_d_panel"),
            ]
        }
        // Chamber 12's wedge (testchmb_a_07, #21): up on the button's platform it swings out of
        // the wall and tips; blue on it, orange on the pit's floor; down into orange and flung
        // out of the wedge up to the exit's ledge.
        "wedge12" => {
            let map = aperture::levels::all().len() + 7;
            let mut v = vec![Map(map), Wait(200), TurretsOff, Teleport(Vec3::new(24.0, 32.9, -5.0)), Launch(Vec3::ZERO), Look(90.0, 15.0), Wait(30), Shot("wedge12_a_closed"), Wait(500), Shot("wedge12_b_tipped")];
            v.extend([
                Portal(PortalColor::Blue, Vec3::new(15.705, 38.697, -4.9), Vec3::new(0.70710677, 0.70710677, 0.0), Vec3::new(-0.70710677, 0.70710677, 0.0)),
                Portal(PortalColor::Orange, Vec3::new(34.297, 13.005, -5.0), Vec3::Y, Vec3::X),
                Wait(10),
                Shot("wedge12_c_portal"),
                // (Dropped straight into orange from the platform's height, as a fall off it
                // would: the run itself depends on the frame rate.)
                Teleport(Vec3::new(34.3, 31.0, -5.0)),
                Launch(Vec3::new(0.0, -4.0, 0.0)),
                Look(-90.0, -60.0),
            ]);
            for k in 0..24 {
                v.extend([Wait(4), Shot(Box::leak(format!("wedge12_d{k:02}").into_boxed_str()))]);
            }
            v.extend([Wait(90), Look(-90.0, 0.0), Wait(5), Shot("wedge12_e_ledge")]);
            v
        }
        // Chamber 01's cycling portals (testchmb_a_00's upper rooms, #21): the cycle started,
        // then the last room's portal on its wall, at Portal's size, from in front of it.
        "chamber01" => {
            let map = aperture::levels::all().len();
            let mut v = vec![Map(map), Wait(100), Teleport(Vec3::new(13.0, 14.7, 27.6)), Wait(20), Teleport(Vec3::new(5.7, 14.7, 30.0)), Look(180.0, 5.0)];
            for k in 0..6 {
                v.extend([Wait(50), Shot(Box::leak(format!("chamber01_{k}").into_boxed_str()))]);
            }
            v
        }
        // The escape's first way on (escape_00, #21): walked into, it takes her to escape_01.
        "escape_exit" => {
            let map = aperture::levels::all().len() + 15;
            vec![Map(map), Wait(150), Shot("escape_exit_a_before"), Teleport(Vec3::new(-55.27, 14.7, -14.63)), Look(90.0, 0.0), Wait(20), Shot("escape_exit_b_after"), Wait(40), Shot("escape_exit_c_after")]
        }
        // Holes in Portal's maps (#27): from a few of each map's lights, four ways round. Run
        // with PORTAL_DEBUG_VOID=1 and anything not drawn shows magenta. PORTAL_FIRST_MAP and
        // PORTAL_LAST_MAP pick the maps.
        "voids" => {
            let first: usize = std::env::var("PORTAL_FIRST_MAP").ok().and_then(|v| v.parse().ok()).unwrap_or(aperture::levels::all().len());
            let last: usize = std::env::var("PORTAL_LAST_MAP").ok().and_then(|v| v.parse().ok()).unwrap_or(first);
            let mut v = Vec::new();
            for i in first..=last {
                v.extend([Map(i), Wait(120)]);
                for k in 0..4 {
                    for (j, yaw) in [0.0f32, 90.0, 180.0, 270.0].into_iter().enumerate() {
                        v.extend([AtLight(k, 4), Look(yaw, -10.0), Wait(3), Shot(Box::leak(format!("void_{i:02}_{k}{j}").into_boxed_str()))]);
                    }
                }
            }
            v
        }
        // Each cube in a Portal map (PORTAL_FIRST_MAP; Chambers 04-05 by default), close up: they
        // should rest on the floor, not in it (#29).
        "cubes" => {
            let map = std::env::var("PORTAL_FIRST_MAP").ok().and_then(|v| v.parse().ok()).unwrap_or(aperture::levels::all().len() + 2);
            let mut v = vec![Map(map), Wait(240)];
            for k in 0..4 {
                v.extend([ViewCube(k), Wait(10), Shot(Box::leak(format!("cube_{k}").into_boxed_str()))]);
            }
            v
        }
        // High energy pellets and the platform they power (#23, #22): Chamber 06, its pellet
        // sent through portals into the catcher.
        "pellets" => vec![
            Map(6),
            Wait(200),
            Teleport(Vec3::new(4.0, 0.0, 4.0)),
            Look(59.0, 15.0),
            Wait(40),
            Shot("pellet_a_launcher"),
            Portal(PortalColor::Blue, Vec3::new(10.0, 6.0, -4.0), Vec3::NEG_X, Vec3::Y),
            Portal(PortalColor::Orange, Vec3::new(10.0, 1.2, -36.0), Vec3::NEG_X, Vec3::Y),
            Teleport(Vec3::new(4.0, 0.0, -6.0)),
            Look(10.0, 0.0),
            Wait(560),
            Shot("pellet_b_caught"),
            Teleport(Vec3::new(0.0, 0.0, -8.0)),
            Look(0.0, -15.0),
            Wait(200),
            Shot("pellet_c_platform"),
            // The catcher, closed on what it caught.
            Teleport(Vec3::new(-6.5, 0.2, -36.0)),
            Launch(Vec3::ZERO),
            Look(90.0, 5.0),
            Wait(2),
            Shot("pellet_d_catcher"),
        ],
        "plate" => vec![
            Map(0),
            Wait(90),
            Teleport(Vec3::new(-14.0, 0.0, -0.5)),
            Look(0.0, -40.0),
            Wait(20),
            Shot("plate_a"),
            Teleport(Vec3::new(-11.0, 0.0, -4.0)),
            Look(90.0, -25.0),
            Wait(20),
            Shot("plate_b"),
            Teleport(Vec3::new(-14.0, 0.0, -4.0)),
            Wait(2),
            Teleport(Vec3::new(-10.5, 0.0, -5.0)),
            Launch(Vec3::ZERO),
            Look(75.0, -20.0),
            Wait(1),
            Shot("plate_c_launch"),
            Wait(12),
            Shot("plate_d_up"),
            Wait(90),
            Shot("plate_e_settling"),
            Wait(120),
            Shot("plate_f_rest"),
            // Chamber 03's, which throws along -X.
            Map(3),
            Wait(60),
            Teleport(Vec3::new(6.0, 0.0, -22.8)),
            Look(85.0, -30.0),
            Wait(20),
            Shot("plate_g_c03"),
        ],
        // Faith's third-person body, seen through portals: from above (a floor portal in
        // front of her, the ceiling portal over her head), then running.
        "body" => vec![
            Map(0),
            Wait(60),
            // Orange on the wall behind her, blue standing in front of her: looking into blue
            // she sees herself from behind.
            Teleport(Vec3::new(-21.5, 0.0, 0.0)),
            Wait(20),
            Portal(PortalColor::Orange, Vec3::new(-24.0, 1.2, 0.0), Vec3::X, Vec3::Y),
            Portal(PortalColor::Blue, Vec3::new(-18.0, 1.2, 0.0), Vec3::NEG_X, Vec3::Y),
            Look(-90.0, -5.0),
            Wait(30),
            Shot("b01_from_behind"),
            Look(-90.0, -5.0),
            Hold(MoveInput { move_axis: Vec2::new(0.0, -1.0), ..default() }, 25),
            Shot("b02_walking_back"),
            Teleport(Vec3::new(-21.5, 0.0, 0.0)),
            Look(-90.0, -5.0),
            Hold(MoveInput { move_axis: Vec2::new(0.0, 1.0), jump_pressed: true, jump_held: true, ..default() }, 1),
            Hold(MoveInput::default(), 12),
            Shot("b03_jumping"),
        ],
        // Running through a wall portal, a frame at a time: no frame should show black.
        "through" => vec![
            Map(0),
            Wait(60),
            Teleport(Vec3::new(-23.0, 0.0, 0.0)),
            Wait(20),
            Portal(PortalColor::Orange, Vec3::new(-24.0, 1.2, 0.0), Vec3::X, Vec3::Y),
            Portal(PortalColor::Blue, Vec3::new(-18.0, 1.2, 0.0), Vec3::NEG_X, Vec3::Y),
            Look(-90.0, 0.0),
            Wait(30),
            Hold(fwd(), 65),
            Film("t", fwd(), 30),
            // Into a wall portal at a sprint, out of the other wall.
            Teleport(Vec3::new(14.0, 0.0, 18.0)),
            Portal(PortalColor::Blue, Vec3::new(24.0, 1.2, 18.0), Vec3::NEG_X, Vec3::Y),
            Portal(PortalColor::Orange, Vec3::new(-24.0, 1.2, 18.0), Vec3::X, Vec3::Y),
            Look(-90.0, 0.0),
            Wait(10),
            Hold(fwd(), 105),
            Film("w", fwd(), 50),
        ],
        // Mirror's Edge's props on Chamber 05's roofs, and its ladder.
        "meprops" => vec![
            Map(5),
            Wait(90),
            Look(20.0, -8.0),
            Wait(10),
            Shot("m01_roof_a"),
            Look(-160.0, -10.0),
            Wait(10),
            Shot("m02_roof_a_back"),
            Teleport(Vec3::new(-3.0, -2.0, -40.5)),
            Look(10.0, 15.0),
            Wait(20),
            Shot("m03_ladder"),
            Teleport(Vec3::new(0.0, -2.0, -36.0)),
            Look(0.0, -5.0),
            Wait(20),
            Shot("m04_roof_b"),
            Teleport(Vec3::new(0.0, -4.0, -74.0)),
            Look(0.0, -5.0),
            Wait(20),
            Shot("m05_roof_c"),
            // Close up, for the textures.
            Teleport(Vec3::new(-3.2, 0.0, 1.0)),
            Look(90.0, -15.0),
            Wait(20),
            Shot("m06_acunit"),
            Teleport(Vec3::new(-3.0, 0.0, -14.0)),
            Look(30.0, -20.0),
            Wait(20),
            Shot("m07_hatch"),
            Teleport(Vec3::new(-2.5, -4.0, -88.0)),
            Look(60.0, 0.0),
            Wait(20),
            Shot("m08_cistern"),
        ],
        "menu" => {
            use crate::menu::Screen;
            use crate::settings::Page;
            vec![
                Map(1),
                Wait(60),
                Shot("hud"),
                Menu(Screen::Title),
                Wait(10),
                Shot("menu_title"),
                Menu(Screen::Pause),
                Wait(5),
                Shot("menu_pause"),
                Menu(Screen::Chambers),
                Wait(5),
                Shot("menu_chambers"),
                Menu(Screen::Settings(Page::Audio)),
                Wait(5),
                Shot("menu_settings_audio"),
                Menu(Screen::Settings(Page::Display)),
                Wait(5),
                Shot("menu_settings_display"),
                Menu(Screen::Settings(Page::Graphics)),
                Wait(5),
                Shot("menu_settings_graphics"),
                Menu(Screen::Controls),
                Wait(5),
                Shot("menu_controls"),
            ]
        }
        // A few of Portal's chambers, from where she starts.
        "native" => {
            use crate::LevelGroup::Portal as N;
            let mut v = Vec::new();
            for (map, shot) in [("testchmb_a_00", "a00"), ("testchmb_a_08", "a08"), ("testchmb_a_11", "a11"), ("escape_02", "e02")] {
                v.extend([Chamber(N, map), Wait(120), Shot(shot)]);
            }
            v
        }
        // From Portal's Chamber 00 into the editor, a wall moved, Portal's brushes and a
        // trigger dropped in, then played.
        // Clicking a prop's model selects it (#85): Chamber 00's clock radio, clicked on its
        // face, well away from its origin (at its foot).
        "editorpick" => {
            use crate::LevelGroup::Portal as N;
            vec![
                Chamber(N, "testchmb_a_00"),
                Wait(60),
                Edit(|_, menu| menu.edit = Some(false)),
                Wait(150),
                Edit(|ed, _| {
                    ed.selected = None;
                    ed.cam = crate::editor::FlyCam { pos: Vec3::new(8.5, 5.0, 15.47), yaw: std::f32::consts::FRAC_PI_2, pitch: 0.0, speed: 8.0 };
                }),
                Wait(30),
                Mouse(Hand::To(0.5, 0.5)),
                Wait(2),
                Mouse(Hand::Press),
                Wait(2),
                Mouse(Hand::Release),
                Wait(10),
                Edit(|ed, _| {
                    let what = ed.selected.and_then(|i| ed.doc.objects.get(i)).map(|o| match &o.kind {
                        aperture::doc::Kind::Entity { class, keys, .. } => format!("{class} {}", keys.iter().find(|(k, _)| k == "model").map_or("", |(_, v)| v.as_str())),
                        k => format!("{k:?}").chars().take(60).collect(),
                    });
                    println!("capture: clicked the radio's face: selected {what:?}");
                }),
                Shot("editorpick_01_selected"),
            ]
        }
        // Start, run two seconds, quit (for what starting and quitting leave behind: the GPU's
        // memory).
        "quit" => vec![Wait(120)],
        // A walk in a Mirror's Edge level (#109): she's put at PORTAL_ME_AT (x,y,z,yaw[,pitch]), runs
        // forward PORTAL_ME_RUN frames (90 unless said), a shot every 15 (`walk_00`, ...).
        "mewalk" => {
            let file: &'static str = Box::leak(std::env::var("PORTAL_ME_LEVEL").unwrap_or_else(|_| "escape".into()).into_boxed_str());
            let f: Vec<f32> = std::env::var("PORTAL_ME_AT").unwrap_or_default().split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let frames: u32 = std::env::var("PORTAL_ME_RUN").ok().and_then(|v| v.parse().ok()).unwrap_or(90);
            let mut v = vec![Chamber(crate::LevelGroup::MirrorsEdge, file), Wait(120)];
            if f.len() >= 4 {
                v.extend([Teleport(Vec3::new(f[0], f[1], f[2])), Look(f[3], f.get(4).copied().unwrap_or(-5.0)), Wait(60)]);
            }
            for k in 0..=frames / 15 {
                v.push(Shot(Box::leak(format!("walk_{k:02}").into_boxed_str())));
                v.push(Hold(MoveInput { move_axis: Vec2::new(0.0, 1.0), ..default() }, 15));
            }
            v
        }
        // Views of a Mirror's Edge level from a spectator's camera: PORTAL_ME_EYES, eyes
        // `x,y,z,yaw,pitch` split by `;`, a shot from each (`spec_00`, ...).
        "mespec" => {
            let file: &'static str = Box::leak(std::env::var("PORTAL_ME_LEVEL").unwrap_or_else(|_| "tutorial".into()).into_boxed_str());
            let mut v = vec![Chamber(crate::LevelGroup::MirrorsEdge, file), Wait(120)];
            for (k, eye) in std::env::var("PORTAL_ME_EYES").unwrap_or_default().split(';').enumerate() {
                let f: Vec<f32> = eye.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if f.len() == 5 {
                    v.extend([Spectate(Vec3::new(f[0], f[1], f[2]), f[3], f[4]), Wait(90), Shot(Box::leak(format!("spec_{k:02}").into_boxed_str()))]);
                }
            }
            v
        }
        // A tour of a Mirror's Edge level (PORTAL_ME_LEVEL, a time trial's best: its course):
        // from its start and each checkpoint, a look along the way to the next, from a little
        // above and behind (`tour_00`, ...).
        "metour" => {
            let file = std::env::var("PORTAL_ME_LEVEL").unwrap_or_else(|_| "tt_tutorial_a01".into());
            let path = crate::editor::levels_dir().join("me").join(format!("{file}.ron"));
            let doc = std::fs::read_to_string(&path).ok().and_then(|t| aperture::doc::LevelDoc::from_ron(&t).ok());
            let points: Vec<Vec3> = doc
                .map(|d| {
                    d.objects
                        .iter()
                        .filter_map(|o| match o.kind {
                            aperture::doc::Kind::Spawn { .. } => Some(o.at),
                            aperture::doc::Kind::Checkpoint { .. } | aperture::doc::Kind::Exit => Some(o.at - Vec3::Y * o.size.y * 0.5),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            let file: &'static str = Box::leak(file.into_boxed_str());
            let mut v = vec![Chamber(crate::LevelGroup::MirrorsEdge, file), Wait(120)];
            for (k, w) in points.windows(2).enumerate() {
                let d = Vec3::new(w[1].x - w[0].x, 0.0, w[1].z - w[0].z).normalize_or(Vec3::NEG_Z);
                let eye = w[0] - d * 4.0 + Vec3::Y * 3.5;
                let yaw = (-d.x).atan2(-d.z).to_degrees();
                let name: &'static str = Box::leak(format!("tour_{k:02}").into_boxed_str());
                v.extend([Spectate(eye, yaw, -12.0), Wait(45), Shot(name)]);
            }
            v
        }
        // A Mirror's Edge level in the editor (#100): opened, a prop in front of the camera
        // lifted a metre, then played.
        "meedit" => vec![
            Chamber(crate::LevelGroup::MirrorsEdge, "tutorial"),
            Wait(90),
            Edit(|_, menu| menu.edit = Some(false)),
            Wait(150),
            Shot("meedit_01_opened"),
            Edit(|ed, _| {
                let (pos, look) = (ed.cam.pos, Quat::from_euler(EulerRot::YXZ, ed.cam.yaw, ed.cam.pitch, 0.0) * Vec3::NEG_Z);
                let i = ed.doc.objects.iter().enumerate().filter(|(_, o)| matches!(&o.kind, aperture::doc::Kind::MeProp { solid: true, .. }) && o.bounds().size().max_element() < 4.0 && (o.at - pos).normalize_or_zero().dot(look) > 0.7).min_by(|a, b| a.1.at.distance(pos).total_cmp(&b.1.at.distance(pos))).map(|(i, _)| i);
                println!("capture: lifting {:?}", i.map(|i| ed.doc.objects[i].kind.label()));
                if let Some(i) = i {
                    ed.selected = Some(i);
                    ed.change(|d| d.objects[i].at.y += 1.0);
                }
            }),
            Wait(90),
            Shot("meedit_02_moved"),
            Edit(|ed, menu| ed.play(menu)),
            Wait(120),
            Shot("meedit_03_playing"),
        ],
        // Which way up Bevy reads a normal map's green (with PORTAL_NORMAL_TEST=1).
        "normaltest" => vec![Map(0), Wait(60), Spectate(Vec3::new(0.0, 3.0, -5.0), 0.0, 0.0), Wait(60), Shot("normaltest")],
        "nativeedit" => {
            use crate::LevelGroup::Portal as N;
            vec![
                Chamber(N, "testchmb_a_00"),
                Wait(60),
                Edit(|_, menu| menu.edit = Some(false)),
                Wait(150),
                Shot("nativeedit_01_opened"),
                Edit(|ed, _| {
                    // The nearest white wall in front of the camera, a metre up.
                    let (pos, look) = (ed.cam.pos, Quat::from_euler(EulerRot::YXZ, ed.cam.yaw, ed.cam.pitch, 0.0) * Vec3::NEG_Z);
                    let i = ed.doc.objects.iter().enumerate().filter(|(_, o)| matches!(&o.kind, aperture::doc::Kind::Brush(b) if b.faces.iter().any(|f| f.mat.starts_with("plastic/plasticwall"))) && (o.at - pos).normalize_or_zero().dot(look) > 0.8).min_by(|a, b| a.1.at.distance(pos).total_cmp(&b.1.at.distance(pos))).map(|(i, _)| i);
                    if let Some(i) = i {
                        ed.selected = Some(i);
                        ed.change(|d| d.objects[i].at.y += 1.0);
                    }
                }),
                Wait(90),
                Shot("nativeedit_02_moved"),
                Edit(|ed, _| {
                    use crate::editor::catalog::find;
                    let (pos, look) = (ed.cam.pos, Quat::from_euler(EulerRot::YXZ, ed.cam.yaw, ed.cam.pitch, 0.0) * Vec3::NEG_Z);
                    let floor = pos + Vec3::new(look.x, 0.0, look.z).normalize_or_zero() * 4.0 - Vec3::Y * 2.5;
                    ed.add_entry(find("Portal black metal"), floor, Vec3::Y, look);
                    ed.add_entry(find("Portal light panel"), floor + Vec3::new(2.0, 0.0, 0.0), Vec3::Y, look);
                    ed.add_entry(find("Trigger"), floor + Vec3::new(-3.0, 0.0, 0.0), Vec3::Y, look);
                    ed.add_entry(find("Relay"), floor + Vec3::new(-3.0, 0.0, 2.0), Vec3::Y, look);
                }),
                Wait(90),
                Shot("nativeedit_03_added"),
                Edit(|ed, menu| ed.play(menu)),
                Wait(120),
                Shot("nativeedit_04_playing"),
            ]
        }
        // Portal's maps' effects: a security camera watching her, a spotlight, steam, an
        // elevator shaft's beam, sparks, a beam, mist.
        "effects" => {
            use crate::LevelGroup::Portal as N;
            vec![
                Chamber(N, "testchmb_a_00"),
                Wait(60),
                Shot("fx_lit"),
                // (The vault's lights dimmed, as GLaDOS has them.)
                Send("color_correction", "Enable", ""),
                Wait(30),
                Shot("fx_dimmed"),
                Send("color_correction", "Disable", ""),
                ViewArt("camera", 0, 3.0),
                Wait(40),
                Shot("fx_camera"),
                ViewArt("beam", 0, 5.0),
                Wait(20),
                Shot("fx_beam"),
                ViewArt("sparks", 0, 2.5),
                Send("env_spark", "StartSpark", ""),
                Wait(90),
                Shot("fx_sparks"),
                ViewArt("spotlight", 0, 5.0),
                Wait(20),
                Shot("fx_spotlight"),
                Chamber(N, "escape_01"),
                Wait(60),
                Send("trigger_hurt", "Disable", ""),
                Send("env_steam", "TurnOn", ""),
                Send("info_particle_system", "Start", ""),
                ViewArt("steam", 0, 3.0),
                Wait(40),
                Shot("fx_steam"),
            ]
        }
        // The chambers menu's folders.
        "chambersmenu" => {
            use crate::menu::Screen;
            vec![
                Wait(30),
                Menu(Screen::Chambers),
                Wait(10),
                Shot("menu_chambers"),
                Menu(Screen::Folder(crate::LevelGroup::Portal)),
                Wait(10),
                Shot("menu_native"),
                Menu(Screen::Folder(crate::LevelGroup::Testing)),
                Wait(10),
                Shot("menu_testing"),
            ]
        }
        // The game's copy of Portal's Chamber 00 in the editor (PORTAL_EDIT=1): opened, a
        // relay's outputs and what fires at it, a brush and the material picker.
        "nativeeditor" => vec![
            Wait(60),
            Edit(|ed, _| {
                let files = crate::source_map::native_files();
                if let Some((p, _)) = files.into_iter().find(|(_, m)| m == "testchmb_a_00") {
                    if let Ok(d) = crate::editor::read_doc(&p) {
                        ed.open(d, Some(p));
                    }
                }
            }),
            Wait(150),
            Shot("native_editor_01_opened"),
            Edit(|ed, _| {
                // The relay that opens the first door, seen from across the room.
                let i = ed.doc.objects.iter().position(|o| matches!(&o.kind, aperture::doc::Kind::Entity { class, .. } if class == "logic_relay") && !o.outputs.is_empty());
                ed.selected = i;
                if let Some(o) = i.map(|i| &ed.doc.objects[i]) {
                    ed.cam = crate::editor::FlyCam { pos: o.at + Vec3::new(0.0, 2.0, 6.0), yaw: 0.0, pitch: -0.25, speed: 8.0 };
                }
            }),
            Wait(30),
            Shot("native_editor_02_relay"),
            Edit(|ed, _| {
                // A white wall brush near the start, and the material picker for it.
                let spawn = ed.cam.pos;
                let i = ed.doc.objects.iter().enumerate().filter(|(_, o)| matches!(&o.kind, aperture::doc::Kind::Brush(b) if b.faces.iter().any(|f| f.mat.starts_with("plastic/plasticwall")))).min_by(|a, b| a.1.at.distance(spawn).total_cmp(&b.1.at.distance(spawn))).map(|(i, _)| i);
                ed.selected = i;
            }),
            Wait(30),
            Shot("native_editor_03_brush"),
            Edit(|ed, _| ed.picking = ed.selected.map(|obj| crate::editor::MatTarget { obj, face: None })),
            Wait(90),
            Shot("native_editor_04_materials"),
        ],
        // The level editor (start it with PORTAL_EDIT=1): the chamber it opens on, a new room
        // with pieces dropped in and wired up, then playing it.
        "editor" => vec![
            Wait(120),
            Shot("editor_01_opened"),
            Edit(|ed, _| ed.open(aperture::doc::LevelDoc::starter("Demo"), None)),
            Wait(60),
            Shot("editor_02_new_room"),
            Edit(|ed, _| {
                use crate::editor::catalog::find;
                let look = Vec3::NEG_Z;
                ed.add_entry(find("Floor button"), Vec3::new(-3.0, 0.0, -6.0), Vec3::Y, look);
                ed.add_entry(find("Storage cube"), Vec3::new(3.0, 0.0, -5.0), Vec3::Y, look);
                ed.add_entry(find("Cut (doorway)"), Vec3::new(0.0, 1.5, -16.0), Vec3::Z, look);
                ed.add_entry(find("Door"), Vec3::new(0.0, 0.0, -16.3), Vec3::Y, look);
                ed.add_entry(find("Laser emitter"), Vec3::new(-6.0, 2.0, -12.0), Vec3::X, look);
                ed.add_entry(find("Faith plate"), Vec3::new(3.0, 0.0, -11.0), Vec3::Y, look);
                ed.add_entry(find("Black stripe"), Vec3::new(0.0, 0.0, -9.0), Vec3::Y, look);
                ed.selected = ed.doc.objects.iter().position(|o| matches!(o.kind, aperture::doc::Kind::Door { .. }));
                ed.cam = crate::editor::FlyCam { pos: Vec3::new(0.0, 4.2, -0.8), yaw: 0.0, pitch: -0.35, speed: 8.0 };
            }),
            Wait(90),
            Shot("editor_03_pieces"),
            Edit(|ed, menu| ed.play(menu)),
            Wait(90),
            Shot("editor_04_playing"),
        ],
        // The level editor's Portal sounds and effects (PORTAL_EDIT=1): a room with a camera,
        // steam, sparks, a spotlight's beam, mist, a fan, a push up, a decal, a sound (and the
        // sound browser for it) and a physics prop, then playing it.
        "editoreffects" => vec![
            Wait(90),
            Edit(|ed, _| {
                use crate::editor::catalog::find;
                use aperture::doc::Kind;
                ed.open(aperture::doc::LevelDoc::starter("Effects"), None);
                ed.tab = Some(crate::editor::catalog::Category::PortalEffects);
                let look = Vec3::NEG_Z;
                ed.add_entry(find("Decal"), Vec3::new(0.0, 2.0, -16.0), Vec3::Z, look);
                ed.add_entry(find("Security camera"), Vec3::new(-6.0, 4.0, -12.0), Vec3::X, look);
                // (Against black, to be seen.)
                ed.add_entry(find("Black metal"), Vec3::new(6.0, 0.0, -9.0), Vec3::NEG_X, look);
                ed.add_entry(find("Steam"), Vec3::new(4.0, 1.0, -9.0), Vec3::NEG_X, look);
                ed.add_entry(find("Black metal"), Vec3::new(-6.0, 2.0, -8.0), Vec3::X, look);
                ed.add_entry(find("Sparks"), Vec3::new(-4.0, 3.0, -8.0), Vec3::X, look);
                ed.add_entry(find("Spotlight beam"), Vec3::new(3.0, 4.5, -16.0), Vec3::Z, look);
                ed.add_entry(find("Particles"), Vec3::new(3.0, 0.0, -12.0), Vec3::Y, look);
                ed.add_entry(find("Fan (brush)"), Vec3::new(-3.0, 0.0, -12.0), Vec3::Y, look);
                ed.add_entry(find("Push volume"), Vec3::new(-3.0, 0.2, -12.0), Vec3::Y, look);
                // (The level's own piece beside the map's: drawn as the game's.)
                ed.add_entry(find("Moving platform"), Vec3::new(-3.5, 0.0, -4.0), Vec3::Y, look);
                let mut chair = aperture::doc::Object::new(Kind::Model { path: "props/portal/props/lab_chair/lab_chair.glb".into(), scale: Vec3::ONE, bounds: Some((Vec3::new(-0.284, -0.6, -0.285), Vec3::new(0.286, 0.472, 0.306))), solid: true }, Vec3::new(1.5, 0.6, -6.0));
                if let Some(k) = crate::editor::ui::physics_prop(&chair.kind) {
                    chair.kind = k;
                }
                ed.doc.objects.push(chair);
                ed.add_entry(find("Sound"), Vec3::new(0.0, 0.0, -6.0), Vec3::Y, look);
                ed.selected = Some(ed.doc.objects.len() - 1);
                ed.sound_picking = ed.selected;
                ed.cam = crate::editor::FlyCam { pos: Vec3::new(0.0, 3.5, -1.0), yaw: 0.0, pitch: -0.2, speed: 8.0 };
            }),
            Wait(120),
            Shot("edfx_01_placed"),
            Edit(|ed, menu| {
                ed.sound_picking = None;
                ed.play(menu);
            }),
            Wait(150),
            Shot("edfx_02_playing"),
            Teleport(Vec3::new(0.0, 0.0, -9.0)),
            Look(-90.0, -10.0),
            Wait(30),
            Shot("edfx_03_steam"),
            Teleport(Vec3::new(0.0, 0.0, -8.0)),
            Look(90.0, 10.0),
            Wait(150),
            Shot("edfx_04_sparks"),
            Teleport(Vec3::new(3.0, 0.0, -6.0)),
            Look(0.0, -20.0),
            Wait(30),
            Shot("edfx_05_mist"),
        ],
        // The level editor by mouse (PORTAL_EDIT=1): a cube dragged in from the content
        // browser, a button clicked and then placed, the wheel over a panel and over the view,
        // and the gizmo on a piece off to the side with the camera turned.
        "editormouse" => {
            vec![
                Wait(90),
                Edit(|ed, _| {
                    ed.open(aperture::doc::LevelDoc::starter("Mouse"), None);
                    ed.cam = crate::editor::FlyCam { pos: Vec3::new(0.0, 4.2, -0.8), yaw: 0.0, pitch: -0.35, speed: 8.0 };
                }),
                Wait(60),
                Mouse(Hand::ToTile("Storage cube")),
                Wait(2),
                Mouse(Hand::Press),
                Wait(2),
                Mouse(Hand::To(0.3, 0.5)),
                Wait(2),
                Mouse(Hand::To(0.45, 0.6)),
                Wait(2),
                Mouse(Hand::To(0.55, 0.65)),
                Wait(6),
                Shot("mouse_01_dragging"),
                Mouse(Hand::Release),
                Wait(20),
                Shot("mouse_02_dropped"),
                Mouse(Hand::ToTile("Floor button")),
                Wait(2),
                Mouse(Hand::Press),
                Wait(2),
                Mouse(Hand::Release),
                Wait(3),
                Mouse(Hand::To(0.4, 0.75)),
                Wait(6),
                Shot("mouse_03_armed"),
                Mouse(Hand::Press),
                Wait(2),
                Mouse(Hand::Release),
                Wait(20),
                Shot("mouse_04_placed"),
                Edit(|ed, _| println!("capture: camera before scrolling {}", ed.cam.pos)),
                Mouse(Hand::ToTile("Glass")),
                Wait(2),
                Mouse(Hand::Scroll(-3.0)),
                Wait(5),
                Edit(|ed, _| println!("capture: camera after scrolling over a panel {}", ed.cam.pos)),
                Mouse(Hand::To(0.5, 0.5)),
                Wait(2),
                Mouse(Hand::Scroll(-3.0)),
                Wait(5),
                Edit(|ed, _| println!("capture: camera after scrolling over the view {}", ed.cam.pos)),
                Edit(|ed, _| {
                    ed.selected = ed.doc.objects.iter().position(|o| matches!(o.kind, aperture::doc::Kind::Cube { .. }));
                    // The cube well off to the side of the view, the camera turned and tipped.
                    ed.cam = crate::editor::FlyCam { pos: Vec3::new(-2.0, 3.0, -2.0), yaw: 0.1, pitch: -0.45, speed: 8.0 };
                }),
                Wait(20),
                Shot("mouse_05_gizmo_turned"),
                Edit(|ed, _| println!("capture: objects {:?}", ed.doc.objects.iter().map(|o| o.kind.label()).collect::<Vec<_>>())),
                // Mirror's Edge's parkour pieces: a ladder on the left wall, a zipline, a swing
                // bar and a balance beam.
                Edit(|ed, _| {
                    use crate::editor::catalog::find;
                    let look = Vec3::NEG_Z;
                    ed.add_entry(find("Ladder"), Vec3::new(-6.0, 1.0, -6.0), Vec3::X, look);
                    ed.add_entry(find("Zipline"), Vec3::new(4.0, 0.0, -3.0), Vec3::Y, look);
                    ed.add_entry(find("Swing pole"), Vec3::new(0.0, 0.0, -12.0), Vec3::Y, look);
                    ed.add_entry(find("Balance beam"), Vec3::new(-3.0, 0.0, -10.0), Vec3::Y, look);
                    ed.selected = None;
                    ed.cam = crate::editor::FlyCam { pos: Vec3::new(0.0, 4.5, -0.8), yaw: 0.0, pitch: -0.3, speed: 8.0 };
                }),
                Wait(30),
                Shot("mouse_06_parkour"),
                Edit(|ed, menu| ed.play(menu)),
                Wait(30),
                Menu(crate::menu::Screen::Title),
                Wait(20),
                Shot("mouse_07_title_menu"),
            ]
        }
        // The level editor's props (PORTAL_EDIT=1): Portal 2's and Mirror's Edge's with their
        // pictures, one of each dragged in; then a zipline's end dragged by its handle.
        "editorprops" => vec![
            Wait(90),
            Edit(|ed, _| {
                ed.open(aperture::doc::LevelDoc::starter("Props"), None);
                ed.cam = crate::editor::FlyCam { pos: Vec3::new(0.0, 4.2, -0.8), yaw: 0.0, pitch: -0.35, speed: 8.0 };
                ed.tab = Some(crate::editor::catalog::Category::Props);
                ed.prop_game = 1;
                ed.prop_folder = Some("props_lab".into());
            }),
            Wait(400),
            Shot("props_01_portal2"),
            Mouse(Hand::ToProp(2)),
            Wait(2),
            Mouse(Hand::Press),
            Wait(2),
            Mouse(Hand::To(0.4, 0.55)),
            Wait(2),
            Mouse(Hand::To(0.45, 0.62)),
            Wait(6),
            Shot("props_02_dragging"),
            Mouse(Hand::Release),
            Wait(60),
            Shot("props_03_placed"),
            Edit(|ed, _| {
                ed.prop_game = 2;
                ed.prop_folder = Some("P_Rooftops".into());
            }),
            Wait(500),
            Shot("props_04_mirrors_edge"),
            Mouse(Hand::ToProp(0)),
            Wait(2),
            Mouse(Hand::Press),
            Wait(2),
            Mouse(Hand::To(0.55, 0.6)),
            Wait(2),
            Mouse(Hand::To(0.6, 0.7)),
            Wait(6),
            Mouse(Hand::Release),
            Wait(60),
            Shot("props_05_me_placed"),
            Edit(|ed, _| {
                use crate::editor::catalog::find;
                ed.add_entry(find("Zipline"), Vec3::new(-4.0, 0.0, -3.0), Vec3::Y, Vec3::NEG_Z);
            }),
            Wait(30),
            Shot("props_06_zipline_handles"),
            Mouse(Hand::ToEnd(0)),
            Wait(3),
            Mouse(Hand::Press),
            Wait(2),
            Mouse(Hand::To(0.75, 0.6)),
            Wait(4),
            Mouse(Hand::To(0.8, 0.62)),
            Wait(6),
            Mouse(Hand::Release),
            Wait(30),
            Shot("props_07_zipline_end_moved"),
            Edit(|ed, _| println!("capture: objects {:?}", ed.doc.objects.iter().map(|o| format!("{} at {} {:?}", o.kind.label(), o.at, o.kind)).filter(|s| !s.contains("block") && !s.contains("Lamp")).collect::<Vec<_>>())),
        ],
        // The level editor's view options and props turned and scaled every way (PORTAL_EDIT=1).
        "editorview" => vec![
            Wait(90),
            Edit(|ed, _| {
                use aperture::doc::{Kind, Object};
                ed.open(aperture::doc::LevelDoc::starter("View"), None);
                ed.cam = crate::editor::FlyCam { pos: Vec3::new(0.0, 3.0, -1.5), yaw: 0.0, pitch: -0.4, speed: 8.0 };
                let bounds = Some((Vec3::new(-0.278, 0.001, -0.124), Vec3::new(0.278, 0.783, 0.124)));
                let radio = Object::new(Kind::Model { path: "props/portal2/props/radio_reference.glb".into(), scale: Vec3::ONE, bounds, solid: true }, Vec3::new(-1.5, 0.0, -6.0));
                let mut big = radio.clone();
                big.at = Vec3::new(1.5, 1.0, -6.0);
                big.pitch = 30.0;
                big.roll = 20.0;
                big.yaw = 45.0;
                if let Kind::Model { scale, .. } = &mut big.kind {
                    *scale = Vec3::new(3.0, 1.5, 2.0);
                }
                let cistern = Object::new(Kind::MeProp { package: "P_Rooftops".into(), mesh: "S_Cistern_07".into(), scale: Vec3::ONE, bounds: None, solid: false, own_shape: false, materials: Vec::new(), hidden: false }, Vec3::new(0.0, 0.0, -11.0));
                ed.doc.objects.extend([radio, big, cistern]);
                ed.selected = Some(ed.doc.objects.len() - 2);
                ed.rebuild_now();
            }),
            Wait(90),
            Shot("view_01_props_turned_and_scaled"),
            Edit(|ed, _| ed.selected = Some(ed.doc.objects.len() - 1)),
            Wait(10),
            Shot("view_02_me_prop_selected"),
            Edit(|ed, _| {
                ed.view.collision = true;
                ed.view.meshes = false;
            }),
            Wait(120),
            Shot("view_03_collision_only"),
            Edit(|ed, _| {
                ed.view.collision = false;
                ed.view.meshes = true;
                ed.view.unlit = true;
            }),
            Wait(60),
            Shot("view_04_unlit"),
            Edit(|ed, _| ed.view.unlit = false),
            Wait(30),
            Shot("view_05_lit_again"),
        ],
        // Ray-traced lighting switched off and on again while playing: a chamber, a rooftop
        // outdoors, one of Portal's maps.
        "rtswitch" => {
            let mut v = Vec::new();
            let maps: [(usize, u32, [&'static str; 3]); 3] = [
                (1, 120, ["rt_chamber_1_on", "rt_chamber_2_off", "rt_chamber_3_on_again"]),
                (7, 120, ["rt_rooftop_1_on", "rt_rooftop_2_off", "rt_rooftop_3_on_again"]),
                (11, 240, ["rt_portal_map_1_on", "rt_portal_map_2_off", "rt_portal_map_3_on_again"]),
            ];
            for (map, wait, [on, off, again]) in maps {
                v.extend([Map(map), Wait(wait), Shot(on), RayTracing(false), Wait(90), Shot(off), RayTracing(true), Wait(150), Shot(again)]);
            }
            v
        }
        // The lighting before #3's work (no ray tracing, bloom, ambient occlusion or auto
        // exposure), with today's raster look, and traced; in a chamber, on a rooftop, in one
        // of Portal's maps. Then a portal pair: the view through blue, and the same view from
        // just in front of orange (which it shows), traced and not.
        "lighting3" => {
            let before: fn(&mut crate::settings::Settings) = |s| {
                s.ray_tracing = 0.0;
                s.bloom = 0.0;
                s.ssao = 0.0;
                s.auto_exposure = 0.0;
            };
            let raster: fn(&mut crate::settings::Settings) = |s| {
                let d = crate::settings::Settings::default();
                (s.bloom, s.ssao, s.auto_exposure, s.ray_tracing) = (d.bloom, d.ssao, d.auto_exposure, 0.0);
            };
            let mut v = Vec::new();
            let maps: [(usize, &'static str); 3] = [(1, "chamber"), (7, "rooftop"), (aperture::levels::all().len() + 11, "portal16")];
            for (map, name) in maps {
                let shot = |k: &str| Shot(Box::leak(format!("l3_{name}_{k}").into_boxed_str()));
                v.extend([Set(before), Map(map), Wait(240), shot("1_before"), Set(raster), Wait(120), shot("2_raster"), RayTracing(true), Wait(240), shot("3_traced")]);
            }
            v.extend([
                Map(0),
                Wait(120),
                Portal(PortalColor::Blue, Vec3::new(24.0, 1.2, 18.0), Vec3::NEG_X, Vec3::Y),
                Portal(PortalColor::Orange, Vec3::new(-24.0, 1.2, 14.0), Vec3::X, Vec3::Y),
                Teleport(Vec3::new(21.0, 0.0, 18.0)),
                Launch(Vec3::ZERO),
                Look(-90.0, 0.0),
                Wait(240),
                Shot("l3_portal_1_through_blue_traced"),
                Teleport(Vec3::new(-23.6, 0.0, 14.0)),
                Launch(Vec3::ZERO),
                Look(-90.0, 0.0),
                Wait(240),
                Shot("l3_portal_2_from_orange_traced"),
                RayTracing(false),
                Wait(120),
                Shot("l3_portal_4_from_orange_raster"),
                Teleport(Vec3::new(21.0, 0.0, 18.0)),
                Launch(Vec3::ZERO),
                Look(-90.0, 0.0),
                Wait(120),
                Shot("l3_portal_3_through_blue_raster"),
                Set(|s| s.ray_tracing = 1.0),
            ]);
            v
        }
        // #25's gaps: a floor indicator strip in Chamber 00; the portal gun on Chamber 02's
        // turning pedestal.
        "gaps25" => {
            use crate::LevelGroup::Portal as N;
            vec![
                Chamber(N, "testchmb_a_00"),
                Wait(60),
                Teleport(Vec3::new(24.0, 3.25, 22.5)),
                Launch(Vec3::ZERO),
                Look(0.0, -45.0),
                Wait(60),
                Shot("gaps25_floor_strip"),
                Chamber(N, "testchmb_a_01"),
                Wait(100),
                Teleport(Vec3::new(-4.06, -3.25, 0.4)),
                Look(0.0, -12.0),
                Wait(60),
                Shot("gaps25_pedestal_a"),
                Wait(90),
                Shot("gaps25_pedestal_b"),
            ]
        }
        // The level editor's lights (PORTAL_EDIT=1): a dark room with a red point light and a
        // blue spot light, then blocks outdoors under an orange sun; each traced and not.
        "editorlights" => vec![
            Wait(90),
            Edit(|ed, _| {
                use aperture::doc::{Ambient, Kind, LevelDoc, Object};
                let mut d = LevelDoc::starter("Lights");
                d.objects.retain(|o| o.kind != Kind::Lamp);
                d.ambient = Some(Ambient { color: Vec3::ONE, brightness: 20.0 });
                d.objects.push(Object::new(Kind::PointLight { color: Vec3::new(1.0, 0.15, 0.1), intensity: 400_000.0, range: 15.0, radius: 0.1, shadows: false }, Vec3::new(-3.5, 1.5, -9.0)));
                let mut spot = Object::new(Kind::SpotLight { color: Vec3::new(0.2, 0.5, 1.0), intensity: 1_500_000.0, range: 20.0, radius: 0.1, angle: 25.0, soft: 0.3, shadows: true }, Vec3::new(3.0, 5.8, -9.0));
                spot.pitch = -90.0;
                d.objects.push(spot);
                d.objects.push(Object::sized(Kind::Block { mat: aperture::Mat::White }, Vec3::new(3.0, 0.5, -9.0), Vec3::new(1.0, 1.0, 1.0)));
                ed.open(d, None);
                ed.selected = Some(ed.doc.objects.len() - 2);
                ed.cam = crate::editor::FlyCam { pos: Vec3::new(0.0, 3.5, -1.0), yaw: 0.0, pitch: -0.35, speed: 8.0 };
            }),
            Wait(150),
            Shot("lights_1_room_traced"),
            RayTracing(false),
            Wait(90),
            Shot("lights_2_room_not_traced"),
            RayTracing(true),
            Wait(30),
            Edit(|ed, _| {
                use aperture::doc::{Kind, LevelDoc, Object};
                use aperture::Mat;
                let mut d = LevelDoc::new("Sun");
                d.indoor = false;
                d.objects.push(Object::sized(Kind::Block { mat: Mat::White }, Vec3::new(0.0, -0.5, -10.0), Vec3::new(30.0, 1.0, 30.0)));
                for (x, z) in [(-4.0, -8.0), (2.0, -12.0), (5.0, -6.0)] {
                    d.objects.push(Object::sized(Kind::Block { mat: Mat::Black }, Vec3::new(x, 1.0, z), Vec3::new(1.5, 2.0, 1.5)));
                }
                d.objects.push(Object::new(Kind::Spawn { gravity: None }, Vec3::new(0.0, 0.0, 2.0)));
                let mut sun = Object::new(Kind::Sun { color: Vec3::new(1.0, 0.55, 0.25), illuminance: 9000.0 }, Vec3::new(0.0, 6.0, -10.0));
                sun.face(Vec3::new(1.0, -0.45, -0.6));
                d.objects.push(sun);
                ed.open(d, None);
                ed.cam = crate::editor::FlyCam { pos: Vec3::new(0.0, 5.0, 4.0), yaw: 0.0, pitch: -0.45, speed: 8.0 };
            }),
            Wait(150),
            Shot("lights_3_sun_traced"),
            RayTracing(false),
            Wait(90),
            Shot("lights_4_sun_not_traced"),
            RayTracing(true),
        ],
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
        // The fling, a frame at a time: into the floor portal, out of the wall.
        "flingfilm" => vec![
            Map(2),
            Wait(30),
            Teleport(Vec3::new(0.0, 12.0, 0.6)),
            Wait(10),
            Look(0.0, -62.0),
            Wait(5),
            Fire(PortalColor::Blue),
            Wait(20),
            Look(0.0, -20.0),
            Hold(fwd(), 6),
            Hold(MoveInput { move_axis: Vec2::new(0.0, 1.0), jump_pressed: true, jump_held: true, ..default() }, 1),
            Hold(MoveInput::default(), 20),
            Film("f", MoveInput::default(), 60),
        ],
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
        // Polish checks: the floor button's light, a cube dropped off-square, a cube thrown
        // by the faith plate, and Chamber 06's pellet (the ball, the launcher's mouth, a
        // bounce mark). Yaw 0 looks along -Z and positive yaw turns left, towards -X.
        "polish" => vec![
            Map(0),
            Wait(180),
            // The floor button, same framing as the rtx close-up, then held down.
            Teleport(Vec3::new(-16.0, 0.0, 5.4)),
            Look(180.0, -35.0),
            Wait(40),
            Shot("button_light"),
            // Closer, on the near rim, where the lamp sits. Far enough that the rim stays in frame.
            Teleport(Vec3::new(-16.0, 0.0, 6.35)),
            Look(180.0, -40.0),
            Wait(20),
            Shot("button_close"),
            Teleport(Vec3::new(-16.0, 0.0, 8.75)),
            Look(0.0, -45.0),
            Wait(20),
            Shot("button_pressed"),
            // The dropper's cube sits at z 17. Face it (+Z), pick it up, turn off a square
            // angle and drop it.
            Teleport(Vec3::new(-16.0, 0.0, 15.2)),
            Look(180.0, -30.0),
            Wait(30),
            Use,
            Wait(20),
            Look(230.0, -15.0),
            Wait(25),
            Use,
            Wait(50),
            Look(230.0, -58.0),
            Wait(15),
            Shot("cube_yaw"),
            // Hold it over the plate (the plate is to the west) and let it fall on.
            Look(230.0, -40.0),
            Wait(10),
            Use,
            Wait(20),
            Teleport(Vec3::new(-12.0, 0.0, -4.0)),
            Look(90.0, -5.0),
            Wait(25),
            Use,
            Wait(8),
            // Aside, where the plate, the arc and the platform are all in view.
            Teleport(Vec3::new(-8.0, 0.0, -8.0)),
            Look(124.0, -12.0),
            Wait(20),
            Shot("plate_cube_launch"),
            Look(67.0, 32.0),
            Wait(35),
            Shot("plate_cube_air"),
            // The arc is still in the air here. Wait it out, then look down onto the platform.
            Wait(50),
            Teleport(Vec3::new(-9.0, 7.5, -12.0)),
            Look(52.0, -17.0),
            Wait(5),
            Shot("plate_cube_land"),
            // Chamber 06. The mouth opens as it fires; the ball crosses at y 6; it scorches
            // the right wall (x 10). A film covers the bounce, whose frame depends on how
            // long the map's shader pause let the sim run.
            Map(6),
            // In front of the mouth (it fires toward +X) so the open and the close fill the frame.
            Teleport(Vec3::new(-6.4, 0.0, -4.0)),
            Look(90.0, 50.0),
            Wait(8),
            Film("launcher", MoveInput::default(), 45),
            Shot("launcher_mouth"),
            Teleport(Vec3::new(0.0, 0.0, 4.0)),
            Look(0.0, 28.0),
            Wait(8),
            Shot("pellet_ball"),
            // The right wall, where the pellet hits. Wait until it's close, then film the mark
            // and one frame after the glow has died.
            Teleport(Vec3::new(6.2, 0.0, -1.6)),
            Look(-56.0, 42.0),
            WaitPelletX(8.6, 240),
            Film("scorch", MoveInput::default(), 18),
            Wait(40),
            Shot("pellet_scorch"),
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

/// One frame of a `Film`.
fn film(commands: &mut Commands, dir: &str, name: &str, k: u32, game: &Game) {
    let path = format!("{dir}/{name}_{k:02}.png");
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    let held = game.sim.held.map(|i| (game.sim.props[i].pos, game.sim.props[i].env));
    let pellets: Vec<Vec3> = game.sim.pellets.iter().filter(|p| p.alive).map(|p| p.pos).collect();
    let mouths: Vec<f32> = game.sim.launchers.iter().map(|l| l.since_shot).collect();
    println!("capture: {name}_{k:02}  feet {} vel {} held {held:?} pellets {pellets:?} since_shot {mouths:?}", game.ctrl.feet, game.ctrl.vel);
}

pub fn run(
    mut commands: Commands,
    mut cap: ResMut<Capture>,
    mut game: ResMut<Game>,
    mut exit: MessageWriter<AppExit>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    mut menu: ResMut<crate::menu::Menu>,
    mut editor: ResMut<crate::editor::Editor>,
    mut mouse: MouseHand,
    mut settings: ResMut<crate::settings::Settings>,
) {
    let fps = diagnostics.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
    cap.frame += 1;
    let now = std::time::Instant::now();
    if let Some(last) = cap.last {
        let ms = (now - last).as_secs_f32() * 1000.0;
        cap.real_ms = if cap.real_ms == 0.0 { ms } else { cap.real_ms + (ms - cap.real_ms) * 0.1 };
    }
    cap.last = Some(now);
    if cap.frame < 20 {
        return;
    }
    let acts = script(&cap.script);
    game.scripted = Some(MoveInput::default());
    if let Some(at) = cap.hold {
        game.ctrl.feet = at;
        game.ctrl.vel = Vec3::ZERO;
    }
    if cap.hold_until.is_some_and(|t| std::time::Instant::now() < t) {
        return;
    }
    if cap.wait > 0 {
        cap.wait -= 1;
        match acts.get(cap.step) {
            Some(Act::Hold(i, _)) => game.scripted = Some(*i),
            Some(Act::Film(name, i, n)) => {
                game.scripted = Some(*i);
                film(&mut commands, &cap.dir, name, n - cap.wait - 1, &game);
            }
            Some(Act::WaitPelletX(x, _)) => {
                if game.sim.pellets.iter().any(|p| p.alive && p.pos.x >= *x) {
                    cap.wait = 0;
                }
            }
            Some(Act::WaitPelletNear(at, d, _)) => {
                if game.sim.pellets.iter().any(|p| p.alive && p.pos.distance(*at) <= *d) {
                    cap.wait = 0;
                }
            }
            _ => {}
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
        Act::Film(_, i, n) => {
            game.scripted = Some(*i);
            cap.wait = *n;
            return;
        }
        Act::WaitPelletX(_, n) | Act::WaitPelletNear(_, _, n) => {
            cap.wait = *n;
            return;
        }
        Act::Look(yaw, pitch) => match &mut game.spectate {
            Some(sp) => (sp.yaw, sp.pitch) = (yaw.to_radians(), pitch.to_radians()),
            None => {
                game.ctrl.yaw = yaw.to_radians();
                game.ctrl.pitch = pitch.to_radians();
            }
        },
        Act::Spectate(eye, yaw, pitch) => game.spectate = Some(crate::Spectator::at(*eye, yaw.to_radians(), pitch.to_radians())),
        Act::TrackPellet => {
            if let Some(sp) = &mut game.spectate {
                sp.track_pellet = true;
            }
        }
        Act::Unspectate => game.spectate = None,
        Act::Fire(c) => game.scripted_fire = Some(*c),
        Act::Use => game.scripted_use = true,
        Act::Shot(name) => {
            let path = format!("{}/{name}.png", cap.dir);
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
            let yaws: Vec<f32> = game.sim.props.iter().filter(|p| p.alive).map(|p| p.rot.to_euler(EulerRot::YXZ).0).collect();
            let pellets: Vec<Vec3> = game.sim.pellets.iter().filter(|p| p.alive).map(|p| p.pos).collect();
            println!(
                "capture: {name}  fps {fps:.0} ({:.1} ms real)  feet {} state {} portals {:?} held {:?} props {:?} yaws {yaws:?} pellets {pellets:?}",
                cap.real_ms,
                game.ctrl.feet,
                game.ctrl.state.name(),
                game.sim.portals.iter().map(|p| p.map(|p| p.center)).collect::<Vec<_>>(),
                game.sim.held,
                game.sim.props.iter().filter(|p| p.alive).map(|p| p.pos).collect::<Vec<_>>()
            );
        }
        Act::Map(i) => {
            game.pending = Some(*i);
            // The first load waits for shaders. Later loads, once frames are already quick,
            // only need a moment: a long pause here lets the sim run, and a launcher's mouth
            // has finished opening before the script looks at it.
            let pause = if cap.real_ms > 0.0 && cap.real_ms < 40.0 { 300 } else { 1500 };
            cap.hold_until = Some(std::time::Instant::now() + std::time::Duration::from_millis(pause));
        }
        Act::Chamber(group, map) => match (0..game.levels.len()).find(|&i| game.groups[i] == *group && game.source_names[i].as_deref() == Some(*map)) {
            Some(i) => {
                cap.hold = None;
                game.pending = Some(i);
                let pause = if cap.real_ms > 0.0 && cap.real_ms < 40.0 { 600 } else { 1500 };
                cap.hold_until = Some(std::time::Instant::now() + std::time::Duration::from_millis(pause));
            }
            None => println!("capture: no {map} in {group:?}"),
        },
        Act::Menu(screen) => menu.open_at(*screen),
        Act::Edit(f) => f(&mut editor, &mut menu),
        Act::Mouse(m) => mouse.act(*m, &editor),
        Act::RayTracing(on) => settings.ray_tracing = if *on { 1.0 } else { 0.0 },
        Act::Set(f) => f(&mut settings),
        Act::AtLight(k, of) => {
            let lights = &game.sim.level.lights;
            if !lights.is_empty() {
                let at = lights[(k * lights.len() / of.max(&1)).min(lights.len() - 1)].at;
                game.ctrl.feet = at - Vec3::Y * 2.0;
                game.ctrl.vel = Vec3::ZERO;
                game.ctrl.state = MoveState::Air;
                println!("capture: light {k} at {at}");
            }
        }
        Act::ViewArt(what, n, d) => {
            let found = game.sim.level.art.as_ref().and_then(|art| {
                art.props
                    .iter()
                    .filter(|p| match (&p.kind, *what) {
                        (aperture::art::PropKind::Camera, "camera") | (aperture::art::PropKind::Spotlight { .. }, "spotlight") | (aperture::art::PropKind::Steam { .. }, "steam") | (aperture::art::PropKind::Sparks { .. }, "sparks") | (aperture::art::PropKind::Beam { .. }, "beam") => true,
                        (aperture::art::PropKind::Particles { .. }, w) => p.model.eq_ignore_ascii_case(w),
                        _ => false,
                    })
                    .nth(*n)
                    .map(|p| (p.at, p.turn))
            });
            if let Some((at, turn)) = found {
                // Where there's a clear view of it: round it (the way it faces first), a
                // little below it (or above), nothing in between.
                let fwd = turn * Vec3::NEG_Z;
                let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::Z);
                let clear = |eye: Vec3| {
                    let to = at - eye;
                    let (_, hit) = game.sim.trace(eye, to.normalize_or(Vec3::Z), to.length(), aperture::Mask::SHOT, 0);
                    // (Her body well clear of goo: she sinks a little between frames.)
                    let body = faith_move::Aabb { min: eye - Vec3::new(0.4, 2.2, 0.4), max: eye + Vec3::splat(0.4) };
                    hit.is_none_or(|h| h.t > to.length() - 0.3) && !game.sim.statics.iter().any(|s| aperture::geom::contains(&s.b, eye)) && !game.sim.goo.iter().any(|g| g.overlaps(&body)) && body.min.y > game.sim.level.kill_y
                };
                let eye = (0..8)
                    .map(|k| Quat::from_rotation_y(k as f32 * std::f32::consts::FRAC_PI_4) * flat)
                    .flat_map(|dir| [0.3f32, 0.0, 0.6, -0.3].map(move |down| at + dir * *d - Vec3::Y * (*d * down)))
                    .find(|e| clear(*e))
                    .unwrap_or(at + flat * *d);
                game.ctrl.feet = eye - Vec3::Y * 1.7;
                game.ctrl.vel = Vec3::ZERO;
                game.ctrl.state = MoveState::Air;
                cap.hold = Some(game.ctrl.feet);
                let to = at - eye;
                game.ctrl.yaw = (-to.x).atan2(-to.z);
                game.ctrl.pitch = (to.y / Vec2::new(to.x, to.z).length().max(1e-3)).atan();
                println!("capture: {what} {n} at {at}");
            } else {
                println!("capture: no {what} {n}");
            }
        }
        Act::ViewCube(n) => {
            if let Some(c) = game.sim.props.iter().filter(|p| p.alive).nth(*n).map(|p| p.pos) {
                let side = if game.sim.statics.iter().any(|s| aperture::geom::contains(&s.b, c + Vec3::new(1.6, 0.5, 0.0))) { -1.0 } else { 1.0 };
                // On the floor the cube's on (no landing to dip the view).
                game.ctrl.feet = c + Vec3::new(1.6 * side, -0.44, 0.0);
                game.ctrl.vel = Vec3::ZERO;
                game.ctrl.state = MoveState::Ground;
                game.ctrl.yaw = if side > 0.0 { 90f32.to_radians() } else { -90f32.to_radians() };
                game.ctrl.pitch = -30f32.to_radians();
                println!("capture: cube {n} at {c}");
            }
        }
        Act::SendPellet(at, v) => {
            if let Some(p) = game.sim.pellets.iter_mut().find(|p| p.alive) {
                p.pos = *at;
                p.vel = *v;
                p.age = 0.0;
            }
        }
        Act::Send(class, input, param) => {
            if let Some(mut io) = game.sim.io.take() {
                for e in 0..io.ents.len() {
                    if io.ents[e].class.eq_ignore_ascii_case(class) {
                        io.send(&mut game.sim, e, input, param);
                    }
                }
                game.sim.io = Some(io);
            }
        }
        Act::TurretsOff => {
            for t in &mut game.sim.turrets {
                t.state = aperture::sim::TurretState::Retired;
            }
        }
        Act::RocketsOn => {
            for t in &mut game.sim.rocket_turrets {
                t.inputs = aperture::level::Inputs::always();
            }
        }
        Act::PlaceCube(at) => {
            if let Some(p) = game.sim.props.iter_mut().find(|p| p.alive) {
                p.pos = *at;
                p.vel = Vec3::ZERO;
            }
        }
        Act::Launch(v) => game.ctrl.launch(*v),
        Act::Portal(c, at, n, up) => {
            let half = game.sim.portal_half();
            game.sim.set_portal(*c, Some(aperture::Portal::sized(*at, *n, *up, half)));
        }
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

/// The mouse, worked by a script: the messages the window system would send.
#[derive(bevy::ecs::system::SystemParam)]
pub struct MouseHand<'w, 's> {
    windows: Query<'w, 's, (Entity, &'static mut Window), With<bevy::window::PrimaryWindow>>,
    moved: MessageWriter<'w, bevy::window::CursorMoved>,
    buttons: MessageWriter<'w, bevy::input::mouse::MouseButtonInput>,
    wheel: MessageWriter<'w, bevy::input::mouse::MouseWheel>,
    /// (What egui reads.)
    events: MessageWriter<'w, bevy::window::WindowEvent>,
}

impl MouseHand<'_, '_> {
    fn act(&mut self, m: Hand, editor: &crate::editor::Editor) {
        use bevy::input::mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel};
        use bevy::input::ButtonState;
        let Ok((window, mut w)) = self.windows.single_mut() else { return };
        let to = match m {
            Hand::ToTile(name) => {
                let i = crate::editor::catalog::find(name);
                let Some((_, r)) = editor.tiles.iter().find(|(j, _)| *j == crate::editor::Pick::Entry(i)) else {
                    println!("capture: no tile {name} on screen");
                    return;
                };
                Some(Vec2::new(r.center().x, r.center().y))
            }
            Hand::ToProp(n) => {
                let Some((_, r)) = editor.tiles.iter().filter(|(p, _)| matches!(p, crate::editor::Pick::Prop(_))).nth(n) else {
                    println!("capture: no prop tile {n} on screen");
                    return;
                };
                Some(Vec2::new(r.center().x, r.center().y))
            }
            Hand::ToEnd(axis) => {
                let Some(p) = editor.end_handle_on_screen(&w, axis) else {
                    println!("capture: no end handle");
                    return;
                };
                Some(p)
            }
            Hand::To(x, y) => Some(Vec2::new(x * w.width(), y * w.height())),
            _ => None,
        };
        if let Some(p) = to {
            let delta = w.cursor_position().map(|c| p - c);
            w.set_cursor_position(Some(p));
            let m = bevy::window::CursorMoved { window, position: p, delta };
            self.moved.write(m.clone());
            self.events.write(m.into());
        }
        let button = |state| MouseButtonInput { button: MouseButton::Left, state, window };
        match m {
            Hand::Press | Hand::Release => {
                let b = button(if matches!(m, Hand::Press) { ButtonState::Pressed } else { ButtonState::Released });
                self.buttons.write(b);
                self.events.write(b.into());
            }
            Hand::Scroll(y) => {
                self.wheel.write(MouseWheel { unit: MouseScrollUnit::Line, x: 0.0, y, window, phase: bevy::input::touch::TouchPhase::Moved });
            }
            _ => {}
        }
    }
}

/// A check of which way up Bevy reads a normal map's green (`PORTAL_NORMAL_TEST=1`, with the
/// `normaltest` script): a wall-facing quad, a dome bumped out of it by a normal map whose green
/// points up the image, lit from above. Its top half lit means green is up the image.
pub fn normal_test(mut commands: Commands, mut done: Local<bool>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, mut images: ResMut<Assets<Image>>) {
    if *done || std::env::var_os("PORTAL_NORMAL_TEST").is_none() {
        return;
    }
    *done = true;
    let n = 256u32;
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for row in 0..n {
        for col in 0..n {
            let x = (col as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let y_up = 1.0 - (row as f32 + 0.5) / n as f32 * 2.0;
            let r2 = x * x + y_up * y_up;
            let (nx, ny) = if r2 < 0.64 { (x, y_up) } else { (0.0, 0.0) };
            let nz = (1.0f32 - nx * nx - ny * ny).max(0.0).sqrt();
            px.extend_from_slice(&[((nx * 0.5 + 0.5) * 255.0) as u8, ((ny * 0.5 + 0.5) * 255.0) as u8, ((nz * 0.5 + 0.5) * 255.0) as u8, 255]);
        }
    }
    let img = crate::me_remaster::with_mips(n, n, px, false);
    let mut mesh = Mesh::from(Rectangle::new(4.0, 4.0));
    let _ = mesh.generate_tangents();
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.7, 0.7, 0.7), normal_map_texture: Some(images.add(img)), perceptual_roughness: 0.9, ..default() })),
        Transform::from_xyz(0.0, 3.0, -10.0),
    ));
}
