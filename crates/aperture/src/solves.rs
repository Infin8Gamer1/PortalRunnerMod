//! Portal's chambers played through by Faith (#21): each map from its start to its exit, with
//! her own movement. She's walked along paths found over the map's floors (A* over 0.25 m
//! cells: where a box's top has room for her above it, stepping up as far as Source's stairs
//! and dropping down), shoots her portals at the walls Portal's solution uses, carries cubes to
//! buttons and rides the lifts. A chamber that can't be finished this way fails here, saying
//! where she got stuck.
//!
//! The maps are read from `assets/extracted/maps` (`cargo run --release -p portal_extract`);
//! without them these pass trivially.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

use faith_move::{Aabb, Controller, Input, State};
use glam::{Vec2, Vec3};

use crate::portal::PortalColor;
use crate::sim::{Aim, Sim, SimEvent};

const DT: f32 = 1.0 / 60.0;
/// The path-finder's cells.
const CELL: f32 = 0.25;
/// How far up a step she takes in her stride (Source's 18 units, as `crate::tuning` sets),
/// and how far down she'll drop.
const STEP: f32 = 0.65;
const DROP: f32 = 4.0;
/// The map's boxes are bucketed in columns this wide.
const BUCKET: f32 = 2.0;

/// A chamber being played.
pub struct Play {
    pub name: String,
    pub sim: Sim,
    pub c: Controller,
    pub events: Vec<SimEvent>,
    pub time: f32,
    /// The map's still boxes, bucketed by column.
    boxes: Vec<Aabb>,
    buckets: HashMap<(i32, i32), Vec<usize>>,
    deaths: usize,
}

/// A path-finder's node: a cell and the floor height in it (in centimetres).
type Node = (i32, i32, i32);

#[derive(PartialEq)]
struct Open(f32, Node);
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0)
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

impl Play {
    /// Portal's map `name` (with GLaDOS's scenes, so the waits are hers), or `None` when it
    /// hasn't been extracted.
    pub fn load(name: &str) -> Option<Play> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/extracted/maps");
        let text = |ext: &str| std::fs::read_to_string(dir.join(format!("{name}.{ext}")));
        let (Ok(solids), Ok(ents)) = (text("solids"), text("ents")) else { return None };
        let scenes = std::fs::read_to_string(dir.join("scenes.txt")).map(|t| crate::source::parse_scenes(&t)).unwrap_or_default();
        let level = crate::source::level_with_scenes(name, "map", &solids, &ents, &scenes);
        let cp = level.checkpoints[0].clone();
        let sim = Sim::new(level);
        let mut c = Controller::new(crate::tuning(5.3, 10.0), cp.spawn, cp.yaw);
        c.state = State::Ground;
        let boxes: Vec<Aabb> = sim.statics.iter().map(|s| s.b).collect();
        let mut buckets: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, b) in boxes.iter().enumerate() {
            for bx in (b.min.x / BUCKET).floor() as i32..=(b.max.x / BUCKET).floor() as i32 {
                for bz in (b.min.z / BUCKET).floor() as i32..=(b.max.z / BUCKET).floor() as i32 {
                    buckets.entry((bx, bz)).or_default().push(i);
                }
            }
        }
        Some(Play { name: name.to_string(), sim, c, events: Vec::new(), time: 0.0, boxes, buckets, deaths: 0 })
    }

    /// One frame with this input.
    pub fn step(&mut self, input: &Input) {
        self.sim.events.clear();
        self.sim.step_player(&mut self.c, input, DT);
        self.sim.step(DT, &mut self.c);
        for e in &self.sim.events {
            if *e == SimEvent::PlayerDied {
                self.deaths += 1;
            }
        }
        self.events.extend(self.sim.events.iter().copied());
        self.time += DT;
    }

    /// Stand still this long.
    pub fn wait(&mut self, secs: f32) {
        let end = self.time + secs;
        while self.time < end {
            self.step(&Input::default());
        }
    }

    /// Stand still until `done` (or fail after `secs`).
    pub fn wait_for(&mut self, secs: f32, what: &str, mut done: impl FnMut(&Play) -> bool) {
        let end = self.time + secs;
        while !done(self) {
            assert!(self.time < end, "{}: waited {secs} s for {what} (at {})", self.name, self.c.feet);
            self.step(&Input::default());
        }
    }

    /// She's died (and gone back to her checkpoint) since the start.
    pub fn assert_alive(&self, doing: &str) {
        assert_eq!(self.deaths, 0, "{}: died {doing} (at {})", self.name, self.c.feet);
    }

    /// The map's boxes in the columns around `b`, and the movers' (doors, lifts) where they
    /// are now.
    fn near(&self, b: &Aabb) -> impl Iterator<Item = Aabb> + '_ {
        let (bx0, bx1) = ((b.min.x / BUCKET).floor() as i32, (b.max.x / BUCKET).floor() as i32);
        let (bz0, bz1) = ((b.min.z / BUCKET).floor() as i32, (b.max.z / BUCKET).floor() as i32);
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for bx in bx0..=bx1 {
            for bz in bz0..=bz1 {
                for &i in self.buckets.get(&(bx, bz)).into_iter().flatten() {
                    if seen.insert(i) && self.boxes[i].overlaps(b) {
                        out.push(self.boxes[i]);
                    }
                }
            }
        }
        out.extend(self.sim.movers.iter().flat_map(|m| m.boxes()).filter(|m| m.overlaps(b)));
        out.extend(self.sim.doors.iter().filter(|d| d.solid()).map(|d| d.b).filter(|d| d.overlaps(b)));
        out.into_iter()
    }

    /// Whether she fits standing with her feet here.
    fn fits(&self, feet: Vec3) -> bool {
        let body = self.c.body();
        let b = Aabb::new(feet + Vec3::new(-body.half_width + 0.02, 0.02, -body.half_width + 0.02), feet + Vec3::new(body.half_width - 0.02, body.height - 0.05, body.half_width - 0.02));
        self.near(&b).next().is_none() && !self.sim.goo.iter().any(|g| g.overlaps(&b))
    }

    /// The floors she can stand on in the cell at (x, z), between `lo` and `hi`.
    fn floors(&self, x: f32, z: f32, lo: f32, hi: f32) -> Vec<f32> {
        // (Anything under her footprint: she stands on a step's edge.)
        let hw = self.c.body().half_width - 0.05;
        let col = Aabb::new(Vec3::new(x - hw, lo - 0.01, z - hw), Vec3::new(x + hw, hi + 0.01, z + hw));
        let mut tops: Vec<f32> = self.near(&col).map(|b| b.max.y).filter(|y| *y >= lo && *y <= hi).collect();
        // Not goo.
        tops.retain(|y| !self.sim.goo.iter().any(|g| g.max.y >= *y - 0.05 && geom_in(g, x, z)));
        tops.sort_by(|a, b| a.total_cmp(b));
        tops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        tops.into_iter().filter(|y| self.fits(Vec3::new(x, *y, z))).collect()
    }

    /// A path over the floors from her feet to within `reach` of `to` (horizontally, and at
    /// about its height), as the cells' feet points.
    pub fn path(&self, to: Vec3, reach: f32) -> Option<Vec<Vec3>> {
        let cell = |p: Vec3| ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32);
        let centre = |i: i32, k: i32| Vec2::new((i as f32 + 0.5) * CELL, (k as f32 + 0.5) * CELL);
        let at = self.c.feet;
        let (si, sk) = cell(at);
        let c0 = centre(si, sk);
        let start_y = self.floors(c0.x, c0.y, at.y - 1.0, at.y + 0.3).into_iter().min_by(|a, b| (a - at.y).abs().total_cmp(&(b - at.y).abs()))?;
        let start: Node = (si, sk, (start_y * 100.0).round() as i32);
        let goal = |n: &Node| {
            let c = centre(n.0, n.1);
            Vec2::new(c.x - to.x, c.y - to.z).length() <= reach && (n.2 as f32 / 100.0 - to.y).abs() < 1.2
        };
        let h = |n: &Node| {
            let c = centre(n.0, n.1);
            Vec2::new(c.x - to.x, c.y - to.z).length()
        };
        let mut open = BinaryHeap::new();
        let mut cost: HashMap<Node, f32> = HashMap::new();
        let mut from: HashMap<Node, Node> = HashMap::new();
        let mut floors_at: HashMap<(i32, i32), Vec<f32>> = HashMap::new();
        cost.insert(start, 0.0);
        open.push(Open(h(&start), start));
        let mut expanded = 0;
        while let Some(Open(_, n)) = open.pop() {
            if goal(&n) {
                let mut path = vec![n];
                let mut k = n;
                while let Some(&p) = from.get(&k) {
                    path.push(p);
                    k = p;
                }
                path.reverse();
                return Some(path.into_iter().map(|(i, k, y)| { let c = centre(i, k); Vec3::new(c.x, y as f32 / 100.0, c.y) }).collect());
            }
            expanded += 1;
            if expanded > 600_000 {
                eprintln!("path: gave up after {expanded} cells");
                return None;
            }
            let g = cost[&n];
            let y = n.2 as f32 / 100.0;
            for (di, dk) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let (i, k) = (n.0 + di, n.1 + dk);
                let fl = floors_at.entry((i, k)).or_insert_with(|| {
                    let c = centre(i, k);
                    self.floors(c.x, c.y, y - 40.0, y + 40.0)
                });
                // The floor she'd step onto: the highest she can step up to, or the one she'd
                // drop to (not too far).
                let next = fl.iter().copied().filter(|f| *f <= y + STEP && *f >= y - DROP).fold(None, |a: Option<f32>, f| Some(a.map_or(f, |a| a.max(f))));
                let Some(ny) = next else { continue };
                // Diagonals only where both sides are open too.
                if di != 0 && dk != 0 {
                    let side = |i: i32, k: i32| {
                        let c = centre(i, k);
                        self.fits(Vec3::new(c.x, y.max(ny), c.y))
                    };
                    if !side(n.0 + di, n.1) || !side(n.0, n.1 + dk) {
                        continue;
                    }
                }
                let m: Node = (i, k, (ny * 100.0).round() as i32);
                let step = if di != 0 && dk != 0 { CELL * std::f32::consts::SQRT_2 } else { CELL } + (y - ny).max(0.0) * 0.5;
                let ng = g + step;
                if cost.get(&m).is_none_or(|&c| ng < c) {
                    cost.insert(m, ng);
                    from.insert(m, n);
                    open.push(Open(ng + h(&m), m));
                }
            }
        }
        let (mut lo, mut hi) = (Vec3::splat(1e9), Vec3::splat(-1e9));
        for n in cost.keys() {
            let c = centre(n.0, n.1);
            let v = Vec3::new(c.x, n.2 as f32 / 100.0, c.y);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        eprintln!("path: searched all {expanded} cells, within {lo}..{hi}, start {start:?}");
        None
    }

    /// Walk to `to` (within `reach`), along a path over the floors. Fails if there's no way
    /// there, or she doesn't get there.
    pub fn go(&mut self, to: Vec3, reach: f32) {
        for attempt in 0..4 {
            let Some(path) = self.path(to, reach) else {
                panic!("{}: no way from {} to {to}", self.name, self.c.feet);
            };
            if self.follow(&path, to, reach) {
                return;
            }
            assert!(attempt < 3, "{}: couldn't walk to {to} (stuck at {})", self.name, self.c.feet);
        }
    }

    /// Steer her along `path` (a little ahead of where she is on it) until she's within
    /// `reach` of `to`; false if she stops getting closer.
    fn follow(&mut self, path: &[Vec3], to: Vec3, reach: f32) -> bool {
        let mut on = 0usize;
        let mut best = f32::MAX;
        let mut since = 0.0;
        loop {
            let feet = self.c.feet;
            let flat = |p: Vec3| Vec2::new(p.x - feet.x, p.z - feet.z).length();
            if flat(to) <= reach.max(0.2) && (feet.y - to.y).abs() < 1.2 && self.c.state == State::Ground {
                return true;
            }
            // Where she is along it (looking a little way ahead of the last point).
            for k in on..(on + 12).min(path.len()) {
                if flat(path[k]) < flat(path[on]) {
                    on = k;
                }
            }
            let ahead = path[(on + 3).min(path.len() - 1)];
            let d = Vec2::new(ahead.x - feet.x, ahead.z - feet.z);
            let left = path.len() - 1 - on;
            if d.length() > 1e-3 {
                self.c.yaw = (-d.x).atan2(-d.y);
            }
            // Walk the last few cells (she runs fast, and would overshoot).
            let speed = if left < 6 { 0.35 } else { 1.0 };
            self.step(&Input { move_axis: Vec2::new(0.0, speed), ..Default::default() });
            let dist = flat(to);
            if dist < best - 0.05 {
                best = dist;
                since = 0.0;
            } else {
                since += DT;
                if since > 2.5 {
                    return false;
                }
            }
        }
    }

    /// Edge straight to `to` (a short way, slowly), keeping her view.
    pub fn nudge(&mut self, to: Vec3) {
        let end = self.time + 2.0;
        while self.time < end {
            let d = Vec2::new(to.x - self.c.feet.x, to.z - self.c.feet.z);
            if d.length() < 0.06 {
                break;
            }
            // Into her frame: forward is -Z turned by yaw, right is +X turned by yaw.
            let (s, c) = self.c.yaw.sin_cos();
            let fwd = Vec2::new(-s, -c);
            let right = Vec2::new(c, -s);
            let k = (d.length() * 1.5).min(0.3) / d.length();
            self.step(&Input { move_axis: Vec2::new(d.dot(right), d.dot(fwd)) * k, ..Default::default() });
        }
        self.wait(0.3);
    }

    /// Turn to look at `p`.
    pub fn look_at(&mut self, p: Vec3) {
        let eye = self.c.view().eye;
        let d = p - eye;
        self.c.yaw = (-d.x).atan2(-d.z);
        self.c.pitch = (d.y / Vec2::new(d.x, d.z).length().max(1e-3)).atan();
    }

    #[allow(dead_code)]
    /// Shoot a portal of `color` at the point `at` (from where she stands): it must open
    /// within a portal's size of it.
    pub fn shoot(&mut self, color: PortalColor, at: Vec3) {
        self.look_at(at);
        let eye = self.c.view().eye;
        let aim = Aim { eye, dir: (at - eye).normalize(), yaw: self.c.yaw };
        let opened = self.sim.fire(color, aim);
        let p = self.sim.portals[color.index()];
        assert!(opened && p.is_some_and(|p| p.center.distance(at) < 1.6), "{}: the {color:?} portal didn't open at {at} (from {eye}; it's at {:?})", self.name, p.map(|p| p.center));
        self.step(&Input::default());
    }

    /// Walk into the portal of `color` and out of the other: she ends up at least `out`
    /// beyond the far one.
    pub fn through(&mut self, color: PortalColor, out: f32) {
        assert!(self.through_at(color, None, None, out));
    }

    /// As [`through`](Self::through); with `at`, false if that portal goes from there before
    /// she's in it.
    fn through_at(&mut self, color: PortalColor, at: Option<Vec3>, to: Option<Vec3>, out: f32) -> bool {
        let p = self.sim.portals[color.index()].expect("no portal");
        let q = self.sim.portals[color.other().index()].expect("no other portal");
        let end = self.time + 6.0;
        let mut entered = false;
        loop {
            entered |= self.sim.player_env == Some(color);
            let gone = |c: PortalColor, want: Option<Vec3>| want.is_some_and(|w| !self.sim.portals[c.index()].is_some_and(|x| x.center.distance(w) < 1.0));
            if !entered && (gone(color, at) || gone(color.other(), to)) {
                return false;
            }
            let feet = self.c.feet;
            // Past the far portal's mouth (in front of it).
            let past = (feet - q.center).dot(q.normal);
            if past > out && Vec2::new(feet.x - q.center.x, feet.z - q.center.z).length() < out + 3.0 {
                break;
            }
            assert!(self.time < end, "{}: didn't get through the {color:?} portal (at {feet})", self.name);
            // Into this one while she's on its side; on out of the far one once through.
            let near_side = feet.distance(p.center) < feet.distance(q.center);
            let d = if near_side { p.center - feet } else { q.normal };
            if Vec2::new(d.x, d.z).length() > 1e-3 {
                self.c.yaw = (-d.x).atan2(-d.z);
            }
            self.step(&Input { move_axis: Vec2::new(0.0, 0.6), ..Default::default() });
        }
        self.wait(0.3);
        true
    }

    /// Pick up the cube nearest `near` (walking up to it first).
    pub fn pick_up(&mut self, near: Vec3) -> usize {
        let i = self.sim.props.iter().enumerate().filter(|(_, p)| p.alive).min_by(|a, b| a.1.pos.distance(near).total_cmp(&b.1.pos.distance(near))).map(|(i, _)| i).expect("no cube");
        let at = self.sim.props[i].pos;
        self.go(Vec3::new(at.x, at.y - crate::sim::CUBE_HALF, at.z), 1.1);
        let at = self.sim.props[i].pos;
        self.look_at(at);
        self.sim.use_key(Aim::of(&self.c));
        assert_eq!(self.sim.held, Some(i), "{}: didn't pick up the cube at {at} (from {})", self.name, self.c.feet);
        // Carried at eye height, looking ahead.
        self.c.pitch = 0.0;
        self.wait(0.5);
        i
    }

    /// Carry what she's holding onto the floor button nearest `near`, and let go: it must stay
    /// pressed.
    pub fn cube_on_button(&mut self, near: Vec3) -> usize {
        let b = self.sim.buttons.iter().enumerate().min_by(|a, b| a.1.at.distance(near).total_cmp(&b.1.at.distance(near))).map(|(i, _)| i).expect("no button");
        let at = self.sim.buttons[b].at;
        let held = self.sim.held.expect("not holding anything");
        // Looking level, the cube rides about 2 m out in front: stand that far back from the
        // button, facing it, and line the cube up over it.
        let back = {
            let d = Vec2::new(self.c.feet.x - at.x, self.c.feet.z - at.z).normalize_or(Vec2::Y);
            at + Vec3::new(d.x, 0.0, d.y) * 1.9
        };
        self.go(back, 0.5);
        for _ in 0..5 {
            let eye = self.c.view().eye;
            self.look_at(Vec3::new(at.x, eye.y, at.z));
            self.wait(0.4);
            let cube = self.sim.props[held].pos;
            let off = Vec2::new(cube.x - at.x, cube.z - at.z);
            if off.length() < 0.12 {
                break;
            }
            let spot = self.c.feet - Vec3::new(off.x, 0.0, off.y);
            self.nudge(spot);
        }
        assert_eq!(self.sim.held, Some(held), "{}: lost the cube lining it up over the button", self.name);
        self.sim.use_key(Aim::of(&self.c));
        self.wait(1.5);
        assert!(self.sim.buttons[b].pressed, "{}: the cube didn't stay on the button at {at}", self.name);
        b
    }

    #[allow(dead_code)]
    /// The mover of the map's entity `name` (a door, a lift, a train).
    pub fn mover(&self, name: &str) -> usize {
        let io = self.sim.io.as_ref().unwrap();
        io.ents.iter().find_map(|e| match e.kind {
            crate::io::Kind::Mover { index, .. } if e.name.eq_ignore_ascii_case(name) => Some(index),
            _ => None,
        }).unwrap_or_else(|| panic!("{}: no mover {name}", self.name))
    }

    /// The map's done: it's named the next one.
    pub fn assert_leads_to(&self, next: &str) {
        let t = self.sim.transition.as_ref().unwrap_or_else(|| panic!("{}: never finished (at {})", self.name, self.c.feet));
        assert_eq!(t.map, next, "{}: led to the wrong map", self.name);
    }
}

fn geom_in(b: &Aabb, x: f32, z: f32) -> bool {
    x >= b.min.x && x <= b.max.x && z >= b.min.z && z <= b.max.z
}

impl Play {
    /// Wait until the `color` portal is open at about `at`.
    pub fn wait_portal(&mut self, color: PortalColor, at: Vec3, secs: f32) {
        self.wait_for(secs, &format!("the {color:?} portal at {at}"), |p| p.sim.portals[color.index()].is_some_and(|q| q.center.distance(at) < 1.0));
    }

    /// Walk up to the `color` portal at `at` (on a wall) and through it, out of the other one
    /// at `to` (or wherever it is). A map's own portals come and go: she waits in front for
    /// both to be where they should, going in just as the one that moves arrives (one that
    /// stays put for 6 s is there for good); if one goes before she's in, she backs off and
    /// waits for it again.
    pub fn enter(&mut self, color: PortalColor, at: Vec3, to: Option<Vec3>) {
        let both = |p: &Play| {
            p.sim.portals[color.index()].is_some_and(|x| x.center.distance(at) < 1.0)
                && p.sim.portals[color.other().index()].is_some_and(|x| to.is_none_or(|to| x.center.distance(to) < 1.0))
        };
        self.wait_portal(color, at, 60.0);
        let q = self.sim.portals[color.index()].unwrap();
        let front = q.center - Vec3::Y * 1.0 + q.normal * 2.5;
        for _ in 0..6 {
            self.go(front, 0.4);
            let end = self.time + 6.0;
            while both(self) && self.time < end {
                self.step(&Input::default());
            }
            self.wait_for(60.0, &format!("the {color:?} portal at {at} and the other at {to:?}"), both);
            if self.through_at(color, Some(at), to, 1.0) {
                return;
            }
        }
        panic!("{}: never got through the {color:?} portal at {at}", self.name);
    }

    /// Into the exit elevator (the train whose path ends changing the level), where its ride
    /// trigger is, and up: the map must lead on to `next`.
    pub fn ride_exit(&mut self, next: &str) {
        let io = self.sim.io.as_ref().unwrap();
        let change = io.ents.iter().find(|e| matches!(&e.kind, crate::io::Kind::ChangeLevel { map, .. } if map == next)).map(|e| e.name.clone()).unwrap_or_else(|| panic!("{}: nothing leads to {next}", self.name));
        let train = io.ents.iter().find_map(|e| match e.kind {
            crate::io::Kind::PathNode { mover, .. } if e.outputs.iter().any(|o| o.target == change && o.input.eq_ignore_ascii_case("ChangeLevel")) => Some(mover),
            _ => None,
        }).unwrap_or_else(|| panic!("{}: no exit elevator", self.name));
        let train_name = io.ents.iter().find(|e| matches!(e.kind, crate::io::Kind::Mover { index, .. } if index == train)).map(|e| e.name.clone()).unwrap_or_default();
        let ride = io.ents.iter().find_map(|e| match e.kind {
            crate::io::Kind::Trigger { b, .. } if e.outputs.iter().any(|o| o.target == train_name && o.input.eq_ignore_ascii_case("StartForward")) => Some(b),
            _ => None,
        }).unwrap_or_else(|| panic!("{}: nothing starts the exit elevator", self.name));
        let car = self.sim.movers[train].boxes().filter(|b| b.size().x > 2.5 && b.size().z > 2.5 && b.size().y < 0.3).map(|b| b.max.y).fold(f32::MIN, f32::max);
        let c = ride.center();
        self.go(Vec3::new(c.x, car, c.z), 0.3);
        self.wait_for(60.0, "the exit elevator to finish the map", |p| p.sim.transition.is_some());
        self.assert_leads_to(next);
    }

    /// The open portal nearest her.
    pub fn nearest_portal(&self) -> PortalColor {
        let feet = self.c.feet;
        PortalColor::BOTH.into_iter().filter(|c| self.sim.portals[c.index()].is_some()).min_by(|a, b| {
            let d = |c: &PortalColor| self.sim.portals[c.index()].unwrap().center.distance(feet);
            d(a).total_cmp(&d(b))
        }).expect("no portal open")
    }
}

/// Chamber 00 (testchmb_a_00): out of the relaxation vault through its portal, the cube from
/// the dropper onto the button, out of the opened doors to the lift and up; ...
#[test]
fn solve_testchmb_a_00() {
    let Some(mut p) = Play::load("testchmb_a_00") else { return };
    // GLaDOS's welcome, then the vault's portal opens.
    p.wait_for(90.0, "the vault's portals", |p| p.sim.portals.iter().all(|x| x.is_some_and(|x| x.center.distance(p.c.feet) < 15.0)));
    let c = p.nearest_portal();
    let q = p.sim.portals[c.index()].unwrap();
    p.go(q.center + q.normal * 1.2 - Vec3::Y * (q.center.y - p.c.feet.y), 0.6);
    p.through(c, 1.0);
    p.assert_alive("leaving the vault");
    p.pick_up(Vec3::new(23.0, 4.0, 13.0));
    p.cube_on_button(Vec3::new(23.58, 3.56, 13.0));
    // Through the doors it opened, to the lift: it takes her up.
    p.go(Vec3::new(21.95, 3.68, 37.39), 0.5);
    let floor = p.c.feet.y;
    p.wait_for(30.0, "the lift to take her up", |p| p.c.feet.y > floor + 14.0 && p.c.state == State::Ground);
    p.assert_alive("riding the lift");
    // Chamber 01: an orange portal by the way in, and the blue one cycling round three
    // rooms. Into the room with the dropper for the cube...
    let orange = Vec3::new(16.1, 16.0, 27.6);
    let blue = |n: u32| match n {
        1 => Vec3::new(9.8, 16.0, 35.0),
        2 => Vec3::new(5.7, 16.0, 34.1),
        _ => Vec3::new(9.8, 16.0, 20.3),
    };
    p.go(Vec3::new(13.5, 15.1, 27.6), 0.6);
    p.enter(PortalColor::Orange, orange, Some(blue(3)));
    // ...the cube, and back out with it.
    p.pick_up(Vec3::new(13.5, 15.1, 20.9));
    p.enter(PortalColor::Blue, blue(3), Some(orange));
    // Into the room with the button when blue's there, and the cube onto it.
    p.enter(PortalColor::Orange, orange, Some(blue(1)));
    p.cube_on_button(Vec3::new(14.5, 14.9, 35.0));
    // Back out, and into the last room when blue's there: its exit door's open.
    p.enter(PortalColor::Blue, blue(1), Some(orange));
    p.enter(PortalColor::Orange, orange, Some(blue(2)));
    p.assert_alive("in chamber 01");
    p.ride_exit("testchmb_a_01");
    p.assert_alive("in the exit elevator");
}

impl Play {
    /// Debugging: which cells she can reach from where she is (the path-finder's search with
    /// no goal), drawn over x0..x1, z0..z1: '#' reached, '.' a floor not reached.
    #[allow(dead_code)]
    pub fn draw_reach(&self, x0: f32, x1: f32, z0: f32, z1: f32) {
        // Flood from her cell.
        let cell = |p: f32| (p / CELL).floor() as i32;
        let centre = |i: i32| (i as f32 + 0.5) * CELL;
        let mut seen: HashMap<(i32, i32), f32> = HashMap::new();
        let mut q = std::collections::VecDeque::new();
        let at = self.c.feet;
        let y0 = self.floors(centre(cell(at.x)), centre(cell(at.z)), at.y - 1.0, at.y + 0.3);
        let Some(&y0) = y0.first() else { eprintln!("no floor under her"); return };
        q.push_back((cell(at.x), cell(at.z), y0));
        seen.insert((cell(at.x), cell(at.z)), y0);
        while let Some((i, k, y)) = q.pop_front() {
            if centre(i) < x0 - 1.0 || centre(i) > x1 + 1.0 || centre(k) < z0 - 1.0 || centre(k) > z1 + 1.0 {
                continue;
            }
            for (di, dk) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (a, b) = (i + di, k + dk);
                if seen.contains_key(&(a, b)) {
                    continue;
                }
                let fl = self.floors(centre(a), centre(b), y - DROP, y + STEP);
                if let Some(ny) = fl.into_iter().reduce(f32::max) {
                    seen.insert((a, b), ny);
                    q.push_back((a, b, ny));
                }
            }
        }
        let mut k = cell(z0);
        while centre(k) <= z1 {
            let row: String = (cell(x0)..=cell(x1)).map(|i| match seen.get(&(i, k)) {
                Some(_) => '#',
                None => if self.floors(centre(i), centre(k), at.y - 3.0, at.y + 3.0).is_empty() { ' ' } else { '.' },
            }).collect();
            eprintln!("z{:6.2} |{row}|", centre(k));
            k += 1;
        }
    }
}
