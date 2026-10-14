//! What a built Mirror's Edge level collides with round a point: `me_solids <level file>
//! x,y,z [half size x,y,z]` (our axes, metres). Its boxes, and its slanted faces.
use aperture::doc::LevelDoc;
use glam::Vec3;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = LevelDoc::from_ron(&std::fs::read_to_string(&a[1]).expect("level")).expect("doc");
    let v = |s: &str| -> Vec3 {
        let f: Vec<f32> = s.split(',').map(|x| x.parse().unwrap()).collect();
        Vec3::new(f[0], f[1], f[2])
    };
    let at = v(&a[2]);
    let half = a.get(3).map_or(Vec3::splat(1.0), |s| v(s));
    let l = doc.build();
    let (lo, hi) = (at - half, at + half);
    for s in &l.solids {
        if s.b.max.cmpgt(lo).all() && s.b.min.cmplt(hi).all() {
            println!("box {:?} .. {:?} {:?}", s.b.min, s.b.max, s.mat);
        }
    }
    // (And which props' shapes make boxes there.)
    for o in &doc.objects {
        let aperture::doc::Kind::MeProp { package, mesh, scale, solid, .. } = &o.kind else { continue };
        let b = o.bounds();
        if b.max.cmplt(lo).any() || b.min.cmpgt(hi).any() {
            continue;
        }
        let Some(shape) = aperture::me::shape(package, mesh) else { continue };
        let (mut solids, mut slopes) = (Vec::new(), Vec::new());
        shape.place(o.at, o.rotation(), *scale, aperture::Mat::Wall, &mut solids, &mut slopes);
        let n = solids.iter().filter(|s| s.b.max.cmpgt(lo).all() && s.b.min.cmplt(hi).all()).count();
        if n > 0 {
            println!("from {} {mesh} (solid {solid}): {n} boxes", o.name);
        }
    }
    for s in &l.slopes {
        if s.pts.iter().any(|p| p.cmpgt(lo).all() && p.cmplt(hi).all()) {
            println!("slope n {:?} pts {:?}", s.normal, s.pts);
        }
    }
}
