//! What a map's placed meshes and other actors are, by class: `map_classes <map package>...`.
fn main() {
    let mut meshes = std::collections::BTreeMap::<String, usize>::new();
    let mut others = std::collections::BTreeMap::<String, usize>::new();
    for f in std::env::args().skip(1) {
        let Ok(pkg) = me_assets::package::Package::open(std::path::Path::new(&f)) else { continue };
        let m = me_assets::map::read(&pkg);
        for p in &m.meshes {
            *meshes.entry(p.class.clone()).or_default() += 1;
        }
        for o in &m.others {
            *others.entry(o.class.clone()).or_default() += 1;
        }
    }
    println!("meshes: {meshes:?}");
    println!("others: {others:?}");
}
