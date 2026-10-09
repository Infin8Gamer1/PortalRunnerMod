//! Lists a package's objects by class: `cargo run -p me_assets --example list_exports --
//! <package.upk> [class]`.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let pkg = me_assets::package::Package::open(std::path::Path::new(&args[1])).expect("open");
    let filter = args.get(2).map(|s| s.to_ascii_lowercase());
    for e in &pkg.exports {
        let class = pkg.class_name(e).to_string();
        if filter.as_ref().is_none_or(|f| class.to_ascii_lowercase() == *f) {
            println!("{class}\t{}\t{} bytes", e.name, e.size);
        }
    }
}
