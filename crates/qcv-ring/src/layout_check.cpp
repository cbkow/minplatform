// S4 — the C++ side of the byte-exact layout test: sizes and field offsets as the C++ compiler lays them out, from the vendored
// canonical headers (vendor/qcbae, QCBridgeAE src/common). The Rust side compares every number.
#include "common/protocol/frame_desc.h"
#include "common/protocol/output_desc.h"
#include "common/protocol/audio_desc.h"
#include "common/surface/shared_ring.h"
#include "common/surface/output_ring.h"
#include "common/surface/audio_ring.h"
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
using namespace qcbae;
struct Entry { const char* name; uint64_t value; };
#define SZ(T) { "sizeof " #T, sizeof(T) }
#define OF(T, f) { #T "." #f, offsetof(T, f) }
static const Entry kEntries[] = {
    SZ(RingHeader), OF(RingHeader, magic), OF(RingHeader, version), OF(RingHeader, slot_count), OF(RingHeader, producer_pid), OF(RingHeader, slot_stride), OF(RingHeader, pixels_offset), OF(RingHeader, pixels_capacity), OF(RingHeader, icc_offset), OF(RingHeader, icc_capacity), OF(RingHeader, slots_offset), OF(RingHeader, total_size), OF(RingHeader, latest), OF(RingHeader, reader_claim), OF(RingHeader, icc_seq), OF(RingHeader, icc_generation), OF(RingHeader, icc_len), OF(RingHeader, host_state),
    SZ(SlotHeader), OF(SlotHeader, seq), OF(SlotHeader, desc),
    SZ(FrameDesc), OF(FrameDesc, width), OF(FrameDesc, height), OF(FrameDesc, bytes_per_row), OF(FrameDesc, pixel_format), OF(FrameDesc, source_tier), OF(FrameDesc, channel_order), OF(FrameDesc, flags), OF(FrameDesc, time_value), OF(FrameDesc, time_scale), OF(FrameDesc, icc_generation), OF(FrameDesc, graphics_white), OF(FrameDesc, value_scale), OF(FrameDesc, comp_name),
    SZ(OutputRingHeader), OF(OutputRingHeader, magic), OF(OutputRingHeader, version), OF(OutputRingHeader, slot_count), OF(OutputRingHeader, producer_pid), OF(OutputRingHeader, slot_stride), OF(OutputRingHeader, pixels_offset), OF(OutputRingHeader, pixels_capacity), OF(OutputRingHeader, slots_offset), OF(OutputRingHeader, total_size), OF(OutputRingHeader, producer_protocol), OF(OutputRingHeader, header_flags), OF(OutputRingHeader, latest), OF(OutputRingHeader, reader_claim), OF(OutputRingHeader, state), OF(OutputRingHeader, consumer_pid), OF(OutputRingHeader, consumer_protocol), OF(OutputRingHeader, heartbeat_ns), OF(OutputRingHeader, last_seen_seq), OF(OutputRingHeader, request_lock), OF(OutputRingHeader, request_generation), OF(OutputRingHeader, request), OF(OutputRingHeader, status_lock), OF(OutputRingHeader, status_generation), OF(OutputRingHeader, status), OF(OutputRingHeader, reply_lock), OF(OutputRingHeader, reply_generation), OF(OutputRingHeader, reply_for), OF(OutputRingHeader, reply),
    SZ(OutputSlotHeader), OF(OutputSlotHeader, seq), OF(OutputSlotHeader, desc),
    SZ(OutputFrameDesc), OF(OutputFrameDesc, width), OF(OutputFrameDesc, bytes_per_row), OF(OutputFrameDesc, presentation_ns), OF(OutputFrameDesc, source_frame_index), OF(OutputFrameDesc, source_fps_num), OF(OutputFrameDesc, timecode),
    SZ(OutputRequest), OF(OutputRequest, max_fps_den), SZ(OutputReply), OF(OutputReply, message), SZ(OutputHdrInfo), OF(OutputHdrInfo, max_fall), SZ(OutputHdrBlock), OF(OutputHdrBlock, info),
    SZ(AudioRingHeader), OF(AudioRingHeader, slot_stride), OF(AudioRingHeader, time_scale), OF(AudioRingHeader, latest), OF(AudioRingHeader, state), OF(AudioRingHeader, host_audio), OF(AudioRingHeader, session_seq), OF(AudioRingHeader, generation), OF(AudioRingHeader, session),
    SZ(AudioSlotHeader), OF(AudioSlotHeader, desc), SZ(AudioSession), OF(AudioSession, speed), OF(AudioSession, push_frames), SZ(AudioPacketDesc), OF(AudioPacketDesc, time_value), OF(AudioPacketDesc, flags),
    { "kRingMagic", kRingMagic }, { "kFrameDescVersion", kFrameDescVersion }, { "kOutputRingMagic", kOutputRingMagic }, { "kOutputDescVersion", kOutputDescVersion }, { "kAudioRingMagic", kAudioRingMagic }, { "kAudioDescVersion", kAudioDescVersion }, { "kOutputHdrInfoOffset", kOutputHdrInfoOffset },
};
extern "C" {
uint32_t qcbae_layout_count() { return sizeof(kEntries) / sizeof(kEntries[0]); }
const char* qcbae_layout_name(uint32_t i) { return kEntries[i].name; }
uint64_t qcbae_layout_value(uint32_t i) { return kEntries[i].value; }
}
