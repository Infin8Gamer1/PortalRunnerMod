//! Mirror's Edge's Kismet, run alongside the sim (#109): what sets its matinees off (she walks
//! into a trigger, kicks a door, the level loads, another sequence calls it), through the ops
//! between (delays, gates, switches), and the matinees playing their movers.
//!
//! An op's input sets it going; it fires outputs, each leading to other ops' inputs (after the
//! output's delay). Events fire from what happens; actions do what they do and fire their first
//! output ("Out"), unless they're one of the few this knows more of. The movers' poses go to
//! the sim's movers (for collision, carrying her) and to `poses` (for drawing).

use glam::{Quat, Vec3};

use faith_move::Controller;

use crate::level::{Level, LogicOp};
use crate::sim::Mover;

/// A matinee's state: how far through it is (seconds), which way it's playing (1 forwards,
/// -1 back) and whether it is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Play {
    t: f32,
    dir: f32,
    on: bool,
}

/// How fast she has to be going into a door for it to be a kick (m/s).
const KICK_SPEED: f32 = 3.0;

/// A highlight's least time on before it fades out (`TdLOIAddOnObject.MinDuration`), seconds.
const LIT_AT_LEAST: f32 = 0.5;

/// A prop's runner-vision highlight (#118): how strong it is (its materials' `LOI_Strength`,
/// 0 to 1), what it's fading to, how fast (a second), and when it was last switched on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lit {
    pub level: f32,
    goal: f32,
    speed: f32,
    since: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeLogic {
    time: f32,
    started: bool,
    /// Inputs to set off: when, which op, which input.
    pending: Vec<(f32, usize, usize)>,
    /// Events' outputs to fire: when, which op, which output (a mover's "Open Finished").
    later: Vec<(f32, usize, usize)>,
    /// Each op's: how many times it's fired (events), when last, whether she's in its region.
    fired: Vec<u32>,
    last: Vec<f32>,
    inside: Vec<bool>,
    /// A gate's: open. A switch's: the next output.
    open: Vec<bool>,
    next: Vec<usize>,
    /// Each matinee op's play.
    plays: Vec<Play>,
    /// Each mover's pose now: how far it's moved and turned about its pivot.
    pub poses: Vec<(Vec3, Quat)>,
    /// Each body's (sim mover's): the mover that last moved it, and its pose (what's drawn).
    pub bodies: std::collections::HashMap<usize, (usize, Vec3, Quat)>,
    /// What's been switched (#111): bodies she doesn't bump into, things (by name) whose
    /// events don't fire, props hidden or shown (and a count of changes to them).
    passable: std::collections::HashSet<usize>,
    untouchable: std::collections::HashSet<String>,
    pub hidden: std::collections::HashMap<usize, bool>,
    pub hidden_changes: u32,
    /// Props runner vision lights up (#118), by their index.
    pub lit: std::collections::HashMap<usize, Lit>,
    /// Training's movement challenge under way (#118), and how many of its checkpoints'
    /// places she's passed.
    pub challenge: Option<(String, usize)>,
    /// Each challenge's checkpoints, a place at a time in order: whether that place can be
    /// missed, and its checkpoints (ops) with what she must be doing going through each.
    tracks: std::collections::HashMap<String, Vec<(bool, Vec<(usize, Vec<String>)>)>>,
}

impl MeLogic {
    /// How strongly runner vision lights up a prop now (0: not at all).
    pub fn lit(&self, prop: usize) -> f32 {
        self.lit.get(&prop).map_or(0.0, |l| l.level)
    }

    /// Where a body (a sim mover) is: its pivot, how far it's moved and turned.
    pub fn body(&self, level: &Level, sim: usize) -> Option<(Vec3, Vec3, Quat)> {
        let &(m, d, q) = self.bodies.get(&sim)?;
        Some((level.me_movers.get(m)?.pivot, d, q))
    }
}

impl MeLogic {
    pub fn new(level: &Level) -> MeLogic {
        let n = level.me_logic.len();
        let open = level.me_logic.iter().map(|o| o.param("open").is_none_or(|v| v != "false")).collect();
        let mut me = MeLogic {
            time: 0.0,
            started: false,
            pending: Vec::new(),
            later: Vec::new(),
            fired: vec![0; n],
            last: vec![f32::MIN; n],
            inside: vec![false; n],
            open,
            next: vec![0; n],
            plays: vec![Play::default(); n],
            poses: Vec::new(),
            bodies: std::collections::HashMap::new(),
            passable: Default::default(),
            untouchable: Default::default(),
            hidden: Default::default(),
            hidden_changes: 0,
            lit: Default::default(),
            challenge: None,
            tracks: Default::default(),
        };
        // (A track's places by their order: those of the same order are one place, to be
        // passed through any of them; a place can be missed if all its checkpoints can.)
        let mut places: std::collections::HashMap<String, std::collections::BTreeMap<i32, (bool, Vec<(usize, Vec<String>)>)>> = Default::default();
        for (k, op) in level.me_logic.iter().enumerate().filter(|(_, o)| o.op == "TdTutorialCheckpoint") {
            // (`track order [optional] [needs MOVE_Jump,ETE_SkillRoll]`.)
            for (_, t) in op.params.iter().filter(|(n, _)| n == "track") {
                let w: Vec<&str> = t.split_whitespace().collect();
                let (Some(track), Some(order)) = (w.first(), w.get(1).and_then(|o| o.parse::<i32>().ok())) else { continue };
                let optional = w.contains(&"optional");
                let needs = w.iter().position(|x| *x == "needs").and_then(|i| w.get(i + 1)).map_or(Vec::new(), |n| n.split(',').map(str::to_string).collect());
                let place = places.entry(track.to_string()).or_default().entry(order).or_insert((true, Vec::new()));
                place.0 &= optional;
                place.1.push((k, needs));
            }
        }
        me.tracks = places.into_iter().map(|(t, p)| (t, p.into_values().collect())).collect();
        me.poses = level.me_movers.iter().map(|m| m.at(0.0)).collect();
        me
    }

    /// The index of an op's output by its name (its params' `outputs`, `|`-separated), else
    /// `default`.
    fn output(op: &LogicOp, name: &str, default: usize) -> usize {
        op.param("outputs").and_then(|o| o.split('|').position(|n| n.eq_ignore_ascii_case(name))).unwrap_or(default)
    }

    /// Fire output `out` of op `k`: its links' inputs, each after its delay.
    fn fire(&mut self, level: &Level, k: usize, out: usize) {
        for &(o, to, input, delay) in &level.me_logic[k].outputs {
            if o == out {
                self.pending.push((self.time + delay, to, input));
            }
        }
    }

    /// An event of op `k` happens: it fires `out`, if it may (it's fired fewer than its most
    /// times, and not too soon after it last did).
    fn event(&mut self, level: &Level, k: usize, out: usize) {
        let op = &level.me_logic[k];
        let most = op.num("max", 0.0) as u32;
        if most > 0 && self.fired[k] >= most {
            return;
        }
        if self.time - self.last[k] < op.num("retrigger", 0.0) {
            return;
        }
        self.fired[k] += 1;
        self.last[k] = self.time;
        self.fire(level, k, out);
    }

    pub fn step(&mut self, level: &Level, movers: &mut [Mover], ctrl: &Controller, used: Option<crate::sim::Aim>, dt: f32) {
        self.time += dt;
        // The level's loaded (and a time trial's race has started).
        if !self.started {
            self.started = true;
            for k in 0..level.me_logic.len() {
                if matches!(level.me_logic[k].op.as_str(), "SeqEvent_LevelLoaded" | "SeqEvt_TTRaceStarted" | "SeqEvent_LevelStartup") {
                    self.event(level, k, 0);
                }
            }
        }
        // What she's in or runs into, kicks, or uses.
        let me = ctrl.body().aabb(ctrl.feet);
        let speed = Vec3::new(ctrl.vel.x, 0.0, ctrl.vel.z).length();
        let kicking = ctrl.melee.is_some();
        let facing = Vec3::new(-ctrl.yaw.sin(), 0.0, -ctrl.yaw.cos());
        for k in 0..level.me_logic.len() {
            let op = &level.me_logic[k];
            if op.regions.is_empty() {
                continue;
            }
            // (Its actor switched off: nothing to touch.)
            if op.param("originator").is_some_and(|o| self.untouchable.contains(o)) {
                self.inside[k] = false;
                continue;
            }
            let reach = if op.op == "SeqEvent_TakeDamage" { 0.3 } else { 0.0 };
            let inside = op.regions.iter().any(|r| r.overlaps(&faith_move::Aabb::new(me.min - Vec3::splat(reach), me.max + Vec3::splat(reach))));
            let was = std::mem::replace(&mut self.inside[k], inside);
            match op.op.as_str() {
                "SeqEvent_Touch" | "SeqEvent_TdTouch" => {
                    if inside && !was {
                        self.event(level, k, 0);
                    } else if was && !inside {
                        let out = Self::output(op, "UnTouched", 1);
                        self.event(level, k, out);
                    }
                }
                // (Used: she pressed use looking at it, in reach.)
                "SeqEvent_TdUsed" | "SeqEvent_Used" => {
                    if let Some(aim) = used {
                        if op.regions.iter().any(|r| crate::geom::ray_aabb(aim.eye, aim.dir, &faith_move::Aabb::new(r.min - Vec3::splat(0.15), r.max + Vec3::splat(0.15))).is_some_and(|(t, _)| t < crate::sim::USE_REACH)) {
                            self.event(level, k, 0);
                        }
                    }
                }
                // (Kicked: run into at speed, or kicked where it's in front of her.)
                "SeqEvent_TakeDamage" => {
                    let kick = kicking && op.regions.iter().any(|r| r.overlaps(&faith_move::Aabb::new(me.min - Vec3::splat(0.2), me.max + Vec3::splat(0.2)).translated(facing * 0.6)));
                    if (inside && !was && speed > KICK_SPEED) || kick {
                        self.event(level, k, 0);
                    }
                }
                // (Stood on, and got off.)
                "SeqEvent_Mover" => {
                    if inside && !was {
                        let out = Self::output(op, "Pawn Attached", 0);
                        self.event(level, k, out);
                    } else if was && !inside {
                        if let Some(out) = op.param("outputs").and_then(|o| o.split('|').position(|n| n.eq_ignore_ascii_case("Pawn Detached"))) {
                            self.fire(level, k, out);
                        }
                    }
                }
                _ => {}
            }
        }
        // A movement challenge's checkpoints, gone through.
        self.challenge_step(level, ctrl);
        // Events' outputs that are due.
        let due: Vec<(f32, usize, usize)> = self.later.iter().copied().filter(|p| p.0 <= self.time).collect();
        self.later.retain(|p| p.0 > self.time);
        for (_, k, out) in due {
            self.fire(level, k, out);
        }
        // The inputs that are due (and those they set off at once).
        let mut guard = 0;
        loop {
            let due: Vec<(f32, usize, usize)> = self.pending.iter().copied().filter(|p| p.0 <= self.time).collect();
            if due.is_empty() || guard > 64 {
                break;
            }
            guard += 1;
            self.pending.retain(|p| p.0 > self.time);
            for (_, k, input) in due {
                self.input(level, k, input);
            }
        }
        // The matinees play.
        for k in 0..level.me_logic.len() {
            let mut p = self.plays[k];
            if !p.on {
                continue;
            }
            let op = &level.me_logic[k];
            let length = op.movers.iter().map(|&m| level.me_movers[m].length).fold(0.0f32, f32::max).max(op.num("length", 0.0));
            let rate = op.movers.first().map_or(1.0, |&m| level.me_movers[m].rate);
            let before = p.t;
            p.t += dt * rate * p.dir;
            // (Its event tracks' outputs, as it plays past them.)
            if p.dir > 0.0 {
                for (_, e) in op.params.iter().filter(|(n, _)| n == "event") {
                    let Some((at, name)) = e.split_once(' ') else { continue };
                    let Ok(at) = at.parse::<f32>() else { continue };
                    if at > before.min(length) && at <= p.t.min(length) || at == 0.0 && before == 0.0 {
                        if let Some(out) = op.param("outputs").and_then(|o| o.split('|').position(|n| n.eq_ignore_ascii_case(name))) {
                            self.fire(level, k, out);
                        }
                    }
                }
            }
            let looping = op.movers.first().is_some_and(|&m| level.me_movers[m].looping);
            if p.dir > 0.0 && p.t >= length {
                if looping && length > 0.0 {
                    p.t -= length;
                } else {
                    p.t = length;
                    p.on = false;
                    self.plays[k] = p;
                    let out = Self::output(op, "Completed", 0);
                    self.fire(level, k, out);
                    // A mover event that set it going: it's open (after its stay-open time).
                    for (e, ev) in level.me_logic.iter().enumerate() {
                        if ev.op == "SeqEvent_Mover" && ev.outputs.iter().any(|o| o.1 == k) {
                            if let Some(out) = ev.param("outputs").and_then(|o| o.split('|').position(|n| n.eq_ignore_ascii_case("Open Finished"))) {
                                self.later.push((self.time + ev.num("stay_open", 1.5), e, out));
                            }
                        }
                    }
                }
            } else if p.dir < 0.0 && p.t <= 0.0 {
                p.t = 0.0;
                p.on = false;
                self.plays[k] = p;
                let out = Self::output(op, "Reversed", 1);
                self.fire(level, k, out);
            }
            self.plays[k] = p;
            for &m in &op.movers {
                let (d, q) = level.me_movers[m].at(p.t);
                self.poses[m] = (d, q);
                self.bodies.insert(level.me_movers[m].sim, (m, d, q));
            }
        }
        // Highlights fade in or out (one switched off holds its least time first).
        let now = self.time;
        for l in self.lit.values_mut() {
            if l.goal < l.level && now - l.since < LIT_AT_LEAST {
                continue;
            }
            let step = l.speed * dt;
            l.level = if l.goal > l.level { (l.level + step).min(l.goal) } else { (l.level - step).max(l.goal) };
        }
        // The sim's movers go where the bodies are; switched off, they're passed through.
        for (&sim, &(_, d, q)) in &self.bodies {
            if let Some(mv) = movers.get_mut(sim) {
                mv.target = Some(d);
                mv.turned = q.angle_between(Quat::IDENTITY) > 0.02;
            }
        }
        for def in &level.me_movers {
            if let Some(mv) = movers.get_mut(def.sim) {
                mv.present = !self.passable.contains(&def.sim);
            }
        }
    }

    /// The events of a kind (`op`) for a movement challenge (`challenge`) happen.
    fn challenge_events(&mut self, level: &Level, op: &str, challenge: &str) {
        for e in 0..level.me_logic.len() {
            if level.me_logic[e].op == op && level.me_logic[e].param("challenge").is_some_and(|c| c.eq_ignore_ascii_case(challenge)) {
                self.event(level, e, 0);
            }
        }
    }

    /// The challenge under way: she passes its next place (or one after only places that can
    /// be missed) when she's in one of its checkpoints doing what it wants (a place a frame);
    /// past its last, it's completed.
    fn challenge_step(&mut self, level: &Level, ctrl: &Controller) {
        let Some((name, passed)) = self.challenge.clone() else { return };
        let Some(places) = self.tracks.get(&name) else { return };
        let mut through = None;
        for (at, place) in places.iter().enumerate().skip(passed) {
            if let Some(&(k, _)) = place.1.iter().find(|(k, needs)| self.inside[*k] && (needs.is_empty() || needs.iter().any(|n| doing(n, ctrl)))) {
                through = Some((at, k));
                break;
            }
            if !place.0 {
                break;
            }
        }
        let Some((at, k)) = through else { return };
        let done = at + 1 == places.len();
        self.challenge = (!done).then(|| (name.clone(), at + 1));
        let me = level.me_logic[k].name.as_str();
        for e in 0..level.me_logic.len() {
            if level.me_logic[e].op == "SeqEvt_TdTutorialCheckpointCompleted" && level.me_logic[e].param("originator") == Some(me) {
                self.event(level, e, 0);
            }
        }
        if done {
            self.challenge_events(level, "SeqEvt_TdMovementChallengeCompleted", &name);
        }
    }

    /// Input `input` of op `k`.
    fn input(&mut self, level: &Level, k: usize, input: usize) {
        let op = &level.me_logic[k];
        match op.op.as_str() {
            // A matinee: play, reverse, stop, pause, change direction.
            "SeqAct_Interp" => {
                let p = &mut self.plays[k];
                let length = op.movers.iter().map(|&m| level.me_movers[m].length).fold(0.0f32, f32::max);
                match input {
                    0 => {
                        if p.t >= length && op.param("rewind_on_play") == Some("true") {
                            p.t = 0.0;
                        }
                        p.dir = 1.0;
                        p.on = true;
                    }
                    1 => {
                        p.dir = -1.0;
                        p.on = true;
                    }
                    2 => p.on = false,
                    3 => p.on = !p.on,
                    _ => {
                        p.dir = if p.dir < 0.0 { 1.0 } else { -1.0 };
                        p.on = true;
                    }
                }
            }
            // Wait, then go on (stop: don't).
            "SeqAct_Delay" => match input {
                0 => {
                    let t = self.time + op.num("duration", 1.0);
                    for &(o, to, i, d) in &op.outputs {
                        if o == 0 {
                            self.pending.push((t + d, to, i));
                        }
                    }
                }
                _ => {
                    let targets: Vec<(usize, usize)> = op.outputs.iter().filter(|o| o.0 == 0).map(|o| (o.1, o.2)).collect();
                    self.pending.retain(|p| !targets.contains(&(p.1, p.2)));
                }
            },
            // In: through, if open; open, close, toggle.
            "SeqAct_Gate" => match input {
                0 => {
                    if self.open[k] {
                        self.fire(level, k, 0);
                    }
                }
                1 => self.open[k] = true,
                2 => self.open[k] = false,
                _ => self.open[k] = !self.open[k],
            },
            // Each time, the next output (round again if it loops).
            "SeqAct_Switch" => {
                let n = op.num("links", 1.0).max(1.0) as usize;
                let i = self.next[k];
                if i < n {
                    self.fire(level, k, i);
                }
                let step = op.num("increment", 1.0).max(1.0) as usize;
                self.next[k] = if op.param("looping") == Some("true") { (i + step) % n } else { (i + step).min(n) };
            }
            // Each output in turn, a delay apart.
            "SeqAct_DelaySwitch" => {
                let n = op.num("links", 1.0).max(1.0) as usize;
                let gap = op.num("delay", 1.0);
                for i in 0..n {
                    for &(o, to, inp, d) in &op.outputs {
                        if o == i {
                            self.pending.push((self.time + gap * (i + 1) as f32 + d, to, inp));
                        }
                    }
                }
            }
            // One of its outputs, at random (the same way each time she plays it: a hash of
            // when).
            "SeqAct_RandomSwitch" => {
                let n = op.num("links", 1.0).max(1.0) as usize;
                let r = ((self.time * 1000.0) as usize).wrapping_mul(2654435761) % n;
                self.fire(level, k, r);
            }
            // Collision on or off (#111): the bodies it names are bumped into or passed
            // through; the volumes, touched or not.
            "SeqAct_ChangeCollision" => {
                let blocks = op.param("blocks") == Some("true");
                let touches = op.param("touches") == Some("true");
                for &b in &op.bodies {
                    if blocks {
                        self.passable.remove(&b);
                    } else {
                        self.passable.insert(b);
                    }
                }
                for (_, t) in op.params.iter().filter(|(k, _)| k == "target") {
                    if touches {
                        self.untouchable.remove(t);
                    } else {
                        self.untouchable.insert(t.clone());
                    }
                }
                self.fire(level, k, 0);
            }
            // Hide, show, or the other way round (#111).
            "SeqAct_ToggleHidden" => {
                let name = op.param("inputs").and_then(|n| n.split('|').nth(input)).unwrap_or("");
                for &p in &op.props {
                    let was = self.hidden.get(&p).copied().unwrap_or(level.me_props[p].hidden);
                    let hide = match name.to_ascii_lowercase().as_str() {
                        "hide" => true,
                        "unhide" | "show" => false,
                        _ => !was,
                    };
                    self.hidden.insert(p, hide);
                }
                self.hidden_changes += 1;
                self.fire(level, k, 0);
            }
            // A movement challenge starts (#118): its checkpoints are to be gone through from
            // the first.
            // (Where there's no such challenge, a time trial on Training's map: nothing.)
            "SeqAct_TdStartMovementChallenge" => {
                let name = op.param("challenge").unwrap_or("").to_string();
                if self.tracks.contains_key(&name) {
                    self.challenge = Some((name.clone(), 0));
                    self.challenge_events(level, "SeqEvt_TdMovementChallengeStarted", &name);
                }
                self.fire(level, k, 0);
            }
            // Runner vision on or off for the props it names (#118), fading at its speed.
            "SeqAct_ActivateLOI" | "SeqAct_DeactivateLOI" => {
                let on = op.op == "SeqAct_ActivateLOI";
                let speed = op.num("fade", if on { 1.0 } else { 4.0 }).max(0.01);
                for &p in &op.props {
                    let l = self.lit.entry(p).or_insert(Lit { level: 0.0, goal: 0.0, speed, since: self.time });
                    l.goal = if on { 1.0 } else { 0.0 };
                    l.speed = speed;
                    if on {
                        l.since = self.time;
                    }
                }
                self.fire(level, k, 0);
            }
            // Events set off by others: a remote event's call, a sub-sequence's input.
            c if c.starts_with("SeqEvent") || c.starts_with("SeqEvt") => self.event(level, k, 0),
            // Anything else: goes on.
            _ => self.fire(level, k, 0),
        }
    }
}

/// Whether she's doing what a movement challenge's checkpoint wants (#118): one of the game's
/// moves (`MOVE_*`, its TdMove classes, as hers are) or tutorial events (`ETE_*`). What she has
/// no way to do here (show the objectives, look at a point, a fight's disarms and reaction
/// time: Training's fights have no one to fight) counts as done.
fn doing(need: &str, c: &Controller) -> bool {
    use faith_move::State as S;
    let ground = matches!(c.state, S::Ground);
    match need {
        "MOVE_Walking" => ground && !c.crouched,
        "MOVE_Crouch" => ground && c.crouched,
        "MOVE_Jump" => c.jumped() == Some(false),
        "MOVE_WallRunJump" => c.jumped() == Some(true),
        "MOVE_Coil" => c.coiled(),
        "MOVE_Slide" => matches!(c.state, S::Slide { .. }),
        "MOVE_Landing" => ground || matches!(c.state, S::Roll { .. } | S::SoftLand { .. }),
        "MOVE_WallRun" | "MOVE_WallRunningLeft" | "MOVE_WallRunningRight" => matches!(c.state, S::WallRun { .. }),
        "MOVE_IntoClimb" | "MOVE_Climb" => matches!(c.state, S::IntoClimb { .. } | S::Climb { .. } | S::ClimbExit { .. }),
        "MOVE_Grabbing" => matches!(c.state, S::LedgeHang { .. } | S::GrabTransfer { .. }),
        "MOVE_GrabPullUp" => matches!(c.state, S::Traverse(t) if t.kind == faith_move::TraverseKind::PullUp),
        "MOVE_SpringBoarding" => matches!(c.state, S::Traverse(t) if t.kind == faith_move::TraverseKind::SpringBoard),
        "MOVE_VaultOver" | "MOVE_SpeedVaulting" | "ETE_SpeedVault" => matches!(c.state, S::Vault(_)) || matches!(c.state, S::Traverse(t) if t.kind == faith_move::TraverseKind::Vault),
        "MOVE_Swing" => matches!(c.state, S::Swing { .. } | S::SwingJump { .. }),
        "MOVE_ZipLine" => matches!(c.state, S::ZipLine { .. }),
        "MOVE_LedgeWalk" => matches!(c.state, S::Balance { .. }) || ground,
        "ETE_SkillRoll" => matches!(c.state, S::Roll { .. }),
        "ETE_MeleeAttack" | "ETE_SpeedVaultAttack" => c.melee.is_some(),
        "ETE_JumpKickAttack" => c.melee.is_some() && matches!(c.state, S::Air),
        "ETE_SlideKickAttack" => c.melee.is_some() && matches!(c.state, S::Slide { .. }),
        "ETE_LowMeleeAttack" => c.melee.is_some() && c.crouched,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use faith_move::{Controller, Input, State};
    use glam::Vec3;

    use crate::doc::{Kind, LevelDoc, Object};
    use crate::io::Output;
    use crate::level::MoverKey;
    use crate::sim::Sim;

    fn link(event: usize, target: &str, input: usize, delay: f32) -> Output {
        Output { event: event.to_string(), target: target.into(), input: input.to_string(), param: String::new(), delay, times: -1 }
    }

    /// A floor, a mover that rises 2 m over 1 s, and a trigger that plays it (through `between`,
    /// if any) when she walks into it.
    fn level(between: Option<(&str, Vec<(String, String)>)>) -> LevelDoc {
        level_with("SeqEvent_Touch", between)
    }

    /// The same, set off by an event `event` (its region in front of her start).
    fn level_with(event: &str, between: Option<(&str, Vec<(String, String)>)>) -> LevelDoc {
        let mut d = LevelDoc::new("logic");
        d.objects.push(Object::sized(Kind::Block { mat: crate::Mat::Concrete }, Vec3::new(0.0, -0.5, 0.0), Vec3::new(40.0, 1.0, 40.0)));
        d.objects.push(Object::new(Kind::Spawn { gravity: None }, Vec3::new(0.0, 0.0, 6.0)));
        let keys = vec![MoverKey { t: 0.0, offset: Vec3::ZERO, ..Default::default() }, MoverKey { t: 1.0, offset: Vec3::new(0.0, 2.0, 0.0), linear: true, ..Default::default() }];
        let mut m = Object::new(Kind::MeMover { keys, length: 1.0, rate: 1.0, looping: false, carries: vec![] }, Vec3::new(5.0, 0.0, 0.0));
        m.name = "lift".into();
        d.objects.push(m);
        let mut touch = Object::sized(Kind::MeLogic { op: event.into(), params: vec![("max".into(), "1".into())] }, Vec3::new(0.0, 1.0, 0.0), Vec3::new(2.0, 2.0, 2.0));
        touch.name = "touch".into();
        let mut interp = Object::new(Kind::MeLogic { op: "SeqAct_Interp".into(), params: vec![("mover".into(), "lift".into()), ("outputs".into(), "Completed|Reversed".into())] }, Vec3::ZERO);
        interp.name = "interp".into();
        match between {
            Some((op, params)) => {
                let mut b = Object::new(Kind::MeLogic { op: op.into(), params }, Vec3::ZERO);
                b.name = "between".into();
                b.outputs.push(link(0, "interp", 0, 0.0));
                touch.outputs.push(link(0, "between", 0, 0.0));
                d.objects.push(b);
            }
            None => touch.outputs.push(link(0, "interp", 0, 0.0)),
        }
        d.objects.push(touch);
        d.objects.push(interp);
        d
    }

    /// How far the lift's risen after she's walked from her start to the trigger and stood
    /// there `wait` seconds more.
    fn risen(d: &LevelDoc, wait: f32) -> f32 {
        let l = d.build();
        let cp = l.checkpoints[0].clone();
        let mut sim = Sim::new(l);
        let mut c = Controller::new(crate::tuning(5.3, 15.0), cp.spawn, 0.0);
        c.state = State::Ground;
        let dt = 1.0 / 60.0;
        let mut t = 0.0;
        while t < 2.5 + wait {
            let input = if c.feet.z > 0.2 { Input { move_axis: glam::Vec2::new(0.0, 1.0), ..Default::default() } } else { Input::default() };
            sim.step_player(&mut c, &input, dt);
            sim.step(dt, &mut c);
            t += dt;
        }
        sim.me.as_ref().map_or(0.0, |m| m.poses[0].0.y)
    }

    /// A highlight (#118) fades in at its speed; switched off soon after, it holds its least
    /// time on, then fades out at its own speed.
    #[test]
    fn a_highlight_fades_in_and_out_at_its_speeds() {
        let mut d = LevelDoc::new("lit");
        d.objects.push(Object::sized(Kind::Block { mat: crate::Mat::Concrete }, Vec3::new(0.0, -0.5, 0.0), Vec3::new(40.0, 1.0, 40.0)));
        d.objects.push(Object::new(Kind::Spawn { gravity: None }, Vec3::new(0.0, 0.0, 6.0)));
        let mut board = Object::new(Kind::MeProp { package: "P".into(), mesh: "S_Board".into(), scale: Vec3::ONE, bounds: None, solid: false, own_shape: false, materials: vec![], hidden: false }, Vec3::new(3.0, 0.0, 0.0));
        board.name = "P.board".into();
        d.objects.push(board);
        for (name, op, fade) in [("on", "SeqAct_ActivateLOI", "1"), ("off", "SeqAct_DeactivateLOI", "4")] {
            let mut o = Object::new(Kind::MeLogic { op: op.into(), params: vec![("target".into(), "P.board".into()), ("fade".into(), fade.into())] }, Vec3::ZERO);
            o.name = name.into();
            d.objects.push(o);
        }
        let l = d.build();
        let (on, off) = (l.me_logic.iter().position(|o| o.name == "on").unwrap(), l.me_logic.iter().position(|o| o.name == "off").unwrap());
        let p = l.me_logic[on].props[0];
        let mut me = super::MeLogic::new(&l);
        let c = Controller::new(crate::tuning(5.3, 15.0), Vec3::new(0.0, 0.0, 6.0), 0.0);
        let dt = 1.0 / 60.0;
        let run = |me: &mut super::MeLogic, secs: f32| {
            for _ in 0..(secs / dt).round() as usize {
                me.step(&l, &mut [], &c, None, dt);
            }
        };
        run(&mut me, 0.1);
        me.input(&l, on, 0);
        run(&mut me, 0.3);
        let up = me.lit(p);
        assert!((up - 0.3).abs() < 0.03, "{up}");
        me.input(&l, off, 0);
        run(&mut me, 0.15);
        assert!((me.lit(p) - up).abs() < 1e-4, "held {}", me.lit(p));
        run(&mut me, 0.15);
        assert_eq!(me.lit(p), 0.0);
    }

    #[test]
    fn walking_into_a_trigger_plays_its_matinee() {
        let d = level(None);
        let up = risen(&d, 2.0);
        assert!((up - 2.0).abs() < 0.01, "risen {up}");
        // (Its sim mover went with it.)
        let l = d.build();
        assert_eq!(l.me_movers.len(), 1);
        assert_eq!(l.me_logic.len(), 2);
    }

    #[test]
    fn a_closed_gate_stops_it_and_a_delay_holds_it() {
        assert!(risen(&level(Some(("SeqAct_Gate", vec![("open".into(), "false".into())]))), 2.0) < 0.01);
        assert!((risen(&level(Some(("SeqAct_Gate", vec![]))), 2.0) - 2.0).abs() < 0.01);
        // (A 5 s delay: not yet risen 2 s after she's in.)
        assert!(risen(&level(Some(("SeqAct_Delay", vec![("duration".into(), "5".into())]))), 2.0) < 0.01);
        assert!((risen(&level(Some(("SeqAct_Delay", vec![("duration".into(), "1".into())]))), 3.0) - 2.0).abs() < 0.01);
    }

    /// Stood `metres` in front of the event's region (facing it), what's risen after `act`
    /// and two seconds.
    fn risen_after(d: &LevelDoc, metres: f32, act: impl Fn(&mut Sim, &mut Controller, usize)) -> f32 {
        let l = d.build();
        let mut sim = Sim::new(l);
        let mut c = Controller::new(crate::tuning(5.3, 15.0), Vec3::new(0.0, 0.0, 1.0 + metres), 0.0);
        c.state = State::Ground;
        let dt = 1.0 / 60.0;
        for f in 0..180 {
            act(&mut sim, &mut c, f);
            sim.step_player(&mut c, &Input::default(), dt);
            sim.step(dt, &mut c);
        }
        sim.me.as_ref().map_or(0.0, |m| m.poses[0].0.y)
    }

    #[test]
    fn a_use_event_fires_on_use_not_on_touch() {
        let d = level_with("SeqEvent_TdUsed", None);
        // (Walked into: nothing.)
        assert!(risen(&d, 2.0) < 0.01);
        // (Used, a metre from it, looking at it: up it goes.)
        let up = risen_after(&d, 1.0, |sim, c, f| {
            if f == 30 {
                sim.use_key(crate::sim::Aim::of(c));
            }
        });
        assert!((up - 2.0).abs() < 0.01, "risen {up}");
    }

    #[test]
    fn a_kick_from_standing_sets_off_a_door() {
        let d = level_with("SeqEvent_TakeDamage", None);
        let up = risen_after(&d, 0.5, |sim, c, f| {
            if f == 30 {
                let input = Input { melee_pressed: true, ..Default::default() };
                sim.step_player(c, &input, 1.0 / 60.0);
            }
        });
        assert!((up - 2.0).abs() < 0.01, "risen {up}");
    }

    #[test]
    fn a_lift_goes_back_down_once_it_s_been_open() {
        // (Standing on it plays it up; "Open Finished" reverses it.)
        let mut d = level_with("SeqEvent_Mover", None);
        for o in d.objects.iter_mut() {
            if let Kind::MeLogic { op, params } = &mut o.kind {
                if op == "SeqEvent_Mover" {
                    params.push(("outputs".into(), "Pawn Attached|Pawn Detached|Open Finished|Hit Actor".into()));
                    params.push(("stay_open".into(), "1".into()));
                    params[0].1 = "0".into();
                    o.outputs.push(link(2, "interp", 1, 0.0));
                }
            }
        }
        let mut heights = vec![];
        let l = d.build();
        let mut sim = Sim::new(l);
        let mut c = Controller::new(crate::tuning(5.3, 15.0), Vec3::new(0.0, 0.0, 0.5), 0.0);
        c.state = State::Ground;
        for f in 0..300 {
            sim.step_player(&mut c, &Input::default(), 1.0 / 60.0);
            sim.step(1.0 / 60.0, &mut c);
            if f % 30 == 29 {
                heights.push((sim.me.as_ref().unwrap().poses[0].0.y * 100.0).round() / 100.0);
            }
        }
        // Up in a second, stays a second, back down in a second.
        assert!(heights[1] > 1.9 && heights.last().unwrap().abs() < 0.05, "{heights:?}");
    }
}
