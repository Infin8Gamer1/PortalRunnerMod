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
/// The widest gap she's walked across with a jump (a running jump goes much further).
const JUMP: f32 = 2.5;

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
        // (Not what she'd step up onto: a ramp's slices, the next stair.)
        self.near(&b).all(|x| x.max.y <= feet.y + 0.3) && !self.sim.goo.iter().any(|g| g.overlaps(&b))
    }

    /// The floors she can stand on in the cell at (x, z), between `lo` and `hi`.
    fn floors(&self, x: f32, z: f32, lo: f32, hi: f32) -> Vec<f32> {
        // (Anything under her footprint: she stands on a step's edge.)
        let hw = self.c.body().half_width - 0.05;
        let col = Aabb::new(Vec3::new(x - hw, lo - 0.01, z - hw), Vec3::new(x + hw, hi + 0.01, z + hw));
        let mut tops: Vec<f32> = self.near(&col).map(|b| b.max.y).filter(|y| *y >= lo && *y <= hi).collect();
        // Not goo, nor an open floor portal (she'd fall in).
        tops.retain(|y| !self.sim.goo.iter().any(|g| g.max.y >= *y - 0.05 && geom_in(g, x, z)));
        tops.retain(|y| {
            !self.sim.portals.iter().flatten().any(|q| q.normal.y > 0.5 && (q.center.y - y).abs() < 0.3 && {
                let l = q.to_local(Vec3::new(x, q.center.y, z));
                (l.x / (q.half.x + 0.3)).powi(2) + (l.y / (q.half.y + 0.3)).powi(2) <= 1.0
            })
        });
        tops.sort_by(|a, b| a.total_cmp(b));
        tops.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        tops.into_iter().filter(|y| self.fits(Vec3::new(x, *y, z))).collect()
    }

    /// A path over the floors from her feet to within `reach` of `to` (horizontally, and at
    /// about its height), as the cells' feet points, each marked if she jumps to it (over a
    /// gap, as far as `JUMP`, to a floor not much higher).
    pub fn path(&self, to: Vec3, reach: f32) -> Option<Vec<(Vec3, bool)>> {
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
        let mut jumped: HashSet<Node> = HashSet::new();
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
                return Some(path.into_iter().map(|n| { let c = centre(n.0, n.1); (Vec3::new(c.x, n.2 as f32 / 100.0, c.y), jumped.contains(&n)) }).collect());
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
                    jumped.remove(&m);
                    open.push(Open(ng + h(&m), m));
                }
            }
            // Jumps: off an edge (no floor at her height next to her) across a gap to a floor
            // not much higher, with room over the gap.
            for (di, dk) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let near = |i: i32, k: i32, lo: f32, hi: f32| {
                    let c = centre(i, k);
                    !self.floors(c.x, c.y, lo, hi).is_empty()
                };
                if near(n.0 + di, n.1 + dk, y - 0.6, y + STEP) {
                    continue;
                }
                for len in 2..=(JUMP / CELL) as i32 {
                    let (i, k) = (n.0 + di * len, n.1 + dk * len);
                    let c = centre(i, k);
                    let over = (1..len).all(|j| {
                        let c = centre(n.0 + di * j, n.1 + dk * j);
                        self.fits(Vec3::new(c.x, y + 0.5, c.y))
                    });
                    if !over {
                        break;
                    }
                    let land = self.floors(c.x, c.y, y - 1.5, y + 0.35).into_iter().reduce(f32::max);
                    let Some(ny) = land else { continue };
                    let m: Node = (i, k, (ny * 100.0).round() as i32);
                    let ng = g + len as f32 * CELL * 1.5 + 1.0;
                    if cost.get(&m).is_none_or(|&c| ng < c) {
                        cost.insert(m, ng);
                        from.insert(m, n);
                        jumped.insert(m);
                        open.push(Open(ng + h(&m), m));
                    }
                    break;
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
    fn follow(&mut self, path: &[(Vec3, bool)], to: Vec3, reach: f32) -> bool {
        let jumps: Vec<bool> = path.iter().map(|p| p.1).collect();
        let path: Vec<Vec3> = path.iter().map(|p| p.0).collect();
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
            // A jump coming up: head for where she lands, and jump at the edge.
            let jump = (on + 1..(on + 4).min(path.len())).find(|&j| jumps[j]);
            let ahead = match jump {
                Some(j) => path[j],
                None => path[(on + 3).min(path.len() - 1)],
            };
            let d = Vec2::new(ahead.x - feet.x, ahead.z - feet.z);
            let left = path.len() - 1 - on;
            if d.length() > 1e-3 {
                self.c.yaw = (-d.x).atan2(-d.y);
            }
            let take_off = jump.is_some_and(|j| flat(path[j - 1]) < 0.3) && self.c.state == State::Ground;
            // Walk the last few cells (she runs fast, and would overshoot).
            let speed = if left < 6 && jump.is_none() { 0.35 } else { 1.0 };
            self.step(&Input { move_axis: Vec2::new(0.0, speed), jump_pressed: take_off, jump_held: take_off || self.c.state != State::Ground, ..Default::default() });
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

    /// Shoot a portal of `color` at the point `at` (from where she stands): it must open
    /// within a portal's size of it.
    pub fn shoot(&mut self, color: PortalColor, at: Vec3) {
        self.look_at(at);
        let eye = self.c.view().eye;
        let aim = Aim { eye, dir: (at - eye).normalize(), yaw: self.c.yaw };
        let (legs, hit) = self.sim.trace(aim.eye, aim.dir, 200.0, crate::sim::Mask::SHOT, 4);
        let opened = self.sim.fire(color, aim);
        if !opened {
            eprintln!("shot: legs {legs:?} hit {:?}", hit.map(|h| (h.point, h.normal, h.what)));
        }
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
        let mut crossed = false;
        loop {
            entered |= self.sim.player_env == Some(color);
            let gone = |c: PortalColor, want: Option<Vec3>| want.is_some_and(|w| !self.sim.portals[c.index()].is_some_and(|x| x.center.distance(w) < 1.0));
            if !entered && (gone(color, at) || gone(color.other(), to)) {
                return false;
            }
            let feet = self.c.feet;
            let flat_q = Vec2::new(feet.x - q.center.x, feet.z - q.center.z).length();
            if crossed && self.sim.player_env.is_none() {
                // Out of a floor portal: once she's down beside it. Out of a wall (or
                // ceiling) one: once she's clear of its mouth.
                let done = if q.normal.y > 0.5 {
                    self.c.state == State::Ground && flat_q > 1.3
                } else {
                    (feet - q.center).dot(q.normal) > out && flat_q < out + 3.0
                };
                if done {
                    break;
                }
            }
            assert!(self.time < end, "{}: didn't get through the {color:?} portal (at {feet})", self.name);
            // Into this one till she's through it; then on out of the far one (off to the side
            // of a floor portal, the way she was going).
            let d = if !crossed { p.center - feet } else if q.normal.y > 0.5 { Vec3::ZERO } else { q.normal };
            if Vec2::new(d.x, d.z).length() > 1e-3 {
                self.c.yaw = (-d.x).atan2(-d.z);
            }
            self.step(&Input { move_axis: Vec2::new(0.0, 0.6), ..Default::default() });
            crossed |= self.sim.events.iter().any(|e| matches!(e, SimEvent::Teleported { entered: c, .. } if *c == color));
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
        // Where she waits: 2.5 m out from it, or nearer where there's no floor that far (on a
        // ledge in front of it), at the foot of it.
        let foot = q.center.y - q.half.y;
        let front = [2.5f32, 2.0, 1.6, 1.2]
            .into_iter()
            .map(|d| q.center + q.normal * d)
            .find_map(|p| self.floors(p.x, p.z, foot - 0.6, foot + 0.5).into_iter().reduce(f32::max).map(|y| Vec3::new(p.x, y, p.z)))
            .unwrap_or(q.center - Vec3::Y * 1.0 + q.normal * 2.5);
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

    /// Into lift `train` (a train), where the trigger that starts it is, and ride it to the
    /// end of its path.
    pub fn ride(&mut self, train_name: &str) {
        let train = self.mover(train_name);
        self.board(train, train_name);
        self.wait_for(60.0, &format!("{train_name} to the top"), |p| p.sim.movers[train].node + 1 == p.sim.movers[train].def.path.len() && p.sim.movers[train].goal.is_none());
        self.wait(2.0);
    }

    /// Into train `train`'s car, where the trigger that starts it is. A trigger that starts it
    /// from outside the car (calling a car waiting up its shaft down to her) she walks through
    /// first, and waits for the car.
    fn board(&mut self, train: usize, train_name: &str) {
        let io = self.sim.io.as_ref().unwrap();
        let starters: Vec<Aabb> = io.ents.iter().filter_map(|e| match e.kind {
            crate::io::Kind::Trigger { b, .. } if e.outputs.iter().any(|o| o.target.eq_ignore_ascii_case(train_name) && o.input.eq_ignore_ascii_case("StartForward")) => Some(b),
            _ => None,
        }).collect();
        assert!(!starters.is_empty(), "{}: nothing starts {train_name}", self.name);
        let car = |p: &Play| p.sim.movers[train].boxes().filter(|b| b.size().x > 2.5 && b.size().z > 2.5 && b.size().y < 0.3).reduce(|a, b| if a.max.y > b.max.y { a } else { b });
        // The car, and the trigger in it (one it's level with: its floor at the trigger's foot).
        let in_car = |p: &Play| {
            let c = car(p)?;
            let room = Aabb::new(c.min - Vec3::new(0.3, 0.5, 0.3), c.max + Vec3::new(0.3, 3.0, 0.3));
            Some((c, starters.iter().copied().find(|b| b.overlaps(&room) && (c.max.y - b.min.y).abs() < 1.0)))
        };
        if let Some((c, None)) = in_car(self) {
            // (Not the one in the car's shaft.)
            let column = Aabb::new(Vec3::new(c.min.x, -1e4, c.min.z), Vec3::new(c.max.x, 1e4, c.max.z));
            for b in starters.iter().copied().filter(|b| !b.overlaps(&column)) {
                let c = b.center();
                let floor = self.floors(c.x, c.z, b.min.y - 0.5, b.max.y).into_iter().reduce(f32::max).unwrap_or(b.min.y);
                self.go(Vec3::new(c.x, floor, c.z), 0.4);
            }
        }
        // (Not while it's on its way.)
        self.wait_for(40.0, "the car to come", |p| p.sim.movers[train].goal.is_none() && in_car(p).is_some_and(|(_, r)| r.is_some()));
        self.wait(1.5);
        let (car, ride) = in_car(self).expect("no car");
        let at = ride.map_or(car.center(), |b| b.center());
        self.go(Vec3::new(at.x, car.max.y, at.z), 0.3);
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
        self.board(train, &train_name);
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

/// Chamber 02 (the first half of testchmb_a_01): the turning gun shoots blue portals round
/// the rooms, orange is fixed in the exit room. Through blue from the left room to orange,
/// back through orange to the gun's room while blue's there, the gun; then her own blue back
/// to orange, through the exit doors (opened as she took it) to the lift up.
#[test]
fn solve_testchmb_a_01() {
    let Some(mut p) = Play::load("testchmb_a_01") else { return };
    let orange = Vec3::new(-7.32, 1.32, 9.75);
    let blue_left = Vec3::new(-14.6, -1.9, -2.5);
    let blue_south = Vec3::new(-4.1, -1.9, 3.3);
    // Out onto the balcony: GLaDOS's welcome, then the way in opens.
    p.go(Vec3::new(-2.0, 0.16, -12.1), 0.5);
    let door = p.mover("entrance_door_left_a_a01");
    p.wait_for(60.0, "the entrance doors", |p| p.sim.movers[door].node == 1);
    p.enter(PortalColor::Blue, blue_left, Some(orange));
    eprintln!("in the exit room at {}", p.c.feet);
    p.enter(PortalColor::Orange, orange, Some(blue_south));
    eprintln!("in the gun's room at {}", p.c.feet);
    let gun = p.sim.gun_pose(0).0;
    p.go(Vec3::new(gun.x, p.c.feet.y, gun.z), 0.6);
    p.wait_for(3.0, "her to pick up the gun", |p| p.sim.gun.blue);
    eprintln!("got the gun at {}", p.c.feet);
    // Her own blue on the gun room's far wall (the gun's last one is on the near wall: a
    // shot at it would go through it), and through to orange.
    p.wait(1.0);
    let blue_north = Vec3::new(-4.1, -1.9, -8.1);
    p.go(blue_north - Vec3::new(0.0, 1.35, -4.5), 0.5);
    p.shoot(PortalColor::Blue, blue_north);
    p.enter(PortalColor::Blue, blue_north, Some(orange));
    eprintln!("back in the exit room at {}", p.c.feet);
    // Through the exit doors to the lift up to Chamber 03.
    p.go(Vec3::new(1.2, 0.0, 6.5), 0.5);
    let floor = p.c.feet.y;
    p.go(Vec3::new(10.0, 0.0, 6.5), 0.4);
    p.wait_for(30.0, "the lift up", |p| p.c.feet.y > floor + 10.0 && p.c.state == State::Ground);
    p.assert_alive("in Chamber 02");
    let lift = p.mover("a01-middleelevator_body");
    p.wait_for(30.0, "the lift to reach the top", |p| p.sim.movers[lift].node + 1 == p.sim.movers[lift].def.path.len() && p.sim.movers[lift].goal.is_none());
    p.wait(2.0);
    eprintln!("up to Chamber 03 at {}", p.c.feet);
    // Chamber 03: orange fixed across a pit.
    p.go(Vec3::new(0.4, p.c.feet.y, 11.4), 0.5);
    p.wait_portal(PortalColor::Orange, Vec3::new(-27.64, 16.05, 15.44), 10.0);
    // Blue on the room's west wall, through to orange...
    let orange2 = Vec3::new(-27.64, 16.05, 15.44);
    let blue_west = Vec3::new(-1.6, 16.15, 6.5);
    p.go(Vec3::new(1.5, 14.63, 6.5), 0.5);
    p.shoot(PortalColor::Blue, blue_west);
    p.enter(PortalColor::Blue, blue_west, Some(orange2));
    eprintln!("across the first pit at {}", p.c.feet);
    // ...then blue on the far wall past the second pit, and back through orange to it.
    let blue_far = Vec3::new(-25.0, 16.1, 40.0);
    p.go(Vec3::new(-25.0, 14.63, 21.5), 0.5);
    p.shoot(PortalColor::Blue, blue_far);
    p.enter(PortalColor::Orange, orange2, Some(blue_far));
    eprintln!("across the second pit at {}", p.c.feet);
    p.ride_exit("testchmb_a_02");
    p.assert_alive("in Chamber 03");
}

/// Chamber 04 (testchmb_a_02's first half): the cube's dropped down a pit. Blue on the pit's
/// far wall, in through the fixed orange by the button to the cube, back out through blue
/// with it, onto the button: the exit doors open, to the lift up to Chamber 05.
#[test]
fn solve_testchmb_a_02() {
    let Some(mut p) = Play::load("testchmb_a_02") else { return };
    let orange = Vec3::new(-0.02, 1.40, 11.38);
    // Past the trigger that opens orange, into the room with the pit.
    p.go(Vec3::new(-4.67, 0.0, 21.0), 0.5);
    p.wait_portal(PortalColor::Orange, orange, 10.0);
    // (As low as the white wall goes: the floor by the wall is a grate, 0.4 m down.)
    let pit_wall = Vec3::new(3.25, -5.08, 0.0);
    p.go(Vec3::new(-5.0, 0.0, 0.0), 0.4);
    p.shoot(PortalColor::Blue, pit_wall);
    p.enter(PortalColor::Orange, orange, Some(pit_wall));
    eprintln!("in the pit at {}", p.c.feet);
    p.pick_up(Vec3::new(-1.7, -6.0, 0.0));
    p.enter(PortalColor::Blue, pit_wall, Some(orange));
    eprintln!("out with the cube at {}", p.c.feet);
    p.cube_on_button(Vec3::new(-4.8, 0.0, 11.6));
    // Out of the opened doors, to the lift.
    let lift = p.mover("a02-middle_elevator_body");
    p.go(Vec3::new(-4.88, 0.0, -21.0), 0.5);
    p.go(Vec3::new(-4.88, 0.5, -22.76), 0.5);
    p.wait_for(40.0, "the lift to reach the top", |p| p.sim.movers[lift].node + 1 == p.sim.movers[lift].def.path.len() && p.sim.movers[lift].goal.is_none());
    p.wait(2.0);
    p.assert_alive("in Chamber 04");
    eprintln!("up to Chamber 05 at {}", p.c.feet);
    // Chamber 05: orange high on the west alcove's wall once she's in the room, a cube in the
    // east alcove (as high) and one in a pit, a button for each.
    let orange = Vec3::new(-29.17, 19.29, -17.89);
    p.go(Vec3::new(-12.2, 14.63, -12.2), 0.5);
    p.wait_portal(PortalColor::Orange, orange, 10.0);
    let floor_blue = Vec3::new(-29.26, 16.1, -13.0);
    let up_to_orange = |p: &mut Play| {
        p.go(Vec3::new(-24.0, 14.63, -10.0), 0.5);
        p.shoot(PortalColor::Blue, floor_blue);
        let at = p.sim.portals[0].unwrap().center;
        p.enter(PortalColor::Blue, at, Some(orange));
    };
    up_to_orange(&mut p);
    eprintln!("on the west alcove at {}", p.c.feet);
    // Blue on the east alcove's back wall, across to its cube and back.
    let east = Vec3::new(-12.9, 19.3, -17.4);
    p.shoot(PortalColor::Blue, east);
    let east = p.sim.portals[0].unwrap().center;
    p.enter(PortalColor::Orange, orange, Some(east));
    p.pick_up(Vec3::new(-14.4, 18.4, -17.4));
    p.enter(PortalColor::Blue, east, Some(orange));
    eprintln!("back with the first cube at {}", p.c.feet);
    p.cube_on_button(Vec3::new(-24.4, 14.63, -13.1));
    // Up again, and from the alcove blue down in the pit by its cube; there and back.
    up_to_orange(&mut p);
    // (The east end of its far wall: a wall hides the west end from the alcove.)
    let pit = Vec3::new(-19.5, 12.82, -27.64);
    p.shoot(PortalColor::Blue, pit);
    let pit = p.sim.portals[0].unwrap().center;
    p.enter(PortalColor::Orange, orange, Some(pit));
    p.pick_up(Vec3::new(-19.8, 11.8, -24.2));
    p.enter(PortalColor::Blue, pit, Some(orange));
    eprintln!("back with the second cube at {}", p.c.feet);
    p.cube_on_button(Vec3::new(-17.9, 14.63, -13.0));
    // Through the exit doors; the ceiling's orange opens over the catwalk. Blue by the floor,
    // and out of the ceiling onto it.
    p.go(Vec3::new(-19.9, 14.63, -3.2), 0.5);
    let ceiling = Vec3::new(-21.12, 24.28, -2.82);
    p.wait_portal(PortalColor::Orange, ceiling, 10.0);
    eprintln!("past the doors at {}", p.c.feet);
    let wall = Vec3::new(-24.38, 16.1, -3.0);
    p.shoot(PortalColor::Blue, wall);
    let wall = p.sim.portals[0].unwrap().center;
    p.enter(PortalColor::Blue, wall, Some(ceiling));
    eprintln!("on the catwalk at {}", p.c.feet);
    p.assert_alive("in Chamber 05");
    p.ride_exit("testchmb_a_03");
}

/// Chamber 06 (testchmb_a_03's first half): the ceiling's launcher fires its pellet down into
/// the fixed orange in the floor; her blue on the ceiling over the catcher (in the floor)
/// drops it in, and the pillar barring the way out sinks. Up the lift to Chamber 07.
#[test]
fn solve_testchmb_a_03() {
    let Some(mut p) = Play::load("testchmb_a_03") else { return };
    let pillar = p.mover("lift_a");
    p.go(Vec3::new(3.24, -3.66, 3.0), 0.5);
    p.shoot(PortalColor::Blue, Vec3::new(3.24, 5.5, 0.01));
    p.wait_for(20.0, "the pellet to be caught", |p| p.sim.movers[pillar].node == 1 && p.sim.movers[pillar].goal.is_none());
    // The lift's come down: onto it, and up to the way out.
    let platform = p.mover("lift_brush_a03");
    let top = p.sim.movers[platform].boxes().reduce(|a, b| Aabb::new(a.min.min(b.min), a.max.max(b.max))).unwrap();
    p.go(Vec3::new(top.center().x, top.max.y, top.center().z), 0.3);
    p.wait_for(20.0, "the lift to go up", |p| p.sim.movers[pillar].node == 0 && p.sim.movers[pillar].goal.is_none());
    p.ride("a03-middle_elevator_body");
    p.assert_alive("in Chamber 06");
    eprintln!("up to Chamber 07 at {}", p.c.feet);
    // Chamber 07: the wall's launcher fires across the room; blue on the far wall in its way
    // sends the pellet up out of the orange in the pit's floor into the ceiling's catcher, and
    // the platform starts along its rail over the pit.
    p.go(Vec3::new(3.0, 30.89, -3.0), 0.5);
    p.assert_alive("going down into the pit");
    p.shoot(PortalColor::Blue, Vec3::new(-8.128, 35.63, -1.63));
    let train = p.mover("func_tracktrain_b00");
    p.wait_for(20.0, "the platform to set off", |p| p.sim.movers[train].goal.is_some());
    p.assert_alive("waiting for the pellet");
    // Onto the platform as Portal has it: blue on the ceiling over where it stops at the west
    // end, then down through the orange in the pit's floor while it's stopped there.
    let over = Vec3::new(-5.33, 40.64, 9.75);
    p.go(Vec3::new(-2.0, 30.89, 2.0), 0.5);
    p.shoot(PortalColor::Blue, over);
    let over = p.sim.portals[0].unwrap().center;
    p.wait_for(40.0, "the platform to stop at the west end", |p| p.sim.movers[train].node == 0 && p.sim.movers[train].goal.is_none());
    p.through(PortalColor::Orange, 1.0);
    p.wait(2.0);
    eprintln!("dropped from {over} to {} (platform at {})", p.c.feet, p.sim.movers[train].at);
    p.assert_alive("dropping onto the platform");
    p.ride_exit("testchmb_a_04");
    p.assert_alive("in Chamber 07");
}

/// Chamber 08 (testchmb_a_04): the launcher's pellet bounces up and down the room's west
/// side. Blue where it hits the far wall sends it out of the fixed orange across the room;
/// while it's away, blue moved to the west wall level with the catcher, and back off the
/// east wall it goes into orange and out of blue into the catcher. The cart it starts carries
/// her over the pit to the exit.
#[test]
fn solve_testchmb_a_04() {
    let Some(mut p) = Play::load("testchmb_a_04") else { return };
    let orange = Vec3::new(-9.65, 1.43, 1.64);
    p.go(Vec3::new(-3.25, 0.0, 9.96), 0.5);
    p.wait_portal(PortalColor::Orange, orange, 10.0);
    p.go(Vec3::new(-4.0, 0.0, 10.5), 0.5);
    p.shoot(PortalColor::Blue, Vec3::new(-8.08, 1.56, -13.3));
    eprintln!("blue at {:?}", p.sim.portals[0].map(|q| (q.center, q.normal)));
    // (Across the room from the orange's ledge, which hides the west wall from the way in.)
    p.go(Vec3::new(2.5, 0.0, 8.6), 0.4);
    p.wait_for(20.0, "the pellet out of orange", |p| p.sim.pellets.iter().any(|x| x.alive && x.vel.x > 1.0));
    p.shoot(PortalColor::Blue, Vec3::new(-9.75, 1.22, -7.67));
    eprintln!("blue at {:?}", p.sim.portals[0].map(|q| (q.center, q.normal)));
    let cart = p.mover("rail_cart_lvl5");
    p.wait_for(20.0, "the cart to set off", |p| p.sim.movers[cart].goal.is_some());
    eprintln!("caught at {}", p.time);
    // To orange's ledge: blue on the wall at the end of the way in, and through.
    let wall = Vec3::new(3.2, 1.42, 8.6);
    p.go(Vec3::new(-1.0, 0.0, 8.6), 0.4);
    p.shoot(PortalColor::Blue, wall);
    let wall = p.sim.portals[0].unwrap().center;
    p.enter(PortalColor::Blue, wall, Some(orange));
    eprintln!("on orange's ledge at {}", p.c.feet);
    // Blue on the ceiling over where the cart waits at its start (the ceiling past that takes
    // no portals); into orange while it's there, down onto it, and ride it to the exit.
    p.shoot(PortalColor::Blue, Vec3::new(1.52, 6.5, -1.83));
    p.wait_for(60.0, "the cart back at its start", |p| p.sim.movers[cart].node == 0 && p.sim.movers[cart].goal.is_none());
    p.through(PortalColor::Orange, 1.0);
    p.wait(1.5);
    eprintln!("on the cart at {} (cart at {})", p.c.feet, p.sim.movers[cart].at);
    p.assert_alive("dropping onto the cart");
    p.wait_for(30.0, "the cart at the exit", |p| p.sim.movers[cart].node == 1 && p.sim.movers[cart].goal.is_none());
    p.ride_exit("testchmb_a_05");
    p.assert_alive("in Chamber 08");
}

/// Chamber 09 (testchmb_a_05): the cube can't go through the grill. With the cube, blue low on
/// the east wall and up through it to the fixed orange's alcove; from there blue down on the
/// far room's floor (shots go through grills), back into orange with the cube and out past the
/// grill, the cube on the button, and out of the doors it opens.
#[test]
fn solve_testchmb_a_05() {
    let Some(mut p) = Play::load("testchmb_a_05") else { return };
    let orange = Vec3::new(4.86, 4.55, 3.25);
    p.wait(3.0);
    p.pick_up(Vec3::new(-0.81, 0.45, 4.88));
    p.wait_portal(PortalColor::Orange, orange, 20.0);
    p.go(Vec3::new(2.0, 0.0, -3.0), 0.5);
    p.shoot(PortalColor::Blue, Vec3::new(7.3, 1.42, -3.0));
    let low = p.sim.portals[0].unwrap().center;
    p.enter(PortalColor::Blue, low, Some(orange));
    eprintln!("in orange's alcove at {} holding {:?}", p.c.feet, p.sim.held);
    p.shoot(PortalColor::Blue, Vec3::new(-10.0, 0.2, -3.0));
    let far = p.sim.portals[0].unwrap().center;
    eprintln!("blue at {far}");
    p.enter(PortalColor::Orange, orange, Some(far));
    eprintln!("past the grill at {} holding {:?}", p.c.feet, p.sim.held);
    p.cube_on_button(Vec3::new(-7.39, 0.2, -1.52));
    p.ride_exit("testchmb_a_06");
    p.assert_alive("in Chamber 09");
}
