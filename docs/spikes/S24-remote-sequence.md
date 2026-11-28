# S24 - numbered frames pulled from another process (opened 2026-11-28)

**Status: VERDICT (macOS 27.2, Apple M5 Max) - PASS (2026-11-28). A reader asked a second process for numbered frames
in the image-sequence cache's order; the pixels crossed in a shared memory block, RGBA 16-bit float. Leaving out the
time the other process needs to make a frame, a frame was usable 0.01 to 0.57 ms after it was asked for at 1920 x 1080
and 0.01 to 2.4 ms at 4096 x 2160, depending on how many times it is copied. 720 of 720 frames of 24 fps playback were
on time at both sizes. After a jump the first new frame came at most 15.6 ms later than its own making. NOT seen:
Windows, sound, the real `ImageSequenceCache`, a picture on screen, a real decoder, more than one frame made at once.**

## Why

QCView's image sequence is a frame-addressed source under a playhead: a window of frames round the playhead, the
playhead's frame first, a jump forgetting what was asked. Only two places in its cache touch a file. The same shape
would serve a source whose frames are made by ANOTHER process on the same machine - a renderer, a converter, a decoder
QCView does not link. Before anything is designed on that, the cost of the crossing itself has to be known.

## The question

Can QCView pull numbered frames from another process through shared memory, in the order the sequence cache asks for
them, at a cost that is small beside decoding?

## Pass bar

The hand-over only; the other process's time to make a frame is left out.

| Measure | Bar |
|---|---|
| asked -> usable, 1920 x 1080 | 2 ms median, 5 ms at the 99th percentile |
| asked -> usable, 4096 x 2160 | 8 ms median |
| 30 s of playback at 24 fps, 1920 wide | no frame late |
| a jump: first new frame usable | 20 ms on top of that frame's own making |

## Time box

Half a day, this Mac only. Kept (about two hours).

## What was built

`spikes/s24-remote-sequence`, one program in two roles. The reader creates a POSIX shared memory block of whole-page
slots and starts the producer. A request is 32 bytes on a pipe (generation, frame, the slot to fill); the answer is
32 bytes on a pipe back. The READER owns the slots: it says which slot a frame goes into, and the block of slots is
its window (72 ahead, 12 behind, at most 32 asked at once). A jump moves a generation counter that lies in the shared
block, so the producer sees it before it starts the next frame. The producer makes one frame at a time; "making" is a
wait of a length given per test, reported back in the answer and taken off. Every frame carries its number and a
known fill, checked by the reader.

## Numbers

Asked -> usable, one frame at a time, 300 frames after every page had been touched (ms: median / 99th / worst):

| Producer | Reader | 1920 x 1080 (16.6 MB) | 4096 x 2160 (70.8 MB) |
|---|---|---|---|
| makes it in the slot | uses it where it lies | 0.008 / 0.013 / 0.013 | 0.007 / 0.013 / 0.016 |
| makes it in the slot | copies it out | 0.229 / 0.261 / 0.281 | 1.027 / 1.111 / 1.166 |
| makes it in the slot | takes it apart into planes | 0.324 / 0.369 / 0.414 | 1.410 / 1.521 / 1.532 |
| copies it into the slot | uses it where it lies | 0.247 / 0.287 / 0.299 | 1.029 / 1.169 / 1.211 |
| copies it into the slot | copies it out | 0.477 / 0.532 / 0.603 | 2.052 / 2.198 / 2.227 |
| copies it into the slot | takes it apart into planes | 0.569 / 0.632 / 0.696 | 2.429 / 2.613 / 2.639 |

Playback, 24 fps for 30 s, started the moment the playhead's frame was there:

| Size | The producer's time a frame | Shown | Late |
|---|---|---|---|
| 1920 x 1080 | 20 ms | 720 | 0 |
| 1920 x 1080 | 38 ms (26 a second, barely enough) | 720 | 0 |
| 4096 x 2160 | 20 ms | 720 | 0 |

Jumps while playing, one a second for a minute (59), 1920 x 1080 - the first new frame, on top of its own making:

| The producer's time a frame | median / 99th / worst (ms) |
|---|---|
| 0 ms | 0.73 / 1.07 / 1.13 |
| 10 ms | 3.38 / 5.08 / 5.15 |
| 20 ms | 12.03 / 13.83 / 15.59 |
| 40 ms | 2.90 / 5.55 / 5.90 |

## Findings

1. **The crossing is nearly free.** The messages cost about 0.01 ms; everything else is copying, at about 65 GB a
   second on this machine. Even two copies and a rearrangement at 4096 x 2160 stay under a third of the bar.
2. **The copies are the design's to choose.** A producer that can be given the slot as its output buffer, and a reader
   that shows the slot as it lies (the ring's zero-copy wrap on macOS, S4), pay nothing. Each copy that is kept costs
   0.25 ms at 1920 x 1080 and 1 ms at 4096 x 2160.
3. **The shared block can be the cache's window.** 88 slots are 1.5 GB at 1920 x 1080 and 6.2 GB at 4096 x 2160; with
   the reader owning the slots nothing is copied out to be kept.
4. **A slot asked for before a jump is not free until it is answered.** The first version gave such slots out again at
   once, and a frame from the place left behind landed in a slot promised to a new frame - caught by the frame-number
   check. The producer now answers EVERY request, made or dropped, and a slot returns to the reader with its answer.
   With more than one frame made at once this rule is what keeps two writers out of one slot.
5. **A jump waits for the frame being made.** The producer cannot stop a frame it has started, so the first new frame
   waits for the rest of that one: up to one frame's making with a single worker (the 20 ms row). Where the producer
   was idle at the moment of the jump, the wait is about a millisecond - the dropped requests being answered.
6. **A measuring fault, mended:** the producer's wait was first a sleep, which overslept by about 5 ms a frame; that
   showed as late frames at 38 ms and as jumps of up to 26 ms. With a busy wait both went away. The numbers above are
   from the busy wait.

## Not seen

Windows (named shared memory and pipes there are other calls); sound; the real `ImageSequenceCache` with a loader in
place of its file read; a frame from the slot uploaded and shown; a real decoder as the producer, its buffers and its
several frames at once; a producer that dies or hangs (the ring's liveness by process id is the model); requests in a
shared block instead of pipes; sizes above 4096 x 2160.

## Verdict

**PASS.** Pulling numbered frames from another process costs between nothing and 2.4 ms a frame against bars of 2 and
8 ms; playback held and a jump stayed inside 20 ms. The crossing does not decide whether such a source is worth
building: the producer's own speed does. Pull, with the reader owning the slots, is a shape that works; push was not
needed and not measured. The spike dies here; its program is not a crate.
