//! A level as a file: what the level editor saves and loads (RON), and makes into a [`Level`].
//!
//! A [`LevelDoc`] is a list of [`Object`]s, each a [`Kind`] (a box of some material, a puzzle
//! piece, the start...) standing somewhere. Boxes are given by their centre and size; pieces by
//! the point the matching [`Level`] builder method takes, and, for the ones that point
//! somewhere (lasers, bridges, catchers), a yaw and pitch: yaw 0 looks down -Z, 90° down -X,
//! as Faith's does. A built chamber can be read back into one ([`LevelDoc::from_level`]) as a
//! starting point: its boxes as they end up, after the rooms are cut.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{EulerRot, Quat, Vec3, Vec4};
use serde::{Deserialize, Serialize};

use faith_move::climb::Ladder;
use faith_move::{Aabb, Fixture};

use crate::art::{ArtOverlay, FaceGroup, MapArt};
use crate::brush::{tool_material, Brush, Collision, Displacement};
use crate::io::Output;
use crate::me::MeShape;
use crate::level::{CubeKind, Gun, Inputs, Item, Launch, Level, Mat, MeProp, ModelProp, MoverDef, Paint, PlacedLight, PresetPortal, Sig, Slope, Solid, SunLight, NO_MODEL};
use crate::portal::PortalColor;
use crate::source::{self, MapBox, Scenes};

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
    /// The fill light everywhere, if the level sets its own (else as indoors or outdoors has it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ambient: Option<Ambient>,
    /// The Portal map it's the game's own copy of (`testchmb_a_00`), if it is one: its level
    /// changes lead to the next map's copy, and it's played by Portal's rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
    /// Its checkpoints are a course (a time trial's), in the order they're in the file: each
    /// counts only after the one before it, and the exit only once they've all been passed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub course: bool,
    /// Walking into any checkpoint makes it the respawn point, in whatever order (a story
    /// level's).
    #[serde(default, skip_serializing_if = "is_false")]
    pub free_checkpoints: bool,
}

/// The fill light everywhere: its colour (sRGB, 0-1) and brightness.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ambient {
    pub color: Vec3,
    pub brightness: f32,
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
    /// What the logic calls it (Portal's entities: other entities' outputs name it).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// What it sets off (a Portal entity's): when it fires an event, an input sent to the
    /// entities with a name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<Output>,
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
    use serde::de::{Error, SeqAccess, Visitor};
    use serde::Deserializer;

    /// A scale: one number for all three axes (older files), or three. (Read by hand rather
    /// than as an untagged enum: RON reads a number of unknown type by searching the rest of
    /// the file, which made a level of thousands of props take seconds to open.)
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec3, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Vec3;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a scale, or one for each axis")
            }
            fn visit_f64<E: Error>(self, v: f64) -> Result<Vec3, E> {
                Ok(Vec3::splat(v as f32))
            }
            fn visit_i64<E: Error>(self, v: i64) -> Result<Vec3, E> {
                Ok(Vec3::splat(v as f32))
            }
            fn visit_u64<E: Error>(self, v: u64) -> Result<Vec3, E> {
                Ok(Vec3::splat(v as f32))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Vec3, A::Error> {
                let mut v = [0.0f32; 3];
                for (i, x) in v.iter_mut().enumerate() {
                    *x = a.next_element()?.ok_or_else(|| A::Error::invalid_length(i, &self))?;
                }
                Ok(Vec3::from(v))
            }
        }
        d.deserialize_any(V)
    }

    pub fn one() -> Vec3 {
        Vec3::ONE
    }
}

fn is_zero(v: &Vec3) -> bool {
    *v == Vec3::ZERO
}

fn one_f() -> f32 {
    1.0
}

fn is_false(b: &bool) -> bool {
    !*b
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
    /// Where Faith starts (the floor under her), facing `yaw`; and the level's gravity for
    /// her (m/s², none: the game's, Portal's 15.2).
    Spawn {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gravity: Option<f32>,
    },
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
    ZipLine {
        to: Vec3,
        /// How far its middle hangs below the straight line to its end (m).
        #[serde(default, skip_serializing_if = "is_zero_f")]
        sag: f32,
    },
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
    /// A light shining all round from here: its colour (sRGB, 0-1), power (lumens), reach and
    /// size (metres), and whether it casts shadows.
    PointLight { color: Vec3, intensity: f32, range: f32, #[serde(default = "light_radius")] radius: f32, #[serde(default)] shadows: bool },
    /// A light shining the way it faces, in a cone `angle` degrees out from its middle, its
    /// edge softened over `soft` of that (0-1); else as a point light.
    SpotLight { color: Vec3, intensity: f32, range: f32, #[serde(default = "light_radius")] radius: f32, angle: f32, #[serde(default = "spot_soft")] soft: f32, #[serde(default)] shadows: bool },
    /// The level's sun, shining the way it faces: its colour (sRGB, 0-1) and brightness (lux).
    Sun { color: Vec3, illuminance: f32 },
    /// A Mirror's Edge static mesh, standing here, turned `yaw`. `bounds` is its box (from here,
    /// before it's turned or scaled); with `solid`, that's a block too (turned with it a quarter
    /// at a time), drawn instead when the game isn't installed. With `own_shape` as well, it
    /// collides with its mesh's own collision instead, read from the game with the mesh (its
    /// box when the game isn't there).
    MeProp {
        package: String,
        mesh: String,
        #[serde(default = "scale3::one", deserialize_with = "scale3::deserialize")]
        scale: Vec3,
        #[serde(default)]
        bounds: Option<(Vec3, Vec3)>,
        #[serde(default)]
        solid: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        own_shape: bool,
        /// Materials in place of its own, a section each (the game's paths; empty: its own),
        /// as a level's placed mesh can have (an orange catwalk, an advert).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        materials: Vec<String>,
    },
    /// A Mirror's Edge mover (#109): what a matinee moves (the props named in `carries`),
    /// turned about here and moved as its keys say (each a time and how far they've moved and
    /// turned from where they stand); how long the matinee is, how fast it plays, whether it
    /// loops. Its Kismet (`MeLogic`) plays it.
    MeMover {
        keys: Vec<crate::level::MoverKey>,
        length: f32,
        #[serde(default = "one_f")]
        rate: f32,
        #[serde(default, skip_serializing_if = "is_false")]
        looping: bool,
        carries: Vec<String>,
    },
    /// One of a Mirror's Edge level's Kismet ops (#109): its class (`SeqEvent_Touch`,
    /// `SeqAct_Interp`...) and settings (an event's region is a box of this object's size,
    /// here; a matinee's `movers`, by name). Its outputs lead to other ops' inputs: an output's
    /// event is its index, its input the other op's.
    MeLogic {
        op: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        params: Vec<(String, String)>,
    },
    /// A model from the extracted props (`props/portal2/...glb`, under the extracted folder),
    /// standing here, turned `yaw`; `bounds` and `solid` as a Mirror's Edge prop's.
    Model { path: String, #[serde(default = "scale3::one", deserialize_with = "scale3::deserialize")] scale: Vec3, #[serde(default)] bounds: Option<(Vec3, Vec3)>, #[serde(default)] solid: bool },
    /// A convex solid of `mat`, its faces' planes (`n·p = d` from here, facing out: `(n, d)`)
    /// turned as it's turned: Mirror's Edge's blocking volumes (clip).
    Hull { planes: Vec<Vec4>, mat: Mat },
    /// One of Portal's brushes: a convex solid, each face with its own material (see
    /// [`crate::brush`]), its planes from here, turned as it's turned.
    Brush(Brush),
    /// A displacement (a bumpy face: a dirt floor's mounds), its corners from here.
    Displacement(Displacement),
    /// A decal on a surface (a sign, an indicator strip, a stain): its corners from here, with
    /// where its texture is at each; facing `normal`. Its name is the one the logic switches
    /// its texture's frame by.
    Overlay { mat: String, corners: [Vec3; 4], uv: [[f32; 2]; 4], normal: Vec3 },
    /// One of Portal's entities: its class and keys (Source's), standing here, turned as it's
    /// turned, with its name and outputs. A brush entity (a door, a trigger) has its brushes,
    /// from here: they move with it, but don't turn with it.
    Entity {
        class: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        keys: Vec<(String, String)>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        brushes: Vec<Brush>,
    },
}

fn light_radius() -> f32 {
    0.1
}

fn spot_soft() -> f32 {
    0.2
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
            Kind::Bridge { .. } | Kind::Laser { .. } | Kind::Catcher { .. } | Kind::GelPipe { .. } | Kind::PelletLauncher { .. } | Kind::PelletCatcher { .. } | Kind::Portal { .. } | Kind::SpotLight { .. } | Kind::Sun { .. } => Some(Turns::Aim),
            Kind::MeProp { .. } | Kind::Model { .. } | Kind::Hull { .. } | Kind::Brush(_) | Kind::Displacement(_) | Kind::Overlay { .. } | Kind::Entity { .. } => Some(Turns::Free),
            Kind::Spawn { .. } | Kind::Turret | Kind::Checkpoint { .. } | Kind::BargeDoor | Kind::Ladder { .. } => Some(Turns::Yaw),
            _ => None,
        }
    }

    /// What it's called in the editor.
    pub fn label(&self) -> String {
        match self {
            Kind::Block { mat } => format!("{mat:?} block"),
            Kind::Decor { mat } => format!("{mat:?} decor"),
            Kind::Cut => "Cut".into(),
            Kind::Spawn { .. } => "Spawn".into(),
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
            Kind::PointLight { .. } => "Point light".into(),
            Kind::SpotLight { .. } => "Spot light".into(),
            Kind::Sun { .. } => "Sun".into(),
            Kind::ZipLine { .. } => "Zipline".into(),
            Kind::SwingPole { .. } => "Swing pole".into(),
            Kind::Beam { .. } => "Balance beam".into(),
            Kind::BargeDoor => "Barge door".into(),
            Kind::BarbedWire => "Barbed wire".into(),
            Kind::SoftPad { .. } => "Soft landing".into(),
            Kind::Ladder { pipe, .. } => if *pipe { "Drainpipe" } else { "Ladder" }.into(),
            Kind::MeProp { mesh, .. } => mesh.clone(),
            Kind::MeMover { carries, .. } => format!("Mover ({} carried)", carries.len()),
            Kind::MeLogic { op, .. } => op.trim_start_matches("Seq").to_string(),
            Kind::Hull { mat, .. } => format!("{mat:?} hull"),
            Kind::Model { path, .. } => path.rsplit('/').next().unwrap_or(path).trim_end_matches(".glb").to_string(),
            Kind::Brush(b) => {
                // (Named after the material most of its drawn faces have.)
                let mut count: HashMap<&str, usize> = HashMap::new();
                for f in b.faces.iter().filter(|f| !tool_material(&f.mat)) {
                    *count.entry(f.mat.as_str()).or_default() += 1;
                }
                let mat = count.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0))).map(|(m, _)| m).or_else(|| b.faces.first().map(|f| f.mat.as_str())).unwrap_or("");
                format!("Brush {}", mat.rsplit('/').next().unwrap_or(mat))
            }
            Kind::Displacement(d) => format!("Displacement {}", d.mat.rsplit('/').next().unwrap_or(&d.mat)),
            Kind::Overlay { mat, .. } => format!("Overlay {}", mat.rsplit('/').next().unwrap_or(mat)),
            Kind::Entity { class, .. } => class.clone(),
        }
    }

    /// Whether it's part of a Portal map's own build (a brush, an entity...): a level with
    /// these is made the way Portal's maps are ([`LevelDoc::build_with`]).
    pub fn is_map_part(&self) -> bool {
        matches!(self, Kind::Brush(_) | Kind::Displacement(_) | Kind::Overlay { .. } | Kind::Entity { .. })
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

/// A map's faces to draw, gathered by brush model and material.
#[derive(Default)]
struct Faces {
    groups: Vec<FaceGroup>,
    at: HashMap<(usize, String), usize>,
    /// What hides faces inside it.
    hiders: Hiders,
}

/// The brushes that hide what's inside them (solid, none of them seen through), bucketed by
/// where they are.
#[derive(Default)]
struct Hiders {
    /// Each one, its box, and which of the world's brushes it is.
    brushes: Vec<(Brush, Aabb, usize)>,
    cells: HashMap<(i32, i32, i32), Vec<usize>>,
}

/// The size of a [`Hiders`] bucket (metres).
const HIDER_CELL: f32 = 4.0;

impl Hiders {
    fn new(brushes: &[Brush]) -> Hiders {
        let mut h = Hiders::default();
        for (w, b) in brushes.iter().enumerate().filter(|(_, b)| b.opaque()) {
            let Some(bx) = b.bounds() else { continue };
            let k = h.brushes.len();
            h.brushes.push((b.clone(), bx, w));
            let (lo, hi) = (cell(bx.min), cell(bx.max));
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    for z in lo.2..=hi.2 {
                        h.cells.entry((x, y, z)).or_default().push(k);
                    }
                }
            }
        }
        h
    }

    /// Whether a polygon (of the world's brush `own`, if it's one: not hidden by itself) is
    /// inside another.
    fn hides(&self, pts: &[Vec3], own: Option<usize>) -> bool {
        if self.brushes.is_empty() {
            return false;
        }
        let mid = pts.iter().copied().sum::<Vec3>() / pts.len() as f32;
        let Some(list) = self.cells.get(&cell(mid)) else { return false };
        list.iter().any(|&k| {
            let (b, bx, w) = &self.brushes[k];
            let inside = pts.iter().all(|p| (p.cmpge(bx.min - 1e-3) & p.cmple(bx.max + 1e-3)).all());
            inside && own != Some(*w) && b.holds(pts)
        })
    }
}

fn cell(p: Vec3) -> (i32, i32, i32) {
    ((p.x / HIDER_CELL).floor() as i32, (p.y / HIDER_CELL).floor() as i32, (p.z / HIDER_CELL).floor() as i32)
}

impl Faces {
    fn group(&mut self, model: usize, mat: &str) -> &mut FaceGroup {
        let i = *self.at.entry((model, mat.to_string())).or_insert_with(|| {
            self.groups.push(FaceGroup { model, mat: mat.to_string(), ..Default::default() });
            self.groups.len() - 1
        });
        &mut self.groups[i]
    }

    /// A brush's faces (not its tool textures', nor what's hidden inside another brush), each a
    /// fan of triangles.
    fn brush(&mut self, model: usize, b: &Brush, own: Option<usize>) {
        for (i, pts) in b.polygons().into_iter().enumerate() {
            let f = &b.faces[i];
            if pts.len() < 3 || tool_material(&f.mat) || self.hiders.hides(&pts, own) {
                continue;
            }
            let n = f.n.normalize();
            let g = self.group(model, &f.mat);
            let base = g.pos.len() as u32;
            for p in &pts {
                g.pos.push(p.to_array());
                g.nrm.push(n.to_array());
                g.uv.push([p.dot(f.u.truncate()) + f.u.w, p.dot(f.v.truncate()) + f.v.w]);
            }
            for k in 1..pts.len() as u32 - 1 {
                g.idx.extend([base, base + k, base + k + 1]);
            }
        }
    }

    /// A displacement's grid, textured as its flat face is, each triangle wound to face the
    /// way the face does, its normals from the grid.
    fn displacement(&mut self, d: &Displacement) {
        let side = d.side();
        let grid = d.grid();
        let n = d.normal();
        let q: Vec<Vec3> = grid.iter().map(|(p, _)| *p).collect();
        let mut nrm = vec![Vec3::ZERO; q.len()];
        let mut tris = Vec::new();
        for i in 0..side - 1 {
            for j in 0..side - 1 {
                let (a, b, c, e) = (i * side + j, (i + 1) * side + j, (i + 1) * side + j + 1, i * side + j + 1);
                for t in [[a, b, c], [a, c, e]] {
                    let w = (q[t[1]] - q[t[0]]).cross(q[t[2]] - q[t[0]]);
                    let t = if w.dot(n) < 0.0 { [t[0], t[2], t[1]] } else { t };
                    let fw = (q[t[1]] - q[t[0]]).cross(q[t[2]] - q[t[0]]);
                    for k in t {
                        nrm[k] += fw;
                    }
                    tris.push(t);
                }
            }
        }
        let g = self.group(0, &d.mat);
        let base = g.pos.len() as u32;
        for (k, (_, flat)) in grid.iter().enumerate() {
            g.pos.push(q[k].to_array());
            g.nrm.push(nrm[k].normalize_or(n).to_array());
            g.uv.push([flat.dot(d.u.truncate()) + d.u.w, flat.dot(d.v.truncate()) + d.v.w]);
        }
        g.idx.extend(tris.into_iter().flat_map(|t| t.map(|k| base + k as u32)));
    }
}

/// The box round some points, at least `pad` thick each way.
fn points_box(pts: impl Iterator<Item = Vec3>, pad: f32) -> Aabb {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in pts {
        lo = lo.min(p);
        hi = hi.max(p);
    }
    if lo.x > hi.x {
        return Aabb::from_center_size(Vec3::ZERO, Vec3::splat(pad * 2.0));
    }
    Aabb::new(lo - pad, hi + pad)
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
        Object { kind, at, size: Vec3::ZERO, yaw: 0.0, pitch: 0.0, roll: 0.0, name: String::new(), outputs: Vec::new() }
    }

    pub fn sized(kind: Kind, at: Vec3, size: Vec3) -> Self {
        Object { kind, at, size, yaw: 0.0, pitch: 0.0, roll: 0.0, name: String::new(), outputs: Vec::new() }
    }

    /// Whether it's turned at all.
    pub fn turned(&self) -> bool {
        self.yaw != 0.0 || self.pitch != 0.0 || self.roll != 0.0
    }

    /// A brush of its own (or one of a brush entity's) as it stands in the world.
    pub fn place_brush(&self, b: &Brush) -> Brush {
        let b = if self.turned() && !matches!(self.kind, Kind::Entity { .. }) { b.turned(self.rotation()) } else { b.clone() };
        b.moved(self.at)
    }

    /// Its displacement as it stands in the world.
    pub fn place_displacement(&self, d: &Displacement) -> Displacement {
        let q = self.rotation();
        let turn = |v: Vec4| {
            let a = q * v.truncate();
            Vec4::from((a, v.w - a.dot(self.at)))
        };
        Displacement { corners: d.corners.map(|c| self.at + q * c), u: turn(d.u), v: turn(d.v), offsets: d.offsets.iter().map(|o| q * *o).collect(), ..d.clone() }
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
            Kind::Spawn { .. } => up(0.6, 1.8),
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
            Kind::ZipLine { to, sag } => {
                let (a, b) = (self.at, self.at + *to);
                Aabb::new(a.min(b) - 0.1 - Vec3::Y * sag.max(0.0), a.max(b) + 0.1)
            }
            Kind::SwingPole { to } => {
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
            Kind::Hull { planes, .. } => {
                let b = crate::me::hull_brush(planes).turned(self.rotation()).bounds();
                b.map_or_else(|| around(0.1), |b| Aabb::new(b.min + self.at, b.max + self.at))
            }
            Kind::Brush(b) => {
                let b = if self.turned() { b.turned(self.rotation()).bounds() } else { b.bounds() };
                b.map_or_else(|| around(0.1), |b| Aabb::new(b.min + self.at, b.max + self.at))
            }
            Kind::Displacement(d) => points_box(self.place_displacement(d).grid().into_iter().map(|(p, _)| p), 0.01),
            Kind::Overlay { corners, .. } => {
                let q = self.rotation();
                points_box(corners.iter().map(|c| self.at + q * *c), 0.01)
            }
            Kind::Entity { brushes, .. } if !brushes.is_empty() => {
                let mut b: Option<Aabb> = None;
                for x in brushes.iter().filter_map(|x| x.bounds()) {
                    let x = Aabb::new(x.min + self.at, x.max + self.at);
                    b = Some(b.map_or(x, |b| Aabb::new(b.min.min(x.min), b.max.max(x.max))));
                }
                b.unwrap_or_else(|| around(0.25))
            }
            Kind::Entity { .. } => around(0.25),
            _ => around(0.35),
        }
    }
}

impl LevelDoc {
    pub fn new(name: &str) -> Self {
        LevelDoc { name: name.into(), intro: String::new(), indoor: true, kill_y: -30.0, blue: true, orange: true, objects: Vec::new(), ambient: None, map: None, course: false, free_checkpoints: false }
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
        d.objects.push(Object::new(Kind::Spawn { gravity: None }, Vec3::new(0.0, 0.0, -2.0)));
        d
    }

    /// As a file: an object a line.
    pub fn to_ron(&self) -> String {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default().struct_names(false).depth_limit(2)).unwrap_or_default()
    }

    pub fn from_ron(text: &str) -> Result<LevelDoc, String> {
        // (A file from before the spawn could set the gravity has a plain `Spawn`: it's
        // `Spawn()` now.)
        let text = if text.contains("kind: Spawn,") || text.contains("kind: Spawn)") { std::borrow::Cow::Owned(text.replace("kind: Spawn,", "kind: Spawn(),").replace("kind: Spawn)", "kind: Spawn())")) } else { std::borrow::Cow::Borrowed(text) };
        ron::from_str(&text).map_err(|e| e.to_string())
    }

    /// Make it into a level. Its words live as long as the program (a level's are `'static`):
    /// a few bytes each time the editor rebuilds it.
    pub fn build(&self) -> Level {
        self.build_with(&Scenes::new())
    }

    /// The same, with GLaDOS's scenes for its entities to play.
    pub fn build_with(&self, scenes: &Scenes) -> Level {
        let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
        let map = self.objects.iter().any(|o| o.kind.is_map_part());
        let mut l = if map { self.build_map(leak(&self.name), scenes) } else { Level::new(leak(&self.name)) };
        // Its own pieces' signals come after the map's logic's, and its cuts only cut its own
        // blocks (the map's faces are drawn as they are).
        let base = if map { l.signal_count() } else { 0 };
        let sg = |s: &Sig| *s + base;
        let inp = |i: &Inputs| Inputs { all: i.all.iter().map(|s| s + base).collect(), invert: i.invert };
        // Mirror's Edge's meshes and volumes collide as their shapes, and aren't drawn as boxes
        // (the meshes are drawn): their solids first, as a map's own are.
        let shape_of = |o: &Object| match &o.kind {
            Kind::MeProp { package, mesh, solid: true, own_shape: true, .. } => crate::me::shape(package, mesh),
            _ => None,
        };
        // (What Mirror's Edge's movers carry collides as they move: theirs, not the world's.)
        let carried: std::collections::HashSet<&str> = self.objects.iter().flat_map(|o| match &o.kind {
            Kind::MeMover { carries, .. } => carries.iter().map(|c| c.as_str()).collect(),
            _ => Vec::new(),
        }).collect();
        let mut carried_solids: HashMap<&str, Vec<Solid>> = HashMap::new();
        for o in &self.objects {
            match &o.kind {
                Kind::MeProp { package, mesh, scale, .. } => {
                    if let Some(shape) = shape_of(o) {
                        if carried.contains(o.name.as_str()) {
                            let mut slopes = Vec::new();
                            shape.place(o.at, o.rotation(), *scale, Mat::Wall, carried_solids.entry(o.name.as_str()).or_default(), &mut slopes);
                        } else {
                            crate::me::place_kept(&format!("{package}.{mesh}"), &shape, o.at, o.rotation(), *scale, Mat::Wall, &mut l.solids, &mut l.slopes);
                        }
                    }
                }
                _ => {}
            }
        }
        if !map {
            l.map_solids = l.solids.len();
        }
        l.me_art = self.objects.iter().any(|o| matches!(o.kind, Kind::MeProp { own_shape: true, .. }));
        let first = l.solids.len();
        l.intro = leak(&self.intro);
        l.indoor = self.indoor;
        l.kill_y = self.kill_y;
        l.course = self.course;
        l.free_checkpoints = self.free_checkpoints;
        l.gun = Gun { blue: self.blue, orange: self.orange };
        let corners = |o: &Object| {
            let b = o.bounds();
            (b.min.to_array(), b.max.to_array())
        };
        for o in &self.objects {
            match &o.kind {
                Kind::Block { mat } => l.solids.push(Solid { b: o.bounds(), mat: *mat }),
                Kind::MeProp { .. } if shape_of(o).is_some() => {}
                Kind::MeProp { bounds: Some(_), solid: true, .. } | Kind::Model { bounds: Some(_), solid: true, .. } => l.solids.push(Solid { b: o.bounds(), mat: Mat::Prop }),
                Kind::Beam { solid: true, .. } => l.solids.push(Solid { b: o.bounds(), mat: Mat::Black }),
                Kind::Hull { planes, mat } => {
                    let shape = MeShape { hulls: vec![planes.clone()], tris: Vec::new() };
                    shape.place(o.at, o.rotation(), Vec3::ONE, *mat, &mut l.solids, &mut l.slopes);
                }
                Kind::SoftPad { solid: true } => l.solids.push(Solid { b: o.bounds(), mat: Mat::Prop }),
                _ => {}
            }
        }
        for o in &self.objects {
            if let Kind::Cut = o.kind {
                let (a, b) = corners(o);
                l.cut_from(first, a, b);
            }
        }
        let mut spawn = None;
        let mut prop_index: HashMap<&str, usize> = HashMap::new();
        for o in &self.objects {
            let at = o.at;
            let dir = o.dir().to_array();
            let (a, b) = corners(o);
            match &o.kind {
                Kind::Block { .. } | Kind::Cut | Kind::Hull { .. } => {}
                Kind::Decor { mat } => {
                    l.decor(*mat, a, b);
                }
                Kind::Spawn { gravity } => {
                    if spawn.is_none() {
                        l.gravity = gravity.filter(|g| *g > 0.0);
                    }
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
                    l.item(Item::FloorButton { sig: sg(sig), at, cube_only: *cube_only });
                }
                Kind::Pedestal { sig, time } => {
                    l.pedestal(sg(sig), at.to_array(), *time);
                }
                Kind::Door { inputs } => {
                    l.door(inp(inputs), a, b);
                }
                Kind::Cube { cube } => {
                    l.cube(*cube, at.to_array());
                }
                Kind::Dropper { cube, inputs } => {
                    l.dropper(*cube, at.to_array(), inp(inputs));
                }
                Kind::Fizzler { inputs } => {
                    l.fizzler(a, b, inp(inputs));
                }
                Kind::FaithPlate { target, apex, inputs } => {
                    l.item(Item::FaithPlate { at, target: *target, apex: *apex, inputs: inp(inputs) });
                }
                Kind::Bridge { inputs } => {
                    l.item(Item::Bridge { at, dir: o.dir(), up: o.rotation() * Vec3::Y, inputs: inp(inputs) });
                }
                Kind::Laser { inputs } => {
                    l.laser(at.to_array(), dir, inp(inputs));
                }
                Kind::Catcher { sig } => {
                    l.catcher(sg(sig), at.to_array(), dir);
                }
                Kind::Relay { sig } => {
                    l.relay(sg(sig), at.to_array());
                }
                Kind::GelPipe { paint, inputs, rate, speed } => {
                    l.item(Item::GelDispenser { at, dir: o.dir(), paint: *paint, inputs: inp(inputs), rate: *rate, speed: *speed });
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
                    l.item(Item::PelletLauncher { at, dir: o.dir(), inputs: inp(inputs), life: *life, launch: Launch::default() });
                }
                Kind::PelletCatcher { sig } => {
                    l.pellet_catcher(sg(sig), at.to_array(), dir);
                }
                Kind::Platform { travel, speed, inputs } => {
                    l.platform(a, b, travel.to_array(), *speed, inp(inputs));
                }
                Kind::Portal { color } => {
                    l.portals.push(PresetPortal { color: *color, center: at, normal: o.dir(), up: o.rotation() * Vec3::Y });
                }
                Kind::Wire { sig, points } => {
                    let pts: Vec<[f32; 3]> = std::iter::once(at).chain(points.iter().map(|p| at + *p)).map(|p| p.to_array()).collect();
                    l.wire(sg(sig), &pts);
                }
                Kind::PointLight { color, intensity, range, radius, shadows } => {
                    l.placed_lights.push(PlacedLight { at, color: *color, intensity: *intensity, range: *range, radius: *radius, spot: None, shadows: *shadows });
                }
                Kind::SpotLight { color, intensity, range, radius, angle, soft, shadows } => {
                    let outer = angle.clamp(1.0, 89.0).to_radians();
                    let spot = Some((o.dir(), outer, outer * (1.0 - soft.clamp(0.0, 1.0))));
                    l.placed_lights.push(PlacedLight { at, color: *color, intensity: *intensity, range: *range, radius: *radius, spot, shadows: *shadows });
                }
                Kind::Sun { color, illuminance } => {
                    l.sun.get_or_insert(SunLight { dir: o.dir(), color: *color, illuminance: *illuminance });
                }
                Kind::ZipLine { to, sag } => l.fixtures.push(Fixture::ZipLine { a: at, b: at + *to, sag: *sag }),
                Kind::SwingPole { to } => l.fixtures.push(Fixture::SwingPole { a: at, b: at + *to }),
                Kind::Beam { to, .. } => l.fixtures.push(Fixture::Beam { a: at, b: at + *to }),
                Kind::BargeDoor => l.fixtures.push(Fixture::Door { b: o.bounds(), n: -o.dir() }),
                Kind::BarbedWire => l.fixtures.push(Fixture::BarbedWire { b: o.bounds() }),
                Kind::SoftPad { .. } => l.fixtures.push(Fixture::SoftPad { b: o.bounds() }),
                Kind::Ladder { height, pipe, exit } => {
                    l.fixtures.push(Fixture::Ladder(Ladder { base: at, top: at.y + *height, normal: o.dir(), pipe: *pipe, exit: *exit }));
                }
                // (Made once the props they name are: below.)
                Kind::MeMover { .. } | Kind::MeLogic { .. } => {}
                Kind::MeProp { package, mesh, scale, bounds, solid, materials, .. } => {
                    if !o.name.is_empty() {
                        prop_index.insert(o.name.as_str(), l.me_props.len());
                    }
                    let shaped = shape_of(o).is_some();
                    let stand_in = (bounds.is_some() && *solid && !shaped).then(|| o.bounds());
                    let materials = materials.iter().map(|m| leak(m)).collect();
                    l.me_props.push(MeProp { package: leak(package), mesh: leak(mesh), at, turn: o.rotation(), scale: *scale, stand_in, materials });
                }
                Kind::Model { path, scale, bounds, solid } => {
                    let solid = (bounds.is_some() && *solid).then(|| o.bounds());
                    l.models.push(ModelProp { path: path.clone(), at, turn: o.rotation(), scale: *scale, solid });
                }
                // (The map's: made into it already.)
                Kind::Brush(_) | Kind::Displacement(_) | Kind::Overlay { .. } | Kind::Entity { .. } => {}
            }
        }
        self.build_me_logic(&mut l, &prop_index, &mut carried_solids);
        match spawn {
            // A start of its own, in a map: instead of the map's.
            Some((at, yaw)) if map && !l.checkpoints.is_empty() => {
                l.checkpoints.remove(0);
                l.spawn(at.to_array(), yaw);
            }
            Some((at, yaw)) => {
                l.spawn(at.to_array(), yaw);
            }
            None if !map || l.checkpoints.is_empty() => {
                l.spawn([0.0; 3], 0.0);
            }
            None => {}
        }
        l.ambient = self.ambient.map(|a| (a.color, a.brightness));
        l
    }

    /// The map it's a copy of, made as `aperture::source` makes Portal's maps (from the boxes
    /// and entities `portal_extract` writes out): its brushes into boxes to collide with,
    /// faces that take portals at a slant and faces to draw; its entities into the game's
    /// pieces and logic; and what's drawn besides.
    pub(crate) fn build_map(&self, title: &'static str, scenes: &Scenes) -> Level {
        let mut boxes: Vec<MapBox> = Vec::new();
        let mut slopes: Vec<(usize, Slope)> = Vec::new();
        let mut faces = Faces::default();
        let kind_of = |c: Collision| -> Option<&'static str> {
            Some(match c {
                Collision::Solid(Mat::White) => "white",
                Collision::Solid(Mat::Glass | Mat::Frosted) => "glass",
                Collision::Solid(Mat::Grate) => "grate",
                Collision::Solid(Mat::Clip) => "clip",
                Collision::Solid(_) => "black",
                Collision::Goo => "goo",
                Collision::None => return None,
            })
        };
        let add_brush = |model: usize, b: &Brush, own: Option<usize>, boxes: &mut Vec<MapBox>, slopes: &mut Vec<(usize, Slope)>, faces: &mut Faces| {
            if let Some(kind) = kind_of(b.collision()) {
                boxes.extend(b.boxes().into_iter().map(|bx| MapBox { kind: kind.to_string(), model, b: bx }));
            }
            slopes.extend(b.slopes(model != 0).into_iter().map(|s| (model, s)));
            faces.brush(model, b, own);
        };
        // The world: its brushes and displacements. (A face inside another brush that hides
        // it, against a wall, isn't drawn: Portal's compiler leaves those out, and glass there
        // would show.)
        let world: Vec<Brush> = self.objects.iter().filter_map(|o| if let Kind::Brush(b) = &o.kind { Some(o.place_brush(b)) } else { None }).collect();
        faces.hiders = Hiders::new(&world);
        let mut w = 0;
        for o in &self.objects {
            match &o.kind {
                Kind::Brush(_) => {
                    add_brush(0, &world[w], Some(w), &mut boxes, &mut slopes, &mut faces);
                    w += 1;
                }
                Kind::Displacement(d) => {
                    let d = o.place_displacement(d);
                    let kind = if d.noportal { "black" } else { "white" };
                    boxes.extend(d.boxes().into_iter().map(|b| MapBox { kind: kind.into(), model: 0, b }));
                    faces.displacement(&d);
                }
                _ => {}
            }
        }
        // Its entities, as Source's (a brush entity's brushes its own model: they move, so the
        // world doesn't hide their faces).
        faces.hiders = Hiders::default();
        let mut ents: Vec<source::Entity> = Vec::new();
        let mut placed: Vec<(source::Entity, Vec3, Quat)> = Vec::new();
        let mut model = 0;
        for o in &self.objects {
            let Kind::Entity { class, keys, brushes } = &o.kind else { continue };
            let mut e: source::Entity = vec![("classname".into(), class.clone())];
            if !o.name.is_empty() {
                e.push(("targetname".into(), o.name.clone()));
            }
            e.push(("origin".into(), source::source_point(o.at)));
            let a = source::angles_of(o.rotation());
            let deg = |x: f32| {
                let x = (x * 1000.0).round() / 1000.0;
                if x == 0.0 { "0".to_string() } else { format!("{x}") }
            };
            e.push(("angles".into(), format!("{} {} {}", deg(a.x), deg(a.y), deg(a.z))));
            if !brushes.is_empty() {
                model += 1;
                e.push(("model".into(), format!("*{model}")));
                let mut bounds: Option<Aabb> = None;
                for b in brushes {
                    let b = o.place_brush(b);
                    if let Some(x) = b.bounds() {
                        bounds = Some(bounds.map_or(x, |y| Aabb::new(y.min.min(x.min), y.max.max(x.max))));
                    }
                    add_brush(model, &b, None, &mut boxes, &mut slopes, &mut faces);
                }
                if let Some(b) = bounds {
                    boxes.push(MapBox { kind: "bounds".into(), model, b });
                }
            }
            e.extend(keys.iter().cloned());
            e.extend(o.outputs.iter().map(|out| (out.event.clone(), out.value())));
            placed.push((e.clone(), o.at, o.rotation()));
            ents.push(e);
        }
        let name = self.map.clone().unwrap_or_default();
        let mut l = source::level_from(&name, title, &boxes, slopes, &ents, scenes);
        let overlays = self
            .objects
            .iter()
            .filter_map(|o| {
                let Kind::Overlay { mat, corners, uv, normal } = &o.kind else { return None };
                let q = o.rotation();
                Some(ArtOverlay { mat: mat.clone(), flags: None, name: o.name.to_ascii_lowercase(), corners: std::array::from_fn(|k| (o.at + q * corners[k], uv[k])), normal: q * *normal })
            })
            .collect();
        let art = MapArt { faces: faces.groups, props: crate::art::entity_props(&placed), overlays, ropes: crate::rope::hang(&ents) };
        l.bodies = crate::art::bodies(&art);
        l.art = Some(Arc::new(art));
        l
    }

    /// Its Mirror's Edge movers and the Kismet that drives them (#109), into the level: each
    /// mover a sim mover (the solids of what it carries), each op with its outputs, regions and
    /// movers found by name.
    fn build_me_logic(&self, l: &mut Level, prop_index: &HashMap<&str, usize>, carried_solids: &mut HashMap<&str, Vec<Solid>>) {
        let mut mover_index: HashMap<&str, usize> = HashMap::new();
        // (Movers that carry the same thing (a door's matinees, one for each way it swings) are
        // one body: one sim mover.)
        let mut body_of: HashMap<&str, usize> = HashMap::new();
        for o in &self.objects {
            let Kind::MeMover { keys, length, rate, looping, carries } = &o.kind else { continue };
            let props: Vec<usize> = carries.iter().filter_map(|c| prop_index.get(c.as_str()).copied()).collect();
            let sim = match carries.first().and_then(|c| body_of.get(c.as_str())) {
                Some(&b) => b,
                None => {
                    let boxes: Vec<Solid> = carries.iter().flat_map(|c| carried_solids.remove(c.as_str()).unwrap_or_default()).collect();
                    // (Its sim mover: what it carries collides where the logic puts it.)
                    let sim = l.items.iter().filter(|i| matches!(i, Item::Mover(_))).count();
                    l.items.push(Item::Mover(MoverDef { model: NO_MODEL, boxes, path: vec![Vec3::ZERO], speed: 0.0, start: 0, home: 0, present: true, shift: Vec3::ZERO, inputs: None, shuttle: false, swing: None, parent: None, follows: false, reset: None, faces: Vec::new(), spin: None, crush: 0.0 }));
                    for c in carries {
                        body_of.insert(c.as_str(), sim);
                    }
                    sim
                }
            };
            if !o.name.is_empty() {
                mover_index.insert(o.name.as_str(), l.me_movers.len());
            }
            l.me_movers.push(crate::level::MeMover { name: o.name.clone(), pivot: o.at, keys: keys.clone(), length: *length, rate: *rate, looping: *looping, carries: props, sim });
        }
        let ops: Vec<&Object> = self.objects.iter().filter(|o| matches!(o.kind, Kind::MeLogic { .. })).collect();
        let op_index: HashMap<&str, usize> = ops.iter().enumerate().filter(|(_, o)| !o.name.is_empty()).map(|(k, o)| (o.name.as_str(), k)).collect();
        for o in ops {
            let Kind::MeLogic { op, params } = &o.kind else { continue };
            let outputs = o
                .outputs
                .iter()
                .filter_map(|out| Some((out.event.parse().ok()?, *op_index.get(out.target.as_str())?, out.input.parse().unwrap_or(0), out.delay)))
                .collect();
            // (Its region: its box, if it has one; and any more it names.)
            let mut regions: Vec<Aabb> = Vec::new();
            if o.size != Vec3::ZERO {
                regions.push(Aabb::from_center_size(o.at, o.size));
            }
            for (k, v) in params {
                if k == "region" {
                    let f: Vec<f32> = v.split(' ').filter_map(|x| x.parse().ok()).collect();
                    if f.len() == 6 {
                        regions.push(Aabb::new(Vec3::new(f[0], f[1], f[2]), Vec3::new(f[3], f[4], f[5])));
                    }
                }
            }
            let movers = params.iter().filter(|(k, _)| k == "mover").filter_map(|(_, v)| mover_index.get(v.as_str()).copied()).collect();
            l.me_logic.push(crate::level::LogicOp { name: o.name.clone(), op: op.clone(), params: params.clone(), outputs, regions, movers });
        }
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
                Object::new(Kind::Spawn { gravity: None }, cp.spawn)
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
                Fixture::ZipLine { a, b, sag } => Object::new(Kind::ZipLine { to: *b - *a, sag: *sag }, *a),
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
        for p in &l.placed_lights {
            let o = match p.spot {
                None => Object::new(Kind::PointLight { color: p.color, intensity: p.intensity, range: p.range, radius: p.radius, shadows: p.shadows }, p.at),
                Some((dir, outer, inner)) => {
                    let soft = if outer > 0.0 { 1.0 - inner / outer } else { 0.0 };
                    facing(Kind::SpotLight { color: p.color, intensity: p.intensity, range: p.range, radius: p.radius, angle: outer.to_degrees(), soft, shadows: p.shadows }, p.at, dir)
                }
            };
            push(&mut d, o);
        }
        if let Some(sun) = &l.sun {
            push(&mut d, facing(Kind::Sun { color: sun.color, illuminance: sun.illuminance }, Vec3::new(0.0, 10.0, 0.0), sun.dir));
        }
        d.ambient = l.ambient.map(|(color, brightness)| Ambient { color, brightness });
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
            let mut o = Object::new(Kind::MeProp { package: p.package.to_string(), mesh: p.mesh.to_string(), scale: p.scale, bounds: None, solid: false, own_shape: false, materials: p.materials.iter().map(|m| m.to_string()).collect() }, p.at);
            o.set_rotation(p.turn);
            if let Some(b) = p.stand_in {
                if let Some(i) = d.objects.iter().position(|o| o.kind == Kind::Block { mat: Mat::Prop } && (o.bounds().min - b.min).length() < 1e-4 && (o.bounds().max - b.max).length() < 1e-4) {
                    d.objects.remove(i);
                }
                o.kind = Kind::MeProp { package: p.package.to_string(), mesh: p.mesh.to_string(), scale: p.scale, bounds: Some(o.unturned_box(&b, p.scale)), solid: true, own_shape: false, materials: p.materials.iter().map(|m| m.to_string()).collect() };
            }
            push(&mut d, o);
        }
        (d, skipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spawn can set the level's gravity for her; one that doesn't (an older file's plain
    /// `Spawn`) leaves the game's.
    #[test]
    fn a_spawn_sets_the_gravity() {
        let old = LevelDoc::from_ron("(name: \"T\", objects: [(kind: Spawn, at: (0.0, 0.0, 0.0))])").expect("a plain Spawn reads");
        assert_eq!(old.objects[0].kind, Kind::Spawn { gravity: None });
        assert_eq!(old.build().gravity, None);
        let mut d = LevelDoc::starter("T");
        for o in &mut d.objects {
            if let Kind::Spawn { gravity } = &mut o.kind {
                *gravity = Some(9.81);
            }
        }
        let back = LevelDoc::from_ron(&d.to_ron()).unwrap();
        assert_eq!(back, d);
        let mut sim = crate::Sim::new(back.build());
        let mut c = faith_move::Controller::new(crate::tuning(5.3, 15.0), Vec3::ZERO, 0.0);
        assert_eq!(c.tuning.gravity, crate::FAITH_GRAVITY);
        sim.step_player(&mut c, &faith_move::Input::default(), 1.0 / 60.0);
        assert_eq!(c.tuning.gravity, 9.81);
    }

    /// A level with a Portal brush in it is built as a map, but its own pieces are still its
    /// own (drawn as the game's): only the map's movers count as the map's.
    #[test]
    fn a_levels_own_pieces_beside_map_parts() {
        let mut d = LevelDoc::starter("T");
        d.objects.push(Object::new(Kind::Brush(crate::brush::box_brush(&Aabb::from_center_size(Vec3::ZERO, Vec3::ONE), "plastic/plasticwall001a", crate::brush::Contents::Solid, 1.0, false)), Vec3::new(0.0, 0.5, -8.0)));
        d.objects.push(Object::sized(Kind::Platform { travel: Vec3::new(0.0, 0.0, -6.0), speed: 2.0, inputs: Inputs::always() }, Vec3::new(3.0, 0.25, -6.0), Vec3::new(3.0, 0.5, 3.0)));
        let sim = crate::Sim::new(d.build());
        assert!(sim.level.source.is_some());
        assert_eq!(sim.map_movers, 0);
        assert_eq!(sim.movers.len(), 1);
    }

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

    /// Lights, the sun and the ambient light build into the level, and read back from it.
    #[test]
    fn lights_build_and_read_back() {
        let mut d = LevelDoc::starter("T");
        d.objects.push(Object::new(Kind::PointLight { color: Vec3::new(1.0, 0.2, 0.1), intensity: 400_000.0, range: 15.0, radius: 0.1, shadows: true }, Vec3::new(-3.0, 1.5, -9.0)));
        let mut spot = Object::new(Kind::SpotLight { color: Vec3::new(0.2, 0.5, 1.0), intensity: 1_000_000.0, range: 20.0, radius: 0.1, angle: 25.0, soft: 0.4, shadows: false }, Vec3::new(3.0, 5.8, -9.0));
        spot.pitch = -90.0;
        d.objects.push(spot);
        let mut sun = Object::new(Kind::Sun { color: Vec3::new(1.0, 0.6, 0.3), illuminance: 9000.0 }, Vec3::ZERO);
        sun.face(Vec3::new(1.0, -1.0, 0.0));
        d.objects.push(sun);
        d.ambient = Some(Ambient { color: Vec3::ONE, brightness: 30.0 });
        assert_eq!(LevelDoc::from_ron(&d.to_ron()).unwrap(), d);
        let l = d.build();
        assert_eq!(l.placed_lights.len(), 2);
        let s = l.placed_lights[1].spot.unwrap();
        assert!((s.0 - Vec3::NEG_Y).length() < 1e-4 && (s.1 - 25f32.to_radians()).abs() < 1e-5 && (s.2 - 15f32.to_radians()).abs() < 1e-4);
        assert!((l.sun.unwrap().dir - Vec3::new(1.0, -1.0, 0.0).normalize()).length() < 1e-4);
        assert_eq!(l.ambient, Some((Vec3::ONE, 30.0)));
        let (back, _) = LevelDoc::from_level(&l);
        assert_eq!(back.objects.iter().filter(|o| matches!(o.kind, Kind::PointLight { .. } | Kind::SpotLight { .. } | Kind::Sun { .. })).count(), 3);
        assert_eq!(back.ambient, d.ambient);
        let rebuilt = back.build();
        assert_eq!(rebuilt.placed_lights.len(), 2);
        assert!((rebuilt.placed_lights[1].spot.unwrap().2 - s.2).abs() < 1e-4);
    }

    #[test]
    fn starter_builds() {
        let l = LevelDoc::starter("Test").build();
        assert_eq!(l.solids.len(), 6);
        assert_eq!(l.checkpoints[0].spawn, Vec3::new(0.0, 0.0, -2.0));
    }
}
