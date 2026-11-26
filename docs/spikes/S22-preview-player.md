# S22 - the family's player, with sound, inside a Quick Look preview (opened 2026-11-26)

**Status: VERDICT (macOS 27.2) - PASS on the question, the bar's own number not read (2026-11-26). QCView's player
plays an MXF with sound inside the sandboxed preview extension: the CoreAudio output unit starts (18 ms) and is heard,
the sound stops when the panel closes, the keys reach the view after a click, a VideoToolbox context is made in the
sandbox, and DNxHR HQ at 1920 x 1080 plays in software without a dropped or late frame at a 185 MB peak. NOT read
off: the ProRes clip past 30 seconds (the bar as written), and whether its decode says VideoToolbox.**

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

Later the same walk: "keys work, sound stops when I close the panel."

Not read off: the ProRes clip past 30 seconds (dropped, late, the peak memory, whether its decode says VideoToolbox);
the sound when Finder's arrows move to another file.

## Findings for the player

1. Sound needs nothing from us: no entitlement, no question to the person; the unit is quick to start here.
2. The keys arrive once the view has been clicked (it then is the first responder). Space and the arrows stay the panel's.
3. Finder's small previews run the same extension: a player that starts by itself plays in the preview column too.
   The view's size is the only thing that tells the two apart.
4. 185 MB with no scrub decoder: the scrub cache's budget is the memory's main term and is ours to choose.

## Verdict

PASS: the MXF player can be built on `Transport` and `AudioPlayer` as they are. The spike dies here.
