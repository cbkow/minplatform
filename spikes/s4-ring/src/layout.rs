//! S4 — the three wire layouts, `#[repr(C)]`, field for field from vendor/qcbae (QCAE v3, QCVO v1, QCAA v1). Atomics are
//! `AtomicU64` / `AtomicU32` (same size and alignment as `std::atomic<uint64_t>` / `<uint32_t>`, lock-free on every target we build).
#![allow(dead_code)]
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicI64};

pub const RING_MAGIC: u32 = 0x5143_4145;        // 'QCAE'
pub const FRAME_DESC_VERSION: u32 = 3;
pub const OUTPUT_RING_MAGIC: u32 = 0x5143_564F; // 'QCVO'
pub const OUTPUT_DESC_VERSION: u32 = 1;
pub const AUDIO_RING_MAGIC: u32 = 0x5143_4141;  // 'QCAA'
pub const AUDIO_DESC_VERSION: u32 = 1;
pub const OUTPUT_HDR_INFO_OFFSET: u64 = 1024;
pub const MAX_COMP_NAME: usize = 128;
pub const ROW_ALIGNMENT: u32 = 256;
pub const OUTPUT_STATUS_BYTES: usize = 128;
pub const OUTPUT_MESSAGE_BYTES: usize = 96;
pub const RING_NAME_AE: &str = "/qcbae-ae";
pub const RING_NAME_PREMIERE: &str = "/qcbae-premiere";
pub const OUTPUT_RING_PREFIX: &str = "/qcview-out-";

#[repr(u32)] #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum PixelFormat { Unknown = 0, Rgba16F = 1, Rgba8Unorm = 2, Rgba16Unorm = 3, Rgba32Float = 4 }
#[repr(u32)] #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum SourceTier { Unknown = 0, Int8 = 1, Int16 = 2, Float32 = 3 }
#[repr(u32)] #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum ChannelOrder { Unknown = 0, Rgba = 1, Argb = 2, Bgra = 3 }
pub const FLAG_PREMULTIPLIED: u32 = 1; pub const FLAG_HAS_INF: u32 = 2; pub const FLAG_HAS_NAN: u32 = 4;
#[repr(u32)] #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum HostState { Active = 0, PausedFocus = 1, Paused = 2, Retired = 3 }

pub const fn aligned_bytes_per_row(width: u32, bpp: u32) -> u32 { (width * bpp).div_ceil(ROW_ALIGNMENT) * ROW_ALIGNMENT }

#[repr(C)]
pub struct RingHeader {
    pub magic: u32, pub version: u32, pub slot_count: u32, pub producer_pid: u32,
    pub slot_stride: u64, pub pixels_offset: u64, pub pixels_capacity: u64, pub icc_offset: u64, pub icc_capacity: u64, pub slots_offset: u64, pub total_size: u64,
    pub latest: AtomicU64, pub reader_claim: AtomicU64, pub icc_seq: AtomicU64, pub icc_generation: AtomicU64, pub icc_len: AtomicU32, pub host_state: AtomicU32,
}
#[repr(C)] #[derive(Clone, Copy)]
pub struct FrameDesc {
    pub width: u32, pub height: u32, pub bytes_per_row: u32, pub pixel_format: u32, pub source_tier: u32, pub channel_order: u32, pub flags: u32, pub _pad0: u32,
    pub time_value: i64, pub time_scale: i64, pub icc_generation: u64, pub graphics_white: f32, pub value_scale: f32, pub comp_name: [u8; MAX_COMP_NAME],
}
#[repr(C)] pub struct SlotHeader { pub seq: AtomicU64, pub desc: FrameDesc }

// ---------- QCVO ----------
#[repr(u32)] #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum OutputPixelFormat { Unspecified = 0, Bgra8 = 1, Rgba16F = 2, Rgb10 = 3 }
#[repr(u32)] #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum OutputState { Active = 0, Paused = 2, Retired = 3 }
pub const OUTPUT_LAUNCHED_BY_PRODUCER: u32 = 1; pub const OUTPUT_HEADER_HAS_HDR_INFO: u32 = 2;
pub const OUTPUT_FRAME_HAS_A: u32 = 1; pub const OUTPUT_FRAME_TIMECODE_VALID: u32 = 16; pub const OUTPUT_FRAME_PAUSED: u32 = 64;
#[repr(C)] #[derive(Clone, Copy, Default)] pub struct OutputRequest { pub protocol_version: u32, pub width: u32, pub height: u32, pub pixel_format: u32, pub transfer: u32, pub fit: u32, pub content: u32, pub layers: u32, pub max_fps_num: u32, pub max_fps_den: u32, pub _pad0: [u32; 2] }
#[repr(C)] #[derive(Clone, Copy)] pub struct OutputReply { pub accepted: u32, pub refusal: u32, pub width: u32, pub height: u32, pub pixel_format: u32, pub transfer: u32, pub content: u32, pub layers: u32, pub message: [u8; OUTPUT_MESSAGE_BYTES] }
#[repr(C)] #[derive(Clone, Copy, Default)] pub struct OutputHdrInfo { pub transfer: u32, pub primaries: u32, pub max_mastering_nits: f32, pub min_mastering_nits: f32, pub max_cll: u32, pub max_fall: u32, pub _pad0: [u32; 2] }
#[repr(C)] pub struct OutputHdrBlock { pub lock: AtomicU64, pub generation: AtomicU64, pub info: OutputHdrInfo }
#[repr(C)] #[derive(Clone, Copy, Default)] pub struct OutputFrameDesc { pub width: u32, pub height: u32, pub bytes_per_row: u32, pub pixel_format: u32, pub transfer: u32, pub content: u32, pub layers: u32, pub flags: u32, pub presentation_ns: i64, pub source_frame_index: i64, pub source_fps_num: u32, pub source_fps_den: u32, pub timecode: u32, pub _pad0: u32 }
#[repr(C)]
pub struct OutputRingHeader {
    pub magic: u32, pub version: u32, pub slot_count: u32, pub producer_pid: u32,
    pub slot_stride: u64, pub pixels_offset: u64, pub pixels_capacity: u64, pub slots_offset: u64, pub total_size: u64,
    pub producer_protocol: u32, pub header_flags: u32,
    pub latest: AtomicU64, pub reader_claim: AtomicU64, pub state: AtomicU32, pub _pad0: u32,
    pub consumer_pid: AtomicU32, pub consumer_protocol: AtomicU32, pub heartbeat_ns: AtomicI64, pub last_seen_seq: AtomicU64,
    pub request_lock: AtomicU64, pub request_generation: AtomicU64, pub request: OutputRequest,
    pub status_lock: AtomicU64, pub status_generation: AtomicU64, pub status: [u8; OUTPUT_STATUS_BYTES],
    pub reply_lock: AtomicU64, pub reply_generation: AtomicU64, pub reply_for: u64, pub reply: OutputReply,
}
#[repr(C)] pub struct OutputSlotHeader { pub seq: AtomicU64, pub desc: OutputFrameDesc }

// ---------- QCAA ----------
#[repr(C)] #[derive(Clone, Copy, Default)] pub struct AudioSession { pub start_time: i64, pub in_time: i64, pub out_time: i64, pub speed: f32, pub sample_rate: u32, pub channels: u32, pub flags: u32, pub push_frames: u32, pub _pad0: u32 }
#[repr(C)] #[derive(Clone, Copy, Default)] pub struct AudioPacketDesc { pub packet_seq: u64, pub first_frame: u64, pub time_value: i64, pub frames: u32, pub channels: u32, pub sample_rate: u32, pub flags: u32 }
#[repr(C)]
pub struct AudioRingHeader {
    pub magic: u32, pub version: u32, pub producer_pid: u32, pub slot_count: u32, pub slot_frames: u32, pub max_channels: u32, pub _pad0: [u32; 2],
    pub slot_stride: u64, pub slots_offset: u64, pub samples_offset: u64, pub total_size: u64, pub time_scale: i64,
    pub latest: AtomicU64, pub state: AtomicU32, pub host_audio: AtomicU32, pub session_seq: AtomicU64, pub generation: AtomicU64, pub session: AudioSession,
}
#[repr(C)] pub struct AudioSlotHeader { pub seq: AtomicU64, pub desc: AudioPacketDesc }

// ---------- the byte-exact check against the C++ compiler ----------
unsafe extern "C" { fn qcbae_layout_count() -> u32; fn qcbae_layout_name(i: u32) -> *const std::ffi::c_char; fn qcbae_layout_value(i: u32) -> u64; }

/// Every size / offset the Rust side knows, by the same names the C++ check prints.
pub fn rust_table() -> Vec<(&'static str, u64)> {
    use std::mem::{offset_of, size_of};
    macro_rules! e { ($($n:literal => $v:expr),* $(,)?) => { vec![$(($n, $v as u64)),*] } }
    e! {
        "sizeof RingHeader" => size_of::<RingHeader>(), "RingHeader.magic" => offset_of!(RingHeader, magic), "RingHeader.version" => offset_of!(RingHeader, version), "RingHeader.slot_count" => offset_of!(RingHeader, slot_count), "RingHeader.producer_pid" => offset_of!(RingHeader, producer_pid), "RingHeader.slot_stride" => offset_of!(RingHeader, slot_stride), "RingHeader.pixels_offset" => offset_of!(RingHeader, pixels_offset), "RingHeader.pixels_capacity" => offset_of!(RingHeader, pixels_capacity), "RingHeader.icc_offset" => offset_of!(RingHeader, icc_offset), "RingHeader.icc_capacity" => offset_of!(RingHeader, icc_capacity), "RingHeader.slots_offset" => offset_of!(RingHeader, slots_offset), "RingHeader.total_size" => offset_of!(RingHeader, total_size), "RingHeader.latest" => offset_of!(RingHeader, latest), "RingHeader.reader_claim" => offset_of!(RingHeader, reader_claim), "RingHeader.icc_seq" => offset_of!(RingHeader, icc_seq), "RingHeader.icc_generation" => offset_of!(RingHeader, icc_generation), "RingHeader.icc_len" => offset_of!(RingHeader, icc_len), "RingHeader.host_state" => offset_of!(RingHeader, host_state),
        "sizeof SlotHeader" => size_of::<SlotHeader>(), "SlotHeader.seq" => offset_of!(SlotHeader, seq), "SlotHeader.desc" => offset_of!(SlotHeader, desc),
        "sizeof FrameDesc" => size_of::<FrameDesc>(), "FrameDesc.width" => offset_of!(FrameDesc, width), "FrameDesc.height" => offset_of!(FrameDesc, height), "FrameDesc.bytes_per_row" => offset_of!(FrameDesc, bytes_per_row), "FrameDesc.pixel_format" => offset_of!(FrameDesc, pixel_format), "FrameDesc.source_tier" => offset_of!(FrameDesc, source_tier), "FrameDesc.channel_order" => offset_of!(FrameDesc, channel_order), "FrameDesc.flags" => offset_of!(FrameDesc, flags), "FrameDesc.time_value" => offset_of!(FrameDesc, time_value), "FrameDesc.time_scale" => offset_of!(FrameDesc, time_scale), "FrameDesc.icc_generation" => offset_of!(FrameDesc, icc_generation), "FrameDesc.graphics_white" => offset_of!(FrameDesc, graphics_white), "FrameDesc.value_scale" => offset_of!(FrameDesc, value_scale), "FrameDesc.comp_name" => offset_of!(FrameDesc, comp_name),
        "sizeof OutputRingHeader" => size_of::<OutputRingHeader>(), "OutputRingHeader.magic" => offset_of!(OutputRingHeader, magic), "OutputRingHeader.version" => offset_of!(OutputRingHeader, version), "OutputRingHeader.slot_count" => offset_of!(OutputRingHeader, slot_count), "OutputRingHeader.producer_pid" => offset_of!(OutputRingHeader, producer_pid), "OutputRingHeader.slot_stride" => offset_of!(OutputRingHeader, slot_stride), "OutputRingHeader.pixels_offset" => offset_of!(OutputRingHeader, pixels_offset), "OutputRingHeader.pixels_capacity" => offset_of!(OutputRingHeader, pixels_capacity), "OutputRingHeader.slots_offset" => offset_of!(OutputRingHeader, slots_offset), "OutputRingHeader.total_size" => offset_of!(OutputRingHeader, total_size), "OutputRingHeader.producer_protocol" => offset_of!(OutputRingHeader, producer_protocol), "OutputRingHeader.header_flags" => offset_of!(OutputRingHeader, header_flags), "OutputRingHeader.latest" => offset_of!(OutputRingHeader, latest), "OutputRingHeader.reader_claim" => offset_of!(OutputRingHeader, reader_claim), "OutputRingHeader.state" => offset_of!(OutputRingHeader, state), "OutputRingHeader.consumer_pid" => offset_of!(OutputRingHeader, consumer_pid), "OutputRingHeader.consumer_protocol" => offset_of!(OutputRingHeader, consumer_protocol), "OutputRingHeader.heartbeat_ns" => offset_of!(OutputRingHeader, heartbeat_ns), "OutputRingHeader.last_seen_seq" => offset_of!(OutputRingHeader, last_seen_seq), "OutputRingHeader.request_lock" => offset_of!(OutputRingHeader, request_lock), "OutputRingHeader.request_generation" => offset_of!(OutputRingHeader, request_generation), "OutputRingHeader.request" => offset_of!(OutputRingHeader, request), "OutputRingHeader.status_lock" => offset_of!(OutputRingHeader, status_lock), "OutputRingHeader.status_generation" => offset_of!(OutputRingHeader, status_generation), "OutputRingHeader.status" => offset_of!(OutputRingHeader, status), "OutputRingHeader.reply_lock" => offset_of!(OutputRingHeader, reply_lock), "OutputRingHeader.reply_generation" => offset_of!(OutputRingHeader, reply_generation), "OutputRingHeader.reply_for" => offset_of!(OutputRingHeader, reply_for), "OutputRingHeader.reply" => offset_of!(OutputRingHeader, reply),
        "sizeof OutputSlotHeader" => size_of::<OutputSlotHeader>(), "OutputSlotHeader.seq" => offset_of!(OutputSlotHeader, seq), "OutputSlotHeader.desc" => offset_of!(OutputSlotHeader, desc),
        "sizeof OutputFrameDesc" => size_of::<OutputFrameDesc>(), "OutputFrameDesc.width" => offset_of!(OutputFrameDesc, width), "OutputFrameDesc.bytes_per_row" => offset_of!(OutputFrameDesc, bytes_per_row), "OutputFrameDesc.presentation_ns" => offset_of!(OutputFrameDesc, presentation_ns), "OutputFrameDesc.source_frame_index" => offset_of!(OutputFrameDesc, source_frame_index), "OutputFrameDesc.source_fps_num" => offset_of!(OutputFrameDesc, source_fps_num), "OutputFrameDesc.timecode" => offset_of!(OutputFrameDesc, timecode),
        "sizeof OutputRequest" => size_of::<OutputRequest>(), "OutputRequest.max_fps_den" => offset_of!(OutputRequest, max_fps_den), "sizeof OutputReply" => size_of::<OutputReply>(), "OutputReply.message" => offset_of!(OutputReply, message), "sizeof OutputHdrInfo" => size_of::<OutputHdrInfo>(), "OutputHdrInfo.max_fall" => offset_of!(OutputHdrInfo, max_fall), "sizeof OutputHdrBlock" => size_of::<OutputHdrBlock>(), "OutputHdrBlock.info" => offset_of!(OutputHdrBlock, info),
        "sizeof AudioRingHeader" => size_of::<AudioRingHeader>(), "AudioRingHeader.slot_stride" => offset_of!(AudioRingHeader, slot_stride), "AudioRingHeader.time_scale" => offset_of!(AudioRingHeader, time_scale), "AudioRingHeader.latest" => offset_of!(AudioRingHeader, latest), "AudioRingHeader.state" => offset_of!(AudioRingHeader, state), "AudioRingHeader.host_audio" => offset_of!(AudioRingHeader, host_audio), "AudioRingHeader.session_seq" => offset_of!(AudioRingHeader, session_seq), "AudioRingHeader.generation" => offset_of!(AudioRingHeader, generation), "AudioRingHeader.session" => offset_of!(AudioRingHeader, session),
        "sizeof AudioSlotHeader" => size_of::<AudioSlotHeader>(), "AudioSlotHeader.desc" => offset_of!(AudioSlotHeader, desc), "sizeof AudioSession" => size_of::<AudioSession>(), "AudioSession.speed" => offset_of!(AudioSession, speed), "AudioSession.push_frames" => offset_of!(AudioSession, push_frames), "sizeof AudioPacketDesc" => size_of::<AudioPacketDesc>(), "AudioPacketDesc.time_value" => offset_of!(AudioPacketDesc, time_value), "AudioPacketDesc.flags" => offset_of!(AudioPacketDesc, flags),
        "kRingMagic" => RING_MAGIC, "kFrameDescVersion" => FRAME_DESC_VERSION, "kOutputRingMagic" => OUTPUT_RING_MAGIC, "kOutputDescVersion" => OUTPUT_DESC_VERSION, "kAudioRingMagic" => AUDIO_RING_MAGIC, "kAudioDescVersion" => AUDIO_DESC_VERSION, "kOutputHdrInfoOffset" => OUTPUT_HDR_INFO_OFFSET,
    }
}

/// Compare the C++ compiler's table with ours. Returns (checked, mismatches).
pub fn check() -> (usize, Vec<String>) {
    let rust: std::collections::HashMap<&str, u64> = rust_table().into_iter().collect();
    let mut bad = vec![]; let mut n = 0;
    let count = unsafe { qcbae_layout_count() };
    for i in 0..count {
        let name = unsafe { std::ffi::CStr::from_ptr(qcbae_layout_name(i)) }.to_str().unwrap();
        let cpp = unsafe { qcbae_layout_value(i) };
        match rust.get(name) { Some(&r) => { n += 1; if r != cpp { bad.push(format!("{name}: C++ {cpp} vs Rust {r}")); } } None => bad.push(format!("{name}: C++ {cpp}, no Rust entry")) }
    }
    (n, bad)
}
