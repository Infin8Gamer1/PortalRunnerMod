//! Prints static meshes' sizes in metres (our axes: x right, y up, z back): `mesh_sizes <pkg> <name>...`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pkg = me_assets::package::Package::open(std::path::Path::new(&a[1])).expect("open");
    for n in &a[2..] {
        match me_assets::StaticMesh::read(&pkg, n) {
            Ok(m) => {
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for p in &m.positions {
                    let q = [p[1] * 0.01, p[2] * 0.01, -p[0] * 0.01];
                    for k in 0..3 {
                        lo[k] = lo[k].min(q[k]);
                        hi[k] = hi[k].max(q[k]);
                    }
                }
                println!("{n}: x {:.2}..{:.2}  y {:.2}..{:.2}  z {:.2}..{:.2}  ({} tris, {} materials)", lo[0], hi[0], lo[1], hi[1], lo[2], hi[2], m.indices.len() / 3, m.sections.len());
            }
            Err(e) => println!("{n}: {e}"),
        }
    }
}
