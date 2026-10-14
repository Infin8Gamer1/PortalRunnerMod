//! The properties of a package's objects of a class (or by name): `props_dump <package.upk>
//! <class or name>`. Numbers, flags, names and objects as values; structs and arrays by size.
use me_assets::package::{Cursor, Package};
use me_assets::props::Props;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pkg = Package::open(std::path::Path::new(&a[1])).expect("open");
    let want = a[2].to_ascii_lowercase();
    for (i, e) in pkg.exports.iter().enumerate() {
        if pkg.class_name(e).to_ascii_lowercase() != want && e.name.to_ascii_lowercase() != want {
            continue;
        }
        println!("== {} ({})", pkg.path(i as i32 + 1), pkg.class_name(e));
        let Ok(p) = Props::read_object(&pkg, e).or_else(|_| Props::read(&pkg, e)) else { continue };
        for t in &p.tags {
            let mut c = Cursor::new(&pkg.data, t.at);
            let v = match t.kind.as_str() {
                "BoolProperty" => format!("{}", t.bool_value),
                "FloatProperty" => format!("{}", c.f32().unwrap_or(0.0)),
                "IntProperty" => format!("{}", c.i32().unwrap_or(0)),
                "NameProperty" | "ByteProperty" => p.name(&pkg, &t.name).unwrap_or_default(),
                "ObjectProperty" | "ComponentProperty" | "ClassProperty" => p.object(&pkg, &t.name).map(|o| pkg.object_name(o).to_string()).unwrap_or_default(),
                "StrProperty" => c.fstring().unwrap_or_default(),
                k => format!("<{k} {} bytes>", t.size),
            };
            println!("  {}{} = {v}", t.name, if t.index > 0 { format!("[{}]", t.index) } else { String::new() });
        }
    }
}
