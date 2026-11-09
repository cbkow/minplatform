// Builds the C++ layout check from vendor/qcbae: the compiler's own sizeof/offsetof for every wire struct. The canonical headers
// include each other as "common/…" (QCBridgeAE's include root is src/), so the vendored tree is mirrored into OUT_DIR/common.
fn main() {
    println!("cargo:rerun-if-changed=src/layout_check.cpp");
    println!("cargo:rerun-if-changed=../../vendor/qcbae");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let inc = out.join("inc"); let common = inc.join("common");
    let _ = std::fs::remove_dir_all(&inc);
    fn copy_dir(src: &std::path::Path, dst: &std::path::Path) { std::fs::create_dir_all(dst).unwrap(); for e in std::fs::read_dir(src).unwrap() { let e = e.unwrap(); let p = e.path(); let d = dst.join(e.file_name()); if p.is_dir() { copy_dir(&p, &d) } else { std::fs::copy(&p, &d).unwrap(); } } }
    copy_dir(std::path::Path::new("../../vendor/qcbae"), &common);
    let mut b = cc::Build::new();
    b.cpp(true).std("c++20").file("src/layout_check.cpp").include(&inc).warnings(false);
    if cfg!(target_os = "macos") { b.flag("-mmacosx-version-min=12.0"); }
    b.compile("qcbae_layout_check");
}
