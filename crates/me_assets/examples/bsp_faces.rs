//! A level package's BSP faces by material and flags: `bsp_faces <package.me1>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pkg = me_assets::package::Package::open(std::path::Path::new(&a[1])).expect("open");
    let b = me_assets::bsp::Bsp::read(&pkg).expect("bsp");
    let mut count: std::collections::BTreeMap<(String, u32), usize> = Default::default();
    for f in &b.faces {
        let s = &b.surfs[f.surf];
        *count.entry((pkg.object_name(s.material).to_string(), s.flags)).or_default() += 1;
    }
    for ((m, fl), n) in count {
        println!("{n:>5} {m} flags {fl:#x}");
    }
    // `bsp_faces <package> x y z r`: the faces within r (ours, metres) of a point, ours.
    if a.len() >= 6 {
        let q: Vec<f32> = a[2..6].iter().map(|v| v.parse().unwrap()).collect();
        let ue = |p: [f32; 3]| [p[1] * 0.01, p[2] * 0.01, -p[0] * 0.01];
        for f in &b.faces {
            let pts: Vec<[f32; 3]> = f.points.iter().map(|p| ue(*p)).collect();
            let c = pts.iter().fold([0.0f32; 3], |s, p| [s[0] + p[0], s[1] + p[1], s[2] + p[2]]).map(|v| v / pts.len() as f32);
            let d = ((c[0] - q[0]).powi(2) + (c[1] - q[1]).powi(2) + (c[2] - q[2]).powi(2)).sqrt();
            if d < q[3] {
                let s = &b.surfs[f.surf];
                println!("{} {:#x} centre {:?} points {:?}", pkg.object_name(s.material), s.flags, c, pts);
            }
        }
    }
}
