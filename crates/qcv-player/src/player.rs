//! The clock + presenter glue over `VideoDecoder` (`Policy::Clocked`): S2's rational clock picks, each refresh, the newest
//! frame at or before the clock frame, and accounts lateness / drops / cadence. Thin by design — the decoder is QCView's.
use crate::{ffmpeg::{OpenOptions, SeqOptions, SessionInfo}, frame_handle::FrameHandle, hw::HwDevice, source::Source, video_decoder::{Policy, State, VideoDecoder}, Error};
use qcv_timeline::Rate;
use std::{collections::BTreeMap, time::{Duration, Instant}};

#[derive(Default, Debug)]
pub struct Stats {
    /// shown-frame lateness vs the ideal clock, ms (negative = early)
    pub lateness_ms: Vec<f32>, /// refreshes per shown frame → count
    pub cadence: BTreeMap<i64, u32>, pub dropped: u32, pub late: u32, pub black: u32, pub shown: u32, pub decode_ms: Vec<f32>, pub loops: u32,
}

pub struct Player {
    pub decoder: VideoDecoder, pub rate: Rate, pub frames: i64,
    clock: Option<(Instant, i64)>, paused_at: Option<Instant>,
    pub stats: Stats, last_shown: Option<i64>, refreshes_on_current: i64,
    /// one refresh in ms — the "late" threshold (16.667 at 60 Hz)
    pub refresh_ms: f32, current: Option<FrameHandle>, /// seek to 0 and continue at the end (QCView stops: "play stops at the end")
    pub looping: bool,
}
/// Frames decoded ahead of the clock (QCView's slot of one, widened for the clock; the presenter's ring holds 3 more).
pub const LOOKAHEAD: usize = 3;

impl Player {
    pub fn open(src: &Source, hw: Option<HwDevice>, threads: i32) -> Result<Player, Error> {
        let opts = OpenOptions { threads, hw, seq: match src { Source::Sequence { rate, start, .. } => Some(SeqOptions { rate: *rate, start_number: *start }), _ => None } };
        let mut decoder = VideoDecoder::new(Policy::Clocked { lookahead: LOOKAHEAD });
        decoder.open(src.path(), &opts)?;
        let info = decoder.info.clone().unwrap();
        Ok(Player { decoder, rate: info.rate(), frames: info.frame_count as i64, clock: None, paused_at: None, stats: Stats::default(), last_shown: None, refreshes_on_current: 0, refresh_ms: 1000.0 / 60.0, current: None, looping: false })
    }
    pub fn info(&self) -> &SessionInfo { self.decoder.info.as_ref().unwrap() }
    pub fn started(&self) -> bool { self.clock.is_some() }
    pub fn paused(&self) -> bool { !self.decoder.is_playing() }
    pub fn at_end(&self) -> bool { self.decoder.state() == State::EndOfStream }
    pub fn current(&self) -> Option<&FrameHandle> { self.current.as_ref() }
    pub fn clock_frame(&self, now: Instant) -> i64 { match self.clock { Some((t0, f0)) => f0 + ((now - t0).as_secs_f64() * self.rate.as_f64()).floor() as i64, None => 0 } }
    pub fn toggle(&mut self, now: Instant) {
        if self.paused() { if let (Some(p), Some((t0, f0))) = (self.paused_at.take(), self.clock) { self.clock = Some((t0 + (now - p), f0)); } self.decoder.play(); }
        else { self.decoder.pause(); self.paused_at = Some(now); }
    }
    /// The frame to show on this refresh, if a new one is due.
    pub fn tick(&mut self, now: Instant) -> Option<&FrameHandle> {
        if self.clock.is_none() {
            let (h, _) = self.decoder.fetch_due(i32::MAX);
            let h = h?;
            self.clock = Some((now, h.frame_no as i64));
            return self.show(h, now);
        }
        if self.paused() && !self.at_end() { return None }
        let target = self.clock_frame(now);
        if self.at_end() && self.decoder.queued() == 0 {
            if self.looping { self.decoder.seek_to_frame(0); self.decoder.play(); self.clock = None; self.last_shown = None; self.stats.loops += 1; }
            return None;
        }
        let (h, dropped) = self.decoder.fetch_due(target.min(i32::MAX as i64) as i32);
        self.stats.dropped += dropped;
        let h = h?;
        self.show(h, now)
    }
    fn show(&mut self, h: FrameHandle, now: Instant) -> Option<&FrameHandle> {
        let k = h.frame_no as i64;
        if let Some(last) = self.last_shown { if k > last + 1 { self.stats.dropped += (k - last - 1) as u32; } *self.stats.cadence.entry(self.refreshes_on_current).or_insert(0) += 1; }
        if let Some((t0, f0)) = self.clock {
            let ideal = t0 + Duration::from_secs_f64((k - f0).max(0) as f64 * self.rate.frame_seconds());
            let late = if now >= ideal { (now - ideal).as_secs_f32() * 1000.0 } else { -((ideal - now).as_secs_f32() * 1000.0) };
            self.stats.lateness_ms.push(late); if late > self.refresh_ms { self.stats.late += 1; }
        }
        self.stats.decode_ms.push(h.decode_ms);
        self.stats.shown += 1; self.last_shown = Some(k); self.refreshes_on_current = 1;
        self.current = Some(h);
        self.current.as_ref()
    }
    /// A refresh that showed nothing new (cadence accounting).
    pub fn idle_refresh(&mut self) { if self.last_shown.is_none() { self.stats.black += 1; } else { self.refreshes_on_current += 1; } }
}
