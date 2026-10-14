//! A static mesh's points' extent (Unreal units, its own axes), and each section's, its
//! triangles' area by which way they face: `mesh_extent <CookedPC> <package> <mesh>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut pkgs = me_assets::Packages::new(std::path::Path::new(&a[1]));
    let pkg = pkgs.get(&a[2]).expect("package");
    let m = me_assets::staticmesh::StaticMesh::read(&pkg, &a[3]).expect("mesh");
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &m.positions {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    println!("{}: {lo:?} .. {hi:?} (bounds origin {:?} extent {:?})", a[3], m.origin, m.extent);
    for (k, s) in m.sections.iter().enumerate() {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let mut faces: std::collections::BTreeMap<String, f32> = Default::default();
        for t in m.indices[s.first_index as usize..(s.first_index + 3 * s.triangles) as usize].chunks_exact(3) {
            let p = [0, 1, 2].map(|k| m.positions[t[k] as usize]);
            for q in &p {
                for c in 0..3 {
                    lo[c] = lo[c].min(q[c]);
                    hi[c] = hi[c].max(q[c]);
                }
            }
            let (u, v) = ([p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]], [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]]);
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if l > 0.0 {
                let key = format!("({:.1} {:.1} {:.1})", n[0] / l, n[1] / l, n[2] / l).replace("-0.0", "0.0");
                *faces.entry(key).or_default() += l * 0.5 / 1e4;
            }
        }
        println!("  section {k} ({} tris, material {}): {lo:?} .. {hi:?}", s.triangles, pkg.object_name(s.material));
        for (n, area) in faces {
            println!("    facing {n}: {area:.2} m2");
        }
    }
}
