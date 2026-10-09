//! Prints every animation in Faith's first-person set and its length:
//! `ME_INSTALL=<dir> cargo run -p faith_anim --example list_anims`.
fn main() {
    let dir = std::env::var_os("ME_INSTALL").expect("set ME_INSTALL to the Mirror's Edge install");
    let arms = me_assets::FaithArms::load(std::path::Path::new(&dir), 4).expect("load");
    let mut names: Vec<_> = arms.anims.sequences.iter().map(|(k, s)| (k.clone(), s.length)).collect();
    names.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    for (n, len) in names {
        println!("{n}\t{len:.2}");
    }
}
