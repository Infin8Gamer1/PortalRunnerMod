//! me_assets: read Mirror's Edge's first-person arms from a local install.
//!
//! Nothing from the game ships with this crate. [`FaithArms::load`] opens the
//! player's own `TdGame/CookedPC` packages at runtime:
//! - `Characters/CH_TKY_Crim_Fixer_1P.upk`: the first-person upper body
//!   (`SK_UpperBody`, the shared 1P skeleton) and Faith's skin/glove textures;
//! - `Animations/AS_C1P_Unarmed.upk`: every unarmed first-person animation.

pub mod anim;
pub mod bsp;
pub mod kismet;
pub mod map;
#[cfg(feature = "prologue")]
pub mod level;
#[cfg(feature = "prologue")]
pub mod collision;
#[cfg(feature = "prologue")]
pub mod material;
pub mod morph;
pub mod package;
#[cfg(feature = "prologue")]
pub mod postfx;
pub mod pose;
pub mod props;
pub mod skeletal;
pub mod sound;
pub mod staticmesh;
pub mod texture;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub use anim::{AnimSet, Sequence};
pub use package::Package;
pub use pose::{Pose, Skinner, TrackMap};
#[cfg(feature = "prologue")]
pub use level::{Level, LevelMesh, Placed, Spawn};
pub use morph::MorphTarget;
pub use skeletal::SkeletalMesh;
pub use staticmesh::{StaticMesh, StaticSection};
pub use sound::{Cue, SoundBank, Wave};
pub use texture::Rgba;

#[derive(Debug)]
pub enum Error {
    Io(String, std::io::Error),
    Format(String),
    Missing(String),
    Truncated,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Io(p, e) => write!(f, "{p}: {e}"),
            Error::Format(s) => write!(f, "unexpected data: {s}"),
            Error::Missing(s) => write!(f, "not found: {s}"),
            Error::Truncated => write!(f, "file ends early"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub const ARMS_PACKAGE: &str = "Characters/CH_TKY_Crim_Fixer_1P.upk";
pub const ANIMS_PACKAGE: &str = "Animations/AS_C1P_Unarmed.upk";
/// TdMove_Disarm's clips (SnatchFwd, SnatchFwd2, SnatchFwd3, SnatchBack, SnatchFail): the
/// one-handed weapon set, as the game swaps it in for the disarm.
pub const DISARM_PACKAGE: &str = "Animations/AS_C1P_OneHanded_Common.upk";
/// The disarm's victim: the patrol cop and the clips it plays as Faith takes its gun
/// (TdAIController.TriggerCannedAnim with the same name as hers).
pub const VICTIM_PACKAGE: &str = "Characters/CH_TKY_Cop_Patrol.upk";
pub const VICTIM_MESH: &str = "SK_TKY_Cop_Patrol";
pub const VICTIM_ANIMS: &str = "Animations/AS_AI_PatrolCop_OneHanded.upk";
pub const VICTIM_SET: &str = "AS_AI_PatrolCop_OneHanded";
pub const ARMS_MESH: &str = "SK_UpperBody";
pub const LEGS_MESH: &str = "SK_LowerBody";
/// The arms' morph targets (TdPawnMesh1p.MorphSets): the forearm twist fixes.
pub const ARMS_MORPHS: &str = "Female1p_UpperBody_MorphSet";

/// One material slot: its diffuse, normal and specular textures.
pub struct MaterialSlot {
    pub material: String,
    /// Where it is: its package, groups and name (`M_Rooftops.M_C_RooftopsMat_01`).
    pub path: String,
    pub diffuse: Option<Rgba>,
    /// Tangent-space normal map (the game's TC_Normalmap textures).
    pub normal: Option<Rgba>,
    /// Specular intensity map (grey-scale in practice).
    pub specular: Option<Rgba>,
    /// Without a diffuse map: the colour its constants give it (linear RGB), if any, and
    /// whether it glows (it has an emissive input).
    pub color: Option<[f32; 3]>,
    pub emissive: bool,
    /// The colour its diffuse map is multiplied by (its material's colour parameter times its
    /// map: an orange catwalk, a red door, a blue rooftop box), linear, if it has one...
    pub tint: Option<[f32; 3]>,
    /// ...and where on the map (a door's paint, not its kick plate).
    pub tint_where: TintWhere,
    /// A colour it has only where its mesh's UVs say (a one-way door's red, on the side it
    /// opens from, by where its lightmap has that side): the colour, and the expression of the
    /// UVs that's 1 where it is.
    pub region: Option<([f32; 3], UvExpr)>,
    /// Its colour parameters (lowercased names, linear colours): an instance's own first, then
    /// its parents', then the defaults its material's parameters have.
    pub vectors: Vec<(String, [f32; 3])>,
    /// How it's drawn (its material's blend mode).
    pub blend: Blend,
    /// Drawn from both sides.
    pub two_sided: bool,
    /// How many times its material repeats its colour map over the mesh's UVs (its texture
    /// coordinates' tiling: a chain-link fence's one link, thirty times over).
    pub uv_scale: [f32; 2],
    /// What glows (its emissive input): a map (where, from its mesh's UVs) times a colour
    /// (linear, as bright as the game has it: a lamp's tube, a lit sign), if it glows.
    pub glow: Option<(Option<Rgba>, [f32; 3])>,
}

/// An expression of a mesh's UVs alone, as a material has it (the UVs scaled, shifted,
/// clamped, a channel of them...): worked out per triangle.
#[derive(Clone, Debug)]
pub enum UvExpr {
    /// A UV set, tiled.
    Coord(usize, [f32; 2]),
    /// A constant: one value, or two.
    Const([f32; 2], bool),
    Add(Box<UvExpr>, Box<UvExpr>),
    Sub(Box<UvExpr>, Box<UvExpr>),
    Mul(Box<UvExpr>, Box<UvExpr>),
    Clamp(Box<UvExpr>, f32, f32),
    /// Its first channel, its second, or both.
    Mask(Box<UvExpr>, bool, bool),
    OneMinus(Box<UvExpr>),
    Ceil(Box<UvExpr>),
    Floor(Box<UvExpr>),
}

impl UvExpr {
    /// Its value for a point with these UV sets: a value (both the same) or two, and whether
    /// it's two.
    pub fn eval(&self, sets: &[[f32; 2]]) -> ([f32; 2], bool) {
        let both = |a: &UvExpr, b: &UvExpr, f: fn(f32, f32) -> f32| {
            let ((x, n), (y, m)) = (a.eval(sets), b.eval(sets));
            ([f(x[0], y[0]), f(x[1], y[1])], n || m)
        };
        let each = |a: &UvExpr, f: &dyn Fn(f32) -> f32| {
            let (x, n) = a.eval(sets);
            ([f(x[0]), f(x[1])], n)
        };
        match self {
            UvExpr::Coord(k, t) => (sets.get(*k).map_or([0.0; 2], |uv| [uv[0] * t[0], uv[1] * t[1]]), true),
            UvExpr::Const(v, two) => (*v, *two),
            UvExpr::Add(a, b) => both(a, b, |x, y| x + y),
            UvExpr::Sub(a, b) => both(a, b, |x, y| x - y),
            UvExpr::Mul(a, b) => both(a, b, |x, y| x * y),
            UvExpr::Clamp(a, lo, hi) => each(a, &|x| x.clamp(*lo, *hi)),
            UvExpr::Mask(a, r, g) => {
                let (x, _) = a.eval(sets);
                match (r, g) {
                    (true, true) => (x, true),
                    (false, true) => ([x[1], x[1]], false),
                    _ => ([x[0], x[0]], false),
                }
            }
            UvExpr::OneMinus(a) => each(a, &|x| 1.0 - x),
            UvExpr::Ceil(a) => each(a, &|x| x.ceil()),
            UvExpr::Floor(a) => each(a, &|x| x.floor()),
        }
    }
}

/// Input `input` of expression `e` as an expression of the UVs alone, if it is one.
fn uv_expr(p: &Package, e: usize, input: &str, depth: u32) -> Option<UvExpr> {
    if depth > 12 {
        return None;
    }
    let x = input_of(p, e, input)?;
    let props = props::Props::read(p, &p.exports[x]).ok()?;
    let sub = |n: &str| uv_expr(p, x, n, depth + 1).map(Box::new);
    let flag = |n: &str| props.get(n).is_some_and(|t| t.bool_value);
    let constant = |n: &str| -> Option<f32> {
        let y = input_of(p, x, n)?;
        (p.class_name(&p.exports[y]) == "MaterialExpressionConstant").then(|| props::Props::read(p, &p.exports[y]).ok()?.f32(p, "R")).flatten()
    };
    Some(match p.class_name(&p.exports[x]) {
        "MaterialExpressionTextureCoordinate" => UvExpr::Coord(
            props.i32(p, "CoordinateIndex").unwrap_or(0).max(0) as usize,
            [props.f32(p, "UTiling").unwrap_or(1.0), props.f32(p, "VTiling").unwrap_or(1.0)],
        ),
        "MaterialExpressionConstant" => {
            let r = props.f32(p, "R").unwrap_or(0.0);
            UvExpr::Const([r, r], false)
        }
        "MaterialExpressionConstant2Vector" => UvExpr::Const([props.f32(p, "R").unwrap_or(0.0), props.f32(p, "G").unwrap_or(0.0)], true),
        "MaterialExpressionAdd" => UvExpr::Add(sub("A")?, sub("B")?),
        "MaterialExpressionSubtract" => UvExpr::Sub(sub("A")?, sub("B")?),
        "MaterialExpressionMultiply" => UvExpr::Mul(sub("A")?, sub("B")?),
        "MaterialExpressionConstantClamp" => UvExpr::Clamp(sub("Input")?, props.f32(p, "Min").unwrap_or(0.0), props.f32(p, "Max").unwrap_or(1.0)),
        "MaterialExpressionClamp" => UvExpr::Clamp(sub("Input")?, constant("Min").unwrap_or(0.0), constant("Max").unwrap_or(1.0)),
        "MaterialExpressionComponentMask" => UvExpr::Mask(sub("Input")?, flag("R"), flag("G")),
        "MaterialExpressionOneMinus" => UvExpr::OneMinus(sub("Input")?),
        "MaterialExpressionCeil" => UvExpr::Ceil(sub("Input")?),
        "MaterialExpressionFloor" => UvExpr::Floor(sub("Input")?),
        _ => return None,
    })
}

/// A colour a material's diffuse has only where its UVs say: a lerp, through what multiplies
/// the diffuse, between white and a colour parameter by an expression of the UVs alone. The
/// colour (an instance's value, else the parameter's own) and where it is (1 there).
fn uv_region(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32, vectors: &[(String, [f32; 3])]) -> Option<([f32; 3], UvExpr)> {
    let mut next = Some((pkg.clone(), m));
    let mut base = None;
    for _ in 0..6 {
        let (p, obj) = next.take()?;
        let (mp, mi) = pkgs.resolve(&p, obj)?;
        if mp.class_name(&mp.exports[mi]) == "Material" {
            base = Some((mp, mi));
            break;
        }
        next = props::Props::read(&mp, &mp.exports[mi]).ok()?.object(&mp, "Parent").map(|par| (mp.clone(), par));
    }
    let (mp, mi) = base?;
    let p: &Package = &mp;
    let class = |e: usize| p.class_name(&p.exports[e]).to_string();
    let white = |e: usize| {
        let Ok(c) = props::Props::read(p, &p.exports[e]) else { return false };
        match class(e).as_str() {
            "MaterialExpressionConstant" => c.f32(p, "R").unwrap_or(0.0) == 1.0,
            "MaterialExpressionConstant3Vector" => ["R", "G", "B"].iter().all(|n| c.f32(p, n).unwrap_or(0.0) == 1.0),
            _ => false,
        }
    };
    let color_of = |v: usize| -> Option<[f32; 3]> {
        let vp = props::Props::read(p, &p.exports[v]).ok()?;
        let name = vp.name(p, "ParameterName").unwrap_or_default().to_ascii_lowercase();
        vectors.iter().find(|(n, _)| !name.is_empty() && *n == name).map(|(_, c)| *c).or_else(|| vp.get("DefaultValue").and_then(|t| linear_color(p, t)))
    };
    // (Through what multiplies the diffuse: each side.)
    let mut todo = vec![(mi, "DiffuseColor")];
    while let Some((e, input)) = todo.pop() {
        let Some(x) = input_of(p, e, input) else { continue };
        match class(x).as_str() {
            "MaterialExpressionMultiply" => todo.extend([(x, "A"), (x, "B")]),
            "MaterialExpressionLinearInterpolate" => {
                let (a, b) = (input_of(p, x, "A"), input_of(p, x, "B"));
                let param = |e: Option<usize>| e.filter(|&e| class(e) == "MaterialExpressionVectorParameter");
                let Some(alpha) = uv_expr(p, x, "Alpha", 0) else { continue };
                if let (Some(a), Some(v)) = (a, param(b)) {
                    if white(a) {
                        return Some((color_of(v)?, alpha));
                    }
                }
                if let (Some(v), Some(b)) = (param(a), b) {
                    if white(b) {
                        return Some((color_of(v)?, UvExpr::OneMinus(Box::new(alpha))));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

impl MaterialSlot {
    /// Its colour where its UVs put it (`region`), for a triangle with these UV sets at its
    /// middle: the colour if the triangle's there.
    pub fn region_at(&self, sets: &[[f32; 2]]) -> Option<[f32; 3]> {
        let (c, e) = self.region.as_ref()?;
        (e.eval(sets).0[0] > 0.5).then_some(*c)
    }
}

/// Where a material's tint colours its colour map.
#[derive(Clone, Debug, Default)]
pub enum TintWhere {
    /// All of it.
    #[default]
    All,
    /// Where its U (axis 0) or V (1) is over this (0-1): a scaffold board's planks, not the
    /// brace beside them on its map.
    Above(usize, f32),
    /// Fading out where a mask's channel (0-3) rises: a door's paint, not its kick plate.
    Mask(Rgba, usize),
}

impl MaterialSlot {
    /// What its colour map is multiplied by at (u, v) on it (0-1): its tint where its material
    /// tints it, else white.
    pub fn tint_at(&self, u: f32, v: f32) -> [f32; 3] {
        let Some(t) = self.tint else { return [1.0; 3] };
        let k = match &self.tint_where {
            TintWhere::All => 1.0,
            TintWhere::Above(axis, at) => {
                if [u, v][*axis].rem_euclid(1.0) > *at { 1.0 } else { 0.0 }
            }
            TintWhere::Mask(m, ch) => {
                let x = ((u.rem_euclid(1.0) * m.width as f32) as u32).min(m.width.max(1) - 1);
                let y = ((v.rem_euclid(1.0) * m.height as f32) as u32).min(m.height.max(1) - 1);
                1.0 - m.pixels.get(((y * m.width + x) * 4) as usize + ch).map_or(0.0, |&c| c as f32 / 255.0)
            }
        };
        [1.0 + (t[0] - 1.0) * k, 1.0 + (t[1] - 1.0) * k, 1.0 + (t[2] - 1.0) * k]
    }
}

/// A material's blend mode.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Blend {
    #[default]
    Opaque,
    /// Cut out where its opacity is under this.
    Masked(f32),
    /// See-through (glass).
    Translucent,
    /// Added over what's behind (glows, light shafts).
    Additive,
}

pub struct FaithArms {
    pub mesh: SkeletalMesh,
    pub anims: AnimSet,
    pub materials: Vec<MaterialSlot>,
    /// Faith's lower body (same skeleton, so the same pose drives it).
    pub legs: Option<(SkeletalMesh, Vec<MaterialSlot>)>,
    /// The arms mesh's morph targets (empty if they don't fit the mesh).
    pub morphs: Vec<MorphTarget>,
    pub cooked_pc: PathBuf,
}

/// Opens packages by name from CookedPC (any subfolder), caching them, so
/// references into other packages (materials, textures, sounds) can be followed.
pub struct Packages {
    cooked: PathBuf,
    files: HashMap<String, PathBuf>,
    open: HashMap<String, Rc<Package>>,
    /// Packages' files as they are on disk (where cooked textures keep their big mips), the
    /// last few read.
    raw: Vec<(String, Rc<Vec<u8>>)>,
}

impl Packages {
    pub fn new(cooked: &Path) -> Self {
        let mut files = HashMap::new();
        let mut stack = vec![cooked.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    files.entry(stem.to_ascii_lowercase()).or_insert(p);
                }
            }
        }
        Packages { cooked: cooked.to_path_buf(), files, open: HashMap::new(), raw: Vec::new() }
    }

    /// A package's file as it is on disk (kept: the last few).
    pub fn raw(&mut self, name: &str) -> Option<Rc<Vec<u8>>> {
        let key = name.to_ascii_lowercase();
        if let Some(k) = self.raw.iter().position(|(n, _)| *n == key) {
            let r = self.raw.remove(k);
            self.raw.push(r.clone());
            return Some(r.1);
        }
        let bytes = Rc::new(std::fs::read(self.files.get(&key)?).ok()?);
        if self.raw.len() >= 4 {
            self.raw.remove(0);
        }
        self.raw.push((key, bytes.clone()));
        Some(bytes)
    }

    pub fn has(&self, name: &str) -> bool {
        self.files.contains_key(&name.to_ascii_lowercase())
    }

    pub fn get(&mut self, name: &str) -> Result<Rc<Package>> {
        let key = name.to_ascii_lowercase();
        if let Some(p) = self.open.get(&key) {
            return Ok(p.clone());
        }
        let path = self
            .files
            .get(&key)
            .cloned()
            .ok_or_else(|| Error::Missing(format!("{name}.upk under {}", self.cooked.display())))?;
        let p = Rc::new(Package::open(&path)?);
        self.open.insert(key, p.clone());
        Ok(p)
    }

    /// Follow an object reference (export, or import from another package) to
    /// the package that actually holds it and the export's index there.
    pub fn resolve(&mut self, pkg: &Rc<Package>, object: i32) -> Option<(Rc<Package>, usize)> {
        if object > 0 {
            return Some((pkg.clone(), object as usize - 1));
        }
        if object == 0 {
            return None;
        }
        let imp = pkg.imports.get((-object) as usize - 1)?;
        let mut top = imp.outer;
        let mut top_name = imp.name.clone();
        while top < 0 {
            let o = pkg.imports.get((-top) as usize - 1)?;
            top_name = o.name.clone();
            top = o.outer;
        }
        let other = self.get(&top_name).ok()?;
        let idx = other
            .exports
            .iter()
            .position(|e| e.name.eq_ignore_ascii_case(&imp.name) && other.class_name(e) == imp.class)?;
        Some((other, idx))
    }
}

impl FaithArms {
    /// Load from a Mirror's Edge install folder (the one containing `TdGame`),
    /// or directly from its `TdGame/CookedPC` folder.
    pub fn load(install: &Path, max_texture: u32) -> Result<Self> {
        let cooked = cooked_pc(install).ok_or_else(|| {
            Error::Missing(format!("TdGame/CookedPC under {}", install.display()))
        })?;
        let mut pkgs = Packages::new(&cooked);
        let arms_pkg = Rc::new(Package::open(&cooked.join(ARMS_PACKAGE))?);
        let mesh = SkeletalMesh::read(&arms_pkg, ARMS_MESH)?;
        let mut anims = AnimSet::read(&Package::open(&cooked.join(ANIMS_PACKAGE))?)?;
        if let Ok(disarm) = Package::open(&cooked.join(DISARM_PACKAGE)).and_then(|p| AnimSet::read(&p)) {
            anims.add_from(&disarm, |n| n.to_ascii_lowercase().starts_with("snatch"));
        }
        let materials = slots(&mut pkgs, &arms_pkg, &mesh, max_texture);
        let legs = SkeletalMesh::read(&arms_pkg, LEGS_MESH).ok().filter(|l| {
            l.bones.len() == mesh.bones.len() && l.bones.iter().zip(&mesh.bones).all(|(a, b)| a.name == b.name)
        });
        let legs = legs.map(|l| {
            let m = slots(&mut pkgs, &arms_pkg, &l, max_texture);
            (l, m)
        });
        let morphs = morph::read_morph_set(&arms_pkg, ARMS_MORPHS)
            .unwrap_or_default()
            .into_iter()
            .filter(|m| m.base_vertices as usize == mesh.vertices.len())
            .collect();
        Ok(FaithArms { mesh, anims, materials, legs, morphs, cooked_pc: cooked })
    }

    /// Another of the game's characters, skeleton and animations only (no textures, legs or
    /// morphs): `mesh` from `package`, the AnimSet `set` from `anims` (e.g. the patrol cop and
    /// AS_AI_PatrolCop_OneHanded, for the disarm's victim side).
    pub fn load_character(install: &Path, package: &str, mesh: &str, anims: &str, set: &str) -> Result<Self> {
        let cooked = cooked_pc(install).ok_or_else(|| Error::Missing(format!("TdGame/CookedPC under {}", install.display())))?;
        let pkg = Package::open(&cooked.join(package))?;
        let mesh = SkeletalMesh::read(&pkg, mesh)?;
        let anims = AnimSet::read_named(&Package::open(&cooked.join(anims))?, set)?;
        Ok(FaithArms { mesh, anims, materials: vec![], legs: None, morphs: vec![], cooked_pc: cooked })
    }

    /// Faith's third-person body (`SK_TKY_Crim_Fixer`: "Fixer" is her codename in the game's
    /// files) with its textures, and her third-person animations (`AS_F3P_Unarmed`).
    pub fn load_body3p(install: &Path, max_texture: u32) -> Result<Self> {
        let cooked = cooked_pc(install).ok_or_else(|| Error::Missing(format!("TdGame/CookedPC under {}", install.display())))?;
        let mut pkgs = Packages::new(&cooked);
        let pkg = Rc::new(Package::open(&cooked.join("Characters/CH_TKY_Crim_Fixer.upk"))?);
        let mesh = SkeletalMesh::read(&pkg, "SK_TKY_Crim_Fixer")?;
        let anims = AnimSet::read_named(&Package::open(&cooked.join("Animations/AS_F3P_Unarmed.upk"))?, "AS_F3P_Unarmed")?;
        let materials = slots(&mut pkgs, &pkg, &mesh, max_texture);
        Ok(FaithArms { mesh, anims, materials, legs: None, morphs: vec![], cooked_pc: cooked })
    }

    pub fn bone(&self, name: &str) -> Option<usize> {
        self.mesh.bones.iter().position(|b| b.name.eq_ignore_ascii_case(name))
    }
}

/// Accept either the install root or the CookedPC folder itself.
pub fn cooked_pc(path: &Path) -> Option<PathBuf> {
    for c in [path.join("TdGame").join("CookedPC"), path.join("CookedPC"), path.to_path_buf()] {
        if c.join(ARMS_PACKAGE).is_file() && c.join(ANIMS_PACKAGE).is_file() {
            return Some(c);
        }
    }
    None
}

fn slots(pkgs: &mut Packages, pkg: &Rc<Package>, mesh: &SkeletalMesh, max_texture: u32) -> Vec<MaterialSlot> {
    slots_for(pkgs, pkg, &mesh.materials, max_texture)
}

/// Each material's textures (diffuse, normal, specular) by its texture parameters, its
/// parent's (for an instance that inherits them), or its own texture-sample expressions
/// (for a plain material).
fn slots_for(pkgs: &mut Packages, pkg: &Rc<Package>, materials: &[i32], max_texture: u32) -> Vec<MaterialSlot> {
    materials
        .iter()
        .map(|&m| {
            let material = pkg.object_name(m).to_string();
            // (Flat textures only: a cube map is a reflection, not a look.)
            let textures: Vec<_> = all_textures(pkgs, pkg, m).into_iter().filter(|(_, tp, ti)| tp.class_name(&tp.exports[*ti]) == "Texture2D").collect();
            // The first of the textures `pick` takes that can be read. (Its big mips from its
            // package's file, where the cooked one keeps them.)
            let files = std::cell::RefCell::new(&mut *pkgs);
            let diffuse_name = std::cell::RefCell::new(String::new());
            let read_named = |pick: &dyn Fn(&str) -> bool| {
                textures.iter().filter(|(name, _, _)| pick(name)).find_map(|(name, tp, ti)| {
                    let mut f = files.borrow_mut();
                    texture::read_texture_from(tp, &tp.exports[*ti], max_texture, &mut |n| f.raw(n)).ok().map(|t| (name.clone(), t))
                })
            };
            let read = |pick: &dyn Fn(&str) -> bool| read_named(pick).map(|(_, t)| t);
            // (The colour map's name kept: how its material tiles it.)
            let read_diffuse = |pick: &dyn Fn(&str) -> bool| {
                read_named(pick).map(|(n, t)| {
                    *diffuse_name.borrow_mut() = n;
                    t
                })
            };
            // A texture's suffix (`T_BoatLip_01_DA`: `da`).
            let suffix = |n: &str| n.rsplit(' ').next().unwrap_or(n).rsplit('_').next().unwrap_or("").to_string();
            let is_normal = |n: &str| n.contains("normal") || suffix(n) == "n";
            let is_spec = |n: &str| (n.contains("spec") || suffix(n) == "s") && !n.contains("tile");
            let is_diffuse = |n: &str| n.contains("diffuse") || matches!(suffix(n).as_str(), "d" | "da" | "dm" | "ds" | "c" | "col" | "diff" | "albedo") || n.contains("_d_");
            // A layered material (a roof with dirt blended over it by a mask): its main layer
            // is the one its normal map belongs to (`T_Roof_02_N`: `T_Roof_02_D`), else one
            // that isn't the dirt or a detail layer.
            let stem = |n: &str| {
                let t = n.rsplit(' ').next().unwrap_or(n);
                t.rsplit_once('_').map_or(t, |(a, _)| a).to_string()
            };
            let overlay = |n: &str| ["dirt", "detail", "grime", "sand", "dust", "mask", "stain"].iter().any(|w| n.rsplit(' ').next().unwrap_or(n).contains(w));
            // (The main layer's normal map: not a dirt layer's, if it has another.)
            let normal_stem = textures.iter().map(|(n, _, _)| n).find(|n| is_normal(n) && !overlay(n)).or_else(|| textures.iter().map(|(n, _, _)| n).find(|n| is_normal(n))).map(|n| stem(n));
            let mut diffuse = normal_stem.as_ref().and_then(|ns| read_diffuse(&|n| is_diffuse(n) && stem(n) == *ns));
            if diffuse.is_none() {
                diffuse = read_diffuse(&|n| is_diffuse(n) && !overlay(n));
            }
            if diffuse.is_none() {
                diffuse = read_diffuse(&is_diffuse);
            }
            let normal = normal_stem.as_ref().and_then(|ns| read(&|n| is_normal(n) && stem(n) == *ns)).or_else(|| read(&is_normal));
            let specular = read(&is_spec);
            if diffuse.is_none() {
                // Anything that isn't a normal, specular, mask or emissive map.
                diffuse = read_diffuse(&|n| !(is_normal(n) || is_spec(n) || n.contains("mask") || matches!(suffix(n).as_str(), "m" | "e" | "em")));
            }
            drop(files);
            let diffuse_name = diffuse_name.into_inner();
            let uv_scale = if diffuse.is_some() { material_tiling(pkgs, pkg, m, &diffuse_name) } else { [1.0, 1.0] };
            // A cut-out's opacity where its material takes it from (another texture, or another
            // channel), into the colour map's alpha.
            let (mut blend, two_sided) = material_blend(pkgs, pkg, m);
            let opacity = match blend {
                Blend::Masked(_) => Some("OpacityMask"),
                Blend::Translucent => Some("Opacity"),
                _ => None,
            };
            if let (Some(input), Some(d)) = (opacity, diffuse.as_mut()) {
                if let Some((t, ch)) = material_opacity(pkgs, pkg, m, max_texture, input) {
                    for y in 0..d.height {
                        for x in 0..d.width {
                            let (sx, sy) = (x * t.width / d.width, y * t.height / d.height);
                            d.pixels[((y * d.width + x) * 4 + 3) as usize] = t.pixels[((sy * t.width + sx) * 4) as usize + ch];
                        }
                    }
                    // A see-through material that's wire and holes (a chain-link fence, barbed
                    // wire) is a cut-out: traced light passes its holes, not its wire.
                    if blend == Blend::Translucent && cut_out(d) {
                        blend = Blend::Masked(0.5);
                    }
                }
            }
            let (color, emissive) = if diffuse.is_none() { constant_color(pkgs, pkg, m) } else { (None, false) };
            let path = material_path(pkgs, pkg, m);
            // (Only a colour variant's, named for its colour: other instances' colour
            // parameters tint through masks this doesn't follow, and would dye them all over.)
            let named = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            let variant = ["orange", "blue", "yellow", "red", "green", "white", "black", "grey", "gray"].iter().any(|c| named.contains(c));
            let vectors = material_vectors(pkgs, pkg, m);
            // (Its own graph's tint where it has one: its map times a colour parameter, maybe
            // only part of the map.)
            let (tint, tint_where) = match diffuse.as_ref().and_then(|_| diffuse_tint(pkgs, pkg, m, &vectors, max_texture)) {
                Some((c, w)) => (Some(c), w),
                None => (if variant { material_tint(pkgs, pkg, m) } else { None }, TintWhere::All),
            };
            let region = uv_region(pkgs, pkg, m, &vectors);
            let glow = material_emissive(pkgs, pkg, m, max_texture);
            MaterialSlot { material, path, diffuse, normal, specular, color, emissive, tint, tint_where, region, vectors, blend, two_sided, uv_scale, glow }
        })
        .collect()
}

/// How many times material `m` repeats texture `name` (as [`all_textures`] names it: `"param
/// texture"` or `"texture"`) over its mesh's UVs: what its samples of that texture take their
/// coordinates from (a TextureCoordinate's tiling, times constants or scalar parameters,
/// through panners and rotators). The most of its samples' (a material that fades between two
/// with distance: the near one's).
fn material_tiling(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32, name: &str) -> [f32; 2] {
    let (param, tex) = name.split_once(' ').unwrap_or(("", name));
    // Its scalar parameters (an instance's own first) and its base material.
    let mut scalars: Vec<(String, f32)> = vec![];
    let mut next = Some((pkg.clone(), m));
    let mut base = None;
    for _ in 0..6 {
        let Some((p, obj)) = next.take() else { break };
        let Some((mp, mi)) = pkgs.resolve(&p, obj) else { break };
        if mp.class_name(&mp.exports[mi]) == "Material" {
            base = Some((mp, mi));
            break;
        }
        let Ok(props) = props::Props::read(&mp, &mp.exports[mi]) else { break };
        for v in props.struct_array(&mp, "ScalarParameterValues") {
            if let (Some(n), Some(x)) = (v.name(&mp, "ParameterName"), v.f32(&mp, "ParameterValue")) {
                if !scalars.iter().any(|(s, _)| s.eq_ignore_ascii_case(&n)) {
                    scalars.push((n, x));
                }
            }
        }
        next = props.object(&mp, "Parent").map(|par| (mp.clone(), par));
    }
    let Some((mp, mi)) = base else { return [1.0, 1.0] };
    let mut best: Option<[f32; 2]> = None;
    for (k, e) in mp.exports.iter().enumerate() {
        if e.outer != mi as i32 + 1 || !mp.class_name(e).contains("TextureSample") {
            continue;
        }
        let Ok(props) = props::Props::read(&mp, e) else { continue };
        let by_param = !param.is_empty() && props.name(&mp, "ParameterName").is_some_and(|n| n.eq_ignore_ascii_case(param));
        let by_texture = props.object(&mp, "Texture").is_some_and(|t| mp.object_name(t).eq_ignore_ascii_case(tex));
        if !(by_param || by_texture) {
            continue;
        }
        let s = uv_of(&mp, k, "Coordinates", &scalars, 0).unwrap_or([1.0, 1.0]);
        if best.is_none_or(|b| s[0].abs().max(s[1].abs()) > b[0].abs().max(b[1].abs())) {
            best = Some(s);
        }
    }
    let s = best.unwrap_or([1.0, 1.0]);
    if s.iter().all(|x| x.is_finite() && x.abs() > 1e-3 && x.abs() < 1e3) { s } else { [1.0, 1.0] }
}

/// What a material's emissive input is: the texture it samples (an instance's in place of its
/// parameter's default), if any, times a colour (its constants and colour and scalar
/// parameters, an instance's own first). None if it doesn't glow, or glows some way this
/// doesn't follow.
fn material_emissive(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32, max_texture: u32) -> Option<(Option<Rgba>, [f32; 3])> {
    let mut textures: Vec<(String, Rc<Package>, i32)> = vec![];
    let mut scalars: Vec<(String, f32)> = vec![];
    let mut next = Some((pkg.clone(), m));
    let mut base = None;
    for _ in 0..6 {
        let (p, obj) = next.take()?;
        let (mp, mi) = pkgs.resolve(&p, obj)?;
        if mp.class_name(&mp.exports[mi]) == "Material" {
            base = Some((mp, mi));
            break;
        }
        for (name, tex) in material_textures(&mp, mi) {
            if !textures.iter().any(|(n, _, _)| n.eq_ignore_ascii_case(&name)) {
                textures.push((name, mp.clone(), tex));
            }
        }
        let props = props::Props::read(&mp, &mp.exports[mi]).ok()?;
        for v in props.struct_array(&mp, "ScalarParameterValues") {
            if let (Some(n), Some(x)) = (v.name(&mp, "ParameterName"), v.f32(&mp, "ParameterValue")) {
                if !scalars.iter().any(|(s, _)| s.eq_ignore_ascii_case(&n)) {
                    scalars.push((n, x));
                }
            }
        }
        next = props.object(&mp, "Parent").map(|par| (mp.clone(), par));
    }
    let (mp, mi) = base?;
    let vectors = material_vectors(pkgs, pkg, m);
    let g = Glow { p: &mp, textures: &textures, scalars: &scalars, vectors: &vectors };
    let (tex, color) = g.of(mi, "EmissiveColor", 0)?;
    if color.iter().all(|c| *c <= 0.01) {
        return None;
    }
    let map = match tex {
        Some((tp, t)) => {
            let (rp, ri) = pkgs.resolve(&tp, t)?;
            let mut f = |n: &str| pkgs.raw(n);
            Some(texture::read_texture_from(&rp, &rp.exports[ri], max_texture, &mut f).ok()?)
        }
        None => None,
    };
    Some((map, color))
}

/// Following a material's emissive input back through its expressions.
struct Glow<'a> {
    p: &'a Rc<Package>,
    textures: &'a [(String, Rc<Package>, i32)],
    scalars: &'a [(String, f32)],
    vectors: &'a [(String, [f32; 3])],
}

impl Glow<'_> {
    /// What input `input` of export `e` gives: a texture sampled (its package and object),
    /// times a colour.
    fn of(&self, e: usize, input: &str, depth: u32) -> Option<(Option<(Rc<Package>, i32)>, [f32; 3])> {
        if depth > 10 {
            return None;
        }
        let p = self.p;
        let x = input_of(p, e, input)?;
        let props = props::Props::read(p, &p.exports[x]).ok()?;
        let mul = |a: [f32; 3], b: [f32; 3]| [a[0] * b[0], a[1] * b[1], a[2] * b[2]];
        match p.class_name(&p.exports[x]) {
            c if c.contains("TextureSample") => {
                let param = props.name(p, "ParameterName");
                let tex = param
                    .and_then(|n| self.textures.iter().find(|(o, _, _)| o.eq_ignore_ascii_case(&n)).map(|(_, tp, t)| (tp.clone(), *t)))
                    .or_else(|| props.object(p, "Texture").filter(|&t| t != 0).map(|t| (p.clone(), t)))?;
                Some((Some(tex), [1.0; 3]))
            }
            "MaterialExpressionMultiply" => match (self.of(x, "A", depth + 1), self.of(x, "B", depth + 1)) {
                (Some(a), Some(b)) => Some((a.0.or(b.0), mul(a.1, b.1))),
                // (Times something this doesn't follow (a fresnel, a flicker): as it is.)
                (Some(a), None) | (None, Some(a)) => Some(a),
                (None, None) => None,
            },
            "MaterialExpressionAdd" => match (self.of(x, "A", depth + 1), self.of(x, "B", depth + 1)) {
                (Some(a), Some(b)) if a.0.is_none() && b.0.is_none() => Some((None, [a.1[0] + b.1[0], a.1[1] + b.1[1], a.1[2] + b.1[2]])),
                (Some(a), Some(b)) => Some(if a.0.is_some() { a } else { b }),
                (Some(a), None) | (None, Some(a)) => Some(a),
                (None, None) => None,
            },
            "MaterialExpressionLinearInterpolate" => self.of(x, "B", depth + 1).or_else(|| self.of(x, "A", depth + 1)),
            "MaterialExpressionClamp" | "MaterialExpressionConstantClamp" | "MaterialExpressionComponentMask" => self.of(x, "Input", depth + 1),
            "MaterialExpressionConstant" => props.f32(p, "R").map(|r| (None, [r; 3])),
            "MaterialExpressionConstant3Vector" | "MaterialExpressionConstant4Vector" => {
                Some((None, [props.f32(p, "R").unwrap_or(0.0), props.f32(p, "G").unwrap_or(0.0), props.f32(p, "B").unwrap_or(0.0)]))
            }
            "MaterialExpressionVectorParameter" => {
                let n = props.name(p, "ParameterName").unwrap_or_default().to_ascii_lowercase();
                self.vectors.iter().find(|(v, _)| *v == n).map(|(_, c)| (None, *c))
            }
            "MaterialExpressionScalarParameter" => {
                let n = props.name(p, "ParameterName").unwrap_or_default();
                let v = self.scalars.iter().find(|(s, _)| s.eq_ignore_ascii_case(&n)).map(|(_, v)| *v).or_else(|| props.f32(p, "DefaultValue")).unwrap_or(0.0);
                Some((None, [v; 3]))
            }
            _ => None,
        }
    }
}

/// The expression an expression's input `input` takes (an export index), if any.
fn input_of(p: &Package, e: usize, input: &str) -> Option<usize> {
    let props = props::Props::read(p, &p.exports[e]).ok()?;
    let t = props.get(input)?;
    let i = props::Props::read_list(p, t.at, t.at + t.size, "ExpressionInput").ok()?;
    i.object(p, "Expression").filter(|&x| x > 0).map(|x| x as usize - 1)
}

/// What input `input` of expression `e` scales the mesh's UVs by, if it's the UVs (scaled):
/// none if it's something else (a constant).
fn uv_of(p: &Package, e: usize, input: &str, scalars: &[(String, f32)], depth: u32) -> Option<[f32; 2]> {
    if depth > 8 {
        return None;
    }
    let Some(x) = input_of(p, e, input) else {
        // (No coordinates: the mesh's own.)
        return (input == "Coordinates").then_some([1.0, 1.0]);
    };
    let props = props::Props::read(p, &p.exports[x]).ok()?;
    match p.class_name(&p.exports[x]) {
        "MaterialExpressionTextureCoordinate" => Some([props.f32(p, "UTiling").unwrap_or(1.0), props.f32(p, "VTiling").unwrap_or(1.0)]),
        "MaterialExpressionMultiply" => {
            let (a, b) = (uv_of(p, x, "A", scalars, depth + 1), uv_of(p, x, "B", scalars, depth + 1));
            match (a, b) {
                (Some(u), None) => constant_of(p, x, "B", scalars, depth + 1).map(|c| [u[0] * c[0], u[1] * c[1]]),
                (None, Some(u)) => constant_of(p, x, "A", scalars, depth + 1).map(|c| [u[0] * c[0], u[1] * c[1]]),
                _ => None,
            }
        }
        "MaterialExpressionDivide" => {
            let u = uv_of(p, x, "A", scalars, depth + 1)?;
            let c = constant_of(p, x, "B", scalars, depth + 1)?;
            Some([u[0] / c[0], u[1] / c[1]])
        }
        // (Moved or turned, not scaled; an offset added.)
        "MaterialExpressionPanner" | "MaterialExpressionRotator" => uv_of(p, x, "Coordinate", scalars, depth + 1),
        "MaterialExpressionAdd" | "MaterialExpressionSubtract" => uv_of(p, x, "A", scalars, depth + 1).or_else(|| uv_of(p, x, "B", scalars, depth + 1)),
        _ => None,
    }
}

/// A constant input's value (a scalar both ways round, or two).
fn constant_of(p: &Package, e: usize, input: &str, scalars: &[(String, f32)], depth: u32) -> Option<[f32; 2]> {
    if depth > 8 {
        return None;
    }
    let x = input_of(p, e, input)?;
    let props = props::Props::read(p, &p.exports[x]).ok()?;
    match p.class_name(&p.exports[x]) {
        "MaterialExpressionConstant" => props.f32(p, "R").or(Some(0.0)).map(|r| [r, r]),
        "MaterialExpressionConstant2Vector" => Some([props.f32(p, "R").unwrap_or(0.0), props.f32(p, "G").unwrap_or(0.0)]),
        "MaterialExpressionScalarParameter" => {
            let n = props.name(p, "ParameterName").unwrap_or_default();
            let v = scalars.iter().find(|(s, _)| s.eq_ignore_ascii_case(&n)).map(|(_, v)| *v).or_else(|| props.f32(p, "DefaultValue")).unwrap_or(0.0);
            Some([v, v])
        }
        "MaterialExpressionMultiply" => {
            let (a, b) = (constant_of(p, x, "A", scalars, depth + 1)?, constant_of(p, x, "B", scalars, depth + 1)?);
            Some([a[0] * b[0], a[1] * b[1]])
        }
        _ => None,
    }
}

/// Where a material's opacity comes from (a cut-out's `OpacityMask` input, a see-through one's
/// `Opacity`), when it's a texture: that texture (an instance's in place of its parameter's
/// default) and its channel (0-3). None when it's worked out some other way (then the colour
/// map's alpha stands in).
fn material_opacity(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32, max_texture: u32, input: &'static str) -> Option<(Rgba, usize)> {
    let mut overrides: Vec<(String, Rc<Package>, i32)> = vec![];
    let mut next = Some((pkg.clone(), m));
    for _ in 0..6 {
        let (p, obj) = next.take()?;
        let (mp, mi) = pkgs.resolve(&p, obj)?;
        let props = props::Props::read(&mp, &mp.exports[mi]).ok()?;
        if mp.class_name(&mp.exports[mi]) != "Material" {
            for (name, tex) in material_textures(&mp, mi) {
                if !overrides.iter().any(|(n, _, _)| n.eq_ignore_ascii_case(&name)) {
                    overrides.push((name, mp.clone(), tex));
                }
            }
            next = props.object(&mp, "Parent").map(|par| (mp.clone(), par));
            continue;
        }
        // (Its sample, through what only scales or fades it: a fence's near and far tilings
        // faded with distance, a clamp, a constant times it.)
        let (mut at, mut name) = (mi, input);
        let (ex, input) = 'find: {
            for _ in 0..8 {
                let p = props::Props::read(&mp, &mp.exports[at]).ok()?;
                let t = p.get(name)?;
                let input = props::Props::read_list(&mp, t.at, t.at + t.size, "ExpressionInput").ok()?;
                let ex = input.object(&mp, "Expression").filter(|&e| e > 0)? as usize - 1;
                match mp.class_name(&mp.exports[ex]) {
                    c if c.contains("TextureSample") => break 'find (ex, input),
                    "MaterialExpressionLinearInterpolate" => name = "A",
                    "MaterialExpressionClamp" | "MaterialExpressionConstantClamp" => name = "Input",
                    "MaterialExpressionMultiply" => {
                        let sampled = |n: &str| input_of(&mp, ex, n).is_some_and(|x| mp.class_name(&mp.exports[x]).contains("TextureSample"));
                        name = if sampled("A") { "A" } else if sampled("B") { "B" } else { return None };
                    }
                    _ => return None,
                }
                at = ex;
            }
            return None;
        };
        let ch = if input.i32(&mp, "MaskA") == Some(1) {
            3
        } else if input.i32(&mp, "MaskR") == Some(1) {
            0
        } else if input.i32(&mp, "MaskG") == Some(1) {
            1
        } else if input.i32(&mp, "MaskB") == Some(1) {
            2
        } else {
            3
        };
        let sample = props::Props::read(&mp, &mp.exports[ex]).ok()?;
        let param = sample.name(&mp, "ParameterName");
        let (tp, tex) = match param.and_then(|n| overrides.iter().find(|(o, _, _)| o.eq_ignore_ascii_case(&n)).cloned()) {
            Some((_, op, t)) => (op, t),
            None => (mp.clone(), sample.object(&mp, "Texture")?),
        };
        let (rp, ri) = pkgs.resolve(&tp, tex)?;
        let read = texture::read_texture_from(&rp, &rp.exports[ri], max_texture, &mut |n| pkgs.raw(n)).ok()?;
        return Some((read, ch));
    }
    None
}

/// Whether a physical material lets Faith land soft on it: its Mirror's Edge property's
/// `bEnableSoftLanding` (a wrapped construction package's plastic), or its parent's.
pub fn phys_soft(pkgs: &mut Packages, pkg: &Rc<Package>, obj: i32) -> bool {
    let mut next = Some((pkg.clone(), obj));
    for _ in 0..4 {
        let Some((p, o)) = next.take().filter(|(_, o)| *o != 0) else { break };
        let Some((pp, pi)) = pkgs.resolve(&p, o) else { break };
        let Ok(props) = props::Props::read(&pp, &pp.exports[pi]) else { break };
        if let Some((qp, qi)) = props.object(&pp, "PhysicalMaterialProperty").filter(|&x| x != 0).and_then(|x| pkgs.resolve(&pp, x)) {
            if props::Props::read(&qp, &qp.exports[qi]).is_ok_and(|q| q.get("bEnableSoftLanding").is_some_and(|t| t.bool_value)) {
                return true;
            }
        }
        next = props.object(&pp, "Parent").map(|x| (pp.clone(), x));
    }
    false
}

/// A material's physical material: an instance's own, else its parents'.
fn material_phys(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32) -> Option<(Rc<Package>, i32)> {
    let mut next = Some((pkg.clone(), m));
    for _ in 0..6 {
        let (p, obj) = next.take().filter(|(_, o)| *o != 0)?;
        let (mp, mi) = pkgs.resolve(&p, obj)?;
        let props = props::Props::read(&mp, &mp.exports[mi]).ok()?;
        if let Some(ph) = props.object(&mp, "PhysMaterial").filter(|&x| x != 0) {
            return Some((mp, ph));
        }
        next = props.object(&mp, "Parent").map(|x| (mp.clone(), x));
    }
    None
}

/// Whether Faith lands soft on a placed mesh (`mesh`, an export of `mesh_pkg`), as the game
/// finds its physical material: its component's (`phys`, of `pkg`), else its mesh's body's,
/// else its materials' (the component's `overrides` in place of the mesh's own) where any is.
pub fn soft_landing(pkgs: &mut Packages, pkg: &Rc<Package>, phys: i32, overrides: &[i32], mesh_pkg: &Rc<Package>, mesh: usize) -> bool {
    if phys != 0 {
        return phys_soft(pkgs, pkg, phys);
    }
    let body = props::Props::read(mesh_pkg, &mesh_pkg.exports[mesh])
        .ok()
        .and_then(|p| p.object(mesh_pkg, "BodySetup"))
        .filter(|&b| b > 0)
        .and_then(|b| props::Props::read(mesh_pkg, &mesh_pkg.exports[b as usize - 1]).ok())
        .and_then(|b| b.object(mesh_pkg, "PhysMaterial"))
        .filter(|&x| x != 0);
    if let Some(ph) = body {
        return phys_soft(pkgs, mesh_pkg, ph);
    }
    let own: Vec<i32> = staticmesh::StaticMesh::read(mesh_pkg, &mesh_pkg.exports[mesh].name).map(|m| m.sections.iter().map(|s| s.material).collect()).unwrap_or_default();
    own.iter().enumerate().any(|(k, &m)| {
        let (p, m) = match overrides.get(k).filter(|&&o| o != 0) {
            Some(&o) => (pkg.clone(), o),
            None => (mesh_pkg.clone(), m),
        };
        material_phys(pkgs, &p, m).is_some_and(|(pp, ph)| phys_soft(pkgs, &pp, ph))
    })
}

/// Whether a map's alpha is holes and solid (a wire mesh's), not shades between (dirty glass,
/// a soft stain): much of it clear, and little of it neither clear nor solid.
fn cut_out(t: &Rgba) -> bool {
    let n = (t.pixels.len() / 4).max(1) as f32;
    let (mut clear, mut between) = (0usize, 0usize);
    for c in t.pixels.chunks_exact(4) {
        match c[3] {
            0..26 => clear += 1,
            26..204 => between += 1,
            _ => {}
        }
    }
    clear as f32 / n > 0.2 && between as f32 / n < 0.35
}

/// A material's blend mode and two-sidedness: its root material's (an instance can't change
/// them).
fn material_blend(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32) -> (Blend, bool) {
    let mut next = Some((pkg.clone(), m));
    for _ in 0..6 {
        let Some((p, obj)) = next.take() else { break };
        let Some((mp, mi)) = pkgs.resolve(&p, obj) else { break };
        let Ok(props) = props::Props::read(&mp, &mp.exports[mi]) else { break };
        if mp.class_name(&mp.exports[mi]) == "Material" {
            let two = props.get("TwoSided").is_some_and(|t| t.bool_value);
            let clip = props.f32(&mp, "OpacityMaskClipValue").unwrap_or(1.0 / 3.0);
            let blend = match props.name(&mp, "BlendMode").as_deref() {
                Some("BLEND_Masked") => Blend::Masked(clip),
                Some("BLEND_Translucent") => Blend::Translucent,
                Some("BLEND_Additive") | Some("BLEND_Modulate") => Blend::Additive,
                // (An opaque material set to mask, as some are.)
                _ if props.get("bIsMasked").is_some_and(|t| t.bool_value) => Blend::Masked(clip),
                _ => Blend::Opaque,
            };
            return (blend, two);
        }
        next = props.object(&mp, "Parent").map(|par| (mp.clone(), par));
    }
    (Blend::Opaque, false)
}

/// A colour parameter's value: a LinearColor, as four floats or tagged R, G, B.
fn linear_color(pkg: &Package, t: &props::Tag) -> Option<[f32; 3]> {
    if t.size == 16 {
        let mut c = package::Cursor::new(&pkg.data, t.at);
        return Some([c.f32().ok()?, c.f32().ok()?, c.f32().ok()?]);
    }
    let inner = props::Props::read_list(pkg, t.at, t.at + t.size, "LinearColor").ok()?;
    Some([inner.f32(pkg, "R")?, inner.f32(pkg, "G")?, inner.f32(pkg, "B")?])
}

/// Every colour parameter a material has, by name: following an instance to its parents, and
/// at the end the material's own parameters' defaults.
fn material_vectors(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32) -> Vec<(String, [f32; 3])> {
    let mut out: Vec<(String, [f32; 3])> = vec![];
    let add = |name: String, c: [f32; 3], out: &mut Vec<(String, [f32; 3])>| {
        if !out.iter().any(|(n, _)| *n == name) {
            out.push((name, c));
        }
    };
    let mut next = Some((pkg.clone(), m));
    for _ in 0..6 {
        let Some((p, obj)) = next.take() else { break };
        let Some((mp, mi)) = pkgs.resolve(&p, obj) else { break };
        let Ok(props) = props::Props::read(&mp, &mp.exports[mi]) else { break };
        for el in props.struct_array(&mp, "VectorParameterValues") {
            let name = el.name(&mp, "ParameterName").unwrap_or_default().to_ascii_lowercase();
            if let Some(c) = el.get("ParameterValue").and_then(|t| linear_color(&mp, t)) {
                add(name, c, &mut out);
            }
        }
        if mp.class_name(&mp.exports[mi]) == "Material" {
            let me = mi as i32 + 1;
            for e in mp.exports.iter().filter(|e| e.outer == me && mp.class_name(e) == "MaterialExpressionVectorParameter") {
                let Ok(c) = props::Props::read(&mp, e) else { continue };
                let name = c.name(&mp, "ParameterName").unwrap_or_default().to_ascii_lowercase();
                if let Some(v) = c.get("DefaultValue").and_then(|t| linear_color(&mp, t)) {
                    add(name, v, &mut out);
                }
            }
            break;
        }
        next = props.object(&mp, "Parent").map(|par| (mp.clone(), par));
    }
    out
}

/// Where a tint fades out, as a lerp's alpha has it (the tinted map at 0, the plain one at 1):
/// a texture's channel, or one side of a line across the map's UVs (`1 - ceil(u + c)`).
enum Fade {
    Mask(Rc<Package>, i32, usize),
    Above(usize, f32),
}

fn fade_of(p: &Rc<Package>, x: usize, overrides: &[(String, Rc<Package>, i32)]) -> Option<Fade> {
    let class = |e: usize| p.class_name(&p.exports[e]).to_string();
    let props = props::Props::read(p, &p.exports[x]).ok()?;
    let t = props.get("Alpha")?;
    let input = props::Props::read_list(p, t.at, t.at + t.size, "ExpressionInput").ok()?;
    let a = input.object(p, "Expression").filter(|&e| e > 0)? as usize - 1;
    if class(a).contains("TextureSample") {
        let ch = ["MaskR", "MaskG", "MaskB", "MaskA"].iter().position(|m| input.i32(p, m) == Some(1)).unwrap_or(0);
        let sample = props::Props::read(p, &p.exports[a]).ok()?;
        let over = sample.name(p, "ParameterName").and_then(|n| overrides.iter().find(|(o, _, _)| o.eq_ignore_ascii_case(&n)).cloned());
        return Some(match over {
            Some((_, op, tex)) => Fade::Mask(op, tex, ch),
            None => Fade::Mask(p.clone(), sample.object(p, "Texture").filter(|&t| t != 0)?, ch),
        });
    }
    // 1 - ceil(u + c): the plain map where u <= -c.
    if class(a) != "MaterialExpressionOneMinus" {
        return None;
    }
    let ceil = input_of(p, a, "Input").filter(|&e| class(e) == "MaterialExpressionCeil")?;
    let add = input_of(p, ceil, "Input").filter(|&e| class(e) == "MaterialExpressionAdd")?;
    let (mask, c) = match (input_of(p, add, "A"), input_of(p, add, "B")) {
        (Some(m), Some(k)) if class(m) == "MaterialExpressionComponentMask" => (m, k),
        (Some(k), Some(m)) if class(m) == "MaterialExpressionComponentMask" => (m, k),
        _ => return None,
    };
    input_of(p, mask, "Input").filter(|&e| class(e) == "MaterialExpressionTextureCoordinate")?;
    let mp = props::Props::read(p, &p.exports[mask]).ok()?;
    let axis = if mp.get("R").is_some_and(|t| t.bool_value) { 0 } else if mp.get("G").is_some_and(|t| t.bool_value) { 1 } else { return None };
    let k = props::Props::read(p, &p.exports[c]).ok().filter(|_| class(c) == "MaterialExpressionConstant")?.f32(p, "R").unwrap_or(0.0);
    Some(Fade::Above(axis, -k))
}

/// A tint found in a material's diffuse: its colour parameter (an export), where on the map,
/// how much of it there is (a lerp's scalar amount: a runner-vision highlight's strength) and
/// what brightens it.
struct Tinted {
    param: usize,
    fade: Option<Fade>,
    amount: f32,
    bright: f32,
}

/// A scalar input's value, if it's a constant or a scalar parameter (an instance's value,
/// else its default).
fn scalar_input(p: &Package, e: usize, input: &str, scalars: &[(String, f32)]) -> Option<f32> {
    let x = input_of(p, e, input)?;
    let props = props::Props::read(p, &p.exports[x]).ok()?;
    match p.class_name(&p.exports[x]) {
        "MaterialExpressionConstant" => props.f32(p, "R").or(Some(0.0)),
        "MaterialExpressionScalarParameter" => {
            let n = props.name(p, "ParameterName").unwrap_or_default();
            scalars.iter().find(|(s, _)| s.eq_ignore_ascii_case(&n)).map(|(_, v)| *v).or_else(|| props.f32(p, "DefaultValue")).or(Some(0.0))
        }
        _ => None,
    }
}

/// The colour parameter a material's diffuse is its map times, and where on the map it tints:
/// through what fades it back to the plain map (or in, by an amount), multiplies or brightens
/// it.
fn tint_param(p: &Rc<Package>, e: usize, input: &str, overrides: &[(String, Rc<Package>, i32)], scalars: &[(String, f32)], depth: u32) -> Option<Tinted> {
    if depth > 8 {
        return None;
    }
    let x = input_of(p, e, input)?;
    let class = |e: usize| p.class_name(&p.exports[e]).to_string();
    let is = |n: &str, c: &str| input_of(p, x, n).filter(|&y| class(y).contains(c));
    match class(x).as_str() {
        "MaterialExpressionMultiply" => {
            for (a, b) in [("A", "B"), ("B", "A")] {
                if let (Some(_), Some(v)) = (is(a, "TextureSample"), is(b, "MaterialExpressionVectorParameter")) {
                    return Some(Tinted { param: v, fade: None, amount: 1.0, bright: 1.0 });
                }
            }
            tint_param(p, x, "A", overrides, scalars, depth + 1).or_else(|| tint_param(p, x, "B", overrides, scalars, depth + 1))
        }
        "MaterialExpressionLinearInterpolate" => {
            let amount = scalar_input(p, x, "Alpha", scalars);
            match tint_param(p, x, "A", overrides, scalars, depth + 1) {
                Some(mut t) => match amount {
                    Some(k) => {
                        // (B is A brightened, by a constant: brighter by the amount. Else the
                        // tint fades out by it.)
                        let a = input_of(p, x, "A");
                        let brighter = input_of(p, x, "B").filter(|&y| class(y) == "MaterialExpressionMultiply").and_then(|y| {
                            let (ya, yb) = (input_of(p, y, "A"), input_of(p, y, "B"));
                            let c = if yb == a { ya } else if ya == a { yb } else { None }?;
                            class(c).starts_with("MaterialExpressionConstant").then(|| props::Props::read(p, &p.exports[c]).ok()?.f32(p, "R")).flatten()
                        });
                        match brighter {
                            Some(c) => t.bright *= 1.0 + (c - 1.0) * k,
                            None => t.amount *= 1.0 - k,
                        }
                        Some(t)
                    }
                    // (The tinted map, faded back to the plain one where the alpha says.)
                    None if t.fade.is_none() && is("B", "TextureSample").is_some() => Some(Tinted { fade: fade_of(p, x, overrides), ..t }),
                    None => Some(t),
                },
                // (The tint faded in by an amount: a highlight, as strong as its amount. Faded in
                // some other way, it's not followed.)
                None => {
                    let t = tint_param(p, x, "B", overrides, scalars, depth + 1)?;
                    let k = amount?;
                    Some(Tinted { amount: t.amount * k, ..t })
                }
            }
        }
        _ => None,
    }
}

/// A material's scalar parameters' values: an instance's own first, then its parents'.
fn material_scalars(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32) -> Vec<(String, f32)> {
    let mut out: Vec<(String, f32)> = vec![];
    let mut next = Some((pkg.clone(), m));
    for _ in 0..6 {
        let Some((p, obj)) = next.take() else { break };
        let Some((mp, mi)) = pkgs.resolve(&p, obj) else { break };
        if mp.class_name(&mp.exports[mi]) == "Material" {
            break;
        }
        let Ok(props) = props::Props::read(&mp, &mp.exports[mi]) else { break };
        for v in props.struct_array(&mp, "ScalarParameterValues") {
            if let (Some(n), Some(x)) = (v.name(&mp, "ParameterName"), v.f32(&mp, "ParameterValue")) {
                if !out.iter().any(|(s, _)| s.eq_ignore_ascii_case(&n)) {
                    out.push((n, x));
                }
            }
        }
        next = props.object(&mp, "Parent").map(|par| (mp.clone(), par));
    }
    out
}

/// A material's diffuse tint from its graph: the colour its map is multiplied by (an
/// instance's value of the parameter, else the parameter's own, as much of it as its graph
/// lets through), and where on the map.
fn diffuse_tint(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32, vectors: &[(String, [f32; 3])], max_texture: u32) -> Option<([f32; 3], TintWhere)> {
    let mut overrides: Vec<(String, Rc<Package>, i32)> = vec![];
    let scalars = material_scalars(pkgs, pkg, m);
    let mut next = Some((pkg.clone(), m));
    let mut base = None;
    for _ in 0..6 {
        let (p, obj) = next.take()?;
        let (mp, mi) = pkgs.resolve(&p, obj)?;
        if mp.class_name(&mp.exports[mi]) == "Material" {
            base = Some((mp, mi));
            break;
        }
        for (name, tex) in material_textures(&mp, mi) {
            if !overrides.iter().any(|(n, _, _)| n.eq_ignore_ascii_case(&name)) {
                overrides.push((name, mp.clone(), tex));
            }
        }
        next = props::Props::read(&mp, &mp.exports[mi]).ok()?.object(&mp, "Parent").map(|par| (mp.clone(), par));
    }
    let (mp, mi) = base?;
    let t = tint_param(&mp, mi, "DiffuseColor", &overrides, &scalars, 0)?;
    let vp = props::Props::read(&mp, &mp.exports[t.param]).ok()?;
    let name = vp.name(&mp, "ParameterName").unwrap_or_default().to_ascii_lowercase();
    let full = vectors
        .iter()
        .find(|(n, _)| !name.is_empty() && *n == name)
        .map(|(_, c)| *c)
        .or_else(|| vp.get("DefaultValue").and_then(|t| linear_color(&mp, t)))?;
    let color = full.map(|c| (1.0 + (c - 1.0) * t.amount) * t.bright);
    // (None of it: no tint.)
    if color.iter().all(|c| (c - 1.0).abs() < 1e-3) {
        return None;
    }
    let place = match t.fade {
        None => TintWhere::All,
        Some(Fade::Above(axis, at)) => TintWhere::Above(axis, at),
        Some(Fade::Mask(tp, tex, ch)) => {
            let (rp, ri) = pkgs.resolve(&tp, tex)?;
            let t = texture::read_texture_from(&rp, &rp.exports[ri], max_texture.min(512), &mut |n| pkgs.raw(n)).ok()?;
            TintWhere::Mask(t, ch)
        }
    };
    Some((color, place))
}

/// A material instance's diffuse tint: its (or the instance it inherits from's) colour
/// parameter named for the diffuse (`DiffuseColor`, `DiffuseColour01`, ...).
fn material_tint(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32) -> Option<[f32; 3]> {
    let mut next = Some((pkg.clone(), m));
    for _ in 0..4 {
        let (p, obj) = next.take()?;
        let (mp, mi) = pkgs.resolve(&p, obj)?;
        let props = props::Props::read(&mp, &mp.exports[mi]).ok()?;
        for el in props.struct_array(&mp, "VectorParameterValues") {
            let name = el.name(&mp, "ParameterName").unwrap_or_default().to_ascii_lowercase();
            if !(name.contains("diffusecol") || name == "color" || name == "colour" || name == "tint") {
                continue;
            }
            let t = el.get("ParameterValue")?;
            let c = if t.size == 16 {
                let mut cur = package::Cursor::new(&mp.data, t.at);
                [cur.f32().ok()?, cur.f32().ok()?, cur.f32().ok()?]
            } else {
                let inner = props::Props::read_list(&mp, t.at, t.at + t.size, "LinearColor").ok()?;
                [inner.f32(&mp, "R")?, inner.f32(&mp, "G")?, inner.f32(&mp, "B")?]
            };
            return Some(c);
        }
        next = props.object(&mp, "Parent").map(|par| (mp.clone(), par));
    }
    None
}

/// A material's full path, where it's defined (an import followed to its package).
fn material_path(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32) -> String {
    match pkgs.resolve(pkg, m) {
        Some((p, i)) => p.path(i as i32 + 1),
        None => pkg.path(m),
    }
}

/// A plain material's colour from its constants (its first Constant3Vector), and whether it
/// has an emissive input, following an instance to its parent.
fn constant_color(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32) -> (Option<[f32; 3]>, bool) {
    let mut next = Some((pkg.clone(), m));
    for _ in 0..4 {
        let Some((p, obj)) = next.take() else { break };
        let Some((mp, mi)) = pkgs.resolve(&p, obj) else { break };
        let Ok(props) = props::Props::read(&mp, &mp.exports[mi]) else { break };
        if mp.class_name(&mp.exports[mi]) == "Material" {
            let emissive = props.get("EmissiveColor").is_some();
            let me_index = mi as i32 + 1;
            let kids = || mp.exports.iter().filter(|e| e.outer == me_index);
            let constant = kids().filter(|e| mp.class_name(e) == "MaterialExpressionConstant3Vector").find_map(|e| {
                let c = props::Props::read(&mp, e).ok()?;
                Some([c.f32(&mp, "R").unwrap_or(0.0), c.f32(&mp, "G").unwrap_or(0.0), c.f32(&mp, "B").unwrap_or(0.0)])
            });
            // A colour parameter's default (a LinearColor: four floats, or tagged R, G, B).
            let parameter = || {
                kids().filter(|e| mp.class_name(e) == "MaterialExpressionVectorParameter").find_map(|e| {
                    let c = props::Props::read(&mp, e).ok()?;
                    let t = c.get("DefaultValue")?;
                    if t.size == 16 {
                        let mut cur = package::Cursor::new(&mp.data, t.at);
                        return Some([cur.f32().ok()?, cur.f32().ok()?, cur.f32().ok()?]);
                    }
                    let inner = props::Props::read_list(&mp, t.at, t.at + t.size, "LinearColor").ok()?;
                    Some([inner.f32(&mp, "R")?, inner.f32(&mp, "G")?, inner.f32(&mp, "B")?])
                })
            };
            let color = constant.or_else(parameter);
            return (color, emissive);
        }
        next = props.object(&mp, "Parent").map(|par| (mp.clone(), par));
    }
    (None, false)
}

/// A material's textures: (name, lowercased with its parameter name, package, export).
pub fn all_textures(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32) -> Vec<(String, Rc<Package>, usize)> {
    let mut out = vec![];
    let mut next = Some((pkg.clone(), m));
    for _ in 0..4 {
        let Some((p, obj)) = next.take() else { break };
        let Some((mp, mi)) = pkgs.resolve(&p, obj) else { break };
        for (pname, tex) in material_textures(&mp, mi) {
            if let Some((tp, ti)) = pkgs.resolve(&mp, tex) {
                let name = format!("{} {}", pname, tp.exports[ti].name).to_ascii_lowercase();
                out.push((name, tp, ti));
            }
        }
        // A plain material: the textures its expressions sample.
        let me_index = mi as i32 + 1;
        for e in &mp.exports {
            if e.outer == me_index && mp.class_name(e).contains("TextureSample") {
                if let Ok(props) = props::Props::read(&mp, e) {
                    if let Some(tex) = props.object(&mp, "Texture") {
                        if let Some((tp, ti)) = pkgs.resolve(&mp, tex) {
                            out.push((tp.exports[ti].name.to_ascii_lowercase(), tp, ti));
                        }
                    }
                }
            }
        }
        // An instance inherits what it doesn't set from its parent.
        let parent = props::Props::read(&mp, &mp.exports[mi]).ok().and_then(|pr| pr.object(&mp, "Parent"));
        next = parent.map(|par| (mp.clone(), par));
    }
    out
}

/// The biggest a mesh's textures need be: a small prop's are seen small (under 3 m: half
/// `max`; under 80 cm: a quarter).
fn texture_for(mesh: &StaticMesh, max: u32) -> u32 {
    let size = mesh.extent.iter().fold(0.0f32, |m, e| m.max(*e)) * 2.0;
    if size < 80.0 {
        (max / 4).max(128)
    } else if size < 300.0 {
        (max / 2).max(128)
    } else {
        max
    }
}

/// One of Mirror's Edge's static meshes (`name` in package `package`) and its materials'
/// textures, read from the install.
pub fn load_static_mesh(pkgs: &mut Packages, package: &str, name: &str, max_texture: u32) -> Result<(StaticMesh, Vec<MaterialSlot>)> {
    let pkg = pkgs.get(package)?;
    let mesh = StaticMesh::read(&pkg, name)?;
    let mats: Vec<i32> = mesh.sections.iter().map(|s| s.material).collect();
    let slots = slots_for(pkgs, &pkg, &mats, texture_for(&mesh, max_texture));
    Ok((mesh, slots))
}

/// The same, with some of its sections' materials replaced (as a placed mesh's component
/// replaces them): each override a material's path in `package` (its groups and name), or
/// empty to keep the mesh's own.
pub fn load_static_mesh_with(pkgs: &mut Packages, package: &str, name: &str, overrides: &[String], max_texture: u32) -> Result<(StaticMesh, Vec<MaterialSlot>)> {
    let pkg = pkgs.get(package)?;
    let mesh = StaticMesh::read(&pkg, name)?;
    let mats: Vec<i32> = mesh
        .sections
        .iter()
        .enumerate()
        .map(|(k, s)| {
            overrides
                .get(k)
                .filter(|o| !o.is_empty())
                .and_then(|o| (0..pkg.exports.len()).find(|&i| pkg.path(i as i32 + 1) == *o))
                .map_or(s.material, |i| i as i32 + 1)
        })
        .collect();
    let slots = slots_for(pkgs, &pkg, &mats, texture_for(&mesh, max_texture));
    Ok((mesh, slots))
}

/// A material instance's texture parameters: (parameter name, texture
/// object index in `pkg`).
pub fn material_textures(pkg: &Package, export: usize) -> Vec<(String, i32)> {
    let mut out = vec![];
    let Some(e) = pkg.exports.get(export) else { return out };
    let Ok(props) = props::Props::read(pkg, e) else { return out };
    let Some(arr) = props.get("TextureParameterValues") else { return out };
    // Array of structs, each a tagged property list: walk it.
    let mut c = package::Cursor::new(&pkg.data, arr.at);
    let Ok(n) = c.count(8) else { return out };
    let mut walk = || -> Option<()> {
        for _ in 0..n {
            let mut pname = String::new();
            let mut value = 0;
            loop {
                let name = package::read_name(&pkg.names, &mut c).ok()?;
                if name == "None" {
                    break;
                }
                let kind = package::read_name(&pkg.names, &mut c).ok()?;
                let size = c.i32().ok()? as usize;
                c.i32().ok()?;
                if kind == "StructProperty" {
                    package::read_name(&pkg.names, &mut c).ok()?;
                }
                if kind == "BoolProperty" {
                    c.i32().ok()?;
                    continue;
                }
                let at = c.pos;
                match name.as_str() {
                    "ParameterName" => pname = package::read_name(&pkg.names, &mut c).ok()?,
                    "ParameterValue" => value = c.i32().ok()?,
                    _ => {}
                }
                c.pos = at + size;
            }
            if value != 0 {
                out.push((pname, value));
            }
        }
        Some(())
    };
    walk();
    out
}

#[cfg(test)]
mod tests;
#[cfg(all(test, feature = "prologue"))]
mod level_tests;
