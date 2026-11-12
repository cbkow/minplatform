# minNotes video blocks — what the Qt app did (Phase 3, video step)

Source read: the minNotes repository at commit `59d1129` (2026-11-10, branch `min-paths`, the tree as checked out),
read only. Nothing in it was built or run; every statement is read from code or from the app's own manual, and anything
deduced rather than read is marked **(inference)**. Line numbers are those of `59d1129`.

This document follows `step-media-spec.md` (the sibling for pictures and PDFs) and supersedes its §9. What that
document already states — the four `src` forms, `toRef` / `resolveRef`, the copy policy, the routing order, the width
and height formulas, Collect Media — is referred to, not repeated.

References are `alias:line` relative to the minNotes repository.

| Alias | File |
|---|---|
| `MS`, `MSh` | `app/core/MediaStore.cpp`, `.h` |
| `BM`, `BMh` | `app/core/BlockModel.cpp`, `.h` |
| `VFP` | `app/core/VideoFrameProvider.h` |
| `Ex` | `app/core/Exporter.cpp` |
| `CEx` | `app/core/CanvasExporter.cpp` |
| `PE`, `PEh` | `app/core/PackageExporter.cpp`, `.h` |
| `SE` | `app/core/SearchEngine.cpp` |
| `Ed` | `app/qml/MinNotes/App/Editor.qml` |
| `MB` | `app/qml/MinNotes/App/MediaBlock.qml` |
| `BV` | `app/qml/MinNotes/App/BlockView.qml` |
| `CT` | `app/qml/MinNotes/App/CompactTransport.qml` |
| `VT` | `app/qml/MinNotes/App/VideoTransport.qml` |
| `LB` | `app/qml/MinNotes/App/Lightbox.qml` |
| `Main` | `app/qml/MinNotes/App/Main.qml` |
| `Insp` | `app/qml/MinNotes/App/Inspector.qml` |
| `SB` | `app/qml/MinNotes/App/SearchBar.qml` |
| `CMI`, `CSF` | `app/qml/MinNotes/App/CanvasMediaItem.qml`, `CanvasSelectionFrame.qml` |
| `AIO`, `AIOh` | `app/notes/annotation_io.cpp`, `.h` |
| `AN`, `ANh` | `app/notes/annotation_note.cpp`, `.h` |
| `AS`, `ASh` | `app/notes/annotation_serializer.cpp`, `.h` |
| `Stk` | `app/notes/active_stroke.h` |
| `Thumb`, `Thumbh` | `app/notes/annotation_thumbnail.cpp`, `.h` |
| `VPA`, `VPAh` | `app/notes/viewport_annotator.cpp`, `.h` |
| `ISM` | `app/notes/ink_stroke_modeler_wrapper.h` |
| `VNM`, `VNMh` | `app/notes/VideoNotesModel.cpp`, `.h` |
| `VA`, `VAh` | `app/notes/VideoAnnotator.cpp`, `.h` |
| `MH` | `app/notes/MediaHost.h` |
| `VD`, `VDh` | `app/player/video_decoder.cpp`, `.h` |
| `VS`, `VSh` | `app/player/VideoSurfaceItem.cpp`, `.h` |
| `FH` | `app/player/frame_handle.h` |
| `FI` | `app/player/frame_index.h` |
| `SD`, `SDh` | `app/player/scrub_decoder.cpp`, `.h` |
| `SFC` | `app/player/scrub_frame_cache.h` |
| `RA`, `RAh` | `app/player/read_ahead.cpp`, `.h` |
| `Sws` | `app/player/sws_threaded.h` |
| `ZC` | `app/player/zero_copy_bridge_metal.mm` |
| `TP` | `app/player/thread_policy.h` |
| `TC`, `TCh` | `app/player/timecode_formatter.cpp`, `.h` |
| `AP`, `APh` | `app/player/audio_player.cpp`, `.h` |
| `Servo` | `app/player/audio_sync_servo.h` |
| `Drain` | `app/player/audio_drain_stage.h` |
| `Ring` | `app/player/audio_ring_buffer.h` |
| `Tempo` | `app/player/tempo_stage.h` |
| `SAE`, `SAEh` | `app/player/shuttle_audio_engine.cpp`, `.h` |
| `Man`, `ManA`, `ManC` | `docs/media.md`, `docs/annotations.md`, `docs/canvas.md` |

New-side references are relative to the minplatform repository.

Scope: the video block inside a **page**. The canvas places the same descriptor as an element and drives the same
player; it is quoted where it shows the same rule with a different constant.

Words used below: the **strip** = the 22 px transport under an inline video; the **full bar** = the 40 px transport of
the studio and the lightbox; the **studio** = the full-window review view of one video; the **lightbox** = the
Quick-Look-style overlay; the **sidecar** = the `.qcview/<media>/` folder beside a video file.

---

## 0. The shape everything below rests on

| Item | Rule | Ref |
|---|---|---|
| One player per window | The editor owns one `VideoDecoder` and one `AudioPlayer`. Every place a video shows (a page block, the studio, the lightbox, a canvas element) binds a surface to that one decoder | `Ed:1003-1009`, `Ed:1393-1394`, `Ed:1408-1409` |
| Why one | Stated in the code: a block's delegate is pooled and recycles mid-scroll, so it cannot own a live decoder; "a single shared decoder gives one video at a time for free" | `Ed:1003-1009` |
| The player is keyed by source | The active video is identified by its **anchor path** (`MediaStore::anchorPathFor`, no IO), not by a block. A page row is carried beside it only to place the inline surface (−1 for a canvas element) | `Ed:1010-1011`, `Ed:1162-1165`, `MS:630-635` |
| The poster is a different decode path | An idle block shows a still decoded by `MediaStore::extractFrame` through an image provider. It shares no code and no cache with the player | `MS:441-490`, `VFP:10-27` |
| The file is never copied and never written | A video is referenced in place; nothing writes to it | `MS:280`, `MSh:22-25` |
| Notes live beside the video, not in the document | Annotations are a folder next to the media file in a sibling app's exact format. They are outside the document database and outside its undo | `VNMh:19-20`, `AIOh:1-5`, `Ed:1418-1424` |
| Nothing about playback is stored in the document | Playhead, loop, speed, volume and mute are session state on the editor or the audio object. One preference (timecode vs frames) is persisted, in application settings | `Ed:1088-1092`, `Ed:1036`, `Ed:1051`, `Ed:51-63` |
| New-side state | `Kind::Media { descriptor }` carries the JSON untouched (see the sibling spec §0). A notes crate (`crates/qcv-notes`) and a player crate (`crates/qcv-player`) already exist in minplatform | minplatform `crates/qcv-notes/src/lib.rs:1-5` |

---

## 1. The descriptor as stored

### 1.1 Keys

Written by `MediaStore::videoDescriptorJson` (`MS:41-51`), read back key by key from the block's `content`
(`BM:4773-4794`).

| Key | JSON type | Meaning | How probed | Ref |
|---|---|---|---|---|
| `src` | string or `{vol, rel}` object | Where the file is. The probe stores the absolute path; `toRef` turns it into a mapped reference when a path mapping covers it (sibling spec §2) | — | `MS:43`, `MS:280` |
| `w` | integer | **Coded** width of the best video stream, in pixels | `codecpar->width` | `MS:281` |
| `h` | integer | Coded height | `codecpar->height` | `MS:282` |
| `kind` | string | Always `"video"` | — | `MS:46` |
| `durMs` | number (integer-valued) | Duration in milliseconds | The video stream's `duration × time_base × 1000`, truncated; when the stream has none, the container's `duration` (microseconds ÷ 1000); 0 when neither is known | `MS:286-289` |
| `frames` | integer | Frame count | The stream's `nb_frames` when > 0; else `fps × durMs / 1000` rounded half up; 0 when neither | `MS:290-293` |
| `fps` | number | Frames per second as a double | `avg_frame_rate` when its numerator ≠ 0, else `r_frame_rate`; `num / den`; 0.0 when the denominator is 0 | `MS:283-285` |

Probe facts:

| Item | Rule | Ref |
|---|---|---|
| Library | libavformat: `avformat_open_input` + `avformat_find_stream_info`. No codec is opened, no frame decoded, no thread started | `MS:269-276`, `MSh:66-68` |
| Which stream | `av_find_best_stream(AVMEDIA_TYPE_VIDEO)` | `MS:277` |
| Thread | The caller's — the GUI thread, inside the drop or paste handler | `BM:4386-4391` |
| Valid | `src` non-empty and `w`, `h` > 0. A file with no video stream, or one FFmpeg cannot open, is not a video | `MSh:26-28`, `MS:273-274` |
| Not probed, not stored | Rotation, sample aspect ratio, codec, pixel format, colour tags, audio presence, start timecode, drop-frame flag, the file's size and mtime | `MS:263-298` |
| Other keys honoured | `dw` (display width) is read for any media kind but no command writes it for a video (§2.2). `ink` is never written for a video: its drawings are in the sidecar (§6) | `BM:2680`, `Ed:831-834` |
| Re-probe | None. The descriptor is written once at insert; no code refreshes it if the file changes | (**not found**) |

### 1.2 Example (synthetic)

Reading order, a 200-frame clip at 24000/1001 on a mapped share:

```json
{"src":{"vol":"01JB2M7Q4T8XK3N5R9W6YHZC0D","rel":["renders","shot-010-v003.mov"]},"w":1920,"h":1080,"kind":"video","durMs":8341,"frames":200,"fps":23.976023976023978}
```

Stored order is alphabetical (`durMs`, `fps`, `frames`, `h`, `kind`, `src`, `w`), because the object is written by
`QJsonDocument::toJson(Compact)` (sibling spec §1.4). `fps` is the full double, not a rounded rate.

### 1.3 Extensions and routing order

| Item | Rule | Ref |
|---|---|---|
| Video extensions | `mp4 mov m4v mkv webm avi mxf m2ts mts ts wmv flv mpg mpeg 3gp`, compared lower-case | `MS:104-115` |
| Order on a page | 1 video (extension **and** a successful probe) → 2 PDF → 3 preview → 4 image → 5 importable document → 6 file chip. A video extension whose probe fails falls through to the later steps | `BM:4449-4475` |
| Order on the canvas | video → pdf → audio → preview → image → another minNotes file → importable → file card | sibling spec §7.3 |
| Paste | A pasted file URL takes the same route; `forceCopy` never applies to a video | `BMh:788-791` |
| Copy policy | Always referenced in place. Collect Media and the package exporter copy a video only on request (§9.2) | `MS:280`, `MSh:22-25` |

The sidecar's own header uses a **different, shorter** list to label `media_type` (§6.3): `mov mp4 m4v mxf mkv avi
webm` are `"video"`, and anything unrecognised is also `"video"` (`AIO:40-68`).

---

## 2. The idle block

### 2.1 What it shows

| Part | Rule | Ref |
|---|---|---|
| Poster | One decoded frame of the file: frame **0** at rest, or the **remembered playhead** once the clip has been played, scrubbed or stepped in this session (§3.9) | `MB:3-6`, `BV:475-476` |
| Play affordance on the picture | None. Stated twice in the code: "there is NO play affordance on the frame; the per-video toolbar is the sole transport" | `MB:5-6`, `Ed:6397` |
| Duration badge | Bottom-right of the poster, inset 8 px; text `m:ss` (minutes not padded, seconds rounded to the nearest); black at 60 % behind bright small text; shown when `durMs` > 0 | `MB:74-79`, `MB:255-266` |
| Backdrop | A rounded rectangle in the **page's own colour**, so an alpha clip's transparent areas show the page — the same as the live surface's fill | `MB:231-239` |
| Reveal | Opacity 0 → 1 over 200 ms, ease-out cubic, whenever the poster becomes visible; hide is instant | `MB:240-242` |
| Under it | The strip (§3.2), always present, in real layout space | `Ed:6474-6511`, `BM:66` |
| Notes | Markers on the strip's scrub line for the clip's notes (§6.9), read from the sidecar before the clip is ever activated | `CT:75-84`, `CT:127-131` |

### 2.2 Size

| Item | Rule | Ref |
|---|---|---|
| Width | `dw` if present, else `min(laneW, w)`; never upscaled by default (sibling spec §5.2) | `BM:2693-2698` |
| Frame height | `floor(width × h / w + 0.5)` | `BM:3380-3386` |
| Block height | `6 + frame + 6 + 22` (the strip, `kVideoBar`) | `BM:66`, `BM:3391-3395`, `BV:216-223` |
| Aspect | From the **coded** `w` / `h`. Rotation and anamorphic pixels are not in the descriptor, so the block's shape ignores them (§5.5) | `MS:281-282` |
| Resize | None. The corner handles are for images only; no menu command sets a video's width | `Ed:831-834`, `Ed:8336-8340` |
| Placement | Left edge of its lane; no alignment | sibling spec §5.5 |
| Narrow lane | When the displayed width is under **300** px the strip is hidden; when the block is in a lane narrower than 300, a click on it opens the studio instead | `Ed:1028-1030`, `Ed:6489-6490`, `Ed:6646-6650` |

### 2.3 The poster decode

| Item | Rule | Ref |
|---|---|---|
| URL | `image://videoframe/<base64url(UTF-8 path)>@<frame>` | `MB:84-98`, `VFP:16-21` |
| Provider | `VideoFrameProvider`, flagged force-asynchronous: the decode runs on Qt's image thread, not the GUI thread | `VFP:12-14` |
| Requested width | `round(displayed width × device pixel ratio)`; 760 when none is given | `MB:252`, `VFP:22` |
| Source | `mediaPlaybackSource(row)`: a local path, or a `subfile` byte-range spec when the clip is still inside a package archive (§5.7) | `MB:46-48`, `MS:603-628` |
| Decode | Opens the container and the codec afresh for each request, software only, no thread or hardware settings. Frame 0: the first decoded frame. Frame N: target = stream start + `N / fps` in stream ticks; `av_seek_frame(BACKWARD)` to the keyframe before; decode forward until a frame's best-effort timestamp ≥ target; at end of file, the last drained frame | `MS:441-490` |
| Scale | Bilinear down to the requested width, never up; both dimensions forced even, minimum 2 | `MS:399-404` |
| Colour | 8-bit RGBA. Matrix from the frame's tag (709, 470BG, 170M, 240M, FCC, 2020); untagged → 709 when width ≥ 1280 or height ≥ 720, else 170M. Source range from the tag (full only when tagged full). Output: 709 coefficients, full range | `MS:410-431` |
| Alpha | Kept (the target is RGBA) | `MS:405-406`, `MB:231-234` |
| Cache | Qt's pixmap cache, keyed by the URL — so by path + frame + requested size. No file on disk, no app-owned budget | `MB:247-250`, sibling spec §6.2 |
| While this block is the live player | The poster source is cleared (no decode) and the surface covers the block | `MB:10-12`, `MB:249`, `BV:478-481` |

### 2.4 Missing or unreadable file

| Case | What the user sees | Ref |
|---|---|---|
| The reference does not resolve, or the file is gone, or the frame cannot be decoded | The backdrop rectangle only. It is the page's colour, so the block reads as an **empty gap** with the duration badge at its lower right and the strip beneath it. There is no "unavailable" text for a video — that text exists only on the image branch | `MB:217-228` (image only), `MB:231-267` |
| The strip | Still drawn from the descriptor: counter, scrub line at the remembered playhead, note markers if the sidecar folder is reachable | `CT:17`, `CT:27` |
| Pressing play, scrubbing, or a transport key | `VideoDecoder::open` fails; the editor clears the active video and returns. No message, no toast | `Ed:1177`, `VD:297-306` |
| Opening the studio | The studio opens on the descriptor: an empty stage (the poster fails the same way), the full bar, and the notes panel with whatever the sidecar holds | `Ed:5016-5018`, `Ed:7276-7287` |
| The lightbox | Shows the word `loading…` and stays there: its text is tied to "the surface is not ready", which never changes for a clip that fails to open | `LB:164-169` |
| A package's clip still extracting | The poster source is empty until the extraction lands and the block re-resolves | `MB:39-49`, `MS:625-627` |

No relink command exists (sibling spec §2.4).

---

## 3. Playback inside the block

### 3.1 What starts it

| Gesture | Result | Ref |
|---|---|---|
| Click on the picture | Parks the caret on the block (it becomes the focused block). **Does not play.** | `Ed:6397`, `MB:5-6` |
| Hover | Nothing. There is no hover skim | (**not found**) |
| Strip: play button | Parks the caret on the block, activates the clip, plays. On the live clip it toggles play / pause | `CT:24-25`, `Ed:1207-1213` |
| Strip: press or drag on the scrub line | Parks the caret, activates the clip **paused**, shows the frame under the pointer (§3.4) | `CT:97-116` |
| Strip: expand glyph | Opens the studio | `CT:26`, `CT:118-125` |
| `K`, `W` or `S` with the caret parked on the block | Activates and toggles play / pause (§4) | `Ed:1289` |
| `Space` with the caret parked on the block | Opens the lightbox, which starts playing unless the clip is already the active one (§8.1) | `Ed:3476-3478`, `LB:74` |
| Block menu → **Open in studio** | The studio, paused at the remembered playhead | `Ed:8330`, `Ed:441-460` |
| Click in a lane narrower than 300 px | The studio | `Ed:6646-6650` |
| A search hit on a note's text | Sets the remembered playhead to the note's frame, then opens the studio | `SB:57-67`, `SE:161-184` |
| Double-click on the picture | Nothing video-specific: the handler reveals a file chip, otherwise runs word selection | `Ed:6813-6831` |
| Return on the focused block | A fresh paragraph below it (the rule for every opaque block), not "open" | `Ed:1791` |

"Activate" (`_activateSource`, `Ed:1164-1193`) is: bank the outgoing clip's playhead; hide the surface; end any
shuttle; close decoder and audio; open the decoder on the new source; open the audio; re-apply the review speed if it
is not 1; seek to the remembered playhead if it is > 0. The decoder **opens paused** (`VD:344-350`) — activation never
plays by itself. Activating the clip that is already active is a no-op.

### 3.2 The strip (`CompactTransport`, 22 px)

Left to right; the strip is as wide as the picture and sits directly under it (`Ed:6500-6503`). It is raised-chrome
colour with a 1 px line on its top edge (`CT:18-20`). It never fades (`Ed:6487-6488`).

| # | Control | Size / place | Behaviour | Ref |
|---|---|---|---|---|
| 1 | Play / pause | 22 × 22 button, 13 px icon, 2 px from the left | Icon is `pause` only when this clip is live and playing. **On a clip that is not live the icon is transparent** (the button is there and clickable, but draws nothing). Tooltip "Play (Space)" / "Pause (Space)" | `CT:37-45` |
| 2 | Counter | Mono small text, right-aligned, width reserved for the clip's widest value (the last frame) so it never reflows | Timecode `HH:MM:SS:FF` by default, or a bare frame number. Shows the live frame, or the remembered playhead when not live. A click flips **every** player's counter between timecode and frames. Brightens on hover; tooltip names the other mode | `CT:46-64` |
| 3 | Scrub line | Fills the remaining width; 22 px tall hit area | A 2 px track (lighter on hover / press), a 2 px played part, a 3 × 12 playhead (bright only when live), and a 2 × 10 tick per note (violet `#b48ef0`; grey when the note is marked addressed). Press and drag scrub (§3.4). Pointing-hand cursor | `CT:65-117` |
| 4 | Expand | 22 × 22 button, `arrows-out-simple` icon, 4 px from the right | Opens the studio. Tooltip "Open in the studio" | `CT:118-125` |

Row spacing is 6 px. Not on the strip: frame-step buttons, rewind / fast-forward, loop, speed, mute, volume, the
notes-visibility switch. Those are keys (§4) or the full bar (§8.3). The canvas uses the same component in a
`minimal` form: play and the scrub line only (`CT:16`, `CSF:44-53`).

Frame ↔ x on the scrub line: `frame = round(clamp(x / width, 0, 1) × (frames − 1))`; the playhead and ticks sit at
`width × frame / (frames − 1)`, clamped inside the track (`CT:70-71`, `CT:79`, `CT:86`).

Every strip is built when the document loads, not as rows scroll in, and is only repositioned and hidden after that;
the stated reason is that creating strips during scroll flickered and reset the scrubber (`Ed:5899-5906`,
`Ed:6474-6478`). A strip is shown only while its row is in view (`Ed:6489-6491`).

### 3.3 The live surface

| Item | Rule | Ref |
|---|---|---|
| Where | One item in the page layer, in content units: it scales with the page zoom, is clipped by the scroll area and scrolls with its row. Positioned at the playing row's column, `y = row top + 6`, sized as the picture | `Ed:6368-6388` |
| When shown | The playing row is on a page view (no full-frame tab open), is within the visible row range, the decoder has published its **first frame of this clip**, and the lightbox is not up | `Ed:1073-1075`, `Ed:6377`, `Ed:1067-1070`, `Ed:1469-1471` |
| Before the first frame | The poster stays; the surface is hidden so it cannot flash the previous clip's last frame | `Ed:1067-1070`, `BV:478-481` |
| Letterbox colour | The page's colour | `Ed:6394-6395` |
| Clicks | The surface takes none; the read-only stroke layer over it is disabled so it is mouse-transparent | `Ed:6397`, `Ed:6431-6439` |
| Over it | The current frame's drawings and a note caption (§6.9) | `Ed:6399-6471` |

### 3.4 Scrubbing

| Item | Rule | Ref |
|---|---|---|
| Press | Park the caret, activate, remember whether it was playing, pause video and audio, scrub to the frame under the pointer, start scrub audio | `CT:97-104` |
| Drag | Each move: scrub to the frame, feed the scrub audio | `CT:105-110` |
| Release | End scrub audio; if it was playing, reposition the streaming decoder and audio to the scrubbed frame and resume | `CT:111-115` |
| Two decoders | A scrub goes to the **scrub decoder** (`scrubToFrame`): its own FFmpeg context, latest request wins, frames served from a decoded-GOP cache. The streaming decoder stays where it was until resume. The editor therefore keeps a "scrub target" frame; "the frame on screen" is the scrub target when set, else the decoder's current frame | `Ed:1138-1157`, `VDh:110-115`, `SDh:1-22` |
| Clamp | `0 … frameCount − 1` | `Ed:1143` |
| Audio seek during a scrub | Skipped while a shuttle gesture owns the audio ("the old seek storm"); the commit seek happens at release | `Ed:1146-1150` |

### 3.5 Scrub audio and shuttle audio (skim with sound)

Both gestures play sound through the shuttle engine (`SAEh:1-37`): a grain thread decodes short chunks forward from a
synchronous reader and feeds a small ring the device callback drains.

| Item | Rule | Ref |
|---|---|---|
| Drag scrub | The speed is **estimated from the drag**: signed source-seconds per wall second, clamped to ±32, smoothed by an exponential average with time constant **40 ms**. Sample gaps under 0.5 ms are ignored | `Ed:1356-1386` |
| Held rewind / fast-forward | A 33 ms timer integrates a position at a speed that starts at **2×** and doubles every second up to **32×** (`min(32, 2 · 2^elapsed)`), scrubs the picture to it each tick, and gives the engine the position and signed speed | `Ed:1329-1350`, `Ed:1437-1455` |
| Pitch | Below 1× pitch follows speed (tape sound). At and above 1× grains play at natural pitch while the cursor chases the target | `SAEh:13-18`, `SAE:36` |
| Grain | 80 ms of output per grain (3840 frames at 48 kHz), 5 ms edge fades; reverse grains are individually reversed | `SAE:15-17`, `SAEh:20-26` |
| Stationary pointer | Silence: below speed 0.05 for 120 ms is a hold | `SAE:42-43` |
| Re-sync | The grain cursor snaps to the target only when they diverge by more than `max(0.25 s, 0.25 × |speed|)`; a grain that cannot decode in time is skipped (silence), never late | `SAEh:28-33` |
| No audio stream | The engine is not started; the picture scrubs silently | `Ed:1346`, `Ed:1369` |
| Exclusive with review speed | Starting either gesture snaps the review speed back to 1× | `Ed:1339-1341`, `Ed:1370` |

### 3.6 Frame stepping, start and end

| Action | Rule | Ref |
|---|---|---|
| Step ±1 frame | Pause video and audio, then scrub to (frame on screen ± 1). Stepping implies review: it always pauses | `Ed:1233-1234` |
| Hold to step | The step keys auto-repeat at the OS key-repeat rate | `Ed:1284-1285` |
| Start | Pause, scrub to frame 0 | `Ed:1235` |
| End | Pause, scrub to the last frame | `Ed:1236` |
| Jump to a frame (a note card, a marker) | Pause, scrub to it | `Ed:1237-1238`, `VT:228-232` |

### 3.7 Rate, loop, in / out

| Item | Rule | Ref |
|---|---|---|
| Review speeds | `0.5, 0.75, 1, 1.25, 1.5, 2`. Cycle = the step after the nearest one, wrapping. Reset = 1 | `Ed:1046-1066` |
| Applied to | The video pacing divisor and the audio tempo stage (constant pitch); the audio is then re-anchored by a seek so old-tempo audio is dropped | `Ed:1052-1057`, `Tempo:1-22` |
| Scope | One value for the editor; it persists across clips in the session and is re-applied on activation. Not stored | `Ed:1051`, `Ed:1183-1186` |
| Readout | Only on the full bar, in the accent colour, only when ≠ 1× (e.g. `1.25×`); a click cycles | `VT:127-140` |
| Reverse playback | None. Reverse exists only as the held-rewind shuttle | (**not found**) |
| Loop | One switch for the editor, default off, session only. At end of stream: seek to 0 and play. The **whole clip** loops | `Ed:1036`, `Ed:1327`, `Ed:1456-1467` |
| End of stream without loop | Audio pauses; the decoder rests in its end state on the last frame. Play from there restarts at frame 0 | `Ed:1464-1466`, `Ed:1219` |
| In / out points, a loop range | **Not found** in the editor. The decoder has a read-ahead range call for a loop range; nothing calls it | `VDh:152` |

### 3.8 Audio

| Item | Rule | Ref |
|---|---|---|
| Volume | Linear gain 0…1, default 1. Not persisted **(inference: no settings write was found)**; it lives on the audio object, which is shared across clips | `APh:86-91`, `APh:206-207` |
| Mute | A flag beside the volume; toggled only when the clip has audio | `Ed:1306` |
| Controls | Mute button and a 64 px volume slider on the full bar, live only when this clip is live and has audio; moving the slider above 0 unmutes. Keys: `M`; `↑` / `↓` ±0.05 in the studio only (§4) | `VT:159-177`, `Ed:1296-1297` |
| Device | The system default output: macOS a default-output audio unit, Windows WASAPI shared mode, both asked for 48 kHz float32 stereo with a 10 ms buffer. No device picker | `AP:94-107`, `app/player/coreaudio_device.h:1-9`, `app/player/wasapi_audio_device.h:9-21` |
| Several audio streams | A file with more than one audio stream is mixed to stereo through a filter graph; routing modes exist (auto, 5.1 downmix, channels 7-8) but the editor has no control for them | `app/player/multi_stream_audio_decoder.h:1-25`, `app/player/audio_routing.h` |
| Audio-only elements | A canvas audio element plays on the same audio object: starting a video stops it and the reverse | `Ed:1012-1027` |

### 3.9 Playhead memory

| Item | Rule | Ref |
|---|---|---|
| What | A map `anchor path → frame number` on the editor, plus a revision counter that makes posters and strips re-read it | `Ed:1088-1096` |
| Written | When the active clip is torn down or replaced: the frame on screen (the scrub target if mid-scrub). Also by a search hit, directly | `Ed:1097-1102`, `Ed:1166`, `Ed:1226`, `SB:57` |
| Read | By the poster (which frame to show), the strip (counter and playhead when not live), the studio stage, and activation (where to resume) | `BV:475-476`, `CT:23-27`, `Ed:1187-1191`, `Ed:7276-7282` |
| Lifetime | The session. It is a plain in-memory object: not in the document, not in settings. It is not cleared on a note switch, so the same file keeps its playhead across documents in one run **(inference from `Ed:534`, `Ed:1091`)** | `Ed:1091` |
| Shared | By every block and canvas element that points at the same file | `Ed:1093-1096`, `CMI:53` |
| Unit | A frame number, in the decoder's frame index | `Ed:1141` |

### 3.10 One at a time, and teardown

| Event | Result | Ref |
|---|---|---|
| A second video is played | The first is closed (its playhead banked), the second opened. Two videos never play together; nothing in the model allows it | `Ed:1164-1193` |
| The playing block scrolls out of the visible row range | The player is **torn down** (decoder and audio closed, playhead banked) — also when it was only paused. The check is deferred one event-loop turn and re-tested, the stated reason being a binding loop | `Ed:1073-1086` |
| It scrolls back | The poster shows the banked frame; playback does not resume by itself | `BV:475-476` |
| A table, PDF or canvas view opens over the page | Torn down, by the same rule | `Ed:1074`, `Ed:1083-1086` |
| The studio opens on the same clip | Not torn down: the studio takes over the same decoder | `Ed:1076-1077`, `Ed:1085` |
| The studio closes | The player stays if the block is visible on the page; it is torn down if the block is off-screen | `Ed:450-458` |
| Switching to another document (tab) | Torn down first, then the strips are rebuilt for the new document | `Ed:5964-5981` |
| Switching page inside the file | The row is re-found by source after the model changes; if no row has that source the player is torn down **(inference from `Ed:5936-5962`)** | `Ed:5943-5962` |
| Blocks inserted or deleted above | The playing row is re-pointed to the nearest row with the same playback source; if none, torn down | `Ed:5950-5962` |
| The playing block is deleted | Torn down | `Ed:5961-5962` |
| The editor is destroyed | Torn down | `Ed:36` |
| The lightbox closes | Paused if playing; the player stays active | `LB:79-84` |
| A canvas video element is deselected | Torn down | `Ed:1400-1405` |

---

## 4. Keyboard

### 4.1 How focus is decided

There is one key router for three contexts (`Ed:1239-1276`):

| Context | Condition | Ref |
|---|---|---|
| `studio` | A video's full-frame view is open | `Ed:1268`, `Ed:478` |
| `canvas` | The canvas is showing and its selection is one video element | `Ed:1269`, `Ed:1400-1403` |
| `page` | The caret is parked on a video block, there is no selection, and no full-frame view is open | `Ed:1246-1248`, `Ed:1270` |
| none | Anything else: the keys type as usual | `Ed:1279` |

A video block is opaque: a click on it, or on its strip, parks the caret on it (`Ed:1119-1136`, `CT:24`). The focused
block is drawn by the row's focus fill, not by any frame on the picture (sibling spec §5.6). Each transport key first
**activates** the context's clip (paused, at its remembered playhead), then acts — so a key works on a clip that was
never played.

Key presses are routed before the document's own handling and before the canvas's single-letter tool keys, which
yield while a video is selected (`Ed:3339-3340`, `Ed:3438-3449`).

### 4.2 Keys

Modifiers: "plain" = no modifier (the keypad flag is ignored). `Ed:1277-1305`.

| Key | Action | Auto-repeat | Contexts |
|---|---|---|---|
| `K`, `W`, `S` | Play / pause | ignored | all |
| `Space` | Play / pause | ignored | **studio only** |
| `Space` | Open / close the lightbox | ignored | page, canvas (a tap) |
| `←`, `Q` | Step one frame back (pauses) | repeats | all |
| `→`, `E` | Step one frame forward (pauses) | repeats | all |
| `J`, `A` held | Rewind shuttle, 2× → 32× with sound; ends on key release | swallowed | all |
| `L`, `D` held | Fast-forward shuttle, the same | swallowed | all |
| `Home` | Go to the first frame (pauses) | ignored | all |
| `End` | Go to the last frame (pauses) | ignored | all |
| `M` | Mute / unmute (only when the clip has audio) | ignored | all |
| `R` | Cycle the review speed | ignored | all |
| `Shift+R` | Review speed back to 1× | ignored | all |
| `V` | Toggle loop (does not activate the clip) | ignored | all |
| `↑` | Volume +0.05; unmutes when the result is > 0 | ignored | studio only |
| `↓` | Volume −0.05 | ignored | studio only |

Not `J K L` in the three-speed editorial sense: `J` and `L` are hold-to-shuttle, `K` is a toggle. In a page `↑` / `↓`
keep walking blocks.

Studio-only keys outside the router: `Cmd/Ctrl+Z`, `Cmd/Ctrl+Shift+Z`, `Cmd/Ctrl+Y` go to the **annotation** undo
(§6.8); `Delete` / `Backspace` delete the selected drawings; `Cmd/Ctrl+C / V / X` are swallowed; `Esc` cancels an
in-flight stroke, else disarms the tool, else clears the drawing selection; every other key is swallowed so typing
cannot edit the hidden page (`Ed:3285-3290`, `Ed:3341-3347`, `Ed:3384-3389`, `Ed:3425-3431`).

The manual lists `Space` as play / pause without the studio-only limit, and names `Space` for the canvas too
(`ManA:54-63`, `ManC:110-111`); the code moved play / pause off `Space` outside the studio on 2026-10-22
(`Ed:1287-1289`). The strip's tooltip also still says "Play (Space)" (`CT:43`).

---

## 5. The player as embedded

### 5.1 Lineage

| Part | Came from | Evidence |
|---|---|---|
| `app/player/` video side (decoder, scrub decoder, frame index, frame handles, the surface item, the Metal bridge, the Windows Vulkan / D3D11 bridges) | A sister file-browser app's copy (`namespace ufbplayer`, resource prefix `:/ufb/player/shaders`), itself taken from QCView | `CMakeLists.txt:130`, `app/CMakeLists.txt:310-327`, `VDh:44`, `app/player/wasapi_audio_device.h:23-24` |
| Audio engine (player, decoders, ring, drain stage, sync servo, tempo stage, shuttle engine, chunk reader, resampler) | QCView `src/audio/`, namespace renamed | `app/CMakeLists.txt:158-170`, `app/player/audio_routing.h:1-4` |
| Read-ahead | QCView `src/decode/read_ahead.h`, "verbatim but for the namespace and the env aliases" | `RAh:1-2` |
| swscale helpers, thread policy, codec routing | QCView `src/decode/` (the FFmpeg 9 cut-over helpers) | `app/CMakeLists.txt:138`, `app/player/sws_rgba_image.h` header |
| Timecode formatter | QCView | `TCh:1-10` |
| `app/notes/annotation_note`, `annotation_io` | QCView `src/annotations/`, "VERBATIM port", dated 2026-10-13 | `AIOh:1-5`, `ANh:1-5` |
| `annotation_serializer`, `active_stroke`, `viewport_annotator`, `ink_stroke_modeler_wrapper`, `annotation_thumbnail` | QCView `src/annotations/`, "direct lift"; the thumbnail painter is made public with a line-width scale | `ASh:1-2`, `Stk:1-2`, `ISM:1-2`, `Thumbh:16-20` |
| Not taken | QCView's GPU stroke renderers and tessellator (a painted item draws strokes instead), its compositor and OCIO passes, its 16-bit surface path, image sequences, the dual (A/B) decoders, live-feed sources, its exporters | `VAh:1-6`, `VSh:9`, `app/player/frame_handle.h` (unused factories) |
| minNotes' own changes to the player | Per-consumer cloning fetch (§5.3); Q_INVOKABLE transport methods so QML can drive it; `subfile` sources; open-paused | `VDh:210-218`, `APh:55-58`, `VD:294-298` |

### 5.2 Classes and threads

| Object | Thread(s) | Job | Ref |
|---|---|---|---|
| `VideoDecoder` | One decode thread per open file | Demux, decode, convert, **pace** (it sleeps until each frame is due), publish | `VDh:48`, `VD:1313-1367`, `VDh:398-401` |
| `ScrubDecoder` (owned by the decoder) | One worker thread | Single-frame fetches with its own FFmpeg context; publishes into the decoder's slot unpaced | `SDh:1-22`, `VDh:220-224` |
| `FrameIndex` | One scan thread after open | PTS ↔ frame number. A formula from time base and rate at open; then an exact table from walking every video packet on its own context. Lookups switch to the table when it is ready | `FI` header |
| `ReadAhead` | One background thread for the whole app, low priority | Touches the bytes of the next frames so a caching network volume has them local | `RAh:3-41` |
| `VideoSurfaceItem` | The scene graph's render thread | Pulls the latest frame and draws it | `VSh:1-12` |
| `AudioPlayer` | Decode thread(s) of its audio decoder; the OS audio callback thread | §5.6 | `APh:1-14` |
| `ShuttleAudioEngine` | One grain thread while a gesture is active | §3.5 | `SAEh:35-38` |
| Editor (QML) | GUI thread | All transport logic; a 33 ms timer that feeds the audio the video's position; a 33 ms timer for the shuttle | `Ed:1431-1455` |
| Poster provider | Qt's image-loading thread | §2.3 | `VFP:12-14` |

### 5.3 How frames reach the screen

| Item | Rule | Ref |
|---|---|---|
| Publish slot | One "latest frame" slot, mutex-guarded, with a monotonically increasing sequence number. Latest wins; there is no frame queue | `VDh:346-353`, `VD:1350-1354` |
| Fetch | Each surface holds its own cursor and **clones** the slot's frame when the sequence is newer. The slot keeps the original, so several surfaces can show one decoder and a re-created surface re-pulls the current frame | `VDh:210-218` |
| The item | A `QQuickRhiItem` (Qt's RHI render-to-texture item) inside the scene graph. No native window, no `QVideoSink` | `VSh:1-12`, `VSh:19` |
| Repaint | On the decoder's `frameAvailable` signal, and on becoming visible | `VSh:47-50` |
| Fit | Letterboxed into the item at the display size: sample-aspect un-squeezed, rotation applied (quarter turns), over `fillColor` | `VS:59-87`, `VS:128-140` |
| Sampling | Linear | `VS:173` |
| Alpha | Straight-alpha "over" the fill colour for formats that carry alpha (4444 on macOS; the Windows Vulkan path; the CPU path) | `VS:185-194`, `VS:220-223` |
| Surfaces bound to the one decoder | The inline overlay, the studio stage, the lightbox, and each visible canvas video item | `Ed:6390-6396`, `Ed:7288-7295`, `LB:144-149`, `CMI:87-93` |

### 5.4 Decode paths

FFmpeg is linked as vendored prebuilt libraries (`avcodec avformat avutil avfilter swscale swresample`,
`CMakeLists.txt:129-137`); code comments refer to FFmpeg 9.0.1 (`TP:5`).

| Platform | Path | Ref |
|---|---|---|
| macOS | VideoToolbox hardware decode, **zero-copy**: the pixel buffer's planes become Metal textures and a fragment shader converts YUV → RGB (biplanar 8 / 10-bit 4:2:0 / 4:2:2 / 4:4:4, and an AYUV shader for 4444 alpha). Falls back to software when the codec has no hardware path | `VD:181`, `VD:747-748`, `VS:254-290`, `ZC:47-62` |
| Windows, ProRes | Vulkan decode on a shared device; a compute pass converts YUV → RGB into a 16-bit-float texture shared with the D3D11 renderer | `VD:763-799`, `VS:322-403` |
| Windows, inter codecs | D3D11VA on the renderer's own device (zero-copy when the device is shared), composited into a 16-bit-float texture; a settings key can turn hardware decode off | `VD:913-931`, `VS:413-482`, `SD:258-266` |
| Any, software | Decode → swscale → 8-bit RGBA image → texture upload → passthrough shader | `VD:1151-1188`, `VS:296-311` |
| Never hardware | ProRes RAW (software only on every platform) | `app/player/hw_routing.h` |
| Software threading | Intra-only codecs with slice threading use slice threads; inter codecs use frame threads; one thread under the Vulkan path | `TP:1-40`, `VD:969-977` |

### 5.5 Colour and geometry

| Item | Rule | Ref |
|---|---|---|
| Dynamic range | **SDR only.** One render pass; no tone mapping, no HDR output, no colour-management pass | `VSh:9` |
| Matrix | From the stream's tag: 601, 709 or 2020 (240M is treated as 601 on the Metal path). **Untagged → 709 at every size**, on every player path | `Sws:73-88`, `ZC:127-141`, `app/player/shaders/yuv_to_rgb_biplanar.frag:15-51` |
| Range | From the tag; untagged → limited. RGB sources get a separate legal → full rule | `Sws:92-104`, `VD:1167-1175` |
| Transfer and primaries | Not converted. The shader's output is described as "BT.709 gamma-encoded"; a PQ or HLG clip is therefore shown with its matrix applied and its curve untouched **(inference)** | `app/player/shaders/yuv_to_rgb_biplanar.frag:12` |
| Window | The window's surface is pinned to sRGB on macOS at start-up | `app/main.cpp:77-99` |
| Bit depth on screen | 8-bit RGBA on the software path; the Windows hardware paths composite through 16-bit float | `VS:296`, `VS:382`, `VS:461` |
| Rotation, anamorphic pixels | Read by the player from the stream (display matrix; sample aspect ratio) and applied at fit time. **Not** applied by the poster decode and not present in the descriptor | `VDh:56-62`, `VD:74-90`, `VS:59-87`, `MS:399-439` |
| Poster vs player | Two differences by construction: the untagged-matrix rule (poster: size heuristic; player: always 709) and rotation / pixel aspect (poster: none) | `MS:424-425`, `Sws:86-88` |

### 5.6 Audio path and A/V sync

```
file → audio decoder thread → resample to 48 kHz float32 stereo
     → tempo stage (constant-pitch speed; bypassed at 1×)
     → lock-free single-producer / single-consumer ring (~500 ms capacity, ~100 ms target depth)
     → [device callback] drain stage (5 ms fades at every discontinuity) → fractional resampler (the servo's trim)
     → gain / mute → the OS output
```

(`APh:1-14`, `Ring:22-28`, `Drain:1-30`, `Drain` `kFadeFrames = 240`, `Tempo:1-22`.)

| Item | Rule | Ref |
|---|---|---|
| Master clock | The **video**. The decode thread paces itself against a wall clock: the first frame after play / seek / speed change sets a baseline; each later frame sleeps until `baseline + (pts − baseline pts) × time base ÷ speed` | `VD:1315-1348` |
| Late frames | There is no drop logic in the pacing: a frame that is already late is published at once **(inference from `VD:1344-1346`)** | `VD:1344-1346` |
| Audio follows | Every 33 ms while playing with audio, the editor calls `audio.update(currentFrame / fps)` | `Ed:1431-1436` |
| Servo | A PI controller turns drift into a consumption ratio: within ±20 ms the trim is capped at ±0.2 %; the cap opens linearly to ±1.5 % at 100 ms; `kP = 0.15 /s`, `kI = 0.01 /s²`, slew ≤ 0.001 per update | `Servo:1-22`, `Servo:92-98` |
| Escape tiers | Drift beyond 100 ms, sustained: a soft re-seek of the audio decoder with a cooldown. Beyond 1 s: an immediate re-seek | `APh:9-14`, `AP:437-443` |
| Tempo range | The stage clamps to 0.25…4.0; the editor uses 0.5…2 | `Tempo:36-37` |

### 5.7 Caches and sizes

| Cache | Size | Key | Ref |
|---|---|---|---|
| Scrub GOP cache | **512 MB** by default (settings key `performance/scrubCacheMB`), byte-bounded LRU; entries hold the decoded frame in native YUV (converted when shown) or, on macOS, a retained pixel buffer | frame number | `SD:100-107`, `SFC:1-35` |
| Read-ahead window | **2 s** of media, capped at **512 MB**, one byte touched per **1 MiB** page; environment overrides `MN_READAHEAD_*`, falling back to the sibling app's names. Nothing is held in the app | — | `RA:36-58`, `RAh:27-41` |
| Read-ahead coverage | Plain files with a full sample index only (MOV / MP4). Not Matroska (cue points only). Not a clip streaming from a package | `RAh:60-66`, `VD:320-331` |
| Audio ring | ~500 ms | — | `Ring:22-28` |
| Decoded playback frames | One: the latest | — | `VDh:346-353` |
| Posters | Qt's pixmap cache, no budget set | URL | §2.3 |
| Disk | None. No proxy, no thumbnail file, no waveform | — | (**not found**) |

A clip inside a package archive is played without extraction when its entry is stored uncompressed: the source is an
FFmpeg `subfile` spec, `subfile,,start,<offset>,end,<offset + size>,,:<archive path>`; a compressed entry is extracted
in the background instead and the source is empty until it lands (`MS:603-628`). Packages are outside this step.

---

## 6. Annotations — the sidecar contract (FROZEN)

Everything in this section is a two-way interchange contract with QCView: either app edits the other's notes. The
code's own rule, repeated at the head of each ported file: folder layout, JSON shape and file names "must stay
byte-compatible — fix QCView first, then re-port" (`AIOh:1-5`). The format takes no extensions, ever.

### 6.1 Where the folder is

| Item | Rule | Ref |
|---|---|---|
| Per-media layout | `<media dir>/.qcview/<media name>/notes.json` and `<media dir>/.qcview/<media name>/images/` | `AIOh:12-14` |
| `<media dir>` | The absolute directory of the media file's path | `AIO:119-123` |
| `<media name>` | The file's **full name including its extension** (`QFileInfo::fileName()`), with each of the nine characters `< > : " / \ | ? *` replaced by `_`. Nothing else is changed: case, spaces, dots and non-ASCII characters are kept | `AIO:109-117`, `AIO:125-131` |
| Which path is "the media file's path" | The **anchor path**: the resolved absolute path for an absolute or `{vol, rel}` source; `<document folder>/.minnotes/<name>` for a sidecar-relative source. Computed without touching the disk, so the folder is derivable for a file that is missing or still inside a package | `MS:630-635`, `MSh:121-124`, `BM:4567-4572` |
| Legacy read fallback | On **read**, if `<media dir>/.qcview` does not exist and `<media dir>/.ump` does, `.ump` is used. The test is on the root folder, not on the per-media folder | `AIO:70-84` |
| Write | Always `.qcview`. Created on first save with `mkpath` | `AIO:333-349`, `AIO:362-370` |
| Windows | When the app **creates** `.qcview`, it sets the hidden attribute on it (and only then). On other systems the leading dot is the whole mechanism | `AIO:97-105`, `AIO:347` |
| Images folder | Created when the first thumbnail is written | `AIO:351-360`, `VNM:415` |

Example (synthetic): the video `/Volumes/shared/renders/shot-010-v003.mov` has its notes at
`/Volumes/shared/renders/.qcview/shot-010-v003.mov/notes.json`. A video collected into a document at
`/Volumes/shared/notes/review.mnd` as `.minnotes/shot-010-v003.mov` has them at
`/Volumes/shared/notes/.minnotes/.qcview/shot-010-v003.mov/notes.json` (`PE:516-531`).

Two other layouts exist in the same code and use the same `notes.json`; they are not video blocks:

| Layout | Path | Ref |
|---|---|---|
| Per project timeline | `<project dir>/.qcview/<sanitised timeline name>/` | `AIOh:16-19`, `AIO:286-331` |
| Per live stream | `<project dir>/.qcview/streams/<sanitised item name>__<first 8 characters of the item id>/`; an empty name becomes `stream`; an existing folder is found by its `__<id8>` suffix so a rename never moves it | `AIOh:21-28`, `AIO:156-196` |

### 6.2 Files in the folder

| File | What | Written by | Ref |
|---|---|---|---|
| `notes.json` | Every note of this media | Any save | `AIO:200-203` |
| `images/note_<stem>.png` | The **clean** frame of a note: the decoded frame with no drawings. The durable baseline both apps recomposite from | When a note is created; **never overwritten** if it exists | `VNM:409-432`, `VNMh:9-11` |
| `images/note_<stem>_annotated.png` | The frame with the note's drawings painted on it. Exists only while the note has drawings | 400 ms after the last drawing change; **removed** when the note has no strokes | `VNM:284-313`, `VA:30-35` |

`<stem>` = the note's key with every `:` and `;` replaced by `_` (`AIO:142-147`). Key `00:00:02:01` → 
`note_00_00_02_01.png` and `note_00_00_02_01_annotated.png`. A stream note's key (a capture stamp) passes through
unchanged.

Both PNGs are part of the contract: the clean PNG is the baseline the other app recomposites its annotated picture
from (`VNMh:9-11`), and a note list prefers the annotated PNG when the note has strokes (`VNM:150-153`).

### 6.3 `notes.json` — the root object

| Key | Type | Meaning | Ref |
|---|---|---|---|
| `media_file` | string | The media file's name (with extension, **not** sanitised) | `AIO:367` |
| `media_type` | string | `"video"`, `"audio"` or `"image"` from the extension (§1.3); `"video"` for anything unknown or extension-less. Metadata only: the code states no consumer reads it | `AIO:31-68`, `AIO:368` |
| `created` | string | An ISO-8601 UTC time. Inserted when the header being written lacks it | `AIO:219-222` |
| `notes` | array of note objects | Sorted by key (§6.5) | `AIO:223-225` |
| `stream`, `stream_id`, `stream_url` | strings | Present **instead of** `media_file` / `media_type` in a stream folder; round-tripped untouched | `AIOh:27-28`, `VNM:387-392` |

For a per-media sidecar the writer builds the header **fresh on every save** (`media_file`, `media_type`), so
`created` is the time of the latest save, and any other root key a file carried is dropped (`AIO:362-370`). In a
stream folder the header read from disk is the baseline and is written back with only missing keys filled
(`VNM:387-392`).

Written with Qt's indented JSON writer through an atomic replace (write a temporary file, then rename over
`notes.json`) (`AIO:228-238`). The byte form is therefore Qt's: object keys in sorted order, four-space indent, Qt's
number formatting, a trailing newline. minplatform's `crates/qcv-notes/src/qt_json.rs` reproduces that form.

### 6.4 A note object

| Key | Type | Required on read | Meaning | Ref |
|---|---|---|---|---|
| `timecode` | string | yes | The note's **key** (§6.5). Unique within the file | `AN:19`, `ANh:42` |
| `timestamp_seconds` | number | yes | `frame / fps` (0 when fps is unknown). For a stream note: seconds since the stream opened | `AN:20`, `VNM:203` |
| `frame` | integer | yes | Zero-based frame number in the clip. 0 for a stream note | `AN:21`, `ANh:58-61` |
| `image` | string | yes | Relative path of the clean PNG: `images/note_<stem>.png` | `AN:22`, `VNM:204-205` |
| `annotation_data` | object, or `null` | no | The note's drawings (§6.6). `null` when there are none. An array is also accepted and kept. Any other type reads as none | `AN:39-53`, `AN:79-95` |
| `text` | string | no (default empty) | The note's text; may hold newlines | `AN:55`, `AN:97` |
| `addressed` | boolean | no (default false) | The "done" mark | `AN:56`, `AN:98` |
| `viewer` | string | no | Written only when non-empty: the viewer aids in effect when the sibling app captured the thumbnail | `AN:23-25`, `ANh:49-53` |
| `ocio` | string | no | Written only when non-empty: the colour chain the thumbnail was captured through | `AN:26-28`, `ANh:54-57` |
| `captured_at` | string | no | Written only when non-empty. ISO-8601 local time with offset. Its presence makes the note a **stream note** | `AN:29-31`, `ANh:58-67` |
| `host_timecode` | string | no | Written only when non-empty: the host application's timecode at capture | `AN:32-34` |
| `stream` | string | no | Written only when non-empty: the stream's display name at capture | `AN:35-37` |

A note object missing any of the four required keys is skipped on read (`AN:62-67`). Keys not in this table are not
carried: the reader fills a fixed struct and the writer emits only these, so an unknown key in a note is **lost on the
next save by either app** (`AN:16-58`). minNotes never writes `viewer` or `ocio` for notes it creates, and preserves
them on notes it loads.

In memory `annotation_data` is a JSON **string**; on disk it is a nested object, for readability. It is re-encoded on
every load (compact) and save (nested), so its whitespace is not significant, and keys inside it that the stroke
parser does not know survive as long as that note's drawings are not edited (`ANh:26-29`, `AN:79-95`).

### 6.5 How a frame is addressed — the key

| Item | Rule | Ref |
|---|---|---|
| Key of a media note | `HH:MM:SS:FF`, the SMPTE-style count of the **frame number** | `VNM:174-182` |
| Derivation | A formatter built with rate `round(fps × 1000) / 1000`, **non-drop**, origin frame 0. Display rate `d = round(fps)` (23.976 → 24, 29.97 → 30, 59.94 → 60). `FF = frame mod d`; `SS = (frame ÷ d) mod 60`; `MM = (frame ÷ 60d) mod 60`; `HH = frame ÷ 3600d` (not wrapped). Each field `%02d`. A negative frame is treated as 0 | `VNM:174-182`, `TC:55-63`, `TC:86-100` |
| Drop-frame | **Never** for a note key, at any rate. Always colons. (`;` is handled in file stems because keys from other sources may carry it) | `VNM:176-181`, `AIO:142-147` |
| Embedded start timecode | **Ignored** for note keys: frame 0 is always `00:00:00:00` | `VNM:176-178` |
| `fps` used | The value the caller supplies — in minNotes the descriptor's `fps` (§1.1), not the decoder's | `Ed:1425-1430`, `CT:130` |
| `fps` ≤ 0 | The formatter is invalid and every frame's key is `00:00:00:00` | `VNM:181` |
| `frame` used | The frame on screen: the decoder's current frame (from its frame index), or the scrub target mid-scrub, or the remembered playhead when the clip is not open | `Ed:7373-7380`, `Ed:7314-7316` |
| One note per frame | A second note at the same key is not created; the existing one is returned | `VNM:198-214` |
| Sort | Notes are kept sorted by key, by plain string comparison | `ANh:78`, `AIO:274`, `VNM:207` |
| Key of a stream note | A capture stamp `YYYYMMDD-HHMMSS-mmm` (sorts chronologically). minNotes never mints one | `ANh:58-62`, `VNM:200` |

There is no millisecond or timecode address independent of the key: `timecode` is the identity, `frame` is the seek
target, `timestamp_seconds` is derived. Both apps must derive the key identically or the same frame gets two notes
(`VNMh:6-8`). minplatform's `qcv_notes::note_key` already encodes the rule (`crates/qcv-notes/src/lib.rs:16-21`).

### 6.6 `annotation_data` — the drawings

```json
{
    "coordinate_system": "normalized",
    "shapes": [ … ],
    "version": "2.0"
}
```

| Key | Type | Rule | Ref |
|---|---|---|---|
| `version` | string | Written `"2.0"`. Read: `"1.0"` and `"2.0"`; absent = `"1.0"`; **any other value → no shapes are read** | `AS:45`, `AS:73-81` |
| `coordinate_system` | string | Written `"normalized"`. Read: if present and not `"normalized"` → no shapes are read | `AS:46`, `AS:83-86` |
| `shapes` | array | The strokes, in draw order (later = on top). Missing or not an array → none | `AS:48-52`, `AS:88-90` |

A shape object:

| Key | Type | Default on read | Meaning | Ref |
|---|---|---|---|---|
| `id` | string | — | `stroke-<ms since epoch>-<random 1000…9999>`. **Not read back.** The Qt writer mints a new id for every shape each time a note's drawings are re-serialised | `AS:15`, `AS:193-198` |
| `type` | string | required | `freehand`, `rect`, `oval`, `arrow`, `line`. A shape with a missing or any other `type` is **dropped** | `AS:108-111`, `AS:170-191` |
| `color` | array of 4 numbers | opaque red | `[r, g, b, a]`, each 0…1, sRGB, straight alpha. Fewer than 4 entries → red | `AS:18-23`, `AS:113-126` |
| `stroke_width` | number | 2.5 | Line width in **source pixels** — pixels of the video frame, not of the screen | `AS:25`, `AS:128-130`, `VAh:8-11` |
| `filled` | boolean | false | `rect` and `oval` only: filled instead of outlined. minNotes always writes false (it has no fill control) | `AS:26`, `AS:132`, `VPAh:186` |
| `is_modeled` | boolean | false | True when the points were already smoothed at capture (§6.7). Forced false for every shape of a `"1.0"` document | `AS:27`, `AS:97-98`, `AS:133` |
| `points` | array of `[x, y]` pairs | required, non-empty | Per-type meaning below. Entries that are not arrays of ≥ 2 numbers are skipped; a shape left with no points is dropped | `AS:29-36`, `AS:135-144` |

Coordinate system:

| Item | Rule | Ref |
|---|---|---|
| Space | The video **frame**: `x` = 0 at the left edge, 1 at the right; `y` = 0 at the top, 1 at the bottom | `VPA:180-190`, `Thumb:19-22` |
| Which rectangle | The frame box itself — the drawing item is sized to the fitted frame, with no letterbox inside it. Coordinates are independent of display size and of pixel aspect | `VAh:8-11`, `Ed:7257-7274`, `Thumbh:3-7` |
| Range | Clamped to 0…1 at capture; a move or resize of a drawing is clamped so it stays inside the frame | `VPA:211-217`, `VA:508-517` |

`points` by type:

| `type` | `points` | Drawn as | Ref |
|---|---|---|---|
| `freehand` | N ≥ 1 points along the path | A polyline with round caps and joins; a single point is a filled dot of diameter = line width | `Thumb:72-90` |
| `line` | 2: start, end | A line | `Thumb:92-98` |
| `arrow` | 2: tail, head | A line stopping short of the head, plus a filled triangle at `points[1]` | `Thumb:133-172` |
| `rect` | 4 corners in the order start-corner, (end x, start y), end-corner, (start x, end y). Readers use `[0]` and `[2]` as opposite corners | Outline, or filled | `VPA:347-352`, `Thumb:100-114` |
| `oval` | 2: `[0]` = centre, `[1]` = **radii** as fractions of frame width and height (not a point) | Ellipse outline, or filled | `VPA:389-393` (`points = { center, radii }`), `Thumb:116-131` |

Rendering rules that both apps must share for the pictures to match (`Thumb:53-179`):

| Item | Rule |
|---|---|
| Pen width | `max(1, stroke_width × scale)` where `scale` = target width ÷ source width (1 when painting on the source-resolution thumbnail) |
| Caps and joins | Round |
| Arrowhead | Size = `4.5 × scale + penWidth × 2.3`; half-angle 25°; the shaft ends `size × cos 25°` before the tip. The code notes this head differs slightly from the sibling app's tessellator; the stored stroke is identical |
| Outline width never distorts | The thumbnail is un-squeezed to display aspect first, then strokes are drawn at uniform thickness |

### 6.7 How a drawing is captured (minNotes side)

| Item | Rule | Ref |
|---|---|---|
| Where | The studio only. The inline block and the lightbox never edit | `Ed:7302-7324`, `Ed:6431-6439` |
| Tools | `freehand`, `rect`, `oval`, `arrow`, `line`, `eraser`, plus select (no tool armed). The text-chip tool drops to select in a video view: text boxes are not part of this format | `VA:78-100`, `Ed:559-562`, `Ed:591` |
| Colour, width | From the inspector's draw state: default red `#FF0000`, default width 6 source pixels, range 1…64; persisted in application settings | `Insp:35-54` |
| Stroke start | Pauses playback: a stroke pins to one frame | `Ed:7321-7323` |
| Freehand smoothing | Points pass through Google's ink stroke modeler at capture (coordinates scaled to 0…1000 for it; wobble 0.04 s / 31 / 620; spring-mass 11/32400; drag 72; prediction off). The **smoothed** points are stored and `is_modeled` is true. A freehand stroke ends when the pointer leaves the frame | `ISM:11-47`, `VPA:230-318` |
| A stroke on a frame with no note | Creates the note first (and its clean PNG), then stores the stroke | `VA:249-269` |
| Eraser | While pressed, removes the topmost stroke whose bounding box, padded by `max(0.012, stroke_width / 1920)` normalised units, contains the pointer — one stroke per pointer event. Does not pause playback | `VA:271-326` |
| Select mode | Click selects; Shift-click toggles; drag on empty stage is a marquee; drag moves the selection (clamped to the frame); a single selection has four corner handles for a proportional resize (minimum 0.03 normalised); Delete removes | `VA:471-654` |
| Every edit | Re-serialises **all** of that frame's strokes (new ids for all), writes the note, and schedules the annotated PNG | `VA:640-654`, `AS:41-56` |
| Erased to nothing | `annotation_data` becomes `null`; the note itself stays; the annotated PNG is removed | `VA:320-323`, `VNM:297-298` |

### 6.8 Notes, saving, undo

| Item | Rule | Ref |
|---|---|---|
| Add a note | The studio's **Add note** tile: mint the key for the frame on screen; if new, append `{timecode, timestamp_seconds, frame, image, text: ""}`, sort, save; then ensure the clean PNG exists | `Ed:7350-7383`, `VNM:198-214` |
| Clean PNG | Decoded by the poster path at **full source resolution** on a worker thread and saved as PNG. Skipped when the file already exists | `VNM:409-432` |
| Annotated PNG | The clean PNG with the strokes painted at scale 1, tagged sRGB, saved beside it; pixel aspect is taken as 1:1 (minNotes does not track it) | `VNM:284-313`, `Thumb:181-218` |
| Text | Edited in the note's card; committed when the field loses focus | `Ed:7526-7535`, `VNM:216-223` |
| Addressed | A toggle on the card | `Ed:7549-7558`, `VNM:225-232` |
| Delete (the card's ×) | Removes the note **and both PNGs** | `VNM:234-249` |
| Save | Every change schedules a save on a worker thread. The save is **read – merge – write**: load what is on disk now; start from our list; add every disk note whose key we do not hold and did not delete in this session; sort; write. For a key both sides hold, **our copy wins** | `VNM:359-407`, `VNMh:13-17` |
| Tombstones | Keys deleted locally are remembered until the next save has completed, so a reload in between cannot resurrect them. Re-adding a key clears its tombstone | `VNM:240`, `VNM:346-350`, `VNM:401`, `VNM:208` |
| Live pickup | A file watcher on `notes.json` (or, until it exists, on the nearest existing ancestor folder); a change is debounced 200 ms, then reloaded on a worker thread. The app's own writes are skipped by remembering the file's modification time after each save. The watch is re-armed after every save because an atomic replace drops it | `VNM:61-76`, `VNM:496-511`, `VNM:397-402` |
| Reload | Disk replaces memory (minus tombstoned keys), unless nothing differs in key, frame, text, addressed, drawings or image path | `VNM:342-357`, `VNM:41-57` |
| No lock | There is no lock file and no version counter; concurrency rests on merge-by-key and the watcher | (**not found**) |
| **Undo rule** | Annotation undo is an **in-memory** pair of stacks owned by the studio's drawing item. Each entry is `{timecode, frame, the note's annotation_data before the edit, wasNew}`. It never opens a document transaction: notes are outside the document | `VAh:13-15`, `VAh:106-111` |
| Undo of a stroke that created its note | Deletes the note but **keeps its PNGs** so a redo can restore it | `VA:328-339`, `VNM:273-282` |
| Granularity | One entry per stroke, per move / resize / delete of a selection, and per eraser press-to-release (per key touched) | `VA:262-264`, `VA:214-231`, `VA:640-647` |
| Scope | Drawings only. Adding a note from the tile, text edits, the addressed toggle and the card's delete are **not** undoable | `VNM:198-249` (no stack) |
| Lifetime | Cleared when the studio's media changes. Never persisted | `VA:50-56` |
| Key routing | While the studio is open, undo / redo keys go to this stack and never to the document's | `Ed:3341-3347` |

### 6.9 How notes show outside the studio

| Place | Rule | Ref |
|---|---|---|
| Strip | A tick per note on the scrub line (§3.2). Not individually clickable: the scrub area is above them | `CT:75-84`, `CT:91-93` |
| Full bar | A 12 px filled triangle per note, straddling the bar's top edge, at the x where the slider's handle centre sits for that frame. Click: activate, pause, go to the frame | `VT:197-235` |
| Marker colour | Violet `#b48ef0` — "a note" is one colour in both apps; grey when addressed | `VT:219-222`, `app/qml/MinNotes/App/Theme.qml:70` |
| Inline drawings | The current frame's strokes painted over the live surface, read-only, strictly per frame | `Ed:6429-6439` |
| Inline caption | A 26 px strip on the picture's bottom edge, black at 62 %: a check icon when addressed, the key in mono, the text on one line, elided. When paused it tracks the frame exactly; during playback, shuttle or scrub it **lingers 2.5 s** after the playhead crosses a note's frame (a one-frame flash is unreadable) | `Ed:6399-6427`, `Ed:6441-6471` |
| Hide switch | One switch for the whole editor hides the drawings and the caption (not the markers, not the studio's cards): the eye button on the full bar, and a tile in the studio | `Ed:1037-1041`, `VT:145-156`, `Ed:7386-7416` |
| Poster | Shows no drawings | `MB:230-267` |
| Search | Note texts are indexed as "Video notes"; a hit opens the studio at the note's frame | `SE:161-184`, `SB:38`, `SB:57-67` |
| Notes into the page | Block menu **Notes below**: one undo step that inserts a level-2 heading `Notes — <file name>`, a divider, then per note an **image block** (a copy of the clean PNG in the document's own media folder) carrying the strokes as that image's own ink, and a paragraph `[✅ ]<key>  ·  frame <n>[ — <text>]`, then a divider. After that the blocks are ordinary document content with no link back | `Ed:1307-1326`, `VNM:436-475`, `Ed:8360` |

### 6.10 What "1:1 with QCView, permanently" requires of a reader / writer

| # | Requirement | Ref |
|---|---|---|
| 1 | Derive the folder exactly as §6.1: media directory, `.qcview`, the full file name with the nine characters replaced | `AIO:109-131` |
| 2 | Derive the key exactly as §6.5: non-drop, `round(fps)`, frame-0 origin, the same `fps` and frame numbering the other app would use for that file | `VNMh:6-8` |
| 3 | Write the note keys of §6.4 with those names and types, `annotation_data` as a nested object or `null`, and nothing else | `AN:16-58` |
| 4 | Write strokes as §6.6: version `"2.0"`, `"normalized"`, the five type names, the per-type point encodings (the oval's centre + radii, the rectangle's four corners), widths in source pixels, colours as four 0…1 numbers | `AS:12-56` |
| 5 | **Add nothing.** No new note key, no new shape type, no new file the other app would need. The sibling app's writer drops unknown note keys and unknown shape types on its next save, so an extension is destroyed, not ignored | `AS:108-111`, `AN:16-58` |
| 6 | Write **both** PNGs under the exact names of §6.2; never overwrite an existing clean PNG; remove the annotated PNG when a note has no strokes | `VNM:418-420`, `VNM:297-298` |
| 7 | Save by atomic replace, after re-reading the file and merging by key, with local deletes remembered | `AIO:228-238`, `VNM:369-384` |
| 8 | Watch the file and pick up the other app's saves while open | `VNM:61-76` |
| 9 | Keep annotation undo in memory and out of the document's undo | `VAh:13-15` |
| 10 | Read `.ump` when `.qcview` is absent; write only `.qcview`; hide it on Windows when creating it | `AIO:70-105` |
| 11 | Tolerate what the other side writes: `viewer`, `ocio`, the stream keys, `annotation_data` as an array, version `"1.0"` strokes (treated as un-smoothed) | `AN:73-95`, `AS:73-98` |
| 12 | Never modify the media file, and never move notes into the document as the primary copy | `VNMh:19-20` |

A copy of the sidecar travels with a copied video and is never rewritten: Collect Media and the package exporter copy
the whole folder to `.minnotes/.qcview/<new name>/` (or `media/.qcview/<packed name>/` in a package) with its content
untouched (`PE:516-531`, `PEh:17-19`).

### 6.11 Example `notes.json` (synthetic)

A 24 fps clip named `shot-010-v003.mov`; one note at frame 49 with a red arrow and an oval, one text-only note at
frame 120:

```json
{
    "created": "2026-11-10T14:02:31Z",
    "media_file": "shot-010-v003.mov",
    "media_type": "video",
    "notes": [
        {
            "addressed": false,
            "annotation_data": {
                "coordinate_system": "normalized",
                "shapes": [
                    {
                        "color": [1, 0, 0, 1],
                        "filled": false,
                        "id": "stroke-1762783351000-4821",
                        "is_modeled": false,
                        "points": [[0.25, 0.5], [0.4, 0.35]],
                        "stroke_width": 6,
                        "type": "arrow"
                    },
                    {
                        "color": [1, 0, 0, 1],
                        "filled": false,
                        "id": "stroke-1762783352500-1377",
                        "is_modeled": false,
                        "points": [[0.7, 0.3], [0.08, 0.12]],
                        "stroke_width": 6,
                        "type": "oval"
                    }
                ],
                "version": "2.0"
            },
            "frame": 49,
            "image": "images/note_00_00_02_01.png",
            "text": "Flicker on the sign",
            "timecode": "00:00:02:01",
            "timestamp_seconds": 2.0416666666666665
        },
        {
            "addressed": true,
            "annotation_data": null,
            "frame": 120,
            "image": "images/note_00_00_05_00.png",
            "text": "Hold two frames longer",
            "timecode": "00:00:05:00",
            "timestamp_seconds": 5
        }
    ]
}
```

Arrays are shown on one line here for reading; Qt's writer puts each array element on its own line. Folder contents
for this file: `notes.json`, `images/note_00_00_02_01.png`, `images/note_00_00_02_01_annotated.png`,
`images/note_00_00_05_00.png`.

---

## 7. Timecode features that touch the player

| Feature | Rule | Ref |
|---|---|---|
| The counter | Every transport's counter shows `HH:MM:SS:FF` computed in QML: `r = round(fps)`; `FF = frame mod r`; seconds = `frame ÷ r`; non-drop; frame-0 origin. With `fps` ≤ 0 it shows the bare frame number | `Ed:60-70` |
| Counter mode | Timecode by default; a click on any counter flips all of them to frames. Persisted in application settings (`view/frameCounter`) | `Ed:51-63`, `CT:59-60` |
| Drop-frame in the counter | None. A 29.97 clip counts 30 frames to the second with colons | `Ed:64-70` |
| The file's own start timecode | Not shown. The decoder reads an embedded start timecode and its drop-frame flag and can format with them; no QML calls those | `VDh:157-183` (no caller found in `app/qml`) |
| Note keys | §6.5 — the same arithmetic as the counter, so a note's key equals the counter's reading at that frame | `VNM:174-182` |
| Seeking by a note | A card, a marker or a search hit seeks by the note's `frame`, never by parsing its key | `Ed:7491-7494`, `VT:228-232`, `SB:57` |
| Timecode **table column** | A column kind that normalises a cell to timecode at the column's rate: a bare integer is read as a frame count, a timecode is re-formatted; rates offered `23.976, 24, 25, 29.97, 30, 60`, default 24; **drop-frame for 29.97 and 59.94** (`;` before the frames); sorts by frame count | `BMh:331-345`, `BM:1241-1286`, `Ed:8452-8463` |
| Does the column drive a player | **No.** A search for a path from a timecode cell to the transport found none: clicking a cell places the caret. The column and the counter also disagree at 29.97 (the column drops frames; the counter and note keys do not) | `BM:1251-1264` (callers: `BM:1025`, `BM:1280-1282` only) |
| Timecode chips in text | **Not found.** No inline chip type carries a timecode or a link to a clip | — |

---

## 8. Lightbox and studio

### 8.1 Lightbox

Opened by `Space` with the caret on an image, video or PDF block (`Ed:3476-3478`), by a Space tap or Return on a
selected canvas element, or by a double-click on a canvas image (`LB:1-11`).

| Item | In-block player | Lightbox | Ref |
|---|---|---|---|
| Surface | In the page, at block size | Fitted inside the window with 40 px margins, over a scrim of the app background at 94 % | `LB:122-131`, `LB:144-149` |
| The block's own surface | — | Hidden while the lightbox shows; one decoder, the lightbox's surface is simply another consumer | `Ed:6377` |
| Transport | The 22 px strip | The **full bar** (40 px), under the picture | `LB:171-184` |
| On open | — | Plays, unless this clip is already the active one (then it keeps its state) | `LB:74` |
| On close | — | Pauses if playing; playhead and active clip kept; focus returns to the editor with the caret where it was | `LB:79-84` |
| `Space` | Opens the lightbox | Closes it (also `Esc`, the × button, a click on the scrim) | `LB:111`, `LB:125` |
| Play / pause | `K` `W` `S`, the strip | `K` `W` `S`, the bar | `LB:8`, `LB:117` |
| `←` / `→` | Step a frame | Step a frame when the editor's key context has a video; otherwise step to the neighbouring media item | `LB:112-115` |
| Top bar | — | ‹ › to the previous / next image, video or PDF block in document order (wrapping), the file's name centred, × | `LB:86-104`, `LB:215-235` |
| Drawings and caption over the picture | Shown | **Not shown** (the lightbox has no stroke layer) | `LB:128-170` |
| Before the first frame | The poster | The word `loading…` | `LB:164-169` |
| Other keys | Type into the document | All swallowed | `LB:118` |
| Navigation underneath | — | Any change of the open view closes the lightbox | `LB:105-106` |

### 8.2 Studio

A full-window view of one video, reached by the strip's expand glyph, the block menu, the path bar's Videos list, a
narrow-lane click or a search hit. It appears in the navigation path like a table or PDF view (`Ed:513-517`,
`app/qml/MinNotes/App/PathBar.qml:297-303`).

| Item | Rule | Ref |
|---|---|---|
| On open | Activates the clip **paused** at its remembered playhead, so the stage shows a frame | `Ed:441-460` |
| Stage | The frame aspect-fitted and centred, 16 px margins, on the page-field colour. An explicit fit box, so the poster, the surface and the drawing layer are the same rectangle | `Ed:7257-7274` |
| Until the first live frame | The poster at the remembered playhead | `Ed:7276-7287` |
| Drawing layer | Shown only over a live surface and when notes are not hidden; armed by the inspector's draw tool (§6.7) | `Ed:7302-7324` |
| Transport | The full bar, between the stage and the notes panel | `Ed:7328-7334` |
| Notes panel | 320 px tall at the bottom: an **Add note** tile and a **Hide / Show** tile at the left, then a horizontal strip of cards, 220 px wide, 12 px apart. A card: thumbnail (16:9 box; the annotated PNG when there is one, else the clean one), the key and `f <frame>`, the text field, an addressed toggle, a delete button. The card at the playhead's frame has a bright border. Clicking a thumbnail seeks to the note's frame, paused. Empty state: "No notes yet" | `Ed:7341-7571`, `VNM:142-172` |
| `Space` | Plays / pauses here | `Ed:1289` |
| Volume keys | `↑` / `↓` here only | `Ed:1296-1297` |
| Document under it | Hidden; typing, paste and cut are swallowed | `Ed:3384-3389`, `Ed:3425-3431` |
| On close | §3.10 | `Ed:450-458` |

### 8.3 The full bar (`VideoTransport`, 40 px)

Left to right (`VT:36-178`):

| # | Control | Behaviour |
|---|---|---|
| 1 | Jump to start | Activate; frame 0, paused |
| 2 | Rewind — hold | Shuttle backward while pressed (§3.5) |
| 3 | Step back | One frame |
| 4 | Play / pause | |
| 5 | Step forward | One frame |
| 6 | Fast-forward — hold | Shuttle forward while pressed |
| 7 | Jump to end | Last frame, paused |
| 8 | Scrub slider | Fills the width; white progress fill; press pauses and starts scrub audio, move scrubs, release resumes if it was playing. When the clip is not live it parks at the remembered playhead |
| 9 | Counter | As the strip's; width reserved at the clip's widest value |
| 10 | Speed readout | Only when ≠ 1×; click cycles |
| 11 | Notes eye | Shows / hides drawings and captions; present only when the clip has notes |
| 12 | Loop | Checked when on |
| 13 | Mute | Only when live with audio |
| 14 | Volume slider | 64 px; only when live with audio |

Controls 11, 13 and 14 keep their space when absent (they turn transparent) so the slider never changes width
mid-gesture (`VT:142-144`). Note markers sit over the bar's top edge (§6.9).

---

## 9. Export

### 9.1 Document exporters

A video is never embedded or transcoded by an exporter. Each writes a still, a reference to the file, and
optionally the clip's notes. The still is always **frame 0**, decoded by the poster path at up to 1280 px wide — not
the remembered playhead (`Ex:488`, `Ex:1128`, `Ex:2395`, `Ex:3281`).

The metadata line is `<w>×<h> · <fps, 5 significant digits> fps · <m:ss or h:mm:ss> · <frames> frames`
(`Ex:494-498`, `Ex:137-143`). `path` is the resolved absolute path on the exporting machine (`Ex:468`).

| Format | Written for a video block | Ref |
|---|---|---|
| Markdown | `![name](assets/<base>_poster…)` when the still decodes; then a fenced block whose info string is `video` holding three lines: file name, path, metadata. Optionally an autolink to open the file in the sister file-browser app | `Ex:483-515` |
| HTML | `<figure class="ref">` with the still, and a caption of file name, path, the optional link, and kind · metadata | `Ex:1125-1170` |
| DOCX | The still as an inline picture; the file name in bold; the path in mono; the optional hyperlink; `video · <metadata>` in mono | `Ex:2391-2436` |
| PDF | The still at 480 pt wide; the same reference lines | `Ex:3278-3297` |
| Canvas HTML | The same parts for a canvas video element | `CEx:340-376` |

Notes, when "Include video notes" is on — the option is offered only when a pre-scan finds notes, and reads
"Include video notes (N notes on M videos)" (`Main:475-496`, `Ex:801-829`):

| Format | Notes output | Ref |
|---|---|---|
| Markdown | `**Notes (N)**`, then per note: the thumbnail (the annotated PNG if it exists; else the clean PNG, recomposited with the strokes when there are any), then `` `key` `` ` — text` ` — ✓ addressed` | `Ex:417-463` |
| HTML | A notes section; per note a card with the clean PNG as a base layer and the strokes rendered to a **separate transparent PNG** stacked above it (falling back to one flattened image), the key, the addressed mark, the text | `Ex:1011-1084` |
| DOCX | A bold `Notes (N)` line, then per note the thumbnail and its line | `Ex:2452-2460` |
| PDF | `Notes (N)`, then per note the thumbnail at **full content width** and a mono line `key — text — ✓ addressed` (muted when addressed) | `Ex:3211-3249` |

Exporters read the sidecar directly from disk; they never write to it.

### 9.2 Copying exporters

| Operation | Videos | Sidecar | Ref |
|---|---|---|---|
| Package (`.mnpkg`) | An "Include videos (count, size)" option appears when the file has videos. The option's state is initialised **on**; the comment beside its checkbox says "default off — size". Left-out videos stay references | The whole `.qcview/<name>/` tree travels under `media/.qcview/<packed name>/`, content untouched | `Main:292`, `Main:305`, `Main:441-471`, `PE:83-104`, `PEh:17-19` |
| Collect Media | Opt-in, default off | Copied to `.minnotes/.qcview/<new name>/` | `Main:551`, `Main:564`, `Main:610-640`, `PE:516-531` |

---

## 10. Open questions / things that looked accidental

Each item is something the Qt code does that reads as a consequence of Qt or of history rather than as intent. They
are for the user to rule on before the new design copies them.

| # | Behaviour | Why it looks accidental | Ref |
|---|---|---|---|
| 1 | Scrolling the playing (or merely paused) block out of view closes the decoder; scrolling back shows a poster and playback must be restarted | The surface is one item positioned over a row; a row outside the visible range has no place to put it. The teardown is also deferred a turn to dodge a binding loop | `Ed:1073-1086` |
| 2 | Only one clip can be open at all — not merely one playing. A second block cannot sit paused on a chosen frame except as a poster | "One video at a time for free" from the single shared decoder, which exists because pooled delegates cannot own one | `Ed:1003-1009` |
| 3 | A missing video is an invisible gap: a page-coloured rectangle with a duration badge, and a strip that looks operable but does nothing, silently | The image branch has an "unavailable" text; the video branch has only its backdrop | `MB:217-267`, `Ed:1177` |
| 4 | The lightbox shows `loading…` for ever on a clip that cannot open | Its text is bound to "surface not ready" only | `LB:164-169` |
| 5 | The strip's play button draws a transparent icon until its clip is live | `iconColor: live ? bright : "transparent"`, with a comment about reading "through tones"; the result is an unlabelled empty square on every idle video | `CT:41-42` |
| 6 | The live surface and the strip compute their width as `min(lane, w)` and ignore `dw`, while the poster honours it | Three copies of the width rule; harmless today only because nothing sets `dw` on a video | `Ed:6383`, `Ed:6495`, `BM:2693-2698` |
| 7 | A video narrower than 300 px on a wide page has no strip and a click does nothing: the strip's test is on displayed width, the open-on-click test is on lane width and requires a lane | Two different conditions for one rule | `Ed:6489-6490`, `Ed:6646-6650` |
| 8 | The block's shape, the poster and the note thumbnails ignore rotation and anamorphic pixels; the live surface applies both. A rotated phone clip is a landscape block with a pillarboxed upright picture when live and a sideways poster when idle **(inference)** | The probe stores coded size only; rotation was added to the player later | `MS:281-282`, `VS:59-87` |
| 9 | Poster and player choose different matrices for untagged standard-definition video, so the colour shifts between idle and live | The poster kept the size heuristic; the player moved to "always 709" | `MS:424-425`, `Sws:86-88` |
| 10 | The poster maps frame → time by `frame / fps`; the player uses a scanned frame table. For variable-frame-rate files the poster of the remembered playhead can be a different picture from the frame that was on screen **(inference)** | Two decode paths | `MS:456-462`, `FI` header |
| 11 | Every poster request opens the file and the codec again, single-threaded, and decodes from the previous keyframe; each new playhead is a new cache entry | An image provider keyed by URL | `MS:441-490`, `MB:247-250` |
| 12 | The probe runs on the GUI thread inside the drop handler (`find_stream_info` on a network file) | A synchronous invokable | `BM:4386-4391` |
| 13 | High-dynamic-range clips are shown without tone mapping **(inference)** | The compositor passes were not ported; "SDR only" | `VSh:9` |
| 14 | Playhead memory is keyed by absolute path and kept for the session only: it survives a note switch but not a restart, and is shared by two blocks of the same file | A JS object on the editor | `Ed:1088-1096` |
| 15 | Loop, review speed, volume and mute are global to the editor, not per clip, and none is remembered across runs; the timecode / frames choice is | Whatever was a property vs a settings entry | `Ed:1036`, `Ed:1051`, `Ed:58` |
| 16 | Loop and review speed have no control on the strip, and the strip gives no sign that either is on; an inline clip can be looping or at 1.5× with nothing showing it | They are on the full bar only | `CT:32-126`, `VT:127-158` |
| 17 | `Space` means play in the studio, "open the lightbox" on a page and the canvas; the manual and the strip's tooltip still say `Space` plays | A ruling of 2026-10-22 that the text did not follow | `Ed:1287-1289`, `CT:43`, `ManA:56` |
| 18 | In the lightbox the transport keys act on the block that holds the **caret**, not on the item the lightbox is showing. After stepping with ‹ › to another video, a key re-activates the caret's clip under the other clip's title; if the caret is on an image, a video reached by ‹ › takes no transport keys **(inference)** | The lightbox forwards keys to the editor's router, whose page context is the caret row; stepping does not move the caret | `LB:65-75`, `LB:112-117`, `Ed:1246-1248` |
| 19 | The eye button on the lightbox's bar toggles note visibility, but the lightbox draws no notes | The bar is shared; the stroke layer is not | `LB:171-184`, `VT:145-156` |
| 20 | Return on a focused video makes a paragraph; double-click selects a word (on an opaque block). Neither opens anything, while the canvas uses both to open | The page predates the canvas's click = select, double-click = open ladder | `Ed:1791`, `Ed:6813-6831` |
| 21 | A drawing on the inline block cannot be made: editing is the studio's alone, and the inline stroke layer must be disabled to stay mouse-transparent | A disarmed drawing item still accepts the left button | `Ed:6429-6439` |
| 22 | Adding a note, editing its text, marking it addressed and deleting it are not undoable; only strokes are | The stack was ported for strokes; the card actions call the model directly | `VNM:198-249`, `VAh:13-15` |
| 23 | The card's delete removes both PNGs at once; with no undo, a note's clean frame cannot be recovered | Mirrors the sibling app's click path | `VNM:234-249` |
| 24 | Every edit of a frame's drawings gives all its strokes new ids, and `created` in a media sidecar is rewritten on every save | The Qt writer mints an id per stroke per serialise and builds the header fresh. (minplatform's notes crate already keeps ids, by a recorded decision) | `AS:15`, `AIO:362-370`, minplatform `crates/qcv-notes/src/lib.rs:5` |
| 25 | A clip whose descriptor has `fps` = 0 keys every note `00:00:00:00`: one note for the whole clip | The formatter is invalid and falls back to a constant | `VNM:181` |
| 26 | The note key uses the descriptor's `fps` (probed once at insert) while the frame number comes from the decoder | Two sources that normally agree | `Ed:1425-1430`, `VNM:174-182` |
| 27 | The `.ump` fallback is decided on the root folder: if only `.ump` exists, notes are read from it; the first save creates `.qcview` and writes there, and the old PNGs stay behind in `.ump` **(inference)** | Read and write resolve the root differently | `AIO:70-84`, `AIO:333-349` |
| 28 | A collected or packed video whose name collided is renamed (`-2`), and its sidecar folder follows the new name, but `media_file` inside `notes.json` keeps the old one | "Content never touched" | `PE:516-531` |
| 29 | Counter and note keys are non-drop at 29.97; the timecode table column is drop-frame at 29.97. A key copied from a note into such a column is re-read as a different frame | Two timecode rules in one app | `Ed:64-70`, `BM:1245-1247` |
| 30 | The file's embedded start timecode is read by the decoder and never shown | The picker for it was not brought over | `VDh:157-183` |
| 31 | The package option for videos starts **on** although its comment and the Collect dialog say off | A default changed without the comment | `Main:292`, `Main:305`, `Main:441` |
| 32 | Every strip in the document is built at load and kept alive, each with its own notes reader and file watcher on its video's sidecar folder | Avoids delegate churn; a document with many videos on a network share opens that many watchers **(inference)** | `Ed:5899-5906`, `CT:127-131`, `VNM:61-76` |
| 33 | Playback pacing never drops a late frame; sync is kept by bending the audio (up to ±1.5 %) and, past 100 ms, re-seeking it **(inference for the no-drop part)** | The video thread is the master clock by construction | `VD:1315-1348`, `Servo:1-22` |
| 34 | Exports always use frame 0 as the still, even when the user has parked the clip on a chosen frame | The playhead is session state the exporter cannot see | `Ex:488` |
| 35 | Clean note thumbnails are full-resolution PNGs of the source frame (a UHD frame is tens of megabytes), written beside the media on what is usually a shared volume | The clean frame is the durable baseline of the format | `VNM:421` |

## 11. Not determined

| Item | Why |
|---|---|
| Whether volume and mute survive a clip change | The audio object is shared and holds them as fields; whether `open` / `close` reset them was not read to the end. Stated in §3.8 as an inference |
| The exact FFmpeg version shipped | The libraries are vendored binaries outside the repository; only code comments name a version |
| Which hardware-decode path runs for which codec on a given machine, and its performance | Read from code only; nothing was run |
| How the sibling app derives `fps` and the frame number for the note key on its side | Only minNotes' port of the rule was read; the claim that they agree is the code's own |
| The `viewer` and `ocio` strings' grammar | Written only by the sibling app; minNotes passes them through |
| The exact soft re-seek cooldown and the shuttle engine's remaining constants | Not read beyond the values quoted |
| What happens to the active player on a page change inside one file | Deduced from the row-reconcile rule (§3.10), not traced |
| Linux | A VAAPI branch exists in the decoder; no Linux audio device was found in the tree |
| Canvas video element behaviour beyond what is quoted | Outside this step |
| The notes-to-blocks path for stream folders, and "Notes as a page below" on the canvas | Outside this step |
| In / out points, a marked range, A/B compare, reverse play, a waveform, captions / subtitles, a frame-export command, a "copy frame" command, a "set poster frame" command | Searched for; **not found** |
| A hover skim on the idle block | Searched for; **not found** |
| A timecode chip or link in running text | Searched for; **not found** |
