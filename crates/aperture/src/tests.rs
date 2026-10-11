//! Faith and the puzzle pieces together, driven by scripted input.

use glam::{EulerRot, Quat, Vec2, Vec3};

use faith_move::{Aabb, Controller, Input, State};

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

/// Holding a cube, walk through a wall portal (blue at z = -6 facing +Z, orange at x = 20
/// facing +X) with `input`, starting at `feet` looking along `yaw`: the cube comes too,
/// smoothly, and is still held, near her, on her side at the end.
fn carry_through(feet: Vec3, yaw: f32, input: Input, secs: f32) {
    let mut l = flat();
    l.solid(Mat::White, [-5.0, 0.0, -6.5], [5.0, 4.0, -6.0]);
    l.solid(Mat::White, [19.5, 0.0, -10.0], [20.0, 4.0, 10.0]);
    let fwd = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    l.cube(CubeKind::Storage, (feet + fwd * 1.6 + Vec3::Y * 0.45).to_array());
    let (mut sim, mut c) = start(l);
    sim.set_portal(PortalColor::Blue, Some(wall_portal(0.0, -6.0, Vec3::Z)));
    sim.set_portal(PortalColor::Orange, Some(wall_portal(20.0, 0.0, Vec3::X)));
    c.feet = feet;
    c.yaw = yaw;
    run(&mut sim, &mut c, 0.3, |_, _| Input::default());
    c.pitch = -0.45;
    sim.use_key(Aim::of(&c));
    assert!(sim.held.is_some(), "didn't pick it up");
    c.pitch = -0.2;
    run(&mut sim, &mut c, 0.5, |_, _| Input::default());
    let mut last = sim.props[0].pos;
    let mut worst = 0.0f32;
    let mut dropped = false;
    let mut through = false;
    run(&mut sim, &mut c, secs, |sim, _| {
        let p = sim.props[0].pos;
        let tele = sim.events.iter().any(|e| matches!(e, SimEvent::PropTeleported { .. }));
        through |= tele;
        dropped |= sim.events.iter().any(|e| matches!(e, SimEvent::Drop { .. }));
        if !tele {
            worst = worst.max(p.distance(last));
        }
        last = p;
        input
    });
    assert!(c.feet.x > 20.5, "she didn't come out of orange: {}", c.feet);
    assert!(through, "the cube never went through");
    assert!(!dropped && sim.held.is_some(), "dropped it");
    assert_eq!(sim.held_through, None, "still through a portal at the end");
    let eye = Aim::of(&c).eye;
    assert!(sim.props[0].pos.distance(eye) < 2.6, "cube {} far from her eye {}", sim.props[0].pos, eye);
    // No frame jumps more than a fast-moving cube covers (25 m/s * 1/60 s, and a bit).
    assert!(worst < 0.5, "the cube jumped {worst} m in one frame");
}

#[test]
fn a_cube_held_through_a_portal_then_walked_after() {
    // Push the cube through blue ahead of her, then follow it.
    carry_through(Vec3::new(0.0, 0.0, -3.0), 0.0, Input { move_axis: Vec2::new(0.0, 0.35), ..Default::default() }, 6.0);
}

#[test]
fn carrying_a_cube_forwards_through_a_portal() {
    carry_through(Vec3::new(0.0, 0.0, -2.0), 0.0, forward(), 2.5);
}

#[test]
fn carrying_a_cube_backwards_through_a_portal() {
    // Facing away from the portal, the cube in front of her, walking backwards into it.
    let back = Input { move_axis: Vec2::new(0.0, -1.0), ..Default::default() };
    carry_through(Vec3::new(0.0, 0.0, -2.5), std::f32::consts::PI, back, 3.5);
}

#[test]
fn carrying_a_cube_sideways_through_a_portal() {
    // Facing along the wall (-X), strafing right (-Z) into the portal.
    let right = Input { move_axis: Vec2::new(1.0, 0.0), ..Default::default() };
    carry_through(Vec3::new(0.0, 0.0, -2.5), std::f32::consts::FRAC_PI_2, right, 3.0);
}

#[test]
fn carrying_a_cube_at_an_angle_through_a_portal() {
    // Looking 45 degrees off, walking straight at it.
    let s = std::f32::consts::FRAC_1_SQRT_2;
    carry_through(Vec3::new(0.0, 0.0, -1.5), std::f32::consts::FRAC_PI_4, Input { move_axis: Vec2::new(s, s), ..Default::default() }, 2.5);
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

#[test]
fn landing_faster_than_she_can_run_skids() {
    let (mut sim, mut c) = start(flat());
    c.feet = Vec3::new(0.0, 0.6, 14.0);
    c.state = State::Air;
    c.launch(Vec3::new(0.0, 1.0, -18.0));
    let top = c.tuning.loco.ground_speed;
    let mut skid_frames = 0;
    let ev = run(&mut sim, &mut c, 1.5, |_, c| {
        if c.skidding {
            skid_frames += 1;
            assert!(c.horizontal_speed() > top, "skidding at {}", c.horizontal_speed());
        }
        forward()
    });
    let _ = ev;
    assert!(skid_frames > 10, "skidded for {skid_frames} frames");
    assert!(!c.skidding);
    assert!((c.horizontal_speed() - top).abs() < 0.3, "ran on at {}", c.horizontal_speed());
}

#[test]
fn speed_gel_doesnt_skid_and_leaving_it_waits_before_skidding() {
    let mut l = flat();
    l.paint(Paint::Speed, [-2.0, 0.0, -10.0], [2.0, 0.0, 30.0], [0.0, 1.0, 0.0]);
    let (mut sim, mut c) = start(l);
    c.feet = Vec3::new(0.0, 0.0, 28.0);
    c.vel = Vec3::new(0.0, 0.0, -17.0);
    let delay = c.tuning.skid_after_speed_gel;
    let (mut t, mut left_at, mut skid_at) = (0.0f32, None, None);
    run(&mut sim, &mut c, 4.5, |_, c| {
        t += DT;
        if std::env::var_os("TRACE").is_some() && (t * 60.0).round() as i32 % 6 == 0 {
            eprintln!("{t:.2} z {:.1} speed {:.2} state {} since {:.2} skid {}", c.feet.z, c.horizontal_speed(), c.state.name(), c.since_speed_gel, c.skidding);
        }
        if c.feet.z > -10.0 {
            assert!(!c.skidding, "skidding on the gel at {t}");
        } else if left_at.is_none() {
            left_at = Some(t);
        }
        if c.skidding && skid_at.is_none() {
            skid_at = Some(t);
            // Kept most of the gel's speed until now.
            assert!(c.horizontal_speed() > 17.0 - c.tuning.gel_exit_decel * (delay + 0.2) - 0.5, "{}", c.horizontal_speed());
        }
        forward()
    });
    let (left_at, skid_at) = (left_at.expect("never left the gel"), skid_at.expect("never skidded"));
    let waited = skid_at - left_at;
    assert!((waited - delay).abs() < 0.1, "skidded {waited} s after leaving the gel");
}

/// Portal's maps, where they've been extracted (`assets/extracted/maps`): each starts with
/// Faith standing (not falling, not stuck), and walking about doesn't drop her out of the map.
#[test]
fn portals_maps_stand_her_up() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let Ok(read) = std::fs::read_dir(&dir) else { return };
    let mut names: Vec<String> = read.filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".solids").map(str::to_string)).collect();
    names.sort();
    let mut bad = Vec::new();
    for name in &names {
        let text = |ext: &str| std::fs::read_to_string(dir.join(format!("{name}.{ext}"))).unwrap_or_default();
        let level = crate::source::level(name, "map", &text("solids"), &text("ents"));
        let spawn = level.checkpoints[0].spawn;
        let (mut sim, mut c) = start(level);
        run(&mut sim, &mut c, 1.0, |_, _| Input::default());
        let stood = c.state == State::Ground && (c.feet.y - spawn.y).abs() < 1.0;
        if !stood {
            bad.push(format!("{name}: spawn {spawn}, after 1 s {:?} at {}", c.state, c.feet));
            continue;
        }
        // The arrival elevator rides; she stays on her feet in it.
        let ev = run(&mut sim, &mut c, 20.0, |_, _| Input::default());
        if ev.contains(&SimEvent::PlayerDied) || c.state != State::Ground {
            bad.push(format!("{name}: after 20 s waiting, {:?} at {} (died: {})", c.state, c.feet, ev.contains(&SimEvent::PlayerDied)));
            continue;
        }
        let stood_at = c.feet;
        run(&mut sim, &mut c, 3.0, |_, _| forward());
        if c.feet.y < stood_at.y - 2.0 {
            bad.push(format!("{name}: walking from {stood_at} fell to {}", c.feet));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Portal's Chamber 00 (where it's been extracted), its logic running: the relaxation vault's
/// portals open once GLaDOS has spoken, a cube is made for the first room, and a cube on that
/// room's button opens the way out.
#[test]
fn portals_chamber_00_runs_its_logic() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let text = |ext: &str| std::fs::read_to_string(dir.join(format!("testchmb_a_00.{ext}")));
    let (Ok(solids), Ok(ents)) = (text("solids"), text("ents")) else { return };
    let scenes = std::fs::read_to_string(dir.join("scenes.txt")).map(|t| crate::source::parse_scenes(&t)).unwrap_or_default();
    let level = crate::source::level_with_scenes("testchmb_a_00", "00", &solids, &ents, &scenes);
    let (mut sim, mut c) = start(level);
    run(&mut sim, &mut c, 5.0, |_, _| Input::default());
    assert!(sim.portals.iter().all(|p| p.is_none()), "portals open too soon");
    assert!(sim.props.iter().any(|p| p.alive), "no cube made");
    run(&mut sim, &mut c, 65.0, |_, _| Input::default());
    assert!(sim.portals.iter().all(|p| p.is_some()), "the vault's portals never opened");

    // A cube on the first room's button: the exit doors open.
    let button = sim.buttons.iter().position(|b| (b.at - Vec3::new(23.58, 3.56, 13.0)).length() < 1.0).expect("the first room's button");
    let at = sim.buttons[button].at;
    let cube = sim.props.iter().position(|p| p.alive).unwrap();
    sim.props[cube].pos = at + Vec3::Y * 0.8;
    sim.props[cube].vel = Vec3::ZERO;
    run(&mut sim, &mut c, 4.0, |_, _| Input::default());
    assert!(sim.buttons[button].pressed, "the cube didn't press the button");
    let door = |sim: &Sim, name: &str| -> usize {
        let io = sim.io.as_ref().unwrap();
        io.ents.iter().find_map(|e| match e.kind {
            crate::io::Kind::Mover { index, .. } if e.name == name => Some(index),
            _ => None,
        }).expect(name)
    };
    for name in ["exit_door_left_a00", "exit_door_right_a00"] {
        let m = &sim.movers[door(&sim, name)];
        assert_eq!(m.node, 1, "{name} didn't open");
    }
    // Its state panel shows the check (frame 1); the other door's still shows the X (#30).
    let panel = |sim: &Sim, name: &str| -> u32 {
        let model = sim.movers[door(sim, name)].def.model;
        sim.io.as_ref().unwrap().frames.get(&model).copied().unwrap_or(0)
    };
    assert_eq!(panel(&sim, "exit_doorstate_a00"), 1, "the panel didn't turn to the check");
    assert_eq!(panel(&sim, "exit_doorstate_a00_b"), 0, "the other door's panel changed too");
    // Off the button: they close again, and the panel's back to the X.
    sim.props[cube].pos = at + Vec3::new(3.0, 0.8, 0.0);
    run(&mut sim, &mut c, 4.0, |_, _| Input::default());
    assert_eq!(sim.movers[door(&sim, "exit_door_left_a00")].node, 0, "didn't close");
    assert_eq!(panel(&sim, "exit_doorstate_a00"), 0, "the panel stayed on the check");

    // The chamber's number sign, seen from the vault, has flickered on since GLaDOS's
    // welcome dimmed the lights: it ends lit (skin 6, #35).
    let io = sim.io.as_ref().unwrap();
    let sign = io.ents.iter().position(|e| e.name == "sign_00").expect("sign_00");
    assert_eq!(io.skins.get(&sign), Some(&6), "the sign didn't come on");
}

/// Chamber 00's lift between its rooms: step in, and five seconds later it carries her up
/// its shaft (576 units, 14.6 m) to the second room.
#[test]
fn portals_chamber_00_lift_carries_her_up() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let text = |ext: &str| std::fs::read_to_string(dir.join(format!("testchmb_a_00.{ext}")));
    let (Ok(solids), Ok(ents)) = (text("solids"), text("ents")) else { return };
    let mut level = crate::source::level("testchmb_a_00", "00", &solids, &ents);
    level.spawn([21.95, 3.75, 37.39], 0.0);
    let (mut sim, mut c) = start(level);
    run(&mut sim, &mut c, 1.0, |_, _| Input::default());
    let floor = c.feet.y;
    assert!(c.state == State::Ground && (floor - 3.68).abs() < 0.2, "not on the lift's floor: {:?} at {}", c.state, c.feet);
    run(&mut sim, &mut c, 22.0, |_, _| Input::default());
    assert!((c.feet.y - (floor + 14.63)).abs() < 0.3, "the lift didn't take her up: {} (from {floor})", c.feet);
    assert_eq!(c.state, State::Ground);
}

/// Chamber 00's exit: into the last elevator, and its ride up ends the chamber (its path's
/// last node changes the level).
#[test]
fn portals_chamber_00_exit_elevator_finishes_it() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let text = |ext: &str| std::fs::read_to_string(dir.join(format!("testchmb_a_00.{ext}")));
    let (Ok(solids), Ok(ents)) = (text("solids"), text("ents")) else { return };
    let mut level = crate::source::level("testchmb_a_00", "00", &solids, &ents);
    level.spawn([6.50, 15.1, 9.75], 0.0);
    let (mut sim, mut c) = start(level);
    let ev = run(&mut sim, &mut c, 40.0, |_, _| Input::default());
    assert!(sim.complete, "the ride didn't end the chamber (at {})", c.feet);
    assert!(ev.contains(&SimEvent::LevelComplete));
    let tr = sim.transition.clone().expect("the ride didn't record which map it changes to");
    assert_eq!(tr.map, "testchmb_a_01");
    assert_eq!(tr.landmark, "a00-a01_landmark_elevator");
    let text = |ext: &str| std::fs::read_to_string(dir.join(format!("testchmb_a_01.{ext}")));
    let (Ok(solids), Ok(ents)) = (text("solids"), text("ents")) else { return };
    let level = crate::source::level("testchmb_a_01", "01", &solids, &ents);
    let (feet, yaw) = tr.spawn_in(&level).expect("chamber 01 has no landmark of that name");
    let mut arrived = Sim::new(level);
    let start = arrived.level.checkpoints[0].spawn;
    assert!(feet.distance(start) > 2.0, "arrived at the player start {feet}, outside the elevator (start {start})");
    let on_car = arrived.movers.iter().flat_map(|m| m.boxes()).any(|b| {
        let s = b.size();
        s.y < 0.35 && s.x > 2.5 && s.z > 2.5 && (feet.x - b.center().x).abs() < 1.2 && (feet.z - b.center().z).abs() < 1.2 && feet.y >= b.max.y - 0.08 && feet.y <= b.max.y + 0.4
    });
    assert!(on_car, "not on the arrival elevator's floor: feet {feet}, start {start}");
    // Arriving runs the map's transition logic, which opens those doors a second later.
    arrived.arrived = true;
    let mut c = Controller::new(crate::tuning(5.3, 10.0), feet, yaw);
    c.state = State::Ground;
    run(&mut arrived, &mut c, 1.3, |_, _| Input::default());
    let opened = arrived.io.as_ref().is_some_and(|io| io.animations.values().any(|(n, _)| n == "open"));
    assert!(opened, "arrival didn't open the elevator doors");
}

/// A 16-unit riser (Portal's stairs) is under Source's 18-unit step and over Mirror's Edge's.
#[test]
fn walks_a_sixteen_unit_stair() {
    let rise = 16.0 * 0.0254;
    let mut w = faith_move::BoxWorld::default();
    w.add(faith_move::Aabb::new(Vec3::new(-4.0, -1.0, -4.0), Vec3::new(4.0, 0.0, 4.0)));
    for i in 0..4 {
        let z0 = -2.0 - i as f32 * 0.48;
        w.add(faith_move::Aabb::new(Vec3::new(-2.0, 0.0, -20.0), Vec3::new(2.0, rise * (i + 1) as f32, z0)));
    }
    let mut c = Controller::new(crate::tuning(5.3, 10.0), Vec3::ZERO, 0.0);
    c.state = State::Ground;
    let mut top = 0.0f32;
    for _ in 0..(5.0 / DT) as i32 {
        c.step(DT, &forward(), &w);
        top = top.max(c.feet.y);
    }
    assert!(top > rise * 4.0 - 0.02, "didn't walk the stair: {top} (feet {})", c.feet);
}

/// Portal's white walls take portals: in each map with a gun, shots from a few metres in front
/// of its big white walls open portals on them.
#[test]
fn portals_maps_white_walls_take_portals() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let Ok(read) = std::fs::read_dir(&dir) else { return };
    let mut names: Vec<String> = read.filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".solids").map(str::to_string)).collect();
    names.sort();
    let mut bad = Vec::new();
    for name in &names {
        let text = |ext: &str| std::fs::read_to_string(dir.join(format!("{name}.{ext}"))).unwrap_or_default();
        let level = crate::source::level(name, "map", &text("solids"), &text("ents"));
        if !level.gun.blue {
            continue;
        }
        let (mut sim, c) = start(level);
        let solid = |sim: &Sim, p: Vec3| sim.statics.iter().any(|s| crate::geom::contains(&s.b, p));
        let (mut tried, mut opened) = (0, 0);
        let walls: Vec<_> = sim.statics.iter().filter(|s| s.mat == Mat::White).map(|s| s.b).collect();
        for b in walls {
            let size = b.size();
            for n in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
                // A face at least 2 m wide and 3 m tall.
                let wide = if n.x != 0.0 { size.z } else { size.x };
                if wide < 2.0 || size.y < 3.0 || tried >= 40 {
                    continue;
                }
                let face = b.center() + n * (if n.x != 0.0 { size.x } else { size.z }) * 0.5;
                let eye = face + n * 3.0;
                if solid(&sim, eye) || solid(&sim, face + n * 0.5) {
                    continue;
                }
                tried += 1;
                let aim = Aim { eye, dir: -n, yaw: c.yaw };
                if sim.fire(PortalColor::Blue, aim) {
                    opened += 1;
                }
            }
        }
        if tried > 0 && opened * 2 < tried {
            bad.push(format!("{name}: {opened} of {tried} white walls took a portal"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn a_pellet_through_portals_lights_its_catcher() {
    // Fired along +X into blue (on a wall at x 6), out of orange (on a wall at z -10, facing +Z),
    // along +Z into the catcher.
    let mut l = flat();
    l.solid(Mat::White, [6.0, 0.0, -3.0], [6.5, 4.0, 3.0]);
    l.solid(Mat::White, [-3.0, 0.0, -10.5], [3.0, 4.0, -10.0]);
    l.item(crate::level::Item::PelletLauncher { at: Vec3::new(0.0, 1.2, 0.0), dir: Vec3::X, inputs: Inputs::always(), life: 12.0, launch: Default::default() });
    l.item(crate::level::Item::PelletCatcher { sig: 0, at: Vec3::new(0.0, 1.2, -5.0), facing: Vec3::NEG_Z });
    l.spawn([-8.0, 0.0, 8.0], 0.0);
    let (mut sim, mut c) = start(l);
    sim.set_portal(PortalColor::Blue, Some(Portal::new(Vec3::new(6.0, 1.2, 0.0), Vec3::NEG_X, Vec3::Y)));
    sim.set_portal(PortalColor::Orange, Some(Portal::new(Vec3::new(0.0, 1.2, -10.0), Vec3::Z, Vec3::Y)));
    let ev = run(&mut sim, &mut c, 6.0, |_, _| Input::default());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::PelletGone { caught: true, .. })), "never caught");
    assert!(sim.pellet_catchers[0].lit && sim.signals[0]);
}

#[test]
fn a_pellet_bounces_off_walls_and_kills() {
    // Fired across the room at a wall: it bounces off.
    let mut l = flat();
    l.solid(Mat::Black, [-3.0, 0.0, -12.5], [3.0, 4.0, -12.0]);
    l.item(crate::level::Item::PelletLauncher { at: Vec3::new(-2.0, 1.0, -2.0), dir: Vec3::new(0.3, 0.0, -1.0).normalize(), inputs: Inputs::always(), life: 12.0, launch: Default::default() });
    l.spawn([8.0, 0.0, 8.0], 0.0);
    let (mut sim, mut c) = start(l);
    let ev = run(&mut sim, &mut c, 4.0, |_, _| Input::default());
    let normal = ev.iter().find_map(|e| match e {
        SimEvent::PelletBounced { normal, .. } => Some(*normal),
        _ => None,
    });
    let normal = normal.expect("didn't bounce");
    // The wall faces her (+Z): the mark sits on that face.
    assert!(normal.z > 0.9, "normal {normal}");
    assert!(!ev.contains(&SimEvent::PlayerDied));
    // Fired straight at her: it kills.
    let mut l = flat();
    l.item(crate::level::Item::PelletLauncher { at: Vec3::new(0.0, 1.0, -6.0), dir: Vec3::Z, inputs: Inputs::always(), life: 12.0, launch: Default::default() });
    l.spawn([0.0, 0.0, 1.0], 0.0);
    let (mut sim, mut c) = start(l);
    let ev = run(&mut sim, &mut c, 4.0, |_, _| Input::default());
    assert!(ev.contains(&SimEvent::PlayerDied), "didn't kill her");
}

/// Dropped off a square angle, a storage cube and a companion cube stay there. A reflector
/// keeps its yaw too (that yaw aims its laser) and is not turned to a right angle either.
#[test]
fn a_resting_cube_keeps_the_yaw_it_was_dropped_at() {
    let mut l = flat();
    l.cube(CubeKind::Storage, [0.0, 0.5, -2.0]);
    l.cube(CubeKind::Companion, [2.0, 0.5, -2.0]);
    l.cube(CubeKind::Reflector, [-2.0, 0.5, -2.0]);
    let (mut sim, mut c) = start(l);
    let yaw = 0.7;
    for p in &mut sim.props {
        p.rot = Quat::from_rotation_y(yaw);
    }
    run(&mut sim, &mut c, 1.5, |_, _| Input::default());
    for p in &sim.props {
        let (got, _, _) = p.rot.to_euler(EulerRot::YXZ);
        assert!((got - yaw).abs() < 0.05, "{:?} yaw {got}, wanted {yaw}", p.kind);
    }
}

/// A cube whose bottom is on an aerial faith plate is thrown and stops on the plate's target.
#[test]
fn a_cube_on_a_faith_plate_is_launched() {
    let mut l = flat();
    let target = Vec3::new(0.0, 0.0, -14.0);
    l.faith_plate([0.0, 0.0, -4.0], target.to_array(), 3.0);
    l.cube(CubeKind::Storage, [0.0, crate::sim::CUBE_HALF + crate::sim::PLATE_TOP, -4.0]);
    let (mut sim, mut c) = start(l);
    c.feet = Vec3::new(5.0, 0.0, 0.0);
    let ev = run(&mut sim, &mut c, 2.2, |_, _| Input::default());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::PlateLaunch { .. })), "the plate didn't fire");
    assert!(c.feet.y < 0.2, "she was thrown too: {}", c.feet);
    let p = sim.props[0].pos;
    assert!(p.distance(target + Vec3::Y * crate::sim::CUBE_HALF) < 1.5, "landed at {p}");
}

/// Portal's Chamber 06 (where it's been extracted): its ceiling launcher fires a pellet, and a
/// pellet in the catcher is taken and raises the lift.
#[test]
fn portals_chamber_06_pellet_catcher_raises_the_lift() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let text = |ext: &str| std::fs::read_to_string(dir.join(format!("testchmb_a_03.{ext}")));
    let (Ok(solids), Ok(ents)) = (text("solids"), text("ents")) else { return };
    let level = crate::source::level("testchmb_a_03", "06", &solids, &ents);
    let (mut sim, mut c) = start(level);
    let ev = run(&mut sim, &mut c, 3.0, |_, _| Input::default());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::PelletLaunched { .. })), "the launcher didn't fire");
    let k = sim.pellets.iter().position(|p| p.alive).expect("a pellet in the air");
    // Into the catcher (its trigger, Source's ball_trap_10).
    sim.pellets[k].pos = Vec3::new(-10.5, 15.2, 67.9);
    sim.pellets[k].vel = Vec3::ZERO;
    let ev = run(&mut sim, &mut c, 6.0, |_, _| Input::default());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::PelletGone { caught: true, .. })), "not caught");
    let io = sim.io.as_ref().unwrap();
    let lift = io.ents.iter().find_map(|e| match e.kind {
        crate::io::Kind::Mover { index, .. } if e.name == "lift_a" => Some(index),
        _ => None,
    });
    assert_eq!(sim.movers[lift.unwrap()].node, 1, "the lift didn't go up");
}

#[test]
fn chamber06_can_be_solved() {
    use crate::levels::c06_scaffold::*;
    let (mut sim, mut c) = start(crate::levels::chamber06());
    // Blue where the pellet hits the right wall; orange low on the right wall, facing the catcher.
    sim.set_portal(PortalColor::Blue, Some(Portal::new(Vec3::new(10.0, LAUNCHER[1], LAUNCHER[2]), Vec3::NEG_X, Vec3::Y)));
    sim.set_portal(PortalColor::Orange, Some(Portal::new(Vec3::new(10.0, CATCHER[1], CATCHER[2]), Vec3::NEG_X, Vec3::Y)));
    let mut t = 0.0;
    while !sim.pellet_catchers[0].lit && t < 10.0 {
        run(&mut sim, &mut c, 0.5, |_, _| Input::default());
        t += 0.5;
    }
    assert!(sim.pellet_catchers[0].lit, "the pellet never reached its catcher");
    // Onto the platform when it's back at the near side, and ride it over.
    let mut t = 0.0;
    while !(sim.movers[0].node == 0 && sim.movers[0].goal.is_none()) && t < 20.0 {
        run(&mut sim, &mut c, 0.1, |_, _| Input::default());
        t += 0.1;
    }
    c.feet = glam::Vec3::new(0.0, 0.0, -11.5);
    c.vel = glam::Vec3::ZERO;
    run(&mut sim, &mut c, 0.5, |_, _| Input::default());
    let mut t = 0.0;
    while !(sim.movers[0].node == 1 && sim.movers[0].goal.is_none()) && t < 12.0 {
        run(&mut sim, &mut c, 0.1, |_, _| Input::default());
        t += 0.1;
    }
    assert!(c.feet.z < -26.0 && c.feet.y > -0.2, "didn't ride over: {}", c.feet);
    // Off it and out.
    run(&mut sim, &mut c, 8.0, |_, _| forward());
    assert!(sim.complete, "not out: {}", c.feet);
}


#[test]
fn a_turret_shoots_her_in_sight_and_not_behind_a_wall() {
    // Facing +Z (yaw 180), her 6 m in front of it.
    let mut l = flat();
    l.turret([0.0, 0.0, -6.0], 180.0);
    l.spawn([0.0, 0.0, 0.0], 0.0);
    let (mut sim, mut c) = start(l);
    let ev = run(&mut sim, &mut c, 3.0, |_, _| Input::default());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::TurretSpotted { .. })));
    assert!(ev.contains(&SimEvent::PlayerDied), "it didn't kill her");
    // A wall between: it never sees her.
    let mut l = flat();
    l.turret([0.0, 0.0, -6.0], 180.0);
    l.solid(Mat::Black, [-3.0, 0.0, -3.5], [3.0, 3.0, -3.0]);
    l.spawn([0.0, 0.0, 0.0], 0.0);
    let (mut sim, mut c) = start(l);
    let ev = run(&mut sim, &mut c, 3.0, |_, _| Input::default());
    assert!(!ev.iter().any(|e| matches!(e, SimEvent::TurretSpotted { .. } | SimEvent::PlayerDied)));
}

#[test]
fn running_into_a_turret_knocks_it_over() {
    // It faces away (yaw 0, looking -Z); she runs up behind it.
    let mut l = flat();
    l.turret([0.0, 0.0, -8.0], 0.0);
    l.spawn([0.0, 0.0, 0.0], 0.0);
    let (mut sim, mut c) = start(l);
    let ev = run(&mut sim, &mut c, 3.0, |_, _| forward());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::TurretTipped { .. })), "not knocked over (at {})", c.feet);
    run(&mut sim, &mut c, 3.0, |_, _| Input::default());
    assert_eq!(sim.turrets[0].state, crate::sim::TurretState::Retired);
    assert!(!sim.turrets[0].upright());
}

#[test]
fn a_thrown_cube_knocks_a_turret_over() {
    let mut l = flat();
    l.turret([0.0, 0.0, -8.0], 0.0);
    l.cube(CubeKind::Storage, [0.0, 0.5, -4.0]);
    l.spawn([4.0, 0.0, 0.0], 0.0);
    let (mut sim, mut c) = start(l);
    sim.props[0].vel = Vec3::new(0.0, 1.0, -8.0);
    let ev = run(&mut sim, &mut c, 1.5, |_, _| Input::default());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::TurretTipped { .. })));
}

#[test]
fn a_punch_knocks_a_turret_over() {
    // Up close behind it, facing it, and punch.
    let mut l = flat();
    l.turret([0.0, 0.0, -1.2], 0.0);
    l.spawn([0.0, 0.0, 0.0], 0.0);
    let (mut sim, mut c) = start(l);
    let mut k = 0;
    let ev = run(&mut sim, &mut c, 2.0, |_, _| {
        k += 1;
        Input { melee_pressed: k == 5, ..Default::default() }
    });
    assert!(ev.iter().any(|e| matches!(e, SimEvent::TurretTipped { .. })), "the punch didn't knock it over");
}

/// GLaDOS (where her scenes have been extracted): Chamber 00's first scene starts six seconds
/// in, and her first line 20.4 s into it.
#[test]
fn portals_chamber_00_glados_speaks_on_cue() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let text = |f: &str| std::fs::read_to_string(dir.join(f));
    let (Ok(solids), Ok(ents), Ok(scenes)) = (text("testchmb_a_00.solids"), text("testchmb_a_00.ents"), text("scenes.txt")) else { return };
    let level = crate::source::level_with_scenes("testchmb_a_00", "00", &solids, &ents, &crate::source::parse_scenes(&scenes));
    let (mut sim, mut c) = start(level);
    run(&mut sim, &mut c, 26.0, |_, _| Input::default());
    let said = |sim: &Sim| sim.io.as_ref().unwrap().voice.iter().map(|(_, s)| s.clone()).collect::<Vec<_>>();
    assert!(!said(&sim).contains(&"vo_00_part1_entry-1".to_string()), "too soon: {:?}", said(&sim));
    run(&mut sim, &mut c, 1.0, |_, _| Input::default());
    assert!(said(&sim).contains(&"vo_00_part1_entry-1".to_string()), "she didn't speak: {:?}", said(&sim));
}

/// Portal's stairs walk (#39): Chamber 02's steel stair (testchmb_a_01, 16-unit risers on
/// 32-unit treads, a clip ramp over them), down and back up without jumping.
#[test]
fn portals_stairs_walk() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let text = |ext: &str| std::fs::read_to_string(dir.join(format!("testchmb_a_01.{ext}")));
    let (Ok(solids), Ok(ents)) = (text("solids"), text("ents")) else { return };
    for x in [-26.0f32, -22.7, -19.4] {
        let mut level = crate::source::level("testchmb_a_01", "02", &solids, &ents);
        // Top of the stair, facing down it (+Z).
        level.spawn([x, 14.64, 20.3], std::f32::consts::PI);
        let (mut sim, mut c) = start(level);
        c.yaw = std::f32::consts::PI;
        let mut log = Vec::new();
        let mut t = 0.0;
        run(&mut sim, &mut c, 4.0, |_, c| {
            t += DT;
            if (t * 10.0).fract() < DT * 10.0 { log.push(format!("{:.1}:{:.2},{:.2}", t, c.feet.y, c.feet.z)); }
            forward()
        });
        assert!(c.feet.y < 11.5 && c.feet.z > 27.0, "x {x}: didn't walk down: {} {:?}\n{}", c.feet, c.state, log.join(" "));
        c.yaw = 0.0;
        let mut log = Vec::new();
        let mut t = 0.0;
        let mut jumped = false;
        run(&mut sim, &mut c, 4.0, |_, c| {
            t += DT;
            if c.state == State::Air && c.vel.y > 1.0 { jumped = true; }
            if (t * 10.0).fract() < DT * 10.0 { log.push(format!("{:.1}:{:.2},{:.2}", t, c.feet.y, c.feet.z)); }
            forward()
        });
        assert!(c.feet.y > 14.5 && c.feet.z < 21.0, "x {x}: didn't walk up: {} {:?}\n{}", c.feet, c.state, log.join(" "));
        assert!(!jumped, "x {x}: went up in the air");
    }
}

/// Faith doesn't pass into walls (#26): in each of Portal's maps, she walks into its walls
/// head-on and at shallow angles, and never ends up more than grazing one.
#[test]
fn portals_maps_walls_stop_her() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let Ok(read) = std::fs::read_dir(&dir) else { return };
    let mut names: Vec<String> = read.filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".solids").map(str::to_string)).collect();
    names.sort();
    let mut bad = Vec::new();
    let (mut tried_all, mut runs) = (0, 0);
    for name in &names {
        let text = |ext: &str| std::fs::read_to_string(dir.join(format!("{name}.{ext}"))).unwrap_or_default();
        let level = crate::source::level(name, "map", &text("solids"), &text("ents"));
        let (sim0, _) = start(level.clone());
        let statics: Vec<faith_move::Aabb> = sim0.statics.iter().map(|s| s.b).collect();
        let free = |b: &faith_move::Aabb| !statics.iter().any(|s| s.overlaps(b));
        let mut tried = 0;
        for w in &statics {
            let size = w.size();
            if size.y < 2.0 || tried >= 60 {
                continue;
            }
            for n in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
                let wide = if n.x != 0.0 { size.z } else { size.x };
                if wide < 1.5 {
                    continue;
                }
                let face = w.center() + n * (if n.x != 0.0 { size.x } else { size.z }) * 0.5;
                // Standing on a floor in front of it, at its foot.
                let at = Vec3::new(face.x, w.min.y, face.z) + n * 0.9;
                let Some(floor) = statics.iter().filter(|s| s.min.x < at.x && s.max.x > at.x && s.min.z < at.z && s.max.z > at.z && (s.max.y - at.y).abs() < 0.3).map(|s| s.max.y).reduce(f32::max) else { continue };
                let feet = Vec3::new(at.x, floor + 0.01, at.z);
                let body = faith_move::Aabb::new(feet + Vec3::new(-0.45, 0.0, -0.45), feet + Vec3::new(0.45, 1.9, 0.45));
                if !free(&body) {
                    continue;
                }
                tried += 1;
                for deg in [0.0f32, 20.0, -20.0, 45.0, -45.0, 70.0, -70.0, 82.0, -82.0] {
                    let mut l = level.clone();
                    let d = Quat::from_rotation_y(deg.to_radians()) * -n;
                    let yaw = (-d.x).atan2(-d.z);
                    l.checkpoints[0].spawn = feet;
                    l.checkpoints[0].yaw = yaw;
                    let (mut sim, mut c) = start(l);
                    c.yaw = yaw;
                    runs += 1;
                    // (A portal closing in the sim's step leaves her in its wall until her
                    // next one: inside two frames running counts.)
                    let mut inside = None;
                    let mut was_inside = false;
                    run(&mut sim, &mut c, 1.2, |sim, c| {
                        if inside.is_none() {
                            let b = c.body().aabb(c.feet);
                            // How deep she is in what she's inside (a vault's path can graze a
                            // corner; passing into a wall goes deep).
                            let w = sim.player_world();
                            let mut hit = Vec::new();
                            w.still.query(&b, &mut hit);
                            // (Vaulting, she goes over what's under her waist.)
                            if matches!(c.state, State::Vault(_)) {
                                hit.retain(|s| s.max.y > c.feet.y + 0.9);
                            }
                            let depth = |s: &faith_move::Aabb| (0..3).map(|a| s.max[a].min(b.max[a]) - s.min[a].max(b.min[a])).fold(f32::MAX, f32::min);
                            if let Some(s) = hit.iter().copied().max_by(|x, y| depth(x).total_cmp(&depth(y))).filter(|s| depth(s) > 0.1) {
                                if was_inside {
                                    inside = Some((c.feet, s, c.state.name()));
                                }
                                was_inside = true;
                            } else {
                                was_inside = false;
                            }
                        }
                        forward()
                    });
                    if let Some((f, s, st)) = inside {
                        bad.push(format!("{name}: from {feet} yaw {:.0} ({deg}° off) inside {:?}-{:?} at {f} ({st})", yaw.to_degrees(), s.min, s.max));
                    }
                }
            }
        }
        tried_all += tried;
    }
    assert!(bad.is_empty(), "{} of {runs} runs ({tried_all} walls):\n{}", bad.len(), bad.join("\n"));
}



/// In each of Portal's maps, a cube on a floor button opens the doors its button's `OnOpen`
/// sends `Open` to (Portal's exits mostly open that way: #31).
#[test]
fn portals_buttons_open_their_doors() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let Ok(read) = std::fs::read_dir(&dir) else { return };
    let mut names: Vec<String> = read.filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".solids").map(str::to_string)).collect();
    names.sort();
    let mut bad = Vec::new();
    let mut checked = 0;
    for name in &names {
        let text = |ext: &str| std::fs::read_to_string(dir.join(format!("{name}.{ext}"))).unwrap_or_default();
        let level = crate::source::level(name, "map", &text("solids"), &text("ents"));
        let (sim0, _) = start(level.clone());
        let io = sim0.io.as_ref().unwrap();
        for (k, e) in io.ents.iter().enumerate() {
            let crate::io::Kind::ButtonDoor { button, .. } = e.kind else { continue };
            let doors: Vec<(String, usize)> = e
                .outputs
                .iter()
                .filter(|o| o.event.eq_ignore_ascii_case("OnOpen") && o.input.eq_ignore_ascii_case("Open"))
                .flat_map(|o| io.ents.iter().filter(|d| d.name.eq_ignore_ascii_case(&o.target)).filter_map(|d| match d.kind {
                    crate::io::Kind::Mover { index, .. } => Some((d.name.clone(), index)),
                    _ => None,
                }))
                .collect();
            if doors.is_empty() {
                continue;
            }
            let (mut sim, mut c) = start(level.clone());
            c.feet = Vec3::new(1000.0, 1000.0, 1000.0);
            run(&mut sim, &mut c, 0.5, |_, _| Input::default());
            let at = sim.buttons[button].at;
            sim.spawn_prop(CubeKind::Storage, at + Vec3::Y * 0.6, None);
            run(&mut sim, &mut c, 4.0, |_, _| Input::default());
            checked += 1;
            if !sim.buttons[button].pressed {
                bad.push(format!("{name}: {} ({k}): the cube didn't press it", e.name));
                continue;
            }
            for (d, m) in doors {
                let mv = &sim.movers[m];
                let last = mv.def.path.len() - 1;
                if mv.node != last && mv.goal != Some(last) {
                    bad.push(format!("{name}: {} didn't open {d} (node {}, goal {:?}, path {})", e.name, mv.node, mv.goal, mv.def.path.len()));
                }
            }
        }
    }
    assert!(checked > 5, "only {checked} buttons");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Swinging doors swing (#31): Chamber 18's pellet launcher (testchmb_a_14) turns its door
/// open as it fires and shut again, and a door built to turn 90 degrees gets there.
#[test]
fn portals_rotating_doors_swing() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let text = |ext: &str| std::fs::read_to_string(dir.join(format!("testchmb_a_14.{ext}")));
    let (Ok(solids), Ok(ents)) = (text("solids"), text("ents")) else { return };
    let level = crate::source::level("testchmb_a_14", "18", &solids, &ents);
    let (mut sim, mut c) = start(level);
    let io = sim.io.as_ref().unwrap();
    let door = io.ents.iter().find_map(|e| match e.kind {
        crate::io::Kind::Mover { index, .. } if e.name == "lab4_rotate_launcher_door1" => Some(index),
        _ => None,
    }).expect("the launcher's door");
    let sw = sim.movers[door].def.swing.expect("it swings");
    assert!((sw.angle.abs() - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "{sw:?}");
    // Her in the trigger that starts the launchers.
    let start_box = io.ents.iter().find_map(|e| match &e.kind {
        crate::io::Kind::Trigger { b, .. } if e.name == "start_spinning_launchers" => Some(*b),
        _ => None,
    }).expect("the launchers' trigger");
    c.feet = Vec3::new(start_box.center().x, start_box.min.y + 0.05, start_box.center().z);
    let mut widest = 0.0f32;
    run(&mut sim, &mut c, 8.0, |sim, _| {
        widest = widest.max(sim.movers[door].at.x);
        Input::default()
    });
    assert!((widest - sw.angle.abs()).abs() < 0.02, "it opened {widest} of {}", sw.angle.abs());
    // The launcher's panel, and the brush on that, turn with it.
    let io = sim.io.as_ref().unwrap();
    let panel = io.ents.iter().find_map(|e| match e.kind {
        crate::io::Kind::Mover { index, .. } if e.name == "lab4_rotate_launcher_brush1" => Some(index),
        _ => None,
    }).expect("the panel");
    let on_panel = sim.movers.iter().filter(|m| m.def.parent == Some(door)).count();
    assert_eq!(sim.movers[panel].def.parent, Some(door));
    assert!(on_panel >= 2, "only {on_panel} brushes ride the hatch");
}



/// Cubes rest on Portal's floors, not in them (#29): in every map, the cubes there from the
/// start or made in the first seconds (droppers, makers) aren't inside anything solid.
#[test]
fn portals_cubes_rest_on_floors() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let Ok(read) = std::fs::read_dir(&dir) else { return };
    let mut names: Vec<String> = read.filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".solids").map(str::to_string)).collect();
    names.sort();
    let mut bad = Vec::new();
    let mut seen = 0;
    for name in &names {
        let text = |ext: &str| std::fs::read_to_string(dir.join(format!("{name}.{ext}"))).unwrap_or_default();
        let level = crate::source::level(name, "map", &text("solids"), &text("ents"));
        let (mut sim, mut c) = start(level);
        c.feet = Vec3::new(1000.0, 1000.0, 1000.0);
        run(&mut sim, &mut c, 4.0, |_, _| Input::default());
        for p in sim.props.iter().filter(|p| p.alive) {
            seen += 1;
            let b = p.aabb();
            let inner = faith_move::Aabb::new(b.min + Vec3::splat(0.05), b.max - Vec3::splat(0.05));
            if let Some(s) = sim.statics.iter().find(|s| s.b.overlaps(&inner)) {
                bad.push(format!("{name}: cube at {} is inside {:?}-{:?}", p.pos, s.b.min, s.b.max));
            }
        }
    }
    assert!(seen > 5, "only {seen} cubes");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}


/// Every pellet catcher in Portal's maps (#23): its ball is hidden till a pellet's caught, and a
/// pellet in its trigger is taken and shows it.
#[test]
fn portals_catchers_take_pellets() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let Ok(read) = std::fs::read_dir(&dir) else { return };
    let mut names: Vec<String> = read.filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".solids").map(str::to_string)).collect();
    names.sort();
    let (mut bad, mut seen) = (Vec::new(), 0);
    for name in &names {
        let text = |ext: &str| std::fs::read_to_string(dir.join(format!("{name}.{ext}"))).unwrap_or_default();
        let level = crate::source::level(name, "map", &text("solids"), &text("ents"));
        let (sim0, _) = start(level.clone());
        let io = sim0.io.as_ref().unwrap();
        let catchers: Vec<(usize, String, faith_move::Aabb)> = io.ents.iter().enumerate().filter_map(|(k, e)| match &e.kind {
            crate::io::Kind::Trigger { b, pellets: true, .. } => Some((k, e.name.clone(), *b)),
            _ => None,
        }).collect();
        for (k, trigger, b) in catchers {
            seen += 1;
            let (mut sim, mut c) = start(level.clone());
            c.feet = Vec3::new(1000.0, 1000.0, 1000.0);
            run(&mut sim, &mut c, 0.2, |_, _| Input::default());
            let io = sim.io.as_ref().unwrap();
            // Its ball: the combineball the trigger enables.
            let ball = io.ents[k].outputs.iter().find(|o| o.input.eq_ignore_ascii_case("Enable") && o.target.ends_with("_ball")).and_then(|o| io.ents.iter().position(|e| e.name.eq_ignore_ascii_case(&o.target)));
            if let Some(ball) = ball {
                if !io.hidden.contains(&ball) {
                    bad.push(format!("{name}: {trigger}'s ball shows before it's caught one"));
                }
            }
            sim.pellets.push(crate::sim::Pellet { id: 999, pos: b.center(), vel: Vec3::ZERO, age: 0.0, life: 10.0, after_portal: 6.0, alive: true });
            let ev = run(&mut sim, &mut c, 0.5, |_, _| Input::default());
            if !ev.iter().any(|e| matches!(e, SimEvent::PelletGone { caught: true, .. })) {
                bad.push(format!("{name}: {trigger} didn't take the pellet"));
            }
            if let Some(ball) = ball {
                if sim.io.as_ref().unwrap().hidden.contains(&ball) {
                    bad.push(format!("{name}: {trigger}'s ball didn't show"));
                }
            }
        }
    }
    assert!(seen >= 8, "only {seen} catchers");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Chamber 19's dirt floor (testchmb_a_15, a displacement, #20): she stands on its mounds, not
/// on the flat brush under them.
#[test]
fn portals_displacements_hold_her_up() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let text = |ext: &str| std::fs::read_to_string(dir.join(format!("testchmb_a_15.{ext}")));
    let (Ok(solids), Ok(ents)) = (text("solids"), text("ents")) else { return };
    let level = crate::source::level("testchmb_a_15", "19", &solids, &ents);
    // The highest of the dirt's cells.
    let top = level.solids.iter().filter(|s| s.b.min.x > -15.5 && s.b.max.x < -7.2 && s.b.min.z > 34.0 && s.b.max.z < 54.2 && s.b.min.y > -8.2 && s.b.size().x < 0.3).max_by(|a, b| a.b.max.y.total_cmp(&b.b.max.y)).expect("the dirt's cells").b;
    assert!(top.max.y > -7.5, "no mounds: {top:?}");
    let (mut sim, mut c) = start(level);
    c.feet = Vec3::new(top.center().x, top.max.y + 1.0, top.center().z);
    run(&mut sim, &mut c, 1.5, |_, _| Input::default());
    assert!(c.feet.y > top.max.y - 0.35, "sank into the mound: feet {} on a top at {}", c.feet, top.max.y);
}

/// A moving brush pushes Faith out of its way, and waits rather than crush her against a wall
/// (#22).
#[test]
fn movers_push_but_dont_crush() {
    let mut l = flat();
    // A wall 2 m along +X from her, and a slab that slides 4 m along +X, starting just behind her.
    l.solid(Mat::White, [2.5, 0.0, -2.0], [3.0, 3.0, 2.0]);
    l.item(crate::level::Item::Mover(crate::level::MoverDef {
        model: crate::level::NO_MODEL,
        boxes: vec![crate::level::Solid { b: faith_move::Aabb::new(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(-0.4, 2.5, 1.0)), mat: Mat::Black }],
        path: vec![Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0)],
        speed: 1.5,
        start: 0,
        present: true,
        shift: Vec3::ZERO,
        inputs: Some(Inputs::always()),
        shuttle: false,
        swing: None,
        parent: None,
        follows: false,
        reset: None,
    }));
    let (mut sim, mut c) = start(l);
    c.feet = Vec3::ZERO;
    let mut overlapped = false;
    run(&mut sim, &mut c, 4.0, |sim, c| {
        let me = c.body().aabb(c.feet);
        overlapped |= sim.movers[0].boxes().any(|b| b.overlaps(&me));
        Input::default()
    });
    assert!(!overlapped, "the slab went into her");
    assert!(c.feet.x > 1.5, "not pushed: {}", c.feet);
    assert!(c.feet.x < 2.5 - 0.29, "pushed into the wall: {}", c.feet);
    assert!(sim.movers[0].at.x < 4.0 - 0.1, "it crushed through her: {}", sim.movers[0].at);
}

/// Portal's pedestal guns (#43): Chamber 02's turns and fires blue portals onto the walls till
/// she takes it, which gives her the gun; Chamber 11's adds orange to the blue she has.
#[test]
fn portals_pedestal_guns() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let text = |m: &str, ext: &str| std::fs::read_to_string(dir.join(format!("{m}.{ext}")));
    let (Ok(solids), Ok(ents)) = (text("testchmb_a_01", "solids"), text("testchmb_a_01", "ents")) else { return };
    let level = crate::source::level("testchmb_a_01", "02", &solids, &ents);
    let (mut sim, mut c) = start(level);
    assert_eq!(sim.gun, crate::level::Gun::NONE, "she has a gun before she's found it");
    assert_eq!(sim.gun_pickups.len(), 1);
    c.feet = Vec3::new(1000.0, 1000.0, 1000.0);
    // (It fires once as the map starts, then each time it's turned, every seven seconds or so.)
    let ev = run(&mut sim, &mut c, 14.0, |_, _| Input::default());
    let shots: Vec<(Vec3, bool)> = ev.iter().filter_map(|e| if let SimEvent::Shot { color: PortalColor::Blue, to, opened, .. } = e { Some((*to, *opened)) } else { None }).collect();
    assert!(shots.len() >= 2 && shots.iter().any(|s| s.1), "the pedestal didn't fire portals as it turned: {shots:?}");
    assert!(shots.windows(2).any(|w| w[0].0.distance(w[1].0) > 1.0), "it fired the same way each time: {shots:?}");
    // To the gun: hers.
    // Up to the pedestal (on the floor beside it): hers.
    let (at, _) = sim.gun_pose(0);
    c.feet = Vec3::new(at.x + 0.6, -3.25, at.z);
    c.vel = Vec3::ZERO;
    run(&mut sim, &mut c, 0.3, |_, _| Input::default());
    assert!(sim.gun_pickups[0].taken && sim.gun.blue && !sim.gun.orange, "picking it up: {:?} (gun at {at}, she's at {})", sim.gun, c.feet);

    let (Ok(solids), Ok(ents)) = (text("testchmb_a_07", "solids"), text("testchmb_a_07", "ents")) else { return };
    let level = crate::source::level("testchmb_a_07", "11", &solids, &ents);
    let (mut sim, mut c) = start(level);
    assert!(sim.gun.blue && !sim.gun.orange, "Chamber 11 starts with blue: {:?}", sim.gun);
    let (at, _) = sim.gun_pose(0);
    c.feet = at - Vec3::Y * 0.9;
    run(&mut sim, &mut c, 0.2, |_, _| Input::default());
    assert!(sim.gun.blue && sim.gun.orange, "the pedestal's orange: {:?}", sim.gun);
}



/// Cubes slide off what they overhang (#45): down a ramp of slices (as an angled brush is
/// collided), and off a ledge they're more than half over; one on flat ground stays put.
#[test]
fn cubes_slide_off_slopes_and_edges() {
    let mut l = flat();
    // A ramp along +X, 4 m long and 1 m high, in 3 cm steps.
    for k in 0..34 {
        let x0 = 2.0 + k as f32 * 0.12;
        l.solid(Mat::Black, [x0, 0.0, -3.0], [x0 + 0.12, 1.0 - k as f32 * 0.03, -1.0]);
    }
    // A ledge 1 m high, ending at x -2.
    l.solid(Mat::Black, [-6.0, 0.0, 2.0], [-2.0, 1.0, 4.0]);
    let (mut sim, mut c) = start(l);
    c.feet = Vec3::new(-20.0, 0.0, -20.0);
    sim.spawn_prop(CubeKind::Storage, Vec3::new(2.4, 1.6, -2.0), None);
    sim.spawn_prop(CubeKind::Storage, Vec3::new(-2.0 + 0.1, 1.6, 3.0), None);
    sim.spawn_prop(CubeKind::Storage, Vec3::new(-10.0, 0.6, 3.0), None);
    run(&mut sim, &mut c, 5.0, |_, _| Input::default());
    let at: Vec<Vec3> = sim.props.iter().map(|p| p.pos).collect();
    assert!(at[0].x > 5.0 && at[0].y < 0.6, "didn't slide down the ramp: {}", at[0]);
    assert!(at[1].y < 0.6, "stayed on the ledge it was over: {}", at[1]);
    assert!((at[2] - Vec3::new(-10.0, 0.45, 3.0)).length() < 0.05, "moved on flat ground: {}", at[2]);
}


/// A rocket turret (#24) turns to her, locks on and fires; the rocket kills her.
#[test]
fn a_rocket_turret_locks_on_and_kills() {
    let mut l = flat();
    // Looking away from her (+Z): it has to turn round.
    l.item(crate::level::Item::RocketTurret { at: Vec3::new(0.0, 1.3, -8.0), yaw: 0.0, inputs: Inputs::always(), mover: None });
    l.spawn([0.0, 0.0, 6.0], 0.0);
    let (mut sim, mut c) = start(l);
    let ev = run(&mut sim, &mut c, 8.0, |_, _| Input::default());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::RocketLocking { .. })), "never saw her");
    let fired = ev.iter().position(|e| matches!(e, SimEvent::RocketFired { .. })).expect("never fired");
    let died = ev.iter().position(|e| *e == SimEvent::PlayerDied).expect("didn't kill her");
    assert!(died > fired);
    // Switched off, it doesn't.
    let mut l = flat();
    l.item(crate::level::Item::RocketTurret { at: Vec3::new(0.0, 1.3, -8.0), yaw: 0.0, inputs: Inputs::on(0), mover: None });
    l.spawn([0.0, 0.0, 6.0], 0.0);
    let (mut sim, mut c) = start(l);
    let ev = run(&mut sim, &mut c, 8.0, |_, _| Input::default());
    assert!(!ev.iter().any(|e| matches!(e, SimEvent::RocketFired { .. } | SimEvent::PlayerDied)));
}

/// A rocket into a portal comes out of the other and goes on into what it's aimed at.
#[test]
fn a_rocket_through_portals_hits_its_target() {
    let mut l = flat();
    l.item(crate::level::Item::RocketTurret { at: Vec3::new(0.0, 1.3, -10.0), yaw: std::f32::consts::PI, inputs: Inputs::always(), mover: None });
    l.item(crate::level::Item::RocketTarget { at: Vec3::new(26.0, 1.3, 0.0), radius: 1.5 });
    l.spawn([0.0, 0.0, 6.0], 0.0);
    let (mut sim, mut c) = start(l);
    // Blue across its line of fire (facing it), orange out along +X towards the target.
    sim.set_portal(PortalColor::Blue, Some(Portal::new(Vec3::new(0.0, 1.3, -3.0), Vec3::NEG_Z, Vec3::Y)));
    sim.set_portal(PortalColor::Orange, Some(Portal::new(Vec3::new(18.0, 1.3, 0.0), Vec3::X, Vec3::Y)));
    let ev = run(&mut sim, &mut c, 6.0, |_, _| Input::default());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::RocketFired { .. })), "never fired");
    let hit = ev.iter().find_map(|e| match e {
        SimEvent::RocketExploded { at, target } => Some((*at, *target)),
        _ => None,
    });
    assert!(matches!(hit, Some((_, Some(0)))), "{hit:?}");
    assert!(!ev.contains(&SimEvent::PlayerDied));
}

/// Portal's escape (#24): escape_01's turret behind glass fires through it (the glass breaks),
/// and escape_02's rides its lift and is shut off by a rocket into GLaDOS.
#[test]
fn portals_rocket_turrets() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let text = |map: &str, ext: &str| std::fs::read_to_string(dir.join(format!("{map}.{ext}")));
    let (Ok(solids), Ok(ents)) = (text("escape_01", "solids"), text("escape_01", "ents")) else { return };
    let level = crate::source::level("escape_01", "map", &solids, &ents);
    let (mut sim, mut c) = start(level);
    assert_eq!(sim.rocket_turrets.len(), 1);
    let io = sim.io.as_ref().unwrap();
    let sig_of = |io: &crate::io::Io, name: &str| io.ents.iter().find_map(|e| match e.kind {
        crate::io::Kind::Switch { sig } if e.name == name => Some(sig),
        _ => None,
    });
    let laser = sig_of(io, "laser_1").expect("laser_1");
    let glass: Vec<usize> = io.ents.iter().filter_map(|e| match e.kind {
        crate::io::Kind::Mover { index, .. } if e.class == "func_breakable_surf" => Some(index),
        _ => None,
    }).collect();
    assert_eq!(glass.len(), 3);
    let rocket_triggers = io.ents.iter().filter(|e| matches!(e.kind, crate::io::Kind::Trigger { rockets: true, .. })).count();
    assert_eq!(rocket_triggers, 4);
    // She's on the far side of the glass (*51) from the turret, which is switched on.
    let turret = sim.rocket_turrets[0].at;
    let at = turret + Vec3::new(0.0, 0.0, -9.0);
    let floor = sim.raycast(at, Vec3::NEG_Y, 6.0, crate::sim::Mask::SOLID).expect("floor").point;
    c.feet = floor + Vec3::Y * 0.05;
    run(&mut sim, &mut c, 0.2, |_, _| Input::default());
    assert!(!sim.io_signals[laser], "laser_1 starts on");
    sim.io_signals[laser] = true;
    let ev = run(&mut sim, &mut c, 12.0, |_, _| Input::default());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::RocketFired { .. })), "never fired");
    assert!(glass.iter().any(|&m| !sim.movers[m].present), "no glass broke");
    assert!(ev.contains(&SimEvent::PlayerDied), "didn't get her");

    let (Ok(solids), Ok(ents)) = (text("escape_02", "solids"), text("escape_02", "ents")) else { return };
    let level = crate::source::level("escape_02", "map", &solids, &ents);
    let (mut sim, mut c) = start(level);
    assert_eq!(sim.rocket_turrets.len(), 1);
    assert!(sim.rocket_turrets[0].mover.is_some(), "not on its lift");
    assert_eq!(sim.rocket_targets.len(), 1);
    c.feet = Vec3::new(1000.0, 1000.0, 1000.0);
    run(&mut sim, &mut c, 0.2, |_, _| Input::default());
    let rocket = sig_of(sim.io.as_ref().unwrap(), "rocket_1").expect("rocket_1");
    sim.io_signals[rocket] = true;
    let (centre, _) = sim.rocket_targets[0];
    let from = centre + Vec3::new(0.0, 0.0, 6.0);
    sim.rockets.push(crate::sim::Rocket { id: 99, pos: from, vel: (centre - from).normalize() * crate::sim::ROCKET_SPEED, age: 0.0, alive: true });
    let ev = run(&mut sim, &mut c, 1.5, |_, _| Input::default());
    assert!(ev.iter().any(|e| matches!(e, SimEvent::RocketExploded { target: Some(0), .. })), "missed GLaDOS");
    assert!(!sim.io_signals[rocket], "the turret's still on");
}

/// Every one of Portal's maps leads on to the next (#21): its exit elevator, ridden from its
/// car, or the way out she walks into (the escape's), ends the map with the next one named.
#[test]
fn portals_maps_lead_on() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
    let order = [
        "testchmb_a_00", "testchmb_a_01", "testchmb_a_02", "testchmb_a_03", "testchmb_a_04", "testchmb_a_05", "testchmb_a_06", "testchmb_a_07", "testchmb_a_08",
        "testchmb_a_09", "testchmb_a_10", "testchmb_a_11", "testchmb_a_13", "testchmb_a_14", "testchmb_a_15", "escape_00", "escape_01", "escape_02",
    ];
    let mut bad = Vec::new();
    let mut seen = 0;
    for w in order.windows(2) {
        let (name, next) = (w[0], w[1]);
        let text = |ext: &str| std::fs::read_to_string(dir.join(format!("{name}.{ext}")));
        let (Ok(solids), Ok(ents)) = (text("solids"), text("ents")) else { continue };
        seen += 1;
        let level = crate::source::level(name, "map", &solids, &ents);
        let (sim, _) = start(level.clone());
        let io = sim.io.as_ref().unwrap();
        let Some((k, touch)) = io.ents.iter().enumerate().find_map(|(k, e)| match &e.kind {
            crate::io::Kind::ChangeLevel { map, touch, .. } if map == next => Some((k, *touch)),
            _ => None,
        }) else {
            bad.push(format!("{name}: nothing leads to {next}"));
            continue;
        };
        let (mut sim, mut c) = start(level);
        let home = c.feet;
        run(&mut sim, &mut c, 0.2, |_, _| Input::default());
        match touch {
            Some(b) => {
                // (From its top: escape_01's is a shaft she drops down.)
                c.feet = Vec3::new(b.center().x, (b.max.y - 1.9).max(b.min.y + 0.05), b.center().z);
                run(&mut sim, &mut c, 1.0, |_, _| Input::default());
            }
            None => {
                // The exit elevator: the train whose path ends at the change.
                let target = io.ents[k].name.clone();
                let io = sim.io.as_ref().unwrap();
                let train = io.ents.iter().find_map(|e| match e.kind {
                    crate::io::Kind::PathNode { mover, .. } if e.outputs.iter().any(|o| o.target == target && o.input.eq_ignore_ascii_case("ChangeLevel")) => Some(mover),
                    _ => None,
                });
                let Some(train) = train else {
                    bad.push(format!("{name}: no exit elevator ends at {target}"));
                    continue;
                };
                let floor_of = |sim: &Sim| sim.movers[train].boxes().find(|b| b.size().x > 3.0 && b.size().z > 3.0 && b.size().y < 0.3);
                if floor_of(&sim).is_none() {
                    bad.push(format!("{name}: the exit elevator has no floor"));
                    continue;
                }
                // What she'd have passed through on her way there first: the triggers that
                // (through scenes and relays) switch on the ones that start the ride.
                let train_name = io.ents.iter().find(|e| matches!(e.kind, crate::io::Kind::Mover { index, .. } if index == train)).map(|e| e.name.clone()).unwrap_or_default();
                let mut before = Vec::new();
                let mut starters = Vec::new();
                for (r, e) in io.ents.iter().enumerate() {
                    if let crate::io::Kind::Trigger { b, .. } = e.kind
                        && e.outputs.iter().any(|o| o.target == train_name && o.input.eq_ignore_ascii_case("StartForward"))
                    {
                        on_the_way(io, r, 0, &mut before);
                        starters.push(b);
                    }
                }
                for t in before {
                    let crate::io::Kind::Trigger { b, .. } = sim.io.as_ref().unwrap().ents[t].kind else { continue };
                    visit(&mut sim, &mut c, b, home, 5.0);
                }
                // The trigger in the car starts the ride; any others call it to her first (a
                // car waiting up its shaft comes down to its stop).
                let in_car = |sim: &Sim| {
                    let car = floor_of(sim)?;
                    let room = Aabb::new(car.min - Vec3::new(0.3, 0.5, 0.3), car.max + Vec3::new(0.3, 3.0, 0.3));
                    Some((car, starters.iter().copied().find(|b| b.overlaps(&room))))
                };
                if in_car(&sim).is_some_and(|(_, ride)| ride.is_none()) {
                    for &b in &starters {
                        visit(&mut sim, &mut c, b, home, 20.0);
                    }
                }
                let Some((car, ride)) = in_car(&sim) else { continue };
                // In the car, where its trigger is (some cover only its back half).
                let at = ride.map_or(car.center(), |b| b.center());
                c.feet = Vec3::new(at.x, car.max.y + 0.05, at.z);
                c.state = State::Ground;
                run(&mut sim, &mut c, 60.0, |_, _| Input::default());
            }
        }
        match &sim.transition {
            Some(t) if t.map == next => {}
            Some(t) => bad.push(format!("{name}: led to {} instead of {next}", t.map)),
            None => bad.push(format!("{name}: never led on to {next} (feet {})", c.feet)),
        }
    }
    if seen > 0 {
        assert_eq!(seen, 17);
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// She walks into trigger `b` (onto the floor in it), then goes back to `home` while what it
/// started plays out (anywhere off a floor she'd fall to her death).
fn visit(sim: &mut Sim, c: &mut Controller, b: Aabb, home: Vec3, wait: f32) {
    let top = Vec3::new(b.center().x, b.max.y - 0.1, b.center().z);
    let floor = sim.raycast(top, Vec3::NEG_Y, b.size().y, crate::sim::Mask::SOLID).map_or(b.min.y, |h| h.point.y);
    c.feet = Vec3::new(top.x, floor + 0.05, top.z);
    c.state = State::Ground;
    run(sim, c, 0.1, |_, _| Input::default());
    c.feet = home;
    c.state = State::Ground;
    run(sim, c, wait, |_, _| Input::default());
}

/// The triggers she passes through that, through scenes and relays, switch on trigger `e`
/// (none, if it starts on), earliest first.
fn on_the_way(io: &crate::io::Io, e: usize, depth: usize, out: &mut Vec<usize>) {
    use crate::io::Kind;
    let on = match io.ents[e].kind {
        Kind::Trigger { enabled, .. } => enabled,
        _ => false,
    };
    if on || depth > 5 {
        if depth > 0 && matches!(io.ents[e].kind, Kind::Trigger { players: true, .. }) && !out.contains(&e) {
            out.push(e);
        }
        return;
    }
    let name = io.ents[e].name.to_ascii_lowercase();
    let wakes = |i: &str| ["enable", "start", "trigger"].iter().any(|w| i.eq_ignore_ascii_case(w));
    for (k, p) in io.ents.iter().enumerate() {
        if k != e && !name.is_empty() && p.outputs.iter().any(|o| o.target == name && wakes(&o.input)) {
            on_the_way(io, k, depth + 1, out);
        }
    }
}
