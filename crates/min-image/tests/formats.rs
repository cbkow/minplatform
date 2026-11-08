//! Every PNG / TIFF / JPEG variant QCView's loaders meet, generated synthetically by FFmpeg (fixtures never travel)
//! and compared with a reference decode: PNG and TIFF against FFmpeg's (IDENTICAL), JPEG against libjpeg-turbo's
//! `djpeg` (QCView's decoder; within one code value).
//! Skips when `ffmpeg` is not on PATH.
#![allow(clippy::chunks_exact_to_as_chunks)] // the reference layouts read clearer as slices
use min_image::{load, Image, Sample};
use std::process::Command;

fn ffmpeg(args: &[&str]) -> bool { Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-y"]).args(args).status().map(|s| s.success()).unwrap_or(false) }
fn dir() -> std::path::PathBuf { let d = std::env::temp_dir().join(format!("min-image-formats-{}", std::process::id())); std::fs::create_dir_all(&d).unwrap(); d }

/// Our planes as RGBA16 (8-bit scaled ×257, missing alpha = opaque) next to FFmpeg's rgba64le decode.
fn rgba16(img: &Image) -> Vec<u16> {
    let (w, h) = (img.width as usize, img.height as usize);
    let at = |pl: usize, x: usize, y: usize| -> u16 { let o = y * img.pitch as usize + x * img.sample.bytes(); match img.sample { Sample::U8 => img.planes[pl][o] as u16 * 257, _ => u16::from_le_bytes([img.planes[pl][o], img.planes[pl][o + 1]]) } };
    let mut v = Vec::with_capacity(w * h * 4);
    for y in 0..h { for x in 0..w { v.extend([at(2, x, y), at(0, x, y), at(1, x, y), if img.alpha { at(3, x, y) } else { 65535 }]) } }
    v
}
/// FFmpeg's decode of the file in the decoder's NATIVE format (`-c:v rawvideo`, no swscale — its conversions are not
/// exact), widened to RGBA16 here by the layout rules of that format.
fn reference(path: &str, native: &str) -> Vec<u16> {
    let raw = format!("{path}.raw");
    assert!(ffmpeg(&["-i", path, "-c:v", "rawvideo", "-pix_fmt", native, "-f", "rawvideo", &raw]));
    let b = std::fs::read(raw).unwrap();
    let (w, h) = (317usize, 181usize);
    let u16s = |be: bool| -> Vec<u16> { b.as_chunks::<2>().0.iter().map(|c| if be { u16::from_be_bytes(*c) } else { u16::from_le_bytes(*c) }).collect() };
    let w8 = |v: u8| v as u16 * 257;
    match native {
        "rgb24" => b.chunks_exact(3).flat_map(|p| [w8(p[0]), w8(p[1]), w8(p[2]), 65535]).collect(),
        "rgba" => b.iter().map(|&v| w8(v)).collect(),
        "gray" => b.iter().flat_map(|&g| [w8(g), w8(g), w8(g), 65535]).collect(),
        "ya8" => b.chunks_exact(2).flat_map(|p| [w8(p[0]), w8(p[0]), w8(p[0]), w8(p[1])]).collect(),
        "monob" => (0..w * h).map(|i| { let (x, y) = (i % w, i / w); let bit = (b[y * w.div_ceil(8) + x / 8] >> (7 - x % 8)) & 1; if bit == 1 { 65535 } else { 0 } }).flat_map(|g| [g, g, g, 65535]).collect(),
        "pal8" => { let pal = &b[w * h..]; b[..w * h].iter().flat_map(|&i| { let c = u32::from_le_bytes(pal[i as usize * 4..i as usize * 4 + 4].try_into().unwrap()); [w8((c >> 16) as u8), w8((c >> 8) as u8), w8(c as u8), w8((c >> 24) as u8)] }).collect() }
        "gbrp" => (0..w * h).flat_map(|i| [w8(b[2 * w * h + i]), w8(b[i]), w8(b[w * h + i]), 65535]).collect(),
        "rgb48be" | "rgb48le" => u16s(native.ends_with("be")).chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 65535]).collect(),
        "rgba64be" | "rgba64le" => u16s(native.ends_with("be")),
        "gray16be" | "gray16le" => u16s(native.ends_with("be")).iter().flat_map(|&g| [g, g, g, 65535]).collect(),
        "ya16be" | "ya16le" => u16s(native.ends_with("be")).chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        other => panic!("no layout rule for {other}"),
    }
}
/// libjpeg-turbo's own decode (`djpeg`: islow IDCT + fancy upsampling = QCView's `jpeg_read_header` defaults) as
/// RGBA16; None when djpeg is not installed.
fn reference_djpeg(path: &str) -> Option<Vec<u16>> {
    let out = Command::new("djpeg").arg(path).output().ok().filter(|o| o.status.success())?.stdout;
    // PNM: "P6\nW H\n255\n" (or P5 for gray), then samples.
    let mut fields = 0; let mut i = 0;
    while fields < 4 { while out[i].is_ascii_whitespace() { i += 1 } while !out[i].is_ascii_whitespace() { i += 1 } fields += 1 }
    let (gray, px) = (out[1] == b'5', &out[i + 1..]);
    Some(if gray { px.iter().flat_map(|&g| [g as u16 * 257, g as u16 * 257, g as u16 * 257, 65535]).collect() }
         else { px.chunks_exact(3).flat_map(|p| [p[0] as u16 * 257, p[1] as u16 * 257, p[2] as u16 * 257, 65535]).collect() })
}

/// `native` = the FFmpeg decoder's output format for the file (exact reference); None = JPEG (PSNR / mean bars).
fn check(ext: &str, pix: &str, extra: &[&str], want: (Sample, bool), native: Option<&str>) {
    let d = dir();
    let path = d.join(format!("t_{pix}_{}.{ext}", extra.join("_").replace('-', ""))).to_string_lossy().into_owned();
    let mut args = vec!["-f", "lavfi", "-i", "testsrc2=size=318x182:rate=1", "-vf", "format=rgba64le,crop=317:181:0:0", "-frames:v", "1", "-pix_fmt", pix];
    args.extend(extra); args.push(&path);
    assert!(ffmpeg(&args), "ffmpeg could not write {pix} {ext}");
    let img = load(&path, "", 0).unwrap_or_else(|e| panic!("{pix} {ext}: {e}"));
    assert_eq!((img.width, img.height), (317, 181));
    assert_eq!((img.sample, img.alpha), want, "{pix} {ext}");
    let ours = rgba16(&img);
    match native {
        Some(n) => {
            let theirs = reference(&path, n);
            let worst = ours.iter().zip(&theirs).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
            assert_eq!(worst, 0, "{pix} {ext} (native {n}): worst {worst}");
            eprintln!("{ext:4} {pix:10} {extra:?} → {:?} alpha {} IDENTICAL to FFmpeg", img.sample, img.alpha);
        }
        None => {
            let Some(theirs) = reference_djpeg(&path) else { eprintln!("{ext} {pix}: no djpeg — decode only"); return };
            let worst = ours.iter().zip(&theirs).map(|(a, b)| a.abs_diff(*b) / 257).max().unwrap();
            let off = ours.iter().zip(&theirs).filter(|(a, b)| a != b).count();
            eprintln!("{ext:4} {pix:10} → worst {worst} code values, {off} of {} samples differ vs libjpeg-turbo", ours.len());
            assert!(worst <= 1, "{pix} {ext}: worst {worst} vs libjpeg-turbo");
        }
    }
}

fn have_ffmpeg() -> bool { Command::new("ffmpeg").arg("-version").output().is_ok() }

#[test]
fn png_every_colour_type_and_depth_matches_ffmpeg() {
    if !have_ffmpeg() { return }
    for (pix, want) in [("rgb24", (Sample::U8, false)), ("rgba", (Sample::U8, true)), ("gray", (Sample::U8, false)), ("ya8", (Sample::U8, true)),
                        ("pal8", (Sample::U8, false)), ("monob", (Sample::U8, false)), ("rgb48be", (Sample::U16, false)),
                        ("rgba64be", (Sample::U16, true)), ("gray16be", (Sample::U16, false)), ("ya16be", (Sample::U16, true))] {
        check("png", pix, &[], want, Some(pix));
    }
}

#[test]
fn tiff_every_layout_and_compression_matches_ffmpeg() {
    if !have_ffmpeg() { return }
    for comp in ["raw", "lzw", "deflate", "packbits"] {
        for (pix, want) in [("rgb24", (Sample::U8, false)), ("rgba", (Sample::U8, true)), ("gray", (Sample::U8, false)), ("ya8", (Sample::U8, true)),
                            ("rgb48le", (Sample::U16, false)), ("rgba64le", (Sample::U16, true)), ("gray16le", (Sample::U16, false))] {
            check("tif", pix, &["-compression_algo", comp], want, Some(pix));
        }
    }
    // Palette TIFFs: the `tiff` crate refuses RGBPalette at read — a clean error → a broken frame (QCView's libtiff
    // RGBA helper read them; rare in sequences — libtiff joins if a real one shows up).
    let d = dir(); let p = d.join("pal.tif").to_string_lossy().into_owned();
    assert!(ffmpeg(&["-f", "lavfi", "-i", "testsrc2=size=64x64:rate=1", "-frames:v", "1", "-pix_fmt", "pal8", &p]));
    assert!(load(&p, "", 0).is_err());
    // FFmpeg writes planar TIFF from its planar RGB formats.
    check("tiff", "gbrp", &["-compression_algo", "lzw"], (Sample::U8, false), Some("gbrp"));
}

#[test]
fn jpeg_rgb_and_gray_match_libjpeg_turbo() {
    if !have_ffmpeg() { return }
    // Two IDCT / upsampling implementations (zune follows libjpeg-turbo's fancy upsampling, QCView's decoder).
    check("jpg", "yuvj420p", &["-q:v", "2"], (Sample::U8, false), None);
    check("jpeg", "yuvj444p", &["-q:v", "2"], (Sample::U8, false), None);
    check("jpg", "gray", &["-q:v", "2"], (Sample::U8, false), None);
}

#[test]
fn float_tiff_lands_as_half_and_orientation_flips() {
    use tiff::{encoder::{colortype, TiffEncoder}, tags::Tag};
    let d = dir();
    let p = d.join("float.tif");
    let (w, h) = (5u32, 3u32);
    let data: Vec<f32> = (0..w * h * 3).map(|i| i as f32 * 0.25).collect();
    {
        let mut enc = TiffEncoder::new(std::fs::File::create(&p).unwrap()).unwrap();
        let mut im = enc.new_image::<colortype::RGB32Float>(w, h).unwrap();
        im.encoder().write_tag(Tag::Orientation, 3u16).unwrap(); // bottom-right: rotated 180°
        im.write_data(&data).unwrap();
    }
    let img = load(&p.to_string_lossy(), "", 0).unwrap();
    assert_eq!((img.sample, img.alpha, img.width, img.height), (Sample::F16, false, 5, 3));
    let r = |x: usize, y: usize| half::f16::from_le_bytes([img.planes[2][y * img.pitch as usize + x * 2], img.planes[2][y * img.pitch as usize + x * 2 + 1]]).to_f32();
    // Stored pixel (0,0) R = 0.0 lands at (4,2); stored (4,2) R = (2*5+4)*3*0.25 = 10.5 lands at (0,0).
    assert_eq!((r(4, 2), r(0, 0)), (0.0, 10.5));
}

#[test]
fn unknown_extensions_and_missing_files_fail_cleanly() {
    assert!(load("/nope/x.png", "", 0).is_err());
    assert!(load("/nope/x.dpx", "", 0).is_err());
    let bad = dir().join("bad.jpg"); std::fs::write(&bad, b"not a jpeg at all").unwrap();
    assert!(load(&bad.to_string_lossy(), "", 0).is_err(), "a corrupt JPEG is an error, never a process exit");
}

/// QCView's layer discovery on a synthetic multi-part EXR (FFmpeg frames combined by OpenEXR's `exrmultipart`; skipped
/// without it): part names in order, the cryptomatte part filtered, each part loads its own pixels, the compression named.
#[test]
fn exr_multipart_layers() {
    let d = std::env::temp_dir().join(format!("min-image-mp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(&d).unwrap();
    let ff = |src: &str, name: &str| std::process::Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", src, "-frames:v", "1", "-pix_fmt", "gbrpf32le", "-c:v", "exr"]).arg(d.join(name)).status().map(|s| s.success()).unwrap_or(false);
    if !(ff("testsrc2=size=64x36", "a.exr") && ff("smptebars=size=64x36", "b.exr") && ff("color=red:size=64x36", "c.exr")) { eprintln!("ffmpeg missing: skipped"); return }
    let multi = d.join("multi.exr");
    let ok = std::process::Command::new("exrmultipart").arg("-combine").arg("-i").arg(format!("{}::beauty", d.join("a.exr").display())).arg(format!("{}::diffuse", d.join("b.exr").display())).arg(format!("{}::CryptoObject", d.join("c.exr").display())).arg("-o").arg(&multi)
        .stdout(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    if !ok { eprintln!("exrmultipart missing: skipped"); return }
    let m = multi.to_string_lossy();
    assert_eq!(min_image::exr::layers(&m).unwrap(), vec!["beauty".to_string(), "diffuse".to_string()]);
    let (a, b) = (min_image::load(&m, "beauty", 1).unwrap(), min_image::load(&m, "diffuse", 1).unwrap());
    assert_ne!(a.planes[0], b.planes[0], "each part loads its own pixels");
    assert_eq!(min_image::load(&m, "", 1).unwrap().planes[0], a.planes[0], "no layer = part 0");
    assert!(min_image::exr::compression(&m).is_some());
    // a single-part file: "default" (bare channels), and it loads
    let single = d.join("a.exr").to_string_lossy().into_owned();
    assert_eq!(min_image::exr::layers(&single).unwrap(), vec!["default".to_string()]);
    assert!(min_image::load(&single, "default", 1).is_ok());
    let _ = std::fs::remove_dir_all(&d);
}
