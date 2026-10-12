//! How many of Mirror's Edge's prop meshes' materials find a diffuse texture, and the ones that
//! don't: `cargo run --release -p me_assets --example prop_textures -- <CookedPC>`.
use std::path::{Path, PathBuf};

fn main() {
    let cooked = PathBuf::from(std::env::args().nth(1).expect("CookedPC folder"));
    let mut pkgs = me_assets::Packages::new(&cooked);
    let mut files = Vec::new();
    for folder in ["Props", "Interiors", "Vehicles", "Accessories"] {
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
    let (mut meshes, mut failed, mut slots, mut textured, mut colored, mut none) = (0, 0, 0, 0, 0, 0);
    let mut missing: Vec<String> = Vec::new();
    for f in files {
        let package = f.file_stem().unwrap().to_string_lossy().into_owned();
        let Ok(pkg) = me_assets::package::Package::open(Path::new(&f)) else { continue };
        let names: Vec<String> = pkg.exports.iter().filter(|e| pkg.class_name(e) == "StaticMesh").map(|e| e.name.clone()).collect();
        for name in names {
            meshes += 1;
            match me_assets::load_static_mesh(&mut pkgs, &package, &name, 16) {
                Ok((_, s)) => {
                    for slot in s {
                        slots += 1;
                        if slot.diffuse.is_some() {
                            textured += 1;
                        } else if slot.color.is_some() {
                            colored += 1;
                        } else if slot.material == "None" {
                            none += 1;
                        } else {
                            missing.push(format!("{package}.{name}: {}", slot.material));
                        }
                    }
                }
                Err(e) => {
                    failed += 1;
                    missing.push(format!("{package}.{name}: unreadable ({e})"));
                }
            }
        }
    }
    println!("{meshes} meshes ({failed} unreadable), {slots} materials: {textured} with a diffuse texture, {colored} a plain colour, {none} none at all (the level gives them one), {} neither", slots - textured - colored - none);
    for m in missing.iter().take(12) {
        println!("  {m}");
    }
    // What a few of the materials without a diffuse map are made of.
    let want: Vec<(String, String)> = std::env::args().skip(2).filter_map(|a| a.split_once('.').map(|(p, m)| (p.to_string(), m.to_string()))).collect();
    for (package, mesh) in want {
        let pkg = pkgs.get(&package).unwrap();
        let sm = me_assets::staticmesh::StaticMesh::read(&pkg, &mesh).unwrap();
        for s in &sm.sections {
            let mut next = Some((pkg.clone(), s.material));
            for depth in 0..4 {
                let Some((p, obj)) = next.take() else { break };
                let Some((mp, mi)) = pkgs.resolve(&p, obj) else { println!("  {mesh}: material {} unresolved", p.object_name(obj)); break };
                let e = &mp.exports[mi];
                println!("  {}{} {} in {}: params {:?}", "  ".repeat(depth), mp.class_name(e), e.name, mp.exports.first().map(|_| "").unwrap_or(""), me_assets::material_textures(&mp, mi).iter().map(|(n, t)| format!("{n}={}", mp.object_name(*t))).collect::<Vec<_>>());
                let idx = mi as i32 + 1;
                let kids: Vec<String> = mp.exports.iter().filter(|x| x.outer == idx).map(|x| format!("{}:{}", mp.class_name(x), x.name)).collect();
                if !kids.is_empty() {
                    println!("  {}  inside: {:?}", "  ".repeat(depth), kids);
                }
                for x in mp.exports.iter().filter(|x| x.outer == idx && mp.class_name(x).contains("TextureSample")) {
                    if let Ok(xp) = me_assets::props::Props::read(&mp, x) {
                        let t = xp.object(&mp, "Texture");
                        println!("  {}  {} texture {:?} -> {:?}", "  ".repeat(depth), x.name, t.map(|t| mp.object_name(t).to_string()), t.and_then(|t| pkgs.resolve(&mp, t)).map(|(tp, ti)| format!("{} {}", tp.class_name(&tp.exports[ti]), tp.exports[ti].name)));
                    }
                }
                if let Ok(props) = me_assets::props::Props::read(&mp, e) {
                    for t in props.object_array(&mp, "ReferencedTextures") {
                        println!("  {}  referenced {} -> {:?}", "  ".repeat(depth), mp.object_name(t), pkgs.resolve(&mp, t).map(|(tp, ti)| format!("{} {}", tp.class_name(&tp.exports[ti]), tp.exports[ti].name)));
                    }
                    println!("  {}  props: {:?}", "  ".repeat(depth), props.tags.iter().map(|t| t.name.clone()).collect::<Vec<_>>());
                    next = props.object(&mp, "Parent").map(|par| (mp.clone(), par));
                }
            }
        }
    }
}
