//! Step 4a's harness (the original's A0 bar): a synthetic picture — flat colour bars, grey steps, a moving block — through the
//! NV12 pass into the encoder's own surfaces, the native H.264 session, MPEG-TS over SRT to a listener.
//!
//!   cargo run --release -p qcv-stream --example stream_out -- "srt://127.0.0.1:9000" [secs 8] [WxH 1920x1080] [fps 25 | 24000/1001] [kbps 6000] [main]
//!
//! It prints `expect x y Y Cb Cr` for the centre of every flat patch (QCView's BT.709 limited maths): decode the received
//! stream to yuv420p and compare.
#[cfg(any(target_os = "macos", windows))]
fn main() {
    use qcv_stream::{mux, nv12::Nv12Pass, pts90k, sink, EncodedUnit, EncoderSpec, Profile};
    use std::sync::{atomic::AtomicBool, mpsc, Arc};
    use std::time::{Duration, Instant};
    let a: Vec<String> = std::env::args().skip(1).collect();
    let url = a.first().cloned().expect("an srt:// URL");
    let secs: f64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(8.0);
    let (w, h) = a.get(2).and_then(|s| s.split_once('x')).and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))).unwrap_or((1920, 1080));
    let (fn_, fd) = a.get(3).map(|s| match s.split_once('/') { Some((n, d)) => (n.parse().unwrap_or(25), d.parse().unwrap_or(1)), None => (s.parse().unwrap_or(25), 1) }).unwrap_or((25u32, 1u32));
    let kbps: u32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(6000);
    let profile = if a.iter().any(|x| x == "main") { Profile::Main } else { Profile::Baseline };

    let e = min_present::engine::Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("a GPU");
    let (dev, q) = (e.device.clone(), e.queue.clone());
    let pool = sink::SurfacePool::new(w, h).expect("the surface pool");
    let pass = Nv12Pass::new(&dev);

    // the picture: 8 bars over the top two thirds, 8 grey steps below — encoded sRGB, as the wire texture is
    let bars: [[u8; 3]; 8] = [[191, 191, 191], [191, 191, 0], [0, 191, 191], [0, 191, 0], [191, 0, 191], [191, 0, 0], [0, 0, 191], [255, 255, 255]];
    let greys: [u8; 8] = [0, 32, 64, 96, 128, 160, 192, 224];
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h { for x in 0..w {
        let i = ((x * 8) / w) as usize;
        let c = if y < h * 2 / 3 { bars[i] } else { [greys[i]; 3] };
        px[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize].copy_from_slice(&[c[0], c[1], c[2], 255]);
    } }
    let src = dev.create_texture(&wgpu::TextureDescriptor { label: Some("picture"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
    let whole = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    q.write_texture(src.as_image_copy(), &px, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) }, whole);
    let view = src.create_view(&Default::default());
    let yuv = |c: [u8; 3]| { let (r, g, b) = (c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0); let y = 0.2126 * r + 0.7152 * g + 0.0722 * b; ((y * 219.0 + 16.0).round() as i32, ((b - y) / 1.8556 * 224.0 + 128.0).round() as i32, ((r - y) / 1.5748 * 224.0 + 128.0).round() as i32) };
    for i in 0..8u32 {
        let x = w * (2 * i + 1) / 16;
        let (y1, cb, cr) = yuv(bars[i as usize]); println!("expect {x} {} {y1} {cb} {cr}", h / 3);
        let (y2, cb, cr) = yuv([greys[i as usize]; 3]); println!("expect {x} {} {y2} {cb} {cr}", h * 5 / 6);
    }

    let (tx, rx) = mpsc::channel::<EncodedUnit>();
    let spec = EncoderSpec { width: w, height: h, fps_num: fn_, fps_den: fd, bitrate_kbps: kbps, gop_frames: (fn_ as f64 / fd as f64).round() as u32, profile };
    // the encoder opens once a surface exists (Windows makes its devices on the first acquire)
    let mut session: Option<sink::Session> = None;
    let mut held = std::collections::VecDeque::new();
    let stop = Arc::new(AtomicBool::new(false));
    let mut m = mux::Muxer::new(stop.clone());
    let cfg = mux::MuxConfig { url: url.clone(), width: w, height: h, fps_num: fn_, fps_den: fd, video_kbps: kbps, srt_latency_ms: 120, ..Default::default() };
    println!("connecting to {url}");
    m.open(&cfg).expect("the connection");
    println!("streaming {w}x{h} at {fn_}/{fd} fps, {kbps} kbps, {profile:?}, {secs} s");

    let period = Duration::from_secs_f64(fd as f64 / fn_ as f64);
    let ticks = (secs * fn_ as f64 / fd as f64).round() as u64;
    let (t0, mut skipped, mut written, mut keys) = (Instant::now(), 0u64, 0u64, 0u64);
    let block = vec![255u8; 64 * 64 * 4]; let black = vec![0u8; 64 * 64 * 4];
    let block_at = |k: u64| ((k * 8) % (w as u64 - 64)) as u32;
    for k in 0..ticks {
        // a white block walking along the top edge: motion for the encoder, away from the patches' centres
        if k > 0 { q.write_texture(wgpu::TexelCopyTextureInfo { texture: &src, mip_level: 0, origin: wgpu::Origin3d { x: block_at(k - 1), y: 0, z: 0 }, aspect: wgpu::TextureAspect::All }, &black, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(64 * 4), rows_per_image: Some(64) }, wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 }); }
        q.write_texture(wgpu::TexelCopyTextureInfo { texture: &src, mip_level: 0, origin: wgpu::Origin3d { x: block_at(k), y: 0, z: 0 }, aspect: wgpu::TextureAspect::All }, &block, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(64 * 4), rows_per_image: Some(64) }, wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 });
        match pool.acquire(&dev) {
            Some(surface) => {
                let mut enc = dev.create_command_encoder(&Default::default());
                pass.encode(&dev, &mut enc, &view, &surface.y, &surface.cbcr);
                surface.before_submit(&mut enc);
                q.submit([enc.finish()]);
                surface.after_submit(&dev, &q);
                let _ = dev.poll(wgpu::PollType::wait_indefinitely());   // the GPU is done: the surface is the encoder's
                if session.is_none() { let tx = tx.clone(); session = Some(sink::Session::open(&spec, &pool, Box::new(move |u| { let _ = tx.send(u); })).expect("the encoder")); }
                session.as_ref().unwrap().encode(&surface, pts90k(k, fn_, fd), pts90k(k + 1, fn_, fd) - pts90k(k, fn_, fd), k == 0);
                held.push_back(surface); if held.len() > 2 { held.pop_front(); }   // as the clock keeps the last one
            }
            None => skipped += 1,
        }
        while let Ok(u) = rx.try_recv() { if u.keyframe { keys += 1; } if let Err(e) = m.write(&u) { eprintln!("{e}"); return } written += 1; }
        let due = t0 + period * (k as u32 + 1);
        if let Some(d) = due.checked_duration_since(Instant::now()) { std::thread::sleep(d); }
    }
    std::thread::sleep(Duration::from_millis(200));   // the last frames out of the encoder
    let (encoded, errors) = session.as_ref().map(|s| (s.encoded(), s.errors())).unwrap_or((0, 0));
    drop(held); drop(session);
    while let Ok(u) = rx.try_recv() { if u.keyframe { keys += 1; } if m.write(&u).is_ok() { written += 1; } }
    let dt = t0.elapsed().as_secs_f64();
    println!("done: {ticks} ticks, {encoded} encoded (+ the tail), {written} written, {keys} keyframes, {skipped} skipped, {errors} encoder errors, {:.0} kbps over {dt:.1} s", m.bytes as f64 * 8.0 / 1000.0 / dt);
    m.close();
}
#[cfg(not(any(target_os = "macos", windows)))]
fn main() { eprintln!("stream_out: no hardware session on this OS"); }
