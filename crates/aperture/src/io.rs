//! Portal's map logic: Source's entity I/O, run alongside the sim.
//!
//! An entity fires an output (`OnTrigger`, `OnPressed`...) and each of its connections sends
//! an input to the entities with a name, after a delay: a relay passes it on, a door opens, a
//! timer starts. Here the entities that matter to play are the ones kept: relays, timers,
//! counters, cases, triggers (touched by Faith or cubes), the buttons' triggers (pressed via
//! the sim's own buttons), doors and other moving brushes (the sim's movers), grills,
//! preset portals, cube makers, choreographed scenes (as a short wait: GLaDOS isn't heard
//! yet) and level changes. Everything else is ignored.

use std::collections::HashMap;

use glam::Vec3;

use faith_move::{Aabb, Controller};

use crate::level::{CubeKind, Sig};
use crate::portal::{Portal, PortalColor};
use crate::sim::{MapTransition, Sim, SimEvent};

/// How long a GLaDOS scene is taken to run (its lines aren't played).
const SCENE_SECONDS: f32 = 3.0;

/// One connection: when the entity fires `event`, send `input` (with `param`) to `target`
/// after `delay` seconds, at most `times` times (-1: always).
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub event: String,
    pub target: String,
    pub input: String,
    pub param: String,
    pub delay: f32,
    pub times: i32,
}

impl Output {
    /// From an entity key (`OnTrigger`) and its value (`target,Input,param,delay,times`).
    pub fn parse(event: &str, value: &str) -> Option<Output> {
        let f: Vec<&str> = value.split([',', '\x1b']).collect();
        if f.len() < 2 {
            return None;
        }
        Some(Output {
            event: event.to_string(),
            target: f[0].trim().to_ascii_lowercase(),
            input: f[1].trim().to_string(),
            param: f.get(2).map(|s| s.trim().to_string()).unwrap_or_default(),
            delay: f.get(3).and_then(|s| s.trim().parse().ok()).unwrap_or(0.0),
            times: f.get(4).and_then(|s| s.trim().parse().ok()).unwrap_or(-1),
        })
    }
}

/// Which way a mover is driven.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoverKind {
    /// Open (to the end of its path) and close (back to the start).
    Door,
    /// Rides along path nodes: start, stop, reverse.
    Train,
    /// There or not (enable, disable).
    Brush,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Relay { enabled: bool, once: bool },
    Timer { enabled: bool, refire: f32, next: f32, random: Option<(f32, f32)> },
    Counter { value: f32, min: f32, max: f32 },
    Case { cases: Vec<String> },
    /// Touched by Faith (`players`), cubes (`cubes`) or high energy pellets (`pellets`: Portal's
    /// catchers, which take the pellet); once: then it's done.
    Trigger { b: Aabb, once: bool, enabled: bool, players: bool, cubes: bool, pellets: bool, player_in: bool, cubes_in: Vec<u32> },
    /// A floor button's trigger: the sim's button `button` pressing it.
    ButtonTrigger { button: usize, enabled: bool, players: bool, cubes: bool, on: bool },
    Mover { index: usize, kind: MoverKind },
    /// Something switched by a signal: a grill, a pellet launcher (on: working).
    Switch { sig: Sig },
    /// A preset portal (`start`: open as the map starts).
    Portal { color: PortalColor, portal: Portal, start: bool },
    /// A choreographed scene (GLaDOS speaking): its lines (start, sound) and length; with none
    /// known, a short wait.
    Scene { lines: Vec<(f32, String)>, length: f32 },
    /// Spawns a cube here (env_entity_maker, point_template).
    Maker { at: Vec3, kind: CubeKind },
    /// A train's path point (`node` on mover `mover`).
    PathNode { mover: usize, node: usize },
    /// Fires its outputs as the map starts.
    Auto,
    /// Finishes the level and names the next map and the landmark the two maps share.
    ChangeLevel { map: String, landmark: String },
    /// A model that animates (prop_dynamic): see [`Io::animations`].
    Animated,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ent {
    pub name: String,
    pub class: String,
    pub kind: Kind,
    pub outputs: Vec<Output>,
    pub alive: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Pending {
    at: f32,
    target: String,
    input: String,
    param: String,
    caller: usize,
}

/// The map's logic.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Io {
    pub ents: Vec<Ent>,
    by_name: HashMap<String, Vec<usize>>,
    queue: Vec<Pending>,
    time: f32,
    started: bool,
    /// Signals to set as the level starts.
    pub initial: Vec<(Sig, bool)>,
    signals: usize,
    /// Each animated prop's (entity's) last animation, and a count that goes up each time
    /// one's started (so the host can tell a replay).
    pub animations: HashMap<usize, (String, u32)>,
    animation_count: u32,
    /// GLaDOS's lines as she says them: a count that goes up with each, and the sound.
    pub voice: Vec<(u32, String)>,
    /// Which preset portal (entity) is open in each colour.
    open: [Option<usize>; 2],
    rng: u32,
}

impl Io {
    pub fn new(ents: Vec<Ent>, initial: Vec<(Sig, bool)>, signals: usize) -> Io {
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, e) in ents.iter().enumerate() {
            if !e.name.is_empty() {
                by_name.entry(e.name.to_ascii_lowercase()).or_default().push(i);
            }
        }
        Io { ents, by_name, initial, signals, rng: 0x1234_5678, ..Default::default() }
    }

    pub fn signal_count(&self) -> usize {
        self.signals
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Fire entity `e`'s output `event` (`value` stands in for empty parameters: a counter's
    /// value, passed on).
    fn fire(&mut self, e: usize, event: &str, value: Option<&str>) {
        let now = self.time;
        let mut sends = Vec::new();
        for o in self.ents[e].outputs.iter_mut() {
            if !o.event.eq_ignore_ascii_case(event) || o.times == 0 {
                continue;
            }
            if o.times > 0 {
                o.times -= 1;
            }
            let param = if o.param.is_empty() { value.unwrap_or("").to_string() } else { o.param.clone() };
            sends.push(Pending { at: now + o.delay, target: o.target.clone(), input: o.input.clone(), param, caller: e });
        }
        self.queue.extend(sends);
    }

    /// One frame of logic (from [`Sim::step`]).
    pub fn step(&mut self, sim: &mut Sim, ctrl: &Controller, dt: f32) {
        sim.player_feet = ctrl.feet;
        sim.player_yaw = ctrl.yaw;
        if !self.started {
            self.started = true;
            for &(s, on) in &self.initial {
                if let Some(v) = sim.io_signals.get_mut(s) {
                    *v = on;
                }
            }
            // Source fires OnMapSpawn whenever a map loads, OnNewGame only for a fresh start,
            // and OnMapTransition only when arriving from the previous map. The arrival
            // elevator's doors open on that last one.
            let arrived = sim.arrived;
            for e in 0..self.ents.len() {
                if self.ents[e].kind == Kind::Auto {
                    self.fire(e, "OnMapSpawn", None);
                    if !arrived {
                        self.fire(e, "OnNewGame", None);
                    }
                    if arrived {
                        self.fire(e, "OnMapTransition", None);
                    }
                }
            }
            for e in 0..self.ents.len() {
                if let Kind::Portal { start: true, .. } = self.ents[e].kind {
                    self.input(sim, e, "SetActivatedState", "1", e);
                }
            }
        }
        self.time += dt;

        // What the sim did: movers at their path points, buttons pressed.
        let events: Vec<SimEvent> = sim.events.clone();
        for ev in events {
            if let SimEvent::MoverAt { mover, node, arrived } = ev {
                for e in 0..self.ents.len() {
                    match self.ents[e].kind {
                        Kind::PathNode { mover: m, node: n } if m == mover && n == node => self.fire(e, "OnPass", None),
                        Kind::Mover { index, kind: MoverKind::Door } if index == mover && arrived => {
                            let last = sim.movers[index].def.path.len() - 1;
                            self.fire(e, if node == last { "OnFullyOpen" } else { "OnFullyClosed" }, None);
                        }
                        _ => {}
                    }
                }
            }
        }
        for e in 0..self.ents.len() {
            if let Kind::ButtonTrigger { button, enabled, players, cubes, on } = self.ents[e].kind {
                let Some(b) = sim.buttons.get(button) else { continue };
                let now = enabled && b.pressed && if b.by_cube { cubes } else { players };
                if now != on {
                    if let Kind::ButtonTrigger { on, .. } = &mut self.ents[e].kind {
                        *on = now;
                    }
                    if now {
                        self.fire(e, "OnStartTouch", None);
                        self.fire(e, "OnTrigger", None);
                    } else {
                        self.fire(e, "OnEndTouch", None);
                        self.fire(e, "OnEndTouchAll", None);
                    }
                }
            }
        }

        // Triggers.
        let me = ctrl.body().aabb(ctrl.feet);
        let cubes: Vec<(u32, Aabb)> = sim.props.iter().filter(|p| p.alive && p.fizzle.is_none()).map(|p| (p.id, p.aabb())).collect();
        for e in 0..self.ents.len() {
            if !self.ents[e].alive {
                continue;
            }
            let Kind::Trigger { b, once, enabled, players, cubes: takes_cubes, pellets, player_in, ref cubes_in } = self.ents[e].kind else { continue };
            if !enabled {
                continue;
            }
            if pellets {
                let inside: Vec<u32> = sim.pellets.iter().filter(|p| p.alive && b.overlaps(&Aabb::from_center_size(p.pos, Vec3::splat(0.4)))).map(|p| p.id).collect();
                for id in inside {
                    sim.remove_pellet(id, true);
                    self.fire(e, "OnStartTouch", None);
                    self.fire(e, "OnTrigger", None);
                    if once {
                        self.ents[e].alive = false;
                        break;
                    }
                }
                continue;
            }
            let player_now = players && b.overlaps(&me);
            let cubes_now: Vec<u32> = if takes_cubes { cubes.iter().filter(|(_, c)| b.overlaps(c)).map(|(id, _)| *id).collect() } else { Vec::new() };
            let entered = (player_now && !player_in) || cubes_now.iter().any(|id| !cubes_in.contains(id));
            let left = (!player_now && player_in) || cubes_in.iter().any(|id| !cubes_now.contains(id));
            let was_any = player_in || !cubes_in.is_empty();
            let any = player_now || !cubes_now.is_empty();
            if let Kind::Trigger { player_in, cubes_in, .. } = &mut self.ents[e].kind {
                *player_in = player_now;
                *cubes_in = cubes_now;
            }
            if entered {
                self.fire(e, "OnStartTouch", None);
                self.fire(e, "OnTrigger", None);
                if !was_any {
                    self.fire(e, "OnStartTouchAll", None);
                }
                if once {
                    self.ents[e].alive = false;
                    continue;
                }
            }
            if left {
                self.fire(e, "OnEndTouch", None);
                if !any {
                    self.fire(e, "OnEndTouchAll", None);
                }
            }
        }

        // Timers.
        let now = self.time;
        for e in 0..self.ents.len() {
            if !self.ents[e].alive {
                continue;
            }
            if let Kind::Timer { enabled: true, next, .. } = self.ents[e].kind {
                if now >= next {
                    let wait = self.timer_wait(e);
                    if let Kind::Timer { next, .. } = &mut self.ents[e].kind {
                        *next = now + wait;
                    }
                    self.fire(e, "OnTimer", None);
                }
            }
        }

        // What's due. (Inputs can queue more for now: run until nothing's due, a bounded
        // number of times.)
        for _ in 0..64 {
            let due: Vec<Pending> = {
                let (due, later): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut self.queue).into_iter().partition(|p| p.at <= self.time);
                self.queue = later;
                due
            };
            if due.is_empty() {
                break;
            }
            for p in due {
                let targets: Vec<usize> = match p.target.as_str() {
                    "!self" | "!caller" => vec![p.caller],
                    t => self.by_name.get(t).cloned().unwrap_or_default(),
                };
                for t in targets {
                    self.input(sim, t, &p.input, &p.param, p.caller);
                }
            }
        }
    }

    fn timer_wait(&mut self, e: usize) -> f32 {
        match self.ents[e].kind {
            Kind::Timer { random: Some((lo, hi)), .. } => {
                let r = self.rand();
                lo + (hi - lo) * r
            }
            Kind::Timer { refire, .. } => refire.max(0.01),
            _ => 1.0,
        }
    }

    /// Send `input` to entity `e`.
    fn input(&mut self, sim: &mut Sim, e: usize, input: &str, param: &str, caller: usize) {
        let i = input.to_ascii_lowercase();
        if !self.ents[e].alive && i != "enable" {
            return;
        }
        if i == "kill" {
            self.ents[e].alive = false;
            if let Kind::Mover { index, .. } = self.ents[e].kind {
                sim.movers[index].present = false;
            }
            return;
        }
        let flag = |p: &str| p.trim().parse::<f32>().map(|v| v != 0.0).unwrap_or(false);
        let num = |p: &str| p.split_whitespace().next().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let now = self.time;
        match self.ents[e].kind.clone() {
            Kind::Relay { enabled, once } => match i.as_str() {
                "trigger" if enabled => {
                    if once {
                        self.ents[e].alive = false;
                    }
                    self.fire(e, "OnTrigger", None);
                }
                "enable" => self.set_enabled(e, true),
                "disable" => self.set_enabled(e, false),
                "toggle" => self.set_enabled(e, !enabled),
                "cancelpending" => self.queue.retain(|p| p.caller != e),
                _ => {}
            },
            Kind::Timer { enabled, .. } => match i.as_str() {
                "enable" | "resettimer" if i != "resettimer" || enabled => {
                    let wait = self.timer_wait(e);
                    if let Kind::Timer { enabled, next, .. } = &mut self.ents[e].kind {
                        *enabled = true;
                        *next = now + wait;
                    }
                }
                "disable" => self.set_enabled(e, false),
                "toggle" => {
                    if enabled {
                        self.set_enabled(e, false)
                    } else {
                        self.input(sim, e, "Enable", "", caller)
                    }
                }
                "refiretime" => {
                    if let Kind::Timer { refire, next, .. } = &mut self.ents[e].kind {
                        *refire = num(param);
                        *next = now + *refire;
                    }
                }
                "firetimer" => self.fire(e, "OnTimer", None),
                _ => {}
            },
            Kind::Counter { value, min, max } => {
                let v = match i.as_str() {
                    "add" => value + num(param),
                    "subtract" => value - num(param),
                    "setvalue" | "setvaluenofire" => num(param),
                    "sethitmax" => {
                        if let Kind::Counter { max, .. } = &mut self.ents[e].kind {
                            *max = num(param);
                        }
                        return;
                    }
                    "sethitmin" => {
                        if let Kind::Counter { min, .. } = &mut self.ents[e].kind {
                            *min = num(param);
                        }
                        return;
                    }
                    "getvalue" => {
                        self.fire(e, "OnGetValue", Some(&fmt_num(value)));
                        return;
                    }
                    _ => return,
                };
                let bounded = max > min;
                let v = if bounded { v.clamp(min, max) } else { v };
                if let Kind::Counter { value, .. } = &mut self.ents[e].kind {
                    *value = v;
                }
                if i != "setvaluenofire" {
                    self.fire(e, "OutValue", Some(&fmt_num(v)));
                }
                if bounded && v >= max && value < max {
                    self.fire(e, "OnHitMax", None);
                }
                if bounded && v <= min && value > min {
                    self.fire(e, "OnHitMin", None);
                }
            }
            Kind::Case { cases } => match i.as_str() {
                "invalue" => {
                    let p = param.trim();
                    let hit = cases.iter().position(|c| !c.is_empty() && (c == p || (num(c) == num(p) && p.parse::<f32>().is_ok() && c.parse::<f32>().is_ok())));
                    match hit {
                        Some(k) => self.fire(e, &format!("OnCase{:02}", k + 1), None),
                        None => self.fire(e, "OnDefault", None),
                    }
                }
                "pickrandom" | "pickrandomshuffle" => {
                    let n = cases.iter().filter(|c| !c.is_empty()).count().max(1);
                    let k = ((self.rand() * n as f32) as usize).min(n - 1);
                    self.fire(e, &format!("OnCase{:02}", k + 1), None);
                }
                _ => {}
            },
            Kind::Trigger { .. } | Kind::ButtonTrigger { .. } => match i.as_str() {
                "enable" => self.set_enabled(e, true),
                "disable" => self.set_enabled(e, false),
                "toggle" => {
                    let on = matches!(self.ents[e].kind, Kind::Trigger { enabled: true, .. } | Kind::ButtonTrigger { enabled: true, .. });
                    self.set_enabled(e, !on)
                }
                _ => {}
            },
            Kind::Mover { index, kind } => {
                let m = &mut sim.movers[index];
                let last = m.def.path.len().saturating_sub(1);
                match (kind, i.as_str()) {
                    (MoverKind::Door, "open") => {
                        if m.node != last || m.goal.is_some() {
                            m.goal = Some(last);
                            self.fire(e, "OnOpen", None);
                        }
                    }
                    (MoverKind::Door, "close") => {
                        if m.node != 0 || m.goal.is_some() {
                            m.goal = Some(0);
                            self.fire(e, "OnClose", None);
                        }
                    }
                    (MoverKind::Door, "toggle") => {
                        let open = m.goal.map_or(m.node == last, |g| g == last);
                        self.input(sim, e, if open { "Close" } else { "Open" }, "", caller);
                    }
                    (MoverKind::Train, "startforward" | "resume" | "toggle" | "startbackward" | "reverse") if m.speed == 0.0 => {
                        m.speed = m.def.speed;
                        m.goal = Some(if i == "startbackward" || i == "reverse" { 0 } else { last });
                    }
                    (MoverKind::Train, "startforward" | "resume" | "toggle") => m.goal = Some(last),
                    (MoverKind::Train, "startbackward" | "reverse") => m.goal = Some(0),
                    (MoverKind::Train, "stop") => m.goal = None,
                    // A fraction of its top speed, and off it goes (0: it stops).
                    (MoverKind::Train, "setspeed" | "setspeeddir") => {
                        let f = num(param);
                        m.speed = m.def.speed * f.abs();
                        m.goal = if f == 0.0 { None } else if f > 0.0 { Some(last) } else { Some(0) };
                    }
                    (MoverKind::Train, "setspeedreal") => {
                        m.speed = num(param).abs() * 0.0254;
                        m.goal = if m.speed == 0.0 { None } else { Some(last) };
                    }
                    (_, "enable" | "turnon" | "enablecollision") => m.present = true,
                    (_, "disable" | "turnoff" | "disablecollision") => m.present = false,
                    (MoverKind::Brush, "toggle") => m.present = !m.present,
                    // A rotating door can't turn here: it gets out of the way.
                    (MoverKind::Brush, "open") => m.present = false,
                    (MoverKind::Brush, "close") => m.present = true,
                    _ => {}
                }
            }
            Kind::Switch { sig } => {
                let on = match i.as_str() {
                    "enable" => Some(true),
                    "disable" => Some(false),
                    "toggle" => sim.io_signals.get(sig).map(|v| !v),
                    _ => None,
                };
                if let (Some(on), Some(v)) = (on, sim.io_signals.get_mut(sig)) {
                    *v = on;
                }
            }
            Kind::Portal { color, portal, .. } => {
                let c = color.index();
                match i.as_str() {
                    "setactivatedstate" if flag(param) => {
                        self.open[c] = Some(e);
                        sim.set_portal(color, Some(Portal { age: 0.0, ..portal }));
                    }
                    "setactivatedstate" | "fizzle" if self.open[c] == Some(e) => {
                        self.open[c] = None;
                        sim.set_portal(color, None);
                    }
                    _ => {}
                }
            }
            Kind::Scene { lines, length } => match i.as_str() {
                "start" => {
                    self.fire(e, "OnStart", None);
                    let length = if length > 0.0 { length } else { SCENE_SECONDS };
                    self.queue.push(Pending { at: now + length, target: "!self".into(), input: "__done".into(), param: String::new(), caller: e });
                    for (at, sound) in lines {
                        self.queue.push(Pending { at: now + at, target: "!self".into(), input: "__say".into(), param: sound, caller: e });
                    }
                }
                "__say" => {
                    let n = self.voice.last().map_or(1, |v| v.0 + 1);
                    self.voice.push((n, param.to_string()));
                    // (Only the last few are kept: the host plays them as they come.)
                    if self.voice.len() > 16 {
                        self.voice.remove(0);
                    }
                }
                "__done" => self.fire(e, "OnCompletion", None),
                "cancel" => self.queue.retain(|p| !(p.caller == e && (p.input == "__done" || p.input == "__say"))),
                _ => {}
            },
            Kind::Maker { at, kind } if i == "forcespawn" => {
                // Not on top of a cube that's still there.
                if !sim.props.iter().any(|p| p.alive && p.pos.distance(at) < 0.5) {
                    sim.spawn_prop(kind, at, None);
                    sim.events.push(SimEvent::CubeDropped { at });
                }
            }
            Kind::Animated if i == "setanimation" => {
                self.animation_count += 1;
                self.animations.insert(e, (param.trim().to_ascii_lowercase(), self.animation_count));
            }
            Kind::ChangeLevel { map, landmark } if i == "changelevel" => {
                if !sim.complete {
                    sim.complete = true;
                    let offset = sim.level.landmarks.iter().find(|(n, _)| n.eq_ignore_ascii_case(&landmark)).map(|(_, p)| sim.player_feet - *p).unwrap_or(Vec3::ZERO);
                    sim.transition = Some(MapTransition { map, landmark, offset, yaw: sim.player_yaw });
                    sim.events.push(SimEvent::LevelComplete);
                }
            }
            _ => {}
        }
    }

    fn set_enabled(&mut self, e: usize, on: bool) {
        match &mut self.ents[e].kind {
            Kind::Relay { enabled, .. } | Kind::Timer { enabled, .. } | Kind::ButtonTrigger { enabled, .. } => *enabled = on,
            Kind::Trigger { enabled, player_in, cubes_in, .. } => {
                *enabled = on;
                if !on {
                    *player_in = false;
                    cubes_in.clear();
                }
            }
            _ => {}
        }
        if on {
            self.ents[e].alive = true;
        }
    }
}

fn fmt_num(v: f32) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{v}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outputs_parse() {
        let o = Output::parse("OnTrigger", "door_left_00,Open,,0.5,1").unwrap();
        assert_eq!((o.target.as_str(), o.input.as_str(), o.delay, o.times), ("door_left_00", "Open", 0.5, 1));
        let o = Output::parse("OnTimer", "intro_hudhint_chooser,Add,1,0,-1").unwrap();
        assert_eq!(o.param, "1");
    }
}
