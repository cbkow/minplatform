//! Open a live stream URL with `StreamFeed` (QCView's LiveStreamDecoder) and print its facts once a second — the step-2a
//! harness, no window: `cargo run --release -p qcv-player --example stream_probe -- "srt://0.0.0.0:9998?mode=listener" 10`
use qcv_player::stream_feed::StreamFeed;
use std::time::{Duration, Instant};
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let url = a.first().cloned().unwrap_or_else(|| "srt://0.0.0.0:9998?mode=listener&latency=120000".into());
    let secs: u64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
    let hw = if a.iter().any(|x| x == "--hw") { qcv_player::HwDevice::platform(None) } else { None };
    let mut f = StreamFeed::open(&url, hw);
    let t0 = Instant::now(); let mut shown = 0u64; let mut last = Instant::now();
    while t0.elapsed() < Duration::from_secs(secs) {
        if let Some(fr) = f.tick() { shown += 1; if shown == 1 { println!("first frame {}×{} at {:.2} s", fr.width, fr.height, t0.elapsed().as_secs_f64()); } }
        if last.elapsed() >= Duration::from_secs(1) { last = Instant::now(); let x = f.facts(); println!("{:>4.1}s {:?} detail {:?} {}×{} {} {} frames {} shown {} drops {} {:.2} Mb tags {:?}", t0.elapsed().as_secs_f64(), x.status, x.detail, x.width, x.height, x.codec, x.pix_fmt, x.frames, shown, x.restarts, x.bytes as f64 * 8.0 / 1e6, f.tags()); }
        std::thread::sleep(Duration::from_millis(5));
    }
}
