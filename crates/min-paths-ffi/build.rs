// The C header include/min_paths.h is COMMITTED (a Qt build needs only the static library + this header) and regenerated
// only on request — `MIN_PATHS_REGEN_HEADER=1 cargo build -p min-paths-ffi` — so a consumer's build never dirties the tree
// (cbindgen's output differs slightly per platform, which blocked a fast-forward on the Windows box once). Every build
// still generates a copy into OUT_DIR and compares it, so a stale committed header fails loudly in `cargo test`.
fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    println!("cargo:rerun-if-env-changed=MIN_PATHS_REGEN_HEADER");
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let Ok(b) = cbindgen::Builder::new().with_crate(&crate_dir).with_config(cbindgen::Config::from_file(format!("{crate_dir}/cbindgen.toml")).unwrap_or_default()).generate() else { return };
    let generated = format!("{out_dir}/min_paths.h");
    b.write_to_file(&generated);
    if std::env::var("MIN_PATHS_REGEN_HEADER").is_ok_and(|v| v == "1") {
        std::fs::copy(&generated, format!("{crate_dir}/include/min_paths.h")).unwrap();
    }
}
