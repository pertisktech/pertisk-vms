fn main() {
    println!("cargo:rerun-if-env-changed=pertisk_vms_VERSION");
    let version = std::env::var("pertisk_vms_VERSION")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    println!("cargo:rustc-env=pertisk_vms_VERSION={version}");
}
