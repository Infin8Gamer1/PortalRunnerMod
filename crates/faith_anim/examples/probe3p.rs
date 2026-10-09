//! Loads Faith's third-person body and animations and prints what's there.
fn main() {
    let dir = std::env::var_os("ME_INSTALL").expect("ME_INSTALL");
    let f = me_assets::FaithArms::load_character(std::path::Path::new(&dir), "Characters/CH_TKY_Crim_Fixer.upk", "SK_TKY_Crim_Fixer", "Animations/AS_F3P_Unarmed.upk", "AS_F3P_Unarmed").expect("load");
    println!("bones {} verts {} sections {}", f.mesh.bones.len(), f.mesh.vertices.len(), f.mesh.sections.len());
    let mut names: Vec<_> = f.anims.sequences.keys().cloned().collect();
    names.sort_by_key(|n| n.to_lowercase());
    println!("{} anims: {}", names.len(), names.join(" "));
    println!("bones: {}", f.mesh.bones.iter().map(|b| b.name.as_str()).collect::<Vec<_>>().join(" "));
}
