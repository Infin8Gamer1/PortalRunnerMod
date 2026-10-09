//! A running test chamber: portals, cubes, buttons, doors, grills, faith plates, light
//! bridges, lasers and gel, and Faith moving through it all.
//!
//! Each frame the host calls [`Sim::step_player`] (Faith's movement, with the world as she
//! sees it, and her trip through any portal) then [`Sim::step`] (everything else). The
//! portal gun is [`Sim::fire`]; picking things up and pressing buttons is [`Sim::use_key`].
//! Nothing here knows about rendering: the host draws what it finds in the public fields
//! and plays sounds for [`SimEvent`]s.

use glam::{Quat, Vec2, Vec3};

use faith_move::{Aabb, BoxWorld, Controller, Event as MoveEvent, Input as MoveInput, World};

use crate::collide::{self, move_box, BodyWorld};
use crate::geom::{self, ray_aabb, Rng};
use crate::gel::{Blob, GelMap, Splat};
use crate::level::{face_rect, CubeKind, Gun, Inputs, Item, Level, Mat, Paint, Sig};
use crate::portal::{carry_view, Link, Portal, PortalColor, PORTAL_HALF};

/// Faith's gravity (faith_move's measured 16.4 m/s²): cubes fall the same.
pub const GRAVITY: f32 = 16.4;
/// Half a cube's size.
pub const CUBE_HALF: f32 = 0.33;
/// The top of a floor button's base above the floor.
pub const BUTTON_TOP: f32 = 0.12;
pub const BUTTON_RADIUS: f32 = 0.7;
/// A faith plate's top above the floor.
pub const PLATE_TOP: f32 = 0.08;
pub const PLATE_HALF: f32 = 0.75;
/// Light bridges: half their width, and how thick.
pub const BRIDGE_HALF_WIDTH: f32 = 0.85;
pub const BRIDGE_THICK: f32 = 0.06;
/// Nothing moves faster than this (m/s): Portal's terminal velocity, near enough.
pub const TERMINAL: f32 = 40.0;
/// How far away you can pick something up or press a button.
pub const USE_REACH: f32 = 2.3;
/// A laser kills after this long in it (seconds).
pub const LASER_KILL: f32 = 0.9;

/// What a still box belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Level(usize),
    Button(usize),
    Pedestal(usize),
    Plate(usize),
    Emitter,
    Catcher(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Static {
    pub b: Aabb,
    pub mat: Mat,
    pub owner: Owner,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prop {
    pub id: u32,
    pub kind: CubeKind,
    /// Centre.
    pub pos: Vec3,
    pub vel: Vec3,
    /// For drawing (and a reflector's aim: its front is local -Z).
    pub rot: Quat,
    pub env: Option<PortalColor>,
    /// Gel on it.
    pub coat: Option<Paint>,
    /// Being vaporised: seconds since it started.
    pub fizzle: Option<f32>,
    pub dropper: Option<usize>,
    pub grounded: bool,
    pub alive: bool,
}

impl Prop {
    pub fn aabb(&self) -> Aabb {
        Aabb::from_center_size(self.pos, Vec3::splat(CUBE_HALF * 2.0))
    }

    /// Which way a reflector cube sends a laser.
    pub fn front(&self) -> Vec3 {
        let f = self.rot * Vec3::NEG_Z;
        let h = Vec3::new(f.x, 0.0, f.z);
        if h.length_squared() > 1e-4 { h.normalize() } else { Vec3::NEG_Z }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Door {
    pub b: Aabb,
    pub inputs: Inputs,
    /// 0 closed .. 1 open.
    pub open: f32,
}

impl Door {
    pub fn solid(&self) -> bool {
        self.open < 0.85
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Button {
    pub sig: Sig,
    pub at: Vec3,
    pub cube_only: bool,
    pub pressed: bool,
    /// What's pressing it is a cube (for the light colour).
    pub by_cube: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pedestal {
    pub sig: Sig,
    pub at: Vec3,
    pub time: f32,
    /// Seconds left on (time <= 0: a toggle, `left` > 0 means on).
    pub left: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dropper {
    pub at: Vec3,
    pub kind: CubeKind,
    pub inputs: Inputs,
    pub was_on: bool,
    /// The cube it dropped last, while that one lasts.
    pub cube: Option<u32>,
    /// Counting down to dropping a replacement.
    pub refill: Option<f32>,
    /// Seconds since it last opened (for the hatch animation).
    pub opened: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fizzler {
    pub b: Aabb,
    pub inputs: Inputs,
    pub on: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plate {
    pub at: Vec3,
    pub target: Vec3,
    pub apex: f32,
    pub inputs: Inputs,
    pub on: bool,
    /// Seconds since it last fired.
    pub fired: f32,
}

/// One straight piece of a light bridge: from `a` to `b` along its walkable surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BridgeSeg {
    pub a: Vec3,
    pub b: Vec3,
    pub up: Vec3,
    pub aabb: Aabb,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bridge {
    pub at: Vec3,
    pub dir: Vec3,
    pub up: Vec3,
    pub inputs: Inputs,
    pub on: bool,
    pub segs: Vec<BridgeSeg>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Laser {
    pub at: Vec3,
    pub dir: Vec3,
    pub inputs: Inputs,
    pub on: bool,
    /// The beam, straight piece by piece (through portals and off reflector cubes).
    pub path: Vec<(Vec3, Vec3)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Catcher {
    pub sig: Sig,
    pub at: Vec3,
    pub facing: Vec3,
    pub lit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Relay {
    pub sig: Sig,
    pub at: Vec3,
    pub lit: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dispenser {
    pub at: Vec3,
    pub dir: Vec3,
    pub paint: Paint,
    pub inputs: Inputs,
    pub rate: f32,
    pub speed: f32,
    pub on: bool,
    acc: f32,
}

/// Things that happened this frame, for sounds and effects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SimEvent {
    /// A shot from the gun: where it went (through portals: the last leg's start), whether a
    /// portal opened.
    Shot { color: PortalColor, from: Vec3, to: Vec3, opened: bool },
    PortalOpened { color: PortalColor, at: Vec3, normal: Vec3 },
    /// The shot hit something portals don't stick to (or found no room).
    PortalFailed { color: PortalColor, at: Vec3, normal: Vec3 },
    /// Both portals closed (an emancipation grill).
    PortalsCleared,
    /// Faith went in through `entered`: and the roll she's left with.
    Teleported { entered: PortalColor, roll: f32 },
    PropTeleported { id: u32, at: Vec3 },
    ButtonDown { at: Vec3 },
    ButtonUp { at: Vec3 },
    PedestalPressed { at: Vec3 },
    DoorOpening { at: Vec3 },
    DoorClosing { at: Vec3 },
    CubeDropped { at: Vec3 },
    CubeFizzled { at: Vec3 },
    CubeLanded { at: Vec3, speed: f32 },
    PickUp { id: u32 },
    Drop { id: u32 },
    PlateLaunch { at: Vec3 },
    GelSplat { at: Vec3, paint: Paint },
    CatcherOn { at: Vec3 },
    CatcherOff { at: Vec3 },
    LaserBurn,
    PlayerDied,
    Checkpoint { index: usize },
    LevelComplete,
}

/// What a ray met.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HitWhat {
    Static(usize),
    Door(usize),
    Bridge(usize),
    Prop(usize),
    Fizzler(usize),
    Portal(PortalColor),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub t: f32,
    pub point: Vec3,
    pub normal: Vec3,
    pub what: HitWhat,
}

/// Which things a ray stops at.
#[derive(Clone, Copy, Debug)]
pub struct Mask {
    pub doors: bool,
    pub bridges: bool,
    pub props: bool,
    pub fizzlers: bool,
    pub portals: bool,
    /// Stop at grating (people and cubes do; shots, lasers and gel go through).
    pub grates: bool,
    /// Glass stops it.
    pub glass: bool,
    /// Skip this prop (the one being held, or a reflector the beam is leaving).
    pub skip_prop: Option<usize>,
}

impl Mask {
    pub const SHOT: Mask = Mask { doors: true, bridges: true, props: true, fizzlers: true, portals: true, grates: false, glass: true, skip_prop: None };
    pub const LASER: Mask = Mask { doors: true, bridges: false, props: true, fizzlers: false, portals: true, grates: false, glass: true, skip_prop: None };
    pub const BRIDGE: Mask = Mask { doors: true, bridges: false, props: false, fizzlers: false, portals: true, grates: true, glass: true, skip_prop: None };
    pub const GEL: Mask = Mask { doors: true, bridges: false, props: true, fizzlers: false, portals: true, grates: false, glass: true, skip_prop: None };
    pub const SOLID: Mask = Mask { doors: true, bridges: true, props: true, fizzlers: false, portals: false, grates: true, glass: true, skip_prop: None };
}

/// Where Faith looks from, for the gun, holding and using.
#[derive(Clone, Copy, Debug)]
pub struct Aim {
    pub eye: Vec3,
    pub dir: Vec3,
    pub yaw: f32,
}

impl Aim {
    pub fn of(c: &Controller) -> Aim {
        let v = c.view();
        Aim { eye: v.eye, dir: crate::portal::view_dir(c.yaw, c.pitch), yaw: c.yaw }
    }
}

pub struct Sim {
    pub level: Level,
    pub statics: Vec<Static>,
    static_world: BoxWorld,
    pub portals: [Option<Portal>; 2],
    carved: [Option<BoxWorld>; 2],
    pub props: Vec<Prop>,
    next_prop: u32,
    pub doors: Vec<Door>,
    pub buttons: Vec<Button>,
    pub pedestals: Vec<Pedestal>,
    pub droppers: Vec<Dropper>,
    pub fizzlers: Vec<Fizzler>,
    pub plates: Vec<Plate>,
    pub bridges: Vec<Bridge>,
    pub lasers: Vec<Laser>,
    pub catchers: Vec<Catcher>,
    pub relays: Vec<Relay>,
    pub dispensers: Vec<Dispenser>,
    pub goo: Vec<Aabb>,
    pub gel: GelMap,
    pub blobs: Vec<Blob>,
    pub signals: Vec<bool>,
    /// The prop Faith is carrying (index into `props`).
    pub held: Option<usize>,
    held_far: f32,
    pub player_env: Option<PortalColor>,
    pub checkpoint: usize,
    pub gun: Gun,
    pub events: Vec<SimEvent>,
    pub time: f32,
    /// Seconds spent in a laser (it kills at [`LASER_KILL`]).
    pub burn: f32,
    pub complete: bool,
    /// Camera roll left over from the last portal (radians), for the host to ease out.
    pub roll: f32,
    /// Steer Faith into portals she's falling at (Portal's funnel).
    pub funnel: bool,
    rng: Rng,
}

impl Sim {
    pub fn new(level: Level) -> Sim {
        let mut statics: Vec<Static> =
            level.solids.iter().enumerate().map(|(i, s)| Static { b: s.b, mat: s.mat, owner: Owner::Level(i) }).collect();
        let mut sim = Sim {
            statics: Vec::new(),
            static_world: BoxWorld::default(),
            portals: [None, None],
            carved: [None, None],
            props: Vec::new(),
            next_prop: 1,
            doors: Vec::new(),
            buttons: Vec::new(),
            pedestals: Vec::new(),
            droppers: Vec::new(),
            fizzlers: Vec::new(),
            plates: Vec::new(),
            bridges: Vec::new(),
            lasers: Vec::new(),
            catchers: Vec::new(),
            relays: Vec::new(),
            dispensers: Vec::new(),
            goo: Vec::new(),
            gel: GelMap::default(),
            blobs: Vec::new(),
            signals: vec![false; level.signal_count()],
            held: None,
            held_far: 0.0,
            player_env: None,
            checkpoint: 0,
            gun: level.gun,
            events: Vec::new(),
            time: 0.0,
            burn: 0.0,
            complete: false,
            roll: 0.0,
            funnel: true,
            rng: Rng::new(0x5eed_1234),
            level: level.clone(),
        };
        for it in &level.items {
            match it.clone() {
                Item::FloorButton { sig, at, cube_only } => {
                    let i = sim.buttons.len();
                    statics.push(Static {
                        b: Aabb::new(at + Vec3::new(-0.8, 0.0, -0.8), at + Vec3::new(0.8, BUTTON_TOP, 0.8)),
                        mat: Mat::Prop,
                        owner: Owner::Button(i),
                    });
                    sim.buttons.push(Button { sig, at, cube_only, pressed: false, by_cube: false });
                }
                Item::Pedestal { sig, at, time } => {
                    let i = sim.pedestals.len();
                    statics.push(Static {
                        b: Aabb::new(at + Vec3::new(-0.15, 0.0, -0.15), at + Vec3::new(0.15, 1.0, 0.15)),
                        mat: Mat::Prop,
                        owner: Owner::Pedestal(i),
                    });
                    sim.pedestals.push(Pedestal { sig, at, time, left: 0.0 });
                }
                Item::Door { b, inputs } => sim.doors.push(Door { b, inputs, open: 0.0 }),
                Item::Cube { at, kind } => {
                    sim.spawn_prop(kind, at, None);
                }
                Item::Dropper { at, kind, inputs } => {
                    sim.droppers.push(Dropper { at, kind, inputs, was_on: false, cube: None, refill: None, opened: 99.0 })
                }
                Item::Fizzler { b, inputs } => sim.fizzlers.push(Fizzler { b, inputs, on: true }),
                Item::FaithPlate { at, target, apex, inputs } => {
                    let i = sim.plates.len();
                    statics.push(Static {
                        b: Aabb::new(at + Vec3::new(-PLATE_HALF, 0.0, -PLATE_HALF), at + Vec3::new(PLATE_HALF, PLATE_TOP, PLATE_HALF)),
                        mat: Mat::Prop,
                        owner: Owner::Plate(i),
                    });
                    sim.plates.push(Plate { at, target, apex, inputs, on: true, fired: 99.0 });
                }
                Item::Bridge { at, dir, up, inputs } => {
                    let w = dir.cross(up).normalize_or_zero();
                    let lo = at - dir * 0.4 - w * (BRIDGE_HALF_WIDTH + 0.15) - up * 0.3;
                    let hi = at - dir * 0.02 + w * (BRIDGE_HALF_WIDTH + 0.15) + up * 0.12;
                    statics.push(Static { b: Aabb::new(lo.min(hi), lo.max(hi)), mat: Mat::Prop, owner: Owner::Emitter });
                    sim.bridges.push(Bridge { at, dir, up, inputs, on: false, segs: Vec::new() });
                }
                Item::Laser { at, dir, inputs } => {
                    statics.push(Static { b: Aabb::from_center_size(at - dir * 0.32, Vec3::splat(0.6)), mat: Mat::Prop, owner: Owner::Emitter });
                    sim.lasers.push(Laser { at, dir, inputs, on: false, path: Vec::new() });
                }
                Item::Catcher { sig, at, facing } => {
                    let i = sim.catchers.len();
                    statics.push(Static { b: Aabb::from_center_size(at - facing * 0.4, Vec3::splat(0.8)), mat: Mat::Prop, owner: Owner::Catcher(i) });
                    sim.catchers.push(Catcher { sig, at, facing, lit: false });
                }
                Item::Relay { sig, at } => sim.relays.push(Relay { sig, at, lit: false }),
                Item::GelDispenser { at, dir, paint, inputs, rate, speed } => {
                    statics.push(Static { b: Aabb::from_center_size(at - dir * 0.35, Vec3::splat(0.7)), mat: Mat::Prop, owner: Owner::Emitter });
                    sim.dispensers.push(Dispenser { at, dir, paint, inputs, rate, speed, on: false, acc: 0.0 });
                }
                Item::Painted { b, normal, paint } => {
                    let (c, u, v, half) = face_rect(&b, normal);
                    sim.gel.fill(c, geom::snap_axis(normal), u, v, half, paint);
                }
                Item::Goo { b } => sim.goo.push(b),
            }
        }
        sim.static_world = BoxWorld::indexed(statics.iter().map(|s| s.b).collect(), Vec::new(), 4.0);
        sim.statics = statics;
        for p in level.portals.clone() {
            let portal = Portal::new(p.center, p.normal, p.up);
            sim.set_portal(p.color, Some(portal));
        }
        sim
    }

    // ------------------------------------------------------------ portals

    pub fn portal(&self, c: PortalColor) -> Option<Portal> {
        self.portals[c.index()]
    }

    pub fn linked(&self) -> bool {
        self.portals[0].is_some() && self.portals[1].is_some()
    }

    /// The link from portal `c` to the other one, when both are open.
    pub fn link(&self, c: PortalColor) -> Option<Link> {
        Some(Link::new(self.portals[c.index()]?, self.portals[c.other().index()]?))
    }

    pub fn set_portal(&mut self, c: PortalColor, p: Option<Portal>) {
        self.portals[c.index()] = p;
        self.rebuild_carved();
    }

    pub fn clear_portals(&mut self) {
        if self.portals.iter().any(|p| p.is_some()) {
            self.events.push(SimEvent::PortalsCleared);
        }
        self.portals = [None, None];
        self.rebuild_carved();
        self.player_env = None;
    }

    fn rebuild_carved(&mut self) {
        let linked = self.linked();
        let boxes: Vec<Aabb> = self.statics.iter().map(|s| s.b).collect();
        for c in PortalColor::BOTH {
            self.carved[c.index()] = match (linked, self.portals[c.index()]) {
                (true, Some(p)) => Some(collide::carve(&boxes, &p)),
                _ => None,
            };
        }
    }

    fn still_for(&self, env: Option<PortalColor>) -> &BoxWorld {
        env.and_then(|c| self.carved[c.index()].as_ref()).unwrap_or(&self.static_world)
    }

    /// Boxes that move: closed doors, bridges, cubes (but not `skip`).
    fn moving_boxes(&self, skip: Option<usize>, include_held: bool) -> Vec<Aabb> {
        let mut v: Vec<Aabb> = self.doors.iter().filter(|d| d.solid()).map(|d| d.b).collect();
        for b in &self.bridges {
            v.extend(b.segs.iter().map(|s| s.aabb));
        }
        for (i, p) in self.props.iter().enumerate() {
            if Some(i) == skip || !p.alive || p.fizzle.is_some() || (!include_held && Some(i) == self.held) {
                continue;
            }
            v.push(p.aabb());
        }
        v
    }

    /// The world as Faith sees it this frame.
    pub fn player_world(&self) -> BodyWorld<'_> {
        BodyWorld {
            still: self.still_for(self.player_env),
            moving: plain(self.moving_boxes(None, false)),
            gel: &self.gel,
            fixtures: &self.level.fixtures,
        }
    }

    // ------------------------------------------------------------ rays

    /// The first thing a ray from `o` along unit `d` meets within `max`.
    pub fn raycast(&self, o: Vec3, d: Vec3, max: f32, mask: Mask) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let mut consider = |t: f32, n: Vec3, what: HitWhat| {
            if t >= 0.0 && t <= max && best.is_none_or(|b| t < b.t) {
                best = Some(Hit { t, point: o + d * t, normal: n, what });
            }
        };
        let dd = d * max;
        let reach = Aabb::new(o.min(o + dd) - Vec3::splat(0.01), o.max(o + dd) + Vec3::splat(0.01));
        for (i, s) in self.statics.iter().enumerate() {
            if !s.b.overlaps(&reach) || (s.mat == Mat::Grate && !mask.grates) || (s.mat == Mat::Glass && !mask.glass) {
                continue;
            }
            if let Some((t, n)) = ray_aabb(o, d, &s.b) {
                consider(t, n, HitWhat::Static(i));
            }
        }
        if mask.doors {
            for (i, door) in self.doors.iter().enumerate() {
                if door.solid() {
                    if let Some((t, n)) = ray_aabb(o, d, &door.b) {
                        consider(t, n, HitWhat::Door(i));
                    }
                }
            }
        }
        if mask.bridges {
            for (i, b) in self.bridges.iter().enumerate() {
                for s in &b.segs {
                    if let Some((t, n)) = ray_aabb(o, d, &s.aabb) {
                        consider(t, n, HitWhat::Bridge(i));
                    }
                }
            }
        }
        if mask.props {
            for (i, p) in self.props.iter().enumerate() {
                if !p.alive || Some(i) == mask.skip_prop {
                    continue;
                }
                if let Some((t, n)) = ray_aabb(o, d, &p.aabb()) {
                    consider(t, n, HitWhat::Prop(i));
                }
            }
        }
        if mask.fizzlers {
            for (i, f) in self.fizzlers.iter().enumerate() {
                if f.on {
                    if let Some((t, n)) = ray_aabb(o, d, &f.b) {
                        consider(t, n, HitWhat::Fizzler(i));
                    }
                }
            }
        }
        if mask.portals && self.linked() {
            for c in PortalColor::BOTH {
                let p = self.portals[c.index()].unwrap();
                let dn = d.dot(p.normal);
                if dn >= -1e-6 {
                    continue;
                }
                let t = (p.center - o).dot(p.normal) / dn;
                if t < 0.0 || t > max {
                    continue;
                }
                let at = o + d * t;
                if p.in_oval(at) {
                    // The portal wins over the wall it's on.
                    let t = (t - 1e-3).max(0.0);
                    consider(t, p.normal, HitWhat::Portal(c));
                }
            }
        }
        best
    }

    /// A ray carried on through portals: each straight leg, and what the last one met.
    pub fn trace(&self, o: Vec3, d: Vec3, max: f32, mask: Mask, hops: usize) -> (Vec<(Vec3, Vec3)>, Option<Hit>) {
        let mut legs = Vec::new();
        let (mut o, mut d, mut left) = (o, d, max);
        for _ in 0..=hops {
            match self.raycast(o, d, left, mask) {
                Some(h) => {
                    legs.push((o, h.point));
                    if let HitWhat::Portal(c) = h.what {
                        let link = self.link(c).unwrap();
                        left -= h.t;
                        d = link.dir(d);
                        o = link.point(h.point) + d * 2e-3;
                        continue;
                    }
                    return (legs, Some(h));
                }
                None => {
                    legs.push((o, o + d * left));
                    return (legs, None);
                }
            }
        }
        (legs, None)
    }

    // ------------------------------------------------------------ the gun

    /// Fire the portal gun. Returns whether a portal opened.
    pub fn fire(&mut self, color: PortalColor, aim: Aim) -> bool {
        let allowed = match color {
            PortalColor::Blue => self.gun.blue,
            PortalColor::Orange => self.gun.orange,
        };
        if !allowed {
            return false;
        }
        let (legs, hit) = self.trace(aim.eye, aim.dir, 200.0, Mask::SHOT, 4);
        let from = legs.last().map(|l| l.0).unwrap_or(aim.eye);
        let Some(h) = hit else {
            let to = legs.last().map(|l| l.1).unwrap_or(aim.eye);
            self.events.push(SimEvent::Shot { color, from, to, opened: false });
            return false;
        };
        // The way she was facing along the last leg (for a floor portal's up).
        let last_dir = legs.last().map(|(a, b)| (*b - *a).normalize_or_zero()).unwrap_or(aim.dir);
        let placed = match h.what {
            HitWhat::Static(i) => self.place(color, h.point, h.normal, i, last_dir),
            _ => None,
        };
        match placed {
            Some(p) => {
                self.set_portal(color, Some(p));
                self.events.push(SimEvent::Shot { color, from, to: h.point, opened: true });
                self.events.push(SimEvent::PortalOpened { color, at: p.center, normal: p.normal });
                // Anything standing in the new portal's way from before is not its concern.
                true
            }
            None => {
                self.events.push(SimEvent::Shot { color, from, to: h.point, opened: false });
                self.events.push(SimEvent::PortalFailed { color, at: h.point, normal: h.normal });
                false
            }
        }
    }

    /// Whether a portal could sit on the surface at `p` facing `n`.
    fn portalable_at(&self, p: Vec3, n: Vec3) -> bool {
        if self.gel.paint_at(p, n) == Some(Paint::Portal) {
            return self.statics.iter().any(|s| geom::contains(&s.b, p - n * 0.02));
        }
        let inside = p - n * 0.02;
        self.statics.iter().any(|s| s.mat.portalable() && matches!(s.owner, Owner::Level(_)) && geom::contains(&s.b, inside))
    }

    /// Whether anything solid is at `p` (the surface's open side must be clear).
    fn solid_at(&self, p: Vec3) -> bool {
        self.statics.iter().any(|s| geom::contains(&s.b, p))
    }

    /// Where a portal shot at `point` (on still box `solid`, face `n`) would open: on that
    /// surface, nudged to fit if it hangs off an edge or overlaps the other portal.
    pub fn place(&self, color: PortalColor, point: Vec3, n: Vec3, _solid: usize, view: Vec3) -> Option<Portal> {
        let n = geom::snap_axis(n);
        if !self.portalable_at(point, n) {
            return None;
        }
        let up = if n.y.abs() > 0.5 {
            // Floor and ceiling portals: their top points the way she's facing.
            let h = Vec3::new(view.x, 0.0, view.z);
            if h.length_squared() < 1e-6 { Vec3::NEG_Z } else { geom::snap_axis(h) }
        } else {
            Vec3::Y
        };
        let base = Portal::new(point + n * 0.0, n, up);
        let right = base.right();
        let other = self.portals[color.other().index()];
        // Candidate centres, nearest first.
        let step = 0.1;
        let (nx, ny) = ((PORTAL_HALF.x * 2.2 / step) as i32, (PORTAL_HALF.y * 1.6 / step) as i32);
        let mut offs: Vec<Vec2> = Vec::new();
        for i in -nx..=nx {
            for j in -ny..=ny {
                offs.push(Vec2::new(i as f32 * step, j as f32 * step));
            }
        }
        offs.sort_by(|a, b| a.length_squared().total_cmp(&b.length_squared()));
        for o in offs {
            let c = point + right * o.x + up * o.y;
            let cand = Portal::new(c, n, up);
            if self.portal_fits(&cand, other.as_ref()) {
                return Some(cand);
            }
        }
        None
    }

    fn portal_fits(&self, p: &Portal, other: Option<&Portal>) -> bool {
        let n = p.normal;
        let (r, u) = (p.right(), p.up);
        // Every sample over the outline: on a portal surface, open in front.
        let (sx, sy) = (4, 6);
        for i in 0..=sx {
            for j in 0..=sy {
                let a = (i as f32 / sx as f32 * 2.0 - 1.0) * (p.half.x - 0.02);
                let b = (j as f32 / sy as f32 * 2.0 - 1.0) * (p.half.y - 0.02);
                let q = p.center + r * a + u * b;
                if !self.portalable_at(q, n) || self.solid_at(q + n * 0.03) {
                    return false;
                }
            }
        }
        // Clear in front (no wall sticking out over it, no door, no bridge across it).
        let front = collide::local_box(p, Vec3::new(-p.half.x + 0.05, -p.half.y + 0.05, 0.02), Vec3::new(p.half.x - 0.05, p.half.y - 0.05, 0.3));
        if self.statics.iter().any(|s| s.b.overlaps(&front)) {
            return false;
        }
        if self.doors.iter().any(|d| d.solid() && d.b.overlaps(&front)) {
            return false;
        }
        if self.bridges.iter().any(|b| b.segs.iter().any(|s| s.aabb.overlaps(&front))) {
            return false;
        }
        // Not over the other portal.
        if let Some(o) = other {
            if o.normal.dot(n) > 0.99 && (o.center - p.center).dot(n).abs() < 0.05 {
                let d = o.center - p.center;
                let (dx, dy) = (d.dot(r).abs(), d.dot(u).abs());
                let (ox, oy) = if o.up == p.up || o.up == -p.up { (o.half.x, o.half.y) } else { (o.half.y, o.half.x) };
                if dx < p.half.x + ox && dy < p.half.y + oy {
                    return false;
                }
            }
        }
        true
    }

    // ------------------------------------------------------------ Faith

    /// Faith's frame: her movement in the world as she sees it, then through any portal she
    /// crossed, then grills, plates, goo, lasers, checkpoints and the exit.
    pub fn step_player(&mut self, ctrl: &mut Controller, input: &MoveInput, dt: f32) {
        let body = ctrl.body();
        let center = ctrl.feet + Vec3::Y * body.height * 0.5;
        let half = Vec3::new(body.half_width, body.height * 0.5, body.half_width);
        let linked = self.linked();
        let mut env = self.player_env;
        collide::update_env(&mut env, &self.portals, linked, center, half, ctrl.vel, dt);
        self.player_env = env;

        if self.funnel && linked && ctrl.state == faith_move::State::Air {
            self.funnel_into_portals(ctrl, dt);
        }
        let eye_before = ctrl.view().eye;
        {
            let world = self.player_world();
            ctrl.step(dt, input, &world);
        }
        if ctrl.vel.length() > TERMINAL {
            ctrl.vel = ctrl.vel.normalize() * TERMINAL;
        }
        let died = ctrl.events.contains(&MoveEvent::Death);
        if died {
            self.player_died(ctrl, false);
            return;
        }

        // Through a portal: her eye crossed one's surface inside the opening.
        let eye = ctrl.view().eye;
        if linked {
            for c in PortalColor::BOTH {
                let p = self.portals[c.index()].unwrap();
                let (a, b) = (p.signed_distance(eye_before), p.signed_distance(eye));
                if a >= 0.0 && b < 0.0 {
                    let s = a / (a - b);
                    let cross = eye_before + (eye - eye_before) * s;
                    if p.in_rect(cross, -0.05) && self.player_env == Some(c) {
                        self.teleport_player(ctrl, c, eye);
                        break;
                    }
                }
            }
        }

        let feet = ctrl.feet;
        let body = ctrl.body();
        let me = body.aabb(feet);

        // Emancipation grills.
        if self.fizzlers.iter().any(|f| f.on && f.b.overlaps(&me)) && self.portals.iter().any(|p| p.is_some()) {
            self.clear_portals();
        }
        // Goo, and falling out of the world.
        if self.goo.iter().any(|g| geom::contains(g, feet + Vec3::Y * 0.2)) || feet.y < self.level.kill_y {
            self.player_died(ctrl, true);
            return;
        }
        // Faith plates.
        for i in 0..self.plates.len() {
            let pl = self.plates[i].clone();
            if !pl.on || pl.fired < 0.6 {
                continue;
            }
            let d = feet - pl.at;
            if d.x.abs() < PLATE_HALF && d.z.abs() < PLATE_HALF && (d.y - PLATE_TOP).abs() < 0.2 && ctrl.vel.y <= 0.5 {
                let v = ballistic(feet, pl.target, pl.apex, ctrl.tuning.gravity);
                ctrl.launch(v);
                self.plates[i].fired = 0.0;
                self.events.push(SimEvent::PlateLaunch { at: pl.at });
            }
        }
        // Lasers burn.
        let in_beam = self.lasers.iter().filter(|l| l.on).flat_map(|l| l.path.iter()).find_map(|(a, b)| {
            let (d, q) = geom::segment_point(*a, *b, Vec3::new(feet.x, (feet.y + 0.2).max(a.y.min(b.y)).min(feet.y + body.height), feet.z));
            let (dc, _) = geom::segment_point(*a, *b, me.center());
            let closest = if dc < d { dc } else { d };
            (closest < body.half_width + 0.1 && q.y >= me.min.y - 0.1 && q.y <= me.max.y + 0.1).then_some((*a, *b, q))
        });
        if let Some((a, b, q)) = in_beam {
            self.burn += dt;
            self.events.push(SimEvent::LaserBurn);
            // Pushed out of it, sideways from the beam.
            let along = (b - a).normalize_or_zero();
            let mut away = me.center() - q;
            away -= along * away.dot(along);
            away.y = 0.0;
            let away = if away.length_squared() < 1e-4 { along.cross(Vec3::Y).normalize_or_zero() } else { away.normalize() };
            ctrl.vel += away * 30.0 * dt;
            if self.burn >= LASER_KILL {
                self.player_died(ctrl, true);
                return;
            }
        } else {
            self.burn = (self.burn - dt * 0.5).max(0.0);
        }
        // Checkpoints and the exit.
        for (i, cp) in self.level.checkpoints.iter().enumerate() {
            if i > self.checkpoint && geom::contains(&cp.region, feet + Vec3::Y * 0.1) {
                self.checkpoint = i;
                ctrl.spawn = cp.spawn;
                ctrl.spawn_yaw = cp.yaw;
                self.events.push(SimEvent::Checkpoint { index: i });
            }
        }
        if !self.complete && self.level.exit.is_some_and(|e| e.overlaps(&me)) {
            self.complete = true;
            self.events.push(SimEvent::LevelComplete);
        }
    }

    /// Portal's funnel: falling at a portal on the floor (or flying at one on a wall), she's
    /// steered towards its middle so she drops cleanly in instead of catching its rim.
    fn funnel_into_portals(&self, ctrl: &mut Controller, dt: f32) {
        let body = ctrl.body();
        let center = ctrl.feet + Vec3::Y * body.height * 0.5;
        let v = ctrl.vel;
        let mut best: Option<(f32, Vec3)> = None;
        for p in self.portals.iter().flatten() {
            let into = -v.dot(p.normal);
            // Only heading into it fast, and mostly along its normal (it's below her, or she
            // is flying at a wall one).
            if into < 3.0 || into < v.length() * 0.6 {
                continue;
            }
            let dist = p.signed_distance(center);
            if dist < 0.0 || dist > 8.0 {
                continue;
            }
            let t = dist / into;
            let at = center + v * t;
            let l = p.to_local(at);
            let (mx, my) = (p.half.x + 1.2, p.half.y + 1.2);
            if l.x.abs() > mx || l.y.abs() > my {
                continue;
            }
            // In-plane change of velocity that would bring her in through the middle.
            let off = p.right() * l.x + p.up * l.y;
            let want = -off / t.max(0.15);
            if best.is_none_or(|(d, _)| dist < d) {
                best = Some((dist, want));
            }
        }
        if let Some((_, want)) = best {
            ctrl.vel += want * (dt * 8.0).min(1.0);
        }
    }

    fn teleport_player(&mut self, ctrl: &mut Controller, entered: PortalColor, eye: Vec3) {
        let link = self.link(entered).unwrap();
        let out = link.to;
        let eye_off = eye - ctrl.feet;
        let new_eye = link.point(eye);
        let mut feet = new_eye - eye_off;
        let mut vel = link.dir(ctrl.vel);
        let (yaw, pitch, roll) = carry_view(&link, ctrl.yaw, ctrl.pitch);
        // Out of a floor portal: at least fast enough to get her feet out of it, and a little
        // nudge forward so she lands beside it rather than back in.
        if out.normal.y > 0.7 {
            let depth = (out.center.y - feet.y).max(0.0) + 0.3;
            let need = (2.0 * ctrl.tuning.gravity * depth).sqrt();
            if vel.y < need {
                vel.y = need;
            }
            let h = Vec3::new(vel.x, 0.0, vel.z);
            if h.length() < 1.5 {
                let fwd = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
                vel += fwd * (1.5 - h.length());
            }
        }
        let keep_ground = link.from.normal.y.abs() < 0.3 && out.normal.y.abs() < 0.3;
        self.player_env = Some(entered.other());
        // If she doesn't fit where she comes out (the opening's edge), find the nearest place
        // she does.
        let body = ctrl.body();
        {
            let world = self.player_world();
            if world.overlaps(&body.aabb(feet)) {
                let mut found = None;
                'search: for k in 0..=24 {
                    let along = k as f32 * 0.05;
                    for (du, dr) in [(0.0, 0.0), (0.1, 0.0), (-0.1, 0.0), (0.0, 0.1), (0.0, -0.1), (0.25, 0.0), (-0.25, 0.0), (0.0, 0.25), (0.0, -0.25), (0.5, 0.0), (-0.5, 0.0)] {
                        let f = feet + out.normal * along + out.up * du + out.right() * dr;
                        if !world.overlaps(&body.aabb(f)) {
                            found = Some(f);
                            break 'search;
                        }
                    }
                }
                if let Some(f) = found {
                    feet = f;
                }
            }
        }
        ctrl.teleport(feet, vel, yaw, pitch, keep_ground);
        self.roll = roll;
        self.events.push(SimEvent::Teleported { entered, roll });
        // What she's carrying comes along, if it was on her side.
        if let Some(h) = self.held {
            let p = &mut self.props[h];
            if link.from.signed_distance(p.pos) > -CUBE_HALF {
                p.pos = link.point(p.pos);
                p.vel = link.dir(p.vel);
                p.rot = link.quat() * p.rot;
                p.env = Some(entered.other());
            }
        }
    }

    fn player_died(&mut self, ctrl: &mut Controller, respawn: bool) {
        if let Some(h) = self.held.take() {
            self.events.push(SimEvent::Drop { id: self.props[h].id });
        }
        if respawn {
            ctrl.respawn();
        }
        self.player_env = None;
        self.burn = 0.0;
        self.events.push(SimEvent::PlayerDied);
    }

    /// Go back to checkpoint `i` (or the last one reached).
    pub fn respawn_at(&mut self, ctrl: &mut Controller, i: Option<usize>) {
        let i = i.unwrap_or(self.checkpoint).min(self.level.checkpoints.len().saturating_sub(1));
        if let Some(cp) = self.level.checkpoints.get(i) {
            ctrl.spawn = cp.spawn;
            ctrl.spawn_yaw = cp.yaw;
            self.checkpoint = i;
        }
        if let Some(h) = self.held.take() {
            self.events.push(SimEvent::Drop { id: self.props[h].id });
        }
        ctrl.respawn();
        self.player_env = None;
        self.burn = 0.0;
    }

    /// Use (E): drop what she's carrying, or pick up the cube she's looking at, or press the
    /// button in front of her.
    pub fn use_key(&mut self, aim: Aim) {
        if let Some(h) = self.held.take() {
            self.events.push(SimEvent::Drop { id: self.props[h].id });
            return;
        }
        let (legs, hit) = self.trace(aim.eye, aim.dir, USE_REACH, Mask { fizzlers: false, ..Mask::SHOT }, 1);
        let _ = legs;
        match hit.map(|h| h.what) {
            Some(HitWhat::Prop(i)) if self.props[i].fizzle.is_none() => {
                self.held = Some(i);
                self.held_far = 0.0;
                self.events.push(SimEvent::PickUp { id: self.props[i].id });
            }
            Some(HitWhat::Static(s)) => {
                if let Owner::Pedestal(i) = self.statics[s].owner {
                    let p = &mut self.pedestals[i];
                    p.left = if p.time > 0.0 { p.time } else if p.left > 0.0 { 0.0 } else { 1.0 };
                    self.events.push(SimEvent::PedestalPressed { at: p.at });
                }
            }
            _ => {
                // A pedestal button is easy to miss by a hair: anything close in front counts.
                let near = self.pedestals.iter().position(|p| {
                    let top = p.at + Vec3::Y * 1.0;
                    let (d, _) = geom::segment_point(aim.eye, aim.eye + aim.dir * USE_REACH, top);
                    d < 0.45
                });
                if let Some(i) = near {
                    let p = &mut self.pedestals[i];
                    p.left = if p.time > 0.0 { p.time } else if p.left > 0.0 { 0.0 } else { 1.0 };
                    self.events.push(SimEvent::PedestalPressed { at: p.at });
                }
            }
        }
    }

    /// Let go of what she's carrying with a push along `dir`.
    pub fn throw(&mut self, dir: Vec3, speed: f32) {
        if let Some(h) = self.held.take() {
            self.props[h].vel += dir * speed;
            self.events.push(SimEvent::Drop { id: self.props[h].id });
        }
    }

    // ------------------------------------------------------------ everything else

    /// Advance the puzzle pieces by `dt` (after [`Sim::step_player`]).
    pub fn step(&mut self, dt: f32, ctrl: &Controller) {
        self.time += dt;
        for p in self.portals.iter_mut().flatten() {
            p.age += dt;
        }
        self.roll *= (-dt / 0.35).exp();
        let aim = Aim::of(ctrl);
        let player_box = ctrl.body().aabb(ctrl.feet);

        self.update_signals(dt);
        self.update_doors(dt);
        for i in 0..self.fizzlers.len() {
            self.fizzlers[i].on = self.fizzlers[i].inputs.eval(&self.signals);
        }
        for i in 0..self.plates.len() {
            self.plates[i].on = self.plates[i].inputs.eval(&self.signals);
            self.plates[i].fired += dt;
        }
        self.update_bridges();
        self.update_droppers(dt);
        self.update_props(dt, aim, player_box);
        self.update_gel(dt);
        self.update_lasers();
        self.update_buttons(ctrl);
    }

    fn update_signals(&mut self, dt: f32) {
        for p in &mut self.pedestals {
            if p.time > 0.0 {
                p.left = (p.left - dt).max(0.0);
            }
        }
        let mut sig = vec![false; self.signals.len()];
        let mut set = |s: Sig| {
            if s < sig.len() {
                sig[s] = true;
            }
        };
        self.buttons.iter().filter(|b| b.pressed).for_each(|b| set(b.sig));
        self.pedestals.iter().filter(|p| p.left > 0.0).for_each(|p| set(p.sig));
        self.catchers.iter().filter(|c| c.lit).for_each(|c| set(c.sig));
        self.relays.iter().filter(|r| r.lit).for_each(|r| set(r.sig));
        self.signals = sig;
    }

    fn update_doors(&mut self, dt: f32) {
        for d in &mut self.doors {
            let want = d.inputs.eval(&self.signals);
            let before = d.open;
            d.open = if want { (d.open + dt * 2.0).min(1.0) } else { (d.open - dt * 2.0).max(0.0) };
            if before == 0.0 && d.open > 0.0 {
                self.events.push(SimEvent::DoorOpening { at: d.b.center() });
            }
            if before == 1.0 && d.open < 1.0 {
                self.events.push(SimEvent::DoorClosing { at: d.b.center() });
            }
        }
    }

    fn update_bridges(&mut self) {
        for i in 0..self.bridges.len() {
            let on = self.bridges[i].inputs.eval(&self.signals);
            self.bridges[i].on = on;
            if !on {
                self.bridges[i].segs.clear();
                continue;
            }
            let (mut o, mut d, mut up) = (self.bridges[i].at, self.bridges[i].dir, self.bridges[i].up);
            let mut segs = Vec::new();
            for _ in 0..5 {
                let hit = self.raycast(o + d * 1e-3, d, 120.0, Mask::BRIDGE);
                let end = hit.map(|h| h.point).unwrap_or(o + d * 120.0);
                segs.push(bridge_seg(o, end, d, up));
                match hit.map(|h| h.what) {
                    Some(HitWhat::Portal(c)) => {
                        let link = self.link(c).unwrap();
                        o = link.point(end);
                        d = link.dir(d);
                        up = link.dir(up);
                        // Snap: portals are axis-aligned, so these stay exact.
                        d = geom::snap_axis(d);
                        up = geom::snap_axis(up);
                    }
                    _ => break,
                }
            }
            self.bridges[i].segs = segs;
        }
    }

    fn update_droppers(&mut self, dt: f32) {
        for i in 0..self.droppers.len() {
            self.droppers[i].opened += dt;
            let on = self.droppers[i].inputs.eval(&self.signals);
            let rising = on && !self.droppers[i].was_on;
            self.droppers[i].was_on = on;
            let alive = self.droppers[i].cube.is_some_and(|id| self.props.iter().any(|p| p.id == id && p.alive && p.fizzle.is_none()));
            if rising {
                if alive {
                    // Asked again: the old one goes, a new one comes.
                    let id = self.droppers[i].cube.unwrap();
                    if let Some(p) = self.props.iter_mut().find(|p| p.id == id) {
                        p.fizzle = Some(0.0);
                        self.events.push(SimEvent::CubeFizzled { at: p.pos });
                    }
                }
                self.droppers[i].refill = Some(if alive { 1.2 } else { 0.0 });
            } else if !alive && self.droppers[i].cube.is_some() && self.droppers[i].refill.is_none() && on {
                self.droppers[i].refill = Some(1.0);
            }
            if let Some(t) = &mut self.droppers[i].refill {
                *t -= dt;
                if *t <= 0.0 {
                    self.droppers[i].refill = None;
                    let (kind, at) = (self.droppers[i].kind, self.droppers[i].at);
                    let id = self.spawn_prop(kind, at, Some(i));
                    self.droppers[i].cube = Some(id);
                    self.droppers[i].opened = 0.0;
                    self.events.push(SimEvent::CubeDropped { at });
                }
            }
        }
    }

    fn spawn_prop(&mut self, kind: CubeKind, at: Vec3, dropper: Option<usize>) -> u32 {
        let id = self.next_prop;
        self.next_prop += 1;
        // Reuse a dead slot so indices stay small.
        let p = Prop {
            id,
            kind,
            pos: at,
            vel: Vec3::ZERO,
            rot: Quat::IDENTITY,
            env: None,
            coat: None,
            fizzle: None,
            dropper,
            grounded: false,
            alive: true,
        };
        if let Some(slot) = self.props.iter().position(|q| !q.alive) {
            if self.held.is_none_or(|h| h != slot) {
                self.props[slot] = p;
                return id;
            }
        }
        self.props.push(p);
        id
    }

    /// Where a held cube should be: in front of her eye, through a portal if she's looking
    /// into one, and not inside a wall.
    fn hold_target(&self, aim: Aim, skip: usize) -> (Vec3, Quat) {
        let dist = 1.5;
        let mask = Mask { skip_prop: Some(skip), fizzlers: false, ..Mask::SOLID };
        let mut rot = Quat::from_rotation_y(aim.yaw);
        let mut o = aim.eye;
        let mut d = aim.dir;
        let mut left = dist;
        // Through a portal she's looking into.
        if let Some(h) = self.raycast(o, d, left, Mask { portals: true, ..mask }) {
            if let HitWhat::Portal(c) = h.what {
                let link = self.link(c).unwrap();
                left -= h.t;
                rot = link.quat() * rot;
                d = link.dir(d);
                o = link.point(h.point) + d * 0.01;
            }
        }
        let t = match self.raycast(o, d, left + CUBE_HALF, mask) {
            Some(h) => (h.t - CUBE_HALF * 1.2).max(0.0),
            None => left,
        };
        (o + d * t, rot)
    }

    fn update_props(&mut self, dt: f32, aim: Aim, player_box: Aabb) {
        let linked = self.linked();
        // The held cube's pull.
        if let Some(h) = self.held {
            if !self.props[h].alive || self.props[h].fizzle.is_some() {
                self.held = None;
            } else {
                let (target, rot) = self.hold_target(aim, h);
                let p = &mut self.props[h];
                let to = target - p.pos;
                p.vel = (to * 14.0).clamp_length_max(25.0);
                p.rot = p.rot.slerp(rot, (dt * 12.0).min(1.0));
                // Lost it behind something: let go.
                if to.length() > 2.2 {
                    self.held_far += dt;
                    if self.held_far > 0.4 {
                        self.held = None;
                        self.events.push(SimEvent::Drop { id: p.id });
                    }
                } else {
                    self.held_far = 0.0;
                }
            }
        }
        for i in 0..self.props.len() {
            if !self.props[i].alive {
                continue;
            }
            if let Some(t) = self.props[i].fizzle {
                let p = &mut self.props[i];
                p.fizzle = Some(t + dt);
                p.vel *= (-4.0 * dt).exp();
                p.pos += (p.vel + Vec3::Y * 0.25) * dt;
                if t + dt > 1.4 {
                    p.alive = false;
                    if self.held == Some(i) {
                        self.held = None;
                    }
                }
                continue;
            }
            let held = self.held == Some(i);
            let half = Vec3::splat(CUBE_HALF);
            // Which portal it's going through.
            let mut env = self.props[i].env;
            collide::update_env(&mut env, &self.portals, linked, self.props[i].pos, half, self.props[i].vel, dt);
            self.props[i].env = env;
            let mut moving = self.moving_boxes(Some(i), true);
            if !held {
                moving.push(player_box);
            }
            let world = BodyWorld {
                still: self.still_for(env),
                moving: plain(moving),
                gel: &self.gel,
                fixtures: &[],
            };
            let mut p = self.props[i];
            if !held {
                p.vel.y -= GRAVITY * dt;
            }
            if p.vel.length() > TERMINAL {
                p.vel = p.vel.normalize() * TERMINAL;
            }
            let bouncy = p.coat == Some(Paint::Bounce);
            let before = p.pos;
            let delta = p.vel * dt;
            let n = (delta.length() / (CUBE_HALF * 0.8)).ceil().max(1.0) as usize;
            let mut landed: Option<f32> = None;
            for _ in 0..n {
                let d = delta / n as f32;
                if let Some(nrm) = move_box(&world, half, &mut p.pos, 1, d.y) {
                    let floor_gel = world.gel(p.pos - Vec3::Y * CUBE_HALF, Vec3::Y);
                    if nrm.y > 0.5 && p.vel.y < -2.0 && (bouncy || floor_gel == faith_move::Gel::Bounce) {
                        p.vel.y = (-p.vel.y * 0.9).max(7.0);
                    } else {
                        if nrm.y > 0.5 && p.vel.y < -3.0 {
                            landed = Some(-p.vel.y);
                        }
                        p.vel.y = 0.0;
                    }
                }
                for axis in [0usize, 2] {
                    if let Some(nrm) = move_box(&world, half, &mut p.pos, axis, d[axis]) {
                        let wall_gel = world.gel(p.pos - nrm * CUBE_HALF, nrm);
                        if bouncy || wall_gel == faith_move::Gel::Bounce {
                            p.vel[axis] = -p.vel[axis] * 0.9;
                        } else {
                            p.vel[axis] = 0.0;
                        }
                    }
                }
            }
            // On the ground: friction (none to speak of on propulsion gel).
            let below = world.sweep(half, p.pos, Vec3::NEG_Y * 0.03);
            p.grounded = below.is_some();
            if p.grounded && !held {
                let slick = world.gel(p.pos - Vec3::Y * CUBE_HALF, Vec3::Y) == faith_move::Gel::Speed;
                let decel = if slick { 1.0 } else { 14.0 };
                let h = Vec3::new(p.vel.x, 0.0, p.vel.z);
                let s = h.length();
                if s > 0.0 {
                    let ns = (s - decel * dt).max(0.0);
                    p.vel.x *= ns / s;
                    p.vel.z *= ns / s;
                }
                // Settle to the nearest quarter turn as it comes to rest.
                if s < 0.5 {
                    let (yaw, _, _) = p.rot.to_euler(glam::EulerRot::YXZ);
                    let snap = (yaw / std::f32::consts::FRAC_PI_2).round() * std::f32::consts::FRAC_PI_2;
                    let rest = Quat::from_rotation_y(if p.kind == CubeKind::Reflector { yaw } else { snap });
                    p.rot = p.rot.slerp(rest, (dt * 6.0).min(1.0));
                }
            }
            if let Some(speed) = landed {
                self.events.push(SimEvent::CubeLanded { at: p.pos, speed });
            }
            // Through a portal: its centre crossed one inside the opening.
            if linked {
                if let Some(c) = p.env {
                    let portal = self.portals[c.index()].unwrap();
                    let (a, b) = (portal.signed_distance(before), portal.signed_distance(p.pos));
                    if a >= 0.0 && b < 0.0 && portal.in_rect(p.pos, -0.1) {
                        let link = self.link(c).unwrap();
                        p.pos = link.point(p.pos);
                        p.vel = link.dir(p.vel);
                        p.rot = link.quat() * p.rot;
                        p.env = Some(c.other());
                        // Out of a floor portal: enough to clear it.
                        if link.to.normal.y > 0.7 {
                            let need = (2.0 * GRAVITY * (CUBE_HALF * 2.0 + 0.2)).sqrt();
                            p.vel.y = p.vel.y.max(need);
                        }
                        self.events.push(SimEvent::PropTeleported { id: p.id, at: p.pos });
                    }
                }
            }
            let pb = p.aabb();
            // Grills vaporise it; goo and the void lose it.
            if self.fizzlers.iter().any(|f| f.on && f.b.overlaps(&pb)) {
                p.fizzle = Some(0.0);
                self.events.push(SimEvent::CubeFizzled { at: p.pos });
                if held {
                    self.held = None;
                }
            } else if self.goo.iter().any(|g| geom::contains(g, p.pos)) || p.pos.y < self.level.kill_y {
                p.alive = false;
                if held {
                    self.held = None;
                }
                self.events.push(SimEvent::CubeFizzled { at: p.pos });
            }
            self.props[i] = p;
        }
    }

    fn update_buttons(&mut self, ctrl: &Controller) {
        for i in 0..self.buttons.len() {
            let b = self.buttons[i];
            let top = b.at.y + BUTTON_TOP;
            let on_it = |p: Vec3, bottom: f32| {
                let d = Vec3::new(p.x - b.at.x, 0.0, p.z - b.at.z);
                d.length() < BUTTON_RADIUS && (bottom - top).abs() < 0.15
            };
            let by_cube = self.props.iter().enumerate().any(|(j, p)| {
                p.alive && p.fizzle.is_none() && Some(j) != self.held && on_it(p.pos, p.pos.y - CUBE_HALF)
            });
            let by_player = !b.cube_only && on_it(ctrl.feet, ctrl.feet.y);
            let pressed = by_cube || by_player;
            if pressed != b.pressed {
                self.events.push(if pressed { SimEvent::ButtonDown { at: b.at } } else { SimEvent::ButtonUp { at: b.at } });
            }
            self.buttons[i].pressed = pressed;
            self.buttons[i].by_cube = by_cube;
        }
    }

    fn update_gel(&mut self, dt: f32) {
        for i in 0..self.dispensers.len() {
            let on = self.dispensers[i].inputs.eval(&self.signals);
            self.dispensers[i].on = on;
            if !on {
                continue;
            }
            self.dispensers[i].acc += dt * self.dispensers[i].rate;
            while self.dispensers[i].acc >= 1.0 {
                self.dispensers[i].acc -= 1.0;
                let d = &self.dispensers[i];
                let (at, dir, speed, paint) = (d.at, d.dir, d.speed, d.paint);
                let jitter = Vec3::new(self.rng.signed(), self.rng.signed(), self.rng.signed());
                let v = dir * speed * (0.9 + 0.2 * self.rng.f32()) + jitter * 0.7;
                if self.blobs.len() < 800 {
                    self.blobs.push(Blob { p: at + jitter * 0.12, v, paint, r: 0.09 + 0.05 * self.rng.f32(), age: 0.0 });
                }
            }
        }
        let mut blobs = std::mem::take(&mut self.blobs);
        let mut keep = Vec::with_capacity(blobs.len());
        for mut b in blobs.drain(..) {
            b.age += dt;
            b.v.y -= GRAVITY * dt;
            let step = b.v * dt;
            let len = step.length();
            if len < 1e-6 {
                keep.push(b);
                continue;
            }
            let d = step / len;
            match self.raycast(b.p, d, len, Mask::GEL) {
                Some(h) => match h.what {
                    HitWhat::Portal(c) => {
                        let link = self.link(c).unwrap();
                        b.v = link.dir(b.v);
                        b.p = link.point(h.point) + link.dir(d) * 0.01;
                        keep.push(b);
                    }
                    HitWhat::Prop(i) => {
                        let p = &mut self.props[i];
                        p.coat = if b.paint == Paint::Water || b.paint == Paint::Portal { None } else { Some(b.paint) };
                    }
                    _ => {
                        let r = 0.55 + 0.35 * self.rng.f32();
                        self.gel.add(Splat { p: h.point, n: geom::snap_axis(h.normal), r, paint: b.paint });
                        if self.rng.f32() < 0.25 {
                            self.events.push(SimEvent::GelSplat { at: h.point, paint: b.paint });
                        }
                    }
                },
                None => {
                    b.p += step;
                    if b.age < 8.0 && b.p.y > self.level.kill_y {
                        keep.push(b);
                    }
                }
            }
        }
        self.blobs = keep;
    }

    fn update_lasers(&mut self) {
        let mut lit_catchers = vec![false; self.catchers.len()];
        let mut lit_relays = vec![false; self.relays.len()];
        for li in 0..self.lasers.len() {
            let on = self.lasers[li].inputs.eval(&self.signals);
            self.lasers[li].on = on;
            self.lasers[li].path.clear();
            if !on {
                continue;
            }
            let (mut o, mut d) = (self.lasers[li].at, self.lasers[li].dir);
            let mut skip: Option<usize> = None;
            let mut path = Vec::new();
            for _ in 0..12 {
                let hit = self.raycast(o, d, 150.0, Mask { skip_prop: skip, ..Mask::LASER });
                let end = hit.map(|h| h.point).unwrap_or(o + d * 150.0);
                path.push((o, end));
                skip = None;
                match hit.map(|h| (h.what, h.normal)) {
                    Some((HitWhat::Portal(c), _)) => {
                        let link = self.link(c).unwrap();
                        d = link.dir(d);
                        o = link.point(end) + d * 2e-3;
                    }
                    Some((HitWhat::Prop(i), _)) if self.props[i].kind == CubeKind::Reflector && self.props[i].fizzle.is_none() => {
                        // In through the cube, out of its lens.
                        let p = self.props[i];
                        path.last_mut().unwrap().1 = p.pos;
                        d = p.front();
                        o = p.pos;
                        skip = Some(i);
                    }
                    Some((HitWhat::Static(s), n)) => {
                        if let Owner::Catcher(c) = self.statics[s].owner {
                            if n.dot(self.catchers[c].facing) > 0.5 {
                                lit_catchers[c] = true;
                            }
                        }
                        break;
                    }
                    _ => break,
                }
            }
            for (a, b) in &path {
                for (ri, r) in self.relays.iter().enumerate() {
                    let pole = Aabb::new(r.at + Vec3::new(-0.25, 0.0, -0.25), r.at + Vec3::new(0.25, 2.0, 0.25));
                    let d = *b - *a;
                    let len = d.length();
                    if len > 1e-4 && ray_aabb(*a, d / len, &pole).is_some_and(|(t, _)| t <= len) {
                        lit_relays[ri] = true;
                    }
                }
            }
            self.lasers[li].path = path;
        }
        for (i, c) in self.catchers.iter_mut().enumerate() {
            if lit_catchers[i] != c.lit {
                self.events.push(if lit_catchers[i] { SimEvent::CatcherOn { at: c.at } } else { SimEvent::CatcherOff { at: c.at } });
            }
            c.lit = lit_catchers[i];
        }
        for (i, r) in self.relays.iter_mut().enumerate() {
            r.lit = lit_relays[i];
        }
    }

    /// The hint for where Faith stands.
    pub fn hint(&self, feet: Vec3) -> Option<&'static str> {
        self.level.hints.iter().find(|(b, _)| geom::contains(b, feet + Vec3::Y * 0.5)).map(|(_, t)| *t)
    }
}

/// One straight piece of bridge from `a` to `b`: its box, the walkable top along `a`-`b`.
fn bridge_seg(a: Vec3, b: Vec3, dir: Vec3, up: Vec3) -> BridgeSeg {
    let w = dir.cross(up).normalize_or_zero();
    let corners = [
        a + w * BRIDGE_HALF_WIDTH,
        a - w * BRIDGE_HALF_WIDTH,
        b + w * BRIDGE_HALF_WIDTH,
        b - w * BRIDGE_HALF_WIDTH,
        a + w * BRIDGE_HALF_WIDTH - up * BRIDGE_THICK,
        b - w * BRIDGE_HALF_WIDTH - up * BRIDGE_THICK,
    ];
    let mut lo = corners[0];
    let mut hi = corners[0];
    for c in corners {
        lo = lo.min(c);
        hi = hi.max(c);
    }
    BridgeSeg { a, b, up, aabb: Aabb::new(lo, hi) }
}

/// The launch velocity that carries something from `from` to land on `to`, peaking `apex`
/// metres above the higher of the two, under `g`.
pub fn ballistic(from: Vec3, to: Vec3, apex: f32, g: f32) -> Vec3 {
    let top = from.y.max(to.y) + apex.max(0.1);
    let vy = (2.0 * g * (top - from.y)).sqrt();
    let t_up = vy / g;
    let t_down = (2.0 * (top - to.y) / g).sqrt();
    let t = t_up + t_down;
    let h = Vec3::new(to.x - from.x, 0.0, to.z - from.z) / t;
    Vec3::new(h.x, vy, h.z)
}

/// A handful of boxes, scanned in full (no grid).
fn plain(boxes: Vec<Aabb>) -> BoxWorld {
    let mut w = BoxWorld::default();
    w.boxes = boxes;
    w
}
