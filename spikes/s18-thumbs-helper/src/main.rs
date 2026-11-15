//! S18 - the system's thumbnails and icons from Rust, and decoding in helper processes
//! (docs/spikes/S18-thumbs-helper.md). One executable; everything runs by script and prints numbers:
//!   s18 gen                 the generated files under target/s18-files (2,000 JPEGs of 2048 x 2048, the poison files)
//!   s18 sys-thumbs          A1, A2: the system's thumbnails and icons, made INSIDE a helper; cold, warm; a cancel
//!   s18 throughput [n ...]  B1: 2,000 files -> 512 px masters through n helpers (default 1 2 4 8), RGBA and JPEG
//!   s18 poison [rounds]     B2: a file that aborts the helper and one that hangs it, among good ones (default 20)
//!   s18 priority            B3: 5,000 queued, 100 made urgent, 4,000 cancelled
//!   s18 startup             B4: a helper's start and its idle memory
//!   s18 --helper            the helper itself (started by the others, never by hand)
//! A spike: it dies with its verdict.
mod helper;
mod pool;
mod proto;
#[cfg(target_os = "macos")]
#[path = "sys_mac.rs"]
mod sys;
#[cfg(windows)]
#[path = "sys_win.rs"]
mod sys;
#[cfg(not(any(target_os = "macos", windows)))]
mod sys {
    use crate::helper::Img;
    type R = Result<Img, (u8, i64)>;
    pub fn thumbnail(_: &str, _: u32, _: std::time::Duration, _: bool) -> R { Err((crate::proto::FAILED, 0)) }
    pub fn icon(_: &str, _: u32) -> R { Err((crate::proto::FAILED, 0)) }
    pub fn type_icon(_: &str, _: u32) -> R { Err((crate::proto::FAILED, 0)) }
}

use pool::{Done, Helper, Job, Pool};
use proto::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const N_FILES: usize = 2000;

fn root() -> PathBuf { PathBuf::from(std::env::var("S18_FILES").unwrap_or_else(|_| "target/s18-files".into())) }
fn ms(d: Duration) -> f64 { d.as_secs_f64() * 1e3 }
fn mean(v: &[f64]) -> f64 { if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 } }
fn pct(v: &[f64], p: f64) -> f64 { if v.is_empty() { return 0.0 } let mut s = v.to_vec(); s.sort_by(|a, b| a.partial_cmp(b).unwrap()); s[((s.len() - 1) as f64 * p).round() as usize] }
fn max(v: &[f64]) -> f64 { v.iter().cloned().fold(0.0, f64::max) }

/// (resident bytes, physical footprint bytes) of a process.
#[cfg(target_os = "macos")]
fn mem(pid: u32) -> (u64, u64) {
    unsafe { let mut ri: libc::rusage_info_v2 = std::mem::zeroed();
        if libc::proc_pid_rusage(pid as i32, libc::RUSAGE_INFO_V2, &mut ri as *mut _ as *mut libc::rusage_info_t) == 0 { (ri.ri_resident_size, ri.ri_phys_footprint) } else { (0, 0) } }
}
#[cfg(windows)]
fn mem(pid: u32) -> (u64, u64) {
    // UNTESTED: (working set, private bytes)
    use windows::Win32::{Foundation::CloseHandle, System::{ProcessStatus::*, Threading::*}};
    unsafe { let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return (0, 0) };
        let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
        let ok = GetProcessMemoryInfo(h, &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32).is_ok();
        let _ = CloseHandle(h); if ok { (c.WorkingSetSize as u64, c.PrivateUsage as u64) } else { (0, 0) } }
}
#[cfg(not(any(target_os = "macos", windows)))]
fn mem(_: u32) -> (u64, u64) { (0, 0) }
fn mb(b: u64) -> f64 { b as f64 / (1 << 20) as f64 }

fn alive(pid: u32) -> bool {
    #[cfg(unix)] { unsafe { libc::kill(pid as i32, 0) == 0 } }
    #[cfg(not(unix))] { mem(pid).0 != 0 }
}

// ---------------------------------------------------------------------------------------------------------- files

fn synth(seed: u32, w: u32, h: u32) -> image::RgbImage {
    let mut s = seed.wrapping_mul(2654435761).wrapping_add(12345);
    let (a, b, c) = (3 + seed % 5, 2 + (seed / 5) % 7, seed % 251);
    image::RgbImage::from_fn(w, h, |x, y| {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let n = s >> 30; // a little grain
        let band = (((x * a + y * b) / 37) % 2) * 24;
        image::Rgb([((x * 255 / w) + n + band).min(255) as u8, ((y * 255 / h) + c / 4 + n).min(255) as u8, (((x + y) * 255 / (w + h)) + c / 2 + band).min(255) as u8])
    })
}
fn save_jpeg(img: &image::RgbImage, p: &Path) {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88).encode(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8).unwrap();
    std::fs::write(p, out).unwrap();
}
fn save_png(img: &image::RgbImage, p: &Path) {
    use image::ImageEncoder;
    let f = std::io::BufWriter::new(std::fs::File::create(p).unwrap());
    image::codecs::png::PngEncoder::new_with_quality(f, image::codecs::png::CompressionType::Fast, image::codecs::png::FilterType::Sub)
        .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8).unwrap();
}
/// A one-page PDF with a line of text, written by hand.
fn save_pdf(p: &Path, seed: u32) {
    let stream = format!("BT /F1 36 Tf 72 700 Td (S18 spike {seed}) Tj ET\n0.2 0.5 0.8 rg 72 300 400 300 re f\n");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}endstream", stream.len(), stream),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut out = String::from("%PDF-1.4\n"); let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() { offs.push(out.len()); out += &format!("{} 0 obj\n{}\nendobj\n", i + 1, o); }
    let xref = out.len();
    out += &format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1);
    for o in offs { out += &format!("{:010} 00000 n \n", o); }
    out += &format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", objs.len() + 1, xref);
    std::fs::write(p, out).unwrap();
}

fn jpg_files() -> Vec<String> {
    let d = root().join("jpg");
    let mut v: Vec<String> = std::fs::read_dir(&d).map(|r| r.filter_map(|e| e.ok()).map(|e| e.path().to_string_lossy().into_owned()).filter(|p| p.ends_with(".jpg")).collect()).unwrap_or_default();
    v.sort();
    if v.len() < N_FILES { eprintln!("run `s18 gen` first ({} of {N_FILES} files in {})", v.len(), d.display()); std::process::exit(2) }
    v
}

fn gen_all() {
    let d = root().join("jpg");
    std::fs::create_dir_all(&d).unwrap();
    let t = Instant::now();
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    std::thread::scope(|s| { for _ in 0..threads { s.spawn(|| loop {
        let i = next.fetch_add(1, Ordering::Relaxed); if i >= N_FILES { break }
        let p = d.join(format!("{i:04}.jpg")); if !p.exists() { save_jpeg(&synth(i as u32, 2048, 2048), &p) }
    }); } });
    let bytes: u64 = std::fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum();
    std::fs::write(root().join("abort.poison"), b"this file makes the helper abort").unwrap();
    std::fs::write(root().join("hang.poison"), b"this file makes the helper loop for ever").unwrap();
    println!("gen: {N_FILES} JPEGs 2048x2048 in {:.1} s, {:.0} MiB in all ({:.0} KiB each), in {}", t.elapsed().as_secs_f64(), mb(bytes), bytes as f64 / N_FILES as f64 / 1024.0, d.display());
}

// ---------------------------------------------------------------------------------------------- A1, A2: the system

fn one(pool: &Pool, id: &mut u64, op: u8, path: &str, px: u16, soft_ms: u32, hard_ms: u32) -> Done {
    *id += 1;
    pool.submit(Job { req: Request { id: *id, op, fmt: FMT_RGBA, size: px, soft_ms, path: path.into() }, hard_ms }, true);
    pool.rx.recv().unwrap()
}
/// The share of pixels that are not fully transparent, and whether any colour is there at all.
fn coverage(d: &Done) -> f64 { if d.payload.is_empty() { 0.0 } else { d.payload.chunks_exact(4).filter(|p| p[3] != 0).count() as f64 / (d.payload.len() / 4) as f64 } }

fn sys_thumbs() {
    // fresh files with fresh names every run: the system's own thumbnail cache has never seen them (cold)
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    let d = root().join("sys"); let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(&d).unwrap();
    let out = root().join("out"); let _ = std::fs::remove_dir_all(&out); std::fs::create_dir_all(&out).unwrap();
    let abs = |p: PathBuf| std::fs::canonicalize(&p).unwrap().to_string_lossy().into_owned();
    let img = synth(seed, 1600, 1200);
    save_png(&img, &d.join(format!("picture-{seed}.png")));
    save_jpeg(&img, &d.join(format!("photo-{seed}.jpg")));
    save_pdf(&d.join(format!("page-{seed}.pdf")), seed);
    std::fs::write(d.join(format!("notes-{seed}.txt")), format!("S18 spike {seed}\n").repeat(40)).unwrap();
    std::fs::create_dir_all(d.join(format!("folder-{seed}"))).unwrap();
    std::fs::write(d.join(format!("folder-{seed}")).join("inside.txt"), "x").unwrap();
    let big = synth(seed ^ 1, 9000, 9000);
    save_png(&big, &d.join(format!("big-a-{seed}.png"))); save_png(&synth(seed ^ 2, 9000, 9000), &d.join(format!("big-b-{seed}.png"))); drop(big);
    let app = ["/System/Applications/Calculator.app", "/Applications/Safari.app"].iter().map(|s| s.to_string()).find(|p| Path::new(p).exists())
        .or_else(|| std::fs::read_dir("/System/Applications").ok()?.filter_map(|e| e.ok()).map(|e| e.path().to_string_lossy().into_owned()).find(|p| p.ends_with(".app")));
    let mut items: Vec<(&str, String)> = vec![
        ("png", abs(d.join(format!("picture-{seed}.png")))), ("jpeg", abs(d.join(format!("photo-{seed}.jpg")))), ("pdf", abs(d.join(format!("page-{seed}.pdf")))),
        ("text", abs(d.join(format!("notes-{seed}.txt")))), ("folder", abs(d.join(format!("folder-{seed}")))) ];
    if let Some(a) = app { items.push(("app bundle", a)) }

    let pool = Pool::new(1, std::env::var_os("S18_VERBOSE").is_none());
    let pid0 = pool.pids[0];
    println!("sys-thumbs: every call below is made in the helper (pid {pid0}): no window, no event loop, no run loop, no NSApplication");
    println!("helper start {:.1} ms; memory before any system call: rss {:.1} MiB, footprint {:.1} MiB", pool.startup_ms[0], mb(mem(pid0).0), mb(mem(pid0).1));
    let mut id = 0u64;
    println!("\n{:<11} {:<22} {:>4}  {:<5} {:<8} {:>9} {:>9} {:>8}", "item", "call", "px", "pass", "result", "pixels", "ms", "covered");
    for pass in ["cold", "warm"] {
        for (kind, path) in &items {
            for (op, name) in [(OP_THUMB, "thumbnail (QuickLook)"), (OP_ICON, "icon (NSWorkspace)"), (OP_QL_ICON, "icon (QuickLook)")] {
                for px in [256u16, 64] {
                    let r = one(&pool, &mut id, op, path, px, 10_000, 15_000);
                    let res = if r.status == OK { "ok".to_string() } else { format!("{} {}", if r.status == FAILED { "none" } else { status_name(r.status) }, r.aux as i32) };
                    println!("{:<11} {:<22} {:>4}  {:<5} {:<8} {:>9} {:>9.2} {:>7.0}%", kind, name, px, pass, res, format!("{}x{}", r.w, r.h), r.rt_us as f64 / 1e3, coverage(&r) * 100.0);
                    if pass == "cold" && r.status == OK { let _ = image::save_buffer(out.join(format!("{}-{}-{px}.png", kind.replace(' ', "-"), op)), &r.payload, r.w as u32, r.h as u32, image::ExtendedColorType::Rgba8); }
                }
            }
        }
    }
    println!("\nicons per TYPE (from the extension alone, no file):");
    for pass in ["cold", "warm"] { for ext in ["png", "jpg", "pdf", "txt", "mov", "zip", "exr", "s18unknown", "/"] { for px in [256u16, 64] {
        let r = one(&pool, &mut id, OP_TYPE_ICON, ext, px, 10_000, 15_000);
        println!("{:<11} {:<22} {:>4}  {:<5} {:<8} {:>9} {:>9.2} {:>7.0}%", ext, "type icon", px, pass, status_name(r.status), format!("{}x{}", r.w, r.h), r.rt_us as f64 / 1e3, coverage(&r) * 100.0);
        if pass == "cold" && px == 256 && r.status == OK { let _ = image::save_buffer(out.join(format!("type-{}-{px}.png", ext.replace('/', "folder"))), &r.payload, r.w as u32, r.h as u32, image::ExtendedColorType::Rgba8); }
    } } }

    println!("\ncancelling a slow request (a 9000 x 9000 PNG the system has never seen):");
    let a = abs(d.join(format!("big-a-{seed}.png"))); let b = abs(d.join(format!("big-b-{seed}.png")));
    let r = one(&pool, &mut id, OP_THUMB, &b, 256, 30_000, 40_000);
    println!("  left alone (the other big file):   {} after {:.0} ms", status_name(r.status), r.rt_us as f64 / 1e3);
    let r = one(&pool, &mut id, OP_THUMB, &a, 256, 50, 5_000);
    println!("  soft bound 50 ms:                  {} after {:.1} ms; {}", status_name(r.status), r.rt_us as f64 / 1e3,
        if r.aux == u32::MAX { "cancelRequest was NOT honoured within 100 ms: the helper stopped waiting and dropped the late answer".to_string() } else { format!("the system called the handler {:.2} ms after cancelRequest", r.aux as f64 / 1e3) });
    let r = one(&pool, &mut id, OP_THUMB, &items[0].1, 256, 10_000, 15_000);
    println!("  the next request on the same helper: {} in {:.2} ms; helper pid still {} ({})", status_name(r.status), r.rt_us as f64 / 1e3, pool.live_pids()[0], if pool.live_pids()[0] == pid0 { "never replaced" } else { "REPLACED" });
    let r = one(&pool, &mut id, OP_THUMB, &a, 256, 30_000, 40_000);
    println!("  the cancelled file asked again:    {} after {:.0} ms (the cancel left nothing broken)", status_name(r.status), r.rt_us as f64 / 1e3);
    let (rss, fp) = mem(pid0);
    println!("\nhelper memory after all of it: rss {:.1} MiB, footprint {:.1} MiB; {} requests; pictures written to {}", mb(rss), mb(fp), id, out.display());
}

// ------------------------------------------------------------------------------------------------- B1: throughput

fn throughput(ns: &[usize]) {
    let files = jpg_files();
    // warm the file cache, so that the numbers are the decoder's and the pipe's, not the disk's
    for f in &files { let _ = std::fs::read(f); }
    println!("throughput: {N_FILES} JPEGs 2048x2048 -> 512 px; the answer comes back as RGBA (1,048,576 bytes) or as a JPEG");
    println!("{:<22} {:>3} {:>8} {:>9} {:>10} {:>10} {:>10} {:>9} {:>8} {:>11}", "mode", "n", "items/s", "x of n=1", "decode ms", "encode ms", "ipc ms", "ipc p95", "ipc %", "bytes/item");
    let mut base = [0.0f64; 3];
    for &n in ns {
        // in-process threads: the same work with no process boundary (what the helpers are compared against)
        let t = Instant::now(); let next = AtomicUsize::new(0);
        std::thread::scope(|s| { for _ in 0..n { s.spawn(|| loop { let i = next.fetch_add(1, Ordering::Relaxed); if i >= files.len() { break } std::hint::black_box(helper::decode(&files[i], 512, FMT_RGBA).unwrap()); }); } });
        let ips = N_FILES as f64 / t.elapsed().as_secs_f64(); if base[2] == 0.0 { base[2] = ips }
        println!("{:<22} {:>3} {:>8.1} {:>9.2} {:>10} {:>10} {:>10} {:>9} {:>8} {:>11}", "threads, no helpers", n, ips, ips / base[2], "-", "-", "-", "-", "-", "-");
        for (k, (fmt, name)) in [(FMT_RGBA, "helpers, RGBA back"), (FMT_JPEG, "helpers, JPEG back")].into_iter().enumerate() {
            let pool = Pool::new(n, true);
            let t = Instant::now();
            pool.submit_many(files.iter().enumerate().map(|(i, f)| Job { req: Request { id: i as u64 + 1, op: OP_DECODE, fmt, size: 512, soft_ms: 0, path: f.clone() }, hard_ms: 30_000 }));
            let (mut work, mut enc, mut ipc, mut bytes, mut bad) = (Vec::new(), Vec::new(), Vec::new(), 0u64, 0);
            for _ in 0..N_FILES {
                let d = pool.rx.recv().unwrap();
                if d.status != OK { bad += 1; continue }
                work.push(d.work_us as f64 / 1e3); enc.push(d.aux as f64 / 1e3);
                ipc.push((d.rt_us as f64 - d.work_us as f64 - d.aux as f64) / 1e3);
                bytes += d.payload.len() as u64 + 28 + 20 + files[0].len() as u64;
            }
            let ips = N_FILES as f64 / t.elapsed().as_secs_f64(); if base[k] == 0.0 { base[k] = ips }
            println!("{:<22} {:>3} {:>8.1} {:>9.2} {:>10.2} {:>10.2} {:>10.3} {:>9.3} {:>7.2}% {:>11.0}{}", name, n, ips, ips / base[k], mean(&work), mean(&enc), mean(&ipc), pct(&ipc, 0.95),
                100.0 * mean(&ipc) / (mean(&work) + mean(&enc)), bytes as f64 / N_FILES as f64, if bad > 0 { format!("  ({bad} FAILED)") } else { String::new() });
        }
    }
    println!("ipc = the parent's round trip (request written -> answer fully read) minus the helper's own decode and encode time");
}

// -------------------------------------------------------------------------------------------- B2: crash isolation

fn poison(rounds: usize) {
    const HANG_BOUND_MS: u32 = 5_000;
    let files = jpg_files();
    let (abort, hang) = (root().join("abort.poison").to_string_lossy().into_owned(), root().join("hang.poison").to_string_lossy().into_owned());
    let pool = Pool::new(4, true);
    let me = std::process::id();
    let mut id = 0u64;
    let mut round = |with_poison: bool| -> (usize, Option<Done>, Option<Done>, f64) {
        let t = Instant::now(); let mut jobs = Vec::new();
        for i in 0..40 {
            if with_poison && i == 20 { id += 1; jobs.push(Job { req: Request { id: id | 1 << 40, op: OP_DECODE, fmt: FMT_RGBA, size: 512, soft_ms: 0, path: abort.clone() }, hard_ms: HANG_BOUND_MS }); }
            if with_poison && i == 30 { id += 1; jobs.push(Job { req: Request { id: id | 2 << 40, op: OP_DECODE, fmt: FMT_RGBA, size: 512, soft_ms: 0, path: hang.clone() }, hard_ms: HANG_BOUND_MS }); }
            id += 1; jobs.push(Job { req: Request { id, op: OP_DECODE, fmt: FMT_RGBA, size: 512, soft_ms: 0, path: files[(id as usize * 7) % files.len()].clone() }, hard_ms: HANG_BOUND_MS });
        }
        let n = jobs.len(); pool.submit_many(jobs.into_iter());
        let (mut good, mut a, mut h) = (0, None, None);
        for _ in 0..n { let d = pool.rx.recv().unwrap(); match d.id >> 40 { 1 => a = Some(d), 2 => h = Some(d), _ => if d.status == OK && d.payload.len() == 512 * 512 * 4 { good += 1 } } }
        (good, a, h, ms(t.elapsed()))
    };
    for _ in 0..3 { round(false); } // the parent's memory settles
    let m0 = mem(me); let first = pool.live_pids();
    println!("poison: 4 helpers; each round = 40 good files + one that aborts the helper + one that hangs it (bound {HANG_BOUND_MS} ms)");
    println!("{:>5} {:>6} {:>22} {:>24} {:>20} {:>10} {:>6}", "round", "good", "abort: failure after", "hang: failure after", "replaced in (ms)", "round ms", "pass");
    let (mut passes, mut ta, mut th, mut rep) = (0, Vec::new(), Vec::new(), Vec::new());
    for r in 1..=rounds {
        let (good, a, h, wall) = round(true);
        // the hung helper's replacement is started after its failure is reported: give it its 500 ms
        let t = Instant::now(); let mut rp = pool.replace_ms(); while rp.len() < 2 && t.elapsed() < Duration::from_millis(1500) { std::thread::sleep(Duration::from_millis(5)); rp.extend(pool.replace_ms()); }
        let (a, h) = (a.unwrap(), h.unwrap());
        let ok = good == 40 && a.status == CRASHED && h.status == HUNG && rp.len() == 2 && max(&rp) < 500.0 && (h.rt_us as f64) < (HANG_BOUND_MS as f64 + 500.0) * 1e3 && pool.live_pids().len() == 4;
        println!("{:>5} {:>6} {:>12} {:>7.1} ms {:>12} {:>9.1} ms {:>20} {:>10.0} {:>6}", r, good, status_name(a.status), a.rt_us as f64 / 1e3, status_name(h.status), h.rt_us as f64 / 1e3,
            rp.iter().map(|x| format!("{x:.1}")).collect::<Vec<_>>().join(", "), wall, if ok { "yes" } else { "NO" });
        if ok { passes += 1 } ta.push(a.rt_us as f64 / 1e3); th.push(h.rt_us as f64 / 1e3); rep.extend(rp);
    }
    for _ in 0..3 { round(false); }
    let m1 = mem(me); let last = pool.live_pids();
    println!("passed {passes} of {rounds}; abort seen after mean {:.1} / max {:.1} ms; hang ended after mean {:.0} / max {:.0} ms; replacement mean {:.1} / max {:.1} ms ({} replacements)",
        mean(&ta), max(&ta), mean(&th), max(&th), mean(&rep), max(&rep), rep.len());
    println!("parent memory: rss {:.1} -> {:.1} MiB, footprint {:.1} -> {:.1} MiB; helpers alive: {} (of the first four, still alive: {})",
        mb(m0.0), mb(m1.0), mb(m0.1), mb(m1.1), last.iter().filter(|p| alive(**p)).count(), first.iter().filter(|p| last.contains(p)).count());
}

// -------------------------------------------------------------------------------------- B3: priority, cancellation

fn priority() {
    let files = jpg_files();
    let pool = Pool::new(4, true);
    let me = std::process::id();
    // warm up, so that memory numbers are not the allocator's first growth
    pool.submit_many((0..64).map(|i| Job { req: Request { id: 1_000_000 + i, op: OP_DECODE, fmt: FMT_RGBA, size: 512, soft_ms: 0, path: files[i as usize].clone() }, hard_ms: 30_000 }));
    for _ in 0..64 { pool.rx.recv().unwrap(); }
    let m0 = mem(me);
    let jobs: Vec<Job> = (0..5000u64).map(|i| Job { req: Request { id: i, op: OP_DECODE, fmt: FMT_RGBA, size: 512, soft_ms: 0, path: files[i as usize % files.len()].clone() }, hard_ms: 30_000 }).collect();
    let t0 = Instant::now();
    pool.submit_many(jobs.into_iter());
    let t_submit = t0.elapsed();
    let m1 = mem(me);
    std::thread::sleep(Duration::from_millis(200)); // the queue is being worked on, in order, when the urgent ones arrive
    let urgent: Vec<u64> = (4900..5000).collect();
    let t = Instant::now(); pool.promote(&urgent); let t_promote = t.elapsed();
    let before = { let mut n = 0; while pool.rx.try_recv().is_ok() { n += 1 } n }; // done before the promotion
    let tu = Instant::now();
    let (mut got_urgent, mut others, mut first_ms, mut done) = (0, 0, 0.0, before);
    while got_urgent < 100 {
        let d = pool.rx.recv().unwrap(); done += 1;
        if d.id >= 4900 { got_urgent += 1; if got_urgent == 1 { first_ms = ms(tu.elapsed()) } } else { others += 1 }
    }
    let all_ms = ms(tu.elapsed());
    println!("priority: 5,000 queued in {:.2} ms (parent footprint {:.1} -> {:.1} MiB); {before} were done when 100 at the END of the queue were marked urgent ({:.0} us)", ms(t_submit), mb(m0.1), mb(m1.1), t_promote.as_secs_f64() * 1e6);
    println!("  first urgent answer after {first_ms:.1} ms, all 100 after {all_ms:.0} ms; answers that were not urgent in between: {others} (at most the 4 in flight) -> {}", if others <= 4 { "PASS" } else { "FAIL" });
    // cancel 4,000 of the queued
    let cancel: Vec<u64> = (900..4900).collect();
    let (jobs_before, ids_before) = pool.queued();
    let m2 = mem(me);
    let t = Instant::now(); let freed = pool.cancel(&cancel); let t_cancel = t.elapsed();
    let (jobs_after, ids_after) = pool.queued();
    let m3 = mem(me);
    println!("  cancel of 4,000 queued: {freed} freed in {:.0} us ({:.0} ns each); queued requests {jobs_before} -> {jobs_after}, ids held {ids_before} -> {ids_after}; parent footprint {:.2} -> {:.2} MiB",
        t_cancel.as_secs_f64() * 1e6, t_cancel.as_secs_f64() * 1e9 / 4000.0, mb(m2.1), mb(m3.1));
    let t = Instant::now(); let (mut rest, mut wrong) = (0, 0);
    while done < 5000 - freed { let d = pool.rx.recv().unwrap(); done += 1; rest += 1; if (900..4900).contains(&d.id) { wrong += 1 } }
    let extra = pool.rx.recv_timeout(Duration::from_millis(300)).is_ok();
    println!("  the {rest} left were answered in {:.0} ms; answers for cancelled requests: {wrong}; anything after the last: {} -> {}", ms(t.elapsed()), if extra { "YES" } else { "no" }, if wrong == 0 && !extra && freed == 4000 { "PASS" } else { "FAIL" });
}

// ---------------------------------------------------------------------------------------- B4: start and idle cost

fn startup() {
    let files = jpg_files();
    let (mut hello, mut ping, mut first) = (Vec::new(), Vec::new(), Vec::new());
    for i in 0..20 {
        let t = Instant::now();
        let mut h = Helper::spawn(true).unwrap();
        hello.push(h.startup_ms);
        write_request(&mut h.stdin, &Request { id: 1, op: OP_PING, fmt: 0, size: 0, soft_ms: 0, path: String::new() }).unwrap(); read_response(&mut h.stdout).unwrap();
        ping.push(ms(t.elapsed()));
        write_request(&mut h.stdin, &Request { id: 2, op: OP_DECODE, fmt: FMT_RGBA, size: 512, soft_ms: 0, path: files[i].clone() }).unwrap();
        let r = read_response(&mut h.stdout).unwrap();
        first.push(ms(t.elapsed()) - r.work_us as f64 / 1e3);
        h.end();
    }
    println!("startup, 20 helpers one after another (ms): spawn -> hello mean {:.2} p95 {:.2} max {:.2}; spawn -> first answer (a ping) mean {:.2}; spawn -> first decoded file, minus the decode itself: mean {:.2} max {:.2}",
        mean(&hello), pct(&hello, 0.95), max(&hello), mean(&ping), mean(&first), max(&first));
    let t = Instant::now(); let pool = Pool::new(8, true);
    println!("8 helpers started together: all ready after {:.1} ms", ms(t.elapsed()));
    let show = |what: &str| { std::thread::sleep(Duration::from_millis(700)); let m: Vec<(u64, u64)> = pool.live_pids().iter().map(|p| mem(*p)).collect();
        println!("  memory per helper, {what}: rss mean {:.1} MiB, footprint mean {:.1} / max {:.1} MiB", mean(&m.iter().map(|x| mb(x.0)).collect::<Vec<_>>()), mean(&m.iter().map(|x| mb(x.1)).collect::<Vec<_>>()), max(&m.iter().map(|x| mb(x.1)).collect::<Vec<_>>())); };
    show("idle, nothing done yet");
    pool.submit_many((0..400u64).map(|i| Job { req: Request { id: i, op: OP_DECODE, fmt: FMT_RGBA, size: 512, soft_ms: 0, path: files[i as usize].clone() }, hard_ms: 30_000 }));
    for _ in 0..400 { pool.rx.recv().unwrap(); }
    show("idle, after 50 decodes each");
    let f = std::fs::canonicalize(&files[0]).unwrap().to_string_lossy().into_owned();
    pool.submit_many((0..160u64).map(|i| Job { req: Request { id: 1000 + i, op: if i % 2 == 0 { OP_ICON } else { OP_THUMB }, fmt: FMT_RGBA, size: 256, soft_ms: 10_000, path: f.clone() }, hard_ms: 15_000 }));
    for _ in 0..160 { pool.rx.recv().unwrap(); }
    show("idle, after system thumbnails and icons too");
    println!("  parent: rss {:.1} MiB, footprint {:.1} MiB", mb(mem(std::process::id()).0), mb(mem(std::process::id()).1));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("--helper") => helper::run(),
        Some("gen") => gen_all(),
        Some("sys-thumbs") => sys_thumbs(),
        Some("throughput") => { let ns: Vec<usize> = args[1..].iter().filter_map(|a| a.parse().ok()).collect(); throughput(if ns.is_empty() { &[1, 2, 4, 8] } else { &ns }) }
        Some("poison") => poison(args.get(1).and_then(|a| a.parse().ok()).unwrap_or(20)),
        Some("priority") => priority(),
        Some("startup") => startup(),
        // probe <op> <path>: one helper, one request, then the helper is kept alive for two seconds (for `lsappinfo`)
        Some("probe") => { let pool = Pool::new(1, true); let mut id = 0; let r = one(&pool, &mut id, args[1].parse().unwrap(), &args[2], 256, 5_000, 8_000);
            println!("helper {} {}", pool.pids[0], status_name(r.status)); std::thread::sleep(Duration::from_secs(2)); }
        _ => { eprintln!("s18 gen | sys-thumbs | throughput [n ...] | poison [rounds] | priority | startup"); std::process::exit(2) }
    }
}
