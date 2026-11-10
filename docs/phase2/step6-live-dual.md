# Spec: a live source as a dual-view side (2.x behaviour → 3.0 plan)

Read 2026-11-10. Read-only work: nothing was built, run or changed in any repository.

- 2.x = `../QCView-Player`, working tree at `v2.7.3-2-gcb890143` (branch `main`, two commits past
  the tag: the appcast and a Windows vcpkg pin — neither touches dual or live code). Paths below are relative to it.
- 3.0 = `../minplatform` at `8b01ff0`. Paths prefixed `mp:` are relative to it.
- "Verified" = read in the code at the cited line. "Inferred" = a consequence I derived from the code but did not run.
  Nothing here was executed; every behavioural claim about a running app is a reading of the code.

---

## 1. What 2.x does

### 1.1 The shape, in one paragraph (verified)

The dual "island" (`src/dual/`) is a clock pump over two `IDualSource`s. A live side is a third source kind,
`DualLiveSource` (`src/dual/dual_live_source.h:1-24`), which owns its own receiver (`HostBridgeSource` for `qcbae://`,
`LiveStreamDecoder` for any other `scheme://` — `dual_live_source.cpp:43-70`), keeps the newest frame in a latest-wins slot
and returns that frame for ANY master frame asked (`dual_live_source.cpp:95-105`). It reports `fps() == 0`,
`frameCount() == 0`, `isLive() == true` (`dual_live_source.h:66-70`); seek and decode-target are no-ops (`.cpp:92-93`).
The controller special-cases `isLive()` in exactly these places:

| Where | What | Evidence |
|---|---|---|
| kind detection | any path containing `://` is `DualSourceKind::Live` | `src/dual/dual_playback_controller.cpp:78-83` |
| master clock | a live side is left out of master fps and master length; two live sides → 24 fps fallback, length 0 | `dual_playback_controller.cpp:214-246` |
| master→source frame | handed back unchanged for a live side (otherwise fps 0 makes every pull return nothing) | `dual_playback_controller.cpp:769-776` |
| past-end / gap test | never past end | `dual_playback_controller.cpp:1000`, `:1019` |
| seek-while-playing hold | a live side is always "ready" | `dual_playback_controller.cpp:1042-1046` |
| scrub decoders | built only for `DualVideoDecoder` sides, so none for live | `dual_playback_controller.cpp:283-298` |
| UI facts | `sideIsLive`, `liveSource(side)`, `hasClockedSide()` | `dual_playback_controller.cpp:911-930` |

### 1.2 How a live source becomes side A or B (verified)

There is no menu item and no shortcut for "make this side live". The routes are the ordinary dual routes, with live URLs
admitted:

- **A = the active item.** Any way of making a live item active (File ▸ Connect to After Effects / Premiere Pro, Connect to
  Blender, Open Stream…, double-click in the Live bin, a drop on the A chip — `ViewportOverlay.qml:322-333` →
  `openMediaPaths`, which routes `://` entries to `addLiveStream`, `window_manager.cpp:3151-3173`). Pressing a dual mode
  button then enters dual with that item as A: the path comes from the item because a live session never opens the
  single-flow decoder (`window_manager.cpp:2291-2297`).
- **A swapped while already in dual.** The load handler treats `LiveStream` as dual-capable and rebuilds dual around the
  new A, keeping B and B's track edits (`window_manager.cpp:1084-1092`, `:1160-1167`).
- **B = `setBSource(path)`.** A `://` path is accepted, added with `addLiveStream` (deduped by exact URL) and recorded as
  the project's B (`window_manager.cpp:5872-5896`). Reached from: a drop on the B chip (`ViewportOverlay.qml:401-424`),
  a drop on the right half / right of the seam (`dropSideAt` / `dropMediaAt` / `dropMediaOnSide`,
  `window_manager.cpp:3175-3216`), a drop on timeline lane B (`TimelinePanel.qml:49-60`, lanes at `:1770-1815`).
- **A bin row can be dragged** to any of those: the row's drag payload passes a live URL through as it is
  (`LeftRail.qml:725-729`), and the file-type gate admits anything containing `://` (`window_manager.cpp:3112`).
- **A saved Dual View item** (§1.9).
- The dual controls are hidden only for a playlist (`ViewportOverlay.qml:54-56`: "A live source can be either side of dual
  now … so only a playlist hides the pairing controls").
- A drop while in Single mode, or on Difference, always lands on A (`window_manager.cpp:3180-3184`); to put a live source
  on B the user enters a dual mode first or uses the B chip. The QCBridgeAE memory records the same from a walk
  ("enter side-by-side first — a drop in single mode always lands on A").
- Playlists never take a live item (`project_manager.cpp:398-406`, `:446-455`) and a playlist cannot be in dual.

### 1.3 What the other side may be (verified)

Anything dual accepts: a video, an image sequence, **another live source**, or nothing. `DualLiveSource`'s header says so
("One receiver per side: two live sides each own theirs, which is also what makes qcbae://ae on one side and srt:// on the
other work", `dual_live_source.h:22-23`). Audio-only media is refused as B (`window_manager.cpp:5877-5888`). An empty side
is allowed (commits a7050450 / d360dc25, 2026-09-23).

Not guarded: the SAME live item on both sides (see §6, item 5).

### 1.4 Time: what the transport does

Verified:

- **File + live:** the file side owns the clock. Master fps and length are the file side's alone
  (`dual_playback_controller.cpp:218-246`). Play, pause, step, seek, scrub, shuttle, review speed, in/out and loop all act
  on the file side only; the live side shows its newest frame regardless. There is **no sync and no offset** between a live
  side and the other side — nothing aligns them.
- **Pause:** the live side keeps updating (picture and audio). On Windows' render-on-demand loop the live source's
  frame-available callback wakes the renderer (`window_manager.cpp:2570-2576`; commit e5f15a23 made a paused dual with a
  live side keep feeding the output bus).
- **Scrub / shuttle:** no scrub decoder exists for a live side, so scrubbing the file side "leaves a live A alone"
  (`dual_live_source.h:19-20`).
- **Loop:** the file side's master range wraps; the live side is unaffected.
- **Live + live, or live + empty:** `hasClockedSide()` is false and a side is live, so `dualSeekable` is false
  (`window_manager.h:881-892`). QML then hides the timeline, the transport row and the timeline-status row
  (`Main.qml:901-910`, `:1587-1593`), and `TransportBar.hasMedia` is false (`TransportBar.qml:71-79`). The pump still ticks
  at 24 fps so the sides keep updating (`dual_playback_controller.cpp:215-217`).
- **Empty + empty** keeps the transport and both lanes (they are drop targets): `window_manager.h:884-891`.

What the timeline shows (verified): with one clocked side, the live side gets **a clip spanning the clocked side's
duration** so it does not read as past-end (`window_manager.cpp:2443-2457`; commit 664ef9b9: "A live side's timeline lane
spans the clocked side's duration"). Lane A's gutter letter becomes **"L"** when A is live (`TimelinePanel.qml:272`). With
two live sides both extents are 0 and the timeline is empty (and hidden).

### 1.5 Audio (verified)

- Only a `qcbae://` side has audio (Premiere's pushed audio segment): the mixer opens a `LiveAudioSource` for a host-bridge
  path; other live URLs (SRT) are silent (`dual_playback_controller.cpp:270-276`, `dual_audio_mixer.cpp:77-79`).
- It joins the mix like a file side, subject to three gates: the side's mute (A heard, B muted by default — the docs and
  `mp:` row 211), the global **Listen** setting `audio/livePlaydown` (default off — `window_manager.cpp:9794-9810`,
  `dual_audio_mixer.cpp:672`, `:738`), and master mute / volume.
- A live side is drained whether heard or not, and the audio device keeps running while any side is live, including while
  the transport is paused (`dual_audio_mixer.cpp:176-184`, `:247`, `:579`; header `dual_audio_mixer.h:141-146`).
- It is never treated as "in a gap", never seeked, never servoed (`dual_audio_mixer.cpp:281-291`, `:347-352`), gets no
  shuttle grain engine and keeps playing through a J / L hold (`:534-536`, `:661-678`) — three fixes from 2026-10-12
  (commits 74a24c7a, f8931e28).
- The Inspector's Audio card reads the inspected side's receiver in dual (`InspectorPanel.qml:1198-1206`) and carries the
  Listen switch (`:1262-1290`).
- The live A/V sync offset's negative half (hold the picture) is pushed to both sides' receivers when the setting changes
  (`window_manager.cpp:9840-9843`).
- The output bus's audio tap is the mixer's sum after per-side mutes, before master mute / volume
  (`dual_audio_mixer.h:132-139`; wired at `window_manager.cpp:2386`).

### 1.6 Colour per side (verified)

Each side renders through its own clip's chain (per-side OCIO, commit 3606bd83; docs/dualview.md "Color per side"). A live
item is a clip like any other: it has a Transfer override (`project_manager.cpp:1011-1034`) and its stream colour tags are
pushed onto its item per side as the receiver learns them (`window_manager.cpp:2577-2592`, `pushLiveTags` `:4673-4678`) —
session facts, not saved. A live side has no Range pill: `DualLiveSource` does not override `setRangeOverride`.

### 1.7 Drops, reconnects, raster / fps changes

Verified:

- **Feed stops** (After Effects loses focus, SRT drops): the last frame stays in the slot; the side holds its picture and
  never blanks (`dual_live_source.h:16-18`). Decided with the owner (memory, §2).
- **Reconnect** is the receiver's own business (the same `HostBridgeSource` / `LiveStreamDecoder` classes as single view);
  the island does nothing.
- **Status** is shown as a 7-px dot on the side's chip — red `#e5484d` when Live, amber `#e6a23c` otherwise — with the
  receiver's `statusDetail` as its tooltip, else "Live source" (`ViewportOverlay.qml:57-69`, `:182-194`, `:433-445`).
- **Windows:** a D3D11VA-decoded live frame goes zero-copy when it is on the renderer's device, else is read back and
  converted (`dual_live_source.cpp:186-206`, `:232-290`); before a58471aa such a side froze.

Inferred:

- **Raster change mid-session:** every published `DualFrame` carries its own width / height (`dual_live_source.cpp:150-152`)
  and `width()` / `height()` read the receiver live (`:131-132`), so the compositor follows the new raster frame by frame. I
  did not trace the compositor's per-frame layout to confirm there is no cached size.
- **fps change mid-session:** nothing in dual depends on a live side's rate (it is 0 by definition), so nothing happens.

### 1.8 Entering and leaving (verified)

- Entering dual stops the single-view live session, because the island opens its own receiver ("two readers of one ring,
  and frames published into a slot nothing fetches in dual" — `window_manager.cpp:2106-2111`). So the feed reconnects on
  the way in, and again on the way out (`rebuildSingleFlowFromActiveItem`, `:2159-2163`).
- In 2.x **every** change of B (set or clear) leaves and re-enters dual (`window_manager.cpp:5898-5915`, `:5920-5933`): the
  playhead, in/out and track edits start over, and a live A is closed and reopened each time.
- `liveActive` is false in dual, so the LiveStrip, LIVE / REVIEW and the locked-transport rule are all single-view only
  (`window_manager.cpp:4711-4724`; `Main.qml:912`).

### 1.9 Saving (verified)

- **Project:** a live item is a `LiveStream` media item whose `path` is the URL (`project_io.cpp:50`, `:63`); the project's B
  is `bSourceMediaId`, which accepts a live item since 664ef9b9.
- **Dual View item (`DualPair`):** `mediaIdA`, `mediaIdB`, master fps / duration, both tracks' clips, in / out
  (`window_manager.cpp:6096-6118`). Nothing live-specific is stored — the item ids are enough ("saved Dual Views store the
  URL and reconnect", memory §2). Default name "A vs B" from the items' names (`:6120-6127`).
- **Load:** A is made active (single view, which starts its live session), B is set by path (a URL goes back through
  `addLiveStream`, deduped), dual is entered, then the saved clips replace the auto-built tracks
  (`window_manager.cpp:6190-6243`). A pair without an A is refused.
- Deleting the item that is B clears B; deleting the active item leaves dual (`window_manager.cpp:6258-6275`).

### 1.10 Outputs, screenshots, notes (verified)

- **Outputs:** the output bus sends the composited canvas (or one side) — a live side is just part of it. Since e5f15a23 a
  live side's new frame counts as a new frame while the transport is paused.
- **Screenshots** capture the renderer's composite (`window_manager.cpp:6386-6394`); nothing live-specific.
- **Notes / annotations:** not available in dual at all (`window_manager.cpp:8230-8233`); a stream's notes and
  LIVE / REVIEW exist only for the single view's stream item (`:8247-8253`, `:4720-4724`). The docs say "freeze and annotate
  a feed in single view".
- **Scopes:** a live side is typed like a video for the waveform (`scope_controller.cpp:398`).

### 1.11 UI strings, verbatim, with their translation contexts

Context = the QML file's name (Qt's `qsTr` rule) or `qcv::WindowManager` for `tr()` in C++.

| Context | String | Translator comment | Where |
|---|---|---|---|
| `ViewportOverlay` | `Live source` | "Chip label for a live source" | `ViewportOverlay.qml:193`, `:444` |
| `ViewportOverlay` | `(drop a file…)` | — (A chip, empty) | `ViewportOverlay.qml:217` |
| `ViewportOverlay` | `(drop a video, sequence or stream…)` | — (B chip, empty) | `ViewportOverlay.qml:458` |
| `ViewportOverlay` | `Couldn't load that file as Source B` | — (toast) | `ViewportOverlay.qml:421` |
| `ViewportOverlay` | `A audio muted — click to unmute` / `A audio on — click to mute` | — | `ViewportOverlay.qml:313-314` |
| `ViewportOverlay` | `B audio muted — click to unmute` / `B audio on — click to mute` | — | `ViewportOverlay.qml:553-554` |
| `ViewportOverlay` | `Close source B` | — | `ViewportOverlay.qml:584` |
| `ViewportOverlay` | `(none)`, `Untitled Dual View` | — (save name parts) | `ViewportOverlay.qml:73-80` |
| `TimelinePanel` | `Drop media onto a lane, or a side of the viewport, to compare` | — | `TimelinePanel.qml:1379` |
| `InspectorPanel` | `Live` | "Media kind: a live stream or bridge feed" | `InspectorPanel.qml:117-118` |
| `InspectorPanel` | `State` | "Row: the bridge's current state" | `InspectorPanel.qml:1221` |
| `InspectorPanel` | `Off in the host` / `Idle` / `Pushing` / `Scrubbing` / `No audio` | "Bridge state: …" | `InspectorPanel.qml:1225-1233` |
| `InspectorPanel` | `Listen in QCView` | "Toggle: play the bridge's audio in QCView" | `InspectorPanel.qml:1274` |
| `InspectorPanel` | `Audio Stream` → `off: tick it in Premiere ▸ Preferences ▸ Playback` | "Row label for the bridge's audio" | `InspectorPanel.qml:1300-1301` |

The lane letter `"L"` (`TimelinePanel.qml:272`) is a bare literal, not a translated string. The chip's tooltip otherwise
shows the receiver's own `statusDetail` (the same strings the LiveStrip uses). No string exists that is specific to
"live in dual" beyond `Live source`. 3.0 already carries the two chip placeholders, the mute tooltips and the B toast
(`mp:apps/qcv/src/dual_controls.rs:141`, `:171`; `mp:apps/qcv/src/main.rs:441`); it already uses `Listen in QCView` in the
`InspectorPanel` context for the live row's speaker (`mp:apps/qcv/src/live_strip.rs:45`). `Live source` is not yet used.

---

## 2. The design reasons outside the code

### 2.1 Claude project memory (`~/.claude/projects/…-QCView-Player/memory/`)

`project_dual_live.md` (the feature's own note), quoted:

> "Built 2026-09-22 (branch `dual-live`, off main). Plan: `~/.claude/plans/witty-plotting-horizon.md`. The feature
> QCBridgeAE exists for: compare an AE/Premiere feed against a reference clip in SBS / wipe / difference."
>
> "**Decided with the owner:** live on either side or both; a stopped feed holds its last frame (never blanks); saved Dual
> Views store the URL and reconnect."
>
> "**Shape:** live+file → the file side keeps the clock; live+live → no clock, `dualSeekable` false, transport and
> timeline hidden."
>
> "What did NOT fit: `fps()==0` makes `translateMasterToSourceFrame` refuse (every pull null) and `frameCount()==0` makes
> a zero-length timeline clip, so the side reads past-end and renders transparent. Hence `IDualSource::isLive()` and three
> bypasses in the controller (master clock, translate, pastEnd), plus a live lane spanning the clocked side's duration."
>
> "**Verified:** … **the owner 2026-09-22: "working remarkably well"** with the real hosts. Merged to main (fast-forward)."

The plan file it names, `~/.claude/plans/witty-plotting-horizon.md`, **no longer exists** (`~/.claude/plans/` holds three
unrelated plans); the memory note and the commit messages are what is left of it.

`project_srt_live_stage2.md`, item 7 — why it was first deferred (2026-07-26):

> "`DualPlaybackController` is a synced-clock pump over two seekable sources; live A breaks it (free-running A +
> user-scrubbed B = compositor pairing, a third playback model). V1 ships with live items *blocked* from dual … bridge docs
> call this the killer feature …, so it's a deferral, not a kill."

`project_blender_beauty_bridge.md`: "Killer feature: dual view A=live stream vs B=approved render."

### 2.2 QCBridgeAE memory (`~/.claude/projects/…-QCBridgeAE/memory/`)

- `ab-follow-mode.md` — the boundary of the feature. Agreed 2026-09-21 as "its own project, after the full release": a B
  reference whose playhead FOLLOWS the host's. "It breaks QCView's clock model (dual view is a synced pump over two seekable
  sources; B would follow an external clock and must play, not seek per frame)". **So in 2.x the live side and the file
  side are deliberately unsynchronised; follow mode was never built.**
- `qcbridgeae-state.md` (2026-10-12): "dual view with a live side — pause/step/seek/speed leave the live side alone by
  design (DualAudioMixer keeps the device running while any side is live)", then the three audio fixes, confirmed by ear.
- `transmit-audio-direction.md`: "Dual with a live side passed (A live, B file; enter side-by-side first — a drop in
  single mode always lands on A)"; the dual Audio card binding the inspected side verified with side B live.
- `qcbridgen-output-plan.md` (2026-10-12): "live-A dual was frozen on the output while QCView was paused" → e5f15a23.
- `stream-annotations-plan.md`: "dual view has no annotations at all (stays that way)"; Review must end on any
  compositor-mode change or dual entry.

`~/.claude/plans/`: only `nifty-scribbling-walrus.md` mentions dual, and only as test matrix lines ("dual view while
streaming"; Review gone on "SBS / dual / playlist"). The QCBridge, QCview-BlenderAddon and QCView-Player-QT memory
directories hold nothing about a live side.

### 2.3 The three docs pages (2.x `docs/`)

- `docs/dualview.md:44-46` ("Live sides"): "A live source — a QCBridge feed from After Effects or Premiere Pro, or an SRT
  stream — can take either side. The chip shows the feed's status dot, and the live side has no clip on the timeline: it
  plays whenever the host sends, and keeps playing while the file side is paused, stepped, shuttled or seeked. Its audio
  joins the mix like a file's, with the same per-side mute and Listen rule. With a live source on both sides there is no
  clock to drive, so the transport and the timeline hide. Notes are not available in dual view; freeze and annotate a feed
  in single view."
- `docs/qcbridge-adobe.md:59`: "You can stream from AE and Premiere at the same time and between them, or use them in
  QCView Dual View review modes." and `:80`: "**Dual view** takes a feed on either side. It keeps playing, with its audio
  in the mix, while the file side is paused, stepped or shuttled."
- `docs/qcbridge-blender.md:112`: "In QCView the stream is a live media item: load it on one side of a Dual View against an
  approved render."

One mismatch between docs and code: the page says the live side "has no clip on the timeline"; the code gives it a
full-width clip (§1.4, §6 item 3).

---

## 3. What 3.0 has

### 3.1 Dual view's structure (verified)

- **One master clock, two follower transports.** `DualTransport` (`mp:crates/qcv-player/src/dual.rs:33-52`) holds
  `sides: [Option<Transport>; 2]` and `media: [Option<MediaInfo>; 2]`, one `PlaybackTimer`, and per-side mutes / gains. Each
  side is a full single-view `Transport` in follower mode (`set_side`, `dual.rs:63-86`). Sides are identified by
  `enum Side { A = 0, B = 1 }` (`dual.rs:16-17`).
- **Time.** Master rate = the higher side's, length = the longer side's (`retime`, `dual.rs:112-127`); each side shows
  `round(master seconds × its fps)` (`side_frame`, `dual.rs:357`). `tick()` returns `[SideUpdate; 2]` = `Same` / `New` /
  `Blank` (`dual.rs:19-20`, `:326-353`). With no side, `frames` is 0 and `play()` returns at once (`dual.rs:155-156`).
- **Presentation.** `min_present::dual::Dual` holds two whole side `Engine`s, `a` and `b`, plus the mode / seam / gain
  (`mp:crates/min-present/src/dual.rs:15-26`). The shell's tick shows each side's new frame on its engine:
  `eng.show(&h.frame)` or `eng.clear_frame()` (`mp:apps/qcv/src/main.rs:870-882`).
- **The edit model.** `qcv_timeline::Editor::load_dual(a, b, master_rate)` builds tracks A and B from `MediaInfo`
  (`mp:crates/qcv-timeline/src/lib.rs:211-216`); `ClipMediaKind` is `Video | ImageSequence | Audio` (`lib.rs:53`). The two
  12-px clip bars are `mp:apps/qcv/src/dual_bars.rs`.
- **Controls.** The chips, modes, seam / gain slider and Save sit on the right of the status strip (row 173;
  `mp:apps/qcv/src/dual_controls.rs`, synced at `mp:apps/qcv/src/main.rs:898-910`).
- **Audio.** Each side has its own `AudioPlayer`, gained by its mute and by "has a picture here"
  (`main.rs:404-406`; `dual.rs:345-347`).
- **Project.** `b_source_media_id` accepts any existing item id (`mp:crates/qcv-project/src/manager.rs:271`) — it does not
  reject a live item. `DualPairData` is QCView's shape (`mp:crates/qcv-project/src/model.rs:38-40`, `io.rs:139-143`).

### 3.2 A live item in single view (verified)

- `App.live: Option<live::Live>` (`mp:apps/qcv/src/main.rs:128`) holds a `Feed` — `Host(HostFeed)` or
  `Stream(StreamFeed)` (`mp:apps/qcv/src/live.rs:10-24`) — the item id, Review state and, for a host, an `AudioPlayer`
  opened on the host's audio segment (`live.rs:53-56`, `:66-84`).
- `load_item` routes a `LiveStream` to `start_live` (`main.rs:232`), which drops the file player (`live.rs:68`).
- Per render tick `live_tick` takes the feed's newest frame and calls `g.presenter.engine.show(&f)` on the MAIN engine
  (`live.rs:103-121`). The feed "lives exactly as long as its item is the loaded one" (`live.rs:1-3`, `:106`).
- The live row replaces the status strip, the transport and the scrubber (`main.rs:922-925`, `:1086-1101`); transport keys
  are locked, Space freezes (`main.rs:2391-2396`).
- **A feed's frame is an `Arc<min_present::Frame>` shown with `Engine::show` — the same call a dual side engine takes**
  (`live.rs:12`, `:113-116` vs `main.rs:877`). That is the reason this port is much smaller than 2.x's: there is no
  frame-kind conversion to write.

### 3.3 What blocks a live side today (verified)

1. **The rule itself.** `dual_media` returns `None` for anything but `Video` / `ImageSequence`
   (`mp:apps/qcv/src/main.rs:379-391`). Consequences: `set_b_source` toasts "Couldn't load that file as Source B" for a live
   item (`main.rs:437-441`); a live A enters dual as an EMPTY A (`main.rs:399`); in dual, loading a live item leaves dual
   (`main.rs:216-226`); a saved Dual View with a live side loads that side empty (`main.rs:476`).
2. **No way in from a live item.** Dual's mode buttons live on the status strip (row 173), and the live row replaces the
   strip (`main.rs:922-925`). There is no menu item or shortcut for the compare modes (`set_compare_mode` is called only
   from `dual_cmd`, `enter_dual` and `load_dual_view`: `main.rs:376`, `:480-483`, `:723`). With a live item loaded the user
   cannot reach the buttons.
3. **`set_compare_mode` does not know about `self.live`** (`main.rs:394-422`). If dual were entered with a live item
   loaded, `loaded_item` is unchanged, so `live_tick` would keep painting the main engine and the live row would stay up
   (inferred — unreachable today because of 2).
4. **`DualTransport` has only clocked sides.** A side is `Option<Transport>`; rate, length, `tick`, gains and `Blank` all
   come from a `Transport` (`dual.rs:33-52`, `:112-127`, `:326-353`). There is no "present, free-running" side.
5. **"Is there a side?" is asked of the transport in many places**, each of which would call a live side absent:
   `d.side(B).is_some()` for the output's "has B" (`mp:apps/qcv/src/outputs.rs:180`, `:321`), `d.media(B).is_some()` for the
   B chip's name (`main.rs:901`), `d.side(..).is_some()` for the colour slot being shown (`mp:apps/qcv/src/color.rs:214`),
   `side_has_audio` for the mute pills (`main.rs:907`), `apply_item_to_side` (`main.rs:624-630`).
6. **The timeline model has no live clip.** `MediaInfo` needs a rate and a duration; `ClipMediaKind` has no live kind
   (`mp:crates/qcv-timeline/src/lib.rs:53`, `:169-172`).
7. **The `Playback` trait** (`mp:crates/qcv-player/src/playback.rs:10-32`) is what the transport bar and strip read; it is
   implemented for `Transport` and the dual transport. With two live sides nothing sensible can answer it (frame count 0);
   the bar would have to hide, as 2.x does.
8. **Listen has no home in dual.** Row 231 put Listen on the live row only and did NOT carry the Inspector's switch. In a
   file + live dual the live row is not shown, so a Premiere side's audio could not be turned on.
9. **Live audio is single-view state.** The host's `AudioPlayer` and the global buffer / sync settings are set up in
   `start_live` (`live.rs:66-84`); dual's per-side players are opened by the side's `Transport`.
10. **Single-view-only live features** hang off `self.live`: the Inspector's live cards (`main.rs:1036-1042`), the project
    panel's connection dot (`main.rs:1097-1098`, `mp:apps/qcv/src/project_panel.rs:333-334`), the Blender card's state
    (`mp:apps/qcv/src/blender.rs:143-159`), the output's source size and transport facts
    (`mp:apps/qcv/src/outputs.rs:271`, `:389`), the output audio tap (`outputs.rs:160`, `:403`).
11. **OS drops of a URL** are filtered out: dropped URIs go through `file_url_to_path` (`main.rs:2468`), so an `srt://`
    link dropped from another app never reaches a side (2.x admits anything with `://`).

Nothing blocks at the presenter (a side engine is a whole `Engine`), the project model, or the feeds themselves.

### 3.4 Canon rows that constrain the design (`mp:docs/rust-wgpu-port.md`)

Dual view:

- **150** — the compare modes live in min-present's composite stage; "the live-status wire contract (append-only numbers)
  in the shell".
- **167 / 212** — no edit mode; two 12-px clip bars; a TARGET side (last clicked chip or bar, B by default) takes the edit
  keys; snapping; the timeline grows left; the A-vs-B offset reads in the status strip.
- **173** — the dual controls sit on the right of the status strip; chips with QCView's strings; hidden for a playlist;
  Save is a dialog; mode / seam / gain reset per launch.
- **205** — one whole colour chain per clip; in dual each side is a clip with its own chain.
- **209** — the waveform overlays A and B, each through its own clip's Input.
- **211** — the playback model; (c) changing a side replaces that side only, keeping playhead, in/out and the other side's
  edits; **(d) "live sources wait for the live port"**; step 5 audio: A heard, B muted by default, silence where a side has
  no picture, `audio/dualSyncOffsetMs`.
- **213** — the colour drawer AND the Inspector follow the target side; no A / B tabs.
- **215** — annotations in single view only ("dual revisited after the walk").
- **216** — dual captures at source resolution (side-by-side = two native pictures; wipe / difference = one).
- **237** — outputs: the composite or one dual side; publish-on-change with a stamp; "B refused when absent"; the audio
  tap before Listen / mute / volume.
- **254** — "a live source as a dual-view side (row 211's deferral, never planned)" ruled back into the plan, "to be
  specified from QCView's code before it is built". The same row says the A / B level mixer is to be checked against 2.x
  first (aside: `src/dual/dual_audio_mixer.h` has per-side mutes and a master volume; I found no per-side level setter by
  name — not searched further, it is outside this spec).

Live sources:

- **227** — the QCBridge round and its order. **228** — the host feed, the LiveStrip, locked transport keys, seven QCView
  bugs fixed. **230** — the Auto transfer label for a host feed. **231** — **Listen lives on the live row only; the
  Inspector's switch is not carried.** **233** — LIVE / REVIEW and stream notes (Review holds the float frame via
  `Engine::hold_shown`). **234** — network streams (`StreamFeed`), passphrase hidden everywhere. **235** — the Blender
  input: listens only while Connect to Blender is on; one item per session. **243** — a listener URL the user opens listens
  without the Blender switch (session-scoped). **244** — the Stream card removed; the SRT INPUT untouched.
- **101–108 / §20** — the frozen bridge contracts (the QCAE ring's consumer rules bear on whether one ring can feed two
  sides — not checked here; see §4 hard parts).

---

## 4. Proposed build outline

Size: **medium — about six walkable steps**, most of it shell wiring. No new decoder, no new frame path, no shader work.
The model change is one new side kind in `DualTransport`; the rest is making every "is there a side / what is its name /
does it have audio" question answer correctly for a live side. Compare 2.x, where the island needed a new source class and
per-platform frame conversions.

**Proposed shape** (a design choice, listed as ruling R1 because it sets the size): `DualTransport` learns a third side
state — present, live, unclocked — carrying the item's `MediaInfo` but no `Transport`. The shell owns the feeds (as it
does in single view), one per live side, and shows their frames on `ed.a` / `ed.b` each tick. The feed type moves from
`App.live` to something both single view and a dual side can hold.

### Step 1 — the side kind and the picture (file + live, no audio, no UI polish)

- `qcv-player::dual`: a live side marker (excluded from `retime`; never `Blank`; `seek` / `scrub` / `play` skip it;
  `side_frame_of` none). 2.x's rules from §1.1.
- Shell: `dual_media` (or a sibling) admits `LiveStream`; a per-side feed slot; the dual tick shows a live side's newest
  frame on its engine; entering dual with a live item loaded MOVES the running feed into side A (no reconnect) and leaving
  moves it back.
- `set_b_source`, `load_item` in dual, `load_dual_view` accept a live item; clearing a side drops its feed.
- A way in from a live item (ruling R2).
- Walk: After Effects on A against a file on B in all three modes; play, pause, scrub, loop the file; lose focus in AE
  (the side holds); B = live against a file A.

### Step 2 — the chips, the bars and the transport

- Chip: the status dot (red live / amber otherwise), tooltip = the feed's detail else `Live source` (context
  `ViewportOverlay`); the B chip's name for a live B.
- Clip bars: what a live side's bar shows (ruling R4); edits refused on it.
- Live + live and live + empty: the transport, scrubber and bars hide (ruling R5).
- The status strip's A-vs-B offset text has nothing to say for a live side (row 212) — it stays empty.

### Step 3 — audio

- A host side opens its audio segment on that side's `AudioPlayer`; per-side mute, Listen, master mute / volume; it keeps
  playing while the master is paused, scrubbed or shuttled; never gain-zeroed as "no picture here".
- Where Listen lives in dual (ruling R6).
- Walk needs Premiere with Audio Stream ticked.

### Step 4 — colour, Inspector, scopes

- The side's item drives its engine's Transfer override and chain (`apply_item_to_side`); a stream's wire tags pushed onto
  its item per side; the Auto label.
- Inspector follows the target side (row 213): the live cards for a live side.
- The waveform reads the side like any other (row 209).

### Step 5 — saved Dual Views, project round-trip, Blender

- Save / Update / load a Dual View with a live side (ruling R7 on what the item stores for the live track).
- Reopen a project whose B is live.
- The Blender session's item as a side: the Connect to Blender gate, the card's state line, session end (ruling R8).

### Step 6 — outputs, captures, the edges

- Outputs: "has B" for a live B; the publish stamp must move when a live side shows a new frame while the master is paused
  (2.x's e5f15a23); source size and transport facts in dual.
- Screenshot of a dual with a live side (row 216: source resolution).
- Same item on both sides (ruling R9); deleting the live item while it is a side; New Project / open project while in a
  live dual; a raster change mid-session; the Windows walk.

### The hard parts, honestly

1. **One feed, two owners.** Moving a running feed between the single view and a dual side without reconnecting is new
   (2.x reconnects every time, and on every B change). `live_tick`'s rule "the feed dies when its item is not the loaded
   one" (`mp:apps/qcv/src/live.rs:106`) has to become "…or a dual side". Most single-view live code reads `self.live`
   directly (about 40 sites in `main.rs`, plus `outputs.rs`, `blender.rs`, `color.rs`); each needs a decision.
2. **The many `side(..).is_some()` readers** (§3.3 item 5). Each is a small change; missing one produces a quiet wrong
   answer (an output that refuses B, a mute pill that never appears, a colour slot treated as hidden).
3. **Audio while the master is paused.** 3.0's dual audio is "each side's Transport drives its own player on its drift
   loop" (row 211 step 5). A live side has no Transport and must play regardless of the master; the "silence where a side
   has no picture" gain rule must not apply to it. The live buffer / sync offset are process-wide globals
   (`audio::live::set_global_*`, `live.rs:72-74`) — fine for one host side, undefined for two host sides with different
   per-host offsets.
4. **Output publish-on-change in dual.** The stamp hashes the MAIN engine's `frames_shown`
   (`mp:apps/qcv/src/outputs.rs:280`), and in dual the frames are shown on the side engines; `output_transport` and
   `output_source_size` return zeros in dual (`outputs.rs:271-274`, `:389-397`). **By reading alone, this looks like dual
   outputs may not follow new frames at all today — I did not run it.** It must be checked before step 6, and it is
   exactly where 2.x had its paused-live bug.
5. **The same ring twice.** Whether two consumers may read one QCAE ring (same host on both sides) I did not establish in
   `qcv-ring`; a `mode=listener` SRT URL certainly cannot be bound twice. Hence ruling R9.
6. **Review / freeze in dual** is out (2.x and row 215), but Space is the freeze key on a live item and the play key in
   dual: in a file + live dual Space must be the file side's play / pause. Stated here so nobody carries the lock by
   accident.
7. **Windows.** A stream side decoded on the hardware path goes to a side engine instead of the main one; 2.x needed two
   fixes here (a58471aa, 30ce1248). 3.0's side engines take the same `Frame`, so it should just work — but it needs the
   box, with a synthetic sender made there.

---

## 5. Things in 2.x that look like bugs or loose ends (for the owner to rule on; none is silently fixed above)

1. **Two live sides leave no live controls at all.** The transport and timeline hide (`Main.qml:901-910`) and the LiveStrip
   does not appear, because it is bound to `liveActive`, which is false in dual (`Main.qml:912`;
   `window_manager.cpp:4708-4713`). The header comment says the opposite ("the LiveStrip replaces the transport and
   timeline", `window_manager.h:882-884`). So in live + live there is no Transfer chip and no screenshot buttons on a row;
   Listen is only in the Inspector. (Verified in code; the on-screen result is inferred.)
2. **Only lane A is marked.** The gutter letter is "L" for a live A (`TimelinePanel.qml:272`); there is no equivalent for
   B — a live B's lane reads "B". (Verified: `dualLiveB` does not appear in `TimelinePanel.qml`.)
3. **The live lane is an ordinary clip.** It is built by the same call as a file's with the other side's duration
   (`window_manager.cpp:2456-2466`), so it can be trimmed, slipped, cut and deleted with no effect on the picture (a live
   side is never past-end: `dual_playback_controller.cpp:1000`), and its clip list is saved into the Dual View item
   (`window_manager.cpp:6112-6115`) with whatever length the file side had that day. The docs say the live side "has no
   clip on the timeline". (Inferred from the code path; not run.)
4. **The Blender gate is bypassed in dual.** Single view refuses to open a `mode=listener` item unless Connect to Blender
   is on (`window_manager.cpp:4581-4589`); `DualLiveSource::open` opens the socket directly (`dual_live_source.cpp:43-70`).
   And the Blender card's state reads the single-view receiver (`window_manager.cpp:3276-3279`), which is null in dual, so
   a Blender stream running as a dual side would show "card", not "live". (Inferred.)
5. **The same live item on both sides is not refused** (`setBSource`, `window_manager.cpp:5862-5918`): two receivers on one
   ring or one listener port — the very thing `teardownSingleFlowForDual`'s comment says to avoid
   (`window_manager.cpp:2108-2110`). Outcome not known.
6. **The live A/V offset in dual.** The per-host settings key is derived from the single-view receiver's URL
   (`window_manager.cpp:9814-9829`), null in dual, so dual always reads and writes the "premiere" key. And the picture-hold
   half is applied to dual's receivers only inside the setter (`:9840-9843`) — I found no call that applies the stored
   value when dual is entered, so a negative offset would not hold a dual live side's picture until the setting is touched.
   (Verified by search; effect inferred.)
7. **Every B change reconnects a live A** and resets playhead, in/out and edits (`window_manager.cpp:5898-5915`). 3.0's row
   211 (c) already replaces a side in place, so this one is already ruled.
8. **Loading a saved Dual View with a live A connects twice**: A is first made active in single view (starting its
   receiver), then dual entry tears that down and the island opens its own (`window_manager.cpp:6211-6222`). (Inferred.)
9. **Docs vs code:** "the live side has no clip on the timeline" (docs/dualview.md:46) vs item 3.

Found on the way in 3.0 (not 2.x): hard part 4 above (the dual output stamp), and blocker 11 in §3.3 (a dropped URL is
filtered before it can reach a side).

---

## 6. Rulings needed

Each with a [proposed] default and its reason.

- **R1 — Architecture.** [proposed] A live side is a third side state inside `DualTransport` (present, unclocked), with
  the shell owning one feed per live side. *Reason: it keeps 2.x's rules in one place and makes every "is there a side"
  reader answer truthfully.*
- **R2 — The way in from a live item.** [proposed] While a live item is loaded, the live row carries the same A / B chips
  and mode buttons the status strip does. *Reason: row 173 put them on a strip the live row replaces; 2.x showed them for a
  stream.* (Alternative: View-menu items for the four modes.)
- **R3 — Which sides and pairs.** [proposed] As 2.x: either side, live + file, live + sequence, live + live, live + empty.
  *Reason: QCView wins ties; the docs promise "either side".*
- **R4 — What a live side's clip bar shows.** [proposed] A full-width, non-editable bar in the side's colour labelled
  LIVE, with no trim / slip / cut / delete and nothing saved for it. *Reason: 2.x's editable full-width clip does nothing
  and contradicts its own docs (§5 item 3) — a written divergence.*
- **R5 — Two live sides (or live + empty).** [proposed] The transport, scrubber and bars hide (2.x), and one live row
  stands in their place with the dual controls, Listen and the screenshot buttons. *Reason: 2.x leaves no controls at all
  there (§5 item 1); the row is what its own header comment intended.*
- **R6 — Listen in a file + live dual.** [proposed] The live side's chip speaker pill is the only control: unmuting a live
  side's pill makes it heard; `audio/livePlaydown` is not consulted in dual. *Reason: row 231 wanted Listen in one place
  and the live row is not on screen here; two gates (pill AND Listen) for one sound is the "two places" the owner
  disliked.* (Alternative, closer to 2.x: both gates, with a Listen speaker added to the dual controls.)
- **R7 — B's default mute for a live side.** [proposed] Unchanged: A heard, B muted by default, live or not. *Reason:
  row 211 step 5 and 2.x.*
- **R8 — Entering and leaving dual with a live item.** [proposed] The running feed moves into the side and back with no
  reconnect; changing the other side never touches it. *Reason: row 211 (c) already ruled in-place side changes; a
  reconnect costs seconds on SRT and a Blender replica must call again.*
- **R9 — The same live item on both sides.** [proposed] Refused with a toast (a new string). *Reason: 2.x does not guard
  it and two readers of one ring / one listener port is undefined; comparing a feed with itself has no use.*
- **R10 — The Blender session as a side.** [proposed] The Connect to Blender gate (rows 235 / 243) applies to a side
  exactly as in single view, and the card's status follows the feed wherever it is shown. *Reason: 2.x bypasses its own
  gate in dual (§5 item 4).*
- **R11 — Saved Dual Views.** [proposed] Byte-for-byte QCView's item (`mediaIdA` / `mediaIdB`, …); for a live side the
  clip list is written EMPTY and ignored on load. *Reason: the `.qcvproj` contract (row 141) holds, and 2.x reading an
  empty list rebuilds its own lane.* (To check before building: that 2.x does load a pair whose live track has no clips —
  `loadDualView` only replaces a track when the saved list is non-empty, `window_manager.cpp:6227-6232`, so it should.)
- **R12 — Notes, annotations, LIVE / REVIEW in dual.** [proposed] None, as 2.x and row 215; Space is play / pause for the
  file side; with two live sides Space does nothing. *Reason: "freeze and annotate a feed in single view" (2.x docs).*
- **R13 — A stopped feed.** [proposed] The side holds its last picture and the chip's dot goes amber; it never blanks.
  *Reason: decided with the owner for 2.x ("a stopped feed holds its last frame (never blanks)").*
- **R14 — Sync between the live side and the file side.** [proposed] None — the two are independent, as in 2.x; "follow
  mode" stays the separate future project the QCBridgeAE memory describes. *Reason: it changes the clock model and was
  explicitly kept out.*
- **R15 — The live A/V offset in dual.** [proposed] Each live side uses its own host's stored offset, applied when the
  side opens. *Reason: 2.x always uses the Premiere key in dual and applies the hold late (§5 item 6).*
- **R16 — The 2.x loose ends in §5** not covered above (items 2 and 8 disappear with R4 and R8; item 9 is a docs line).
  [proposed] Fix in 3.0 as listed; leave 2.x alone. *Reason: 2.x is the shipped line; the port's rule is to list, then fix
  on the owner's word.*
- **R17 — The dual output stamp** (hard part 4). [proposed] Check it on a release build before step 6 and, if dual
  outputs do go stale, fix it as its own small change first. *Reason: it is independent of live sides and would mask
  their walk.*
- **R18 — Dropped URLs.** [proposed] An `srt://` (or other `scheme://`) link dropped on a chip or a side is admitted, as
  2.x does. *Reason: QCView wins ties; today it is filtered silently.*

New strings this would add (each needs 11-language drafts): `Live source` (existing 2.x string, context
`ViewportOverlay`), the LIVE bar label if R4 stands, and the refusal toast if R9 stands.

---

## 8. Progress (the build, from 2026-11-10)

| Step | What | State |
|---|---|---|
| 1a | `qcv-player::dual`: the live side — present, unclocked (`set_live_side`, `is_live`, `present`, `has_clocked_side`); out of the master's rate and length, never Blank, passed by seek / play | Built, unit-tested (a live side beside a file, alone, replaced by a file) |
| 1b | The shell: `live.rs` makes a feed apart from adopting it (`make_live` / `adopt_live`); `App.side_live` holds one feed per live side; `set_dual_side` puts a file OR a live item on a side in place — a running feed MOVES in from the single view and back out on leaving dual (R8, no reconnect); the same live item on both sides is refused with a toast (R9, one new string); a live item loaded while in dual becomes A in place; `set_b_source` takes a live item; each live side's newest frame is shown on its side's engine every tick; a live side has no clip on the lane (R4's LIVE bar is step 2); the outputs' "has B" and the colour slots ask `present()` | Built. Checked in an isolated run with the synthetic host (`probe`) on A against a file on B: one connection, the live side showing 24 new frames a second beside the file (`QCV_TRACE_DUAL=1` prints `dual live: shown A n B n`). **Walked by the user 2026-11-10 (After Effects as a side): "this works great".** |
| 1c | The way in from a live item (R2): while a live item is loaded the dual controls — the A / B chips and the mode buttons — sit on the live row, before its Transfer chip; a mode button enters dual with the running feed as A | Built (`live_strip::controls_left`, the controls laid out and painted on the live row). **Walked 2026-11-10 ("that works") after one fix: the live row swallowed the controls' presses** |
| 2a | The lane of a live side (R4): one bar across the lane in the side's wash, labelled LIVE; no clip, no edit, nothing saved | Built. **Walk owed** |
| 2b | A live side's chip: the dot (red while frames arrive, the notice colour otherwise) and, in its tooltip, the feed's line (its detail while not live, else `Live source`) | Built. **Walked 2026-11-10 ("this all works")** |
| 2c | Live + live and live + empty (R5): with no clocked side the transport, the scrubber and the lanes hide and the live row stands in — fed by the TARGET side's feed (else the other live side), with the dual controls on it; its Transfer chip acts on that side | Built. **Walk owed** (two hosts at once, or a live A with no B) |
| 3 | Audio (R6 / R7): a host side's player (its `LiveAudioSource` on the host's audio segment) is heard by its SIDE's mute alone; **a live side starts MUTED on either side** (the user 2026-11-10, after walking it: "lets make live feeds muted by default because there is a good chance it will be a echo situation" — R7's "A heard" reversed for live sides; a file that replaces one gets the side's own default back); the Listen setting is not consulted in dual — and keeps playing whatever the master does; the chip's speaker pill appears once the segment carries sound; master volume / mute and the Inspector's Mix reach it. A stream side is silent (QCView) | Built; **walked by the user 2026-11-10 with After Effects and Premiere ("awesome. it works")**; the log of that walk shows three `LiveAudioSource` underruns in twelve seconds of Premiere audio — asked about, not yet answered. Was: walk owed, Premiere with Audio Stream ticked (or the synthetic `probe premiere …` host, which beeps). Open: the live A/V offset is one process-wide value — two host sides share it (the spec's hard part 3); the outputs' audio tap does not yet take a live side's sound (step 6) |
| 4 | Colour, Inspector, scopes: a live side's item drives its engine's Transfer override and its chain like any side's (`apply_item_to_side`, the colour slots by `present()`); a stream side's wire tags are pushed onto its item every tick; the Inspector shows the live cards when the TARGET side is live; the waveform reads the side's engine | Built — two small changes, the rest fell out of steps 1–3. **Walked 2026-11-10 ("it works")** |
| 5–6 | Saved Dual Views / Blender; outputs / captures / edges | Not started |

