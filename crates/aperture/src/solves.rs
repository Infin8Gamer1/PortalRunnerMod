//! Portal's chambers played through by Faith (#21): each map from its start to its exit, with
//! her own movement. She's walked along paths found over the map's floors (A* over 0.25 m
//! cells: where a box's top has room for her above it, stepping up as far as Source's stairs,
//! dropping down as far as she lands on her feet, jumping gaps), shoots her portals at the walls
//! Portal's solution uses, carries cubes to buttons and rides the lifts. A chamber that can't
//! be finished this way fails here, saying where she got stuck.
//!
//! The chambers are the game's own copies of Portal's maps (`levels/portal/`), so these run
//! without anything extracted (GLaDOS's scenes, when they are, set her waits).

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
const DROP: f32 = 9.0;
/// The map's boxes are bucketed in columns this wide.
const BUCKET: f32 = 2.0;
/// The widest gap she's walked across with a jump (a running jump goes much further).
const JUMP: f32 = 2.5;
/// The tallest ledge she vaults or climbs onto at a run.
const CLIMB: f32 = 1.75;

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
    /// How hard she pushes forward going through a portal (1: running).
    pub entry_speed: f32,
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
    /// Portal's map `name`: the game's own copy of it (`levels/portal/`), with GLaDOS's
    /// scenes (so the waits are hers) where they've been extracted.
    pub fn load(name: &str) -> Option<Play> {
        let level = crate::port::native_level(name)?;
        let cp = level.checkpoints[0].clone();
        let sim = Sim::new(level);
        let mut c = Controller::new(crate::tuning(5.3, 15.0), cp.spawn, cp.yaw);
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
        Some(Play { name: name.to_string(), sim, c, events: Vec::new(), time: 0.0, boxes, buckets, deaths: 0, entry_speed: 0.6 })
    }

    /// One frame with this input.
    pub fn step(&mut self, input: &Input) {
        // (As the game does: what she did between frames, using a switch, is in this frame's
        // events for the logic.)
        let before = self.c.feet;
        self.sim.step_player(&mut self.c, input, DT);
        self.sim.step(DT, &mut self.c);
        for e in &self.sim.events {
            if *e == SimEvent::PlayerDied {
                self.deaths += 1;
                eprintln!("(died at {before}: pellets {:?}, the held cube {:?})", self.sim.pellets.iter().filter(|q| q.alive).map(|q| q.pos).collect::<Vec<_>>(), self.sim.held.map(|h| self.sim.props[h].pos));
            }
        }
        self.events.extend(self.sim.events.iter().copied());
        self.sim.events.clear();
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
        // Cubes in the way (not the one she's carrying).
        out.extend(self.sim.props.iter().enumerate().filter(|(i, c)| c.alive && Some(*i) != self.sim.held).map(|(_, c)| c.aabb()).filter(|c| c.overlaps(b)));
        // And turrets, standing or knocked over.
        out.extend(self.sim.turrets.iter().map(|t| t.aabb()).filter(|t| t.overlaps(b)));
        out.into_iter()
    }

    /// Whether the cell at `c` is open from her height `y` all the way down to the floor at
    /// `ny` she'd drop to (no wall over its foot, no floor between).
    fn open_down(&self, c: Vec2, ny: f32, y: f32) -> bool {
        let hw = self.c.body().half_width - 0.02;
        let col = Aabb::new(Vec3::new(c.x - hw, ny + 0.3, c.y - hw), Vec3::new(c.x + hw, y + self.c.body().height - 0.05, c.y + hw));
        self.near(&col).next().is_none()
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
        self.floors_over(x, z, lo, hi, true)
    }

    /// As [`floors`](Self::floors); `holes`: not where an open floor portal is.
    fn floors_over(&self, x: f32, z: f32, lo: f32, hi: f32, holes: bool) -> Vec<f32> {
        // (Anything under her footprint: she stands on a step's edge.)
        let hw = self.c.body().half_width - 0.05;
        let col = Aabb::new(Vec3::new(x - hw, lo - 0.01, z - hw), Vec3::new(x + hw, hi + 0.01, z + hw));
        let mut tops: Vec<f32> = self.near(&col).map(|b| b.max.y).filter(|y| *y >= lo && *y <= hi).collect();
        // Not goo (not a pool on a floor above, though), nor an open floor portal (she'd fall in).
        tops.retain(|y| !self.sim.goo.iter().any(|g| g.max.y >= *y - 0.05 && g.min.y <= *y + 0.5 && geom_in(g, x, z)));
        tops.retain(|y| {
            !holes
                || !self.sim.portals.iter().flatten().any(|q| q.normal.y > 0.5 && (q.center.y - y).abs() < 0.3 && {
                    let l = q.to_local(Vec3::new(x, q.center.y, z));
                    // (Her feet clear of it: her half-width round it.)
                    (l.x / (q.half.x + 0.35)).powi(2) + (l.y / (q.half.y + 0.35)).powi(2) <= 1.0
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
        // (Where she's standing, whatever's there: beside a floor portal she's come out of. Up
        // against a wall, her cell may not have room for her: the next one that does.)
        let (ci, ck) = cell(at);
        let start = [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)].into_iter().find_map(|(di, dk)| {
            let (i, k) = (ci + di, ck + dk);
            let c = centre(i, k);
            let y = self.floors_over(c.x, c.y, at.y - 1.0, at.y + 0.3, false).into_iter().min_by(|a, b| (a - at.y).abs().total_cmp(&(b - at.y).abs()))?;
            Some((i, k, (y * 100.0).round() as i32))
        })?;
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
                // Down off an edge: open at her own height too (not through a wall whose foot
                // has a floor beyond it).
                if ny < y - 0.3 && !self.open_down(centre(i, k), ny, y) {
                    continue;
                }
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
                // Up onto a ledge too tall to step up (she vaults or climbs it at a run, jumping
                // at its foot): the floor just over the edge.
                let climb = (1..=2).find_map(|len| {
                    let (i, k) = (n.0 + di * len, n.1 + dk * len);
                    let c = centre(i, k);
                    self.floors(c.x, c.y, y + STEP, y + CLIMB).into_iter().reduce(f32::max).map(|ny| (i, k, ny))
                });
                if let Some((i, k, ny)) = climb {
                    let m: Node = (i, k, (ny * 100.0).round() as i32);
                    let ng = g + 2.0;
                    if cost.get(&m).is_none_or(|&c| ng < c) {
                        cost.insert(m, ng);
                        from.insert(m, n);
                        jumped.insert(m);
                        open.push(Open(ng + h(&m), m));
                    }
                    continue;
                }
                // The nearest landing across the gap at about her height; failing that, one
                // well below (jumping down, she's longer in the air: a metre further).
                let jump_to = |below: f32, above: f32, max: f32| {
                    for len in 2..=(max / CELL) as i32 {
                        let (i, k) = (n.0 + di * len, n.1 + dk * len);
                        let c = centre(i, k);
                        let over = (1..len).all(|j| {
                            let c = centre(n.0 + di * j, n.1 + dk * j);
                            self.fits(Vec3::new(c.x, y + 0.5, c.y))
                        });
                        if !over {
                            return None;
                        }
                        if let Some(ny) = self.floors(c.x, c.y, y - below, y + above).into_iter().reduce(f32::max) {
                            // (Down onto it: nothing in the way, a floor between.)
                            return (ny >= y - 0.3 || self.open_down(c, ny, y)).then_some((i, k, ny, len));
                        }
                    }
                    None
                };
                let Some((i, k, ny, len)) = jump_to(1.5, 0.5, JUMP).or_else(|| jump_to(DROP, -2.0, JUMP + 1.0)) else { continue };
                let m: Node = (i, k, (ny * 100.0).round() as i32);
                let ng = g + len as f32 * CELL * 1.5 + 1.0;
                if cost.get(&m).is_none_or(|&c| ng < c) {
                    cost.insert(m, ng);
                    from.insert(m, n);
                    jumped.insert(m);
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
            assert!(attempt < 3, "{}: couldn't walk to {to} (stuck at {}, {:?}, deaths {}, hurt {})", self.name, self.c.feet, self.c.state, self.deaths, self.sim.hurt);
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
            if crossed {
                // Out of a floor portal: once she's down beside it, clear of it. Out of a wall
                // (or ceiling) one: once she's clear of its mouth.
                let done = if q.normal.y > 0.5 {
                    // (Or up on a step beside it: not in it.)
                    let l = q.to_local(feet);
                    self.c.state == State::Ground && (l.x.abs() > q.half.x + 0.4 || l.y.abs() > q.half.y + 0.4 || (feet.y > q.center.y + 0.1 && flat_q > q.half.x))
                } else {
                    self.sim.player_env.is_none() && (feet - q.center).dot(q.normal) > out
                };
                if done {
                    break;
                }
            }
            assert!(self.time < end, "{}: didn't get through the {color:?} portal (at {feet}; crossed {crossed}, env {:?}, {:?}, {flat_q} from the far one)", self.name, self.sim.player_env, self.c.state);
            // Into this one till she's through it; then on out of the far one (off to the side
            // of a floor portal, the way she was going).
            // (Off a floor portal sideways, across its narrow way.)
            let d = if !crossed {
                p.center - feet
            } else if q.normal.y > 0.5 {
                let side = q.right() * q.to_local(feet).x.signum();
                if side.length_squared() > 0.0 { side } else { q.right() }
            } else {
                q.normal
            };
            if Vec2::new(d.x, d.z).length() > 1e-3 {
                self.c.yaw = (-d.x).atan2(-d.z);
            }
            let seen = self.events.len();
            self.step(&Input { move_axis: Vec2::new(0.0, self.entry_speed), ..Default::default() });
            crossed |= self.events[seen..].iter().any(|e| matches!(e, SimEvent::Teleported { entered: c, .. } if *c == color));
        }
        self.wait(0.3);
        true
    }

    /// Pick up the cube nearest `near` (walking up to it first).
    pub fn pick_up(&mut self, near: Vec3) -> usize {
        // (A cube: not one of the map's physics props, a chair or a mug.)
        let nearest = |p: &Play| p.sim.props.iter().enumerate().filter(|(_, c)| c.alive && c.fizzle.is_none() && c.kind != crate::level::CubeKind::Physics).min_by(|a, b| a.1.pos.distance(near).total_cmp(&b.1.pos.distance(near))).map(|(i, _)| i);
        // (One waiting in a dropper's tube: the map drops it when it's ready.)
        if nearest(self).is_some_and(|i| self.sim.in_dropper(i)) {
            self.wait_for(30.0, "the cube to drop", |p| nearest(p).is_some_and(|i| !p.sim.in_dropper(i) && p.sim.props[i].vel.length() < 0.1));
        }
        let i = nearest(self).expect("no cube");
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
        self.drop_held_at(at);
        self.wait(1.5);
        assert!(self.sim.buttons[b].pressed, "{}: the cube didn't stay on the button at {at}", self.name);
        b
    }

    /// Carry what she's holding over `at` (on the floor) and let go.
    pub fn drop_held_at(&mut self, at: Vec3) {
        let held = self.sim.held.expect("not holding anything");
        // Looking level, the cube rides about 2 m out in front: stand that far back from the
        // spot, facing it, and line the cube up over it.
        // (Her side of it, or round it: somewhere with floor, not over a portal.)
        let back = {
            let d0 = Vec2::new(self.c.feet.x - at.x, self.c.feet.z - at.z).normalize_or(Vec2::Y);
            (0..8)
                .map(|k| Vec2::from_angle(k as f32 * std::f32::consts::FRAC_PI_4).rotate(d0))
                .map(|d| at + Vec3::new(d.x, 0.0, d.y) * 1.9)
                .find(|p| !self.floors(p.x, p.z, at.y - 0.6, at.y + 0.6).is_empty())
                .unwrap_or(at + Vec3::new(d0.x, 0.0, d0.y) * 1.9)
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
        assert_eq!(self.sim.held, Some(held), "{}: lost the cube lining it up over {at}", self.name);
        self.sim.use_key(Aim::of(&self.c));
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
        self.enter_when(color, at, to, true);
    }

    /// As [`enter`](Self::enter), straight in (portals she's placed herself, that stay put).
    pub fn enter_now(&mut self, color: PortalColor, at: Vec3, to: Option<Vec3>) {
        self.enter_when(color, at, to, false);
    }

    fn enter_when(&mut self, color: PortalColor, at: Vec3, to: Option<Vec3>, fresh: bool) {
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
            let end = self.time + if fresh { 6.0 } else { 0.0 };
            while both(self) && self.time < end {
                self.step(&Input::default());
            }
            self.wait_for(60.0, &format!("the {color:?} portal at {at} and the other at {to:?}"), both);
            if self.through_at(color, Some(at), to, 0.4) {
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
    // Into the room: the dropper drops the cube.
    p.go(Vec3::new(20.0, 3.25, 13.8), 0.6);
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
    /// Shoot `color` at `target` from a spot within `radius` of `near` (on its floor) that has
    /// `target` in sight and that no standing turret can see.
    pub fn safe_shot(&mut self, color: PortalColor, target: Vec3, near: Vec3, radius: f32) {
        let unseen = |p: &Play, feet: Vec3| !(0..p.sim.turrets.len()).any(|t| p.sim.turret_sees(t, feet + Vec3::Y * 1.2));
        let in_sight = |p: &Play, eye: Vec3| p.sim.trace(eye, (target - eye).normalize(), 200.0, crate::sim::Mask::SHOT, 0).1.is_some_and(|h| h.point.distance(target) < 0.8);
        // Spots with it in sight: unseen ones, and seen ones with cover a step away.
        let mut safe: Vec<(f32, Vec3)> = Vec::new();
        let mut peeks: Vec<(f32, Vec3, Vec3)> = Vec::new();
        let n = (radius / 0.5) as i32;
        let eye_off = self.c.view().eye - self.c.feet;
        let floor_at = |p: &Play, x: f32, z: f32| p.floors(x, z, near.y - 0.3, near.y + 0.3).into_iter().reduce(f32::max).map(|y| Vec3::new(x, y, z));
        for i in -n..=n {
            for k in -n..=n {
                let Some(feet) = floor_at(self, near.x + i as f32 * 0.5, near.z + k as f32 * 0.5) else { continue };
                if !in_sight(self, feet + eye_off) {
                    continue;
                }
                let d = feet.distance(self.c.feet);
                if unseen(self, feet) {
                    safe.push((d, feet));
                } else {
                    let cover = [(1.0f32, 0.0f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)].into_iter().find_map(|(dx, dz)| floor_at(self, feet.x + dx, feet.z + dz).filter(|c| unseen(self, *c)));
                    if let Some(c) = cover {
                        peeks.push((d, feet, c));
                    }
                }
            }
        }
        safe.sort_by(|a, b| a.0.total_cmp(&b.0));
        // (Least seen on the way there first: how much of the walk is in a turret's sight.)
        let exposure = |p: &Play, to: Vec3| p.path(to, 0.3).map(|w| w.iter().filter(|(q, _)| !unseen(p, *q)).count() as f32 * CELL);
        let mut safe: Vec<(f32, Vec3)> = safe.into_iter().take(24).filter_map(|(d, s)| exposure(self, s).map(|e| (e.max(1.5) * 100.0 + d, s))).collect();
        safe.sort_by(|a, b| a.0.total_cmp(&b.0));
        safe.truncate(8);
        for (_, spot) in safe {
            self.go(spot, 0.3);
            self.nudge(spot);
            if unseen(self, self.c.feet) && in_sight(self, self.c.view().eye) {
                self.shoot(color, target);
                return;
            }
        }
        // A quick look out from cover (a turret takes a moment to spot her).
        peeks.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut peeks: Vec<(f32, Vec3, Vec3)> = peeks.into_iter().take(24).filter_map(|(d, s, c)| exposure(self, c).map(|e| (e.max(1.5) * 100.0 + d, s, c))).collect();
        peeks.sort_by(|a, b| a.0.total_cmp(&b.0));
        peeks.truncate(8);
        for (_, spot, cover) in peeks {
            self.go(cover, 0.3);
            self.peek_shot(color, target, spot - cover);
            if self.sim.portals[color.index()].is_some_and(|q| q.center.distance(target) < 1.6) {
                return;
            }
        }
        panic!("{}: nowhere near {near} to shoot {target} from", self.name);
    }

    /// A quick look out from cover: sidestep `side` until `at` is in sight, fire `color` at it
    /// at once, and step back (a turret takes a moment to spot her).
    pub fn peek_shot(&mut self, color: PortalColor, at: Vec3, side: Vec3) {
        let home = self.c.feet;
        let side = Vec3::new(side.x, 0.0, side.z).normalize();
        let mut fired = false;
        for _ in 0..120 {
            let eye = self.c.view().eye;
            let dir = (at - eye).normalize();
            let seen = self.sim.trace(eye, dir, 200.0, crate::sim::Mask::SHOT, 0).1.is_some_and(|h| h.point.distance(at) < 0.8);
            if seen {
                self.look_at(at);
                fired = self.sim.fire(color, Aim { eye, dir, yaw: self.c.yaw });
                break;
            }
            // (Strafing, still facing the target.)
            let fwd = crate::portal::view_dir(self.c.yaw, 0.0);
            let right = Vec3::new(-fwd.z, 0.0, fwd.x);
            self.look_at(at);
            self.step(&Input { move_axis: Vec2::new(side.dot(right).signum(), 0.0), ..Default::default() });
        }
        assert!(fired, "{}: no {color:?} portal at {at} peeking out from {home}", self.name);
        self.step(&Input::default());
        self.go(home, 0.3);
    }

    /// Knock turret `i` over: up to it from `side` (a direction from the turret: behind it, out
    /// of its sight, as a rule), and a kick; or, failing that, a run into it.
    pub fn topple(&mut self, i: usize, side: Vec3) {
        let t = self.sim.turrets[i];
        let side = Vec3::new(side.x, 0.0, side.z).normalize();
        let stand = t.at + side * 1.0;
        let floor = self.floors(stand.x, stand.z, t.at.y - 1.0, t.at.y + 1.0).into_iter().reduce(f32::max).unwrap_or(t.at.y);
        self.go(Vec3::new(stand.x, floor, stand.z), 0.7);
        for _ in 0..3 {
            if !self.sim.turrets[i].upright() {
                break;
            }
            self.look_at(t.at + Vec3::Y * 0.8);
            self.step(&Input { melee_pressed: true, ..Default::default() });
            self.wait(0.6);
        }
        let end = self.time + 3.0;
        while self.sim.turrets[i].upright() && self.time < end {
            let d = t.at - self.c.feet;
            self.c.yaw = (-d.x).atan2(-d.z);
            self.step(&Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() });
        }
        assert!(!self.sim.turrets[i].upright(), "{}: turret {i} at {} still standing (she's at {})", self.name, t.at, self.c.feet);
        self.wait(0.5);
        self.assert_alive(&format!("knocking turret {i} over"));
    }

    /// Debugging: walk to `at` and shoot all round, listing where portals would open (none
    /// is left open).
    #[allow(dead_code)]
    pub fn probe_portals(&mut self, at: Vec3) {
        self.go(at, 0.5);
        let eye = self.c.view().eye;
        let mut seen: Vec<(Vec3, Vec3)> = Vec::new();
        for yaw_deg in (0..360).step_by(8) {
            for pitch_deg in (-80..=85).step_by(5) {
                let (y, pi) = ((yaw_deg as f32).to_radians(), (pitch_deg as f32).to_radians());
                let dir = crate::portal::view_dir(y, pi);
                let (saved, on) = (self.sim.portals, self.sim.portal_mover);
                let ok = self.sim.fire(PortalColor::Blue, Aim { eye, dir, yaw: y });
                let got = self.sim.portals[0];
                (self.sim.portals, self.sim.portal_mover) = (saved, on);
                if let Some(g) = got.filter(|_| ok) {
                    if !seen.iter().any(|(c, n)| c.distance(g.center) < 1.5 && n.dot(g.normal) > 0.9) {
                        seen.push((g.center, g.normal));
                    }
                }
            }
        }
        for (c, n) in seen {
            // (Where she'd land coming out of it: the floor under a step out of it.)
            let out = c + n * 0.8;
            let land = self.floors(out.x, out.z, out.y - 30.0, out.y).into_iter().reduce(f32::max);
            eprintln!("portal from {}: {c:.2?} facing {n:.2?} lands on {land:.2?}", self.c.feet);
        }
    }

    /// Debugging: every floor she can walk to from here (as the path-finder's search with no
    /// goal), as `x z y` lines written to `path`.
    #[allow(dead_code)]
    pub fn dump_reach(&self, path: &str) {
        let cell = |p: f32| (p / CELL).floor() as i32;
        let centre = |i: i32| (i as f32 + 0.5) * CELL;
        let mut seen: HashMap<(i32, i32), f32> = HashMap::new();
        let mut q = std::collections::VecDeque::new();
        let at = self.c.feet;
        let Some(&y0) = self.floors(centre(cell(at.x)), centre(cell(at.z)), at.y - 1.0, at.y + 0.3).first() else { return };
        q.push_back((cell(at.x), cell(at.z), y0));
        seen.insert((cell(at.x), cell(at.z)), y0);
        while let Some((i, k, y)) = q.pop_front() {
            for (di, dk) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (a, b) = (i + di, k + dk);
                if seen.contains_key(&(a, b)) || seen.len() > 400_000 {
                    continue;
                }
                if let Some(ny) = self.floors(centre(a), centre(b), y - DROP, y + STEP).into_iter().reduce(f32::max) {
                    seen.insert((a, b), ny);
                    q.push_back((a, b, ny));
                }
            }
        }
        let text: String = seen.iter().map(|((i, k), y)| format!("{} {} {}\n", centre(*i), centre(*k), y)).collect();
        let _ = std::fs::write(path, text);
    }

    /// Debugging: which cells she can reach from where she is (the path-finder's search with
    /// no goal), drawn over x0..x1, z0..z1: reached ones by their height over hers (metres,
    /// 0-9 then A-Z), '.' a floor not reached.
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
                // (Its height above hers, a metre a step: 0-9, then A-Z.)
                Some(&y) => char::from_digit(((y - at.y).round().max(0.0) as u32).min(35), 36).unwrap_or('#').to_ascii_uppercase(),
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
/// the east wall and up through it to the fixed orange's raised alcove; from there blue on the
/// button room's far wall, through the square opening in the dividing wall (not through the
/// grill: portal shots stop at grills), back into orange with the cube and out past the grill,
/// the cube on the button, and out of the doors it opens.
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
    p.shoot(PortalColor::Blue, Vec3::new(-15.25, 3.01, -6.5));
    let far = p.sim.portals[0].unwrap().center;
    eprintln!("blue at {far}");
    p.enter(PortalColor::Orange, orange, Some(far));
    eprintln!("past the grill at {} holding {:?}", p.c.feet, p.sim.held);
    p.cube_on_button(Vec3::new(-7.39, 0.2, -1.52));
    p.ride_exit("testchmb_a_06");
    p.assert_alive("in Chamber 09");
}

/// Chamber 10 (testchmb_a_06): Portal's first flings. Area A: through the fixed orange behind
/// the corridor's end wall, at a run, onto the platform beyond. Area B: from the ledge over the
/// pit, blue on its floor below, and a jump in: flung out of the orange high on the wall
/// behind, over the pit. Area C: pillars slide out over the pit (orange on its floor); blue on
/// the lower one's end, down into orange and out onto the first ledge; the upper one slides
/// further, blue on its end, and down again from the ledge: out up to the exit.
#[test]
fn solve_testchmb_a_06() {
    let Some(mut p) = Play::load("testchmb_a_06") else { return };
    // Area A: past the trigger that opens the orange behind the corridor's end wall; blue on
    // the corridor's wall, and through to it.
    let orange_a = Vec3::new(-3.25, 5.08, 38.51);
    p.go(Vec3::new(-3.25, 0.0, 39.6), 0.4);
    p.wait_portal(PortalColor::Orange, orange_a, 10.0);
    p.go(Vec3::new(-3.25, 0.0, 46.0), 0.4);
    p.shoot(PortalColor::Blue, Vec3::new(-4.88, 1.42, 43.0));
    let wall = p.sim.portals[0].unwrap().center;
    // (Running in: out of orange she has to carry over onto the platform beyond the wall.)
    p.entry_speed = 1.0;
    p.enter(PortalColor::Blue, wall, Some(orange_a));
    p.entry_speed = 0.6;
    eprintln!("past the wall at {}", p.c.feet);
    // Area B: up the stairs to the ledge over the pit (its trigger opens orange high on the
    // wall behind); blue on the pit's floor below, and in: out of orange, flung over the pit.
    let orange_b = Vec3::new(-3.25, 11.38, 26.21);
    p.go(Vec3::new(-3.25, 3.25, 26.0), 0.5);
    p.wait_portal(PortalColor::Orange, orange_b, 10.0);
    eprintln!("on the ledge at {}", p.c.feet);
    p.go(Vec3::new(-3.25, 3.25, 19.55), 0.2);
    // (On the pit's lower floor, down below.)
    let pit = Vec3::new(-3.2, -3.25, 17.1);
    p.shoot(PortalColor::Blue, pit);
    eprintln!("blue {:?}", p.sim.portals[0].map(|q| (q.center, q.normal)));
    p.entry_speed = 1.0;
    p.through(PortalColor::Blue, 1.0);
    p.entry_speed = 0.6;
    p.wait_for(10.0, "her to land", |p| p.c.state == State::Ground);
    p.wait(0.5);
    eprintln!("flung to {}", p.c.feet);
    p.assert_alive("flung over the pit");
    // Area C: in, the pillars slide out over the pit (orange opens on its floor). Blue on the
    // lower pillar's end; down into orange, out over the pit onto the first ledge.
    p.go(Vec3::new(-3.25, 3.25, -8.0), 0.5);
    p.wait(8.0);
    p.go(Vec3::new(-3.25, 3.25, -12.5), 0.5);
    p.shoot(PortalColor::Blue, Vec3::new(-3.25, 11.4, -8.13));
    // (Off the edge at a walk: run, and she overshoots the portal's middle and is pulled back
    // the other way, which comes out of blue as a fall.)
    p.entry_speed = 0.35;
    p.through(PortalColor::Orange, 1.0);
    p.entry_speed = 0.6;
    p.wait_for(5.0, "her to land", |p| p.c.state == State::Ground);
    eprintln!("on the first ledge at {}", p.c.feet);
    p.assert_alive("flung onto the first ledge");
    // Landing, the upper pillar slides further out: blue on its end, and down into orange
    // again, out up to the exit.
    let top = p.mover("fling_pillar3_door_bottom_a2");
    p.wait_for(10.0, "the upper pillar to slide out", |p| p.sim.movers[top].node == 0 && p.sim.movers[top].goal.is_none());
    p.shoot(PortalColor::Blue, Vec3::new(-3.35, 14.42, -10.16));
    // (At a run this time, from the ledge's side: past the portal's middle, she's pulled back
    // towards it, which comes out of blue as a lift.)
    p.entry_speed = 1.0;
    p.through(PortalColor::Orange, 1.0);
    p.entry_speed = 0.6;
    p.wait_for(5.0, "her to land", |p| p.c.state == State::Ground);
    eprintln!("at the exit at {}", p.c.feet);
    p.assert_alive("flung to the exit");
    p.ride_exit("testchmb_a_07");
}

impl Play {
    /// Press the switch (a door she uses) of the map's entity `name`: walk up to it, look at it,
    /// use it.
    pub fn press(&mut self, name: &str) {
        let m = self.mover(name);
        let at = self.sim.movers[m].boxes().next().expect("a switch with no box").center();
        self.press_at(at);
    }

    /// Press the switch at `at` (one with no name).
    pub fn press_at(&mut self, at: Vec3) {
        let floor = self.floors(at.x, at.z, at.y - 3.0, at.y).into_iter().reduce(f32::max).unwrap_or(at.y - 1.0);
        self.go(Vec3::new(at.x, floor, at.z), 1.2);
        self.look_at(at);
        self.sim.use_key(Aim::of(&self.c));
        self.wait(0.3);
        assert!(self.events.iter().rev().take(40).any(|e| matches!(e, SimEvent::PedestalPressed { .. })), "{}: pressing the switch at {at} did nothing (from {})", self.name, self.c.feet);
    }
}

/// Chamber 11 (testchmb_a_07's first half): the turning gun puts orange in three places in
/// turn. Blue in the corridor, out of orange behind the switch's platform, the switch (it opens
/// the launcher room); blue on its back wall, in by orange; blue where the pellet hits, out of
/// orange opposite the catcher; onto the scaffold by blue; the upgraded gun, the switch that
/// rises, orange beyond the door it opens, blue where the scaffold stops; over on it and through.
/// Then Chamber 12 (its second half), as Portal's walkthrough has it: flung from the pit's
/// floor out of the panels jutting from the wall, higher each time, up to the button's
/// platform; there the wedge swings out of the wall and tips, and a portal on it flings her
/// up to the exit's ledge, where the cube drops; down with it onto the button, flung up again
/// and out.
#[test]
fn solve_testchmb_a_07() {
    let Some(mut p) = Play::load("testchmb_a_07") else { return };
    let spot_a = Vec3::new(9.8, 4.7, -11.4);
    p.go(Vec3::new(10.5, 9.96, -4.6), 0.5);
    p.shoot(PortalColor::Blue, Vec3::new(6.5, 11.6, -4.6));
    let blue = p.sim.portals[0].unwrap().center;
    eprintln!("blue at {:?}", p.sim.portals[0].map(|q| (q.center, q.normal)));
    p.enter(PortalColor::Blue, blue, Some(spot_a));
    eprintln!("behind the switch at {}", p.c.feet);
    p.press("button_1");
    eprintln!("pressed at {}", p.c.feet);
    let door = p.mover("door_launcher_room");
    p.wait_for(10.0, "the launcher room's door", |p| p.sim.movers[door].node == 1 && p.sim.movers[door].goal.is_none());
    // Blue through it on the launcher room's back wall; through orange when it's back behind
    // her, and out in there.
    p.shoot(PortalColor::Blue, Vec3::new(9.43, 8.2, 21.94));
    let back = p.sim.portals[0].unwrap().center;
    p.enter(PortalColor::Orange, spot_a, Some(back));
    eprintln!("in the launcher room at {}", p.c.feet);
    // Part 2: blue where the pellet hits the east wall; when orange's opposite the catcher,
    // the pellet goes through to it.
    let spot_b = Vec3::new(-4.88, 4.34, 4.86);
    p.shoot(PortalColor::Blue, Vec3::new(12.9, 8.14, 16.2));
    eprintln!("blue at {:?}", p.sim.portals[0].map(|q| (q.center, q.normal)));
    let cart = p.mover("rail_cart_rm6");
    let wall = p.sim.portals[0].unwrap().center;
    p.wait_for(40.0, "the pellet caught (the cart sets off)", |p| p.sim.movers[cart].goal.is_some());
    eprintln!("cart path {:?} home {}", p.sim.movers[cart].def.path, p.sim.movers[cart].def.home);
    p.wait_for(20.0, "the cart to stop", |p| p.sim.movers[cart].goal.is_none());
    eprintln!("cart at {} boxes {:?}", p.sim.movers[cart].at, p.sim.movers[cart].boxes().collect::<Vec<_>>());
    // Through blue onto the scaffold, when orange's at B again.
    p.enter(PortalColor::Blue, wall, Some(spot_b));
    eprintln!("out at {}", p.c.feet);
    // Part 3: the cart takes her back to the middle, to the gun on its pedestal: hers (orange too).
    p.wait_for(30.0, "the cart back", |p| p.sim.movers[cart].node == 0 && p.sim.movers[cart].goal.is_none());
    eprintln!("cart back, her at {}", p.c.feet);
    let gun = p.sim.gun_pose(0).0;
    p.go(Vec3::new(gun.x, p.c.feet.y, gun.z), 0.8);
    p.wait_for(3.0, "her to take the gun", |p| p.sim.gun.orange);
    eprintln!("both colours at {}", p.c.feet);
    let lift = p.mover("button_pedistal");
    // (It starts down, open: taking the gun shuts it, raising the switch.)
    p.wait_for(15.0, "the switch to rise", |p| p.sim.movers[lift].node == 0 && p.sim.movers[lift].goal.is_none());
    p.press("button_exit_door_a07_pA");
    let door = p.mover("door_exit_a07_pA");
    p.wait_for(10.0, "the exit door to open", |p| p.sim.movers[door].node == 1 && p.sim.movers[door].goal.is_none());
    p.go(Vec3::new(8.0, 3.15, 4.88), 0.4);
    // Orange through the opened door on the wall beyond; blue on the wall where the scaffold
    // stops, onto the scaffold, over, and through.
    p.shoot(PortalColor::Orange, Vec3::new(30.07, 8.9, 4.88));
    let beyond = p.sim.portals[1].unwrap().center;
    eprintln!("orange at {:?}", p.sim.portals[1].map(|q| (q.center, q.normal)));
    // (Since she stood by the gun, the cart shuttles: 2 s at each end.)
    let west = Vec3::new(-4.88, 4.6, 4.86);
    p.shoot(PortalColor::Blue, west);
    let west = p.sim.portals[0].unwrap().center;
    p.wait_for(40.0, "the cart back by the pedestal", |p| p.sim.movers[cart].node == 0 && p.sim.movers[cart].goal.is_none());
    let deck = p.sim.movers[cart].boxes().next().unwrap();
    p.go(Vec3::new(deck.center().x, deck.max.y, deck.center().z), 0.4);
    p.wait_for(40.0, "the cart over to the west wall", |p| p.sim.movers[cart].node == 1 && p.sim.movers[cart].goal.is_none());
    // (In before it sets off back.)
    p.enter_now(PortalColor::Blue, west, Some(beyond));
    eprintln!("past the exit door at {}", p.c.feet);
    p.assert_alive("in Chamber 11");
    p.ride("07a_07b_elevator_body");
    eprintln!("up to Chamber 12 at {}", p.c.feet);
    // Chamber 12, as Portal's walkthrough has it. From the ledge she arrives on: orange on the
    // pit's floor, blue on the lower panel jutting from the far wall; down into orange, out
    // of blue onto the ledge over the arrival one.
    p.go(Vec3::new(29.4, 19.9, -5.0), 0.4);
    p.nudge(Vec3::new(30.1, 19.5, -5.0));
    p.shoot(PortalColor::Orange, Vec3::new(34.5, 13.0, -5.0));
    p.shoot(PortalColor::Blue, Vec3::new(36.6, 30.7, -5.0));
    p.through(PortalColor::Orange, 1.0);
    p.wait_for(5.0, "her to land", |p| p.c.state == State::Ground);
    eprintln!("landed at {}", p.c.feet);
    // Blue on the higher panel; off this ledge into orange, out onto the button's platform.
    p.shoot(PortalColor::Blue, Vec3::new(30.9, 35.7, -5.0));
    p.through(PortalColor::Orange, 1.0);
    p.wait_for(5.0, "her to land", |p| p.c.state == State::Ground);
    eprintln!("landed at {}", p.c.feet);
    // Up here she sees the wedge: its piston pushes it out of the wall, and it tips.
    let wedge = p.mover("rotating_door_wedge");
    p.wait_for(20.0, "the wedge to tip", |p| p.sim.movers[wedge].node == 1 && p.sim.movers[wedge].goal.is_none());
    // Blue on it; off the platform, down into orange in the pit, flung out of the wedge up
    // onto the exit's ledge.
    p.go(Vec3::new(24.0, 32.8, -5.0), 0.5);
    p.shoot(PortalColor::Blue, Vec3::new(15.7, 38.7, -4.9));
    // (Off the platform at a run, to clear the ledge below it.)
    p.entry_speed = 1.0;
    p.through(PortalColor::Orange, 1.0);
    p.entry_speed = 0.6;
    p.wait_for(5.0, "her to land", |p| p.c.state == State::Ground);
    eprintln!("on the exit's ledge at {}", p.c.feet);
    p.wait(3.0);
    // The cube's dropped beside her: down with it onto the button (an 8 m drop, which she
    // lands hard but whole).
    p.pick_up(Vec3::new(34.1, 41.1, -8.2));
    p.cube_on_button(Vec3::new(19.6, 32.75, -4.9));
    let exit = p.mover("exit_door_left_a07_2");
    p.wait_for(5.0, "the exit doors to open", |p| p.sim.movers[exit].node == 1 && p.sim.movers[exit].goal.is_none());
    // Flung up to the exit ledge the same way again, and out.
    p.go(Vec3::new(24.0, 32.8, -5.0), 0.5);
    p.entry_speed = 1.0;
    p.through(PortalColor::Orange, 1.0);
    p.entry_speed = 0.6;
    p.wait_for(5.0, "her to land", |p| p.c.state == State::Ground);
    p.assert_alive("back up at the exit");
    p.ride_exit("testchmb_a_08");
}

/// Chamber 13 (testchmb_a_08): Portal's first with both colours. The first cube's up on a
/// ledge: up to it by her portals, down off the ledge with it onto the first button; back up
/// and through the door it opens into the big room; the pellet from the wall it bounces off to
/// a ceiling portal over the catcher; down onto the cart from a ceiling portal for its cube,
/// onto the second button from another; herself onto the third; the exit through her portals.
#[test]
fn solve_testchmb_a_08() {
    let Some(mut p) = Play::load("testchmb_a_08") else { return };
    p.go(Vec3::new(5.0, 3.5, 12.5), 0.6);
    // (To the side of the cube: it stands just in front of the wall.)
    p.shoot(PortalColor::Blue, Vec3::new(-6.5, 8.33, 13.2));
    let ledge = p.sim.portals[0].unwrap().center;
    p.shoot(PortalColor::Orange, Vec3::new(6.5, 4.9, 12.34));
    let near = p.sim.portals[1].unwrap().center;
    p.enter_now(PortalColor::Orange, near, Some(ledge));
    p.pick_up(Vec3::new(-5.54, 7.14, 11.94));
    p.cube_on_button(Vec3::new(-0.79, 3.5, 11.46));
    // Back up to the ledge, through the door the button opened, down into the big room.
    p.enter_now(PortalColor::Orange, near, Some(ledge));
    p.go(Vec3::new(-2.0, 0.0, 3.0), 0.6);
    p.assert_alive("dropping into the big room");
    // The pellet: blue on the ceiling over the catcher, orange on the wall it bounces off.
    p.shoot(PortalColor::Blue, Vec3::new(-0.39, 9.75, -2.19));
    let wall_z = p.sim.raycast(Vec3::new(-4.88, 4.3, 0.0), Vec3::Z, 20.0, crate::sim::Mask::SOLID).unwrap().point.z;
    p.shoot(PortalColor::Orange, Vec3::new(-4.88, 4.3, wall_z));
    p.wait_for(20.0, "the pellet caught", |p| p.events.iter().any(|e| matches!(e, SimEvent::PelletGone { caught: true, .. })));
    let cart = p.mover("func_tracktrain_lvl7");
    // The cart's cube: blue on the ceiling over its track, orange low on the south wall; in as
    // the cube comes up, down onto the cart, the cube, off with it.
    let low = Vec3::new(0.0, 1.42, 6.5);
    p.go(Vec3::new(0.0, 0.0, 3.5), 0.4);
    p.shoot(PortalColor::Orange, low);
    let low = p.sim.portals[1].unwrap().center;
    p.shoot(PortalColor::Blue, Vec3::new(7.2, 9.75, -6.0));
    let over = p.sim.portals[0].unwrap().center;
    let cube = p.sim.props.iter().position(|c| c.alive && c.kind != crate::level::CubeKind::Physics && c.pos.x > 5.0).expect("the cart's cube");
    p.wait_for(40.0, "the cube coming up under the ceiling portal", |p| p.sim.movers[cart].goal == Some(1) && (p.sim.props[cube].pos.z - over.z - 3.0).abs() < 0.3);
    p.enter_now(PortalColor::Orange, low, Some(over));
    p.wait_for(5.0, "her to land", |p| p.c.state == State::Ground);
    eprintln!("landed at {} (cube at {})", p.c.feet, p.sim.props[cube].pos);
    p.pick_up(p.sim.props[cube].pos);
    // Over button C (on its platform) by a ceiling portal beside it, with the cube.
    p.go(Vec3::new(2.0, 0.0, -4.0), 0.6);
    p.shoot(PortalColor::Blue, Vec3::new(-3.0, 9.75, -7.24));
    let over_c = p.sim.portals[0].unwrap().center;
    p.enter_now(PortalColor::Orange, low, Some(over_c));
    p.wait_for(5.0, "her to land", |p| p.c.state == State::Ground);
    eprintln!("over button C at {}", p.c.feet);
    // (Its platform's too small to stand back from it: from beside it, looking down at it.)
    let c = p.sim.buttons.iter().position(|b| b.at.distance(Vec3::new(-4.75, 3.25, -7.24)) < 1.0).expect("button C");
    let at = p.sim.buttons[c].at;
    p.look_at(at);
    p.wait(0.6);
    p.sim.use_key(Aim::of(&p.c));
    p.wait(1.5);
    assert!(p.sim.buttons[c].pressed, "the cube's not on button C (it's at {})", p.sim.props[cube].pos);
    p.assert_alive("putting the second cube down");
    // Herself onto button B (on its pillar) by a ceiling portal over it.
    p.shoot(PortalColor::Blue, Vec3::new(-7.36, 9.75, 0.89));
    let over_b = p.sim.portals[0].unwrap().center;
    p.enter_now(PortalColor::Orange, low, Some(over_b));
    p.wait_for(5.0, "her to land", |p| p.c.state == State::Ground);
    let b = p.sim.buttons.iter().position(|x| x.at.distance(Vec3::new(-7.36, 5.69, 0.89)) < 1.0).expect("button B");
    eprintln!("on button B at {}: pressed {}", p.c.feet, p.sim.buttons[b].pressed);
    let door = p.mover("exit_door_left_a08");
    p.wait_for(5.0, "the exit doors", |p| p.sim.movers[door].node == 1);
    // Orange through the open doors onto the floor beyond, blue low on the south wall; down off
    // the pillar (the doors shut again) and through.
    p.shoot(PortalColor::Orange, Vec3::new(0.0, 3.25, -15.0));
    let beyond = p.sim.portals[1].unwrap().center;
    p.shoot(PortalColor::Blue, Vec3::new(0.0, 1.42, 6.5));
    let low = p.sim.portals[0].unwrap().center;
    p.enter_now(PortalColor::Blue, low, Some(beyond));
    eprintln!("past the exit doors at {}", p.c.feet);
    // (Off the doors' sill, should she come down on it, to the floor beside the portal.)
    p.nudge(Vec3::new(1.5, 3.25, -14.0));
    p.assert_alive("in Chamber 13");
    p.ride_exit("testchmb_a_09");
}

/// Chamber 14 (testchmb_a_09), as Portal's own walkthrough has it: up the stairs (they rise as
/// she comes to them, and sink once she's at the top), flung off the ledge into the sunken
/// stairs and out high over the cube's shelf; the cube down the stairs onto the button, which
/// opens the way to the pellet; the pellet sent by portal down into the ledge's catcher, which
/// lowers the lift; back over the goo, up the lift and out.
#[test]
fn solve_testchmb_a_09() {
    let Some(mut p) = Play::load("testchmb_a_09") else { return };
    // The stairs rise as she comes up to them.
    p.go(Vec3::new(4.0, 0.0, -13.8), 0.5);
    let top = p.mover("door_stair_a09_16");
    p.wait_for(10.0, "the stairs to rise", |p| p.sim.movers[top].node == 0 && p.sim.movers[top].goal.is_none());
    p.go(Vec3::new(17.0, 6.5, -13.8), 0.6);
    p.assert_alive("on the stairs");
    p.go(Vec3::new(22.0, 6.5, -13.0), 0.6);
    // The stairs sink behind her. Blue high on the far wall, facing the cube's shelf; orange
    // on the sunken stairs; off the ledge into orange, flung out of blue onto the shelf.
    p.wait_for(10.0, "the stairs to sink", |p| p.sim.movers[top].goal.is_none() && p.sim.movers[top].node != 0);
    p.shoot(PortalColor::Blue, Vec3::new(32.5, 14.6, -27.4));
    p.go(Vec3::new(18.4, 6.5, -13.8), 0.3);
    p.nudge(Vec3::new(18.0, 6.5, -13.8));
    p.shoot(PortalColor::Orange, Vec3::new(15.0, -3.25, -13.8));
    p.through(PortalColor::Orange, 1.0);
    // She catches the shelf's lip; up onto it, and the cube.
    p.wait_for(3.0, "her to catch the shelf", |p| matches!(p.c.state, State::LedgeHang { .. } | State::Ground));
    p.c.yaw = std::f32::consts::FRAC_PI_2;
    for _ in 0..90 {
        p.step(&Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() });
    }
    p.wait(0.5);
    p.assert_alive("flung onto the shelf");
    p.pick_up(Vec3::new(19.6, 12.7, -27.5));
    // Down off the shelf with it: the stairs rise for good. Down them, onto the button by the
    // hall.
    p.go(Vec3::new(25.0, 6.5, -24.0), 0.6);
    p.wait_for(10.0, "the stairs to rise again", |p| p.sim.movers[top].node == 0 && p.sim.movers[top].goal.is_none());
    p.go(Vec3::new(-20.0, 0.0, -13.0), 0.6);
    p.cube_on_button(Vec3::new(-22.0, 0.2, -13.0));
    let door = p.mover("door_left_a08");
    p.wait_for(3.0, "the doors to the pellet room to open", |p| p.sim.movers[door].goal.is_none() && p.sim.movers[door].node != 0);
    // The pellet: orange on the ceiling over the ledge's catcher (from the stairs), blue on
    // the launcher room's wall where the pellet turns back. It comes down into the catcher.
    p.go(Vec3::new(12.0, 4.0, -13.8), 0.6);
    p.shoot(PortalColor::Orange, Vec3::new(21.9, 13.0, -13.0));
    // Over the goo to the launcher room: its platforms sink for a while every 12 s once she's
    // in, so each is crossed just as it's come back up (to the island between them, then on).
    let cross = |p: &mut Play, lift: &str, to: Vec3| {
        let m = p.mover(lift);
        p.wait_for(15.0, &format!("{lift} to sink"), |p| p.sim.movers[m].node == 1);
        p.wait_for(15.0, &format!("{lift} to come back up"), |p| p.sim.movers[m].node == 0 && p.sim.movers[m].goal.is_none());
        p.go(to, 0.6);
    };
    p.go(Vec3::new(-47.5, 0.0, -21.0), 0.6);
    cross(&mut p, "lift_lvl7a_goo1", Vec3::new(-47.0, 0.0, -10.5));
    cross(&mut p, "lift_lvl7a_goo3", Vec3::new(-47.5, 0.0, 0.5));
    // Across the pellet's path just after it's gone by, heading off to the far wall.
    p.wait_for(20.0, "the pellet to pass", |p| p.sim.pellets.iter().any(|q| q.alive && q.vel.x > 0.0 && q.pos.x > -45.5 && q.pos.x < -44.0));
    p.go(Vec3::new(-44.0, 0.0, 8.3), 0.6);
    p.assert_alive("past the pellet");
    p.shoot(PortalColor::Blue, Vec3::new(-39.4, 1.56, 5.69));
    let lift = p.mover("lift_lvl7a");
    p.wait_for(30.0, "the pellet to reach the catcher (the lift to rise)", |p| p.sim.movers[lift].goal.is_some() || p.sim.movers[lift].node != 0);
    p.assert_alive("sending the pellet");
    // Back over the goo the same way.
    p.go(Vec3::new(-47.5, 0.0, 0.5), 0.6);
    cross(&mut p, "lift_lvl7a_goo3", Vec3::new(-47.0, 0.0, -10.5));
    cross(&mut p, "lift_lvl7a_goo1", Vec3::new(-47.5, 0.0, -21.0));
    p.assert_alive("back over the goo");
    // Down the hall onto the lift it lowered; up, and on to the elevator.
    p.wait_for(10.0, "the lift to come down", |p| p.sim.movers[lift].goal.is_none());
    let deck = p.sim.movers[lift].boxes().fold(f32::MIN, |m, b| m.max(b.max.y));
    p.go(Vec3::new(-13.0, deck, -9.75), 0.4);
    p.wait_for(10.0, "the lift to take her up", |p| p.sim.movers[lift].goal.is_some());
    p.wait_for(15.0, "the lift to stop", |p| p.sim.movers[lift].goal.is_none());
    eprintln!("up the lift at {}", p.c.feet);
    p.ride_exit("testchmb_a_10");
}

/// Portals go on Portal's tilted faces on their true plane (#47): a diagonal wall in
/// Chamber 16 (testchmb_a_11), walked through; a 45° ramp face in Chamber 18 (testchmb_a_14).
#[test]
fn tilted_faces_take_portals() {
    if let Some(mut p) = Play::load("testchmb_a_11") {
        let at = Vec3::new(-6.5, 1.6, -21.13);
        let n = Vec3::new(1.0, 0.0, 1.0).normalize();
        let stand = at + n * 3.0;
        // (The chamber's turrets watch this corner.)
        for t in &mut p.sim.turrets {
            t.state = crate::sim::TurretState::Retired;
        }
        let floor = p.floors(stand.x, stand.z, -2.0, 1.5).into_iter().reduce(f32::max).expect("no floor in front of the diagonal wall");
        p.c.feet = Vec3::new(stand.x, floor, stand.z);
        p.c.vel = Vec3::ZERO;
        p.wait(0.3);
        p.shoot(PortalColor::Blue, at);
        let blue = p.sim.portals[0].unwrap();
        assert!(blue.normal.dot(n) > 0.999, "the portal on the diagonal wall faces {}", blue.normal);
        assert!(blue.up.dot(Vec3::Y) > 0.999, "and its up is {}", blue.up);
        // Out of a floor portal behind her.
        let back = Vec3::new(stand.x + n.x * 2.0, floor, stand.z + n.z * 2.0);
        p.shoot(PortalColor::Orange, back);
        p.through(PortalColor::Blue, 0.5);
        // And back: down the floor portal, out of the diagonal wall.
        p.through(PortalColor::Orange, 0.5);
        let out = p.c.feet - blue.center;
        assert!(out.dot(n) > 0.4, "not out in front of the diagonal wall: {}", p.c.feet);
        p.assert_alive("through the diagonal wall");
    }
    if let Some(mut p) = Play::load("testchmb_a_14") {
        let at = Vec3::new(-19.145, 77.63, 30.89);
        let n = Vec3::new(1.0, 1.0, 0.0).normalize();
        let eye = at + n * 3.0;
        assert!(p.sim.fire(PortalColor::Blue, Aim { eye, dir: -n, yaw: 0.0 }), "no portal on the 45° face");
        let blue = p.sim.portals[0].unwrap();
        assert!(blue.normal.dot(n) > 0.999 && blue.center.distance(at) < 1.6, "the portal on the 45° face: {blue:?}");
        // Its top up the slope.
        assert!(blue.up.y > 0.7, "its up is {}", blue.up);
    }
}

/// Chamber 15 (testchmb_a_10), much as Portal's walkthrough has it. Section 1: flung over the
/// glass from the panel jutting from the back wall (twice through, to pick up speed). Section 2:
/// the pellet sent back through the grill by her portals, then (past the grill) into the
/// receptacle out of a ceiling portal; the lift it lowers takes her up to section 3. Section 3:
/// flung over the glass again, by portals reshot as she falls. Section 4: over the partition
/// (she climbs it), the pellet into its catcher off the far end's angled panel; across the goo
/// on the scaffold (by portals onto it at the corridor's corner, then at its bend) to the west
/// end; up a lift, the two switches (by portals between their rooms), the last pellet through
/// the doors they open into its catcher, and up the victory lift to the exit.
#[test]
fn solve_testchmb_a_10() {
    let Some(mut p) = Play::load("testchmb_a_10") else { return };
    p.wait(3.0);
    // Section 1: blue on the panel jutting from the back wall, high; orange on the floor in
    // front of the glass; in, out of blue, down into orange again and out fast, over the glass
    // and its grill.
    p.go(Vec3::new(70.0, -9.3, 27.0), 0.5);
    p.shoot(PortalColor::Blue, Vec3::new(70.0, 1.62, 34.14));
    p.shoot(PortalColor::Orange, Vec3::new(70.0, -9.75, 25.0));
    p.through(PortalColor::Orange, 1.0);
    p.wait_for(5.0, "her to land past the glass", |p| p.c.state == State::Ground && p.c.feet.z < 22.0);
    p.assert_alive("flung over the glass");
    eprintln!("over the glass at {}", p.c.feet);
    // Section 2: the launcher fires into a plate in front of it, which sends its pellet back
    // across the room, through the grill, to the end wall. Orange on the panels over the lift,
    // blue on that wall where the pellet bounces: it comes back out of orange along another
    // line, through the grill to the far side's end wall, and back.
    p.go(Vec3::new(54.0, -9.75, 0.0), 0.4);
    p.shoot(PortalColor::Orange, Vec3::new(58.06, -5.06, 6.5));
    p.shoot(PortalColor::Blue, Vec3::new(50.54, -5.02, 6.5));
    p.wait_for(15.0, "the pellet out of orange", |p| p.sim.pellets.iter().any(|q| q.alive && (q.pos.x - 58.06).abs() < 0.2 && q.pos.z > -2.0 && q.vel.z < 0.0));
    // Through the grill (her portals go; the pellet bounces on along its new line), blue on
    // the far wall where it bounces, orange on the ceiling over the receptacle.
    p.go(Vec3::new(59.5, -9.75, -5.0), 0.5);
    assert!(p.sim.portals.iter().all(Option::is_none), "the grill didn't clear her portals");
    p.shoot(PortalColor::Blue, Vec3::new(58.06, -5.03, -12.8));
    p.shoot(PortalColor::Orange, Vec3::new(58.37, -3.25, -8.06));
    let lift = p.mover("lift_a_rm8");
    p.wait_for(20.0, "the pellet in the receptacle (the lift to come down)", |p| p.sim.movers[lift].goal.is_some() || p.sim.movers[lift].node != 0);
    p.wait_for(10.0, "the lift to come down", |p| p.sim.movers[lift].goal.is_none());
    let deck = p.sim.movers[lift].boxes().fold(f32::MIN, |m, b| m.max(b.max.y));
    p.go(Vec3::new(58.5, deck, 8.9), 0.4);
    p.wait_for(10.0, "the lift to take her up", |p| p.sim.movers[lift].goal.is_some());
    p.wait_for(15.0, "the lift to stop", |p| p.sim.movers[lift].goal.is_none());
    eprintln!("up the lift at {}", p.c.feet);
    p.assert_alive("up to section 3");
    // Section 3: over the glass (x 23) by a fling. Orange on the white floor by the glass, blue
    // high on the wall over it: in, and down out of blue into orange again (shot where she'll
    // land, should she miss it); falling the second time, blue onto the panels high over the
    // hallway's end, facing the glass: she comes out of them fast, and over.
    let g = p.sim.level.gravity.unwrap_or(crate::FAITH_GRAVITY);
    p.go(Vec3::new(25.6, -6.5, 12.0), 0.3);
    p.shoot(PortalColor::Blue, Vec3::new(25.6, 4.68, 16.26));
    // (Orange under her own feet: she drops in from standing, and out of blue slowly.)
    p.go(Vec3::new(25.6, -6.5, 14.4), 0.2);
    p.c.pitch = -1.5;
    p.wait(0.2);
    assert!(p.sim.fire(PortalColor::Orange, Aim::of(&p.c)), "orange didn't open on the floor by the glass");
    let land_at = |p: &Play| {
        let (v, y) = (p.c.vel, p.c.feet.y + 6.5);
        let t = (v.y + (v.y * v.y + 2.0 * g * y).sqrt()) / g;
        Vec3::new(p.c.feet.x + v.x * t, -6.5, p.c.feet.z + v.z * t)
    };
    let (mut passes, mut fixed, mut panels) = (0, false, false);
    let end = p.time + 10.0;
    while p.time < end && !(passes >= 2 && p.c.state == State::Ground) {
        let seen = p.events.len();
        if passes == 0 {
            p.c.yaw = std::f32::consts::PI;
        }
        // Coming down from blue the first time: orange where she'll land (if it isn't).
        if passes == 1 && !fixed && p.c.vel.y < -1.0 && p.sim.player_env.is_none() {
            let land = land_at(&p);
            let o = p.sim.portals[1].unwrap();
            if Vec2::new(land.x - o.center.x, land.z - o.center.z).length() > 0.5 {
                let eye = p.c.view().eye;
                p.sim.fire(PortalColor::Orange, Aim { eye, dir: (land - eye).normalize(), yaw: p.c.yaw });
            }
            fixed = true;
        }
        // Falling into orange the second time: blue onto the panels.
        if passes == 1 && fixed && !panels && p.c.feet.y < 1.0 {
            let target = Vec3::new(42.27, 4.0, p.c.feet.z);
            let eye = p.c.view().eye;
            p.c.yaw = (-(target.x - eye.x)).atan2(-(target.z - eye.z));
            panels = p.sim.fire(PortalColor::Blue, Aim { eye, dir: (target - eye).normalize(), yaw: p.c.yaw });
        }
        p.step(&Input::default());
        passes += p.events[seen..].iter().filter(|e| matches!(e, SimEvent::Teleported { .. })).count();
    }
    assert!(passes >= 2 && p.c.feet.x < 22.0, "short of the glass in section 3 (at {})", p.c.feet);
    p.assert_alive("flung over the glass in section 3");
    // Section 4's first pellet: its launcher fires south, over a partition with a grill above
    // it, to the far wall, and its catcher's on the wall behind the launcher. Over the
    // partition (she climbs it). At the far end it turns off an angled panel west, to the side
    // wall: orange there, blue on the other side wall; out of blue, it comes back west into the
    // other angled panel, which turns it north, over the partition and through the grill, into
    // the catcher. (It starts the scaffolds.)
    p.go(Vec3::new(12.0, -6.5, 3.0), 0.3);
    p.c.yaw = 0.0;
    let end = p.time + 5.0;
    while p.time < end && !(p.c.feet.z < -0.8 && p.c.state == State::Ground) {
        let near = p.c.feet.z < 1.4 && p.c.state == State::Ground;
        p.step(&Input { move_axis: Vec2::new(0.0, 1.0), jump_pressed: near, jump_held: true, ..Default::default() });
    }
    assert!(p.c.feet.z < -0.8, "didn't climb the partition (at {})", p.c.feet);
    p.safe_shot(PortalColor::Orange, Vec3::new(9.76, -1.7, -14.52), Vec3::new(16.0, -6.5, -9.0), 5.0);
    p.safe_shot(PortalColor::Blue, Vec3::new(22.75, -1.7, -14.52), Vec3::new(14.0, -6.5, -9.0), 5.0);
    let trap = |p: &Play, name: &str| p.sim.io.as_ref().unwrap().ents.iter().any(|e| e.name == name && e.alive && matches!(e.kind, crate::io::Kind::Trigger { enabled: true, .. }));
    p.wait_for(30.0, "the first catcher to take the pellet", |p| !trap(p, "ball_trap_11"));
    // (Back over the partition.)
    p.go(Vec3::new(12.0, -6.5, -3.0), 0.3);
    p.c.yaw = std::f32::consts::PI;
    let end = p.time + 5.0;
    while p.time < end && !(p.c.feet.z > 0.8 && p.c.state == State::Ground) {
        let near = p.c.feet.z > -1.4 && p.c.state == State::Ground;
        p.step(&Input { move_axis: Vec2::new(0.0, 1.0), jump_pressed: near, jump_held: true, ..Default::default() });
    }
    // Across the goo on the scaffold, to the chamber's west end. (One scaffold at a time here,
    // round and round.) Orange on the room's wall, blue at the corridor's end: through, onto the
    // scaffold as it comes round the corner there; from it, blue on the wall at the corridor's
    // far bend; back to the room on it. Round again: through orange, out of blue onto the
    // scaffold at the bend; orange on the far wall of the chamber's west end, down the corridor;
    // back through blue, and out there.
    let carts: Vec<usize> = p.sim.io.as_ref().unwrap().ents.iter().filter_map(|e| match e.kind { crate::io::Kind::Mover { index, .. } if e.name == "tractrain_brush_2" || e.name == "spawned_rail_cart_intro_lvl7b" => Some(index), _ => None }).collect();
    let cart = move |p: &Play| carts.iter().filter(|&&m| p.sim.movers[m].present).find_map(|&m| p.sim.movers[m].boxes().fold(None::<faith_move::Aabb>, |a, b| Some(a.map_or(b, |a| a.union(&b)))).map(|b| b.center()));
    p.go(Vec3::new(10.6, -6.5, 8.1), 0.3);
    p.shoot(PortalColor::Blue, Vec3::new(-3.25, -5.0, 8.4));
    let corner = p.sim.portals[0].unwrap().center;
    p.shoot(PortalColor::Orange, Vec3::new(9.75, -5.0, 4.32));
    let room = p.sim.portals[1].unwrap().center;
    // (A run-up in front of orange: out of blue fast enough to land well out on the scaffold.)
    let in_front = Vec3::new(room.x + 2.5, -6.5, room.z);
    p.entry_speed = 1.0;
    p.go(in_front, 0.15);
    p.wait_for(60.0, "the scaffold coming round the corridor's corner", |p| cart(p).is_some_and(|c| (c.x + 1.63).abs() < 0.3 && c.z > 5.6 && c.z < 6.6));
    p.enter_now(PortalColor::Orange, room, Some(corner));
    p.wait(0.3);
    assert!(p.c.feet.y > -6.6, "didn't land on the scaffold (at {})", p.c.feet);
    p.look_at(Vec3::new(-1.63, -5.0, -3.25));
    assert!(p.sim.fire(PortalColor::Blue, Aim::of(&p.c)), "blue didn't open on the wall at the corridor's far bend");
    let bend = p.sim.portals[0].unwrap().center;
    // (Off into the room as the scaffold comes in.)
    p.wait_for(15.0, "the scaffold into the room's end", |p| cart(p).is_some_and(|c| c.x > 7.8));
    let end = p.time + 3.0;
    while p.time < end && p.c.feet.x < 10.5 {
        // (Through the middle of the doorway.)
        let to = Vec3::new(11.0, 0.0, 8.1) - p.c.feet;
        p.c.yaw = (-to.x).atan2(-to.z);
        p.step(&Input { move_axis: Vec2::new(0.0, 0.6), ..Default::default() });
    }
    p.assert_alive("off the scaffold");
    // Round again: in front of orange, and in as the scaffold comes to the bend.
    p.go(in_front, 0.15);
    p.wait_for(60.0, "the scaffold coming to the corridor's bend", |p| cart(p).is_some_and(|c| (c.z + 1.63).abs() < 0.3 && c.x > -2.6 && c.x < -1.7));
    p.enter_now(PortalColor::Orange, room, Some(bend));
    assert!(p.c.feet.y > -6.8 && p.c.feet.z > -3.0, "didn't land on the scaffold at the bend (at {})", p.c.feet);
    p.look_at(Vec3::new(-31.5, -5.0, -1.63));
    assert!(p.sim.fire(PortalColor::Orange, Aim::of(&p.c)), "orange didn't open on the west end's far wall");
    // (Back off the scaffold's end and into blue, a jump across the goo opening behind it.)
    let seen = p.events.len();
    let end = p.time + 3.0;
    while p.time < end && !p.events[seen..].iter().any(|e| matches!(e, SimEvent::Teleported { .. })) {
        let to = bend - p.c.feet;
        p.c.yaw = (-to.x).atan2(-to.z);
        let edge = p.c.state == State::Ground && cart(&p).is_some_and(|c| p.c.feet.z < c.z - 1.2);
        p.step(&Input { move_axis: Vec2::new(0.0, 1.0), jump_pressed: edge, jump_held: true, ..Default::default() });
    }
    p.wait(1.0);
    assert!(p.c.feet.x < -12.0 && p.c.feet.y > -7.0, "not in the west end (at {})", p.c.feet);
    p.assert_alive("over to the west end");
    let lift_a = p.mover("lift_pillar_door_top_a");
    p.wait_for(40.0, "lift a down", |p| p.sim.movers[lift_a].goal.is_none() && p.sim.movers[lift_a].node == 1);
    let top = p.sim.movers[lift_a].boxes().fold(f32::MIN, |m, b| m.max(b.max.y));
    p.go(Vec3::new(-17.88, top, 3.25), 0.4);
    p.wait(20.0);
    // The two switches, each in a room up on a lift, each opening one of the two doors in front
    // of the last catcher for a while: blue on this room's wall, orange on the other's; the
    // switch here, through, the switch there. Then, the doors open, orange on the wall facing
    // the catcher and blue on the floor under the launcher: its pellet falls into blue, out of
    // orange, through the doors and into the catcher.
    p.shoot(PortalColor::Blue, Vec3::new(-20.37, 1.43, 9.75));
    let here = p.sim.portals[0].unwrap().center;
    p.shoot(PortalColor::Orange, Vec3::new(-31.56, 1.66, -13.0));
    let there = p.sim.portals[1].unwrap().center;
    p.press("button_catcher_door01_rm8");
    p.enter_now(PortalColor::Blue, here, Some(there));
    p.press("button_catcher_door02_rm8");
    let feet = p.c.feet;
    p.safe_shot(PortalColor::Orange, Vec3::new(-13.82, 1.62, -1.67), feet, 4.0);
    let feet = p.c.feet;
    p.safe_shot(PortalColor::Blue, Vec3::new(-21.94, -6.5, -1.55), feet, 7.0);
    let trap = |p: &Play, name: &str| p.sim.io.as_ref().unwrap().ents.iter().any(|e| e.name == name && e.alive && matches!(e.kind, crate::io::Kind::Trigger { enabled: true, .. }));
    p.wait_for(15.0, "the last catcher to take the pellet", |p| !trap(p, "ball_trap_10"));
    p.assert_alive("catching the last pellet");
    // The victory lift comes down: on it, up to the chamberlock and the exit elevator.
    let victory = p.mover("lift_pillar_door_top_a1");
    p.wait_for(40.0, "the victory lift down", |p| p.sim.movers[victory].goal.is_none() && p.sim.movers[victory].node == 1);
    let deck = p.sim.movers[victory].boxes().fold(f32::MIN, |m, b| m.max(b.max.y));
    p.go(Vec3::new(-30.89, deck, 4.88), 0.4);
    p.wait_for(40.0, "the victory lift up", |p| p.sim.movers[victory].goal.is_none() && p.sim.movers[victory].node == 0);
    p.assert_alive("in Chamber 15");
    p.ride_exit("testchmb_a_11");
    p.assert_alive("catching section 4's first pellet");
}

/// Chamber 16 (testchmb_a_11), the turrets' chamber, much as Portal's walkthrough has it: each
/// turret knocked over from behind or beside it, out of its sight (by a portal behind it where
/// there's no way round), or with a cube dropped on it through a portal in the ceiling over it,
/// or dropped itself through a portal in the floor under it; a cube on the button, and out.
#[test]
fn solve_testchmb_a_11() {
    let Some(mut p) = Play::load("testchmb_a_11") else { return };
    p.wait(3.0);
    // Down the entry hall: the doors open once GLaDOS has had her say (18 s). Turret 0, from
    // behind.
    p.go(Vec3::new(24.6, 0.0, -13.0), 0.5);
    let door = p.mover("exit_door_left_a_a11");
    p.wait_for(30.0, "the entry doors", |p| p.sim.movers[door].node == 1 && p.sim.movers[door].goal.is_none());
    let t0 = p.sim.turrets[0];
    p.topple(0, -t0.facing());
    // Turret 6 looks down the corridor north: from its mouth, a quick look and blue on the
    // wall behind it, then orange out of its sight, and through behind it.
    p.go(Vec3::new(13.8, 0.0, 2.0), 0.3);
    p.peek_shot(PortalColor::Blue, Vec3::new(10.62, 1.5, 14.1), Vec3::NEG_X);
    let behind = p.sim.portals[0].unwrap().center;
    p.go(Vec3::new(14.5, 0.0, 1.5), 0.4);
    p.shoot(PortalColor::Orange, Vec3::new(14.14, 1.42, -0.81));
    let wall = p.sim.portals[1].unwrap().center;
    p.enter_now(PortalColor::Orange, wall, Some(behind));
    let t6 = p.sim.turrets[6];
    p.topple(6, -t6.facing());
    // Turret 9 at the top of the stairs, from beside it (so it falls out of the way down).
    p.topple(9, Vec3::X);
    // Turret 1 looks east along the west room's south side: in along its north wall (in its
    // sight only a moment), and behind it.
    p.go(Vec3::new(-10.1, 0.0, 9.0), 0.5);
    p.go(Vec3::new(-20.6, 0.0, 7.5), 0.5);
    p.topple(1, Vec3::NEG_X);
    // Turret 5 looks west along the strip to the cubes' pile: blue on the ceiling over it (a
    // quick look round the corner), orange on the floor here, and a cube from this room dropped
    // in onto it.
    p.go(Vec3::new(-11.0, 0.0, 4.0), 0.4);
    p.peek_shot(PortalColor::Blue, Vec3::new(0.94, 6.45, 0.84), Vec3::NEG_Z);
    p.shoot(PortalColor::Orange, Vec3::new(-13.0, 0.0, 5.5));
    let hole = p.sim.portals[1].unwrap().center;
    p.pick_up(Vec3::new(-17.96, 1.35, 9.17));
    p.drop_held_at(hole);
    p.wait_for(5.0, "the cube to knock turret 5 over", |p| !p.sim.turrets[5].upright());
    p.assert_alive("dropping a cube on turret 5");
    // Turrets 8 and 7 watch the way south to the button: the same for each, orange now on the
    // strip, with the other cube from this room, then one from the pile.
    p.safe_shot(PortalColor::Blue, Vec3::new(-8.53, 6.45, -6.10), Vec3::new(-6.0, 0.0, 0.5), 5.0);
    p.go(Vec3::new(-9.0, 0.0, 2.0), 0.4);
    p.shoot(PortalColor::Orange, Vec3::new(-11.5, 0.0, 1.2));
    let hole = p.sim.portals[1].unwrap().center;
    p.pick_up(Vec3::new(-17.98, 0.45, 10.0));
    p.drop_held_at(hole);
    p.wait_for(5.0, "the cube to knock turret 8 over", |p| !p.sim.turrets[8].upright());
    p.assert_alive("dropping a cube on turret 8");
    p.safe_shot(PortalColor::Blue, Vec3::new(-8.53, 6.45, -10.16), Vec3::new(-5.0, 0.0, -3.0), 5.0);
    p.pick_up(Vec3::new(2.44, 1.35, -3.62));
    p.drop_held_at(hole);
    p.wait_for(5.0, "the cube to knock turret 7 over", |p| !p.sim.turrets[7].upright());
    p.assert_alive("dropping a cube on turret 7");
    // Turrets 4, 3 and 2 watch the button, and 3 and 2 each other: blue on the floor under
    // each of 4 and 3 (from where none of them sees her), and it drops through, out of orange
    // on the strip.
    let under = |p: &mut Play, i: usize, near: Vec3, r: f32| {
        let t = p.sim.turrets[i].at;
        // (Anywhere on the floor round its foot: the portal opens under it all the same.)
        let shot = [(0.0f32, 0.0f32), (0.0, -0.4), (0.4, 0.0), (-0.4, 0.0), (0.0, 0.4)].into_iter().any(|(dx, dz)| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| p.safe_shot(PortalColor::Blue, Vec3::new(t.x + dx, 0.0, t.z + dz), near, r))).is_ok()
        });
        assert!(shot, "{}: no portal under turret {i}", p.name);
        p.wait_for(3.0, &format!("turret {i} to drop through"), |p| !p.sim.turrets[i].upright());
        p.assert_alive(&format!("dropping turret {i} through"));
    };
    under(&mut p, 4, Vec3::new(-2.0, 0.0, -16.0), 6.0);
    // (3's floor from the west end of the corridor, round the button room's corner.)
    under(&mut p, 3, Vec3::new(-3.0, 0.0, -21.0), 2.0);
    // 2 stands on floor that takes no portals: blue on the wall behind it, and through.
    p.safe_shot(PortalColor::Blue, Vec3::new(13.0, 1.42, -20.3), Vec3::new(2.0, 0.0, -19.0), 10.0);
    p.go(hole + Vec3::new(0.0, 0.0, 2.2), 0.4);
    p.through(PortalColor::Orange, 0.5);
    p.topple(2, p.c.feet - p.sim.turrets[2].at);
    // A cube from the pile onto the button opens the exit doors.
    p.pick_up(Vec3::new(4.08, 1.35, -4.10));
    // (Wide round the corners, looking down so it rides low and close: she carries it a couple
    // of metres out in front.)
    for w in [Vec3::new(3.0, 0.0, -7.6), Vec3::new(1.0, 0.0, -8.9), Vec3::new(-1.1, 0.0, -10.0), Vec3::new(-1.1, 0.0, -16.6), Vec3::new(6.5, 0.0, -16.5)] {
        let eye = p.c.view().eye;
        p.look_at(Vec3::new(w.x, eye.y, w.z));
        p.c.pitch = -0.8;
        p.wait(0.5);
        p.go(w, 0.5);
    }
    p.c.pitch = 0.0;
    p.cube_on_button(Vec3::new(6.5, 0.2, -19.52));
    // Turret 10 beyond looks through the doors: out of its sight, blue on the wall behind it,
    // and through.
    p.go(Vec3::new(2.0, 0.0, -19.0), 0.6);
    let exit = p.mover("exit_door_left_a11");
    p.wait_for(5.0, "the exit doors to open", |p| p.sim.movers[exit].node == 1 && p.sim.movers[exit].goal.is_none());
    p.assert_alive("onto the button");
    let t10 = p.sim.turrets[10].at;
    let wall = p.sim.raycast(t10 + Vec3::Y * 1.2, Vec3::NEG_Z, 6.0, crate::sim::Mask::SHOT).map(|h| h.point).expect("no wall behind turret 10");
    p.safe_shot(PortalColor::Blue, Vec3::new(wall.x, 1.42, wall.z), Vec3::new(8.9, 0.0, -20.0), 6.0);
    p.go(hole + Vec3::new(0.0, 0.0, 2.2), 0.4);
    p.through(PortalColor::Orange, 0.5);
    p.topple(10, p.c.feet - p.sim.turrets[10].at);
    // Turret 10's room is shut in; turret 11, next door, looks away from it: blue on the floor
    // behind 11, orange on this room's floor, and down through.
    p.go(Vec3::new(9.0, 0.0, -30.0), 0.5);
    p.shoot(PortalColor::Blue, Vec3::new(12.94, 0.0, -31.2));
    p.shoot(PortalColor::Orange, Vec3::new(8.75, 0.0, -28.6));
    p.through(PortalColor::Orange, 0.5);
    // (Blue's opening reaches under 11 too: it may have dropped through already.)
    if p.sim.turrets[11].upright() {
        p.topple(11, Vec3::NEG_X);
    }
    p.assert_alive("past turret 11");
    p.ride_exit("testchmb_a_13");
}

/// Chamber 17 (testchmb_a_13), the companion cube's chamber, as Portal's walkthrough has it:
/// up the ledges with the cube (she vaults them; from below, it's held up over each and let go
/// onto it, and she climbs after it), orange where the first hallway's pellet bounces, through
/// the pellet hallways behind the cube, the first catcher's pellet bounced off it, the first
/// hallway's pellet down out of blue on an angled panel into the second catcher, and out of
/// blue on the wall facing the doors into the third (the cube on one door's button, herself on
/// the other's once it's past); up by the portals onto the ledge with the cube, across the
/// lifts, the cube on the button, the switch, the cube into the incinerator, and out.
#[test]
fn solve_testchmb_a_13() {
    let Some(mut p) = Play::load("testchmb_a_13") else { return };
    // Out of the elevator: GLaDOS's welcome, and the cube drops as she finishes.
    p.wait(3.0);
    p.go(Vec3::new(-3.25, 0.0, -64.5), 0.5);
    let cube = p.pick_up(Vec3::new(-4.92, 0.45, -52.06));
    let set_up = |p: &mut Play, stand: Vec3| {
        p.go(stand, 0.25);
        p.c.yaw = std::f32::consts::FRAC_PI_2;
        p.c.pitch = 0.35;
        p.wait(0.8);
        p.sim.use_key(Aim::of(&p.c));
        p.c.pitch = 0.0;
        p.wait(1.0);
    };
    set_up(&mut p, Vec3::new(-11.8, 0.0, -44.4));
    p.go(Vec3::new(-13.4, 1.63, -44.4), 0.3);
    p.pick_up(p.sim.props[cube].pos);
    set_up(&mut p, Vec3::new(-13.2, 1.63, -44.4));
    assert!(p.sim.props[cube].pos.y > 3.25, "the cube isn't up top: {}", p.sim.props[cube].pos);
    p.go(Vec3::new(-15.5, 3.25, -44.0), 0.5);
    // Orange on the first hallway's end wall, where its pellet bounces: it brings the second
    // and third catchers their pellets (its launcher runs till the last one's caught). From
    // just inside the hallway, while its pellet's far off down it and going away.
    p.wait_for(30.0, "the first hallway's pellet far off", |p| p.sim.pellets.iter().any(|q| q.alive && (q.pos.z + 40.61).abs() < 0.5 && q.pos.x > 0.0 && q.vel.x > 0.0));
    p.safe_shot(PortalColor::Orange, Vec3::new(-16.26, 4.37, -40.61), Vec3::new(-13.5, 3.25, -42.6), 1.6);
    p.go(Vec3::new(-15.5, 3.25, -44.0), 0.5);
    p.pick_up(p.sim.props[cube].pos);
    p.assert_alive("up the ledges with the cube");
    // The pellet hallways, the cube held low in front (a pellet bounces off it): along the
    // first to its far end, up and back along the second, down the ramp and through the door
    // to the ledge over the lifts' room, and down into it.
    p.c.pitch = -0.2;
    p.wait(0.5);
    // (Along each pellet's line: it meets the cube, not her.)
    for w in [Vec3::new(-12.0, 3.25, -40.61), Vec3::new(15.0, 3.25, -40.61), Vec3::new(17.9, 2.2, -40.4), Vec3::new(18.4, 1.9, -38.6), Vec3::new(11.4, 4.9, -38.6), Vec3::new(10.1, 4.88, -37.62), Vec3::new(-9.0, 4.88, -37.62), Vec3::new(-14.9, 2.9, -37.1)] {
        p.go(w, 0.4);
        assert_eq!(p.sim.held, Some(cube), "lost the cube on the way to {w} (at {})", p.c.feet);
    }
    // Down the ramp at the second hallway's west end, under its pellet's way.
    for w in [Vec3::new(-12.6, 0.0, -27.9), Vec3::new(-11.9, -6.5, -25.4)] {
        p.go(w, 0.4);
        assert_eq!(p.sim.held, Some(cube), "lost the cube on the way to {w} (at {})", p.c.feet);
    }
    p.assert_alive("through the pellet hallways");
    // The first catcher: in launcher 3's room (in straight through its doorway, south of the
    // pellet's line), the cube held turned 45° in the pellet's way; it glances off into the
    // catcher on the wall beside, and the first lift rises.
    p.go(Vec3::new(2.75, -6.5, -26.0), 0.3);
    p.go(Vec3::new(2.74, -6.5, -19.6), 0.3);
    p.c.yaw = -3.0 * std::f32::consts::FRAC_PI_4;
    p.c.pitch = -0.1;
    p.wait(0.6);
    let want = Vec3::new(4.15, 0.0, -17.85);
    for _ in 0..5 {
        let at = p.sim.props[cube].pos;
        let off = Vec2::new(at.x - want.x, at.z - want.z);
        if off.length() < 0.1 {
            break;
        }
        let spot = p.c.feet - Vec3::new(off.x, 0.0, off.y);
        p.nudge(spot);
        p.wait(0.4);
    }
    let lift1 = p.mover("lift1_rm5");
    p.wait_for(10.0, "the first catcher to take the pellet (the first lift to rise)", |p| p.sim.movers[lift1].goal.is_some());
    p.assert_alive("catching the first pellet");
    assert_eq!(p.sim.held, Some(cube));
    // The second catcher sits in an angled panel, facing up and out: blue on the angled panel
    // across from it. The first hallway's pellet comes out of it, up off the ceiling and down
    // into the catcher.
    p.sim.use_key(Aim::of(&p.c));
    p.go(Vec3::new(-6.0, -6.5, -20.0), 0.2);
    // (Where on the panel matters: high enough that the pellet comes down into the catcher's
    // notch, in line with it. The shot nearest that, aiming round about.)
    let eye = p.c.view().eye;
    let want = |c: Vec3| (c.x + c.y + 3.0).abs() + (c.z + 18.83).abs();
    let mut best: Option<(f32, f32, f32)> = None;
    for i in -20..=20 {
        for j in -20..=20 {
            let (yaw, pitch) = (-1.8432 + i as f32 * 0.005, 0.62 + j as f32 * 0.005);
            let (saved, on) = (p.sim.portals, p.sim.portal_mover);
            let ok = p.sim.fire(PortalColor::Blue, Aim { eye, dir: crate::portal::view_dir(yaw, pitch), yaw });
            let got = p.sim.portals[0];
            (p.sim.portals, p.sim.portal_mover) = (saved, on);
            if let Some(g) = got.filter(|g| ok && g.normal.x < -0.5 && g.normal.y > 0.5) {
                if best.is_none_or(|(s, ..)| want(g.center) < s) {
                    best = Some((want(g.center), yaw, pitch));
                }
            }
        }
    }
    let (score, yaw, pitch) = best.expect("no shot opens blue on the angled panel");
    assert!(score < 0.4, "blue can't go where the pellet would reach the catcher (off by {score})");
    p.c.yaw = yaw;
    p.c.pitch = pitch;
    assert!(p.sim.fire(PortalColor::Blue, Aim::of(&p.c)), "blue didn't open on the angled panel");
    let trap = |p: &Play, name: &str| p.sim.io.as_ref().unwrap().ents.iter().any(|e| e.name == name && e.alive && matches!(e.kind, crate::io::Kind::Trigger { enabled: true, .. }));
    p.wait_for(20.0, "the second catcher to take the pellet", |p| !trap(p, "ball_trap_12"));
    p.assert_alive("catching the second pellet");
    // The third catcher, behind two doors, each opened by a button: the cube on the one on the
    // ledge (the far door), and herself on the one in the corridor (the near door) once the
    // pellet's gone past her. The first hallway's pellet, out of blue on the wall facing the
    // doors.
    let b_button = Vec3::new(-1.75, -4.88, 3.24);
    // (Up onto the ledge's button from the floor below it: the cube held up over the ledge's
    // edge, lined up over the button, and let go.)
    p.pick_up(p.sim.props[cube].pos);
    p.go(Vec3::new(b_button.x, -6.5, 0.9), 0.3);
    p.c.yaw = std::f32::consts::PI;
    p.c.pitch = 0.35;
    p.wait(0.8);
    for _ in 0..6 {
        let at = p.sim.props[cube].pos;
        let off = Vec2::new(at.x - b_button.x, at.z - b_button.z);
        if off.length() < 0.12 {
            break;
        }
        let spot = p.c.feet - Vec3::new(off.x, 0.0, off.y);
        p.nudge(spot);
        p.wait(0.4);
    }
    p.sim.use_key(Aim::of(&p.c));
    p.c.pitch = 0.0;
    p.wait(1.5);
    let b = p.sim.buttons.iter().position(|x| x.at.distance(b_button) < 1.0).expect("the ledge's button");
    assert!(p.sim.buttons[b].pressed, "the cube's not on the ledge's button (it's at {})", p.sim.props[cube].pos);
    p.assert_alive("the cube on the ledge's button");
    // (Blue from off to the side: out of it, the pellet goes straight up the corridor.)
    p.go(Vec3::new(-8.6, -6.5, -23.0), 0.4);
    p.shoot(PortalColor::Blue, Vec3::new(-6.3, -5.03, -29.87));
    let wall = p.sim.portals[0].unwrap().center;
    let line_x = wall.x;
    // Beside the corridor's button, out of the pellet's way; on it once the pellet's past.
    let a_button = p.sim.buttons.iter().position(|x| x.at.distance(Vec3::new(-6.43, -6.5, -11.47)) < 1.0).expect("the corridor's button");
    let a_at = p.sim.buttons[a_button].at;
    p.go(Vec3::new(a_at.x + 1.3, -6.5, a_at.z - 0.5), 0.2);
    let feet_z = p.c.feet.z;
    p.wait_for(40.0, "the pellet past her up the corridor", |p| p.sim.pellets.iter().any(|q| q.alive && (q.pos.x - line_x).abs() < 0.6 && q.pos.z > feet_z + 0.8 && q.vel.z > 0.0));
    p.nudge(Vec3::new(a_at.x, -6.5, a_at.z));
    let trap = |p: &Play, name: &str| p.sim.io.as_ref().unwrap().ents.iter().any(|e| e.name == name && e.alive && matches!(e.kind, crate::io::Kind::Trigger { enabled: true, .. }));
    p.wait_for(40.0, "the third catcher to take the pellet", |p| !trap(p, "ball_trap_11"));
    p.assert_alive("catching the third pellet");
    // The lifts up: orange on the wall over the upper ledge (in sight from the room), the
    // cube off the ledge's button, in by blue (still on the wall facing the doors) with it,
    // and out onto the ledge; across the three lifts as they've risen.
    let lift3 = p.mover("lift3_rm5");
    p.wait_for(20.0, "the lifts to rise", |p| p.sim.movers[lift3].goal.is_none());
    p.safe_shot(PortalColor::Orange, Vec3::new(-8.41, 4.63, -35.76), Vec3::new(-6.0, -6.5, -20.0), 3.0);
    let over_ledge = p.sim.portals[1].unwrap().center;
    p.go(Vec3::new(b_button.x, b_button.y, b_button.z - 0.9), 0.4);
    p.pick_up(p.sim.props[cube].pos);
    p.enter_now(PortalColor::Blue, wall, Some(over_ledge));
    for w in [Vec3::new(-11.25, 0.0, -29.0), Vec3::new(-11.4, -0.2, -24.4), Vec3::new(-17.0, -0.2, -24.4), Vec3::new(-22.7, -0.4, -24.4)] {
        p.go(w, 0.4);
        assert_eq!(p.sim.held, Some(cube), "lost the cube on the way to {w} (at {})", p.c.feet);
    }
    // Off the third lift with a running jump, down onto the ledge across from it (south).
    p.go(Vec3::new(-22.7, -0.4, -23.0), 0.2);
    p.c.yaw = 0.0;
    let end = p.time + 4.0;
    while p.time < end && !(p.c.state == State::Ground && p.c.feet.y < -3.0) {
        let take_off = p.c.state == State::Ground && p.c.feet.z < -25.6;
        p.step(&Input { move_axis: Vec2::new(0.0, 1.0), jump_pressed: take_off, jump_held: take_off || p.c.state != State::Ground, ..Default::default() });
    }
    assert_eq!(p.sim.held, Some(cube), "lost the cube jumping off the third lift (at {})", p.c.feet);
    p.assert_alive("off the lifts");
    // Up the ramp to the button: the cube on it opens the doors to the incinerator's switch.
    // The switch; the cube back off the button, and into the incinerator. Its going opens the
    // way out.
    // (The cube held low, round the ledge's corner into the narrow way along to the ramp.)
    p.c.pitch = -0.3;
    p.wait(0.3);
    for w in [Vec3::new(-22.7, -3.25, -37.9), Vec3::new(-17.5, -3.25, -37.9), Vec3::new(-9.0, 0.0, -37.9)] {
        p.go(w, 0.4);
        assert_eq!(p.sim.held, Some(cube), "lost the cube on the way to {w} (at {})", p.c.feet);
    }
    p.cube_on_button(Vec3::new(-7.04, 0.0, -39.92));
    let doors = p.mover("switch_door_left_a11");
    p.wait_for(5.0, "the switch's doors", |p| p.sim.movers[doors].goal.is_none() && p.sim.movers[doors].node == 1);
    p.press("button_incineratordoor_a13");
    let hatch = p.mover("incinerator_door_1");
    p.wait_for(5.0, "the incinerator to open", |p| p.sim.movers[hatch].node == 1);
    p.pick_up(p.sim.props[cube].pos);
    // (From the hall in front of it: the incinerator's mouth is in its north wall.)
    p.go(Vec3::new(4.88, -1.6, -38.0), 0.4);
    p.drop_held_at(Vec3::new(4.88, -1.6, -35.2));
    p.wait_for(5.0, "the cube in the incinerator", |p| !p.sim.props[cube].alive);
    p.assert_alive("in Chamber 17");
    p.ride_exit("testchmb_a_14");
}

/// Chamber 00's clock runs its hundredths and thousandths (#48), not just its seconds.
impl Play {
    /// A running jump through a portal, catching a ledge out of the far one: run along `yaw`
    /// and jump once `take_off` (her feet); out of the far portal she faces `face`, and once
    /// she's below `push_below` (her feet) pushes forward, to catch the ledge she's falling past
    /// and pull herself up onto it. Until she's standing again.
    pub fn leap(&mut self, yaw: f32, take_off: impl Fn(Vec3) -> bool, face: f32, push_below: f32) {
        self.portal_ride(Some((yaw, &take_off)), face, push_below);
    }

    /// A running jump along `yaw` from where she stands, taking off once `take_off` (her
    /// feet), catching the ledge she's jumping for if she comes up short and pulling herself
    /// up. Until she's standing again.
    pub fn run_jump(&mut self, yaw: f32, take_off: impl Fn(Vec3) -> bool) {
        let end = self.time + 6.0;
        let mut flying = false;
        loop {
            self.c.yaw = yaw;
            self.c.pitch = 0.0;
            let hanging = matches!(self.c.state, State::LedgeHang { .. });
            let jump = (!flying && self.c.state == State::Ground && take_off(self.c.feet)) || hanging;
            self.step(&Input { move_axis: Vec2::new(0.0, 1.0), jump_pressed: jump, jump_held: true, ..Default::default() });
            flying |= self.c.state != State::Ground;
            if flying && self.c.state == State::Ground {
                break;
            }
            assert!(self.time < end, "{}: the jump didn't end (at {}, {:?})", self.name, self.c.feet, self.c.state);
        }
        self.wait(0.5);
    }

    /// Down through a `color` portal opened under her feet, and out of the other one, as
    /// [`leap`](Self::leap) has it.
    pub fn drop_through(&mut self, color: PortalColor, face: f32, push_below: f32) {
        self.open_underfoot(color);
        self.portal_ride(None, face, push_below);
    }

    /// Open a `color` portal in the floor under her (she drops in at once).
    pub fn open_underfoot(&mut self, color: PortalColor) {
        self.c.pitch = -1.5;
        let eye = self.c.view().eye;
        let opened = self.sim.fire(color, Aim { eye, dir: crate::portal::view_dir(self.c.yaw, self.c.pitch), yaw: self.c.yaw });
        assert!(opened && self.sim.portals[color.index()].is_some_and(|q| q.center.distance(self.c.feet) < 1.0), "{}: no {color:?} portal under her at {} ({opened}, {:?})", self.name, self.c.feet, self.sim.portals[color.index()].map(|q| q.center));
    }

    fn portal_ride(&mut self, run: Option<(f32, &dyn Fn(Vec3) -> bool)>, face: f32, push_below: f32) {
        let mut through = false;
        let end = self.time + 8.0;
        loop {
            let seen = self.events.len();
            self.c.yaw = match run {
                Some((yaw, _)) if !through => yaw,
                _ if through => face,
                _ => self.c.yaw,
            };
            self.c.pitch = 0.0;
            let hanging = matches!(self.c.state, State::LedgeHang { .. });
            let take_off = run.is_some_and(|(_, t)| t(self.c.feet));
            let jump = (!through && self.c.state == State::Ground && take_off) || hanging;
            let push = if through && self.c.feet.y <= push_below || run.is_some() && !through { 1.0 } else { 0.0 };
            self.step(&Input { move_axis: Vec2::new(0.0, push), jump_pressed: jump, jump_held: !through && run.is_some(), ..Default::default() });
            through |= self.events[seen..].iter().any(|e| matches!(e, SimEvent::Teleported { .. }));
            if through && self.c.state == State::Ground {
                break;
            }
            assert!(self.time < end, "{}: the portal ride didn't end (at {}, {:?}; through: {through})", self.name, self.c.feet, self.c.state);
        }
        self.wait(1.0);
    }
}

/// Chamber 18 (testchmb_a_14). Faith's route keeps to Portal's where it lets her (as #79 asks),
/// and climbs where Portal's would drop her further than she can land:
/// - Over the goo: blue on the ceiling over the hallway's end, orange in its floor under her;
///   round the loop once, blue onto the wall over the hallway's mouth on the way down, and out
///   across fast.
/// - Up through the goo room: out of ceiling portals beside the platforms' edges, catching the
///   ledges on the way down (Portal's drops onto them are further than she survives); a floor
///   portal on D out over H, and off H's edge into C's floor, flung out of the panel high on the
///   north wall to the ledge with the switch.
/// - The turret room: the two turrets covering the scaffold's way knocked over by pellets sent
///   back down their lines of fire; a pellet out of the panel in line with the catcher's cage,
///   and the cage's switch pressed on the way past it out of a portal high on the wall.
/// - The scaffold to the far end, a running jump up onto the box platform, the cube back on the
///   scaffold and in front of the glass, blue above it; back to the far end, down behind the
///   switch there, the switch (the passage back opens) and through orange to the cube; the cube
///   onto the button at the first switch.
/// - Down onto D and out over H again, through the opened doors to the shaft, and in at the
///   corridor's wall, out of the shaft's ceiling onto the exit's floor.
#[test]
fn solve_testchmb_a_14() {
    use std::f32::consts::{FRAC_PI_2, PI};
    let Some(mut p) = Play::load("testchmb_a_14") else { return };
    p.wait(3.0);
    // Over the goo: blue on the ceiling over the hallway's end, orange in the floor under her.
    // Round the loop once (fast by the second time down), and blue onto the wall over the
    // hallway's mouth on the way: out of it across the goo.
    p.go(Vec3::new(4.9, 6.5, -21.0), 0.2);
    p.nudge(Vec3::new(4.9, 6.5, -21.0));
    p.shoot(PortalColor::Blue, Vec3::new(4.9, 26.01, -21.0));
    p.c.yaw = PI;
    p.open_underfoot(PortalColor::Orange);
    let (mut passes, end) = (0, p.time + 6.0);
    while !(passes == 2 && p.c.state == State::Ground) {
        assert!(p.time < end, "testchmb_a_14: didn't get across the goo (at {})", p.c.feet);
        let seen = p.events.len();
        p.step(&Input::default());
        passes += p.events[seen..].iter().filter(|e| matches!(e, SimEvent::Teleported { .. })).count();
        if passes == 1 && p.c.view().eye.y < 17.0 && p.sim.portals[0].is_some_and(|q| q.normal.y < -0.5) {
            p.shoot(PortalColor::Blue, Vec3::new(4.9, 13.0, -22.76));
        }
    }
    assert!(p.c.feet.z > -9.0, "testchmb_a_14: came down short of the far side (at {})", p.c.feet);
    p.assert_alive("over the goo");
    p.wait(2.0);
    // Up onto B, the platform over the goo channel west of here: blue on the ceiling over the
    // channel beside it, orange on the wall across the channel, and a running jump in; out of
    // blue she catches B's edge on the way down and pulls herself up.
    p.go(Vec3::new(1.5, 6.5, 0.0), 0.3);
    p.shoot(PortalColor::Blue, Vec3::new(-2.45, 32.51, 0.0));
    p.shoot(PortalColor::Orange, Vec3::new(-3.25, 8.13, 0.0));
    p.go(Vec3::new(6.0, 6.5, 0.0), 0.3);
    p.leap(FRAC_PI_2, |f| f.x < 0.6, FRAC_PI_2, 12.0);
    p.assert_alive("up onto the platform over the goo channel");
    // Blue on the ceiling high over D's north edge (seen from B's south edge), back down to the
    // far ledge, into orange again, and she catches D's edge.
    p.go(Vec3::new(-7.0, 13.0, -3.0), 0.3);
    p.shoot(PortalColor::Blue, Vec3::new(-8.0, 39.01, -15.45));
    p.go(Vec3::new(6.0, 6.5, 0.0), 0.3);
    p.leap(FRAC_PI_2, |f| f.x < 0.6, 0.0, 18.5);
    p.assert_alive("up onto D");
    // Over to H, high in the room's north end: blue on the wall beyond it, orange under her.
    p.go(Vec3::new(-12.0, 19.5, -18.0), 0.3);
    p.shoot(PortalColor::Blue, Vec3::new(-17.96, 31.23, -1.63));
    p.drop_through(PortalColor::Orange, 0.0, 0.0);
    p.assert_alive("over to H");
    // Up to the ledge over the south wall, with the switch: blue on the panel standing out high
    // on the north wall, orange on C below H's south edge; off the edge into it, and flung out
    // across the room.
    p.shoot(PortalColor::Blue, Vec3::new(-17.0, 37.2, -3.25));
    p.go(Vec3::new(-17.0, 26.01, -9.0), 0.3);
    p.nudge(Vec3::new(-17.0, 26.01, -9.8));
    p.shoot(PortalColor::Orange, Vec3::new(-17.0, 14.63, -12.8));
    p.c.yaw = 0.0;
    p.c.pitch = 0.0;
    let (mut through, end) = (false, p.time + 8.0);
    while !(through && p.c.state == State::Ground) {
        assert!(p.time < end, "testchmb_a_14: didn't get to the switch's ledge (at {})", p.c.feet);
        let seen = p.events.len();
        p.step(&Input { move_axis: Vec2::new(0.0, if through { 0.0 } else { 0.3 }), ..Default::default() });
        through |= p.events[seen..].iter().any(|e| matches!(e, SimEvent::Teleported { .. }));
    }
    assert!(p.c.feet.y > 29.0, "testchmb_a_14: didn't land on the switch's ledge (at {})", p.c.feet);
    p.assert_alive("up to the switch's ledge");
    p.wait(2.0);
    // The switch: the wedge swings out of the way into the turret room.
    p.press_at(Vec3::new(-22.35, 30.4, -28.85));
    p.wait(1.0);
    p.go(Vec3::new(-30.0, 29.26, -36.0), 0.4);
    // The two turrets covering the scaffold's way (on pillars to the west, facing east), each
    // knocked over by a pellet sent back down its line of fire: out of orange on the east wall,
    // in through blue where the launcher's next shot hits it. From the north-west corner, out
    // of their sight.
    p.go(Vec3::new(-36.1, 29.26, -37.1), 0.3);
    for (t, into, out) in [(1, Vec3::new(-6.5, 31.03, -47.14), Vec3::new(-6.5, 33.3, -43.05)), (3, Vec3::new(-14.63, 31.03, -65.02), Vec3::new(-6.5, 34.6, -51.21))] {
        p.shoot(PortalColor::Blue, into);
        p.shoot(PortalColor::Orange, out);
        p.wait_for(30.0, &format!("turret {t} knocked over"), |p| !p.sim.turrets[t].upright());
    }
    p.assert_alive("knocking the turrets over");
    // The catcher, in its cage in the north-west corner: a pellet in through blue where the
    // launcher's westward shot hits the west wall, out of orange on the panel at the room's far
    // end in line with the cage. Once it's on its way, the cage's switch, on its column: blue
    // high on the west wall facing it, orange under her, out past the switch and pressing it on
    // the way by; the cage opens as the pellet gets there.
    p.shoot(PortalColor::Blue, Vec3::new(-39.01, 31.03, -47.14));
    p.shoot(PortalColor::Orange, Vec3::new(-34.14, 34.14, -63.4));
    p.go(Vec3::new(-35.12, 29.26, -38.12), 0.2);
    p.wait_for(40.0, "a pellet on its way to the cage", |p| p.sim.pellets.iter().any(|q| q.alive && q.vel.z > 5.0 && (q.pos.x + 34.14).abs() < 0.6 && q.pos.z > -62.0 && q.pos.z < -60.0));
    p.wait(2.0);
    p.shoot(PortalColor::Blue, Vec3::new(-39.0, 35.2, -38.9));
    p.c.yaw = FRAC_PI_2;
    p.open_underfoot(PortalColor::Orange);
    let (mut pressed, end) = (false, p.time + 5.0);
    while !(pressed && p.c.state == State::Ground) {
        assert!(p.time < end, "testchmb_a_14: didn't press the cage's switch (at {})", p.c.feet);
        if !pressed && p.sim.player_env.is_none() && p.c.feet.y > 32.0 {
            p.look_at(Vec3::new(-36.98, 34.54, -38.0));
            p.sim.use_key(Aim::of(&p.c));
            pressed = p.sim.events.iter().any(|e| matches!(e, SimEvent::MoverUsed { .. }));
            // (And orange off the floor under her, onto the wall behind the switch.)
            if pressed {
                p.shoot(PortalColor::Orange, Vec3::new(-37.4, 30.9, -32.51));
            }
        }
        // (Pressed, she lets go of the switch's top, if she's caught hold of it.)
        let hanging = matches!(p.c.state, State::LedgeHang { .. });
        p.step(&Input { crouch_pressed: pressed && hanging, crouch_held: pressed && hanging, ..Default::default() });
    }
    p.wait_for(10.0, "the pellet caught", |p| p.events.iter().rev().take(400).any(|e| matches!(e, SimEvent::PelletGone { caught: true, .. })));
    p.assert_alive("at the catcher");
    // The scaffold, to the far end; a running jump up onto the box platform, for its cube.
    let cart = p.mover("func_tracktrain_b00");
    let at_end = |p: &Play, node: usize| p.sim.movers[cart].node == node && p.sim.movers[cart].goal.is_none();
    p.go(Vec3::new(-21.13, 29.26, -41.8), 0.3);
    p.wait_for(30.0, "the scaffold away", |p| p.sim.movers[cart].goal.is_some());
    p.wait_for(30.0, "the scaffold back", |p| at_end(p, 0));
    p.go(Vec3::new(-21.13, 29.26, -44.1), 0.4);
    p.wait_for(30.0, "the scaffold's far end", |p| at_end(p, 1));
    p.assert_alive("on the scaffold");
    p.go(Vec3::new(-21.13, 29.24, -57.9), 0.2);
    p.run_jump(0.0, |f| f.z < -60.6);
    let cube = p.pick_up(Vec3::new(-22.8, 31.34, -68.32));
    // Back onto the scaffold with it when it's next at this end, and to the near end: the
    // cube in front of the glass, and blue on the wall high above it.
    p.go(Vec3::new(-21.13, 30.9, -66.5), 0.2);
    p.wait_for(30.0, "the scaffold away", |p| p.sim.movers[cart].goal.is_some());
    p.wait_for(30.0, "the scaffold here", |p| at_end(p, 1));
    p.run_jump(PI, |f| f.z > -64.2);
    assert_eq!(p.sim.held, Some(cube), "testchmb_a_14: lost the cube jumping back onto the scaffold (at {})", p.c.feet);
    p.wait_for(30.0, "the scaffold's near end", |p| at_end(p, 0));
    p.go(Vec3::new(-21.13, 29.26, -36.0), 0.3);
    p.drop_held_at(Vec3::new(-23.4, 29.26, -34.0));
    let above_glass = Vec3::new(-21.13, 40.6, -32.51);
    p.go(Vec3::new(-21.13, 29.26, -38.0), 0.3);
    p.shoot(PortalColor::Blue, above_glass);
    // To the far end again, and off it down into the room under the box platform, behind the
    // switch there.
    p.go(Vec3::new(-21.13, 29.26, -41.8), 0.3);
    p.wait_for(30.0, "the scaffold away", |p| p.sim.movers[cart].goal.is_some());
    p.wait_for(30.0, "the scaffold back", |p| at_end(p, 0));
    p.go(Vec3::new(-21.13, 29.26, -44.1), 0.4);
    p.wait_for(30.0, "the scaffold's far end", |p| at_end(p, 1));
    p.go(Vec3::new(-21.13, 29.24, -57.9), 0.2);
    let end = p.time + 4.0;
    while !(p.c.state == State::Ground && p.c.feet.y < 24.0) {
        assert!(p.time < end, "testchmb_a_14: didn't get down behind the far switch (at {})", p.c.feet);
        p.c.yaw = 0.0;
        p.step(&Input { move_axis: Vec2::new(0.0, 1.0), ..Default::default() });
    }
    p.wait(2.0);
    p.assert_alive("down behind the far switch");
    // Orange on the wall beside her (its foot above her head: a jump in), the switch (the
    // passage back opens again), and through: out above the glass, down by the cube.
    let beside = Vec3::new(-17.88, 25.82, -66.66);
    p.go(Vec3::new(-21.5, 22.76, -66.6), 0.3);
    p.shoot(PortalColor::Orange, beside);
    p.press_at(Vec3::new(-23.98, 24.79, -63.8));
    p.go(Vec3::new(-23.0, 22.76, -66.66), 0.2);
    p.leap(-FRAC_PI_2, |f| f.x > -20.0, 0.0, -100.0);
    p.assert_alive("through to the glass");
    // The cube, through the passage, onto the button by the first switch: the exit's doors.
    p.pick_up(p.sim.props[cube].pos);
    p.cube_on_button(Vec3::new(-17.14, 29.38, -20.41));
    let doors = p.mover("exit_door_left_b00");
    p.wait_for(5.0, "the exit's doors", |p| p.sim.movers[doors].node == 1);
    // Down onto D (clear of the cube), and out over H by the open doors as before.
    p.go(Vec3::new(-18.5, 29.26, -22.2), 0.3);
    p.run_jump(-FRAC_PI_2, |f| f.x > -15.0);
    p.assert_alive("down onto D again");
    p.go(Vec3::new(-12.0, 19.5, -18.0), 0.3);
    p.shoot(PortalColor::Blue, Vec3::new(-17.96, 31.23, -1.63));
    p.drop_through(PortalColor::Orange, 0.0, 0.0);
    p.assert_alive("over to H again");
    // Through the doors, along the corridor to the shaft, and up it: blue on its ceiling over
    // the exit's floor, orange on the corridor's wall; in, and down onto the floor.
    p.go(Vec3::new(-21.0, 22.76, 27.0), 0.3);
    let top = Vec3::new(-1.79, 94.28, 27.6);
    p.shoot(PortalColor::Blue, top);
    let wall = Vec3::new(-22.76, 24.25, 23.05);
    p.shoot(PortalColor::Orange, wall);
    p.enter_now(PortalColor::Orange, wall, Some(top));
    p.wait_for(5.0, "her feet", |p| p.c.state == State::Ground);
    p.wait(2.0);
    assert!(p.c.feet.y > 84.0, "testchmb_a_14: didn't land on the exit's floor (at {})", p.c.feet);
    p.assert_alive("up the shaft");
    p.ride_exit("testchmb_a_15");
}

impl Play {
    /// Fire `color` from where she stands, aimed about `yaw` and `pitch` (searching two degrees
    /// round them, a tenth of one at a time): the first shot that opens one `fits` (one through
    /// a narrow gap: a vent's slats, a grate).
    pub fn shoot_near(&mut self, color: PortalColor, yaw: f32, pitch: f32, fits: impl Fn(&crate::portal::Portal) -> bool) {
        let eye = self.c.view().eye;
        // (Outward from the middle.)
        let mut tries: Vec<(i32, i32)> = (-20..=20).flat_map(|i| (-20..=20).map(move |j| (i, j))).collect();
        tries.sort_by_key(|(i, j)| i * i + j * j);
        for (i, j) in tries {
            let (y, pi) = (yaw + (i as f32 * 0.1).to_radians(), pitch + (j as f32 * 0.1).to_radians());
            let dir = crate::portal::view_dir(y, pi);
            let (saved, on) = (self.sim.portals, self.sim.portal_mover);
            if self.sim.fire(color, Aim { eye, dir, yaw: y }) && self.sim.portals[color.index()].is_some_and(|q| fits(&q)) {
                return;
            }
            (self.sim.portals, self.sim.portal_mover) = (saved, on);
        }
        panic!("{}: no {color:?} portal to be had from {} about yaw {yaw}, pitch {pitch}", self.name, self.c.feet);
    }

    /// [`shoot_near`](Self::shoot_near) aimed at `at`, for one that opens within a metre of it.
    pub fn shoot_past(&mut self, color: PortalColor, at: Vec3) {
        let d = at - self.c.view().eye;
        let (yaw, pitch) = ((-d.x).atan2(-d.z), d.y.atan2(Vec2::new(d.x, d.z).length()));
        self.shoot_near(color, yaw, pitch, |q| q.center.distance(at) < 1.0);
    }

    /// Walk into the portal at `at` (one she can't stop in front of: a floor one beside her,
    /// one round a corner) and on until she's come out of the other one and is standing again.
    pub fn walk_into_portal(&mut self, at: Vec3, speed: f32) {
        let (mut through, end) = (false, self.time + 6.0);
        while !(through && self.c.state == State::Ground) {
            assert!(self.time < end, "{}: didn't get through the portal at {at} (at {})", self.name, self.c.feet);
            let n = self.events.len();
            if !through {
                let d = at - self.c.feet;
                self.c.yaw = (-d.x).atan2(-d.z);
            }
            self.step(&Input { move_axis: Vec2::new(0.0, if through { 0.0 } else { speed }), ..Default::default() });
            through |= self.events[n..].iter().any(|e| matches!(e, SimEvent::Teleported { .. }));
        }
        self.wait(1.0);
    }
}

/// Chamber 19 (testchmb_a_15), and its escape, to the level change into escape_00. Portal's
/// route where Faith can take it:
/// - The pellet: portals on the two tilted panels send it up into the catcher.
/// - The scaffold: Faith's crouch is taller than Chell's, so rather than ride under the pipe
///   she drops onto it from a ceiling portal past it, by way of the corner's platform; off at
///   the switch's platform, the switch; through the door while it's open, a portal on the
///   track's far end, and through onto the next scaffold. The bouncing pellet ahead sent up
///   out of her way (it lives for ever: kept bouncing over her head); the panel across the
///   track got past with portals; out of the incinerator by a jump into a portal on its wall.
/// - The escape: up through the window over the incinerator, the broken stairs, over the
///   contaminated pool by portal; into the white room through its ceiling (shot through a
///   vent's slats); the exit's button (behind a grill, so no lid chunk from the broken
///   dropper gets to it by portal: she stands on it, and portals past the doors that shut
///   behind her); down the empty elevator shaft by portal (too far to fall), the crushers'
///   rooms with a climb, and through the grate's last room by portal.
#[test]
fn solve_testchmb_a_15() {
    use std::f32::consts::{FRAC_PI_2, PI};
    let Some(mut p) = Play::load("testchmb_a_15") else { return };
    p.wait(3.0);
    // The pellet into the catcher: blue on the tilted panel under the launcher, orange on the
    // one under the catcher (from each side of them); bounced back onto blue off the south
    // wall, it comes up out of orange into the catcher.
    p.go(Vec3::new(1.6, 0.0, 2.0), 0.4);
    p.shoot(PortalColor::Blue, Vec3::new(1.61, 1.23, 6.47));
    p.go(Vec3::new(1.6, 0.0, 13.5), 0.4);
    p.shoot(PortalColor::Orange, Vec3::new(1.61, 1.23, 9.77));
    p.wait_for(20.0, "the pellet caught", |p| p.events.iter().any(|e| matches!(e, SimEvent::PelletGone { caught: true, .. })));
    p.assert_alive("at the catcher");
    // Onto the scaffold past the pipe over the west side (Faith doesn't crouch as low as
    // Chell): blue on the ceiling over the track's south-west corner, and orange under her as
    // the scaffold comes to it; down onto the corner's platform, and off its far side onto the
    // scaffold.
    let brush = p.mover("rail_cart_brush_lab2");
    // (The scaffold's floor: none for a moment as each one's made.)
    let platform = |p: &Play, m: usize| p.sim.movers[m].boxes().next().unwrap_or(Aabb { min: Vec3::splat(1e6), max: Vec3::splat(1e6) });
    p.go(Vec3::new(-12.125, 0.0, 12.125), 0.3);
    p.shoot(PortalColor::Blue, Vec3::new(-12.3, 6.5, -1.68));
    p.wait_for(60.0, "the scaffold near the corner", |p| {
        let c = platform(p, brush).center();
        (c.x + 13.0).abs() < 0.2 && c.z < 1.5 && c.z > -1.0
    });
    p.drop_through(PortalColor::Orange, FRAC_PI_2, 0.0);
    p.wait_for(10.0, "the scaffold past the corner's platform", |p| {
        let b = platform(p, brush);
        b.min.x > p.c.feet.x - 0.2 && b.min.x < p.c.feet.x + 1.0
    });
    p.c.yaw = -FRAC_PI_2;
    let end = p.time + 2.0;
    while p.time < end && !(p.c.state == State::Ground && p.c.feet.y < 0.2) {
        p.step(&Input { move_axis: Vec2::new(0.0, 0.8), ..Default::default() });
    }
    p.assert_alive("onto the scaffold");
    // Off it onto the switch's platform to the south, and the switch: the door across the
    // track ahead opens for a moment.
    p.wait_for(10.0, "the scaffold by the switch's platform", |p| p.c.feet.x > -6.6);
    let mut jumped = false;
    p.c.yaw = 0.0;
    while !(jumped && p.c.state == State::Ground) {
        let jump = !jumped && p.c.state == State::Ground;
        jumped |= jump;
        p.step(&Input { move_axis: Vec2::new(0.0, 1.0), jump_pressed: jump, jump_held: true, ..Default::default() });
    }
    p.wait(0.5);
    p.assert_alive("onto the switch's platform");
    p.press("func_rot_button_lab2_tracktraindoor");
    // Through the door while it's open, blue on the wall at the track's far end; orange on the
    // wall by her, and through it as the next scaffold comes by blue.
    let door = p.mover("railway_door_lab2");
    p.go(Vec3::new(-7.25, 0.0, -3.75), 0.2);
    p.nudge(Vec3::new(-7.25, 0.0, -3.75));
    p.wait_for(3.0, "the track's door open", |p| p.sim.movers[door].node == 1 || p.sim.movers[door].goal == Some(0));
    p.shoot(PortalColor::Blue, Vec3::new(8.13, 1.45, -0.3));
    p.shoot(PortalColor::Orange, Vec3::new(-8.13, 1.45, -5.66));
    let next = p.mover("spawned_rail_cart_brush_lab2");
    p.go(Vec3::new(-6.9, 0.0, -5.66), 0.2);
    p.wait_for(90.0, "the next scaffold coming to the far end", |p| {
        let c = platform(p, next).center();
        (c.z + 1.63).abs() < 0.2 && c.x > 6.2
    });
    // (On board: the pellet bouncing across the track ahead, into blue on the wall it
    // bounces off and out of orange high on the hall's end wall; once it's up there, blue
    // high on the side wall too: it bounces about over her head from then on, for good.)
    let (mut crossed, mut redirected, mut high) = (false, false, false);
    p.c.yaw = FRAC_PI_2;
    let end = p.time + 20.0;
    while !(high && p.c.feet.z > 18.0 && p.c.feet.x < 0.5) {
        assert!(p.time < end, "testchmb_a_15: didn't ride the scaffold round to the north side (at {})", p.c.feet);
        let n = p.events.len();
        // (Out onto the scaffold: in from its edge.)
        let walk = !crossed || (p.c.state == State::Ground && p.c.feet.x > 6.9);
        p.step(&Input { move_axis: Vec2::new(0.0, if walk { 0.4 } else { 0.0 }), ..Default::default() });
        crossed |= p.events[n..].iter().any(|e| matches!(e, SimEvent::Teleported { .. }));
        if crossed && !redirected && p.c.state == State::Ground && p.c.feet.x < 6.9 {
            p.shoot(PortalColor::Blue, Vec3::new(8.13, 1.0, 8.12));
            p.shoot(PortalColor::Orange, Vec3::new(6.5, 4.9, 21.13));
            redirected = true;
        }
        if redirected && !high && p.sim.pellets.iter().any(|q| q.alive && q.pos.y > 3.0 && q.vel.z < -3.0) {
            p.shoot(PortalColor::Blue, Vec3::new(8.13, 4.9, 3.0));
            high = true;
        }
    }
    p.assert_alive("past the bouncing pellet");
    // The panel hanging across the track: blue on it, orange on the island's wall beyond it
    // (hidden by the panel: shot through blue, out of orange on the far wall for a moment); and,
    // pushed back by the panel, through, back onto the scaffold coming out from under it.
    // (From the back of the scaffold: the far portal's beside its front half once she's
    // through.)
    for _ in 0..90 {
        let to = platform(&p, next).center() + Vec3::new(0.85, 0.0, 0.0);
        let d = Vec2::new(to.x - p.c.feet.x, to.z - p.c.feet.z);
        if d.length() < 0.08 {
            break;
        }
        let (s, c) = p.c.yaw.sin_cos();
        let (fwd, right) = (Vec2::new(-s, -c), Vec2::new(c, -s));
        let k = (d.length() * 2.0).min(0.5) / d.length();
        p.step(&Input { move_axis: Vec2::new(d.dot(right), d.dot(fwd)) * k, ..Default::default() });
    }
    let panel = Vec3::new(-3.25, 1.62, 18.44);
    p.shoot(PortalColor::Blue, panel);
    p.shoot(PortalColor::Orange, Vec3::new(-4.75, 1.45, 21.13));
    let eye = p.c.view().eye;
    let dir = (Vec3::new(panel.x, eye.y, panel.z) - eye).normalize();
    assert!(p.sim.fire(PortalColor::Orange, Aim { eye, dir, yaw: (-dir.x).atan2(-dir.z) }), "testchmb_a_15: no orange beyond the panel");
    let mut through = false;
    p.c.yaw = FRAC_PI_2;
    p.c.pitch = 0.0;
    let end = p.time + 30.0;
    while !(through && p.c.feet.z > 30.0 && p.c.state == State::Ground) {
        assert!(p.time < end, "testchmb_a_15: didn't get past the panel (at {})", p.c.feet);
        let n = p.events.len();
        let walk = (!through && p.c.feet.x < -2.6) || (through && p.c.state == State::Ground && p.c.feet.z < 18.2);
        if through {
            p.c.yaw = PI;
        }
        p.step(&Input { move_axis: Vec2::new(0.0, if walk { 0.4 } else { 0.0 }), ..Default::default() });
        through |= p.events[n..].iter().any(|e| matches!(e, SimEvent::Teleported { .. }));
    }
    p.assert_alive("past the panel");
    // Out of the incinerator: blue on its east wall beside the scaffold, orange on the north
    // wall just above the deck at its far end; a running jump off the scaffold into blue, and
    // out onto the deck.
    p.wait_for(10.0, "the scaffold into the incinerator", |p| platform(p, next).center().z > 34.0);
    p.shoot(PortalColor::Orange, Vec3::new(-11.0, 1.45, 53.64));
    p.shoot(PortalColor::Blue, Vec3::new(-7.32, -3.2, 41.5));
    p.wait_for(10.0, "the scaffold by blue", |p| platform(p, next).center().z > 39.0);
    p.leap(-FRAC_PI_2, |f| f.x > -10.5, PI, -100.0);
    p.assert_alive("out of the incinerator");
    p.wait(2.0);
    // Up through the window high over the incinerator's mouth: blue on the ceiling of the room
    // behind it, orange under her on the deck.
    p.go(Vec3::new(-11.0, 0.0, 52.5), 0.3);
    p.shoot(PortalColor::Blue, Vec3::new(-9.87, 21.13, 23.38));
    p.drop_through(PortalColor::Orange, 0.0, 0.0);
    p.assert_alive("up through the window");
    p.wait(2.0);
    // West through the backstage, up what's left of the broken stairs (a hop from the last
    // piece onto the landing), and on to the escape's catwalks. (Through the middle of the
    // doorways: clear of their posts.)
    for w in [Vec3::new(-14.3, 8.94, 32.5), Vec3::new(-17.0, 8.94, 32.5), Vec3::new(-34.38, 12.18, 25.3)] {
        p.go(w, 0.25);
    }
    p.run_jump(0.0, |_| true);
    for w in [Vec3::new(-34.95, 13.0, 23.4), Vec3::new(-34.95, 13.0, 21.0), Vec3::new(-22.76, 16.26, 3.05)] {
        p.go(w, 0.25);
    }
    p.assert_alive("up the broken stairs");
    // Over the contaminated pool (its catwalk fallen in): blue on the far wall of the room beyond
    // it, just above its floor, orange on the walkway's side wall, and in.
    p.go(Vec3::new(-30.9, 16.26, 3.0), 0.25);
    p.go(Vec3::new(-30.9, 16.26, -1.0), 0.2);
    let far = Vec3::new(-27.24, 21.79, -29.26);
    p.shoot(PortalColor::Blue, far);
    let side = Vec3::new(-29.26, 17.75, -1.3);
    p.shoot(PortalColor::Orange, side);
    p.enter_now(PortalColor::Orange, side, Some(far));
    p.wait_for(5.0, "her feet", |p| p.c.state == State::Ground);
    p.assert_alive("over the pool");
    // Down into the white room beyond: blue on its ceiling, shot through the slats of the vent
    // between the rooms, and orange under her. (Between the post and the crates on the way.)
    for w in [Vec3::new(-14.5, 19.51, -27.9), Vec3::new(-9.0, 19.51, -27.9), Vec3::new(-3.07, 19.51, -30.0)] {
        p.go(w, 0.15);
    }
    p.shoot_near(PortalColor::Blue, 0.0454, 0.3875, |q| q.center.z < -34.6 && q.normal.y < -0.9);
    p.drop_through(PortalColor::Orange, 0.0, 0.0);
    p.assert_alive("into the white room");
    // The exit's doors: open while she stands on their button (behind a grill, so no lid chunk
    // from the broken dropper gets to it by portal), and shut the moment she's off it. So,
    // standing on it, blue on the end wall of the corridor beyond them, orange in the floor
    // beside her, and in: out past the doors.
    p.go(Vec3::new(-6.5, 16.26, -43.0), 0.3);
    p.go(Vec3::new(-6.5, 16.26, -48.04), 0.2);
    p.nudge(Vec3::new(-6.5, 16.26, -48.04));
    let doors = p.mover("exit_door_left_a05");
    p.wait_for(5.0, "the exit's doors open", |p| p.sim.movers[doors].node == 1);
    p.shoot(PortalColor::Blue, Vec3::new(-6.69, 17.66, -63.4));
    let beside = Vec3::new(-8.8, 16.26, -49.0);
    p.shoot(PortalColor::Orange, beside);
    p.go(Vec3::new(-8.8, 16.26, -51.0), 0.3);
    p.walk_into_portal(beside, 0.4);
    p.assert_alive("past the exit's doors");
    // Down the empty elevator shaft (too far to fall): blue on its floor, shot from the far
    // side of the hole (she comes up out of it heading on east, the way she's going), orange
    // on the wall beside it, and in.
    p.go(Vec3::new(-22.15, 16.26, -61.18), 0.15);
    p.shoot(PortalColor::Blue, Vec3::new(-19.6, -2.44, -62.3));
    let side = Vec3::new(-15.39, 17.75, -64.15);
    p.shoot(PortalColor::Orange, side);
    let q = p.sim.portals[1].unwrap();
    p.go(q.center + q.normal * 1.5 - Vec3::Y * (q.center.y - 16.26), 0.3);
    p.walk_into_portal(q.center, 0.5);
    p.assert_alive("down the shaft");
    // (Past the little crushers, and through the doorway beyond, clear of its post.)
    for w in [Vec3::new(-12.0, -1.63, -59.8), Vec3::new(-8.5, -1.63, -55.1), Vec3::new(-5.8, -1.63, -55.1)] {
        p.go(w, 0.25);
    }
    // Up into the passage at the far end of the crushers' room: a jump and a climb.
    p.go(Vec3::new(1.4, 0.0, -65.0), 0.3);
    p.run_jump(0.0, |f| f.z < -67.75);
    // At its low end, crouched, a grate looks across the fan's shaft into the last room, which
    // only a portal gets her into: blue through it onto that room's ceiling. Back down into the
    // room below, orange in its floor, and down into the last room: out.
    p.go(Vec3::new(1.2, 3.26, -79.0), 0.3);
    p.c.yaw = 0.0;
    let end = p.time + 4.0;
    while p.c.feet.z > -80.9 {
        assert!(p.time < end, "testchmb_a_15: stuck in the low passage (at {})", p.c.feet);
        p.step(&Input { move_axis: Vec2::new(0.0, 0.5), crouch_held: true, crouch_pressed: true, ..Default::default() });
    }
    p.wait(0.5);
    p.shoot_near(PortalColor::Blue, -0.2374, 0.4817, |q| q.center.z < -85.3);
    p.c.yaw = PI;
    let end = p.time + 4.0;
    while p.c.feet.z < -79.0 {
        assert!(p.time < end, "testchmb_a_15: stuck in the low passage (at {})", p.c.feet);
        p.step(&Input { move_axis: Vec2::new(0.0, 0.5), crouch_held: true, ..Default::default() });
    }
    p.wait(0.5);
    p.go(Vec3::new(-3.0, 0.0, -66.0), 0.3);
    p.drop_through(PortalColor::Orange, 0.0, 0.0);
    p.assert_alive("into the last room");
    let end = p.time + 6.0;
    while p.sim.transition.is_none() {
        assert!(p.time < end, "testchmb_a_15: never left for escape_00 (at {})", p.c.feet);
        let d = Vec3::new(-0.4, 4.88, -89.8) - p.c.feet;
        p.c.yaw = (-d.x).atan2(-d.z);
        p.step(&Input { move_axis: Vec2::new(0.0, 0.5), ..Default::default() });
    }
    p.assert_leads_to("escape_00");
}

#[test]
fn chamber_00_clock_runs() {
    let Some(mut p) = Play::load("testchmb_a_00") else { return };
    let model = |p: &Play, name: &str| p.mover(name);
    let walls = [model(&p, "brush_clock_centiseconds_wall"), model(&p, "brush_clock_milliseconds_wall")].map(|m| p.sim.movers[m].def.model);
    let frames = |p: &Play| walls.map(|m| p.sim.io.as_ref().unwrap().frames.get(&m).copied());
    // (The map starts it about 7 s in.)
    p.wait(8.0);
    let a = frames(&p);
    p.wait(0.05);
    let b = frames(&p);
    p.wait(0.33);
    let c = frames(&p);
    eprintln!("clock frames {a:?} {b:?} {c:?}");
    assert!(a[1].is_some() && (a[1] != b[1] || b[1] != c[1]), "the thousandths don't run: {a:?} {b:?} {c:?}");
    assert!(a[0] != c[0] || b[0] != c[0], "the hundredths don't run: {a:?} {b:?} {c:?}");
}





/// Chamber 15's section-2 pellet (#61): its launcher fires into a plate set just in front of
/// it, which sends the pellet back out across the room (not trapped between the plate and the
/// launcher's wall); and no portal goes over the launcher, set in its wall.
#[test]
fn chamber_15_pellet_comes_out_of_its_launcher() {
    let Some(mut p) = Play::load("testchmb_a_10") else { return };
    p.c.feet = Vec3::new(70.0, -9.3, 27.0);
    let mut far = f32::MIN;
    for _ in 0..240 {
        p.step(&Input::default());
        far = p.sim.pellets.iter().filter(|q| q.alive && (q.pos.x - 50.54).abs() < 0.1).map(|q| q.pos.z).fold(far, f32::max);
    }
    assert!(far > 5.0, "the pellet didn't come out across the room (furthest z {far})");
    // From the far side, aimed at the launcher's wall right over it.
    p.c.feet = Vec3::new(51.0, -9.75, -7.0);
    p.c.vel = Vec3::ZERO;
    let at = Vec3::new(50.54, -4.99, -12.8);
    p.look_at(at);
    let eye = p.c.view().eye;
    p.sim.portals = [None, None];
    p.sim.fire(PortalColor::Orange, Aim { eye, dir: (at - eye).normalize(), yaw: p.c.yaw });
    assert!(p.sim.portals[1].is_none_or(|o| !o.in_rect(Vec3::new(at.x, at.y, o.center.z), -0.2)), "a portal went over the launcher: {:?}", p.sim.portals[1]);
}


/// Chamber 15's moving platforms: once section 3's catcher has its pellet, the two rail carts
/// on the track start (wildcard targets: `tractrain_brush_*`) and are gone at its end, and
/// more come along from the maker one after another, each running the track and gone at the
/// end (every rail node, shared by them all, sees each pass).
#[test]
fn chamber_15_rail_carts_run() {
    let Some(mut p) = Play::load("testchmb_a_10") else { return };
    p.wait(1.0);
    let mut io = p.sim.io.take().unwrap();
    for (name, input) in [("logic_enable_tracknodes", "Trigger"), ("timer_spawn_tracks_intro_lvl7b", "Enable")] {
        let e = io.ents.iter().position(|e| e.name == name).unwrap();
        io.input(&mut p.sim, e, input, "", e);
    }
    p.sim.io = Some(io);
    let train = |p: &Play, name: &str| p.mover(name);
    let (one, two, cart) = (train(&p, "tractrain_brush_1"), train(&p, "tractrain_brush_2"), train(&p, "spawned_rail_cart_intro_lvl7b"));
    assert!(!p.sim.movers[cart].present, "the maker's cart is there before it's made");
    p.wait(2.0);
    assert!(p.sim.movers[two].goal.is_some(), "the carts didn't start");
    p.wait(20.0);
    assert!(!p.sim.movers[one].present && !p.sim.movers[two].present, "the carts weren't gone at the end of the track");
    // The maker's: out along the track, gone at its end, and out again.
    let (mut runs, mut was) = (0, false);
    for _ in 0..(70 * 60) {
        p.step(&Input::default());
        let now = p.sim.movers[cart].present && p.sim.movers[cart].goal.is_some();
        runs += (now && !was) as u32;
        was = now;
    }
    assert!(runs >= 2, "the maker's cart ran {runs} times in 70 s");
}

/// Chamber 16's hideout panel (testchmb_a_11): the stacked cubes keep it shut, resting against
/// it, not in it, until they're moved (#68).
#[test]
fn chamber_16_cubes_hold_the_hideout_panel() {
    let Some(mut p) = Play::load("testchmb_a_11") else { return };
    p.wait(3.0);
    let panel = p.mover("door_spaz_pannel1");
    assert!(p.sim.movers[panel].node == 0 && p.sim.movers[panel].goal.is_none(), "the panel opened by itself");
    let top = p.sim.props.iter().position(|c| c.alive && (c.pos.x + 17.98).abs() < 0.1 && c.pos.y > 1.0).expect("no cube on top of the stack");
    // (The panel's face is at z 8.7376.)
    assert!(p.sim.props[top].pos.z - crate::sim::CUBE_HALF > 8.73, "the top cube is in the panel: {}", p.sim.props[top].pos);
    // Taken away: the panel spazzes open.
    p.sim.props[top].alive = false;
    p.wait_for(3.0, "the panel to open", |p| p.sim.movers[panel].node == 1);
}

/// Chamber 16's clock radio, mug and tools stand on a cube (testchmb_a_11): on ours, smaller
/// than Portal's, they sit on its top, not over it, and drop to the floor when it's taken (#72).
#[test]
fn chamber_16_props_sit_on_their_cube() {
    let Some(mut p) = Play::load("testchmb_a_11") else { return };
    // (The radio, mug and wrench: where the map has them, and how far their lowest points are
    // below that, as `portal_extract` writes them.)
    let on = [(Vec3::new(239.0, -104.0, 44.0), 0.339), (Vec3::new(248.0, -77.0, 45.356), 3.175), (Vec3::new(254.779, -97.016, 43.201), 0.249)];
    let mut perched: Vec<_> = on.iter().map(|&(at, bottom)| crate::source::Perched::find(&p.sim, crate::source::to_ours(at), Some(bottom * 0.0254)).expect("not on a cube")).collect();
    let cube = perched[0].cube.unwrap();
    assert!(perched.iter().all(|q| q.cube == Some(cube)), "not all on the one cube");
    let foot = |q: &crate::source::Perched, bottom: f32| q.at.y - bottom * 0.0254;
    for _ in 0..180 {
        p.wait(1.0 / 60.0);
        perched.iter_mut().for_each(|q| q.step(&p.sim, 1.0 / 60.0));
    }
    let i = p.sim.props.iter().position(|c| c.id == cube).unwrap();
    let top = p.sim.props[i].pos.y + crate::sim::CUBE_HALF;
    for (q, &(_, bottom)) in perched.iter().zip(&on) {
        assert!((foot(q, bottom) - top).abs() < 0.01, "{q:?} isn't on the cube's top ({top})");
    }
    // Taken away: they fall to the floor.
    p.sim.props[i].alive = false;
    for _ in 0..120 {
        perched.iter_mut().for_each(|q| q.step(&p.sim, 1.0 / 60.0));
    }
    for (q, &(_, bottom)) in perched.iter().zip(&on) {
        assert!(q.cube.is_none() && foot(q, bottom).abs() < 0.01, "{q:?} didn't land on the floor");
    }
}

/// Chamber 16's cage lights (testchmb_a_11) hang from the den's cables: each cable, hung as
/// Source hangs it, runs through the top of the light under it (#67).
#[test]
fn chamber_16_cage_lights_hang_from_cables() {
    let Ok(text) = std::fs::read_to_string(format!("{}/../../assets/extracted/maps/testchmb_a_11.ents", env!("CARGO_MANIFEST_DIR"))) else { return };
    let ropes = crate::rope::hang(&crate::source::parse_entities(&text));
    // (A cage light's hook is its model's top, 11 units over its origin.)
    for light in [Vec3::new(-576.248, 704.455, 213.485), Vec3::new(-608.487, 960.892, 209.592), Vec3::new(-660.248, 679.455, 162.485)] {
        let o = crate::source::to_ours(light);
        let over = ropes.iter().flat_map(|r| r.points.iter()).filter(|q| Vec3::new(q.x - o.x, 0.0, q.z - o.z).length() < 0.06).map(|q| q.y - o.y).fold(f32::MAX, f32::min);
        assert!((0.1..0.3).contains(&over), "the cable over the light at {o} is {over} m above it");
    }
}

/// Chamber 15's grills (testchmb_a_10) run from emitter to emitter: section 2's, wall to
/// wall, though Portal's trigger for it stops 1.6 m short of the west wall (#60).
#[test]
fn chamber_15_grills_reach_their_emitters() {
    let Some(p) = Play::load("testchmb_a_10") else { return };
    let section2 = p.sim.fizzlers.iter().find(|f| (f.b.center().z + 3.28).abs() < 0.1).expect("no grill in section 2");
    assert!((section2.b.min.x - 45.517).abs() < 0.02 && (section2.b.max.x - 61.773).abs() < 0.02, "section 2's grill: {:?}", section2.b);
    // Each grill's edge emitters sit on its ends.
    for &at in &p.sim.level.grill_edges {
        let f = p.sim.fizzlers.iter().find(|f| at.y > f.b.min.y && at.y < f.b.max.y && (at - f.b.center()).abs().cmplt(f.b.size() * 0.5 + Vec3::splat(0.5)).all());
        assert!(f.is_some(), "an emitter at {at} is off every grill");
    }
}

/// Chamber 15's section 3 fling (#57): over the glass (x 23) from the room by the lift, as
/// Portal has it. Blue high on the east wall, orange on the floor: into orange, out of blue,
/// and orange again where she'll come down, facing the way she's flying (as Portal turns a
/// floor portal shot while she's in the air): the second time she comes out rising, fast,
/// and over the glass.
#[test]
fn chamber_15_section_3_fling_clears_the_glass() {
    let Some(mut p) = Play::load("testchmb_a_10") else { return };
    let half = p.sim.portal_half();
    let floor = |x: f32| Some(crate::Portal::sized(Vec3::new(x, -6.5, 8.87), Vec3::Y, Vec3::NEG_X, half));
    p.sim.set_portal(PortalColor::Blue, Some(crate::Portal::sized(Vec3::new(42.27, 3.87, 8.87), Vec3::NEG_X, Vec3::Y, half)));
    p.sim.set_portal(PortalColor::Orange, floor(39.0));
    // (Dropping in from the ledge over it.)
    p.c.feet = Vec3::new(39.0, -3.0, 8.87);
    p.c.state = State::Air;
    let g = p.sim.level.gravity.unwrap_or(crate::FAITH_GRAVITY);
    let (mut high, mut moved) = (false, false);
    for _ in 0..600 {
        p.step(&Input::default());
        high |= p.c.feet.y > 0.0;
        if high && !moved && p.c.vel.y < 0.0 && p.sim.player_env.is_none() {
            let (v, y) = (p.c.vel, p.c.feet.y + 6.5);
            let t = (v.y + (v.y * v.y + 2.0 * g * y).sqrt()) / g;
            p.sim.set_portal(PortalColor::Orange, floor(p.c.feet.x + v.x * t));
            moved = true;
        }
    }
    assert_eq!(p.deaths, 0);
    assert!(p.c.state == State::Ground && p.c.feet.x < 22.0, "short of the glass: at {}", p.c.feet);
}

#[test]
#[ignore]
fn dbg_survey() {
    let map = std::env::var("MAP").unwrap();
    let Some(mut p) = Play::load(&map) else { return };
    if let Ok(at) = std::env::var("AT") {
        let v: Vec<f32> = at.split(',').map(|x| x.parse().unwrap()).collect();
        p.c.feet = Vec3::new(v[0], v[1], v[2]);
        p.wait(0.5);
    }
    let text: String = p.sim.statics.iter().map(|s| format!("{} {} {} {} {} {} {:?}\n", s.b.min.x, s.b.min.y, s.b.min.z, s.b.max.x, s.b.max.y, s.b.max.z, s.mat)).collect();
    std::fs::write(format!("/tmp/{map}.boxes"), text).unwrap();
    p.dump_reach(&format!("/tmp/{map}.reach"));
    eprintln!("spawn {} yaw {} cubes {:?}", p.c.feet, p.c.yaw, p.sim.props.iter().filter(|q| q.kind != crate::level::CubeKind::Physics).map(|q| q.pos).collect::<Vec<_>>());
    eprintln!("turrets {:?}", p.sim.turrets.iter().map(|t| (t.at, t.facing())).collect::<Vec<_>>());
    eprintln!("launchers {:?}", p.sim.launchers.iter().map(|l| (l.at, l.dir)).collect::<Vec<_>>());
    eprintln!("goo {:?}", p.sim.goo);
    let io = p.sim.io.as_ref().unwrap();
    for e in io.ents.iter().filter(|e| matches!(e.kind, crate::io::Kind::ChangeLevel { .. } | crate::io::Kind::Mover { .. })) {
        eprintln!("ent {} {:?}", e.name, e.kind);
    }
    if std::env::var("PROBE").is_ok() {
        let at = p.c.feet;
        p.probe_portals(at);
    }
}

#[allow(dead_code)]
fn dbg_can(p: &mut Play, stands: &[Vec3]) {
    let eye_off = Vec3::new(0.0, p.c.view().eye.y - p.c.feet.y, 0.0);
    let mut seen: Vec<(Vec3, Vec3)> = Vec::new();
    for &stand in stands {
        let eye = stand + eye_off;
        for yaw_deg in (0..360).step_by(3) { for pitch_deg in (-87..=87).step_by(3) {
            let (y, pi) = ((yaw_deg as f32).to_radians(), (pitch_deg as f32).to_radians());
            let dir = crate::portal::view_dir(y, pi);
            let (saved, on) = (p.sim.portals, p.sim.portal_mover);
            let ok = p.sim.fire(PortalColor::Orange, Aim { eye, dir, yaw: y });
            let got = p.sim.portals[1];
            (p.sim.portals, p.sim.portal_mover) = (saved, on);
            if let Some(g) = got.filter(|_| ok) {
                if !seen.iter().any(|(c, n)| c.distance(g.center) < 1.2 && n.dot(g.normal) > 0.9) {
                    seen.push((g.center, g.normal));
                    let out = g.center + g.normal * 0.8;
                    let land = p.floors(out.x, out.z, out.y - 30.0, out.y).into_iter().reduce(f32::max);
                    eprintln!("DBG can {:.2} n {:.0} from {stand} lands {land:.2?}", g.center, g.normal);
                }
            }
        } }
    }
}

#[test]
#[ignore]
fn dbg_e0() {
    use std::f32::consts::{FRAC_PI_2, PI};
    let Some(mut p) = Play::load("escape_00") else { return };
    // (The head on the piston by the ladder's wall.)
    let head = (0..p.sim.movers.len()).find(|&m| p.sim.movers[m].def.parent == Some(p.mover("crusher_room_1_2")) && p.sim.movers[m].def.follows).unwrap();
    let top = |p: &Play| p.sim.movers[head].boxes().map(|b| b.max.y).fold(-1e9, f32::max);
    p.go(Vec3::new(18.6, -21.1, -6.6), 0.2);
    eprintln!("DBG at {:.1} {} top {}", p.time, p.c.feet, top(&p));
    p.wait_for(30.0, "the head down", |p| top(p) < -20.0);
    p.run_jump(0.0, |_| true);
    eprintln!("DBG on {:.1} {} top {}", p.time, p.c.feet, top(&p));
    p.nudge(Vec3::new(20.0, p.c.feet.y, -9.0));
    eprintln!("DBG nudged {:.1} {} {:?} top {}", p.time, p.c.feet, p.c.state, top(&p));
    p.wait_for(30.0, "the head up", |p| top(p) > -11.45);
    eprintln!("DBG up {:.1} {} {:?}", p.time, p.c.feet, p.c.state);
    let mut jumped = false;
    p.c.yaw = 0.0;
    let end = p.time + 5.0;
    while !(jumped && p.c.state == State::Ground) && p.time < end {
        let jump = (!jumped && p.c.feet.z < -10.3) || matches!(p.c.state, State::LedgeHang { .. });
        jumped |= jump;
        p.step(&Input { move_axis: Vec2::new(0.0, 1.0), jump_pressed: jump, jump_held: true, ..Default::default() });
    }
    eprintln!("DBG climbed {:.1} {} {:?}", p.time, p.c.feet, p.c.state);
    p.go(Vec3::new(18.69, -8.13, -16.46), 0.3);
    p.go(Vec3::new(-3.0, -14.63, -16.0), 0.3);
    eprintln!("DBG room2 {:.1} {}", p.time, p.c.feet);
    p.go(Vec3::new(-1.0, -17.88, -31.0), 0.3);
    p.shoot_past(PortalColor::Blue, Vec3::new(-13.0, -6.31, -20.59));
    eprintln!("DBG blue {:?}", p.sim.portals[0].map(|q| (q.center, q.normal)));
    p.go(Vec3::new(-13.3, -17.88, -30.0), 0.2);
    p.run_jump(FRAC_PI_2, |_| true);
    eprintln!("DBG tube {} {:?}", p.c.feet, p.c.state);
    p.go(Vec3::new(-21.5, -16.3, -30.0), 0.3);
    let end = Vec3::new(-24.38, -14.8, -30.0);
    p.shoot(PortalColor::Orange, end);
    p.walk_into_portal(end, 0.5);
    eprintln!("DBG out {:.1} {} {:?}", p.time, p.c.feet, p.c.state);
    p.c.yaw = -FRAC_PI_2;
    for _ in 0..40 { p.step(&Input { move_axis: Vec2::new(0.0, 0.5), ..Default::default() }); }
    p.wait(0.5);
    eprintln!("DBG walked {} {:?}", p.c.feet, p.c.state);
    p.dump_reach("/tmp/escape_00.reach");
    dbg_can(&mut p, &[Vec3::new(-11.0, -8.13, -20.0), Vec3::new(-5.0, -8.13, -16.0), Vec3::new(-8.0, -8.13, -15.0), Vec3::new(-4.0, -8.13, -22.0)]);
    let _ = (FRAC_PI_2, PI);
}
