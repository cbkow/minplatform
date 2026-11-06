// S1 step 2: with `--features ocio`, compile the OCIO shim against an installed OpenColorIO 2.5
// (`OCIO_ROOT=<prefix>` or `pkg-config OpenColorIO`; QCView's `external/install` works for both).
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/ocio_shim.cpp");
    println!("cargo:rerun-if-env-changed=OCIO_ROOT");
    if std::env::var_os("CARGO_FEATURE_OCIO").is_none() { return; }
    let (inc, lib) = if let Some(root) = std::env::var_os("OCIO_ROOT") {
        let r = std::path::PathBuf::from(root); (r.join("include"), r.join("lib"))
    } else {
        let var = |flag: &str| String::from_utf8(std::process::Command::new("pkg-config").args([flag, "OpenColorIO"]).output().expect("pkg-config").stdout).unwrap().trim().to_string();
        (std::path::PathBuf::from(var("--variable=includedir")), std::path::PathBuf::from(var("--variable=libdir")))
    };
    cc::Build::new().cpp(true).std("c++17").file("src/ocio_shim.cpp").include(&inc).warnings(false).compile("ocio_shim");
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=dylib=OpenColorIO");
    if !cfg!(windows) { println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display()); }   // Windows: the OCIO DLL dir goes on PATH at launch
}
