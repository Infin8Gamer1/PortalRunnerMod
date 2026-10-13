//! The props browser's list: every model `portal_extract` wrote into `props/` (Portal's and
//! Portal 2's, with their bounds in `props/index.tsv`), and every static mesh in Mirror's
//! Edge's prop packages, read from the install. Read on a thread the first time the editor
//! opens.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

use bevy::prelude::*;

use aperture::doc::{Kind, Object};

/// The games the props come from.
pub const GAMES: [&str; 3] = ["Portal", "Portal 2", "Mirror's Edge"];

/// Mirror's Edge's folders (under CookedPC) whose packages are props.
const ME_FOLDERS: [&str; 4] = ["Props", "Interiors", "Vehicles", "Accessories"];

pub enum Source {
    /// A model under the extracted folder (`props/portal2/...glb`).
    Model(String),
    /// A Mirror's Edge package and static mesh.
    Me(&'static str, &'static str),
}

pub struct Prop {
    pub name: String,
    /// Which of [`GAMES`].
    pub game: usize,
    /// Its folder in the browser (the model's first folder; the package).
    pub folder: String,
    pub source: Source,
    /// Its box, from its origin (Mirror's Edge's: once its thumbnail's been made).
    pub bounds: Option<(Vec3, Vec3)>,
    /// How many of its materials have a look of their own (a texture, a colour), of how many
    /// (a model's, from the index).
    pub looks: Option<(usize, usize)>,
}

#[derive(Resource, Default)]
pub struct Library {
    pub props: Vec<Prop>,
    /// Each game's folders, sorted.
    pub folders: [Vec<String>; 3],
    /// The models by path (`props/portal2/...glb`).
    pub by_path: std::collections::HashMap<String, usize>,
    pub loaded: bool,
    loading: Option<Mutex<Receiver<Vec<Prop>>>>,
}

impl Library {
    /// The object a prop makes, dropped at `p` on a surface facing `n`, seen looking along
    /// `look`: its front to the camera (a quarter turn at a time), resting on the surface.
    pub fn object(&self, i: usize, p: Vec3, n: Vec3, look: Vec3) -> Object {
        let prop = &self.props[i];
        let bounds = prop.bounds;
        let kind = match &prop.source {
            Source::Model(path) => Kind::Model { path: path.clone(), scale: Vec3::ONE, bounds, solid: bounds.is_some() },
            Source::Me(package, mesh) => Kind::MeProp { package: package.to_string(), mesh: mesh.to_string(), scale: Vec3::ONE, bounds, solid: bounds.is_some(), own_shape: true },
        };
        let mut o = Object::new(kind, p);
        o.face(Vec3::new(-look.x, 0.0, -look.z));
        o.yaw = (o.yaw / 90.0).round() * 90.0;
        o.pitch = 0.0;
        if bounds.is_some() {
            // Out of the surface by as much as it goes into it.
            let b = o.bounds();
            let sink = (0..8)
                .map(|k| Vec3::new(if k & 1 == 0 { b.min.x } else { b.max.x }, if k & 2 == 0 { b.min.y } else { b.max.y }, if k & 4 == 0 { b.min.z } else { b.max.z }))
                .map(|c| (c - p).dot(n))
                .fold(f32::INFINITY, f32::min);
            o.at -= n * sink;
        }
        o
    }
}

/// Start reading the list the first time the editor's open; take it when it's read.
pub fn load(mut lib: ResMut<Library>, ed: Res<super::Editor>, arms: Option<Res<crate::me_viewmodel::MeArms>>) {
    if lib.loaded || !ed.active {
        return;
    }
    if let Some(rx) = &lib.loading {
        let got = rx.lock().ok().and_then(|r| r.try_recv().ok());
        if let Some(props) = got {
            let mut folders: [Vec<String>; 3] = Default::default();
            for p in &props {
                folders[p.game].push(p.folder.clone());
            }
            for f in &mut folders {
                f.sort_by_key(|s| s.to_lowercase());
                f.dedup();
            }
            info!("Props for the editor: {} ({} Portal, {} Portal 2, {} Mirror's Edge)", props.len(), props.iter().filter(|p| p.game == 0).count(), props.iter().filter(|p| p.game == 1).count(), props.iter().filter(|p| p.game == 2).count());
            lib.by_path = props.iter().enumerate().filter_map(|(i, p)| if let Source::Model(path) = &p.source { Some((path.clone(), i)) } else { None }).collect();
            lib.props = props;
            lib.folders = folders;
            lib.loaded = true;
            lib.loading = None;
        }
        return;
    }
    let extracted = crate::extracted::find_root();
    let cooked = arms.map(|a| a.cooked_pc().to_path_buf());
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut props = extracted.map(|r| models(&r)).unwrap_or_default();
        if let Some(c) = cooked {
            props.extend(me_meshes(&c));
        }
        let _ = tx.send(props);
    });
    lib.loading = Some(Mutex::new(rx));
}

/// The extracted models, from `props/index.tsv`.
fn models(root: &Path) -> Vec<Prop> {
    let Ok(text) = std::fs::read_to_string(root.join("props/index.tsv")) else {
        warn!("No props/index.tsv: run the extractor (cargo run --release -p portal_extract -- --only-props) for Portal's props");
        return Vec::new();
    };
    let v = |s: &str| -> Option<Vec3> {
        let n: Vec<f32> = s.split(' ').filter_map(|x| x.parse().ok()).collect();
        (n.len() == 3).then(|| Vec3::new(n[0], n[1], n[2]))
    };
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let (game, rel, lo, hi) = (*f.first()?, *f.get(1)?, *f.get(2)?, *f.get(3)?);
            // (`textured total`, from extractors that write it.)
            let looks = f.get(4).and_then(|l| l.split_once(' ')).and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)));
            let game = match game {
                "portal" => 0,
                "portal2" => 1,
                _ => return None,
            };
            let name = rel.rsplit('/').next().unwrap_or(rel).to_string();
            let folder = rel.split('/').next().unwrap_or("").to_string();
            Some(Prop { name, game, folder, source: Source::Model(format!("props/{}/{rel}.glb", ["portal", "portal2"][game])), bounds: v(lo).zip(v(hi)), looks })
        })
        .collect()
}

/// Mirror's Edge's static meshes in its prop packages.
fn me_meshes(cooked: &Path) -> Vec<Prop> {
    let mut out = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for folder in ME_FOLDERS {
        let mut stack = vec![cooked.join(folder)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("upk")) {
                    files.push(p);
                }
            }
        }
    }
    files.sort();
    for f in files {
        let Some(package) = f.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
        let Ok(pkg) = me_assets::package::Package::open(&f) else { continue };
        // (Leaked: the editor's level keeps them, as `'static` names, like a level's own.)
        let package: &'static str = Box::leak(package.into_boxed_str());
        let mut names: Vec<&str> = pkg.exports.iter().filter(|e| pkg.class_name(e) == "StaticMesh").map(|e| e.name.as_str()).collect();
        names.sort();
        names.dedup();
        for name in names {
            let mesh: &'static str = Box::leak(name.to_string().into_boxed_str());
            out.push(Prop { name: mesh.to_string(), game: 2, folder: package.to_string(), source: Source::Me(package, mesh), bounds: None, looks: None });
        }
    }
    out
}
