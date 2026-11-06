// S4 — the C++ half of the cross-implementation test: QCBridgeAE's own SharedRing (vendor/qcbae, unmodified) publishing a moving
// 4K RGBA16F gradient at 24 fps into a ring the Rust consumer reads. Build: tools/s4/build-cxx.sh. Usage: cxx_producer NAME SECONDS
#include "common/surface/shared_ring.h"
#include <chrono>
#include <cstdio>
#include <cstring>
#include <thread>
#include <vector>
using namespace qcbae;
static uint16_t half(float v) { uint32_t b; std::memcpy(&b, &v, 4); uint16_t s = (b >> 16) & 0x8000; int e = ((b >> 23) & 0xff) - 127 + 15; uint32_t m = b & 0x7fffff; if (e <= 0) return s; return s | (uint16_t(e) << 10) | uint16_t(m >> 13); }
int main(int argc, char** argv) {
    const char* name = argc > 1 ? argv[1] : "/qcbae-probe"; const double seconds = argc > 2 ? atof(argv[2]) : 10.0;
    const uint32_t w = 3840, h = 2160, bpr = aligned_bytes_per_row(w, 8);
    SharedRing ring;
    if (!ring.create(name, uint64_t(bpr) * h, 3)) { std::fprintf(stderr, "cxx producer: %s\n", ring.error().c_str()); return 1; }
    ring.set_host_state(HostState::Active);
    std::printf("cxx producer (QCBridgeAE SharedRing): ring %s %ux%u RGBA16F rows %u, total %llu MB\n", name, w, h, bpr, (unsigned long long)(ring.header()->total_size >> 20));
    std::vector<uint16_t> row(bpr / 2);
    const auto t0 = std::chrono::steady_clock::now(); uint64_t seq = 0;
    while (std::chrono::duration<double>(std::chrono::steady_clock::now() - t0).count() < seconds) {
        const auto due = t0 + std::chrono::duration_cast<std::chrono::steady_clock::duration>(std::chrono::duration<double>(seq / 24.0));
        std::this_thread::sleep_until(due);
        void* dst = ring.begin_write(uint64_t(bpr) * h);
        if (!dst) { std::fprintf(stderr, "begin_write failed\n"); break; }
        const float phase = float(seq % 240) / 240.0f;
        for (uint32_t x = 0; x < w; ++x) { float r = std::fmod(float(x) / w + phase, 1.0f); row[x * 4] = half(r); row[x * 4 + 1] = half(1.0f - r); row[x * 4 + 2] = half(phase); row[x * 4 + 3] = half(1.0f); }
        for (uint32_t y = 0; y < h; ++y) std::memcpy(static_cast<uint8_t*>(dst) + size_t(y) * bpr, row.data(), bpr);
        FrameDesc d{}; d.width = w; d.height = h; d.bytes_per_row = bpr; d.pixel_format = PixelFormat::RGBA16F; d.source_tier = SourceTier::Float32; d.channel_order = ChannelOrder::RGBA; d.flags = kFlagPremultiplied;
        d.time_value = std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::steady_clock::now().time_since_epoch()).count(); d.time_scale = 1000000000; d.value_scale = 1.0f;
        std::snprintf(d.comp_name, kMaxCompName, "cxx producer");
        ring.commit(d); ++seq;
    }
    ring.set_host_state(HostState::Retired);
    std::printf("cxx producer: %llu frames published; retired\n", (unsigned long long)seq);
    std::this_thread::sleep_for(std::chrono::milliseconds(300));
    return 0;
}
