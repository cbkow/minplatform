# The Quick Look extension: what the code offers it (read 2026-11-26)

A reading of the current code for the design of QCView's Quick Look extensions (canon rows 605-608; spikes
`docs/spikes/S20-quicklook.md`, `docs/spikes/S21-preview-view.md`). Nothing here is a ruling. "Read" means the cited
lines say so; "inferred" means it follows from them but was not run; [proposed] marks a suggestion for the owner.
Paths are relative to the repository root.

Decided before this reading: a thumbnail extension and a view-based preview extension for `org.smpte.mxf` and
`com.ilm.openexr-image`; a Swift shell around a Rust static library each; the plain sandbox (one file, no neighbours);
the preview is the picture on black with one hover bar (EXR: layer and colour pickers; MXF: play, playbar, timecode,
sound); the default colour is what QCView shows for the file with no saved chain.

## 1. EXR layers

All in `crates/min-image/src/exr.rs` (OpenEXRCore through FFI, `exr.rs:1-6`).

**Listing and names** (`layers`, `exr.rs:44-59`, read):
- A multi-part file lists its part names in part order; a nameless part is `default (part N)` (`exr.rs:47`).
- A single-part file lists `default` first when any channel has no dot, then each distinct prefix before a channel's
  LAST dot, in channel order (`exr.rs:48-56`). A file with nested names (`a.b.R`) therefore lists `a.b`.
- Names containing "crypto" (any case) are dropped (`exr.rs:38`, `47`, `52`). The list is never empty (`exr.rs:57`).

**Which layer QCView shows first.** The app stores the list at import (`apps/qcv/src/main.rs:198-205`) and opens a
sequence with the item's layer, `""` when none was chosen (`crates/qcv-player/src/transport.rs:161`). `""` loads part 0
with the bare `R/G/B`, or, when the file has no bare `R`, the first channel ending in `.R` gives the prefix
(`exr.rs:86`, `96-98`). The harness and spike S21 take `layers[0]` (`apps/qcv/src/main.rs:2941`,
`spikes/s21-preview-view/src/lib.rs:39`). Inferred: `""` and `layers[0]` name the same picture for single-part files and
for multi-part files whose part 0 is not cryptomatte; they differ when part 0 is filtered out of the list.

**Loading** (`load`, `exr.rs:83-170`): the part named `layer`, else part 0 (`exr.rs:86`); channels `<layer>.R/G/B/A`,
falling back to the bare names when the layer is absent (`exr.rs:96-97`). `"default"` must be passed as `""`
(S21 does it, `spikes/s21-preview-view/src/lib.rs:16`). The image is the DISPLAY window with the data window placed in
it, zeros outside (`exr.rs:155-169`). Output: planar G, B, R [, A] half floats, rows padded to 256 bytes
(`crates/min-image/src/lib.rs:48-52`). Chunks decode in parallel on scoped threads; `threads == 0` means all cores,
capped at 8 (`exr.rs:116`, `120-152`). `dimensions` reads headers only (`exr.rs:71-79`).

**Alpha.** A fourth plane exists only when the layer has an `A` channel; none is synthesised (`exr.rs:6`, `101-102`).
The engine blends the picture over its viewport background by that alpha, premultiplied
(`crates/min-present/src/engine.rs:189`, `237`, `305`); the background mode is `Engine::set_background(0..3)`
(`engine.rs:552`), QCView's setting `display/backgroundMode` (`apps/qcv/src/settings.rs:45`).

**Limits** (read):

| Case | Behaviour | Where |
|---|---|---|
| Deep data | refused: "deep EXR is not supported" | `exr.rs:113` |
| Tiled | level 0 only (`exr_get_tile_counts(..., 0, 0, ...)`) | `exr.rs:107-109`, `128` |
| Multi-part | by part name; a layer inside a part other than 0 is reachable only as that part | `exr.rs:86` |
| Subsampled channels | skipped (left zero) unless x and y sampling are 1 | `exr.rs:138-143` |
| Layer without R, G and B | `Error::NoRgb`: a single-channel layer (depth, a matte) does not show | `exr.rs:100` |
| Size | no cap and no file-size gate | `exr.rs:18` |
| Compression | whatever the linked OpenEXRCore decodes; `compression()` names types 0-9 (NONE to DWAB) only | `exr.rs:63-68` |
| A failed chunk | the whole load fails | `exr.rs:153-154` |

`min_image::load_thumbnail` has no reduced decode for EXR: it decodes the whole frame with 2 threads
(`crates/min-image/src/lib.rs:83-91`).

## 2. Default colour with no saved chain

**The finding that matters: with no saved chain QCView shows the file in BYPASS.** Colour is switched off at every
launch (`apps/qcv/src/main.rs:2778`), at every new project (`main.rs:366`) and at every project open (`main.rs:336`),
and is switched on only when a clip of the project carries its own chain (`apps/qcv/src/color.rs:154-167`) or the user
picks or edits one (`color.rs:341-347`). The schema default of `color/enabled` is false (`apps/qcv/src/settings.rs:126`).
With colour off `apply_color` binds no pass: `engine.set_color_pass(None)` (`color.rs:240-243`).

So the decided default ("what QCView shows for that file with no saved chain") is the presenter's own conversion from
the frame's tags, with no minColor and no OCIO:

| File | Tags the frame carries | Where |
|---|---|---|
| EXR (half float) | linear, Rec.709 primaries, full range, RGB | `crates/qcv-player/src/sequence_decoder.rs:190-194` |
| 8 / 16-bit still | sRGB, Rec.709 primaries | same |
| MXF (any video) | the stream's own matrix, range, transfer and primaries (`SessionInfo.meta`) | used at `apps/ufb/src/thumbs/helper.rs:124-125` |

Inferred from `apps/qcv/src/thumb_color.rs:1-6`, `40-43` and its tests (`thumb_color.rs:116-126`): bypass shows a linear
EXR through the plain sRGB encode on an SDR target (values above 1.0 clip, or use the screen's headroom on the
extended-range surface), and an HDR-tagged video as HDR (BT.2390 only when a capture is brought to SDR). Spike S21
already built exactly these EXR tags (`spikes/s21-preview-view/src/lib.rs:20`), so its picture was QCView's default.

**What a chain is, once the user picks one.**
- `qcv_color::Chain` (`crates/qcv-color/src/lib.rs:15-24`): minColor's flat struct (input gamut / transfer / limited,
  Knee, AgX, Rendering). Its default is Rec.709 / Rec.1886 in, everything else off (`lib.rs:44-52`).
- `OcioChain`, `ClipChain { MinColor(Chain), Ocio(OcioChain) }`, `Stored { chain, preset }`
  (`apps/qcv/src/color_model.rs:9-22`). These live in the APP, not in a crate.
- The project default: `Stored::from_map(project.color_default())`, else `fallback_default()` = the first built-in
  preset, "Rec.709 video" (`color.rs:168`, `color_model.rs:131`, `153-154`). It is stored in the project file
  (`crates/qcv-project/src/io.rs:22`, `178`), not in the settings.
- A clip's chain: `chain_of` (`color.rs:174-178`): its own maps, else the project default passed through
  `stream_nudge`.
- `stream_nudge` (`color.rs:138-151`) changes only the INPUT of a default minColor chain, for two cases: a ProRes RAW
  clip (linear Rec.2020) and a live stream tagged PQ. Neither is an MXF or an EXR. There is no nudge for EXR: with
  colour on and the fallback default, an EXR would be read as Rec.1886.

**From a chain to the presenter's pass** (`apply_color`, `color.rs:230-269`):
- minColor: one `MinColorPass` built once on a worker (`mincolor_ahead`, `color.rs:123-128`); a chain change is
  `MinColorPass::set_chain` (uniforms only, `crates/qcv-color/src/lib.rs:156`); bound with
  `engine.set_color_pass(Some(Box::new(SharedPass(rc))))` (`color.rs:248-249`).
- OCIO: `qcv_color::ocio::Config::open(path)` then `OcioPass::new(device, queue, &config, Spec, knee)` on a worker,
  bound when it lands (`color.rs:258-265`, `278`).
- The slot: `Engine::set_color_pass(Option<Box<dyn ColorPass>>)` (`crates/min-present/src/engine.rs:373`); the trait is
  `ColorPass` (`engine.rs:26-34`). minColor takes the picture encoded as Rec.1886 and returns linear
  (`crates/qcv-color/src/lib.rs:167-168`).

**The thumbnail's look** (`apps/qcv/src/thumb_color.rs`): a second small `Engine` on the presenter's device shows the
thumbnail's samples tagged with the clip's transfer and primaries, runs a pass built like the slot's but re-targeted to
SDR (`peak_nits` and `agx_peak` = 100, `thumb_color.rs:78-81`; OCIO through `capture::sdr_view`, `:87`), captures,
`to_sdr` (BT.2390 for an HDR source in bypass), `srgb8` (`thumb_color.rs:39-45`). With colour off it is bypass
(`thumb_color.rs:73-74`). `qcv_player::poster::Poster` is the same stage without looks, for other programs
(`crates/qcv-player/src/poster.rs:1-6`).

## 3. Colour controls that map to dropdown pickers

| Control | Source of the list | Engine | Labels |
|---|---|---|---|
| Presets | `color_model::builtins()` (15 entries, `apps/qcv/src/color_model.rs:127-151`) + the user's `color-presets.json` beside the settings file (`color_model.rs:156-165`) | both | names are literals, not translated |
| Input gamut | `qcv_color::input_gamuts()` (`crates/qcv-color/src/lib.rs:109`): 19 entries (`core/opendrt_params.h:61`), order from `src/ffi.cpp:26` | minColor | names from the vendored core, not translated |
| Input transfer | `qcv_color::input_transfers()` (`lib.rs:110`), order from `src/ffi.cpp:23`, inverse curves hidden (`ffi.cpp:100`) | minColor | same |
| Rendering | Un-tone-mapped / OpenDRT with `look_names()` (`lib.rs:102`) / AgX (`apps/qcv/src/color_drawer.rs:195`) | minColor | `ColorPanel` |
| Config | `BUNDLED` (ACES 2.0, ACES 1.3, Blender 5.3; `apps/qcv/src/color.rs:74`) or a file the user opened | OCIO | literal |
| Input / Look / View | `Config::inputs()`, `looks()`, `views()` (`crates/qcv-color/src/ocio.rs:59-62`) | OCIO | the config's own names |

The captions ("Input gamut", "Input transfer", "Rendering", "Config", "Input", "Look", "View", "Presets", "Color On",
"Color Off") are all `min_i18n::trc("ColorPanel", ...)` (`apps/qcv/src/color_drawer.rs:21`). The drawer shows these as
list columns (`color_drawer.rs:190-191`), not as `min_ui::Picker`s; a bar needs pickers, which S21 used
(`crates/min-ui/src/picker.rs`).

[proposed] The smallest set for the preview bar, minColor only:
1. One picker "Color": first entry = off (bypass, the default of point 2), then the built-in presets
   (`builtins()`), which already cover sRGB, ACEScg, ACES 2065-1, linear Rec.709 / Rec.2020 with and without AgX, PQ
   and the camera logs.
2. For EXR that one picker may be enough. If the owner wants "colour-space dropdowns" literally: a second and third
   picker, Input gamut and Input transfer, shown only when the first is not off, editing the chosen preset's `Chain`.
3. No OCIO in the first version (cost and reach: point 4). No Knee, no AgX or OpenDRT parameters, no Video gamma.

Open under this proposal: bypass and "Rec.709 video" differ for an EXR, and the user's own presets are out of reach
unless point 8 is solved.

## 4. What the colour path needs at run time

| Need | Where it comes from | Outside the bundle? |
|---|---|---|
| minColor kernel | WGSL generated at build time and compiled in (`crates/qcv-color/src/lib.rs:113`, `build.rs:1-2`); the host derive is C++ linked statically (`src/ffi.cpp`) | no files at all |
| minColor pipeline build | 515 ms cold on macOS, built off the main thread (`apps/qcv/src/color.rs:5`); logged per run (`lib.rs:151`) | the system's shader cache, in the extension's own container (inferred from S21 finding 2) |
| OCIO library | `libOpenColorIO.dylib`, linked dynamically behind the crate feature `ocio` (`crates/qcv-color/build.rs:50-68`); shipped in `Contents/Frameworks` | inside the bundle |
| OCIO configs | `ocio_configs_dir()`: `QCV_OCIO_CONFIGS`, else `assets/OCIO` beside the executable, else `../Resources/assets/OCIO`, else the workspace copy (`color.rs:55-64`); staged by `packaging/macos/make-app.sh:14` | inside the APP's Resources |
| An OCIO pass | "tens to hundreds of ms" per chain (`crates/qcv-color/src/ocio.rs:245`), native MSL through `PASSTHROUGH_SHADERS` (`ocio.rs:1-3`, `engine.rs:280`) | no |
| A user's own OCIO config, a Scene LUT | absolute paths stored in the chain (`color_model.rs:10`, `color.rs:258-259`) | YES: unreadable in the plain sandbox |
| User presets | `color-presets.json` beside `settings.json` (`color_model.rs:156-160`) | YES (point 8) |
| A failed OCIO kernel | dumped to `std::env::temp_dir()` (`ocio.rs:203-206`) | S21 finding 4: that folder is not writable there; the write is `.ok()`, so harmless |

Things to flag:
- `ocio_configs_dir()` is in the app and resolves relative to `current_exe()`. From an `.appex` executable,
  `../Resources` is the extension's own Resources, not the app's. An extension would need its own resolver
  (four levels up to the app's `Contents/Resources`). Whether the sandbox lets an extension read its host app's
  Resources was not tested; S20 and S21 only proved loading dylibs from the host's `Contents/Frameworks`.
- `Stored`, `ClipChain`, `OcioChain`, `builtins()` and the map readers are in `apps/qcv/src/color_model.rs`. A static
  library for the extension cannot depend on the app crate: these move to a crate (`qcv-color` is the natural place)
  or are duplicated. The same holds for `thumb_color.rs`; `Poster` already is in a crate but has no looks.
- minColor alone needs nothing on disk and no dylib. OCIO adds a dylib of the app's Frameworks and the configs.

## 5. Playing one MXF with sound in a view that is not a winit window

**No winit anywhere below the app.** `crates/qcv-player`, `crates/min-ffmpeg`, `crates/qcv-ring`, `crates/min-present`
and `crates/min-ui` do not name winit (searched). `Transport` takes time as an argument (`tick(now: Instant)`,
`crates/qcv-player/src/transport.rs:748`) and hands back the frame to show; the host owns the loop. The harness paths in
`apps/qcv/src/main.rs:2795-2827` and `3077-3085` drive a `Transport` with a plain loop and `engine.show(&d.frame)`.

**The smallest surface** (all read in `crates/qcv-player/src/transport.rs` and `playback.rs`):

| Need | Call |
|---|---|
| Open | `Transport::open(&Source::from_path(path, None), TransportOptions { .. })` (`transport.rs:219`); opens paused and seeks to frame 0 (`:234-239`) |
| Sound | `AudioPlayer::new()` then `transport.set_audio(Box::new(a))` (`audio/player.rs:79`, `transport.rs:611`); the Transport opens the file's audio itself |
| Each display refresh | `transport.tick(now)` -> `Option<&FrameHandle>`; `engine.show(&h.frame)`; `idle_refresh()` when none (`transport.rs:748`, `848`; `apps/qcv/src/main.rs:955-959`) |
| Play / pause | `play`, `pause`, `toggle_playback` (`transport.rs:380-393`) |
| Seek | `seek_to_frame` (`:405`); the playbar's gesture is `begin_scrub` / `scrub_to_frame` / `end_scrub` (`:527-571`) |
| Position | `current_frame`, `frame_count`, `rate`, `is_playing` (`:312-323`); or the `Playback` trait, which the bar widgets read (`playback.rs:10-31`) |
| Timecode | `Playback::timecode_info()` (`playback.rs:8`, `23`) + `qcv_meta::TimecodeFormatter` |
| Volume, mute | `transport.audio.set_volume`, `set_muted`, `toggle_mute` (`transport.rs:76-81`) |
| End | `Transport::retire()` drops decoders on threads (`:367`); `wait_retired` before the device goes (`:132`) |

**What the host must supply** (`TransportOptions`, `transport.rs:106-115`; QCView's values at
`apps/qcv/src/main.rs:370-381`):

| Field | QCView | Note for the extension |
|---|---|---|
| `hw` | `HwDevice::platform(dev)` = a VideoToolbox context on macOS, independent of the presenter's device (`crates/min-ffmpeg/src/hw.rs:42-48`) | `None` is valid (software). VideoToolbox inside the sandbox was not tried |
| `hw_intra` | Windows only | `None` |
| `threads` | `performance/ffmpegThreads`, 0 = automatic | 0 |
| `scrub` | `Some(ScrubOptions { budget_bytes: scrubCacheMB << 20, .. })`, default 512 MB | `None` is valid (tests use it); the playbar then scrubs through plain seeks. Memory matters in an extension |
| `loop_enabled` | `playback/loopEnabled`, default on | a choice |
| `refresh_ms`, `clock_policy` | the display's | `WallClock` default |
| `uploader` | the engine's (`engine.rs:591`) | optional; for sequences |

**App-level state the Transport does NOT need:** the project, the playlist editor, the settings store, the thumbnails
cache and the waveform engine are all wired in `App::adopt` (`apps/qcv/src/main.rs:875-884`), outside the crate.

**Threads it starts** (read): the video decoder's, the scrub decoder's when asked, an off-thread slot drop
(`transport.rs:125-129`), the frame index's background scan with its own format context
(`crates/min-ffmpeg/src/frame_index.rs:6-7`), the audio decode thread (`audio/decoder.rs:54` or
`multi_decoder.rs:103`), and CoreAudio's render callback. Nothing needs a run loop of its own.

**Audio output.** `default_device` = CoreAudio's default-output AudioUnit, 48 kHz, no device picker
(`crates/qcv-player/src/audio/device.rs:57-60`, `coreaudio.rs:3`, `34-51`). Routing is not a setting: `RoutingMode::Auto`
is the default (`audio/routing.rs:4-5`, `15-19`) and a per-clip choice lives in the project. Inferred, not tested: audio
OUTPUT needs no entitlement in the app sandbox (input does); whether a Quick Look preview extension may hold an output
unit, and what happens to it when the panel closes or moves to the next file, is unproven. `AudioOutputUnitStart` /
`Stop` block about 120 ms each on the owner's Mac (`audio/player.rs:28-30`): the open and close of a preview pay that
unless the unit is started lazily on the first play.

**Disk.** No cache of the player is written to disk: the frame index, the scrub cache, the hover thumbnails
(`thumbnail_cache.rs:1-12`) and the waveform probes (`audio/waveform.rs:1-6`) are in memory. Every `temp_dir` use in
`qcv-player`, `min-ffmpeg` and `min-image` is in tests (searched). `min-paths` reads `HOME` only to classify paths
(`crates/min-paths/src/classify.rs:84-86`).

**What cannot work in the plain sandbox** (inferred): an MXF whose essence or sound is in OTHER files (OP-Atom sets,
external references) plays without what it cannot open; only a self-contained file (OP1a) is whole. `Transport::probe`
and `qcv_meta` re-open the same path, which is allowed.

## 6. The playbar

`min_ui::Scrubber` (`crates/min-ui/src/scrubber.rs`) is one node with no dependency on the player:
- State in: `ScrubState { frames, fps, current, in_point, out_point, playing, scrubbing, cached }`
  (`scrubber.rs:23-28`).
- Commands out: `ScrubCmd::Begin / To(frame) / End` (and a note press), which QCView maps straight to
  `begin_scrub` / `scrub_to_frame` / `end_scrub` (`scrubber.rs:32`; `apps/qcv/src/main.rs:1460`).
- Calls: `layout(rect, scale)` (`:82`), `event(&Event, now, &state, scale)` (`:147`), `paint(scene, fonts, scale,
  &state, &[WaveLayer])` (`:179`), `follow` while playing (`:126`), `wheel` for zoom and pan (`:111`).
- `compact: true` is the line state QCView's compact strip uses (`scrubber.rs:45`;
  `apps/qcv/src/compact_strip.rs:1-4`): a 32 px band with play / pause and the timecode at its left. [proposed] the
  preview bar is that strip's layout without the pin and the exit.
- The band draws the ruler, ticks and labels itself (`scrubber.rs:1-5`); the cache underline comes from `cached`.

**Timecode.** `StatusStrip` builds a `qcv_meta::TimecodeFormatter::new(num, den, drop_frame)` from
`Playback::timecode_info()` and formats the current frame plus the source start (`apps/qcv/src/status_strip.rs:36`,
`54-60`). `status_strip.rs` and `compact_strip.rs` are app files: the extension either moves the small readout into a
crate or formats with `TimecodeFormatter` directly.

**Waveform.** `WaveformProbeEngine::new()` starts one worker; `request_window(path, t0, t1, columns)` then
`sample_columns(...)` give the peaks, `data_generation()` says when to repaint, `set_playback_active` suspends probing
while playing (`crates/qcv-player/src/audio/waveform.rs:40-57`). The scrubber takes them as `WaveLayer { peaks, color }`
(`scrubber.rs:19-20`; `apps/qcv/src/main.rs:1361`). It opens the file a second time, read-only: allowed.

## 7. Thumbnails without a surface

There is no pure-CPU path with the full look. The existing pieces:

| Piece | What it does | GPU |
|---|---|---|
| `VideoThumbLoader::open(path)` + `load_thumbnail(frame, max_size)` (`crates/qcv-player/src/thumbnail_cache.rs:250-313`) | own software session, seek backward, the frame nearest the target, swscale to RGB48 code values with the frame's matrix and range | none |
| `min_image::load_thumbnail(path, layer, max)` + `image_thumb` (`crates/min-image/src/lib.rs:83`; `thumbnail_cache.rs:400-414`) | EXR: full decode, then every Nth pixel, half floats kept | none |
| `ThumbPixels::rgba8()` (`thumbnail_cache.rs:64-71`) | no look: code values as stored; half floats clamped to 0..1 through the sRGB curve | none |
| `Poster::headless()` + `convert(None, &px, &meta)` (`crates/qcv-player/src/poster.rs:16-41`) | `Engine::headless` (`crates/min-present/src/engine.rs:336-341`: a device, no surface), shows the samples with the clip's tags, captures, BT.2390 for HDR, sRGB 8 bit; falls back to `rgba8()` without a GPU | a device, no surface |
| UFB's helper (`apps/ufb/src/thumbs/helper.rs`) | video: the MIDDLE frame, `Poster`, pixel aspect and rotation (`:123-133`); EXR: `min_image::load_thumbnail` then alpha-aware sRGB on the CPU (`:172-186`, `202-222`) | video only |

Which to call [proposed]:
- **MXF thumbnail:** the body of `helper.rs::video_frame` (`:123-133`): `VideoThumbLoader` + `Poster::headless()`. It is
  private to `apps/ufb`; lift it into `qcv-player` beside `Poster`. For an SDR MXF `ThumbPixels::rgba8()` alone gives the
  same picture with no GPU device at all; the device matters only for HDR-tagged files.
- **EXR thumbnail:** `min_image::load_thumbnail` + the CPU conversion of `helper.rs::rgba8` (`:202-222`). This equals
  QCView's bypass for SDR values (point 2) and keeps alpha. No GPU.
- `TimelineThumbnailCache` itself (workers, LRU, hover slot) is not wanted: one file, one picture.

Costs on record: S20 measured the whole round trip in a sandboxed extension at 256 px from 1920 x 1080 sources:
18-57 ms for MXF, 92-105 ms for a float zip EXR (FFmpeg's EXR decoder, not `min-image`), about 240-390 ms for the first
call of a process. S21: `min-image` decode plus upload 5-10 ms for a 1920 x 1080 EXR; a headless device costs what S21's
presenter did, 23-25 ms warm and 1.7 s the first time on a machine. The look stage has a timing test
(`apps/qcv/src/thumb_color.rs:106-110`) and the loader a bench (`thumbnail_cache.rs:428-435`), neither with a number
written down. Not measured: a 4K or larger multi-layer EXR through `min-image` at thumbnail size (it decodes whole).

## 8. Settings

**Store** (`crates/min-settings/src/lib.rs`): one JSON object per app, flat keys `group/name`, values bool / int /
float / string (`lib.rs:1-6`, `18-22`, `134-147`); unknown keys are kept. On macOS the file is `settings.json` in the
per-user Application Support folder, subfolder `QCView` (`lib.rs:65-70`, `75`; `apps/qcv/src/settings.rs:10`, `157-162`).
`QCV_SETTINGS_DIR` redirects it (`lib.rs:4`). `Settings::open` creates and may write; a reader in the extension should
parse the file itself or use a read-only constructor that does not exist yet (`in_memory` exists, `lib.rs:85`).

**Keys that could matter to the extension** (`apps/qcv/src/settings.rs`):

| Key | Default | Use |
|---|---|---|
| `audio/volume`, `audio/muted` (`:15-16`) | 1.0, false | the MXF's sound |
| `audio/syncOffsetMs` (`:18`) | 0 | A/V offset |
| `playback/loopEnabled` (`:59`) | true | loop |
| `display/backgroundMode` (`:45`) | 0 | what shows under an EXR's alpha |
| `ui/language` (`:62`) | "" = the system's | the bar's strings |
| `performance/ffmpegThreads` (`:68`) | 0 | decode threads |
| `performance/hardwareDecodeEnabled` (`:69`) | true | read on Windows only (`apps/qcv/src/main.rs:372`); no effect on macOS |
| `color/enabled` (`:76`) | false | forced false at every launch (`main.rs:2778`): NOT a preference |
| `color/videoEmulation` (`:31`) | false | acts only with colour on |

**There is no colour default in the settings.** The default chain is per project (point 2), the user's presets are in
`color-presets.json` in the same folder (`apps/qcv/src/color_model.rs:156-160`), and audio routing is per clip. So
"start from QCView's own settings" gives the extension volume, mute, loop, background and language, and for colour
only the preset LIST. The agreed default (bypass) needs no settings at all.

**Open question: reaching the file from the sandbox.** The plain sandbox cannot read the user's Application Support
folder. Options seen:
1. An app-group container: needs an `application-groups` entitlement on the app and the extensions, and the app (today
   signed with no entitlements, `packaging/macos/sign-macos.sh:5-6`) writing a copy there. Whether a non-sandboxed
   Developer ID app can use a group container without a provisioning profile was not checked.
2. A read-only temporary exception for that one folder
   (`com.apple.security.temporary-exception.files.home-relative-path.read-only`): no app change; S20 notes that
   notarization with an exception was not seen, and row 607 chose not to use the exception for neighbours.
3. The app writes a copy somewhere both can read. No such place is known without 1 or 2; the extension's own container
   is closed to other programs under the Developer ID signature (S21 finding 5).
4. No settings at all: built-in defaults; the bar's choices kept in the extension's own container (its own
   `UserDefaults`). This also answers row 607's other open
   point (carrying a layer and colour from file to file) without touching QCView's store.

## 9. Packaging

What exists: `packaging/macos/make-app.sh` builds `qcv`, stages `Contents/MacOS`, `Resources/assets/OCIO`,
`Frameworks` (every non-system dylib, `@rpath` ids, Homebrew refused, `:38-57`), `Helpers` (ffmpeg, ffprobe), Sparkle,
the notices, then a leak and minimum-OS check and an ad hoc signature (`:64-70`). `sign-macos.sh` signs inside-out with
the hardened runtime and no entitlements (`:19-25`), notarizes, makes the DMG.

What each would have to gain:

| File | Addition |
|---|---|
| `make-app.sh` | build the extension's Rust static library with `MACOSX_DEPLOYMENT_TARGET=13.0` (as `:11`); compile the Swift shells with `swiftc -parse-as-library -application-extension`, entry `_NSExtensionMain`, rpath `@loader_path/../../../../Frameworks` (the spikes' line, `spikes/s21-preview-view/build.sh:27-29`); write `Contents/PlugIns/<name>.appex/Contents/{MacOS,Info.plist}`; add the appex executables to the dependency walk (`:53-57`) so a library only they link is copied, and to the leak / `minos` loop (`:65-68`), which today names the exe, the helpers and `Frameworks/*.dylib` only; sign the appex ad hoc before the app (`:70`) |
| `make_plist.py` | two more plists: `CFBundlePackageType` `XPC!`, `NSExtension` with `NSExtensionPointIdentifier` `com.apple.quicklook.preview` (and `com.apple.quicklook.thumbnail`), `NSExtensionPrincipalClass`, `QLSupportedContentTypes` = the two types, `QLIsDataBasedPreview` false (`spikes/s21-preview-view/build.sh:23-25`); version taken from `apps/qcv/Cargo.toml` like the app's (`make_plist.py:9`); the same `LSMinimumSystemVersion`. Both types are the system's (`make_plist.py:16`, `22`), so no imported type declaration is needed |
| `sign-macos.sh` | an entitlements file (new, none exists in `packaging/macos/`): `com.apple.security.app-sandbox` and `com.apple.security.files.user-selected.read-only` (`spikes/s21-preview-view/build.sh:31`), plus whatever point 8 rules. Order: Sparkle's parts, the dylibs, the helpers, then each `.appex` WITH `--entitlements`, then the app (`:21-25`); the app must still be signed without `--deep`, which would re-sign the extensions without their entitlements. `codesign --verify --deep --strict` (`:26`) stays |
| `make_notices.py` | its staged-file scan covers `Contents/MacOS`, `Contents/Helpers`, `Contents/Frameworks` (`:75`): add `Contents/PlugIns/*/Contents/MacOS`. The crate list comes from cargo-about over ONE manifest (`:65`): the extension's library is a second manifest whose crates must be in the notices too (mostly the same set). Swift's runtime is the system's on macOS 13 and later (inferred): no entry |

Libraries the extensions load from `Contents/Frameworks` (inferred from the crates they would link): the FFmpeg
dylibs (MXF), `libOpenEXRCore` and `libImath` (S21 copied these two, `build.sh:17`), `libOpenColorIO` only if OCIO is
in, and whatever `min-image` links for the other formats (TurboJPEG) unless the extension's build leaves them out.

Traps on record in the spikes:
- A container belongs to the signature that made it: the same extension identifier signed ad hoc and then with the
  Developer ID hung for ever. One identifier for the release, always the release signature; a walk build gets its own
  identifier (S20 finding 2).
- Finder refuses an ad hoc extension that `qlmanage -p` accepts: `make-app.sh`'s ad hoc bundle cannot be walked in
  Finder; a walk needs the Developer ID signature under a walk identifier (S21 finding 6).
- The released extension's container is closed and the unified log gave nothing back: diagnostics must be on screen
  or otherwise designed (S20 finding 5, S21 finding 5).
- `std::env::temp_dir()` is not the sandbox's temporary folder; use `NSTemporaryDirectory()` (S21 finding 4).
- The first run on a machine pays about 1.7 s of shader compilation per container (S21 finding 2); minColor's pipeline
  adds its own first build (point 4).
- S21's extension was 37 MB unstripped; two extensions each link the Rust library statically. Strip, and consider one
  shared dylib in `Contents/Frameworks` for both.
- Not yet seen: notarization of the bundle with extensions, a clean machine, Sparkle replacing the bundle and the
  system noticing the new extension (S20 "Not seen").

## 10. Open questions and risks beyond the points above

1. **"QCView's default" is bypass** (point 2). If the owner expected a tone-mapped or managed default for EXR, that is
   a new ruling, and it would differ from what QCView shows.
2. **The colour model is in the app crate** (point 4): a move into a crate is a precondition for sharing presets and
   chains with the extension, and touches QCView itself.
3. **A layer without R, G and B does not load** (point 1): the layer picker would list depth or matte layers that then
   fail. QCView has the same behaviour; for a preview it may want a single-channel fallback (a divergence to rule).
4. **Layer inside a non-zero part** (point 1): `layers()` lists parts for a multi-part file and prefixes for a
   single-part one, never both.
5. **Memory.** A full-size EXR is decoded whole per layer change and per thumbnail; `ScrubOptions` defaults to a
   512 MB cache; `MinColorPass`, the presenter and FFmpeg all live in a process the system may limit (S21 "Not seen").
6. **Audio in a preview extension** (point 5): unproven; also whether sound should stop when the panel loses the file,
   and the 120 ms start / stop cost.
7. **VideoToolbox in the sandbox** was not tried; software decode is the safe default and S20's numbers are software.
8. **MXF with external essence** (point 5) shows a partial file with no message unless one is designed.
9. **Strings.** The bar's labels reuse `ColorPanel` / `TransportBar` contexts, but the catalogs are embedded by the app
   (`apps/qcv/src/i18n.rs:8`, `22`), and the language comes from the unreadable settings or the system
   (`i18n.rs:31-45`). The extension needs the catalogs compiled in too, and new strings need the 11 drafts.
10. **Keys and Finder's arrows** are still unanswered (S21 "Not seen"): play / pause on space collides with the
    panel's own use of space.
11. **Two engines per preview** if thumbnails inside the preview were ever wanted; not needed for the decided scope.
12. **Windows HDR white level** (`Presenter::follow_screen`) is not relevant on macOS; on macOS `Presenter::new` takes
    the hosted `NSView` for its layer report (`crates/min-present/src/presenter.rs:85-86`), as S21 passed it.
