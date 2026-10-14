//! How a static mesh collides: its simple-collision flags, its body's shapes, its triangles:
//! `mesh_collision <package.upk> <mesh>...`.
use me_assets::map::{self, Chain};
use me_assets::package::Package;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pkg = Package::open(std::path::Path::new(&a[1])).expect("open");
    for name in &a[2..] {
        let Some(i) = pkg.exports.iter().position(|e| e.name == *name && pkg.class_name(e) == "StaticMesh") else {
            println!("{name}: not here");
            continue;
        };
        let m = Chain::new(&pkg, i);
        let body = m.object("BodySetup").filter(|&b| b > 0);
        let shapes = body.map_or(0, |b| map::body_shapes(&pkg, b as usize - 1).len());
        let tris = me_assets::staticmesh::StaticMesh::collision_tris(&pkg, &pkg.exports[i]).map_or(0, |t| t.len());
        println!(
            "{name}: simple box {:?} line {:?} rigid {:?}, body {}, {shapes} shapes, {tris} triangles",
            m.bool("UseSimpleBoxCollision"),
            m.bool("UseSimpleLineCollision"),
            m.bool("UseSimpleRigidBodyCollision"),
            body.is_some()
        );
    }
}
