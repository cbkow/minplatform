//! S4 — the `ringslot::` discipline, generic over the ring header / slot header / desc (vendor/qcbae `ring_slots.h`):
//! the writer picks the next slot skipping the reader's claim (≥ 3 slots ⇒ one always free), seq odd → write → seq even → `latest`
//! release; the reader loads `latest`, stores `reader_claim` BEFORE reading, double-checks the seqlock; the slot comes from the
//! packed value, never from the sequence. Latest-wins; the producer never waits.
use std::sync::atomic::{AtomicU64, Ordering::*};

pub const SLOT_BITS: u32 = 8; pub const SLOT_MASK: u64 = (1 << SLOT_BITS) - 1;
pub const fn pack_latest(seq: u64, slot: u32) -> u64 { (seq << SLOT_BITS) | (slot as u64 & SLOT_MASK) }
pub const fn latest_seq(p: u64) -> u64 { p >> SLOT_BITS }
pub const fn latest_slot(p: u64) -> u32 { (p & SLOT_MASK) as u32 }

/// What a ring header must expose for the discipline (QCAE, QCVO and QCAA headers all do, by the same names).
pub trait RingLike {
    fn slot_count(&self) -> u32; fn slot_stride(&self) -> u64; fn slots_offset(&self) -> u64; fn pixels_offset(&self) -> u64; fn pixels_capacity(&self) -> u64;
    fn latest(&self) -> &AtomicU64; fn reader_claim(&self) -> &AtomicU64;
}
pub trait SlotLike { type Desc: Copy; fn seq(&self) -> &AtomicU64; fn desc(&self) -> &Self::Desc; fn desc_mut(&mut self) -> &mut Self::Desc; }

pub unsafe fn slot_at<H: RingLike, S>(base: *mut u8, h: &H, i: u32) -> *mut S { unsafe { base.add((h.slots_offset() + i as u64 * h.slot_stride()) as usize) as *mut S } }
pub unsafe fn pixels_at<H: RingLike>(base: *mut u8, h: &H, i: u32) -> *mut u8 { unsafe { base.add((h.slots_offset() + i as u64 * h.slot_stride() + h.pixels_offset()) as usize) } }

#[derive(Default)] pub struct Writer { pub slot: Option<u32>, pub seq: u64 }

pub unsafe fn begin_write<H: RingLike, S: SlotLike>(base: *mut u8, h: &H, w: &mut Writer, bytes: u64) -> Option<*mut u8> { unsafe {
    if w.slot.is_some() || bytes > h.pixels_capacity() { return None }
    let packed = h.latest().load(Relaxed); let next = latest_seq(packed) + 1; let count = h.slot_count();
    let claim = h.reader_claim().load(Acquire);
    let mut index = if packed == 0 { 0 } else { (latest_slot(packed) + 1) % count };
    if claim != 0 && index == latest_slot(claim) { index = (index + 1) % count; }
    let slot = &*slot_at::<H, S>(base, h, index);
    let s = slot.seq().load(Relaxed); slot.seq().store(s + 1, Release);   // odd: write in progress
    std::sync::atomic::fence(Release);
    w.slot = Some(index); w.seq = next;
    Some(pixels_at(base, h, index))
} }
pub unsafe fn commit<H: RingLike, S: SlotLike>(base: *mut u8, h: &H, w: &mut Writer, desc: S::Desc) { unsafe {
    let Some(i) = w.slot.take() else { return };
    let slot = &mut *slot_at::<H, S>(base, h, i);
    *slot.desc_mut() = desc;
    let s = slot.seq().load(Relaxed); slot.seq().store(s + 1, Release);   // even: stable
    h.latest().store(pack_latest(w.seq, i), Release);
} }
pub unsafe fn abandon<H: RingLike, S: SlotLike>(base: *mut u8, h: &H, w: &mut Writer) { unsafe {
    let Some(i) = w.slot.take() else { return };
    let slot = &*slot_at::<H, S>(base, h, i); let s = slot.seq().load(Relaxed); slot.seq().store(s + 1, Release);
} }
/// The consumer: the newest frame if it is newer than `last_seen`; holds the slot (`held`) until `release`.
pub unsafe fn acquire_latest<H: RingLike, S: SlotLike>(base: *mut u8, h: &H, held: &mut u64, last_seen: &mut u64) -> Option<(S::Desc, *const u8)> { unsafe {
    acquire_latest_seq::<H, S>(base, h, held, last_seen).map(|(d, p, _)| (d, p))
} }
/// [`acquire_latest`] with the slot's validated (even) sequence — a caller that copies the pixels compares it afterwards.
pub unsafe fn acquire_latest_seq<H: RingLike, S: SlotLike>(base: *mut u8, h: &H, held: &mut u64, last_seen: &mut u64) -> Option<(S::Desc, *const u8, u64)> { unsafe {
    let packed = h.latest().load(Acquire);
    if packed == 0 { return None }
    let seq = latest_seq(packed); let index = latest_slot(packed);
    if seq == *last_seen || index >= h.slot_count() { return None }
    let slot = &*slot_at::<H, S>(base, h, index);
    h.reader_claim().store(packed, Release);
    let s1 = slot.seq().load(Acquire);
    if s1 & 1 != 0 { h.reader_claim().store(*held, Release); return None }
    let desc = *slot.desc();
    std::sync::atomic::fence(Acquire);
    let s2 = slot.seq().load(Acquire);
    if s1 != s2 { h.reader_claim().store(*held, Release); return None }
    *held = packed; *last_seen = seq;
    Some((desc, pixels_at(base, h, index), s1))
} }
pub fn release<H: RingLike>(h: &H, held: &mut u64) { *held = 0; h.reader_claim().store(0, Release); }

/// Seqlocked single records (request / reply / status / HDR info): writer bumps lock odd → writes → generation++ → lock even.
pub unsafe fn write_record<T: Copy>(lock: &AtomicU64, generation: &AtomicU64, record: *mut T, value: T) { unsafe {
    let s = lock.load(Relaxed); lock.store(s + 1, Release); std::sync::atomic::fence(Release);
    std::ptr::write_volatile(record, value);
    generation.fetch_add(1, Release); lock.store(s + 2, Release);
} }
pub unsafe fn read_record<T: Copy>(lock: &AtomicU64, generation: &AtomicU64, record: *const T, known: &mut u64) -> Option<T> { unsafe {
    for _ in 0..8 {
        let s1 = lock.load(Acquire); if s1 & 1 != 0 { continue }
        let g = generation.load(Acquire); if g == *known || g == 0 { return None }
        let v = std::ptr::read_volatile(record);
        std::sync::atomic::fence(Acquire);
        if lock.load(Acquire) == s1 { *known = g; return Some(v) }
    }
    None
} }
