//! A map's actors of a class, with their properties: `actor_props <map package> <class> [max]`.
use me_assets::package::Package;
use me_assets::props::Props;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pkg = Package::open(std::path::Path::new(&a[1])).expect("open");
    let max: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
    for e in pkg.exports.iter().filter(|e| pkg.class_name(e) == a[2]).take(max) {
        println!("{} {}", a[2], e.name);
        let props = match Props::read_object(&pkg, e) {
            Ok(p) => p,
            Err(err) => {
                println!("    ({err})");
                continue;
            }
        };
        for t in &props.tags {
            let raw = &pkg.data[t.at..t.at + t.size.min(24)];
            let v = match t.kind.as_str() {
                "FloatProperty" => format!("{}", f32::from_le_bytes(raw[..4].try_into().unwrap())),
                "IntProperty" => format!("{}", i32::from_le_bytes(raw[..4].try_into().unwrap())),
                "NameProperty" | "ByteProperty" if t.size >= 8 => props.name(&pkg, &t.name).unwrap_or_default(),
                "ObjectProperty" => {
                    let o = i32::from_le_bytes(raw[..4].try_into().unwrap());
                    if o > 0 { pkg.exports[o as usize - 1].name.clone() } else { format!("{o}") }
                }
                "BoolProperty" => format!("{}", t.bool_value),
                _ => format!("{raw:?}"),
            };
            println!("    {} {} = {v}", t.name, t.kind);
            if t.kind == "ArrayProperty" {
                for (k, el) in props.struct_array(&pkg, &t.name).iter().enumerate() {
                    let parts: Vec<String> = el.tags.iter().map(|x| {
                        let r = &pkg.data[x.at..x.at + x.size.min(8)];
                        let v = match x.kind.as_str() {
                            "IntProperty" => format!("{}", i32::from_le_bytes(r[..4].try_into().unwrap())),
                            "FloatProperty" => format!("{}", f32::from_le_bytes(r[..4].try_into().unwrap())),
                            // (An array of enum values: names after its count.)
                            "ArrayProperty" => {
                                let mut c = me_assets::package::Cursor::new(&pkg.data, x.at);
                                let n = c.i32().unwrap_or(0).max(0) as usize;
                                let names: Vec<String> = (0..n.min(16)).filter_map(|_| me_assets::package::read_name(&pkg.names, &mut c).ok()).collect();
                                format!("{names:?}")
                            }
                            _ => el.name(&pkg, &x.name).unwrap_or_else(|| format!("{r:?}")),
                        };
                        format!("{}={v}", x.name)
                    }).collect();
                    if !parts.is_empty() {
                        println!("      [{k}] {}", parts.join(" "));
                    }
                    // (Its own arrays of structs: each one's properties.)
                    for x in el.tags.iter().filter(|x| x.kind == "ArrayProperty") {
                        for (j, sub) in el.struct_array(&pkg, &x.name).iter().enumerate() {
                            let v: Vec<String> = sub.tags.iter().map(|y| {
                                let r = &pkg.data[y.at..y.at + y.size.min(8)];
                                let v = match y.kind.as_str() {
                                    "IntProperty" => format!("{}", i32::from_le_bytes(r[..4].try_into().unwrap())),
                                    "FloatProperty" => format!("{}", f32::from_le_bytes(r[..4].try_into().unwrap())),
                                    "BoolProperty" => format!("{}", y.bool_value),
                                    _ => sub.name(&pkg, &y.name).unwrap_or_else(|| format!("{r:?}")),
                                };
                                format!("{}={v}", y.name)
                            }).collect();
                            println!("        {}[{j}] {}", x.name, v.join(" "));
                        }
                    }
                }
            }
        }
    }
}
