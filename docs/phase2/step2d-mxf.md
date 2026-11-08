# Step 2d — the MXF scanner (canon row 137)

**Built 2026-11-08** (the user: "this sounds cleaner if we can pull it off"). `crates/qcv-mxf`: ExifTool's MXF reader
(`Image::ExifTool::MXF::ProcessMXF`, the oracle QCView bundled) carried rule for rule onto Rust; its tag table generated from
ExifTool's `MXF.pm` v1.09 by `tools/mxf-tags/gen.py` (1 652 entries — the hidden "Unknown" ones kept: ExifTool never
prints them but decodes InstanceUID and the strong references through them to build the object tree).

| | |
|---|---|
| What is read | header metadata only: the header partition, then the footer's copy when the header is not closed + complete |
| Values | ExifTool's `-n` forms (durations in seconds by the group's EditRate, timestamps, ProductVersion, ULs vs reversed GUIDs, UMIDs, batches) |
| Groups | `MXF` / `Track<n>` numbered by first TrackID seen, inherited down the tree from each Preface |
| Acceptance | `mxf-parity` (ExifTool `-a -n -G1 -MXF:all` vs ours, multiset per file): **1 413 of 1 413 of the user's MXFs IDENTICAL** (121 of them `.mxf` names over another format — no MXF on both sides); 7 of 7 synthetic FFmpeg MXFs (OP1a DNxHD / DNxHR HQX / MPEG-2 / H.264 / ProRes, D-10, OP-Atom) IDENTICAL |
| Speed | 0.2 ms a file warm (2.9 ms across the corpus with cold reads) vs ExifTool's 63 ms |

**Licence (a deviation from row 137's "inside `qcv-meta`"):** the generated table is ExifTool's (Perl's terms, GPL-1.0-or-later
OR Artistic-1.0), so the scanner is its own crate under the workspace's GPL-3.0-or-later; `qcv-meta` stays MIT OR Apache-2.0
(row 132) and does not depend on it. A permissive build would regenerate the table from the SMPTE registers instead.
`regxmllib-rs` (row 137's donor) was not needed.

Not in this step: the Inspector's MXF section (a design talk — which of the ~50 tags people see), DMS-1 / AS-11 contents
(their sets are read and named; the user's corpus carries none).

**Windows (rhombus, 2026-11-08, at 52ef5a5):** the unit test green; 4 of 4 synthetic MXFs made on the box (OP1a DNxHD + timecode, MPEG-2,
ProRes, OP-Atom) IDENTICAL to ExifTool.exe; 0.33 ms a file vs 294 ms.
