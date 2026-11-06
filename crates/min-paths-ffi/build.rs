// Regenerates include/min_paths.h from src/lib.rs. The header is committed so a Qt build needs only the static library.
fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    if let Ok(b) = cbindgen::Builder::new().with_crate(&crate_dir).with_config(cbindgen::Config::from_file(format!("{crate_dir}/cbindgen.toml")).unwrap_or_default()).generate() {
        b.write_to_file(format!("{crate_dir}/include/min_paths.h"));
    }
}
