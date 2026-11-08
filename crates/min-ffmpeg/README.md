# min-ffmpeg

The family's FFmpeg substrate (canon row 188): FFmpeg 9 behind thin RAII, lifted out of `qcv-player` on 2026-11-07 so
every app — the QCView port today, minNotes / ufb / MinRender as they port — decodes through one crate.

| Module | What it carries (QCView's source, module by module) |
|---|---|
| `ffmpeg` | `Session`: open (file or image sequence), read / send / receive, seek, the hwaccel attach, decoded frames wrapped as `min_present::Frame` (zero-copy native handles or CPU planes) |
| `frame_index` | PTS ↔ frame, keyframes, the two-tier background scan |
| `hw`, `hw_routing` | `HwDevice` (VideoToolbox / D3D12VA, shared with the presenter's device) and QCView's per-codec routing |
| `read_ahead` | the network-volume warmer (utility QoS / disk I/O policy) |
| `thread_policy` | slice threads for intra codecs, frame threads otherwise |
| `rgb_range`, `yuv_planar` | the legal→full RGB rule and the planar level constants |
| `seek_compat`, `source`, `frame_handle`, `decoder_cleanup` | the seek shim, the media source descriptor, the handle, the slow-teardown queue |
| `vk_bridge` (Windows) | S6: FFmpeg's Vulkan-decoded frames (ProRes / FFV1 / APV) copied on the GPU into shared D3D12 plane textures the presenter samples — the GPU ProRes route for Windows (canon row 149) |

The FFmpeg itself is the ONE pinned build per machine: `tools/ffmpeg/` (9.0.2; Homebrew on macOS, BtbN gpl-shared on
Windows fetched into `external/ffmpeg/win64`). `ffmpeg-sys-next` binds it at build time through pkg-config / `FFMPEG_DIR`.

Licence: MIT OR Apache-2.0 (the author's own code, as `min-paths`). The player above it stays GPL-3.0-or-later.
