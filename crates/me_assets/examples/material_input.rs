//! What feeds one of a material's inputs, as a tree of expressions: `material_input <CookedPC>
//! <package> <material path or name> <input>` (`Opacity`, `OpacityMask`, `DiffuseColor`, ...).
use me_assets::props::Props;

fn show(p: &me_assets::package::Package, e: usize, input: &str, depth: usize) {
    let Ok(props) = Props::read(p, &p.exports[e]) else { return };
    let Some(t) = props.get(input) else { return };
    let Ok(i) = Props::read_list(p, t.at, t.at + t.size, "ExpressionInput") else { return };
    let Some(x) = i.object(p, "Expression").filter(|&x| x > 0).map(|x| x as usize - 1) else { return };
    let masks: String = ["MaskR", "MaskG", "MaskB", "MaskA"].iter().filter(|m| i.i32(p, m) == Some(1)).map(|m| &m[4..]).collect();
    let xp = Props::read(p, &p.exports[x]).unwrap();
    let mut what = String::new();
    for n in ["R", "G", "B", "A"].iter().filter(|_| p.class_name(&p.exports[x]).contains("Constant")) {
        if let Some(v) = xp.f32(p, n) {
            what += &format!(" {n}={v}");
        }
    }
    if let Some(n) = xp.name(p, "ParameterName") {
        what += &format!(" param {n}");
    }
    if let Some(t) = xp.object(p, "Texture").filter(|&t| t != 0) {
        what += &format!(" texture {}", p.object_name(t));
    }
    println!("{}{input}{}: {} {}{what}", "  ".repeat(depth), if masks.is_empty() { String::new() } else { format!(" [{masks}]") }, p.class_name(&p.exports[x]), p.exports[x].name);
    for sub in ["A", "B", "Alpha", "Input", "Coordinates", "Coordinate", "Base", "Exponent", "X", "Y"] {
        show(p, x, sub, depth + 1);
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut pkgs = me_assets::Packages::new(std::path::Path::new(&a[1]));
    let pkg = pkgs.get(&a[2]).expect("package");
    let Some(m) = (0..pkg.exports.len()).find(|&i| pkg.path(i as i32 + 1) == a[3] || pkg.exports[i].name == a[3]) else { panic!("no {}", a[3]) };
    let (mut p, mut i) = pkgs.resolve(&pkg, m as i32 + 1).expect("resolve");
    // (An instance: its parents' down to the material.)
    for _ in 0..6 {
        println!("== {} ({})", p.path(i as i32 + 1), p.class_name(&p.exports[i]));
        if p.class_name(&p.exports[i]) == "Material" {
            break;
        }
        let props = Props::read(&p, &p.exports[i]).unwrap();
        let Some(par) = props.object(&p, "Parent") else { return };
        (p, i) = pkgs.resolve(&p, par).expect("parent");
    }
    show(&p, i, &a[4], 0);
}
