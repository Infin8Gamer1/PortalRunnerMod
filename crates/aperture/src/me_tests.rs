//! Mirror's Edge's levels as the game's own (`levels/me/`, #100): they build, she stands
//! where they start her, and their ladders, zipline, swing poles and beam work.

use faith_move::{Controller, Fixture, Input, State};
use glam::{Vec2, Vec3};

use crate::doc::LevelDoc;
use crate::sim::{Sim, SimEvent};

const DT: f32 = 1.0 / 60.0;

fn level(file: &str) -> Option<crate::Level> {
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
        // From a couple of metres out, walking at it.
        let at = x.base + x.normal * 1.5;
        let mut r = Run::new(l.clone(), at, yaw_to(-x.normal));
        r.run(1.0, |_| Input::default());
        let ground = r.c.feet.y;
        // (A ladder that starts high up a wall, reached by climbing the wall: ours are only
        // got onto from their foot, so far.)
        if x.base.y - ground > 1.0 {
            eprintln!("ladder at {}: starts {:.1} m up the wall: not tried", x.base, x.base.y - ground);
            continue;
        }
        let mut climbed = false;
        let top = x.top;
        r.run(4.0 + (x.top - x.base.y) * 1.2, |c| {
            climbed |= matches!(c.state, State::Climb { .. } | State::IntoClimb { .. });
            if climbed && c.state == State::Ground && c.feet.y > top - 0.3 { Input::default() } else { forward() }
        });
        assert!(climbed, "ladder at {}: not climbed (stood at {ground}, ended {:?} {:?})", x.base, r.c.feet, r.c.state);
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
    let (a, b) = l.fixtures.iter().find_map(|f| if let Fixture::ZipLine { a, b } = f { Some((*a, *b)) } else { None }).expect("a zipline");
    assert!(a.y > b.y && a.distance(b) > 50.0, "{a} -> {b}");
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
        let cp = l.checkpoints[0].clone();
        let mut r = Run::new(l, cp.spawn, cp.yaw);
        r.run(2.0, |_| Input::default());
        assert_eq!(r.deaths, 0, "{file}");
        assert!(r.c.state == State::Ground && r.c.feet.distance(cp.spawn) < 0.5, "{file}: she fell from her start: {:?} {:?}", r.c.feet, r.c.state);
    }
}
