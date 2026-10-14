//! Which way a mesh's tangent basis runs over its texture: `tangent_sense <install> [package mesh]...`.
//! For each triangle, the direction its surface goes as V grows (down the image) against its
//! vertices' TangentY (the normal map's green axis in the game's shader): mostly along means the
//! game reads green as down the image (DirectX's way), mostly against means up (OpenGL's).
//! Faith's arms, legs and third-person body, then any static meshes named.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let install = std::path::Path::new(&a[1]);
    let first = me_assets::FaithArms::load(install, 64).expect("load");
    let third = me_assets::FaithArms::load_body3p(install, 64).expect("load 3p");
    let mut meshes = vec![&first.mesh, &third.mesh];
    if let Some((l, _)) = &first.legs {
        meshes.push(l);
    }
    for m in meshes {
        let get = |i: u32| {
            let v = &m.vertices[i as usize];
            (v.position, v.uv, v.bitangent, v.normal, v.tangent)
        };
        let (along, against, x_along) = sense(m.indices.chunks_exact(3).map(|t| [get(t[0]), get(t[1]), get(t[2])]));
        println!("{}: TangentY along +V {along:.0}%, against {against:.0}%; TangentX along +U {x_along:.0}%", m.name);
    }
    let cooked = me_assets::cooked_pc(install).expect("CookedPC");
    let mut pkgs = me_assets::Packages::new(&cooked);
    for pair in a[2..].chunks_exact(2) {
        let (mesh, _) = me_assets::load_static_mesh(&mut pkgs, &pair[0], &pair[1], 64).expect("static mesh");
        let get = |i: u32| {
            let i = i as usize;
            let t = mesh.tangents[i];
            let (x, z) = ([t[0], t[1], t[2]], mesh.normals[i]);
            // TangentY = (TangentZ ^ TangentX) * W, as Unreal's vertex factory rebuilds it.
            let y = cross(z, x).map(|c| c * t[3]);
            (mesh.positions[i], mesh.uvs[i], y, z, x)
        };
        let (along, against, x_along) = sense(mesh.indices.chunks_exact(3).map(|t| [get(t[0]), get(t[1]), get(t[2])]));
        println!("{}.{}: TangentY along +V {along:.0}%, against {against:.0}%; TangentX along +U {x_along:.0}%", pair[0], pair[1]);
        // As the game draws it here (`me_props`: our axes, the sign turned round), Bevy's
        // bitangent, w × (normal ^ tangent).
        let ue = |p: [f32; 3]| [p[1], p[2], -p[0]];
        let drawn = |i: u32| {
            let i = i as usize;
            let t = mesh.tangents[i];
            let (x, z) = (ue([t[0], t[1], t[2]]), ue(mesh.normals[i]));
            let b = cross(z, x).map(|c| c * -t[3]);
            (ue(mesh.positions[i]), mesh.uvs[i], b, z, x)
        };
        let (along, against, _) = sense(mesh.indices.chunks_exact(3).map(|t| [drawn(t[0]), drawn(t[1]), drawn(t[2])]));
        println!("    as drawn, Bevy's bitangent along +V {along:.0}%, against {against:.0}%");
    }
}

type Corner = ([f32; 3], [f32; 2], [f32; 3], [f32; 3], [f32; 3]);

/// Per cent of triangles (by count) whose TangentY runs along and against +V, and whose
/// TangentX runs along +U.
fn sense(tris: impl Iterator<Item = [Corner; 3]>) -> (f32, f32, f32) {
    let (mut along, mut against, mut x_along, mut n) = (0, 0, 0, 0);
    for [a, b, c] in tris {
        let (e1, e2) = (sub(b.0, a.0), sub(c.0, a.0));
        let (d1, d2) = ([b.1[0] - a.1[0], b.1[1] - a.1[1]], [c.1[0] - a.1[0], c.1[1] - a.1[1]]);
        let det = d1[0] * d2[1] - d1[1] * d2[0];
        if det.abs() < 1e-12 {
            continue;
        }
        // dP/dU and dP/dV over the triangle.
        let dpdu = [(e1[0] * d2[1] - e2[0] * d1[1]) / det, (e1[1] * d2[1] - e2[1] * d1[1]) / det, (e1[2] * d2[1] - e2[2] * d1[1]) / det];
        let dpdv = [(e2[0] * d1[0] - e1[0] * d2[0]) / det, (e2[1] * d1[0] - e1[1] * d2[0]) / det, (e2[2] * d1[0] - e1[2] * d2[0]) / det];
        let y = [a.2[0] + b.2[0] + c.2[0], a.2[1] + b.2[1] + c.2[1], a.2[2] + b.2[2] + c.2[2]];
        let x = [a.4[0] + b.4[0] + c.4[0], a.4[1] + b.4[1] + c.4[1], a.4[2] + b.4[2] + c.4[2]];
        n += 1;
        if dot(y, dpdv) > 0.0 { along += 1 } else { against += 1 }
        if dot(x, dpdu) > 0.0 {
            x_along += 1;
        }
    }
    let n = n.max(1) as f32;
    (along as f32 * 100.0 / n, against as f32 * 100.0 / n, x_along as f32 * 100.0 / n)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
