// rust-embed bakes `static/` into the release binary, so cargo must rebuild when the UI changes.
// The folder is gitignored (Vite output). Create it so cargo works before `npm run build`.
fn main() {
    let dir = std::path::Path::new("static");
    if !dir.exists() {
        std::fs::create_dir_all(dir).expect("create crates/pertisk-daemon/static");
    }
    println!("cargo:rerun-if-changed=static");
}
