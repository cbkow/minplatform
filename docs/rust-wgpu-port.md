# minNotes → Rust + wgpu → a shared media platform for the family: the canon

Started 2026-11-05 in `minNotes/private/`; MOVED here (`minplatform/docs/`) on 2026-11-06 on the
user's first "go" (§21: create the repo, move the canon, run P0). A thinking document that became the
platform's record. Return here before any decision to build; update the Decisions table when a
ruling lands. Section numbers are stable; §13 = where we are, §21 = the path forward,
`docs/spikes/` = the measured verdicts.

Repo state at the time of writing: main = adbb0d6 (2.0.6 shipped on macOS, submitted to the
Microsoft Store). Qt 6.11.1, C++20, QML.

---

## 1. What minNotes is, measured (2026-11-05)

| Layer | Lines | Where Qt is load-bearing |
|---|---|---|
| Core engine `app/core` (blocks, spans, ranks, undo, SQLite, import/export, package) | ~27k C++ | QString/QJson plumbing only; **QTextDocument in the PDF + DOCX exporters** |
| Player `app/player` (FFmpeg decode, scrub, audio, zero-copy Metal / D3D11VA / Vulkan) | ~13k C++ | QRhi in `VideoSurfaceItem`; QObject/QImage/QTimer plumbing everywhere else |
| Ink + video annotation `app/notes` (capsule SDF ink, stroke modeler, QCView sidecar) | ~5k C++ | one QSGGeometry node (`InkNodeItem`); `VideoAnnotator` is QPainter |
| Spell + grammar `app/spell` | ~1.7k C++ | none that matters |
| UI `app/qml` (editor, canvas, chrome, dialogs) | ~18.5k QML | **total**: TextEdit + QTextDocument + two QSyntaxHighlighters + the delegate pool |
| Regression suite `tests/regression_test.cpp` | 9.5k, 131 checks | Qt test harness, drives BlockModel directly |

Qt class histogram (app/, top of the list): QString 3841, QJsonObject 357, QPointF 271,
QFileInfo 228, QImage 199, QColor 156, QRectF 155, QRegularExpression 100, QXmlStreamReader 54,
QTextDocument 41, QSqlQuery 37, QPainter 37, QTextCharFormat 32, QRhi 31, QSGGeometry 20,
QPdfDocument 13, QNetworkReply 14. The renderer is NOT where Qt is load-bearing. **Text is.**

Every block is a Qt `TextEdit` over a `QTextDocument` (BlockView.qml:555), with
`InlineMarkdownHighlighter` and `CodeHighlighter` (KSyntaxHighlighting) painting spans and code
colours as QTextCharFormats. The PDF exporter lays pages out through a second QTextDocument →
QPdfWriter. Qt supplies for free: shaping (HarfBuzz), font fallback, emoji, bidi, grapheme and
word cursor motion, IME preedit, macOS press-and-hold accents and dictation, accessibility,
selection geometry, rich clipboard. **That is the thing a port rebuilds.**

A standing note in CMakeLists.txt (line ~47): "pure C++/Qt — no Rust/cxx-qt; the virtualization
spike proved the whole stack in C++". That ruling was about embedding Rust INSIDE the Qt app for
the view model. A full Rust app is a different question; a standalone engine crate behind a C
ABI is a third.

---

## 2. What ports, what gets reinvented, what gets simpler

### Straight ports (logic, language-agnostic)
- Document engine: ULIDs, fractional ranks, DFS-flat block list, `LayoutIndex` (Fenwick), span
  interval algebra (`InlineText`), undo transaction log, clipboard codec (`application/x-mnd-blocks`),
  `DocumentMerger`, `PathMap`, `MediaStore`, `PackageFormat` (.mnpkg zip) → rusqlite, serde_json,
  zip. The `.mnd` SQLite file IS the contract; a Rust build is a second implementation of the
  same file, not a migration.
- Importers: quick-xml (docx/xlsx/odt/enex), pulldown-cmark, csv. Exporters: Markdown + HTML
  straight; DOCX via docx-rs; **PDF is the hard one** (see §2 "Reinvented").
- Spell: `spellbook` (pure-Rust Hunspell, Helix) + the SCOWL merged dictionary as-is. The
  LanguageTool rule interpreter (`GrammarRules.cpp`) ports directly; needs `fancy-regex` for
  lookarounds. (The 2026-09-09 "no Rust / no Harper" record was about embedding in Qt; moot here.)
- Code highlighting: KSyntaxHighlighting → `syntect` (Sublime syntaxes) or tree-sitter.
- PDF viewing: QtPdf (PDFium) → `pdfium-render` (same engine, BSD).
- Previews: PSD/PSB `psd_sdk` stays C++ behind FFI or the `psd` crate (limited); HEIC/RAW via
  ImageIO through objc2; Radiance HDR → `image` crate; miniz → `zip`.
- Ink shader `ink.vert/frag` → WGSL, 1:1. `ink_tessellate` is pure math.
- Player logic (decode loops, scrub, GOP cache, frame index, read-ahead, audio engines) — see §4.

### Reinvented (the UI is the product)
- The editor UI: Editor.qml (8.7k) + BlockView + every flyout, picker, context menu, scrollbar,
  tooltip, drop target, the title-row drag, the pin.
- Rich text layout + editing + IME + accessibility (see §3).
- Rich clipboard with the custom block MIME: `arboard` covers text + image only → direct
  NSPasteboard (objc2) + OLE (windows-rs). Cross-app DnD beyond file drops is thin in winit (the
  ufb repo's year of DnD edge cases would be relearned).
- Native menus (`muda`), dialogs (`rfd`), window chrome (winit transparent titlebar +
  objc2-app-kit for the pin/fullscreen; Windows keeps the native bar as ruled 2026-10-24).
- Sparkle via objc2 FFI; Windows stays the Store (MSIX packages any exe).
- Settings/paths/watchers: `directories`, `notify`, `reqwest` — each a small integration.

### Simpler (what disappears)
- Four rendering paths today (QML text, QSG ink, QRhi video, QPainter exports) → one (vello /
  own wgpu passes, used on screen and offscreen). Ink parity between screen and export stops
  being a maintained invariant.
- Screen text and PDF text share ONE layout engine (today TextEdit vs QTextDocument).
- The slot pool, per-slot revisions, delegate recycling, binding-loop class of bugs: all exist
  because QML delegates are expensive. A custom renderer draws the visible window of blocks
  from the LayoutIndex every frame and caches each block's shaped layout keyed on
  (text, spans, width). Replaced by a smaller class of cache-invalidation bugs.
- No Qt pin/KF6 build/QtPdf module install; smaller binary; faster start.

---

## 3. The real decision is the text stack, not wgpu

wgpu is a GPU API, not a toolkit. No Rust toolkit ships a rich block editor. The choice:

1. **Pure-Rust shaping: parley + vello** (Linebender), or cosmic-text + glyphon. Identical output
   on both OSes (matches today's `textRenderType: TextEdit.QtRendering`). Parley has fallback,
   bidi, a plain-text editing core; everything above is ours. Pre-1.0 churn. **[proposed]**
2. **Platform shaping: CoreText / DirectWrite** via objc2 / windows-rs + a wgpu glyph atlas.
   How Zed's GPUI does it. Native emoji/fallback/quality on day one; two code paths forever.
   GPUI itself: worth studying (a shipping GPU editor toolkit), Metal-first, not literally wgpu
   on Windows.
3. **A toolkit**: Slint (closest migration for 18k lines of QML, has a wgpu renderer, single-style
   text editing), iced / egui (same story), Xilem/Masonry (the one built on the stack a text app
   wants; young).

Recommendation: (1) with parley + vello for text/shapes/UI, plus the custom capsule pipeline for
ink and a custom pipeline for video. Only path where screen, export and PDF share one layout.

Programme shape (mirrors how 2.0 was built): **engine → read-only viewer → editor**.
- Engine first, with the regression suite rewritten against it as the oracle.
- Viewer = canvas, cards (read-only pages), media, video + transport, ink, lightbox, HTML export.
  Shippable on its own; exercises every hard platform seam except text input.
- Editor last.
Scope cut worth considering: DOCX + PDF exporters move late (last Qt dependency with no clean
equivalent; Markdown + HTML carry most of the utility). PDF export in Rust = our own parley
layout onto `krilla` (typst's PDF backend).

---

## 4. The player + QCView interop (the part that gets messy)

### 4.1 Lineage: this is QCView's player, carried by copy
`app/player` and `app/notes` are QCView-Player's code, ported into ufb, then into minNotes, as
independent copies with namespace renames (`qcv`/`ufbplayer`). Headers say so: "Ported into UFB
from QCView-Player", "minNotes copy of QCView … verbatim … Re-sync from QCView; push fixes
upstream", "VERBATIM port … fix QCView first, then re-port" (annotation_io/note/serializer,
read_ahead, vulkan_hw_device_ctx, vulkan_device_manager, wasapi_audio_device, simple_lru,
rgb_range, sws_rgba_image, d3d11_vulkan_yuv_compositor). Three Qt apps share one player by copy.

Consequence: a Rust port of minNotes alone FORKS the player lineage; fixes stop flowing by copy.
Two honest options:
- **A. Port the player to Rust as its own crate** (`qcv-player`-shaped: decode + audio + sidecar,
  no UI), designed so QCView/ufb could adopt it later. Fork accepted. **[proposed]**
- **B. Keep the C++ player behind a C ABI**, rewrite only the GPU surface in wgpu. The data
  crossing the boundary is small (FrameHandle = pointer + pts + dims; transport + audio commands;
  callbacks for frameAvailable/state). BUT the C++ is Qt-shaped: VideoDecoder/AudioPlayer are
  QObjects with signals, FrameHandle holds a QImage, annotation_io uses QDir/QFile/QtConcurrent,
  the shuttle integrates on a QTimer. De-Qt-ing 13k lines is half a port and leaves two
  languages in the hot path. (QCView-Player's pre-Qt shapes exist in its history: the comments
  record std::string/std::thread → Qt adaptations.)

### 4.2 What is in the player (so the port is sized honestly)
- `VideoDecoder`: decode thread; FFmpeg; a publish slot + `fetchLatest(consumerSeq)`;
  `FrameHandle` = move-only discriminated handle {Cpu QImage | Metal CVPixelBufferRef | Vulkan
  AVFrame(AVVkFrame) | D3D11 AVFrame} with `clone()` for fan-out; per-codec hw routing
  (`hw_routing.h`: ProRes RAW software-only; Windows: inter codecs → D3D11VA, ProRes → Vulkan
  compute decoder, FFV1/APV software because 6–20× slower on GPU); `thread_policy.h` (slice vs
  frame threading, measured); source timecode / drop-frame.
- `ScrubDecoder` + `ScrubFrameCache`: own FFmpeg context, latest-request-wins, GOP LRU keeping
  frames in native YUV with deferred RGBA convert; publishes into the main decoder's slot.
- `FrameIndex`: formula tier + async packet scan for exact pts↔frame.
- `ReadAhead`: page-touch warming for LucidLink/SMB; one utility-QoS thread.
- Audio: `AudioPlayer` (decoder thread + ring + ±0.2 % servo resampler against the master clock),
  `MultiStreamAudioDecoder` (libavfilter amerge/pan graph for broadcast masters),
  `AudioDrainStage` (fades on seek/underrun/hold), `ShuttleAudioEngine` + `AudioChunkReader`
  (grain varispeed for J/K/L, pitch cap rule), `TempoStage` (SoundTouch), CoreAudio / WASAPI
  devices, `timecode_formatter`.
- Renderer `VideoSurfaceItem` (QQuickRhiItem): macOS Metal zero-copy (CVMetalTextureCache planes
  → biplanar/AYUV shaders: NV12, P010, P210, AYpCbCr16 4444; BT.601/709/2020, limited/full,
  quarter-turn rotation); Windows Vulkan-decoded → Vulkan compute YUV→RGBA16F → NT shared handle
  → D3D11 texture; Windows D3D11VA → D3D11 compute → RGBA16F; CPU RGBA8 upload fallback. SDR
  only (QCView's OCIO/compositor passes dropped).
- The QML surface actually used by the transport is SMALL (counted from VideoTransport /
  CompactTransport / Editor / Lightbox): decoder = isPlaying, fps, currentFrame, frameCount,
  state, open/close, play/pause, seekToFrame, scrubToFrame, setPlaybackSpeed, readAheadActive;
  audio = open/close/initialize, play/pause, seek, volume/muted, beginShuttle/shuttleTarget/
  endShuttle, setPlaybackTempo, routingMode, hasAudio, update. ~30 calls. The Rust API is that.

### 4.3 Rust mapping of the logic (ports cleanly)
- FFmpeg: the code lives in hwcontext, hwframes pools, `get_format` callbacks, libavfilter
  graphs. The safe wrappers (`ffmpeg-next`) don't cover that; write against `ffmpeg-sys-next` /
  `rsmpeg` raw bindings with thin own RAII wrappers (the C++ does the same). FFmpeg 9 vendored
  as today.
- `FrameHandle` → an enum with `Drop` (CFRelease / av_frame_free). Publish slot → `Mutex<Option>`
  + `AtomicU64` seq, or a `watch` channel. Threads → std + `parking_lot`; the 33 ms shuttle
  QTimer → a dedicated thread or the winit event-loop timer; decode→UI "frameAvailable" → an
  `EventLoopProxy` user event that requests a redraw.
- Audio devices: `cpal` covers CoreAudio + WASAPI with an f32 callback, but the servo wants the
  device's playout latency, which cpal doesn't report uniformly → keep direct `coreaudio-rs` and
  windows-rs WASAPI (as today) behind one trait. SoundTouch: the `soundtouch` FFI crate, or the
  `signalsmith-stretch` Rust port. `rubato` for resampling if the fractional resampler isn't
  simply ported (it is tiny; port it).
- Rust's ownership is a genuine win in exactly this subsystem: the FrameHandle lifetime
  contracts, the "fresh clone, handle owns it" rules, the thread-confined chunk reader, the
  "destroy bridge before compositor" declaration-order rules all become types.

### 4.4 GPU interop — the actual mess, and it is a WINDOWS mess
**macOS (easy, well-trodden).** wgpu Metal backend. CVPixelBuffer → CVMetalTextureCache →
MTLTexture per plane → `Device::create_texture_from_hal::<hal::api::Metal>` with formats
R8Unorm/Rg8Unorm (NV12), R16Unorm/Rg16Unorm (P010/P210), Rgba16Unorm (4444). Lifetime: the C++
releases the previous frame's wrappers on the next bridge() call relying on Qt's frame ordering;
in wgpu hold a short ring of handles and release on `queue.on_submitted_work_done`. Surface
format `Bgra8UnormSrgb` keeps the sRGB-pinned presentation (the P3 oversaturation fix). Shaders
→ WGSL as-is, fragment samples the planes directly (no RGBA16F intermediate needed).

**Windows: wgpu has NO D3D11 backend (DX12 + Vulkan only).** Both existing Windows paths target
D3D11 and do not fit. Options:
- **W1. DX12 backend + FFmpeg `d3d12va` hwaccel (FFmpeg ≥ 7) [proposed].** Same shape as today's
  `d3d11va_hw_device_ctx`: hand wgpu's ID3D12Device to FFmpeg as `AVD3D12VADeviceContext`;
  frames are ID3D12Resource textures (`AV_PIX_FMT_D3D12`, NV12/P010) on OUR device; wrap with
  the dx12 hal `texture_from_raw`. Sync: each AVD3D12VAFrame carries an ID3D12Fence + value;
  wgpu's public API has no fence wait on submit → `queue.as_hal` and call
  `ID3D12CommandQueue::Wait(fence, value)` on wgpu's own queue before the frame that samples it.
  One device, one queue, a hwaccel FFmpeg maintains. Covers H.264/HEVC/AV1/VP9 = today's D3D11VA
  set. LOSES the Vulkan ProRes compute decoder → ProRes on Windows goes software with slice
  threading (measured 2026-09-09: ~3 ms/frame 1080p 422 HQ, fine; 4K/8K ProRes is where Vulkan
  helped). Deletes `vulkan/`, both bridges, the compositor (~1.5k lines + the shaderc dep).
- **W2. Vulkan backend for wgpu + FFmpeg Vulkan hwaccel on the SAME VkDevice.** wgpu-hal lets
  us create the Adapter/Device from our own VkInstance/VkDevice with our own extension list
  (video_queue, video_decode_*, external_memory, timeline semaphores, push_descriptor — exactly
  the list `vulkan_device_manager` mirrors from FFmpeg today), then share it as
  `AVVulkanDeviceContext`. Decoded AVVkFrames are VkImages on wgpu's device → hal
  `texture_from_raw`. Sync: AVVkFrame carries per-image timeline semaphores + layouts; wgpu has
  no public semaphore wait → either drop to the hal Vulkan queue, or run a one-off raw
  VkCommandBuffer (layout transition + YUV→RGBA convert) into a wgpu-owned image synchronised
  by the semaphore, then let wgpu sample it (one GPU pass, zero CPU copies — what the
  D3D11VulkanYuvCompositor does today). Keeps ProRes on the GPU. Risk: Vulkan video decode
  driver quality across NVIDIA/AMD/Intel vs D3D12VA's universal maturity; and the raw-Vulkan
  code is the most fragile code in the repo today.
- **W3. Hybrid (today's shape): decode-only VkDevice + NT shared handle into DX12.** The
  complexity we are trying to escape. No.
- In all cases the **CPU fallback must stay first-class**: over remote desktop wgpu lands on
  WARP (software DX12), where d3d12va is unavailable — and the user works over remote desktop
  often. `queue.write_texture` of RGBA8, or better: upload the Y/UV planes and reuse the YUV
  shader (the scrub cache's deferred swscale convert can go away entirely).

**Cross-cutting risks**
- Every zero-copy path uses `as_hal` escape hatches whose signatures move across wgpu
  releases (breaking releases every few months). Pin wgpu; isolate in one module per backend
  behind a `FramePresenter` trait; test with the video probe.
- Texture release timing vs GPU completion (ring + on_submitted_work_done; never free on
  next publish).
- Presentation pacing: Qt's scene graph drove `update()` on frameAvailable; winit = request
  redraw from the decode thread's event, present Fifo; audio-master servo unchanged.
- Multi-surface (lightbox/full-frame + canvas card): one device, many textures; fine.
- 125 % DPI Windows box: winit scale factor; nothing special.

### 4.5 The QCView sidecar + annotation stack
- **Sidecar contract (`.qcview/<media>/notes.json` + `images/note_<TC>.png`)** is byte-compatible
  with QCView and PERMANENT (memory ruling). `annotation_note/io/serializer` port verbatim to
  serde_json with `preserve_order`; keep the quirk (annotation_data = JSON string in memory,
  nested object on disk); Windows hidden attribute via windows-rs; legacy `.ump` read fallback;
  stream-notes header round-trip; the per-stream folder naming (`<name>__<id8>`).
- `VideoNotesModel` glue: QCView's EXACT timecode minting (non-drop, fps*1000/1000, frame-0
  origin), idempotent clean-thumbnail capture (never overwrite), file watcher reload (`notify`),
  merge-on-save keyed by timecode + local delete tombstones. Pure logic; port exactly; it has
  tests in the regression suite to carry over.
- **ink-stroke-modeler**: the modeled points are BAKED into the sidecar, so a different modeler
  means slightly different strokes, not incompatibility. Still: the same pinned commit behind
  cxx/a C shim (drags Abseil; `cmake` crate) **[proposed]** for identical rendering here and in
  QCView; `ink-stroke-modeler-rs` (the Rust port Rnote uses) as the fallback if the build pain
  outweighs parity.
- `VideoAnnotator` is QPainter today → becomes the SAME wgpu capsule pipeline as canvas ink
  (frame-normalized coords × the frame box as `scale`). One ink renderer for canvas, media
  pages and video. Round-cap/round-join parity with `qcv::paintStroke` is what the SDF shader
  already emulates.
- Annotated thumbnails (`_annotated` PNG recomposites, the 400 ms rule) and the exporters' ink
  overlays need a CPU rasterizer: `tiny-skia` (exact round-cap strokes, simple) or vello
  offscreen + readback.
- Composition: video frame + ink + text chips + selection handles + transport in ONE render
  pass; no QQuickRhiItem / texture-node juggling.

### 4.6 Player verdict
The logic (≈ 11k of the 13k lines) ports cleanly and gets safer. The renderer (≈ 2k lines today,
counting the Windows bridges) is where the work is, and on Windows it is a re-architecture, not a
port: pick W1 (DX12 + d3d12va, software ProRes) unless 4K+ ProRes on Windows is a requirement, in
which case W2. macOS is a known quantity. The sidecar and interop are pure logic with a frozen
contract; parity risk lives only in the stroke modeler choice. The fork-the-lineage question
(§4.1) is the one that needs a ruling before anything else.

---

## 5. Decisions (rulings-needed, with proposed defaults)

| # | Decision | Proposed default | Status |
|---|---|---|---|
| 1 | Text stack | parley + vello (pure Rust) | open |
| 2 | Toolkit | none; own scene over wgpu (vello for 2D/UI) | open |
| 3 | Programme order | engine → read-only viewer → editor | open |
| 4 | PDF/DOCX exporters | late; parley layout onto krilla | open |
| 5 | Player lineage | A: Rust crate, fork accepted (vs B: C ABI around de-Qt'd C++) | open |
| 6 | Windows video | W1: DX12 + d3d12va, software ProRes; drop the Vulkan device + bridges | **RULED 2026-11-05: W1 (DX12 + d3d12va)** — user: "no issues with dx12, doable path" |
| 7 | Stroke modeler | FFI to the same pinned C++ commit (fallback: ink-stroke-modeler-rs) | open |
| 8 | Audio devices | direct CoreAudio + WASAPI behind one trait (not cpal) | open |
| 9 | CPU video fallback | plane upload + YUV shader (first-class; remote desktop) | open |
| 10 | Engine crate shared with the Qt app during overlap? | no (double maintenance); revisit if the port stalls | open |

## 6. Open questions / things to verify before building anything
- wgpu version at the time: confirm `create_texture_from_hal` / `texture_from_raw` shapes for
  metal + dx12, and whether the hal queue exposes fence/semaphore waits (it did not publicly as
  of the wgpu 2x series; `as_hal` needed).
- FFmpeg 9 `d3d12va`: codec coverage + whether the vendored build has it enabled; P010 output.
- parley editing surface at the time: IME preedit, grapheme motion, selection geometry.
- AccessKit text-edit coverage on macOS + Windows.
- winit: NSTextInputClient coverage (press-and-hold accents, dictation), pinch gestures,
  file drop only vs rich DnD.
- `spellbook` vs Hunspell on the merged US+UK SCOWL dictionary (affix compat).
- `pdfium-render` dynamic lib distribution inside the MSIX + the notarized DMG.

---

## 7. Step back: what could be BETTER in a Rust build (2026-11-05)

Framing: "pseudo-Obsidian-like" here means a polished local-first notes app with markdown as the
editing idiom — NOT the linking model. The standing ruling holds: no vault, no sidebar, no
inter-note links / backlinks / graph. Everything below stays inside one file.

Honesty first: most of the wins below come from OWNING LAYOUT AND RENDERING, not from the
language. Rust matters because its 2026 ecosystem (wgpu, vello, parley, syntect, spellbook,
rusqlite, serde, proptest) makes owning layout affordable for one person, and because ownership
types fit the frame-lifetime / threading problems. Typing speed is NOT a win to expect (already
0.4 ms per keystroke). The data model is NOT a weakness (the 2.0 invariant: "we worked very hard
to get this working and it does work great"); where an item below touches the block store it is
a deliberate departure that needs its own ruling.

### 7.1 The block editor

**L1 — heights are MEASURED BACK from widgets.** Today a delegate renders, reports its height,
the Fenwick absorbs it, the view compensates scroll. Hence: estimated heights, coalesced
measure-back timers, recycle transients, the two-regime undo height rule, the pinned
`lineCount × round(fontSize × 1.35)` hack because TextEdit has no lineHeight.
→ **Height becomes a pure function computed in the layout pass** (shape text → lines → height)
before anything is drawn. No estimates, no measure-back, no transients, undo is pure data.
The Fenwick stays, fed by computed layout. This single change retires reactivity rules 1b, 1c,
1d and the "reload fixes it" bug class.

**L2 — two copies of state (model + delegates) reconciled by revision ints.** Rules 1, 1e, 2,
6 exist because QML delegates hold stale copies and bindings can be elided by qmlcachegen.
→ **One retained document → derived layout → render**, with per-block generation counters.
No second copy; nothing to desync. Replaces the whole revision-dep discipline with ordinary
cache invalidation.

**L3 — one plain-text TextEdit per block.** Consequences today: one font per block (heading
Light vs body Medium cannot mix inside a block), chips/choice spans drawn through char-format
tricks + overlays, markers dimmed in place rather than hidden, no inline objects.
→ **Inline boxes are first-class**: chips, inline images, footnote markers, math, link pills
are layout items with their own width; mixed fonts/sizes per span; real line-height,
ligature/feature control; "clean view" (markers hidden) vs "source view" (markers shown) is a
per-block display choice at layout time — the stated endgame of InlineMarkdownHighlighter.h,
reachable because we lay out whichever string we like and own the caret mapping.

**L4 — nesting is lists-only (`depth` on list items, max 8) + one level of lanes.** No quote
containing a list, no list item containing a code block or media, no callouts, no collapsible
sections; frames "NO NESTING in v1" for the same hit-test/drag/undo cost reasons.
→ Keep the DFS-flat store (ranks + ULIDs) but let `depth` mean "child of the nearest shallower
block" for EVERY type: container blocks (quote, callout, toggle, list item) hold arbitrary
children. LayoutIndex generalises from two levels to N (nested Fenwick or an interval tree).
**Departure from the invariant — needs a ruling; file-format bump (mnd3).**

**L5 — selection is special-cased around opaque blocks.** Block-grain selection, the band,
opaque atomicity (rule 3), the parked block-select refinement.
→ One document-wide selection = (anchor, head) as (block path, offset). Cross-block text
selection, copy and delete become the ordinary case; opaque blocks are just inline-less blocks.
The parked refinement becomes a design conversation, not a plumbing one.

**L6 — undo = region snapshots + patches, with height seeding.** Fine today, but tied to the
measured-height world.
→ With L1, undo is pure data. Option: operation-based undo (insert/delete/format ops over
block paths), which is also the door to multi-caret and, if ever wanted, sync. NOT a goal;
noting the door.

**L7 — span offsets are UTF-16 code units** (QString ↔ JS). Fine, but a standing caution in
the markdown export.
→ Internal offsets become UTF-8 byte or grapheme indices; the `.mnd` loader converts at one
chokepoint (keep UTF-16 on disk for mnd2 compatibility, or bump).

**L8 — the dense media table pops during zoom (parked).** Root cause: delegate rebind on
re-entry → async re-decode → blank → fade.
→ No delegates, no rebind: an app-level texture cache keyed (source, width) with a measured
budget keeps textures resident across scroll; a freeze-frame during gestures is free when you
own the swapchain. The parked bug is fixed by the architecture, not by a patch.

**Not better, and must be re-earned:** IME, accessibility, platform text services, native
menus/dialogs feel. (§2.)

### 7.2 The infinite canvas

**C1 — the page-height cap** (a live page card stops at a square; editing past it goes through
the focused page view) exists because each card is a full delegate pool.
→ A page on the canvas is a clip rect + scroll offset over the SAME block layout system; culling
is per block in world space across all pages (one spatial index, e.g. `rstar`). The cap can go,
or stay as a design choice rather than a cost ceiling.

**C2 — LOD tiers with hysteresis + the element slot pool.**
→ Natural LOD: per-page render-to-texture snapshots (invalidated on edit) at low zoom, greeked
text bars below a threshold, full layout when readable. No slot pool. Smooth zoom with no pop.

**C3 — frames keep ABSOLUTE child coordinates so exporters/search/ink never learn about
nesting; no nested frames.**
→ A real scene tree with a transform stack: children in LOCAL coordinates, nested frames
(groups in boards, boards in boards), per-frame clipping and scale as ordinary nodes. The
storyboard layer (numbering by slot, captions, present mode, contact-sheet PDF) sits on this
without special cases. **File-format decision: absolute vs local child coords.**

**C4 — ink.** Already GPU capsules. Pressure/tilt are one field away (`halfW` per segment) once
the input layer delivers them (winit pen events on Windows; NSEvent pressure via objc2 on mac).
Vello renders the same strokes offscreen → identical ink in every export.

**C5 — canvas export is HTML + PNG poster only.** With our own layout + `krilla`, the canvas
(and a board) exports to PDF directly; SVG is a small emitter. The storyboard's contact sheet
stops being a QPdfWriter special case.

**C6 — scale.** `visibleElements` is a scan; thousands of elements want an R-tree. Trivial.
Minimap / overview = one more render-to-texture. Present mode = camera animation (exists).

**Unchanged by the port (design work, not platform):** connectors/arrow routing, storyboard
fields, membership rules, the context-menu grammar, the title-row drag semantics.

### 7.3 The markdown processor (inside the no-linking ruling)

**M1 — markdown knowledge is in FIVE places**: hand-coded typing triggers scattered through
BlockModel.cpp (`- `, `* `, `[ ] `, `# `, ```` ``` ````, `**`), the inline highlighter's own
marker scanner, the importer, the exporter mapping table, the clipboard codec.
→ **One grammar, every use**: a single incremental markdown parser (markdown-rs / comrak /
tree-sitter-markdown) + one serializer. Typing triggers = parse the current block; paste/import
= parse the text; source mode = serialize + re-parse; export = serialize. Round-trip
(blocks → md → blocks is identity on the supported subset) enforced by property tests
(`proptest`). This is the biggest "processor" win and it is a design win, not a speed win.

**M2 — source mode / live preview.** The Obsidian live-preview idiom: the block under the caret
shows its markers; every other block renders clean. Today impossible cleanly (plain-text
TextEdit + dimming). With L3 it is a per-block display flag at layout time, with exact caret
mapping between the two strings because both layouts are ours. Whole-page raw view = the same
engine in monospace.

**M3 — constructs absent today** that a notes app expects: callouts / admonitions
(`> [!note]`, needs L4), footnotes, math (`$…$` as an inline box — `ReX`-style TeX layout or
typst's math layout as a library), collapsible headings/toggles (L4), definition lists, proper
hard-break semantics, nested blockquotes. Mermaid-style diagrams: NO — the canvas is the
diagram tool. Tags / embeds / wikilinks: NO (ruling).

**M4 — markdown as a live companion, not a format change.** `.mnd` stays canonical (media,
canvas, undo, SMB-safe working copy). Optional: a write-through `.md` per page on save (the
exporter, automatically) so a folder of `.mnd` files is also a folder of readable text.
Trustworthy only once M1 holds. Not a ruling yet; noting it.

**M5 — export fidelity.** PDF today = QTextDocument re-layout (tables as spreadsheet column
bands, header rows repeated: a different rendering from the screen). With one layout engine the
PDF IS the screen layout (the QCView review → board → PDF loop gets WYSIWYG). DOCX stays a
mapping (docx-rs).

**M6 — search.** Folded substring today; `nucleo` (Helix's fuzzy matcher) or a trigram index
for in-file search; still one file, still no vault.

**M7 — code blocks.** syntect or tree-sitter → folding, bracket matching, per-language
indentation; language pill as today.

### 7.4 File-format implications of the above (all optional, all rulings)
- `blocks.depth` generalised to every type (L4) → mnd3, one-way converter from mnd2
  (the 1.0 → 2.0 precedent: read-only until Save As).
- Span offsets: keep UTF-16 on disk, convert at load (L7) — or bump with mnd3.
- `elements` child coordinates absolute → local (C3) — or keep absolute and derive.
- Everything else (ranks, ULIDs, elements/kind/json, canvas_meta, the `.qcview` sidecar,
  `.mnpkg`) unchanged. The Rust build reads mnd2 from day one.

### 7.5 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 11 | Computed layout (no measure-back) | yes — the architectural premise | open |
| 12 | Container nesting for all block types (L4) + mnd3 | yes, in the editor phase | open |
| 13 | One markdown grammar for triggers/paste/import/export/source mode (M1) | yes, engine phase | open |
| 14 | Source mode / live preview per block (M2) | yes, editor phase | open |
| 15 | Frame children: local coords + nesting (C3) | local coords, nesting allowed | open |
| 16 | Page-height cap on the canvas (C1) | drop the cost cap; keep a design cap if wanted | open |
| 17 | Write-through `.md` companion (M4) | not in v1 | open |
| 18 | Math inline boxes (M3) | yes, after source mode | open |

---

## 8. What I would CHANGE in a major pivot (time not an issue) — 2026-11-05

The user: "what would you consider changing in a major pivot like this? Time is not an issue."
These are my recommendations, not a survey. Ordered by how much they simplify everything else.

### 8.1 ONE node tree with FOUR layout kinds (the structural change)
Today the file has two worlds stitched together: `blocks` (flat, ranks, depth, page_id) and
`elements` (x/y/w/h/z, parent_id, json), two undo logs routed by focus, two exporters, and a
`MediaHost` seam because media has "two homes, one implementation". Frames honour `parentId`
with absolute child coords; split rows are lanes; tables are derived from lanes with header rows;
boards are frames with a grid. Every one of these is a CONTAINER with a layout rule.

→ **One `nodes` table**: `id, parent_id, rank, kind, attrs(json), content`. Everything is a node:
the canvas root, a frame, a page, a split row, a lane, a paragraph, a media item, an ink
element. A container carries exactly one of four layout kinds, each with a fixed, simple
algorithm — deliberately NOT a general layout engine (the Figma / Blender trap the user named):
- `flow`  — vertical stack, measure-constrained (a page, a quote, a callout, a list item's body)
- `row`   — side-by-side lanes with weights (a split row; a header role on a row makes a table)
- `grid`  — N columns + gap, one child per slot, cover-crop (a board)
- `free`  — children carry a transform (x, y, scale) and z (the canvas root, a group frame)
A page = `flow` container with a `measure` attr placed in a `free` parent. A board = `grid`
frame. A table = consecutive `row` containers headed by a row with `header: n`. Lists = `flow`
containers nested. Callouts / toggles / quotes-with-lists fall out (§7 L4).

What this dissolves: `page_id`, the per-page BlockModel instances, `MediaHost`,
`CanvasEditorFacade`, two undo logs (one log over node paths; "focus routing" becomes a UI
filter), two exporter walkers (one walker, per-kind emitters), the elements-vs-blocks
clipboard split, the frames' absolute-coordinate workaround (children are local under `free`).
Rank + ULID + DFS order stay exactly as they are; `depth` becomes derived from `parent_id`.

Risk: over-generalising. Guard: the four kinds are closed; no auto-layout, no constraints, no
components. If a fifth kind is ever wanted it is a product conversation, not a parameter.

### 8.2 The in-memory tree is the TRUTH; SQLite is persistence
Today SQLite is the LIVE store (skinny scan + lazy content fetch), chosen for the virtualization
spike + crash safety; the SMB WAL crash fix added a scratch working copy + explicit save.
→ A persistent (immutable, structurally shared) tree in memory (`im`-style or hand-rolled
arena with generations). A transaction produces a new root; **undo = keep old roots** (O(log n)
per step, no region snapshots, no patches, no height seeding). Persistence = write-behind of
dirty nodes to the scratch SQLite after each transaction (same crash safety as today), explicit
Save = checkpoint/copy to the original (unchanged UX), Save As = vacuum copy. Documents fit in
memory (blocks are text; the 5 GB is media in the sidecar). Load = one scan into the tree.

### 8.3 Computed layout, one renderer, one text engine (the premise — §7 L1–L3)
Layout is a pure function of (tree, viewport, zoom, theme). Render is a pure function of
layout. Text = parley + vello everywhere (screen, exports, PDF via krilla). Ink + video are two
custom wgpu passes. No measure-back, no delegates, no slot pools, no LOD tiers held in state —
LOD is a render-time choice from zoom.

### 8.4 One markdown grammar (§7 M1) + a CLI
One parser + one serializer behind the engine. And an **`mn` CLI crate** over the same engine:
`mn inspect`, `mn convert` (mnd2 → mnd3, md → mnd), `mn export --pdf|--html|--md|--docx`,
`mn lint`, `mn board --contact-sheet`. Replaces the `--export-pdf` dev flag and the shell probes
that parse stderr; scriptable from the justfile and from the user's VFX pipelines (batch-export
boards). Tests drive the same API.

### 8.5 One Transport, one media clock
Today QML orchestrates `videoDec` + `videoAudio` + `mediaHost` + `rowNotes` with ~30 calls and
the shuttle on a QTimer. → A `Transport` object owns decoder, scrub decoder, audio player,
shuttle engine and the notes model behind ONE API (open/close, play/pause, seek, scrub, shuttle
begin/target/end, speed, volume, routing) and ONE clock; playback / scrub / shuttle are states of
one machine. The UI binds to the Transport, never to its parts. Same for the player across
cards + lightbox + full-frame: one Transport per media, many views.

### 8.6 One asset pipeline
Previews are spread today (PreviewPsd, PreviewRender_mac, MediaStore::extractFrame /
renderPdfPage, annotation_thumbnail, bookmark meta fetch, remote image localise) over
QThreadPool + QtConcurrent with a "benign log flakes" list. → One `AssetPipeline`: a priority
job queue with cancellation (rayon/tokio), decoders behind a `Decoder` trait (image / video
poster / PDF page / PSD / HEIC-RAW / HDR / favicon), one texture + thumbnail cache with a
measured budget keyed (source, width), and one "loading state" model (the reveal-only fade
philosophy stays). Media loading windows remain the product; they just come from one place.

### 8.7 Headless, deterministic testing from day one
Today: a 9.5k-line Qt-bound regression suite over BlockModel, shell probes over a real app
(absolute fixture paths, stderr parsing, FAIL-only output), and desktop click scripts that break
at 125 % DPI. → Engine crate: unit + property tests (ranks, spans, markdown round-trip, undo
inverses) with `proptest` + `insta` snapshots. Renderer: offscreen wgpu render → golden images
with a tolerance diff (one fixture file = one PNG per view/zoom). App: a headless **driver**
(the same command API the CLI uses) so a probe is a script of commands + assertions, not
clicks. Perf: the keystroke / key→swap probes become benches (`criterion`) over the engine +
a frame-time budget test over the renderer.

### 8.8 Accessibility and input as outputs of the same tree
AccessKit tree generated from the layout tree (not retrofitted); IME anchored to the layout's
caret rect; pen pressure/tilt in the input model from the start (ink). These are the things Qt
gave for free; design them in rather than catch up later.

### 8.9 UI architecture: modes as state machines, one input router
No 8.7k-line god file. Explicit modes (canvas, page edit, full-frame media, lightbox, guide),
a tool stack per mode, ONE input router (the "central mouse layer" rule 4 is a symptom of not
having one), the Esc ladder + NavHistory as the navigation spine, Theme as a tokens struct
(tonal ladder, 3/6 rounding, Geist). Context-menu grammar ported as data (sections, hide-don't-
grey, red last).

### 8.10 Side effects worth taking
- **Linux** nearly free (wgpu Vulkan + winit); not a goal, don't block on it, don't prevent it.
- **Op-based undo** (8.2 makes it cheap) opens multi-caret and, if EVER wanted, sync. Not a goal.

### 8.11 What I would NOT change (contracts and rulings)
- Product rulings: no vault, no sidebar, no links/backlinks/graph; one canvas per file; import in.
- The `.qcview` sidecar, byte-for-byte, and the QCView timecode minting rules.
- `.mnd` as a SQLite container; `.mnpkg`; explicit save + working copy UX; fractional ranks +
  ULIDs + DFS order; the span kinds and their numbering; the media descriptor JSON shape; the
  `{vol, rel}` path mapping; the content-hashed `.minnotes/` sidecar.
- The design language (tokens, radius 3/6, Geist, hover/pressed ramps, popup enter-fade only,
  accent rationed, body 14 / chrome 13 / mono 13), the context-menu grammar, the Esc ladder,
  the lightbox + back/forward model, the title-row drag, the frame membership rules.
- The user-facing feature set: nothing dropped. Internal things dropped: dual undo logs,
  MediaHost, slot pools, LOD tiers in state, measured heights, the Vulkan Windows bridges,
  the KF6 build step, the QtPdf module dependency, QML.

### 8.12 Approach given unlimited time
Build the unified node tree FIRST (do not port the two-table model and unify later — that is
two migrations). Order: engine (8.1–8.4, 8.7) with the CLI as its first consumer → the viewer
(8.3, 8.5, 8.6) → the editor (8.8, 8.9). The Rust build reads mnd2 read-only from day one and
writes mnd3; the Qt app keeps shipping meanwhile. Keep every ruling in the Decisions table;
when the editor phase starts, walk the parked items (block select, media zoom pass, split-row
caret) against the new model instead of carrying their workarounds.

### 8.13 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 19 | One `nodes` table, four layout kinds (8.1) → mnd3 | yes, from day one | open |
| 20 | In-memory persistent tree is truth; SQLite write-behind (8.2) | yes | open |
| 21 | `mn` CLI over the engine (8.4) | yes, engine phase | open |
| 22 | One Transport object (8.5) | yes, viewer phase | open |
| 23 | One AssetPipeline (8.6) | yes, viewer phase | open |
| 24 | Golden-image renderer tests + headless driver (8.7) | yes, from the first pixel | open |
| 25 | Linux build | allowed, never blocking | open |
| 26 | Layout kinds closed at four | yes | open |

---

## 9. Repo shape: one workspace first, shared crates by contract, UI extracted on the second consumer — 2026-11-05

The user: "since we are building a UI from scratch essentially, does it become its own repo so we
can share it with other apps and build on it?"

**Recommendation:** one Cargo workspace (`crates/`) from day one; crate boundaries now, repo
boundaries later. Extraction is cheap in Cargo (path dep → git dep pinned in the lockfile; history
preserved with `git filter-repo` / subtree) PROVIDED the dependency direction is held:
- `mn-ui` never depends on `mn-engine`; `mn-engine` never depends on `mn-ui`; only `mn-app`
  depends on both.
- Shared crates never depend on anything minNotes-specific.

**The UI toolkit is NOT the first thing to share.** Rule of two consumers: the first app shapes
the API by real use; the second proves what generalises; split then. Building `mn-ui` as a
framework for hypothetical consumers is how a product becomes a toolkit project.

**The first shared crates are the ones with a FROZEN CONTRACT — media, not UI** (today shared by
copy across QCView-Player / ufb / minNotes, §4.1):
- `qcv-player` — decode, scrub, audio, shuttle, the Transport (8.5), the wgpu presenter (per
  backend behind a `FramePresenter` trait). **Optional C ABI (cbindgen)** so the Qt apps can
  consume the Rust player through FFI while minNotes consumes it natively → the lineage fork
  of §4.1 becomes a dependency instead of a copy. This is the one that fixes a real cost today.
- `qcv-notes` — AnnotationNote, sidecar I/O, timecode minting, merge-on-save + tombstones,
  stream folders. Byte-compatible contract, pure logic, three consumers already.
- `qcv-ink` — capsule tessellation, the wgpu pass (WGSL), the stroke-modeler binding, the
  tiny-skia rasteriser for thumbnails/exports.

**Layout**
| Crate | Role | Home |
|---|---|---|
| `mn-engine` | node tree (8.1/8.2), markdown grammar, undo, import/export, package, spell | minNotes |
| `mn-cli` | `mn inspect/convert/export/lint/board` (8.4) | minNotes |
| `mn-app` | winit + wgpu app; depends on everything | minNotes |
| `mn-ui` | scene, computed layout, input router, text (parley/vello), widgets, theme tokens | minNotes until a 2nd consumer |
| `qcv-player` | decode/audio/Transport/presenter (+ `capi` feature) | shared repo, early |
| `qcv-notes` | sidecar + minting + merge | shared repo, early |
| `qcv-ink` | tessellation + wgpu pass + modeler + CPU raster | shared repo, early |

**Licensing decision:** minNotes is GPL-3.0. A GPL crate cannot be consumed by an app under other
terms, so the SHARED crates need their own licence chosen deliberately — MIT OR Apache-2.0
[proposed] — so QCView / ufb can take them whatever their terms. `mn-*` stays GPL-3.0.

### 9.1 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 27 | Repo shape | one Cargo workspace; shared `qcv-*` crates in their own repo once their API settles (first weeks, not first day) | open |
| 28 | `mn-ui` extraction | only on a second real consumer | open |
| 29 | `qcv-player` C ABI for the Qt apps | yes (`capi` feature, cbindgen) | open |
| 30 | Shared-crate licence | MIT OR Apache-2.0; `mn-*` GPL-3.0 | open |
| 31 | Dependency direction (ui ⟂ engine; shared ⟂ minNotes) | enforced (CI check on Cargo.toml deps) | open |

---

## 10. The family, holistically — sister repos surveyed 2026-11-05

Cursory survey of `~/Documents/GitHub/*` (sizes = source lines, excluding externals/builds).

| Repo | Stack | Size | Licence | Role |
|---|---|---|---|---|
| **QCView-Player** | Qt 6.11 QML shell over NATIVE renderers (D3D11 + DComp / Metal), FFmpeg, OCIO 2.5, OpenEXR 3.4, Vulkan video, shaderc | ~102k C++ + 21k QML (39 files) | GPL-3 | The ORIGIN of the player + annotations. HDR10 / EDR output, dual view, image sequences (EXR multipart, TIFF/PNG/JPEG/HTJ2K), OCIO + minColor, scopes, timeline/playlists, `.qcvproj`, live inputs (QCBridge* shared rings, SRT), stream output encoders |
| **ufb** | Qt 6.11 QML + **Rust already** (cxx-qt via corrosion; `core/` 33.6k Rust; separate `agent/` crate for the VFS) | 32k C++ (player ~15k, thumbnails 11k) + 20k QML + 33.6k Rust | GPL-3 | File browser for VFX; NAS mounts (WinFsp / FileProvider), tantivy search, mesh sync, thumbnails (EXR/HDR/HEIF/PDF/PSD/video/shell), transcode via bundled CLIs |
| **minNotes** | Qt 6.11 QML, C++ | 62k C++ + 18.5k QML | GPL-3 | This repo |
| **MinRender** | Qt 6.8 QML + Widgets (tray), **Rust `mr-agent`** (1.7k) | 28k C++/QML | **MIT** | Render-farm coordinator (HTTP mesh, SQLite snapshots, job templates). Frame previews via the QCView image loaders |
| **minColorAE** | C++ After Effects plugin + `core/` colour maths | — | GPL-3 (OpenDRT-derived) | minColor core (AgX, knee, OpenDRT presets) with shim headers for C++ / CUDA / GLSL / HLSL / MSL; vendored by copy into QCView at a pinned commit |
| QCBridge / QCBridgeAE / QCBridgeA / QCBridgeN | Blender add-on + agent; Mercury Transmit plugin; AJA out; NDI out | AE 2.9k spine + 0.8k; Blender 12.3k py + 6.1k rs; A 1.2k; N 0.7k | GPL-3 (ring spine MIT; A/N MIT) | QCView's live-input / output companions — THREE byte-stable contracts, see §20 |
| minSolo | native menu-bar audio mute/solo | small | — | unrelated |

### 10.1 What is shared TODAY, by copy, with measured drift
| Subsystem | Shared between | Drift (changed lines, `diff`) |
|---|---|---|
| **Player** (whole `app/player` ≈ 60 files) | QCView → ufb → minNotes | `video_decoder.cpp` QCView↔minNotes **507** (QCView 1782 lines vs 1675: QCView has moved on); ufb↔minNotes 103; `audio_player.cpp` 55; `d3d11_vulkan_yuv_compositor.cpp` 111. ufb has NO shuttle engine |
| **Annotations** (note / io / serializer / thumbnail / viewport_annotator / stroke modeler / active_stroke) | QCView ↔ minNotes | `annotation_io.cpp` **6** — the contract has held |
| **Stroke RENDERER** | QCView = `StrokeTessellator` (polyline → triangles, ~1.5 px arcs) on Metal / D3D11; minNotes = capsule SDF on QSG | two implementations of one visual contract (`qcv::paintStroke`) |
| **Image loaders** (EXR / PNG / JPEG / TIFF + mmap istream + thumbnail provider) | QCView ↔ MinRender | **0** (verbatim copy) |
| **Image decoding, third stack** | ufb `thumbnails/` backends (EXR / HDR / HEIF / PDF / PSD / video / shell); minNotes `Preview*` (PSD via psd_sdk, HEIC/RAW via ImageIO, HDR via stb) | three unrelated implementations for overlapping formats |
| **Colour** | minColorAE `core/` → QCView `src/color/mincolor` | pinned-commit copy ("keep in sync by re-copying") |
| **Chrome** | `WindowChrome` (minNotes, ufb, MinRender); `Theme.qml` (minNotes 118 tokens, MinRender 70, **35 names shared**); `ThemedMenu*` (3 apps); Sparkle updater (3); `build_msix.ps1` (3) | an informal design system + distribution kit, copied |

### 10.2 The honest read
- The overlap is **media + platform**, not UI. The UI shells are small QML layers each shaped by
  its app (a file browser, a player's chrome, an editor). Only minNotes has a UI that is the
  product itself.
- **QCView is the upstream** of everything media. Its player is the most advanced copy (live
  streams, dual decode, image sequences, HDR). Any shared Rust player must be designed from
  QCView's version, not from minNotes' subset — or it is a fourth fork.
- **The Rust ↔ Qt bridge is already proven in-house**: ufb ships cxx-qt + corrosion with a
  33k-line Rust core and sixteen bridged services. "A Rust crate consumed by a Qt app" is not
  hypothetical for this family; it is how ufb works today.
- **QCView is the costliest to move to a Rust UI and gains the least from it**: ~100k lines,
  native renderers already working, HDR, OCIO (no Rust OCIO; stays C++). Its UI is chrome
  around a native view. ufb's UI is lists and trees — exactly what Qt does well. MinRender is
  small, MIT, tray-shaped.
- Therefore the **UI toolkit (`min-ui`) has ONE realistic consumer for years: minNotes.**
  The shared crates that pay off across all four apps immediately are the media ones, consumed
  from the Qt apps through FFI.

### 10.3 Holistic proposal: a Rust MEDIA PLATFORM for the family first; the UI second
**Tier 1 — shared media crates (own repo, permissive licence, C ABI feature for the Qt apps):**
- `qcv-player` — decode, scrub + GOP cache, frame index, read-ahead, audio (servo, multi-stream,
  drain fades, shuttle, tempo), **Transport**, wgpu presenter per backend. Feature flags:
  `shuttle`, `dual`, `live` (SRT + shared rings), `image-seq`. Designed from QCView's copy.
  Consumers: minNotes (native), QCView + ufb (C ABI via cbindgen, or cxx-qt as ufb already does).
- `qcv-notes` — the sidecar contract, timecode minting, merge + tombstones, thumbnails.
- `qcv-ink` — ONE stroke renderer: the capsule SDF (exact round caps/joins, 1-px feather,
  resolution-independent, matches `paintStroke`) replaces QCView's polyline tessellator too.
  Stroke modeler behind FFI. CPU raster (tiny-skia) for thumbnails + exports.
- `min-image` — ONE image decoding stack replacing THREE: EXR (`exr` crate, pure Rust,
  multipart/multichannel — or OpenEXR FFI where speed matters), TIFF/PNG/JPEG/HDR (`image`),
  HEIF (libheif FFI), PSD (psd_sdk FFI), PDF (pdfium-render), HTJ2K (OpenJPH FFI), video
  poster (via qcv-player), shell thumbnails / system icons (platform). The ufb `thumbnails/`
  backends are the first migration test (ufb already has the bridge).
- `min-color` — minColor core gains a **WGSL shim** beside its C++/CUDA/GLSL/HLSL/MSL ones, plus
  a Rust host. Stays **GPL-3 (OpenDRT-derived)**, so only the GPL apps consume it. OCIO stays
  C++ (QCView-only; no Rust OCIO).
- `min-present` — the wgpu video presenter incl. **HDR output**: Rgba16Float surfaces; EDR on
  Metal (**2026-11-06: wgpu 30 does this itself via `SurfaceColorSpace`, see S1 — the objc2 route below is OBSOLETE**) needs the CAMetalLayer's extended-dynamic-range flag + colourspace set through objc2 on
  the layer wgpu exposes via hal; HDR10 on DX12 needs the DXGI colour space + HDR10 metadata on
  the swapchain (hal access, possibly an own swapchain). **The hardest shared piece; QCView-only
  today; do not block minNotes on it.**

**Tier 2 — platform kit (shared, small):**
- `min-theme` — the design tokens as DATA (one JSON/TOML source → generated `Theme.qml` for the
  Qt apps today, a Rust struct for `min-ui` later). The 35 shared names say the system exists.
- `min-chrome` — window chrome rules (transparent title bar, pin, fullscreen round-trip fix,
  native bar on Windows), Sparkle + Store packaging scripts. A scripts repo more than a crate.

**Tier 3 — `min-ui`** (minNotes' from-scratch wgpu UI): stays in the minNotes workspace until a
second app rebuilds on it (§9). If QCView is ever rebuilt a second time, this is its shell.

### 10.4 Order that respects the upstream
1. `qcv-notes` + `qcv-ink` (small, frozen contracts, two consumers today) — prove the shared-repo
   + C ABI flow on something cheap.
2. `min-image` — migrate ufb's thumbnails first (the bridge exists), then MinRender's previews
   (MIT needs the permissive licence), then QCView's loaders, then minNotes' previews.
3. `qcv-player` — designed from QCView's copy with feature flags; minNotes' new app is its
   first native consumer; QCView + ufb adopt through FFI when it reaches parity with their
   copies (QCView last: HDR + dual + live are the long tail).
4. `min-ui` + minNotes proper (§8).

### 10.5 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 32 | The shared Rust player is designed from QCView's copy (feature-flagged), not minNotes' subset | yes | open |
| 33 | One stroke renderer for the family = capsule SDF (`qcv-ink`), replacing QCView's tessellator | yes | open |
| 34 | One image stack (`min-image`) replacing QCView loaders, ufb backends, minNotes previews, MinRender copy | yes; ufb thumbnails first | open |
| 35 | minColor gets a WGSL shim; `min-color` stays GPL-3 | yes | open |
| 36 | HDR presentation (`min-present`) is QCView's problem, never blocks minNotes | yes | open |
| 37 | Design tokens as data (`min-theme`) generating Theme.qml today | yes, cheap, immediate | open |
| 38 | Shared-crate licence MIT OR Apache-2.0 so MinRender (MIT) can consume; `min-color` excepted | yes | open |

---

## 11. The HDR curveball — QCView is HDR-capable, the rest don't care (2026-11-05)

**Principle: HDR is a PRESENTATION concern. Decode produces frames + colour metadata; only the
last stage knows what the display can do.** Isolate it there and the SDR apps pay nothing except
carrying metadata they ignore.

### 11.1 What the shared crates carry regardless (cheap, and the mistake to avoid is NOT carrying it)
`FrameHandle` / the decoded-frame type carries colour metadata end to end from day one:
- matrix (601/709/2020), range (limited/full), **transfer** (sRGB / BT.1886 / PQ / HLG / linear),
  **primaries** (709 / P3 / 2020), bit depth, chroma layout, rotation, SAR,
- mastering display + MaxCLL / MaxFALL when the container has them,
- the source's declared intent + per-clip overrides (QCView's Range pill, OCIO chain, minColor).
Source: FFmpeg `AVFrame` colour fields; VideoToolbox CVPixelBuffer attachments
(`kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ` / `ITU_R_2100_HLG`); EXR headers
(chromaticities, linear). Retrofitting this through an SDR-only presenter is the known trap;
carrying it costs a struct.

### 11.2 `min-present` as a staged shader graph; apps compile the stages they need
```
 planes/float  →  [1] decode to LINEAR working RGB     (YUV matrix+range, transfer EOTF, primaries → working)
               →  [2] colour chain (OPTIONAL)         (OCIO-baked LUTs | minColor WGSL | CDL | .cube)
               →  [3] display transform               (SDR: tone/knee or clip → sRGB   | HDR: scRGB linear or PQ)
               →  [4] composite with UI               (UI drawn in the same space; SDR white = 1.0 × paper-white scale)
```
- **"Don't care" apps** (minNotes, ufb, MinRender): stages 1 + 3-SDR + 4. ONE shader per layout.
  Bonus they get for free: a PQ/HLG master in a minNotes card no longer renders flat/dark (today's
  presenter is "SDR only, BT.709 gamma assumed"); stage 3-SDR applies minColor's **highlight knee**
  (that is literally what the knee is for: HDR masters on SDR displays). Feature flag `hdr` OFF.
- **QCView**: all four, `hdr` ON, OCIO + minColor + scopes tapping the working buffer after stage 2.
- Working format: RGBA16F only where a chain exists (stage 2); the YUV fast path samples planes
  directly in one pass (minNotes' presenter today). The SDR apps do not pay 2× bandwidth.

### 11.3 Owning the UI makes HDR SIMPLER than QCView's current arrangement
QCView uses **multi-visual DirectComposition** on Windows (an SDR swapchain for the Qt UI + an
HDR swapchain for the video, composed by Windows) precisely because Qt owns the UI's swapchain
and cannot draw the UI into an HDR surface. With `min-ui` drawing the UI itself, there is **one
swapchain** in the display space: scRGB `Rgba16Float` linear on Windows (UI at 1.0 scaled by the
system SDR white level, as Chrome/games do) and an **EDR** `CAMetalLayer` on macOS
(`wantsExtendedDynamicRangeContent`, extended-linear colourspace, UI at 1.0 = SDR white, headroom
from `NSScreen.maximumExtendedDynamicRangeColorComponentValue`). No DComp visual tree, no
multi-swapchain sync. The Qt-era complexity was a consequence of not owning the UI.

### 11.4 wgpu specifics (the verify list)
- Surface format `Rgba16Float` is offered by wgpu on Metal and DX12; `Rgb10a2Unorm` for
  HDR10/PQ on DX12. **Colour space is NOT in wgpu's public API**: set it via `surface.as_hal`:
  Metal → the `CAMetalLayer` (objc2: EDR flag + `CGColorSpace` extended linear sRGB / P3);
  DX12 → `IDXGISwapChain3::SetColorSpace1` (+ `SetHDR10MetaData` for PQ). Same escape-hatch
  class as the video import (§4.4); isolate in the per-backend module.
- Frame pacing for review playback: `desired_maximum_frame_latency`, Fifo/Mailbox per OS.
- **OCIO → WGSL does not exist.** OCIO's GPU shader generator emits GLSL / HLSL / MSL / OSL, not
  WGSL. Two routes: (a) **bake LUTs** (shaper 1D + 3D, as QCView's `ocio_lut_baker` already does)
  and apply them with ONE generic WGSL LUT shader — the chain is data, not code; (b) take OCIO's
  GLSL text and transpile through **naga** (`glsl-in` → WGSL/SPIR-V) at runtime — works for most
  OCIO output but is the fragile path. Default (a); (b) only for ops that don't bake well.
  OCIO itself stays C++ behind FFI (no Rust port); QCView-only dependency.
- minColor: the core already has C++/CUDA/GLSL/HLSL/MSL shims → add a **WGSL shim** (§10.3).
- 10-bit SDR output (`Rgb10a2Unorm`) is a QCView nicety; nothing else needs it.
- EXR / float sources: stage 1 is a no-op (already linear); upload `Rgba16Float` directly.

### 11.5 What this does to the plan
- Nothing moves for minNotes: it compiles stages 1 + 3-SDR + 4 and ships. HDR never blocks it.
- `min-present` is designed with all four stages and the metadata struct from the first commit,
  even though stage 2 and `hdr` land only when QCView adopts. The ORDER of §10.4 holds.
- QCView's adoption criterion becomes concrete: parity = EDR + scRGB/PQ output, baked-OCIO LUT
  path, minColor WGSL, scopes tapping stage 2, dual view compositing in the working space.

### 11.6 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 39 | Colour metadata carried on every frame from day one | yes | open |
| 40 | `min-present` = 4-stage graph; SDR apps compile 1 + 3 + 4; `hdr` feature for QCView | yes | open |
| 41 | SDR apps apply minColor's knee (or clip) to PQ/HLG sources instead of assuming 709 gamma | knee, default on | open |
| 42 | HDR output = ONE swapchain (scRGB float / EDR layer); no DComp multi-visual | yes | open |
| 43 | OCIO on wgpu = baked shaper+3D LUTs in one WGSL LUT shader; naga GLSL transpile as fallback | yes | open |

---

## 12. Porting QCView itself: the performance tradeoffs when chasing milliseconds (2026-11-05)

The user: parity for ufb/minNotes makes sense since they are ports of QCView's player; but "what do
we gain from porting QCView itself? HDR is a win, OCIO is not; what are the tradeoffs if we are
chasing ms?"

### 12.1 QCView's render path today (read from the source)
A NATIVE child surface under the Qt chrome: `PlayerWindow` (own QWindow: CAMetalLayer on macOS;
child HWND + D3D11 + DComp on Windows, HTTRANSPARENT so input falls through to Qt), a dedicated
render thread, render-on-demand, triple buffering (`maximumDrawableCount`, FLIP_DISCARD), a
CADisplayLink on macOS. Video → RGBA16F canvas (capped at 3840×2160 for OCIO cost) → optional
OCIO → present-blit → annotations + guides on the swapchain. Image sequences upload on a GPU
upload thread through a texture ring; software frames upload as RGBA64 to keep >8-bit depth.
Scopes = compute with atomics. Output bus (QCBridgeN/A) blits into ring slots wrapped ONCE as
no-copy MTLBuffers ("no CPU copy anywhere"). Two full renderer trees: `render/metal` 9.7k +
`render/d3d11` 13.9k + `dual/metal` 1.2k ≈ 25k lines doing the same thing twice.
No performance docs or harness were found in `docs/` or `ideas/` (grep: latency, pacing, budget).

### 12.2 Where the milliseconds live, and what the port does to each
| Cost centre | Today | After (Rust + wgpu) | Delta |
|---|---|---|---|
| Decode (FFmpeg sw/hw, EXR) | C++ orchestration | Rust orchestration of the same libs | **0** — FFmpeg is FFmpeg; thread policy ports as measured |
| Zero-copy hw frames | VT→Metal; Vulkan/D3D11VA→D3D11 | VT→Metal via hal; Windows per §12.3 | 0 on macOS; Windows depends on backend choice |
| Image-sequence upload (4K EXR fp16 ≈ 70 MB/frame, 1.7 GB/s at 24 fps) | `replaceRegion` / no-copy MTLBuffer wraps; upload ring | `Queue::write_texture` = staging copy + GPU copy. wgpu will NOT wrap existing CPU memory without hal | **small loss (~0.3–0.5 ms/frame on M-series)** unless the no-copy wrap is done via hal (`texture/buffer_from_hal`) — recoverable, isolated |
| Output bus rings (QCBridge) | blit into no-copy MTLBuffer slots | same via hal wrap; DX12: mapped readback buffer | 0 with hal; one copy without |
| Present pacing | own thread, triple buffer, display link | own loop (wgpu owns no loop), `present_mode` + `desired_maximum_frame_latency` (→ maximumDrawableCount / SetMaximumFrameLatency); own display-link thread calls `get_current_texture` | **0** — pacing fidelity equals native when you drive the loop; `presentsWithTransaction` is the one Metal knob not exposed (hal) |
| Per-frame CPU overhead | direct API | wgpu-core validation + resource tracking, tens of µs per encoder | negligible vs a 16/41 ms frame |
| Shader quality | hand-written MSL / HLSL | WGSL → naga → MSL / HLSL / SPIR-V, same math | within noise; naga output is driver-compiled like any source; `PipelineCache` for startup |
| Compute (scopes, YUV, OCIO LUTs) | Metal / D3D11 compute | WGSL compute; `atomicMax` on u32 storage (float-bits trick works) | 0 |
| OCIO | baked LUTs (`ocio_lut_baker`) | same LUTs, one WGSL LUT shader | **0** (confirms the user's read) |
| Parallel encoding (dual view, scopes, output bus) | D3D11 immediate context is single-threaded (multithread-protected); Metal parallel | wgpu is Send + Sync; explicit APIs (DX12 / Vulkan / Metal) encode in parallel | **small gain on Windows** |
| Two UI layers (Qt swapchain + native surface, DComp / child window) | two render threads, two swapchains, compositor composes | ONE swapchain, one thread (§11.3) | **small gain**: fewer context switches at present time, one fewer hop pen → ViewportAnnotator → pixel |
| Driver maturity | D3D11 + Metal (very mature for simple present loops) | DX12 / Vulkan + Metal | neutral in practice for a player; flip model identical |

**Bottom line on ms: a wash on the GPU, with two recoverable small losses (uploads, rings — both fixed
by hal wraps) and two small gains (one swapchain, parallel encoding).** Nobody should port QCView for
speed. The port's value is structural: ~25k lines of duplicated renderer collapse into one presenter,
HDR becomes one swapchain instead of a DComp visual tree, and the player stops being three copies.

### 12.3 The Windows backend is a PER-APP choice — the DX12 ruling was minNotes' ruling
The one real performance trap: QCView is the app that plays 4K/8K ProRes on Windows, and that is the
Vulkan compute ProRes decoder on the shared VkDevice. DX12 + d3d12va (ruled for minNotes, §4.4 W1)
loses it. Resolution: **wgpu's whole point is backend choice**. The shared crates (`qcv-player`,
`min-present`) support both; each app picks:
- **minNotes / ufb → DX12 + d3d12va** (H.264/HEVC/AV1/VP9 hw; ProRes software — fine at 1080p).
- **QCView → Vulkan backend + FFmpeg Vulkan hwaccel on wgpu's own VkDevice** (§4.4 W2): one
  device for decode AND present; H.264/HEVC/AV1 video-decode extensions + ProRes/FFV1 compute
  decoders; zero-copy through the AVVkFrame timeline semaphores (hal queue wait). QCView ALREADY
  ships Vulkan video on Windows, so the driver-quality risk is the one it carries today — and the
  NT-shared-handle bridge into D3D11 (the most fragile code in the family) disappears because the
  presenter is on the same VkDevice. HDR10 on the Vulkan backend: `VK_EXT_swapchain_colorspace`
  (`HDR10_ST2084` / `EXTENDED_SRGB_LINEAR`) + `VK_EXT_hdr_metadata` — again via hal; QCView's
  `vulkan_device_manager` comments list exactly these extensions as the ones it stripped when the
  renderer moved to D3D11, i.e. it has been here before.
Cost: two backends live in CI (both already exercised: minNotes DX12, QCView Vulkan); the hal
escape-hatch modules exist per backend anyway (video import, colour space).

### 12.4 What QCView would actually GAIN (not ms)
1. One renderer instead of two trees (+ dual/metal): the Metal and D3D11 implementations of the
   compositor, OCIO pass, scopes, annotation renderer, HDR swapchain, texture pool, upload
   thread, dual compositor each exist twice today.
2. HDR as one swapchain with the UI drawn in the display space (§11.3); no DComp multi-visual.
3. The player as a dependency, not the upstream of two copies (§10).
4. Pen → pixel latency: one fewer process hop for strokes (Qt input → native surface).
5. Parallel command encoding on Windows (dual view + scopes + output bus).
6. Linux (Vulkan) nearly free — QCBridge's Blender users are a Linux-shaped audience.

### 12.5 What QCView would LOSE or re-earn
- ~100k lines of working code rewritten, for an app that works; the long tail (dual, live SRT
  + shared rings, timeline/playlists, project files, output encoders, scopes, i18n) is QCView-only.
- Qt's Quick Controls / QML chrome → `min-ui` must exist and be good first (§9: QCView is the
  second consumer, which is also the extraction trigger).
- OCIO stays C++ FFI (no Rust OCIO); OpenEXR: `exr` crate or FFI; OpenJPH FFI; shaderc gone (naga).
- Every hal escape hatch (video import, colour space, no-copy wraps, semaphore waits) tracks
  wgpu's breaking releases. Pin and isolate — one module per backend.
- IME/a11y matter less for a player; menus/dialogs/tooltips still have to be rebuilt.

### 12.6 Before any of it: a perf harness (QCView has none in the repo)
Measure first on both boxes (M5 Max; the 5090 at 125 % DPI): decode→present latency per codec,
pen→pixel latency, frame-time p50/p99 at 24/25/30/60, upload bandwidth for 4K/8K EXR, dual-view
frame time, scopes cost, memory. Record as the baseline the port must meet. `criterion` benches
for the engine-side pieces; a frame-time budget test over the presenter. Without numbers the
"wash" above is a prediction, not a result.

### 12.7 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 44 | QCView is NOT ported for performance; it adopts shared crates via FFI first, rebuilds its shell on `min-ui` last (if ever) | yes | open |
| 45 | Windows wgpu backend is per app: minNotes/ufb DX12 + d3d12va; QCView Vulkan + Vulkan hwaccel (keeps GPU ProRes) | per-app split | open — **user 2026-11-05: "we might want to consider dx12 anyway for better PQ / H.265 etc; cross that bridge later"** (d3d12va HEVC Main10 PQ maturity vs Vulkan ProRes — to weigh with the perf baseline, #47) |
| 46 | No-copy CPU→GPU wraps (image sequences, output rings) via hal, not `write_texture` | yes, in `min-present` | open |
| 47 | A measured perf baseline (both boxes) BEFORE any QCView renderer work | yes | open |

---

## 13. WHERE WE ARE — resume here (end of day 2026-11-05)

**State:** a theoretical discussion, nothing scheduled, nothing built. One repo change: `/private/`
added to `.gitignore` (uncommitted). This file (§1–12) + `private/README.md` are the record.

**Rulings taken so far (user):**
- Windows video for minNotes: DX12 + d3d12va (§4.4 W1; decision 6).
- Backend per app (decision 45) is PROPOSED; the user is inclined to weigh DX12 for QCView too
  ("better PQ / H.265"), bridge to cross later with the perf baseline (decision 47).
Everything else in the Decisions tables (§5, 7.5, 8.13, 9.1, 10.5, 11.6, 12.7 — 47 rows) is a
proposed default awaiting the user.

**The through-line so far:** build a Rust MEDIA PLATFORM for the family first (`qcv-player`,
`qcv-notes`, `qcv-ink`, `min-image`, `min-color`, `min-present`), consumed from the Qt apps via
FFI (ufb already has cxx-qt); minNotes is the first native app on it with its own from-scratch
wgpu UI (`min-ui`, one node tree, four layout kinds, computed layout, one markdown grammar);
QCView adopts last and is never ported for speed. **→ REVISED by §21 (2026-11-06): QCView-SHAPED spikes + reference app FIRST as the test; the Qt QCView product is archived last; the parity bar stands.** HDR is a presentation-stage concern; the SDR
apps compile three of four presenter stages.

**NEXT TOPICS (user, 2026-11-05 evening) — in order:**
1. ~~Drag and drop into specific panels, across ALL the apps~~ — **SURVEYED + PROPOSED 2026-11-06,
   see §14** (four repos read; 10 decision rows #48–57 await the user; the one pre-port deliverable
   is `min-paths`, #51). Nothing ruled yet.
2. ~~ufb as a FILE BROWSER~~ — **SURVEYED + PROPOSED 2026-11-06, see §15** (the UI shell + the
   Rust core read; rows #58–67 await the user; headline: the OS half is already outside Qt, the
   file-browser half is MISSING on both sides, the JSON bridge is the cost; ufb stays Qt and
   becomes the SECOND `min-ui` consumer; a toolkit-independent bug list to fix now).

**Both of the user's topics are now on paper. 2026-11-06 rulings: defaults #48–65 stand; ufb LAST; the
tracking layer STAYS incl. its MESH sync (#66); the mount CACHE layer GOES (#68); previews without a webview + the lightbox hosting rule
→ §15.6. Plain SMB mounts stay (#71). The agenda = the Decisions tables (71 rows, 6 ruled). Paneling / rails /
widgets SURVEYED + PROPOSED → §16 (no docking anywhere; ~45-kind widget union; text input = the long pole;
rows #72–82 open). The TIMELINE read hard → §17 (a content
surface, not a widget; rational time, `qcv-timeline` + one Sequencer, gapless playlists; rows #83–90). OCIO → §18 (native
passthrough as the display path, baked LUT as fallback; rows #91–94). Scopes + guides → §19 (two
overlay classes: OUTPUT via a vello texture, UI as nodes; rows #95–100). The QCBRIDGE companions audited → §20
(three byte-stable contracts to FREEZE; `qcv-ring` = QCView's end in Rust with cross-impl tests; companions
untouched; rows #101–108). **108 rows, 7 ruled.** THE PATH FORWARD → §21 (2026-11-06): QCView-SHAPED spikes first (S1 present, S2 scrub, S4 ring, S5 window, S3 text in parallel; P0 = the perf baseline), ONE new workspace repo (`minplatform` [name]) with the canon in `docs/`, `min-ui` born inside a QCView-shaped reference app, minNotes the SECOND consumer, gates G1/G2; rows #109–118. **118 rows, 8 ruled. 2026-11-06 the first 'go' was GIVEN and DONE: this repo exists (private), the canon lives here, P0 is measured (`docs/spikes/P0-baseline.md`; Windows owed). S1 + S3 SCAFFOLDED the same day (wgpu 30 HDR surfaces native — EDR-via-objc2 CLOSED). **S1 steps 1 + 1b DONE 2026-11-06:** real video (ProRes 4K/1080p, HEVC10 4K, H.264 4K) on the EDR surface, software+CPU-upload control AND VideoToolbox ZERO-COPY into wgpu via hal (`docs/spikes/S1-present.md`): open→first frame 123–195 ms vs P0 385–500, present p95 17 ms vs QCView's 17–20, 0.1 ms/frame on the render thread, picture verified (snapshot; zero-copy vs control PSNR 58.7 dB). **S1 step 2 DONE 2026-11-06:** OCIO 2.5's generated MSL runs through wgpu 30's `PASSTHROUGH_SHADERS` path in QCView's own kernel wrapper, LUTs as wgpu textures, and is EXACT vs OCIO's CPU processor (≤ 0.13/255 on ACES 1.3, Blender AgX 3D-LUT, Blender Filmic 1D+3D-LUT chains at 4K); the baked 65³ LUT (route B) is 8-bit-exact only (worst 1.7/255, ~1 % of samples off at 10-bit); the colour pass costs 0.3–0.5 ms at 4K either way, pacing unchanged (`docs/spikes/S1-present.md`). Rows #91/#92 now have their evidence (A = display path, B = fallback/export). **S1 step 3 (2026-11-06, in progress):** the spike matches QCView's own `QCV_CAPTURE_PNG` of the same 4K frame at 55.9 dB PSNR / SSIM 0.998 (max 2/255); a synthetic PQ ramp decodes exactly to the file through BT.2020 + PQ onto the extended-linear surface and the built-in XDR ENGAGES EDR (headroom 1.0 → 8.5 while the window is up); **wgpu-hal 30.0.1 BUG found: `Bt2100Pq` / `Bt2100Hlg` / `ExtendedDisplayP3` surfaces CRASH in `configure` on Metal (the dynamic kCGColorSpace lookup passes the symbol's address as the CFString) — worked around in the spike by setting the layer's colour space by name; upstream report owed.** Step 3 DONE the same day (the PQ surface shown on the XDR, EDR headroom 8.8; software + VideoToolbox PQ identical; footprint 0.40–0.66 GB vs QCView 1.36). **S1 VERDICT 2026-11-06: GO on macOS** — every bar met except the whole-frame GPU figure of the UNFUSED spike graph (1.9 ms p95 vs the 1 ms bar; the colour pass itself 0.3–0.5 ms — the fused presenter reads the planes inside the first colour stage); Windows numbers (DX12 + D3D12VA + DXIL + PQ) OWED before gate G1. **S2 SCRUB DONE 2026-11-06, VERDICT GO (macOS; `docs/spikes/S2-scrub.md`):** rational time + two prewarmed decoder slots + a software scrub decoder (slice threads intra / frame threads + retarget + GOP-wide LRU inter) on the S1 presenter: seeks 15.9 ms (4K ProRes) / 7.4 ms (1080p 5-min) via the scrub decoder vs QCView 60–130; scrubs 5.6 / 3.5 ms vs 30–40; H.264 scrubs 1.3 fwd / 13.5 p95 back; the playlist CUT is GAPLESS (no black, no dup, exact 3:2 cadence, mid-GOP H.264 too); shuttle 2×→32× one frame per refresh; per-video-frame lateness p95 < 1 refresh with 0 drops (the number P0 could not take). Rows #83 (rational time) and #85 (two slots) have their evidence; the §4.2 scrub-decoder facts gained threading rules. Owed: Windows, audio. Next = S3 step 1 (AccessKit) or S4 ring, or rulings on #109–118 / #48–108.**

**How to resume:** read §13, then §14 (DnD) + §15 (ufb), both awaiting rulings, then §10–12 for the
family picture; the Decisions tables are the agenda (108 rows; §15.6 = the latest rulings; §16 widgets/panels; §17 the timeline; §18 OCIO; §19 scopes + guides; §20 the QCBridge contracts; **§21 THE PATH FORWARD — read this before any 'go'**). Memory file: `minnotes-rust-wgpu-port-discussion`.

## 14. Drag and drop into specific panels, across the family — surveyed 2026-11-05/06

Four read-only surveys (ufb, minNotes, QCView-Player, MinRender) + the Rust platform state.
Headline: **every app's DnD is Qt-level today and almost nothing is native**; the hard-won edges
live in ufb's and QCView's code COMMENTS (ufb's history is a squashed import from 2026-07-12, so
the "year of fixes" is not in `git log`). A from-scratch UI relearns the Qt-specific half of
nothing and the OS-specific half of everything — and the OS half is a known, finite list (§14.2).

### 14.1 What exists today, per app

| App | Drop targets (what they accept) | Drag sources | Native code |
|---|---|---|---|
| **minNotes** | 3 QML DropAreas: **canvas** (`text/uri-list`, `text/plain`, `text/html`; files → per-type router, http → bookmark/remote image, text → chip; joins the frame under the point; NO hover feedback), **page/editor** (`text/uri-list` only; aims a lane gap / side edge / header-or-untyped table cell with a pulsing line; gated off under a full-frame tab), **welcome screen** (.mnd/.mnpkg/importables). No drops on the lightbox, player, media blocks, kanban, Layers, tabs (tab-merge drag REMOVED, ruling 2026-10-18). | **None OS-level, ever** (no QDrag / `Drag` in the whole history). All internal drags hand-rolled: rail block drag (4 px, rail chip, no content ghost), table row/column grips, lane pull (⌘), canvas element move + ⌘-drag frame membership (frame lights), resize, marquee, Layers reorder, kanban card (ghost + edge timer), title-row window move. | none for DnD; `QClipboard` only. Rich clipboard = `application/x-mnd-blocks` (NOT "x-minnotes-blocks" — the memory name is wrong), clipboard-only. |
| **ufb** | One pane-level DropArea per file pane (z:100 over the views; per-row areas "would never receive the drag"); sidebar bookmark / job / mount rows (mount only when ready); kanban lanes (internal `ufb/kanban-card`); column-manager reorder. Not: tabs, path bar, player, lightbox. | File rows → `text/uri-list` ONLY (list/grid/tree), `Drag.Automatic`, CopyAction only, grabToImage drag image w/ an 80 ms fallback under software rendering (RDP); kanban cards `Drag.Internal` on purpose ("no DoDragDrop modal loop"). | none for DnD. Rust owns the **payload codec**: `path_to_file_url`, `to_uri_list`, `file_url_to_path`, `same_volume`, `is_drop_onto_self`, copy/move (sync-aware); plus objc2/Win32 **clipboard** (per-item `public.file-url`; CF_HDROP + "Preferred DropEffect"). |
| **QCView** | Viewport = **native per OS**: macOS `PlayerWindow::event()` (QWindow has no drag virtuals), Windows a COM `IDropTarget` on the D3D11 child HWND (`CF_HDROP` only → URL/text drags die over the surface on Windows); side-aware A/B highlight as a shader uniform. QML DropAreas: centerStage (only live while a modal hides the surface — so drops ARE accepted under a modal), media bin, A/B chips, playlist (snap preview), timeline lanes A/B. All funnel through `admitMediaPaths` (one extension gate, `.lnk` resolution, toasts). No LUT/OCIO/sidecar/.qcvproj drop. Dock/Finder opens via `QEvent::FileOpen`. | **One**: bin rows → `text/uri-list`, `LinkAction` only (an unrestricted action let Finder MOVE the real files on an accidental drop). No frame-grab drag-out (clipboard only). | the D3D11 `IDropTarget` (+ `OleInitialize` refcount dance); Metal/HLSL highlight. No NSPasteboard code at all. |
| **MinRender** | **none** (jobs arrive via a watched submissions folder; dialogs only). In-app queue reorder = plain MouseArea, 32-px row math, refresh paused mid-drag. | none | none |

What ALL FOUR share: file lists in → `text/uri-list` → local paths; a 4–6 px press threshold,
never press-starts; `preventStealing` against Flickables; delegate recycling as the enemy of
a live drag; the window-move-by-chrome handle (`DragMouseArea`). What NONE do: promised files,
virtual files (Outlook `FileGroupDescriptor`), app-private cross-process types, spring-loading,
drag-out of anything that is not already a file, drops onto the dock/taskbar.

### 14.2 The edges worth carrying (the "do not relearn" list)

Split by whether they survive a from-scratch UI. **Qt-specific, GONE with Qt:** `DropArea.keys`
filtering on `Drag.keys` not mime (Finder drags have no keys → any filter rejects everything);
topmost-enabled-DropArea-wins regardless of declaration order + DropAreas shadowing presses;
Qt6 truncating `drop.urls` to ONE entry on native multi-file drags (ufb reads both `urls` and
the raw `text/uri-list` and keeps the longer); declarative `Drag.mimeData` bindings going stale
before `Drag.active` flips (assign imperatively); a `QQuickItem` holding a live `Drag` dying on
model reset → qFatal (ufb's pane-scoped `dragProxy`, minNotes' persistent content MouseArea);
`drop.modifiers` unavailable in QML (ufb's Move/Copy dialog exists because of it); QWindow lacking
drag virtuals; the native-surface-above-QML "three-plane" model (QCView: no QML DropArea behind
the surface ever sees a drop; Windows OLE does NOT fall through `HTTRANSPARENT`, a child HWND
needs its own `IDropTarget`).

**OS-specific, STAY (and are the spec for `min-dnd`):**
- **Windows URL forms.** `file:///C:/…` (three slashes; two makes `C:` the host). UNC as
  `file://server/share/…`, never `file://///`. A drive-less path (`\Volumes\X` after a cross-OS
  mapping) must NEVER be emitted: `file:////Volumes/X` reads as UNC `\\Volumes\X` and the failed
  host lookup **wedges OLE drag-and-drop system-wide until a reboot** (ufb `path_to_file_url`,
  test `file_url_never_emits_four_slashes`). Incoming: strip the `/` before `X:`, decode, `\`.
- **uri-list = CRLF-terminated, INCLUDING the last line**, or the native CF_HDROP translation
  drops the final file (ufb `to_uri_list`). `#` lines are comments.
- **macOS multi-file = one `public.file-url` per item** via `writeObjects:` NSURLs; one legacy
  `NSFilenamesPboardType` plist collapses to ONE item through the UTI bridge. Read with
  `readObjectsForClasses:[NSURL]` (covers legacy plists, `furl`, `file://localhost/`).
- **Windows OLE threading.** The GUI thread is the OLE STA (`OleInitialize`, refcounted — Qt's
  `S_FALSE` lesson); WinRT/MTA work goes to throwaway threads or `OleInitialize` fails.
  `DoDragDrop` is a modal loop: hover state can go stale, `positionChanged` is not guaranteed →
  **recompute the target from the drop point at drop time**, never trust the cached hover.
  Window deactivation under the pointer = treat as leave/cancel (ufb kanban cancels on
  `Window.active == false`).
- **Clipboard/OLE delayed rendering.** `GetClipboardData` on an Explorer/Office/browser/Adobe
  owner is a synchronous `WM_RENDERFORMAT` round trip that stalls the GUI thread — probe with
  `IsClipboardFormatAvailable` only; fetch bytes off-thread or on demand (winit 0.31's async
  fetch is the right shape).
- **Drop onto self.** A drag-out that returns and drops on its own source pane: a naive copy
  `File::create`s the destination first and **truncates the source to 0 bytes** (ufb
  `is_drop_onto_self`: canonicalise, `\`→`/`, trim, lowercase). Still missing there: folder into
  its own descendant; collisions overwrite silently.
- **Action semantics.** Drag-out of REAL files to Finder/Explorer: Copy (ufb) or Link (QCView,
  so an accidental drop never moves the user's media). Internal move vs copy: modifiers at drop
  time (Ctrl/⌘ = copy, Shift = move) with `same_volume` as the default; ufb's sidebar and pane
  disagree today — the port picks one.
- **Live/non-file URLs** must round-trip untouched (`srt://` got mangled to `file://srt://`).
- **Refusals must be visible** (QCView: every drop through one gate + a toast; silent vanishing
  was a bug). Folders: refuse (QCView) or expand (minNotes → folder element) — per app.
- **`.lnk` resolution** on Windows drops (QCView).
- **Remote desktop.** A click jitters past the drag threshold (minNotes 634810e: the drag path
  must still do the click's focus work); software rendering stalls the drag-image grab (ufb's
  80 ms fallback); wheel arrives as TouchPad notches. Thresholds: 4 px (editor), 6 px (rows,
  chrome) — keep per-surface thresholds, never press-start.
- **Modal cover.** Drag events are NOT gated by the modal gate today (QCView centerStage accepts
  drops under a modal) — the router must gate drops with the same predicate as pointer input.
- **Refresh pause mid-drag** (MinRender's 2 s backend refresh, ufb's async listing): a live drag
  freezes the model under it; defer rebuilds, replay after.

### 14.3 What the Rust platform gives a from-scratch UI (2026-11-06 state)

- **winit 0.31 (beta.3 2026-09-04, beta.4 since; 0.31.0 has no date)** replaced
  `DroppedFile/HoveredFile` with a **typed data-transfer API**: `DragEntered{id, position}`,
  `DragPosition{…, proposed_action}`, `DragDropped`, `DragLeft`; the app lists the offer's
  types and fetches bytes ASYNC (`fetch_data_transfer` → `DataTransferReceived`); the same API
  serves the clipboard (`ActiveEventLoop::clipboard/set_clipboard`, macOS + Windows + Wayland
  + X11). Receive side + clipboard: **covered, probably** (open: which type names surface —
  `text/uri-list` vs `public.file-url`/CF_HDROP translation, HTML, images, app-private types —
  verify in `winit-core/src/data-transfer.rs`). **Drag-OUT: not seen** in the API.
- **Drag-out**: the `drag` crate (CrabNebula, used by Tauri) starts an outbound file drag from a
  raw-window-handle on macOS/Windows with a preview image + a result callback. Files only; no
  promised files; no custom types. Small enough to own instead.
- **Native, when winit is not enough**: macOS `NSDraggingDestination` on an `NSView` subclass
  via objc2 `define_class!` + `registerForDraggedTypes` (features `NSDragging`, `NSPasteboard`);
  winit owns its NSView class, so this is an overlay/subclass or the data-transfer API, not
  both. `NSDraggingSource` + `NSFilePromiseProvider` for drag-out / promises. Windows: ONE
  `IDropTarget` per HWND (`RegisterDragDrop`); winit registers its own → use the opt-out
  (`with_drag_and_drop(false)` on `WindowAttributesExtWindows`, verify it survived 0.31) and
  register ours; `IDataObject` + `IDropSource` for drag-out (`#[implement]` in windows-rs);
  `CFSTR_FILEDESCRIPTORW` + `CFSTR_FILECONTENTS` for virtual files both ways.
- **The structural win**: a wgpu window is ONE native view / ONE HWND. QCView's child-HWND
  `IDropTarget`, the macOS `event()` override, the hidden-surface fallback DropArea, the
  three-plane model — all disappear. One OS drop target per window, then the app's own node
  tree hit-tests the zone. "Into specific panels" becomes a tree question, not a toolkit one.

### 14.4 Proposal: DnD as a property of the node tree + one shared payload codec

**(a) `min-dnd` lives in `min-ui`'s input router (§8.9), not in a crate of its own.**
- A **drop zone** is a node attribute: `accepts: fn(&Offer) -> Option<Action>` + `on_hover` /
  `on_drop`. Hit-test = the same top-down tree walk as the pointer; the **innermost accepting
  zone wins** (Qt's "topmost DropArea" without z-order accidents). A zone may be a page gap,
  a lane edge, a table cell, a frame, a timeline snap index — the zone computes its own
  sub-target from the point (minNotes' `aimDrop`, QCView's `snapEdgeIndexAtTime`) and returns
  a **DropAim** the overlay layer draws from tokens (line / side bar / cell wash / lane tint /
  frame border — the existing vocabulary, as data).
- **One `DragSession`** in the router: `{ payload: Offer, origin: Internal(node) | External
  (DataTransfer id), image, allowed_actions, modifiers-at-drop }`. Internal drags never leave
  the process unless STARTED as an OS drag (explicit `start_os_drag(files|text|image)`).
  Generic behaviours on the session, not per zone: **edge auto-scroll** (one timer, 44 px
  margin, re-aim after scroll — minNotes'), **spring-loading** (hover-timer on a zone flagged
  `container`), **cancel** on Esc / window deactivate / grab loss, **modal gate** = the
  pointer's predicate, **model-rebuild deferral** while a session is live, drop target
  **recomputed at drop**.
- **`Offer` = typed items, lazily fetched**: `Files(Vec<PathBuf>)`, `Urls`, `Text`, `Html`,
  `Image(bytes)`, `Virtual(Vec<FileDescriptor>)` (materialised to the scratch dir on demand),
  `AppPrivate(TypeId, Arc<dyn Any>)` (process-local, never serialised). Cross-process family
  types = files + a `qcview://` / `minnotes://` link line in `text/plain` — no shared binary
  pasteboard type (ruling-needed, #52).
- **Thresholds and feel**: 4 px editor / 6 px rows + chrome; no press-start; drag image =
  node snapshot via the renderer (no async grab, no RDP fallback needed — we own the frame);
  no content ghost for block drags (ruling 2026-07-12 holds), rail chip + line.

**(b) The shareable piece is the PAYLOAD CODEC, and it already exists in Rust (ufb).**
`min-paths` (Tier 2 kit, §10.3): `path_to_file_url` (four-slash guard, UNC, drive), `to_uri_list`
(CRLF incl. last), `file_url_to_path` (+ `file://host/share` and `file://localhost/`, which ufb
misses), `same_volume` (a real macOS impl via `statfs`, not first-component), `is_drop_onto_self`
(+ descendant guard), objc2/Win32 clipboard file lists (per-item file-url; CF_HDROP + Preferred
DropEffect; `IsClipboardFormatAvailable` probe only), plus minNotes' `{vol,rel}` PathMap refs and
the network/volatile source classifiers (`isNetworkSource`, `isVolatileSource`). Consumed by ufb
today (it is ufb's code), by minNotes' Qt build via FFI if wanted before the port, by `min-ui`
natively. **This is the one DnD deliverable that pays before any UI exists.**

**(c) Per-app zone inventory for the port (today's zones kept; marked ⊕ = new, ⊖ = declined).**
- **minNotes**: canvas (⊕ hover feedback + the frame-under-point LIGHTS for external drops, as
  it does for ⌘-drags); page (gaps / lanes / cells as today; ⊕ accept `text/plain`/`html` into a
  gap as a paragraph, ⊕ http URL → bookmark chip instead of a file chip with an http "path");
  frames on the canvas (zone = the frame; drop joins — today's behaviour, now explicit);
  Layers rows (⊕ files dropped on a frame row join that frame); welcome; ⊖ tabs (2026-10-18
  ruling holds); ⊖ lightbox (a viewer, not a target); ⊖ player. **Drag-out ⊕**: any media
  element/block out to Finder/other apps as its REAL file (it is in `.minnotes` or referenced —
  a Copy drag, never Link/Move); blocks out as `text/plain` markdown + `x-mnd-blocks`
  (app-private, same process only) — the clipboard codec reused.
- **QCView**: one window-level target replaces viewport+centerStage; A/B side-aware aim stays
  a zone sub-target (the shader highlight stays a presenter uniform); bin, A/B chips, playlist
  snap, lanes as today; ⊕ Windows surface accepts URL/text (`srt://`) like macOS; ⊕ frame-grab
  drag-out as a temp PNG (real file, not a promise); ⊕ `.qcvproj` / sidecar / LUT drops as
  gated zones (ruling-needed, #55). Bin drag-out stays Link.
- **ufb**: unchanged in Qt (§10.2 holds); adopts `min-paths` by replacing its own copies in
  place (it IS the origin). The pane-vs-sidebar action inconsistency is a ufb bug to fix now.
- **MinRender**: ⊕ queue accepts file/folder drops as a submission shortcut (the dropbox stays
  canonical); nothing else. Lowest priority.

### 14.5 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 48 | DnD architecture | zones as node-tree attributes + ONE `DragSession` in the input router; innermost accepting zone wins; generic autoscroll/spring/cancel/modal-gate on the session | open |
| 49 | OS integration | winit 0.31 data-transfer API for receive + clipboard; OWN native drag-out (objc2 `NSDraggingSource` / windows-rs `IDataObject`+`IDropSource`), the `drag` crate only as a reference | open — verify the type-name surface first (§14.6) |
| 50 | Promised / virtual files | v1: NONE out (write a real temp file, then drag); IN: materialise `FileGroupDescriptor` / promises to scratch on drop (volatile → copies anyway) | open |
| 51 | `min-paths` crate (Tier 2) | extract ufb's path/url/uri-list/same_volume/self-drop + clipboard file-list code + minNotes' PathMap refs & source classifiers; ufb consumes it first | open — **the first DnD deliverable, pays pre-port** |
| 52 | Cross-process family payloads | files (`text/uri-list` / per-item `public.file-url` / CF_HDROP) + a `qcview://` / `minnotes://` link in `text/plain`; no shared binary pasteboard type | open |
| 53 | Drag-out actions | real files: Copy from minNotes/ufb, Link from QCView's bin; internal move/copy: modifiers at DROP (Ctrl/⌘ copy, Shift move), `same_volume` default | open |
| 54 | minNotes new zones | canvas hover + frame lighting for external drops; Layers frame-row file drops; page text/html/URL drops; media + block drag-out | open |
| 55 | QCView new zones | window-level target (fixes Windows URL drops), frame-grab drag-out as temp file; `.qcvproj` / sidecar / LUT drop zones | open |
| 56 | Refusal UX | every drop through one per-app gate + a visible toast (QCView's model) — in all four apps | open |
| 57 | MinRender queue drops | yes, as a submission shortcut; last | open |

### 14.6 Verify before building (adds to §6)
- winit 0.31: the exact `DataTransfer` type-name surface per OS (uri-list vs file-url vs HDROP,
  HTML, images, custom UTI/CF formats); whether drag-OUT exists at all; whether
  `with_drag_and_drop(false)` survived; whether the macOS backend already implements
  `NSDraggingDestination` on its view (then an overlay subclass conflicts).
- objc2: `NSFilePromiseProvider` + `NSDraggingSource` class definitions compile cleanly with
  `define_class!` (delegate protocols with optional methods).
- windows-rs: `#[implement(IDropTarget, IDataObject, IDropSource)]` ergonomics; `OleInitialize`
  on winit's thread before window creation; `CFSTR_FILEDESCRIPTORW` round trip with Outlook.
- Measure: how much of the §14.2 list a 300-line native layer needs vs winit alone (a spike,
  not a crate).

## 15. ufb as a FILE BROWSER — system ops, UX expectations, and whether its UI ever leaves Qt (2026-11-06)

Two read-only surveys (the QML/C++/native shell; the Rust core + agent) + the Rust-ecosystem
state. Headline: **ufb is not a Finder/Explorer-class file browser today; it is a VFX production
browser**: a hand-built QML list/grid/tree (one 3,987-line `FileBrowser.qml`) over a JSON listing,
on a Rust core that is mostly **mounts/VFS (agent ~17k) + production tracking (columns,
subscriptions, mesh ~8k)**, plus the family's media previews. The §10.2 premise "ufb's UI is lists
and trees — Qt's strength" is **wrong in detail**: ufb uses NONE of Qt's item-view machinery
(no QFileSystemModel, TreeView, TableView, DelegateModel, QFileIconProvider, QMimeDatabase,
QFileSystemWatcher, QStorageInfo). It uses ListView/GridView/ListModel and JS. Qt earns its keep
in ufb in exactly four places: **DnD (`Drag.Automatic` + `DropArea`), the `QQuickRhiItem` player,
the async image providers + thread pool, and QtWebEngine for HTML/docx preview.**

### 15.1 What a file browser is expected to do — and where ufb stands

| Expectation | macOS / Windows mechanism | ufb today | Where it lives |
|---|---|---|---|
| Native context menu | Finder's menu is NOT summonable (no public API); Explorer's is (`IContextMenu`) | mac: own `NSMenu` (Open / Open With via LaunchServices / Show in Finder / Get Info via Apple event + Automation consent / Share picker / Tags); win: `IContextMenu` → `TrackPopupMenuEx` on a fire-and-forget STA thread, **single path only** | `MacShellMenu.mm` (AppKit), `shell_context_menu.rs` (Rust) |
| Reveal / Open / Open With | `NSWorkspace` / `opener` / LaunchServices | yes; **win reveal = first path only**; label says "Reveal in Explorer" on Mac | Rust |
| Quick Look / preview handlers | `QLPreviewPanel` / `IPreviewHandler` | in-app lightbox (Space) with QCView keys; `qlmanage -p` wired but unused; no Windows preview handlers | QML + C++ doc renderers |
| System icons | `SHGetFileInfoW` (fake `C:\fake.<ext>`), `NSWorkspace iconForContentType` | per-EXTENSION only; no per-file / .app / overlays; 256-px jumbo always (small lists came back blank); `OleInitialize` per pool thread + 5× retry | C++/ObjC++ |
| Shell thumbnails | `IShellItemImageFactory`, `QLThumbnailGenerator` | stage 0 of the thumbnailer, 10 s timeout, detached thread | C++/ObjC++ |
| Trash | `NSFileManager trashItemAtURL` (no Put Back) / `IFileOperation` | `trash` crate; fallback win = `SHFileOperationW` dialog; **fallback mac = `remove_dir_all` = SILENT PERMANENT DELETE** | Rust |
| Copy/move with progress, cancel, conflicts | OS: `IFileOperation` / `NSFileManager` + own UI | `fs_extra` one item at a time; progress yes (100 ms throttle, footer chips); **no cancel; `overwrite = true` always (silent); no skip/rename/merge; no undo** | Rust + QML |
| Rename in place | stem selection, validation, case-only, reserved names (`CON`, trailing dots/spaces), long paths | modal dialog, stem selected; **no validation; bare `fs::rename` REPLACES an existing target; errors go to `console.warn` only** | QML + Rust |
| Natural sort | Finder/Explorer both | **no** (lowercase `<`, twice: JS and Rust) | — |
| Watching | FSEvents / `ReadDirectoryChangesW` (NOT on SMB/NFS mounts → polling) | **none**; own ops emit `dirs_changed` + 350/1500 ms timers; `notify` declared, unused; the agent's FSEvents watcher deleted | — |
| Listing | typed, streamed, cancellable | `read_dir` + `metadata` per entry, sequential, **whole listing as ONE JSON string**, `ListModel.clear()+append` per refresh; no cancel, no paging; `FileEntry` = name/path/isDir/size/mtime/ext only (no ctime, hidden flag, symlink flag, tags, attributes) | Rust + QML |
| Image-sequence collapsing | (VFX expectation; Finder doesn't) | **none** — every frame a row (QCView has a detector; minNotes' folder element another) | — |
| Selection | shift/⌘ ranges, rubber band, type-ahead | ranges yes; **no rubber band, no type-ahead**; selection survives reload by path | QML |
| Keyboard per OS | mac: ⌘↓ open, ⌘↑ up, ⌘⌫ trash, Return = rename, ⌘I; win: Enter open, Backspace up, F2, Del | ONE map for both (Enter open, Backspace up, F2, Del, F5, Ctrl+L) → on a Mac keyboard "Del" is forward-delete and Delete goes UP | QML |
| Views | list / icons / columns(Miller) / gallery | list / grid (48–256) / flat tree; **no columns view**; per-folder view prefs (Finder-style) | QML |
| Sidebar | Known Folders / `NSSearchPath`, volumes, recents, tags | hard-coded `home + "Desktop"` …; bookmarks, jobs, trackers, mounts, mesh; **no volumes list, no recents** | QML |
| Hidden files | toggle (⌘⇧.) ; Windows attributes | fixed filter (.gitkeep/.DS_Store/Thumbs.db/desktop.ini); dotfiles shown | QML |
| Symlinks / aliases / `.lnk` | badge + resolve | followed, no badge, unresolvable = "directory"; no `.lnk`/alias resolution | Rust |
| Long paths / UNC | `\\?\`, `longPathAware` manifest | `\\?\` stripped only; no manifest; UNC breadcrumbs partial; incoming `file://host/share` mis-parsed | — |
| Status / counts / free space / folder sizes | all four | "N selected" only; dirs show "—" | QML |
| Cloud / sync state in-list | badges, hydrate-on-demand, placeholders | **none in ufb's own list** (badges only in Finder via the FinderSync appex); `sync_aware.rs` is STALE (targets `C:\Volumes\ufb` + Cloud Filter; never fires on the WinFsp letter) | Rust / Swift |
| Mounts | — | the strongest part: GUI-owned plain SMB (NetFS / WNet, Keychain / Cred Manager), agent VFS (NFS loopback + SQLite chunk cache on mac; WinFsp drive letter on win), FSM shared, heal-on-open, conflicts as sidecars, FinderSync badges | Rust (core + agent) |
| Search | Spotlight / Windows Search | `mdfind` / Everything `es.exe` + a Tantivy name index (full rebuild per start; incremental unwired); mac fallback walk is the common case on mounts | Rust |
| Archives / transcode | — | 7-Zip CLI (never overwrites), ffmpeg/ffprobe/exiftool CLIs, both cancellable (the ONLY cancellable ops) | Rust |

**Bugs worth fixing NOW, toolkit-independent** (all in Rust or trivial QML): the silent permanent
delete on trash failure (mac); silent overwrite on copy/move AND rename; swallowed rename errors;
the stale `sync_aware` fast path; Windows reveal of one path; the Mac wording ("recycle bin",
"Reveal in Explorer") and the Del/Backspace map; the pane-vs-sidebar drop action split (§14);
incoming UNC file URLs. None of these need a design discussion.

### 15.2 The honest read

- **The OS-bound half is ALREADY outside Qt.** Shell menus, icons, shell thumbnails, trash, reveal,
  open, clipboard, mounts, VFS, badges: AppKit/Win32 in `.mm`/`.cpp`/Rust with no Qt in them
  (the icon/thumbnail code hands back images; that's the only seam). A non-Qt UI inherits all of
  it unchanged. This is the opposite of QCView, where the native half is the renderer.
- **The file-browser half is mostly MISSING, on both sides**, not Qt-bound: no watcher, no natural
  sort, no sequences, no cancel/conflict/undo, no rubber band, no type-ahead, no per-OS keys.
  These are UI-independent contracts that belong in the Rust core and do not exist there yet.
- **The bridge is the bottleneck, not the toolkit.** ~10.5k lines of cxx-qt bindings passing JSON
  strings; the listing is one blob; `DirectoryTree` re-serialises the whole tree per expand;
  `active_ops_json` re-serialises every op per tick; no QAbstractListModel because cxx-qt 0.7
  had none (verify 0.8+). Spacedrive, the one serious Rust file-manager core, restarted as a
  **daemon with a typed JSON-RPC API** for exactly this reason (alpha; beta due 2026-11-15).
  ufb already has that split for mounts (the agent); the browser core stays in-process.
- **Half of ufb's UI is not a file browser at all**: tracker / item list / kanban / columns /
  settings ≈ 8.1k of 20.7k QML lines are a production-tracking product (forms + grids) sharing the
  shell. Any toolkit decision is for BOTH surfaces.
- **The §9 extraction trigger**: `min-ui` leaves the minNotes workspace on a second real
  consumer. ufb is the only candidate in the family, and its two surfaces (list/grid/tree +
  tracker grids) are the `row`/`grid` layout kinds' real test. QCView never qualifies (§12).

### 15.3 Proposal

**(a) Now, in Rust, toolkit-independent — the "file browser contract" (`ufb-core` grows up):**
1. **Typed listing**: a rich `Entry` (kind incl. symlink/alias/lnk + target, hidden/attribute
   flags, ctime/mtime/atime, size, Finder tags/labels, cloud/hydration state from the agent,
   sequence membership), **streamed in chunks, cancellable**, natural-sorted in Rust (ICU-style
   numeric collation; `natord`/`icu_collator`), per-folder view state as data.
2. **Watcher**: `notify` 7 (declared, unused) for local trees; **polling** on SMB/NFS/WinFsp
   mounts (FSEvents/RDCW do not fire there — verify on the WinFsp letter) with a
   dirs-changed bus that unifies own-ops events, watcher events and the agent's sync events.
3. **File ops**: an own walker (not `fs_extra`) with **cancel**, a **conflict policy** (ask once:
   overwrite / skip / keep both / merge folders, "apply to all"), an **undo journal** (rename back,
   restore from trash where the OS can, delete the copy), **trash that never falls back to
   permanent delete**, server-side copy on SMB where available, long-path + reserved-name +
   case-only-rename rules, `.lnk`/alias resolution. `trash` crate with the Finder route for Put
   Back on mac (verify).
4. **One sequence detector** for the family — QCView's rules (3+ digits, 2+ siblings, `_.-`
   separators) as the reference — in `min-paths` (§14 #51), used by ufb's listing, minNotes'
   folder element, QCView's import, MinRender's queue.
5. **Replace the JSON bridge with a typed one**: a C++ `QAbstractListModel` over Rust-owned storage
   (cxx, not cxx-qt, for the model), typed structs for ops/mount state; delete the `*_json`
   properties. Same typed API is what `min-ui` would consume later — **this is the step that
   makes the toolkit question cheap** (the Spacedrive lesson without the daemon).
6. The §15.1 bug list.

**(b) The UI: ufb stays Qt until `min-ui` has shipped minNotes; then ufb is the SECOND consumer
and the extraction trigger (§9).** Reasons it is not first: it is a product in use (1.4.0) with
two surfaces; the four things Qt still provides it (DnD, player item, image providers, WebEngine)
are exactly what the platform plan builds anyway (§14 DnD, `min-present`, `min-image`) — except
**WebEngine**: HTML/docx/mnd preview would move to the platform webviews (WKWebView / WebView2
via `wry`) or be dropped (ruling #64). Reasons it is not never: ufb uses nothing of Qt's item
views; its real UI is JS over JSON; the bridge is its cost; and a second consumer is what proves
`min-ui` is a toolkit rather than minNotes' private renderer. **The per-OS key map, the context
menu grammar and the sidebar roots become DATA now** (like §8.9's menus), so they port verbatim.

**(c) What NEVER moves:** the agent (NFS/WinFsp/cache FSM), the FinderSync appex (Swift), NetFS/
WNet mounting, the Win32 `IContextMenu` thread, `MacShellMenu` (becomes objc2 or stays ObjC++ —
irrelevant to the toolkit). The thumbnail pipeline's OS stage 0 likewise; its decoders are
`min-image` (§10.4 step 2, ufb FIRST).

### 15.4 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 58 | ufb's UI toolkit | stays Qt now; becomes the SECOND `min-ui` consumer (and the extraction trigger) only after minNotes ships on it | open |
| 59 | Typed bridge now | replace `*_json` cxx-qt properties with a C++ `QAbstractListModel` over Rust storage + typed structs; the same API later feeds `min-ui` | open — **pre-port, pays now** |
| 60 | Listing contract in Rust | rich `Entry`, chunked + cancellable, natural sort, per-folder view state, sequence membership | open |
| 61 | Watching | `notify` locally; polling on mounts; one dirs-changed bus (ops + watcher + agent sync) | open |
| 62 | File-ops contract in Rust | own walker: cancel, conflict policy w/ apply-to-all, undo journal, trash never → permanent, long paths / reserved names / case-only, `.lnk`+alias resolution | open |
| 63 | Sequence detector | ONE, QCView's rules, in `min-paths`; all four apps | open |
| 64 | HTML / docx / mnd preview off Qt | platform webviews via `wry` (WKWebView / WebView2), or drop HTML preview | open |
| 65 | Finder/Explorer parity backlog (UI, any toolkit) | per-OS key map as DATA; rubber band; type-ahead; in-place rename w/ validation; hidden toggle; Shift+Del permanent w/ confirm; Known Folders + volumes + recents in the sidebar; counts/free space; symlink + cloud badges in-list | open |
| 66 | The tracking layer (tracker / kanban / columns / mesh) | a second product surface; ports WITH the browser (the `grid` kind's test) or not at all; never first | open |
| 67 | The §15.1 bug list | fix now in ufb, toolkit-independent | open |

### 15.5 Verify before building
- cxx-qt ≥ 0.8: `QAbstractListModel` support (else the cxx route in #59).
- `notify` on SMB/NFS mounts (mac) and on the WinFsp drive letter (win): what fires; polling cost
  on large NAS folders.
- `trash` crate: macOS Finder route for Put Back; behaviour on network paths (both OSes).
- Server-side SMB copy from Rust (`FSCTL_SRV_COPYCHUNK` / `copyfile(3)` with `COPYFILE_CLONE`);
  what `fs::copy` already does on APFS clones.
- `wry` as a preview surface inside a wgpu window (a child view/HWND again — the §14 lesson:
  drops over it).
- Spacedrive 2.x: read its listing/ops API shape once the beta lands (2026-11-15) as a sanity
  check on #59–62, not as a dependency.

### 15.6 Rulings 2026-11-06 + previews WITHOUT a webview + does the lightbox need wgpu?

**Rulings taken (user, 2026-11-06):**
- Groups 1–3 of §14.5/§15.4 (#48–65): **the proposed defaults stand** ("this all sounds fine").
- **#58 RULED: ufb is the LAST thing done.** Stays Qt until then.
- **#66 OVERTURNED → RULED: the tracking layer (tracker / item list / kanban / columns / mesh) is
  the MOST IMPORTANT part of ufb and STAYS.** It is the priority surface, not an appendix; when ufb
  moves, it moves first-class (the `grid` layout kind's real test).
- **#68 RULED (corrected 2026-11-06): the MOUNT CACHE LAYER goes away; the MESH SYNC STAYS.** "Sync"
  names two unrelated things in ufb. (a) **The agent's VFS / cache layer** — "sync mounts": NFS loopback +
  SQLite chunk cache on mac, WinFsp + sparse cache on win, the shared mount FSM, heal-on-open, conflict
  sidecars, `sync_aware.rs`, the FinderSync hydration badges, the Mount Manager's hydration UI — **a failed
  experiment, ELIMINATED** (~17k agent lines + ~2k `mount.rs` bindings + the Swift appex leave scope).
  (b) **The MESH sync of the tracking DB** — `core/src/mesh/` (axum + reqwest + UDP multicast, leader
  snapshots to the NAS, epoch isolation) replicating jobs / columns / metadata between peers — **STAYS**;
  it is part of the tracking layer ruled first-class in #66. **#71 RULED: GUI-owned PLAIN SMB mounts
  (NetFS / WNet, Keychain / Cred Manager, `local_mounts.rs`) STAY — "part of the big flow".**

**Previews without a webview (#64, revised).** WebEngine exists in ufb because docx and mnd were
converted to HTML as the lowest-common-denominator renderer. In a Rust UI the text stack IS the
rich-text renderer, so the HTML intermediate disappears:
| Kind | Today (ufb) | Port |
|---|---|---|
| `.mnd` | MndbDoc → HTML → WebEngine | **native**: the minNotes page renderer as a crate (`mn-core` render), the same code that draws minNotes' own pages |
| `.docx` | DocxDoc → HTML → WebEngine | minNotes' existing docx IMPORTER → block model → the same renderer (reader-grade fidelity = the importer's fidelity; fine for a preview) |
| PDF / AI | PDFium (`PdfDoc`) | `pdfium-render` → texture (`min-image`, already proposed) |
| text / code | `TextInfo` sniff | parley + the highlighter replacement (syntect / tree-sitter) |
| images, EXR layers | Qt + backends | `min-image` |
| video / audio | the player | `qcv-player` + `min-present`; waveform from the decoder |
| local `.html` | WebEngine | **Blitz** (DioxusLabs: stylo + taffy + parley + vello on wgpu — OUR stack; no JS; 0.3.0-beta.2 2026-08-24, "beta", renders no-JS sites; reader-grade) |
| everything else (Office, Keynote, …) | nothing | **platform fallback**: macOS Quick Look (covers hundreds of types via QL plugins); Windows `IPreviewHandler` is fragile and Office-dependent → "Open" instead |
Net: MORE coverage on macOS than today, equal on Windows, **no Chromium (~150 MB off the bundle),
no `wry`, no webview child view at all.**

**Does the lightbox always need to be wgpu? No — but a native view INSIDE the wgpu window is the
one shape to refuse.** Three shapes:
1. **wgpu overlay in the same window** — everything we render ourselves. Chrome bars, fades, blur,
   clipping, drops, popups: all ours. The default; minNotes' lightbox is 100 % this (all owned kinds).
2. **A native child view in the content rect** (webview, `QLPreviewView`, a Qt item) — QCView's
   three-plane tax returns: nothing wgpu can draw above it, drops/input need their own handling
   (§14), snapshot-and-hide under modals, DPI/zoom handoff. **Refuse.**
3. **A separate native panel** — how Quick Look itself works (`QLPreviewPanel`): OS-owned window,
   no compositing, no child-view tax, keyboard handoff by focus. **The fallback kinds go here.**
Rule: owned kinds → shape 1; fallback kinds → shape 3 (macOS QL panel; Windows: open externally);
shape 2 never. Same rule for any future "embed a web page" idea in minNotes (bookmark cards stay
cards: title + favicon + screenshot, as today).

**Consequence:** a shared **`min-preview`** crate = the lightbox's routing + the owned renderers,
with ufb and minNotes as consumers (QCView is a player, not a previewer). The docx/rtf/html/enex
IMPORTERS move from minNotes' app into `mn-core` so ufb's preview and minNotes' import are one code.

| # | Decision | Proposed default | Status |
|---|---|---|---|
| 64 (rev.) | Previews off Qt | native text stack for mnd/docx/text, `pdfium-render`, `min-image`, **Blitz** for `.html`, macOS Quick Look panel as the fallback, Windows "Open"; NO webview | open (revised 2026-11-06) |
| 66 (rev.) | Tracking layer | **RULED: stays, first-class, the priority surface of ufb** | **RULED 2026-11-06** |
| 68 | The mount CACHE layer vs the mesh sync | **RULED: the agent VFS / cache layer is removed** (NFS/WinFsp loopback, chunk caches, sync_aware, FinderSync badges, hydration UI — a failed experiment); **the MESH sync of the tracking DB stays** (part of #66) | **RULED 2026-11-06 (corrected)** |
| 69 | Lightbox hosting rule | owned kinds = wgpu overlay; fallback kinds = separate native panel; never a native child view in the wgpu window | open |
| 70 | `min-preview` crate + importers into `mn-core` | yes; consumers ufb + minNotes | open |
| 71 | Plain SMB mounts (NetFS / WNet) | stay (GUI-owned, no agent) | **RULED 2026-11-06: stay — "part of the big flow"** |

## 16. Paneling, rails and widgets across the family — what sizes `min-ui` (2026-11-06)

Four read-only inventories of the UI SHELLS (chrome, panels, reusable widgets; not content).
Headline: **no app in the family has docking, floating panels, tear-off or rearranging — and QCView
explicitly dropped "detach" by product direction.** The layout vocabulary is bands + splitters +
collapsible rails + tabs + overlays. The widget union is ~45 kinds, each small; three apps keep
~13–21 reusable widget files and re-implement the rest inline. **The long pole is not chrome; it is
text input** (editing, IME, selection, accessibility) — shared with the editor itself.

### 16.1 The four shells, measured

| | minNotes | ufb | QCView | MinRender |
|---|---|---|---|---|
| QML total | 18.6k (chrome 4.3k; Editor 8.7k incl. 2.4k probes) | 20.7k (shell 3.2k, sidebar 1.4k, browser 4.3k, **tracking 7.6k**, previews 1.6k, dialogs 1.8k) | 20.8k (shell 2.5k, rails 7.0k, bottom bands 8.2k — ColorPanel 2.6k, Timeline 3.2k, LeftRail 3.6k) | 5.7k (panels 4.0k) |
| Reusable widget files | ~13 + ~20 file-local | ~11 + inline (ToolStrip unused) | 21 + ~26 file-local | 16 |
| Regions / panels | 6 fixed regions + ONE panel (Inspector, fixed 248, slides 160 ms, push layout); lightbox / welcome / toast as z-layers | tab strip = title bar; sidebar \| content; dual panes collapse to a 34-px strip; footer 22 px; 4 SplitViews (handle copy-pasted ×4) | 14 bands/rails: 2 collapsible rails (animate to 0, 140 ms, clamped 200–600), 5 bottom bands (mutually exclusive Color/Notes), 2 hand-rolled resize handles (1-px seam, ±3 px hot zone); **no SplitView** | 3 nested stock SplitViews; hand-built tab strip; tray |
| Docking / float / tear-off | none (a dead `floating: false`) | none | none (detach dropped) | none |
| Tabs | none (the path bar's File segment is the switcher; strips removed 2026-10-18) | 3 custom tab strips (main / job subtabs / AE subtabs), StackLayout slots | none (FlatButton segments) | hand-built 2-tab strip |
| Persistence | QSettings: geometry (500 ms debounce, on-screen check), inspector/open, view, swatches… | Settings service: geometry, tab session JSON, 2 splitter widths, per-folder view prefs (view mode, grid size, sort, column widths, subtabs) | QSettings `ui.layout`: rail widths + collapse flags, colour-panel height, scope sections; "always open minimal" | QSettings: panel width/height/visible, column widths |
| Qt style | Fusion (non-mac) + native (mac); dark forced | **FluentWinUI3 on both OSes** (34 raw Dialogs, raw TextField ×33, CheckBox ×12); dark forced | Fusion, palette pinned to tokens | Fusion; dark QPalette |
| Shortcuts | NO `Shortcut{}`; the Editor's ONE `Keys.onPressed` + menu accelerators; `ShortcutCatalog.js` is docs only | ONE `Shortcut{}` (kanban Esc); 25 `Action`s gated `active && visible` (ambiguous-shortcut trap) | C++ `eventFilter` (window_manager.cpp 6893–7300) + 13 `Shortcut`s + menu accelerators | 1 StandardKey |
| Focus / Esc | editor = single key sink; chrome never keeps focus; 7-rung Esc ladder | `forceActiveFocus` ×11; Esc per widget | `requestActivate` reclaim; 3-rung Esc ladder in C++ then QML | ListSelection; dialogs NoAutoClose |
| Popups over content | all in-window (`Popup.Item`), z 60/70 inside Overlay; one `modalDim`; flyouts `modal:true; dim:false` | FluentWinUI3 menus with the cacheBuffer hang fix; dialogs centred by hand ×21 | **`Popup.Window` for menus/tooltips/toast** (must beat the native surface); dialogs as separate `Window`s; modal = snapshot-and-hide | QQC Dialog via Loader; toast inline |
| Theme | 39 colours + 14 dims + fonts/icons; dark only | 29 colours + ~33 dims; dark only; hard-coded greys leak | ~45 colours (incl. side A/B) + metrics; dark only | 70 tokens; 27 names shared with minNotes |
| Icons in use | Phosphor font, 4 weights (count n/a) | Phosphor, ~78 names | Phosphor, ~62 names | Phosphor, 24 names |

### 16.2 The widget UNION (what `min-ui` must provide)

Deduplicated across the four apps; **(n)** = how many apps implement it today; **★** = implemented
more than once in DIFFERENT ways (the design system exists only as copied tokens).

**Primitives:** Icon (Phosphor glyph, 4 weights) (4) · Label (4) · FlatButton w/ variants default /
primary / danger / subtle / raised / neutral, icon+text, checkable, built-in tooltip (4 ★) · chip /
pill: toggle chip, status badge, count chip, colour pill, keycap (4 ★ — ~6 inline recipes in ufb
alone) · segmented control (ufb; FlatButton segments in QCView) · slider (3 ★) · spin box (QCView,
MinRender) · switch (QCView) · checkbox (4 ★ — minNotes copy-pastes a Rectangle ×5) · text field,
single-line, recessed well (4 ★ — minNotes has NONE as a widget; raw TextInput in Rectangles) ·
text area (ufb notes, QCView notes) · combo / picker (ufb raw, QCView FlatComboBox + "reels" = always-
visible lists chosen because popups can't cross the native surface) · search / filter field (3) ·
scrollbar, fixed thickness, AsNeeded (4 ★ all on Templates.ScrollBar for the same reasons) · tooltip
500 ms / 5 s (4 ★) · toast with kinds + action (4 ★, inline ×3) · progress bar 60×4 (3 ★, inline
recipes) · spinner (QCView MiniSpinner, BusyIndicator elsewhere) · status dot (3) · section header
(3) · colour swatch (3) · **colour picker HSV square + hue + hex** (minNotes 166 lines, QCView 487 —
two implementations of one widget ★) · calendar / date picker (ufb, on MonthGrid) · key–value row +
chip row (QCView) · card (QCView InspectorCard) · collapsible section / accordion (QCView) · resizable
column header with drag guide (ufb ×6 inline copies, MinRender ResizableHeaderLabel ★) · splitter
handle, 1-px seam, hot zone, accent hover (3 ★) · tab pill (ufb ×3 sites, MinRender inline ★) ·
breadcrumb / path bar (minNotes, ufb) · list row (zebra, hover, selection) (4) · tree row (indent +
caret) (minNotes Layers, ufb tree) · **typed-cell grid**: text / number / checkbox / dropdown /
priority / date / notes / links (ufb CellEditor 800 lines; MinRender's chunk tables) · kanban lane +
card (minNotes BlockKanban, ufb KanbanBoard ★) · timecode well + go-to popup (QCView) · scrub bar /
ruler / clips / markers (QCView Timeline + CompactStrip ★; minNotes transports) · transport button
row (QCView, minNotes ★).

**Containers / window-level:** title row with traffic-light inset 70 + drag reserve 120 + 6-px drag
threshold (4 — the ONE truly shared pattern) · tab strip as title (ufb) · native menubar on macOS vs
in-window hamburger menu elsewhere (4) · rail: collapsible, animated, clamped, persisted (QCView,
minNotes) · sidebar (ufb) · splitter row / column with persisted sizes + collapse-to-strip (ufb,
MinRender, QCView hand-rolled) · footer / status strip 22 px (ufb, QCView) · tool strip 32 px (4) ·
modal scrim + centred dialog (4 ★) · flyout ("press outside closes AND goes no further") (minNotes) ·
context menu (grammar: sections, hide-don't-grey, red last; rows / sep / header) (4 ★) · app menu w/
submenus built dynamically (ufb Instantiators) · lightbox overlay (minNotes, ufb ★) · welcome /
empty state (minNotes, MinRender) · tray icon + native menu (ufb, MinRender) · scope faces (QCView).

**Count: ~45 widget kinds + ~15 container patterns.** None is exotic; each QML version is 25–250
lines. The three genuinely hard ones: **text input** (editing, selection, IME, dictation, press-and-
hold accents, accessibility — one implementation serves chrome AND the block editor), **the typed-
cell grid** (ufb's tracking layer = the priority surface per #66), **the timeline/scrub family**
(QCView-only; last — and per §17 a content SURFACE with its own model, not a widget).

### 16.3 Layout behaviours actually needed (vs §8.1's four kinds)

Everything observed maps onto the four kinds: fixed bands and rails = `row`/`column` with fixed or
clamped children; splitters = a `row`/`column` whose handle children carry persisted sizes, min/max,
and a collapse-to-strip rule; collapsible rails = animated width 0↔W with push layout; tabs = a slot
switch; overlays (popups, lightbox, scrim, toast, welcome) = `free` children of the window node with
z; lists/grids = `flow`/`grid`. **No docking kind is needed.** Mode gating (fullscreen / compact /
live / chromeless hides SETS of bands) is a visibility predicate per node, as today. Persistence of
layout = the same per-node data serialised (geometry, sizes, collapse flags, tab session, per-folder
view prefs) — one mechanism instead of four `Settings` dialects.

### 16.4 Input plumbing: what none of the four has, and `min-ui` core must

- **One shortcut registry with contexts** (the active surface / focused subtree), replacing: no
  registry (minNotes), `active && visible` gating (ufb), a 400-line C++ event filter (QCView). Hold-
  keys (A/J, D/L) and auto-repeat suppression are registry features, not app code.
- **One focus model**: chrome never keeps focus (minNotes' rule, generalised); text fields are the
  only focus sinks besides the content surface; focus returns to the surface on popup close.
- **Esc as a cancel STACK**: every cancelable (stroke, drag, selection, search, mode, popup, fullscreen)
  pushes itself; Esc pops. The two hand-written ladders become data.
- **Popups are nodes** in the one scene (there is no native surface to beat — QCView's `Popup.Window`
  workaround, the snapshot-and-hide modal, the "reels instead of combos", the in-scene-vs-OS-window
  split all disappear). Keep ONE escape hatch: a popup flagged `may_leave_window` becomes an OS
  window (menus near a screen edge) — deferred until needed.
- **Hover/press ramps** 100 ms from an alpha-0 twin of the hover colour (never "transparent"), enter-
  fade-only popups 80 ms, tooltips 500/5000, 6-px drag thresholds, fixed-thickness scrollbars,
  1-px hairline handles with ±3-px hot zones, radius 3/6/10, Geist 13/14, Phosphor — **the chrome
  contracts** (§8.11), now with the measurements behind them.

### 16.5 Build vs borrow (ecosystem 2026-11)

- **Masonry** (Linebender, the widget layer under Xilem 0.4, alpha, Oct 2026) sits on parley + vello
  — OUR stack — with a growing widget set (grid, slider, passthrough, resize observer) and AccessKit.
  Reference and parts donor (its text-input widget, its AccessKit wiring), not a dependency: alpha,
  breaking changes, and its retained-tree model is not §8's computed-layout tree.
- **GPUI** (Zed): macOS-focused, no built-in text input. **egui_dock / egui_tiles**: docking we don't
  need, on a renderer we don't use. **iced**: slow; `pane_grid` only.
- **Taffy** (flexbox/grid solver, used by Blitz and Masonry) could BE the `row`/`grid` kinds' solver
  — verify it fits "computed layout, no measure-back" (text measure IS a measure-back; §8.3 must
  state the one allowed exception).
- **Verdict**: write the ~45 widgets in-house (they are small and the family's look is bespoke
  anyway); borrow the text stack (parley `PlainEditor` + IME via winit; verify maturity), AccessKit,
  and possibly Taffy. Rough size: `min-ui` core (tree, layout, router, focus, shortcuts, popups,
  theme) + widgets + chrome containers ≈ **15–20k lines of Rust** — about one app's QML — with text
  input the long pole at any size.

### 16.6 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 72 | Docking | **none** — bands + splitters + collapsible rails + tabs + overlays (the family has none; detach was dropped) | open |
| 73 | Widget set | the §16.2 union (~45 kinds + ~15 containers), written in-house on parley/vello; Masonry = reference + parts donor | open |
| 74 | Input core | one shortcut registry w/ contexts; one focus model (chrome never keeps focus); Esc = cancel stack | open |
| 75 | Popups | nodes in the one scene; `may_leave_window` → OS window as a deferred escape hatch | open |
| 76 | Theme | dark-only v1; tokens as DATA (`min-theme`, §10.3) — the FIRST real shared design system (today: 3 Qt styles, 4 token files, 27–35 shared names) | open |
| 77 | Chrome contracts | the §16.4 measurements carried verbatim (ramps, fades, thresholds, hairlines, radii, type, icons, title-row geometry) | open |
| 78 | The typed-cell grid | ufb's CellEditor kinds (text / number / checkbox / dropdown / priority / date / notes / links) = the `grid` kind's spec; add `user` later | open |
| 79 | Text input | parley editor + winit IME + AccessKit from day one; ONE implementation for chrome fields and the block editor | open — **the long pole** |
| 80 | Layout solver | Taffy for `row`/`grid` if it fits computed layout with text-measure as the single measure-back exception; else own | open — verify |
| 81 | Icons | keep the Phosphor set; render as glyphs via parley OR as SVG via `vello_svg` (one choice; SVG avoids icon-font hinting at 125 %) | open — verify |
| 82 | The duplicate widgets NOW (two colour pickers, 6 resize handles, 4 splitter handles, 3 tab strips, 5 checkboxes) | leave them in Qt; port ONCE | open |

### 16.7 Verify before building
- parley: editing / selection / IME / press-and-hold status in 2026 (`parley::editor` or `PlainEditor`),
  and winit's IME surface (`Ime::Preedit/Commit`, NSTextInputClient coverage, dictation).
- AccessKit via winit (`accesskit_winit`) + what Masonry wires; VoiceOver / Narrator on a wgpu tree.
- Taffy: does a text node's intrinsic size count as measure-back in §8.3's sense; performance on a
  10k-row virtualised list (or lists bypass the solver).
- `vello_svg` rendering the Phosphor SVGs at 12–24 px vs glyph rendering through parley.
- Masonry's widget list as of its next release (grid, slider, text input) for parts to lift.

## 17. The timeline: clips, edits, scrubbing — read hard (2026-11-06)

One deep read-only survey of QCView's timeline (`src/timeline/` ≈ 3.5k C++, `TimelinePanel.qml`
3,193, the scrub / shuttle / thumbnail / dual paths, and ~10k lines of orchestration inside
`window_manager.cpp`) plus minNotes' two transports. **Verdict: the timeline is a second CONTENT
SURFACE with its own model, edit ops, undo, camera and playback mapping — not a widget.** §16's
"timeline/scrub family" line is upgraded accordingly. It is QCView-only and last, but its MODEL
belongs in the shared player crate from day one because minNotes already carries copies of its
scrub and shuttle constants.

### 17.1 The model as it is
- **OTIO-shaped structs, OTIO not linked** (`timeline_types.h`): `Clip{startTime, duration,
  sourceIn, sourceOut — all DOUBLE SECONDS; isGap; mediaKind Video/ImageSequence/Audio; cached
  sourceFps/W/H/duration; sequence fields; audioMuted}`, `Track{clips, locked, muted, zIndex}`,
  `Timeline{mode SingleMedia|Playlist (Dual is NOT a mode), policy{ripple, interClipGap,
  allowOverlaps}, frameRate (24 default), tracks}`. Per-clip PAR/rotation/range/OCIO live on
  `MediaItem`, pushed to the renderer on clip change.
- **Time = double seconds everywhere; frames derived at each boundary with `lround(sec*fps)`.**
  Commit a9c8bb78 "de-globalised" fps: the timeline speaks seconds, fps is per decoder. Timeline fps
  by mode: decoder fps (single), sequence fps (image seq, retunable), `masterFps` (playlist; the UI
  HARD-CODES 24 fps and gap 0), **max(fpsA, fpsB)** (dual; master frame count = round(maxDur ×
  masterFps)). Mixed-fps playlists hide the frame/SMPTE wells and show seconds.
- **One in-memory `Timeline` in `TimelineController`** with a `PlaybackTimer` and ONE `QUndoStack`.
  QML reads QVariantMaps rebuilt on every `timelineChanged`; `clipToMap` DROPS the sequence fields
  and `audioMuted`, so `replaceTrackClips` round-trips lossily.
- **Persistence** (`.qcvproj` v2): a playlist is `PlaylistData{masterFps, canvasW/H, defaultGapFrames,
  loop, items[{mediaId, range{in,out} seconds, −1 = full}]}` — gaps and `startTime` are re-derived
  on load; edits are written back on EVERY `timelineChanged` incl. preview ticks (no-op when equal).
  Dual is `DualPairData{mediaIdA/B, masterFps, masterDuration, clipsA/B (maps), in/out as MASTER
  FRAMES}`.
- **Undo**: one command per gesture; live drags mutate via `preview*` with no entry; on release
  QML restores the original map and pushes the real command (snapshot before/after per clip).
  `clearUndoStack()` has NO callers — the stack survives every load. `canUndo()` doubles as "user
  has edited".

### 17.2 Edit operations as they are
| Op | Gesture | Rule | Gaps |
|---|---|---|---|
| Trim head / tail | 8-px handle zones, 5-px drag threshold | right/left edge anchored; min length 1/fps; `sourceIn ≥ 0` | **no upper clamp to `sourceDuration`** (trims past the source end) |
| Slip | Ctrl + body | `sourceIn/Out` shift together | no upper clamp |
| Slide (dual) | body | `startTime` only, ≥ 0 | **no overlap check** despite `allowOverlaps=false`; `locked` never enforced |
| Reorder (playlist) | body | slot = cursor time vs neighbour midpoints; live ripple via `previewSlide` on every clip; `MoveClipEdit` on release | — |
| Split | Ctrl+K on every editing track at the playhead | first half in place, second half new UUID | — |
| Delete | Del/Backspace on the selected clip | playlist ripples (`recomputeContiguousStartTimes` after every op); dual leaves a hole | — |
| Insert (drop-to-add) | playlist DropArea | 12-px snap to t=0 / clip edges / track end (the ONLY snapping); 5-s placeholder duration patched when metadata arrives | — |
| In / Out | I / O, buttons | **int frames of the active clock, NOT in the model**; auto-loop; disabled in playlist | — |
**Not frame-snapped**: trim/slide/slip commit raw `Δpx/pps` seconds (only the preview frame is
rounded). **Not implemented**: multi-select, roll, overwrite, duplicate, user markers. **Visual
grammar** (carry): amber ghost of the original rect + a floating badge ("head/tail/start/slip <tc>"
or "→ slot n/N"); accent reorder ghost + yellow insertion bar; dashed 5-s drop ghost; handle hover
amber; cursors SizeHor / OpenHand / ClosedHand; the playhead dims to 0.35 and dashes over an editing
track; **live viewport feedback during trim/slip/slide** (`beginEditScrub` → side-local scrub frame
without moving the master clock; compositor caches held while scrubbing, 953629d4).

### 17.3 Drawing + camera as they are
All QML Items: two `Repeater`s over the clip maps (**wholesale delegate rebuild on every preview
tick**, no virtualisation; gesture catchers must live at panel level because delegates die under
the pointer), ruler ticks for the WHOLE duration (not clamped to the view), a `QQuickPaintedItem`
waveform (single-clip rows only; min/max per pixel column from a probe engine on a power-of-two
stride ladder, base 5 ms, sample-exact ≤ 60 s, probes pause during playback), in/out flags, note
pennants, an image-seq cache band, overview bar (drag middle = pan, edge within 6 px = zoom with
the other edge anchored). **Camera**: `pps` (px/s; −1 = fit) + `scrollX`; wheel zooms around the
cursor (`1.0015^dy`), Alt+wheel pans (Windows horizontal fold), Ctrl+=/− = one notch about the
playhead, no pinch; min = fit, max = max(200 pps, fit×50); window resize keeps the time range;
page-turn follow at a 20-px margin. Rows 34 px, ruler 20, overview 14. **This is the canvas camera
(§7.2) with one axis.**

### 17.4 Scrub + seek mechanics as they are
- **Pointer → time is continuous seconds** (`(x+scrollX)/pps`); each decoder rounds. Click and drag
  are one press/move/release machine; **every move issues a request, no QML throttle; decoders
  coalesce latest-wins** (`ScrubDecoder::requestFrame`, atomic pending target; GOP LRU 512 MB default;
  3-way decision hit / forward-no-reseek ≤ 90 frames / keyframe seek; intra codecs bypass the
  cache — all already in §4.2).
- **Per mode**: single video → scrub decoder, release = `seekToTime` + resume; **playlist → clip
  resolve per move, a DIFFERENT clip = synchronous decoder re-open (the ~50–100 ms stall)**, release
  through a 50-ms debounce (fixed a teardown-cascade crash); dual → `beginScrub` parks the streaming
  decoders, per-side scrub decoders, `endScrub` warm-seeks both; image seq / audio → timer-driven.
- **Playhead follows the cursor, not the decode**, with an optimistic `timer.seek` on release.
- **Scrub audio** = the grain engine (§4.2): velocity EMA τ 40 ms clamped ±32×, pitch follows speed
  only below 1× (cap 1.0 after the squeal fix 6da94518), 80-ms grains, ~40-ms ring, direction flip
  flushes. **minNotes carries the SAME constants, re-typed in QML** (`Editor.qml:1368-1390`).
- **Shuttle** (A/J, D/L hold, auto-repeat ignored, in the C++ event filter): 2× doubling per second
  to 32×, 33-ms ticks, "timer leads, decoder chases"; playlist shuttles the timer only and swaps the
  clip on release. **minNotes re-types this ramp in a QML Timer too** (`Editor.qml:1438-1452`).
- **Hover thumbnail**: 16-ms throttle while not playing/pressed → `(path, source frame)` per side →
  non-blocking cache (3 workers → 1 while playing; LRU 100 × 320×180 ≈ 23 MB; nearest-frame
  fallback, exact frame replaces it on a 33-ms drain) → **composited by the RENDERER into the raw
  frame BEFORE OCIO** (A bottom-left, B bottom-right, 0.18 of the frame). Thumbnail-pool deletions
  once never drained → VRAM freeze (c8655ee1).
- **Loop**: timer range for single/image seq, a frame-watch wrap for single video, the dual timer
  range for dual, app-level wrap for playlists (timer looping forced off). **Timecode**: correct
  drop-frame maths in `timecode_formatter`; dual always non-drop at the master rate; go-to popup
  pads digits Resolve-style.
- **Playlist playback has ONE video decoder: every clip boundary = synchronous re-open; no gapless
  path; only image sequences prewarm the next clip (5-s lead).** Boundary detection = the decoder →
  timer mirror (`sourceTime ≥ sourceOut − 0.5/fps`), gated by the seek debounce + a seek serial
  (66d4a4d8). Clip containment is half-open (next clip wins at the boundary); prev/next uses `ceil`.
- **Dual**: one master playhead mirrored into the controller timer; `translateMasterToSourceFrame`
  per side (gap → −1 → transparent); no offset field (slide/slip ARE the offset); seek-while-playing
  holds the clock until both sides decoded (2.5-s deadline, edae9809); pump/render threads read an
  immutable `shared_ptr<const Timeline>` snapshot (TSan race 45f10658); B hot-swap rebuilds the whole
  island and loses track edits (bde75934); `dualSeekable` false only when BOTH sides are live.

### 17.5 What this means for the port
1. **Time base.** Double seconds + per-boundary rounding works until it doesn't (prev/next needing
   `ceil`, in/out living as frames outside the model, mixed fps hiding the wells, dual mixing fps
   spaces until a9c8bb78). The shared model should use **rational time — `{value: i64, rate:
   Rational}` as OTIO does** — with positions at the timeline rate and source trims at the clip's
   source rate; seconds for display only. Edits snap to frames by construction.
2. **`qcv-timeline` is pure logic and ports first**: types, the eight ops as **ops with inverse**
   (= §8.2's op-based undo; no snapshot commands, no "restore the map then push"), the flattener,
   snap rules, playback mapping, `EditPolicy` ENFORCED (locked, overlaps, source bounds), undo
   cleared on load. The ~10k lines of orchestration in `window_manager.cpp` (seek debounce, mirror
   gates, boundary detection, loop wraps per mode, fast-seek integration) become ONE `Sequencer`
   in `qcv-player` that owns the clock, the clip→decoder dispatch and the mode table — the thing
   §8.5 called "one Transport, one media clock", now with its spec.
3. **Gapless playlists**: two decoder slots (ping-pong) + next-clip prewarm for VIDEO, not only
   image sequences; the boundary stall and the frame-1 flash both disappear. This is the biggest
   user-visible win the port offers QCView's playlists, and it is a player-crate feature.
4. **The timeline surface in `min-ui`** = one custom node with the **canvas camera** (one axis:
   pps/scrollX ≡ zoom/pan; fit, zoom-about-cursor, follow, overview) drawing straight from the
   model each frame (clips as rects + text, ruler ticks for the VISIBLE range only, waveform from
   the probe memo as a path/texture, markers, playhead, ghosts and badges as overlay layers). No
   per-clip nodes, no rebuild-per-tick, gestures owned by the surface — the three Qt gotchas (press
   grab lost on rebuild, DropArea z-war, WheelHandler not firing) all vanish.
5. **The hover thumbnail becomes a UI node over the viewport**, not a renderer composite pass; the
   presenter stops compositing UI (which also moves it out of the pre-OCIO path — thumbnails
   should be display-referred anyway). The cache is `min-image`'s thumbnail service (ufb's
   thumbnails are the first consumer, §10.4), keyed by (path, frame).
6. **Scrub/shuttle constants live in `Transport` ONCE**: the EMA, pitch cap, grain sizes, the 2×→32×
   ramp, the 33-ms tick, the 50-ms seek debounce, the 16-ms hover throttle. minNotes' two QML
   re-typings retire; CompactTransport (note markers, frame-based scrub) and VideoTransport become
   the same surface node with fewer layers.
7. **Feature set v1 = today's** (trim head/tail, slip, slide, reorder-with-ripple, split, delete,
   insert-with-snap, in/out) **+ frame snapping + source-bound clamps + in/out IN the model**.
   Not v1: multi-select, roll, overwrite, user markers, per-clip transforms in the timeline.

### 17.6 QCView issues found (fix in C++ regardless of any port)
Undo stack never cleared across loads; trim-tail/slip have no upper clamp to `sourceDuration`;
edits commit un-snapped seconds; `clipToMap` drops sequence fields + `audioMuted` (lossy restore);
`locked` unenforced; dual allows overlaps against its own policy; playlist fps/gap hard-coded 24/0
in the UI; `precacheRange` has no callers; the undo stack is used as an "edited" flag.

### 17.7 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 83 | Time base in the shared timeline model | rational time (value i64 + rate), own type; seconds display-only; edits frame-snapped by construction | open — **OTIO itself RULED OUT 2026-11-06; evidence in (S2): the cut and the 3:2 cadence fell out of it** |
| 84 | `qcv-timeline` in the player crate | types + 8 ops with inverses (op-based undo) + flattener + snap + mapping; policy enforced; undo cleared on load; ONE `Sequencer` replaces the WindowManager orchestration | open |
| 85 | Gapless playlists | two decoder slots + video next-clip prewarm in `qcv-player` | open — **evidence in (S2): gapless on ProRes and mid-GOP H.264, 0 black / 0 dup** |
| 86 | Timeline rendering | one surface node on the canvas camera (1-axis), draws from the model per frame, visible-range only; no per-clip nodes | open |
| 87 | Hover thumbnail | a UI node over the viewport (display-referred), fed by `min-image`'s thumbnail cache; the presenter composites no UI | open |
| 88 | Scrub / shuttle / debounce constants | in `Transport` once; minNotes' QML copies retire | open |
| 89 | Edit feature set v1 | today's ops + frame snapping + source clamps + in/out in the model; no multi-select / roll / overwrite / markers | open |
| 90 | §17.6 bug list | fix now in QCView | open |

### 17.8 Verify
- ~~OTIO Rust bindings vs own `RationalTime`~~ **RULED 2026-11-06: no OTIO — "overkill for what we
  have; stick with the current flow"** (own structs, own time type; no `.otio` import/export). #83's
  rational time stays a PROPOSED default independent of OTIO.
- Two-decoder gapless on Windows DX12 with d3d12va: two hw decoder contexts resident at once (VRAM,
  session limits) for 4K ProRes/HEVC pairs.
- **From S2 (2026-11-06):** the scrub decoder is SOFTWARE on Apple with SLICE threads for intra codecs and FRAME threads for
  inter codecs (frame threading after a flush costs a pipeline of packets before the first frame — 64 ms for one 4K ProRes
  frame; slice-threaded 5.6 ms); a streaming decoder's first frame after a flush is a pipeline fill (VideoToolbox 65 ms ProRes /
  280 ms H.264 / nothing within a sweep for inter after a flush; FFmpeg frame threads 140 ms EXR), so the Sequencer answers
  seeks from the scrub decoder and the stream resumes; latest-wins must RETARGET forward within reach instead of abandoning
  (QCView's literal rule starved at 60 Hz on 4K H.264); every frame of a GOP fill enters the LRU (backward sweeps hit).
- The waveform probe engine's memo as a shared `qcv-player` service (minNotes' audio blocks could
  show waveforms too).

## 18. OCIO inside `min-present` (2026-11-06)

No new repo survey: §16's QCView read already captured the plumbing. Facts + the design choice.

### 18.1 What QCView does today (from `src/color/`, `src/render/*/`)
- `OcioChainBuilder` builds a processor, then `GpuShaderDesc` with `setLanguage(lang)` →
  `getShaderText()`; **`buildSplit` cuts the chain into PRE and POST processors around the
  CPU/GPU "linear stage"** (the Highlight Knee, `linear_stage.h` 450 lines) so the knee runs in
  scene-linear between two OCIO halves.
- **Metal**: `Language::Msl_2_0`; the MSL is wrapped in a COMPUTE kernel, compiled with
  `newLibraryWithSource` at runtime, dispatched compositeRaw → compositeCorrected; 1D/2D/3D LUTs
  from `getTexture*`/`get3DTexture*` become `MTLTexture`s (`metal_ocio_renderer.mm` 739 +
  `metal_ocio_luts.mm` 157).
- **D3D11**: `Language::Hlsl_Sm_5_0`; `D3DCompile` (seconds → compiled ASYNC) as a full-screen
  PIXEL shader; LUTs as D3D11 textures (`d3d11_ocio_renderer.cpp` 1072 + luts 256).
- **QRhi legacy**: GLSL 4.0 through QShaderBaker (non-native build only).
- **minColor** is OCIO-free: uniform-driven, vendored OpenDRT/AgX with hand-written MSL/HLSL shims.
- **CPU LUT baking exists for EXPORT only** (`ocio_lut_baker`, 65³ `.cube`; a sample function when
  the Knee is in the chain). No baked-LUT display fallback.
- **Scopes** run an extra "OCIOScope" processor (source → interchange space) in their compute pass
  (§16 QCView §4): a second GPU OCIO consumer.
- Config/UI: engine switch OCIO | minColor; bundled configs (ACES 1.3 / 2.0, Blender 4.x/5.x);
  Input / Look / Scene LUT (.cube/.cc/.ccc/.cdl) / Knee / Display / View / Display LUT / AgX;
  exposure, gamma, channel view, HDR display mode; Export LUT. **Drift risk already banked:**
  the vendored minColor core is a pinned copy (§10.1).

### 18.2 The platform facts (2026-11)
- **OCIO has no WGSL target.** `GpuShaderDesc` languages: GLSL 1.2/1.3/4.0/ES, Cg, HLSL SM 5.0,
  MSL 2.0, OSL. (2.5 is current; an MSL 2.0 1D-LUT channel bug was reported against 2.5.0.)
- **wgpu passes native shaders through.** v25 added `create_shader_module_passthrough` +
  `ShaderModuleDescriptorPassthrough`; Metal MSL passthrough landed (PR #7326); DX12 takes
  per-entry-point HLSL compiled to DXIL (one entry point per artifact as of wgpu-core 30); v27
  unified it under an EXPERIMENTAL feature that needs an `unsafe` opt-in (`PASSTHROUGH_SHADERS` /
  `EXPERIMENTAL_PRECOMPILED_SHADERS` — the name has moved; pin and verify).
- **naga's GLSL front-end is not a route**: "GLSL 440+, Vulkan semantics only", WGSL output is
  "secondary support", and **combined image/samplers are unsupported** — OCIO's GLSL uses
  `sampler1D/2D/3D` uniforms throughout. Dead end without rewriting OCIO's output.
- A string-level OCIO-GLSL → WGSL transpiler of our own is possible (the generated code is
  formulaic) and a maintenance liability against every OCIO release. Not proposed.

### 18.3 The choice
| Route | Exactness | Backends | Cost | Where |
|---|---|---|---|---|
| **A. Passthrough** — OCIO → MSL 2.0 on Metal, HLSL SM5 → DXIL (dxc) on DX12, loaded via wgpu passthrough; the same compute-kernel / full-screen wrapping QCView ships today | exact (today's) | per backend (2 wrappers, as today) | ports QCView's existing wrapper code; `unsafe` experimental gate; LUT textures via wgpu as usual; DXIL compile stays async | `min-present`, an **opaque native pass** slot |
| **B. Baked LUT** — CPU processor → 1D shaper + 3D LUT (65³ half-float) at config/preset change; ONE WGSL stage | approximate for some ops (OCIO's own caveat; fine for SDR review, questionable for HDR PQ precision + the Knee split) | all, one shader | ~tens of ms rebake per change; the export baker already exists | `min-present`, a WGSL stage |
| C. naga GLSL-in | — | — | blocked (combined samplers) | — |
| D. own transpiler | exact until OCIO changes | all | permanent liability | — |

**Proposal: A is the display path (it is the status quo expressed in wgpu terms; QCView is last and
the only OCIO consumer), B is the fallback + the export path + the "no native pass available"
path (and the one minNotes/ufb/MinRender would ever use, if they ever showed OCIO at all — they
don't).** The §11.2 presenter graph gains **one opaque-native-pass slot** between linearise and
present, with the Knee split preserved: `[decode/unpack] → [OCIO pre | native] → [linear stage:
knee, WGSL] → [OCIO post | native] → [present: viewer aids, HDR output, WGSL]`. minColor stays a
WGSL stage (its shim, §10.3), so the engine switch is a graph re-plumb, not a backend switch.
The scopes' OCIOScope conversion reuses the same native-pass mechanism in their compute pass.
Ruling #45's Windows backend question doesn't change: DX12 + d3d12va + HLSL/DXIL passthrough is
the pair; Vulkan would be SPIR-V passthrough (OCIO GLSL → glslang → SPIR-V), also available.

### 18.4 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 91 | OCIO display path | **A: native passthrough** (MSL / DXIL) inside `min-present`'s opaque-native-pass slot; QCView's wrapper code ported | open — **evidence in (S1 step 2, 2026-11-06): exact on Metal, 0.3–0.5 ms at 4K; DXIL owed with Windows** |
| 92 | OCIO fallback + export | **B: baked shaper + 3D LUT** as a WGSL stage; also the export baker | open — **evidence in (S1 step 2): 65³ = 8-bit-exact, NOT 10-bit-exact (worst 1.7/255) — a fallback, never the display path for 10-bit review** |
| 93 | Presenter graph shape | 5 stages with the Knee linear stage between two OCIO halves; minColor = WGSL stage; engine switch = graph re-plumb | open (refines §11.2) |
| 94 | OCIO stays C++ via FFI | yes (restated from §10.3): `ocio-sys`-style bindings owned by us; configs bundled as today | open |

### 18.5 Verify
- ~~The exact wgpu feature + descriptor names at the pinned version~~ **CLOSED by S1 step 2 (2026-11-06):** wgpu 30 =
  `Features::PASSTHROUGH_SHADERS` + `Device::create_shader_module_passthrough(ShaderModuleDescriptorPassthrough { msl | hlsl |
  dxil | spirv, entry_points: [{ name, workgroup_size }] })`; the gate is an `unsafe fn`, not a cargo feature — fine for shipping.
- ~~Metal: can the passthrough MSL be a compute kernel bound to wgpu-created textures~~ **CLOSED, YES (S1 step 2):** wgpu's Metal
  backend numbers buffers / textures / samplers sequentially per stage in bind-group-entry order, so QCView's wrapper
  (`src texture(0), dst texture(1), src_smp sampler(0), LUTs texture(2..)`) is reproduced by listing the entries in that order;
  no reflection → explicit pipeline layout, no binding validation. 1D LUTs arrive as `texture2d` 4096×2 R, 3D as `texture3d`;
  exact vs the CPU processor on ACES 1.3 / Blender AgX / Blender Filmic. Route B measured: 8-bit-exact, not 10-bit-exact.
- DX12: dxc availability on the box (ship `dxcompiler.dll`), DXIL signing, one entry point per
  module, async compile pipeline (QCView's "seconds" compile).
- **NEW (S1 step 3, 2026-11-06): wgpu-hal 30.0.1's Metal surface crashes on `Bt2100Pq` / `Bt2100Hlg` / `ExtendedDisplayP3`**
  (`lookup()` in `metal/surface.rs` returns the symbol's address as `&'static CFString`; SIGBUS in `CGColorSpaceCreateWithName`).
  One-line fix upstream; until then the presenter sets the CAMetalLayer colour space itself by name after `configure` (the
  kCGColorSpace* constants' values are their own names). The §11/§12 "wgpu sets EDR itself" fact stands for the extended-linear
  surface, which is the one that works unpatched.
- ~~OCIO 2.5's MSL 1D-LUT channel bug (issue #2272) — does QCView's current pin hit it.~~ **Not on this pin (2.5.0, S1 step 2):**
  Filmic's red-only 4096-entry 1D LUT through the native path matched the CPU processor at every sample.
- Route B's error vs route A on the ACES 2.0 display transforms at PQ 1000 nits (a measured
  comparison before calling B a "fallback" for HDR).

## 19. Scopes + title-safe guides: viewport overlays in the port (2026-11-06)

From the QCView read (§16 report §3–4); no new survey.

### 19.1 Scopes today
- **A vectorscope and a luma waveform. No histogram, no RGB parade** (`scope_math.h` mentions them;
  nothing implements them). QML 455 lines (faces), `scope_controller` 1,021, maths 362, Metal 605,
  D3D11 746.
- **GPU, in the frame's own command buffer**: a `scope_accum` COMPUTE kernel samples the source at
  ≤ 960 px wide with ATOMIC adds into count grids (waveform adds a full-res peak pass); `scope_draw`
  writes a 512² RGBA8 image into one of THREE shared readback buffers; a completion handler
  publishes it (allowed to lag, never blocks); **D3D11 uses a tap pixel shader instead** (two
  implementations again). The image reaches QML as `image://qcvscope/<serial>`; **the graticule,
  targets, gamut hexagons, skin line and level lines are QML `Canvas` items** over it.
- **Tap point = the SOURCE texture(s), before the display/view transform**, with an optional
  "OCIOScope" conversion to the interchange space (tiers Input / Assumed / Signal).
- Controls: vectorscope zoom / colorize / brightness; waveform scale Auto / %Y′ / nits, peak 300–4k,
  Frame/Max readouts; dual overlays A cyan + B orange. **Work happens only while the rail section
  is expanded** (bindings on `scope.active`).

### 19.2 Guides today
- **No parametric title-safe / action-safe guide exists.** Guides are **13 SVG files** in
  `assets/safety/`: `16x9.svg` = two unlabeled rects at 90 % and 93 % of 1920×1080 (de facto
  title/action safe), the other 12 = platform templates (YouTube, Meta, TikTok, Pinterest,
  Snapchat, Samsung). `SafetyOverlay` (338 lines) + `SvgPathParser` (254) tessellate them on the CPU
  into thick-line triangle meshes drawn by the ANNOTATION renderer pipeline, aspect-fit to the
  image rect incl. anamorphic un-squeeze (`meshForImage`); opacity / width / colour from the
  LeftRail; the picker lists the folder. **Guides can be sent to the OUTPUTS** (Output ▸ "Guides"
  chip) — so they are an output-composited overlay, not UI.
- Also absent on the viewport: zoom/pan + a zoom % readout, centre cross, thirds grid, aspect
  mattes, pixel probe / eyedropper, timecode burn-in. (Noted; none requested.)

### 19.3 The port
**Two overlay classes, one rule.** Everything drawn over the video today is drawn by the renderer
because QML could not cross the native surface. In the one-scene world that reason is gone, so the
split becomes a PRODUCT one:
- **OUTPUT overlays** — things that must appear on SDI/NDI/Transmit and in exports: **guides,
  annotations (ink), and any future burn-in**. They render (via vello) into an RGBA overlay texture
  that `min-present`'s present stage composites into the output frame; the UI shows the same frame.
  One vello scene is the single source for both.
- **UI overlays** — things only the operator sees: **scopes, hover thumbnail (§17), drop
  highlight, A/B badges, loading spinner, seam highlight**. Plain `min-ui` nodes over the viewport
  node; the presenter never composites them. (Today's `setDropHighlight`, `setLoadingActive`,
  `setHoverThumbnail`, `setSplitSeamHighlight` renderer entry points all retire.)

**Scopes** = one WGSL compute path in `min-present` for all backends (storage-buffer atomics are
core wgpu on Metal / DX12 / Vulkan): a TAP stage reads either the source/interchange texture
(today's tiers; the OCIOScope conversion uses §18's native-pass slot) or, newly, the display-
referred frame; `accum` scatters into count grids (vectorscope UV grid, waveform column×level,
and — same kernel, different binning — **histogram and RGB parade for free**); `draw` renders the
scope image into a texture **that the UI node samples directly: no readback buffers, no completion
handler, no serial image provider, no lag**. The faces (graticule, targets, gamut hexagons, skin
line, nits/%Y′ rulings, A/B tints) are vello vector overlays in the same node — resolution-
independent, themed from tokens. Activity follows node visibility, generalised: **an offscreen
node schedules no GPU work** (`min-ui` rule; §16.4).

**Guides** = a vello vector layer in image-rect coordinates (aspect-fit + anamorphic un-squeeze is
a 2-D transform, not a mesh rebuild): a **parametric generator** — action safe / title safe at
configurable percentages (defaults 93 / 90 for 16:9; 95 / 90 legacy 4:3), centre cross, thirds
grid, aspect mattes (2.39, 2.0, 1.85, 4:3, 1:1, 9:16, 4:5 …) with shaded letterbox, custom
rectangles — **plus the SVG templates via `vello_svg`** (the 12 platform files keep working; drop
a new SVG in the folder as today). `SafetyOverlay`, `SvgPathParser` and the CPU tessellator
retire; stroke width is in output pixels, as now. The layer is an OUTPUT overlay: rendered once
into the overlay texture, shared with ink.

### 19.4 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 95 | Overlay classes | OUTPUT overlays (guides, ink, burn-in) = a vello scene → texture composited by the presenter; UI overlays (scopes, hover thumb, drop highlight, badges, spinner, seam) = `min-ui` nodes; the renderer composites no UI | open |
| 96 | Scopes | one WGSL compute path (tap → accum w/ atomics → draw to texture) for all backends; UI samples the texture directly (no readback); faces as vello overlays from tokens | open |
| 97 | Scope tap points | source / interchange (today's tiers) + display-referred (new) | open |
| 98 | Scope set | v1 = vectorscope + luma waveform (parity); histogram + RGB parade follow from the same kernel when wanted | open |
| 99 | Guides | parametric generator (action/title safe %, centre, thirds, aspect mattes, custom) + SVG templates via `vello_svg`; CPU tessellator + SVG parser retire; guides remain an OUTPUT overlay | open |
| 100 | Offscreen = idle | a hidden/collapsed node schedules no GPU work (the scopes rule, generalised in `min-ui`) | open |

### 19.5 Verify
- vello → offscreen RGBA texture → sampled by the present stage (vello renders to a wgpu texture;
  confirm premultiplied-alpha conventions and colour space: the overlay texture is display-referred
  SDR over an HDR frame on QCView → compose AFTER tone mapping, in output space).
- `vello_svg` coverage of the 13 SVGs (paths/rects/strokes only — expected fine).
- Atomic scatter cost: 960-wide sample at 60 fps + the waveform's full-res peak pass on 8K sources
  (today's design; measure in the perf harness, #47).
- One shader for the vectorscope's UV grid in both Rec.709 and Rec.2020 gamuts (today's hexagons
  per gamut).

## 20. The QCBridge companions: three wire contracts the port must not break (2026-11-06)

Two read-only surveys: the input side (QCBridge for Blender, QCBridgeAE the Adobe Mercury
Transmit device) and the output side (QCBridgeA for AJA, QCBridgeN for NDI, plus QCView's own
ring code). Headline: **there are THREE contracts, not one, and all three are byte-stable with
magic + exact version checks — the sidecar's shape (§4.5). The ring sources are one canonical
set of files (QCBridgeAE `src/common`, MIT-licensed on purpose) vendored with ZERO content drift
into QCView (2,643 lines) and both sinks (1,974 each). Nothing in the companions consumes any
family code, and nothing needs to.** The port's job is QCView's END of each contract, in Rust,
proven against the C++ copies.

### 20.1 The companions, measured
| | What | Stack | Size | Status / licence |
|---|---|---|---|---|
| **QCBridgeAE** 0.3.3 | Mercury Transmit device "QCBridgeAE to QCView" (one exported symbol `xTransmitEntry`), loads in AE + Premiere via MediaCore; publishes frames into the `/qcbae-ae` / `/qcbae-premiere` rings | C++20; Adobe SDK in `private/` | product 803 lines; ring/protocol/convert spine `src/common` 2,904; tests 2,276 (two-process tearing tests, bit-exact half-convert vs hardware, audio quiesce, Metal zero-copy, TSan dir) | shipped; GPL-3 with `src/common/{surface,protocol}` **MIT** |
| **QCBridge** 0.4.0 | Blender extension (≥ 4.5): host→REPLICA scene sync over QUIC; the replica renders and streams **SRT** to QCView; a tray **Rust agent** (quinn/tokio/tray-icon/eframe) owns the link, discovery, token, replica lifecycle, path mapping; encode helpers (Swift Metal+VideoToolbox 648 lines; Rust D3D11+MediaFoundation 1,445) read an INTERNAL shm ring | Python 12,276 (`ring0` bpy 7,206 / `ring1` pure 4,145 — "ring" = code layer) + Rust agent 6,126 | tests 2,603 + two-Blender smokes 4,538 + colour harness (PSNR > 30 dB, ΔE < 1) | shipped; GPL-3 |
| **QCBridgeA** 0.0.1 | headless sink: output bus → AJA card (SDI 1 + HDMI 1; 1080p/720p/UHD-12G; RGB10 frame store = the bus's RGB10 byte-for-byte; CSC to YCbCr; VPID PQ/HLG/SDR + HDMI HDR InfoFrame; stereo in ch 1-2; **no timecode, no genlock**, free-run AutoCirculate 4 frames) | C++ on libajantv2 17.5 (submodule) | 1,182 + probe 480 + vendor 1,974 | installers built; **never run against an AJA device on a Mac**; Phase-1 measurements owed; MIT |
| **QCBridgeN** 0.0.2 | tray sink: output bus → NDI 6 (`NDIlib_v6_load` at runtime; BGRA8 progressive, planar f32 stereo, **synthesized timecode**, receiver count as tally) | C++ (Qt tray) | 721 + tools 703 + vendor 1,974 | shipped (QCView ≥ 2.5.4); MIT; NDI headers never committed |

### 20.2 The three contracts
**A. `QCAE` — Transmit → QCView frame ring (+ `QCAA` audio).** Canonical = QCBridgeAE `src/common`.
- Names fixed per host: `/qcbae-ae`, `/qcbae-premiere` (+`-audio`, Premiere only), `/qcbae-probe`.
  No registry, no socket (privacy rule: "no network code, at all"). QCView maps `qcbae://ae|premiere|
  probe` → names in its OWN `host_bridge_url.h`.
- Mapping: POSIX `shm_open` (`O_CREAT|O_EXCL`, 0600, unlink-first; names ≤ 31 bytes); Windows
  `CreateFileMappingW` on the pagefile as **`Local\`+name** (session-local; sections can't be
  unlinked → the producer retires the old header and retries ~1.5 s). Open = map header, check
  magic + **exact** version, read `total_size`, verify the object size, then map all.
- Header (magic `0x51434145`, **v3**): slot_count (3..255, default 3), producer_pid, strides/offsets
  (**`slots_offset` page-aligned in absolute terms — Metal rejects 16-B-misaligned linear
  textures**), a 64 KiB ICC region (never used by the shipped device), atomics `latest` =
  `(seq<<8)|slot`, `reader_claim`, `host_state` (Active / PausedFocus / Paused / Retired).
  `SlotHeader{atomic seq (seqlock, odd = writing); FrameDesc}`; `FrameDesc` = 192 bytes: w, h,
  bytes_per_row (**rows padded to 256**), PixelFormat (RGBA16F / RGBA8 / RGBA16Unorm / RGBA32F),
  SourceTier, ChannelOrder, flags (Premultiplied / HasInf / HasNaN), `time_value/time_scale`
  rational, icc_generation, graphics_white, value_scale (`65535/32768` for AE 16 bpc), comp_name[128]
  (always the host label — privacy). **The shipped device writes ONLY RGBA16F / RGBA / top-down /
  Float32 / value_scale 1; QCView ACCEPTS ONLY that** (`host_bridge_source.cpp:~322`). No PAR, no
  fps (QCView derives it from three equal time deltas), no timecode (derived; Premiere only), no
  colour semantics at all ("container facts, not colour semantics" — the user picks the OCIO input).
- Concurrency = the `ringslot::` discipline shared by all three rings: writer picks the next slot
  skipping `reader_claim` (≥ 3 slots ⇒ one always free), seq odd → write → seq even → `latest`
  release; reader loads `latest`, stores `reader_claim` BEFORE reading, double-checks the seqlock;
  **never derive the slot from the sequence** (the a1 "bug 1"). Latest-wins; producer never waits.
- Lifetime: created lazily on the first frame, process-wide, grows never shrinks, retired on the
  last Transmit instance dispose (AE quits without Shutdown; 582 MiB rings were found behind a
  dead AE). Liveness = `process_alive(pid)` (Windows pid recycling accepted). Size cap: the device
  asks the host for ≤ 3840×2160 via a module reset.
- QCView's consumer today: polls **2 ms** healthy / 250 ms absent; **CPU memcpy per row into a
  pooled `QImage` RGBA16F** → `FrameHandle::cpu`. Zero-copy measured and PARKED (0.95 ms copy +
  2.3 ms `replaceRegion` at 4K; "D3D11 can't sample a file mapping"). Audio ring (`QCAA` v1):
  planar f32 in the host's layout, 128 × 1024-frame slots, no reader claim (lapped reader resyncs),
  "audio is a mirror, never a clock".

**B. `QCVO` — QCView → sinks output bus (+ `QCAA` audio).** Canonical = the same `src/common`.
- Names `/qcview-out-<id>` (id ≤ 13, `[a-z0-9-]`) + `-audio`. Sinks are **discovered by MANIFEST
  JSON** in `…/QCView/Outputs/` (`id, label, product, exec, args, protocol, raster:"auto"`), first
  registration wins; QCView **launches** the sink (`QProcess`, env `QCVO_ID`, `--id`), restarts ≤ 3/min,
  retires rings on disable (a QCView-launched sink quits; a user-started one waits).
- Header (magic `0x5143564F`, **v1**): request/reply seqlocks — sink writes `OutputRequest` (48 B:
  raster or 0×0 = producer's, format with fallback, transfer or 0 = producer's setting, layers),
  QCView answers `OutputReply` (128 B) or refuses (VersionMismatch / TooLarge > 3840×2160 /
  UnsupportedFormat / UnsupportedContent); consumer pid + **heartbeat 250 ms, 1-s freshness
  window** + a 128-byte status line shown verbatim in the rail; `OutputHdrInfo` (32 B) behind a
  seqlock at offset 1024. `OutputFrameDesc` 64 B: presentation_ns (`now_ns` = mach_absolute_time /
  QPC), source_frame_index, fps num/den, packed timecode, flags HasA/HasB/HasInf/HasNaN/
  TimecodeValid/DropFrame/Paused. Ring sized `output_frame_bytes(3840,2160,BGRA8)` × 3 slots.
- Wire: **BGRA8 and RGB10 (10:10:10:2, full range) ONLY**; RGBA16F defined but REFUSED (and would not
  fit the ring at UHD); v210/P216 don't exist. Transfer is a TAG, not a conversion: the picture goes
  out "as displayed" (SDR outputs via separate OCIO instances; PQ/HLG via the live chain). Layers
  granted = request ∧ the global Guides / Notes chips. Audio: 48 kHz stereo planar f32, 1024-frame
  packets, up to 4 rings, muted = zeros + `kAudioPacketMuted`.
- Render side: publish-on-change via a memberwise `OutputStamp`; composite at the output raster into
  RGBA16F → OCIO → wire texture + black letterbox + guides + strokes (annotations encoded at 203-nit
  graphics white for PQ, 75 % for HLG) → **Metal: blit into the slot wrapped ONCE with
  `newBufferWithBytesNoCopy` (zero CPU copies); D3D11: `CopyResource` → 3-deep staging ring →
  `Map(DO_NOT_WAIT)` from a 3-ms idle tick → row memcpy (ONE copy; the 3-deep ring + idle-tick
  publish fixed half-rate output and 160 → 80 ms latency)**. Lossy by design when `beginFrame`
  fails. **QCView NEVER fills timecode / TimecodeValid / Paused / HasInf / HasNaN** on the output;
  NDI requests SDR explicitly so its Transfer dropdown is dead.
- Stream output (`src/output/` 2,327): paced thread + audio + mux; **H.264 only** via VideoToolbox
  (hardware, RealTime) / Media Foundation (hardware MFT, CBR, low latency); AAC via FFmpeg; MPEG-TS
  over `srt://` (caller; latency/streamid/passphrase) or FLV over `rtmp(s)://`; picture = SDR
  BGRA8 composite → NV12 BT.709 limited on the GPU (no CPU copy); reconnect 1-2-4-8-10 s at a keyframe.

**C. Blender → QCView: SRT + a card file (no shm into QCView at all).**
- Wire: MPEG-TS **HEVC Main10 4:2:0, PQ with scene 1.0 = 100 nit, BT.2020 / bt2020nc / limited**,
  VUI pinned in the mux; 50 Mbps / 2560 wide / 30 fps / GOP 1 s defaults; SRT latency 120 ms,
  passphrase `sha256("qcb-srt:"+token)[:32]`, `pbkeylen=16`. **QCView LISTENS, the replica's
  ffmpeg CALLS** (0.4+). Keyframes for the first 3 s + idle re-sends ≤ 10 Hz; QCView joins with
  `fpsprobesize 0`.
- The **card** `blender.json` (`v:1`) in `…/QCView/Inputs/` (`kind, name, host, url, port,
  latency_ms, status, colour{…}, rows, mismatches, stream{…}, ts`), written atomically, rewritten
  on content change, removed on stop; QCView's `blender_card.{h,cpp}` (204 lines) watches it only
  after File ▸ Connect to Blender. Legacy `qcview://stream?url=…&name=…` link kept.
- **Colour-chain keys** (`wire, input, ws, ws_source, ws_assumed, config, config_bundled, display,
  emulation, display_cs, view, look, exposure, gamma, wb, curves, hdr, engine, cm_ok, cm_n,
  display_xy, display_gamma`) parsed by `applyStreamChainFromLink` (`window_manager.cpp:9894-10000`);
  **only `wire == "pq-bt2020-100"` is accepted** → pins OCIO input "Rec.2100-PQ Scene" / minColor
  `DRT_IN_REC2020` + `DRT_OETF_PQ_100`, engages OCIO, applies the host's view for the host's
  display ("host display emulate" in the LiveStrip).
- What leaves Blender is **scene-linear** (Cycles `read_color` after `view_draw`, bit-identical to
  stock; EEVEE via `GPUOffScreen` ≤ 15 Hz, change-hash skip); working→2020 matrix from
  PyOpenColorIO; the helper converts to PQ/2020 P010 on the GPU; **the VIEW transform is applied in
  QCView**. QCBridge's internal `QCBF` ring (Python/Swift/Rust, **three hand-synced layout copies,
  no cross-language test**) borrows the qcbae discipline but is NOT qcbae-compatible and never
  reaches QCView.

### 20.3 What the port must do, and must not
1. **Freeze all three contracts byte-exact** — magic, versions (QCAE v3, QCAA v1, QCVO v1, card v1),
   names, the `Local\` rule, the 256-B rows, the page-aligned slots, the manifest keys, the
   `QCVO_ID` launch protocol, the colour-chain keys and the single accepted wire. Exactly as the
   sidecar is frozen (§4.5). The companions are shipped, MIT where it matters, 1–3k lines each,
   and will not be touched by the port.
2. **`qcv-ring`**: a Rust implementation of the `ringslot::` discipline + the three ring layouts,
   `#[repr(C)]` with `static_assert`-equivalent size tests, covering QCView's END of each contract
   only: the QCAE/QCAA CONSUMER (today `host_bridge_source` + `live_audio_source`, 1,225 lines), the
   QCVO/QCAA PRODUCER (today `output_bus` + the two output passes, ~2,300). **Its acceptance test is
   the missing cross-implementation test: the Rust side against the vendored C++ copies, both
   directions, two real processes, on both OSes** (QCBridgeAE's tearing-test shape). Feature `live`
   in `qcv-player` (§10.3) = this crate + the SRT/FFmpeg live decoder + the card watcher.
3. **Zero-copy policy (restates #46 with the input side):** Metal wraps the mapped slot bytes as a
   no-copy buffer via hal in BOTH directions (the output pass already does; the input side was
   parked under Qt — the page-aligned `slots_offset` exists for exactly this); DX12 keeps one CPU
   copy each way (a file mapping is not a GPU resource on Windows). The 2-ms poll stays: the ring
   carries no cross-process wake-up, by design.
4. **The output pass is a presenter stage (§11.2 / §18.3 / §19.3 all meet here):** composite at the
   output raster → the SECOND OCIO chain instance for SDR outputs (two native-pass slots, §18) →
   wire texture BGRA8/RGB10 + letterbox + the OUTPUT-overlay texture (guides + ink, §19) → ring
   blit, with the publish-on-change stamp retained. The NV12 conversion for the stream encoder is
   one more consumer of the same wire texture.
5. **Stream encoding**: VideoToolbox via objc2 (`objc2-video-toolbox`) and Media Foundation via
   windows-rs behind one `VideoEncoder` trait; AAC + TS/FLV mux via the FFmpeg bindings already
   chosen (§4.3). H.264 only, as today.
6. **Cheap wins the Rust producer should take**: fill `timecode / TimecodeValid / Paused / HasInf /
   HasNaN` on the output ring (the fields exist; nobody writes them) — QCBridgeA can then do RP188
   later; honour `OutputRequest.transfer` for NDI or hide its dropdown.
7. **Not a port item, worth doing now in QCBridge**: a cross-language layout test for the QCBF
   triple copy (`ring_layout.py` ↔ Swift ↔ Rust) and refreshed vendor stamps (all say `3d7de5b`,
   all contain later changes).

### 20.4 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 101 | The three bridge contracts (QCAE/QCAA, QCVO/QCAA, SRT + card + chain keys) | **FROZEN byte-exact**, like the sidecar; companions untouched | open |
| 102 | `qcv-ring` | Rust `ringslot` + the three layouts, QCView's end only (QCAE consumer, QCVO producer); cross-implementation two-process tests vs the vendored C++ on both OSes | open |
| 103 | Companions | stay as they are (C++ / Python / Swift / Rust agent); `vendor/qcbae` remains the distribution; stamps refreshed | open |
| 104 | Zero-copy both directions | Metal no-copy buffer wrap via hal in + out; DX12 one CPU copy in + out; 2-ms poll stays | open (extends #46) |
| 105 | Output pass = presenter stage | second OCIO instance for SDR outputs; wire texture shared by rings + NV12 stream; output overlays from §19; publish-on-change stamp | open |
| 106 | Stream encoder | VT + MF behind one trait via FFI; FFmpeg AAC + mux; H.264 only | open |
| 107 | Unfilled output fields | the Rust producer fills timecode / paused / inf-nan; AJA RP188 later | open |
| 108 | QCBF cross-language layout test + stamp refresh | do now in QCBridge; not a port item | open |

### 20.5 Verify
- wgpu-hal Metal: `MTLDevice::newBuffer(bytesNoCopy:)` on an mmap'd shm page from Rust (objc2-metal)
  and importing it as a wgpu buffer/texture (hal `Device::buffer_from_raw` / texture_from_raw).
- A wgpu app packaged as MSIX opening `Local\` sections (the Qt MSIX did; confirm for a non-Qt exe).
- `objc2-video-toolbox` coverage of `VTCompressionSession` with RealTime + no-reordering properties;
  windows-rs MF hardware MFT selection (the vendor encoder) and D3D11/12 texture input to the MFT
  under a DX12 device (today it is D3D11; the DX12 ruling #6 means a D3D11-on-12 shim or the MF
  DX12 path — check what MF supports for hardware encode from a DX12 resource).
- The 2-ms poll on a wgpu event loop: a dedicated reader thread + `EventLoopProxy` wake, not the
  render loop.

---

## 21. The path forward: QCView-SHAPED first, one workspace repo, spikes with verdicts (2026-11-06)

The user: "I think we have a general shape of things. I am wondering if QCView is the first test
since it's the hardest and most up in the air and could guide a lot of the UI. Should we start a
new repo for min-ui and other repos for spikes and tests per app? What is the best way forward?"

### 21.1 Where this collides with what the canon already says
- §13's through-line: media platform first; minNotes = the first native app; "QCView adopts last
  and is never ported for speed". §10.4's order: `qcv-notes` + `qcv-ink` → `min-image` →
  `qcv-player` → `min-ui` + minNotes.
- §9 (rows 27/28/30/31): one Cargo workspace from day one; crate boundaries now, repo boundaries
  later; `min-ui` is extracted only on a SECOND real consumer; shared crates MIT OR Apache-2.0.
- But five of the last seven sections (§16–§20: panels, timeline, OCIO, scopes, rings) were
  QCView's problems, and every one of the PLAN-KILLING unknowns lives there: an HDR wgpu surface
  on both OSes, OCIO's generated shaders through wgpu's native passthrough, zero-copy from an
  mmap'd ring into a Metal texture, the presenter graph, a timeline as a content surface.
  minNotes needs none of those; if they fail, "a shared Rust media platform" collapses to
  "minNotes on its own toolkit", a much weaker case. So the user's instinct is right on RISK.

**Resolution: separate "the first TEST" from "the first PORT".** The first test is QCView-shaped
(the spikes, then a QCView-shaped reference app that gives birth to `min-ui`). What SHIPS first
is decided at a later gate, not now. §12's bar stands: QCView is not ported for speed; parity
is the bar; the Qt QCView keeps shipping untouched the whole way.

### 21.2 Repos: ONE new workspace, not one per crate and not one per app
**Recommendation: a single new repository holding a Cargo workspace** — crates, spikes, vertical
slices and the canon together. Name `minplatform` [proposed; crates keep the `min-*` / `qcv-*`
names]. Private until gate G1 (below), public after (every sibling repo is public).

Why not a `min-ui` repo: a toolkit repo with no app inside it drifts into a framework (the
Masonry failure mode, §16); row 28 already says extraction waits for the second consumer, and the
user's own premise — QCView guides the UI — requires `min-ui` to grow in the same tree as the
QCView slice. Why not per-app spike repos: spikes are per RISK, not per app; the Rust GPU
ecosystem moves in lock-step (wgpu majors ~quarterly; vello/parley track wgpu; winit 0.31 is a
beta pinned by git rev) and ONE lockfile is the only sane way to hold the pin for everyone;
cross-crate API churn in the first months must be atomic commits, not PR chains. Extraction
later is cheap (§9) as long as dependency direction is enforced (row 31).

```
minplatform/
  Cargo.toml        workspace; [workspace.package] + [workspace.dependencies] own EVERY pin
                    (wgpu, winit, vello, parley, taffy, objc2-*, windows-rs); resolver = "2"
  docs/             this canon moves here (first commit); docs/spikes/Sx-verdict.md per spike
  crates/           only crates with a consumer or a frozen contract (never speculative)
  spikes/           bin crates, numbered; each README = question / pass bar / time box / verdict
  apps/             vertical slices; apps/qcv first (21.4), apps/notes second
  vendor/           byte-exact C++ reference copies for cross-impl tests (qcbae ring, stamped)
  tools/            CI: dependency-direction check (row 31), vendor-stamp check, licence check
```
Rules: spikes depend on crates, never the reverse; a spike that passes is NOT renamed into
`crates/` — the crate is written fresh with the spike as its first integration test (spike code is
allowed to be ugly precisely because it dies); nothing in this repo touches the Qt repos — the
only interop is the frozen contracts (sidecar, `.mnd`, the three rings) and later the C ABI
(row 29). Licences per crate: shared crates MIT OR Apache-2.0 (row 30; `qcv-ring` MUST be MIT —
the MIT companions vendor the ring sources; MinRender is MIT and may only consume permissive
crates); `apps/*` GPL-3.0-or-later like their Qt ancestors; `min-color` derived from minColorAE
is the user's own code and can be relicensed permissive if MinRender ever needs it.

### 21.3 Phase 1 — the kill-shot spikes (QCView-shaped, plus the text long pole in parallel)
**Spike protocol:** one question, one pass bar WITH A NUMBER, one time box, one written verdict
(go / go-with-change / no-go) in `docs/spikes/`. No verdict = not done. A no-go is a legitimate
outcome and is the cheapest moment to learn it.

**P0 — the perf baseline (no Rust; row 47 made concrete).** Measure the Qt QCView and minNotes
first: seek-to-display latency, key→swap, scrub frame pacing, decode throughput per codec,
memory at 4K, startup. Every spike's pass bar is "vs this number". Time box: days.

| Spike | Question | Pass bar (numbers vs P0) | Answers | Box |
|---|---|---|---|---|
| **S1 present** | one decoded frame (EXR + ProRes via `min-image`/FFmpeg FFI) → wgpu → an HDR surface (Metal EDR extended-linear rgba16f; DX12 scRGB fp16 + HDR10) through an OCIO shader via native passthrough (MSL / HLSL→DXIL) with the baked-LUT path as the control | pixel-diff vs Qt QCView on the same frame within tolerance; EDR peak + PQ correct on both OSes; OCIO's generated shader compiles unmodified or with a documented patch; latency ≤ P0 | §11, §12 presenter, §18 #91–94 | 2 wk |
| **S2 scrub** | `qcv-timeline` rational time + two decoder slots + the Transport EMA on long ProRes, H.264 and an EXR sequence; one gapless cut | seek-to-display ≤ P0; gapless cut has no black/dup frame; shuttle ±32× holds pitch cap | §17 #83–90 | 2 wk |
| **S3 text** | parley editor + IME (macOS marked text; Windows TSF) + AccessKit in a winit window: multi-line, selection, clipboard, undo | CJK IME commits correctly on both OSes; VoiceOver + Narrator read it; 60 fps at 10k lines | §3, §16 "the long pole" | starts day 1, runs the whole phase |
| **S4 ring** | `qcv-ring`: the `QCAE` consumer + `QCVO` producer; Metal zero-copy via hal from the mmap'd slot; two-process tests vs `vendor/qcbae` | byte-exact layout tests pass on both OSes; the real AE Transmit device shows in the spike window; DX12 = one copy, measured | §20 #102 #104 | 1 wk |
| **S5 window** | winit 0.31 on both OSes: DnD receive (typed data transfer) + drag-out via objc2 / windows-rs; HiDPI; two windows (the dual-output shape); fullscreen; the native menu bar | §14's four-repo edge list reproduces or is ruled out per item | §14 #48–57 | 1 wk |

S1 is first because its no-go changes everything; S3 runs alongside because it is the longest;
S2/S4/S5 follow in that order. Calendar ≈ 6–8 weeks if S1→S2→S4→S5 are sequential.

**Gate G1 (end of Phase 1): go / go-with-LUT / no-go.** Go = passthrough works and the numbers
hold → Phase 2. Go-with-LUT = passthrough fails but the baked shaper+3D-LUT path (§18 route B)
matches within tolerance → Phase 2 with route B as the display path. No-go = the HDR/present
story does not hold on wgpu → the fallback is §10's FFI media platform WITHOUT a UI toolkit
(`qcv-notes`, `qcv-ink`, `min-image`, `qcv-ring`, `min-paths` as Rust crates consumed by the Qt
apps through cxx-qt / the C ABI). That fallback is still a good outcome and costs nothing extra
to keep open, which is why the crates are shaped as crates from the first spike.

### 21.4 Phase 2 — the reference app `apps/qcv`, where `min-ui` is born
A QCView-SHAPED vertical slice, not QCView: open a file or folder → presenter (S1) → transport +
timeline surface (S2) → rails/panels/popups/shortcut registry/focus model (§16) → scopes + guides
(§19) → DnD (§14) → annotations over the frozen sidecar (`qcv-notes` + `qcv-ink`) → the Adobe
ring (S4). OUT of the slice: dual output, AJA/NDI/stream sinks, live inputs, Blender SRT, HDR
beyond what S1 proved, ufb-style file browsing. `min-ui` is written inside this slice, API by
real use, no speculative widgets (the §16 union is the backlog, not the plan).
**Gate G2:** the slice plays the user's real QC sessions side by side with Qt QCView for a week
of actual work; the §16 widget list is reconciled against what the slice needed.

### 21.5 Phase 3 onward
- **Phase 3 — minNotes as the SECOND consumer** (`apps/notes`): S3's editor matured + the canvas +
  blocks + ink; `qcv-ink`/`qcv-notes` already shared. This is §9's rule of two in action: the
  second consumer proves what in `min-ui` generalises. minNotes has no long tail, so it may well
  be the first app to SHIP on the platform — decided at G2, not now.
- **Phase 4 — QCView's long tail** (dual, live, sinks, Blender, the full HDR matrix) → QCView 2
  ships; the Qt QCView is archived only then.
- **Phase 5 — MinRender** (MIT: permissive crates only) when its UI wants `min-ui`; **ufb LAST**
  (ruled 2026-11-06), as the Qt app that consumes the platform through cxx-qt first.
- Optional, parallel, near-zero risk: `min-paths` (#51) as crate #1, consumed by ufb through its
  existing cxx-qt bridge now — §10.4's "prove the shared-crate flow on something cheap", and the
  one pre-port deliverable that fixes a real bug today.

### 21.6 What this changes in the canon
- §13's "QCView adopts last" is REPLACED by "QCView-shaped first as the TEST; QCView-the-product
  last to be archived". §10.4's order becomes the Phase 2–5 order above. §12's "never ported for
  speed; parity is the bar" is unchanged. Row 28 (extract `min-ui` on the second consumer) is
  unchanged and now has a name for the second consumer: minNotes.
- The canon leaves `minNotes/private/` and becomes `minplatform/docs/` on the repo's first
  commit; `minNotes/private/README.md` keeps a pointer. **DONE 2026-11-06.**

### 21.7 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 109 | What the first test is | QCView-SHAPED spikes + a QCView-shaped reference app; NOT porting QCView first; §12 parity bar stands; Qt QCView untouched throughout | open |
| 110 | Repo | ONE new workspace repo (`minplatform`) holding crates + spikes + apps + the canon in `docs/`; private until G1, public after | **RULED 2026-11-06 (created, private)** |
| 111 | No `min-ui` repo, no per-app spike repos | `min-ui` grows inside `apps/qcv`; row 28 holds; spikes are per risk under `spikes/` | open |
| 112 | Spike protocol | question / pass bar with a number / time box / written verdict in `docs/spikes/`; spikes die, never renamed into `crates/` | open |
| 113 | Phase 1 set + gate | P0 baseline, then S1 present → S2 scrub → S4 ring → S5 window with S3 text in parallel; G1 = go / go-with-LUT / no-go→FFI-only fallback | open |
| 114 | Reference-app scope | §21.4 in/out list; G2 = a week of real QC work side by side with Qt QCView | open |
| 115 | Second consumer / first to ship | minNotes is the second consumer; which app ships first is decided at G2 | open |
| 116 | Licences in the workspace | per-crate: shared crates MIT OR Apache-2.0, `qcv-ring` MIT, `apps/*` GPL-3.0-or-later; CI licence check | open |
| 117 | Ecosystem pins | the workspace owns every pin in `[workspace.dependencies]`; one lockfile; **pinned 2026-11-06: wgpu 30.0.1, winit 0.30.13 (accesskit_winit 0.34 holds it; 0.31 beta for S5), vello 0.11, parley 0.12, accesskit 0.25** | open (pins taken) |
| 118 | `min-paths` now | crate #1, consumed by ufb via cxx-qt in parallel with Phase 1 (optional) | open |

**The first "go" — GIVEN 2026-11-06:** "go ahead and create the minplatform repo, move the canon, run P0".
DONE the same day: private repo `github.com/cbkow/minplatform` (workspace skeleton, `CLAUDE.md`, this canon in
`docs/`), **P0 MEASURED on macOS → `docs/spikes/P0-baseline.md`** (decoder floor, QCView startup/pacing/seek/
scrub/memory from Metal System Traces with no instrumentation, minNotes typing/startup; Windows owed; the one
number P0 could not take = QCView's per-video-frame lateness → S2's harness). **S1 + S3 SCAFFOLDED 2026-11-06** on "let's continue": `spikes/s1-present` (wgpu 30 window, Rgba16Float swapchain,
EDR ramp, `--edr`/`--pq`) and `spikes/s3-text` (parley 0.12 `PlainEditor` drawn by vello 0.11 with IME hooks) build and
run; verdict docs `docs/spikes/S1-present.md` + `S3-text.md` hold the pass bars and the remaining plan. S1 then went on the same day to steps 1 + 1b (sw control + VideoToolbox zero-copy; QCView-or-better numbers; picture
verified — see the spike doc). Two facts fell
out already: **wgpu 30 has HDR surfaces natively** (`SurfaceColorSpace::{ExtendedSrgbLinear, Bt2100Pq, Bt2100Hlg…}`;
wgpu-hal sets the CAMetalLayer EDR flag + colourspace and DX12's `SetColorSpace1` itself → the §11/§12/§18 "EDR via
objc2" item is CLOSED), and `accesskit_winit` 0.34 pins winit 0.30.13 (the 0.31 beta waits for S5). Row 110 RULED;
rows 109/111–118 still open.
