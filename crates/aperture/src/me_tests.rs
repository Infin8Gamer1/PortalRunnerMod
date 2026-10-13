//! Mirror's Edge's levels as the game's own (`levels/me/`, #100): they build, she stands
//! where they start her, and their ladders, zipline, swing poles and beam work. (Their props
//! collide with their meshes' shapes, read from the game: without it, these don't run.)

use faith_move::{Controller, Fixture, Input, State};
use glam::{Vec2, Vec3};

use crate::doc::LevelDoc;
use crate::sim::{Sim, SimEvent};

const DT: f32 = 1.0 / 60.0;

/// A level, if it's been made and the game's there to read its props' shapes from.
fn level(file: &str) -> Option<crate::Level> {
    if !crate::me::installed() {
        return None;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../levels/me").join(format!("{file}.ron"));
    let doc = LevelDoc::from_ron(&std::fs::read_to_string(path).ok()?).ok()?;
    Some(doc.build())
}

struct Run {
    sim: Sim,
    c: Controller,
    deaths: usize,
}

impl Run {
    fn new(l: crate::Level, at: Vec3, yaw: f32) -> Run {
        let sim = Sim::new(l);
        let mut c = Controller::new(crate::tuning(5.3, 15.0), at, yaw);
        c.state = State::Ground;
        Run { sim, c, deaths: 0 }
    }

    fn run(&mut self, secs: f32, mut input: impl FnMut(&Controller) -> Input) {
        for _ in 0..(secs / DT) as usize {
            let i = input(&self.c);
            self.sim.step_player(&mut self.c, &i, DT);
            self.sim.step(DT, &mut self.c);
            self.deaths += self.sim.events.iter().filter(|e| **e == SimEvent::PlayerDied).count();
            self.sim.events.clear();
        }
    }
}

fn forward() -> Input {
    Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() }
}

/// Faces `dir` (on the ground): Faith's yaw.
fn yaw_to(dir: Vec3) -> f32 {
    (-dir.x).atan2(-dir.z)
}

#[test]
fn training_builds_and_she_stands_at_its_start() {
    let Some(l) = level("tutorial") else { return };
    assert!(l.me_art && l.me_props.len() > 2000, "{} props", l.me_props.len());
    assert!(l.map_solids > 50_000, "its collision isn't drawn as boxes: {} of {}", l.map_solids, l.solids.len());
    let cp = l.checkpoints[0].clone();
    let mut r = Run::new(l, cp.spawn, cp.yaw);
    r.run(2.0, |_| Input::default());
    assert_eq!(r.deaths, 0);
    assert!(r.c.state == State::Ground && r.c.feet.distance(cp.spawn) < 0.3, "she fell: {:?} {:?}", r.c.feet, r.c.state);
    // And walks off along the roof without dying.
    r.run(1.5, |_| forward());
    assert_eq!(r.deaths, 0);
}

#[test]
fn training_ladders_climb() {
    let Some(l) = level("tutorial") else { return };
    let ladders: Vec<_> = l.fixtures.iter().filter_map(|f| if let Fixture::Ladder(x) = f { Some(*x) } else { None }).collect();
    assert_eq!(ladders.len(), 4);
    for x in ladders {
        // From a few metres out on the floor in front of it, running at it.
        let at = x.base + x.normal * 3.0;
        let mut r = Run::new(l.clone(), at, yaw_to(-x.normal));
        r.run(1.0, |_| Input::default());
        let ground = r.c.feet.y;
        // One that starts high up a wall (#102) is reached from the air: she's dropped in front
        // of its lowest step, as if she'd jumped to it, and catches it.
        let high = x.base.y - ground > 1.0;
        if high {
            r = Run::new(l.clone(), x.base + x.normal * 0.6, yaw_to(-x.normal));
            r.c.state = State::Air;
        }
        let mut climbed = false;
        let top = x.top;
        r.run(4.0 + (x.top - ground) * 1.2, |c| {
            climbed |= matches!(c.state, State::Climb { .. } | State::IntoClimb { .. });
            if climbed && c.state == State::Ground && c.feet.y > top - 0.3 {
                return Input::default();
            }
            forward()
        });
        assert!(climbed, "ladder at {} ({}): not climbed (stood at {ground}, ended {:?} {:?})", x.base, if high { "up the wall" } else { "on the floor" }, r.c.feet, r.c.state);
        // Up and out onto the top, where it lets her out.
        if x.exit {
            assert!(r.c.state == State::Ground && (r.c.feet.y - x.top).abs() < 0.3, "ladder at {}: not out on top ({}): {:?} {:?}", x.base, x.top, r.c.feet, r.c.state);
        }
        assert_eq!(r.deaths, 0);
    }
}

/// The top of the highest solid under `p` (within `depth`).
fn floor_under(l: &crate::Level, p: Vec3, depth: f32) -> Option<f32> {
    l.solids
        .iter()
        .filter(|s| p.x > s.b.min.x && p.x < s.b.max.x && p.z > s.b.min.z && p.z < s.b.max.z && s.b.max.y <= p.y + 0.01 && s.b.max.y > p.y - depth)
        .map(|s| s.b.max.y)
        .fold(None, |m: Option<f32>, y| Some(m.map_or(y, |m| m.max(y))))
}

/// The props of a mesh whose name has `part`, as boxes.
fn props_named(l: &crate::Level, part: &str) -> Vec<faith_move::Aabb> {
    l.me_props.iter().filter(|p| p.mesh.contains(part)).map(|p| faith_move::Aabb::from_center_size(p.at, Vec3::splat(0.2))).collect()
}

#[test]
fn training_zipline_runs_between_its_anchors() {
    let Some(l) = level("tutorial") else { return };
    let (a, b, sag) = l.fixtures.iter().find_map(|f| if let Fixture::ZipLine { a, b, sag } = f { Some((*a, *b, *sag)) } else { None }).expect("a zipline");
    assert!(a.y > b.y && a.distance(b) > 50.0, "{a} -> {b}");
    // Its cable sags as the game's does (#103).
    assert!((0.5..2.0).contains(&sag), "sag {sag}");
    // Its anchors are the level's (ZipLineBase), at both ends.
    let bases = props_named(&l, "ZipLineBase");
    for end in [a, b] {
        let d = bases.iter().map(|x| x.center().distance(end)).fold(f32::MAX, f32::min);
        assert!(d < 6.0, "no anchor near {end}: nearest {d} m");
    }
    // Its top is a jump up from a floor (2.2-3.2 m: hands at 1.8, a jump's metre).
    let y = floor_under(&l, a - Vec3::new(b.x - a.x, 0.0, b.z - a.z).normalize(), 4.0).expect("a floor under its top");
    assert!((2.2..3.2).contains(&(a.y - y)), "its top {} m over the floor", a.y - y);
}

#[test]
fn training_swing_poles_are_the_level_s_poles() {
    let Some(l) = level("tutorial") else { return };
    let poles: Vec<_> = l.me_props.iter().filter(|p| p.mesh.starts_with("S_SwingPole")).map(|p| p.at).collect();
    let mut n = 0;
    for (a, b) in l.fixtures.iter().filter_map(|f| if let Fixture::SwingPole { a, b } = f { Some((*a, *b)) } else { None }) {
        n += 1;
        let mid = (a + b) * 0.5;
        assert!((a.y - b.y).abs() < 0.01, "level: {a} -> {b}");
        // A pole's mesh along it, at its height.
        let on = poles.iter().any(|p| (p.y - mid.y).abs() < 0.3 && {
            let u = (b - a).normalize();
            let d = *p - a;
            (d - u * d.dot(u)).length() < 0.3
        });
        assert!(on, "swing pole {a} -> {b}: no pole mesh along it (poles at {poles:?})");
    }
    assert_eq!(n, 2);
}

#[test]
fn training_beam_is_walked() {
    let Some(l) = level("tutorial") else { return };
    let (a, b) = l.fixtures.iter().find_map(|f| if let Fixture::Beam { a, b } = f { Some((*a, *b)) } else { None }).expect("a beam");
    let u = (b - a).normalize();
    // Its line is the top of what it is: standing on its end, she's on it.
    let top = floor_under(&l, (a + b) * 0.5 + Vec3::Y * 0.3, 1.0).expect("something under the beam");
    assert!((top - (a.y + b.y) * 0.5).abs() < 0.05, "the beam's line {} isn't on its top {top}", (a.y + b.y) * 0.5);
    let mut r = Run::new(l.clone(), a + u * 0.3, yaw_to(Vec3::new(u.x, 0.0, u.z)));
    let mut balanced = false;
    r.run(8.0, |c| {
        balanced |= matches!(c.state, State::Balance { .. });
        forward()
    });
    assert!(balanced, "the beam from {a}: not walked (ended {:?} {:?})", r.c.feet, r.c.state);
    assert_eq!(r.deaths, 0);
}

#[test]
fn time_trials_start_her_on_their_course() {
    for (file, checkpoints) in [("tt_tutorial_a01", 4), ("tt_tutorial_a02", 9), ("tt_tutorial_a03", 11)] {
        let Some(l) = level(file) else { continue };
        // Their start, their checkpoints, their finish line.
        assert_eq!(l.checkpoints.len(), checkpoints + 1, "{file}");
        assert!(l.exit.is_some(), "{file}: no finish");
        assert!(l.course, "{file}: its checkpoints aren't a course (#104)");
        let cp = l.checkpoints[0].clone();
        let mut r = Run::new(l, cp.spawn, cp.yaw);
        r.run(2.0, |_| Input::default());
        assert_eq!(r.deaths, 0, "{file}");
        assert!(r.c.state == State::Ground && r.c.feet.distance(cp.spawn) < 0.5, "{file}: she fell from her start: {:?} {:?}", r.c.feet, r.c.state);
    }
}

/// Every ported level (#100): it builds, and she stands where it starts her, alive, for two
/// seconds.
#[test]
fn every_level_starts_her_on_her_feet() {
    if !crate::me::installed() {
        return;
    }
    let mut problems = vec![];
    for (_, file, _) in crate::me::MAPS {
        let Some(l) = level(file) else { continue };
        // (Its ladders are ladders: a volume whose path was never set made one millions of
        // metres tall.)
        for f in &l.fixtures {
            if let Fixture::Ladder(x) = f {
                if !(0.5..60.0).contains(&(x.top - x.base.y)) {
                    problems.push(format!("{file}: a ladder at {} {} m tall", x.base, x.top - x.base.y));
                }
            }
        }
        let cp = l.checkpoints[0].clone();
        let props = l.me_props.len();
        let mut r = Run::new(l, cp.spawn, cp.yaw);
        r.run(2.0, |_| Input::default());
        let ok = r.deaths == 0 && r.c.state == State::Ground && r.c.feet.distance(cp.spawn) < 0.5;
        eprintln!("{file}: {props} props, start {} -> {:?} {}", cp.spawn, r.c.feet, if ok { "ok" } else { "FELL" });
        if !ok {
            problems.push(format!("{file}: from {} to {:?} ({:?}, {} deaths)", cp.spawn, r.c.feet, r.c.state, r.deaths));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Movers (#109): their Kismet plays them. What the level sets going as it loads (Edge's
/// traffic below) moves as she stands there.
#[test]
fn movers_play_as_their_kismet_says() {
    for file in ["edge", "escape"] {
        let Some(l) = level(file) else { continue };
        let movers = l.me_movers.len();
        let cp = l.checkpoints[0].clone();
        let mut r = Run::new(l, cp.spawn, cp.yaw);
        let at0: Vec<(Vec3, glam::Quat)> = r.sim.me.as_ref().map(|m| m.poses.clone()).unwrap_or_default();
        let t = std::time::Instant::now();
        r.run(3.0, |_| Input::default());
        let per_step = t.elapsed().as_secs_f32() / 180.0 * 1000.0;
        let poses = r.sim.me.as_ref().map(|m| m.poses.clone()).unwrap_or_default();
        let moved = poses.iter().zip(&at0).filter(|(a, b)| a.0.distance(b.0) > 0.05 || a.1.angle_between(b.1) > 0.01).count();
        eprintln!("{file}: {movers} movers, {moved} moved in 3 s ({per_step:.2} ms a step)");
        // (Fast enough to run every frame.)
        assert!(per_step < 4.0, "{file}: {per_step} ms a step");
        assert!(movers > 0, "{file}: no movers");
        assert!(moved > 0, "{file}: none moved");
        assert_eq!(r.deaths, 0);
    }
}

/// A door kicked open (#109): running into one of Escape's doors sets off its Kismet, and
/// its matinee swings it.
#[test]
fn running_into_a_door_kicks_it_open() {
    let Some(l) = level("escape") else { return };
    let door = l.me_logic.iter().find(|o| o.name == "Escape_Off_Spt.SeqEvent_TakeDamage_2").expect("the door's event").regions[0];
    // (From whichever side has a floor to run on.)
    let mut opened = false;
    for side in [1.0f32, -1.0] {
        let n = Vec3::new(0.0, 0.0, side);
        let start = Vec3::new(door.center().x, door.min.y + 0.05, door.center().z) + n * 4.0;
        let mut r = Run::new(l.clone(), start, yaw_to(-n));
        let before = r.sim.me.as_ref().unwrap().poses.clone();
        r.run(0.5, |_| Input::default());
        if (r.c.feet.y - start.y).abs() > 0.3 {
            continue;
        }
        r.run(2.5, |_| forward());
        let after = r.sim.me.as_ref().unwrap().poses.clone();
        // (The door's own: its movers by it.)
        let near: Vec<usize> = r.sim.level.me_movers.iter().enumerate().filter(|(_, m)| m.pivot.distance(door.center()) < 2.5).map(|(k, _)| k).collect();
        let moved = near.iter().filter(|&&k| after[k].0.distance(before[k].0) > 0.05 || after[k].1.angle_between(before[k].1) > 0.05).count();
        eprintln!("side {side}: ended {:?}, {moved} of the {} movers by the door moved", r.c.feet, near.len());
        // (And she runs on through it.)
        let through = (r.c.feet - door.center()).dot(n) < -0.5;
        opened |= moved > 0 && through;
    }
    assert!(opened, "the door didn't open, or she didn't get through it");
}



