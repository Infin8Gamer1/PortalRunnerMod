//! Where a Mirror's Edge level's drawn faces lie on each other (z-fighting): pairs of props
//! whose flat faces are in the same plane, facing the same way, overlapping. `me_zfight <level
//! file> [x0,y0,z0,x1,y1,z1]` (our axes, metres: only there).
use aperture::doc::{Kind, LevelDoc};
use glam::{Vec2, Vec3};
use std::collections::HashMap;

fn ue(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[1] * 0.01, p[2] * 0.01, -p[0] * 0.01)
}

/// A convex polygon clipped by another's edge (both counter-clockwise).
fn clip(poly: &[Vec2], a: Vec2, b: Vec2) -> Vec<Vec2> {
    let inside = |p: Vec2| (b - a).perp_dot(p - a) >= 0.0;
    let mut out = vec![];
    for k in 0..poly.len() {
        let (p, q) = (poly[k], poly[(k + 1) % poly.len()]);
        match (inside(p), inside(q)) {
            (true, true) => out.push(q),
            (true, false) | (false, true) => {
                let d = (b - a).perp_dot(q - p);
                let t = if d.abs() > 1e-12 { (b - a).perp_dot(a - p) / d } else { 0.0 };
                let x = p + (q - p) * t;
                out.push(x);
                if !inside(p) {
                    out.push(q);
                }
            }
            _ => {}
        }
    }
    out
}

fn area(p: &[Vec2]) -> f32 {
    (0..p.len()).map(|k| p[k].perp_dot(p[(k + 1) % p.len()])).sum::<f32>() * 0.5
}

fn ccw(t: [Vec2; 3]) -> Vec<Vec2> {
    if area(&t) < 0.0 { vec![t[0], t[2], t[1]] } else { t.to_vec() }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = LevelDoc::from_ron(&std::fs::read_to_string(&a[1]).expect("level")).expect("doc");
    let region: Option<(Vec3, Vec3)> = a.get(2).map(|r| {
        let v: Vec<f32> = r.split(',').map(|x| x.parse().unwrap()).collect();
        (Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], v[5]))
    });
    let cooked = aperture::me::find_install().expect("Mirror's Edge");
    let mut pkgs = me_assets::Packages::new(&cooked);
    // Each flat face: its prop, its section's material, its height, which way up, its corners.
    struct Face {
        prop: usize,
        mat: String,
        y: f32,
        up: bool,
        tri: [Vec2; 3],
    }
    let mut faces: Vec<Face> = vec![];
    let mut meshes: HashMap<(String, String), Option<(me_assets::StaticMesh, Vec<String>)>> = HashMap::new();
    let mut names = vec![];
    for o in &doc.objects {
        let Kind::MeProp { package, mesh, scale, hidden: false, .. } = &o.kind else { continue };
        if let Some((lo, hi)) = region {
            let b = o.bounds();
            if b.max.cmplt(lo).any() || b.min.cmpgt(hi).any() {
                continue;
            }
        }
        let m = meshes.entry((package.clone(), mesh.clone())).or_insert_with(|| {
            let p = pkgs.get(package).ok()?;
            let m = me_assets::StaticMesh::read(&p, mesh).ok()?;
            let mats = m.sections.iter().map(|s| p.object_name(s.material).to_string()).collect();
            Some((m, mats))
        });
        let Some((m, mats)) = m else { continue };
        names.push(format!("{} {} at {}", o.name, mesh, o.at));
        let id = names.len() - 1;
        let turn = o.rotation();
        for (s, mat) in m.sections.iter().zip(mats.iter()) {
            for t in m.indices[s.first_index as usize..(s.first_index + 3 * s.triangles) as usize].chunks_exact(3) {
                let p = [0, 1, 2].map(|i| o.at + turn * (*scale * ue(m.positions[t[i] as usize])));
                let n = (p[1] - p[0]).cross(p[2] - p[0]);
                if n.length_squared() < 1e-8 {
                    continue;
                }
                let n = n.normalize();
                if n.y.abs() < 0.995 {
                    continue;
                }
                faces.push(Face { prop: id, mat: mat.clone(), y: (p[0].y + p[1].y + p[2].y) / 3.0, up: n.y > 0.0, tri: p.map(|q| Vec2::new(q.x, q.z)) });
            }
        }
    }
    // By height (bins of the gap looked for, and the next), then by place.
    const GAP: f32 = 0.03;
    let mut bins: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        bins.entry((f.y / GAP).floor() as i64).or_default().push(i);
    }
    let mut pairs: HashMap<(usize, usize, String, String), f32> = HashMap::new();
    for (b, list) in &bins {
        let mut near: Vec<usize> = list.clone();
        near.extend(bins.get(&(b + 1)).into_iter().flatten());
        for (x, &i) in list.iter().enumerate() {
            for &j in &near[x + 1..] {
                let (f, g) = (&faces[i], &faces[j]);
                if f.up != g.up || (f.y - g.y).abs() > GAP {
                    continue;
                }
                let (fmin, fmax) = (f.tri.iter().fold(Vec2::MAX, |m, p| m.min(*p)), f.tri.iter().fold(Vec2::MIN, |m, p| m.max(*p)));
                let (gmin, gmax) = (g.tri.iter().fold(Vec2::MAX, |m, p| m.min(*p)), g.tri.iter().fold(Vec2::MIN, |m, p| m.max(*p)));
                if fmax.cmplt(gmin).any() || gmax.cmplt(fmin).any() {
                    continue;
                }
                let mut poly = ccw(f.tri);
                let gt = ccw(g.tri);
                for k in 0..3 {
                    poly = clip(&poly, gt[k], gt[(k + 1) % 3]);
                    if poly.len() < 3 {
                        break;
                    }
                }
                if poly.len() >= 3 {
                    let ar = area(&poly).abs();
                    let key = if f.prop < g.prop { (f.prop, g.prop, f.mat.clone(), g.mat.clone()) } else { (g.prop, f.prop, g.mat.clone(), f.mat.clone()) };
                    *pairs.entry(key).or_default() += ar;
                }
            }
        }
    }
    let mut list: Vec<_> = pairs.into_iter().filter(|(_, ar)| *ar > 0.05).collect();
    list.sort_by(|a, b| b.1.total_cmp(&a.1));
    for ((p, q, m, n), ar) in list.iter().take(60) {
        println!("{ar:7.2} m2  {} ({m})  /  {} ({n})", names[*p], names[*q]);
    }
}
