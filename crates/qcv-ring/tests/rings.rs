//! qcv-ring's acceptance in one process (the two-process runs against the shipped C++ were spike S4's): the layouts byte-exact
//! with the C++ compiler's view of vendor/qcbae; a producer thread racing the consumer never shows a torn frame; the audio
//! reader delivers in order, resyncs when lapped, reads sessions.
use qcv_ring::{audio, layout::*, qcae::SharedRing};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};

fn name(tag: &str) -> String { format!("/qcvr-{tag}-{}", std::process::id() % 100000) }

#[test]
fn the_layouts_are_byte_exact_with_the_cpp() {
    let (n, bad) = qcv_ring::layout::check();
    assert!(bad.is_empty(), "mismatches: {bad:?}");
    assert!(n >= 100, "only {n} checks");
}

#[test]
fn a_racing_producer_never_shows_a_torn_frame() {
    let nm = name("f"); let (w, h) = (64u32, 32u32); let bpr = aligned_bytes_per_row(w, 8);
    let mut prod = SharedRing::create(&nm, bpr as u64 * h as u64, 3).unwrap();
    let mut cons = SharedRing::open(&nm).unwrap();
    assert_eq!(cons.header().slot_count, 3);
    let stop = Arc::new(AtomicBool::new(false)); let s2 = stop.clone();
    let t = std::thread::spawn(move || {
        let mut k = 0u16;
        while !s2.load(Ordering::Relaxed) {
            let dst = prod.begin_write(bpr as u64 * h as u64).unwrap();
            k = k.wrapping_add(1);
            // every u16 of the frame = k: a torn read would mix two values
            unsafe { std::slice::from_raw_parts_mut(dst as *mut u16, (bpr * h / 2) as usize).fill(k) };
            let desc = FrameDesc { width: w, height: h, bytes_per_row: bpr, pixel_format: PixelFormat::Rgba16F as u32, source_tier: SourceTier::Float32 as u32, channel_order: ChannelOrder::Rgba as u32, flags: 0, _pad0: 0, time_value: k as i64, time_scale: 24, icc_generation: 0, graphics_white: 0.0, value_scale: 1.0, comp_name: [0; MAX_COMP_NAME] };
            prod.commit(desc);
        }
        prod
    });
    let mut seen = 0; let mut dropped = 0; let t0 = std::time::Instant::now();
    while seen < 2000 && t0.elapsed().as_secs() < 10 {
        if let Some((d, px, _)) = cons.acquire_latest() {
            let copy: Vec<u16> = unsafe { std::slice::from_raw_parts(px as *const u16, (d.bytes_per_row * d.height / 2) as usize) }.to_vec();
            // an unpaced producer can land on the held slot before the claim does: the intact check catches it, the frame is dropped
            if cons.held_intact() {
                let v = copy[0]; assert!(copy.iter().all(|x| *x == v), "torn frame at {seen}");
                assert_eq!(d.time_value, v as i64, "the desc belongs to the pixels");
                seen += 1;
            } else { dropped += 1; }
            cons.release();
        }
    }
    stop.store(true, Ordering::Relaxed); let prod = t.join().unwrap();
    assert!(seen >= 200, "only {seen} frames ({dropped} dropped as touched)");
    eprintln!("{seen} frames verified, {dropped} dropped by the intact check");
    assert!(cons.producer_alive(), "our own pid is alive");
    drop(prod);
}

#[test]
fn audio_arrives_in_order_and_a_lapped_reader_resyncs() {
    let nm = name("a");
    let mut prod = audio::AudioRing::create(&nm, 48_000, 8, 16, 2).unwrap();
    let cons = audio::AudioRing::open(&nm).unwrap();
    let mut last = 0u64; cons.skip_to_latest(&mut last);
    let mut planes = vec![0f32; 2 * 16]; let mut desc = AudioPacketDesc::default(); let mut dropped = 0;
    let l: Vec<f32> = (0..40).map(|i| i as f32).collect(); let r: Vec<f32> = l.iter().map(|v| -v).collect();
    assert_eq!(prod.push(&[&l, &r], 48_000, 40, 0, 0), 3, "40 frames in packets of 16");
    let mut got = vec![];
    while cons.next_packet(&mut last, &mut desc, &mut planes, &mut dropped) == audio::Next::Packet { got.extend_from_slice(&planes[..desc.frames as usize]); assert_eq!(planes[16], -planes[0]); }
    assert_eq!(got, l, "in order, whole");
    assert_eq!(desc.time_value, 32, "time advanced by frames per packet");
    // lap the reader: 20 packets into 8 slots
    for _ in 0..20 { prod.push(&[&l[..16], &r[..16]], 48_000, 16, 0, 0); }
    assert_eq!(cons.next_packet(&mut last, &mut desc, &mut planes, &mut dropped), audio::Next::Resynced);
    assert!(dropped > 0);
    let mut n = 0; while cons.next_packet(&mut last, &mut desc, &mut planes, &mut dropped) == audio::Next::Packet { n += 1; }
    assert_eq!(n, 6, "rejoined just behind the newest: slots − 2");
    // the session
    let mut known = 0;
    assert!(cons.read_session(&mut known).is_none());
    prod.begin_session(AudioSession { sample_rate: 44_100, channels: 2, flags: audio::SESSION_SCRUBBING, ..Default::default() });
    let s = cons.read_session(&mut known).unwrap(); assert_eq!((s.sample_rate, s.channels), (44_100, 2));
    assert!(cons.read_session(&mut known).is_none(), "unchanged generation");
    assert_eq!(cons.state(), audio::State::Pushing);
}
