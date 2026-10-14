//! A map's actors with runner-vision (LOI) settings of their own: `loi_actors <map package>`.
//! Each one's class, name and LOI properties (the rest are the class defaults: seen from 15 m,
//! on for at least 1.5 s, not by being looked at).
use me_assets::package::Package;
use me_assets::props::Props;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pkg = Package::open(std::path::Path::new(&a[1])).expect("open");
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for e in &pkg.exports {
        let class = pkg.class_name(e);
        if !class.ends_with("Actor") {
            continue;
        }
        let Ok(props) = Props::read(&pkg, e) else { continue };
        let lois: Vec<String> = props
            .tags
            .iter()
            .filter(|t| t.name.starts_with("LOI") || t.name == "TdLOIAddOn")
            .map(|t| {
                let raw = &pkg.data[t.at..t.at + t.size.min(12)];
                let v = if t.kind == "FloatProperty" { format!("{}", f32::from_le_bytes(raw[..4].try_into().unwrap())) } else { format!("{raw:?}") };
                format!("{}={v}", t.name)
            })
            .collect();
        if !lois.is_empty() {
            for l in &lois {
                *counts.entry(l.split('=').next().unwrap().to_string()).or_default() += 1;
            }
            println!("{class} {}: {}", e.name, lois.join(", "));
        }
    }
    println!("{counts:?}");
}
