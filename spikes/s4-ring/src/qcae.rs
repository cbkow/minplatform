//! S4 — the QCAE ring (`shared_ring`): create (the test producer) / open (QCView's end) + the discipline bound to its header.
use crate::layout::*;
use crate::mapping::{self, Mapping};
use crate::ringslot::{self, RingLike, SlotLike, Writer};
use std::sync::atomic::{AtomicU64, Ordering::*};

impl RingLike for RingHeader { fn slot_count(&self) -> u32 { self.slot_count } fn slot_stride(&self) -> u64 { self.slot_stride } fn slots_offset(&self) -> u64 { self.slots_offset } fn pixels_offset(&self) -> u64 { self.pixels_offset } fn pixels_capacity(&self) -> u64 { self.pixels_capacity } fn latest(&self) -> &AtomicU64 { &self.latest } fn reader_claim(&self) -> &AtomicU64 { &self.reader_claim } }
impl SlotLike for SlotHeader { type Desc = FrameDesc; fn seq(&self) -> &AtomicU64 { &self.seq } fn desc(&self) -> &FrameDesc { &self.desc } fn desc_mut(&mut self) -> &mut FrameDesc { &mut self.desc } }

pub struct SharedRing { pub map: Mapping, writer: Writer, pub held: u64, pub last_seen: u64 }
impl SharedRing {
    pub fn header(&self) -> &RingHeader { unsafe { &*(self.map.base as *const RingHeader) } }
    pub fn create(name: &str, pixels_capacity: u64, slot_count: u32) -> Result<SharedRing, String> {
        let ps = mapping::page_size();
        let icc_capacity = mapping::round_up(64 * 1024, ps);
        let pix_off = mapping::round_up(std::mem::size_of::<SlotHeader>() as u64, ps);
        let pix_cap = mapping::round_up(pixels_capacity, ps);
        let stride = pix_off + pix_cap;
        let head = mapping::round_up(std::mem::size_of::<RingHeader>() as u64, ps);
        let total = head + icc_capacity + stride * slot_count as u64;
        let map = Mapping::create(name, total)?;
        let h = unsafe { &mut *(map.base as *mut RingHeader) };
        h.magic = RING_MAGIC; h.version = FRAME_DESC_VERSION; h.slot_count = slot_count; h.producer_pid = mapping::self_pid();
        h.slot_stride = stride; h.pixels_offset = pix_off; h.pixels_capacity = pix_cap; h.icc_offset = head; h.icc_capacity = icc_capacity; h.slots_offset = head + icc_capacity; h.total_size = total;
        Ok(SharedRing { map, writer: Writer::default(), held: 0, last_seen: 0 })
    }
    pub fn open(name: &str) -> Result<SharedRing, String> {
        let map = Mapping::open(name, std::mem::size_of::<RingHeader>() as u64, RING_MAGIC, FRAME_DESC_VERSION, |p| unsafe { std::ptr::read_volatile(&(*(p as *const RingHeader)).total_size) })?;
        Ok(SharedRing { map, writer: Writer::default(), held: 0, last_seen: 0 })
    }
    pub fn begin_write(&mut self, bytes: u64) -> Option<*mut u8> { let h = self.header() as *const RingHeader; unsafe { ringslot::begin_write::<RingHeader, SlotHeader>(self.map.base, &*h, &mut self.writer, bytes) } }
    pub fn commit(&mut self, desc: FrameDesc) { let h = self.header() as *const RingHeader; unsafe { ringslot::commit::<RingHeader, SlotHeader>(self.map.base, &*h, &mut self.writer, desc) } }
    pub fn set_host_state(&self, s: HostState) { self.header().host_state.store(s as u32, Release); }
    pub fn host_state(&self) -> u32 { self.header().host_state.load(Acquire) }
    /// The newest frame newer than the last one taken; the slot stays claimed until `release`.
    pub fn acquire_latest(&mut self) -> Option<(FrameDesc, *const u8, u32)> {
        let h = self.header() as *const RingHeader;
        let r = unsafe { ringslot::acquire_latest::<RingHeader, SlotHeader>(self.map.base, &*h, &mut self.held, &mut self.last_seen) };
        r.map(|(d, p)| (d, p, ringslot::latest_slot(self.held)))
    }
    pub fn release(&mut self) { let h = self.header() as *const RingHeader; ringslot::release(unsafe { &*h }, &mut self.held); }
    pub fn producer_alive(&self) -> bool { mapping::process_alive(self.header().producer_pid) }
    /// Byte offset of slot `i`'s pixels from the mapping base (page-aligned by construction — the zero-copy wrap needs it).
    pub fn pixels_offset_of(&self, i: u32) -> u64 { let h = self.header(); h.slots_offset + i as u64 * h.slot_stride + h.pixels_offset }
}
