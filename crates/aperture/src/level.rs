//! A test chamber, as data: boxes of some material, the puzzle pieces, checkpoints.
//!
//! Levels are written in Rust with [`Level`]'s builder methods: lay down rooms and boxes, cut
//! doorways and pits out of them with [`Level::cut`], then place the puzzle pieces. Every
//! coordinate is in metres, Y up; Faith's yaw 0 looks down -Z.

use glam::{Vec2, Vec3};

use faith_move::greybox::{self, Checkpoint, Look};
use faith_move::{Aabb, Fixture};

use crate::geom;
use crate::portal::PortalColor;

/// What a box is made of: what it looks like, and whether portals stick to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mat {
    /// White panels: portals go here.
    White,
    /// Dark metal: no portals.
    Black,
    /// Glass: solid, see-through, no portals; stops portal shots and lasers.
    Glass,
    /// Grating: solid to people and cubes, but shots and lasers go through.
    Grate,
    /// Mirror's Edge red: the way to go. No portals.
    Runner,
    /// Crates, pipes, machinery. No portals.
    Prop,
    /// Mirror's Edge rooftops (concrete): portals go here.
    Roof,
    /// Mirror's Edge walls (concrete): portals go here.
    Wall,
    /// Concrete that portals don't stick to (rooftops built as courses).
    Concrete,
}

impl Mat {
    pub fn portalable(self) -> bool {
        matches!(self, Mat::White | Mat::Roof | Mat::Wall)
    }

    /// Whether shots and lasers go through it.
    pub fn see_through(self) -> bool {
        matches!(self, Mat::Grate)
    }

    /// Mirror's Edge's footstep set for it (faith_move's greybox looks).
    pub fn look(self) -> Look {
        match self {
            Mat::White | Mat::Wall => Look::Wall,
            Mat::Roof | Mat::Concrete => Look::Roof,
            Mat::Black | Mat::Grate => Look::Runner,
            Mat::Runner => Look::Runner,
            Mat::Glass | Mat::Prop => Look::Prop,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solid {
    pub b: Aabb,
    pub mat: Mat,
}

/// A signal wire: buttons, laser catchers and relays drive one; doors, bridges, lasers and the
/// rest listen.
pub type Sig = usize;

/// What turns something on: every listed signal on (none listed: always on), or with
/// `invert`, not that.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inputs {
    pub all: Vec<Sig>,
    pub invert: bool,
}

impl Inputs {
    pub fn always() -> Self {
        Inputs::default()
    }

    pub fn on(sig: Sig) -> Self {
        Inputs { all: vec![sig], invert: false }
    }

    pub fn all(sigs: &[Sig]) -> Self {
        Inputs { all: sigs.to_vec(), invert: false }
    }

    pub fn off(sig: Sig) -> Self {
        Inputs { all: vec![sig], invert: true }
    }

    pub fn eval(&self, sigs: &[bool]) -> bool {
        let v = self.all.iter().all(|s| sigs.get(*s).copied().unwrap_or(false));
        v != self.invert
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CubeKind {
    /// Weighted storage cube.
    Storage,
    /// Weighted companion cube.
    Companion,
    /// Discouragement redirection cube: a laser that hits it comes out of its front.
    Reflector,
}

/// Portal 2's paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Paint {
    /// Repulsion gel (blue): bounce.
    Bounce,
    /// Propulsion gel (orange): speed.
    Speed,
    /// Conversion gel (white): portals stick to it.
    Portal,
    /// Cleansing water: washes gel off.
    Water,
}

impl Paint {
    pub fn gel(self) -> faith_move::Gel {
        match self {
            Paint::Bounce => faith_move::Gel::Bounce,
            Paint::Speed => faith_move::Gel::Speed,
            _ => faith_move::Gel::None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A floor button (the 1500 Megawatt Aperture Science Heavy Duty Super-Colliding Super
    /// Button). `at` is the floor under its middle; a cube (or Faith, unless `cube_only`)
    /// standing on it holds it down.
    FloorButton { sig: Sig, at: Vec3, cube_only: bool },
    /// A button on a post: use it (E) and it stays on for `time` seconds, ticking.
    Pedestal { sig: Sig, at: Vec3, time: f32 },
    /// A door that's open while its inputs are on.
    Door { b: Aabb, inputs: Inputs },
    /// A cube sitting there at the start (`at` is its centre).
    Cube { at: Vec3, kind: CubeKind },
    /// A cube dropper hanging over `at` (the new cube's centre): drops one when its inputs come
    /// on (always on: at the start), and a new one whenever the last is destroyed.
    Dropper { at: Vec3, kind: CubeKind, inputs: Inputs },
    /// A material emancipation grill: walking through it closes your portals, carrying a cube
    /// through it vaporises the cube, and shots don't get past it.
    Fizzler { b: Aabb, inputs: Inputs },
    /// An aerial faith plate on the floor at `at`: throws whoever steps on it to land on
    /// `target`, peaking `apex` metres above the higher end.
    FaithPlate { at: Vec3, target: Vec3, apex: f32, inputs: Inputs },
    /// A hard light bridge from the emitter face at `at`, out along `dir`, walkable side `up`.
    Bridge { at: Vec3, dir: Vec3, up: Vec3, inputs: Inputs },
    /// A thermal discouragement beam (laser) from `at` along `dir`.
    Laser { at: Vec3, dir: Vec3, inputs: Inputs },
    /// A laser catcher whose lens is at `at`, facing `facing`.
    Catcher { sig: Sig, at: Vec3, facing: Vec3 },
    /// A laser relay standing on the floor at `at`: on while a laser goes through it.
    Relay { sig: Sig, at: Vec3 },
    /// A gel pipe at `at` pouring `paint` along `dir` (`rate` blobs a second at `speed` m/s).
    GelDispenser { at: Vec3, dir: Vec3, paint: Paint, inputs: Inputs, rate: f32, speed: f32 },
    /// Gel already painted on a flat region of a surface (a box flat along `normal`).
    Painted { b: Aabb, normal: Vec3, paint: Paint },
    /// Deadly goo (or a bottomless drop): falling in kills you, and cubes are lost.
    Goo { b: Aabb },
}

/// A cable from a button to what it drives, drawn on the walls and floor (Portal's
/// indicator lights: dots that turn on with the signal).
#[derive(Clone, Debug, PartialEq)]
pub struct Wire {
    pub sig: Sig,
    pub points: Vec<Vec3>,
}

/// A portal placed by the level, not the gun.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresetPortal {
    pub color: PortalColor,
    pub center: Vec3,
    pub normal: Vec3,
    pub up: Vec3,
}

/// Which portals the gun can shoot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gun {
    pub blue: bool,
    pub orange: bool,
}

impl Gun {
    pub const BOTH: Gun = Gun { blue: true, orange: true };
    pub const NONE: Gun = Gun { blue: false, orange: false };
}

#[derive(Clone, Debug)]
pub struct Level {
    pub name: &'static str,
    /// A line shown when the chamber starts.
    pub intro: &'static str,
    pub solids: Vec<Solid>,
    /// Painted-on markings (no collision): stripes, arrows, signs.
    pub decor: Vec<(Aabb, Mat)>,
    pub checkpoints: Vec<Checkpoint>,
    /// Mirror's Edge's ziplines, swing poles, balance beams and ladders.
    pub fixtures: Vec<Fixture>,
    pub items: Vec<Item>,
    pub wires: Vec<Wire>,
    pub portals: Vec<PresetPortal>,
    pub gun: Gun,
    /// Walking in here finishes the chamber.
    pub exit: Option<Aabb>,
    /// Messages shown while you stand in a region.
    pub hints: Vec<(Aabb, &'static str)>,
    /// Falling below this kills you.
    pub kill_y: f32,
    /// Indoors (a test chamber: dark sky, lamps) or out on the roofs.
    pub indoor: bool,
    /// Ceiling lamps (indoor): positions.
    pub lamps: Vec<Vec3>,
}

impl Level {
    pub fn new(name: &'static str) -> Self {
        Level {
            name,
            intro: "",
            solids: Vec::new(),
            decor: Vec::new(),
            checkpoints: Vec::new(),
            fixtures: Vec::new(),
            items: Vec::new(),
            wires: Vec::new(),
            portals: Vec::new(),
            gun: Gun::BOTH,
            exit: None,
            hints: Vec::new(),
            kill_y: -30.0,
            indoor: true,
            lamps: Vec::new(),
        }
    }

    // ------------------------------------------------------------ building

    /// A solid box from corner to corner.
    pub fn solid(&mut self, mat: Mat, a: [f32; 3], b: [f32; 3]) -> &mut Self {
        self.solids.push(Solid { b: Aabb::new(Vec3::from(a), Vec3::from(b)), mat });
        self
    }

    /// A painted marking (no collision).
    pub fn decor(&mut self, mat: Mat, a: [f32; 3], b: [f32; 3]) -> &mut Self {
        self.decor.push((Aabb::new(Vec3::from(a), Vec3::from(b)), mat));
        self
    }

    /// A closed room: the inside runs from `a` to `b`, with walls `t` thick round it. Floor,
    /// ceiling and walls are each their own material.
    pub fn room(&mut self, a: [f32; 3], b: [f32; 3], t: f32, floor: Mat, walls: Mat, ceiling: Mat) -> &mut Self {
        let (a, b) = (Vec3::from(a).min(Vec3::from(b)), Vec3::from(a).max(Vec3::from(b)));
        let s = |m: Mat, lo: Vec3, hi: Vec3| Solid { b: Aabb::new(lo, hi), mat: m };
        self.solids.extend([
            s(floor, Vec3::new(a.x - t, a.y - t, a.z - t), Vec3::new(b.x + t, a.y, b.z + t)),
            s(ceiling, Vec3::new(a.x - t, b.y, a.z - t), Vec3::new(b.x + t, b.y + t, b.z + t)),
            s(walls, Vec3::new(a.x - t, a.y, a.z - t), Vec3::new(a.x, b.y, b.z + t)),
            s(walls, Vec3::new(b.x, a.y, a.z - t), Vec3::new(b.x + t, b.y, b.z + t)),
            s(walls, Vec3::new(a.x, a.y, a.z - t), Vec3::new(b.x, b.y, a.z)),
            s(walls, Vec3::new(a.x, a.y, b.z), Vec3::new(b.x, b.y, b.z + t)),
        ]);
        self
    }

    /// Cut a box out of every solid (doorways, windows, pits).
    pub fn cut(&mut self, a: [f32; 3], b: [f32; 3]) -> &mut Self {
        let cut = Aabb::new(Vec3::from(a), Vec3::from(b));
        let mut out = Vec::with_capacity(self.solids.len());
        for s in &self.solids {
            out.extend(geom::subtract(&s.b, &cut).into_iter().map(|b| Solid { b, mat: s.mat }));
        }
        self.solids = out;
        self
    }

    /// Re-make the solids inside a region of another material (a strip of black metal
    /// across a white wall, say).
    pub fn repaint(&mut self, mat: Mat, a: [f32; 3], b: [f32; 3]) -> &mut Self {
        let region = Aabb::new(Vec3::from(a), Vec3::from(b));
        let mut out = Vec::with_capacity(self.solids.len());
        for s in &self.solids {
            if !s.b.overlaps(&region) {
                out.push(*s);
                continue;
            }
            out.extend(geom::subtract(&s.b, &region).into_iter().map(|b| Solid { b, mat: s.mat }));
            let inter = Aabb::new(s.b.min.max(region.min), s.b.max.min(region.max));
            out.push(Solid { b: inter, mat });
        }
        self.solids = out;
        self
    }

    /// The first checkpoint: where you start, facing `yaw_deg` (0 = -Z, 90 = -X).
    pub fn spawn(&mut self, at: [f32; 3], yaw_deg: f32) -> &mut Self {
        let p = Vec3::from(at);
        self.checkpoints.insert(
            0,
            Checkpoint { name: "Start", region: Aabb::from_center_size(p, Vec3::new(3.0, 3.0, 3.0)), spawn: p, yaw: yaw_deg.to_radians() },
        );
        self
    }

    /// Walking into `a`-`b` makes `at` the respawn point.
    pub fn checkpoint(&mut self, name: &'static str, a: [f32; 3], b: [f32; 3], at: [f32; 3], yaw_deg: f32) -> &mut Self {
        self.checkpoints.push(Checkpoint {
            name,
            region: Aabb::new(Vec3::from(a), Vec3::from(b)),
            spawn: Vec3::from(at),
            yaw: yaw_deg.to_radians(),
        });
        self
    }

    pub fn exit(&mut self, a: [f32; 3], b: [f32; 3]) -> &mut Self {
        self.exit = Some(Aabb::new(Vec3::from(a), Vec3::from(b)));
        self
    }

    pub fn hint(&mut self, a: [f32; 3], b: [f32; 3], text: &'static str) -> &mut Self {
        self.hints.push((Aabb::new(Vec3::from(a), Vec3::from(b)), text));
        self
    }

    pub fn item(&mut self, item: Item) -> &mut Self {
        self.items.push(item);
        self
    }

    pub fn button(&mut self, sig: Sig, at: [f32; 3]) -> &mut Self {
        self.item(Item::FloorButton { sig, at: Vec3::from(at), cube_only: false })
    }

    pub fn cube_button(&mut self, sig: Sig, at: [f32; 3]) -> &mut Self {
        self.item(Item::FloorButton { sig, at: Vec3::from(at), cube_only: true })
    }

    pub fn pedestal(&mut self, sig: Sig, at: [f32; 3], time: f32) -> &mut Self {
        self.item(Item::Pedestal { sig, at: Vec3::from(at), time })
    }

    pub fn door(&mut self, inputs: Inputs, a: [f32; 3], b: [f32; 3]) -> &mut Self {
        self.item(Item::Door { b: Aabb::new(Vec3::from(a), Vec3::from(b)), inputs })
    }

    pub fn cube(&mut self, kind: CubeKind, at: [f32; 3]) -> &mut Self {
        self.item(Item::Cube { at: Vec3::from(at), kind })
    }

    pub fn dropper(&mut self, kind: CubeKind, at: [f32; 3], inputs: Inputs) -> &mut Self {
        self.item(Item::Dropper { at: Vec3::from(at), kind, inputs })
    }

    pub fn fizzler(&mut self, a: [f32; 3], b: [f32; 3], inputs: Inputs) -> &mut Self {
        self.item(Item::Fizzler { b: Aabb::new(Vec3::from(a), Vec3::from(b)), inputs })
    }

    pub fn faith_plate(&mut self, at: [f32; 3], target: [f32; 3], apex: f32) -> &mut Self {
        self.item(Item::FaithPlate { at: Vec3::from(at), target: Vec3::from(target), apex, inputs: Inputs::always() })
    }

    pub fn bridge(&mut self, at: [f32; 3], dir: [f32; 3], inputs: Inputs) -> &mut Self {
        self.item(Item::Bridge { at: Vec3::from(at), dir: Vec3::from(dir), up: Vec3::Y, inputs })
    }

    pub fn laser(&mut self, at: [f32; 3], dir: [f32; 3], inputs: Inputs) -> &mut Self {
        self.item(Item::Laser { at: Vec3::from(at), dir: Vec3::from(dir), inputs })
    }

    pub fn catcher(&mut self, sig: Sig, at: [f32; 3], facing: [f32; 3]) -> &mut Self {
        self.item(Item::Catcher { sig, at: Vec3::from(at), facing: Vec3::from(facing) })
    }

    pub fn relay(&mut self, sig: Sig, at: [f32; 3]) -> &mut Self {
        self.item(Item::Relay { sig, at: Vec3::from(at) })
    }

    pub fn gel_pipe(&mut self, paint: Paint, at: [f32; 3], dir: [f32; 3], inputs: Inputs) -> &mut Self {
        self.item(Item::GelDispenser { at: Vec3::from(at), dir: Vec3::from(dir), paint, inputs, rate: 30.0, speed: 7.0 })
    }

    /// Paint a flat region of a floor (normal +Y), wall or ceiling.
    pub fn paint(&mut self, paint: Paint, a: [f32; 3], b: [f32; 3], normal: [f32; 3]) -> &mut Self {
        self.item(Item::Painted { b: Aabb::new(Vec3::from(a), Vec3::from(b)), normal: Vec3::from(normal), paint })
    }

    pub fn goo(&mut self, a: [f32; 3], b: [f32; 3]) -> &mut Self {
        self.item(Item::Goo { b: Aabb::new(Vec3::from(a), Vec3::from(b)) })
    }

    pub fn wire(&mut self, sig: Sig, points: &[[f32; 3]]) -> &mut Self {
        self.wires.push(Wire { sig, points: points.iter().map(|p| Vec3::from(*p)).collect() });
        self
    }

    pub fn portal(&mut self, color: PortalColor, center: [f32; 3], normal: [f32; 3], up: [f32; 3]) -> &mut Self {
        self.portals.push(PresetPortal { color, center: Vec3::from(center), normal: Vec3::from(normal), up: Vec3::from(up) });
        self
    }

    pub fn lamp(&mut self, at: [f32; 3]) -> &mut Self {
        self.lamps.push(Vec3::from(at));
        self
    }

    // ------------------------------------------------------------ reading

    /// How many signal wires the level uses.
    pub fn signal_count(&self) -> usize {
        let mut n = 0;
        let mut see = |s: Sig| n = n.max(s + 1);
        for it in &self.items {
            match it {
                Item::FloorButton { sig, .. } | Item::Pedestal { sig, .. } | Item::Catcher { sig, .. } | Item::Relay { sig, .. } => see(*sig),
                Item::Door { inputs, .. }
                | Item::Dropper { inputs, .. }
                | Item::Fizzler { inputs, .. }
                | Item::FaithPlate { inputs, .. }
                | Item::Bridge { inputs, .. }
                | Item::Laser { inputs, .. }
                | Item::GelDispenser { inputs, .. } => inputs.all.iter().for_each(|s| see(*s)),
                _ => {}
            }
        }
        for w in &self.wires {
            see(w.sig);
        }
        n
    }

    /// Mirror's Edge's view of the level (for its footstep sounds and checkpoints).
    pub fn greybox(&self) -> greybox::Level {
        greybox::Level {
            name: self.name,
            finish: self.exit,
            solids: self.solids.iter().map(|s| (s.b, s.mat.look())).collect(),
            decor: Vec::new(),
            checkpoints: self.checkpoints.clone(),
            fixtures: self.fixtures.clone(),
        }
    }

    /// A Mirror's Edge course as a level: roofs and walls take portals.
    pub fn from_greybox(g: &greybox::Level) -> Level {
        let mut l = Level::new(g.name);
        l.indoor = false;
        l.kill_y = g.solids.iter().map(|(b, _)| b.min.y).fold(f32::INFINITY, f32::min).min(0.0) - 2.0;
        for (b, look) in &g.solids {
            let mat = match look {
                Look::Roof => Mat::Roof,
                Look::Wall => Mat::Wall,
                Look::Runner => Mat::Runner,
                Look::Prop => Mat::Prop,
                Look::Finish => Mat::Runner,
                Look::Skyline => Mat::Prop,
            };
            l.solids.push(Solid { b: *b, mat });
        }
        for (b, look) in &g.decor {
            l.decor.push((*b, if *look == Look::Finish { Mat::Runner } else { Mat::Runner }));
        }
        l.checkpoints = g.checkpoints.clone();
        l.fixtures = g.fixtures.clone();
        l.exit = g.finish;
        l
    }

    /// The material of the static solid whose face at `p` faces `n`, if any.
    pub fn surface_at(&self, p: Vec3, n: Vec3) -> Option<(usize, Mat)> {
        let inside = p - n * 0.02;
        self.solids.iter().enumerate().find(|(_, s)| geom::contains(&s.b, inside)).map(|(i, s)| (i, s.mat))
    }
}

/// A rectangle on a surface, for painting: its centre, and half sizes along two in-plane
/// axes.
pub fn face_rect(b: &Aabb, normal: Vec3) -> (Vec3, Vec3, Vec3, Vec2) {
    let n = geom::snap_axis(normal);
    let (ua, va) = if n.x.abs() > 0.5 {
        (Vec3::Z, Vec3::Y)
    } else if n.y.abs() > 0.5 {
        (Vec3::X, Vec3::Z)
    } else {
        (Vec3::X, Vec3::Y)
    };
    let c = b.center();
    let s = b.size() * 0.5;
    // The face: at the box's side the normal points out of (for a flat region, its middle).
    let face = c + n * (s.dot(n.abs()));
    (face, ua, va, Vec2::new(s.dot(ua), s.dot(va)))
}
