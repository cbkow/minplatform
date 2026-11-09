//! A QCBridge host feed (QCView `HostBridgeSource`, canon rows 227–228): After Effects / Premiere frames from the QCAE ring.
//! QCView's behaviour, with its consumer moved onto the render tick (latest-wins either way; no second thread, and the Mac's
//! zero-copy texture is made where it is drawn): open the ring by name every 250 ms while absent; the producer's PID is the
//! liveness (AE quits without unlinking; POSIX removes the dead ring); Retired → re-open at once; PausedFocus / Paused from the
//! host state; RGBA16F only; the host fps from three equal time deltas; a timecode from the host time (Premiere).
//! QCView's bugs fixed (row 228): restarts count once per producer death; a stale "not running" clears on a re-open; every
//! detail is a translatable English key; an unaccepted frame says so in the detail; `has_frame` only with a frame shown.
use min_present::{ColorMeta, Frame, Pixels, Plane, PlanarLayout, Range, Transfer};
use qcv_ring::{layout::*, qcae::SharedRing, url::Host};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status { Connecting, Live, Paused, Reconnecting }

/// What the shell shows (the LiveStrip): the status, its detail as (English key, argument) — the shell translates and fills.
#[derive(Clone, Debug, PartialEq)]
pub struct Facts { pub status: Status, pub detail: Option<(&'static str, String)>, pub width: u32, pub height: u32, pub frames: u64, pub restarts: u32, pub non_finite: bool, pub has_frame: bool }

pub const WAITING: &str = "Waiting for %1 (QCBridge Transmit device not publishing)";
pub const NOT_RUNNING: &str = "%1 is not running";
pub const INCOMPATIBLE: &str = "Incompatible QCBridgeAE plugin (%1)";
pub const PAUSED_FOCUS: &str = "%1 stopped sending when it lost focus — untick “Disable video output when in the background” in its preferences";
pub const PAUSED: &str = "%1 paused the feed";
pub const UNSUPPORTED: &str = "%1 sent a frame QCView can't show (%2) — it needs the QCBridgeAE device's RGBA16F output";

const ABSENT_POLL: Duration = Duration::from_millis(250);

pub struct HostFeed {
    pub host: Host,
    ring: Option<SharedRing>,
    last_try: Option<Instant>,
    status: Status, detail: Option<(&'static str, String)>,
    ever_live: bool, live_since: Option<Instant>,
    /// the PID whose death was already counted (row 228 fix 1: once per producer, never per poll)
    counted_dead: Option<u32>,
    restarts: u32, frames: u64, bytes: u64, size: (u32, u32), non_finite: bool, has_frame: bool,
    /// host-time deltas toward the rate (QCView: three equal positive deltas)
    last_time: Option<(i64, i64)>, run: (i64, u32), fps: Option<f64>, last_host_time: Option<(i64, i64)>,
    #[cfg(target_os = "macos")] slots: Option<(usize, qcv_ring::mtl::SlotBuffers)>,
    /// QCView `setVideoHoldMs` (the negative part of the live A/V offset): frames wait this long before they show — copied
    /// out of the ring (a held slot would stall a 3-slot producer), queued with their due time
    hold: Duration, held: std::collections::VecDeque<(Instant, Frame)>,
}

impl HostFeed {
    pub fn new(host: Host) -> HostFeed {
        HostFeed { host, ring: None, last_try: None, status: Status::Connecting, detail: Some((WAITING, host.app().into())), ever_live: false, live_since: None, counted_dead: None,
            restarts: 0, frames: 0, bytes: 0, size: (0, 0), non_finite: false, has_frame: false, last_time: None, run: (0, 0), fps: None, last_host_time: None,
            #[cfg(target_os = "macos")] slots: None, hold: Duration::ZERO, held: Default::default() }
    }
    pub fn facts(&self) -> Facts { Facts { status: self.status, detail: self.detail.clone(), width: self.size.0, height: self.size.1, frames: self.frames, restarts: self.restarts, non_finite: self.non_finite, has_frame: self.has_frame } }
    pub fn status(&self) -> Status { self.status }
    pub fn host(&self) -> Host { self.host }
    pub fn host_fps(&self) -> Option<f64> { self.fps }
    pub fn live_seconds(&self) -> f64 { self.live_since.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0) }
    fn set_status(&mut self, s: Status) { if s != self.status { if s == Status::Live { self.live_since = Some(Instant::now()); } else { self.live_since = None; } self.status = s; } }
    fn waiting(&self) -> Status { if self.ever_live { Status::Reconnecting } else { Status::Connecting } }
    /// QCView `hostTimecode`: HH:MM:SS:FF at the host rate (non-drop, from seconds), else HH:MM:SS.mmm; empty for AE (no host time).
    pub fn host_timecode(&self) -> String {
        let Some((v, s)) = self.last_host_time.filter(|(v, s)| *v >= 0 && *s > 0) else { return String::new() };
        let secs = v as f64 / s as f64; let whole = secs.floor() as i64; let (h, m, sec) = (whole / 3600, whole / 60 % 60, whole % 60);
        match self.fps { Some(f) => format!("{h:02}:{m:02}:{sec:02}:{:02}", ((secs - secs.floor()) * f + 1e-6) as i64), None => format!("{h:02}:{m:02}:{sec:02}.{:03}", ((secs - secs.floor()) * 1000.0) as i64) }
    }
    fn drop_ring(&mut self) { self.ring = None; #[cfg(target_os = "macos")] { self.slots = None; } }

    /// Per tick: the newest frame when there is one (latest wins). `device`: the Mac wraps the ring slot for the GPU without a
    /// copy (S4: 0.2 ms at 4K); elsewhere, and without a device, the slot is copied (DX12: one copy).
    /// The negative part of the live A/V offset (QCView's video hold), 0 = none.
    pub fn set_video_hold_ms(&mut self, ms: i32) { self.hold = Duration::from_millis(ms.max(0) as u64); }
    /// Per tick: the frame to show now, if any (a hold delays each frame by its time; switched off, the newest held one shows).
    pub fn tick(&mut self, now: Instant, device: Option<&wgpu::Device>) -> Option<Frame> {
        if self.hold.is_zero() {
            let flushed = self.held.drain(..).last().map(|(_, f)| f);
            return self.take(now, device).or(flushed)
        }
        if let Some(f) = self.take(now, None) { self.held.push_back((now + self.hold, f)); }
        while self.held.len() > 64 { self.held.pop_front(); }   // a bound, never reached at ≤ 200 ms
        let mut out = None;
        while self.held.front().is_some_and(|(due, _)| *due <= now) { out = self.held.pop_front().map(|(_, f)| f); }
        out
    }
    fn take(&mut self, now: Instant, device: Option<&wgpu::Device>) -> Option<Frame> {
        let _ = &device;
        if self.ring.is_none() {
            if self.last_try.is_some_and(|t| now.duration_since(t) < ABSENT_POLL) { return None }
            self.last_try = Some(now);
            match SharedRing::open(self.host.ring_name()) {
                Ok(r) => {
                    // fix 2: a ring that opens clears a stale "not running" (QCView kept it until a first frame)
                    if self.detail.as_ref().is_some_and(|(k, _)| *k == NOT_RUNNING || *k == INCOMPATIBLE) { self.detail = Some((WAITING, self.host.app().into())); }
                    log::info!("host feed {}: ring open ({} slots)", self.host.key(), r.header().slot_count);
                    self.ring = Some(r);
                }
                Err(e) => {
                    self.detail = Some(if e.contains("version") { (INCOMPATIBLE, e) } else if self.detail.as_ref().is_some_and(|(k, _)| *k == NOT_RUNNING) { self.detail.clone().unwrap() } else { (WAITING, self.host.app().into()) });
                    let w = self.waiting(); self.set_status(w);
                    return None
                }
            }
        }
        let (pid, state) = { let r = self.ring.as_ref()?; (r.header().producer_pid, r.host_state()) };
        if !qcv_ring::process_alive(pid) {
            self.drop_ring();
            if qcv_ring::mapping::unlink(self.host.ring_name()) { log::info!("host feed {}: removed the ring a dead producer left", self.host.key()); }
            if self.ever_live && self.counted_dead != Some(pid) { self.restarts += 1; }
            self.counted_dead = Some(pid);
            self.detail = Some((NOT_RUNNING, self.host.app().into()));
            let w = self.waiting(); self.set_status(w); self.last_try = Some(now);
            return None
        }
        if state == HostState::Retired as u32 { self.drop_ring(); self.last_try = None; return None }   // re-open at once, no count
        let ring = self.ring.as_mut()?;
        let Some((d, px, slot)) = ring.acquire_latest() else {
            // nothing new: the host state says why; a feed that was live and is not paused is live (an unchanged comp sends nothing)
            if state == HostState::PausedFocus as u32 { self.detail = Some((PAUSED_FOCUS, self.host.app().into())); self.set_status(Status::Paused); }
            else if state == HostState::Paused as u32 { self.detail = Some((PAUSED, self.host.app().into())); self.set_status(Status::Paused); }
            else if self.ever_live && self.status != Status::Live { self.detail = None; self.set_status(Status::Live); }
            return None
        };
        // QCView accepts only what the shipped device writes: RGBA16F, RGBA order, rows ≥ w × 8
        if !(d.pixel_format == PixelFormat::Rgba16F as u32 && d.channel_order == ChannelOrder::Rgba as u32 && d.width > 0 && d.height > 0 && d.bytes_per_row >= d.width * 8) {
            ring.release();
            let what = format!("format {} · order {} · {}×{}", d.pixel_format, d.channel_order, d.width, d.height);
            log::warn!("host feed {}: unexpected frame ({what}) dropped", self.host.key());
            self.detail = Some((UNSUPPORTED, what));   // fix 4: said in the strip, not only logged
            return None
        }
        let (w, h, bpr) = (d.width, d.height, d.bytes_per_row);
        let premultiplied = d.flags & FLAG_PREMULTIPLIED != 0;
        let meta = ColorMeta { transfer: Transfer::Srgb, range: Range::Full, bit_depth: 16, ..Default::default() };   // untagged: as stored (QCView)
        let pixels = 'px: {
            #[cfg(target_os = "macos")]
            if let Some(dev) = device {
                // zero-copy: the slot's pages wrapped once per mapping, a texture view per frame; the claim holds the slot
                let ring = self.ring.as_ref().unwrap();
                if self.slots.as_ref().map(|(n, _)| *n) != Some(ring.map.base as usize) {
                    let offs: Vec<u64> = (0..ring.header().slot_count).map(|i| ring.pixels_offset_of(i)).collect();
                    self.slots = qcv_ring::mtl::SlotBuffers::new(dev, ring.map.base, &offs, ring.header().pixels_capacity).map(|s| (ring.map.base as usize, s));
                }
                if let Some(tex) = self.slots.as_ref().and_then(|(_, s)| s.texture(dev, slot as usize, w, h, bpr)) {
                    break 'px Pixels::Gpu(std::sync::Arc::new(min_present::GpuPlanes { textures: vec![tex], layout: PlanarLayout::RgbaF16 { premultiplied } }))
                }
            }
            let _ = slot;
            let ring = self.ring.as_mut().unwrap();
            let bytes = unsafe { std::slice::from_raw_parts(px, (bpr * h) as usize) }.to_vec();
            let intact = ring.held_intact(); ring.release();
            if !intact { return None }   // a writer touched the slot mid-copy: never shown (qcv-ring's check)
            Pixels::Planar { planes: vec![Plane::owned(bytes, w, h, bpr)], layout: PlanarLayout::RgbaF16 { premultiplied } }
        };
        // the facts (QCView: counters, geometry, inf / NaN per frame, the rate from three equal deltas, Live on a frame)
        self.frames += 1; self.bytes += (bpr * h) as u64; self.size = (w, h); self.has_frame = true;
        self.non_finite = d.flags & (FLAG_HAS_INF | FLAG_HAS_NAN) != 0;
        if d.time_value >= 0 && d.time_scale > 0 {
            if let Some((v0, s0)) = self.last_time && s0 == d.time_scale {
                let delta = d.time_value - v0;
                if delta > 0 && delta == self.run.0 { self.run.1 += 1; if self.run.1 >= 3 { self.fps = Some(d.time_scale as f64 / delta as f64); } } else { self.run = (delta, 1); }
            }
            self.last_time = Some((d.time_value, d.time_scale)); self.last_host_time = Some((d.time_value, d.time_scale));
        }
        self.ever_live = true; self.detail = None; self.set_status(Status::Live);
        Some(Frame { width: w, height: h, pixels, meta })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame_desc(w: u32, h: u32, t: i64) -> FrameDesc { FrameDesc { width: w, height: h, bytes_per_row: aligned_bytes_per_row(w, 8), pixel_format: PixelFormat::Rgba16F as u32, source_tier: SourceTier::Float32 as u32, channel_order: ChannelOrder::Rgba as u32, flags: 0, _pad0: 0, time_value: t, time_scale: 24_000, icc_generation: 0, graphics_white: 0.0, value_scale: 1.0, comp_name: [0; MAX_COMP_NAME] } }
    fn publish(r: &mut SharedRing, d: FrameDesc) { let dst = r.begin_write(d.bytes_per_row as u64 * d.height as u64).unwrap(); unsafe { std::ptr::write_bytes(dst, 0, (d.bytes_per_row * d.height) as usize) }; r.commit(d); }

    /// QCView's states through a real ring (the probe's name), with row 228's fixes 1, 2 and 4.
    #[test]
    fn the_states_and_the_fixes() {
        let host = Host::Probe; let _ = qcv_ring::mapping::unlink(host.ring_name());
        let mut f = HostFeed::new(host); let t = Instant::now();
        assert!(f.tick(t, None).is_none()); assert_eq!(f.facts().status, Status::Connecting); assert_eq!(f.facts().detail.unwrap().0, WAITING);
        let (w, h) = (32u32, 16u32);
        let mut p = SharedRing::create(host.ring_name(), aligned_bytes_per_row(w, 8) as u64 * h as u64, 3).unwrap();
        p.set_host_state(HostState::Active);
        // frames 1001 apart at 24000 ticks/s → 23.976 after three equal deltas
        let mut later = t + Duration::from_secs(1);
        for i in 0..5 { publish(&mut p, frame_desc(w, h, i * 1001)); later += Duration::from_millis(5); assert!(f.tick(later, None).is_some(), "frame {i}"); }
        let fx = f.facts(); assert_eq!((fx.status, fx.width, fx.frames, fx.has_frame), (Status::Live, w, 5, true));
        assert!((f.host_fps().unwrap() - 23.976).abs() < 1e-3);
        assert!(f.host_timecode().starts_with("00:00:00:"), "{}", f.host_timecode());
        // the host loses focus
        p.set_host_state(HostState::PausedFocus); later += Duration::from_millis(5); f.tick(later, None);
        assert_eq!((f.facts().status, f.facts().detail.unwrap().0), (Status::Paused, PAUSED_FOCUS));
        p.set_host_state(HostState::Active); later += Duration::from_millis(5); f.tick(later, None);
        assert_eq!(f.facts().status, Status::Live, "resumes without a frame (an unchanged comp sends nothing)");
        // an unsupported frame says so (fix 4)
        let mut bad = frame_desc(w, h, 9999); bad.pixel_format = PixelFormat::Rgba8Unorm as u32; publish(&mut p, bad); later += Duration::from_millis(5);
        assert!(f.tick(later, None).is_none()); assert_eq!(f.facts().detail.unwrap().0, UNSUPPORTED);
        // the producer dies: its pid made dead by hand; counted ONCE however many ticks pass (fix 1)
        unsafe { (*(p.map.base as *mut RingHeader)).producer_pid = 999_999 };
        for _ in 0..8 { later += Duration::from_millis(300); f.tick(later, None); }
        assert_eq!((f.facts().status, f.facts().restarts), (Status::Reconnecting, 1));
        assert_eq!(f.facts().detail.unwrap().0, NOT_RUNNING);
        // the host comes back (a new ring by the same name): the stale "not running" clears on the open (fix 2)
        drop(p); let _ = qcv_ring::mapping::unlink(host.ring_name());
        let mut p2 = SharedRing::create(host.ring_name(), aligned_bytes_per_row(w, 8) as u64 * h as u64, 3).unwrap(); p2.set_host_state(HostState::Active);
        later += Duration::from_millis(300); f.tick(later, None);
        assert_ne!(f.facts().detail.map(|d| d.0), Some(NOT_RUNNING), "not running cleared when the ring reopened (an ever-live feed resumes as Live, QCView)");
        publish(&mut p2, frame_desc(w, h, 0)); later += Duration::from_millis(5); assert!(f.tick(later, None).is_some());
        assert_eq!((f.facts().status, f.facts().restarts), (Status::Live, 1));
    }
}
