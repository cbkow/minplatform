//! The stream SESSION driven headless, as the app drives it (both OSes; no window, so it runs over SSH on the Windows box):
//! a synthetic source — colour bars with a moving block, a full-white flash for two frames every 4 s with a 1 kHz beep on
//! the same instant — rendered at the source's rate through the NV12 pass into the encoder's own surfaces, handed over from
//! the GPU's work-done callback, with the audio pushed in real time; `StreamSink` does the rest (the paced clock, repeat-last,
//! AAC, the mux, reconnects).
//!
//!   cargo run --release -p qcv-stream --example stream_session -- <url> [secs 20] [WxH 1920x1080] [fps 25 | 24000/1001] [kbps 6000] [audio kbps 160] [pause@s:len] [main]
//!
//! `pause@8:4` stops the picture for 4 s from second 8 (a paused viewport: the session repeats its last frame).
#[cfg(any(target_os = "macos", windows))]
fn main() {
    use qcv_stream::{nv12::Nv12Pass, sink::{StreamConfig, StreamSink}, Profile};
    use std::time::{Duration, Instant};
    let a: Vec<String> = std::env::args().skip(1).collect();
    let url = a.first().cloned().expect("a URL");
    let secs: f64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(20.0);
    let (w, h) = a.get(2).and_then(|s| s.split_once('x')).and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))).unwrap_or((1920, 1080));
    let (fn_, fd) = a.get(3).map(|s| match s.split_once('/') { Some((n, d)) => (n.parse().unwrap_or(25), d.parse().unwrap_or(1)), None => (s.parse().unwrap_or(25), 1) }).unwrap_or((25u32, 1u32));
    let kbps: u32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(6000);
    let audio_kbps: u32 = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(160);
    let pause = a.iter().find_map(|x| x.strip_prefix("pause@")).and_then(|p| p.split_once(':')).and_then(|(s, l)| Some((s.parse::<f64>().ok()?, l.parse::<f64>().ok()?)));
    let profile = if a.iter().any(|x| x == "main") { Profile::Main } else { Profile::Baseline };
    let (url, stream_id) = qcv_stream::sink::apply_key(&url, &std::env::var("STREAM_KEY").unwrap_or_default());

    let e = min_present::engine::Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("a GPU");
    let (dev, q) = (e.device.clone(), e.queue.clone());
    let pass = Nv12Pass::new(&dev);
    let bars: [[u8; 3]; 8] = [[191, 191, 191], [191, 191, 0], [0, 191, 191], [0, 191, 0], [191, 0, 191], [191, 0, 0], [0, 0, 191], [255, 255, 255]];
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h { for x in 0..w { let c = bars[((x * 8) / w) as usize]; px[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize].copy_from_slice(&[c[0], c[1], c[2], 255]); } }
    let white = vec![255u8; (w * h * 4) as usize];
    let whole = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    let mk = |data: &[u8]| { let t = dev.create_texture(&wgpu::TextureDescriptor { label: Some("picture"), size: whole, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] }); q.write_texture(t.as_image_copy(), data, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) }, whole); t };
    let (bars_tex, white_tex) = (mk(&px), mk(&white));
    let (bars_view, white_view) = (bars_tex.create_view(&Default::default()), white_tex.create_view(&Default::default()));
    let block = vec![255u8; 64 * 64 * 4]; let black = vec![0u8; 64 * 64 * 4];

    let cfg = StreamConfig { url, width: w, height: h, bitrate_kbps: kbps, profile, srt_latency_ms: 120, stream_id, passphrase: std::env::var("STREAM_PASSPHRASE").unwrap_or_default(), fps_num: 0, fps_den: 1, audio_kbps };
    let mut sink = match StreamSink::start(cfg, (fn_, fd)) { Ok(s) => s, Err(e) => { eprintln!("start refused: {e}"); std::process::exit(2) } };
    let pool = sink.pool();
    println!("session started: {w}x{h} source {fn_}/{fd} fps, {kbps} kbps {profile:?}, audio {audio_kbps} kbps, {secs} s{}", pause.map(|(s, l)| format!(", picture paused {l} s from {s} s")).unwrap_or_default());

    let period = fd as f64 / fn_ as f64;
    let (t0, mut frame, mut audio_done, mut last_report, mut last_state) = (Instant::now(), 0u64, 0u64, 0.0f64, String::new());
    loop {
        let t = t0.elapsed().as_secs_f64();
        if t >= secs { break }
        let paused = pause.is_some_and(|(s, l)| t >= s && t < s + l);
        if paused { frame = (t / period).ceil() as u64 + 1; }   // the source's clock runs on; no picture is published
        // the source's next frame, at its own rate (the app: a new frame shown → publish)
        if !paused && t >= frame as f64 * period {
            let ft = frame as f64 * period;   // the frame's own time: the flash and the beep share it
            let flash = ft % 4.0 < 2.0 * period && ft > 1.0;
            let x = ((frame * 8) % (w as u64 - 64)) as u32; let px0 = (((frame.max(1) - 1) * 8) % (w as u64 - 64)) as u32;
            let at = |x: u32| wgpu::TexelCopyTextureInfo { texture: &bars_tex, mip_level: 0, origin: wgpu::Origin3d { x, y: 0, z: 0 }, aspect: wgpu::TextureAspect::All };
            let small = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(64 * 4), rows_per_image: Some(64) }; let e64 = wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 };
            q.write_texture(at(px0), &black, small, e64); q.write_texture(at(x), &block, small, e64);
            if let Some(surface) = pool.acquire(&dev) {
                let mut enc = dev.create_command_encoder(&Default::default());
                pass.encode(&dev, &mut enc, if flash { &white_view } else { &bars_view }, &surface.y, &surface.cbcr);
                surface.before_submit(&mut enc);
                q.submit([enc.finish()]);
                surface.after_submit(&dev, &q);
                let sub = sink.submitter();
                q.on_submitted_work_done(move || sub.submit(surface));
            }
            frame += 1;
        }
        let _ = dev.poll(wgpu::PollType::Poll);
        // the audio the device would have played by now: silence, with an 80 ms 1 kHz beep where the flash is
        let want = (t * 48000.0) as u64 / 1024 * 1024;
        if want > audio_done {
            let n = (want - audio_done) as usize;
            let mut buf = Vec::with_capacity(n * 2);
            for i in 0..n { let st = (audio_done + i as u64) as f64 / 48000.0; let v = if st % 4.0 < 0.08 && st > 1.0 && !pause.is_some_and(|(s, l)| st >= s && st < s + l) { (0.8 * (2.0 * std::f64::consts::PI * 1000.0 * st).sin()) as f32 } else { 0.0 }; buf.push(v); buf.push(v); }
            sink.push_audio(&buf, false);
            audio_done = want;
        }
        let st = sink.status();
        let state = format!("{:?}", st.state);
        if state != last_state { println!("{t:6.2}s  {state}"); last_state = state; }
        if t - last_report >= 5.0 { last_report = t; println!("{t:6.2}s  {:.0} kbps  {:.3} fps  encoded {} repeated {} skipped {} dropped {} silent {} send errors {} reconnects {}", st.kbps, st.fps, st.encoded, st.repeated, st.skipped, st.dropped, st.audio_silent, st.mux_errors, st.reconnects); }
        if st.failed { break }
        std::thread::sleep(Duration::from_millis(2));
    }
    let st = sink.status();
    println!("end: frames rendered {frame}; {:.0} kbps  {:.3} fps  encoded {} repeated {} skipped {} dropped {} silent {} send errors {} reconnects {}  state {:?}", st.kbps, st.fps, st.encoded, st.repeated, st.skipped, st.dropped, st.audio_silent, st.mux_errors, st.reconnects, st.state);
    sink.stop_wait(Duration::from_secs(3));
    println!("stopped: {:?}", sink.status().state);
}
#[cfg(not(any(target_os = "macos", windows)))]
fn main() { eprintln!("stream_session: no hardware session on this OS"); }
