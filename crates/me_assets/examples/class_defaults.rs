//! A script package's objects whose path matches, with their paths, and the properties of each
//! class default object (`Default__<class>`) that matches: `class_defaults <package.u> <substring>`.
use me_assets::package::Package;
use me_assets::props::Props;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pkg = Package::open(std::path::Path::new(&a[1])).expect("open");
    let want = a[2].to_ascii_lowercase();
    for (i, e) in pkg.exports.iter().enumerate() {
        let path = pkg.path(i as i32 + 1);
        if !path.to_ascii_lowercase().contains(&want) {
            continue;
        }
        println!("{} {} ({} bytes)", pkg.class_name(e), path, e.size);
        if e.name.starts_with("Default__") {
            if let Ok(props) = Props::read(&pkg, e) {
                for t in &props.tags {
                    let raw = &pkg.data[t.at..t.at + t.size.min(16)];
                    let f = (t.size == 4).then(|| f32::from_le_bytes(raw[..4].try_into().unwrap()));
                    println!("    {} {} {:?} f32 {:?}", t.name, t.kind, raw, f);
                }
            }
        }
    }
}
