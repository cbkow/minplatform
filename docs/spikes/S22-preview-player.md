# S22 - the family's player, with sound, inside a Quick Look preview (opened 2026-11-26)

**Status: OPEN (2026-11-26). Built and registered; the extension starts on an MXF, stays alive and leaves no crash
report. Everything that answers the question is on its screen and in the owner's ears: the walk is owed.**

## Why

Canon rows 606-610: the preview of an MXF is a player in the bar (play, the playbar, timecode, sound). The spec
(`docs/phase2/quicklook-spec.md`, section 5) found the player has no need of a window of ours, and left unproven: a
CoreAudio output unit in a preview extension, VideoToolbox in the sandbox, which keys reach the view, and the memory.

## The question

Can QCView's player play one MXF with sound inside the sandboxed preview extension?

## Pass bar

A 1920 x 1080 ProRes MXF with sound plays in Finder's panel at its frame rate for **30 seconds** with **no dropped
frame** reported by the player, and the sound is heard.

## Time box

Two hours. About forty minutes used when the walk was handed over.

## How to run

`zsh spikes/s22-preview-player/build.sh` (after `packaging/macos/make-app.sh` once) builds, signs with the Developer
ID identity and registers `target/s22/S22Host.app`: one preview extension for `org.smpte.mxf` - a Swift shell and this
spike's Rust library (`Transport::open` with a VideoToolbox context and no scrub decoder, `AudioPlayer::new`,
`tick` at 60 Hz from a timer). It plays at once. A click on the picture toggles play; K does too, J and L step a
second, comma and period a frame - if the keys arrive. The bar is the readout: how the open went (hardware decode, the
audio unit), frames shown / dropped / late, the first picture's time, the peak memory, the last key.
`--remove` takes it off the system.

## Measurements

None yet.

## Verdict

None yet.
