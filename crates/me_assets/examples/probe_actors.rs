//! The static mesh actors in a package (a prefab's pieces): each one's mesh, place, turn and
//! scale. `probe_actors <package.upk> [name filter]`.
use me_assets::package::Package;
use me_assets::props::Props;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let pkg = Package::open(std::path::Path::new(&args[1])).expect("open");
    let filter = args.get(2).map(|s| s.to_ascii_lowercase());
    for e in pkg.exports.iter().filter(|e| pkg.class_name(e) == "StaticMeshActor") {
        if filter.as_ref().is_some_and(|f| !e.name.to_ascii_lowercase().contains(f)) {
            continue;
        }
        let Ok(p) = Props::read_object(&pkg, e) else { continue };
        let comp = p.object(&pkg, "StaticMeshComponent");
        let mesh = comp.and_then(|c| usize::try_from(c - 1).ok()).and_then(|c| pkg.exports.get(c)).and_then(|ce| Props::read_object(&pkg, ce).ok()).and_then(|cp| cp.object(&pkg, "StaticMesh")).map(|m| pkg.path(m));
        println!("{}\tmesh {:?}\tat {:?}\trot {:?}\tscale {:?} {:?}", e.name, mesh, p.vec3(&pkg, "Location"), p.rotator(&pkg, "Rotation"), p.f32(&pkg, "DrawScale"), p.vec3(&pkg, "DrawScale3D"));
    }
}
