//! Faith's materials, first person (arms and legs) and third person, as they resolve:
//! `faith_materials <install> [max texture size]`. Prints each slot's path, blend, texture sizes
//! and alpha, tint, colour parameters and glow, and which way up its normal map's green is.
//! `--textures`: each material's texture parameters and textures, where they're from and the
//! biggest that reads. `--dump <dir> [texture]...`: third-person textures by name as raw RGBA,
//! and each third-person section's UVs and bounds.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let install = std::path::Path::new(&a[1]);
    let max: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1024);
    let first = me_assets::FaithArms::load(install, max).expect("load");
    let third = me_assets::FaithArms::load_body3p(install, max).expect("load 3p");
    let mut sets = vec![("arms", &first.mesh, &first.materials)];
    if let Some((l, m)) = &first.legs {
        sets.push(("legs", l, m));
    }
    sets.push(("body3p", &third.mesh, &third.materials));
    // Textures by name from the third-person package, as raw RGBA (`--dump <out dir> <name>...`),
    // and each section's UVs (u v per corner, a triangle a line).
    if let Some(k) = a.iter().position(|x| x == "--dump") {
        let out = std::path::Path::new(&a[k + 1]);
        let cooked = me_assets::cooked_pc(install).expect("CookedPC");
        let pkg = me_assets::package::Package::open(&cooked.join("Characters/CH_TKY_Crim_Fixer.upk")).expect("open");
        let mut pkgs = me_assets::Packages::new(&cooked);
        for name in &a[k + 2..] {
            let Some(e) = pkg.exports.iter().find(|e| e.name == *name) else { continue };
            let t = me_assets::texture::read_texture_from(&pkg, e, 1024, &mut |n| pkgs.raw(n)).expect("read");
            std::fs::write(out.join(format!("{name}_{}x{}.rgba", t.width, t.height)), &t.pixels).unwrap();
        }
        for (k, sec) in third.mesh.sections.iter().enumerate() {
            let mut lines = String::new();
            for t in me_assets::pose::section_indices(&third.mesh, k).chunks_exact(3) {
                for &i in t {
                    let uv = third.mesh.vertices[i as usize].uv;
                    lines += &format!("{} {} ", uv[0], uv[1]);
                }
                lines += "\n";
            }
            std::fs::write(out.join(format!("section{k}_m{}.uv", sec.material)), lines).unwrap();
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for &i in &me_assets::pose::section_indices(&third.mesh, k) {
                let p = third.mesh.vertices[i as usize].position;
                for c in 0..3 {
                    lo[c] = lo[c].min(p[c]);
                    hi[c] = hi[c].max(p[c]);
                }
            }
            println!("section {k} (material {}): {lo:.0?}..{hi:.0?}", sec.material);
        }
        return;
    }
    // Each material's textures: where they're from, and the biggest that reads.
    if a.iter().any(|x| x == "--textures") {
        let cooked = me_assets::cooked_pc(install).expect("CookedPC");
        let mut pkgs = me_assets::Packages::new(&cooked);
        for (file, mesh) in [("CH_TKY_Crim_Fixer_1P", first.mesh.name.as_str()), ("CH_TKY_Crim_Fixer", third.mesh.name.as_str())] {
            let Ok(pkg) = pkgs.get(file) else { continue };
            let mesh = if mesh == first.mesh.name { &first.mesh } else { &third.mesh };
            for &mat in &mesh.materials {
                println!("-- {}", pkg.object_name(mat));
                if let Some((mp, mi)) = pkgs.resolve(&pkg, mat) {
                    for (pname, tex) in me_assets::material_textures(&mp, mi) {
                        let to = pkgs.resolve(&mp, tex).map(|(tp, ti)| tp.path(ti as i32 + 1));
                        println!("     param {pname:?} = {} ({}) -> {to:?}", mp.object_name(tex), mp.path(tex));
                    }
                }
                for (name, tp, ti) in me_assets::all_textures(&mut pkgs, &pkg, mat) {
                    let home = tp.path(ti as i32 + 1);
                    let big = me_assets::texture::read_texture_from(&tp, &tp.exports[ti], 4096, &mut |n| pkgs.raw(n)).map(|t| format!("{}x{}", t.width, t.height));
                    let inline = me_assets::texture::read_texture_export(&tp, &tp.exports[ti], 4096).map(|t| format!("{}x{}", t.width, t.height));
                    let first = home.split('.').next().unwrap_or("");
                    println!("     {name} ({home}; its file {first} {}): read {big:?}, inline {inline:?}", if pkgs.has(first) { "found" } else { "missing" });
                }
            }
        }
        return;
    }
    for (what, mesh, slots) in sets {
        println!("== {what}: {} ({} sections, {} slots)", mesh.name, mesh.sections.len(), slots.len());
        for (k, s) in slots.iter().enumerate() {
            let size = |t: &Option<me_assets::Rgba>| match t {
                Some(t) => {
                    let clear = t.pixels.chunks_exact(4).filter(|c| c[3] < 250).count() as f32 / (t.pixels.len() / 4).max(1) as f32;
                    format!("{}x{} (alpha<250 {:.0}%)", t.width, t.height, clear * 100.0)
                }
                None => "-".into(),
            };
            println!("  [{k}] {} ({})", s.path, s.material);
            println!("      blend {:?} two_sided {} uv_scale {:?}", s.blend, s.two_sided, s.uv_scale);
            println!("      diffuse {}  normal {}  specular {}", size(&s.diffuse), size(&s.normal), size(&s.specular));
            println!("      tint {:?} where {:?} region {} color {:?} emissive {} glow {:?}", s.tint, s.tint_where, s.region.is_some(), s.color, s.emissive, s.glow.as_ref().map(|g| g.1));
            if !s.vectors.is_empty() {
                println!("      vectors {:?}", s.vectors);
            }
            if let Some(sp) = &s.specular {
                let n = (sp.pixels.len() / 4).max(1) as f32;
                let mean = sp.pixels.chunks_exact(4).map(|c| c[0] as f32 * 0.299 + c[1] as f32 * 0.587 + c[2] as f32 * 0.114).sum::<f32>() / n;
                println!("      specular mean {mean:.0}/255");
            }
            if let Some(nm) = &s.normal {
                let n = (nm.pixels.len() / 4).max(1) as f32;
                let g = nm.pixels.chunks_exact(4).map(|c| c[1] as f32).sum::<f32>() / n;
                let b = nm.pixels.chunks_exact(4).map(|c| c[2] as f32).sum::<f32>() / n;
                println!("      normal mean G {g:.0} B {b:.0}; {}; at a quarter: {}", curl(nm), curl(&quarter(nm)));
            }
        }
    }
}

/// Which way up a normal map's green is, by which reading of it is a height's slopes: a height's
/// slopes have no curl. With green down the image (DirectX), the slopes along and down are
/// (-x/z, -y/z); with green up (OpenGL), (-x/z, y/z).
fn curl(t: &me_assets::Rgba) -> String {
    let (w, h) = (t.width as usize, t.height as usize);
    let slope = |x: usize, y: usize| {
        let c = &t.pixels[(y * w + x) * 4..];
        let n = [c[0] as f32 / 127.5 - 1.0, c[1] as f32 / 127.5 - 1.0, (c[2] as f32 / 127.5 - 1.0).max(0.2)];
        (-n[0] / n[2], -n[1] / n[2])
    };
    let (mut down, mut up) = (0.0f64, 0.0f64);
    for y in 0..h - 1 {
        for x in 0..w - 1 {
            let (p, q) = slope(x, y);
            let (_, qx) = slope(x + 1, y);
            let (py, _) = slope(x, y + 1);
            let (dp, dq) = ((py - p) as f64, (qx - q) as f64);
            down += (dp - dq) * (dp - dq);
            up += (dp + dq) * (dp + dq);
        }
    }
    format!("curl green-down {:.3} green-up {:.3} -> {}", down.sqrt(), up.sqrt(), if down < up { "DirectX" } else { "OpenGL" })
}

/// A map a quarter the size (box-filtered): its broad shapes, not its pores.
fn quarter(t: &me_assets::Rgba) -> me_assets::Rgba {
    let (w, h) = ((t.width / 4).max(1), (t.height / 4).max(1));
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let mut sum = 0u32;
                for dy in 0..4 {
                    for dx in 0..4 {
                        let (sx, sy) = ((x * 4 + dx).min(t.width - 1), (y * 4 + dy).min(t.height - 1));
                        sum += t.pixels[((sy * t.width + sx) * 4 + c) as usize] as u32;
                    }
                }
                pixels[((y * w + x) * 4 + c) as usize] = (sum / 16) as u8;
            }
        }
    }
    me_assets::Rgba { width: w, height: h, pixels }
}
