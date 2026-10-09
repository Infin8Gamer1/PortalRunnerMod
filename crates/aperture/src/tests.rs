//! Faith and the puzzle pieces together, driven by scripted input.

use glam::{Vec2, Vec3};

use faith_move::{Controller, Input, State};

use crate::level::{CubeKind, Inputs, Level, Mat, Paint};
use crate::portal::{Portal, PortalColor};
use crate::sim::{Aim, Sim, SimEvent};

const DT: f32 = 1.0 / 60.0;

fn start(level: Level) -> (Sim, Controller) {
    let cp = level.checkpoints[0].clone();
    let sim = Sim::new(level);
    let mut c = Controller::new(crate::tuning(5.3, 10.0), cp.spawn, cp.yaw);
    c.state = State::Ground;
    (sim, c)
}

fn run(sim: &mut Sim, c: &mut Controller, secs: f32, mut input: impl FnMut(&Sim, &Controller) -> Input) -> Vec<SimEvent> {
    let mut events = Vec::new();
    let mut t = 0.0;
    while t < secs {
        let i = input(sim, c);
        sim.events.clear();
        sim.step_player(c, &i, DT);
        sim.step(DT, c);
        events.extend(sim.events.iter().copied());
        t += DT;
    }
    events
}

fn forward() -> Input {
    Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() }
}

/// A white floor and nothing else, plus whatever the test adds.
fn flat() -> Level {
    let mut l = Level::new("flat");
    l.solid(Mat::White, [-40.0, -2.0, -40.0], [40.0, 0.0, 40.0]);
    l.spawn([0.0, 0.0, 0.0], 0.0);
    l
}

fn wall_portal(x: f32, z: f32, normal: Vec3) -> Portal {
    Portal::new(Vec3::new(x, 1.2, z), normal, Vec3::Y)
}

#[test]
fn walking_through_a_wall_portal() {
    // Two walls: one ahead (facing +Z, at z = -6), one far off to the side facing +X.
    let mut l = flat();
    l.solid(Mat::White, [-5.0, 0.0, -6.5], [5.0, 4.0, -6.0]);
    l.solid(Mat::White, [19.5, 0.0, -10.0], [20.0, 4.0, 10.0]);
    let (mut sim, mut c) = start(l);
    sim.set_portal(PortalColor::Blue, Some(wall_portal(0.0, -6.0, Vec3::Z)));
    sim.set_portal(PortalColor::Orange, Some(wall_portal(20.0, 0.0, Vec3::X)));
    let ev = run(&mut sim, &mut c, 3.0, |_, _| forward());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::Teleported { entered: PortalColor::Blue, .. })), "never went through");
    // Out of orange, walking on along +X, still on her feet.
    assert!(c.feet.x > 21.0, "feet {}", c.feet);
    assert!(c.vel.x > 2.0, "vel {}", c.vel);
    assert!(c.feet.y.abs() < 0.05, "fell? {}", c.feet);
    let d = crate::portal::view_dir(c.yaw, c.pitch);
    assert!(d.x > 0.9, "looking {d}");
}

#[test]
fn a_lone_portal_is_just_a_wall() {
    let mut l = flat();
    l.solid(Mat::White, [-5.0, 0.0, -6.5], [5.0, 4.0, -6.0]);
    let (mut sim, mut c) = start(l);
    sim.set_portal(PortalColor::Blue, Some(wall_portal(0.0, -6.0, Vec3::Z)));
    run(&mut sim, &mut c, 3.0, |_, _| forward());
    assert!(c.feet.z > -6.0, "went into the wall: {}", c.feet);
}

#[test]
fn the_gun_puts_portals_on_white_and_not_on_black() {
    let mut l = flat();
    l.solid(Mat::White, [-5.0, 0.0, -6.5], [5.0, 4.0, -6.0]);
    l.solid(Mat::Black, [-5.0, 0.0, 6.0], [5.0, 4.0, 6.5]);
    let (mut sim, c) = start(l);
    let mut aim = Aim::of(&c);
    aim.dir = Vec3::new(0.0, 0.0, -1.0);
    assert!(sim.fire(PortalColor::Blue, aim));
    let p = sim.portal(PortalColor::Blue).unwrap();
    assert_eq!(p.normal, Vec3::Z);
    // Shot low at the wall: nudged up so it fits above the floor.
    assert!(p.center.y >= p.half.y - 1e-3, "{}", p.center);
    aim.dir = Vec3::Z;
    assert!(!sim.fire(PortalColor::Orange, aim), "black metal took a portal");
    // Not on top of the other portal: it's moved aside.
    aim.dir = Vec3::new(0.0, 0.0, -1.0);
    assert!(sim.fire(PortalColor::Orange, aim));
    let o = sim.portal(PortalColor::Orange).unwrap();
    assert!((o.center.x - p.center.x).abs() >= p.half.x * 2.0 - 1e-3, "{} vs {}", o.center, p.center);
}

#[test]
fn falling_into_a_floor_portal_flings_her_out_of_a_wall() {
    // A tower 8 m up to drop from, a floor portal below it, a wall portal facing +X.
    let mut l = flat();
    l.solid(Mat::Black, [-2.0, 0.0, -2.0], [2.0, 8.0, 2.0]);
    l.solid(Mat::White, [19.5, 0.0, -10.0], [20.0, 6.0, 10.0]);
    l.spawn([0.0, 8.0, 0.0], 0.0);
    let (mut sim, mut c) = start(l);
    sim.set_portal(PortalColor::Blue, Some(Portal::new(Vec3::new(0.0, 0.0, -5.5), Vec3::Y, Vec3::NEG_Z)));
    sim.set_portal(PortalColor::Orange, Some(Portal::new(Vec3::new(20.0, 3.0, 0.0), Vec3::X, Vec3::Y)));
    let mut best_speed = 0.0f32;
    let ev = run(&mut sim, &mut c, 4.0, |sim, c| {
        let _ = sim;
        best_speed = best_speed.max(if c.feet.x > 20.0 { c.vel.x } else { 0.0 });
        // Run off the tower's front.
        if c.feet.z > -2.1 { forward() } else { Input::default() }
    });
    assert!(ev.iter().any(|e| matches!(e, SimEvent::Teleported { .. })), "didn't go through: {}", c.feet);
    assert!(best_speed > 10.0, "flung at {best_speed}");
    assert!(c.feet.x > 28.0, "landed at {}", c.feet);
}

#[test]
fn carrying_a_cube_onto_a_button_opens_the_door() {
    let mut l = flat();
    l.cube(CubeKind::Storage, [0.0, 0.5, -2.0]);
    l.button(0, [0.0, 0.0, -6.0]);
    l.door(Inputs::on(0), [-1.0, 0.0, -12.0], [1.0, 3.0, -11.5]);
    let (mut sim, mut c) = start(l);
    run(&mut sim, &mut c, 0.5, |_, _| Input::default());
    c.pitch = -0.5;
    sim.use_key(Aim::of(&c));
    assert!(sim.held.is_some(), "didn't pick it up");
    // Carry it to the button, drop it there.
    run(&mut sim, &mut c, 4.0, |_, c| if c.feet.z > -4.2 { forward() } else { Input::default() });
    sim.use_key(Aim::of(&c));
    let ev = run(&mut sim, &mut c, 2.0, |_, _| Input::default());
    assert!(sim.buttons[0].pressed, "cube at {}", sim.props[0].pos);
    assert!(ev.iter().any(|e| matches!(e, SimEvent::ButtonDown { .. })));
    assert!(sim.doors[0].open > 0.9);
}

#[test]
fn a_cube_goes_through_portals() {
    let mut l = flat();
    l.cube(CubeKind::Storage, [0.0, 4.0, -5.0]);
    let (mut sim, mut c) = start(l);
    sim.set_portal(PortalColor::Blue, Some(Portal::new(Vec3::new(0.0, 0.0, -5.0), Vec3::Y, Vec3::NEG_Z)));
    sim.set_portal(PortalColor::Orange, Some(Portal::new(Vec3::new(15.0, 0.0, -5.0), Vec3::Y, Vec3::NEG_Z)));
    let ev = run(&mut sim, &mut c, 1.2, |_, _| Input::default());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::PropTeleported { .. })));
    // Out of the other floor portal, up into the air.
    assert!(sim.props[0].pos.x > 14.0, "{}", sim.props[0].pos);
}

#[test]
fn a_reflector_cube_turns_a_laser_into_the_catcher() {
    let mut l = flat();
    l.laser([0.0, 0.45, -10.0], [0.0, 0.0, 1.0], Inputs::always());
    l.catcher(0, [8.0, 0.45, 0.0], [-1.0, 0.0, 0.0]);
    l.door(Inputs::on(0), [-1.0, 0.0, 12.0], [1.0, 3.0, 12.5]);
    l.cube(CubeKind::Reflector, [0.0, 0.7, 0.0]);
    let (mut sim, mut c) = start(l);
    c.feet = Vec3::new(-6.0, 0.0, 6.0);
    // Turn the cube to face the catcher (+X): its front is local -Z.
    sim.props[0].rot = glam::Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
    run(&mut sim, &mut c, 1.0, |_, _| Input::default());
    assert!(sim.catchers[0].lit, "path {:?}", sim.lasers[0].path);
    assert!(sim.doors[0].open > 0.5);
}

#[test]
fn a_light_bridge_through_a_portal() {
    let mut l = flat();
    // A pit in the floor, a wall the bridge hits.
    l.solid(Mat::White, [-10.0, 0.0, -14.5], [10.0, 6.0, -14.0]);
    l.bridge([0.0, 1.0, 4.0], [0.0, 0.0, -1.0], Inputs::always());
    let (mut sim, mut c) = start(l);
    c.feet = Vec3::new(6.0, 0.0, 6.0);
    run(&mut sim, &mut c, 0.1, |_, _| Input::default());
    assert_eq!(sim.bridges[0].segs.len(), 1);
    assert!((sim.bridges[0].segs[0].b.z + 14.0).abs() < 0.05);
    sim.set_portal(PortalColor::Blue, Some(Portal::new(Vec3::new(0.0, 1.5, -14.0), Vec3::Z, Vec3::Y)));
    sim.set_portal(PortalColor::Orange, Some(Portal::new(Vec3::new(-30.0, 0.0, 0.0), Vec3::Y, Vec3::NEG_Z)));
    run(&mut sim, &mut c, 0.1, |_, _| Input::default());
    // Into the wall portal, up out of the floor one: a wall of light.
    assert_eq!(sim.bridges[0].segs.len(), 2, "{:?}", sim.bridges[0].segs);
    let s = sim.bridges[0].segs[1];
    assert!(s.b.y > s.a.y + 5.0, "{s:?}");
}

#[test]
fn repulsion_gel_bounces_and_propulsion_gel_is_fast() {
    let mut l = flat();
    l.paint(Paint::Bounce, [-2.0, 0.0, -8.0], [2.0, 0.0, -4.0], [0.0, 1.0, 0.0]);
    l.paint(Paint::Speed, [10.0, 0.0, -40.0], [14.0, 0.0, 10.0], [0.0, 1.0, 0.0]);
    let (mut sim, mut c) = start(l);
    c.feet = Vec3::new(0.0, 3.0, -6.0);
    c.state = State::Air;
    let ev = run(&mut sim, &mut c, 0.8, |_, _| Input::default());
    let _ = ev;
    assert!(c.vel.y > 0.0 || c.feet.y > 1.0, "didn't bounce: {} {}", c.feet, c.vel);
    // On propulsion gel she runs far past her sprint speed.
    c.feet = Vec3::new(12.0, 0.0, 8.0);
    c.vel = Vec3::ZERO;
    c.state = State::Ground;
    c.yaw = 0.0;
    let mut top = 0.0f32;
    run(&mut sim, &mut c, 2.5, |_, c| {
        top = top.max(c.horizontal_speed());
        forward()
    });
    assert!(top > 12.0, "top speed {top}");
}

#[test]
fn conversion_gel_takes_a_portal_until_it_is_washed_off() {
    let mut l = flat();
    l.solid(Mat::Black, [-5.0, 0.0, -6.5], [5.0, 4.0, -6.0]);
    let (mut sim, mut c) = start(l);
    let mut aim = Aim::of(&c);
    aim.dir = Vec3::new(0.0, 0.0, -1.0);
    assert!(!sim.fire(PortalColor::Blue, aim), "black metal took a portal");
    sim.gel.add(crate::gel::Splat { p: Vec3::new(0.0, 1.6, -6.0), n: Vec3::Z, r: 1.8, paint: Paint::Portal });
    assert!(sim.fire(PortalColor::Blue, aim), "conversion gel didn't take a portal");
    run(&mut sim, &mut c, 0.2, |_, _| Input::default());
    assert!(sim.portal(PortalColor::Blue).is_some(), "fizzled on its own");
    // Other gel painted beside it doesn't matter.
    sim.gel.add(crate::gel::Splat { p: Vec3::new(4.0, 0.5, -6.0), n: Vec3::Z, r: 0.4, paint: Paint::Bounce });
    run(&mut sim, &mut c, 0.2, |_, _| Input::default());
    assert!(sim.portal(PortalColor::Blue).is_some());
    // Washing the gel under it closes it.
    sim.gel.add(crate::gel::Splat { p: Vec3::new(0.0, 1.0, -6.0), n: Vec3::Z, r: 0.5, paint: Paint::Water });
    let ev = run(&mut sim, &mut c, 0.2, |_, _| Input::default());
    assert!(sim.portal(PortalColor::Blue).is_none(), "still open on bare black metal");
    assert!(ev.iter().any(|e| matches!(e, SimEvent::PortalFizzled { color: PortalColor::Blue, .. })));
}

#[test]
fn an_emancipation_grill_clears_portals_and_vaporises_cubes() {
    let mut l = flat();
    l.fizzler([-2.0, 0.0, -4.1], [2.0, 3.0, -3.9], Inputs::always());
    l.solid(Mat::White, [-5.0, 0.0, -20.5], [5.0, 4.0, -20.0]);
    l.cube(CubeKind::Storage, [0.0, 0.5, -1.5]);
    let (mut sim, mut c) = start(l);
    sim.set_portal(PortalColor::Blue, Some(wall_portal(0.0, -20.0, Vec3::Z)));
    run(&mut sim, &mut c, 0.3, |_, _| Input::default());
    c.pitch = -0.6;
    sim.use_key(Aim::of(&c));
    assert!(sim.held.is_some());
    let ev = run(&mut sim, &mut c, 2.5, |_, _| forward());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::CubeFizzled { .. })));
    assert!(ev.iter().any(|e| matches!(e, SimEvent::PortalsCleared)));
    assert!(sim.portal(PortalColor::Blue).is_none());
}

#[test]
fn a_faith_plate_throws_her_onto_its_target() {
    let mut l = flat();
    l.solid(Mat::Black, [-3.0, 0.0, -27.0], [3.0, 5.0, -18.0]);
    l.faith_plate([0.0, 0.0, -2.0], [0.0, 5.0, -21.0], 3.0);
    let (mut sim, mut c) = start(l);
    let ev = run(&mut sim, &mut c, 3.0, |_, c| {
        if c.feet.z > -1.6 { forward() } else { Input::default() }
    });
    assert!(ev.iter().any(|e| matches!(e, SimEvent::PlateLaunch { .. })));
    assert!((c.feet.y - 5.0).abs() < 0.1 && c.feet.z < -18.5, "landed at {}", c.feet);
}

#[test]
fn every_chamber_builds() {
    for (name, build) in crate::levels::all() {
        let l = build();
        assert!(!l.checkpoints.is_empty(), "{name} has no spawn");
        let sim = Sim::new(l);
        // Nobody starts inside a wall.
        let cp = &sim.level.checkpoints[0];
        let c = Controller::new(crate::tuning(5.3, 10.0), cp.spawn, cp.yaw);
        use faith_move::World;
        assert!(!sim.player_world().overlaps(&c.body().aabb(c.feet + Vec3::Y * 0.02)), "{name}: spawn inside a wall at {}", cp.spawn);
    }
}

#[test]
fn walking_onto_a_floor_portal_drops_her_in() {
    let mut l = flat();
    l.solid(Mat::White, [19.5, 0.0, -10.0], [20.0, 6.0, 10.0]);
    let (mut sim, mut c) = start(l);
    sim.set_portal(PortalColor::Blue, Some(Portal::new(Vec3::new(0.0, 0.0, -4.0), Vec3::Y, Vec3::NEG_Z)));
    sim.set_portal(PortalColor::Orange, Some(Portal::new(Vec3::new(20.0, 3.0, 0.0), Vec3::X, Vec3::Y)));
    let mut log = Vec::new();
    let ev = run(&mut sim, &mut c, 2.0, |sim, c| {
        log.push(format!("{} {} {:?} env {:?}", c.state.name(), c.feet, c.vel, sim.player_env));
        forward()
    });
    assert!(ev.iter().any(|e| matches!(e, SimEvent::Teleported { .. })), "walked over it:\n{}", log.iter().step_by(6).cloned().collect::<Vec<_>>().join("\n"));
}

// ------------------------------------------------------------------ solving the chambers

/// Turn her to look at `target`.
fn aim(c: &mut Controller, target: Vec3) {
    let (yaw, pitch) = crate::portal::yaw_pitch(target - c.view().eye);
    c.yaw = yaw;
    c.pitch = pitch;
}

/// Steer: the yaw that faces `target` from her feet.
fn face(c: &mut Controller, target: Vec3) {
    let d = target - c.feet;
    c.yaw = (-d.x).atan2(-d.z);
}

fn walk(sim: &mut Sim, c: &mut Controller, target: Vec3, secs: f32) -> Vec<SimEvent> {
    let mut events = Vec::new();
    let mut t = 0.0;
    let mut log = std::collections::VecDeque::new();
    while t < secs {
        log.push_back(format!("{} {} {}", c.state.name(), c.feet, c.vel));
        if log.len() > 30 {
            log.pop_front();
        }
        face(c, target);
        let d = Vec3::new(target.x - c.feet.x, 0.0, target.z - c.feet.z);
        if d.length() < 0.35 {
            break;
        }
        let i = Input { move_axis: Vec2::new(0.0, if d.length() < 1.5 { 0.5 } else { 1.0 }), ..Default::default() };
        sim.events.clear();
        sim.step_player(c, &i, DT);
        sim.step(DT, c);
        events.extend(sim.events.iter().copied());
        t += DT;
    }
    let d = Vec3::new(target.x - c.feet.x, 0.0, target.z - c.feet.z);
    assert!(d.length() < 0.6, "didn't reach {target}: at {} ({})\n{}", c.feet, c.state.name(), log.iter().step_by(3).cloned().collect::<Vec<_>>().join("\n"));
    events
}

fn settle(sim: &mut Sim, c: &mut Controller, secs: f32) -> Vec<SimEvent> {
    run(sim, c, secs, |_, _| Input::default())
}

/// Walk forward (the way she faces) until she goes through a portal.
fn walk_through(sim: &mut Sim, c: &mut Controller, secs: f32) {
    let mut through = false;
    let ev = run(sim, c, secs, |sim, _| {
        through |= sim.events.iter().any(|e| matches!(e, SimEvent::Teleported { .. }));
        if through { Input::default() } else { forward() }
    });
    assert!(ev.iter().any(|e| matches!(e, SimEvent::Teleported { .. })), "didn't go through a portal: at {} ({})", c.feet, c.state.name());
}

#[test]
fn chamber01_can_be_solved() {
    use crate::levels::c01_portals::*;
    let (mut sim, mut c) = start(crate::levels::chamber01());
    walk(&mut sim, &mut c, Vec3::new(5.0, 0.0, -6.0), 6.0);
    settle(&mut sim, &mut c, 0.3);
    // Blue high on the wall above the ledge, orange down here on the right wall.
    aim(&mut c, Vec3::new(-7.0, 8.6, -12.5));
    assert!(sim.fire(PortalColor::Blue, Aim::of(&c)), "blue didn't open");
    let blue = sim.portal(PortalColor::Blue).unwrap();
    assert!(blue.center.y - blue.half.y >= LEDGE_TOP - 0.01, "blue below the ledge top: {}", blue.center);
    aim(&mut c, Vec3::new(7.0, 1.0, -6.0));
    assert!(sim.fire(PortalColor::Orange, Aim::of(&c)), "orange didn't open");
    // Through orange, out of blue onto the ledge.
    aim(&mut c, Vec3::new(7.0, 1.66, -6.0));
    c.pitch = 0.0;
    walk_through(&mut sim, &mut c, 3.0);
    settle(&mut sim, &mut c, 1.5);
    assert!((c.feet.y - LEDGE_TOP).abs() < 0.05, "not on the ledge: {}", c.feet);
    // Pick up the cube.
    aim(&mut c, glam::Vec3::from(CUBE));
    sim.use_key(Aim::of(&c));
    assert!(sim.held.is_some(), "no cube: at {}, cube {}", c.feet, sim.props[0].pos);
    // Blue again, low on the wall behind her, and back through it with the cube.
    let z = c.feet.z;
    aim(&mut c, Vec3::new(-7.0, LEDGE_TOP + 0.6, z));
    assert!(sim.fire(PortalColor::Blue, Aim::of(&c)), "low blue didn't open");
    c.pitch = 0.0;
    walk_through(&mut sim, &mut c, 3.0);
    settle(&mut sim, &mut c, 0.8);
    assert!(c.feet.y.abs() < 0.05 && c.feet.x > 4.0, "not back down: {}", c.feet);
    assert!(sim.held.is_some(), "lost the cube on the way");
    // Onto the button.
    walk(&mut sim, &mut c, Vec3::new(BUTTON[0], 0.0, BUTTON[2] + 1.9), 6.0);
    aim(&mut c, Vec3::new(BUTTON[0], 0.4, BUTTON[2]));
    settle(&mut sim, &mut c, 0.6);
    sim.use_key(Aim::of(&c));
    let ev = settle(&mut sim, &mut c, 1.5);
    assert!(sim.buttons[0].pressed, "cube at {}", sim.props[0].pos);
    assert!(ev.iter().any(|e| matches!(e, SimEvent::DoorOpening { .. })) || sim.doors[0].open > 0.9);
    // Out through the door, the grill, to the exit.
    walk(&mut sim, &mut c, Vec3::new(0.0, 0.0, -16.0), 6.0);
    let ev = walk(&mut sim, &mut c, Vec3::new(0.0, 0.0, -25.0), 6.0);
    assert!(ev.iter().any(|e| matches!(e, SimEvent::PortalsCleared)), "the grill didn't clear the portals");
    walk(&mut sim, &mut c, Vec3::new(0.0, 0.0, -30.0), 6.0);
    assert!(sim.complete, "not finished at {}", c.feet);
}

#[test]
fn chamber02_can_be_solved() {
    use crate::levels::c02_momentum::*;
    let (mut sim, mut c) = start(crate::levels::chamber02());
    // Blue on the floor below.
    walk(&mut sim, &mut c, Vec3::new(0.0, TOWER_TOP, 0.1), 4.0);
    aim(&mut c, Vec3::new(0.0, 0.0, -6.0));
    let a = Aim::of(&c);
    let tr = sim.trace(a.eye, a.dir, 200.0, crate::sim::Mask::SHOT, 4);
    assert!(sim.fire(PortalColor::Blue, Aim::of(&c)), "no blue: feet {} trace {:?}", c.feet, tr);
    assert!(!sim.fire(PortalColor::Orange, Aim::of(&c)), "orange fired with a blue-only gun");
    // Off the edge.
    c.pitch = -0.3;
    let mut log = Vec::new();
    let ev = run(&mut sim, &mut c, 3.5, |sim, c| {
        log.push(format!("{} {} {}", c.state.name(), c.feet, c.vel));
        if c.feet.z > -0.5 && c.feet.y > TOWER_TOP - 0.5 && sim.player_env.is_none() { forward() } else { Input::default() }
    });
    let died = ev.iter().any(|e| matches!(e, SimEvent::PlayerDied));
    assert!(!died && ev.iter().any(|e| matches!(e, SimEvent::Teleported { .. })), "no fling:\n{}", log.iter().step_by(5).cloned().collect::<Vec<_>>().join("\n"));
    assert!((c.feet.y - LANDING_TOP).abs() < 0.05 && c.feet.z < -14.0 && c.feet.z > -35.0, "landed at {}", c.feet);
    // The wallrun across the last gap.
    walk(&mut sim, &mut c, Vec3::new(WALL_X - 0.5, LANDING_TOP, -17.0), 8.0);
    c.yaw = 0.0;
    c.pitch = 0.0;
    let mut jumped = false;
    let mut log = Vec::new();
    run(&mut sim, &mut c, 5.0, |_, c| {
        log.push(format!("{} {} {}", c.state.name(), c.feet, c.vel));
        let mut i = forward();
        if !jumped && c.feet.z < -34.3 {
            jumped = true;
            i.jump_pressed = true;
        }
        if jumped {
            // A little towards the wall (ME finds no wall dead parallel to it).
            i.move_axis.x = 0.4;
        }
        if c.feet.z < -46.0 {
            i.move_axis = Vec2::ZERO;
        }
        i
    });
    assert!((c.feet.y - LANDING_TOP).abs() < 0.05 && c.feet.z < -44.0, "didn't make the gap: {}\n{}", c.feet, log.iter().skip(118).step_by(3).cloned().collect::<Vec<_>>().join("\n"));
    walk(&mut sim, &mut c, Vec3::new(0.0, LANDING_TOP, -48.0), 4.0);
    walk(&mut sim, &mut c, Vec3::new(0.0, LANDING_TOP, -54.0), 4.0);
    assert!(sim.complete);
}

#[test]
fn chamber03_can_be_solved() {
    use crate::levels::c03_hard_light::*;
    let (mut sim, mut c) = start(crate::levels::chamber03());
    settle(&mut sim, &mut c, 0.2);
    assert_eq!(sim.bridges[0].segs.len(), 1);
    // Blue where the bridge meets the right wall, orange on the wall behind the start.
    walk(&mut sim, &mut c, Vec3::new(6.0, 0.0, 1.0), 4.0);
    aim(&mut c, Vec3::new(10.0, 1.0, BRIDGE_Z));
    assert!(sim.fire(PortalColor::Blue, Aim::of(&c)), "no blue");
    aim(&mut c, Vec3::new(-2.0, 1.2, 6.0));
    assert!(sim.fire(PortalColor::Orange, Aim::of(&c)), "no orange");
    settle(&mut sim, &mut c, 0.2);
    let segs = &sim.bridges[0].segs;
    assert_eq!(segs.len(), 2, "the bridge didn't go through: {segs:?}");
    assert!(segs[1].b.z < -30.0 && (segs[1].a.y - BRIDGE_Y).abs() < 0.05, "{:?}", segs[1]);
    // Onto the bridge from the side, and across.
    walk(&mut sim, &mut c, Vec3::new(0.5, 0.0, 2.5), 4.0);
    walk(&mut sim, &mut c, Vec3::new(-2.0, BRIDGE_Y, 2.5), 3.0);
    assert!((c.feet.y - BRIDGE_Y).abs() < 0.05, "not on the bridge: {}", c.feet);
    walk(&mut sim, &mut c, Vec3::new(-2.0, BRIDGE_Y, -21.0), 8.0);
    // The faith plate up to the balcony.
    walk(&mut sim, &mut c, Vec3::new(PLATE[0] - 1.4, 0.0, PLATE[2]), 4.0);
    face(&mut c, glam::Vec3::from(PLATE));
    let mut launched = false;
    let ev = run(&mut sim, &mut c, 3.0, |sim, _| {
        launched |= sim.events.iter().any(|e| matches!(e, SimEvent::PlateLaunch { .. }));
        if launched { Input::default() } else { Input { move_axis: Vec2::new(0.0, 0.5), ..Default::default() } }
    });
    assert!(ev.iter().any(|e| matches!(e, SimEvent::PlateLaunch { .. })), "no launch");
    assert!((c.feet.y - BALCONY_TOP).abs() < 0.05, "not on the balcony: {}", c.feet);
    aim(&mut c, glam::Vec3::from(CUBE));
    sim.use_key(Aim::of(&c));
    assert!(sim.held.is_some(), "no cube");
    // Down, and into the laser facing the catcher.
    walk(&mut sim, &mut c, Vec3::new(-4.0, 0.0, -22.3), 5.0);
    settle(&mut sim, &mut c, 0.5);
    walk(&mut sim, &mut c, Vec3::new(-4.0, 0.0, -22.3), 2.0);
    c.yaw = 0.0;
    aim(&mut c, Vec3::new(-4.0, 0.45, -24.0));
    c.yaw = 0.0;
    settle(&mut sim, &mut c, 0.8);
    sim.use_key(Aim::of(&c));
    settle(&mut sim, &mut c, 1.5);
    assert!(sim.catchers[0].lit, "catcher dark: cube {} path {:?}", sim.props[0].pos, sim.lasers[0].path);
    assert!(sim.doors[0].open > 0.9);
    // Round the faith plate (or it throws her back up), then over the laser to the door.
    walk(&mut sim, &mut c, Vec3::new(-1.0, 0.0, -22.9), 4.0);
    walk(&mut sim, &mut c, Vec3::new(5.5, 0.0, -22.9), 4.0);
    c.yaw = 0.0;
    let mut jumped = false;
    run(&mut sim, &mut c, 1.5, |_, c| {
        let mut i = forward();
        if !jumped && c.feet.z < -23.0 {
            jumped = true;
            i.jump_pressed = true;
        }
        if c.feet.z < -27.0 {
            i.move_axis = Vec2::ZERO;
        }
        i
    });
    assert!(sim.burn < 0.5, "burned crossing the laser: {}", sim.burn);
    walk(&mut sim, &mut c, Vec3::new(5.5, 0.0, -30.0), 4.0);
    walk(&mut sim, &mut c, Vec3::new(5.5, 0.0, -36.0), 4.0);
    assert!(sim.complete, "not out: {}", c.feet);
}

#[test]
fn chamber04_can_be_solved() {
    use crate::levels::c04_gel::*;
    let (mut sim, mut c) = start(crate::levels::chamber04());
    // The orange runway, and the jump at its end.
    let mut top = 0.0f32;
    let mut jumped = false;
    let mut log = Vec::new();
    run(&mut sim, &mut c, 6.0, |_, c| {
        top = top.max(c.horizontal_speed());
        log.push(format!("{} {} {:.1}", c.state.name(), c.feet, c.horizontal_speed()));
        let mut i = forward();
        if !jumped && c.feet.z < GAP.1 + 0.4 {
            jumped = true;
            i.jump_pressed = true;
        }
        if jumped && c.state == State::Ground {
            i.move_axis = Vec2::ZERO;
        }
        i
    });
    assert!(top > 14.0, "top speed {top}");
    assert!(c.feet.z < GAP.0 && c.feet.y.abs() < 0.05, "didn't clear the gap: {} (top {top})\n{}", c.feet, log.iter().skip(150).step_by(6).cloned().collect::<Vec<_>>().join("\n"));
    // Up the wall off the blue gel.
    walk(&mut sim, &mut c, Vec3::new(0.0, 0.0, WALL_Z + 1.2), 6.0);
    c.yaw = 0.0;
    c.pitch = 0.0;
    let mut jumped = false;
    let mut log = Vec::new();
    run(&mut sim, &mut c, 4.0, |_, c| {
        log.push(format!("{} {} {}", c.state.name(), c.feet, c.vel));
        let mut i = forward();
        if !jumped {
            jumped = true;
            i.jump_pressed = true;
        }
        if c.feet.y > UPPER - 0.1 && c.feet.z < WALL_Z - 1.0 {
            i.move_axis = Vec2::ZERO;
        }
        i
    });
    assert!((c.feet.y - UPPER).abs() < 0.05, "not up the wall: {}\n{}", c.feet, log.iter().step_by(4).cloned().collect::<Vec<_>>().join("\n"));
    // Portals: the white patch across the goo, the white panel here.
    walk(&mut sim, &mut c, Vec3::new(4.0, UPPER, -58.0), 4.0);
    aim(&mut c, glam::Vec3::from(PATCH));
    assert!(sim.fire(PortalColor::Blue, Aim::of(&c)), "no portal on the white gel");
    aim(&mut c, glam::Vec3::from(PANEL));
    assert!(sim.fire(PortalColor::Orange, Aim::of(&c)), "no portal on the panel");
    c.pitch = 0.0;
    walk_through(&mut sim, &mut c, 3.0);
    let ev = settle(&mut sim, &mut c, 2.0);
    assert!(!ev.iter().any(|e| matches!(e, SimEvent::Teleported { .. })), "fell back in");
    assert!((c.feet.y - UPPER).abs() < 0.05 && c.feet.z < FAR_Z, "not across: {}", c.feet);
    // Round the floor portal (or back through it she goes).
    let z = c.feet.z.min(-73.0);
    walk(&mut sim, &mut c, Vec3::new(3.0, UPPER, z), 4.0);
    walk(&mut sim, &mut c, Vec3::new(3.0, UPPER, -79.0), 4.0);
    walk(&mut sim, &mut c, Vec3::new(0.0, UPPER, -79.0), 4.0);
    walk(&mut sim, &mut c, Vec3::new(0.0, UPPER, -84.0), 4.0);
    assert!(sim.complete);
}

#[test]
fn chamber05_can_be_solved() {
    use crate::levels::c05_rooftops::*;
    let (mut sim, mut c) = start(crate::levels::chamber05());
    // Round the AC unit, crouch under the pipe, and wallrun the billboard over gap 1.
    walk(&mut sim, &mut c, Vec3::new(RUN_WALL_X - 0.5, 0.0, -2.0), 4.0);
    c.yaw = 0.0;
    c.pitch = 0.0;
    let mut jumped = false;
    let mut log = Vec::new();
    run(&mut sim, &mut c, 9.0, |_, c| {
        log.push(format!("{} {} {}", c.state.name(), c.feet, c.vel));
        let mut i = forward();
        // Under the pipe: crouched (a slide at speed).
        let under = c.feet.z < -11.5 && c.feet.z > -15.0;
        i.crouch_held = under;
        i.crouch_pressed = under && c.feet.z > -11.7;
        if !jumped && c.feet.z < -23.6 {
            jumped = true;
            i.jump_pressed = true;
        }
        if jumped {
            i.move_axis.x = 0.4;
        }
        if c.feet.z < -38.0 {
            i.move_axis = Vec2::ZERO;
        }
        i
    });
    assert!((c.feet.y - ROOF_B).abs() < 0.05 && c.feet.z < -34.0, "didn't make gap 1: {}\n{}", c.feet, log.iter().skip(200).step_by(4).cloned().collect::<Vec<_>>().join("\n"));
    // Blue on the billboard's white back.
    walk(&mut sim, &mut c, Vec3::new(5.0, ROOF_B, -52.0), 6.0);
    walk(&mut sim, &mut c, Vec3::new(5.0, ROOF_B, -59.5), 6.0);
    walk(&mut sim, &mut c, Vec3::new(0.0, ROOF_B, -60.5), 6.0);
    aim(&mut c, Vec3::new(0.0, 4.0, BILLBOARD_Z));
    assert!(sim.fire(PortalColor::Blue, Aim::of(&c)), "no blue on the billboard");
    let blue = sim.portal(PortalColor::Blue).unwrap();
    assert_eq!(blue.normal, Vec3::NEG_Z);
    // Up the water tower.
    walk(&mut sim, &mut c, Vec3::new(5.0, ROOF_B, -59.5), 6.0);
    walk(&mut sim, &mut c, Vec3::new(5.0, ROOF_B, -52.0), 6.0);
    walk(&mut sim, &mut c, Vec3::new(-4.0, ROOF_B, -41.0), 6.0);
    c.yaw = 0.0; // facing -Z, at the ladder
    c.pitch = 0.0;
    run(&mut sim, &mut c, 12.0, |_, c| if c.feet.y > TOWER_TOP - 0.1 && c.state == State::Ground { Input::default() } else { forward() });
    assert!((c.feet.y - TOWER_TOP).abs() < 0.05, "not up the tower: {} ({})", c.feet, c.state.name());
    // Orange on the patch below, and off the edge into it.
    walk(&mut sim, &mut c, Vec3::new(-2.2, TOWER_TOP, -46.0), 4.0);
    aim(&mut c, Vec3::new(1.6, ROOF_B, -46.0));
    assert!(sim.fire(PortalColor::Orange, Aim::of(&c)), "no orange on the patch");
    c.yaw = -std::f32::consts::FRAC_PI_2; // facing +X, out over the edge
    c.pitch = -0.3;
    let mut log = Vec::new();
    let ev = run(&mut sim, &mut c, 5.0, |sim, c| {
        log.push(format!("{} {} {} env {:?}", c.state.name(), c.feet, c.vel, sim.player_env));
        let mut i = Input::default();
        if c.feet.y > TOWER_TOP - 0.2 && c.feet.x < -1.9 && c.state == State::Ground {
            i = forward();
        }
        // Roll the landing: crouch as the roof comes up.
        if c.state == State::Air && c.vel.y < 0.0 && c.feet.y < ROOF_C + 1.2 && c.feet.z < -68.0 {
            i.crouch_pressed = true;
            i.crouch_held = true;
        }
        i
    });
    let died = ev.iter().any(|e| matches!(e, SimEvent::PlayerDied));
    assert!(!died && (c.feet.y - ROOF_C).abs() < 0.05 && c.feet.z < -72.0, "no fling to roof C: {} died {died}\n{}", c.feet, log.iter().step_by(6).cloned().collect::<Vec<_>>().join("\n"));
    walk(&mut sim, &mut c, Vec3::new(0.0, ROOF_C, -98.0), 8.0);
    assert!(sim.complete);
}

#[test]
fn mirrors_edge_roofs_take_portals() {
    let (mut sim, mut c) = start(Level::from_greybox(&faith_move::rooftops::rooftops()));
    settle(&mut sim, &mut c, 0.3);
    // Straight down at the roof she stands on.
    c.pitch = -1.4;
    assert!(sim.fire(PortalColor::Blue, Aim::of(&c)), "no portal on the roof");
    assert_eq!(sim.portal(PortalColor::Blue).unwrap().normal, Vec3::Y);
}
