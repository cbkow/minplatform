//! (1) The minColor GPU kernel: the vendored core through its GLSL shim + our compute main → naga → WGSL in OUT_DIR
//! (validated here, so a core that naga cannot take fails the build, not the app). (2) The host derive: the C++ core +
//! `src/ffi.cpp`, compiled with `cc`.
use std::{env, fs, path::PathBuf};

const CORE: [&str; 4] = ["opendrt_params.h", "opendrt_kernel.h", "mincolor_knee.h", "mincolor_agx.h"];

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let mut src = String::from("#version 440\n");
    src += &fs::read_to_string("src/opendrt_shim_naga.h").unwrap(); src.push('\n'); println!("cargo:rerun-if-changed=src/opendrt_shim_naga.h");
    for f in CORE { src += &fs::read_to_string(format!("core/{f}")).unwrap(); src.push('\n'); println!("cargo:rerun-if-changed=core/{f}"); }
    src += &fs::read_to_string("src/mincolor_main.glsl").unwrap().replace("// @AGX_LOADER@", &agx_loader());
    println!("cargo:rerun-if-changed=src/mincolor_main.glsl");
    let mut fe = naga::front::glsl::Frontend::default();
    let module = fe.parse(&naga::front::glsl::Options::from(naga::ShaderStage::Compute), &src)
        .unwrap_or_else(|e| panic!("minColor GLSL: {}", e.emit_to_string(&src)));
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module)
        .unwrap_or_else(|e| panic!("minColor validation: {}", e.emit_to_string(&src)));
    let wgsl = naga::back::wgsl::write_string(&module, &info, naga::back::wgsl::WriterFlags::empty()).unwrap();
    fs::write(out.join("mincolor.wgsl"), wgsl).unwrap();

    cc::Build::new().cpp(true).std("c++17").include("core")
        .file("core/opendrt_presets.cpp").file("core/mincolor_agx.cpp").file("src/ffi.cpp")
        .flag_if_supported("-fno-fast-math").flag_if_supported("/fp:precise")
        .compile("mincolor_core");
    for f in ["src/ffi.cpp", "core/opendrt_presets.cpp", "core/mincolor_agx.cpp", "core/opendrt_presets.h", "core/mincolor_agx_host.h", "core/opendrt.h"] { println!("cargo:rerun-if-changed={f}"); }
}

/// `DrtAgxParams qcv_agx()`: the struct rebuilt from the flat `agxRaw[]`, in declaration order, from the vendored header.
fn agx_loader() -> String {
    let h = fs::read_to_string("core/mincolor_agx.h").unwrap();
    let body = &h[h.find("struct DrtAgxParams {").unwrap()..]; let body = &body[..body.find("};").unwrap()];
    let mut out = String::from("DrtAgxParams qcv_agx() {\n    DrtAgxParams a;\n");
    let mut i = 0usize;
    for line in body.lines().skip(1) {
        let code = line.split("/*").next().unwrap().trim();
        let Some(decl) = code.strip_prefix("float ") else { continue };
        for name in decl.trim_end_matches(';').split(',').map(str::trim).filter(|n| !n.is_empty()) {
            if let Some((n, k)) = name.strip_suffix(']').and_then(|x| x.split_once('[')) {
                for j in 0..k.parse::<usize>().unwrap() { out += &format!("    a.{n}[{j}] = agxRaw[{i}];\n"); i += 1; }
            } else { out += &format!("    a.{name} = agxRaw[{i}];\n"); i += 1; }
        }
    }
    assert_eq!(i, 84, "DrtAgxParams scalar count changed in the vendored core");
    out + "    return a;\n}\n"
}
