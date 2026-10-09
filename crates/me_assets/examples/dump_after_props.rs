//! Prints the words after an object's property block: `dump_after_props <pkg> <class> <name> [n]`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pkg = me_assets::package::Package::open(std::path::Path::new(&a[1])).expect("open");
    let e = pkg.find(&a[2], &a[3]).expect("no such object");
    let props = me_assets::props::Props::read(&pkg, e).expect("props");
    let n: usize = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(48);
    println!("object at {} size {}, props end at {} (+{})", e.offset, e.size, props.end, props.end - e.offset);
    for k in 0..n {
        let at = props.end + 4 * k;
        if at + 4 > e.offset + e.size {
            break;
        }
        let b = &pkg.data[at..at + 4];
        let i = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let f = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        println!("{:4} {:10} {:14.4} {:02x?}", 4 * k, i, f, b);
    }
}
