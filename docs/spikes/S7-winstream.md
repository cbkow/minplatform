# S7 — winstream: the Windows stream's surface — a DX12 picture to the hardware H.264 encoder with no CPU copy

**Question (§21.3; canon row 239, `docs/phase2/step4-stream-card.md` 4d):** QCView fed Media Foundation's hardware H.264
encoder from its own D3D11 renderer — the encoder's NV12 textures were the renderer's render targets. The port renders on
DX12 (wgpu). Can a picture rendered by wgpu's DX12 device reach the hardware H.264 MFT **with no CPU copy**, through
(A) a D3D12 resource handed to the MFT directly (a D3D12 device manager), or (B) that resource shared into a D3D11 device?
**Pass bar:** on the box (RTX 4090, Windows 11 26200), 1080p30 for 10 s: every frame sent comes out encoded (300 / 300), a
keyframe a second, no B-frames, BT.709 limited-range tags, **every flat patch decodes back within 1 code value** of QCView's
maths (the macOS bar, and QCView's own result on this box: "picture within 1 code value"), no CPU copy of the picture
anywhere. Reference: QCView's A2 on this box — 6.05 Mbps at a 6000 target, exact pts.
**Time box:** one session, 2026-11-09. **Status: VERDICT GO by route B (2026-11-09). Route A: NO on this encoder.**

## What the spike is
`spikes/s7-winstream` (Windows only; `windows` 0.62 + wgpu, no FFmpeg): wgpu (DX12) uploads the macOS harness's picture (8
bars over 8 grey steps, a moving block; `S7_BUSY` adds a block of fresh noise each frame), runs the stream's REAL NV12 pass
(`crates/qcv-stream/src/nv12.rs`, by path) into an R8 and an RG8 texture, then one texel of each is copied out so wgpu leaves
them in `COPY_SOURCE`. A raw D3D12 command list **on wgpu's own queue** (`hal.raw_queue()`, so it runs right behind wgpu's
work) copies the two textures into plane 0 and plane 1 of an NV12 `ID3D12Resource` from a ring of six, and a fence says when
the GPU is done. Media Foundation wraps that surface (`MFCreateDXGISurfaceBuffer`) as the input sample of the first hardware
H.264 MFT, set up as QCView's (`mf_video_encoder.cpp` 436–553: async, low latency, CBR, GOP = one second, Baseline, 709 tags,
16–235). The access units go to a raw `.h264`; it prints the patches' expected codes and the latencies.
Modes: `probe`, `d3d12 <out>` (A), `d3d11 <out>` (B), then `[secs] [WxH] [fps] [kbps]`.

## Findings
| | Result |
|---|---|
| The encoder | one hardware H.264 MFT: **NVIDIA H.264 Encoder MFT** — async, `MF_SA_D3D11_AWARE` = 1 |
| The session | everything ran over SSH (no desktop session needed for the GPU or the MFT) |
| **Route A** — the device manager on wgpu's D3D12 device | `ResetDevice(ID3D12Device)` succeeds and the MFT ACCEPTS `MFT_MESSAGE_SET_D3D_MANAGER` — but then refuses `SetOutputType` with `MF_E_UNSUPPORTED_D3D_TYPE` (0xC00D6D76, "The input type is not supported for D3D device"), with or without `MF_MT_D3D_RESOURCE_VERSION = MF_D3D12_RESOURCE` on the input type. **NVIDIA's MFT does not encode from D3D12.** |
| **Route B** — the NV12 resources created `D3D12_HEAP_FLAG_SHARED` (+ `ALLOW_RENDER_TARGET | ALLOW_SIMULTANEOUS_ACCESS`), `CreateSharedHandle` → `ID3D11Device1::OpenSharedResource1` in a D3D11 device on the SAME adapter (by LUID; `VIDEO_SUPPORT`, multithread-protected), the device manager on that D3D11 device | **works** |
| B, 1080p30, quiet picture, 10 s | 300 sent / 300 encoded, 10 keyframes; Constrained Baseline, level 4.0, no B-frames, bt709 / tv tags; **all 16 patches decode back EXACT (worst difference 0)** |
| B, encoder latency (input to output) | p50 2.4 ms, p95 2.8, max 3.5 (1080p30); 5.5 / 6.0 / 6.8 (2160p30) |
| B, our GPU work per frame (NV12 pass + plane copy + the wait for the GPU) | p50 1.27 ms, p95 2.10, max 6.39 (1080p, quiet) |
| B, worst-case picture (a quarter of the frame fresh noise every frame) | 1080p30 @ 6000: 6566 kbps; 2160p30 @ 16000: 17382 kbps; 1080p60 @ 6000: 6312 kbps — 600 / 600 at 60 fps |
| `CODECAPI_AVEncMPVDefaultBPictureCount = 0` | refused (E_INVALIDARG) by this MFT; no B-frames come out regardless |

No CPU copy of the picture on route B: wgpu's targets → the NV12 planes is a GPU copy on one queue; the D3D11 texture is the
same memory; the MFT reads it on the GPU. One GPU plane copy more than QCView (whose D3D11 renderer drew straight into the
NV12 planes) — wgpu cannot render into an NV12 texture's planes (its NV12 is binding-only).

## Verdict
**GO, route B.** The Windows session is QCView's — a native Media Foundation session on a D3D11 device — with the port's DX12
picture arriving through shared NV12 surfaces. What the build takes from the spike:
- the D3D11 device must sit on the adapter wgpu chose (the box also lists two virtual display adapters);
- the plane copy runs on wgpu's own queue; the surface carries a fence value, and the encoder's thread waits for it before
  `ProcessInput` (the render side never blocks);
- wgpu's intermediate targets are left in `COPY_SOURCE` by a one-texel copy — the state the raw copy needs;
- a D3D12-aware MFT (route A) would drop the D3D11 device; none is here. Not pursued.

**Not answered here (the build's own checks):** AMD / Intel MFTs (the original's notes: FFmpeg's AMF / QSV wrappers emitted
nothing on a comparable box — the native MFT route is the one QCView proved on NVIDIA only); the bitrate on real pictures
(noise overshoots the target by 5–9 %; QCView's reference clip gave 6.05 Mbps at 6000); a session started before the device
exists and the wire texture's binding (the original's two traps).
