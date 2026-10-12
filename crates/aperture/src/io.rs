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
use crate::geom;
use crate::sim::{MapTransition, Sim, SimEvent, ROCKET_BLAST};

/// She picks up a gun her middle comes this close to.
const GUN_REACH: f32 = 1.5;

/// How long a GLaDOS scene is taken to run (its lines aren't played).
const SCENE_SECONDS: f32 = 3.0;

/// One connection: when the entity fires `event`, send `input` (with `param`) to `target`
/// after `delay` seconds, at most `times` times (-1: always).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Output {
    pub event: String,
    pub target: String,
    pub input: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub param: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub delay: f32,
    #[serde(default = "always", skip_serializing_if = "is_always")]
    pub times: i32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

fn always() -> i32 {
    -1
}

fn is_always(v: &i32) -> bool {
    *v == -1
}

impl Output {
    /// As an entity key's value has it: `target,Input,param,delay,times`.
    pub fn value(&self) -> String {
        format!("{},{},{},{},{}", self.target, self.input, self.param, self.delay, self.times)
    }

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
    /// A fan: spinning or not (start, stop).
    Rotor,
}

/// A sound playing (an ambient_generic's): whether it is, how many times it's been started
/// (the host starts it again when this goes up), its volume (0-1) and pitch (1: as recorded).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundState {
    pub playing: bool,
    pub starts: u32,
    pub volume: f32,
    pub pitch: f32,
}

/// A shake of the view (env_shake): `n` goes up with each; `amplitude` 0 stops one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shake {
    pub n: u32,
    pub at: Vec3,
    pub amplitude: f32,
    pub frequency: f32,
    pub duration: f32,
    /// How far it's felt (metres; infinite: everywhere).
    pub radius: f32,
}

/// A fade of the screen (env_fade): to `color` at `alpha` over `duration`, held `hold`, then
/// back (unless it `stays`); `from`: from the colour to clear instead.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub n: u32,
    pub color: Vec3,
    pub alpha: f32,
    pub duration: f32,
    pub hold: f32,
    pub from: bool,
    pub stays: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Relay { enabled: bool, once: bool },
    Timer { enabled: bool, refire: f32, next: f32, random: Option<(f32, f32)> },
    Counter { value: f32, min: f32, max: f32 },
    Case { cases: Vec<String> },
    /// Touched by Faith (`players`), cubes (`cubes`), high energy pellets (`pellets`: Portal's
    /// catchers, which take the pellet) or rockets (`rockets`: flying in, or going off in it);
    /// once: then it's done. Parented to a mover (`mover`), it rides along with it (a lift's).
    Trigger { b: Aabb, once: bool, enabled: bool, players: bool, cubes: bool, pellets: bool, rockets: bool, mover: Option<usize>, player_in: bool, cubes_in: Vec<u32> },
    /// A floor button's moving top (a func_door in the map; the sim's button `button` stands in
    /// for it): it opens as the button goes down and closes as it comes up.
    ButtonDoor { button: usize, down: bool },
    /// A floor button's trigger: the sim's button `button` pressing it.
    ButtonTrigger { button: usize, enabled: bool, players: bool, cubes: bool, on: bool },
    Mover { index: usize, kind: MoverKind },
    /// Something switched by a signal: a grill (on: working).
    Switch { sig: Sig },
    /// A pellet launcher, switched by a signal like a grill; the sim's launcher `index`. It
    /// says when it's fired (OnPostSpawnBall).
    Launcher { sig: Sig, index: usize },
    /// A preset portal (`start`: open as the map starts).
    Portal { color: PortalColor, portal: Portal, start: bool },
    /// A choreographed scene (GLaDOS speaking): its lines (start, sound), length and the
    /// outputs it fires as it goes (start, n: `OnTriggerN`); with none known, a short wait.
    Scene { lines: Vec<(f32, String)>, length: f32, triggers: Vec<(f32, u32)> },
    /// Spawns a cube here (env_entity_maker, point_template).
    /// `trains`: the trains (and the brushes riding them) its template makes, as movers: a
    /// spawn starts them from the start of their track, when they're not already out on it.
    Maker { cubes: Vec<(Vec3, CubeKind, usize)>, trains: Vec<usize> },
    /// A train's path point (`node` on mover `mover`).
    /// A train's path point (`mover`'s `node`), and the other trains' it's on the path of
    /// (Chamber 15's carts share one rail).
    PathNode { mover: usize, node: usize, others: Vec<(usize, usize)> },
    /// Fires its outputs as the map starts.
    Auto,
    /// Finishes the level and names the next map and the landmark the two maps share: told to
    /// (ChangeLevel), or walked into (`touch`, once she's been outside it: not the one she
    /// arrives in, which leads back).
    ChangeLevel { map: String, landmark: String, touch: Option<Aabb>, armed: bool },
    /// A model that animates (prop_dynamic): see [`Io::animations`] and [`Io::skins`].
    Animated,
    /// A glow sprite (env_sprite), shown or hidden: see [`Io::hidden`].
    Sprite,
    /// A portal gun to pick up: the sim's pickup `index`.
    GunPickup { index: usize },
    /// Picks which frame of its brush's texture shows (env_texturetoggle: a door's state
    /// panel, X or check): see [`Io::frames`].
    TextureToggle { target: String },
    /// Runs its parent brush's texture frame (material_modify_control on `$frame`: Chamber
    /// 00's clock's hundredths). `lerp`: from, to, over (seconds), looping, since when.
    FrameLerp { parent: String, lerp: Option<(f32, f32, f32, bool, f32)> },
    /// A sound (ambient_generic): what it plays (a soundscript or a file), where (riding
    /// `mover`), how loud and at what pitch to start with, how far it carries (metres; 0:
    /// as its soundscript says), heard everywhere, and whether it loops: see [`Io::sounds`].
    Sound { at: Vec3, sound: String, volume: f32, pitch: f32, reach: f32, everywhere: bool, looped: bool, mover: Option<usize> },
    /// Where an area's ambience is (env_soundscape): heard within `radius` (metres; infinite:
    /// anywhere), while it's on.
    Soundscape { at: Vec3, radius: f32, scape: String, enabled: bool },
    /// Shakes the view (env_shake): see [`Io::shakes`].
    Shake { at: Vec3, amplitude: f32, frequency: f32, duration: f32, radius: f32 },
    /// Fades the screen (env_fade): see [`Io::fades`].
    Fade { color: Vec3, alpha: f32, duration: f32, hold: f32, from: bool, stays: bool },
    /// Colour correction (color_correction): a table (`file`, its name without the folder:
    /// `cc_testchamber.raw`) weighed in fully within `near` metres, fading out by `far`
    /// (infinite: anywhere), at most `weight`, while it's on; it fades in and out over
    /// `fade_in` and `fade_out` seconds.
    Correction { at: Vec3, near: f32, far: f32, weight: f32, file: String, enabled: bool, fade_in: f32, fade_out: f32 },
    /// One of the sim's switched regions (a push, a hurt, a no-portal volume, a portal bumper
    /// or detector).
    Volume { index: usize },
    /// Something shown or not, as a sprite is (a spotlight's beam, steam, sparks): see
    /// [`Io::hidden`].
    Effect,
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
    /// What set it off, for `!activator` (the train passing a path point).
    activator: Option<usize>,
    /// Or the cube (id) that did (into a trigger).
    cube: Option<u32>,
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
    /// Animated props and sprites (entities) that aren't shown: they start disabled (or off), or
    /// the logic has disabled (hidden) them: a catcher's ball and its halo, until it's caught.
    pub hidden: std::collections::HashSet<usize>,
    /// Each animated prop's (entity's) skin, where the logic has changed it.
    pub skins: HashMap<usize, u32>,
    /// The texture frame each brush model shows, where the logic has picked one.
    pub frames: HashMap<usize, u32>,
    /// And by the name the logic switched (lower case), for what isn't a brush: overlays.
    pub named_frames: HashMap<String, u32>,
    /// GLaDOS's lines as she says them: a count that goes up with each, and the sound.
    pub voice: Vec<(u32, String)>,
    /// Movers she can use (E) to open (a door flagged Use Opens: Portal's pedestal switches),
    /// and whether using it toggles it (flagged Toggle).
    pub usable: HashMap<usize, bool>,
    /// Train path points that start disabled (mover, point): the train stops short of them till
    /// they're enabled (EnablePath).
    pub closed_nodes: Vec<(usize, usize)>,
    /// What a rocket going off hurts (fires its OnTakeDamage): entity, centre and radius.
    pub damageable: Vec<(usize, Vec3, f32)>,
    /// Cube droppers' covers (entity) and the hatch (mover) each holds shut: open, it's gone.
    pub hatches: HashMap<usize, usize>,
    /// Each grill's (fizzler's) entity: it fires OnDissolve as it takes a cube.
    pub cleansers: Vec<usize>,
    /// The entity each cube the map puts down is (in order), and each cube's (by id): its
    /// outputs are the cube's (OnOutOfWorld, as it's lost).
    pub cube_ents: Vec<usize>,
    owners: HashMap<u32, usize>,
    /// Which preset portal (entity) is open in each colour.
    open: [Option<usize>; 2],
    rng: u32,
    /// Each sound's (entity's) state.
    pub sounds: HashMap<usize, SoundState>,
    /// The view's shakes, the last few (the host starts each new one).
    pub shakes: Vec<Shake>,
    /// The screen's fades, the last few.
    pub fades: Vec<Fade>,
    /// The sounds (entities) and whether each starts playing.
    pub initial_sounds: Vec<(usize, bool)>,
    /// What's been knocked down (a security camera a portal's opened under: Ragdoll).
    pub knocked: std::collections::HashSet<usize>,
    /// Each tonemap setting the logic's changed (env_tonemap_controller): its input (lower
    /// case: `setautoexposuremin`, `setbloomscale`...) and value.
    pub tonemap: HashMap<String, f32>,
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

    /// Sends an entity an input at once, as an output would (for tools: the scripted runs).
    pub fn send(&mut self, sim: &mut Sim, e: usize, input: &str, param: &str) {
        self.input(sim, e, input, param, e);
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
        self.fire_by(e, event, value, None);
    }

    /// Fire, with what set it off (for its outputs' `!activator`).
    fn fire_by(&mut self, e: usize, event: &str, value: Option<&str>, activator: Option<usize>) {
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
            sends.push(Pending { activator, cube: None, at: now + o.delay, target: o.target.clone(), input: o.input.clone(), param, caller: e });
        }
        self.queue.extend(sends);
    }

    /// Fire, set off by cube `id` (for its outputs' `!activator`).
    fn fire_cube(&mut self, e: usize, event: &str, id: u32) {
        let n = self.queue.len();
        self.fire(e, event, None);
        for p in &mut self.queue[n..] {
            p.cube = Some(id);
        }
    }

    /// One frame of logic (from [`Sim::step`]).
    /// Show frame `n` of the texture on the brushes named `name`.
    fn set_frame(&mut self, sim: &Sim, name: &str, n: u32) {
        for t in self.by_name.get(name).cloned().unwrap_or_default() {
            if let Kind::Mover { index, .. } = self.ents[t].kind {
                self.frames.insert(sim.movers[index].def.model, n);
            }
        }
    }

    pub fn step(&mut self, sim: &mut Sim, ctrl: &Controller, dt: f32) {
        sim.player_feet = ctrl.feet;
        sim.player_yaw = ctrl.yaw;
        if !self.started {
            self.started = true;
            for (k, p) in sim.props.iter().enumerate() {
                if let Some(&e) = self.cube_ents.get(k) {
                    self.owners.insert(p.id, e);
                }
            }
            for &(s, on) in &self.initial {
                if let Some(v) = sim.io_signals.get_mut(s) {
                    *v = on;
                }
            }
            for &(m, n) in &self.closed_nodes {
                if let Some(c) = sim.movers.get_mut(m).and_then(|m| m.closed.get_mut(n)) {
                    *c = true;
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
            // Sounds that don't start silent start (`initial_sounds`).
            for (e, on) in std::mem::take(&mut self.initial_sounds) {
                if let Kind::Sound { volume, pitch, .. } = self.ents[e].kind {
                    self.sounds.insert(e, SoundState { playing: on, starts: u32::from(on), volume, pitch });
                }
            }
        }
        self.time += dt;

        // What the sim did: movers at their path points, buttons pressed.
        let events: Vec<SimEvent> = sim.events.clone();
        for ev in events {
            if let SimEvent::PelletLaunched { at } = ev {
                let fired = sim.launchers.iter().position(|l| (l.at + l.dir * 0.3).distance(at) < 0.01);
                if let Some(e) = fired.and_then(|k| self.ents.iter().position(|e| matches!(e.kind, Kind::Launcher { index, .. } if index == k))) {
                    self.fire(e, "OnPostSpawnBall", None);
                }
            }
            if let SimEvent::RocketExploded { at, .. } = ev {
                self.rocket_blast(sim, at);
            }
            // A portal opened in a detector (or closed out of it): it fires for its colour,
            // for any, and once both portals are open, for a linked one.
            if let SimEvent::PortalDetected { volume, color, inside } = ev {
                let both = sim.portals.iter().all(|p| p.is_some());
                let other = sim.volumes.get(volume).is_some_and(|v| v.portals[color.other().index()]);
                for e in 0..self.ents.len() {
                    if self.ents[e].kind != (Kind::Volume { index: volume }) {
                        continue;
                    }
                    let n = if color == PortalColor::Blue { 1 } else { 2 };
                    let (on, off) = if inside { ("OnStartTouch", "") } else { ("OnEndTouch", "") };
                    let _ = off;
                    self.fire(e, &format!("{on}Portal{n}"), None);
                    self.fire(e, &format!("{on}Portal"), None);
                    if both {
                        self.fire(e, &format!("{on}LinkedPortal"), None);
                        if inside && other {
                            self.fire(e, "OnStartTouchBothLinkedPortals", None);
                        }
                    }
                }
            }
            // A switch she's pressed.
            if let SimEvent::MoverUsed { mover } = ev {
                for e in 0..self.ents.len() {
                    let Kind::Mover { index, .. } = self.ents[e].kind else { continue };
                    let Some(&toggle) = self.usable.get(&e) else { continue };
                    if index == mover && self.ents[e].alive {
                        let at = sim.movers[index].boxes().next().map_or(Vec3::ZERO, |b| b.center());
                        sim.events.push(SimEvent::PedestalPressed { at });
                        self.fire(e, "OnUse", None);
                        self.input(sim, e, if toggle { "Toggle" } else { "Open" }, "", e);
                    }
                }
            }
            if let SimEvent::MoverAt { mover, node, arrived } = ev {
                for e in 0..self.ents.len() {
                    let on_path = matches!(&self.ents[e].kind, Kind::PathNode { mover: m, node: n, others } if (*m == mover && *n == node) || others.contains(&(mover, node)));
                    match self.ents[e].kind {
                        Kind::PathNode { .. } if on_path => {
                            // (The train passing it is what set it off.)
                            let train = self.ents.iter().position(|t| matches!(t.kind, Kind::Mover { index, .. } if index == mover));
                            self.fire_by(e, "OnPass", None, train);
                        }
                        Kind::Mover { index, kind: MoverKind::Door } if index == mover && arrived => {
                            let last = sim.movers[index].def.path.len() - 1;
                            self.fire(e, if node == last { "OnFullyOpen" } else { "OnFullyClosed" }, None);
                            if let Some(wait) = sim.movers[index].def.reset.filter(|_| node == last) {
                                self.queue.push(Pending { activator: None, cube: None, at: self.time + wait, target: "!self".into(), input: "Close".into(), param: String::new(), caller: e });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        for e in 0..self.ents.len() {
            if let Kind::ButtonDoor { button, down } = self.ents[e].kind {
                let now = sim.buttons.get(button).is_some_and(|b| b.pressed);
                if now != down {
                    self.ents[e].kind = Kind::ButtonDoor { button, down: now };
                    if now {
                        self.fire(e, "OnOpen", None);
                        self.fire(e, "OnFullyOpen", None);
                    } else {
                        self.fire(e, "OnClose", None);
                        self.fire(e, "OnFullyClosed", None);
                    }
                }
            }
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

        // Cubes lost (in goo, out of the world): the map hears of it from the cube.
        for id in std::mem::take(&mut sim.lost) {
            if let Some(&e) = self.owners.get(&id) {
                self.fire_cube(e, "OnOutOfWorld", id);
            }
        }

        // Grills that have taken cubes.
        for k in 0..self.cleansers.len() {
            let e = self.cleansers[k];
            for _ in std::mem::take(&mut sim.fizzlers[k].dissolved) {
                self.fire(e, "OnDissolve", None);
                self.fire(e, "OnDissolveBox", None);
            }
        }

        // A pickup gun she's come to: hers (its colours), and the map hears of it.
        for e in 0..self.ents.len() {
            let Kind::GunPickup { index } = self.ents[e].kind else { continue };
            if sim.gun_pickups[index].taken {
                continue;
            }
            let (at, _) = sim.gun_pose(index);
            if at.distance(ctrl.feet + Vec3::Y * 0.9) < GUN_REACH {
                let g = &mut sim.gun_pickups[index];
                g.taken = true;
                sim.gun.blue |= g.gun.blue;
                sim.gun.orange |= g.gun.orange;
                self.fire(e, "OnPlayerPickup", None);
            }
        }

        // Triggers.
        let me = ctrl.body().aabb(ctrl.feet);
        for e in 0..self.ents.len() {
            let Kind::ChangeLevel { touch: Some(b), armed, .. } = self.ents[e].kind else { continue };
            let inside = b.overlaps(&me);
            if inside && armed && self.ents[e].alive {
                self.input(sim, e, "ChangeLevel", "", e);
            }
            if let Kind::ChangeLevel { armed, .. } = &mut self.ents[e].kind {
                *armed |= !inside;
            }
        }
        // (The map's triggers were placed round Portal's cubes, which are bigger than ours: a
        // cube touches one as a cube of Portal's size standing where it stands would. Chamber
        // 16's stacked cubes keep the hideout's panel shut this way: ours, smaller, settle
        // lower than the trigger over them.)
        let cubes: Vec<(u32, Aabb)> = sim.props.iter().filter(|p| p.alive && p.fizzle.is_none()).map(|p| (p.id, crate::source::portal_cube(p.pos))).collect();
        for e in 0..self.ents.len() {
            if !self.ents[e].alive {
                continue;
            }
            let Kind::Trigger { b, once, enabled, players, cubes: takes_cubes, pellets, rockets, mover, player_in, ref cubes_in } = self.ents[e].kind else { continue };
            if !enabled {
                continue;
            }
            let b = mover.and_then(|m| sim.movers.get(m)).map_or(b, |m| b.translated(m.offset()));
            if rockets {
                // (Each rocket once: `cubes_in` holds the ones that have been in.)
                let new: Vec<u32> = sim.rockets.iter().filter(|r| r.alive && geom::contains(&b, r.pos) && !cubes_in.contains(&r.id)).map(|r| r.id).collect();
                for id in new {
                    if let Kind::Trigger { cubes_in, .. } = &mut self.ents[e].kind {
                        cubes_in.push(id);
                    }
                    self.fire(e, "OnStartTouch", None);
                    self.fire(e, "OnTrigger", None);
                    if once {
                        self.ents[e].alive = false;
                        break;
                    }
                }
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
            // (A cube coming in is its `!activator`.)
            let new_cube = cubes_now.iter().find(|id| !cubes_in.contains(id)).copied().filter(|_| !(player_now && !player_in));
            let left = (!player_now && player_in) || cubes_in.iter().any(|id| !cubes_now.contains(id));
            let was_any = player_in || !cubes_in.is_empty();
            let any = player_now || !cubes_now.is_empty();
            if let Kind::Trigger { player_in, cubes_in, .. } = &mut self.ents[e].kind {
                *player_in = player_now;
                *cubes_in = cubes_now;
            }
            if entered {
                match new_cube {
                    Some(id) => {
                        self.fire_cube(e, "OnStartTouch", id);
                        self.fire_cube(e, "OnTrigger", id);
                    }
                    None => {
                        self.fire(e, "OnStartTouch", None);
                        self.fire(e, "OnTrigger", None);
                    }
                }
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

        // Texture frames on the run.
        for e in 0..self.ents.len() {
            if let Kind::FrameLerp { ref parent, lerp: Some((from, to, over, looping, since)) } = self.ents[e].kind {
                let t = (now - since) / over;
                let t = if looping { t.fract() } else { t.min(1.0) };
                let parent = parent.clone();
                self.set_frame(sim, &parent, (from + (to - from) * t).floor().max(0.0) as u32);
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
                // A cube that set it off, killed (one lost, or into a trigger that takes it).
                if let (Some(id), "!activator") = (p.cube, p.target.as_str()) {
                    if p.input.eq_ignore_ascii_case("kill") {
                        if let Some(k) = sim.props.iter().position(|c| c.id == id && c.alive) {
                            sim.props[k].alive = false;
                            if sim.held == Some(k) {
                                sim.held = None;
                            }
                        }
                    }
                    continue;
                }
                let targets: Vec<usize> = match p.target.as_str() {
                    "!self" | "!caller" => vec![p.caller],
                    "!activator" => p.activator.into_iter().collect(),
                    // (A name ending in `*` is every entity whose name starts so: Source's
                    // wildcard, as Chamber 15's moving platforms are started.)
                    t => match t.strip_suffix('*') {
                        Some(prefix) => {
                            let mut v: Vec<usize> = self.by_name.iter().filter(|(n, _)| n.starts_with(prefix)).flat_map(|(_, es)| es.iter().copied()).collect();
                            v.sort_unstable();
                            v
                        }
                        None => self.by_name.get(t).cloned().unwrap_or_default(),
                    },
                };
                for t in targets {
                    self.input(sim, t, &p.input, &p.param, p.caller);
                }
            }
        }
    }

    /// A rocket's gone off at `at`: what's near is hurt, glass near enough breaks, and triggers
    /// for rockets it went off in hear of it.
    fn rocket_blast(&mut self, sim: &mut Sim, at: Vec3) {
        for (e, c, r) in self.damageable.clone() {
            if self.ents[e].alive && at.distance(c) < r + ROCKET_BLAST * 0.5 {
                self.fire(e, "OnTakeDamage", None);
            }
        }
        for e in 0..self.ents.len() {
            match self.ents[e].kind {
                Kind::Mover { index, .. } if self.ents[e].class == "func_breakable_surf" && self.ents[e].alive => {
                    let m = &mut sim.movers[index];
                    if m.present && m.boxes().any(|b| at.clamp(b.min, b.max).distance(at) < ROCKET_BLAST) {
                        m.present = false;
                        self.fire(e, "OnBreak", None);
                    }
                }
                Kind::Trigger { b, enabled: true, rockets: true, mover, .. }
                    if self.ents[e].alive && {
                        let b = mover.and_then(|m| sim.movers.get(m)).map_or(b, |m| b.translated(m.offset()));
                        geom::contains(&Aabb::new(b.min - Vec3::splat(0.5), b.max + Vec3::splat(0.5)), at)
                    } =>
                {
                    self.fire(e, "OnStartTouch", None);
                    self.fire(e, "OnTrigger", None);
                    if matches!(self.ents[e].kind, Kind::Trigger { once: true, .. }) {
                        self.ents[e].alive = false;
                    }
                }
                _ => {}
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
    pub(crate) fn input(&mut self, sim: &mut Sim, e: usize, input: &str, param: &str, caller: usize) {
        let i = input.to_ascii_lowercase();
        if !self.ents[e].alive && i != "enable" {
            return;
        }
        // A new output (Source's "Event target:input:param:delay:times"; a key's new value,
        // "key value", is left alone).
        if i == "addoutput" {
            let mut it = param.trim().splitn(2, char::is_whitespace);
            if let (Some(event), Some(rest)) = (it.next(), it.next())
                && rest.contains(':')
                && let Some(o) = Output::parse(event, &rest.replace(':', ","))
            {
                self.ents[e].outputs.push(o);
            }
            return;
        }
        if i == "kill" || i == "killhierarchy" {
            self.ents[e].alive = false;
            match self.ents[e].kind {
                Kind::Mover { index, .. } => {
                    sim.movers[index].present = false;
                    sim.movers[index].goal = None;
                    // (And what rides on it.)
                    if i == "killhierarchy" {
                        for m in sim.movers.iter_mut().filter(|m| m.def.parent == Some(index)) {
                            m.present = false;
                        }
                    }
                }
                Kind::Animated if self.hatches.contains_key(&e) => sim.movers[self.hatches[&e]].present = false,
                Kind::Sound { .. } => {
                    if let Some(st) = self.sounds.get_mut(&e) {
                        st.playing = false;
                    }
                }
                // A launcher that's gone fires no more.
                Kind::Launcher { sig, .. } => {
                    if let Some(v) = sim.io_signals.get_mut(sig) {
                        *v = false;
                    }
                }
                _ => {}
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
            // (Only off and back onto the mover it follows: Chamber 18's launcher arm, let go of
            // by its turning hatch at the end of each quarter turn and taken up again once the
            // hatch has turned back.)
            Kind::Mover { index, .. } if matches!(i.as_str(), "clearparent" | "setparent") => sim.detach_mover(index, i == "clearparent"),
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
                    // A door's own speed (units, or for a swinging door degrees, a second).
                    (MoverKind::Door, "setspeed") => {
                        let v = num(param).abs();
                        m.speed = if m.def.swing.is_some() { v.to_radians() } else { v * 0.0254 };
                    }
                    (MoverKind::Train, "setspeed" | "setspeeddir") => {
                        let f = num(param);
                        m.speed = m.def.speed * f.abs();
                        m.goal = if f == 0.0 { None } else if f > 0.0 { Some(last) } else { Some(0) };
                    }
                    (MoverKind::Train, "setspeedreal") => {
                        m.speed = num(param).abs() * 0.0254;
                        m.goal = if m.speed == 0.0 { None } else { Some(last) };
                    }
                    // A fan: on and off, faster and slower, the other way round.
                    (MoverKind::Rotor, "start" | "turnon" | "enable") => m.spin_on = true,
                    (MoverKind::Rotor, "stop" | "turnoff" | "disable") => m.spin_on = false,
                    (MoverKind::Rotor, "toggle") => m.spin_on = !m.spin_on,
                    (MoverKind::Rotor, "startforward") => {
                        m.spin_scale = m.spin_scale.abs();
                        m.spin_on = true;
                    }
                    (MoverKind::Rotor, "startbackward") => {
                        m.spin_scale = -m.spin_scale.abs();
                        m.spin_on = true;
                    }
                    (MoverKind::Rotor, "reverse") => m.spin_scale = -m.spin_scale,
                    // A fraction of its top speed.
                    (MoverKind::Rotor, "setspeed") => m.spin_scale = m.spin_scale.signum() * num(param).clamp(0.0, 1.0),
                    (_, "enable" | "turnon" | "enablecollision") => m.present = true,
                    (_, "disable" | "turnoff" | "disablecollision") => m.present = false,
                    (MoverKind::Brush, "toggle") => m.present = !m.present,
                    (MoverKind::Brush, "break" | "shatter") if m.present => {
                        m.present = false;
                        self.fire(e, "OnBreak", None);
                    }
                    _ => {}
                }
            }
            Kind::Launcher { index, .. } if i == "launchball" => {
                if let Some(l) = sim.launchers.get_mut(index) {
                    l.queued = true;
                }
            }
            Kind::Switch { sig } | Kind::Launcher { sig, .. } => {
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
            Kind::Scene { lines, length, triggers } => match i.as_str() {
                "start" => {
                    self.fire(e, "OnStart", None);
                    let length = if length > 0.0 { length } else { SCENE_SECONDS };
                    self.queue.push(Pending { activator: None, cube: None, at: now + length, target: "!self".into(), input: "__done".into(), param: String::new(), caller: e });
                    for (at, sound) in lines {
                        self.queue.push(Pending { activator: None, cube: None, at: now + at, target: "!self".into(), input: "__say".into(), param: sound, caller: e });
                    }
                    for (at, n) in triggers {
                        self.queue.push(Pending { activator: None, cube: None, at: now + at, target: "!self".into(), input: "__trigger".into(), param: n.to_string(), caller: e });
                    }
                }
                "__trigger" => self.fire(e, &format!("OnTrigger{}", param.trim()), None),
                "__say" => {
                    let n = self.voice.last().map_or(1, |v| v.0 + 1);
                    self.voice.push((n, param.to_string()));
                    // (Only the last few are kept: the host plays them as they come.)
                    if self.voice.len() > 16 {
                        self.voice.remove(0);
                    }
                }
                "__done" => self.fire(e, "OnCompletion", None),
                "cancel" => self.queue.retain(|p| !(p.caller == e && p.input.starts_with("__"))),
                _ => {}
            },
            Kind::Maker { ref trains, .. } if i == "forcespawn" && !trains.is_empty() && trains.iter().all(|&m| !sim.movers[m].present) => {
                // (One of each: it runs its track and is gone at the end; the next spawn after
                // that sets it off again.)
                for &m in trains.clone().iter() {
                    let mv = &mut sim.movers[m];
                    let start = mv.def.start;
                    mv.at = mv.def.path.get(start).copied().unwrap_or(Vec3::ZERO);
                    mv.node = start;
                    mv.present = true;
                    mv.carried = Vec3::ZERO;
                    mv.goal = (mv.def.path.len() > 1).then_some(mv.def.path.len() - 1);
                    if let Some(k) = self.ents.iter().position(|x| matches!(x.kind, Kind::Mover { index, .. } if index == m)) {
                        self.ents[k].alive = true;
                    }
                }
            }
            Kind::Maker { cubes, .. } if i == "forcespawn" => {
                for (at, kind, from) in cubes {
                    // Not on top of a cube that's still there.
                    if !sim.props.iter().any(|p| p.alive && p.pos.distance(at) < 0.5) {
                        let id = sim.spawn_prop(kind, at, None);
                        self.owners.insert(id, from);
                        sim.events.push(SimEvent::CubeDropped { at });
                    }
                }
            }
            Kind::Animated if matches!(i.as_str(), "enable" | "turnon" | "disable" | "turnoff") => {
                if i == "enable" || i == "turnon" {
                    self.hidden.remove(&e);
                } else {
                    self.hidden.insert(e);
                }
            }
            Kind::GunPickup { index } => match i.as_str() {
                "fireportal1" => {
                    sim.fire_pickup_gun(index, PortalColor::Blue);
                }
                "fireportal2" => {
                    sim.fire_pickup_gun(index, PortalColor::Orange);
                }
                "setparent" => {
                    let mover = self.by_name.get(&param.trim().to_ascii_lowercase()).into_iter().flatten().find_map(|&t| match self.ents[t].kind {
                        Kind::Mover { index, .. } => Some(index),
                        _ => None,
                    });
                    sim.reparent_gun(index, mover);
                }
                "clearparent" => sim.reparent_gun(index, None),
                _ => {}
            },
            Kind::Sound { volume, pitch, .. } => {
                let st = self.sounds.entry(e).or_insert(SoundState { playing: false, starts: 0, volume, pitch });
                match i.as_str() {
                    "playsound" | "fadein" => {
                        st.playing = true;
                        st.starts += 1;
                    }
                    "stopsound" | "fadeout" => st.playing = false,
                    "togglesound" => {
                        st.playing = !st.playing;
                        if st.playing {
                            st.starts += 1;
                        }
                    }
                    // (0-10; nothing: silent.)
                    "volume" => {
                        st.volume = (num(param) / 10.0).clamp(0.0, 1.0);
                        if st.volume <= 0.0 {
                            st.playing = false;
                        }
                    }
                    "pitch" => st.pitch = (num(param) / 100.0).clamp(0.01, 2.55),
                    _ => {}
                }
            }
            Kind::Soundscape { enabled, .. } => {
                let on = match i.as_str() {
                    "enable" => true,
                    "disable" => false,
                    "toggleenabled" => !enabled,
                    _ => enabled,
                };
                if let Kind::Soundscape { enabled, .. } = &mut self.ents[e].kind {
                    *enabled = on;
                }
            }
            Kind::Correction { .. } => {
                if let Kind::Correction { enabled, fade_in, fade_out, .. } = &mut self.ents[e].kind {
                    match i.as_str() {
                        "enable" => *enabled = true,
                        "disable" => *enabled = false,
                        "setfadeinduration" => *fade_in = num(param).max(0.0),
                        "setfadeoutduration" => *fade_out = num(param).max(0.0),
                        _ => {}
                    }
                }
            }
            Kind::Shake { at, amplitude, frequency, duration, radius } => {
                let amplitude = match i.as_str() {
                    "startshake" => amplitude,
                    "stopshake" => 0.0,
                    "amplitude" => num(param),
                    _ => return,
                };
                let n = self.shakes.last().map_or(1, |s| s.n + 1);
                self.shakes.push(Shake { n, at, amplitude, frequency, duration, radius });
                if self.shakes.len() > 8 {
                    self.shakes.remove(0);
                }
            }
            Kind::Fade { color, alpha, duration, hold, from, stays } if i == "fade" || i == "fadereverse" => {
                let n = self.fades.last().map_or(1, |f| f.n + 1);
                self.fades.push(Fade { n, color, alpha, duration, hold, from: from != (i == "fadereverse"), stays });
                if self.fades.len() > 8 {
                    self.fades.remove(0);
                }
            }
            Kind::Volume { index } => {
                if let Some(v) = sim.volumes.get_mut(index) {
                    match i.as_str() {
                        "enable" | "setactive" => v.on = true,
                        "disable" | "setinactive" => v.on = false,
                        "toggle" | "toggleactive" => v.on = !v.on,
                        "setspeed" => {
                            if let crate::level::VolumeKind::Push { speed, .. } = &mut v.def.kind {
                                *speed = num(param) * 0.0254;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Kind::Effect if i == "ragdoll" || i == "break" => {
                self.knocked.insert(e);
            }
            Kind::Effect => match i.as_str() {
                "lighton" | "turnon" | "startspark" | "start" | "showsprite" => {
                    self.hidden.remove(&e);
                }
                "lightoff" | "turnoff" | "stopspark" | "stop" | "stopplayendcap" | "hidesprite" => {
                    self.hidden.insert(e);
                }
                "toggle" | "togglespark" if !self.hidden.remove(&e) => {
                    self.hidden.insert(e);
                }
                _ => {}
            },
            // Whoever receives it, a tonemap controller's inputs are kept for the host.
            Kind::Other if i.starts_with("setautoexposure") || i.starts_with("setbloom") || i == "settonemaprate" => {
                self.tonemap.insert(i.clone(), num(param));
            }
            Kind::Other if i == "usedefaultautoexposure" => {
                self.tonemap.retain(|k, _| !k.starts_with("setautoexposure"));
            }
            Kind::Other if i == "usedefaultbloomscale" => {
                self.tonemap.remove("setbloomscale");
            }
            Kind::Sprite => match i.as_str() {
                "showsprite" => {
                    self.hidden.remove(&e);
                }
                "hidesprite" => {
                    self.hidden.insert(e);
                }
                "togglesprite" if !self.hidden.remove(&e) => {
                    self.hidden.insert(e);
                }
                _ => {}
            },
            Kind::Animated if i == "skin" => {
                self.skins.insert(e, num(param).max(0.0) as u32);
            }
            Kind::FrameLerp { .. } if i == "startfloatlerp" => {
                let v: Vec<f32> = param.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if v.len() >= 3 {
                    let lerp = Some((v[0], v[1], v[2].max(1e-3), v.get(3).is_some_and(|l| *l != 0.0), self.time));
                    if let Kind::FrameLerp { lerp: l, .. } = &mut self.ents[e].kind {
                        *l = lerp;
                    }
                }
            }
            Kind::FrameLerp { parent, .. } if i == "setmaterialvar" => {
                if let Kind::FrameLerp { lerp, .. } = &mut self.ents[e].kind {
                    *lerp = None;
                }
                self.set_frame(sim, &parent, num(param).max(0.0) as u32);
            }
            Kind::TextureToggle { target } if i == "settextureindex" || i == "incrementtextureindex" => {
                // (Overlays go by name: a door's indicator strips.)
                let n = if i == "settextureindex" { num(param).max(0.0) as u32 } else { self.named_frames.get(&target).map_or(1, |f| f + 1) };
                self.named_frames.insert(target.clone(), n);
                for t in self.by_name.get(&target).cloned().unwrap_or_default() {
                    let Kind::Mover { index, .. } = self.ents[t].kind else { continue };
                    let model = sim.movers[index].def.model;
                    let n = if i == "settextureindex" { num(param).max(0.0) as u32 } else { self.frames.get(&model).map_or(1, |f| f + 1) };
                    self.frames.insert(model, n);
                }
            }
            Kind::PathNode { mover, node, ref others } if i == "enablepath" || i == "disablepath" => {
                for (mover, node) in std::iter::once((mover, node)).chain(others.iter().copied()) {
                    if let Some(c) = sim.movers.get_mut(mover).and_then(|m| m.closed.get_mut(node)) {
                        *c = i == "disablepath";
                    }
                }
            }
            Kind::Animated if i == "setanimation" => {
                if let Some(&m) = self.hatches.get(&e) {
                    sim.movers[m].present = !param.trim().eq_ignore_ascii_case("open");
                }
                self.animation_count += 1;
                self.animations.insert(e, (param.trim().to_ascii_lowercase(), self.animation_count));
            }
            Kind::ChangeLevel { map, landmark, .. } if i == "changelevel" => {
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
