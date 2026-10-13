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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Mat {
    /// White panels: portals go here.
    White,
    /// Dark metal: no portals.
    Black,
    /// Glass: solid, see-through, no portals; stops portal shots and lasers.
    Glass,
    /// Frosted glass: the same, but you can only make out shapes through it.
    Frosted,
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
    /// An invisible wall that only stops Faith and cubes (Portal's player clip): shots and
    /// lasers go through, portals don't stick.
    Clip,
}

impl Mat {
    pub fn portalable(self) -> bool {
        matches!(self, Mat::White | Mat::Roof | Mat::Wall)
    }

    /// Glass, clear or frosted.
    pub fn glass(self) -> bool {
        matches!(self, Mat::Glass | Mat::Frosted)
    }

    /// Whether shots and lasers go through it.
    pub fn see_through(self) -> bool {
        matches!(self, Mat::Grate | Mat::Clip)
    }

    /// Mirror's Edge's footstep set for it (faith_move's greybox looks).
    pub fn look(self) -> Look {
        match self {
            Mat::White | Mat::Wall => Look::Wall,
            Mat::Roof | Mat::Concrete => Look::Roof,
            Mat::Black | Mat::Grate => Look::Runner,
            Mat::Runner => Look::Runner,
            Mat::Glass | Mat::Frosted | Mat::Prop | Mat::Clip => Look::Prop,
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
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Inputs {
    #[serde(default)]
    pub all: Vec<Sig>,
    #[serde(default)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CubeKind {
    /// Weighted storage cube.
    Storage,
    /// Weighted companion cube.
    Companion,
    /// Discouragement redirection cube: a laser that hits it comes out of its front.
    Reflector,
    /// Not a cube: one of a Portal map's physics props (a radio, a chair), its own size. It
    /// doesn't hold buttons down.
    Physics,
}

/// Portal 2's paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
    /// A high energy pellet launcher at `at`, firing along `dir` while its inputs are on: a
    /// pellet that lives `life` seconds (infinite: for good), and a new one each time the last
    /// is gone, as `launch` says.
    PelletLauncher { at: Vec3, dir: Vec3, inputs: Inputs, life: f32, launch: Launch },
    /// A portal gun to pick up (on a pedestal), turned `turn`, riding mover `mover` (turning as
    /// it turns): touching it adds its colours to hers.
    GunPickup { at: Vec3, turn: glam::Quat, gun: Gun, mover: Option<usize> },
    /// A rocket turret (Portal's escape) at `at`, aiming `yaw` at rest, working while its inputs
    /// are on: it tracks her, locks on and fires a slow rocket. It rides mover `mover`.
    RocketTurret { at: Vec3, yaw: f32, inputs: Inputs, mover: Option<usize> },
    /// Something a rocket can hit and hurt (GLaDOS): a sphere.
    RocketTarget { at: Vec3, radius: f32 },
    /// A sentry turret standing at `at`, facing `yaw` (faith_move's: 0 looks down -Z).
    Turret { at: Vec3, yaw: f32 },
    /// A pellet catcher: its lens at `at`, facing `facing`; on for good once a pellet's in.
    PelletCatcher { sig: Sig, at: Vec3, facing: Vec3 },
    /// Something solid that moves along a path (Portal's maps: doors, elevators, walls that
    /// come and go), driven by the map's logic ([`crate::io`]).
    Mover(MoverDef),
}

/// A moving brush entity: its boxes where the level starts, and the offsets it travels
/// between (the first is where it starts, zero).
#[derive(Clone, Debug, PartialEq)]
pub struct MoverDef {
    /// The map's brush model it draws (Portal's maps).
    pub model: usize,
    pub boxes: Vec<Solid>,
    pub path: Vec<Vec3>,
    /// Metres a second.
    pub speed: f32,
    /// Which point on its path it starts at.
    pub start: usize,
    /// Which point on its path its boxes and model are at as the map has them (a train that
    /// starts part way along its track: the node it starts on).
    pub home: usize,
    /// Solid and drawn at the start.
    pub present: bool,
    /// How far it was moved from where the map has it (a train put on its first path node):
    /// models riding on it move as far.
    pub shift: Vec3,
    /// Driven by signals rather than a map's logic: while on, to the end of its path (and
    /// with `shuttle`, back and forth along it); off, back to the start (or, shuttling, it
    /// stops where it is).
    pub inputs: Option<Inputs>,
    pub shuttle: bool,
    /// A door that swings rather than slides (func_door_rotating): its path's first point to its
    /// second is how far round it's turned, in radians.
    pub swing: Option<Swing>,
    /// The mover it's parented to (Portal's maps). A brush that doesn't move by itself goes
    /// where that goes and turns as that turns (`follows`: a launcher's panel on its swinging
    /// hatch); one that does is carried along by it (a gun's turning pedestal on its lift).
    pub parent: Option<usize>,
    pub follows: bool,
    /// A door that shuts by itself this long after it's fully open (Source's `wait`).
    pub reset: Option<f32>,
    /// Its sides that take portals, big enough to hold one, as the map has them (portals go
    /// on these however it's turned: a panel on a swinging door).
    pub faces: Vec<Slope>,
    /// A fan, spinning (func_rotating).
    pub spin: Option<Spin>,
    /// What it does to her when she's in its way with nowhere to go (Source's `dmg`: damage
    /// a tick, of her 100): a crusher's kills her; none, it waits for her.
    pub crush: f32,
}

/// How a fan spins (func_rotating): about `axis` through `pivot`, up to `speed` radians a
/// second (signed), getting there (or stopping) in about `ramp` seconds; on from the start, or
/// not. The sound it makes while it spins (a soundscript or a file), if any.
#[derive(Clone, Debug, PartialEq)]
pub struct Spin {
    pub pivot: Vec3,
    pub axis: Vec3,
    pub speed: f32,
    pub ramp: f32,
    pub on: bool,
    pub sound: String,
}

/// A physics prop to knock about (Portal's maps'): its box's centre and half size, how its
/// model's turned, where the model's origin is from the box's centre, and its prop in the
/// map's art.
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub at: Vec3,
    pub half: Vec3,
    pub turn: glam::Quat,
    pub origin: Vec3,
    pub art: usize,
}

/// A region a Portal map's logic can switch on and off (from the start, `on`). `mover`: what
/// it's parented to (it rides it).
#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    pub b: Aabb,
    pub kind: VolumeKind,
    pub on: bool,
    pub mover: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VolumeKind {
    /// Pushes Faith (`players`) and cubes (`cubes`) along `dir` at `speed` (m/s: trigger_push).
    Push { dir: Vec3, speed: f32, players: bool, cubes: bool },
    /// Hurts Faith `damage` a second (100 kills her: trigger_hurt).
    Hurt { damage: f32 },
    /// No portal opens on anything in it (func_noportal_volume).
    NoPortal,
    /// Portals fit themselves away from it (func_portal_bumper).
    Bumper,
    /// Says when a portal opens in it (func_portal_detector: the map's logic hears).
    Detector,
}

/// How a swinging door turns: about `axis` through `pivot`, by `angle` (radians, signed) when
/// fully open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swing {
    pub pivot: Vec3,
    pub axis: Vec3,
    pub angle: f32,
}

impl Swing {
    /// Its turn when it's `travelled` (radians) along its path.
    pub fn rotation(&self, travelled: f32) -> glam::Quat {
        glam::Quat::from_axis_angle(self.axis, travelled * self.angle.signum())
    }
}

/// How a pellet launcher fires (Portal's launchers each set their own).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Launch {
    /// How fast its pellets fly (m/s).
    pub speed: f32,
    /// Through a portal, a pellet lives at least this much longer (seconds).
    pub after_portal: f32,
    /// After its pellet's gone, it fires again this much later; `None`: only when told to
    /// (Portal's LaunchBall).
    pub reload: Option<f32>,
    /// Where the map's launcher model is, when it's set into a wall in front of where it fires
    /// from (Chamber 15's): the wall stands for its body, and portals don't go over it. `None`:
    /// its body is a box just behind it.
    pub model: Option<Vec3>,
}

impl Default for Launch {
    fn default() -> Self {
        Launch { speed: 7.0, after_portal: 6.0, reload: Some(1.5), model: None }
    }
}

/// No brush model (a mover in a hand-built chamber, drawn from its boxes).
pub const NO_MODEL: usize = usize::MAX;

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
    /// The gravity Faith falls under here (m/s²; none: [`crate::FAITH_GRAVITY`]).
    pub gravity: Option<f32>,
    /// Indoors (a test chamber: dark sky, lamps) or out on the roofs.
    pub indoor: bool,
    /// Ceiling lamps (indoor): positions.
    pub lamps: Vec<Vec3>,
    /// Mirror's Edge's props, for dressing (drawn from the player's install when it's there).
    pub me_props: Vec<MeProp>,
    /// One of Mirror's Edge's levels: its ladders and swing poles are its own meshes (the
    /// game draws none of its own on them).
    pub me_art: bool,
    /// Models from the extracted props library (the level editor's): drawn only (a block
    /// stands for its collision, if it has one).
    pub models: Vec<ModelProp>,
    /// Point and spot lights placed in the level (the level editor's).
    pub placed_lights: Vec<PlacedLight>,
    /// The sun as the level wants it (else as indoors or outdoors has it).
    pub sun: Option<SunLight>,
    /// The fill light everywhere, as the level wants it (colour, brightness).
    pub ambient: Option<(Vec3, f32)>,
    /// One of Portal's own maps (its name, `testchmb_a_00`): drawn from its faces, not
    /// [`Level::solids`]' boxes (the first [`Level::map_solids`] of them: the map's own), and
    /// played by Portal's rules (its portals are Portal's size). The game's own copy of a map
    /// (a level file) names it too.
    pub source: Option<String>,
    /// How many of [`Level::solids`] (the first) are the map's, drawn from its faces.
    pub map_solids: usize,
    /// And how many of [`Level::items`] (the first): the map's own movers, launchers and
    /// catchers are drawn as its props; the pieces after them (a level's own) as the game's.
    pub map_items: usize,
    /// What's drawn of the map: its faces, props, overlays and cables.
    pub art: Option<std::sync::Arc<crate::art::MapArt>>,
    /// Its lights (Portal's `light` and `light_spot` entities).
    pub lights: Vec<SourceLight>,
    /// Where its emancipation grills' emitters are: the middle of each one's edge, a grill's
    /// height (128 units) of it (Portal's `cleanser_edge_1` particles, a column of them up a
    /// tall grill's sides). Empty in the game's own chambers.
    pub grill_edges: Vec<Vec3>,
    /// Its brush models not to draw (stood in for by the game's own pieces: buttons, grills).
    pub hidden_models: Vec<usize>,
    /// Its logic.
    pub io: Option<crate::io::Io>,
    /// `info_landmark` points (name, position). A level change keeps the offset from the
    /// landmark of the same name.
    pub landmarks: Vec<(String, Vec3)>,
    /// Tilted faces that take portals (Portal's maps): its boxes only step along these, so a
    /// portal shot at one goes on its true plane.
    pub slopes: Vec<Slope>,
    /// Regions its logic switches (Portal's maps: pushes, hurts, no-portal volumes, portal
    /// bumpers and detectors).
    pub volumes: Vec<Volume>,
    /// Physics props to knock about (Portal's maps').
    pub bodies: Vec<Body>,
}

/// A flat, tilted face that takes portals: its outward normal and its corners, in order round.
#[derive(Clone, Debug, PartialEq)]
pub struct Slope {
    pub normal: Vec3,
    pub pts: Vec<Vec3>,
}

impl Slope {
    /// Its area (m²).
    pub fn area(&self) -> f32 {
        let c = self.pts[0];
        self.pts.windows(2).skip(1).map(|w| (w[0] - c).cross(w[1] - c).dot(self.normal)).sum::<f32>() * 0.5
    }

    /// The same face moved: each point and the normal through `f`, `turn`.
    pub fn moved(&self, f: impl Fn(Vec3) -> Vec3, turn: glam::Quat) -> Slope {
        Slope { normal: turn * self.normal, pts: self.pts.iter().map(|p| f(*p)).collect() }
    }

    /// Where a ray from `o` along `d` meets the face's front, if it does.
    pub fn hit(&self, o: Vec3, d: Vec3) -> Option<f32> {
        let dn = d.dot(self.normal);
        if dn > -1e-6 {
            return None;
        }
        let t = (self.pts[0] - o).dot(self.normal) / dn;
        (t >= 0.0 && self.contains(o + d * t, 0.0)).then_some(t)
    }

    /// Whether a point on (or near) its plane lies within its outline, by at least `inset`.
    pub fn contains(&self, p: Vec3, inset: f32) -> bool {
        let n = self.normal;
        let k = self.pts.len();
        (0..k).all(|i| {
            let (a, b) = (self.pts[i], self.pts[(i + 1) % k]);
            // Inward, along the face: the corners go round counter-clockwise seen from the front.
            let inward = n.cross(b - a).normalize_or_zero();
            (p - a).dot(inward) >= inset
        })
    }

    /// How far `p` is in front of its plane.
    pub fn distance(&self, p: Vec3) -> f32 {
        (p - self.pts[0]).dot(self.normal)
    }
}

/// A light in one of Portal's maps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceLight {
    pub at: Vec3,
    /// Colour, 0-1.
    pub color: Vec3,
    /// Brightness, as Portal gives it (about 200 for an ordinary lamp).
    pub brightness: f32,
    /// A spotlight's direction and outer cone half-angle (radians).
    pub spot: Option<(Vec3, f32)>,
}

/// One of Mirror's Edge's static meshes placed in a level: drawn only, no collision (give it a
/// `solid` where it should block). Without the game installed it isn't drawn at all.
#[derive(Clone, Debug)]
pub struct MeProp {
    /// The package (`P_Ventilation`) and the StaticMesh in it (`S_ACUnit_05`).
    pub package: &'static str,
    pub mesh: &'static str,
    /// Where its origin goes, how it's turned, and how it's scaled along its own axes.
    pub at: Vec3,
    pub turn: glam::Quat,
    pub scale: Vec3,
    /// A solid that blocks where it stands and stands in for it without the game (its box
    /// is hidden when the prop is drawn).
    pub stand_in: Option<Aabb>,
}

/// A light placed in the level: a point light, or a spot light pointing `dir`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacedLight {
    pub at: Vec3,
    /// Colour (sRGB, 0-1).
    pub color: Vec3,
    /// Luminous power (lumens; a spot light's as if it shone all round).
    pub intensity: f32,
    /// How far it reaches (metres), and how big the light itself is.
    pub range: f32,
    pub radius: f32,
    /// A spot light's direction and its cone: outer and inner half-angles (radians).
    pub spot: Option<(Vec3, f32, f32)>,
    pub shadows: bool,
}

/// The sun: the way its light goes, its colour (sRGB, 0-1) and how bright (lux).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunLight {
    pub dir: Vec3,
    pub color: Vec3,
    pub illuminance: f32,
}

/// A model from the extracted props: `path` under the extracted folder (`props/portal2/...glb`).
#[derive(Clone, Debug, PartialEq)]
pub struct ModelProp {
    pub path: String,
    /// Where its origin goes, how it's turned, and how it's scaled along its own axes.
    pub at: Vec3,
    pub turn: glam::Quat,
    pub scale: Vec3,
    /// The block that stands for it (not drawn while it is).
    pub solid: Option<Aabb>,
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
            gravity: None,
            indoor: true,
            lamps: Vec::new(),
            me_props: Vec::new(),
            me_art: false,
            models: Vec::new(),
            placed_lights: Vec::new(),
            sun: None,
            ambient: None,
            source: None,
            map_solids: 0,
            map_items: 0,
            art: None,
            lights: Vec::new(),
            grill_edges: Vec::new(),
            hidden_models: Vec::new(),
            io: None,
            landmarks: Vec::new(),
            slopes: Vec::new(),
            volumes: Vec::new(),
            bodies: Vec::new(),
        }
    }

    // ------------------------------------------------------------ building

    /// A solid box from corner to corner.
    pub fn solid(&mut self, mat: Mat, a: [f32; 3], b: [f32; 3]) -> &mut Self {
        self.solids.push(Solid { b: Aabb::new(Vec3::from(a), Vec3::from(b)), mat });
        self
    }

    /// A painted marking (no collision).
    /// A Mirror's Edge prop: `mesh` from `package`, its origin at `at`, turned `yaw_deg` (see
    /// [`MeProp`]).
    pub fn me_prop(&mut self, package: &'static str, mesh: &'static str, at: [f32; 3], yaw_deg: f32) -> &mut Self {
        self.me_props.push(MeProp { package, mesh, at: Vec3::from(at), turn: glam::Quat::from_rotation_y(yaw_deg.to_radians()), scale: Vec3::ONE, stand_in: None });
        self
    }

    /// A Mirror's Edge prop that's in the way: a `Prop` solid from `a` to `b` blocks there,
    /// drawn as a plain box when the game isn't installed and hidden behind the prop when it
    /// is.
    pub fn me_prop_solid(&mut self, package: &'static str, mesh: &'static str, at: [f32; 3], yaw_deg: f32, a: [f32; 3], b: [f32; 3]) -> &mut Self {
        self.solid(Mat::Prop, a, b);
        let stand_in = Some(Aabb::new(Vec3::from(a).min(Vec3::from(b)), Vec3::from(a).max(Vec3::from(b))));
        self.me_props.push(MeProp { package, mesh, at: Vec3::from(at), turn: glam::Quat::from_rotation_y(yaw_deg.to_radians()), scale: Vec3::ONE, stand_in });
        self
    }

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

    /// Cut a region out of the solids from the `first` on (the ones before are a map's, drawn
    /// as they are).
    pub fn cut_from(&mut self, first: usize, a: [f32; 3], b: [f32; 3]) -> &mut Self {
        let cut = Aabb::new(Vec3::from(a), Vec3::from(b));
        let mut out: Vec<Solid> = self.solids[..first.min(self.solids.len())].to_vec();
        for s in self.solids.iter().skip(first) {
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

    /// A platform (box `a`..`b`) that shuttles back and forth by `travel` at `speed` while
    /// its inputs are on.
    pub fn platform(&mut self, a: [f32; 3], b: [f32; 3], travel: [f32; 3], speed: f32, inputs: Inputs) -> &mut Self {
        let boxes = vec![Solid { b: Aabb::new(Vec3::from(a), Vec3::from(b)), mat: Mat::Black }];
        self.item(Item::Mover(MoverDef { model: NO_MODEL, boxes, path: vec![Vec3::ZERO, Vec3::from(travel)], speed, start: 0, home: 0, present: true, shift: Vec3::ZERO, inputs: Some(inputs), shuttle: true, swing: None, parent: None, follows: false, reset: None, faces: Vec::new(), spin: None, crush: 0.0 }))
    }

    pub fn turret(&mut self, at: [f32; 3], yaw_deg: f32) -> &mut Self {
        self.item(Item::Turret { at: Vec3::from(at), yaw: yaw_deg.to_radians() })
    }

    pub fn pellet_launcher(&mut self, at: [f32; 3], dir: [f32; 3], inputs: Inputs) -> &mut Self {
        self.item(Item::PelletLauncher { at: Vec3::from(at), dir: Vec3::from(dir).normalize(), inputs, life: 12.0, launch: Launch::default() })
    }

    pub fn pellet_catcher(&mut self, sig: Sig, at: [f32; 3], facing: [f32; 3]) -> &mut Self {
        self.item(Item::PelletCatcher { sig, at: Vec3::from(at), facing: Vec3::from(facing) })
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
                Item::FloorButton { sig, .. } | Item::Pedestal { sig, .. } | Item::Catcher { sig, .. } | Item::Relay { sig, .. } | Item::PelletCatcher { sig, .. } => see(*sig),
                Item::Door { inputs, .. }
                | Item::Dropper { inputs, .. }
                | Item::Fizzler { inputs, .. }
                | Item::FaithPlate { inputs, .. }
                | Item::Bridge { inputs, .. }
                | Item::Laser { inputs, .. }
                | Item::GelDispenser { inputs, .. }
                | Item::PelletLauncher { inputs, .. }
                | Item::RocketTurret { inputs, .. } => inputs.all.iter().for_each(|s| see(*s)),
                Item::Mover(MoverDef { inputs: Some(inputs), .. }) => inputs.all.iter().for_each(|s| see(*s)),
                _ => {}
            }
        }
        for w in &self.wires {
            see(w.sig);
        }
        if let Some(io) = &self.io {
            n = n.max(io.signal_count());
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
        let mat_of = |look: &Look| match look {
            Look::Roof => Mat::Roof,
            Look::Wall => Mat::Wall,
            Look::Runner | Look::Finish => Mat::Runner,
            Look::Prop | Look::Skyline => Mat::Prop,
        };
        for (b, look) in &g.solids {
            l.solids.push(Solid { b: *b, mat: mat_of(look) });
        }
        for (b, look) in &g.decor {
            l.decor.push((*b, mat_of(look)));
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
