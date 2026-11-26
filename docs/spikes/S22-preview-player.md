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

The owner's walk, 2026-11-26, Finder's space-bar panel, the Developer ID build:

- "looks great. The sound works": the picture plays and the tone is heard.
- A picture of the panel on the generated DNxHR HQ clip (1920 x 1080, 25 fps, 24-bit sound in the MXF), two seconds in:
  `decode: software (VideoToolbox context made)`, `audio unit ok in 18 ms, file has sound: true`, `shown 54 dropped 0
  late 0`, `peak memory 185 MB`, `no key yet`. So: the VideoToolbox device context is made inside the sandbox; DNxHR has
  no hardware decoder and plays in software; the CoreAudio output unit starts in 18 ms and plays.
- The readout's "first picture 0 ms" is the spike's fault (its clock starts after the open), not a measurement.
- "I don't think the thumbnails should be full video though": Finder's small previews (the preview column, the gallery,
  Get Info) show the same extension, and the spike plays wherever it is shown. For the player: small views are a still.

Still owed for the bar: the ProRes clip past 30 seconds (dropped, late, the peak memory, whether its decode says
VideoToolbox), the keys after a click, and whether the sound stops when the panel closes or moves to another file.

## Verdict

None yet.
