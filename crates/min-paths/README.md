# min-paths

The min*/QCView family's path kit — crate #1 of the platform (canon `docs/rust-wgpu-port.md` §22, rows 119–126, ruled
2026-11-06). One copy of what ufb, minNotes, QCView and MinRender each carried separately, with the disagreements the survey
found ruled one way and tested.

| Module | What | Taken from |
|---|---|---|
| `url` | `path_to_file_url`, `file_url_to_path` (+ `file://host/share`, `file://localhost/`), ONE percent codec, `uri_list::{encode, decode, decode_paths}` (CRLF after every entry) | ufb, corrected |
| `map` | `Mapping {id, label, win, mac, lin, enabled}`; `to_ref` / `resolve_ref` (`{vol, rel}`); `translate(src, dst, path, &[Mapping])`; canonical Windows form | minNotes (refs), ufb / MinRender (strings), boundary check everywhere |
| `sequence` | `detect(path, listing)` — QCView's `detectImageSequence` rules byte-compatible; `detect_on_disk`; `frame_name`; `####` ↔ `%0Nd` | QCView's public import path; proven by `tools/min-paths/seqdiff` |
| `classify` | `is_network_source`, `is_volatile_source(path, extra_roots)`, `share_host_for_path`, `fs_type_name` | minNotes' rules via `statfs` / Win32; MinRender's host rule |
| `volume` | `same_volume` (device identity), `is_drop_onto_self`, `is_drop_into_itself`, `drop_guard`, `external_path`, `drive_or_unc` | ufb, with the device-identity and descendant fixes |
| `clipboard` | `copy_paths` / `cut_paths` / `paste_paths` / `paste_intent` / `has_paths`, the cut registry, text via arboard | ufb's AppKit / CF_HDROP code, moved |
| `names` | `sanitize_file_name`, `is_reserved_name`, `web_link_file_name` | ufb + Windows reserved names |
| `links` | `ufb_link` / `parse_ufb_link`, `minnotes_link`, `qcview_project_link` / `parse_qcview_link` (project + stream) | minNotes, ufb, QCView |

Every OS rule takes an `Os` so the other platform's behaviour is tested on this one. No UI, no async; the only IO is
`metadata` / `statfs` / Win32 volume queries and the pasteboard. `serde` is an optional feature (the `{vol, rel}` shape).

**Tests:** `cargo test -p min-paths` (67 on both OSes). **Differential test vs QCView:** `tools/min-paths/seqdiff/seqdiff.sh`
slices `detectImageSequence` unmodified out of the QCView checkout, builds it against Qt, and runs it and the crate over
`cases.txt` in fresh scratch dirs — the outputs must be identical.

**Adoption order (row 125):** ufb first (natively, its own session, its suite is the gate), minNotes second (C ABI),
MinRender third, QCView last on its own release. Behaviour changes on adoption, by design: ufb's cross-volume drag on macOS
defaults to Copy (row 119); MinRender's translation gains the boundary check (row 121); QML drag sources gain percent-encoding
(row 124); ufb's sanitizer refuses reserved names (row 123).
