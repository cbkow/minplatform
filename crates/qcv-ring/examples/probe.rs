//! A synthetic QCBridge host (QCView's `qcbae://probe` test signal): publishes a moving RGBA16F ramp that runs past 1.0 into a
//! QCAE ring, as the QCBridgeAE Transmit device does, so the live feed can be walked without After Effects.
//!   cargo run -p qcv-ring --example probe -- [probe|ae|premiere] [WxH] [fps] [seconds]
//! The ring's name follows the host (`/qcbae-probe` by default); Ctrl+C stops it — the next producer replaces the ring.
use qcv_ring::{layout::*, qcae::SharedRing, url::Host};
use std::time::{Duration, Instant};

fn half(v: f32) -> u16 { let b = v.to_bits(); let s = ((b >> 16) & 0x8000) as u16; let e = ((b >> 23) & 0xff) as i32 - 127 + 15; let m = (b & 0x7f_ffff) >> 13; if e <= 0 { s } else if e >= 31 { s | 0x7c00 } else { s | ((e as u16) << 10) | m as u16 } }

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let host = match a.first().map(String::as_str) { Some("ae") => Host::AfterEffects, Some("premiere") => Host::Premiere, _ => Host::Probe };
    let (w, h) = a.get(1).and_then(|s| s.split_once('x')).and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?))).unwrap_or((1920u32, 1080u32));
    let fps: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(24.0);
    let seconds: f64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(600.0);
    let bpr = aligned_bytes_per_row(w, 8);
    let _ = qcv_ring::mapping::unlink(host.ring_name());
    let mut ring = SharedRing::create(host.ring_name(), bpr as u64 * h as u64, 3).expect("create the ring");
    ring.set_host_state(HostState::Active);
    println!("probe: {} {w}x{h} @ {fps} fps for {seconds} s — connect qcv to {}", host.ring_name(), host.url());
    let t0 = Instant::now(); let dt = Duration::from_secs_f64(1.0 / fps); let mut n = 0u64;
    let mut row = vec![0u16; (bpr / 2) as usize];
    while t0.elapsed().as_secs_f64() < seconds {
        if let Some(d) = (t0 + dt.mul_f64(n as f64)).checked_duration_since(Instant::now()) { std::thread::sleep(d); }
        let Some(dst) = ring.begin_write(bpr as u64 * h as u64) else { break };
        let phase = (n % (fps as u64 * 4).max(1)) as f32 / (fps as f32 * 4.0);
        for y in 0..h as usize {
            // a horizontal ramp 0 → 1.5 (runs past 1.0, as QCView's probe does), a moving bar, a vertical colour split
            for x in 0..w as usize {
                let r = x as f32 / w as f32 * 1.5; let bar = ((x as f32 / w as f32 - phase).abs() < 0.01) as u8 as f32;
                let band = y * 3 / h as usize;
                let (cr, cg, cb) = match band { 0 => (r, r, r), 1 => (r, r * 0.5, 0.0), _ => (0.0, r * 0.6, r) };
                row[x * 4] = half(cr.max(bar)); row[x * 4 + 1] = half(cg.max(bar)); row[x * 4 + 2] = half(cb.max(bar)); row[x * 4 + 3] = half(1.0);
            }
            unsafe { std::ptr::copy_nonoverlapping(row.as_ptr() as *const u8, dst.add(y * bpr as usize), bpr as usize) }
        }
        let mut desc = FrameDesc { width: w, height: h, bytes_per_row: bpr, pixel_format: PixelFormat::Rgba16F as u32, source_tier: SourceTier::Float32 as u32, channel_order: ChannelOrder::Rgba as u32, flags: 0, _pad0: 0,
            time_value: if host == Host::AfterEffects { -1 } else { (n as f64 * 254_016_000_000.0 / fps) as i64 }, time_scale: 254_016_000_000, icc_generation: 0, graphics_white: 0.0, value_scale: 1.0, comp_name: [0; MAX_COMP_NAME] };
        desc.comp_name[..5].copy_from_slice(b"probe");
        ring.commit(desc); n += 1;
    }
    ring.set_host_state(HostState::Retired);
    println!("probe: {n} frames, retired");
}
