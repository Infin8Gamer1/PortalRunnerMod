//! What in a package uses a physical material (a material's `PhysMaterial`, a mesh's or a
//! component's override): `phys_users <package.upk> <physical material name>`.
use me_assets::package::Package;
use me_assets::props::Props;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pkg = Package::open(std::path::Path::new(&a[1])).expect("open");
    for (i, e) in pkg.exports.iter().enumerate() {
        let Ok(p) = Props::read_object(&pkg, e).or_else(|_| Props::read(&pkg, e)) else { continue };
        for t in &p.tags {
            if t.kind == "ObjectProperty" && t.name.contains("PhysMaterial") {
                if let Some(o) = p.object(&pkg, &t.name).filter(|&o| o != 0) {
                    if pkg.object_name(o).eq_ignore_ascii_case(&a[2]) {
                        println!("{} ({}) {}", pkg.path(i as i32 + 1), pkg.class_name(e), t.name);
                    }
                }
            }
        }
    }
}
