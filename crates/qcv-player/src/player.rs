//! The player: one streaming slot and the RATIONAL clock (S2's play loop — the clock starts when the first frame is
//! ready, each tick shows the newest ready frame at or before the clock frame; exact 3:2 cadence on 60 Hz falls out,
//! and per-frame lateness is a one-line statistic). Step 2 adds the scrub decoder, the second slot and the Transport.
use crate::{ffmpeg::{Decoded, OpenOptions, SeqOptions}, hw::HwDevice, slot::{Ready, Slot, SlotInfo}, source::Source, Error};
use qcv_timeline::Rate;
use std::{collections::BTreeMap, time::{Duration, Instant}};

#[derive(Default, Debug)]
pub struct Stats {
    /// shown-frame lateness vs the ideal clock, ms (negative = early)
    pub lateness_ms: Vec<f32>, /// refreshes per shown frame → count
    pub cadence: BTreeMap<i64, u32>, pub dropped: u32, pub late: u32, pub black: u32, pub shown: u32, pub decode_ms: Vec<f32>, pub loops: u32,
}

pub struct Player {
    slot: Slot, pub rate: Rate, pub frames: i64,
    clock: Option<(Instant, i64)>, ready: BTreeMap<i64, Ready>,
    paused: bool, paused_at: Option<Instant>,
    pub stats: Stats, last_shown: Option<i64>, refreshes_on_current: i64,
    /// one refresh in ms — the "late" threshold (16.667 at 60 Hz)
    pub refresh_ms: f32, current: Option<Decoded>, pub looping: bool,
}

impl Player {
    pub fn open(src: &Source, hw: Option<HwDevice>, threads: i32) -> Result<Player, Error> {
        let opts = OpenOptions { threads, hw, low_latency: false, seq: match src { Source::Sequence { rate, start, .. } => Some(SeqOptions { rate: *rate, start_number: *start }), _ => None } };
        let slot = Slot::open(src.path(), &opts, 0, None, 3)?;   // S1 held 3 frames ahead; the ring behind the presenter holds 3 more
        let (rate, frames) = (slot.info.rate, slot.info.frames);
        Ok(Player { slot, rate, frames, clock: None, ready: BTreeMap::new(), paused: false, paused_at: None, stats: Stats::default(), last_shown: None, refreshes_on_current: 0, refresh_ms: 1000.0 / 60.0, current: None, looping: true })
    }
    pub fn info(&self) -> &SlotInfo { &self.slot.info }
    pub fn started(&self) -> bool { self.clock.is_some() }
    pub fn paused(&self) -> bool { self.paused }
    pub fn current(&self) -> Option<&Decoded> { self.current.as_ref() }
    pub fn clock_frame(&self, now: Instant) -> i64 { match self.clock { Some((t0, f0)) => f0 + ((now - t0).as_secs_f64() * self.rate.as_f64()).floor() as i64, None => 0 } }
    pub fn toggle(&mut self, now: Instant) {
        if self.paused { if let (Some(p), Some((t0, f0))) = (self.paused_at.take(), self.clock) { self.clock = Some((t0 + (now - p), f0)); } self.paused = false; }
        else { self.paused = true; self.paused_at = Some(now); }
    }
    /// Frames decoded ahead of the clock that the player holds; the slot's channel holds as many again, and the presenter's
    /// ring three more. A decoder faster than real time (software ProRes at 250 fps, EXR) must block here, not fill memory.
    pub const LOOKAHEAD: usize = 3;
    fn pump(&mut self) {
        while self.ready.len() < Self::LOOKAHEAD {
            let Ok(r) = self.slot.rx.try_recv() else { break };
            if r.serial != self.slot.serial { continue }
            self.stats.decode_ms.push(r.decoded.decode_ms);
            let n = self.slot.frame_no(r.decoded.pts);
            self.ready.insert(n, r);
        }
    }
    /// The frame to show on this refresh, if a new one is due. The caller presents whatever it last showed otherwise.
    pub fn tick(&mut self, now: Instant) -> Option<&Decoded> {
        self.pump();
        if self.clock.is_none() {
            let Some((&k, _)) = self.ready.iter().next() else { return None };
            self.clock = Some((now, k));
        }
        if self.paused { return None }
        let target = self.clock_frame(now);
        if self.frames > 0 && target >= self.frames {
            if self.looping { self.slot.seek(0); self.ready.clear(); self.clock = None; self.last_shown = None; self.stats.loops += 1; }
            return None;
        }
        let candidate = self.ready.range(..=target).next_back().map(|(k, _)| *k);
        match candidate {
            Some(k) if Some(k) != self.last_shown => {
                if let Some(last) = self.last_shown { if k > last + 1 { self.stats.dropped += (k - last - 1) as u32; } *self.stats.cadence.entry(self.refreshes_on_current).or_insert(0) += 1; }
                let r = self.ready.remove(&k).unwrap();
                let (t0, f0) = self.clock.unwrap();
                let ideal = t0 + Duration::from_secs_f64((k - f0) as f64 * self.rate.frame_seconds());
                let late = if now >= ideal { (now - ideal).as_secs_f32() * 1000.0 } else { -((ideal - now).as_secs_f32() * 1000.0) };
                self.stats.lateness_ms.push(late); if late > self.refresh_ms { self.stats.late += 1; }
                self.stats.shown += 1; self.last_shown = Some(k); self.refreshes_on_current = 1;
                self.ready.retain(|f, _| *f > k);
                self.current = Some(r.decoded);
                self.current.as_ref()
            }
            Some(_) => { self.refreshes_on_current += 1; None }
            None => { if self.last_shown.is_none() { self.stats.black += 1; } else { self.refreshes_on_current += 1; } None }
        }
    }
}
