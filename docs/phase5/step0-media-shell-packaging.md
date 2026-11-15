# Phase 5, step 0 — UFB audit: media, the app shell, packaging

Audited: the `ufb` repository, branch `main` @ `9e197f0`, version 1.4.2 (Qt 6.11 / QML + Rust, macOS + Windows).
Compared with: this repository (`minplatform`) @ `2eec356`.
Read only; nothing was built or run. Every UFB claim cites `path:line` in the ufb repository; minplatform claims cite
this repository's files (prefixed `mp:`). "Not found" / "not verified" mean exactly that.

Other readers cover the browser QML, file operations, projects / metadata, mesh sync, and the mount agent. Where this
area touches theirs (agent start / stop, the FinderSync extension, transcode), only the seam is described.

Sizes, for scale: `app/` C++ / Objective-C++ is about 24 400 lines without the vendored `stb_image.h`; of that the
player (`app/player/**`) is about 15 400. The QML is about 20 700 lines, the Rust about 52 000 (counted with `wc -l`).

---

## 1. Thumbnails

### 1.1 Pipeline

One request = one grid cell. QML asks `image://ufb-thumbs/<absolute path>` (`app/qml/Ufb/App/FileBrowser.qml:3324`);
the provider runs on a dedicated pool (`app/UfbImageProviders.cpp:297-352`):

1. `stat` the file (size + mtime) (`app/UfbImageProviders.cpp:308-310`).
2. Disk cache lookup; a hit is decoded and scaled down to the request (`:315-320`).
3. Miss: **stage 0, the OS shell** — unless the format is one Qt decodes itself (`:331-335`).
4. Shell gave nothing: UFB's own back end by extension (`:337-340`).
5. Store the 512 px master in the cache, scale to the request (`:342-348`).

A null result is reported as a load ERROR, so QML's fallback (the OS file-type icon under the thumbnail) is the single
"no picture" path (`app/UfbImageProviders.cpp:200-213`, `app/qml/Ufb/App/FileBrowser.qml:3253-3256`).

### 1.2 File types and back ends

| Types | Back end | Notes | Evidence |
|---|---|---|---|
| jpg jpeg jpe jfif png gif bmp ico icns svg svgz webp tiff tif tga wbmp | Qt `QImageReader` (in process, no shell stage) | EXIF orientation applied; decode pre-scaled (`setScaledSize`: JPEG skips DCT levels; most other plugins decode whole first) | `app/thumbnails/Thumbnailer.cpp:34-43`, `:208-262` |
| psd psb | psd_sdk (static) | merged composite only; 8 / 16-bit RGB(A), 8-bit CMYK by arithmetic (no ICC); 32-bit, Lab, duotone, indexed, multichannel, **grayscale** skipped; no composite (saved without "Maximize Compatibility") = no thumbnail | `app/thumbnails/PsdBackend.cpp:126-156`, `:79-89` |
| exr | OpenEXR (C++ API) | default view = `RgbaInputFile` on part 0; files with only prefixed channels fall to the first non-cryptomatte layer; a named layer reads `<layer>.R/G/B/A` | `app/thumbnails/ExrBackend.cpp:256-298`, `:113-245` |
| hdr rgbe | `stb_image` (HDR only compiled in) | header read first for the size cap, then full float decode | `app/thumbnails/HdrBackend.cpp:11-14`, `:43-60` |
| heic heif hif (+ any file whose `ftyp` brand is HEIF, whatever its extension) | libheif + libde265 | sniffed BEFORE the Qt branch (renamed phone photos); embedded thumbnail used when large enough; orientation + tile grid handled by libheif; stub when built without libheif | `app/thumbnails/Thumbnailer.cpp:111-133`, `:198-206`; `app/thumbnails/HeifBackend.cpp:12-18`, `:160-184` |
| pdf ai | PDFium | first page on white; one global mutex (PDFium is not thread-safe) | `app/thumbnails/Thumbnailer.cpp:61-65`; `app/thumbnails/PdfBackend.cpp:20-29`, `:185-189` |
| mp4 m4v mov qt mkv webm ogv avi wmv mpg mpeg ts m2ts mts mxf flv 3gp 3g2, and dpx cin as one-frame "video" | FFmpeg (libav*, software, 1 thread, `FLAG2_FAST`) | seek to 0.5 s (keyframe before), first decodable frame, swscale bilinear to RGBA | `app/thumbnails/Thumbnailer.cpp:74-88`; `app/thumbnails/VideoBackend.cpp:104-126`, `:43-60` |
| cr2 cr3 nef arw dng raf orf rw2 pef srw, doc docx xls xlsx ppt pptx, avif | **OS shell only** (no UFB decoder) | these extensions only widen the grid's gate; no thumbnail if the OS has no handler | `app/thumbnails/Thumbnailer.cpp:90-105`, `:156-162` |

The grid decides whether to create a thumbnail element at all from a pure extension check, no I/O
(`Thumbnailer::mayHaveThumbnail`, `app/thumbnails/Thumbnailer.cpp:156-162`; gate at
`app/qml/Ufb/App/FileBrowser.qml:3317-3319`). Thumbnails exist only in GRID view: list / tree show icons, and the gate
is explicit because a hidden GridView still instantiates delegates (`app/qml/Ufb/App/FileBrowser.qml:3306-3316`).

### 1.3 OS shell thumbnails (stage 0)

| OS | API | Behaviour | Evidence |
|---|---|---|---|
| macOS | `QLThumbnailGenerator`, representation type Thumbnail only (never an icon) | blocks the pool worker on a semaphore, 10 s ceiling, then falls through | `app/thumbnails/ShellThumbnail_mac.mm:74-104` |
| Windows | `IShellItemImageFactory::GetImage` with `SIIGBF_THUMBNAILONLY \| SIIGBF_BIGGERSIZEOK` | runs on a THROWAWAY thread with its own OLE apartment; 10 s wait; on timeout the thread is detached and abandoned | `app/thumbnails/ShellThumbnail_win.cpp:58-89`, `:93-118` |

Consequence: for PSD, PDF, video, HEIC on a machine whose shell has a handler, the picture the user sees is the OS's,
not UFB's back end's; UFB's decoders are the fallback (and the only source for EXR / HDR on macOS, per
`app/thumbnails/ShellThumbnail_mac.mm:6-8`).

### 1.4 System icons

`image://ufb-icons/<extension | "folder" | "file">` — keyed by EXTENSION, never by path
(`app/UfbImageProviders.cpp:456-463`); so per-file icons (an application's own, a folder with a custom icon) are not
shown — not found anywhere in this area.

- Windows: `SHGetFileInfoW` with a fake path + `SHGetImageList`, always the 256 px jumbo list (the small lists gave
  blank folder icons); alpha rebuilt from the AND-mask for legacy icons; the first call per thread is retried
  (`app/UfbImageProviders.cpp:505-515`; `app/thumbnails/SystemIcons_win.cpp:65-74`, `:170-175`, `:198-202`).
- macOS: `NSWorkspace iconForContentType:` (UTType), drawn into a bitmap (`app/thumbnails/SystemIcons_mac.mm:1-16`).
- In-memory cache per (extension, size), unbounded by design; icons run on the GLOBAL pool so thumbnail backlog cannot
  starve them (`app/UfbImageProviders.cpp:470-494`, `:534`).

### 1.5 The cache

| Aspect | Value | Evidence |
|---|---|---|
| Store | one SQLite file `thumbnails.db` in the OS cache location for the app (temp dir as fallback); separate from the metadata database so it can be wiped alone | `app/thumbnails/ThumbCache.cpp:19-33`; `app/thumbnails/ThumbCache.h:10-15` |
| Schema | `thumbs(path PK, file_size, file_mtime, width, height, blob, data_size, created, last_access)` + index on `last_access` | `app/thumbnails/ThumbCache.cpp:57-71` |
| Key | the path string as given (no normalisation found) | `app/thumbnails/ThumbCache.cpp:128-133` |
| Invalidation | row is used only if size AND mtime (ms) match; no content hash | `app/thumbnails/ThumbCache.cpp:128-133`; `app/thumbnails/ThumbCache.h:40-43` |
| Master size | 512 px longest side, one master per file serves every grid size and pixel ratio | `app/thumbnails/ThumbCache.h:30-34` |
| Encoding | PNG when the image has alpha, else JPEG quality 85 | `app/thumbnails/ThumbCache.cpp:157-166` |
| Budget | 512 MB of blobs; after each insert, least-recently-used rows past the cap are deleted | `app/thumbnails/ThumbCache.h:56-58`; `app/thumbnails/ThumbCache.cpp:185-204` |
| Concurrency | one connection per worker thread, WAL, `synchronous=NORMAL`, 4 s busy timeout | `app/thumbnails/ThumbCache.cpp:74-113` |
| Removal of dead entries | only by LRU; nothing removes rows for deleted / renamed files (not found) | `app/thumbnails/ThumbCache.cpp:185-204` |

Under the Store package the cache folder is file-virtualised; the app exposes `LoginItem.packaged` so QML's "Reveal"
of the cache can account for it (`app/main.cpp:543-544`).

### 1.6 Scheduling, priority, cancellation

- A dedicated pool of **4** threads, idle threads expire after 5 s (`app/UfbImageProviders.cpp:277-287`).
- FIFO. No priority by visibility or scroll position was found; the only ordering is Qt's delegate creation order.
- Cancellation: when a delegate is destroyed QML calls `cancel()`; a QUEUED job then returns at once; a job already
  decoding runs to completion (`app/UfbImageProviders.cpp:181-225`, `:236-245`). So after leaving a folder of heavy
  files the catch-up is at most 4 in-flight decodes.
- Requested size: grid cell × device pixel ratio (`app/qml/Ufb/App/FileBrowser.qml:3325-3331`); grid size is a user
  slider, default 128 (`app/qml/Ufb/App/FileBrowser.qml:54`, `:2575-2577`).

### 1.7 Size limits

| Limit | Value | Evidence |
|---|---|---|
| Thumbnail decode ceiling | 256 MP source area for every back end that takes a cap | `app/thumbnails/Thumbnailer.cpp:169-178` |
| Lightbox ceiling | a decoded-buffer budget = physical RAM / 8, clamped to 2–8 GB, divided by the back end's worst bytes per pixel (8 for Qt / PSD / HEIF, 16 for EXR / HDR) | `app/thumbnails/Thumbnailer.cpp:179-196`, `:202-279` |
| Qt allocation limit | raised to the active budget (Qt's default 256 MB silently nulled large TIFFs) | `app/thumbnails/Thumbnailer.cpp:232-238` |
| Unknown dimensions | file over 1 GB (thumbnail) / over the budget (preview) is skipped | `app/thumbnails/Thumbnailer.cpp:243-252` |
| EXR layer view | fixed 1.5 GB / 16 bpp ≈ 96 MP (not RAM-scaled) | `app/UfbImageProviders.cpp:446-451` |
| PDF, video | no pixel cap applied (`decodePdf` / `decodeVideo` take none) | `app/thumbnails/Thumbnailer.cpp:281-287` |

### 1.8 Colour and alpha

- EXR and HDR: clamp to 0..1, then a 2.2 gamma, to 8 bits. No exposure, no view transform, no OCIO, by decision
  (`app/thumbnails/ExrBackend.cpp:33-42`; `app/thumbnails/HdrBackend.cpp:24-31`).
- EXR alpha goes through the SAME gamma function as colour and lands in a straight-alpha `RGBA8888` image
  (`app/thumbnails/ExrBackend.cpp:96-99`, `:232-235`) — see rough spots.
- HDR is always opaque (`app/thumbnails/HdrBackend.cpp:77-87`).
- Video frames: swscale's default conversion to RGBA, no transfer / primaries handling (`app/thumbnails/VideoBackend.cpp:43-60`);
  HDR (PQ / HLG) sources are therefore not tone-mapped. Rotation metadata: no handling found in `VideoBackend.cpp`.
- PDF pages are rendered on white (`app/thumbnails/PdfBackend.cpp:185-187`).
- PSD 16-bit is truncated to 8 (`>> 8`) (`app/thumbnails/PsdBackend.cpp:63-67`).

### 1.9 Cost on network shares

- Every request stats the file on a worker (`app/UfbImageProviders.cpp:308-310`); a cache hit costs that stat plus one
  SQLite read and one write (the LRU touch) (`app/thumbnails/ThumbCache.cpp:140-146`).
- A miss reads the file through whichever decoder answers. EXR, HDR, PSD, HEIF and most Qt formats read the WHOLE
  picture before reducing (`app/thumbnails/ExrBackend.cpp:86-88`; `app/thumbnails/HdrBackend.cpp:60`;
  `app/thumbnails/HeifBackend.cpp:118-130` reads the whole file into memory; `app/thumbnails/Thumbnailer.cpp:217-223`).
- The shell stage may stall on a share; both OS paths are bounded at 10 s per file, which with 4 workers bounds a
  stalled folder at 4 blocked workers per 10 s (`app/thumbnails/ShellThumbnail_win.cpp:96-103`).
- The lightbox's video open probes the file synchronously on the GUI thread (`app/qml/Ufb/App/VideoPreview.qml:41-46`)
  and autoplay is delayed 400 ms for slow opens (`app/qml/Ufb/App/VideoPreview.qml:25-30`).
- The player's read-ahead warmer is tunable by environment (`app/player/read_ahead.h:9`, `:32-33`, `:87`).

---

## 2. Previews (the lightbox, Space)

An in-window overlay, one file at a time, routed by extension in this order: video, audio, PDF, HTML, notes document,
Word document, text, else still (`app/qml/Ufb/App/PreviewLightbox.qml:186-193`). Space opens and closes; Esc closes
(or steps back from an EXR layer); Left / Right move to the neighbour file (`app/qml/Ufb/App/PreviewLightbox.qml:85-115`).
The content tree is created asynchronously with a delayed spinner (`:173-212`).

| Type | What the user can do | Depends on | Evidence |
|---|---|---|---|
| Stills (everything in 1.2 that is not video / PDF) | fit to window only — **no zoom, no pan** (a v1 decision); decoded at view size × pixel ratio; failure shows the file icon | the thumbnail back ends at full resolution; shell thumbnail as fallback for RAW / Office | `app/qml/Ufb/App/ImagePreview.qml:70-109`; `devdocs/lightbox-preview-plan.md:40`; `app/UfbImageProviders.cpp:362-394` |
| EXR with more than one layer | a contact sheet of every layer (cryptomatte hidden); click a layer for full frame; Esc / Backspace / "Layers" button back. No per-channel (R / G / B / A) view, no exposure control | OpenEXR | `app/qml/Ufb/App/ImagePreview.qml:30-58`, `:111-191`; `app/thumbnails/ExrBackend.cpp:300-345` |
| PDF, AI | continuous vertical scroll of all pages, rendered lazily at view width, off-screen pages released; Up / Down / PageUp / PageDown / Home / End. No zoom, no text selection, no search, no links. All pages sized from page 0's aspect | PDFium | `app/qml/Ufb/App/PdfReader.qml:21-63`; `app/thumbnails/PdfBackend.cpp:43-77` |
| Video (+ animated GIF / WebP) | play / pause, frame step, accelerating shuttle (2×→32×), seek bar scrub, Home / End, loop (persisted), mute, volume; audio in sync. Keys: K / W / S, Q / E, A·J / D·L, M, V, Up / Down, Home / End | the player (section 3), FFmpeg | `app/qml/Ufb/App/PreviewLightbox.qml:116-134`; `app/qml/Ufb/App/VideoPreview.qml:105-208`; `app/MediaInfo.h:21-25` |
| Audio (wav wave bwf mp3 aiff aif aifc flac m4a aac ogg oga opus wma caf) | same transport minus frame steps (Q / E = one second); a time readout over the file icon; **no waveform** | the player's audio half | `app/qml/Ufb/App/PreviewLightbox.qml:28-30`; `app/qml/Ufb/App/AudioPreview.qml:1-8`, `:69-71`, `:148` |
| Text, code, logs, Markdown | read-only, selectable, wrapped; Markdown rendered by Qt's rich text; first 1 MB only, with a truncation line; **no syntax highlighting** | Qt text only | `app/qml/Ufb/App/TextPreview.qml:41-53`, `:64-89`; `app/TextInfo.cpp:65-75` |
| HTML | with the web engine: a real page (JavaScript ON, links leave to the system browser, navigation frozen); without it: Qt's rich-text approximation, 32 MB cap | QtWebEngine (optional at build time) | `app/qml/Ufb/App/HtmlPreview.qml:7-14`, `:154-169`; `app/qml/Ufb/App/PreviewLightbox.qml:40-44`; `app/qml/Ufb/App/TextPreview.qml:41-46` |
| Notes documents (`.mnd`, pre-1.0 `.mndb`, `.mnpkg`) | read-only rendered page | a C++ re-implementation of the notes app's block model → temp HTML → QtWebEngine; SQLite; miniz for packages. Without the web engine: file icon | `app/MndbDoc.h:1-41`; `app/qml/Ufb/App/PreviewLightbox.qml:45-54` |
| Word (`.docx .docm .dotx`) | read-only approximation (paragraphs, headings, lists, tables, links, raster images; no headers / footers, footnotes, comments, floating objects, math); links limited to http / https / mailto | own OOXML → HTML renderer + miniz + QtWebEngine; without it only the shell thumbnail | `app/DocxDoc.h:1-26`; `app/qml/Ufb/App/PreviewLightbox.qml:55-62`, `:237-247` |
| Other Office, RAW, AVIF | the OS thumbnail at whatever size the shell gives, else the icon | OS shell | `app/UfbImageProviders.cpp:381-387` |

Which files count as text: a list of about 90 extensions, else a 4 KB sniff (no NUL, under 5 % control bytes) for files
no media back end claims (`app/TextInfo.cpp:12-33`, `:40-63`).

QtWebEngine facts the rebuild inherits or sheds: it must be initialised before the application object and runs with
GPU compositing disabled as a workaround (`app/main.cpp:238-260`); its helper process needs JIT entitlements on macOS
(`app/QtWebEngineProcess.entitlements:11-14`); Windows needs a `qt.conf` so it finds its resources (`installer/qt.conf`).
Document content is treated as untrusted input by the Word renderer (`app/DocxDoc.h:21-26`) while HTML previews keep
JavaScript on by a stated decision (`app/qml/Ufb/App/HtmlPreview.qml:8-11`).

---

## 3. The player

### 3.1 Provenance

`app/player/` is an independent COPY of the review player's decode and audio stacks ("borrow a lot, share nothing"),
namespace renamed, last re-synced 2026-10-23 (`devdocs/lightbox-preview-plan.md:22-25`, `:43`, `:355-359`;
`app/player/vulkan_hw_device_ctx.h:14-15`; `app/player/wasapi_audio_device.h:23-24`).

### 3.2 Differences from the original, as the code and plan state them

| Original has | UFB's copy | Evidence |
|---|---|---|
| A native player window, HDR output, OCIO colour management, compositor passes | none: SDR only, one render pass into the QML scene (`QQuickRhiItem`) | `app/player/VideoSurfaceItem.h:1-9`; `devdocs/lightbox-preview-plan.md:23-25` |
| Annotations, timeline, dual (A/B) view, projects | left behind | `devdocs/lightbox-preview-plan.md:93`, `:103` |
| 16-bit CPU path | 8-bit RGBA only | `app/player/rgb_range.h:6-7`; `devdocs/lightbox-preview-plan.md:514` |
| Shuttle audio (varispeed) and a dual mixer | none; the resampler serves only the sync servo | `app/player/fractional_resampler.h:8-9`; `app/player/audio_drain_stage.h:4-5` |
| Driven from C++ | driven from QML (transport, clocks, key routing live in QML) | `app/player/audio_player.h:59`, `:162-163`; `app/qml/Ufb/App/VideoPreview.qml:105-113` |
| GPU YUV conversion for the CPU path | not ported | `devdocs/lightbox-preview-plan.md:403` |
| An HLSL spinner | a QML busy indicator | `devdocs/lightbox-preview-plan.md:164` |

Kept: the streaming decoder with a latest-wins publish slot, the GOP-cached scrub decoder, the two-tier frame index,
read-ahead, the decoder cleanup queue, drop-frame timecode, legal→full RGB range expansion, the audio servo and drain
stage (`devdocs/lightbox-preview-plan.md:93-99`, `:361-373`; `app/player/timecode_formatter.h:5-17`).

### 3.3 Hardware decode

| OS | Path | Display | Evidence |
|---|---|---|---|
| macOS | VideoToolbox for everything it serves | zero-copy: `CVPixelBuffer` → Metal plane textures → YUV→RGB shader (biplanar and AYUV, so ProRes 4444 alpha shows) | `app/player/video_decoder.cpp:182-183`; `app/player/VideoSurfaceItem.h:5-8`; `app/player/shaders/` |
| Windows, ProRes | FFmpeg's Vulkan (compute) decoder on a decode-only Vulkan device created at startup | Vulkan → D3D11 bridge (shared NT handles) | `app/main.cpp:211-222`; `app/player/vulkan_hw_device_ctx.cpp:287-300`; `app/player/d3d11/d3d11_vulkan_decode_bridge.cpp` |
| Windows, everything else | D3D11VA on the scene graph's own D3D11 device (handed over when the window's scene graph starts) | zero-copy texture array, readback as fallback | `app/player/VideoSurfaceItem.h:34-43`; `app/player/VideoSurfaceItem.cpp:396-451`, `:510-540`; `app/main.cpp:594-602` |
| Any OS, ProRes RAW | forced software (both registered hardware decoders are unusable) | CPU RGBA upload | `app/player/hw_routing.h:8-18`, `:30-33` |
| Scrubbing | hardware only for intra-only codecs; software for long-GOP | — | `app/player/scrub_decoder.cpp:222-228` |
| FFV1, APV | software (the Vulkan decoders were measured 6–20× slower) | — | `app/player/vulkan_hw_device_ctx.cpp:289-298` |

The scene graph back end is pinned (Metal / Direct3D 11) so the interop is deterministic (`app/main.cpp:199-209`).
Three local FFmpeg patches are carried (DNxHD colour transform, MXF RGBA range, ProRes RAW Bayer patterns)
(`scripts/ffmpeg-patches/`).

### 3.4 Audio

CoreAudio (macOS) and WASAPI (Windows) devices (`app/player/coreaudio_device.h`, `app/player/wasapi_audio_device.h`);
a ring buffer, a drain stage that owns resampling and fades, a continuous sync servo with a soft re-seek beyond its
window (`app/player/audio_player.h:79`, `:129`, `:181`); multi-stream sources and channel routing modes
(`app/player/multi_stream_audio_decoder.h:165`; `app/player/audio_routing.h`); 5.1 → stereo by ITU-R BS.775 with LFE
dropped (`app/player/audio_decoder.cpp:68-71`). Video is the master clock; every play arms an audio hold that the
first landed frame releases (`app/qml/Ufb/App/VideoPreview.qml:1-7`, `:88-93`). Audio-only files run the same player
against a QML wall clock (`app/qml/Ufb/App/AudioPreview.qml:1-6`).

### 3.5 What the browser uses it for

Only the lightbox: `VideoPreview`, `AudioPreview`, and animated GIF / WebP (`app/qml/Ufb/App/PreviewLightbox.qml:214-222`,
`:31-37`). Grid thumbnails do NOT use it — `VideoBackend.cpp` opens its own software session
(`app/thumbnails/VideoBackend.cpp:16-18`, `:104-112`). Transcoding runs the bundled `ffmpeg` / `ffprobe` programs as
child processes, not this player (`scripts/build-external-ffmpeg-mac.sh:104-111`; `bindings/src/services/transcode.rs:109`).

---

## 4. Metadata inspectors

There are none in the usual sense in this area. The three "Info" singletons are routing helpers for the lightbox:

| Singleton | What it answers | Shown to the user | Evidence |
|---|---|---|---|
| `ExrInfo.layers(path)` | layer / part names (header only) | the names under the layer contact sheet | `app/ExrInfo.h:22-25`; `app/qml/Ufb/App/ImagePreview.qml:168-172` |
| `MediaInfo.isAnimated(path)` | is a GIF / WebP multi-frame | nothing; picks the player over the still view | `app/MediaInfo.h:21-25`; `app/MediaInfo.cpp:5-13` |
| `TextInfo.isText` / `readHead` | is it text; its first bytes | the text itself + a truncation line | `app/TextInfo.h:32-41` |
| `PdfDoc.pageCount` / `pageAspect` | page count, aspect | nothing directly (sizes the page list) | `app/PdfDoc.h:21-30` |

No EXR header view (compression, windows, attributes), no codec / resolution / frame-rate / timecode panel, and no
timecode readout in `VideoPreview.qml` were found. ExifTool is bundled for the transcode step (copying metadata onto
the output), not for display (`LICENSES/THIRD_PARTY_NOTICES.txt:183-193`). Whether another panel shows file metadata
is the browser readers' area — not verified here.

---

## 5. Reuse: UFB component → minplatform equivalent → gap

| UFB component | minplatform equivalent | Gap |
|---|---|---|
| Qt image plugins: JPEG, PNG, TIFF, TGA, BMP | `mp:crates/min-image/src/lib.rs` (`Format`: Exr, Png, Tiff, Jpeg, Dpx, Tga, Bmp; `load_thumbnail` with JPEG DCT scaling and row skipping) | JPEG CMYK / YCCK is refused there (`mp:crates/min-image/src/jpeg.rs:3`); EXIF orientation is applied by the notes app, not the crate (`mp:apps/mn/src/media.rs:9-10`) |
| GIF, WebP (stills) | notes app only: `image` crate, first frame (`mp:apps/mn/Cargo.toml:39-40`, `mp:apps/mn/src/media.rs:684`) | not in `min-image`; animated playback would go through the player |
| SVG | notes app only: `resvg` (`mp:apps/mn/Cargo.toml:41-42`, `mp:apps/mn/src/media.rs:601`) | not in a shared crate |
| ICO, ICNS, WBMP | not found | small; decide whether wanted |
| EXR default + named layer, layer list | `mp:crates/min-image/src/exr.rs` (`layers`, `load`, `dimensions`, `compression`; same layer rules incl. the "no bare RGB" fallback, display window honoured) | none for decoding; a tone map for display of thumbnails must be chosen (the notes app applies the sRGB curve, `mp:apps/mn/src/media.rs:10`); cryptomatte filtering of the layer list — not verified in `exr.rs` |
| Radiance HDR (`.hdr`, `.rgbe`) | **not found** | missing decoder |
| DPX | `mp:crates/min-image/src/dpx.rs` | UFB reads DPX through FFmpeg; here it is native. `.cin` (Cineon): not found |
| PSD / PSB composite | notes app: `mp:apps/mn/src/psd.rs` (PSD + PSB; 8 / 16 / 32-bit; RGB, **Grayscale**, CMYK; streamed row by row and reduced while read; reports "no composite") | wider than UFB's (grayscale, 32-bit, streaming) but lives in the app, not in `min-image` — needs lifting to a shared place |
| HEIC / HEIF | notes app, **macOS only**, through ImageIO (`mp:apps/mn/src/imageio.rs:1-3`); other systems answer "not read on this system yet" (`mp:apps/mn/src/media.rs:596`, `:618-620`) | **Windows has no HEIF decoder**; UFB bundles libheif + libde265 on both. Content sniffing of renamed files: not found |
| Camera RAW | notes app, macOS only, ImageIO (`mp:apps/mn/src/media.rs:584-585`) | Windows: none (UFB also has none of its own — shell only) |
| AVIF, Office thumbnails | not found | UFB relies on the OS shell; see next row |
| OS shell thumbnails (QuickLook / `IShellItemImageFactory`) | **not found** | missing; needed for RAW / Office / AVIF parity and as the stage-0 policy |
| OS file-type icons | not found in the shared crates (not verified inside `apps/qcv`) | missing: NSWorkspace / SHGetImageList bridge |
| PDF first page + page reader | notes app: `mp:apps/mn/src/pdf.rs` on `hayro`, pure Rust (page count and sizes, a page at a width, page TEXT with positions; fenced calls; recent files' bytes kept) | engine fidelity vs PDFium on production PDFs / `.ai` not verified; the notes app already draws `.ai` this way (`mp:apps/mn/src/media.rs:594-595`). Text selection is a gain over UFB |
| Video thumbnail (frame at 0.5 s) | `mp:crates/qcv-player/src/thumbnail_cache.rs` (software session, nearest frame, legal-range rule) and the notes app's poster `mp:apps/mn/src/video.rs:25` with colour through the presenter (HDR → SDR) | better colour than UFB; needs a "representative frame" rule and a batch mode for a folder |
| Persistent thumbnail cache (SQLite, 512 MB LRU) | **not found** — `thumbnail_cache.rs` is an in-memory LRU of 100 GPU textures keyed by (path, frame); the notes app keeps previews with its collection | missing: a disk cache, its key / invalidation, eviction |
| Thumbnail scheduling (4 workers, cancel when queued) | the notes app's off-thread decode under a memory budget (`mp:apps/mn/src/media.rs:1-3`, `:20-21`); the timeline cache's front-of-queue priority and generation bump (`mp:crates/qcv-player/src/thumbnail_cache.rs:1-12`) | pieces exist; a grid scheduler with visibility priority does not |
| The player (decode, scrub, audio, transport, keys) | `mp:crates/qcv-player/src/lib.rs` (the ORIGINAL the copy was taken from, carried whole: `video_decoder`, `scrub_decoder`, `transport`, `key_map`, `audio/*` incl. `shuttle`, `waveform`) on `mp:crates/min-ffmpeg/src/lib.rs` and `mp:crates/min-present` | none functionally — the rebuild gets MORE (HDR-aware presenter, waveform, shuttle audio). Windows decode is D3D12VA + Vulkan here (`mp:crates/min-ffmpeg/src/hw.rs:1-3`) instead of D3D11VA + Vulkan |
| Audio-only preview | `mp:crates/qcv-player/src/audio/` (`player.rs`, `waveform.rs`) | a small audio pane to assemble; a waveform becomes possible |
| Text / code / Markdown preview | text widgets in `mp:crates/min-ui/src/text_area.rs`; Markdown import in the notes app (not verified which module) | a read-only large-file text view with a byte cap; highlighting is not in UFB either |
| HTML preview (QtWebEngine) | **not found** — no web engine in the workspace manifests | the one hard gap: decide between no rendered HTML, the OS web view, or "open in browser" |
| Notes-document preview (C++ re-implementation, 2 029 lines) | the real thing: `mp:crates/mn-doc`, `mn-store`, `mn-layout` (the notes app's own model, store and layout) | none in principle — render with the notes app's crates instead of a second renderer; pre-1.0 `.mndb` reading: the notes app has `mp:apps/mn/src/import_old.rs` (not verified what it covers) |
| Word preview (own OOXML → HTML, 1 282 lines) | `mp:apps/mn/src/import_office.rs` reads `.docx` (and `.xlsx`, `.odt`, `.ods`) into the notes block model | a path exists (docx → blocks → notes layout) without a web engine; fidelity vs UFB's renderer not verified |
| Single instance + forwarded launch argument | `mp:apps/mr/src/instance.rs` (local socket / named pipe, stale socket taken over) | none |
| Tray / menu-bar icon, close-to-tray, start hidden | `mp:apps/mr/src/shell.rs` (`tray-icon`, window hides, one way out) | none; the dynamic "Mounts" submenu is app work |
| URL schemes + documents from the OS | `mp:apps/mn/src/mac_open.rs` (`application:openURLs:`); Windows protocol in `mp:packaging/windows/msix-minnotes/AppxManifest.xml.in:63` | none |
| Sparkle | `mp:apps/mn/src/sparkle.rs`, `mp:apps/mr/src/sparkle.rs` (framework loaded at run time) | UFB's feed delivers a **pkg**, the siblings a DMG — see section 7 |
| Chromeless title row on macOS | the notes app's window (the model UFB copied, `app/WindowChrome.h:1`) | none expected; not verified line by line |
| macOS accent colour following the system | not found (not verified in `mp:crates/min-theme`) | small bridge |
| Native "Finder menu" (Open With, Share, Tags, Get Info) | not found | AppKit work to carry (`app/MacShellMenu.h:9-17`) |
| Start at login (Store startup task / Run key) | not found in the shared crates; minRender's manifest carries a startup task (`mp:packaging/windows/make-msix-minrender.ps1:10`) | the WinRT toggle and "blocked by system" state to carry |
| Packaging: notarized artifact, MSIX, notices | `mp:packaging/macos/*.sh`, `mp:packaging/windows/make-msix*.ps1`, `mp:packaging/notices/make_notices.py` | a **pkg** flow, an app-extension + second bundle, and an App Group do not exist there yet |

---

## 6. App shell

### 6.1 Startup sequence (`app/main.cpp`)

1. Windows: `--prime-smartscreen` sleeps 3 s and exits (run by the old installer so the reputation check is done
   before first use) (`:150-166`).
2. Windows, unpackaged only: set the explicit AppUserModelID before any window (`:168-179`); attach the parent console
   for logs (`:181-190`).
3. Optional log file from `UFB_LOG_FILE`; a message handler that prints every Qt message to stderr (`:118-146`, `:193-197`).
4. Pin the graphics API (Metal / D3D11) (`:199-209`); Windows: create the decode-only Vulkan device, soft failure (`:211-222`);
   macOS: force an sRGB surface so colours do not oversaturate on P3 panels (`:224-236`).
5. Web engine: add `--disable-gpu-compositing`, initialise (`:238-260`).
6. Application object (a subclass that catches file-open events for deep links on macOS) (`:262-266`; `app/UfbApplication.h:1-12`).
7. Parse arguments: `--background`, one positional path or URI; Windows: a startup-task launch or a `union:` link means
   start hidden (`:274-298`).
8. macOS: log whether a LaunchAgent file exists (nothing is installed any more); hide the Dock icon when starting hidden
   (`:300-317`; `app/MacOSBootstrap.mm:32-57`, `:65-81`).
9. Window icon from an embedded PNG (not on macOS) (`:319-353`).
10. Single-instance gate (6.2).
11. Fonts (bundled Geist), the Controls style (FluentWinUI3 on BOTH systems), Windows forced dark (`:390-433`).
12. Quit hook into Rust: cancels in-flight mount requests, never stops the agent (`:106-113`, `:439-446`).
13. Engine: image providers, context objects (`AppCtl`, `WindowChrome`, `Updater`, `LoginItem`, `MacAccent`, `ShellMenu`),
    `setQuitOnLastWindowClosed(false)` (`:465-563`).
14. Local server for forwarded launches; load `Main` (`:565-592`).
15. Windows: hand the D3D11 device to the decoder side; set icons by `WM_SETICON`; cloak the window until the first frame
    (white-flash fix), then show — or stay hidden for `--background` (`:594-602`, `:638-720`).

### 6.2 Single instance

A lock file in the app's data folder (stale locks reclaimed at once) decides primary vs secondary; a secondary sends
its one positional argument over a local socket and exits 0; the primary raises its window and navigates
(`app/main.cpp:355-388`, `:565-590`). The socket is a named pipe on Windows and a Unix socket inside the App Group
container on macOS (`app/main.cpp:80-103`).

### 6.3 URL schemes, file associations

- Schemes `ufb:` and `union:` on both systems (`app/Info.plist.in:40-51`;
  `packaging/windows/msix/AppxManifest.xml.in:68-80`). `ufb:` opens the target in the app; on Windows `union:` reveals
  it in Explorer and a cold launch for it starts hidden (`app/main.cpp:293-297`).
- macOS delivery is by Apple event → file-open event, with a pending slot for a cold start (`app/UfbApplication.h:8-12`;
  `app/main.cpp:612-629`).
- A plain path argument is accepted too (`app/main.cpp:488-500`).
- File type associations (document types): none found in `Info.plist.in` or the MSIX manifest.

### 6.4 Window chrome

- macOS: chromeless — content under a transparent title bar, native traffic lights kept, the tab strip is the drag
  area; the title text is hidden natively and re-applied whenever the native window is re-created after close-to-tray
  (`app/WindowChrome.h:1-10`, `:52-58`; `app/WindowChrome_mac.mm:17-23`). `--native-frame` is a developer switch
  (`app/main.cpp:458-459`).
- Windows: the STANDARD frame since 1.3.2, forced dark; the custom title row was withdrawn because Qt delivered every
  title-row press twice (`app/main.cpp:448-462`, `:409-416`; `app/WindowChrome.h:11-22`).

### 6.5 Menus, tray, accent, login item

- No application menu bar was found (`Main.qml` declares none); actions live in QML menus. "Check for Updates" is a
  status-bar item (`app/qml/Ufb/App/Main.qml:2328`; `devdocs/auto-update-setup.md:124`).
- Tray / menu-bar icon hosted by the app itself: Open, a live "Mounts" submenu, Start at Login (Windows), Quit; a
  template image on macOS; left click opens the window on Windows (`app/qml/Ufb/App/Main.qml:147-205`). Closing the
  window keeps the app resident; quitting is explicit (`app/main.cpp:512-519`).
- Accent colour: macOS `controlAccentColor`, re-read on system colour changes, exposed to QML
  (`app/MacAccent.h:1-12`). Windows accent: not found.
- macOS "Finder menu" built in-app from AppKit (Open With, Show in Finder, Get Info by Apple event, Share, Tags)
  (`app/MacShellMenu.h:1-17`).
- Start at login: Windows only. Store build = the manifest's startup task toggled through WinRT on a helper thread,
  with a "blocked by system" state that sends the user to Settings; unpackaged = the HKCU Run value with `--background`
  (`app/LoginItem.h:1-18`; `app/LoginItem.cpp:28-33`, `:147-202`). macOS has none since 1.2.0 (`app/LoginItem.h:17-18`).

### 6.6 Helpers: how they start and stop

| Helper | Start | Stop | Evidence |
|---|---|---|---|
| Mount / sync agent (`ufb-agent`) | on demand by the app when a sync mount needs it ("heal on open"); no LaunchAgent, no login item | NOT stopped at app quit (it hosts the mounts); killed by the installers before they write | `app/MacOSBootstrap.mm:32-45`; `bindings/src/services/mount.rs:653`, `:756`; `app/main.cpp:109-112`; `packaging/macos/scripts/preinstall:18-22` |
| FinderSync extension (macOS) | by the system; embedded in the app bundle's PlugIns | by the system | `scripts/release-mac.sh:118-126` |
| `ffmpeg`, `ffprobe`, `exiftool`, 7-Zip | child processes per job, found beside the executable, then on PATH | with the job | `bindings/src/services/transcode.rs:109`; `app/CMakeLists.txt:863-870` |
| Web engine helper | by Qt when an HTML preview opens | by Qt | `app/main.cpp:241-242` |
| Sparkle's updater helpers | by Sparkle | by Sparkle | `scripts/sign-mac-dev.sh:194-229` |

### 6.7 Crash handling and logging

- No crash reporter and no app-level panic log were found in the GUI. The agent installs a panic hook
  (`agent/src/main.rs:349`).
- Decoder crashes inside a thumbnail job are caught: Windows structured exceptions are translated to C++ exceptions
  (that one file is compiled `/EHa`); macOS installs handlers for SIGSEGV / SIGBUS / SIGILL / SIGFPE and long-jumps
  out of the job (`app/UfbImageProviders.cpp:39-78`, `:81-125`, `:135-161`; `app/CMakeLists.txt:480-492`).
- Logging: Qt messages to stderr (+ optional file); Rust side through `env_logger` (`app/main.cpp:124-146`;
  `bindings/src/lib.rs:47`); the agent uses its own logger (`agent/src/main.rs:288`). No rotating GUI log file found.

---

## 7. Updater and the identity to KEEP

### 7.1 macOS

Sparkle 2.9.2, standard controller started with the app, daily check (`app/updater/Updater_mac.mm:23-34`;
`app/Info.plist.in:99-106`). Since 1.2.0 the feed item is a signed installer **pkg**; Sparkle verifies the EdDSA
signature and runs a guided package install with an administrator prompt, then relaunches
(`scripts/make-appcast.sh:8-14`; `devdocs/auto-update-setup.md:14-19`). Feed entries are inserted at a marker in the
committed feed file; pushing the site publishes (`scripts/make-appcast.sh:20-21`; `devdocs/auto-update-setup.md:106-108`).
The feed carries `minimumSystemVersion` 14.0 (`docs/appcast-mac.xml:17`).

### 7.2 Windows

The Microsoft Store is the only channel and the updater since 1.4.0; the app's updater is a stub and the menu item
hides (`app/updater/Updater_stub.cpp:1-22`; `packaging/windows/msix/README.md:3`). The old WinSparkle feed is frozen at
1.3.3 and the Inno installer is an unpublished fallback (`devdocs/auto-update-setup.md:3-5`; `installer/README.md:3-5`).
Machines still on the 1.3.x installer build are not moved to the Store by anything found here.

### 7.3 Constants (all public in the audited repository's packaging files)

| Constant | Value | Evidence |
|---|---|---|
| macOS bundle id (app) | `dev.ufb.app` | `app/Info.plist.in:9-10` |
| macOS bundle id (agent) | `dev.ufb.agent` | `agent/macos/Info.plist:9-10` |
| macOS bundle id (FinderSync extension) | `dev.ufb.tray.findersync` | `macos-helpers/project.yml:49` |
| macOS executable / bundle names | `ufb` in `UFB.app`; `ufb-agent.app` | `app/Info.plist.in:7-8`; `scripts/build-mac-pkg.sh:7-8` |
| Install location | `/Applications/UFB/` (both bundles + an uninstall script), non-relocatable | `packaging/macos/distribution.xml:5-7` |
| Installer package id | `dev.ufb.UFB` | `scripts/build-mac-pkg.sh:38` |
| App Group | `5Z4S9VHV56.group.com.unionfiles.ufb` (team prefix `5Z4S9VHV56`) | `app/UFB.entitlements:36-39`; `app/main.cpp:97` |
| Sparkle feed | `https://ufbrowser.com/appcast-mac.xml` | `app/Info.plist.in:99-100` |
| Sparkle EdDSA public key | the value at `CMakeLists.txt:44` (private half in the owner's keychain only) | `CMakeLists.txt:39-45` |
| Release asset name the feed links to | `UFB-<version>-arm64.pkg` on the repository's release `v<version>` | `docs/appcast-mac.xml:30`; `devdocs/auto-update-setup.md:117-119` |
| URL schemes | `ufb`, `union` | `app/Info.plist.in:45-49` |
| MSIX identity name | `cbkow.ufBrowser` | `packaging/windows/msix/CMakeLists.txt:20` |
| MSIX publisher | `CN=C64A1D41-EFAA-4EED-AE81-0C074867D4E9` | `packaging/windows/msix/CMakeLists.txt:21` |
| Package family name / Store ID | `cbkow.ufBrowser_k3nw8mfd599r4` / `9N71KC2RQ65R` | `packaging/windows/msix/CMakeLists.txt:17` |
| Store display name | `ufBrowser` | `packaging/windows/msix/AppxManifest.xml.in:42-43` |
| MSIX application id / executable | `ufb` / `ufb.exe` | `packaging/windows/msix/AppxManifest.xml.in:57` |
| Startup task id | `ufbStartup` | `packaging/windows/msix/AppxManifest.xml.in:83`; `app/LoginItem.cpp:29` |
| Firewall rules | TCP 49221–49222 in, UDP 4265 in, on `ufb.exe` | `packaging/windows/msix/AppxManifest.xml.in:90-95` |
| Windows data folder | `%LOCALAPPDATA%\ufb` (virtualised under the package) | `app/main.cpp:66-78`; `packaging/windows/msix/AppxManifest.xml.in:13-15` |
| macOS data folder | `~/Library/Application Support/ufb` | `app/main.cpp:70-75` |
| Unpackaged-only (legacy): AUMID, Inno AppId, Run value | `com.unionfiles.ufb`; the GUID at `installer/ufb_installer.iss:42`; `UFB` | `app/main.cpp:179`; `installer/ufb_installer.iss:246`, `:261` |

To update in place the rebuild must keep: the bundle ids (TCC consent and Sparkle are keyed to them —
`app/Info.plist.in:53-61`), the feed URL and key, the pkg layout or a migration from it, the App Group (the agent
socket and the single-instance socket live in its container), and the whole MSIX identity + startup task id.

---

## 8. Packaging

### 8.1 macOS

- **Artifact: a signed, notarized installer pkg**, Apple silicon only, macOS 14+ (`packaging/macos/distribution.xml:12-15`).
- **Why a pkg, not a DMG** (the stated reasons): the product is TWO bundles side by side (`UFB.app` + `ufb-agent.app`),
  which a drag-install cannot lay down deterministically; a pkg can stop the running app and agent first; and it is
  what lets Sparkle install updates itself — a drag-install DMG could only be linked to
  (`scripts/build-mac-pkg.sh:11-15`; `devdocs/auto-update-setup.md:20-22`).
- Preinstall (root): kill app and agent, remove the old bundles; deliberately no per-user work
  (`packaging/macos/scripts/preinstall:1-28`).
- Release sequence: vcpkg (static libheif + OpenEXR pinned to the deployment target) → cargo (agent) → CMake (app, with
  macdeployqt) → xcodebuild (FinderSync extension) → embed the extension → gates → sign → notarize both bundles →
  build pkg (Developer ID **Installer** identity) → notarize the pkg → feed entry (`scripts/release-mac.sh:1-17`,
  `:54-73`, `:102-126`; `scripts/build-mac-pkg.sh:26-29`). Two trips through the notary queue, 15–30 minutes
  (`scripts/release-mac.sh:16-17`).
- Gates in the release script: every Mach-O's minimum OS ≤ 14.0; libheif / OpenEXR must come from vcpkg, never
  Homebrew (`scripts/release-mac.sh:131-162`); and a check that every bundled dylib's install-name leaf equals the
  name it is requested by (`scripts/fix-ffmpeg-install-names-mac.sh:27-32`).
- Signing order (inside-out): bundled dylibs → FinderSync extension (own entitlements) → Sparkle's XPC services,
  Updater.app, Autoupdate → the web engine helper (JIT entitlements) → every framework → `ffmpeg` / `ffprobe` → the
  app last with its entitlements (`scripts/sign-mac-dev.sh:160-273`).
- App entitlements: unsigned executable memory, library validation OFF, network client + server, Apple events,
  user-selected files, the App Group (`app/UFB.entitlements:10-39`). Hardened runtime, no sandbox entitlement found.
- Provisioning profile: NOT in use. A shared keychain group was tried and rejected at launch without a profile; a
  design for per-bundle Developer ID profiles under a git-ignored `signing/` folder exists as a document only
  (`app/UFB.entitlements:41-52`; `devdocs/macos-provisioning-profile.md:1-9`, `:55-75`). The agent's entitlements are
  the mount reader's area.
- Privacy usage strings for Documents, Downloads, Desktop, removable and network volumes, and Apple events
  (`app/Info.plist.in:62-84`).
- Icon: legacy `.icns` plus a layered icon compiled with `actool` (needs full Xcode) (`app/Info.plist.in:21-28`;
  `app/CMakeLists.txt:571-576`).
- Size: the 1.4.2 pkg in the local `dist/` folder is about 350 MB (untracked build output; not a published figure).

### 8.2 Windows — the Store package

- Capabilities: exactly `runFullTrust` + `privateNetworkClientServer`; no `unvirtualizedResources` (a sibling app was
  refused for it); no hidden application entries or execution aliases (`packaging/windows/msix/AppxManifest.xml.in:9-25`,
  `:98-101`).
- Extensions: two protocols, a startup task (enabled by default after first launch), firewall rules
  (`packaging/windows/msix/AppxManifest.xml.in:67-96`).
- Minimum Windows 10.0.17763, x64 only (`packaging/windows/msix/AppxManifest.xml.in:39`, `:49`).
- **WinFsp is not in the package** — the Store cannot install drivers. Users install it themselves for sync mounts;
  the listing must disclose that first and the app shows an "Install WinFsp…" control
  (`packaging/windows/msix/AppxManifest.xml.in:17-18`; `packaging/windows/msix/README.md:73-74`, `:121`).
- Build: stage the release tree (windeployqt runs inside the pack script) + the agent built for the explicit target
  triple → C++ runtime → MakePri → MakeAppx; unsigned for Partner Center, optional signed sideload twin
  (`packaging/windows/msix/CMakeLists.txt:1-8`, `:34-52`; `packaging/windows/msix/README.md:18`, `:42-56`).
- Size: the 1.4.2 MSIX in the local `dist/` folder is about 282 MB.

### 8.3 Windows — the older Inno installer (fallback, unpublished since 1.4.0)

Bundled and did: the app tree, the agent, `ffmpeg` / `ffprobe`, `exiftool` + its Perl tree, 7-Zip, and the **WinFsp
redistributable MSI**, installed silently when the registry shows no WinFsp (`installer/ufb_installer.iss:115-201`,
`:336-358`); protocol registration in HKCR; AUMID registration; an optional Run-key autostart; kills app and agent
before copying; runs both with `--prime-smartscreen` (`installer/ufb_installer.iss:214-261`, `:295-301`, `:371-386`).

### 8.4 Bundled command-line tools

| Tool | Used for | macOS | Windows | Licence |
|---|---|---|---|---|
| `ffmpeg`, `ffprobe` | transcode + frame count | built from source 9.0.1 with libx264 static; in `Contents/MacOS` | a GPL shared prebuilt of the 9.0 branch, the whole `bin/` | GPL-2.0-or-later (x264) |
| ExifTool 13.59 | copy metadata onto transcoded files | the Perl script + `lib/` on the SYSTEM perl, under Resources | the official exe + its Perl tree | Artistic-1.0 or GPL |
| 7-Zip 26.03 | extract (zip / 7z / rar / tar), compress to zip | `7zz`, built from unmodified source, universal | official `7z.exe` + `7z.dll` | LGPL-2.1+ with the unRAR restriction; BSD-3 parts |

Evidence: `LICENSES/THIRD_PARTY_NOTICES.txt:47-107`, `:183-193`; `scripts/build-external-ffmpeg-mac.sh:31`, `:57-61`;
`app/CMakeLists.txt:863-870`, `:900-903`, `:963-968`; `installer/ufb_installer.iss:166-186`.

### 8.5 Third-party notices

A hand-maintained `LICENSES/THIRD_PARTY_NOTICES.txt` (13 direct entries) plus per-component licence files, plus a
generated Rust attribution file (about 9 250 lines) from `cargo-about` with an allow-list that fails on an unknown
licence (`LICENSES/THIRD_PARTY_NOTICES.txt:16-212`; `about.toml:1-37`; `scripts/gen-rust-notices.sh`). Source tarballs
for the copyleft parts are attached to each release (`scripts/fetch-third-party-sources.sh`;
`LICENSES/THIRD_PARTY_NOTICES.txt:59-60`). minplatform generates its page per app and checks it against the staged
files (`mp:packaging/notices/make_notices.py:1-15`).

---

## 9. Licences of what is bundled; constraints on a GPL-3 rebuild and on the Store

UFB itself: GPL-3.0-or-later, stated as chosen partly to satisfy WinFsp's FLOSS exception
(`LICENSE:1-2`; `LICENSES/THIRD_PARTY_NOTICES.txt:4-14`).

| Component | Licence | Linkage in UFB | Rebuild / Store note |
|---|---|---|---|
| Qt 6.11.1 (incl. WebEngine) | LGPL-3.0 | dynamic | **gone** in a Rust rebuild, with its relinking obligations and the Chromium notices |
| cxx-qt | MIT / Apache-2.0 | static | gone |
| FFmpeg 9.0.x + x264 | GPL-2.0-or-later | dynamic + CLI | compatible; minplatform already ships a patched 9.0.2 with x264 listed (`mp:packaging/notices/components.json:778-782`). No "nonfree" parts (`LICENSES/THIRD_PARTY_NOTICES.txt:57-58`) |
| PDFium | BSD-3 (+ Apache-2.0 parts) | dynamic | replaceable by `hayro` (check its licence when adopting; not verified here) |
| OpenEXR + Imath | BSD-3 | dynamic (Windows) / static (macOS) | already in `min-image` |
| libheif + libde265 | LGPL-3.0 | static on macOS, DLLs on Windows | fine under GPL-3 with public source (`LICENSES/THIRD_PARTY_NOTICES.txt:129-135`); HEVC patents are a distribution question the notices do not address — not verified. On macOS, ImageIO avoids bundling a decoder |
| psd_sdk | BSD-2 | static | replaceable by the notes app's own reader |
| stb_image (HDR only) | not listed in the notices file (public domain / MIT upstream) | compiled in (`app/thumbnails/HdrBackend.cpp:11-14`) | a notices omission to fix or drop |
| miniz | MIT | static | replaced by the `zip` crate already used (`mp:apps/mn/Cargo.toml:59-60`) |
| SQLite | public domain | static (Rust) and via Qt | same as the siblings |
| WinFsp | GPL-3.0 with FLOSS exception | **not shipped** since 1.4.0; loaded if installed | the Store forbids driver installs; the listing must disclose the prerequisite first (`packaging/windows/msix/AppxManifest.xml.in:17-18`) |
| ExifTool | Artistic-1.0 or GPL | separate process | fine; ships a Perl runtime tree on Windows (size) |
| 7-Zip | LGPL-2.1+, unRAR restriction | separate process | fine as a separate program; the app must never CREATE RAR (it only extracts) (`LICENSES/THIRD_PARTY_NOTICES.txt:104-107`) |
| Sparkle | MIT | dynamic | same as the siblings |
| Phosphor icons, Geist fonts | MIT, OFL-1.1 | assets | already the family's (`mp:packaging/notices/licenses/`) |
| Rust crates | permissive allow-list; `winfsp` crates GPL-3.0 | static | same generator pattern as minplatform |

Store constraints observed: no drivers; capabilities must stay minimal (two siblings' refusals are recorded in the
manifest's notes); the display name `ufb` was taken, hence `ufBrowser`; identity mismatches are rejected at ingestion
(`packaging/windows/msix/AppxManifest.xml.in:9-25`; `packaging/windows/msix/CMakeLists.txt:14-19`).

---

## 10. Build system: size and pain points

| Piece | Size | Evidence |
|---|---|---|
| CMake | 173 (root) + 1 070 (`app/`) + 201 (`cmake/external.cmake`) + 57 (MSIX) lines | `CMakeLists.txt`, `app/CMakeLists.txt`, `cmake/external.cmake`, `packaging/windows/msix/CMakeLists.txt` |
| Scripts | 16 files, about 2 480 lines (shell + PowerShell) | `scripts/` |
| Tool chains needed | CMake, Qt 6.11 (+ optional WebEngine module, + ShaderTools), cargo (two workspaces), Corrosion, cxx-qt (needs `QMAKE`), vcpkg (overlay triplets), Xcode + xcodegen (extension, `actool`), MSVC, the Vulkan SDK on Windows, Inno Setup (legacy) | `CMakeLists.txt:79-90`, `:111-157`; `app/CMakeLists.txt:240`; `scripts/release-mac.sh:60-73`, `:107-115`; `justfile:6-11` |
| Externals fetched or built outside the build | FFmpeg (built from source on macOS, about 15 min; copied from a SIBLING checkout or downloaded on Windows), PDFium prebuilt, 7-Zip, ExifTool (copied from a legacy sibling checkout), Sparkle, WinFsp MSI | `scripts/build-external-ffmpeg-mac.sh:7`; `scripts/setup-external.ps1:8-35` |

Pain points, each recorded in the repository:

- The version lives in up to seven files moved together by a script, whose list still names a deleted helper bundle
  (`scripts/bump-version.sh:4-21`).
- Qt's own `FindRust` stub beats Corrosion's unless the module path is prepended by hand (`CMakeLists.txt:119-126`).
- A local Qt header must be patched for an Apple Clang error on Qt 6.11.0 (`scripts/patch-qt-mac-headers.sh:4-16`).
- The deployment target must be pinned in four places (CMake, vcpkg triplets, cargo environment, the release gate)
  after releases that only ran on the build machine's OS (`CMakeLists.txt:12-19`; `scripts/release-mac.sh:47-52`).
- macdeployqt bundles SQL drivers with dangling references that must be deleted, and its auto-sign cannot be used
  (`app/CMakeLists.txt:724-729`, `:714-720`).
- MSVC drops the Rust bindings' QML registrations unless the static library is whole-archived (`app/CMakeLists.txt:461-465`).
- FFmpeg headers must be forced first on the include path or struct layouts shift and the driver crashes
  (`app/CMakeLists.txt:448-457`).
- A dylib naming mismatch made 1.4.1 fail at launch on a newer macOS; fixed in 1.4.2 with a rename pass and a release
  gate (`scripts/fix-ffmpeg-install-names-mac.sh:17-25`; `docs/appcast-mac.xml:19-27`).
- A stale agent binary from the wrong target folder shipped in several installers (`installer/ufb_installer.iss:36-38`).
- Windows debug is really RelWithDebInfo because of the Rust runtime (`justfile:15`; `app/CMakeLists.txt:527`).
- The Windows build depends on sibling checkouts for FFmpeg and ExifTool (`scripts/setup-external.ps1:8-16`).

What a pure-Rust rebuild on minplatform removes: CMake, Corrosion, cxx-qt and the QML type registration issues; Qt and
its two deploy tools, plugins, `qt.conf`, the header patch; QtWebEngine (the helper process, JIT entitlements, the
GPU-compositing workaround, most of the package size — not measured); the second copy of the player and its D3D11 +
Vulkan bridges (about 15 400 lines); PDFium, psd_sdk, miniz, stb, the C++ OpenEXR API, the `/EHa` and signal-jump
crash guards (Rust decoders return errors; the FFI ones still need fencing, as `mp:apps/mn/src/pdf.rs:5-7` does);
the per-file ARC flags. What it does NOT remove: the two-bundle macOS layout with an app extension (Swift, Xcode), the
pkg flow, the App Group, FFmpeg's build, and — if HEIF on Windows is kept — a C decoder.

---

## 11. Rough spots

Bugs and risks:

1. **PDF: a cached document is closed but left in the cache.** On a bitmap allocation failure `decodePdfPage` closes
   `doc`, which the single-entry cache still points to; the next call uses a freed handle
   (`app/thumbnails/PdfBackend.cpp:179-184` vs the ownership rule at `:62-63`, `:130`).
2. **macOS crash guard is a `siglongjmp` out of a fatal signal** inside decoder code; the file itself calls it
   best-effort and names out-of-process decoding as the correct design (`app/UfbImageProviders.cpp:90-95`). Locks held
   by the decoder at the fault — including the PDFium mutex (`app/thumbnails/PdfBackend.cpp:128`) — are never released.
3. **Windows shell thumbnails leak a thread per timeout** (detached, never joined) (`app/thumbnails/ShellThumbnail_win.cpp:101-117`);
   the macOS path blocks a pool worker up to 10 s (`app/thumbnails/ShellThumbnail_mac.mm:95-103`).
4. **Thumbnail cache does a full-table `SUM` after every insert** and a write on every hit; rows for deleted or renamed
   files are never removed except by LRU; the key is the raw path string (`app/thumbnails/ThumbCache.cpp:140-146`,
   `:185-192`).
5. **EXR alpha is gamma-encoded like colour and stored as straight alpha**, though EXR colour is premultiplied — edges
   and semi-transparent areas are wrong (`app/thumbnails/ExrBackend.cpp:96-99`, `:232-235`).
6. **HEIF reads the whole file into memory** before the pixel cap is checked (`app/thumbnails/HeifBackend.cpp:118-130`,
   `:150-158`); libheif needs a fake plugin path to stop a ~70-library load scan per thumbnail (`:24-45`).
7. **psd_sdk's macOS file reader has a use-after-free** under load; UFB replaces it with its own synchronous reader
   (`app/thumbnails/SyncPsdFile.h:3-11`).
8. **GUI-thread I/O**: PDF page count / aspect and the EXR layer list are synchronous calls from QML bindings
   (`app/PdfDoc.h:21-22`; `app/ExrInfo.h:22-25`); the video open probes the file on the GUI thread
   (`app/qml/Ufb/App/VideoPreview.qml:41-46`); notes and Word previews render synchronously
   (`app/MndbDoc.h:55-59`; `app/DocxDoc.h:43-46`).
9. **HTML previews run JavaScript from job-folder files** by decision; only the Word route narrows link schemes
   (`app/qml/Ufb/App/HtmlPreview.qml:8-11`, `:29-34`).
10. **PDF reader assumes page 0's aspect for every page** (`app/qml/Ufb/App/PdfReader.qml:22-25`).
11. **No visibility-based priority and no mid-decode cancel** for thumbnails (`app/UfbImageProviders.cpp:186-190`).
12. **Video thumbnails ignore HDR transfer and rotation** (nothing found) (`app/thumbnails/VideoBackend.cpp:43-60`).
13. **Windows custom title bar withdrawn** after two broken releases; code kept "for a later attempt"
    (`app/WindowChrome.h:11-22`).
14. **Close-to-tray re-creates the native window**, so native setup must be re-applied each time
    (`app/WindowChrome.h:52-57`; `app/main.cpp:413-414`).
15. **macOS agent auto-start was removed** because every probe triggered consent prompts; auto-start at login is noted
    as future work (`app/MacOSBootstrap.mm:33-45`).

Qt workarounds: sRGB surface forced on macOS (`app/main.cpp:224-236`); web engine GPU compositing off
(`app/main.cpp:244-250`); deferred web view creation and scripted scrollbar CSS (`app/qml/Ufb/App/HtmlPreview.qml:83-90`,
`:126-131`); thumbnail element behind a `Loader` to stop recycle flicker (`app/qml/Ufb/App/FileBrowser.qml:3291-3299`);
jumbo icons only (`app/UfbImageProviders.cpp:505-512`); window cloaking against the white flash (`app/main.cpp:681-705`);
`WM_SETICON` by hand (`app/main.cpp:639-649`); a context object declared before the engine to avoid teardown errors
(`app/main.cpp:448-453`); provider image size not scaled by pixel ratio (`app/qml/Ufb/App/FileBrowser.qml:3325-3329`).

Stale comments that contradict the code (they will mislead a porter):

- Thumbnailer header: "skeleton … returns false", "64 MP", "~1.5 GB" vs 256 MP and RAM-scaled 2–8 GB
  (`app/thumbnails/Thumbnailer.h:19-22`, `:55-59`; `app/thumbnails/Thumbnailer.cpp:169-193`).
- PSD header says GraphicsMagick; the code is psd_sdk (`app/thumbnails/PsdBackend.h:3-10`; `app/thumbnails/PsdBackend.cpp:3-13`).
- Shell thumbnail header says macOS "added in a later phase" (`app/thumbnails/ShellThumbnail.h:12`).
- `Info.plist.in` and `Updater.h` describe an informational DMG feed; the feed installs a pkg since 1.2.0
  (`app/Info.plist.in:86-98`; `app/updater/Updater.h:4-7`; `scripts/make-appcast.sh:8-14`).
- The video decoder's header describes a software-only skeleton (`app/player/video_decoder.h:1-17`).
- `main.cpp`'s header and the bootstrap header describe phases long past (`app/main.cpp:1-7`; `app/MacOSBootstrap.h:1-15`).
- The notices list PDFium, OpenEXR and libheif as DLLs "next to ufb.exe" while macOS links two of them statically
  (`LICENSES/THIRD_PARTY_NOTICES.txt:113-114`, `:122-123`, `:132-133`).

TODO / FIXME / HACK markers: a search of this area's sources for those words found none; the debt is recorded in
prose comments (cited above).

Duplicated decoders across the family:

| Capability | Copies |
|---|---|
| The player (decode, scrub, audio) | UFB `app/player/` (C++ copy) · the review player's original (C++) · `mp:crates/qcv-player` + `mp:crates/min-ffmpeg` (Rust) |
| EXR layer loader | UFB `app/thumbnails/ExrBackend.cpp` (ported from the review player, `:3-10`) · `mp:crates/min-image/src/exr.rs` |
| PSD composite | UFB (psd_sdk) · `mp:apps/mn/src/psd.rs` |
| PDF pages | UFB (PDFium) · `mp:apps/mn/src/pdf.rs` (hayro) |
| Video frame for a thumbnail | UFB `VideoBackend.cpp` · UFB's player · `mp:crates/qcv-player/src/thumbnail_cache.rs` · `mp:apps/mn/src/video.rs` |
| Notes documents | the notes app's model · UFB `app/MndbDoc.cpp` (a second renderer, contract "verified against" a dated notes version, `app/MndbDoc.h:13-14`) |
| Word documents | UFB `app/DocxDoc.cpp` (to HTML) · `mp:apps/mn/src/import_office.rs` (to blocks) |
| Zip reading | UFB miniz · Rust `zip` crate · 7-Zip program |
| FFmpeg builds | UFB's 9.0.1 (macOS source build, Windows prebuilt) · the family's patched 9.0.2 (per the project notes of this repository) |
| Sparkle signing order, notarization, MSIX packing | UFB `scripts/` · `mp:packaging/` |

---

## 12. Open questions for the owner

1. **Rendered HTML.** There is no web engine on minplatform. Drop rendered HTML previews (text only), use the OS web
   view (WKWebView / WebView2), or hand `.html` to the browser? This also decides how notes and Word previews render
   (natively through the notes crates, which needs no engine).
2. **macOS delivery.** Keep the pkg at `/Applications/UFB/` (admin prompt on every update, two bundles), or move to
   one bundle with the agent and extension inside and a DMG like the siblings? A move needs a migration for installed
   1.4.x (their Sparkle expects a pkg enclosure) and a ruling on the agent's location.
3. **Bundle id and App Group.** Confirm they are kept as they are (`dev.ufb.app`, the `com.unionfiles` group), mixed
   naming included.
4. **HEIF on Windows.** Bundle libheif + libde265 as UFB does, rely on the OS codec through the shell, or leave HEIC
   without a picture there? (And: is the renamed-phone-photo sniff still wanted?)
5. **OS shell first, or ours first?** UFB shows the OS's thumbnail for PSD / PDF / video when the shell has one. Keep
   that order (cheaper, but looks differ per machine and can stall on shares) or decode ourselves first and use the
   shell only for RAW / Office / AVIF?
6. **PDF engine.** Is `hayro` acceptable for production PDFs and Illustrator files, or must PDFium stay for the browser?
7. **Thumbnail colour.** Keep UFB's clamp + 2.2 gamma for EXR / HDR, or use the presenter's pipeline as the review
   player and the notes app do (and tone-map HDR video)? What should EXR alpha look like?
8. **Lightbox scope.** Zoom / pan for stills, per-channel EXR view, PDF text selection, an audio waveform, a timecode
   readout, a metadata panel — all absent today and all cheap or free on minplatform. Which belong in the first release?
9. **Per-file icons.** Icons are per extension today. Should applications, custom-icon folders and aliases show their
   own icons?
10. **Thumbnail cache.** Carry `thumbnails.db` as it is (so existing caches survive), or start a new cache with a new
    key (volume-relative path? content-independent id?) and proper removal of dead rows?
11. **Bundled tools.** Keep ExifTool (with its Perl tree on Windows) and 7-Zip as programs, or replace the uses with
    crates (metadata copy through FFmpeg, archives through Rust crates — RAR extraction is the hard part)?
12. **Windows 1.3.x installs.** Machines still on the Inno build receive nothing. Is a last WinSparkle item pointing
    at the Store wanted, or is that fleet already moved?
13. **Start at login on macOS.** It was removed in 1.2.0; is it wanted back in the rebuild (a login item or a
    LaunchAgent), given the consent-prompt history?
14. **Windows title row.** Standard frame (as now) or the chromeless row the siblings use on Windows?
15. **stb_image** is compiled in but not listed in the notices; and the notices' linkage lines are Windows-only.
    Fix in 1.4.x, or let the rebuild's generated page supersede them?
