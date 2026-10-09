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
