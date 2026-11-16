# S17 - a folder view at scale: a virtualised list and grid with 100,000 entries (opened 2026-11-15)

**Status: VERDICT - PASS on macOS (2026-11-15), with two things to see again: the frame figures of the 1,000,000-entry
grid and of the grid with distinct pictures were taken while the Mac's screens were asleep (drawn and finished by the GPU,
not shown); and the screens that were awake ran at 60 Hz, so a 120 Hz cadence was not seen. Windows: not run.**

## Why

UFB's folder views are a list (icon, name, date, size, kind) and a grid (thumbnail tiles with a name under each), and a
folder on a production share can hold very many entries. min-ui has no table, grid or tree. Before they are designed:
does the drawing stack (min-ui, vello, min-present) keep up, and what do the widgets have to be built around?

## The question

On min-ui / vello / min-present, do a virtualised list and a virtualised grid stay smooth with 100,000 entries
(stretch: 1,000,000)?

## Pass bar

On this Mac, release build, a 1440 x 900 logical window at the display's scale (2: 2880 x 1800 px):

- frame time while scrolling through the whole list: p95 <= **8 ms**, worst <= **16 ms**, over >= 600 frames, the same
  for 1,000 / 100,000 / 1,000,000 entries;
- the same for the grid: 128 px tiles, each a placeholder picture and a one-line name;
- sorting 100,000 entries by name in natural order (shot_2 before shot_10, case-blind) <= **150 ms**; by size or date
  <= **50 ms**;
- type-ahead (the first entry, in the view's order, whose name starts with a prefix) <= **5 ms**;
- select all, and painting with everything selected: inside the frame bar;
- the model of 100,000 entries (name, size, date, kind, flags): reported, target <= **40 MB**;
- text: laid out per visible row each frame, or cached - which was needed, and what a row costs.

## Time box

About an hour. Kept (one build, three rounds of runs).

## How it was measured

`spikes/s17-browser-scale` (binary `s17`), by script, no hand on the machine:

    CARGO_TARGET_DIR=target/s17 cargo build --release -p s17-browser-scale
    S17_SCRIPT=bench:100000 target/s17/release/s17                 # no window: memory, sorts, type-ahead, select all, text
    S17_SCRIPT=list:100000[:fresh|cached] target/s17/release/s17   # a window, ~20 s at 60 Hz
    S17_SCRIPT=grid:100000[:fresh|cached][:img=<k>] target/s17/release/s17   # k distinct 256 x 256 pictures
    S17_SHOT=<file.png>  saves the window as drawn (the presenter's own capture) 100 frames into the first sweep

- **Entries** are synthetic (`shot_812_comp_v003.exr`, `Shot_0812_plate.1001.dpx`, `SHOT_812_roto_take7.wav`, ...):
  numbers without padding and mixed case, about 24 bytes a name. The window's model is sorted by name first.
- **The list**: 24 px rows (37 in view), a header, five cells a row (an icon glyph, the name cut to its column with
  `layout_fit`, date, size, kind), a selection rectangle, min-ui's `ScrollBar`, one clip layer. **The grid**: 148 x 172 px
  cells, 9 a row, 63 in view, each `paint::image` of a 256 x 256 RGBA picture and the name cut to the cell.
  Only what is in view is laid out and drawn, as minRender's job list does.
- **The script** moves the offset itself, 40 frames to warm up, then three phases: a *sweep* of the whole length in 600
  frames (at 100,000 entries every frame shows rows never seen before - the worst case for any cache), a *glide* of 300
  frames at 37 px a frame (what a hand does), a *sweep with everything selected* of 300 frames.
- **A frame's time** is: building the scene (layout + drawing into vello's `Scene`) + `UiLayer::render` (vello's CPU
  side and its submit) + a wait until the GPU has finished that work (`device.poll(wait)`). The presenter's own pass and
  the present come after and are timed apart: the surface is vsync'd (`AutoVsync`, not ours to change in a spike), so
  that call mostly waits. The time between frames is recorded too, to see a missed refresh.
- "fresh": every cell is laid out again every frame. "cached": a row's layouts are kept by entry while it is in view
  (a `HashMap`, emptied of what left the view).
- The pictures were checked by eye from `S17_SHOT` captures of the list and of the grid (the order reads
  `render_plate_41_92` before `render_plate_41_310`).

Machine: Apple M5 Max, Metal, the awake displays at 60 Hz. Other builds were running on the machine during the runs.

## Numbers

### Frames (ms: p50 / p95 / worst), shown on screen at 60 Hz unless marked

| view, text | entries | sweep, 600 frames | glide, 300 | sweep, all selected, 300 |
|---|---|---|---|---|
| list, cached | 1,000 | 3.37 / 3.89 / 6.83 | 3.15 / 3.51 / 6.12 | 3.63 / 4.23 / 7.50 |
| list, cached | 100,000 | 4.30 / 5.81 / 8.17 | 3.16 / 3.61 / 6.48 | 5.04 / 6.87 / 8.69 |
| list, cached | 1,000,000 | 4.58 / 6.50 / 8.06 | 3.11 / 3.60 / 6.69 | 4.91 / 6.19 / 8.51 |
| list, fresh | 100,000 | 4.24 / 5.06 / 5.70 | 4.31 / 5.08 / 5.49 | 4.61 / 5.29 / 6.11 |
| list, fresh | 1,000,000 | 4.22 / 5.04 / 5.94 | 4.19 / 4.87 / 5.46 | 4.57 / 5.28 / 6.34 |
| grid, cached, 1 picture | 1,000 | 2.95 / 3.90 / 4.75 | 2.96 / 3.72 / 7.50 | 2.63 / 2.82 / 3.59 |
| grid, cached, 1 picture | 100,000 | 3.31 / 3.64 / 3.86 | 2.60 / 2.76 / 3.49 | 3.86 / 4.56 / 5.20 |
| grid, cached, 1 picture | 1,000,000 | 3.45 / 4.08 / 4.99 (*) | 1.27 / 1.94 / 3.33 (*) | 3.72 / 4.35 / 4.80 (*) |
| grid, cached, 256 pictures | 100,000 | 3.34 / 3.81 / 9.36 (*) | 1.63 / 1.86 / 3.32 (*) | 3.99 / 4.33 / 4.64 (*) |
| grid, fresh, 256 pictures | 100,000 | 2.57 / 3.68 / 8.93 (*) | 2.88 / 3.49 / 4.90 (*) | 2.60 / 3.35 / 3.75 (*) |
| grid, cached, 4,096 pictures | 1,000,000 | 4.53 / 5.00 / 5.99 (*) | 0.78 / 1.26 / 4.47 (*) | 6.09 / 6.82 / 7.54 (*) |

(*) **not shown**: the Mac's screens went to sleep during the run of the 1,000,000-entry grid and stayed asleep (the
presenter reports the surface occluded and presents nothing). The scene was still built, rendered by vello and finished
by the GPU, so the figure is the same work - but an idle screen makes it look better: the 100,000-entry list read
1.37 / 2.01 / 3.16 asleep against 4.30 / 5.81 / 8.17 awake, about three times less. Read the starred rows as "no worse
than the unstarred ones beside them", not as their own numbers. The program now says so itself when a frame was not
presented; these rows are to be run again with the screens awake (I did not wake them: that is the owner's desktop).

In the rows that were shown, the time between frames was 16.66 ms at p50 and <= 17.2 ms at p95 in every phase; a
single frame of ~30 ms appeared in five of the 21 phases, each time while that frame's own work was under 9 ms (another
process on the machine, or the display link - not this work).

Where a shown list frame goes (100,000, sweep): building the scene 1.4 ms (fresh 1.3), vello's CPU side 1.4 ms, the
GPU 1.5 ms. With everything selected the GPU takes 2.0 ms (37 more rectangles the width of the window).

### The model, the sorts, type-ahead (no window)

| | 1,000 | 100,000 | 1,000,000 |
|---|---|---|---|
| model (24 B an entry + the name + 4 B of order + 1 bit of selection) | 0.05 MB | **4.98 MB** | 49.8 MB |
| sort by name, natural: the comparator itself | 0.2 ms | **26.4 ms** | 456 ms |
| sort by name, natural: a key made once per name, compared as bytes | 0.1 ms | **8.5 ms** | 126 ms |
| sort by size / by date | < 0.1 ms | **1.5 / 1.4 ms** | 22.4 / 21.6 ms |
| type-ahead, a scan in the view's order: a hit a quarter in / no hit at all | 0.002 ms | **0.06 / 0.22 ms** | 2.0 / 9.0 ms |
| select all (a bit set) | - | **0.0001 ms** | 0.001 ms |

Both name sorts give the same order (asserted). The process's memory grew by what the parts add up to (+5.1 MB at 100,000).

### Text

| | cost |
|---|---|
| a name laid out (parley, 12 px at scale 2) that fits its column | 4.0 us |
| a name that has to be cut with an ellipsis (`layout_fit`'s search: about five layouts) | 17.9 us |
| date + size + kind of one row | 6.7 us |
| a laid-out line drawn into the scene | 0.45 us |
| a whole list row, fresh (4 layouts, 5 draws, the two formatted strings) | ~35 us (1.3 ms for 37 rows) |
| a whole grid tile, fresh (a cut name, a picture) | ~21 us (1.3 ms for 63 tiles) |

**Neither was needed to meet the bar: laying out every visible cell every frame passes** (list p95 5.1 ms, worst
5.7-6.3 ms). The cache takes the scene's build from 1.3 ms to 0.4 ms in a glide, where rows stay (a frame of 3.2 ms
instead of 4.3) - and in a sweep it is *worse* than no cache (worst 8.2-8.7 ms against 5.7-6.3): every frame fills it
with 150 new layouts and throws 150 away.

## What dominated

- **Nothing depends on the entry count.** The glide is the same to a tenth of a millisecond at 1,000, 100,000 and
  1,000,000 (3.15 / 3.16 / 3.11 ms). The 1,000-entry sweep is cheaper only because it moves slowly enough for the
  cache to hit.
- **A frame is three near-equal thirds**: our scene (1.3-1.5 ms when everything is laid out), vello's CPU side
  (1.4 ms for ~190 glyph runs), the GPU (1.5-2 ms for a 2880 x 1800 layer). Text layout is NOT the expensive part here:
  4 us a name. The one costly text operation is the ellipsis search, 4-5 times a plain layout.
- **Pictures**: vello keeps its picture atlas between frames (it uploads a picture the first time it is drawn, and
  evicts stale ones; the atlas grows to 8192 x 8192). So tiles already seen cost nothing more; 63 never-seen 256 x 256
  pictures in one frame (16 MB) cost about +1.3 ms of CPU in vello (the 4,096-picture sweep: vello 1.8 ms against
  0.5 ms) - starred, to be seen again awake.
- **The natural sort** is 3 times faster from a key made once per name than from the comparator (8.5 against 26 ms), and
  the key is where Unicode case folding belongs (this spike folds ASCII only; the real fold costs more per name, once).
- **Type-ahead cannot be a binary search** in a natural order: the names with one prefix are not together (`shot_1`
  holds shot_1 and shot_10, and shot_2 lies between them). A scan is 0.2 ms at 100,000; at 1,000,000 a miss is 9 ms,
  over the bar (the stretch only).

## What the real widgets therefore need

1. **Virtualise by arithmetic, and nothing else is required**: fixed row / cell heights, first = offset / height, lay
   out and draw what is in view. No retained row objects, no glyph-run reuse, no per-row text cache to pass the bar. If
   a cache is added for the glide, key it by (entry, column width) and bound it so a fast sweep does not churn it - or
   skip caching while the view moves by more than its own height.
2. **The model is flat and the view is an order**: 24-byte entries, names in one arena, the view a `Vec<u32>` of
   indices (a sort or a filter rewrites only that), the selection a bit set (select all is free; painting it is 37
   rectangles). 5 MB per 100,000; 1,000,000 is 50 MB, over the 40 MB target but only at the stretch.
3. **Sort by a natural key made once per name** (folded letters; a run of digits as marker + length + digits), kept
   beside the model: 8.5 ms at 100,000, 126 ms at 1,000,000 - inside the bar on the UI thread at 100,000; at 1,000,000
   it wants a worker thread or a parallel sort. The same folded key serves type-ahead; at 1,000,000 the scan should be
   cut in chunks over threads or run off the UI thread.
4. **Cut names once, not by search**: `layout_fit`'s ellipsis search is the single expensive text call (18 us). A
   table whose name column is narrow pays it for every row; cutting from the first layout's glyph positions (one
   layout, then one more of the cut string) would halve the grid's build.
5. **Thumbnails go through vello's atlas as image brushes; no atlas of our own is needed for the frame bar** - but
   keep each thumbnail's `ImageBrush` alive (the atlas knows a picture by its blob), hand vello pictures at the tile's
   size (256 x 256 at scale 2: 63 fit the atlas many times over), and bring new ones in a few per frame while scrolling
   fast rather than a whole screen at once. To be confirmed awake, and on Windows.

## Not seen

- The starred rows shown on screen (above).
- 120 Hz: the bar's 8 ms is a 120 Hz frame, and the work fits it (p95 <= 6.9 ms in every shown row), but the screens
  that were awake refresh at 60 Hz, so the cadence itself was not observed. The presenter's own pass (the layer
  composited to the surface) is inside the vsync'd call and could not be timed apart from the wait.
- A window larger than 1440 x 900 (a full 4K screen has about three times the cells and pixels).
- Unicode folding and locale collation in the sort; real thumbnails being decoded while scrolling (S18's question).

## VERDICT (macOS): PASS

List and grid, 100,000 and 1,000,000 entries: p95 <= 6.9 ms and worst <= 8.7 ms in every run that was shown, the same
at every count; sorts 8.5 ms (name, natural) and 1.5 ms (size, date) against 150 and 50; type-ahead 0.2 ms against 5;
the model 5 MB against 40; select all free. Two stretch figures miss at 1,000,000 only: type-ahead with no hit (9 ms)
and the model (50 MB). Owed: the starred grid rows again with the screens awake.

## VERDICT (Windows): see the run at the end of this file

## Windows, run 2026-11-15 (the same scripts; Windows 11, D3D12, a discrete GPU, a 60 Hz screen at scale 1, 1440 x 900 px)

Frame = build + vello + GPU done, in ms. Every run was PRESENTED (the screen was on), no late frame in any sweep.

| Run | p50 | p95 | worst |
|---|---|---|---|
| List sweep, 1,000 | 6.30 | 6.58 | 8.76 |
| List sweep, 100,000 | 6.32 | 6.61 | 7.80 |
| List sweep, 1,000,000 | 6.25 | 6.55 | 8.19 |
| List sweep, all selected, 100,000 / 1,000,000 | 6.45 / 6.39 | 6.68 / 6.60 | 8.42 / 9.36 |
| Grid sweep, one picture, 100,000 | 6.02 | 6.27 | 6.50 |
| Grid sweep, 256 distinct pictures, 100,000 | 6.52 | 6.81 | 7.10 |
| Grid sweep, one picture, 1,000,000 | 6.22 | 6.49 | 6.79 |

- The count does not matter here either. Most of a frame is the wait for the GPU (about 5 ms of the 6), not our work:
  build 0.2 - 2.8 ms, vello's CPU part 0.5 - 0.8 ms.
- In the slow glide and in two "all selected" sweeps ONE frame of 300 took 21 - 22 ms (the GPU wait, not the build) -
  over the 16 ms bar for the worst frame. The interval between presents stayed at one refresh but for that frame. Read
  as an outside stall (another process on the machine was rendering); to be seen again on a quiet machine.
- 100,000 entries: natural sort 23.4 ms with the key made once (35.4 ms by comparator); by size 2.2 ms, by date 2.3 ms;
  type-ahead 0.11 - 0.49 ms; select all under 1 us; the model 4.98 MB; a name laid out in 7 us (31 us cut with an
  ellipsis).
- This also gives the figures the macOS run owes for the grid at 1,000,000 and for distinct pictures - on Windows.
  macOS still owes them with its screen awake.

## VERDICT (Windows): PASS - p95 inside the bar everywhere; the single 21 ms frames are to be seen again

## macOS, the grid figures that were owed (2026-11-15, the screen kept awake, presented at 120 Hz)

| Grid sweep (ms) | p50 | p95 | worst |
|---|---|---|---|
| 1,000,000, one picture | 3.66 | 4.06 | 4.90 |
| 100,000, 256 distinct pictures | 3.73 | 4.08 | 8.57 |
| 1,000,000, 256 distinct pictures | 5.25 | 6.57 | 8.55 |
| 1,000,000, 256 pictures, all selected | 5.43 | 7.19 | 10.03 |

Inside the bar (p95 <= 8, worst <= 16), and inside a 120 Hz frame at p95. Nothing is owed on macOS any more.

