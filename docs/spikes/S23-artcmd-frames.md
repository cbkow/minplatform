# S23 - a camera-original clip played as a sequence that is not on disk (opened 2026-11-27)

**Status: VERDICT (macOS 27.2, Apple M5 Max) - PASS at review size, FAIL at full size (2026-11-27). A 4608 x 3164
ARRIRAW HDE clip, asked of ARRI's free command-line renderer in ranges of EXR frames through named pipes and decoded
with the family's EXR reader, was ready on time for every one of 720 frames of 24 fps playback at 1920 wide, twice; a
jump to a far frame took 149 ms (median of 8, worst 151 ms). At the clip's full size it reached about 11 frames a
second, and with the renderer on the CPU about 10. NOT seen: the real `ImageSequenceCache`, a picture on screen, sound,
Windows, a clip read cold from shared storage, anything longer than 253 frames.**

## Why

The owner (2026-11-27) wants camera-original ARRI clips reviewed in QCView. Clips are many minutes long and come in
many takes, so converting each before it can be watched does not suit a review player. ARRI gives away a command-line renderer, `art-cmd` (ARRI Reference Tool CMD 1.1.1), which the user
installs themselves and whose manual describes rendering a range of a clip (`--start`, `--duration`) to EXR frames.
QCView already plays EXR sequences through a read-ahead cache that never holds more than a window round the playhead
(`qcv-player`'s `ImageSequenceCache`: 72 frames ahead, 12 behind, the playhead's frame first). Seen by hand the same
day: the renderer writes an EXR frame into a named pipe byte for byte as it writes it into a file.

## The question

Fed that way, is a 4.6K clip ready frame by frame at 24 frames a second, and how long does a jump take?

## Pass bar

On this Mac, at 1920 wide: every frame of 20 seconds of playback decoded before the playhead reaches it, and the first
frame after a jump to a far frame within **300 ms**.

## Time box

Three hours. Kept (about one).

## How to run

`cargo build --release -p s23-artcmd-frames`, then
`target/release/s23-artcmd-frames --tool <the renderer> --clip <a clip> --frames <its length> [--seconds 30] [--width 1920] [--platform metal|cpu]`
(the EXR reader needs the vendored libraries: `DYLD_LIBRARY_PATH=external/deps/macos-arm64/lib`). The renderer and the
clip come from the command line; neither is in this repository. The clip was one of the vendor's own published sample
clips (4608 x 3164, 24 fps, 253 frames, HDE), already in the system's file cache.

What the harness does: for a range it makes one pipe per frame, named by the renderer's frame counter, starts the
renderer once with the folder as its output, and reads the pipes in order. A reader thread only takes the bytes; eight
decoder threads decode them. A playhead runs at the clip's rate (the clip taken round and round) and asks, as the cache
does, for what lies ahead in batches of up to 36, never further than 72 frames ahead.

## Measurements

| Run | Frames ready on time | Other |
|---|---|---|
| 1920 wide, GPU, 30 s | **720 of 720** | first frame 172 ms; 111 starts of the renderer; 399 MB/s through the pipes |
| the same again | **720 of 720** | first frame 175 ms; 110 starts |
| a jump to a far frame, 1920 wide, 8 tries | - | **median 149 ms, worst 151 ms** |
| 4608 wide (full size), GPU, 20 s | 1 of 480 | 480 frames took 42.3 s (about 11 a second); a frame is 87.5 MB; a jump 251 ms |
| 1920 wide, renderer on the CPU, 10 s | 2 of 240 | 240 frames took 22.8 s (about 10 a second) |
| 1920 wide, GPU, the bytes read but not decoded | 480 of 480 | - |
| 1920 wide, GPU, decoding on the thread that reads the pipe (first build) | 101 of 480 | 480 frames took 20.8 s: just under the clip's rate |

Beside the harness, by hand: the renderer's own measure for the clip (`--output perf-measure`) 213 frames a second at
full size and 281 at 1920 wide on the GPU, 7 on the CPU; a single frame through a pipe identical to the file
(`cmp`), and 24 of 24 for a range.

## Findings that change the design

1. **It holds at review size and not at full size.** Half-float EXR without compression is 15 MB a frame at 1920 wide
   and 87.5 MB at 4608: at full size the transport, not the debayer, is the limit.
2. **Decoding must not sit on the thread that reads the pipe.** A pipe blocks its writer: with the decode in the reading
   loop the renderer waited on us and the whole chain fell just under the clip's rate.
3. **The family's EXR reader cannot read a pipe** (`exr_start_read` fails: it needs a file it can seek in). The harness
   put each frame's bytes into a file of its own for the reader and took the file away again - so this spike DID write
   every frame to the temporary folder, 15 MB at a time. The real thing needs the reader to take bytes from memory.
4. **Only EXR passes through a pipe.** TIFF arrives as 8 bytes with the renderer reporting success; ProRes in MXF fails
   with an error. The manual does not mention pipes at all: this is behaviour seen in one version, not a promise.
5. **The renderer is started often once the window is full** (about four times a second here, each for a few frames).
   It still held; asking only when a full batch is wanted would start it far less.
6. **Two renderers at once: the harness hung** and the run was ended after five minutes. Not looked into (the fault may
   be the harness's); one renderer at a time is what the numbers above are.
7. **The renderer's licence** (its `EULA.txt`, read in full): use is for "the purposes described in the Documentation";
   the user states that they act "as entrepreneur and not as consumer"; it may not be passed on, so each user installs
   their own; nothing forbids another program starting it, and its manual describes scripted and batch use.

## Later the same day: ProRes segments in place of EXR frames

Asked by the owner as a devil's advocate ("EXR is just such a beast"; and of the renderer's start, about 0.2 s: "might
be a blocker"). The renderer writes no light single-frame format - no JPEG 2000, DPX or PNG; its outputs are EXR and
TIFF without compression, ProRes in MXF, and HDE - and ProRes does not pass through a pipe, so the light route is short
ProRes FILES. `spikes/s23-artcmd-frames/segments.py` is a dummy playback of that: one renderer at a time writes segment
after segment (LogC4) ahead of a playhead at the clip's rate; a segment counts when the renderer has ended. Same clip,
same Mac, 30 s each.

| Run | Frames ready on time | First picture | Starts | Written |
|---|---|---|---|---|
| Proxy, 1920 wide: 6 frames first, then 24, 72 ahead | 718 of 720 (worst 59 ms late) | 217 ms | 36 | 65 MB/s |
| Proxy, 1920 wide: 6, then 48, 96 ahead | 717 of 720 (worst 107 ms) | 197 ms | 21 | 40 MB/s |
| Proxy, 1920 wide: 12, then 96, 192 ahead | 719 of 720 (worst 44 ms) | 223 ms | 12 | 25 MB/s |
| ProRes 422, full size: 6, then 48, 96 ahead | 705 of 720 (worst 602 ms) | 308 ms | 21 | 127 MB/s |
| ProRes 4444, full size: 6, then 48, 96 ahead | 701 of 720 (worst 763 ms) | 314 ms | 21 | 259 MB/s |

One segment alone, the time to write it against the time it plays: Proxy at 1920 wide 1.2 times faster at 6 frames,
3.3 at 24, 5.8 at 48, 8.1 at 96; ProRes 422 at full size 0.9, 2.0, 2.5, 2.8; ProRes 4444 at full size 0.8, 1.7, 2.0,
2.2. A jump to a far frame: one frame 201 ms (median of 6), six frames 230 ms as Proxy and 317 ms as 4444 at full size.

What it shows:

- **The start is not what limits playback.** It is paid once a segment, and a segment of a second or more is written
  several times faster than it plays. It is what a jump costs: about 0.2 s before anything else.
- **Every file carries about 50 MB that is not picture** (a ONE-frame Proxy segment is 50 MB; 96 frames are 63 MB), so
  short segments write far more than their pictures weigh. Not looked into what the 50 MB are.
- **Full size is in reach as ProRes** (2 to 2.8 times the clip's rate once segments are long), where EXR was not; the
  late frames above sit where a 6-frame segment is followed by a 48-frame one - a ramp of lengths was not tried.
- **The family's FFmpeg reads a segment**: ProRes, two mono sound tracks, a data track; it warns once ("could not
  resolve file descriptor strong ref") and decodes all frames. So the sound comes with the picture.
- **A file can be read while it is being written**: 1.5 s into a long render (571 MB written, the renderer still
  running) ffprobe counted 79 packets. One long render read behind its writer may need no segments at all. Seen once,
  with the probe only.

Not seen here: segments joined in a player, with sound, without a seam; the same on Windows; what the renderer does when
its output file is deleted under it; a second version of the renderer.

## Not seen

- The real `ImageSequenceCache` (the harness only asks in its order), a frame on screen, the renderer and the player
  sharing one GPU while the player draws.
- Sound (the renderer exports a clip's sound as WAV; not tried here).
- Windows: the renderer on CUDA or OpenCL, and whether anything like a pipe is accepted there.
- A slower Mac; a clip read cold, from shared storage (an HDE frame of this clip is about 13.5 MB in the file).
- Uncompressed ARRIRAW and ARRICORE in this harness (by hand, at 2048 x 1152, both were faster than HDE).
- Stepping at a stride (the manual shows no step for a range), reverse, a clip longer than 253 frames.
- A new version of the renderer.

## Verdict

PASS at 1920 wide, FAIL at full size. A camera-original clip can be watched at its rate without converting it first, at
review size, on this Mac. The spike dies here: `spikes/s23-artcmd-frames` is not the feature's start.
