//! The QCAA audio ring (QCView / QCBridgeAE `audio_ring.{h,cpp}`, v1): the CONSUMER — QCView's `next_packet` (in order,
//! resync just behind the newest packet when lapped or replaced, the packet's own `packet_seq` checked: a stale slot agrees
//! with itself), `skip_to_latest`, the seqlocked session — and a PRODUCER for the tests (QCView's `push`).
use crate::layout::*;
use crate::mapping::{self, Mapping};
use std::sync::atomic::Ordering::*;

pub const MAX_CHANNELS: u32 = 16;
pub const DEFAULT_SLOTS: u32 = 128;
pub const DEFAULT_SLOT_FRAMES: u32 = 1024;
pub const MIN_SLOTS: u32 = 4;
const PLANE_ALIGNMENT: u64 = 64;
pub const SESSION_SCRUBBING: u32 = 1 << 1;
pub const PACKET_SCRUBBING: u32 = 1 << 0;
pub const PACKET_MUTED: u32 = 1 << 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum State { Idle, Pushing, Retired }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum HostAudio { Unknown, Off, On }
#[derive(Debug, PartialEq, Eq)] pub enum Next { None, Packet, Resynced }

pub struct AudioRing { pub map: Mapping, write_seq: u64, write_frames: u64 }

impl AudioRing {
    pub fn header(&self) -> &AudioRingHeader { unsafe { &*(self.map.base as *const AudioRingHeader) } }
    fn slot(&self, index: u32) -> *mut AudioSlotHeader { let h = self.header(); unsafe { self.map.base.add((h.slots_offset + index as u64 * h.slot_stride) as usize) as *mut AudioSlotHeader } }
    fn planes(&self, index: u32) -> *mut f32 { let h = self.header(); unsafe { self.map.base.add((h.slots_offset + index as u64 * h.slot_stride + h.samples_offset) as usize) as *mut f32 } }

    /// QCView `AudioRing::open`: magic + exact version, then the geometry must fit the mapping.
    pub fn open(name: &str) -> Result<AudioRing, String> {
        let map = Mapping::open(name, std::mem::size_of::<AudioRingHeader>() as u64, AUDIO_RING_MAGIC, AUDIO_DESC_VERSION, |p| unsafe { std::ptr::read_volatile(&(*(p as *const AudioRingHeader)).total_size) })?;
        let r = AudioRing { map, write_seq: 0, write_frames: 0 };
        let h = r.header();
        let need = h.slots_offset + h.slot_count as u64 * h.slot_stride;
        if h.slot_count < MIN_SLOTS || h.slot_frames == 0 || h.max_channels == 0 || h.max_channels > MAX_CHANNELS
            || h.samples_offset + h.slot_frames as u64 * 4 * h.max_channels as u64 > h.slot_stride || need > r.map.size as u64 {
            return Err("audio ring geometry does not fit its mapping".into())
        }
        Ok(r)
    }
    /// The producer side (tests, and a synthetic host): QCView `AudioRing::create`.
    pub fn create(name: &str, time_scale: i64, slot_count: u32, slot_frames: u32, max_channels: u32) -> Result<AudioRing, String> {
        if slot_count < MIN_SLOTS || slot_frames == 0 || max_channels == 0 || max_channels > MAX_CHANNELS || time_scale <= 0 { return Err("bad audio ring geometry".into()) }
        let ps = mapping::page_size();
        let head = mapping::round_up(std::mem::size_of::<AudioRingHeader>() as u64, ps);
        let samples_off = mapping::round_up(std::mem::size_of::<AudioSlotHeader>() as u64, PLANE_ALIGNMENT);
        let stride = mapping::round_up(samples_off + slot_frames as u64 * 4 * max_channels as u64, PLANE_ALIGNMENT);
        let total = head + stride * slot_count as u64;
        let map = Mapping::create(name, total)?;
        let h = unsafe { &mut *(map.base as *mut AudioRingHeader) };
        h.magic = AUDIO_RING_MAGIC; h.version = AUDIO_DESC_VERSION; h.producer_pid = mapping::self_pid(); h.slot_count = slot_count; h.slot_frames = slot_frames; h.max_channels = max_channels;
        h.slot_stride = stride; h.slots_offset = head; h.samples_offset = samples_off; h.total_size = total; h.time_scale = time_scale;
        Ok(AudioRing { map, write_seq: 0, write_frames: 0 })
    }
    pub fn begin_session(&self, s: AudioSession) {
        let h = unsafe { &mut *(self.map.base as *mut AudioRingHeader) };
        let q = h.session_seq.load(Relaxed); h.session_seq.store(q + 1, Release); std::sync::atomic::fence(Release);
        h.session = s; h.generation.fetch_add(1, Release); h.session_seq.store(q + 2, Release); h.state.store(1, Release);
    }
    pub fn set_state(&self, s: State) { self.header().state.store(match s { State::Idle => 0, State::Pushing => 1, State::Retired => 2 }, Release); }
    pub fn set_host_audio(&self, a: HostAudio) { self.header().host_audio.store(match a { HostAudio::Unknown => 0, HostAudio::Off => 1, HostAudio::On => 2 }, Release); }
    /// QCView `push`: `frames` of `planes` as packets of ≤ slot_frames; never blocks.
    pub fn push(&mut self, planes: &[&[f32]], sample_rate: u32, mut frames: u32, mut time_value: i64, flags: u32) -> u32 {
        let h = self.header(); let (count, slot_frames) = (h.slot_count, h.slot_frames); let ch = (planes.len() as u32).min(h.max_channels); let ts = h.time_scale;
        let mut written = 0; let mut offset = 0usize;
        while frames > 0 {
            let n = frames.min(slot_frames); let seq = self.write_seq + 1; let index = ((seq - 1) % count as u64) as u32;
            let slot = unsafe { &mut *self.slot(index) }; let dst = self.planes(index);
            let s = slot.seq.load(Relaxed); slot.seq.store(s + 1, Release); std::sync::atomic::fence(Release);
            for c in 0..ch as usize { unsafe { std::ptr::copy_nonoverlapping(planes[c][offset..].as_ptr(), dst.add(c * slot_frames as usize), n as usize) } }
            slot.desc.packet_seq = seq; slot.desc.first_frame = self.write_frames; slot.desc.time_value = time_value; slot.desc.frames = n; slot.desc.channels = ch; slot.desc.sample_rate = sample_rate; slot.desc.flags = flags;
            slot.seq.store(s + 2, Release); self.header().latest.store(seq, Release);
            self.write_seq = seq; self.write_frames += n as u64; time_value += n as i64 * ts / sample_rate as i64; offset += n as usize; frames -= n; written += 1;
        }
        written
    }

    // ---- the consumer ----
    pub fn skip_to_latest(&self, last_seen: &mut u64) { *last_seen = self.header().latest.load(Acquire); }
    /// QCView `next_packet`: packet `last_seen + 1` into `planes_out` (room for max_channels × slot_frames; plane c at
    /// c × slot_frames), or Resynced (`last_seen` moved just behind the newest, `dropped` counts the skipped) — call again.
    pub fn next_packet(&self, last_seen: &mut u64, out: &mut AudioPacketDesc, planes_out: &mut [f32], dropped: &mut u64) -> Next {
        let h = self.header();
        let latest = h.latest.load(Acquire);
        if latest == 0 || latest == *last_seen { return Next::None }
        let count = h.slot_count as u64;
        // behind by a ring's worth (or ahead: a replaced ring): rejoin just behind the newest, the writer's next slot left alone
        fn resync(latest: u64, count: u64, last_seen: &mut u64, dropped: &mut u64) -> Next { let target = latest.saturating_sub(count - 2); if target > *last_seen { *dropped += target - *last_seen; } *last_seen = target; Next::Resynced }
        if *last_seen > latest || latest - *last_seen > count - 2 { return resync(latest, count, last_seen, dropped) }
        let p = *last_seen + 1; let index = ((p - 1) % count) as u32;
        let slot = unsafe { &*self.slot(index) };
        let s1 = slot.seq.load(Acquire);
        if s1 & 1 != 0 { return resync(latest, count, last_seen, dropped) }
        let desc = slot.desc;
        let slot_frames = h.slot_frames as usize;
        if desc.frames > h.slot_frames || desc.channels > h.max_channels || planes_out.len() < desc.channels as usize * slot_frames { return resync(latest, count, last_seen, dropped) }
        let src = self.planes(index);
        for c in 0..desc.channels as usize { unsafe { std::ptr::copy_nonoverlapping(src.add(c * slot_frames), planes_out[c * slot_frames..].as_mut_ptr(), desc.frames as usize) } }
        std::sync::atomic::fence(Acquire);
        let s2 = slot.seq.load(Acquire);
        if s1 != s2 || desc.packet_seq != p { return resync(latest, count, last_seen, dropped) }
        *last_seen = p; *out = desc;
        Next::Packet
    }
    /// The session when its generation moved (QCView `read_session`).
    pub fn read_session(&self, known_generation: &mut u64) -> Option<AudioSession> {
        let h = self.header();
        for _ in 0..8 {
            let q1 = h.session_seq.load(Acquire); if q1 & 1 != 0 { continue }
            let g = h.generation.load(Acquire); if g == 0 || g == *known_generation { return None }
            let s = unsafe { std::ptr::read_volatile(&h.session) };
            std::sync::atomic::fence(Acquire);
            if h.session_seq.load(Acquire) != q1 { continue }
            *known_generation = g; return Some(s)
        }
        None
    }
    pub fn state(&self) -> State { match self.header().state.load(Acquire) { 0 => State::Idle, 1 => State::Pushing, _ => State::Retired } }
    pub fn host_audio(&self) -> HostAudio { match self.header().host_audio.load(Acquire) { 1 => HostAudio::Off, 2 => HostAudio::On, _ => HostAudio::Unknown } }
    pub fn slot_frames(&self) -> u32 { self.header().slot_frames }
    pub fn max_channels(&self) -> u32 { self.header().max_channels }
    pub fn producer_alive(&self) -> bool { mapping::process_alive(self.header().producer_pid) }
}
