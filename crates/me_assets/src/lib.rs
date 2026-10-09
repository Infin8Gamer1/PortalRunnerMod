//! me_assets: read Mirror's Edge's first-person arms from a local install.
//!
//! Nothing from the game ships with this crate. [`FaithArms::load`] opens the
//! player's own `TdGame/CookedPC` packages at runtime:
//! - `Characters/CH_TKY_Crim_Fixer_1P.upk`: the first-person upper body
//!   (`SK_UpperBody`, the shared 1P skeleton) and Faith's skin/glove textures;
//! - `Animations/AS_C1P_Unarmed.upk`: every unarmed first-person animation.

pub mod anim;
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
    pub diffuse: Option<Rgba>,
    /// Tangent-space normal map (the game's TC_Normalmap textures).
    pub normal: Option<Rgba>,
    /// Specular intensity map (grey-scale in practice).
    pub specular: Option<Rgba>,
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
        Packages { cooked: cooked.to_path_buf(), files, open: HashMap::new() }
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
            let textures = all_textures(pkgs, pkg, m);
            let read = |pick: &dyn Fn(&str) -> bool| {
                let (tp, ti) = textures.iter().find(|(name, _, _)| pick(name)).map(|(_, p, i)| (p.clone(), *i))?;
                texture::read_texture(&tp, &tp.exports[ti].name, max_texture).ok()
            };
            let mut diffuse = read(&|n| n.contains("diffuse") || n.ends_with("_d") || n.contains("_d_") || n.ends_with("_c"));
            let normal = read(&|n| n.contains("normal") || n.ends_with("_n"));
            let specular = read(&|n| (n.contains("spec") || n.ends_with("_s")) && !n.contains("tile"));
            if diffuse.is_none() {
                // Anything that isn't a normal or specular map.
                diffuse = read(&|n| !(n.contains("normal") || n.ends_with("_n") || n.contains("spec") || n.ends_with("_s") || n.contains("mask")));
            }
            MaterialSlot { material, diffuse, normal, specular }
        })
        .collect()
}

/// A material's textures: (name, lowercased with its parameter name, package, export).
fn all_textures(pkgs: &mut Packages, pkg: &Rc<Package>, m: i32) -> Vec<(String, Rc<Package>, usize)> {
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
