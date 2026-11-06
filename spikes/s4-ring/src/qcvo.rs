//! S4 — the QCVO output ring (`output_ring`): QCView's PRODUCER end (create, answer requests, publish, watch the sink) and the
//! sink's end for the self-test (open, attach, request, heartbeat, take frames).
use crate::layout::*;
use crate::mapping::{self, Mapping};
use crate::ringslot::{self, RingLike, SlotLike, Writer};
use std::sync::atomic::{AtomicU64, Ordering::*};

impl RingLike for OutputRingHeader { fn slot_count(&self) -> u32 { self.slot_count } fn slot_stride(&self) -> u64 { self.slot_stride } fn slots_offset(&self) -> u64 { self.slots_offset } fn pixels_offset(&self) -> u64 { self.pixels_offset } fn pixels_capacity(&self) -> u64 { self.pixels_capacity } fn latest(&self) -> &AtomicU64 { &self.latest } fn reader_claim(&self) -> &AtomicU64 { &self.reader_claim } }
impl SlotLike for OutputSlotHeader { type Desc = OutputFrameDesc; fn seq(&self) -> &AtomicU64 { &self.seq } fn desc(&self) -> &OutputFrameDesc { &self.desc } fn desc_mut(&mut self) -> &mut OutputFrameDesc { &mut self.desc } }

/// The bus clock: `mach_absolute_time` in ns on macOS (the sinks compare heartbeats against it), QPC on Windows.
pub fn now_ns() -> i64 {
    #[cfg(target_os = "macos")] unsafe {
        static TB: std::sync::OnceLock<(u32, u32)> = std::sync::OnceLock::new();
        let (n, d) = *TB.get_or_init(|| { let mut t = libc::mach_timebase_info { numer: 0, denom: 0 }; libc::mach_timebase_info(&mut t); (t.numer, t.denom) });
        (libc::mach_absolute_time() as u128 * n as u128 / d as u128) as i64
    }
    #[cfg(windows)] unsafe {
        // QPC in ns — exactly what MSVC's steady_clock::now() is, so a C++ producer's time_value compares to ours.
        use windows_sys::Win32::System::Performance::*;
        static F: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
        let f = *F.get_or_init(|| { let mut f = 0i64; QueryPerformanceFrequency(&mut f); f });
        let mut c = 0i64; QueryPerformanceCounter(&mut c);
        ((c as i128 * 1_000_000_000) / f as i128) as i64
    }
    #[cfg(not(any(target_os = "macos", windows)))] { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as i64 }
}
pub fn name_for(id: &str) -> String { format!("{OUTPUT_RING_PREFIX}{id}") }

pub struct OutputRing { pub map: Mapping, writer: Writer, pub held: u64, pub last_seen: u64, known_request: u64, known_reply: u64, known_status: u64 }
impl OutputRing {
    pub fn header(&self) -> &OutputRingHeader { unsafe { &*(self.map.base as *const OutputRingHeader) } }
    pub fn create(name: &str, pixels_capacity: u64, slot_count: u32, header_flags: u32) -> Result<OutputRing, String> {
        let ps = mapping::page_size();
        let pix_off = mapping::round_up(std::mem::size_of::<OutputSlotHeader>() as u64, ps);
        let pix_cap = mapping::round_up(pixels_capacity, ps);
        let stride = pix_off + pix_cap;
        let head = mapping::round_up(std::mem::size_of::<OutputRingHeader>() as u64, ps);
        let total = head + stride * slot_count as u64;
        let map = Mapping::create(name, total)?;
        let h = unsafe { &mut *(map.base as *mut OutputRingHeader) };
        h.magic = OUTPUT_RING_MAGIC; h.version = OUTPUT_DESC_VERSION; h.slot_count = slot_count; h.producer_pid = mapping::self_pid();
        h.slot_stride = stride; h.pixels_offset = pix_off; h.pixels_capacity = pix_cap; h.slots_offset = head; h.total_size = total;
        h.producer_protocol = OUTPUT_DESC_VERSION; h.header_flags = header_flags | OUTPUT_HEADER_HAS_HDR_INFO;
        Ok(OutputRing { map, writer: Writer::default(), held: 0, last_seen: 0, known_request: 0, known_reply: 0, known_status: 0 })
    }
    pub fn open(name: &str) -> Result<OutputRing, String> {
        let map = Mapping::open(name, std::mem::size_of::<OutputRingHeader>() as u64, OUTPUT_RING_MAGIC, OUTPUT_DESC_VERSION, |p| unsafe { std::ptr::read_volatile(&(*(p as *const OutputRingHeader)).total_size) })?;
        Ok(OutputRing { map, writer: Writer::default(), held: 0, last_seen: 0, known_request: 0, known_reply: 0, known_status: 0 })
    }
    // ----- producer -----
    pub fn write_hdr_info(&self, info: OutputHdrInfo) { let bp = unsafe { self.map.base.add(OUTPUT_HDR_INFO_OFFSET as usize) } as *mut OutputHdrBlock; unsafe { let b = &*bp; ringslot::write_record(&b.lock, &b.generation, std::ptr::addr_of_mut!((*bp).info), info) } }
    pub fn read_request(&mut self) -> Option<OutputRequest> { let h = unsafe { &*(self.header() as *const OutputRingHeader) }; unsafe { ringslot::read_record(&h.request_lock, &h.request_generation, &h.request, &mut self.known_request) } }
    pub fn request_generation(&self) -> u64 { self.header().request_generation.load(Acquire) }
    pub fn write_reply(&self, reply: OutputReply, request_generation: u64) { let hp = self.map.base as *mut OutputRingHeader; unsafe { let h = &*hp; std::ptr::write_volatile(std::ptr::addr_of_mut!((*hp).reply_for), request_generation); ringslot::write_record(&h.reply_lock, &h.reply_generation, std::ptr::addr_of_mut!((*hp).reply), reply) } }
    pub fn consumer_present(&self, now: i64) -> bool { let h = self.header(); h.consumer_pid.load(Acquire) != 0 && now - h.heartbeat_ns.load(Acquire) < 1_000_000_000 }
    pub fn read_status(&mut self) -> Option<String> { let h = unsafe { &*(self.header() as *const OutputRingHeader) }; let s: Option<[u8; OUTPUT_STATUS_BYTES]> = unsafe { ringslot::read_record(&h.status_lock, &h.status_generation, &h.status, &mut self.known_status) }; s.map(|b| String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).into_owned()) }
    pub fn begin_write(&mut self, bytes: u64) -> Option<*mut u8> { let h = self.header() as *const OutputRingHeader; unsafe { ringslot::begin_write::<OutputRingHeader, OutputSlotHeader>(self.map.base, &*h, &mut self.writer, bytes) } }
    pub fn commit(&mut self, desc: OutputFrameDesc) { let h = self.header() as *const OutputRingHeader; unsafe { ringslot::commit::<OutputRingHeader, OutputSlotHeader>(self.map.base, &*h, &mut self.writer, desc) } }
    pub fn set_state(&self, s: OutputState) { self.header().state.store(s as u32, Release); }
    // ----- sink -----
    pub fn attach(&self, now: i64) { let h = self.header(); h.consumer_protocol.store(OUTPUT_DESC_VERSION, Release); h.heartbeat_ns.store(now, Release); h.consumer_pid.store(mapping::self_pid(), Release); }
    pub fn heartbeat(&self, now: i64) { self.header().heartbeat_ns.store(now, Release); }
    pub fn report_seen(&self, seq: u64) { self.header().last_seen_seq.store(seq, Release); }
    pub fn write_request(&self, req: OutputRequest) { let hp = self.map.base as *mut OutputRingHeader; unsafe { let h = &*hp; ringslot::write_record(&h.request_lock, &h.request_generation, std::ptr::addr_of_mut!((*hp).request), req) } }
    pub fn read_reply(&mut self) -> Option<(OutputReply, u64)> { let h = unsafe { &*(self.header() as *const OutputRingHeader) }; let r: Option<OutputReply> = unsafe { ringslot::read_record(&h.reply_lock, &h.reply_generation, &h.reply, &mut self.known_reply) }; r.map(|r| (r, h.reply_for)) }
    pub fn write_status(&self, text: &str) { let h = self.header(); let mut b = [0u8; OUTPUT_STATUS_BYTES]; let n = text.len().min(OUTPUT_STATUS_BYTES - 1); b[..n].copy_from_slice(&text.as_bytes()[..n]); let hp = self.map.base as *mut OutputRingHeader; unsafe { ringslot::write_record(&h.status_lock, &h.status_generation, std::ptr::addr_of_mut!((*hp).status), b) } }
    pub fn acquire_latest(&mut self) -> Option<(OutputFrameDesc, *const u8)> { let h = self.header() as *const OutputRingHeader; unsafe { ringslot::acquire_latest::<OutputRingHeader, OutputSlotHeader>(self.map.base, &*h, &mut self.held, &mut self.last_seen) } }
    pub fn release(&mut self) { let h = self.header() as *const OutputRingHeader; ringslot::release(unsafe { &*h }, &mut self.held); }
    pub fn producer_alive(&self) -> bool { mapping::process_alive(self.header().producer_pid) }
}
