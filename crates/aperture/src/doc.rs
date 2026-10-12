//! A level as a file: what the level editor saves and loads (RON), and makes into a [`Level`].
//!
//! A [`LevelDoc`] is a list of [`Object`]s, each a [`Kind`] (a box of some material, a puzzle
//! piece, the start...) standing somewhere. Boxes are given by their centre and size; pieces by
//! the point the matching [`Level`] builder method takes, and, for the ones that point
//! somewhere (lasers, bridges, catchers), a yaw and pitch: yaw 0 looks down -Z, 90° down -X,
//! as Faith's does. A built chamber can be read back into one ([`LevelDoc::from_level`]) as a
//! starting point: its boxes as they end up, after the rooms are cut.

use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};

use faith_move::climb::Ladder;
use faith_move::{Aabb, Fixture};

use crate::level::{CubeKind, Gun, Inputs, Item, Launch, Level, Mat, MeProp, ModelProp, MoverDef, Paint, PresetPortal, Sig, Solid, NO_MODEL};
use crate::portal::PortalColor;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelDoc {
    pub name: String,
    #[serde(default)]
    pub intro: String,
    #[serde(default = "yes")]
    pub indoor: bool,
    #[serde(default = "kill_y")]
    pub kill_y: f32,
    #[serde(default = "yes")]
    pub blue: bool,
    #[serde(default = "yes")]
    pub orange: bool,
    #[serde(default)]
    pub objects: Vec<Object>,
}

fn yes() -> bool {
    true
}

fn kill_y() -> f32 {
    -30.0
}

/// One thing in the level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub kind: Kind,
    /// A box's centre; a piece's point (see [`Kind`]).
    pub at: Vec3,
    /// A box's size (unused by pieces).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub size: Vec3,
    /// Degrees: which way it faces (turned about Y, then tipped about X, then rolled about Z).
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub yaw: f32,
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub pitch: f32,
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub roll: f32,
}

/// How something can be turned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Turns {
    /// Round (yaw): the start, a turret.
    Yaw,
    /// Round and up and down (yaw and pitch): what points somewhere, a laser, a catcher.
    Aim,
    /// Any way (about X, Y and Z): props.
    Free,
}

/// A prop's scale: along its own X, Y and Z (a single number scales all three alike).
mod scale3 {
    use glam::Vec3;
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec3, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum S {
            One(f32),
            Three(Vec3),
        }
        Ok(match S::deserialize(d)? {
            S::One(s) => Vec3::splat(s),
            S::Three(v) => v,
        })
    }

    pub fn one() -> Vec3 {
        Vec3::ONE
    }
}

fn is_zero(v: &Vec3) -> bool {
    *v == Vec3::ZERO
}

fn is_zero_f(v: &f32) -> bool {
    *v == 0.0
}

/// What an object is. The ones marked "box" use the object's `size`; the rest are a point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Kind {
    /// Box: solid.
    Block { mat: Mat },
    /// Box: painted on, no collision.
    Decor { mat: Mat },
    /// Box: removed from every block (doorways, windows, pits).
    Cut,
    /// Where Faith starts (the floor under her), facing `yaw`.
    Spawn,
    /// Box: walking in makes it the respawn point, `spawn` from its centre, facing `yaw`.
    Checkpoint { name: String, spawn: Vec3 },
    /// Box: walking in finishes the chamber.
    Exit,
    /// Box: shows `text` while Faith stands in it.
    Hint { text: String },
    /// A ceiling light.
    Lamp,
    /// The floor under its middle.
    FloorButton { sig: Sig, #[serde(default)] cube_only: bool },
    /// The floor under it; on for `time` seconds (0: toggles).
    Pedestal { sig: Sig, time: f32 },
    /// Box.
    Door { inputs: Inputs },
    /// Its centre.
    Cube { cube: CubeKind },
    /// The new cube's centre.
    Dropper { cube: CubeKind, inputs: Inputs },
    /// Box.
    Fizzler { inputs: Inputs },
    /// The floor under it; throws to land on `target` (in the world), `apex` m above the higher end.
    FaithPlate { target: Vec3, apex: f32, #[serde(default)] inputs: Inputs },
    /// The emitter's face, shining the way it faces.
    Bridge { inputs: Inputs },
    /// Where the beam starts, firing the way it faces.
    Laser { inputs: Inputs },
    /// Its lens, facing the way it faces.
    Catcher { sig: Sig },
    /// The floor under it.
    Relay { sig: Sig },
    /// Pouring the way it faces.
    GelPipe { paint: Paint, inputs: Inputs, #[serde(default = "gel_rate")] rate: f32, #[serde(default = "gel_speed")] speed: f32 },
    /// Box (flat): gel painted on the surface it lies on, facing `normal`.
    Painted { paint: Paint, normal: Vec3 },
    /// Box: deadly.
    Goo,
    /// The floor under it, facing `yaw`.
    Turret,
    /// Firing the way it faces.
    PelletLauncher { inputs: Inputs, #[serde(default = "pellet_life")] life: f32 },
    /// Its lens, facing the way it faces.
    PelletCatcher { sig: Sig },
    /// Box: a black platform travelling `travel` and back at `speed` while its inputs are on.
    Platform { travel: Vec3, speed: f32, inputs: Inputs },
    /// A portal already open there, facing the way it faces.
    Portal { color: PortalColor },
    /// Indicator lights from here along `points` (from here), lit with `sig`.
    Wire { sig: Sig, points: Vec<Vec3> },
    /// A zipline from here (its high end) down to here + `to`.
    ZipLine { to: Vec3 },
    /// A swing bar from here to here + `to`.
    SwingPole { to: Vec3 },
    /// A balance beam along the centre of its top, from here to here + `to`; with `solid`, the
    /// beam itself (a black box under the line), else it's a block of its own.
    Beam { to: Vec3, #[serde(default)] solid: bool },
    /// Box: a door Faith barges or kicks open, swinging away from the way it faces.
    BargeDoor,
    /// Box: barbed wire (she stumbles in it).
    BarbedWire,
    /// Box: a soft landing (mattress, cardboard); with `solid`, the pad itself, else it marks
    /// the top of a block.
    SoftPad { #[serde(default)] solid: bool },
    /// A ladder (or drainpipe) on a wall: its foot here, on the wall's face, facing out of the
    /// wall; she climbs out `height` up.
    Ladder { height: f32, #[serde(default)] pipe: bool, #[serde(default = "yes")] exit: bool },
    /// A Mirror's Edge static mesh, standing here, turned `yaw`. `bounds` is its box (from here,
    /// before it's turned or scaled); with `solid`, that's a block too (turned with it a quarter
    /// at a time), drawn instead when the game isn't installed.
    MeProp { package: String, mesh: String, #[serde(default = "scale3::one", deserialize_with = "scale3::deserialize")] scale: Vec3, #[serde(default)] bounds: Option<(Vec3, Vec3)>, #[serde(default)] solid: bool },
    /// A model from the extracted props (`props/portal2/...glb`, under the extracted folder),
    /// standing here, turned `yaw`; `bounds` and `solid` as a Mirror's Edge prop's.
    Model { path: String, #[serde(default = "scale3::one", deserialize_with = "scale3::deserialize")] scale: Vec3, #[serde(default)] bounds: Option<(Vec3, Vec3)>, #[serde(default)] solid: bool },
}

fn gel_rate() -> f32 {
    30.0
}

fn gel_speed() -> f32 {
    7.0
}

fn pellet_life() -> f32 {
    12.0
}

impl Kind {
    /// Whether it's sized as a box.
    pub fn is_box(&self) -> bool {
        matches!(
            self,
            Kind::Block { .. }
                | Kind::Decor { .. }
                | Kind::Cut
                | Kind::Checkpoint { .. }
                | Kind::Exit
                | Kind::Hint { .. }
                | Kind::Door { .. }
                | Kind::Fizzler { .. }
                | Kind::Painted { .. }
                | Kind::Goo
                | Kind::Platform { .. }
                | Kind::BargeDoor
                | Kind::BarbedWire
                | Kind::SoftPad { .. }
        )
    }

    /// How it can be turned, if which way it faces matters.
    pub fn turns(&self) -> Option<Turns> {
        match self {
            Kind::Bridge { .. } | Kind::Laser { .. } | Kind::Catcher { .. } | Kind::GelPipe { .. } | Kind::PelletLauncher { .. } | Kind::PelletCatcher { .. } | Kind::Portal { .. } => Some(Turns::Aim),
            Kind::MeProp { .. } | Kind::Model { .. } => Some(Turns::Free),
            Kind::Spawn | Kind::Turret | Kind::Checkpoint { .. } | Kind::BargeDoor | Kind::Ladder { .. } => Some(Turns::Yaw),
            _ => None,
        }
    }

    /// What it's called in the editor.
    pub fn label(&self) -> String {
        match self {
            Kind::Block { mat } => format!("{mat:?} block"),
            Kind::Decor { mat } => format!("{mat:?} decor"),
            Kind::Cut => "Cut".into(),
            Kind::Spawn => "Spawn".into(),
            Kind::Checkpoint { name, .. } => format!("Checkpoint '{name}'"),
            Kind::Exit => "Exit".into(),
            Kind::Hint { .. } => "Hint".into(),
            Kind::Lamp => "Lamp".into(),
            Kind::FloorButton { sig, cube_only } => format!("{} button -> {sig}", if *cube_only { "Cube" } else { "Floor" }),
            Kind::Pedestal { sig, .. } => format!("Pedestal button -> {sig}"),
            Kind::Door { inputs } => format!("Door {}", inputs_label(inputs)),
            Kind::Cube { cube } => format!("{cube:?} cube"),
            Kind::Dropper { cube, inputs } => format!("{cube:?} dropper {}", inputs_label(inputs)),
            Kind::Fizzler { inputs } => format!("Grill {}", inputs_label(inputs)),
            Kind::FaithPlate { .. } => "Faith plate".into(),
            Kind::Bridge { inputs } => format!("Light bridge {}", inputs_label(inputs)),
            Kind::Laser { inputs } => format!("Laser {}", inputs_label(inputs)),
            Kind::Catcher { sig } => format!("Laser catcher -> {sig}"),
            Kind::Relay { sig } => format!("Laser relay -> {sig}"),
            Kind::GelPipe { paint, .. } => format!("{paint:?} gel pipe"),
            Kind::Painted { paint, .. } => format!("{paint:?} gel"),
            Kind::Goo => "Goo".into(),
            Kind::Turret => "Turret".into(),
            Kind::PelletLauncher { inputs, .. } => format!("Pellet launcher {}", inputs_label(inputs)),
            Kind::PelletCatcher { sig } => format!("Pellet catcher -> {sig}"),
            Kind::Platform { inputs, .. } => format!("Platform {}", inputs_label(inputs)),
            Kind::Portal { color } => format!("{color:?} portal"),
            Kind::Wire { sig, .. } => format!("Wire {sig}"),
            Kind::ZipLine { .. } => "Zipline".into(),
            Kind::SwingPole { .. } => "Swing pole".into(),
            Kind::Beam { .. } => "Balance beam".into(),
            Kind::BargeDoor => "Barge door".into(),
            Kind::BarbedWire => "Barbed wire".into(),
            Kind::SoftPad { .. } => "Soft landing".into(),
            Kind::Ladder { pipe, .. } => if *pipe { "Drainpipe" } else { "Ladder" }.into(),
            Kind::MeProp { mesh, .. } => mesh.clone(),
            Kind::Model { path, .. } => path.rsplit('/').next().unwrap_or(path).trim_end_matches(".glb").to_string(),
        }
    }

    /// The signal it drives, if it's a source.
    pub fn drives(&self) -> Option<Sig> {
        match self {
            Kind::FloorButton { sig, .. } | Kind::Pedestal { sig, .. } | Kind::Catcher { sig } | Kind::Relay { sig } | Kind::PelletCatcher { sig } | Kind::Wire { sig, .. } => Some(*sig),
            _ => None,
        }
    }

    /// What turns it on, if it listens.
    pub fn inputs(&self) -> Option<&Inputs> {
        match self {
            Kind::Door { inputs }
            | Kind::Dropper { inputs, .. }
            | Kind::Fizzler { inputs }
            | Kind::FaithPlate { inputs, .. }
            | Kind::Bridge { inputs }
            | Kind::Laser { inputs }
            | Kind::GelPipe { inputs, .. }
            | Kind::PelletLauncher { inputs, .. }
            | Kind::Platform { inputs, .. } => Some(inputs),
            _ => None,
        }
    }
}

/// A balance beam's own box: 30 cm wide and deep, under the line along its top.
fn beam_box(at: Vec3, to: Vec3) -> Aabb {
    let (a, b) = (at, at + to);
    let w = Vec3::new(0.15, 0.0, 0.15);
    Aabb::new(a.min(b) - w - Vec3::Y * 0.3, a.max(b) + w)
}

fn inputs_label(i: &Inputs) -> String {
    if i.all.is_empty() {
        return String::new();
    }
    let list = i.all.iter().map(|s| s.to_string()).collect::<Vec<_>>().join("+");
    format!("<- {}{list}", if i.invert { "!" } else { "" })
}

impl Object {
    pub fn new(kind: Kind, at: Vec3) -> Self {
        Object { kind, at, size: Vec3::ZERO, yaw: 0.0, pitch: 0.0, roll: 0.0 }
    }

    pub fn sized(kind: Kind, at: Vec3, size: Vec3) -> Self {
        Object { kind, at, size, yaw: 0.0, pitch: 0.0, roll: 0.0 }
    }

    /// A prop's box in the world: `b` (from it, before it's turned or scaled) scaled, turned
    /// as it's turned, and placed: the box round that (a quarter turn about any axis keeps it
    /// exact; other turns make it bigger than the prop).
    pub fn prop_box(&self, b: (Vec3, Vec3), scale: Vec3) -> Aabb {
        let turn = self.rotation();
        let (lo, hi) = (b.0 * scale, b.1 * scale);
        let (mut a, mut z) = (Vec3::MAX, Vec3::MIN);
        for k in 0..8 {
            let c = turn * Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z });
            a = a.min(c);
            z = z.max(c);
        }
        // (Rounded: a quarter turn of a float isn't exactly one.)
        let round = |v: Vec3| (v * 1e4).round() / 1e4;
        Aabb::new(self.at + round(a), self.at + round(z))
    }

    /// A world box round this prop, as its box from it (before it's turned or scaled): what
    /// [`Object::prop_box`] takes back (exactly, turned a quarter at a time).
    pub fn unturned_box(&self, b: &Aabb, scale: Vec3) -> (Vec3, Vec3) {
        let back = self.rotation().inverse();
        let (mut a, mut z) = (Vec3::MAX, Vec3::MIN);
        for k in 0..8 {
            let c = back * (Vec3::new(if k & 1 == 0 { b.min.x } else { b.max.x }, if k & 2 == 0 { b.min.y } else { b.max.y }, if k & 4 == 0 { b.min.z } else { b.max.z }) - self.at);
            a = a.min(c);
            z = z.max(c);
        }
        let round = |v: Vec3| (v * 1e4).round() / 1e4;
        (round(a / scale), round(z / scale))
    }

    pub fn with_size(mut self, size: Vec3) -> Self {
        self.size = size;
        self
    }

    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw.to_radians(), self.pitch.to_radians(), self.roll.to_radians())
    }

    /// The way it faces (exactly along an axis when it's meant to be: no rounding noise).
    pub fn dir(&self) -> Vec3 {
        let d = self.rotation() * Vec3::NEG_Z;
        let d = Vec3::select(d.abs().cmplt(Vec3::splat(1e-6)), Vec3::ZERO, d);
        d.normalize()
    }

    /// Turned as `q` is.
    pub fn set_rotation(&mut self, q: Quat) {
        let (y, p, r) = q.to_euler(EulerRot::YXZ);
        // (Rounded to a thousandth of a degree: no noise in the file.)
        let d = |a: f32| (a.to_degrees() * 1000.0).round() / 1000.0;
        (self.yaw, self.pitch, self.roll) = (d(y), d(p), d(r));
    }

    /// Turn it to face `dir`.
    pub fn face(&mut self, dir: Vec3) {
        let d = dir.normalize_or(Vec3::NEG_Z);
        self.yaw = (-d.x).atan2(-d.z).to_degrees();
        self.pitch = d.y.clamp(-1.0, 1.0).asin().to_degrees();
    }

    /// The box it takes up: a box's own, or roughly the piece's (for picking it out).
    pub fn bounds(&self) -> Aabb {
        if self.kind.is_box() {
            return Aabb::from_center_size(self.at, self.size.max(Vec3::splat(0.02)));
        }
        let up = |w: f32, h: f32| Aabb::from_center_size(self.at + Vec3::Y * (h * 0.5), Vec3::new(w, h, w));
        let around = |r: f32| Aabb::from_center_size(self.at, Vec3::splat(r * 2.0));
        match &self.kind {
            Kind::Spawn => up(0.6, 1.8),
            Kind::FloorButton { .. } => up(1.9, 0.15),
            Kind::Pedestal { .. } => up(0.5, 1.1),
            Kind::FaithPlate { .. } => up(1.6, 0.2),
            Kind::Relay { .. } => up(0.6, 1.2),
            Kind::Turret => up(0.7, 1.2),
            Kind::Cube { .. } | Kind::Dropper { .. } => around(0.45),
            Kind::Portal { .. } => {
                let r = self.rotation();
                let (n, u) = (r * Vec3::NEG_Z, r * Vec3::Y);
                let side = n.cross(u);
                let ext = (side.abs() * 0.7 + u.abs() * 1.2 + n.abs() * 0.05).max(Vec3::splat(0.05));
                Aabb::new(self.at - ext, self.at + ext)
            }
            Kind::Wire { points, .. } => {
                let mut b = Aabb::from_center_size(self.at, Vec3::splat(0.2));
                for p in points {
                    let q = self.at + *p;
                    b = Aabb::new(b.min.min(q - 0.1), b.max.max(q + 0.1));
                }
                b
            }
            Kind::ZipLine { to } | Kind::SwingPole { to } => {
                let (a, b) = (self.at, self.at + *to);
                Aabb::new(a.min(b) - 0.1, a.max(b) + 0.1)
            }
            Kind::Beam { to, .. } => beam_box(self.at, *to),
            Kind::Ladder { height, .. } => {
                let n = self.dir();
                let side = Vec3::Y.cross(n).normalize_or(Vec3::X);
                let (lo, hi) = (self.at - side.abs() * 0.3, self.at + side.abs() * 0.3 + n * 0.3 + Vec3::Y * *height);
                Aabb::new(lo.min(hi), lo.max(hi))
            }
            Kind::MeProp { bounds: Some(b), scale, .. } | Kind::Model { bounds: Some(b), scale, .. } => self.prop_box(*b, *scale),
            Kind::MeProp { .. } | Kind::Model { .. } => up(1.0, 1.0),
            _ => around(0.35),
        }
    }
}

impl LevelDoc {
    pub fn new(name: &str) -> Self {
        LevelDoc { name: name.into(), intro: String::new(), indoor: true, kill_y: -30.0, blue: true, orange: true, objects: Vec::new() }
    }

    /// A white room to start from: floor, walls, ceiling, a lamp grid and the start.
    pub fn starter(name: &str) -> Self {
        let mut d = LevelDoc::new(name);
        let (w, h, l, t) = (12.0, 6.0, 16.0, 1.0);
        let b = |mat: Mat, lo: Vec3, hi: Vec3| Object::sized(Kind::Block { mat }, (lo + hi) * 0.5, hi - lo);
        let (lo, hi) = (Vec3::new(-w / 2.0, 0.0, -l), Vec3::new(w / 2.0, h, 0.0));
        d.objects.extend([
            b(Mat::White, Vec3::new(lo.x - t, -t, lo.z - t), Vec3::new(hi.x + t, 0.0, hi.z + t)),
            b(Mat::Black, Vec3::new(lo.x - t, h, lo.z - t), Vec3::new(hi.x + t, h + t, hi.z + t)),
            b(Mat::White, Vec3::new(lo.x - t, 0.0, lo.z - t), Vec3::new(lo.x, h, hi.z + t)),
            b(Mat::White, Vec3::new(hi.x, 0.0, lo.z - t), Vec3::new(hi.x + t, h, hi.z + t)),
            b(Mat::White, Vec3::new(lo.x, 0.0, lo.z - t), Vec3::new(hi.x, h, lo.z)),
            b(Mat::White, Vec3::new(lo.x, 0.0, hi.z), Vec3::new(hi.x, h, hi.z + t)),
        ]);
        for x in [-3.0, 3.0] {
            for z in [-4.0, -12.0] {
                d.objects.push(Object::new(Kind::Lamp, Vec3::new(x, h - 0.05, z)));
            }
        }
        d.objects.push(Object::new(Kind::Spawn, Vec3::new(0.0, 0.0, -2.0)));
        d
    }

    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default().struct_names(false)).unwrap_or_default()
    }

    pub fn from_ron(text: &str) -> Result<LevelDoc, String> {
        ron::from_str(text).map_err(|e| e.to_string())
    }

    /// Make it into a level. Its words live as long as the program (a level's are `'static`):
    /// a few bytes each time the editor rebuilds it.
    pub fn build(&self) -> Level {
        let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
        let mut l = Level::new(leak(&self.name));
        l.intro = leak(&self.intro);
        l.indoor = self.indoor;
        l.kill_y = self.kill_y;
        l.gun = Gun { blue: self.blue, orange: self.orange };
        let corners = |o: &Object| {
            let b = o.bounds();
            (b.min.to_array(), b.max.to_array())
        };
        for o in &self.objects {
            match &o.kind {
                Kind::Block { mat } => l.solids.push(Solid { b: o.bounds(), mat: *mat }),
                Kind::MeProp { bounds: Some(_), solid: true, .. } | Kind::Model { bounds: Some(_), solid: true, .. } => l.solids.push(Solid { b: o.bounds(), mat: Mat::Prop }),
                Kind::Beam { solid: true, .. } => l.solids.push(Solid { b: o.bounds(), mat: Mat::Black }),
                Kind::SoftPad { solid: true } => l.solids.push(Solid { b: o.bounds(), mat: Mat::Prop }),
                _ => {}
            }
        }
        for o in &self.objects {
            if let Kind::Cut = o.kind {
                let (a, b) = corners(o);
                l.cut(a, b);
            }
        }
        let mut spawn = None;
        for o in &self.objects {
            let at = o.at;
            let dir = o.dir().to_array();
            let (a, b) = corners(o);
            match &o.kind {
                Kind::Block { .. } | Kind::Cut => {}
                Kind::Decor { mat } => {
                    l.decor(*mat, a, b);
                }
                Kind::Spawn => {
                    spawn.get_or_insert((at, o.yaw));
                }
                Kind::Checkpoint { name, spawn } => {
                    l.checkpoint(leak(name), a, b, (at + *spawn).to_array(), o.yaw);
                }
                Kind::Exit => {
                    l.exit(a, b);
                }
                Kind::Hint { text } => {
                    l.hint(a, b, leak(text));
                }
                Kind::Lamp => {
                    l.lamp(at.to_array());
                }
                Kind::FloorButton { sig, cube_only } => {
                    l.item(Item::FloorButton { sig: *sig, at, cube_only: *cube_only });
                }
                Kind::Pedestal { sig, time } => {
                    l.pedestal(*sig, at.to_array(), *time);
                }
                Kind::Door { inputs } => {
                    l.door(inputs.clone(), a, b);
                }
                Kind::Cube { cube } => {
                    l.cube(*cube, at.to_array());
                }
                Kind::Dropper { cube, inputs } => {
                    l.dropper(*cube, at.to_array(), inputs.clone());
                }
                Kind::Fizzler { inputs } => {
                    l.fizzler(a, b, inputs.clone());
                }
                Kind::FaithPlate { target, apex, inputs } => {
                    l.item(Item::FaithPlate { at, target: *target, apex: *apex, inputs: inputs.clone() });
                }
                Kind::Bridge { inputs } => {
                    l.item(Item::Bridge { at, dir: o.dir(), up: o.rotation() * Vec3::Y, inputs: inputs.clone() });
                }
                Kind::Laser { inputs } => {
                    l.laser(at.to_array(), dir, inputs.clone());
                }
                Kind::Catcher { sig } => {
                    l.catcher(*sig, at.to_array(), dir);
                }
                Kind::Relay { sig } => {
                    l.relay(*sig, at.to_array());
                }
                Kind::GelPipe { paint, inputs, rate, speed } => {
                    l.item(Item::GelDispenser { at, dir: o.dir(), paint: *paint, inputs: inputs.clone(), rate: *rate, speed: *speed });
                }
                Kind::Painted { paint, normal } => {
                    l.paint(*paint, a, b, normal.to_array());
                }
                Kind::Goo => {
                    l.goo(a, b);
                }
                Kind::Turret => {
                    l.turret(at.to_array(), o.yaw);
                }
                Kind::PelletLauncher { inputs, life } => {
                    l.item(Item::PelletLauncher { at, dir: o.dir(), inputs: inputs.clone(), life: *life, launch: Launch::default() });
                }
                Kind::PelletCatcher { sig } => {
                    l.pellet_catcher(*sig, at.to_array(), dir);
                }
                Kind::Platform { travel, speed, inputs } => {
                    l.platform(a, b, travel.to_array(), *speed, inputs.clone());
                }
                Kind::Portal { color } => {
                    l.portals.push(PresetPortal { color: *color, center: at, normal: o.dir(), up: o.rotation() * Vec3::Y });
                }
                Kind::Wire { sig, points } => {
                    let pts: Vec<[f32; 3]> = std::iter::once(at).chain(points.iter().map(|p| at + *p)).map(|p| p.to_array()).collect();
                    l.wire(*sig, &pts);
                }
                Kind::ZipLine { to } => l.fixtures.push(Fixture::ZipLine { a: at, b: at + *to }),
                Kind::SwingPole { to } => l.fixtures.push(Fixture::SwingPole { a: at, b: at + *to }),
                Kind::Beam { to, .. } => l.fixtures.push(Fixture::Beam { a: at, b: at + *to }),
                Kind::BargeDoor => l.fixtures.push(Fixture::Door { b: o.bounds(), n: -o.dir() }),
                Kind::BarbedWire => l.fixtures.push(Fixture::BarbedWire { b: o.bounds() }),
                Kind::SoftPad { .. } => l.fixtures.push(Fixture::SoftPad { b: o.bounds() }),
                Kind::Ladder { height, pipe, exit } => {
                    l.fixtures.push(Fixture::Ladder(Ladder { base: at, top: at.y + *height, normal: o.dir(), pipe: *pipe, exit: *exit }));
                }
                Kind::MeProp { package, mesh, scale, bounds, solid } => {
                    let stand_in = (bounds.is_some() && *solid).then(|| o.bounds());
                    l.me_props.push(MeProp { package: leak(package), mesh: leak(mesh), at, turn: o.rotation(), scale: *scale, stand_in });
                }
                Kind::Model { path, scale, bounds, solid } => {
                    let solid = (bounds.is_some() && *solid).then(|| o.bounds());
                    l.models.push(ModelProp { path: path.clone(), at, turn: o.rotation(), scale: *scale, solid });
                }
            }
        }
        let (at, yaw) = spawn.unwrap_or((Vec3::ZERO, 0.0));
        l.spawn(at.to_array(), yaw);
        l
    }

    /// A built level as objects, and how many of its things have no object to be (Mirror's
    /// Edge's fixtures, a Portal map's machinery).
    pub fn from_level(l: &Level) -> (LevelDoc, usize) {
        let mut d = LevelDoc::new(l.name);
        d.intro = l.intro.to_string();
        d.indoor = l.indoor;
        d.kill_y = l.kill_y;
        d.blue = l.gun.blue;
        d.orange = l.gun.orange;
        let mut skipped = l.slopes.len() + l.lights.len();
        let boxed = |kind: Kind, b: &Aabb| Object::sized(kind, b.center(), b.size());
        let facing = |kind: Kind, at: Vec3, dir: Vec3| {
            let mut o = Object::new(kind, at);
            o.face(dir);
            o
        };
        let push = |d: &mut LevelDoc, o: Object| d.objects.push(o);
        for s in &l.solids {
            push(&mut d, boxed(Kind::Block { mat: s.mat }, &s.b));
        }
        for (b, mat) in &l.decor {
            push(&mut d, boxed(Kind::Decor { mat: *mat }, b));
        }
        for (i, cp) in l.checkpoints.iter().enumerate() {
            let mut o = if i == 0 {
                Object::new(Kind::Spawn, cp.spawn)
            } else {
                boxed(Kind::Checkpoint { name: cp.name.to_string(), spawn: cp.spawn - cp.region.center() }, &cp.region)
            };
            o.yaw = cp.yaw.to_degrees();
            push(&mut d, o);
        }
        if let Some(b) = &l.exit {
            push(&mut d, boxed(Kind::Exit, b));
        }
        for f in &l.fixtures {
            let o = match f {
                Fixture::ZipLine { a, b } => Object::new(Kind::ZipLine { to: *b - *a }, *a),
                Fixture::SwingPole { a, b } => Object::new(Kind::SwingPole { to: *b - *a }, *a),
                // (Its beam is one of the level's blocks already.)
                Fixture::Beam { a, b } => Object::new(Kind::Beam { to: *b - *a, solid: false }, *a),
                Fixture::Door { b, n } => facing(Kind::BargeDoor, b.center(), -*n).with_size(b.size()),
                Fixture::BarbedWire { b } => boxed(Kind::BarbedWire, b),
                Fixture::SoftPad { b } => boxed(Kind::SoftPad { solid: false }, b),
                Fixture::Ladder(lad) => facing(Kind::Ladder { height: lad.top - lad.base.y, pipe: lad.pipe, exit: lad.exit }, lad.base, lad.normal),
            };
            push(&mut d, o);
        }
        for (b, text) in &l.hints {
            push(&mut d, boxed(Kind::Hint { text: text.to_string() }, b));
        }
        for p in &l.lamps {
            push(&mut d, Object::new(Kind::Lamp, *p));
        }
        for it in &l.items {
            let o = match it {
                Item::FloorButton { sig, at, cube_only } => Object::new(Kind::FloorButton { sig: *sig, cube_only: *cube_only }, *at),
                Item::Pedestal { sig, at, time } => Object::new(Kind::Pedestal { sig: *sig, time: *time }, *at),
                Item::Door { b, inputs } => boxed(Kind::Door { inputs: inputs.clone() }, b),
                Item::Cube { at, kind } => Object::new(Kind::Cube { cube: *kind }, *at),
                Item::Dropper { at, kind, inputs } => Object::new(Kind::Dropper { cube: *kind, inputs: inputs.clone() }, *at),
                Item::Fizzler { b, inputs } => boxed(Kind::Fizzler { inputs: inputs.clone() }, b),
                Item::FaithPlate { at, target, apex, inputs } => Object::new(Kind::FaithPlate { target: *target, apex: *apex, inputs: inputs.clone() }, *at),
                Item::Bridge { at, dir, inputs, .. } => facing(Kind::Bridge { inputs: inputs.clone() }, *at, *dir),
                Item::Laser { at, dir, inputs } => facing(Kind::Laser { inputs: inputs.clone() }, *at, *dir),
                Item::Catcher { sig, at, facing: f } => facing(Kind::Catcher { sig: *sig }, *at, *f),
                Item::Relay { sig, at } => Object::new(Kind::Relay { sig: *sig }, *at),
                Item::GelDispenser { at, dir, paint, inputs, rate, speed } => facing(Kind::GelPipe { paint: *paint, inputs: inputs.clone(), rate: *rate, speed: *speed }, *at, *dir),
                Item::Painted { b, normal, paint } => boxed(Kind::Painted { paint: *paint, normal: *normal }, b),
                Item::Goo { b } => boxed(Kind::Goo, b),
                Item::PelletLauncher { at, dir, inputs, life, .. } => facing(Kind::PelletLauncher { inputs: inputs.clone(), life: *life }, *at, *dir),
                Item::PelletCatcher { sig, at, facing: f } => facing(Kind::PelletCatcher { sig: *sig }, *at, *f),
                Item::Turret { at, yaw } => {
                    let mut o = Object::new(Kind::Turret, *at);
                    o.yaw = yaw.to_degrees();
                    o
                }
                Item::Mover(MoverDef { model, boxes, path, speed, inputs: Some(inputs), .. }) if *model == NO_MODEL && boxes.len() == 1 && path.len() == 2 => {
                    boxed(Kind::Platform { travel: path[1] - path[0], speed: *speed, inputs: inputs.clone() }, &boxes[0].b)
                }
                _ => {
                    skipped += 1;
                    continue;
                }
            };
            push(&mut d, o);
        }
        for p in &l.portals {
            let mut o = facing(Kind::Portal { color: p.color }, p.center, p.normal);
            // Portals on floors and ceilings: keep their up.
            if p.normal.y.abs() > 0.9 {
                o.yaw = (-p.up.x).atan2(-p.up.z).to_degrees();
            }
            push(&mut d, o);
        }
        for m in &l.models {
            let mut o = Object::new(Kind::Model { path: m.path.clone(), scale: m.scale, bounds: None, solid: false }, m.at);
            o.set_rotation(m.turn);
            if let Some(b) = m.solid {
                o.kind = Kind::Model { path: m.path.clone(), scale: m.scale, bounds: Some(o.unturned_box(&b, m.scale)), solid: true };
            }
            push(&mut d, o);
        }
        for w in &l.wires {
            let Some(&first) = w.points.first() else { continue };
            push(&mut d, Object::new(Kind::Wire { sig: w.sig, points: w.points[1..].iter().map(|p| *p - first).collect() }, first));
        }
        for p in &l.me_props {
            // A prop that stands in a box of its own: that box is the prop's, not a block.
            let mut o = Object::new(Kind::MeProp { package: p.package.to_string(), mesh: p.mesh.to_string(), scale: p.scale, bounds: None, solid: false }, p.at);
            o.set_rotation(p.turn);
            if let Some(b) = p.stand_in {
                if let Some(i) = d.objects.iter().position(|o| o.kind == Kind::Block { mat: Mat::Prop } && (o.bounds().min - b.min).length() < 1e-4 && (o.bounds().max - b.max).length() < 1e-4) {
                    d.objects.remove(i);
                }
                o.kind = Kind::MeProp { package: p.package.to_string(), mesh: p.mesh.to_string(), scale: p.scale, bounds: Some(o.unturned_box(&b, p.scale)), solid: true };
            }
            push(&mut d, o);
        }
        (d, skipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built chamber reads into a document, saves, loads back the same, and builds into
    /// a level with the same boxes and pieces.
    #[test]
    fn chambers_round_trip() {
        for (name, f) in crate::levels::all() {
            let level = f();
            let (doc, _) = LevelDoc::from_level(&level);
            let text = doc.to_ron();
            let back = LevelDoc::from_ron(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(doc, back, "{name}");
            let built = back.build();
            assert_eq!(built.solids.len(), level.solids.len(), "{name}: solids");
            assert_eq!(built.checkpoints.len(), level.checkpoints.len(), "{name}: checkpoints");
            // (To the millimetre: a box's corners come back through its centre and size.)
            let mm = |f: &[Fixture]| format!("{f:.3?}").replace("-0.000", "0.000");
            assert_eq!(mm(&built.fixtures), mm(&level.fixtures), "{name}: fixtures");
            let spawn = (built.checkpoints[0].spawn - level.checkpoints[0].spawn).length();
            assert!(spawn < 1e-4, "{name}: spawn moved {spawn}");
            let movers = level.items.iter().filter(|i| matches!(i, Item::Mover(m) if m.model != NO_MODEL || m.boxes.len() != 1 || m.path.len() != 2)).count();
            let others = level.items.iter().filter(|i| matches!(i, Item::GunPickup { .. } | Item::RocketTurret { .. } | Item::RocketTarget { .. })).count();
            assert_eq!(built.items.len(), level.items.len() - movers - others, "{name}: items");
            for (a, b) in built.items.iter().zip(level.items.iter().filter(|i| !matches!(i, Item::Mover(m) if m.model != NO_MODEL || m.boxes.len() != 1 || m.path.len() != 2) && !matches!(i, Item::GunPickup { .. } | Item::RocketTurret { .. } | Item::RocketTarget { .. }))) {
                assert!(same_item(a, b), "{name}: {a:?} became {b:?}");
            }
        }
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-3
    }

    fn same_item(a: &Item, b: &Item) -> bool {
        match (a, b) {
            (Item::Laser { at: a1, dir: d1, .. }, Item::Laser { at: a2, dir: d2, .. }) => near(*a1, *a2) && near(*d1, d2.normalize()),
            (Item::Catcher { at: a1, facing: f1, .. }, Item::Catcher { at: a2, facing: f2, .. }) => near(*a1, *a2) && near(*f1, f2.normalize()),
            (Item::Bridge { at: a1, dir: d1, .. }, Item::Bridge { at: a2, dir: d2, .. }) => near(*a1, *a2) && near(*d1, d2.normalize()),
            (Item::GelDispenser { at: a1, dir: d1, .. }, Item::GelDispenser { at: a2, dir: d2, .. }) => near(*a1, *a2) && near(*d1, d2.normalize()),
            (Item::Turret { at: a1, yaw: y1 }, Item::Turret { at: a2, yaw: y2 }) => near(*a1, *a2) && (y1 - y2).abs() < 1e-3,
            _ => std::mem::discriminant(a) == std::mem::discriminant(b),
        }
    }

    /// The example in docs/levels.md reads.
    #[test]
    fn the_documented_example_reads() {
        let md = include_str!("../../../docs/levels.md");
        let start = md.find("```ron\n").expect("a ron example") + 7;
        let end = start + md[start..].find("```").unwrap();
        let doc = LevelDoc::from_ron(&md[start..end]).unwrap();
        let l = doc.build();
        assert_eq!(l.items.len(), 3);
        let Item::Laser { dir, .. } = &l.items[2] else { panic!() };
        assert!((*dir - Vec3::X).length() < 1e-5);
    }

    /// A prop turned about every axis and scaled along each reads back the same, builds turned
    /// and scaled so, with a box round all of it; a level saved with one scale for all three
    /// still reads.
    #[test]
    fn props_turn_and_scale_every_way() {
        let mut o = Object::new(Kind::Model { path: "props/portal2/props/radio.glb".into(), scale: Vec3::new(3.0, 1.5, 2.0), bounds: Some((Vec3::new(-0.5, 0.0, -0.25), Vec3::new(0.5, 1.0, 0.25))), solid: true }, Vec3::new(1.0, 2.0, -3.0));
        (o.pitch, o.yaw, o.roll) = (30.0, 45.0, 20.0);
        let mut d = LevelDoc::new("T");
        d.objects.push(o.clone());
        let back = LevelDoc::from_ron(&d.to_ron()).unwrap();
        assert_eq!(back, d);
        let l = back.build();
        let m = &l.models[0];
        assert!(m.turn.dot(o.rotation()).abs() > 1.0 - 1e-6);
        assert_eq!(m.scale, Vec3::new(3.0, 1.5, 2.0));
        // The box holds every corner of the turned, scaled prop.
        let b = m.solid.unwrap();
        for k in 0..8 {
            let c = Vec3::new(if k & 1 == 0 { -1.5 } else { 1.5 }, if k & 2 == 0 { 0.0 } else { 1.5 }, if k & 4 == 0 { -0.5 } else { 0.5 });
            let p = o.at + o.rotation() * c;
            assert!((p.cmpge(b.min - 1e-3) & p.cmple(b.max + 1e-3)).all(), "{p} outside {b:?}");
        }
        // Read back from the level, it's turned the same.
        let (again, _) = LevelDoc::from_level(&l);
        let r = again.objects.iter().find(|x| matches!(x.kind, Kind::Model { .. })).unwrap();
        assert!(r.rotation().dot(o.rotation()).abs() > 1.0 - 1e-6);
        // An older file: one number for the scale.
        let old = "(name: \"Old\", objects: [(kind: MeProp(package: \"P_Rooftops\", mesh: \"S_Cistern_01\", scale: 2.0), at: (0.0, 0.0, 0.0))])";
        let d = LevelDoc::from_ron(old).unwrap();
        assert!(matches!(d.objects[0].kind, Kind::MeProp { scale, .. } if scale == Vec3::splat(2.0)));
    }

    #[test]
    fn starter_builds() {
        let l = LevelDoc::starter("Test").build();
        assert_eq!(l.solids.len(), 6);
        assert_eq!(l.checkpoints[0].spawn, Vec3::new(0.0, 0.0, -2.0));
    }
}
