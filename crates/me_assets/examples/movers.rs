//! A map package's matinee moves and what sets them off: `movers <package.me1> [actors]`
//! (#109; `actors`: its movable actors too, where they are and the events they set off).
use me_assets::kismet;
use me_assets::package::Package;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pkg = Package::open(std::path::Path::new(&a[1])).expect("open");
    let ops = kismet::ops(&pkg);
    let moves = kismet::moves(&pkg, &ops);
    for m in &moves {
        let actors: Vec<String> = m.actors.iter().map(|&x| format!("{} ({})", pkg.exports[x].name, pkg.class_name(&pkg.exports[x]))).collect();
        println!("{} group {} len {} rate {} loop {} relative {} actors {:?}", pkg.exports[m.op].name, m.group, m.length, m.rate, m.looping, m.relative, actors);
        for k in &m.keys {
            println!("    t {:.2} pos {:?} euler {:?} {}", k.t, k.pos, k.euler, if k.linear { "linear" } else { "" });
        }
        // What leads into it.
        for o in &ops {
            for (j, out) in o.outputs.iter().enumerate() {
                for &(to, input, delay) in out {
                    if to == m.op {
                        println!("    <- {} {} out {j} -> in {input} (delay {delay})", o.class, o.name);
                    }
                }
            }
        }
        for &x in &m.actors {
            let ev: Vec<String> = kismet::events_of(&pkg, x).iter().map(|&e| format!("{} {}", pkg.class_name(&pkg.exports[e]), pkg.exports[e].name)).collect();
            if !ev.is_empty() {
                println!("    {} sets off {:?}", pkg.exports[x].name, ev);
            }
        }
    }
    println!("{} moves", moves.len());
    if a.get(2).is_some_and(|x| x == "actors") {
        let m = me_assets::map::read(&pkg);
        for p in m.meshes.iter().filter(|p| p.class != "StaticMeshActor") {
            let ev: Vec<String> = kismet::events_of(&pkg, p.export).iter().map(|&e| pkg.exports[e].name.clone()).collect();
            println!("  {} {} {} at {:?} rot {:?} events {:?}", p.class, p.name, p.mesh_name, p.location, p.rotation, ev);
        }
    }
}
