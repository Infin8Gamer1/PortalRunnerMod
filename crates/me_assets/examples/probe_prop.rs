//! A static mesh and what its materials resolve to: `probe_prop <CookedPC> <package> <mesh> [out dir]`.
//! Prints each section's material and textures and the UV range, and writes the diffuse maps
//! as PPM for a look.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut pkgs = me_assets::Packages::new(std::path::Path::new(&a[1]));
    let (mesh, slots) = me_assets::load_static_mesh(&mut pkgs, &a[2], &a[3], 1024).expect("load");
    let out = a.get(4).cloned().unwrap_or_else(|| ".".into());
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for uv in &mesh.uvs {
        for k in 0..2 {
            lo[k] = lo[k].min(uv[k]);
            hi[k] = hi[k].max(uv[k]);
        }
    }
    println!("{}: {} verts, uv {lo:?}..{hi:?}", mesh.name, mesh.positions.len());
    // The UV layout, as a list of triangles (u v u v u v per line), and each face's normal.
    let mut tris = String::new();
    for t in mesh.indices.chunks_exact(3) {
        let n = mesh.normals[t[0] as usize];
        for &i in t {
            let uv = mesh.uvs[i as usize];
            tris += &format!("{} {} ", uv[0], uv[1]);
        }
        tris += &format!("{:.2} {:.2} {:.2}\n", n[0], n[1], n[2]);
    }
    std::fs::write(format!("{out}/{}.uv", mesh.name), tris).unwrap();
    // How well the stored normals agree with the triangles' own (wound either way).
    let (mut agree, mut against) = (0, 0);
    for t in mesh.indices.chunks_exact(3) {
        let p = |k: usize| glam::Vec3::from(mesh.positions[t[k] as usize]);
        let g = (p(1) - p(0)).cross(p(2) - p(0)).normalize_or_zero();
        let n = glam::Vec3::from(mesh.normals[t[0] as usize]);
        let d = g.dot(n);
        if d > 0.5 {
            agree += 1;
        } else if d < -0.5 {
            against += 1;
        }
    }
    println!("normals: {agree} agree, {against} opposite, of {}", mesh.indices.len() / 3);
    for (s, slot) in mesh.sections.iter().zip(&slots) {
        let size = |t: &Option<me_assets::Rgba>| t.as_ref().map(|t| format!("{}x{}", t.width, t.height));
        println!("  {} tris: {} diffuse {:?} normal {:?} spec {:?} uv x{:?}", s.triangles, slot.material, size(&slot.diffuse), size(&slot.normal), size(&slot.specular), slot.uv_scale);
        if let Some(t) = &slot.diffuse {
            let mut ppm = format!("P6 {} {} 255\n", t.width, t.height).into_bytes();
            ppm.extend(t.pixels.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]));
            std::fs::write(format!("{out}/{}.ppm", slot.material), ppm).unwrap();
        }
    }
}
