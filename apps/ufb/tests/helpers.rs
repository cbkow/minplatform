//! The helper copies of the executable, as the application uses them (canon row 456; spike S18): the real binary is
//! started with `--thumb-helper` and spoken to through the application's own pool.
#[allow(dead_code)] #[path = "../src/thumbs/proto.rs"] mod proto;
#[path = "../src/thumbs/pool.rs"] mod pool;
use pool::{Done, Job, Pool};
use proto::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn make(n: usize, idle_ms: u64) -> Pool { Pool::new(PathBuf::from(env!("CARGO_BIN_EXE_UFB")), vec![("UFB_HELPER_TEST".into(), "1".into())], n, Duration::from_millis(idle_ms), Arc::new(|| {})) }
fn job(id: u64, op: u8, path: &str, hard_ms: u32) -> Job { Job { req: Request::new(id, op, 256, 5000, path), hard_ms } }
fn next(p: &Pool) -> Done { p.rx.recv_timeout(Duration::from_secs(30)).expect("an answer") }
struct Temp(PathBuf);
impl Temp { fn new(tag: &str) -> Temp { let p = std::env::temp_dir().join(format!("ufb-helpers-{tag}-{}", std::process::id())); let _ = std::fs::remove_dir_all(&p); std::fs::create_dir_all(&p).unwrap(); Temp(p) } fn path(&self, name: &str) -> String { self.0.join(name).to_string_lossy().into_owned() } }
impl Drop for Temp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
/// 400 x 200: the left half opaque red, the right half clear.
fn picture(path: &str) { let mut px = vec![0u8; 400 * 200 * 4]; for y in 0..200 { for x in 0..200 { let i = (y * 400 + x) * 4; px[i] = 255; px[i + 3] = 255; } } min_image::write::png_rgba8(path, &px, 400, 200).unwrap(); }

#[test]
fn our_decoder_answers_a_fitted_picture_with_its_transparency() {
    let t = Temp::new("decode"); let file = t.path("half.png"); picture(&file);
    let p = make(2, 30_000); assert_eq!(p.counts(), (0, 0, 0), "no helper before the first request");
    p.submit(job(1, OP_DECODE, &file, 20_000)); let d = next(&p);
    assert_eq!((d.id, d.status, d.w, d.h), (1, OK, 256, 128)); assert_eq!(d.payload.len(), 256 * 128 * 4);
    assert_eq!(&d.payload[..4], [255, 0, 0, 255], "opaque red"); assert_eq!(&d.payload[255 * 4..256 * 4], [0, 0, 0, 0], "clear: colour multiplied by its alpha");
    assert_eq!(p.counts().2, 1, "one request, one helper");
    // a Radiance picture (linear light): 3 x 2 of one colour, 1.0 / 0.5 / 0.0 - shown clipped, with the sRGB curve
    let mut hdr = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 2 +X 3\n".to_vec(); for _ in 0..6 { hdr.extend_from_slice(&[128, 64, 0, 129]); } std::fs::write(t.path("light.hdr"), hdr).unwrap();
    p.submit(job(9, OP_DECODE, &t.path("light.hdr"), 20_000)); let d = next(&p); assert_eq!((d.status, d.w, d.h), (OK, 3, 2)); assert_eq!(&d.payload[..4], [255, 188, 0, 255]);
    // a file that is no film: the player says so, and its helper lives
    std::fs::write(t.path("noise.mov"), vec![7u8; 5000]).unwrap();
    p.submit(job(10, OP_VIDEO, &t.path("noise.mov"), 20_000)); assert_eq!(next(&p).status, FAILED);
    // a file that is no picture, and one that is not there: an answer, not a death
    std::fs::write(t.path("noise.exr"), vec![7u8; 5000]).unwrap();
    p.submit(job(2, OP_DECODE, &t.path("noise.exr"), 20_000)); p.submit(job(3, OP_DECODE, &t.path("gone.tif"), 20_000));
    let mut got = [next(&p), next(&p)].map(|d| (d.id, d.status)); got.sort(); assert_eq!(got, [(2, FAILED), (3, FAILED)]);
}

#[test]
fn a_helper_that_dies_or_hangs_costs_its_own_request_only() {
    let p = make(1, 30_000);
    p.submit(job(1, OP_PING, "", 5000)); assert_eq!(next(&p).status, OK);
    p.submit(job(2, OP_TEST_ABORT, "", 5000)); p.submit(job(3, OP_PING, "", 5000));
    let d = next(&p); assert_eq!((d.id, d.status), (2, CRASHED)); let d = next(&p); assert_eq!((d.id, d.status), (3, OK), "the next request has a new helper");
    let t0 = Instant::now(); p.submit(job(4, OP_TEST_HANG, "", 400)); p.submit(job(5, OP_PING, "", 5000));
    let d = next(&p); assert_eq!((d.id, d.status), (4, HUNG)); let took = t0.elapsed(); assert!(took >= Duration::from_millis(390) && took < Duration::from_secs(5), "ended at its bound: {took:?}");
    let d = next(&p); assert_eq!((d.id, d.status), (5, OK)); assert_eq!(p.counts().2, 3, "three helpers were started in all");
}

#[test]
fn what_waits_can_be_called_off_and_an_idle_helper_is_ended() {
    let p = make(1, 300);
    p.submit(job(1, OP_TEST_HANG, "", 600)); std::thread::sleep(Duration::from_millis(150));
    for id in 2..=6 { p.submit(job(id, OP_PING, "", 5000)); }
    assert_eq!(p.cancel(&[1, 2, 3, 4, 5]), [2, 3, 4, 5], "the one with the helper is not freed; the waiting ones are"); assert_eq!(p.counts().0, 1);
    let d = next(&p); assert_eq!((d.id, d.status), (1, HUNG)); let d = next(&p); assert_eq!((d.id, d.status), (6, OK));
    assert!(p.rx.recv_timeout(Duration::from_millis(200)).is_err(), "nothing comes for what was called off");
    assert_eq!(p.counts().1, 1); std::thread::sleep(Duration::from_millis(900)); assert_eq!(p.counts().1, 0, "no work for a while: no helper");
    p.submit(job(7, OP_PING, "", 5000)); assert_eq!(next(&p).status, OK, "and the next request starts one again");
}

#[test]
fn no_helper_is_an_answer_too() {
    let p = Pool::new(PathBuf::from("/nowhere/UFB-not-there"), vec![], 1, Duration::from_secs(30), Arc::new(|| {}));
    p.submit(job(1, OP_PING, "", 5000)); assert_eq!(next(&p).status, NO_HELPER);
}

#[cfg(target_os = "macos")]
#[test]
fn the_system_draws_what_we_do_not() {
    let t = Temp::new("system"); let file = t.path("half.png"); picture(&file); std::fs::write(t.path("noise.jpg"), vec![7u8; 5000]).unwrap();
    let p = make(1, 30_000);
    p.submit(job(1, OP_THUMB, &file, 12_000)); let d = next(&p); assert_eq!((d.status, d.w, d.h), (OK, 256, 128)); assert_eq!(d.payload.len(), 256 * 128 * 4);
    assert!(d.payload[0] > 200 && d.payload[1] < 60 && d.payload[3] == 255, "red, as the file has it: {:?}", &d.payload[..4]);
    p.submit(job(2, OP_THUMB, &t.path("noise.jpg"), 12_000)); assert_eq!(next(&p).status, FAILED, "no thumbnail: said, and the type's icon stays");
    p.submit(job(3, OP_ICON, "/System/Applications/Calculator.app", 12_000)); let d = next(&p); assert_eq!(d.status, OK, "an application's own icon"); assert!(d.w > 0 && d.w <= 256);
}

/// The cache on disk, as the application uses it: asked first, filled by the request that makes the picture.
#[test]
fn a_picture_is_made_once_and_then_comes_from_the_cache() {
    let t = Temp::new("cache"); let file = t.path("half.png"); picture(&file); std::fs::write(t.path("noise.exr"), vec![7u8; 5000]).unwrap();
    let db = t.path("db");
    let pool = || Pool::new(PathBuf::from(env!("CARGO_BIN_EXE_UFB")), vec![("UFB_HELPER_TEST".into(), "1".into()), ("UFB_THUMB_CACHE".into(), db.clone())], 2, Duration::from_secs(30), Arc::new(|| {}));
    let ask = |id: u64, op: u8, flags: u8, px: u16, path: &str, mtime: i64| Job { req: Request { id, op, flags, size: px, soft_ms: 5000, fsize: 77, mtime, path: path.into() }, hard_ms: 20_000 };
    let p = pool();
    p.submit(ask(1, OP_CACHED, 0, 256, &file, 1000)); assert_eq!(next(&p).status, MISS, "nothing kept yet");
    p.submit(ask(2, OP_DECODE, KEEP, 256, &file, 1000)); let made = next(&p); assert_eq!((made.status, made.w, made.h), (OK, 256, 128), "answered at the size asked");
    p.submit(ask(3, OP_CACHED, 0, 256, &file, 1000)); let kept = next(&p); assert_eq!((kept.status, kept.w, kept.h), (OK, 256, 128)); assert_eq!(kept.payload, made.payload, "the same picture, exactly (transparency: lossless)");
    // the master serves another size; a changed file is not this picture's
    p.submit(ask(4, OP_CACHED, 0, 512, &file, 1000)); let big = next(&p); assert_eq!((big.status, big.w, big.h), (OK, 400, 200), "the kept master, never enlarged");
    p.submit(ask(5, OP_CACHED, 0, 256, &file, 1001)); assert_eq!(next(&p).status, MISS);
    // a file with no picture: remembered, forgotten by the folder's refresh
    p.submit(ask(6, OP_MARK, 0, 0, &t.path("noise.exr"), 5)); assert_eq!(next(&p).status, OK);
    p.submit(ask(7, OP_CACHED, 0, 256, &t.path("noise.exr"), 5)); assert_eq!(next(&p).status, NONE);
    drop(p);
    // another run of the application: other helpers, the same database
    let p = pool();
    p.submit(ask(8, OP_CACHED, 0, 256, &file, 1000)); let again = next(&p); assert_eq!(again.status, OK); assert_eq!(again.payload, made.payload);
    p.submit(ask(9, OP_FORGET, 0, 0, &t.0.to_string_lossy(), 0)); assert_eq!(next(&p).status, OK);
    p.submit(ask(10, OP_CACHED, 0, 256, &t.path("noise.exr"), 5)); assert_eq!(next(&p).status, MISS, "asked again after a refresh");
    // the file goes: what was kept for it goes with the next reading of its folder
    std::fs::remove_file(&file).unwrap();
    p.submit(ask(11, OP_PRUNE, 0, 0, &t.0.to_string_lossy(), 0)); assert_eq!(next(&p).status, OK);
    p.submit(ask(12, OP_CACHED, 0, 256, &file, 1000)); assert_eq!(next(&p).status, MISS);
    // without the folder's name the helpers keep nothing, and say so
    let none = make(1, 30_000); none.submit(ask(13, OP_DECODE, KEEP, 256, &t.path("noise.exr"), 5)); assert_eq!(next(&none).status, FAILED);
    picture(&file); none.submit(ask(14, OP_DECODE, KEEP, 256, &file, 1)); assert_eq!(next(&none).status, OK); none.submit(ask(15, OP_CACHED, 0, 256, &file, 1)); assert_eq!(next(&none).status, MISS);
}
