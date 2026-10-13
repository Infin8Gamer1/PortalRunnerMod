//! A static mesh's points' extent (Unreal units, its own axes): `mesh_extent <CookedPC> <package> <mesh>`.
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
}
