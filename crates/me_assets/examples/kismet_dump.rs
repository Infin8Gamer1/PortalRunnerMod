//! A map package's Kismet: each sequence op, where its outputs lead and what its variables
//! hold: `kismet_dump <package.me1> [class filter]`. (How a level's movers are set off, #109.)
use me_assets::package::Package;
use me_assets::props::Props;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pkg = Package::open(std::path::Path::new(&a[1])).expect("open");
    let only = a.get(2).cloned();
    let name = |o: i32| if o > 0 { format!("{}#{}", pkg.exports[o as usize - 1].name, o) } else { format!("{o}") };
    for (i, e) in pkg.exports.iter().enumerate() {
        let class = pkg.class_name(e);
        if !(class.starts_with("Seq") || class.starts_with("Interp")) || class == "Sequence" && only.is_none() {
            continue;
        }
        if only.as_ref().is_some_and(|f| !class.contains(f.as_str())) {
            continue;
        }
        let Ok(p) = Props::read(&pkg, e) else { continue };
        println!("{class} {}#{} (in {})", e.name, i + 1, if e.outer > 0 { pkg.exports[e.outer as usize - 1].name.clone() } else { "-".into() });
        for t in &p.tags {
            match t.kind.as_str() {
                "FloatProperty" => println!("    {} = {}", t.name, p.f32(&pkg, &t.name).unwrap()),
                "IntProperty" => println!("    {} = {}", t.name, p.i32(&pkg, &t.name).unwrap()),
                "BoolProperty" => println!("    {} = {}", t.name, t.bool_value),
                "NameProperty" | "ByteProperty" => println!("    {} = {:?}", t.name, p.name(&pkg, &t.name)),
                "ObjectProperty" => println!("    {} = {}", t.name, name(p.object(&pkg, &t.name).unwrap_or(0))),
                "StrProperty" => {}
                "ArrayProperty" if t.name == "OutputLinks" => {
                    for (k, l) in p.struct_array(&pkg, "OutputLinks").iter().enumerate() {
                        let to: Vec<String> = l.struct_array(&pkg, "Links").iter().map(|x| format!("{}[{}]", name(x.object(&pkg, "LinkedOp").unwrap_or(0)), x.i32(&pkg, "InputLinkIdx").unwrap_or(0))).collect();
                        if !to.is_empty() {
                            println!("    out {k} -> {}", to.join(", "));
                        }
                    }
                }
                "ArrayProperty" if t.name == "VariableLinks" => {
                    for (k, l) in p.struct_array(&pkg, "VariableLinks").iter().enumerate() {
                        let vars: Vec<String> = l.object_array(&pkg, "LinkedVariables").iter().map(|&v| {
                            let val = if v > 0 { Props::read(&pkg, &pkg.exports[v as usize - 1]).ok().and_then(|vp| vp.object(&pkg, "ObjValue")).map(|o| name(o)).unwrap_or_default() } else { String::new() };
                            format!("{}={}", name(v), val)
                        }).collect();
                        if !vars.is_empty() {
                            println!("    var {k}: {}", vars.join(", "));
                        }
                    }
                }
                "ArrayProperty" if t.name == "InputLinks" => {}
                "ArrayProperty" => println!("    {} [array {} bytes]", t.name, t.size),
                "StructProperty" => println!("    {} [struct {} bytes]", t.name, t.size),
                _ => {}
            }
        }
    }
}
