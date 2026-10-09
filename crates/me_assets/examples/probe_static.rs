//! Reads every StaticMesh in the packages given and says how it went.
fn main() {
    for path in std::env::args().skip(1) {
        let pkg = me_assets::package::Package::open(std::path::Path::new(&path)).expect("open");
        let names = me_assets::StaticMesh::names(&pkg);
        let mut ok = 0;
        for n in &names {
            match me_assets::StaticMesh::read(&pkg, n) {
                Ok(m) => {
                    ok += 1;
                    if ok <= 6 {
                        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                        for p in &m.positions {
                            for a in 0..3 {
                                lo[a] = lo[a].min(p[a]);
                                hi[a] = hi[a].max(p[a]);
                            }
                        }
                        println!("  {n}: {} verts, {} tris, {} sections, bounds {:?}..{:?} (ext {:?}), uv0 {:?}", m.positions.len(), m.indices.len() / 3, m.sections.len(), lo, hi, m.extent, m.uvs.first());
                    }
                }
                Err(e) => println!("  {n}: {e}"),
            }
        }
        println!("{path}: {ok} of {} read", names.len());
    }
}
