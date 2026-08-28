// rust-embed bakes `static/` into the release binary, so cargo must rebuild when the UI changes.
fn main() {
    println!("cargo:rerun-if-changed=static");
}
