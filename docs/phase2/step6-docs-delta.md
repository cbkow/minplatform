# The docs rewrite: what changed AFTER the audit (`step6-docs-audit.md`, written the morning of 2026-11-10)

The audit is the per-page map. This page overrides it wherever the two disagree: everything below was built later the same
day (canon rows 254–264; specs `step6-luts.md` §8, `step6-live-dual.md` §8, `step6-output-cadence.md`). Where a writer is
unsure, the CODE in `apps/qcv/src` and the canon row decide — never the 2.x page, never this summary.

## Audit findings that are now REVERSED (3.0 has these)

| Audit said | Now |
|---|---|
| No Scene LUT / ASC CDL / Correction ID controls (§4 item 5; index.md row) | **Built (row 257).** OCIO chains only. A **Scene LUT** card in the Color drawer between Look and View, collapsed to a strip by default: "+ Add LUT" / "Replace LUT", the file's name, ✕ Clear LUT, and for `.ccc` / `.cdl` a **Correction ID** field (empty = the first). Formats: `.cube .3dl .csp .cc .ccc .cdl`. The file acts on the Input's values after the Look (not "scene-linear" — 2.x's wording was wrong). Part of the clip's whole chain (Reset / Apply to all / presets carry it). A file that cannot load: a red toast naming it, the card in an error state, that clip shown with Color bypassed. No Display LUT (never ported). |
| No Export LUT (§4 item 6) | **Built (row 257).** "Export LUT…" at the right end of the Color drawer's bottom row, either engine, Color on or off. A 65³ `.cube`: the chain's SDR picture, sRGB-encoded, the Input's code values in; no suggested file name. Includes the Highlight Knee (a toast says so if it had to be left out) and **Video gamma when it is lit** (the toast says "with Video gamma"); never exposure / gamma / channel. |
| A live source cannot be a dual-view side (§4 item 15) | **Built (row 258).** Either side, against a file, a sequence, another live source or nothing. The dual controls (A / B chips, mode buttons) also sit on the live row, so dual can be entered from a live item; a running feed moves in and out with no reconnect. A live side: a red / yellow dot on its chip, a LIVE bar on its lane (no clip, no edits), **muted by default on either side** (its chip's speaker pill unmutes it; the Listen button is the single view's only), its own colour chain and Transfer. Two live sides (or live + empty): the transport hides and the live row stands in. The same live item on both sides is refused. A stream link dropped on a side becomes that side. No sync between a live side and a file side. |
| Prev clip / Next clip and the hover-thumbnail toggle are drawn but disabled (§4 items 16, 17) | **Removed** from the transport row (row 254). Do not document them. |
| No "Open in minNotes" (§4 item 20) | **Built (row 256):** the last row of a STREAM item's Export notes menu; greyed with "minNotes is not installed" when it is not. |
| No note markers on the timeline (§4 item 25) | **Built (rows 166 / 256):** violet pennants on the scrubber's bottom rail — filled for an open note, hollow for an addressed one; hover shows the timecode and text, a click seeks. None on the compact strip. |
| The A / B level mixer (§4 item 26) | Never existed in 2.x either; one 2.x docs sentence claims it — remove the claim. |
| Video gamma: "screen only" (§3; canon 246) | **Changed (row 262):** it now also reaches the NDI / AJA outputs, screenshots, note images, thumbnails (timeline, hover, the Inspector's file thumbnail) and Export LUT. The waveform still reads the accurate picture. It acts only with Color on and an SDR picture. |
| "amber" pills; A / B "in cyan and orange" (§3, §5) | **Colours changed (rows 262–264).** The state pills (KNEE, VIDEO GAMMA, "SDR display — highlights clip") and warnings are YELLOW. Dual view's A is cyan, **B is a muted tan**; the lanes' clips carry a faint wash of their side's colour; the targeted lane's label cell is filled with its side's colour under a white letter. Say "yellow", "cyan", "tan" — or avoid naming colours where it adds nothing. |
| AJA: tested-hardware claims before the hardware walk (§6 question 4) | Walked on a **T-TAP Pro on macOS** on 2026-11-10 (1080p and UHD; SDR, PQ, HLG). Windows not yet walked on a card with 3.0. **QCBridgeA 0.0.2** is the version to name: it queues frames for the card (smoother playback). |

## New since the audit (no page mentions these)

- **Output latency / smoothness (rows 259–261):** nothing user-facing to document beyond "QCBridgeA 0.0.2 or later".
  Dual view now sends AUDIO to the outputs (the first heard side); before today it sent none.
- **The Output section's empty state** reads "No outputs found".
- **A saved project shows its file name** in the Media section's title and the window title ("Untitled" until saved; a
  "•" marks unsaved changes).
- **QCView 2 projects with a pinned Scene LUT (row 257, R6):** opening one shows a notice; the LUT is not applied until the
  clip gets an OCIO chain, at which point the 2.x Input / Look / Scene LUT / Knee are taken over. Nothing is erased.
- **Known limits worth one honest line each where relevant:** a LUT file edited on disk under the same name is re-read
  only after a restart; Export LUT has no shaper (a scene-linear Input covers 0–1 only); drive / mount path mapping for a
  LUT path saved on another OS does not exist yet; two live host sides share one A/V sync offset.

## Rules for the writers

1. Truth first: every sentence about behaviour must be true of 3.0 TODAY. Check the code (`apps/qcv/src`, `crates/`) or
   the canon row; if it cannot be confirmed, leave the sentence out and list it under "UNVERIFIED" in the hand-back.
2. Keep the site's voice and structure (front matter, permalinks, nav order, heading style, British / American spelling as
   the page has it). Rewrite sections; do not restyle the site.
3. Shortcuts: only keys that exist — `apps/qcv/src/shortcuts.rs` and `crates/qcv-player/src/key_map.rs` are the lists.
4. Screenshots: every `qcv0NN` image is stale. Remove the image line and leave an HTML comment in its place
   `<!-- SHOT: <what the new screenshot must show> -->`; collect those into the hand-back's shot list. Leave
   `qcbridge_*` images that show After Effects / Premiere / Blender, not QCView.
5. No client names, job names, media file names or personal paths anywhere. Generic example names only.
6. Do not touch `appcast.xml`, `_config.yml`, `_sass`, `CNAME`, or the Version History log in `installation.md` (history
   stays as written); do not commit — leave the working tree for review.
