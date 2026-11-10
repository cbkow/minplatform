# QCView 3.0 — Microsoft Store listing (draft)

## Short description
A media player and reviewer for post-production: accurate, colour-managed playback of video, EXR and image sequences, with notes, dual-view comparison and HDR.

## Description
QCView is a media player and reviewer for artists and post-production teams who need to see exactly what is in the file: frame-accurate, colour-managed, and fast.

QCView 3 is a complete rebuild. The picture and interface are both drawn on the GPU with a new DirectX 12 renderer; the interface is new, and you can edit a comparison or playlist directly on the timeline. It opens your QCView 2 projects and notes as they are.

PLAYBACK
• Hardware-accelerated decoding of ProRes, H.264, H.265 and more, with a software fallback
• Frame-accurate stepping, press-and-hold fast forward and rewind, review speeds
• Embedded timecode with a selectable origin
• Audio with a Mix picker for 5.1 and 8-channel deliveries, and an A/V sync trim
• Playlists with per-clip trims edited on the timeline, and hover thumbnails

IMAGE SEQUENCES
• OpenEXR with multichannel and multipart support
• TIFF, PNG, JPEG, DPX, TGA and BMP sequences

COLOUR
• One colour chain per clip, and per side of a comparison
• minColor, the built-in engine: pick the input, and it works — no config to set up
• OpenColorIO as the option, with ACES 2.0, ACES 1.3 and Blender configs included, or your own
• Scene LUTs and ASC CDLs, and Export LUT to write a clip's chain as a .cube
• A GPU waveform, in percent on SDR and in nits on HDR

HDR
• No display mode to pick: QCView uses an HDR monitor's range when the window is on one, and gives SDR monitors a 10-bit picture
• Highlight Knee for reviewing HDR masters on an SDR display

REVIEW
• Dual view: side by side, split wipe and difference, each side with its own colour and audio
• Notes with drawing tools on the frame, markers on the timeline, and export to Markdown, HTML, PDF and DOCX
• Safety guides for broadcast and social formats
• Background switching for reviewing alpha
• Screenshots through the clip's colour chain, as PNG, PNG 16-bit, JPEG or TIFF

LIVE AND OUTPUT
• Live feeds from Adobe After Effects and Premiere Pro, and from Blender, through the free QCBridge add-ons
• Output to NDI and to AJA video cards through the QCBridge add-ons
• SRT streams open as sources

PROJECTS
• Project files (.qcvproj) and qcview:// links for sharing a review
• Eleven interface languages besides English

Manual: https://qcview.app

## What's new in this version
QCView 3.0 is a complete rebuild under the same name.

• New DirectX 12 renderer: HDR monitors get a floating-point picture and SDR monitors a 10-bit one, switching as the window moves between them
• New interface, with one timeline that carries the ruler, in / out, the cache line and an audio waveform
• Dual view and playlists are edited directly on the timeline — no edit mode
• Colour: minColor is the default engine and OpenColorIO the option; one chain per clip; Scene LUT, CDL and Export LUT
• Notes open in a drawer, with markers on the timeline
• A live feed can be either side of a dual view
• DPX, TGA and BMP sequences
• Removed: the SRT / RTMP stream output (NDI covers it), the vectorscope, Minimal Mode

Your 2.x projects and notes open as they are. Settings start from their defaults.

Full list: https://qcview.app/installation/

## Product features (up to 20, 200 characters each)
1. Colour-managed playback of video, OpenEXR and image sequences
2. Hardware-accelerated decoding with frame-accurate stepping
3. HDR with no mode to pick: floating-point on HDR monitors, 10-bit on SDR
4. Two colour engines: built-in minColor, or OpenColorIO with ACES configs included
5. Scene LUTs, ASC CDLs and Export LUT (.cube)
6. GPU waveform in percent or nits
7. Dual view: side by side, split wipe and difference
8. Notes with drawing tools, exported to Markdown, HTML, PDF and DOCX
9. Playlists edited directly on the timeline
10. Multichannel and multipart OpenEXR; TIFF, PNG, JPEG, DPX, TGA, BMP
11. Safety guides for broadcast and social formats
12. Live feeds from After Effects, Premiere Pro and Blender (QCBridge add-ons)
13. Output to NDI and AJA video cards (QCBridge add-ons)
14. Audio mix-down for 5.1 and 8-channel files
15. Embedded timecode with selectable origin

## Search terms (7 at most, 30 characters each)
video player · EXR viewer · image sequence · color management · HDR review · post-production · dailies

## Screenshot captions
1. store_01_main — Accurate, colour-managed playback, with every detail of the file in the Inspector
2. store_02_color — A color chain per clip: OpenColorIO with ACES built in, and a GPU waveform
3. store_03_hdr — Review HDR masters on any display, with a waveform in nits and the Highlight Knee
4. store_04_split — Compare two versions with a split wipe
5. store_05_difference — Difference mode shows exactly what changed between versions
6. store_06_notes — Notes on the frame, with drawing tools and markers on the timeline
7. store_07_playlist — Playlists, edited directly on the timeline
8. store_08_guides — Safety guides for broadcast and social formats
