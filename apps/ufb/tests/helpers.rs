//! The helper copies of the executable, as the application uses them (canon row 456; spike S18): the real binary is
//! started with `--thumb-helper` and spoken to through the application's own pool.
#[path = "../src/thumbs/proto.rs"] mod proto;
#[path = "../src/thumbs/pool.rs"] mod pool;
use pool::{Done, Job, Pool};
use proto::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn make(n: usize, idle_ms: u64) -> Pool { Pool::new(PathBuf::from(env!("CARGO_BIN_EXE_UFB")), vec![("UFB_HELPER_TEST".into(), "1".into())], n, Duration::from_millis(idle_ms), Arc::new(|| {})) }
fn job(id: u64, op: u8, path: &str, hard_ms: u32) -> Job { Job { req: Request { id, op, size: 256, soft_ms: 5000, path: path.into() }, hard_ms } }
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
