//! `AudioRingBuffer` (QCView `audio/audio_ring_buffer.h`), in f32 SAMPLES rather than bytes: a single-producer /
//! single-consumer ring whose positions are monotonic counters. A seek never clears it — the producer MARKS the stale
//! boundary (`mark_stale`) and bumps a generation; the consumer drains the pre-seek tail through a fade and then
//! `discard_stale`s (the drain stage, `drain.rs`). Producer = the decode thread, consumer = the device callback.
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::*};

pub struct AudioRing {
    capacity: usize,
    buf: UnsafeCell<Vec<f32>>,
    write_pos: AtomicU64,
    read_pos: AtomicU64,
    stale_pos: AtomicU64,
    stale_gen: AtomicU32,
}
// SPSC by contract: exactly one writer thread and one reader thread touch the buffer regions they own.
unsafe impl Sync for AudioRing {}
unsafe impl Send for AudioRing {}

impl AudioRing {
    /// `capacity` in samples (stereo frames × 2).
    pub fn new(capacity: usize) -> AudioRing { AudioRing { capacity, buf: UnsafeCell::new(vec![0.0; capacity]), write_pos: AtomicU64::new(0), read_pos: AtomicU64::new(0), stale_pos: AtomicU64::new(0), stale_gen: AtomicU32::new(0) } }
    pub fn capacity(&self) -> usize { self.capacity }
    pub fn clear(&self) { self.write_pos.store(0, Release); self.read_pos.store(0, Release); self.stale_pos.store(0, Release); }
    /// Consumer: drop everything queued.
    pub fn discard_all(&self) { self.read_pos.store(self.write_pos.load(Acquire), Release); }
    /// Producer: everything written so far is pre-seek; the consumer fades it out and discards it.
    pub fn mark_stale(&self) { self.stale_pos.store(self.write_pos.load(Relaxed), Release); self.stale_gen.fetch_add(1, Release); }
    pub fn stale_generation(&self) -> u32 { self.stale_gen.load(Acquire) }
    pub fn stale_samples(&self) -> usize { let s = self.stale_pos.load(Acquire); let r = self.read_pos.load(Relaxed); if s > r { (s - r) as usize } else { 0 } }
    pub fn discard_stale(&self) { let s = self.stale_pos.load(Acquire); let r = self.read_pos.load(Relaxed); if s > r { self.read_pos.store(s, Release); } }
    /// Samples written after the stale mark (the producer's back-pressure measure).
    pub fn fresh_samples(&self) -> usize { let w = self.write_pos.load(Relaxed); let r = self.read_pos.load(Acquire); let s = self.stale_pos.load(Relaxed); let base = if s > r { s } else { r }; if w >= base { (w - base) as usize } else { 0 } }
    pub fn available_read(&self) -> usize { let w = self.write_pos.load(Acquire); let r = self.read_pos.load(Relaxed); if w >= r { (w - r) as usize } else { 0 } }
    pub fn available_write(&self) -> usize { self.capacity - self.available_read() }
    /// Producer: returns how many samples were written (≤ `data.len()`).
    pub fn write(&self, data: &[f32]) -> usize {
        let n = data.len().min(self.available_write()); if n == 0 { return 0 }
        let w = (self.write_pos.load(Relaxed) % self.capacity as u64) as usize;
        let buf = unsafe { &mut *self.buf.get() };
        let first = (self.capacity - w).min(n);
        buf[w..w + first].copy_from_slice(&data[..first]);
        if first < n { buf[..n - first].copy_from_slice(&data[first..n]); }
        self.write_pos.fetch_add(n as u64, Release);
        n
    }
    /// Consumer: returns how many samples were read (≤ `out.len()`); the rest of `out` is untouched.
    pub fn read(&self, out: &mut [f32]) -> usize {
        let n = out.len().min(self.available_read()); if n == 0 { return 0 }
        let r = (self.read_pos.load(Relaxed) % self.capacity as u64) as usize;
        let buf = unsafe { &*self.buf.get() };
        let first = (self.capacity - r).min(n);
        out[..first].copy_from_slice(&buf[r..r + first]);
        if first < n { out[first..n].copy_from_slice(&buf[..n - first]); }
        self.read_pos.fetch_add(n as u64, Release);
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wraps_and_marks_stale() {
        let r = AudioRing::new(8);
        assert_eq!(r.write(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), 6);
        let mut o = [0.0; 4]; assert_eq!(r.read(&mut o), 4); assert_eq!(o, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(r.write(&[7.0, 8.0, 9.0, 10.0, 11.0]), 5, "wraps; one slot left");
        r.mark_stale(); assert_eq!(r.stale_generation(), 1); assert_eq!(r.stale_samples(), 7);
        assert_eq!(r.write(&[12.0]), 1); assert_eq!(r.fresh_samples(), 1);
        r.discard_stale(); let mut o = [0.0; 2]; assert_eq!(r.read(&mut o), 1); assert_eq!(o[0], 12.0);
    }
}
