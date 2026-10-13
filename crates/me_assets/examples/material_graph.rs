//! A material's expressions and their properties: `material_graph <CookedPC> <package> <material
//! path or name>`. (For how a material samples its textures: tiling, masks, parameters.)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut pkgs = me_assets::Packages::new(std::path::Path::new(&a[1]));
    let pkg = pkgs.get(&a[2]).expect("package");
    let Some(m) = (0..pkg.exports.len()).find(|&i| pkg.path(i as i32 + 1) == a[3] || pkg.exports[i].name == a[3]) else { panic!("no {}", a[3]) };
    // (Follow it to where it's defined: an import's package.)
    let (mp, mi) = pkgs.resolve(&pkg, m as i32 + 1).expect("resolve");
    let mut chain = vec![(mp.clone(), mi)];
    while let Some((p, i)) = chain.last().cloned() {
        println!("== {} ({})", p.path(i as i32 + 1), p.class_name(&p.exports[i]));
        let props = me_assets::props::Props::read(&p, &p.exports[i]).unwrap();
        for t in &props.tags {
            let raw = &p.data[t.at..t.at + t.size.min(16)];
            println!("  {} {} [{}] {:02x?}", t.name, t.kind, t.size, raw);
        }
        let Some(parent) = props.object(&p, "Parent").filter(|&x| x != 0) else { break };
        let Some(next) = pkgs.resolve(&p, parent) else { break };
        chain.push(next);
    }
    let (p, i) = chain.last().unwrap().clone();
    for (k, e) in p.exports.iter().enumerate() {
        if e.outer != i as i32 + 1 { continue; }
        println!("-- {} {} (#{})", p.class_name(e), e.name, k + 1);
        let Ok(props) = me_assets::props::Props::read(&p, e) else { continue };
        for t in &props.tags {
            let v = match t.kind.as_str() {
                "FloatProperty" => format!("{}", props.f32(&p, &t.name).unwrap_or(f32::NAN)),
                "IntProperty" => format!("{}", props.i32(&p, &t.name).unwrap_or(0)),
                "ObjectProperty" => props.object(&p, &t.name).map_or("?".into(), |o| p.path(o)),
                "NameProperty" => props.name(&p, &t.name).unwrap_or_default(),
                "BoolProperty" => format!("{}", t.bool_value),
                _ => format!("{:02x?}", &p.data[t.at..t.at + t.size.min(24)]),
            };
            println!("    {} = {v}", t.name);
        }
    }
}
