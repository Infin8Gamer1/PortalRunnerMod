//! me_assets: read Mirror's Edge's first-person arms from a local install.
//!
//! Nothing from the game ships with this crate. [`FaithArms::load`] opens the
//! player's own `TdGame/CookedPC` packages at runtime:
//! - `Characters/CH_TKY_Crim_Fixer_1P.upk`: the first-person upper body
//!   (`SK_UpperBody`, the shared 1P skeleton) and Faith's skin/glove textures;
//! - `Animations/AS_C1P_Unarmed.upk`: every unarmed first-person animation.

pub mod anim;
pub mod bsp;
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
    /// The colour its diffuse map is multiplied by (a colour variant's `DiffuseColor`
    /// parameter: an orange catwalk, a blue rooftop box), linear, if it has one.
    pub tint: Option<[f32; 3]>,
    /// Its colour parameters (lowercased names, linear colours): an instance's own first, then
    /// its parents', then the defaults its material's parameters have.
    pub vectors: Vec<(String, [f32; 3])>,
    /// How it's drawn (its material's blend mode).
    pub blend: Blend,
    /// Drawn from both sides.
    pub two_sided: bool,
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
            let read = |pick: &dyn Fn(&str) -> bool| {
                textures.iter().filter(|(name, _, _)| pick(name)).find_map(|(_, tp, ti)| {
                    let mut f = files.borrow_mut();
                    texture::read_texture_from(tp, &tp.exports[*ti], max_texture, &mut |n| f.raw(n)).ok()
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
            let normal_stem = textures.iter().map(|(n, _, _)| n).find(|n| is_normal(n)).map(|n| stem(n));
            let overlay = |n: &str| ["dirt", "detail", "grime", "sand", "dust", "mask", "stain"].iter().any(|w| n.rsplit(' ').next().unwrap_or(n).contains(w));
            let mut diffuse = normal_stem.as_ref().and_then(|ns| read(&|n| is_diffuse(n) && stem(n) == *ns));
            if diffuse.is_none() {
                diffuse = read(&|n| is_diffuse(n) && !overlay(n));
            }
            if diffuse.is_none() {
                diffuse = read(&is_diffuse);
            }
            let normal = read(&is_normal);
            let specular = read(&is_spec);
            if diffuse.is_none() {
                // Anything that isn't a normal, specular, mask or emissive map.
                diffuse = read(&|n| !(is_normal(n) || is_spec(n) || n.contains("mask") || matches!(suffix(n).as_str(), "m" | "e" | "em")));
            }
            drop(files);
            // A cut-out's opacity where its material takes it from (another texture, or another
            // channel), into the colour map's alpha.
            let (blend, two_sided) = material_blend(pkgs, pkg, m);
            if let (Blend::Masked(_), Some(d)) = (blend, diffuse.as_mut()) {
                if let Some((t, ch)) = material_opacity(pkgs, pkg, m, max_texture) {
                    for y in 0..d.height {
                        for x in 0..d.width {
                            let (sx, sy) = (x * t.width / d.width, y * t.height / d.height);
                            d.pixels[((y * d.width + x) * 4 + 3) as usize] = t.pixels[((sy * t.width + sx) * 4) as usize + ch];
                        }
                    }
                }
            }
            let (color, emissive) = if diffuse.is_none() { constant_color(pkgs, pkg, m) } else { (None, false) };
            let path = material_path(pkgs, pkg, m);
            // (Only a colour variant's, named for its colour: other instances' colour
            // parameters tint through masks this doesn't follow, and would dye them all over.)
            let named = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            let variant = ["orange", "blue", "yellow", "red", "green", "white", "black", "grey", "gray"].iter().any(|c| named.contains(c));
            let tint = if variant { material_tint(pkgs, pkg, m) } else { None };
            let vectors = material_vectors(pkgs, pkg, m);
            MaterialSlot { material, path, diffuse, normal, specular, color, emissive, tint, vectors, blend, two_sided }
        })
        .collect()
}

/// Where a cut-out material's opacity comes from, when it's a texture: that texture (an
/// instance's in place of its parameter's default) and its channel (0-3). None when it's worked
/// out some other way (then the colour map's alpha stands in).
fn material_opacity(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32, max_texture: u32) -> Option<(Rgba, usize)> {
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
        let t = props.get("OpacityMask")?;
        let input = props::Props::read_list(&mp, t.at, t.at + t.size, "ExpressionInput").ok()?;
        let ex = input.object(&mp, "Expression").filter(|&e| e > 0)? as usize - 1;
        let class = mp.class_name(&mp.exports[ex]).to_string();
        if !class.contains("TextureSample") {
            return None;
        }
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

/// One of Mirror's Edge's static meshes (`name` in package `package`) and its materials'
/// textures, read from the install.
pub fn load_static_mesh(pkgs: &mut Packages, package: &str, name: &str, max_texture: u32) -> Result<(StaticMesh, Vec<MaterialSlot>)> {
    let pkg = pkgs.get(package)?;
    let mesh = StaticMesh::read(&pkg, name)?;
    let mats: Vec<i32> = mesh.sections.iter().map(|s| s.material).collect();
    let slots = slots_for(pkgs, &pkg, &mats, max_texture);
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
    let slots = slots_for(pkgs, &pkg, &mats, max_texture);
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
