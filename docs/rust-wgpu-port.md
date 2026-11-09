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
| 6 | Windows video | W1: DX12 + d3d12va, software ProRes; drop the Vulkan device + bridges | **RULED 2026-11-05: W1 (DX12 + d3d12va)** — user: "no issues with dx12, doable path" — **REVISED 2026-11-07 night: the "software ProRes" clause is WITHDRAWN** (the user: "Vulkan ProRes is not an option on Windows. I thought we had that settled" — it is REQUIRED, row 149). DX12 + d3d12va stays for the inter codecs and the HDR surfaces; ProRes on Windows decodes on the GPU through FFmpeg's Vulkan compute decoder, the route (shared device vs external-memory export into DX12) = spike S6. Software ProRes is the interim only. |
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
| 38 | Shared-crate licence MIT OR Apache-2.0 so MinRender (MIT) can consume; `min-color` excepted | yes | **CLOSED 2026-11-07** — the workspace is GPL (row 116 revised); `min-color` needs no exception |

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
| 45 | Windows wgpu backend is per app: minNotes/ufb DX12 + d3d12va; QCView Vulkan + Vulkan hwaccel (keeps GPU ProRes) | per-app split | open — **user 2026-11-05: "we might want to consider dx12 anyway for better PQ / H.265 etc; cross that bridge later"** (d3d12va HEVC Main10 PQ maturity vs Vulkan ProRes — to weigh with the perf baseline, #47) — **evidence in (Windows catch-up on rhombus, 2026-11-06): wgpu's DEFAULT instance on an RTX 4090 picks VULKAN with an 8-bit sRGB swapchain only → the port forces DX12 (fp16 scRGB + 10-bit PQ offered); D3D12VA zero-copy into wgpu works (NV12/P010 via `texture_from_raw(..).with_plane_slice` + `add_wait_fence`, 0.27 ms/frame, 11 % of a core at 4K H.264); OCIO HLSL passthrough exact (FXC, SM5, explicit registers + one shared sampler); ProRes has NO hw decode on NVIDIA — QCView's Windows ProRes path is FFmpeg Vulkan video, which the port must measure (Vulkan↔DX12 interop or a per-codec path) before the per-app split is ruled** |
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
untouched; rows #101–108). **188 rows, 84 ruled / built (2026-11-07 night; the state of the port and the queue = §26 — 2026-11-07 pm: §25 the `min-ui` design direction, rows 160–168 RULED — Spectrum 2's structure and scales, Geist + Phosphor kept, Spectrum blue rationed to four uses, the timeline as a 32-px scrubber with the waveform as a faded fill, dual without an edit mode; step 2's rows 154–159 RULED — "go, start with the scrub decoder"; §24 rows 138–153 ALL RULED; the workspace GPL-3.0-or-later by row 116 revised, rows 38 / 140 / 151 closed; §24 the full-app audit — rows 143, 145, 146, 149, 152, 153 RULED by the user's steers, 138–142 / 144 / 147 / 148 / 150 / 151 proposed; §23 metadata rows 132–137 RULED; §22 min-paths rows 119–126 ruled, `crates/min-paths` BUILT 2026-11-06: 8 modules, 67 tests green on macOS + Windows, the row-120 differential test vs QCView's C++ = 28 cases IDENTICAL; ALL FOUR CONSUMERS ADOPTED 2026-11-06 on pushed, UNMERGED `min-paths` branches — ufb b0662f7, minNotes d3f1e8e, MinRender 9526557, QCView 9500e643 (pinned git deps since the public flip) — which stay unmerged until the plan has earned confidence, the user's ruling. **PHASE 2 STEP 1 DONE 2026-11-07 — GO (`docs/phase2/step1-open-present.md`), STEP 1b DONE the same day — GO (`step1b-qcview-decoder.md`: row 131, the decoder re-derived from QCView's `src/decode`); next = step 2 (transport + timeline), a talk first.**)** THE PATH FORWARD → §21 (2026-11-06): QCView-SHAPED spikes first (S1 present, S2 scrub, S4 ring, S5 window, S3 text in parallel; P0 = the perf baseline), ONE new workspace repo (`minplatform` [name]) with the canon in `docs/`, `min-ui` born inside a QCView-shaped reference app, minNotes the SECOND consumer, gates G1/G2; rows #109–118. **118 rows, 8 ruled. 2026-11-06 the first 'go' was GIVEN and DONE: this repo exists (private), the canon lives here, P0 is measured (`docs/spikes/P0-baseline.md`; Windows owed). S1 + S3 SCAFFOLDED the same day (wgpu 30 HDR surfaces native — EDR-via-objc2 CLOSED). **S1 steps 1 + 1b DONE 2026-11-06:** real video (ProRes 4K/1080p, HEVC10 4K, H.264 4K) on the EDR surface, software+CPU-upload control AND VideoToolbox ZERO-COPY into wgpu via hal (`docs/spikes/S1-present.md`): open→first frame 123–195 ms vs P0 385–500, present p95 17 ms vs QCView's 17–20, 0.1 ms/frame on the render thread, picture verified (snapshot; zero-copy vs control PSNR 58.7 dB). **S1 step 2 DONE 2026-11-06:** OCIO 2.5's generated MSL runs through wgpu 30's `PASSTHROUGH_SHADERS` path in QCView's own kernel wrapper, LUTs as wgpu textures, and is EXACT vs OCIO's CPU processor (≤ 0.13/255 on ACES 1.3, Blender AgX 3D-LUT, Blender Filmic 1D+3D-LUT chains at 4K); the baked 65³ LUT (route B) is 8-bit-exact only (worst 1.7/255, ~1 % of samples off at 10-bit); the colour pass costs 0.3–0.5 ms at 4K either way, pacing unchanged (`docs/spikes/S1-present.md`). Rows #91/#92 now have their evidence (A = display path, B = fallback/export). **S1 step 3 (2026-11-06, in progress):** the spike matches QCView's own `QCV_CAPTURE_PNG` of the same 4K frame at 55.9 dB PSNR / SSIM 0.998 (max 2/255); a synthetic PQ ramp decodes exactly to the file through BT.2020 + PQ onto the extended-linear surface and the built-in XDR ENGAGES EDR (headroom 1.0 → 8.5 while the window is up); **wgpu-hal 30.0.1 BUG found: `Bt2100Pq` / `Bt2100Hlg` / `ExtendedDisplayP3` surfaces CRASH in `configure` on Metal (the dynamic kCGColorSpace lookup passes the symbol's address as the CFString) — worked around in the spike by setting the layer's colour space by name; upstream report owed.** Step 3 DONE the same day (the PQ surface shown on the XDR, EDR headroom 8.8; software + VideoToolbox PQ identical; footprint 0.40–0.66 GB vs QCView 1.36). **S1 VERDICT 2026-11-06: GO on macOS** — every bar met except the whole-frame GPU figure of the UNFUSED spike graph (1.9 ms p95 vs the 1 ms bar; the colour pass itself 0.3–0.5 ms — the fused presenter reads the planes inside the first colour stage); Windows numbers (DX12 + D3D12VA + DXIL + PQ) OWED before gate G1. **S2 SCRUB DONE 2026-11-06, VERDICT GO (macOS; `docs/spikes/S2-scrub.md`):** rational time + two prewarmed decoder slots + a software scrub decoder (slice threads intra / frame threads + retarget + GOP-wide LRU inter) on the S1 presenter: seeks 15.9 ms (4K ProRes) / 7.4 ms (1080p 5-min) via the scrub decoder vs QCView 60–130; scrubs 5.6 / 3.5 ms vs 30–40; H.264 scrubs 1.3 fwd / 13.5 p95 back; the playlist CUT is GAPLESS (no black, no dup, exact 3:2 cadence, mid-GOP H.264 too); shuttle 2×→32× one frame per refresh; per-video-frame lateness p95 < 1 refresh with 0 drops (the number P0 could not take). Rows #83 (rational time) and #85 (two slots) have their evidence; the §4.2 scrub-decoder facts gained threading rules. Owed: Windows, audio. **S3 steps 1 + 2 DONE on macOS 2026-11-06:** the AccessKit tree verified through the AX API (VoiceOver's tree; selection set from outside round-trips); IME = press-and-hold accents + dead-key composition confirmed on the user's keyboard — after finding and patching FIVE winit 0.30 text-input gaps in a VENDORED winit (`vendor/winit`, `[patch.crates-io]`; upstream needs an `Ime::Replace`); the CJK bar rewritten to "composition commits" (the user types none). The WINDOWS catch-up (P0 + S1 + S2) runs on rhombus in a fork (branch `windows`). **S3 step 3 DONE + S3 VERDICT 2026-11-06: GO on macOS** — ONE EDITOR PER BLOCK (parley's `PlainEditor` relayouts its whole buffer per edit: 7 ms/key at 1k lines in one editor, 24 µs/key at 10k lines in blocks), per-key 35 µs p95 vs minNotes' 984, key→present 2.3 ms p50 with accessibility live vs minNotes' 3.4, 1.3 ms CPU per frame at 10k lines; clipboard (`arboard`) + undo-as-ops in; the `min-ui` text node = a block editor stacked by its owner (as §16 assumed), never a document editor. Owed: dictation, Windows (TSF, Narrator). Phase 1 on macOS now has S1, S2, S3 at GO. **WINDOWS CATCH-UP (rhombus, 2026-11-06, branch `windows` merged):** P0 decoder floor + QCView-Windows taken; S1: DX12 forced (wgpu defaulted to Vulkan/8-bit), D3D12VA zero-copy into wgpu WORKS (0.27 ms/frame, 11 % core), OCIO HLSL passthrough EXACT, PQ arithmetic identical, pixel-diff vs QCView-Windows 56.4 dB; S2: the Sequencer runs unchanged on D3D12VA slots, two hw contexts cut gaplessly, D3D12VA tolerates flushes so the hardware scrub decoder is the Windows choice (7.5 ms). **BUT rhombus has NO PHYSICAL DISPLAY (a Parsec virtual display at ≈ 13 Hz), so Windows PACING, EDR and PQ-on-a-display are still owed — they need a real (HDR) monitor on the box.** Also owed: ProRes via Vulkan video on Windows, minNotes typing on Windows, S3 on Windows (TSF, Narrator). **S5 WINDOW DONE on macOS 2026-11-06, VERDICT GO (`docs/spikes/S5-window.md`):** winit 0.31.0-beta.4 is the window layer — typed receive (Finder's two files → two URIs), clipboard, single-item drag-out (a real file lands in Finder), self-drop + actions, two windows, fullscreen, HiDPI; native-layer items: multi-file drag-out (the beta HANGS on multi-item sessions — §14.3 corrected: drag-out exists), the menu bar (objc2 `NSMenu`), S3's text-input patch carried to 0.31; "never press-start" holds for winit. Windows owed. **S4 RING DONE on macOS 2026-11-06, VERDICT GO (`docs/spikes/S4-ring.md`):** the three `#[repr(C)]` layouts BYTE-EXACT vs the vendored C++ (106 checks, 0 mismatches); the QCAE consumer imports a slot ZERO-COPY on Metal (no-copy buffer → linear texture → wgpu hal) at 0.2 ms per 4K frame (CPU copy 7 ms; QCView parked 0.95 + 2.3) with Rust AND QCBridgeAE-C++ producers; the REAL After Effects Transmit device shows in the spike window zero-copy at 0.08 ms per 1080p frame (copy 1.6), no skips; the Rust QCVO producer drives the unmodified shipped QCBridgeN sink end to end (attach, request/reply, heartbeat, frames taken, clean exit). Contract facts: QCAE v3 has NO wall-clock stamp (latency = the 2-ms poll, by design); AE writes `comp_name` = "After Effects", `time_value` −1 over AE ticks (254016000000), RGBA16F / Float32 / RGBA, flags 0; the transfer of AE's values is the presenter's job. **S4 WINDOWS DONE 2026-11-06 (rhombus, MSVC 18):** layouts BYTE-EXACT under cl (106 checks), `Local\` sections + the C++ create/open/liveness semantics mirrored exactly, MSVC-built QCBridgeAE `SharedRing` → Rust consumer green, one-copy DX12 import ≈ 5 ms p50 per 4K frame (a 66 MB memcpy into the staging belt; a threaded memcpy into a mapped upload buffer would make it ≈ 1.5 ms later), the hand-over between a retired section and a fresh one of the same name walked correctly; QCVO section created; no AE / QCBridgeN on the box (contract covered on macOS, same bytes). **S4 = GO on both OSes. macOS Phase 1 COMPLETE: S1, S2, S3, S4, S5 at GO; Windows has S1/S2/S4 numbers, S3/S5 on Windows + pacing/EDR/PQ need the user at a real monitor. **GATE G1 RULED GO 2026-11-06 + rows 109–118 RULED (`docs/spikes/G1-gate.md`): Phase 2 = the reference app `apps/qcv` (§21.4 + S2 scrub policy + the Adobe ring from week one); row 118 reversed — `min-paths` is crate #1 NOW (every app uses it), ufb via cxx-qt; row 117 = winit split (window layer 0.31 / text layer vendored 0.30) until accesskit_winit follows; the public flip waits on the user's word after a last pass over the vendored winit patch + spike docs. 118 rows, 17 ruled.** Next = the Windows runs (rhombus needs a real HDR monitor for pacing/EDR/PQ; S3/S4/S5 on the box need none), then gate G1 (row 113), or rulings on #109–118 / #48–108.**

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
- **Drag-out — CORRECTED by S5 (2026-11-06): winit 0.31 HAS it** (`ActiveEventLoop::start_drag` with a
  `DataTransferSendBuilder` + `DragIcon`, `OutgoingDragDropped { action }` / `OutgoingDragCanceled`); single-item drags work
  end to end (a real file copied into Finder); multi-item drags HANG `beginDraggingSessionWithItems:` in beta.4 (frame and
  file-url-item fixes tried, not the cause) → multi-file drag-out stays a native-layer item until upstream fixes it. The
  `drag` crate (CrabNebula, used by Tauri) remains the reference for that layer.
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
| 49 | OS integration | winit 0.31 data-transfer API for receive + clipboard + SINGLE-item drag-out; OWN native MULTI-file drag-out (objc2 `NSDraggingSource`, one `public.file-url` per item) until winit's multi-item path works; the menu bar via objc2 | open — **evidence in (S5, macOS 2026-11-06)**; Windows owed |
| 50 | Promised / virtual files | v1: NONE out (write a real temp file, then drag); IN: materialise `FileGroupDescriptor` / promises to scratch on drop (volatile → copies anyway) | open |
| 51 | `min-paths` crate (Tier 2) | extract ufb's path/url/uri-list/same_volume/self-drop + clipboard file-list code + minNotes' PathMap refs & source classifiers; ufb consumes it first | open — **the first DnD deliverable, pays pre-port** |
| 52 | Cross-process family payloads | files (`text/uri-list` / per-item `public.file-url` / CF_HDROP) + a `qcview://` / `minnotes://` link in `text/plain`; no shared binary pasteboard type | open |
| 53 | Drag-out actions | real files: Copy from minNotes/ufb, Link from QCView's bin; internal move/copy: modifiers at DROP (Ctrl/⌘ copy, Shift move), `same_volume` default | open |
| 54 | minNotes new zones | canvas hover + frame lighting for external drops; Layers frame-row file drops; page text/html/URL drops; media + block drag-out | open |
| 55 | QCView new zones | window-level target (fixes Windows URL drops), frame-grab drag-out as temp file; `.qcvproj` / sidecar / LUT drop zones | open |
| 56 | Refusal UX | every drop through one per-app gate + a visible toast (QCView's model) — in all four apps | open |
| 57 | MinRender queue drops | yes, as a submission shortcut; last | open |

### 14.6 Verify before building (adds to §6)
- ~~winit 0.31: the exact `DataTransfer` type-name surface per OS; whether drag-OUT exists; whether the macOS backend
  implements `NSDraggingDestination`~~ **CLOSED on macOS by S5 (2026-11-06):** `TypeHint` = Plaintext / UriList / Html / Rtf /
  Audio / Image; macOS maps `public.file-url` + legacy `NSFilenamesPboardType` → UriList (read via `readObjectsForClasses`
  NSURL, one item per file: Finder's two selected files arrived as two `file:///` URIs), String → Plaintext, HTML → Html;
  drag-OUT exists (above); the AppKit backend implements `NSDraggingDestination` on its WindowDelegate (periodic updates
  on) — one OS drop target per window, no overlay subclass. Still open: `with_drag_and_drop(false)` on Windows, CF formats.
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
| 73 | Widget set | the §16.2 union (~45 kinds + ~15 containers), written in-house on parley/vello; Masonry = reference + parts donor | **RULED 2026-11-07** — the vocabulary is Spectrum 2's catalogue (§25 row 165) |
| 74 | Input core | one shortcut registry w/ contexts; one focus model (chrome never keeps focus); Esc = cancel stack | open |
| 75 | Popups | nodes in the one scene; `may_leave_window` → OS window as a deferred escape hatch | open |
| 76 | Theme | dark-only v1; tokens as DATA (`min-theme`, §10.3) — the FIRST real shared design system (today: 3 Qt styles, 4 token files, 27–35 shared names) | **RULED 2026-11-07** — in Spectrum 2's three-tier shape (§25 row 160) |
| 77 | Chrome contracts | the §16.4 measurements carried verbatim (ramps, fades, thresholds, hairlines, radii, type, icons, title-row geometry) | open |
| 78 | The typed-cell grid | ufb's CellEditor kinds (text / number / checkbox / dropdown / priority / date / notes / links) = the `grid` kind's spec; add `user` later | open |
| 79 | Text input | parley editor + winit IME + AccessKit from day one; ONE implementation for chrome fields and the block editor | open — **the long pole** |
| 80 | Layout solver | Taffy for `row`/`grid` if it fits computed layout with text-measure as the single measure-back exception; else own | open — verify |
| 81 | Icons | keep the Phosphor set; render as glyphs via parley OR as SVG via `vello_svg` (one choice; SVG avoids icon-font hinting at 125 %) | **RULED 2026-11-07** — glyphs through parley, QCView's slug → codepoint table ported; SVG deferred (§25 row 161) |
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
**2026-11-07, from the audio step:** `PlaybackTimer::seek` / `play` set `m_firstUpdate`, so the next update only re-bases the delta and the clock loses one refresh (~16–50 ms) at every play and seek landing — audible as audio ~50 ms ahead until the servo grinds it out; the port takes the base at the call instead. `AudioPlayer::releaseHold` compares the master to anchor + consumed without the device latency (audio resumes one playout latency late). `AudioOutputUnitStart` on the UI thread blocks ~105 ms per play. `PlaybackTimer::seek` returns early on an unchanged position without re-basing its delta, so with the seek gate holding `update` the first tick after a landing integrates the whole landing time (the clock 20–40 ms ahead after every seek).
Undo stack never cleared across loads; trim-tail/slip have no upper clamp to `sourceDuration`;
edits commit un-snapped seconds; `clipToMap` drops sequence fields + `audioMuted` (lossy restore);
`locked` unenforced; dual allows overlaps against its own policy; playlist fps/gap hard-coded 24/0
in the UI; `precacheRange` has no callers; the undo stack is used as an "edited" flag.
**Added 2026-11-07 (found by the port's tests):** `MoveClipEdit::undo` moves the clip back to slot `from` — right for
forward moves, wrong for backward ones (post-move the clip sits before its old slot, so the old slot is `from + 1`); a
two-clip playlist with the second moved to the front never undoes at all (the target is its own slot + 1, the no-op case).

### 17.7 Added decisions
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 83 | Time base in the shared timeline model | rational time (value i64 + rate), own type; seconds display-only; edits frame-snapped by construction | open — **OTIO itself RULED OUT 2026-11-06; evidence in (S2): the cut and the 3:2 cadence fell out of it** |
| 84 | `qcv-timeline` in the player crate | types + 8 ops with inverses (op-based undo) + flattener + snap + mapping; policy enforced; undo cleared on load; ONE `Sequencer` replaces the WindowManager orchestration | **BUILT 2026-11-07** (step 2 piece 3: `qcv-timeline`'s `Editor`; the Sequencer's clip → slot dispatch = the next piece) |
| 85 | Gapless playlists | two decoder slots + video next-clip prewarm in `qcv-player` | **BUILT 2026-11-07** (step 2 piece 5: the Transport's two slots + the threaded prewarm; gapless on every fixture pair, the wrap in one refresh; S2 retired) |
| 86 | Timeline rendering | one surface node on the canvas camera (1-axis), draws from the model per frame, visible-range only; no per-clip nodes | open |
| 87 | Hover thumbnail | a UI node over the viewport (display-referred), fed by `min-image`'s thumbnail cache; the presenter composites no UI | open |
| 88 | Scrub / shuttle / debounce constants | in `Transport` once; minNotes' QML copies retire | **BUILT 2026-11-07** (step 2 piece 2: `transport.rs` constants; the hover throttle + drain beside the thumbnail cache) |
| 89 | Edit feature set v1 | today's ops + frame snapping + source clamps + in/out in the model; no multi-select / roll / overwrite / markers | open |
| 90 | §17.6 bug list | fix now in QCView | open |

### 17.8 Verify
- ~~OTIO Rust bindings vs own `RationalTime`~~ **RULED 2026-11-06: no OTIO — "overkill for what we
  have; stick with the current flow"** (own structs, own time type; no `.otio` import/export). #83's
  rational time stays a PROPOSED default independent of OTIO.
- ~~Two-decoder gapless on Windows DX12 with d3d12va: two hw decoder contexts resident at once~~ **ANSWERED on rhombus
  (2026-11-06): two 4K D3D12VA contexts (H.264 + HEVC10) resident on one ID3D12Device cut gaplessly at 18–20 % of a core, no
  VRAM/session limit hit (RTX 4090).**
- **From S2 (2026-11-06):** the scrub decoder is SOFTWARE on Apple with SLICE threads for intra codecs and FRAME threads for
  inter codecs (frame threading after a flush costs a pipeline of packets before the first frame — 64 ms for one 4K ProRes
  frame; slice-threaded 5.6 ms); a streaming decoder's first frame after a flush is a pipeline fill (VideoToolbox 65 ms ProRes /
  280 ms H.264 / nothing within a sweep for inter after a flush; FFmpeg frame threads 140 ms EXR), so the Sequencer answers
  seeks from the scrub decoder and the stream resumes; latest-wins must RETARGET forward within reach instead of abandoning
  (QCView's literal rule starved at 60 Hz on 4K H.264); every frame of a GOP fill enters the LRU (backward sweeps hit).
  **The scrub-decoder policy is PER PLATFORM (Windows catch-up, 2026-11-06):** D3D12VA recovers from `avcodec_flush_buffers` in
  42–149 ms and a HARDWARE scrub decoder on H.264 4K answers in 7.5 ms p50 at 41 % of a core vs 43 ms / 365 % software
  frame-threaded on the i9 — the opposite of Apple, where VideoToolbox is the one to avoid for inter codecs. Software ProRes
  scrubs are ~3× slower on the i9 than on the M5 Max (18.8 vs 5.6 ms).
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
- ~~DX12: dxc availability on the box (ship `dxcompiler.dll`), DXIL signing, one entry point per module~~ **CLOSED on rhombus
  (2026-11-06): wgpu's DX12 backend compiles OCIO's HLSL SM 5.0 with FXC — no dxcompiler.dll — through
  `ShaderModuleDescriptorPassthrough { hlsl }` once OCIO's globals carry explicit `register()`s and its samplers map to one
  shared `s0`; exact vs the CPU processor (0.13/255), 65 ms to build; DX12 timestamp queries work (2.2 ms for the 4K pass — an
  upper bound from a virtual display).** The async compile pipeline question is the fused presenter's, not the spike's.
- **NEW (S1 step 3, 2026-11-06): wgpu-hal 30.0.1's Metal surface crashes on `Bt2100Pq` / `Bt2100Hlg` / `ExtendedDisplayP3`**
  (`lookup()` in `metal/surface.rs` returns the symbol's address as `&'static CFString`; SIGBUS in `CGColorSpaceCreateWithName`).
  One-line fix upstream; until then the presenter sets the CAMetalLayer colour space itself by name after `configure` (the
  kCGColorSpace* constants' values are their own names). The §11/§12 "wgpu sets EDR itself" fact stands for the extended-linear
  surface, which is the one that works unpatched.
- ~~OCIO 2.5's MSL 1D-LUT channel bug (issue #2272) — does QCView's current pin hit it.~~ **Not on this pin (2.5.0, S1 step 2):**
  Filmic's red-only 4096-entry 1D LUT through the native path matched the CPU processor at every sample.
- Route B's error vs route A on the ACES 2.0 display transforms at PQ 1000 nits (a measured
  comparison before calling B a "fallback" for HDR).

### 18.6 The colour architecture, revisited (2026-11-07 night) — BANKED, not yet ruled
**How it surfaced.** Chasing "Auto range is wrong for ProRes": the range rules are QCView's verbatim (untagged YCbCr →
limited, RGB → full — `effectiveSourceRange`; every ProRes fixture is tagged tv; our code values match FFmpeg within ±2).
The real finding: `Output::Auto` takes the EDR layer and the NO-CHAIN display path linearises BT.1886 with gamma 2.4, which
macOS then re-encodes with the sRGB curve — shadows crushed for EVERY SDR clip (stored 20 → 7 on screen, 71 → 61, 128 → 121).
The headless `Snapshot::Encoded` probe is blind to it by design (QCView's PNG-capture equivalent: matrix + range only).
QCView: the swapchain defaults to SDR sRGB (BGRA8, a Color-panel picker 0..4, not persisted); with no chain the view is in
BYPASS = as stored, and on a linear drawable the compositor's `srgbToLinearSrc` only undoes the OS encode; Rec.1886 lives in
the chain's INPUT half alone (`MinColorInput.transfer` default, the OCIO presets' named input). So the S1 presenter's "one
EOTF for every path" is the deviation; the parked question was whether to carry QCView's picker and SDR default.

**The user's proposal (2026-11-07 night), assessed and shaped:**
1. **One linear-light surface, always** — Apple EDR extended-linear sRGB / Windows scRGB fp16, 1.0 = SDR white. The OS owns
   the display encode, the panel's gamut and the HDR mapping (both vendors' recommended path). SDR does not break: a 709 clip
   lands where it does on an 8-bit sRGB surface, at 16-bit precision. The output-mode PICKER RETIRES (the decision the user did
   not want to hand to users). Captures (frame grabs, thumbnails, the review PNG, a QCBridge feed) still need OUR encode —
   stage 3's sRGB / PQ encodes stay for files only.
2. **minColor is the only engine; its Display half goes, its Rendering half stays** (Knee / AgX / OpenDRT + the target gamut +
   nits), because the OS only CLIPS: an HDR or wide-gamut source on an SDR panel still needs tone + gamut mapping. The
   Rendering target is the one view-like decision minColor retains.
3. **OCIO is NOT ripped out — it becomes a LUT source.** Ripping it out loses what client pipelines dictate (ACES 1.3 / 2.0
   output transforms, Blender's AgX config, studio configs via `$OCIO`, the camera-log inputs, looks, CDLs) — the biggest
   "deciding for the user" of all. Instead each preset is baked OUT OF PROCESS with `ociobakelut` (OCIO's own CLI, shipped):
   input space + look + VIEW against ONE patched display, a log shaper for scene-linear inputs, FLOAT output (values > 1.0 and
   negative components survive), applied as one float-LUT node inside minColor's pipeline. No OCIO in the Rust process, no
   OCIO GPU shader generation, no vcpkg build on Windows (rows 91 / 93 / 94 are superseded if this is ruled; row 92's
   8-bit-exactness finding stands and is the known cost — re-measure at 65³ vs 129³ float before ruling).
4. **OCIO needs one display, not the user.** A view is defined against a display colorspace; ours is fixed: QCView's EDR patch
   ("Linear sRGB EDR" at 1.0 = 100 nits, today one of five) becomes the ONLY display in every bundled config; the P3 twin goes
   (extended-linear sRGB carries P3 through out-of-range components). The user picks config · input colorspace · view · look.
   An "ACES 2.0 SDR 100 nits" view peaks at 1.0, an "HDR 1000 nits" view at 10.0; the OS does the rest on an HDR panel.
5. **Bypass = as stored, on every surface.** With no chain, display-referred SDR (sRGB / BT.1886 tags) is linearised with the
   sRGB EOTF so the OS encode is a no-op — QCView's rule, not a colour opinion. NOT "BT.1886 → sRGB by default": the
   declaration of what the file is happens when the user engages a chain (minColor's Input default = Rec.1886, as QCView).
6. **The gate is row 130** (the Windows float-surface check on a real HDR monitor): HDR off → DWM converts the fp16 surface
   (banding?); HDR on → 1.0 follows the user's SDR-brightness slider, so our `white` must track the reported SDR white level
   for HDR views to land at their nits. Until it runs, Windows stays measured only on a virtual display.
7. **The Inspector's Transfer row is QCView's, not a new chip set** (the user asked whether as-stored needs a BT.1886 / sRGB / linear
   override): Auto · SDR 709 · PQ 2020 · PQ P3 · HLG · Linear, per clip, persisted as `transferOverride` — "how the scopes read this clip
   when its tags are missing or wrong"; there is deliberately NO 2.2-vs-2.4 chip because bypass applies neither curve, and with a
   chain the clip's Input decides. WIRED 2026-11-07 night (engine-level `TransferChoice`; the chips label Auto with the tags'
   resolution). The line beside the code: matrix · range · transfer tag · aspect · rotation are DECODE facts (Inspector); curves ·
   gamuts · renderings are INTERPRETATION (the chain).
8. **Worms left in the can (named, not solved):** an HDR view on an SDR panel clips at 1.0 (Knee / Rendering territory);
   LUT precision for 10-bit review (row 92); the per-config patch upkeep when configs upgrade (§18.1's checklist); scopes keep
   tapping the working buffer after the chain (unchanged).

| # | Decision | Proposed default | Status |
|---|---|---|---|
| 179 | The surface | ONE linear-light float surface always (EDR extended-linear sRGB / scRGB fp16), 1.0 = SDR white; the output-mode picker retires; `Output::Auto` → always linear; stage-3 encodes kept for captures only | **BANKED 2026-11-07 night** (the user: "ok. lets bank this") — RULE after row 130 |
| 180 | Bypass semantics | no chain = AS STORED on every surface (sRGB EOTF only undoes the OS encode); never a default transfer opinion | **LANDED as the interim 2026-11-07 night** (the user: "got it and agreed"): the no-chain display set selects the sRGB EOTF for sRGB / BT.1886 tags; the chain-in set keeps the tagged transfer; `Snapshot::Display` + `--snap-display` read the screen path (the 1080p master now matches FFmpeg ±2 through it); PQ / HLG still decode to nits in bypass (open) |
| 181 | minColor's halves | Rendering kept (Knee / AgX / OpenDRT, target gamut + nits); Display half removed | banked |
| 182 | OCIO's role | a LUT SOURCE: `ociobakelut` out of process → float shaper + 3D LUT node in minColor; no OCIO in-process (supersedes 91 / 93 / 94) | **REJECTED 2026-11-08** (row 200: native shaders) |
| 183 | OCIO's display | ONE patched "Linear sRGB EDR" display per bundled config (the existing EDR patch promoted); the user picks config · input · view · look | banked |
| 184 | The gate | row 130's Windows HDR-monitor session: fp16 with HDR off (banding), SDR white level with HDR on | banked **DONE 2026-11-08 on rhombus's HDR monitor (HDR on):** the surface = scRGB Rgba16Float ExtendedSrgbLinear on the RTX 4090; SDR looked dim (scRGB 1.0 = 80 nits) → `sdr_white.rs` reads the window's monitor's DISPLAYCONFIG_SDR_WHITE_LEVEL every 2 s (248 nits on the box) and the picture, UI, background and waveform passes take level / 80 (a07cd74); the user: "that is much better", then "perfect". 1.0 = SDR white on both OSes. Not run: HDR OFF (DWM's conversion / banding). |

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
| 101 | The three bridge contracts (QCAE/QCAA, QCVO/QCAA, SRT + card + chain keys) | **FROZEN byte-exact**, like the sidecar; companions untouched | open; evidence 2026-11-06: S4 — the three layouts byte-exact (106 checks), real AE + real QCBridgeN speak to the Rust end unmodified |
| 102 | `qcv-ring` | Rust `ringslot` + the three layouts, QCView's end only (QCAE consumer, QCVO producer); cross-implementation two-process tests vs the vendored C++ on both OSes | open; evidence 2026-11-06: S4 GO on BOTH OSes — `ringslot` + layouts + `Mapping` (shm / `Local\` sections) + the per-platform import proven vs the unmodified C++ |
| 103 | Companions | stay as they are (C++ / Python / Swift / Rust agent); `vendor/qcbae` remains the distribution; stamps refreshed | open; evidence 2026-11-06: S4 ran against the SHIPPED QCBridgeAE device and QCBridgeN sink, nothing touched |
| 104 | Zero-copy both directions | Metal no-copy buffer wrap via hal in + out; DX12 one CPU copy in + out; 2-ms poll stays | open (extends #46); evidence 2026-11-06: Metal in = 0.2 ms/4K zero-copy, DX12 in = one copy ≈ 5 ms/4K (S4, both measured); out owed with the output pass (#105) |
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
| **S3 text** | parley editor + IME (macOS marked text; Windows TSF) + AccessKit in a winit window: multi-line, selection, clipboard, undo | composition commits correctly on both OSes (dead keys, press-and-hold, dictation on macOS; TSF on Windows — CJK rides the same path, chased on report; REWRITTEN 2026-11-06); VoiceOver + Narrator read it; 60 fps at 10k lines | §3, §16 "the long pole" | starts day 1, runs the whole phase |
| **S4 ring** | `qcv-ring`: the `QCAE` consumer + `QCVO` producer; Metal zero-copy via hal from the mmap'd slot; two-process tests vs `vendor/qcbae` | byte-exact layout tests pass on both OSes; the real AE Transmit device shows in the spike window; DX12 = one copy, measured | §20 #102 #104 | 1 wk |
| **S5 window** | winit 0.31 on both OSes: DnD receive (typed data transfer) + drag-out via objc2 / windows-rs; HiDPI; two windows (the dual-output shape); fullscreen; the native menu bar | §14's four-repo edge list reproduces or is ruled out per item | §14 #48–57 | 1 wk |

S1 is first because its no-go changes everything; S3 runs alongside because it is the longest;
S2/S4/S5 follow in that order. Calendar ≈ 6–8 weeks if S1→S2→S4→S5 are sequential.

**Gate G1 (end of Phase 1): go / go-with-LUT / no-go. RULED GO 2026-11-06** (evidence: `docs/spikes/G1-gate.md`; the one carried condition: the Windows display numbers on a real HDR monitor before the Windows presenter is called done; route A native passthrough is the display path, route B the test control). Go = passthrough works and the numbers
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
| 109 | What the first test is | QCView-SHAPED spikes + a QCView-shaped reference app; NOT porting QCView first; §12 parity bar stands; Qt QCView untouched throughout | **RULED 2026-11-06** — vindicated by S1–S5 (every spike found something a minNotes-shaped one would have missed) |
| 110 | Repo | ONE new workspace repo (`minplatform`) holding crates + spikes + apps + the canon in `docs/`; private until G1, public after | **RULED 2026-11-06 (created, private)** |
| 111 | No `min-ui` repo, no per-app spike repos | `min-ui` grows inside `apps/qcv`; row 28 holds; spikes are per risk under `spikes/` | **RULED 2026-11-06** — widgets are pulled into existence by `apps/qcv`; S3's one-editor-per-block rule came that way |
| 112 | Spike protocol | question / pass bar with a number / time box / written verdict in `docs/spikes/`; spikes die, never renamed into `crates/` | **RULED 2026-11-06** — followed five times; the spikes stay in the tree until the crate replacing each is proven against the spike's own numbers, then deleted |
| 113 | Phase 1 set + gate | P0 baseline, then S1 present → S2 scrub → S4 ring → S5 window with S3 text in parallel; G1 = go / go-with-LUT / no-go→FFI-only fallback | **RULED 2026-11-06** — Phase 1 done as written; **G1 = GO** with one carried condition: Windows pacing / EDR / PQ measured on a real HDR monitor before the Windows presenter in `apps/qcv` is called done (`docs/spikes/G1-gate.md`) |
| 114 | Reference-app scope | §21.4 in/out list; G2 = a week of real QC work side by side with Qt QCView | **RULED 2026-11-06** — §21.4 list holds, plus two "in": S2's scrub-decoder policy as a first-class part of the slice, and the Adobe ring from week one |
| 115 | Second consumer / first to ship | minNotes is the second consumer; which app ships first is decided at G2 | **RULED 2026-11-06** — minNotes second; first to ship decided at G2, not before |
| 116 | Licences in the workspace | **REVISED 2026-11-07: the workspace is GPL-3.0-or-later** — every crate and every app — with ONE permissive exception, `min-paths` (+ its FFI), which stays MIT OR Apache-2.0 because it already shipped that way and costs nothing; the ring WIRE CONTRACT (the vendored `qcbae` C++ spine) stays MIT OR GPL-3.0 so QCBridgeN can carry it. Consequences: minColor is an ordinary crate (row 38's exception closes), SoundTouch (LGPL) links statically (row 140 closes), FFmpeg's GPL build raises no question, and the CI licence check is one rule: no MIT repo depends on the workspace | **RULED 2026-11-07** (user: "MinRender can go GPL 3.0"; "QCBridgeN has to stay MIT because of the NDI SDK, so will have to build that one custom with lessons from our crates and not link to them") — MinRender's LICENSE flip is the user's one-file commit in that repo; QCBridgeA (libajantv2 is MIT) follows MinRender to GPL by default unless the user keeps it permissive |
| 117 | Ecosystem pins | the workspace owns every pin in `[workspace.dependencies]`; one lockfile; **pinned 2026-11-06: wgpu 30.0.1, winit 0.30.13 (accesskit_winit 0.34 holds it; 0.31 beta for S5), vello 0.11, parley 0.12, accesskit 0.25** | **RULED 2026-11-06** — wgpu 30.0.1 holds; winit SPLITS: the window layer on 0.31 (typed data transfer, drag-out), the text layer on the vendored 0.30 patch while accesskit_winit holds 0.30 — separate crates, separate pins; the workspace converges on 0.31 once accesskit_winit follows, with S3's text-input patch carried to 0.31 as the first native-layer task; parley 0.12, accesskit 0.25, vello 0.11 stay |
| 118 | `min-paths` now | crate #1, consumed by ufb via cxx-qt in parallel with Phase 1 (optional) | **RULED 2026-11-06** — **NOW, not parked**: min-paths is used by every app in the family, so it is crate #1, built in parallel with Phase 2; ufb consumes it via cxx-qt |
| 127 | Phase 2 slice order | the §21.4 order — open → presenter (S1) → transport + timeline (S2) → rails/panels/popups/shortcuts/focus (§16) → scopes + guides (§19) → DnD (§14) → annotations over the frozen sidecar → the Adobe ring (S4) — ONE spike-grade step at a time, each with its own evidence note under `docs/phase2/` | **RULED 2026-11-06** ("sounds good to me") |
| 128 | Where Phase 2 lives | `apps/qcv` in this workspace; the spikes' proven pieces become real crates ONLY as the slice needs them, rewritten as crates (row 112: never renamed), each spike deleted once its crate meets the spike's own numbers | **RULED 2026-11-06** |
| 129 | Window + text layers in Phase 2 | row 117's split stands: winit 0.31 for the window layer, the vendored 0.30 text-input patch until accesskit_winit follows | **RULED 2026-11-06** |
| 130 | G1's carried Windows condition | Phase 2 proceeds on macOS; the Windows presenter check (pacing / EDR / PQ on a real display) runs when the user moves their HDR monitor to rhombus — offered 2026-11-06: "when we are ready I can flip over to windows and assign this monitor there" | **RULED 2026-11-06** |
| 131 | **Port QCView's basics, never reinvent them** | For every piece of the player, the renderer and the sidecar stack, the FIRST move is to read QCView's implementation of it (`QCView-Player/src/decode`, `render/metal`, `dual`, `annotations`) and carry its design — the publish slot + `fetchLatest`, `thread_policy.h`, `hw_routing.h`, `FrameIndex`, `ReadAhead`, the scrub cache, the texture-ring release rules, the YUV shaders' exact range constants, 4444 / rotation handling — with the spikes supplying only what QCView never had (wgpu / hal). Divergence needs a reason written next to the code. Step 1's two defects (8-bit range constants at every depth; an unbounded decode lookahead) were both wheels QCView had already built | **RULED 2026-11-07** (user: "some of these were part of the original QCView architecture … we should not be reinventing the wheel on the basics. We spent years building that"); step 1 AUDITED the same day → `docs/phase2/step1-qcview-audit.md`; **addendum 2026-11-07 (user): "improvements are welcome, but need to be tested and discussed, but we have a solid base to reference, so let's use it" — QCView is the reference base; every improvement over it is a measured, discussed divergence. Step 1b (`step1b-qcview-decoder.md`) re-derived the decoder side from QCView's files — **DONE 2026-11-07, GO**: `qcv-player` = `src/decode` module by module, stage 1 = `yuv_planar.h`, the CV bridge table; step 1's numbers intact or better (4K ProRes first frame 133 ms, 251 MB; slice threads: software ProRes 62 ms, EXR 68 ms); ProRes 4444 zero-copy (`y416`) and software both play, exact to their own decoders** |

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

---

## 22. `min-paths` — crate #1, the plan (surveyed 2026-11-06; rulings below)
**Why first (row 118, ruled):** every app in the family carries its own copy of the same path code, and the surveys found
them disagreeing. ufb (Rust, `core/src/utils.rs`, `core/src/file_ops.rs`, `bindings/src/services/file_ops.rs`) owns the
URL / uri-list / clipboard / same-volume / drop-onto-self code; minNotes (`app/core/PathMap.cpp`, `MediaStore.cpp`) owns the
`{vol,rel}` refs and the network / volatile classifiers; QCView (`project/project_manager.cpp`) owns the sequence detector;
MinRender (`core/path_mapping.h`) owns a third path-translation model. None of it has a shared test.

### 22.1 What the surveys found (facts, 2026-11-06)
- **Three path-mapping models.** minNotes: structured `{vol: ULID, rel: [segments]}` refs over an app-wide table
  `{id,label,win,mac,lin,enabled}` (QSettings `pathMappings/json`), matched against the current-OS root with a BOUNDARY check,
  resolved with forward slashes on every OS. ufb: string translation `translate_path_to(src_os, dst_os, path, mappings)` plus
  `vol:` / `native:` identities (`identity.rs`), case-insensitive on win, component-boundary matching. MinRender: string
  translation with canonical-Windows storage, **no boundary check** (`/Vol/share` matches `/Vol/share-extra`), and `\`→`/`
  even for POSIX sources (minNotes keeps a POSIX backslash literal — it is a legal filename byte).
- **No app identifies a volume by anything but the user-typed root strings.** OS probing exists only in classifiers.
- **ufb's `same_volume` on POSIX compares the first path component** — every absolute path is "same volume" (the comment blames
  stable Rust for lacking `dev`; `std::os::unix::fs::MetadataExt::dev` has been stable for years). Windows compares
  drive-or-UNC. Consequence today: a macOS drag between two volumes in ufb defaults to MOVE.
- **`is_drop_onto_self` has no descendant guard** (a folder dropped into its own subfolder is not caught).
- **`file_url_to_path` misses `file://host/share` (→ `server\share`, no leading `\\`) and `file://localhost/`** (both OSes);
  `path_to_file_url` is right (four-slash guard, host form for UNC, drive letters) and tested. Three separate percent-decoders
  (bindings, macos_mounts, agent fallback) with different malformed-escape rules; QCView's and minNotes' QML drag sources
  build file URLs with **no percent-encoding at all**; MinRender's picker seed handles neither UNC nor POSIX.
- **uri-list:** ufb encodes CRLF after every entry incl. the last (Qt drops the last URL otherwise); decoding is duplicated in
  two QML files (keeps the longer of `drop.urls` vs the raw `text/uri-list`, Qt 6 truncates `drop.urls` to one entry).
- **Clipboard file lists** are entirely in ufb's Rust: per-item `public.file-url` via objc2 (`writeObjects:` of NSURLs — the
  old `NSFilenamesPboardType` collapsed to one item), CF_HDROP + `Preferred DropEffect` via clipboard-win, probe with
  `IsClipboardFormatAvailable` only (delayed rendering can hang the GUI), a cut-record registry keyed on the pasteboard
  change count / clipboard sequence number. None of it has a test.
- **Sequence detection exists only in QCView — twice, disagreeing.** `ProjectManager::detectImageSequence`: stem split by
  `^(.+)([_.\-])(\d+)$` (last separator wins) or `^(.+?)(\d{3,})$` (no separator, only if the first fails); **< 3 digits
  → not a sequence**; glob siblings, re-check case-insensitively, **identical padding required**, **≥ 2 siblings**, range =
  min..max with MISSING frames listed, pattern = `base sep %0Nd . ext` (`%`→`%%`). The dual-mode `DualImageSeqSource` takes
  the LAST digit run anywhere, no minimum, and re-indexes 0..N−1 (gaps collapsed). MinRender's preview sweep converts `#`
  runs (and AE's `[####]`) to `(\d+)`. ufb has no detector.
- **Classifiers:** minNotes `isNetworkSource` (win: UNC or `GetDriveTypeW == DRIVE_REMOTE`; POSIX: `QStorageInfo` fs type
  contains smb/afp/nfs/cifs/dav/sshfs/ftp; unknown → "not a share", the safe side is copying) and `isVolatileSource` (temp
  dirs, `~/Library`, `/var/folders`, `%TEMP%` `%LOCALAPPDATA%` `%APPDATA%`, the scratch dir; both the absolute and the
  canonical path tested because `/tmp` is a symlink). ufb has the SMB mount-table parser (`mount` output on macOS, WNet on
  Windows) duplicated between core and the agent. MinRender's `shareHostForPath` does `statfs().f_mntfromname` on macOS.
- **Long paths:** only minNotes `FileLinks.cpp` (`GetFinalPathNameByHandleW`, strips `\\?\UNC\` / `\\?\`) and ufb
  `external_path` do it; nobody handles Windows reserved names (CON, PRN, NUL, COM1…); ufb's `web_link_file_name` is the
  only filename sanitizer; QCView's `sanitizeMediaName` is the sidecar's (frozen, `qcv-notes`' business).
- **Family links:** `ufb:///{win|mac}/<percent-encoded>` is built in minNotes (`ufbLinkFor`, tested) AND ufb
  (`build_path_uri`); `qcview://<project>/<media>` and `minnotes:` are parsed in their apps.
- **Tests:** ufb has ~60 unit tests (URL building, translation, identity, mount parsing, share helpers); nothing covers
  `file_url_to_path`, uri-list, `same_volume`, drop-onto-self or the clipboard. minNotes covers `ufbLinkFor`, `toRef`,
  `anchorPathFor` in the regression suite. QCView and MinRender have no path tests.

### 22.2 The crate
`crates/min-paths`, MIT OR Apache-2.0 (row 116), no UI, no IO beyond `std::fs::metadata` / `statfs` / Win32 volume queries, no
async. Modules:
| Module | Contents | Source of truth |
|---|---|---|
| `url` | `path_to_file_url` (ufb's: four-slash guard, UNC → host form, drive letters), `file_url_to_path` (+ `file://host/share` → UNC, `file://localhost/`), ONE percent codec (RFC 3986 unreserved + `/ :`; malformed escapes kept), `uri_list::{encode, decode}` (CRLF incl. last; `#` comments; blank lines) | ufb `utils.rs` + the two QML decoders |
| `volume` | `same_volume(a, b)` = device identity (`st_dev` on unix; volume serial via `GetFileInformationByHandle`, falling back to drive-or-UNC for paths that do not exist yet); `is_drop_onto_self(src, target)` + the DESCENDANT guard; `external_path` (long-path prefix strip) | ufb, corrected |
| `classify` | `is_network_source` (UNC / `DRIVE_REMOTE` / `statfs f_fstypename` ∈ {smb, afp, nfs, cifs, dav, sshfs, ftp}; unknown → false), `is_volatile_source` (minNotes' roots, both the absolute and the canonical path), `share_host_for_path` (MinRender's) | minNotes `MediaStore.cpp`, MinRender `net_utils.cpp` |
| `map` | ONE table shape `Mapping {id, label, win, mac, lin, enabled}`; `to_ref(abs) → Ref::{Vol{id, rel}, Abs(String)}` and `resolve_ref(Ref) → Option<path>` (minNotes' rules: boundary check, list order, forward-slash resolve, disabled/unknown → None); `translate(src_os, dst_os, path, &[Mapping])` (ufb's / MinRender's string form, WITH the boundary check, POSIX backslash literal); `canonical_windows_form` for MinRender | minNotes `PathMap.cpp` for refs, ufb `translate_path_to` for strings |
| `sequence` | ONE `detect(path) → Option<Sequence {dir, base, sep, padding, ext, start, end, missing, pattern_printf, pattern_hashes}>` with QCView's `detectImageSequence` rules exactly (last-separator-wins, ≥ 3 digits, identical padding, ≥ 2 siblings, case-insensitive siblings, missing frames); `frame_path(n)`; `hashes_to_printf` / `printf_to_hashes` (`####` ↔ `%04d`, AE `[####]`); the directory listing is INJECTED (a `&[name]` slice), so the detector is pure and testable | QCView `project_manager.cpp` |
| `clipboard` | file lists in/out: objc2 per-item `public.file-url` (reads `isFileURL` incl. legacy plists / `furl` / `file://localhost/`), CF_HDROP + `Preferred DropEffect`, `has_paths()` by `IsClipboardFormatAvailable` only, `PasteIntent::{Copy, Move}` + the cut-record registry; text fallback (`looks_like_path`) | ufb `file_ops.rs` (moved, not rewritten) |
| `names` | filename sanitizer (ufb's rules), Windows reserved names (CON PRN AUX NUL COM1–9 LPT1–9, with any extension, trailing `.`/space), `web_link_file_name` | ufb + new |
| `links` | `ufb_link(path) → ufb:///{win|mac}/…`, `parse_ufb_link`, `minnotes_link`, `qcview_link` builders/parsers (row 52's `text/plain` link line) | minNotes `FileLinks.cpp`, ufb `build_path_uri`, QCView `main.cpp` |
OUT of the crate: the `.qcview` sidecar layout and `sanitizeMediaName` (frozen format → `qcv-notes`), the SMB mount/unmount
machinery and the agent (ufb's `macos_mounts` / `windows_mounts` stay in ufb; the crate only reads the mount table for
classification), `.lnk` / alias resolution (row 61's file-ops layer), `${PROJECT_DIR}` collapse (QCView's project format).
**Tests:** every ufb test ported; new tests for everything the survey found untested (`file_url_to_path` incl. host /
localhost forms, uri-list round trips with CR, LF, CRLF, comments and a missing final CRLF, `same_volume` across a real
second volume where the CI box has one, `is_drop_onto_self` incl. descendant, the sequence rules as a table of QCView's
documented cases — `logo_v2.png` no, `shot_1` vs `shot_001` no, `A.0001.exr` + `A.0002.exr` yes with `%04d`, gaps listed).
**FFI:** a C ABI (`min_paths.h` via cbindgen, UTF-8 in / caller-freed strings out) for the Qt apps if wanted before the port;
ufb consumes it natively (it is a Rust crate already) through its `bindings` crate, replacing its own copies in place; the
QML decoders become one `FileOps.decode_uri_list` call.
**Time:** one week for the crate + tests; the ufb adoption is its own session in the ufb repo (its suite is the gate).
**Built 2026-11-06** (`crates/min-paths`, README there): the eight modules above, 67 unit tests (ufb's ported + the untested areas covered), green on macOS and on rhombus under MSVC; `tools/min-paths/seqdiff` slices QCView's `detectImageSequence` read-only, builds it against Qt 6.11 and runs both over 28 listings — IDENTICAL, quirks included (`0001.exr` → base `0`, padding 3; `_001` → empty separator; a glob metacharacter in the base; identical-padding and ≥ 3-digit rejections; gaps). Not built yet: the C ABI (cbindgen) — it lands with minNotes' adoption, the second consumer. **ufb ADOPTION 2026-11-06 (row 125, first consumer):** branch `min-paths` in the ufb repo (commit 6ac2204, NOT pushed — the user's call): ufb's copies of `path_to_file_url` / `to_uri_list` / `urls_to_paths`, `translate_path_to` (+ boundary check), `to_native_path`, `external_path`, the whole clipboard file-list block and cut registry, `web_link_file_name`, the drop guard, `same_volume`, macos_mounts' percent decoder and the two QML uri-list parsers (now `FileOps.decode_uri_list`) replaced by the crate as a PATH dependency (`../../minplatform/crates/min-paths`; a git dependency once minplatform is public); 873 lines out, 80 in; ufb's workspace suite green (117 core + bindings); CHECKS DONE 2026-11-06: the ufb macOS app builds in `build/mac-debug` (binary launches; the single-instance gate handed off to the user's running UFB, so the UI walk is the user's), `FileOps.decode_uri_list` is in the generated qmltypes, and ufb's workspace suite passes on rhombus under MSVC (111 core + bindings, worktree `~/Documents/GitHub/ufb-minpaths`). Merge + push of the ufb branch = the user's call. **minNotes ADOPTION 2026-11-06 (second consumer, through the C ABI):** `crates/min-paths-ffi` (staticlib + cbindgen `include/min_paths.h`, 24 `mp_*` functions: the mapping table + refs + translate, classifiers, URLs + uri-lists, links, names, volumes, sequences; UTF-8 in, `mp_free` out); minNotes branch `min-paths` (e545be4, NOT pushed): `cmake/MinPaths.cmake` has cargo build the library from the sibling checkout (`MINPLATFORM_DIR`), `app/core/MinPaths.h` is the RAII edge, `PathMap.cpp` feeds the crate's table and keeps minNotes' API, `MediaStore`'s classifiers and `FileLinks` call the ABI, the two hand-built QML file URLs go through a new `pathMap.fileUrl` (percent-encoded — row 124); minNotes' regression suite ALL CHECKS PASSED on macOS (regression 129 caught that the ufb link must encode `:` like both older apps — fixed in the crate, 5c5eaac). WINDOWS CHECK DONE 2026-11-06 (rhombus, after the user installed Qt PDF): `cmake/MinPaths.cmake` ran cargo inside the Ninja build, the link with the system libs + the cargo registry's `windows.0.53.0.lib` held, `minNotes.exe` + `mn_regression_test.exe` built, the suite ALL CHECKS PASSED under MSVC. (Rhombus's `external\ffmpeg` junction had died — recreated with `mklink /J` to QCView's `ffmpeg-win64`.) The adopted minNotes is proven on both OSes; merge + push of the branch = the user's call. **MinRender ADOPTION 2026-11-06 (third consumer, through the C ABI):** branch `min-paths` in the MinRender repo (735d73d, NOT pushed): `cmake/MinPaths.cmake` (same shape as minNotes'), `translatePath` keeps its signature and delegates to the new stateless `mp_translate_with` with its rows as JSON (MinRender's rows have no id — fine), `shareHostForPath` and `urlToLocalPath` delegate, the submission form's picker seed goes through `appBridge.localPathToUrl`; THIRD_PARTY_NOTICES lists the crate. Both targets (`minrender`, `minrender-headless`) build on macOS; MinRender has no suite, so a probe compiled against its real `path_mapping.h` + the static library ran 10 cases (boundary, case-insensitive Windows source, bare-drive mapping, same-OS idempotence, the canonical round trip) — all pass. WINDOWS DONE 2026-11-06: both `minRender.exe` and `minrender-headless.exe` build on rhombus under MSVC in a separate `build-minpaths` dir (cargo inside Ninja; the farm node untouched). Lesson folded into the crate: the committed C header now regenerates only with `MIN_PATHS_REGEN_HEADER=1` (cbindgen on Windows rewrote it slightly and dirtied the clone, blocking a fast-forward) and a test checks it is current. **Three consumers adopted: ufb (native), minNotes + MinRender (C ABI); QCView last, on its own release.** **RULING 2026-11-06 (user): the adoption branches are PUSHED as branches (ufb `min-paths` 6ac2204, minNotes `min-paths` e545be4, MinRender `min-paths` 735d73d, all on origin) and STAY UNMERGED from `main` "for a long time", until the plan has earned confidence; the apps keep shipping from their own copies meanwhile. QCView's adoption follows on its own `min-paths` branch under the same rule (last of the four, row 125).** **QCView ADOPTION 2026-11-06 (fourth and last consumer, through the C ABI):** branch `min-paths` in the QCView repo (a139008a, pushed, unmerged): `cmake/MinPaths.cmake` (target `qcview::minpaths`, linked by `qcv_project`, `qcv_window`, `qcv_annotations`); `ProjectManager::detectImageSequence` keeps its signature and delegates to `mp_sequence_detect` (its own rules; `DualImageSeqSource` untouched — row 120; `tools/min-paths/seqdiff` now reads its C++ reference from the PINNED pre-adoption revision cb890143 via `git show`, so the working tree's delegation cannot erase the reference — 28/28 still IDENTICAL); `WindowManager::urlToOsPath` decodes through the codec (QCView's forward-slash convention on Windows kept by `QDir::fromNativeSeparators`; live `srt://` / `qcbae://` URIs pass through as before; the ONE deliberate behaviour change: `file://localhost/…` is the local file — Qt's old answer was the UNC-shaped `//localhost/…`), new `Q_INVOKABLE osPathToUrl` replaces LeftRail's drag-mime and NotesPanel's thumbnail-source string concatenations (row 124: percent-encoded now — a `#` in a name no longer breaks the Image source), the HTML export's `<img src>` file URLs likewise; `buildProjectLink` / `openProjectLink` / `openNotesInMinNotes` use the crate's `qcview://` + `minnotes:` codecs (row 126; two new ABI entries `mp_qcview_project_link` / `mp_parse_qcview_link`; the stream form's chain keys stay QCBridge's QUrlQuery contract); THIRD_PARTY_NOTICES lists the crate (taken under Apache-2.0). `qcview` BUILDS on macOS (`cmake --build build --target qcview`, cargo on PATH); QCView has no path tests, so a Qt probe against the real ABI compares QCView's OLD expressions with the crate: 37 checks, 0 failures (the localhost case asserted as the deliberate difference; the stream link's url/name equal the old `QUrlQuery` decode). WINDOWS DONE 2026-11-06: `qcview.exe` builds on rhombus under MSVC in a separate `build-minpaths` dir (`C:\qcb-lab\qcv-minpaths-win.ps1` — the windows-release preset's cache values by hand, vcpkg's manifest install reused from `<src>\vcpkg_installed`, cargo inside Ninja, BtbN FFmpeg staged); rhombus's QCView checkout put back on `main` afterwards (the public app), the build dir kept. **Row 125 complete: all four consumers adopted, each proven on both OSes, all on pushed, unmerged `min-paths` branches.** **PUBLIC FLIP 2026-11-06 (the user flipped cbkow/minplatform public): the four path dependencies became PINNED git dependencies on the same branches — ufb's two Cargo.toml rows `min-paths = { git = "https://github.com/cbkow/minplatform.git", rev = <sha> }`; the three `cmake/MinPaths.cmake` files FetchContent the workspace at `MINPLATFORM_REV` into `<build>/_deps/minplatform-src` by default (a commit hash cannot be fetched shallowly; no root CMakeLists, so populate only) and cargo builds there, with `-DMINPLATFORM_DIR=<checkout>` as the opt-in override for working on the crate itself (the pin then does not apply). Bumping a consumer = editing one revision string. VERIFIED 2026-11-06 with the sibling override UNSET (`cmake -U MINPLATFORM_DIR`), on macOS AND on rhombus/MSVC: ufb's suite green from the git source (117 core on macOS, 111 on Windows; `Cargo.lock` records `git+…?rev=1d48ee4`), minNotes builds + regression suite ALL CHECKS PASSED on both, MinRender's two targets and QCView's `qcview` build on both; every `_deps/minplatform-src` sits at 1d48ee4. Branch heads: ufb b0662f7, minNotes d3f1e8e, MinRender 9526557, QCView 9500e643 (pushed, unmerged).** **PHASE 2 STEP 1 STARTED 2026-11-06 (rows 127–130 ruled "sounds good to me"): `docs/phase2/README.md` + `step1-open-present.md` (question, pass bar, evidence). The first real crates exist — `crates/qcv-timeline` (rational time), `crates/min-present` (the fused four-stage graph with the `ColorPass` slot, zero-copy imports via hal, SDR / EDR / PQ surfaces, headless engine + tests), `crates/qcv-player` (FFmpeg 9 RAII, `HwDevice` shared with the presenter, the streaming `Slot`, the rational-clock `Player`, `Source` with the image-sequence detector) — and `apps/qcv` on winit 0.31 (the workspace's `winit31` pin; `use winit31 as winit;` until the split ends). Found on the way: S1's stage 1 used 8-bit range constants at every depth (10-bit white 0.9969, a +0.9/255 tint on grey); `min-present` derives them per depth, and qcv's frame agrees with QCView's capture at 59.2 dB vs the spike's 54.7 (same sampler), the PQ arithmetic exact to the fixture's codes. **STEP 1 VERDICT 2026-11-07: GO** — on the awake screen every row held: 4K ProRes zero-copy first frame 197 ms, present p95 17.0, lateness p95 15.7, 0 dropped, cadence 2/3, **the fused frame 0.48 ms p95 GPU at 4K (the bar S1 missed at 1.9)**, 11 % of a core, 272 MB (S1's number, after a second defect the numbers caught — the player drained the decoder without bound: 19.7 GB on software ProRes — was fixed with a 3-frame lookahead), EDR headroom 9.31, the PQ surface via the layer name, the EXR sequence from one frame at cadence. Windows: builds, D3D12VA zero-copy through the crate bit-identical to software decode, the virtual display idles at ≈ 4 Hz with no session attached (the S1 spike too), so the Windows display rows wait for the monitor (row 130). Carried into step 2: H.264's first frame 228–239 vs S1's 195 (the scrub decoder answers first frames in step 2), the software upload's staging blit (row 46's hal wrap). Next = step 2: transport + timeline.** ufb's `build_path_uri` / `parse_path_uri` and the agent's duplicates stay (the agent is a separate workspace with no dependencies by design).

### 22.3 Rulings needed
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 119 | `same_volume` semantics | device identity (`st_dev` / volume serial), not the leading component; BEHAVIOUR CHANGE in ufb on macOS: a drag between two volumes defaults to COPY (today: MOVE, by the heuristic's bug) | **RULED 2026-11-06** ("go, build it in minplatform") |
| 120 | The one sequence detector | QCView's `detectImageSequence` rules verbatim (≥ 3 digits, last separator wins, identical padding, ≥ 2 siblings, missing frames listed, never re-indexed) — the PUBLIC import path is the reference; outputs byte-compatible with QCView's (`%0Nd` pattern, base label, 0-based missing list) and proven by a DIFFERENTIAL test: a one-off harness in `tools/` compiles QCView's `detectImageSequence` read-only (as S4 did with the vendored ring code) and runs the crate and the C++ over the same synthetic listings; the dual-mode detector's "last digit run anywhere + gaps collapsed" is NOT carried — QCView's dual mode keeps its own code untouched until QCView adopts the crate | **RULED 2026-11-06** ("go, build it in minplatform") — steer 2026-11-06: defer to QCView |
| 121 | The one mapping table | the shared shape `{id, label, win, mac, lin, enabled}` (ufb's and MinRender's roots + minNotes' id) serves both refs and string translation; boundary check everywhere (ufb and minNotes have it; MinRender's lack of one is a bug, fixed on adoption); backslash handling defers to the OLDER apps (ufb, MinRender): `\` is treated as a separator when COMPARING against a mapping root on every OS, the stored/resolved path is never rewritten — minNotes' literal-backslash rule (the newest app's) is dropped; MinRender's rows get ids on adoption | **RULED 2026-11-06** ("go, build it in minplatform") — steer 2026-11-06: defer to the older apps |
| 122 | Classifier rules | minNotes' network + volatile rules are the reference (unknown → not a share → copy); `statfs` on macOS instead of QStorageInfo, same fs-type list | **RULED 2026-11-06** ("go, build it in minplatform") |
| 123 | Reserved names + long paths | in the crate as `names::is_reserved` + `volume::external_path`; ufb's sanitizer gains the reserved-name rule (today a file named `con.txt` can be created on a share and never opened on Windows) | **RULED 2026-11-06** ("go, build it in minplatform") |
| 124 | Percent codec | ONE codec; encode = RFC 3986 unreserved + `/` + `:` (ufb's), decode keeps malformed escapes; the QML / JS URL builders in minNotes and QCView that encode nothing are bugs fixed on adoption | **RULED 2026-11-06** ("go, build it in minplatform") |
| 125 | Consumers and order | ufb FIRST (native, its own session, its suite is the gate — the sanctioned exception to "never modify the sibling Qt repos", limited to replacing its copies with the crate); minNotes SECOND through the C ABI (one user, lowest risk); MinRender third; **QCView LAST** — the public app adopts only on its own release, with row 120's differential test green and its dual mode untouched; `min-ui` natively | **RULED 2026-11-06** ("go, build it in minplatform") — steer 2026-11-06: QCView last; **addendum 2026-11-06: every adoption lives on a pushed `min-paths` branch, none merged until the plan has earned confidence (the user's call)** |
| 126 | Family links | `links` holds the `ufb:///`, `minnotes:`, `qcview://` codecs so row 52's `text/plain` link line is built by one function in every app | **RULED 2026-11-06** ("go, build it in minplatform") |

---

## 23. Metadata — the hole in §21.4 (surveyed 2026-11-07; rulings proposed below)
**The gap.** §21.4 names open → presenter → transport → panels, but not the layer every one of them reads: QCView's
`MetadataService` (an async orchestrator with latest-wins generations per item), `FFmpegMetadataExtractor` (861 lines:
container identity incl. the MXF operational pattern, the display-ready codec flavours — ProRes marketing names from the
fourcc, DNxHD / DNxHR bandwidth names from the frame header's compression ID × fps — pixel format / bit depth / alpha,
SAR via `av_guess_sample_aspect_ratio`, the tkhd rotation, colour tags + NCLC + HDR static metadata (MaxCLL / MaxFALL /
mastering peak), the effective range, audio across ALL streams (Premiere's 8 mono tracks are 8 channels, not "mono"),
the `timecode` tag, camera vendor / model for unsupported-codec toasts, bitrates, encoder tool; plus the header-only
re-probes that migrate older `.qcvproj` caches field by field so an offline volume never clobbers good cached metadata),
and `AdobeMetadataExtractor` (the bundled exiftool, 200–500 ms per file: the Adobe project links + the extended
timecodes that feed the Inspector's Timecode origin picker and "open in After Effects / Premiere"). Consumers: the
Transport (timecode origin, drop-frame, the frame count before decode), the Inspector (1 892 lines of QML over these
structs), the project cache, the playlist's placeholder durations, the unsupported-media toast — and, across the family,
ufb's columns + thumbnails (today ffprobe / exiftool CLIs, §15), minNotes' video cards, MinRender's preview sweep.

**What FFmpeg already gives in-process (verified on the fixtures and the client masters, 2026-11-07):** everything the
FFmpeg extractor reads, AND the whole XMP packet of MOV / MP4 files (`mov` demuxer option `export_xmp`: the Adobe XMP
Core packet with `xmpDM:startTimecode` / `altTimecode` / `videoFrameRate`, `xmpMM` history, `xmp:CreateDate` — and the
Premiere / After Effects project-link structs when the render wrote them), the tmcd track's `timecode` tag, per-stream
`creation_time`. The MXF demuxer exports the operational-pattern UL and the timecode but NOT XMP.

**Rust substitutes, assessed:** `xmp_toolkit` 1.12 (Adobe's own XMP SDK, MIT OR Apache-2.0; a C++ build like OCIO's;
XMPFiles has no MXF handler) · `xmpkit` 0.1.6 (pure Rust XMP, young) · `quick-xml` (an RDF/XML reader for the dozen keys
we want: ~100 lines) · `nom-exif` 3.8 (EXIF / HEIF / MOV stills + video tags; licence to verify) · `kamadak-exif` (BSD) ·
`filmcraft-mxf` 0.9 / `smpte-mxf` (MXF KLV readers, MIT OR Apache-2.0 — a route to Adobe's XMP KLV inside MXF if that
workflow matters). No crate replaces exiftool wholesale; nothing needs to — FFmpeg carries the packet for MOV / MP4.

### 23.1 Rulings needed
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 132 | Where metadata lives | a FAMILY crate `qcv-meta` (MIT OR Apache-2.0, C ABI like `min-paths`): QCView's `VideoMetadata` / `AdobeMetadata` / `ImageSequenceData` structs and the FFmpeg extractor PORTED verbatim (row 131: the codec-flavour tables, the SAR / rotation / range / HDR rules, the all-streams audio rule, the migration probes), the async service with latest-wins generations on a thread pool; consumed natively by `apps/qcv`, through the ABI by ufb (replacing its ffprobe / exiftool CLIs), minNotes and MinRender later, on unmerged branches as min-paths was | **RULED 2026-11-07** ("this makes sense, let's add to the plan") — **BUILT 2026-11-07** (`crates/qcv-meta`; the C ABI with the first non-Rust consumer) |
| 133 | When | **step 2** (the Transport's timecode origin + drop-frame + frame count need it before the UI exists), ahead of the Inspector (step 3 consumes it) — §21.4's slice order gains the line "open → **probe** → presenter"; the project-cache migrations come with the project file (step 3) | **RULED 2026-11-07** ("this makes sense, let's add to the plan") |
| 134 | exiftool | **retired for MOV / MP4**: the XMP packet from FFmpeg's `export_xmp` parsed by a small `quick-xml` reader for exactly exiftool's dozen keys (the `aeProjectLink` namespace, `xmpDM`'s `macAtom` / `windowsAtom` project-path structs, `startTimecode` / `altTimecode`, create dates), verified against an AE render and a Premiere render before the Perl bundle goes. **MXF, measured on the user's own corpus 2026-11-07 (1 413 MXFs indexed on the Mac, 40 sampled across it): every one written by Premiere Pro or Media Encoder, NONE carries an XMP packet or a project link** — exiftool's Adobe role on MXF is nothing. What exiftool DOES read from them is ~53 structural tags (row 137). exiftool stays bundled as the MXF scanner and the parity oracle until row 137's reader matches it, then goes | **RULED 2026-11-07** (revised after the MXF survey; "this makes sense") — **BUILT 2026-11-07 for MOV / MP4** (the reader EQUAL to exiftool on the masters' keys) |
| 135 | Stills metadata (EXIF / HEIF / camera RAW, EXR headers) | `min-image`'s job (row 34), not `qcv-meta`'s: `kamadak-exif` (BSD) or `nom-exif` after its licence is checked; EXR channels / layers / chromaticities from the EXR crate's header | **RULED 2026-11-07** ("this makes sense, let's add to the plan") |
| 136 | The timecode formatter | `TimecodeFormatter` ported verbatim into `qcv-player` with the Transport (step 2): drop-frame maths, the DF `;` signaller, `parse` / `format`, the start-frame offset; `qcv-notes`' minting rules stay the sidecar's (frozen) | **RULED 2026-11-07** ("this makes sense, let's add to the plan") — **BUILT 2026-11-07** (in `qcv-meta`, re-exported by `qcv-player`) |
| 137 | **The MXF scanner** (the user 2026-11-07: "we do need to scan MXFs for more than just Adobe metadata") | FFmpeg's `mxf` demuxer already exports the identification set (company / product / version / platform / modification date / UIDs), the operational-pattern UL, the timecode, the material-package UMID and name, per-stream file-package name / UMID and track name, and the essence descriptors as `codecpar`. **The gap vs exiftool's MXF group on the user's files:** `VideoCodingSchemeID` (the essence-coding UL = the true flavour), `VideoLineMap` (field order / line positions), sampled vs display dimensions + offsets (the crop), the stored `PresentationAspectRatio`, the timecode component's `DropFrame` + `RoundedTimecodeTimebase`, per-track `EditRate` / `Origin` / `Duration`, `AudioReferenceLevel` / `DialNorm` / `LockedIndicator` / `BlockAlign`, per-package dates — and, when present in other people's files, DMS-1 / AS-11 DPP shim / Avid / camera descriptive sets. Default: a Rust header-metadata reader inside `qcv-meta` — KLV + primer pack + local sets resolved through the SMPTE registers (ST 335 / 395 / 2001-1) — with `regxmllib-rs` as the standards-based donor (`smpte-types` / `smpte-klv` / `smpte-mxf` / `regxml-dict`, MIT, a 2026 port of SMPTE's reference RegXML implementation: young, 13 commits, an 8-file golden suite against the Java reference, the register XMLs in the repo) — vendored and audited, not depended on blindly. **Acceptance:** field-for-field parity with exiftool's MXF group across a sample of the user's corpus, then the Avid / DMS sets on files that have them. Scheduled after step 2's `qcv-meta` core and before the Inspector's MXF section (step 3) | **RULED 2026-11-07** ("this makes sense, let's add to the plan") — **BUILT 2026-11-08** (`crates/qcv-mxf`, `docs/phase2/step2d-mxf.md`): ExifTool's MXF reader carried rule for rule, its table generated from `MXF.pm` (so the crate is GPL-3.0-or-later, `qcv-meta` stays permissive; `regxmllib-rs` not needed); **1 413 / 1 413 of the user's MXFs + 7 / 7 synthetic FFmpeg MXFs IDENTICAL to ExifTool's MXF group**, 0.2 ms a file vs 63 ms; the Inspector's MXF section is the next talk |


---

## 24. The full-app audit of QCView (2026-11-07) — the holes in §21.4, surveyed from the code
**Why.** Phase 1 read QCView for the viability spikes and §21.4 named the slice from that reading. Four read-only surveys
(audio / output / dual / live; UI / window / app; project / annotations / timeline; colour / render / tools / packaging)
walked the whole tree (~90 k lines of C++ / ObjC++ / QML plus docs/ and ideas/) against what the canon covers. The
numbers and rules below are the surveys' readings of the headers; port from the CODE, not the comments (several dual/
headers are stale — they say no hw decode and RGBA16F EXR where the .cpp has both).

### 24.1 Coverage map
| Subsystem (lines) | What is there | Plan |
|---|---|---|
| Decoders, index, read-ahead, hw devices (`decode/`, 12.6 k) | ported in step 1b; Windows: QCView routes ProRes / FFV1 / APV to a decode-only Vulkan device (ProRes on the GPU), inter codecs to D3D11VA; one shared Vulkan YUV pipeline (per-stream pipelines stuttered dual on Intel Arc) | COVERED (D3D12VA); **Vulkan ProRes NAMED-ONLY** (row 45) |
| Image loaders + sequence cache (`decode/image_*`, `exr_*`, `image_sequence_cache`, ~3.5 k) | OpenEXR multi-part by layer name, a parallel mode (4K DWAB 225 → 56 ms on 8 threads), scanline-skip thumbnails, one-pass all-layer contact sheet, mmap with sequential / prefetch hints; files < 15 KB = broken (mid-grey #787878), gaps transparent; cache tick 10 ms, a jump of > +20 / < −2 frames = a seek, 16 IO workers, ≤ 32 requests, failed frames never retry, ~3 s ahead / 0.5 s behind, byte budget from the real frame size, GPU push / evict callbacks → the upload thread; PNG 16-bit byte swap, TIFF must not be swapped, float TIFF = broken, the JPEG fast thumbnail path never ported | **NOT IN PLAN** (row 34 `min-image` is "later"; S2 played EXR through FFmpeg's decoder, which has no layers / parts) |
| Audio (`audio/`, 6.8 k) | VIDEO is the master, audio follows: a PI servo trims audio speed ±0.2 % (≈ 3.5 cents) inside ±20 ms of drift, opening linearly to ±1.5 % at 100 ms, anti-windup + slew; soft re-seek with cooldown between ~100 ms and 1 s, immediate above 1 s; every break gets a 5 ms fade; the ring marks a "stale" boundary instead of clearing (SPSC); Catmull-Rom resampler with 5 frames of history (the 3-frame version lost a sample per block: a 100 Hz crackle); ring ≈ 500 ms, target 100 ms; fixed f32 stereo 48 kHz; multi-stream decoder (≥ 2 streams: one codec each + an FFmpeg merge / pan / format graph swapped atomically, ~21 ms dropped), routing Auto / 5.1 → stereo (BS.775, 0.707 C / Ls / Rs, LFE dropped) / channels 7–8, meters per real channel before the fold; tempo = SoundTouch WSOLA (**LGPL-2.1**, tag 2.4.1), 0.25–4×, 1.0 = byte-identical bypass, re-seek after a change; shuttle = 80 ms grains, pitch follows speed below 1× and stays natural at ≥ 1×, snaps when off by > max(0.25 s, 0.25 × speed), reverse grains reversed one by one; CoreAudio default output only (no picker), WASAPI shared / event-driven / MMCSS Pro Audio; no Linux; waveform strip: peaks per screen column memoised on a power-of-two stride ladder, exact ≤ 60 s, 20 ms probes beyond, stops while the transport runs; live audio: a user-set jitter buffer (Premiere sends 21 ms packets), drop one when 2 over target | **NAMED-ONLY** — no step, no design |
| Timeline (`timeline/`, 3.7 k + `TimelinePanel.qml` 3.2 k) | the model is S2's; the EDIT set is undo commands one per gesture (trim head / tail, slide, slip, split, delete without ripple, insert, move; insert / move recompute the track gapless; drag preview edits the clip directly, Esc restores the snapshot; `snapTolPx` 12; a clip's duration is patched when metadata lands after the drop only if untrimmed; the stack clears on leaving dual); timers WallClock / FrameLocked (sequences FrameLocked), loop wraps with fmod, in / out belong to the view and are off in playlist mode; hover cache 320×180, LRU 100 (≈ 23 MB), 3 workers idle / 1 playing, prefetch stride 6, ≤ 64 uploads per frame, a clear-generation counter, EXR keys `?layer=`, nearest cached frame on a miss, the QML request throttled 16 ms; 20 px ruler, 44 px lanes, cache band, red ≥ 1-px ticks for failed / missing frames (not in dual), note diamonds | model COVERED; **edits + undo, hover cache, bands, timers NOT IN PLAN** |
| Project (`project/`, 5 k) | `.qcvproj` `schema:"qcview.project.v2"`, atomic QSaveFile, `${PROJECT_DIR}/rel` with a separator check, `://` untouched, six FIXED bins (Videos open, Audio, Images, Playlists, Dual Views, Live) routed by type, migrations (no bins → the six; pre-2.2.3 → Live bin; dangling ids dropped; header-only re-probes for rotation −1 / empty container / old ProRes RAW / old DNx names), dedupe keys (file = absolute path, sequence = folder + pattern, SRT = exact URL), a duplicate add still bumps Recents, `.lnk` resolved first, batch delete = one-step undo, playlists `Playlist-YYYYMMDD-HHMMSS` / `Playlist_NN` at 24 fps 1920×1080 gap 10 (gaps transparent), entries `{mediaId, in, out}`, DualPair keeps both layouts (B = a media-id reference), a still = a 1-frame sequence with `%` → `%%`, the extension table (32 video incl. webp / gif / dpx; 13 audio; 6 image; .r3d / .braw / .ari refused on purpose, ARRIRAW-in-MXF badged unsupported); the docs say three bins, the code has six | **NOT IN PLAN** |
| Annotations (`annotations/`, 4.2 k) | sidecar + ink + modeler COVERED; the stroke JSON (`version 2.0`, `coordinate_system normalized`, shapes freehand / rect / oval / arrow / line, v1.0 loads unmodeled, ids `stroke-<ms>-<rand>`), tools + defaults (width 6 on macOS / 4 elsewhere; modeler 0..1000 scale, wobble 0.04 s / 31 / 620, spring 11 / 32400, drag 72, prediction off; arrowhead 12 + 2 × width at 25°; points unclamped, drawing clipped; a stroke that never touches the picture is dropped; Select / Eraser never saved; a drawing tool cancels the debounced annotated-thumbnail save), the EXPORTERS: Markdown (a folder `{media}-yyyyMMdd-HHmmss/` with notes.md + images/, `## TC (frame N) ✅`), HTML (one file, base64 images, 1600 / 780 px columns, system-ui, h1 22 / h2 15, 64-px brand), PDF (Letter, 15 mm, 150 dpi, cover + a note per page, image ≤ 60 % of the page, explicit no-page-break on every block), DOCX (hand-written OOXML, 914400 EMU/in, 6-in images, Heading2 + image + text + page break); a note with a missing image is skipped; stream notes headed by capture time | **exporters, stroke JSON, tools NOT IN PLAN** |
| Colour (`color/`, 8.7 k) | OCIO config inventory: `$OCIO`, then bundled Blender 5.2 (default, first) / 5.1 (presets name it) / legacy / ACES 2.0 / 1.3, every bundled config carrying an **EDR patch** (Linear sRGB / P3 EDR displays at 1.0 = 100 nits, an ST2084-P3-D65 input; `assets/OCIO/patches/README.md` + "CONFIG UPGRADE" checklist comments); OCIO off at boot, Input = `color_picking` role → `scene_linear` → first, Display = default, View prefers "Un-tone-mapped"; per-clip PINS on input / look / scene LUT / knee, the display side shared, renderers read an immutable snapshot, `sameShader()` = knee-only changes are uniform updates; LUT import (.cube / .3dl / .csp / .cc / .ccc / .cdl with CCC id; display slot .cube / .3dl / .csp), export .cube 65³ < 100 ms (a sampled pre → stage → post function when the knee is on); the **linear stage** pre → (gain → knee) → post in linear Rec.2020 at 1.0 = 100 nits, knee BT.2390 Hermite by default / hyperbola with a user start, three twins (C++ / MSL / HLSL) that must agree; viewer aids gamma + channel view (never saved, in captures); capture resolves to the closest sRGB SDR Display / View pair; 25 built-in presets in five sections, user presets in AppData `color-presets.json`, six Kind badges gating by swapchain mode; minColor vendored (GPL-3, OpenDRT 1.1.0): Input → Knee → AgX → (Un-tone-mapped \| OpenDRT) → Display, AgX forces Un-tone-mapped, fixed kernel; host-display emulation | OCIO route + minColor COVERED; **config inventory, EDR patch, presets, LUTs, the linear stage / knee, pins, viewer aids, capture rule, emulation NOT IN PLAN** |
| Scopes + guides (`render/*scope*`, `assets/safety`) | ONLY vectorscope + waveform exist (no histogram, no parade): grid 512², 4 atomic copies, tap ≤ 960 px wide, tiers Signal / Assumed / Input, waveform SDR −10…110 %, HDR linear nits to 300 / 600 / 1000 / 2000 / 4000, a separate peak pass over every source pixel, white 100 nits with a BT.2408 203-nit hook, vectorscope zoom 1 / 2 / 4, dual overlay cyan / orange, ≈ 13 MB freed when idle; guides = 13 safety SVGs (16×9, YouTube, Meta, TikTok, Pinterest, Samsung, Snapchat) fitted to the source frame and stretched by the un-squeeze — no grid, no letterbox; backgrounds Black / DarkGray / DarkChecker / LightChecker with bounds-mix 0 / 1 / 0.7 / 0.7; no zoom / pan / 1:1 / pixel probe anywhere | §19 is over-scoped vs the code (histogram / parade / grids / probe would be NEW work) |
| Presenter + renderers (`render/`, 29 k: metal 7.8 k, d3d11 12.5 k) | COVERED by min-present; the rules to carry: pixel aspect is display-only (width × num / den), rotation 0 / 90 / 180 / 270 swaps the fit after the un-squeeze and overlays follow; compositor modes Single / SideBySide / Wipe (14-px seam) / Difference (abs(A−B) × gain), an inactive side transparent, an expected-but-missing side mirrored; capture = a full-resolution source render through a SEPARATE SDR chain + baked strokes (not a readback; `QCV_CAPTURE_PNG=path[@secs]`); budgets: texture pool 8 GB + a 256 MB thumbnail pool, graveyard 3 frames (30 kept ≈ 480 MB alive), upload ring 3 slots, canvas cap 3840×2160, 3 drawables in flight; HDR on D3D11: scRGB FP16 for EDR modes, HDR10 = R10G10B10A2 + G2084 / P2020 with FIXED metadata (MaxMastering 1000, MaxCLL 1000, MaxFALL 400, or the knee target), SDR white 80 nits — while the header says Hdr10 → scRGB and the HdrPq presets depend on PQ; DComp child HWND with `WS_EX_NOREDIRECTIONBITMAP`, HTTRANSPARENT passes mouse, drops need their own IDropTarget | **compare modes, capture graph, budgets, the one-rule-per-surface HDR10 question NOT IN PLAN** |
| Dual view (`dual/`, 8.8 k) | a deliberate second copy of the playback path: wall-clock timer at max(fpsA, fpsB) on a 60 Hz pump, 16-frame ring filling 8 ahead, controller rebuilt on a source change, master frame → each side's source frame through the timeline, sequence threads halved per side, ProRes on B forced to software (NVIDIA Vulkan), Win / Linux scrub on CPU (device-lost), teardown: scrub decoder before streaming decoder; audio mixer unity sum, per-side mute + servo, a side in a gap muted | "Phase 4" by §21.4 — **the compare modes belong to stage 4's design now** |
| Live inputs + output (`decode/live*`, `host_bridge*`, `blender_card`, `output/`, 4.2 k) | status numbers are a wire contract with QML (append only); SRT reconnect 250 ms doubling to 3 s, drains to the live edge on connect, waits for the first keyframe (sender GOP 1 s), publishes at least every 250 ms while catching up, publishes into the main decoder's slot; host bridge polls 250 ms waiting / 2 ms healthy, liveness by pid, frames copied into a reused pool, **macOS-only**; Blender card = a JSON card in an Inputs folder (`QCV_INPUTS_DIR`), an SRT listener, off at every launch; output: a paced repeat-last clock, hw H.264 (VT / Media Foundation, CBR, no B-frames, 1 s GOP, 6000 kbps, 8 surfaces, queue cap 96 ≈ 1.5 s), AAC-LC 1024, MPEG-TS over SRT or FLV over RTMP(S) (waits for a keyframe for SPS / PPS), backoff 1 / 2 / 4 / 8 / 10 s, `stop()` waits for GPU-held surfaces, no Linux encoder | rings COVERED (§20); **receivers, the card, the live audio buffer, the output stream NOT IN PLAN** ("Phase 4") |
| The shell (`ui/qml` 20.7 k, `window/` 12 k, `app/` 1.6 k) | menus (File / View / Tools / About with the full accelerator map), the KEY MAP (Space / K / W / S; Q / ← E / → step with auto-repeat; A / J D / L shuttle hold, auto-repeat ignored; Home / End; I / O / ⇧I; V and ⌘L loop; R cycles 0.5 / 0.75 / 1 / 1.25 / 1.5 / 2, ⇧R = 1×; M; ↑ / ↓ ±0.05; B backgrounds; F; T / ⌘T screenshots; ⌘Z / ⇧⌘Z / Del in annotation mode; ⌘R; bin Del / ⌫ / ⌘A / F2; edit mode ⌘K / Del / Esc) routed by a GLOBAL event filter so plain letters never steal from text fields (text focus detected via `cursorPosition` / `selectionStart`, stale focus cleared, a click outside drops it, popup fields re-routed); shuttle 2× doubling each second to 32× on a 33 ms tick, resets review speed, holds at the media edge, a final commit seek on release; Esc order: in-flight stroke → selection → live review → fullscreen + Compact together; modes Compact (borderless, a 22-px strip, timecode = move handle) / Minimal (rails collapsed) / always-open-minimal / Always on Top (OS calls, no hint) / borderless fullscreen; Color and Notes panels mutually exclusive; the chromeless gate never touches `*Visible`; the live strip replaces timeline + transport only when nothing is seekable; toasts 3.4 s / 5.6 s with an action (Undo), one at a time; go-to popup (frame / time / timecode); modal backdrop (frozen frame, pauses, resumes on close); window frame saved debounced 500 ms (screen before geometry, clamp if the title strip is unreachable, normal frame beside the maximized flag); drag thresholds 8 px window / 5 px timeline; hover 100 ms, popup fade 80 ms, band 90 ms; the SETTINGS panel (performance: sequence threads 16, cache window 72, video threads auto, hw decode on, scrub cache 512 MB, hw scrub off; interface: language, hover thumbs, drag-viewport-moves-window, waveforms, screenshot format; audio: A/V sync 11 / 7 ms single, 17 / 15 dual, scrub mute, listen to live, Blender sessions, live buffer 60 ms; updates) over ~60 QSettings keys with defaults (layout rails 280 / 320 clamped 200–600, colour panel 360 clamped 140–800, first run 1480×900, recents cap 10, loop default on, output / stream 6000 kbps 1080p SRT 120 ms baseline AAC 160, scope zoom / brightness / peak 1000, safety #ffff00, `stream/emulateHostDisplay` on) — "Reset all" only resets part; single instance (socket `qcview-singleinstance-<user>`, 500 ms probe, args forwarded, skipped with a dev flag); URL schemes `qcview://` (projects via Recents; `stream` with a colour chain), `qcbae://`, `srt://`, `minnotes:`; macOS Open events deferred a tick; file associations DISAGREE (macOS lists no audio / .hdr / .qcvexr, MSIX does); ~80 `QCV_*` env switches (`QCV_SETTINGS_DIR`, `QCV_KEY_DEBUG` …); 12 machine-translated catalogs (en + 11), applied on next launch, CSV round-trip + glossary, `QCV_PSEUDO_LOC`; logging to `~/Library/Logs/QCView` / `%LOCALAPPDATA%\QCView\logs` with thread ids and `.prev` rotation; NO crash reporter; NO accessibility; `toolbox.json` advertises the bundled ffmpeg to sister apps (mirrored to `%LOCALAPPDATA%\QCView\bin` because MSIX can't spawn in place); Sparkle daily; `detach()` / `reattach()` are dead code; tooltip / shortcut DRIFT (loop "L" vs V, "⌥T" vs ⌘T, "⌃3" vs ⌘3, a Notes Ctrl+5 comment) | §16's model COVERED; **everything named here NOT IN PLAN or NAMED-ONLY** |
| Packaging, deps, assets (`installer/`, `packaging/`, `scripts/`, `LICENSES/`) | macOS: stage → macdeployqt → `bundle_dylibs` (refuses Homebrew libs, 13.0 floor) → `prune_bundle` (179 → 124 MB) → inside-out signing with Sparkle's XPCs re-signed first → notarize + staple app and DMG → `update_appcast` (EdDSA); secrets: Developer ID, notarytool profile, the Sparkle private key, `CODESIGN_IDENTITY` / `TEAM_ID`; entitlements: unsigned-executable-memory, disable-library-validation, user-selected read-write, network.client; MSIX: windeployqt → MakeAppx / MakePri, runFullTrust, 4 associations, `qcview:` protocol, MinVersion 17763, unsigned for the Store, PFX subject = Publisher for sideload; no Store listing in the repo; deps: Qt 6.11 LGPL, FFmpeg n9.0.1 GPL (+ x264 / x265 / dav1d / vpx / SVT-AV1 / LAME / opus / srt MPL / mbedtls), OpenEXR + Imath BSD, libpng / jpeg-turbo / tiff, OCIO 2.5 BSD (+ yaml-cpp / expat / pystring / minizip-ng), minColor GPL-3, ink-stroke-modeler + Abseil Apache, ExifTool + Strawberry Perl GPL / Artistic, SoundTouch LGPL-2.1, Sparkle MIT, Geist OFL, Phosphor MIT; `dependencies.md` lags `vcpkg.json` (OCIO 2.5.2 / OpenEXR 3.4.15 / Imath 3.2.3) | `min-chrome` NAMED-ONLY; **the licence boundary for the crates (row 116) must list GPL / LGPL pieces explicitly** |
| Tools (`tools/`, 4.5 k) | bench-decode, probe-exr-decode (16 workers), probe-linear-stage (1e-6 monotonicity), probe-ocio-range (8-bit extremes 1.0913 / −0.0685 survive), probe-ocio-d3d11 (5e-3), probe-ocio-metal (1 % + 1e-3), probe-ocio-emulate (red 0.72 vs 0.86), probe-ocio-pins, probe-qt-dcomp-coexist, probe-mincolor, winharness PSNR / md5 parity | the ports of `tools/` are the colour step's pass bars |
| `docs/` + `ideas/` | 24 manual pages (+ appcast of 44 releases); the manager page says three bins vs the code's six; `ideas/`: the camera-RAW companion-process plan (RED incl. N-RAW / R3D NE, BRAW first; ARRI partner programme + Codex HDE; Sony / Canon asks; CinemaDNG in the player with a debayer stage; Store + sideload MSIX, app alias, `uap3:AppExtension`, `LOCAL\` pipes, D3D11 NT handles; IOSurface on macOS; a CPU shared-memory fallback) — nothing built | the manual comes with G2; **the RAW companion = a future decoder boundary, NOT IN PLAN** |

### 24.2 Rulings needed (the holes, in step order)
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 138 | **§21.4's slice, re-cut** | 1 open → probe → presenter (done) · **2 transport + timeline** = S2's model + QCView's Transport constants, the EDIT set with undo, FrameLocked / WallClock, review speeds, loop, in / out, the hover-thumbnail cache, the cache / failed-frame bands, `qcv-meta` (row 133) · **2b audio** = QCView's chain ported whole (servo tiers, stale-ring fades, the 5-frame resampler, the multi-stream graph + routing modes, meters, the shuttle grain engine, device backends, the waveform strip) · **2c image sequences** = `min-image` pulled forward (OpenEXR layers / parts, the parallel mode, mmap, PNG / TIFF / JPEG quirks, the sentinels, the cache tuning, the GPU upload thread, hover thumbnails through OCIO) · **3 the shell** = §16's rails / panels / inspector PLUS the menu map, the key map with its routing rules, the modes, the settings model, the project file, recents, single instance, URL schemes + file associations, toasts / go-to / modal, logging, localisation, the Esc order · **4 colour + scopes + guides** = the config inventory + EDR patch, presets, LUT import / export, the linear stage / knee in the stage-2 slot, pins, viewer aids, the capture graph, emulation, the two scopes, the 13 safety guides, backgrounds, budgets · 5 DnD · **6 annotations + the four exporters** · 7 the Adobe ring · **Phase 4**: dual (compare modes designed in stage 4 NOW, built then), live inputs + the Blender card + live audio, the output stream, AJA / NDI, the RAW companion process | **RULED 2026-11-07** ("every other proposal is fine") |
| 139 | Audio | a step of its own (2b) right after the Transport — J / K / L is unusable without it; port QCView's design whole (row 131), the one measured divergence allowed is none until it plays; `cpal` is NOT the device layer (the servo wants the device's playout latency: direct CoreAudio + WASAPI behind one trait, as §4.3 said); the audio-master mode the canon mused about does not exist in QCView — video stays the master | **RULED 2026-11-07** ("every other proposal is fine") — **BUILT 2026-11-07, slices 1–3** (`qcv-player::audio`: servo / drain / ring / resampler / routing / decoder / chunk reader / CoreAudio device / player / shuttle grains / waveform probe, measured in `docs/phase2/step2b-audio.md`: drift p50 ≈ 6 ms, 0 underruns, one fade per break); **slices 4–5 BUILT the same day**: tempo (SoundTouch WSOLA, 0.25–4×) and the multi-stream decoder (avfilter merge / pan graph, the routing plan verbatim); still owed: WASAPI (rhombus), the routing picker + meters + sync-offset nodes, the review-speed chip |
| 140 | SoundTouch (LGPL-2.1) | not statically linked into a permissive crate: either dynamic linking behind an optional feature of `qcv-player`, or a permissive WSOLA / phase-vocoder replacement measured against SoundTouch on the same clips (`signalsmith-stretch`-class) — decided by a listening test, not by licence alone | **CLOSED 2026-11-07** by row 116 — SoundTouch links statically inside the GPL crate; a permissive replacement is only a listening-test curiosity now |
| 141 | The project file | `.qcvproj` v2 is a FROZEN contract like the sidecar: a `qcv-project` crate reads and writes it byte-for-byte (schema string, `${PROJECT_DIR}`, atomic writes, the six bins, every migration and default, UUIDs, the dedupe keys, the extension table incl. the refusals), tested against the user's real projects; the manual's "three bins" is corrected to the code | **RULED 2026-11-07; BUILT 2026-11-07 night** — `crates/qcv-project` (model / extensions / io / manager); 33 of 33 of the user's real `.qcvproj` files byte-identical after load → save (`QCV_PROJECT_FIXTURES_DIR`); docs/phase2/step-project-crate.md; the shell wiring BUILT the same night: the Add Media section, load by type, per-clip persistence of the Inspector's pickers, Open / Save / Save As / New with rfd dialogs, drops into the project, recents in the store — § The shell wiring in the note; owed: context menu, rename, drag-to-playlist, toasts |
| 142 | Settings | a settings SERVICE with a typed key schema, defaults, a real "reset all", `QCV_SETTINGS_DIR`-style isolation for tests, and the layout clamps — one implementation for the family (ufb's and minNotes' QSettings have the same shape) | **RULED 2026-11-07** ("every other proposal is fine") |
| 143 | The shell's contracts | carried verbatim: the key map + its routing rules + the Esc order, the shuttle constants, the modes and their interaction rules, toast / popup / band timings, the window-frame restore rules, the drag thresholds, single-instance + URL schemes (`qcview://`, `qcbae://`, `srt://`, `minnotes:`), the file associations reconciled to ONE list; the shortcut registry (row 74) GENERATES tooltips and the reference card so the drift cannot recur — and, named by the user 2026-11-07 as shell items in their own right: window pinning to the foreground (Always on Top by OS calls), the menu bar, tooltips, modals and modal panels (§16's widget union gains them as first-class kinds, not afterthoughts) | **RULED 2026-11-07** (user: "we have a lot of keyboard shortcuts to port over faithfully"; "window pinning to foreground, menus, tooltips, modals, modal panels" named as shell items) — **BUILT 2026-11-08 (the shell round):** the MENU BAR (QCView's File / View / Tools / About via `muda`: NSMenu with an app menu on macOS — AppKit's tab + full-screen extras off — a Win32 bar on Windows; the Recent submenus live; Compact / Always on Top checkmarks; unlanded items disabled; modified shortcuts are the menu's on macOS, shown-only on Windows where the key map keeps them; About = the platform panel, a written deviation), SINGLE INSTANCE (`qcv-singleinstance-<user>`: a socket file in the per-user temp dir / a named pipe; forwards files + `qcview://` links, raises the window; stale sockets recovered; tested cross-session on rhombus), `qcview://` project links + Copy Project Link, the FILE LOGGER (`qcv-log.txt` + `.prev` in `~/Library/Logs/qcv` / `%LOCALAPPDATA%\qcv\logs`), `.lnk` shortcuts (IShellLink) on every way in. OWED: file associations + the URL-scheme registration and macOS's Apple-event opens (Finder / `qcview://`) — they need the app bundle / MSIX (packaging); localisation (QCView ships 13 `.ts` translations — a design talk) |
| 144 | Localisation, logging, diagnostics, accessibility | i18n from day one in `min-ui` (12 catalogs exist; the CSV pipeline + glossary carried; plural forms; applies live, not on restart); logging to the same directories with rotation; a crash reporter is NEW (none exists); accessibility is NEW (AccessKit from S3 — nothing to preserve) | **RULED 2026-11-07** ("every other proposal is fine") |
| 145 | Colour, scoped to the code | §18 gains the config inventory, the EDR patch + upgrade checklist, the preset model (Kind badges by surface), LUT import / export, the linear stage / knee as the first occupant of the stage-2 slot (three twins → ONE WGSL / passthrough pair verified by the ported `probe-linear-stage`), pins, viewer aids, the capture graph (full-resolution, a second SDR chain, baked strokes), host-display emulation; `tools/` probes ported as the pass bars; **one HDR rule per surface** (EDR linear on Metal; scRGB FP16 for EDR and R10G10B10A2 PQ with the fixed metadata on DX12) | **RULED 2026-11-07** (user: "color is fine tuned and all of that is important") |
| 146 | §19 re-scoped to the code | scopes = vectorscope + waveform with QCView's maths (grid 512², the tiers, the ranges, the peak pass, the 203-nit hook) — histogram / parade / grids / pixel probe / zoom / pan are NEW and wait for a request; guides = the 13 safety SVGs + the un-squeeze rule; backgrounds = the four + bounds mix — REVISED: zoom / pan / 1:1 / pixel probe are IN the port, scheduled after the faithful setup (post-G2, as additions over the ported base); histogram / parade wait for a scope-UI rethink (the right rail is cramped) rather than a request; guides move to row 152 | **RULED 2026-11-07, revised** (user: zoom / pan / pixel probe "we will add in this port, but not until we have a faithful setup first"; histogram / parade "might be nice, but we would want to reconsider UI there — it's getting cramped in that rail") |
| 147 | Budgets | ported as numbers: texture pool 8 GB + 256 MB thumbnails, graveyard 3, upload ring 3, canvas cap 3840×2160, 3 frames in flight, scrub cache 512 MB, sequence cache 3 s / 0.5 s, 16 IO workers, hover LRU 100 | **RULED 2026-11-07** ("every other proposal is fine") |
| 148 | Image sequences | `min-image` is step 2c, not "later": OpenEXR (FFI, the C library — the parallel mode and multi-part are what the user's 4K EXR workflow needs) + PNG / TIFF / JPEG with QCView's quirks, the sequence cache + GPU upload thread, hover thumbnails; FFmpeg's `exr` decoder stays the fallback | **RULED 2026-11-07** ("every other proposal is fine")  — **2026-11-08: pieces 1, 1b, 2, 3 BUILT** (sequences play through QCView's cache on both OSes) (`crates/min-image`; EXR bit-exact vs FFmpeg on both OSes, 4K in 15 ms on the box; PNG / TIFF bit-exact vs FFmpeg, JPEG bit-exact vs libjpeg-turbo; `docs/phase2/step2c-image-sequences.md`) |
| 149 | Windows ProRes | S1's finding stands (software on NVIDIA); QCView's Vulkan-video ProRes route is measured on the monitor session (row 130) before the Windows presenter is called done; D3D12VA keeps the inter codecs — REVISED: GPU ProRes on Windows is a REQUIREMENT of the Windows presenter, not an option. Software ProRes was measured at 161–168 % of a core for 4K (S1 Windows) and does not reach 6K / 8K. This reopens §4.4's W1 / W2 / W3 for QCView on Windows as a spike (**S6, on the monitor session with row 130**): (a) wgpu's Vulkan backend + FFmpeg's Vulkan hwaccel on the SAME VkDevice (zero-copy; the open question is HDR surfaces on wgpu-Vulkan — S1 saw an 8-bit-only swapchain on NVIDIA; `VK_EXT_swapchain_colorspace` / HDR10 via wgpu-hal is the thing to prove), vs (b) a decode-only Vulkan device exporting through `VK_KHR_external_memory_win32` into DX12 (today's hybrid shape, one GPU-side import, no CPU copy), vs (c) D3D12 + a Vulkan→DX12 shared-fence pair; the pass bar = QCView's own 3× over software at 4K and real-time 6K / 8K ProRes playback; inter codecs stay on D3D12VA | the D3D12VA SCRUB check DONE 2026-11-07 on rhombus (headless, no display): hw scrub is SLOWER than software (1080p h264 21 vs 4.8 ms; 4K h264 one request in 2 s vs 6.8 ms) → QCView's OFF default holds; the display-session pacing / EDR / PQ measurements remain owed (row 130); see docs/phase2/step2c-rails-settings.md § Windows — **SETTLED 2026-11-07 night (the user): GPU ProRes on Windows is REQUIRED; row 6's software clause withdrawn. Route (b) — a decode-only Vulkan device exporting through `VK_KHR_external_memory_win32` into the DX12 presenter, QCView's own hybrid shape — needs NO HDR monitor and can be spiked on the virtual display now; route (a) waits for row 130. First check: the box's FFmpeg build (BtbN ffmpeg-win64) must carry the Vulkan ProRes decoder (QCView's notes: vcpkg's did not). Today's software figures on the box: 6.7K ProRes shows in ~12 ms (4:2:2 10-bit, ~100 MB per frame uploaded), 4K at 161–168 % of a core (S1).** — **S6 DONE 2026-11-07 night, route (b) BUILT in `min-ffmpeg`:** FFmpeg's own Vulkan device (`HwDevice::vulkan`, its default device enables the win32 external memory + semaphore extensions), QCView's per-codec routing (ProRes / FFV1 / APV → Vulkan, inter codecs → D3D12VA), `vk_bridge.rs` = shared per-plane D3D12 textures + a shared fence imported into Vulkan, `vkCmdCopyImage` per plane (the ProRes 422 frame arrives as THREE separate R16 images, LSB-aligned 10-bit), slots reused only after the presenter releases the frame. MEASURED on the RTX 4090: 4K ProRes HQ 37 % of one core vs 511 % software, show 0.37 ms vs 5.96; 1080p 19 % vs 236 %; scrub previews 4.4 ms vs 8.7 readback; pixels IDENTICAL to software at both sizes (encoded + display paths), and for 4:4:4 12-bit (planar3) and 4444 + alpha (planar4) on synthetic clips; inter codecs unchanged on D3D12VA. No HDR monitor needed; route (a) is moot. Owed: ProRes RAW (software, as QCView), a LUID match on multi-GPU machines. |
| 150 | Dual, live, output, RAW | Phase 4 as ruled, with three things designed EARLIER because stage 4 and the shell own them: the compare modes (Single / SideBySide / Wipe / Difference, the seam, the mirrored-missing rule) in min-present's composite stage; the live-status wire contract (append-only numbers) in the shell; the RAW companion-process boundary (IOSurface / NT handles / `LOCAL\` pipes) as a `qcv-player` source kind | **RULED 2026-11-07** ("every other proposal is fine") |
| 151 | Licences, explicit | row 116 gains the list: FFmpeg's GPL build (x264 / x265), minColor GPL-3, ExifTool GPL / Artistic, SoundTouch LGPL-2.1 live ONLY in `apps/*` (GPL-3.0-or-later) or behind dynamic links; the shared crates stay MIT OR Apache-2.0 with OpenEXR / OCIO / Abseil (BSD / Apache) allowed; the workspace's pins (row 117) replace `dependencies.md` | **RULED 2026-11-07 as revised by row 116** — the explicit list shrinks to: `min-paths` MIT OR Apache-2.0; the `qcbae` spine MIT OR GPL-3.0; everything else GPL-3.0-or-later; QCBridgeN links nothing from the workspace; `dependencies.md` retired in favour of the workspace's pins |
| 152 | **Guides, drawn by the annotation renderer** (user 2026-11-07: "there is an argument that we use our annotating drawing system to render these instead of having a separate system … the SVGs were just an easy way for me to make coordinates visually … it might make sense to rethink that discrete separation") | ONE overlay vector layer: safety guides, annotations, the wipe seam, selection handles and any future burn-ins are shapes in the same renderer (`qcv-ink`'s capsule-SDF strokes + vello paths in one overlay pass over stage 4), in frame-normalised coordinates that follow the un-squeeze and rotation like strokes do; the 13 SVGs stay as an AUTHORING / import format (loaded through `vello_svg` into the same layer, converted to shape data once so the guide set is editable in-app later); QCView's "fit to the source frame, stretch with the anamorphic un-squeeze" rule carried; the safety colour (`safety/colorHex`, #ffff00) becomes a shape style; guides never reach the sidecar | **RULED 2026-11-07** (the user's steer; the design lands in step 4 with the scopes, built on step 6's renderer — so step 6's ink renderer is pulled into step 4) |
| 153 | The viewport background under alpha (user 2026-11-07: "background shader / color changes behind alphas in the viewport") | stage 4 composites the four backgrounds (Black / DarkGray / DarkChecker / LightChecker, the bounds-mix 0 / 1 / 0.7 / 0.7, `QCV_BOUNDS_MIX`) UNDER the frame's alpha — the alpha plane now rides through stage 1 (step 1b) and the packed 4444 kernel keeps it, so this is a composite-stage rule, not a decode one; the checker is drawn in the display space so it never passes through the colour chain | **RULED 2026-11-07; BUILT 2026-11-07 pm** — `min-present` stage 0 (`background.wgsl`: the four fills, 32 device-px tiles, the bounds mix toward #161616 at 0 / 1 / 0.7 / 0.7, `QCV_BOUNDS_MIX`; the picture pipelines blend One / OneMinusSrcAlpha over it, planar4 / packed / the blit premultiplied); `display/backgroundMode` drives it; the Background rail section = a fused 2 × 2 block of size-S chips above Settings (QCView's order); the bar's Background button and Ctrl+, reveal sections (QCView's `revealSection`); one test (fill under alpha, mix outside, checker tiles) |

### 24.3 Step 2's rulings (proposed 2026-11-07; the note is `docs/phase2/step2-transport-timeline.md`)
| # | Decision | Proposed default | Status |
|---|---|---|---|
| 154 | The Transport | QCView's transport slice ported verbatim (row 131): the shuttle ladder (2× doubling per second to 32×, 33 ms tick, resets review speed, holds at the media edge, a final commit seek on release), the review speeds with ⇧R, the 50 ms playlist seek-and-resume debounce, loop default on, in / out off in playlist mode, the modal pause / resume; the transport keys of row 143 are the shortcut registry's first client | **RULED 2026-11-07** ("go, start with the scrub decoder") — **BUILT 2026-11-07** (piece 2: `transport.rs` + `playback_timer.rs` + `key_map.rs`) |
| 155 | The scrub decoder per OS | SOFTWARE on Apple (VideoToolbox pays a flush per seek: S2), slice threads for intra / frame threads for inter, GOP LRU, RETARGET (S2's measured addition to QCView's rule); HARDWARE (D3D12VA) on Windows for inter codecs (S2: 7.5 vs 42.8 ms) with QCView's pool rule — pool-based backends are never cached as hardware frames, entries are read back to planes first | **RULED 2026-11-07** ("go, start with the scrub decoder") — **BUILT 2026-11-07** (piece 1: the retarget rule; the Windows hw scrub run owed) |
| 156 | `min-ui`'s first nodes | the transport bar, the timecode well + go-to popup, the timeline panel (ruler / lanes / cache band / ticks / in-out), tooltips — vello + parley in the one scene over the composite (stage 4's UI layer); the go-to field is the first text input (S3's editor); `min-theme`'s tokens are born here from `Theme.qml`'s names | **RULED 2026-11-07** ("go, start with the scrub decoder") |
| 157 | `qcv-meta` lands in step 2 | per rows 132–134 / 136: the extractor verbatim, the service's generations, the XMP reader, the timecode formatter; the MXF scanner as 2d; `tools/probe-metadata` is the parity oracle | **RULED 2026-11-07** ("go, start with the scrub decoder") — **BUILT 2026-11-07** (piece 4) |
| 158 | The hover-thumbnail cache | QCView's `timeline_thumbnail_cache` design as a `qcv-player` service (320×180, LRU 100, workers 3 / 1, stride 6, ≤ 64 uploads per frame, generations, nearest-cached on a miss, 16 ms throttle); through the colour chain from step 4 | **RULED 2026-11-07** ("go, start with the scrub decoder") — **BUILT 2026-11-07** (piece 3: `thumbnail_cache.rs`; + the mouse-hover additions the same day: latest-wins hover slot, the strip at frames / 400 with a 500 budget, threaded loader, the GOP-cache peek — measured in the step-2 note) |
| 159 | Timeline edits | `timeline_edits` ported verbatim as undo commands (the eight edits, gapless recompute, 12-px snap, Esc restores, the duration patch rule, the stack cleared on leaving dual); the overview zoom bar and ⌘K cut with the panel | **RULED 2026-11-07** ("go, start with the scrub decoder") — **BUILT 2026-11-07** (piece 3: `qcv-timeline`'s `Editor`; QCView's backward-move undo bug filed in §17.6) |

## 25. `min-ui` — what it is: Spectrum 2's structure, our fonts, a scrubber not an NLE (2026-11-07)

The user, before the first node is built: "lets talk about what min-ui is. Right now our setup for QCView is fairly
barebones and the timeline area is NLE inspired and possibly occupying too much space, clip editing is clunky in dual
view. I really like Adobe Spectrum as inspiration when we start our own — soft greys, rounding, dropdown, tree styling,
etc. I do want to keep Phosphor and Geist though." Adobe's Spectrum 2 token set is public (`adobe/spectrum-design-data`,
Apache-2.0 JSON: a 13-step gray ramp per theme, alias layers, component tokens, scales for radius / size / spacing /
type, three-layer drop shadows) and was read against QCView's `Theme.qml` (45 colours, 7 type sizes, 7 control heights,
radii 3 / 6 / 10, no shadows).

### 25.1 What the lean told us about the current setup
- **The tone is already Spectrum-dark; the discipline isn't.** QCView's surfaces sit on S2's dark ramp (bg #161616
  between base gray-25 #111 and layer-1 gray-50 #1b1b1b; toolbar #1f1f1f ≈ layer-2 gray-75 #222; hover #252525 ≈
  gray-100 #2c2c2c) but as 14 use-named tones with no ramp beneath — why four token files share only 27–35 names.
- **Contrast roles.** textSecondary #888 on bg = 5.1:1 (S2 subdued gray-700 #afafaf = 8.6:1); textMuted #666 = 3.2:1
  — S2 has no low-contrast role for live text; gray-400 #444 (1.9:1) is disabled only. Cobalt #0189f1 under white text
  = 3.6:1 (S2 blue-800 dark = 4.5:1).
- **Scales.** Radii 3/6/10 → S2 4/8/10/16/full (pills for real buttons, 8 on toolbar actions, 16 on dialogs); seven
  control heights (16…32) → 20/24/32/40; spacing gains 6/16/24/32; type 10/10/11/12/13/14/16 → 12/14/16/18 at
  line-height 1.3 (the 10-px mono readouts are below S2's floor → Geist Mono 12); toolbar icons 16 → 20.
- **Elevation.** Flat today; S2 floats popovers, menus, tooltips and dragged items on a three-layer soft shadow
  (dark: 0/4/12 @ .24, 0/2/6 @ .12, 0/0/2 @ .36) — vello draws blurred rounded rects natively.
- **Accent.** Checked toolbar toggles, selection, the in/out zone, the waveform and the playhead are all blue or green
  today; S2 reserves the accent for *emphasized* states and makes a selected action button a neutral gray-800 fill.
- **Workarounds that die with the native surface.** The colour-panel "reels" (a Picker could not cross the surface)
  become a real Picker; the Popup.Window tooltip becomes a node.
- **Components.** S2's catalogue (Picker, TreeView, Tabs, Disclosure, ActionGroup, Tag, Badge, StatusLight, Toast,
  Tooltip, Popover, Slider, Stepper, Switch, Checkbox, SearchField, TextField, Dialog, Breadcrumbs, Divider, Meter)
  maps onto §16.2's union one to one. S2 has no timeline; its principle that applies is quiet by default with
  progressive disclosure — a review tool is not an NLE.

### 25.2 The timeline as a scrubber (the user: "the timeline as a scrubber is great … I love our frame ruler setup")
Not contradictory once the ruler *is* the scrubber: today's bulk is the lanes (status 22 + transport 32 + ruler 20 +
lanes 34 / 68 + overview 18 = 126 single / 160 dual), not the ruler.
- **One 32-px band** = ruler ticks and labels along the top, the playhead through it, in/out flags and zone, note
  pennants on the bottom edge, the cache band + failed-frame ticks as a 2-px underline. Wheel zooms about the cursor as
  today (`1.0015^dy`, Alt-wheel pans, 20-px follow). **Frame ticks appear at ≥ 6 px per frame** and labels switch to
  frames at the next step — today's ruler never shows frames ("SS.f s" above 30 px/s). The overview bar retires: a 2-px
  extent marker inside the band when zoomed, double-click fits.
- **Audio = the band's quiet background**: the waveform as a very faded gray fill under the ticks, bottom-anchored, no
  outline — the user's reference is Lightroom's histogram in the Spectrum 2 panel (a near-black well, the curve as a
  faded hatched gray fill, labels in tiny tracked capitals). Costs no height; present whenever the clip has audio; in
  dual it shows the audible side. (Today: 28 %-alpha blue inside the 34-px lane, single-clip rows only.)
- **Dual without a mode.** The edit toggle exists because the lane is both the scrub target and the edit target.
  Separate the targets and the toggle has nothing to decide: two **12-px clip bars** (A, B, side colours) under the
  scrubber show each side's extent against the master (the offset is visible at a glance). By target: ruler press =
  scrub; bar click = seek, bar drag past 5 px = slide (open / closed hand); 8-px end handles = trim; Alt-drag = slip;
  right-click or the bar's chip = a **precision popover** (offset in frames as a stepper, "align start to playhead",
  "match in points", reset) — the path the 34-px lanes never had. The playhead runs through the bars; the ghost + badge
  grammar and the trim scrub into the viewport carry over. **Playlist** = one 20-px bar whose segments are the clips
  (names, reorder by drag, trim handles, the drop ghost). **Single media** = the scrubber alone.
- **Bottom bands**: status 24 + transport 32 + scrubber 32 = 88 single, +24 dual = 116 (vs 126 / 160), +20 playlist.

### 25.3 Rulings
| # | Decision | Ruling | Status |
|---|---|---|---|
| 160 | The design system's structure | `min-theme` is born in Spectrum 2's three-tier shape — a 13-step global gray ramp per theme → alias layers (base / layer-1 / layer-2 / elevated; content / subdued / disabled; accent / positive / notice / negative / informative) → component tokens — in S2's JSON token file shape (`value` / `sets` / `{alias}`), with S2's scales: radius 4 / 8 / 10 / 16 / full, control heights 20 / 24 / 32 / 40, spacing 2 / 4 / 6 / 8 / 12 / 16 / 24 / 32, type 12 / 14 / 16 / 18 at line-height 1.3, icons 20 / 16. Our values on the ramp (QCView's tones re-expressed); **dark only, by product direction** (the user 2026-11-07: "our app will be dark mode only too") — the ramp keeps a light flip possible but nothing is built for it | **RULED 2026-11-07** ("I really like Adobe Spectrum as inspiration … soft greys, rounding, dropdown, tree styling") — **BUILT 2026-11-07** (`crates/min-theme`: 5 token files → 396 resolved tokens, build.rs generator, 7 tests incl. the contrast roles, `min-theme-dump`) |
| 161 | Fonts and icons | Geist Sans / Geist Mono / Phosphor stay (the six files QCView bundles, with their licences); Phosphor as glyphs through parley with QCView's slug → codepoint table ported; toolbar icons at 20 | **RULED 2026-11-07** ("I do want to keep phosphor and geist though") — **BUILT 2026-11-07** (the six faces + licences and the 1 512-name Phosphor table live in `min-theme`) |
| 162 | The accent | Spectrum's dark blue: blue-800 rgb(64,105,253) for fills under white text, blue-900 rgb(86,129,255) for accent text and icons, blue-1000 rgb(105,149,254) on hover; cobalt #0189f1 retires. Spent in FOUR places only: the focus ring, emphasized buttons in dialogs, the loaded-media row in the rails, the active side chip. Everything else neutral: selected toolbar toggles = gray-800 fill with inverted text, list selection = quiet gray-200 fill, the in/out zone = white at 6 %, the waveform gray | **RULED 2026-11-07** ("lets move away from cobalt as its a bit bright compared to the spectrum blue choices which are better and I think less use of the blue highlight in general is a good idea") |
| 163 | Contrast roles | content = gray-800, subdued = gray-700, disabled = gray-400; the "muted" live-text role retires; disabled danger buttons go gray like everything else | **RULED 2026-11-07** (with the lean) |
| 164 | Elevation | S2's `drop-shadow-elevated` on popovers, menus, tooltips and dragged items; flat everywhere else; 1-px dividers between button clusters replaced by spacing (ActionGroup) | **RULED 2026-11-07** (with the lean) |
| 165 | Component vocabulary | `min-ui`'s widgets take Spectrum 2's catalogue names, t-shirt sizes (XS 20 / S 24 / M 32 / L 40) and state model (default / hover / down / focus / disabled / selected; quiet vs emphasized; static colour); the Picker returns (the reels retire), TreeView, Tabs, Disclosure, ActionGroup, Tag / Badge / StatusLight, Popover with tip, Stepper, Meter; FlatButton becomes ActionButton (quiet / emphasized, neutral selected), dialog buttons are pills | **RULED 2026-11-07** (supersedes row 73) — **the first three BUILT 2026-11-07**: `min-ui::{ActionButton, ActionGroup, Tooltips}` held against Spectrum CSS's `spectrum-two` theme (32 / 20 / 6 / radius 8 / no border; hover gray-200, down gray-300 = the family's one step past; 8-px gaps, 16 between clusters, compact groups fused with the radius on the ends; the tooltip gray-500 + white, 600 / 5000 ms, a 10 × 5 tip). **Selected = a deliberate mid grey (gray-300, light glyph), NOT Spectrum's inverted gray-800** (the user: "I would prefer more deliberate greys than inverts") |
| 166 | The timeline surface | the 32-px scrubber of §25.2: ruler + playhead + in/out + pennants + cache underline in one band, frame ticks at ≥ 6 px/frame, the overview retired, the waveform as the faded fill (Lightroom-histogram reference) | **RULED 2026-11-07** ("the timeline as a scrubber is great"; "I prefer the fill … very faded") — **BUILT 2026-11-07** for single media (`min-ui::Scrubber`: camera, gesture, ticks, in/out, extent, hover; the waveform waits for the audio probe, pennants for notes) |
| 167 | Dual and playlist without an edit mode | two 12-px clip bars (A / B) under the scrubber, interaction by target (ruler scrubs; bar click seeks, drag slides, ends trim, Alt slips), a precision popover; playlist = one 20-px clip bar; §17's model and edits unchanged, only the surface's resting state | **RULED 2026-11-07** ("I don't love the mechanics of dual having to toggle into edit mode"; "your instinct on 2 is a good idea") — **PLAYLIST HALF BUILT 2026-11-08** (the user: "go" on the proposal): loading (QCView `startPlaylist`; a new playlist loads on creation), the 20-px clip bar (`apps/qcv/src/clip_bar.rs`: ends trim with a ±frames badge, body reorders with the insertion bar + ghost, Ctrl/Cmd slips — QCView's modifier, Alt stays the wheel pan — a click seeks + selects), drops of rows / OS files onto the bar or the scrubber (edge within 12 px else append, audio refused, the live insertion bar), Delete / Ctrl+K / Ctrl+Z ⇧Z, the precision popover (`clip_popover.rs`: Source in · out · Duration as timecode), every edit written back to the project item. Owed: the viewport preview while trimming (QCView's edit scrub), clip thumbnails in the bar; dual's two bars stay row 173. |
| 168 | The surface's colours | lean Spectrum: playhead gray-900 (near-white), in-point = `positive-visual-color`, out-point = `notice-visual-color`, failed frames = `negative-visual-color`, side A / B re-picked from S2's categorical palette in the theme pass; the waveform a faded gray fill (solid at ~12 % alpha; the hatch of the reference as a one-line option) | **RULED 2026-11-07** ("Green is not sacred — lets lean toward spectrum for color choices") |
| 169 | The bottom bands | status 24 + transport 32 + scrubber 32 (+24 dual, +20 playlist — **the playlist bar went to 28 px on the walk, 2026-11-08: "make the tracks a little taller so text fits"**); the transport bar drawn at 32 with 20-px icons and spacing instead of dividers, every QCView button in order (unbacked ones disabled) | **RULED 2026-11-07** ("this is a perfect layout"; the transport cluster as a compact fused group — "I agree with the clustering suggestions") — **the transport bar and the status strip BUILT 2026-11-07** as nodes (`apps/qcv/src/transport_bar.rs`, `status_strip.rs` with the go-to popover = the first text input, row 156's S3 editor; the video fitted above the bands by `Engine::set_content_rect`); the scrubber BUILT the same day (6e) — the three bands are all nodes |
| 170 | The hover thumbnail | a `min-ui` node over the viewport (§17.5 item 5), display-referred, QCView's placement (bottom-left, 0.18 of the frame width) | **BUILT 2026-11-07** as proposed (`apps/qcv/src/hover_thumbs.rs`); **revised the same day** (the user: QCView parked it bottom-left only because of the Qt render-thread split, "this might be more ergonomic"): in a SOLO view the thumbnail follows the pointer horizontally above the bands, clamped to the content rect (the user: "this works great"); **dual (when it arrives): the two thumbnails track the pointer as a PAIR, A above B at the hovered spot, instead of QCView's parking** (the user 2026-11-07: "in dual view we can just have two that track the playhead instead of parking") |
| 171 | Fullscreen | the picture alone (QCView's chromeless rule: every band and rail hidden); the three bands slide in from the bottom edge while the pointer rests there (a 24-px zone) and slide away 800 ms after it leaves (160 ms, the slide token); the content rect becomes the whole window; the keys work throughout; Escape leaves fullscreen before it quits; F, the transport button and Escape share one toggle. Replaces the need for a separate minimal mode | **RULED + BUILT 2026-11-07**; revised on the walk (2026-11-07 pm): the bands POP in and out like the rails — the 160-ms slide is retired (the user: "it's simpler"); the 24-px reveal zone and the 800-ms hide delay stay |
| 172 | Compact mode | the scrubber band ALONE (32 px): a play / pause button at its left end, the timecode at its right, the pin and an exit beyond — a layout state of the scrubber node, not a second widget; the window drops its title bar as QCView's does and the band is the drag handle | **RULED + BUILT 2026-11-07 pm** (the user: "lets go" on the item, "perfect" on the build) — QCView's `CompactStrip` carried at the family's 32-px band: play / pause + the timecode (the window's move handle via `drag_window`) at the left, the scrubber's LINE state (`Scrubber::compact`: a 2-px line, the in / out marks, the playhead; the same gesture and camera) through the middle, the pin + the exit at the right; `set_decorations(false)`; Ctrl+Shift+C and the bar's button toggle, Escape leaves compact before fullscreen, fullscreen is left first when entering; the pin = `set_window_level` + `ui/alwaysOnTop` (Ctrl+Alt+T) |
| 173 | Dual's UI | the clip bars of row 167 under the scrubber; built when the dual compositor (Phase 4) or a playlist exists to drive them. **2026-11-08, the controls' place RULED** (the user picked "Status strip"): the A / B chips (each side's clip, × clears B), QCView's four mode buttons (Single `rectangle` · Side-by-side `square-split-horizontal` · Split wipe `arrows-out-line-horizontal` · Difference `intersect-square`), the seam / gain slider (seam in Split, gain in Difference) and Save as Dual View sit on the RIGHT of the status strip beside the frame / timecode wells — no new row; Single shows the A chip + the modes; hiding the strip (Ctrl+5) hides them (accepted). QCView's toolbar above the picture is not carried. **2026-11-08, SAVE RULED**: Save as Dual View opens a DIALOG (a name field prefilled with QCView's "A vs B", Save / Cancel) — QCView's inline save mode was a workaround for its modals and is not carried; rename later in the project panel like any item; Update stays a plain button when the session came from a saved Dual View (the user: "the saving ui was a hack because modals were difficult in our old setup" … "agreed, go with the dialog"). **2026-11-08, STEP 3 RULED** (the user: "1 yes, 2 both, 3 agreed — go build it"): QCView's controls carried with its strings — A / B chips (letter in the side colour, name middle-elided, QCView's empty-side placeholders, B's × keeps dual with B blank, both chips drop targets), the four modes as one group (any non-Single mode enters dual with the project's B, empty allowed; Single leaves; hidden for a playlist), the slider = seam 0–1 in Split / gain ×1–×16 in Difference, the seam ALSO dragged on the picture (14-px grab, resize cursor, grey → white), drops on the picture by side, Save / Update as ruled; side colours = Spectrum 2 categorical CYAN (A) and ORANGE (B) everywhere (chips, clip bars, the waveform); mode / seam / gain reset per launch (QCView). Mute pills wait for step 5; KNEE / VIEWER pills are the drawer's. | **RULED 2026-11-08** (placement; save; step 3) |
| 174 | **The rails** (user 2026-11-07: "do side rails because we need a settings panel before we go too much further") | QCView's rail geometry VERBATIM: left 280 / right 320 logical, clamp 200–600, persisted (`ui.layout/*`), collapse animated 0 ↔ W (QCView 140 ms OutCubic → the family's one slide duration, 160 ms), a 1-px splitter with a ±3-px hot zone (hover one grey step up — QCView lights it accent; blue is rationed), Ctrl+1 / Ctrl+2 toggle, a narrow caret tab at the window's edge reopens a collapsed rail (QCView's `ViewportOverlay` chevrons), both hidden in fullscreen, the bands run full width beneath. NO icon rail (the Lightroom reference has one, QCView does not; QCView wins the tie). Sections in §25.4's flat form: bold 14 header, caret LEFT, hairline between, no card; the 3-px overlay scroller. | **RULED 2026-11-07** (the proposal taken: "go with sub-headers, right rail starts collapsed") — `min-ui::{Rail, Section}` BUILT — **REVISED 2026-11-08 on the walk** (the user asked how Spectrum 2 groups panel sections — no containers or shadows for sections, layering by tone; three spacings and a layer-2 variant were rendered side by side; the user on the live tiles: "this helps a lot"): every section a flat rounded tile in `background-layer-2` (header + body, radius 8, no shadow) on the layer-1 rail, the hairline retired, one `rail-section-gap` (16) after every section and one `rail-group-gap` (16) above each sub-header in all four panels (they had 8–16 ad hoc), chip / meter backings one grey step up on the tile (`min_ui::section::card_color`); `QCV_RAIL_FLAT=1` shows the flat form for comparison |
| 175 | **Settings** = the left rail's first section, QCView's Settings section row for row with its own GroupHeaders as sub-headers (Performance / Interface / Audio; Updates waits for an updater) | rows whose feature has not landed are drawn disabled (gray-400) so the panel's SHAPE is final; the label wraps to two lines, its help is the tooltip (QCView's `RowLabel`); the control column is QCView's 110 px; "Reset all preferences…" confirms in a Dialog, then `reset_all` and the owner re-applies. Five S2 widgets: Switch (26 × 14 neutral — on = gray-800, no accent), NumberField (mono, stacked ± steppers, a focusing press selects the number, the wheel steps, "Auto" for 0), the quiet Picker (text + caret, menu below, the chosen row ticked), the pill TextButton, Dialog (scrim + elevated card). `performance/hwScrubInterCodecs` keeps QCView's default OFF on both OSes until the D3D12VA scrub check (row 149) is done. | **RULED + BUILT 2026-11-07** — `apps/qcv/settings_panel.rs`, `min-ui::{Switch, NumberField, Picker, TextButton, Dialog}` |
| 176 | **`min-settings`** (row 142 made real) | a permissive crate: `Schema` of `Key`s (name = QCView's `group/name` spelling, typed default, numeric clamp), `Settings::open(app, env_prefix, schema)` → one JSON file in the platform config dir (`~/Library/Application Support/<app>`, `%APPDATA%\<app>`, `$XDG_CONFIG_HOME/<app>`), `<ENV>_SETTINGS_DIR` redirects it (QCView's `QCV_SETTINGS_DIR` dev aid), atomic writes, unknown keys kept and written back (QSettings' downgrade safety), a change `generation` the UI compares instead of per-key signals, a real `reset_all`. qcv's schema = QCView's 36 keys verbatim + `ui.layout/*` + `window/*` (geometry restored at launch, saved at close). The right rail's default is collapsed (QCView: open) — the ruling. | **BUILT 2026-11-07** (tests: clamps / kinds / generations; atomic save, reload, unknown keys, reset; the schema) |
| 177 | **The Inspector** = the right rail, QCView's `InspectorPanel` carried ROW FOR ROW (the user 2026-11-07: "ensure we are faithfully porting over what we had for videos, some of those fields were specific user requests") on `qcv-meta`'s extractor + XMP reader via its service at open: File (Name, Folder tail-elided, Size, Duration, Container · MXF OP, Encoder), Video (Resolution, Frame rate, Total frames, Codec, Profile, Bitrate + "file avg" note, Pixel format + "RGB" note, Bit depth "+ alpha", HDR in the warn colour; ours: Decode), Geometry (Detected SAR, Display aspect + dims swapped for 90 / 270, Rotation "Auto (90°)"), Color (Colorspace, Primaries, Transfer, HDR metadata MaxCLL / MaxFALL / mastering nits, NCLC tag, Range tag + its note, the RANGE PICKER Auto-labelled-with-detected / Full / Limited → the decoders' override), Audio (Codec, Sample rate, Channels "(stereo)", Layout, the MIX PICKER Auto / 5.1 → 2 / 7-8 → the routing matrix, per-channel METERS, ours: Device latency), Timecode (the ORIGIN PICKER From start / Embedded / QT Start / QT TC / XMP Alt → the strip's base, Source + "(frame N)", QT Start / QT TimeCode / XMP Alt, Created / Media Created; ours: Drop frame) | **BUILT 2026-11-07**, refilled faithfully 2026-11-07 evening; Geometry's Aspect + Rotation pickers LIVE 2026-11-07 evening (the presenter fits with QCView's `effDim` + the rotation swap, the sampler inverse-rotates — `Engine::set_geometry`, `Geometry { par: Square / Detected / Custom, rotation: Auto / forced }`, Square + Auto by default; one test); Custom PAR too (the two decimal fields, QCView's clamps + rationalising); owed: Color's Transfer picker (the colour stage), Copy path / Reveal, Linked Projects / Playlist Contents (the project crate); the per-clip choices persist when media items exist (row 141) |
| 178 | **FILED: Windows software HEVC 10-bit 4K scrub** (rhombus 2026-11-07) | the scrub session serves 0 of 50 requests at a 40-ms cadence (each completes in ~400 ms at a slow cadence; ~3 threads busy) where macOS serves 49 at p50 1.2 ms on the same fixture; h264 4K and ProRes 4K software scrub are fine on the box → the session's thread count / the BtbN hevc decoder on Windows, not the GOP walk | open — next Windows session (docs/phase2/step2c-rails-settings.md § Windows) |
| 185 | **The pointer grab** (found 2026-11-07 night via "scrubbing overruns until the end") | a press on a band node (scrubber · transport button · volume slider) owns Move / Up until the release, wherever the pointer is — the hit-tested rail / strip routing yields; Qt's MouseArea grab QCView relied on | **FIXED 2026-11-07 night**; closes the parked Windows "clicks lost after leaving and returning" (same swallow; confirm on the next rhombus pass) |
| 186 | **The stream decoder opens PAUSED and the scrub / shuttle press pauses it unconditionally** (QCView `VideoDecoder::open` + `TimelinePanel.qml pressAt`) | a live decoder beside a paused timer refilled the publish slot behind every scrub preview, so the viewport "played" toward the pointer on heavy media (398 stream frames vs 132 previews in the 2026-11-07 trace); the scrub bench now FAILs on any stream frame shown mid-sweep | **FIXED 2026-11-07 night**; owed: the timer seeked once on release (QCView), scrub skim audio |
| 187 | **ONE FFmpeg per machine, pinned in the shared repo** (the user 2026-11-07 night: "move it to this shared repo so all apps can use it") | `tools/ffmpeg/ffmpeg.pin` = 9.0.2 (no 9.1 — still beta, no ProRes RAW gains yet, the user); macOS = Homebrew via pkg-config, Windows = BtbN gpl-shared (Vulkan + libplacebo + d3d12va) fetched by `tools/ffmpeg/fetch-win64.ps1` into the gitignored `external/ffmpeg/win64`, sha256-verified; every app — Rust via `ffmpeg-sys-next`, Qt via CMake — reads `FFMPEG_DIR` from there; binaries never enter the public repo | **RULED + BUILT 2026-11-07 night** (QCView's `external/ffmpeg-win64` left untouched) |
| 188 | **`min-ffmpeg` — the FFmpeg RAII as a shared crate** | lift the session RAII, the frame index, the hw routing and the hwaccel contexts out of `qcv-player` into a permissive crate beside `min-paths`, `qcv-player` its first consumer; S6's Vulkan ProRes bridge is written INTO it once, the Qt apps take it at their ports (canon §4's adoption order) | **BUILT 2026-11-07 night** (the user: "lets do it"): `crates/min-ffmpeg` = ffmpeg · frame_index · frame_handle · hw · hw_routing · read_ahead · rgb_range · seek_compat · source · thread_policy · yuv_planar · decoder_cleanup + the `Error` type, moved by `git mv`; `qcv-player` re-exports them so its paths and the apps' imports are unchanged; tests 3 + 15 + 11 green, the harness paths unchanged; GPL-3.0-or-later per row 151 (first published as MIT OR Apache-2.0 by mistake, corrected 2026-11-08). S6 writes the Vulkan ProRes bridge into it. Still in the player: the audio FFmpeg session (`audio/ffmpeg_audio.rs`) — follows when audio is shared |
| 189 | **The image libraries** (proposed 2026-11-08) | `min-image` links TWO C libraries — OpenEXRCore (row 148) and libjpeg-turbo's TurboJPEG (JPEG is lossy: the pixels are the decoder's, and the pure-Rust `zune-jpeg` measured 2–4 code values off QCView's libjpeg-turbo) — and decodes PNG + TIFF in pure Rust (`png`, `tiff`: lossless, bit-identical to FFmpeg on every layout tested, no per-OS pin). Pins in `tools/image-libs` (Homebrew on macOS, vcpkg `OPENEXR_DIR` / `TURBOJPEG_DIR` on Windows). Cost: palette / YCbCr / Lab TIFFs become broken frames where QCView read them | **RULED 2026-11-08** with the TIFF gap closed (the user: "Cover those TIFFs too"): what the `tiff` crate refuses (palette, subsampled YCbCr, CIE / ICC Lab, old JPEG) falls back to QCView's own path, libtiff's `TIFFReadRGBAImageOriented` → 8-bit RGBA — a THIRD C library (`libtiff-4` by pkg-config / `TIFF_DIR`), only on that fallback; palette = FFmpeg's decode, YCbCr 4:4:4 / 4:2:2 / 4:2:0 / 4:1:1 = libtiff's `tiff2rgba` |
| 190 | **DPX sequences** (beyond QCView, which opened a `.dpx` as a one-frame video) | a native Rust DPX loader in `min-image` (`dpx.rs`: RGB / RGBA / ABGR / luma; 8 / 10 / 12 / 16 bits; both byte orders; 10-bit filled A / B and packed; 12-bit A / B; orientation 0–3; RLE and transposed refused), widened 10 / 12 → 16 by bit replication; `.dpx` moves from the video table to the image table so it detects as a sequence and rides the sequence cache; shown as its code values (no log transform) | **RULED 2026-11-08** (the user: "Native Rust DPX loader") — **BUILT 2026-11-08**: 13 layouts IDENTICAL to FFmpeg's decode; a 10-bit sequence's frame 30 encoded-identical to the image2 route, display within one code value |
| 191 | **TGA and BMP** (QCView read neither) | the `image` crate's pure-Rust decoders, built with those two formats only; 8-bit RGB / RGBA; in the image table | **RULED 2026-11-08** (the user: "Support them natively") — **BUILT 2026-11-08**: IDENTICAL to FFmpeg (a 32-bit BI_RGB BMP is opaque by the spec) |
| 192 | **The playlist's contents in the Media section** (QCView's Inspector card "Playlist Contents" moved left — the right rail starts collapsed, and the playlist is made and filled in the left rail) | the LOADED playlist's row opens into a tree of its clips (indented, type glyph + name, elided in the middle); the playing clip in the quiet selected grey (no blue, row 162); a click seeks to that clip (QCView `seekToPlaylistItem`); the clip count + canvas on the playlist row, subdued; DRAG TO REORDER (the user 2026-11-08: "is it possible to drag/drop reorder in this tree?"): past 6 px a row drags, an insertion line between the rows + the row's chip under the pointer, the release = the clip bar's one undoable move; a click seeks on release; BIN ROWS DROP INTO THE TREE (the user 2026-11-08: "accept drag drops from the main media bin to add new items to the playlist" — ours; QCView's bin took no drops on a playlist): over the clips at the line, on the playlist's row = append, the clip bar's undoable insert (audio refused); the tree comes from the playlist editor, so an EMPTY loaded playlist takes drops too; trim / remove stay on the clip bar, Delete keeps meaning the project's rows | **RULED + BUILT 2026-11-08** (the user: "I am thinking this makes more sense under the media panel in the left" … "go with the tree") |
| 193 | **Paths in the Inspector + Linked Projects** (the user 2026-11-08: "talk about adobe links and the open/reveal chips for them and the paths in the inspector") | QCView's File-card actions (Copy path · Reveal; a sequence acts on its first frame) as the card's last row, and its Linked Projects card (After Effects · Premiere (Mac) · Premiere (Windows): the app, the path in native separators elided in the middle, Open · Copy · Reveal) after Timecode — quiet small action buttons; OURS: Open / Reveal disabled with a tooltip ("Not reachable from this machine") when the path does not exist here (QCView's did nothing); Reveal = `open -R` / Explorer's raw `/select,"…"` (QCView's quoting); Open = the OS association. Cross-OS mapping (min-paths `translate`, row 121) waits for a mapping table + its Settings section. MXF: only the fields our functions use are scanned into them, no MXF card | **RULED + BUILT 2026-11-08** ("go with your recommendations, mapping later"; MXF: "just what we need for our functions") — the XMP reader also learned the creatorAtom form of the AE link newer After Effects writes (149 / 149 linked MOVs now agree with exiftool) |
| 194 | **The Windows menu bar is QCView's in-window bar, always dark** (the user 2026-11-08: "we need an always dark mode menu for windows with the deliberate coloring from like the old app"; option 2 chosen: "lets do option 2") | QCView never used a native Windows menu: Main.qml's Qt Quick `MenuBar` drew in-window, pinned to its Theme (surfaceRecess strip + divider hairline; open title = affordanceHover grey, bright text, never the accent — the user 2026-10-21) with `ThemedMenu` popups. The 1ec363f Win32 menu (muda) followed the OS scheme and system colours. BUILT 2026-11-08 (875d616): `min_ui::MenuBar` (tokens `menu-bar-*`: height 24, background-base, divider hairline, title fill = hover grey) whose popups share the context menu's rows (`menu::paint_row`: rounded hover, check gutter, submenu caret, cascade right / left at the edge); Windows' pointer rules (press title opens, hover switches, hover a submenu row cascades, release chooses, press outside closes + is eaten) and keyboard rules (Alt tap / F10, Alt+letter, arrows, Enter / Space, mnemonics underlined only while the keyboard drives, Escape steps back). `apps/qcv/src/menu.rs` = ONE `spec()` → muda NSMenu on macOS (muda now macOS-only) and the in-window bar elsewhere (`QCV_MENU_BAR=1` on macOS); rails + picture start below it; fullscreen / compact hide it (QCView's `menuBar.height` 0); a menu closes on focus loss; Windows' About = QCView's own modal (Docs / Close) instead of muda's message box. Tests: min-ui 3 (pointer, cascade, keyboard), qcv 1 (spec → bar). Snapshot `QCV_UI_SNAP_MENU=1 --ui-bench` identical on both OSes. DEVIATIONS: rows are our 32-px `menu-item-height` (QCView 26) so the bar's popups match the context menu; no popup fade-in yet. WALKED 2026-11-08 on rhombus (the user: "looks great"); the lone-Alt tap without a native menu was not reported on either way — reopen if Alt ever does nothing or eats the next key. | **RULED 2026-11-08; BUILT; WALKED 2026-11-08** |
| 195 | **Translations: QCView's catalogs carried as gettext `.po`, English as the key** (the user 2026-11-08: "we can carry over our translations if it makes sense because they have been triple checked, but I am not tied to the system"; the recommendation taken: `.po` over Fluent — Fluent needs invented ids and rewrites every checked message — and over keeping Qt's `.ts`, which nothing in Rust reads) | `tools/i18n/ts2po.py` converted QCView's 12 `.ts` (11 languages × 866 messages + the English plurals) into `apps/qcv/i18n/qcview_<code>.po`: msgid = the English, msgctxt = QCView's context (+ `|disambiguation`), `#.` = the developer comment, Qt's numerus forms = gettext plurals in the same order; obsolete rows dropped; QCView's GLOSSARY rules stand (key names and shortcuts stay as written, one `&` per menu title, every `%1`/`%n` kept). `crates/min-i18n` (MIT OR Apache-2.0): a lenient `.po` reader, a process-wide active language (`register` / `set_language` / `resolve`: "de_AT" → de, "pt" → pt_BR), `tr` / `trc(context, …)` returning the catalog's `&'static str` or the English, `trn` with Qt's plural rules (English / French-style for fr + pt_BR / Polish / East Slavic / none for ja, ko, zh), `fill` for `%1..%9`, `trc_hint` (our tooltips' "(key)" suffix kept when only the head is in the catalog). Each panel looks up under its QCView context (InspectorPanel / ImageSequenceInspector / LeftRail / Main / TransportBar / CompactStrip / TimelineStatus / TimelinePanel / qcv::WindowManager) — the few English words with two translations (Range, Display, Media, "(none)") resolve right. Ids a click dispatches on stay English (the Inspector's chips carry an `id`). Test: every catalog parses whole and keeps its placeholders. | **RULED 2026-11-08; BUILT (the slice)** |
| 196 | **No "machine translation" label** (the user 2026-11-08: "lets drop the label. If someone wants to edit them, we will let them, but no need to add more ui language than we need") | QCView flagged all 11 languages unreviewed (`kReviewed` = en only; every row `unfinished`) and suffixed "· machine translation" in its picker. The carry marks every row reviewed and the picker (row 197) shows plain native names. Corrections stay welcome — the `.po` is the file a translator edits. | **RULED 2026-11-08** |
| 197 | **Localisation in two halves: the slice now, the pass at the end** (the user 2026-11-08: "do the small slice now, then back to panels"; and "Lets save packaging until after we do color and dual views") | NOW (built with row 195): every user-facing string in apps/qcv goes through the lookup (Inspector, Settings, menus, transport + compact strips, status strip, Media / Background / clip popover, the shell's toasts + file dialogs + About); new panels write `tr`/`trc` from day one and REUSE QCView's English (row 131) so they arrive translated; `QCV_LANG=<code>` runs a language (harness: `QCV_LANG=de --ui-bench N --ui-snap X.png`). Snapshots 2026-11-08: German + Japanese render (macOS falls back to a system CJK face on its own). THE END PASS (after the panels): the Settings ▸ Interface ▸ Language picker (native names, `ui/language`, system default), live switching (panels rebuild their labels), extraction of the strings that are ours + a first pass with the glossary, `QCV_PSEUDO_LOC` + `QCV_TRANSLATIONS_DIR`, the CJK fallback check on Windows, clipping (German "Dunkles Schachbrett" already overruns its Background chip). QUEUE ORDER after this: the panels → colour (§18.6) → dual views → packaging. | **RULED 2026-11-08** — **THE END PASS BUILT 2026-11-09** (the user: "I think we are ready for the localization right?"; on who writes our strings: "yes please" to drafts by the assistant, findable, unlabelled per row 196): the Language picker (Settings ▸ Interface; QCView's rows — System default + native names, QCView's 190-px width, `ui/language`, "Applies at once"; System default = the OS's preferred languages via `sys-locale`, the first we carry, English ends the search); LIVE switching (`min_i18n::generation` bumps on every `set_language`; the shell relabels once per change: each panel's `relabel()` takes the constructor's words and keeps its state — sections' open state, values, selections; the Settings panel is rebuilt whole since its values come from the store; the Inspector and Color drawer rebuild their models; dialogs rebuild when closed; menus rebuilt, macOS's NSMenu re-installed; min-ui's own words — Hex, Filter…, the rail tab's tooltip, now QCView's "Expand panel" / "Collapse panel" — translate at paint); the strings that are ours: `tools/i18n/extract.py` (literal lookups per helper context + the indirect calls) and `QCV_I18N_MISSES=<file>` (min-i18n records every lookup the language lacks; runs in the app and in `--ui-bench`) found 131; DRAFTED in all 11 languages with QCView's glossary terms and forms of address, merged by `tools/i18n/add_drafts.py` (refuses changed placeholders or a lost key hint) with `#. drafted 2026-11-09, unreviewed` on each — one search finds them; `extract.py` now reports 0 missing; test `every_label_is_translated` holds the shortcuts card to every catalog; `QCV_PSEUDO_LOC=1` (accented, ~40 % longer, bracketed; placeholders and key hints kept) and `QCV_TRANSLATIONS_DIR=<dir>` (loose `qcview_<code>.po` ahead of the embedded) carried from QCView; clipping: `Fonts::layout_fit` elides with … — action-button labels (German "Dunkles Schachbrett") and picker values; ⌘ / ⌥ drawn from Phosphor in every text layout (Geist has neither: tooltips and the card showed boxes); `QCV_UI_SNAP_RELABEL=<code> --ui-bench` = the live switch in the harness (English → ja snapped). LEFT AS QCVIEW: the project name "Untitled" (stored data, untranslated in QCView too). CJK ON WINDOWS checked on rhombus 2026-11-09 (605eb75): ja / ko / ru rendered at once; zh_CN drew boxes (设 / 项 / 显) — the Han fallback chose a Japanese face — until min-ui gave every layout the interface language as its locale (`fonts::ui_locale`); now whole. OWED: a person's review of the drafts, the Background chips' elided labels have no tooltip yet. |
| 198 | **minColor is the default engine; OCIO is the option** (the user 2026-11-08: "make min color the default and OCIO is a secondary option that people can use if they want. All our presets move to mincolor. And we carry no presets for OCIO, but still allow the user to make them"; then "we still bundle … but we only need Blender 5.2, ACES 1.3, and ACES 2.0 -- the legacy blenders can go. The knee stays--its useful for reviewing super bright HDR material regardless") | Every built-in preset is a minColor preset; QCView's per-output triples (→ sRGB / → EDR P3 / → PQ) collapse to one per input since the output is always the linear surface (row 179). OCIO keeps its engine, user-made presets and the bundled CONFIGS Blender 5.2 · ACES 1.3 · ACES 2.0 (Blender 5.1 / 4.x dropped); no built-in OCIO presets (reading to confirm: "bundle" = the configs). The Highlight Knee stays in both engines. | **RULED 2026-11-08** |
| 199 | **The Color panel: a bottom drawer above the Timeline panel, the waveform inside, no output rows** (the user 2026-11-08: "move the waveform into this panel since we no longer need rendering/display/output rows … move this panel above the timeline panel and put an up carrot button on the viewport to toggle it like our side rails"; "Correct on keeping our menu timeline buttons and shortcuts and pushing the image up") | The drawer pushes the picture up as the rails push it sideways; an up-caret on the viewport toggles it like the rails' tabs; QCView's Color Panel menu item, transport button and Ctrl+3 stay; hidden in fullscreen / compact. The output / display rows go (one surface); Rendering (Knee / AgX / OpenDRT) becomes part of a preset. The preset UX is to be RETHOUGHT (the user: "its a bit confusing how to save presets") — sketches owed after a read of QCView's panel + Guide 05. Vectorscope: open. | **RULED 2026-11-08** (UX sketches owed → rows 205–208). **SLICE 1 BUILT 2026-11-08:** `apps/qcv/src/color_drawer.rs` + `color_model.rs` + min-ui `ListColumn`: the drawer full-width on the bands (rails + picture pushed up, 280 px default, open + height in `ui.layout/colorDrawer*`), the caret tab at the picture's bottom centre (fades like the rails' tabs), View ▸ Color Panel + the transport's palette button + Ctrl+3; the header (clip · Own chain / Following the project default · Reset to default · Apply to all clips · Make project default); the preset list (Custom (from …) · minColor built-ins · Yours with an engine tag; Save as preset… via a name popover; Rename… / Update to this chain / Delete on right-click); minColor's columns (Input gamut · Input transfer · Highlight Knee · Rendering = Un-tone-mapped / AgX, an OpenDRT look shown when a chain carries one); the one On / Off (`color/enabled`). The chain per clip in `minColorClip` / `ocioClip` (QCView's keys + ours), the project default as an optional top-level `colorDefault` (written only when set: the 33 real projects still round-trip byte for byte), presets in `color-presets.json` beside the settings. Engine: the shared minColor pass (built ahead at startup) takes chain changes as uniforms; an OCIO chain builds on a worker. Snapshot `QCV_UI_SNAP_COLOR=1 --ui-bench`. NEXT slices: OCIO's columns (+ New OCIO chain…), the viewer aids row, the waveform. |
| 200 | **OCIO runs as native shaders; no baked LUTs** (the user 2026-11-08: "We will not be using baked luts since there are infinite possibilities, but will do native shaders--how we use the shaders in our setup is up for discussion as we want to keep everything as low latency as possible and GPU based") | Banked row 182 is REJECTED; rows 91 (MSL / HLSL passthrough, exact on both OSes in S1) and 94 (OCIO in-process via our own FFI) come back. Row 92's baker survives for LUT EXPORT only. The pass architecture is the next talk (§18.7). Rows 179 (one linear surface; the user: "we are only outputting linear sRGB EDR and scrgb"), 181 (Rendering kept, Display removed) and 183 (one patched display; the user picks config · input · view · look) are RULED with it; row 184 (the gate: row 130's HDR-monitor session on rhombus) stands. | **RULED 2026-11-08** |
| 201 | **Desktop vs Video, a viewer aid on the exposure / gamma row, Video the default** (the user 2026-11-08, on emulating a BT.1886 monitor: "that might be another toggle on the bottom exposure/gamma row Desktop vs Video and perhaps video is even the default as that would be less confusing for viewers who are not mentally prepared for the sRGB vs bt1886 gamma differences"; "I love this because it becomes a universal app thing") | A DELIBERATE deviation from QCView (whose default was as stored = Desktop): by default SDR video is shown as a BT.1886 2.4 display shows it. Scopes read the signal, never the viewing condition. REFINED the same day: inputs are never touched (the user: "I don't want to adjust inputs on mincolor or ocio as they need to behave the way the user is expecting") — Video would be one blunt last-pass adjustment on the finished image, `out = in^(2.4/2.2)` (open: pass-through above 1.0; the overlap with a Rec.1886 chain Input; which side is the default — Desktop matches every other player's familiar "2.4 wash", Video shows the reference look). THEN DEFERRED (the user 2026-11-08: "lets keep the video toggle in mind and build the rest first--not disregarding it, but not doing it until we understand our working flow better … perhaps we end up using 'views' for OCIO and alt 'display' for mincolor instead"). The cost it answers is row 179's: a linear-only surface has no BT.1886 view to count on. | **DEFERRED 2026-11-08** (not disregarded; revisit once the chain and drawer run) |
| 202 | **The working image is half float** (the user 2026-11-08: "I am fully onboard with half float -- even without perfect exactness. Most VFX pipelines are half float anyway--full float is overkill for ACEScg type linear flows") | The linear working image between the convert stage, the colour chain and the screen pass is RGBA16F at the video's size (half the memory traffic of full float). Exactness against full float is NOT a gate; the error may be measured once for the record, never to block. | **RULED 2026-11-08** |
| 203 | **Waveform only; the vectorscope is dropped** (the user 2026-11-08: "vectorscope is gone. I don't use it. We are not a color correction software and Waveforms is all we need with our same SDR/HDR logic to scaling the waveform view") | Amends row 146: the scopes are QCView's waveform alone, with its SDR / HDR scale logic (the tiers, the ranges, the peak pass, the 203-nit hook), living in the Color drawer (row 199). CORRECTED 2026-11-08 (the user: "correct, we want it via the input/transfer like qcview" — Claude had written "after the chain" without reading QCView's code): the scope reads the SOURCE through the clip's Input, as QCView does (docs/mincolor.md: "The scopes read the source through the clip's Input") — minColor's Input half (linear Rec.2020, 1.0 = 100 nits), OCIO's input → interchange role (QCView's OCIOScope processor) + the fixed matrices, bypass the stage-1 decode of the file's tags (QCView's Assumed tier) — never the Look, View, Knee, display or viewer aids; SDR scale = Rec.709 through a pure 2.4 inverse, Y′ −10…110 %; HDR scale = luminance in nits, 0…peak (300 / 600 / 1 000 / 2 000 / 4 000). The vectorscope's maths are not ported. Also 2026-11-08: the user can switch the HDR monitor to rhombus on request ("I can toggle the HDR monitor on windows whenever you need it") — row 184's gate is available whenever the surface is ready to check. **BUILT 2026-11-08 (91fe4bc):** min-present `ScopeView` + `scope.wgsl` (the tap → a 512² atomic histogram at ≤ 960 columns → drawn over the UI in the drawer's card with QCView's 1 − e^(−gain·count) brightness, out-of-range bins red); taps: bypass = stage 1's linear decode of the tags, minColor = a second output of its kernel right after `drt_input_transform` (no new compile), OCIO = QCView's OCIOScope (input → interchange role) + the fixed matrices; tests: the ramp at 0 % / 100 % (SDR) and 100 nits (HDR), the viewer aids never move it, minColor's tap with AgX on is the Input stage. The card: SDR / HDR chips (HDR by default for PQ / HLG sources), the HDR top cycled through QCView's five, the graticule; collapsible like the Knee and AgX; the drawer's columns scroll sideways past the window's width. Not carried: QCView's Signal tier for untagged files (our decode always assumes a transfer), the peak readout. | **RULED 2026-11-08; BUILT** |
| 204 | **The pass design (§18 #5), delegated** (the user 2026-11-08: "#5 is the part that I know the least about so I am leaning on you to help me through it"; "build the rest first") | Low latency = few full-resolution passes, no CPU readback, no mid-playback compiles. (1) minColor = ONE WGSL compute pass at video size (Input → Knee → Rendering), every control a uniform — a change never recompiles; ported from QCView's MSL / HLSL shims + the vendored OpenDRT / AgX and checked against QCView. (2) OCIO = its generated MSL / HLSL run unchanged through wgpu passthrough as ONE pass (S1: exact, 0.3–0.5 ms at 4K); a new preset compiles in the BACKGROUND and the previous chain stays on screen until it is ready; OCIO's dynamic properties ride as uniforms; with the Knee on, QCView's split (OCIO pre → Knee → OCIO post). (3) The chain runs once per new frame or setting change; viewer aids stay in the screen pass. (4) The waveform is a GPU compute pass over the chain's output. (5) An engine switch swaps the slot only. Later, measured first: folding the convert stage into minColor's pass. | **RULED 2026-11-08 by delegation** — reviewable when the chain runs. **minColor BUILT 2026-11-08 (`crates/qcv-color`, 9fc8c64…693793b):** the vendored minColorAE core (57eeebb) compiled at build time by naga's GLSL front end through minColorAE's own GLSL shim + our host shim (C semantics for atan2(0,0) / pow edge cases — wgpu compiles Metal with fast math and the first OpenDRT run gave NaN on black; QCView had switched fast math off); the AgX block read from a flat std430 buffer via a loader generated from the header; host derive = QCView's resolve in C++ (C++20), display fixed to the linear Rec.709 hand-off. GPU vs the C++ core on default / AgX / OpenDRT 1000 / Knee / P3 PQ / three looks incl. saturated, out-of-range and HDR inputs: worst 9.5e-4 (Metal) / 9.6e-4 (DX12) relative at f16, no NaN. 50% grey ProRes: bypass 127, Rec.709 chain 119 on both OSes. Cold pipeline build 0.5 s macOS, 1.5–5.3 s Windows (FXC) → built on a worker at startup; OWED: a compiled-shader cache or DXC on Windows, the chain run only on a new frame / setting (it runs every refresh today), the 4K pass timing. App: harness-only `QCV_MINCOLOR=<chain>` until the drawer. **OCIO BUILT 2026-11-08 (161e4e2, c5c7b26):** OpenColorIO **2.6.0 pinned on both OSes** (Homebrew / vcpkg) in-process; `src/ocio.cpp` gives every config a **"Linear Screen" display built in code** (display-linear Rec.709, XYZ D65 → Rec.709, 1.0 = 100 nits) whose views are the config's own gathered from its displays (sRGB display first, first name wins) — no hand-made EDR patch per config (Blender 5.2 → Standard / AgX / ACES 2.0 HDR … ; ACES 2.0 → 31 views incl. the D60 sims; ACES 1.3 → 18); validation is a warning (OCIO 2.6 rejects an interop ID in QCView's ACES 2.0 EDR patch). Chain = input → [look] → [scene LUT / CDL] → the display's view; OCIO's MSL / HLSL in QCView's compute wrapper via passthrough. GPU vs OCIO's CPU processor on six SDR / HDR / AgX chains over the three configs: worst 9.0e-4 (Metal) / 9.6e-4 (DX12); Standard on linear Rec.709 = identity (the matrix). Builds 83–151 ms macOS, 22–44 ms Windows. App harness `QCV_OCIO=<config>|<input>|<view>[|<look>]`; grey ProRes on both OSes: Rec.1886/Standard 119 (= minColor), sRGB/Standard 127 (= bypass), ACES 2.0 SDR 144. Configs read from QCView's checkout (`QCV_OCIO_CONFIGS`) — where they ship from (19 MB of Blender LUTs) is a packaging decision. OWED: OCIO's dynamic properties, display LUT (if kept). **THE KNEE FOR OCIO BUILT 2026-11-08 (8b478d0):** QCView's split (pre = [look] [scene LUT] → aces_interchange / cie_xyz_d65_interchange; post = → the view) around a second, small naga kernel from the vendored core (shim + params + `mincolor_knee.h`: QCView's matrices into linear Rec.2020, `drt_knee`, back); an SDR view targets 100 nits (the view name decides, one display); numbers = a buffer write, on/off = a new pass. Three split chains vs OCIO's CPU halves + the C++ knee: worst 5.5e-3 against the pixel level on both OSes (two extra f16 hand-offs, row 202), and the Knee bites. Harness `QCV_KNEE=<src>,<tgt>[,<start>]`. Step 3 (engines) DONE; next = the preset talk (step 4). |
| 205 | **One whole chain per clip, with a project default** (the user 2026-11-08: "1 yes if you agree" — Claude agreed) | Replaces QCView's clip half (Input · Look · Scene LUT · Knee, per clip) + view half (Output · View · Display LUT, everything on screen): a clip's chain is the whole thing — Input, Look / Scene LUT, Knee, Rendering (minColor's un-tone-mapped / AgX / OpenDRT look; OCIO's view). Clips that never set one follow the project's default chain, and the drawer says which in words ("Following the project default" / "Own chain · Reset to default"), with **Apply to all clips** — the ↺ markers and the invisible default go. Dual view: each side is a clip, so each has its own chain. Storage: the per-clip maps `.qcvproj` already carries opaquely (`ocioClip`, `minColorClip`) gain the view-side keys, so the byte-for-byte contract (row 141) holds and QCView still reads the clip-side pins. | **RULED 2026-11-08** |
| 206 | **Saving presets: the Custom row** (the user 2026-11-08: "2a") | Presets are starting points, never live links: picking one copies its chain onto the clip. Editing any column turns the clip's chain into an unsaved **Custom (from <preset>)** row at the top of the list with one action, **Save as preset…**; Rename · Update · Delete live on a preset's right-click menu (built-ins: none). Replaces QCView's Save / Save as… / Delete + italic "modified" — a preset can no longer be overwritten by accident. | **RULED 2026-11-08** |
| 207 | **One preset list for both engines** (the user 2026-11-08: "3 yes") | minColor's built-ins first, then the user's own (minColor and OCIO, each marked with its engine); picking an OCIO preset puts the clip on OCIO — the OCIO \| minColor segment goes. A **New OCIO chain…** row starts an OCIO chain (it asks for the config: Blender 5.2 · ACES 1.3 · ACES 2.0, or a file). | **RULED 2026-11-08** |
| 208 | **Presets live in the user's library; projects carry chains, not preset references** (the user 2026-11-08: "I see the need for C, but currently usage is A. Projects will save the chain anyway right?" — yes) | Saved presets go to the user's settings, offered in every project (QCView's way). Each clip's whole chain is stored in the project (row 205), so a project shows the same picture on any machine without the preset. Project-level presets (option C: "Save as preset…" asks Mine / This project; a second list section) stay possible later — one key in `.qcvproj`, one section — and nothing built now may preclude it. Caveat: a Scene LUT is stored by path (min-paths maps drives / mounts, not a missing file). | **RULED 2026-11-08** |
| 209 | **Dual view's waveform overlays A and B** (the user 2026-11-08 picked "Overlay A + B") | QCView's dual scope: both sides on one waveform, A cyan, B orange, each read through its own clip's Input (row 203); the readout per side. | **RULED 2026-11-08** |
| 210 | **Annotations and safety guides are drawn by the presenter, in the picture's space** (the user 2026-11-08 picked "Presenter draws them") | QCView's approach: strokes and guides are part of the picture's GPU render in picture coordinates — they scale, rotate and letterbox with the image, stay with their own side in dual view, and a capture re-renders them at full resolution (the screenshots talk, after dual view and annotations). The UI layer only hosts the tools. | **RULED 2026-11-08** |
| 211 | **Dual playback: one master clock, each side our own Transport following it** (the user 2026-11-08: "I agree with your choices. If we can make those all better, we are better for it") | QCView's behaviour (src/dual): a wall-clock master at the higher side's rate; master length = the longer side; each side picks lround(master s × its fps) (the lower rate repeats); a seek while playing holds the clock until both sides land (2.5 s cap); step in master frames; scrub through each side's scrub decoder; shuttle 2× → 32× on the 33 ms tick; loop on by default; a gap or a shorter side shows the BACKGROUND (no hold); from a playlist, the clip under the playhead becomes A; B is never audio-only. Ours differs where QCView is worse: (a) the sides REUSE our streaming / scrub / sequence decoders (a `Transport` per side in follower mode — the clock is the master's; gapless cuts, landings and sequences come for free) instead of QCView's dedicated dual decoders, which existed only because its single decoder paced itself; (b) the loop wraps at out + 1 with the sub-frame carry (QCView skips the out frame), loop off STOPS at the end (QCView ran on, blank), image sequences keep their own rate in dual (QCView: always 24); (c) changing B replaces B's side only — the playhead, in / out and A's edits stay (QCView rebuilt the dual view, a use-after-free workaround); (d) live sources wait for the live port. **Step 5 BUILT 2026-11-08 (audio)**: each side drives its own audio player (QCView's DualAudioMixer mixed two decoders into one device; here two streams, each on the side's drift loop, the OS mixing at unity) under a side-gain stage — QCView's per-side mutes (A heard, B muted by default; the chips' speaker pills with its tooltips) and silence where a side has no picture (a gap, past its end); master volume / mute drive both; `audio/dualSyncOffsetMs` live with QCView's defaults (17 macOS / 15 elsewhere). Also: the trim / slip drag previews on its side (QCView `requestScrubFrameForSide`). | **RULED 2026-11-08** |
| 212 | **Dual editing without an edit mode: precision first** (the user 2026-11-08: "What is missing from dragging to trim is frame precision" … "lets try it out. Its worth a shot") | QCView gated ⌘K / Delete on per-track edit-mode toggles (TimelinePanel.qml: ⌘K cuts the tracks in edit mode); row 167 retired the mode, so: (1) a TARGET side (the last clicked bar or chip; B by default) takes ⌘K and the keys; (2) drags SNAP within ~8 px to the playhead, then the other side's cuts, in / out, this side's edges, with a snap line; (3) edit-to-playhead keys on the selected clip (else the target side's clip under the playhead): `[` trims the head to the playhead, `]` the tail, ⌘K cuts, Alt+← / → slides 1 frame (Shift 10); (4) the drag badge shows the delta and the resulting frame; (5) moving a clip CARRIES the later clips on its side (one version of the cut), Shift moves it alone; Delete leaves a gap (the background shows), Shift+Delete closes it; every edit undoable. Trial ruling — revisit on the walk. **Walk 2026-11-08** ("that works very well"): the timeline GROWS to the left — a slide / nudge / head trim past frame 0 moves every other clip (both sides) right by the overshoot, the playhead with them, one undo step (QCView clamped at 0); the offset against the other side ("B −14 f vs A") reads in the STATUS STRIP right of the timecode — the hovered clip, else the selected, else the target side's under the playhead (first a hover badge over the scrubber: "a bit inconvenient"); the lanes went NEUTRAL (the user: "These colors are a bit intense … something more spectrum 2 based" → "lets go with your recco"): the playlist bar's greys, the rows LABELLED instead (the user: "the line across the clips doesn't work because it no longer feels like one unit … label their rows"): an A / B cell at each lane's left end in the side colour, the target lane's cell filled — the chips lose their target outline; the lanes are 16 px each (12 at first — "a few pix taller for ergonomics"; the dual band +32); the scrubber's end inset is 28 px on every OS (the label cell — 16 sat the letters on the rounded corners: "the idea works with more padding"); clips are drawn clipped to the lane (zoomed tight on a long film, off-screen extents broke vello's coverage — both bars); macOS: the scrubber's camera insets 16 px past the window's rounded bottom corners so the ends' trim zones stay reachable. | **RULED 2026-11-08** (trial; walked) |
| 213 | **Dual colour follows the one selection** (the user 2026-11-08: "since we have chip selection for editing, do we use that for color too? Instead of tabs, just labels based on what is selected?") | QCView put A / B tabs on the colour panel's Clip half ("A's clip — changes stay with this clip"); here the TARGET side (row 212: the chip or lane last clicked, B by default) is also the side the colour drawer edits — no tabs; the drawer's label leads with the side letter in its colour, then the clip. Each side binds its OWN clip's chain on its engine (a minColor pass per side, an OCIO build per side; row 205's clip-owned chains unchanged); the waveform taps both through their chains (row 209). The INSPECTOR follows the same selection (the user: "that makes sense for the inspector too"): it shows the target side's clip — its File header leads with the side letter in its colour ("Do we need to label File A File B"), and its Range / Mix / Aspect / Rotation / Transfer write that side's item and act on that side's decoders / engine / audio; each side applies its clip's choices as it loads (a split's rebuild keeps the Range). The waveform readout sits in a band UNDER the card (QCView's place; on the caption row it vanished when the chips left no room) — a line per side in dual; the brightest channel moved to its TOOLTIP as in QCView (the user: "move it to a tooltip like QCView"), QCView's `formatPeak` numbers. Localisation owed: QCView's `ScopeController` context was never converted into our catalogs (its two tooltip strings fall back to English). | **RULED + BUILT 2026-11-08** |
| 214 | **Annotation stroke smoothing: ours, in Rust** (the user 2026-11-08: "go with your recommendation for smoothing. We can reassess if we can't pull off nice smooth lines and matching exports") | QCView used Google's C++ ink-stroke-modeler. Here: (1) INPUT — a small smoother of our own: the modeler's spring-mass-damper position model plus a one-euro filter (heavy smoothing when the pen is slow, light when it is fast — no wobble, no lag); (2) GEOMETRY — the smoothed points fitted to cubic Béziers with kurbo (`fit_to_bezpath`), stored compactly in picture space and drawn by vello at any zoom; a perfect-freehand-style outline if strokes taper / follow pressure. No new dependency. Fallback if the feel or the exports fall short: the pure-Rust `ink-stroke-modeler-rs` (MIT / Apache-2.0, partial rewrite of Google's). | **RULED 2026-11-08** (trial) |
| 215 | **Annotations and safety guides: QCView's, with its bugs fixed** (the user 2026-11-08: "1a, 2a, 3a, 4a") | Carried: notes = one per frame keyed by SMPTE non-drop timecode (text, addressed, strokes, a clean + an annotated thumbnail), the SIDECAR `<media_dir>/.qcview/<file>/notes.json` + `images/` byte-for-byte (minNotes reads it — 1:1 forever), annotation_data schema 2.0 (normalized display-space points); tools Select / Freehand / Rectangle / Oval / Arrow / Line / Eraser with QCView's icons + tooltips, the colour picker (red default), width 1–24 (6), Ctrl+4, ⌘Z / ⌘⇧Z, Esc, Del; strokes over the colour chain in picture space (row 210) with row 214's smoothing. Safety guides = QCView's 13 SVGs, one at a time, fitted to the picture (aspect, PAR, rotation), a Safety Guides section (Guide / Opacity / Width / Color) + the transport button. RULED: (1) annotations in SINGLE view only (QCView's gate; dual revisited after the walk); (2) strokes show on their exact frame (QCView); (3) the Notes panel is a drawer in the colour drawer's slot — Ctrl+3 Color / Ctrl+4 Notes, one open at a time; (4) the guide, opacity, width AND colour persist (QCView kept only the colour). Fixed, not carried: undo kept across clips, redo losing a new note's frame, the clean thumbnail catching the first stroke, the selection baked into screenshots, bounding-box hit tests, ids changing every save, a one-point arrow saved invisible, Windows' 4-vs-6 width, notched guide corners. Exports (Markdown / HTML / PDF / DOCX) come with the screenshots / captures step. | **RULED 2026-11-08** |
| 216 | **Captures and thumbnails: the review picture, rendered for an SDR file** (the user 2026-11-08: "agreed, lets go" on the proposal) | QCView (read 2026-11-08): T = file to the Desktop (`qcview-screenshot-yyyy-MM-dd_HH-mm-ss`, PNG / JPEG q95 / TIFF per `export/screenshotFormat`), Ctrl/Cmd+T = clipboard; source → chain → viewer aids → 8-bit sRGB, the chain re-targeted to its SDR equivalent when the view is HDR (`setSdrCapture`: minColor at peak 100, OCIO the nearest SDR view); annotations baked, guides never (single); no chain = code values written as stored, labelled sRGB (PQ / HLG / log washed out); pixel aspect IGNORED (anamorphic squeezed); thumbnails inconsistent (hover: BT.601 sws, drawn through the live chain; Inspector / EXR tiles: raw, no chain). RULED: ONE pipeline for captures AND thumbnails — the source through its tags + the clip's overrides (range, matrix, transfer), the clip's own chain (SDR-retargeted when the view targets HDR), an sRGB encode, on the GPU (the screen's engine); bypass = as stored (row 180) — SDR code values exact, PQ / HLG sources mapped to SDR by a BT.2390 tone map (not raw code values); CAPTURES include the viewer aids (QCView; the note records them), THUMBNAILS never; pixel aspect applied, rotation applied, the correct matrix / range everywhere, files tagged sRGB; dual captures at source resolution (side-by-side = two native pictures, wipe / difference = one); safety guides in captures = an "include guides" setting, OFF by default; annotations baked. Later, on request: an HDR capture option (16-bit PQ PNG with a cICP tag). | **RULED 2026-11-08**; AMENDED 2026-11-08: the file is named after the media — `<clip file stem or playlist name>-yyyy-MM-dd_HH-mm-ss.<ext>` (the user: "media_name-datetime … instead of qcview-screenshot"), unsafe characters → `_`, `qcview-screenshot` only when nothing is named ; BUILT 2026-11-08 — dual captures: `Engine::capture_dual` (side by side = both native pictures, each centred vertically on black; wipe / difference = one picture at A's display size through the screen's compositor; regions carry each side's bypass peak + geometry), each side's chain SDR-retargeted, the guide over each side (side by side) or the whole (wipe / difference); a dual file is named after the saved Dual View, else `<A>_vs_<B>` ; BUILT 2026-11-08 — thumbnails: swscale given the frame's matrix + range (no more BT.601 default), then `apps/qcv/src/thumb_color.rs`: our own small engine on the presenter's device shows the R'G'B' thumbnail as planar RGB tagged with the clip's transfer + primaries, runs a pass built like slot 0's (minColor at peak 100; OCIO's nearest SDR view; bypass = as stored, ±2 codes), BT.2390 for HDR in bypass; the hover cache keeps the R'G'B' and redraws only the shown thumbnail when the look's generation moves; the Inspector's video hero the same (sequences keep their still loader's picture) ; 2026-11-08 later — ONE PATH for every thumbnail (the user: "go with your recommendation, build it"): `ThumbPixels` carries the source's own samples (video = swscale RGB48 code values; stills = the file's 8/16-bit codes ×257 or EXR half floats, linear; `rgba8()` the plain fallback), the look stage shows them as 16-bit / half-float planar RGB, so image sequences (hover, the Inspector hero, the EXR layer tiles) go through OCIO / minColor / bypass exactly like the screen; tests: an EXR ramp in bypass = the viewport's encode ±2 with its shadow steps kept ; 2026-11-08 — DUAL hover thumbnails BUILT per row 170 (the pair tracks the pointer, A above B — first built parked by mistake, corrected the same day; the user: "if it doesn't hurt, then maybe it does help"): each side's frame under the master hover through its own edits, its own cache + its own look (B through B's chain); guarded — nothing requested while dual plays or scrubs, no strip precache; the look stage costs 1.3 ms per new 320×180 thumbnail (bypass, M-series) |
| 217 | **Annotation exports** (the user 2026-11-08: "for PDFs and Word, we can stick to notes per page and keep it simple. Same for markdown, but HTML we can get fancy since our lightbox allow us to see full images without having to enumerate them below our tables") | QCView (read 2026-11-08, `annotation_exporter.cpp` + NotesPanel's Export panel): Markdown folder (`<name>/<name>.md` + `images/`), HTML (one file, base64 images), PDF, DOCX (hand-rolled OOXML); each = brand icon · media name · exported · source · per note: TC heading (frame, ✅ addressed) · image (annotated else clean) · italic OCIO / viewer lines · text. minNotes' HTML export (`Exporter.cpp`) = the family look: Geist + Geist Mono embedded, CSS tokens, the mono doc header + the measure toggle, note cards, the lightbox (‹ › / arrows / Esc / n of N, ink layered over the clean frame). RULED: **Markdown, DOCX, PDF = QCView's simple list, one note per page** (Markdown: one section per note); **HTML = the fancy one**: the family look in Geist, a header with counts, a SYNOPSIS TABLE (thumb · timecode / frame · the note · addressed · viewer / colour records) as the document — no per-note list under it; the minNotes LIGHTBOX opens any frame full size, the strokes on their own transparent layer over the clean frame (a toggle shows / hides them); names `<media>-yyyy-MM-dd_HH-mm-ss`; the Notes drawer's Export menu → a save dialog → a toast. Note thumbnails come from the capture path (row 216) and record QCView's viewer / OCIO tags (minColor named by its preset). | **RULED 2026-11-08**; BUILT 2026-11-08 — all four: `qcv-notes::export` (Markdown, HTML) + `export_doc` (DOCX = OOXML zip with a real styles.xml in Geist and relationships per image that exists — QCView's had neither; PDF = krilla 0.8, Geist embedded, skrifa-measured wrap, a long note continues on the next page; Letter, 15 mm, frames as JPEG q92) |
| 218 | **Keyboard Shortcuts: QCView's Right Rail card, with a filter, drawn from one table** (the user 2026-11-09: "I am wondering is the shortcuts is better as a modal or another window. there are so many"; the recommendation taken: "go with your recommendation, build it") | The card stays where QCView keeps it — the foot of the Right Rail, collapsed by default; Tools ▸ Keyboard Shortcuts and Ctrl+/ (⌘/) open the rail, expand the card and scroll its header to the top (QCView `revealSection`). NOT a modal (QCView's modals pause the picture, so a key can't be tried while reading it) and NOT a window (a second surface, and the keys would land in it). A FILTER field on top (label, group heading or key: "loop" → Toggle Loop · V · ⌘L; a heading keeps its group); same-label rows merge their caps; each binding its own recessed cap, ⌘ / ⌥ drawn from Phosphor (Geist has neither). `apps/qcv/src/shortcuts.rs` `GROUPS` is the ONE table (row 143's registry, the reference half): QCView's groups with the port's keys added (New Project, Quit, Always on Top, Notes Panel, annotation Delete, bins Select All, Playlist, Dual View); QCView's "Timeline (edit mode)" and the overview-drag rows gone (row 160 / not ported). Tests hold it to the shell: every transport binding goes through `qcv_player::map_key` and must answer the listed action; every menu accelerator must be on the card; Windows / macOS spellings. Labels go through `trc("RightRail", …)` (QCView never translated them — the localisation pass). Snapshot: `QCV_UI_SNAP_SHORTCUTS=1` (or a filter text) `--ui-bench`. OWED: tooltips generated from the table (they are catalog strings today); rail tooltips spell ⌘ in Geist (boxes, as the card did). | **RULED 2026-11-09; BUILT** |
| 219 | **2026-11-09 rulings + the owed items closed** (the user: "lets remove the timeline panel toggle since we have compact mode which is better … we should just use arial or something both systems will have … I don't want to visually insinuate they are less important") | (1) View ▸ Timeline REMOVED — Compact Mode replaces it; View ▸ Notes enabled (Ctrl+4 already worked). (2) The DOCX names Arial / Courier New (the PDF still embeds Geist). (3) The status strip = the transport row's height (`status-strip-height` → `{transport-bar-height}`, 32 px); dual view's mode / save / update buttons = the transport's size M in a compact group; the wells stay the review-speed chip's 20 px; `QCV_UI_SNAP_DUAL=1`. (4) Thumbnails follow the clip's Range override — hover, scrub peek, Inspector hero (`?range=full|limited` in the loader key, `range_key` / `split_range_key`). (5) Select-tool CORNER SCALING = QCView `annotationResizeTo` verbatim (uniform, aspect-locked, pivot = opposite corner, dominant axis, 0.03 floor, clamped to the picture, one undo step); handles hit within 9 px of the drawn outline's corners. (6) The notes sidecar WATCHER = minNotes `VideoNotesModel` (§4.5): another app's write reloads (less our tombstones; our own write's (mtime, length) is no reload), saves MERGE (theirs added unless tombstoned, ours wins a shared key) — by a stat twice a second, ONLY while the Notes panel is open (the user 2026-11-09) and paused, not `notify` (OS watchers don't fire on SMB / NFS, where review media lives). (7) Background chips with a cut label show a tooltip. | **RULED + BUILT 2026-11-09** |

**The rails (rows 174–177, RULED + BUILT 2026-11-07):** the user's two calls — Settings gains QCView's own sub-headers
(Performance / Interface / Audio) rather than one long list, and the right rail STARTS COLLAPSED now that the strip and
the go-to popover carry the timecode and the file facts. Pointer routing order in the shell: a modal dialog or an open
menu first → the rails (splitter drag, the open tab, then the panel inside the content rect; everything over a rail is
consumed) → the strip's popover → the bar → the scrubber. Keys: a focused number field / open menu / dialog first, then
Ctrl+1 / Ctrl+2, then the strip, then the player's key map. The store writes back what the bar and the keys change
(volume, mute, loop, the rail widths and collapse flags) and saves when dirty, once per tick at most. The headless
`--ui-bench` snap renders both rails (the settings with a switch's help tooltip and a focused field). Piece note:
`docs/phase2/step2c-rails-settings.md`.

### 25.4 Reference reading — rails, sliders, scrollers, and when to round (the user's second pointer, 2026-11-07)
The same Adobe image's bottom panel (the current Lightroom on Spectrum 2) is the reference for the rails. What it
shows, as rules for `min-ui`:
- **Rail = layer-1 over the viewport's base**, sections separated by full-width hairlines, no cards for the sections
  themselves: a section is a bold 14 header with a chevron on the LEFT ("⌄ Light", "> Effects" when closed). Sub-panels
  that open elsewhere ("Curve", "Color Mixer", "Point Color", "Color Grading") are the only cards: raised layer-2 tiles
  at radius 8 with an icon tile on the left and a small caret on the right.
- **Slider row** = label left and value right-aligned in subdued gray on one line, the slider on its own line below:
  a 1-px track (filled and unfilled halves one step apart in gray, never accent), the knob a 16-px hollow ring (dark
  fill, 2-px light stroke); gradient tracks only where the value is a colour (temp, tint, hue).
- **Rounding by role**: pills (`corner-radius-full`) for standalone action buttons ("Copy Edit Settings") and the search
  field; small radius 4 for chips and buttons inside a group ("Auto", "B&W", "HDR" as raised gray chips with 12-px bold
  text; the "★ ≥ 4 ×" filter tag); medium 8 for cards and for the lighter square behind the active tool in the icon
  rail; hairlines and dividers stay square. Quiet pickers ("Profile  Adobe Standard ⌄", "Fit 100 % ⌄") are text plus a
  chevron, no border.
- **Icon rail**: ~40 px wide, 20-px icons, groups separated by hairlines; the active tool gets the radius-8 lighter
  backing and a tiny accent dot — one of the four blue uses (row 162: the active side chip generalises to "the active
  tool / side").
- **Scroller**: a 2–3 px overlay bar at the rail's edge, no track, visible on hover / scroll — §16.4's fixed-thickness
  rule, drawn S2's way.
- **Type in the rail**: body 14 for headers and values, 12 for labels; the 2017 panel's tracked tiny capitals
  ("TOOLS", "HISTOGRAM") are the detail style the go-to popover's title already uses.

**Build order (row 156, revised by §25):** `min-theme` (**BUILT 2026-11-07** — the S2-shaped tokens, JSON source + build.rs → typed consts; the Qt `Theme.qml` generator later, row 37) → the UI layer (**BUILT 2026-11-07** — `min-present` stage 4 `set_overlay` + `crates/min-ui` `UiLayer` / `Fonts`: vello into an Rgba8 overlay
at window size, composited premultiplied after the video pass, UI white = SDR reference white, redrawn on dirty only; the token sketch of the bands in `apps/qcv` is the first picture of §25)
→ Tooltip + ActionButton → the transport bar → the timecode strip + go-to popover (S3's parley editor as a
single-line TextField; the AccessKit adapter attaches when `accesskit_winit` reaches winit 0.31 — the one written
row-79 deviation) → the scrubber node → the dual bars + popover. Harness: headless UI-layer renders to PNG goldens
under `fixtures/`, `--ui-bench` reporting scene-build + vello ms per frame; bar ≤ 1 ms CPU when the layer changes,
nothing when it doesn't.

## 26. The state of the port, 2026-11-07 night (handoff)

Written for whoever picks the work up next, in any model. `main` = c61b480 (public, GitHub cbkow/minplatform; this section's own commit); rhombus's clone
is fed by git bundles and sits at the same commit. Every ruling is dated in the rows above; this is the map.

### 26.1 What exists and runs on both OSes
- **The QCView port** (`apps/qcv` + `crates/qcv-player` + `crates/min-present` + `crates/min-ui` + `min-theme` + `min-settings`
  + `qcv-timeline` + `qcv-meta` + `qcv-project` + `crates/min-ffmpeg`): open · play · scrub (QCView's scrub decoder + GOP
  cache) · shuttle · in/out · loop · review speed · audio (CoreAudio / WASAPI, servo, tempo, routing) · waveform · hover
  thumbnails · the rails (Add Media / Background / Settings left, the Inspector right) · compact mode · fullscreen · .qcvproj
  byte-for-byte · the metadata service · stage-0 background · geometry (PAR / rotation) · Range + Transfer overrides · the
  as-stored bypass (row 180) · the pointer grab (185) · the paused-open stream decoder (186) · the event-loop heartbeat.
- **Hardware decode**: macOS VideoToolbox zero-copy (Metal); Windows D3D12VA zero-copy for the inter codecs and, since
  tonight, **FFmpeg's Vulkan compute decoder + the D3D12 bridge for ProRes / FFV1 / APV** (row 149, S6 — 4K ProRes HQ at 37 %
  of a core, pixel-identical to software). ProRes RAW stays software on both (as QCView).
- **One FFmpeg per machine**: 9.0.2, pinned in `tools/ffmpeg/` (Homebrew on macOS; BtbN gpl-shared fetched into the
  gitignored `external/ffmpeg/win64` on Windows); `ffmpeg-sys-next` 9.0.1; 123 symbols in use, none deprecated (row 187).

### 26.2 Decisions banked but NOT ruled (do not build without the user)
- **§18.6 the colour architecture** (rows 179–184): one linear-light surface always (the output-mode picker retires), minColor
  the only engine with its Display half removed and its Rendering half kept, OCIO reduced to an out-of-process `ociobakelut`
  LUT source against ONE patched "Linear sRGB EDR" display (the user picks config · input · view · look), bypass = as stored
  (row 180, LANDED as the interim). The gate is row 130 (the Windows HDR-monitor session). The user is wary of handing colour
  decisions to users: "against the whole purpose of this app" — every default stays QCView's, every decision lives in the chain.
  Open inside it: PQ / HLG in bypass decode to nits today; QCView's SDR-surface bypass showed the code values.

### 26.3 The queue, in the order the user set
1. **The Add Media section's owed QCView pieces** (before the user walks it): the row context menu (Create Playlist from
   Selection · Input · Delete), rename, New Playlist, drag-to-playlist, the "Can't be played here" badge / toasts, the
   unsupported-codec gate at load (`docs/phase2/step-project-crate.md`, the shell wiring note).
2. **The user's walk on both OSes**: the project panel, the Transfer chips, the grab, the loaded-row treatment, the dimmed frame
   numbers, the 6.7K NAS clips through the bridge on Windows.
3. **Step 2c image sequences** (`min-image`, EXR layer discovery), then **2d the MXF scanner** (§21.4's slice, row 138).
- Smaller owed items: ~~scrub skim audio + the clock once on release~~ (BUILT 2026-11-08, 0b6c88b); ~~Copy path / Reveal + the
  hero thumbnail~~ (BUILT 2026-11-08, rows 193 + the mid-clip hero); ~~MXF timecode~~ (2026-11-08: the session read only the stream's
  tag — QCView's stream → container → any-stream lookup carried, 6280b51); `.lnk` resolution; menus / Recents UI; row 178 (Windows software HEVC10 4K scrub); the live WASAPI
  listen; a LUID match for the Vulkan device on multi-GPU machines; the audio FFmpeg session still lives in the player
  (`audio/ffmpeg_audio.rs`) — it joins `min-ffmpeg` when audio is shared.

### 26.4 Recipes that cost a session to learn
- **Windows iteration**: edit on the Mac → `cargo build --release -p qcv` (cfg(windows) code is NOT compiled here; the
  mingw / gnu cross-check attempts failed in bindgen — do not retry) → commit → `git bundle create <f> <box-head>..main` → scp
  to `C:\qcb-lab\` → on the box `git checkout -- Cargo.lock; git fetch <bundle> main:refs/bundles/main; git merge --ff-only
  refs/bundles/main` (an UNTRACKED file that the commit adds aborts the merge silently: remove it first) →
  `powershell -ExecutionPolicy Bypass -File C:\qcb-lab\build-2d.ps1` (FFMPEG_DIR = the repo's `external\ffmpeg\win64`; aborts
  while qcv.exe runs — the user's instance is never killed) → a scp'd `.ps1` for every probe (inline PowerShell through ssh
  mangles quotes; keep scripts ASCII — an em-dash broke one silently) → launch on the desktop with
  `run.ps1 -Name manual-qcv -Command C:\qcb-lab\mp-manual-qcv-info.cmd` (info log + scrub trace), read
  `C:\qcb-lab\minplatform-logs\manual-qcv.log` by scp.
- **Harness flags**: `--headless N [--play] [--sw] [--snap-out p@N] [--snap-display p@N]`, `--headless 1 --scrub a-b:ms`
  (`--scrub` / `--seeks` need `--headless` or a WINDOW opens on the user's desktop), `--ui-bench N --ui-snap p`;
  `QCV_TRACE_SCRUB=1`, `QCV_TRACE_INPUT=1`, `QCV_NO_AUDIO=1`, `QCV_SETTINGS_DIR`. The scrub bench FAILs on any stream frame
  shown mid-sweep. zsh does not word-split unquoted variables; macOS has no `timeout`.
- **Fixtures**: client-derived, gitignored, never leave their machine; synthetic clips for layout checks come from
  `ffmpeg -f lavfi -i testsrc2 … -c:v prores_ks` ON the box.
