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
