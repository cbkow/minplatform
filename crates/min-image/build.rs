//! OpenEXRCore's C API through bindgen. macOS: pkg-config `OpenEXR` (Homebrew 3.5, pinned in tools/image-libs). Windows:
//! `OPENEXR_DIR` = a vcpkg-style prefix (`include/OpenEXR`, `lib/OpenEXRCore-3_5.lib`, the DLL on PATH at run time).
//! TurboJPEG (libjpeg-turbo 3's `tj3` API — QCView's JPEG decoder) the same way: pkg-config `libturbojpeg` (Homebrew
//! `jpeg-turbo`), or `TURBOJPEG_DIR` = a vcpkg-style prefix (`include/turbojpeg.h`, `lib/turbojpeg.lib`).
//! libtiff (QCView's TIFF library — the fallback for the layouts the `tiff` crate refuses, canon row 189) likewise:
//! pkg-config `libtiff-4` (Homebrew `libtiff`), or `TIFF_DIR` = a vcpkg-style prefix (`include/tiffio.h`, `lib/tiff.lib`).
use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=OPENEXR_DIR");
    let mut includes: Vec<PathBuf> = vec![];
    if let Ok(dir) = env::var("OPENEXR_DIR") {
        let d = PathBuf::from(dir);
        includes.push(d.join("include").join("OpenEXR")); includes.push(d.join("include"));
        println!("cargo:rustc-link-search=native={}", d.join("lib").display());
        let lib = std::fs::read_dir(d.join("lib")).ok().and_then(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).find(|n| n.starts_with("OpenEXRCore") && n.ends_with(".lib") && !n.contains("_d")))
            .map(|n| n.trim_end_matches(".lib").to_string()).unwrap_or_else(|| "OpenEXRCore".into());
        println!("cargo:rustc-link-lib={lib}");
    } else {
        let lib = pkg_config::Config::new().atleast_version("3.2").probe("OpenEXR").expect("OpenEXR not found — `brew install openexr` (macOS) or set OPENEXR_DIR");
        includes.extend(lib.include_paths.iter().cloned());
    }
    let mut b = bindgen::Builder::default().header_contents("wrap.h", "#include <openexr.h>\n")
        .allowlist_function("exr_.*").allowlist_type("exr_.*").allowlist_var("EXR_.*")
        .derive_default(true).layout_tests(false).generate_comments(false);
    for i in &includes { b = b.clang_arg(format!("-I{}", i.display())); }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("openexr.rs");
    b.generate().expect("bindgen openexr.h").write_to_file(out).unwrap();
    turbojpeg();
    libtiff();
}

fn turbojpeg() {
    println!("cargo:rerun-if-env-changed=TURBOJPEG_DIR");
    let mut includes: Vec<PathBuf> = vec![];
    if let Ok(dir) = env::var("TURBOJPEG_DIR") {
        let d = PathBuf::from(dir);
        includes.push(d.join("include"));
        println!("cargo:rustc-link-search=native={}", d.join("lib").display());
        println!("cargo:rustc-link-lib=turbojpeg");
    } else {
        let lib = pkg_config::Config::new().atleast_version("3.0").probe("libturbojpeg").expect("TurboJPEG not found — `brew install jpeg-turbo` (macOS) or set TURBOJPEG_DIR");
        includes.extend(lib.include_paths.iter().cloned());
    }
    let mut b = bindgen::Builder::default().header_contents("tj.h", "#include <stddef.h>\n#include <turbojpeg.h>\n")
        .allowlist_function("tj3.*").allowlist_type("TJ.*").allowlist_var("TJ.*").allowlist_var("TURBOJPEG_VERSION_NUMBER")
        .default_enum_style(bindgen::EnumVariation::Consts).prepend_enum_name(false).layout_tests(false).generate_comments(false);
    for i in &includes { b = b.clang_arg(format!("-I{}", i.display())); }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("turbojpeg.rs");
    b.generate().expect("bindgen turbojpeg.h").write_to_file(&out).unwrap();
    // 3.2 made tj3Init a macro over tj3InitVersion(type, TURBOJPEG_VERSION_NUMBER); 3.0 / 3.1 export tj3Init itself.
    println!("cargo::rustc-check-cfg=cfg(tj_init_version)");
    if std::fs::read_to_string(&out).unwrap().contains("fn tj3InitVersion") { println!("cargo:rustc-cfg=tj_init_version"); }
}

fn libtiff() {
    println!("cargo:rerun-if-env-changed=TIFF_DIR");
    let mut includes: Vec<PathBuf> = vec![];
    if let Ok(dir) = env::var("TIFF_DIR") {
        let d = PathBuf::from(dir);
        includes.push(d.join("include"));
        println!("cargo:rustc-link-search=native={}", d.join("lib").display());
        println!("cargo:rustc-link-lib=tiff");
    } else {
        let lib = pkg_config::Config::new().atleast_version("4.0").probe("libtiff-4").expect("libtiff not found — `brew install libtiff` (macOS) or set TIFF_DIR");
        includes.extend(lib.include_paths.iter().cloned());
    }
    let mut b = bindgen::Builder::default().header_contents("t.h", "#include <tiffio.h>\n")
        .allowlist_function("TIFFOpen").allowlist_function("TIFFOpenW").allowlist_function("TIFFClose").allowlist_function("TIFFGetField")
        .allowlist_function("TIFFReadRGBAImageOriented").allowlist_function("TIFFSetWarningHandler").allowlist_function("TIFFSetErrorHandler")
        .allowlist_var("TIFFTAG_IMAGEWIDTH").allowlist_var("TIFFTAG_IMAGELENGTH").allowlist_var("TIFFTAG_EXTRASAMPLES").allowlist_var("ORIENTATION_TOPLEFT")
        .layout_tests(false).generate_comments(false);
    for i in &includes { b = b.clang_arg(format!("-I{}", i.display())); }
    b.generate().expect("bindgen tiffio.h").write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join("libtiff.rs")).unwrap();
}
