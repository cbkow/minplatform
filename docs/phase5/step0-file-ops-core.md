# Phase 5, step 0 — audit: the file-browser back end and its bridge

Audited: the `ufb` repository, branch `main` @ `9e197f0`, version 1.4.2 (`Cargo.toml:17`). Read only; nothing was built or run.
Date of the audit: 2026-11-15.

Scope: `core/src/{file_ops, archive, transcode, search, search_index, volumes, bookmarks, utils, shell_context_menu, settings,
app_state, events, lib, jobs, sync_aware}.rs`, `core/tests/*`, `bindings/src/*` and `bindings/build.rs`, `app/AppController.h`,
`devdocs/archive-7zip.md`. The QML front end, projects / metadata / columns / templates / db, mesh sync, mounts and the agent,
and media decoding are other readers' areas; they appear here only as touch points.

Conventions: every claim cites `path:line` in the ufb repository. "Not found" = searched for and absent. "Not verified" = it
depends on something outside the files read. Where behaviour comes from a third-party crate, the crate's source is cited as
`fs_extra 1.3.0 src/...` (the version pinned in ufb's `Cargo.lock`).

---

## 0. Summary

| Topic | What is there today |
|---|---|
| Listing | One blocking `read_dir` + one `stat` per entry, whole folder returned as one JSON string; no paging, no cancel |
| Watching | **None.** No file-system watcher anywhere; views refresh only after the app's own operations, on F5, and on two timers |
| Copy / move | The `fs_extra` crate, 64 kB buffer, always overwrite, no prompt, no cancel, no verification, no metadata preserved; a move is always copy + delete, even on the same volume |
| Delete | OS trash through the `trash` crate; on failure macOS **deletes permanently without asking** |
| Undo | None |
| Archives | Bundled 7-Zip CLI, a sequential in-memory job queue, careful placement rules, cancel with clean-up. The best-built module of the area |
| Transcode | One fixed preset (H.264 CRF 25 + AAC 192k into an `MP4/` subfolder), ffmpeg + ffprobe + exiftool CLIs |
| Search | Names only. OS tool (Everything CLI / Spotlight) with a breadth-first walk fallback, or a Tantivy n-gram index rebuilt from scratch on each session |
| Volumes | No enumeration of drives or mounted volumes in this area. `volumes.rs` is the path-identity registry, not a device list |
| Settings | One JSON file, 16 top-level keys, of which 6 groups are no longer read by anything |
| Bridge | 16 cxx-qt objects, 9,892 lines; everything crosses as JSON text in a `QString`. The core crate has no Qt dependency at all |

---

## 1. Directory listing

### 1.1 How a folder is read

| Step | Behaviour | Evidence |
|---|---|---|
| Entry point | `list_directory(path) -> Result<Vec<FileEntry>, String>`; synchronous | `core/src/file_ops.rs:71` |
| Record | `FileEntry { name, path, is_dir, size, modified (ms since epoch, optional), extension }` — six fields, nothing else | `core/src/file_ops.rs:16-25` |
| Enumeration | `std::fs::read_dir`; an entry that fails to read is skipped silently | `core/src/file_ops.rs:80-84` |
| Stat | One `std::fs::metadata` per entry (follows links), falling back to `symlink_metadata`; an entry whose stat fails is dropped from the listing | `core/src/file_ops.rs:88-95` |
| Path | The entry path is kept as is; deliberately not canonicalised, so that prefix matching for subscriptions and mounts keeps working | `core/src/file_ops.rs:97-100` |
| Names | `to_string_lossy` — a name that is not valid Unicode is altered and can no longer be opened by its listed path | `core/src/file_ops.rs:96,100` |
| Sort | Folders first, then `name.to_lowercase()` compared bytewise. No natural (numeric) order, no locale collation | `core/src/file_ops.rs:127-131` |
| Failure | Any error becomes the string `"Not a directory or cannot read: …"` | `core/src/file_ops.rs:75-78` |
| Windows first-run | `read_dir` is retried up to 4 times (200 / 400 / 800 ms sleeps) on OS error 448, the SmartScreen reputation check on reparse points | `core/src/file_ops.rs:27-68` |

The UI sorts again by its own column and filters four names (`.gitkeep`, `.DS_Store`, `Thumbs.db`, `desktop.ini`)
(`app/qml/Ufb/App/FileBrowser.qml:1787-1792`, `:1857-1865`). That is the whole hidden-file policy.

### 1.2 What is and is not handled

| Case | Status | Evidence |
|---|---|---|
| Dot-files | Shown. No "show hidden" switch in the core | `core/src/file_ops.rs:80-125` (no filter) |
| Windows hidden / system attribute | Not read. `FileEntry` has no such flag | `core/src/file_ops.rs:16-25` |
| Symbolic links | Followed for size, date and kind. A link whose target cannot be stat'ed is assumed to be a folder. The entry carries no "is a link" flag | `core/src/file_ops.rs:86-115` |
| macOS aliases, Windows `.lnk` | No handling: listed as ordinary files | not found |
| Windows junctions / mount points | Treated as folders through `metadata`; only the error-448 retry is specific to them | `core/src/file_ops.rs:39-68` |
| Packages (`.app`, bundles) | No detection: shown as plain folders | not found |
| Case rules | Sort is case-insensitive. The same-file guard lowercases both sides on every OS, including case-sensitive volumes | `core/src/file_ops.rs:1684-1690` |
| Long Windows paths | No explicit `\\?\` handling. Relies on the Rust standard library; the shell calls (`SHFileOperationW`, `SHCreateItemFromParsingName`) keep their own limits — not verified | `core/src/file_ops.rs:195-223`, `core/src/shell_context_menu.rs:125-127` |
| Network shares | No special path. Every entry costs one more round trip (the per-entry stat) | `core/src/file_ops.rs:90-91` |
| File kind, icon, owner, permissions, creation date | Not in the record. Thumbnails and icons are C++ (`core/Cargo.toml:79-81`) | — |

### 1.3 Cost

- A folder of N entries costs 1 `read_dir` + N `stat` calls, then serialises all N entries to one JSON string that the UI parses
  again (`bindings/src/services/directory.rs:125-130`). No streaming, no partial results, no cancel: navigating away from a slow
  share leaves the blocking task running to its end, and a late result can overwrite a newer one because nothing ties a result
  to the request that asked for it (`bindings/src/services/directory.rs:117-140`; compare the `epoch` guard that search has,
  `bindings/src/services/search.rs:220-232`).
- The tree view re-serialises the entire flat node list on every expand / collapse and, on refresh, re-lists the root and every
  expanded folder one after another (`bindings/src/services/directory_tree.rs:11-15`, `:317-331`).
- The listing itself runs off the UI thread (`spawn_blocking`, `bindings/src/services/directory.rs:124`).

### 1.4 File watching

**Not found.** The `notify` crate is declared for "live index updates" (`core/Cargo.toml:33-39`) but no source file in `core/`
or `bindings/` uses it, and no `QFileSystemWatcher`, FSEvents or `ReadDirectoryChangesW` use exists in `app/`. A view refreshes
only when:

| Trigger | Evidence |
|---|---|
| A copy / move / delete started by this app completes and reports its `changed_dirs` | `core/src/events.rs:225-229`, `bindings/src/services/file_ops.rs:1173-1179`, `app/qml/Ufb/App/FileBrowser.qml:1983-2001` |
| `create_directory`, `rename_path`, `create_web_link` succeed (emitted synchronously) | `bindings/src/services/file_ops.rs:357,510,531` |
| An archive job ends | `bindings/src/services/archive.rs:190-204`, `app/qml/Ufb/App/Main.qml:1895-1900` |
| Two fallback timers, 350 ms then 1500 ms after a local operation | `app/qml/Ufb/App/FileBrowser.qml:767-793` |
| F5 | `app/qml/Ufb/App/FileBrowser.qml:1100-1107` |

A file added by another program, another machine on the share, or a render finishing does not appear until one of those happens.
Template and date-prefixed folder creation do not emit `dirs_changed` at all and depend on the timers
(`app/qml/Ufb/App/FileBrowser.qml:767-771`; `bindings/src/services/file_ops.rs:653-671,798-852`).

---

## 2. File operations

### 2.1 The simple ones

| Operation | Implementation | Notes | Evidence |
|---|---|---|---|
| New folder | `create_dir_all` | Succeeds silently when the folder already exists; creates missing parents | `core/src/file_ops.rs:137-139` |
| Rename | `std::fs::rename(old, new)` | **No existence check**: renaming onto an existing file replaces it. The UI only checks that the name changed | `core/src/file_ops.rs:142-144`, `app/qml/Ufb/App/FileBrowser.qml:1639-1650` |
| Free name for a new folder | `base`, then `base` + `a`…`z`; after 26 collisions returns the colliding name | `core/src/file_ops.rs:735-748` |
| Duplicate | Not found | — |
| New web link | Writes a `.url` file, suffix ` 2`, ` 3`… on collision; never overwrites | `core/src/file_ops.rs:1498-1529` |
| Open | `opener::open`; `.url` / `.webloc` are parsed and their target opened; on macOS eleven video / image extensions are sent to a sibling player app first | `core/src/file_ops.rs:1557-1593`, `:1534-1536` |
| Reveal | macOS: `NSWorkspace.activateFileViewerSelectingURLs` (multi-select). Windows: first path only | `core/src/file_ops.rs:688-728` |

Project-specific creators that live in the same file and belong to the project-management reader: date-prefixed folders and notes,
template copy, folder-layout detection (`core/src/file_ops.rs:758-891`, `:1005-1346`; `core/src/jobs.rs:46-89`).

### 2.2 Copy and move — algorithm

Both are `async fn` that hand the whole job to one `tokio::task::spawn_blocking` closure and loop over the sources one at a time
(`core/src/file_ops.rs:1698-1838`, `:1841-2013`).

| Aspect | Behaviour | Evidence |
|---|---|---|
| Engine | `fs_extra::file::copy_with_progress` / `fs_extra::dir::copy_with_progress`, and the `move_*` twins | `core/src/file_ops.rs:1769,1791,1937,1959` |
| Chunk size | 64,000 bytes (library default, never changed) | `fs_extra 1.3.0 src/file.rs:32` |
| Write path | Destination opened with `File::create` (truncating) and written in place — no temporary name, no rename at the end, no `fsync` | `fs_extra 1.3.0 src/file.rs:196-222` |
| Verification | None (no size or checksum comparison after the copy) | same |
| Resume | None | — |
| Conflict policy | `overwrite = true`, unconditionally. No prompt, no skip, no keep-both | `core/src/file_ops.rs:1765,1787,1933,1955` |
| Folder onto an existing folder of the same name | `copy_inside = true` with an existing target makes the library push the folder name once more: the copy lands **nested** (`dest/name/name`), it is not merged | `core/src/file_ops.rs:1764-1769`; `fs_extra 1.3.0 src/dir.rs:1227-1229` |
| Folder pre-scan | The whole source tree is walked before the first byte moves; no progress is reported during it | `fs_extra 1.3.0 src/dir.rs:1231` |
| Move | Always copy, then delete the source — `std::fs::rename` is never tried, so a move inside one volume costs a full copy and double the space | `fs_extra 1.3.0 src/file.rs:298-305`, `src/dir.rs:1376-1378` |
| Move inside a sync root | The one exception: a plain `rename` when source and target are in the same configured sync root | `core/src/sync_aware.rs:50-67`, `core/src/file_ops.rs:1915-1929` |
| Copy inside a sync root (Windows) | Creates a cloud placeholder instead of copying bytes; removes an existing target first | `core/src/sync_aware.rs:71-115` |
| Same-file guard | Skips an item whose target equals its source (the truncating open would empty it). Skipped items are counted neither as done nor as failed | `core/src/file_ops.rs:1666-1695`, `:1741-1748` |
| Folder into itself or into its own descendant | No guard | not found |
| Cancellation | None. No token, no bridge call; the library's abort hook is never used | `core/src/file_ops.rs:1781`; bridge surface `bindings/src/services/file_ops.rs:36-323` |
| Permissions | Not copied by the progress variant | `fs_extra 1.3.0 src/file.rs:196-222` |
| Timestamps, ACLs, extended attributes, resource forks, Finder tags, alternate data streams | Not preserved (the destination is a new file written by plain read / write) | same |
| Sparse files | Expanded to full size | same |
| Symbolic links | Followed: the target's content is copied; a link to a folder is copied as a full tree | `core/src/file_ops.rs:1763` (`is_dir` follows links) |
| Error handling | Per source: the error text is collected and the loop continues. Inside a folder, the first error aborts that folder | `core/src/file_ops.rs:1808-1811`, `:1781` |
| Result | All sources failed → `op_error`. Otherwise `op_completed` with counts and the error strings | `core/src/file_ops.rs:1821-1835` |

**A share dropping mid-operation.** Copy: the destination file exists, truncated to what was written; if it replaced an older
file, that file's content is already gone (truncating open). Move of a folder: files already moved have been deleted from the
source one by one, so the tree is left split between source and destination; nothing records how far it got. Not tested here —
derived from the code paths above.

### 2.3 Progress and the task model

- Events are a trait with four calls — started, progress, completed, error (`core/src/events.rs:240-245`). Progress carries the
  bytes of the **current source item** only, the current file name and item counters, throttled to one tick per 100 ms
  (`core/src/events.rs:205-215`, `core/src/file_ops.rs:1770-1780`). There is no total-bytes figure for the whole job, no rate,
  no time remaining.
- Operation ids are `"<verb>-<pid>-<counter>"` (`core/src/file_ops.rs:1660-1664`).
- The bridge keeps a process-wide map of live operations and republishes the whole map as JSON on every tick
  (`bindings/src/services/file_ops.rs:1002-1032`, `:1082-1092`); the overall fraction is computed there, each source item
  weighted equally whatever its size (`:1133-1144`).
- **There is no queue for file operations.** Every call spawns its own task at once; any number run in parallel, including
  several onto the same destination (`bindings/src/services/file_ops.rs:415-449`).
- **Errors do not reach the user.** On completion or failure the operation is removed from the map and the error strings go to
  the log only (`bindings/src/services/file_ops.rs:1150-1189`).
- The "task queue" the user sees holds archive and transcode jobs only. Those are two separate in-memory lists, each drained by
  its own worker that polls every 500 ms; nothing is persisted across a restart (`core/src/archive.rs:100-104`, `:248-267`;
  `core/src/transcode.rs:38-42`, `:163-183`).

### 2.4 Clipboard and drag payloads

| Aspect | Behaviour | Evidence |
|---|---|---|
| Copy to clipboard | Windows: `CF_HDROP` + `Preferred DropEffect`. macOS: one `NSURL` per file through `writeObjects:` | `core/src/file_ops.rs:337-375`, `:384-412` |
| Cut | Windows writes the move effect, so Explorer honours it. macOS has no such flag: a process-global record of "what we cut" plus the pasteboard change count decides | `core/src/file_ops.rs:242-305`, `:326-335` |
| Paste | Reads paths and the copy / move intent in one pass; falls back to path-looking lines of plain text | `core/src/file_ops.rs:522-551`, `:462-493` |
| "Can paste?" for menus | Windows: format list only, never renders the data (a busy clipboard owner would stall the UI). macOS: performs the full read | `core/src/file_ops.rs:570-588` |
| `text/uri-list` | Built in the bridge; a Windows path with no drive is dropped rather than emitted as a four-slash URL that wedges system drag-and-drop | `core/src/utils.rs:223-254`, `bindings/src/services/file_ops.rs:375-395` |
| Same volume (to choose copy or move on drop) | Windows: drive letter or `\\server\share` string compare. macOS: compares only the **first path component**, so every absolute path is "the same volume" | `bindings/src/services/file_ops.rs:939-962` |
| External drops | Always copy | `app/qml/Ufb/App/FileBrowser.qml:737-753` |

---

## 3. Trash / Recycle Bin and undo

| Aspect | Behaviour | Evidence |
|---|---|---|
| Normal path | `trash::delete(path)` per item (`trash` 5.2.5). Which macOS mechanism the crate uses (Finder scripting or `NSFileManager`), and therefore whether "Put Back" works, is not verified | `core/src/file_ops.rs:147-149`, `core/Cargo.toml:61` |
| Fallback, Windows | `SHFileOperationW(FO_DELETE, FOF_ALLOWUNDO)` — the system shows its own "delete permanently?" dialog for locations with no Recycle Bin. Called from a worker thread with no owner window | `core/src/file_ops.rs:194-223`, `:2035-2067` |
| Fallback, macOS | `remove_dir_all` / `remove_file` — **permanent, with no further question**. The only confirmation the user saw says "Move N items to the recycle bin?" | `core/src/file_ops.rs:162-173`, `app/qml/Ufb/App/FileBrowser.qml:1654-1674` |
| Why trashing failed | Discarded (`map_err(|_| ())`) | `core/src/file_ops.rs:148` |
| User cancels the Windows dialog | Returns `Ok`, and every item in the fallback batch is counted as deleted | `core/src/file_ops.rs:215-218`, `:2059-2060` |
| Progress | Item counter only | `core/src/file_ops.rs:2041-2049` |
| Permanent delete as a deliberate action | Not found | — |
| Undo / redo of any operation | **Not found.** No journal, no history | — |
| Empty trash, restore | Not found | — |

---

## 4. Archives

Source: `core/src/archive.rs` (1,442 lines), `bindings/src/services/archive.rs`, `devdocs/archive-7zip.md`.

| Aspect | Behaviour | Evidence |
|---|---|---|
| Extract formats | 22 extensions, matched on the last extension, case-insensitive: zip, 7z, rar, tar, gz, tgz, bz2, tbz, tbz2, xz, txz, zst, tzst, lz, lzma, z, cab, iso, wim, arj, lzh, cpio | `core/src/archive.rs:35-38`, `:47-56` |
| Create | ZIP only | `core/src/archive.rs:437-440` |
| Tool | 7-Zip CLI: `7zz` on macOS (built from source for the deployment target), `7z.exe` + `7z.dll` on Windows | `devdocs/archive-7zip.md:17-29` |
| Tool lookup | Beside the executable, then `PATH` (`7zz`, `7z`, `7za`), then the bare name so the spawn error names it | `bindings/src/services/archive.rs:101-124`, `bindings/src/services/transcode.rs:109-132` |
| Model | One worker, one child process at a time; states Queued → Processing → Completed / Failed / Cancelled; queue in memory | `core/src/archive.rs:65-73`, `:248-267` |
| Extract switches | `x -y -p -aou -bso0 -bse2 -bsp1 -sccUTF-8 -o<dest> -- <archive>` (`-aou` = auto-rename, never overwrite; `-p` with an empty password makes encrypted archives fail instead of prompting; `--` ends switches) | `core/src/archive.rs:306-311` |
| Planning | The archive is listed first (`l -slt -ba`) and the record stream parsed for `Path` / `Folder` | `core/src/archive.rs:323-357`, `:766-801` |
| Placement | A single top-level entry that does not exist beside the archive → straight into the parent. Otherwise a new sibling folder `<stem>`, `<stem> 2`, … | `core/src/archive.rs:342-356`, `:862-878` |
| Compressed tar | Two processes piped: `x -so` into `x -si -ttar`; the inner one is the killable child | `core/src/archive.rs:375-389`, `:601-660` |
| Compress placement | Beside the shallowest selected item; one item → `<name>.zip`, several → `Archive.zip`, ` 2` / ` 3` on collision, re-resolved when the job starts | `core/src/archive.rs:886-916`, `:412-423` |
| Compress grouping | One `7z a` per source folder so the archive stays flat; if base names collide, one pass relative to the common ancestor. `.DS_Store` excluded | `core/src/archive.rs:921-973`, `:438` |
| Progress | The `-bsp1` stream is parsed incrementally (segments split on backspace / CR / LF); emitted on a change of percent or every 150 ms | `core/src/archive.rs:713-755`, `:1011-1076` |
| Exit codes | 0 done, 1 done with a warning, ≥ 2 failed; stderr reduced to its first `ERROR:` / `WARNING:` line; four known causes get a plain sentence | `core/src/archive.rs:1081-1128` |
| Password | Not supported: a readable failure ("password-protected") | `core/src/archive.rs:1095-1097`; follow-up in `devdocs/archive-7zip.md:63` |
| Cancel | Kills the child, then removes what the job created — always a path that did not exist when the job was planned — plus 7-Zip's `.tmp` staging file | `core/src/archive.rs:216-232`, `:495-531`, `:1149-1166` |
| Refresh | A finished job reports its parent folder; the bridge emits "completed" then "dirs changed" | `core/src/archive.rs:314-318`, `bindings/src/services/archive.rs:190-204` |

**Extraction safety.** The app itself does not validate entry paths. Its one defence is in planning: entries that are absolute,
contain `..` or contain `:` veto the "extract straight into the parent" shortcut, so such an archive always lands in a fresh
folder (`core/src/archive.rs:812-832`). Inside that folder it relies on 7-Zip's own sanitising. Symbolic links stored in an
archive get 7-Zip's default treatment; the dev doc lists that as open (`devdocs/archive-7zip.md:64`). A link pointing outside
the destination followed by a later entry written through it is therefore not excluded by this code — not verified against the
bundled 7-Zip version.

Licensing touch point: 7-Zip is LGPL with the unRAR restriction; the app extracts RAR and only ever creates ZIP
(`devdocs/archive-7zip.md:48-59`).

---

## 5. Transcode

Source: `core/src/transcode.rs` (470 lines).

| Aspect | Behaviour | Evidence |
|---|---|---|
| Offer | One action, "Transcode to MP4", gated in the UI by a list of video extensions | `app/qml/Ufb/App/FileBrowser.qml:410-427`, `:1194` |
| Output | `<source folder>/MP4/<stem>.mp4` | `core/src/transcode.rs:96-100` |
| Preset | Exactly one, hard-coded: `-c:v libx264 -pix_fmt yuv420p -crf 25 -preset fast -c:a aac -b:a 192k -y` | `core/src/transcode.rs:333-354` |
| Frame count | `ffprobe -count_packets` on the first video stream, for the percentage. This reads the whole file once before encoding starts | `core/src/transcode.rs:290-323` |
| Progress | `-progress pipe:1`, lines `frame=` and `fps=` | `core/src/transcode.rs:370-402` |
| Metadata | `exiftool -TagsFromFile <in> -overwrite_original <out>`; a failure is logged and ignored | `core/src/transcode.rs:438-460`, `:274-276` |
| States | Queued → Processing → CopyingMetadata → Completed / Failed / Cancelled | `core/src/transcode.rs:15-22` |
| Cancel | Kills the child and deletes the output file | `core/src/transcode.rs:124-145`, `:244-250` |
| Tools | `ffmpeg`, `ffprobe`, `exiftool` beside the executable (exiftool under the bundle's resources on macOS), else `PATH` | `bindings/src/services/transcode.rs:109-132`, `:142-144` |
| Diagnostics | ffmpeg's stderr is discarded; a failure reports the exit code only | `core/src/transcode.rs:356`, `:432` |

Not offered: any choice of codec, size, rate, audio layout, colour handling, image sequences as input, a destination, or a name.

---

## 6. Search

### 6.1 Two back ends, one at a time

| | OS mode (default) | Indexed mode |
|---|---|---|
| What is searched | File and folder **names** | Names (with extension). No contents, no metadata |
| Windows | Everything's CLI at two fixed install paths; the query is `"<scope> <query>"` as one argument; `-max-results 200`; blocking, cannot be cancelled. If the tool is absent: the walk below | — |
| macOS | `mdfind -onlyin <scope> "kMDItemDisplayName == '*<q>*'c"`, quotes stripped from the input; if it returns nothing: the walk below | — |
| Fallback walk | Breadth-first, case-insensitive substring, no descent into dot-folders or linked folders, stops at 200 hits, results streamed in batches of 25, cancellable; refuses to run without a scope | — |
| Index | — | Tantivy, one index per root under `<app data>/index/<md5 of the root path>` |
| Tokeniser | — | N-grams of 2–3 characters, lower-cased; the query's grams are AND-ed |
| Limit | 200 | 500 |
| Evidence | `core/src/search.rs:10-14`, `:55-88`, `:93-108`, `:114-168` | `core/src/search_index.rs:48-59`, `:79-109`, `:222-253`; `bindings/src/services/search.rs:25` |

### 6.2 Freshness and cost of the index

- The index is **rebuilt from scratch every time a search session starts** — `delete_all_documents`, then a full crawl with
  `ignore::WalkBuilder` (hidden files included, links not followed), committing every 4,000 documents so results fill in
  (`core/src/search_index.rs:132-187`; `bindings/src/services/search.rs:125-159`).
- Incremental `upsert` / `remove` exist (`core/src/search_index.rs:190-218`) but **nothing calls them**: there is no watcher.
  Each would open a 64 MB writer and commit for a single path (`:51`, `:192-201`).
- One process-wide write lock: a second pane that starts an indexed search waits for the first pane's crawl
  (`core/src/search_index.rs:38-46`).
- Index folders are never pruned — no clean-up code found.
- A hit is rebuilt from stored fields without touching the disk (`core/src/search_index.rs:311-334`), so results can name files
  that no longer exist.
- AND-ed n-grams are a superset of "contains the query": a name holding the grams in separate places also matches; there is no
  check afterwards (`core/src/search_index.rs:222-253`). Order is the engine's relevance score, not name or depth.

### 6.3 Query syntax and scope

No syntax: no wildcards, no extension or date or size filters, no AND / OR, no "this folder only". The scope is always the
pane's current root, recursively (`bindings/src/services/search.rs:37-39`). Queries shorter than 2 characters return nothing in
indexed mode (`core/src/search_index.rs:263-278`). A session-and-query generation counter discards stale results
(`bindings/src/services/search.rs:79-85`, `:220-232`).

---

## 7. Volumes and bookmarks

### 7.1 Volumes

`core/src/volumes.rs` is **not** a device enumerator. It holds the path-identity registry: a deterministic UUID per configured
(Windows root, macOS root) pair (`:12-30`), views built from the user's path mappings (`:46-57`), an overlay of where shares are
mounted right now, keyed by the lower-cased last path component (`:136-241`), and a three-way mode switch (`:113-134`). It
belongs with the metadata / identity reader.

| Wanted | Status |
|---|---|
| List of local drives and mounted volumes (per OS) | Not found in `core/` or `bindings/` |
| Free and total space | Not found |
| Eject / unmount of a removable volume | Not found (mount start / stop for configured shares is the mounts reader's area) |
| Network volumes | Only the app's own configured mounts, through the mount service |
| Detect "is this path on a network or removable volume" | Not found here |

The sidebar is fed by three sources only: three fixed rows built from the home folder (Desktop, Downloads, Documents), the
bookmarks table, and the mount service's state (`app/qml/Ufb/App/Sidebar.qml:107-123`, `:141-143`, `:178-180`).

### 7.2 Bookmarks

| Aspect | Behaviour | Evidence |
|---|---|---|
| Store | Table `bookmarks` in the shared SQLite database: `id, path, display_name, created_time, is_project_folder` | `core/src/bookmarks.rs:5-13`, `:43-49` |
| Path form | Stored as a tagged identity (`vol:<uuid>/<rel>` or `native:<os>/<path>`), translated back to a native path when read | `core/src/bookmarks.rs:15-21`, `bindings/src/services/bookmarks.rs:89` |
| Operations | Add (ignored if the path exists), remove, rename, list ordered by display name | `core/src/bookmarks.rs:32-110` |
| Order | Alphabetical only; no user ordering, no groups | `core/src/bookmarks.rs:81` |
| Cost | Every add / remove / rename re-reads and re-parses the settings file to get the mappings | `core/src/bookmarks.rs:18-21` |
| Missing target | Not checked | not found |

---

## 8. Native shell menu and "open with"

| OS | Mechanism | Evidence |
|---|---|---|
| Windows | `SHCreateItemFromParsingName` → `IContextMenu` (`BindToHandler(BHID_SFUIObject)`) → `QueryContextMenu` → `TrackPopupMenuEx` at the cursor → `InvokeCommand`. Runs on its own apartment thread with a hidden zero-size popup window as owner; fire-and-forget so the UI never waits; a flag prevents a second menu stacking on the first | `core/src/shell_context_menu.rs:23-67`, `:70-184` |
| Windows limits | **One path only** — a multi-selection gets the menu of one item. Extended verbs (Shift) not requested. The result of the chosen verb is only logged | `core/src/shell_context_menu.rs:23`, `:137`, `:165-167` |
| macOS | Not in the core (the function returns an error by design). The menu is built in Objective-C++ from public APIs: Open, Open With (LaunchServices), Show in Finder, Get Info, Share, Tags. Third-party Finder extensions and Quick Actions are not reachable this way | `core/src/shell_context_menu.rs:186-198`, `app/MacShellMenu.h:1-16` |
| "Open with" on Windows | Only through the shell menu above; no own chooser | not found |
| Quick Look | macOS only: spawns `/usr/bin/qlmanage -p <paths>` | `bindings/src/services/paths.rs:158-200` |

---

## 9. Settings

File: `settings.json` in `<local config dir>/ufb/` — the per-user local application-data folder on Windows, the per-user
Application Support folder on macOS (`core/src/utils.rs:3-11`, `:95-97`). The same folder holds the database, `device_id.txt`,
the search indexes and the `.bak` / `.tmp` siblings (`core/src/utils.rs:68-92`, `core/src/search_index.rs:55-59`).

JSON keys are camelCase; snake_case aliases are accepted on read (`core/src/settings.rs:218-276`).

| Key | Type | Default | Read by the bridge? | Evidence |
|---|---|---|---|---|
| `window.{x,y}` | i32 | -1 | yes | `settings.rs:8-19`, `:284-294` |
| `window.{width,height}` | u32 | 1914 × 1060 | yes | same |
| `window.maximized` | bool | false | yes | same |
| `panels.{showSubscriptions, showBrowser1, showBrowser2}` | bool | true | no accessor found | `settings.rs:21-34`, `:296-306` |
| `panels.showTranscodeQueue` | bool | false | no accessor found | same |
| `panels.useWindowsAccent` | bool | true | no accessor found | same |
| `appearance.useWindowsAccentColor` | bool | true | no accessor found | `settings.rs:36-49`, `:308-318` |
| `appearance.customAccentColorIndex` | i32 | -1 | no accessor found | same |
| `appearance.customPickerColor{R,G,B}` | f32 | 0.5 | no accessor found | same |
| `ui.fontScale` | f32 | 1.0 | no accessor found | `settings.rs:72-81`, `:320-328` |
| `ui.browserPanelRatios` | [f32] | [0.20, 0.40, 0.40] | no accessor found | same |
| `ui.browserColumns.{size, modified, type}` | bool | true | no accessor found | `settings.rs:51-70` |
| `sync.enabled` | bool | false | no accessor found | `settings.rs:105-110` |
| `meshSync.{nodeId, farmPath, tags}` | string | "" | yes (mesh reader's area) | `settings.rs:83-103` |
| `meshSync.{httpPort, dataPort}` | u16 | the mesh module's defaults | yes | `settings.rs:90-95`, `:584-590` |
| `meshSync.enabled` | bool | false | yes | `settings.rs:98-102` |
| `pathMappings[]` `{win, mac, enabled (true), label}` | list | [] | yes | `settings.rs:112-137` |
| `jobViews[]` `{jobPath, jobName}` | list | [] | no accessor found | `settings.rs:139-146` |
| `aggregatedTrackerOpen` | bool | false | no accessor found | `settings.rs:237-238` |
| `tabState.currentIndex` | i32 | 0 | yes | `settings.rs:198-216` |
| `tabState.files[]` `{leftPath, rightPath, leftCollapsed, rightCollapsed}` | list | [] | yes | `settings.rs:148-162` |
| `tabState.jobs[]` `{jobPath}` | list | [] | yes | `settings.rs:164-173` |
| `tabState.{trackerOpen, transcodeOpen}` | bool | false | yes | `settings.rs:212-215` |
| `splitterState.{outerSidebarWidth, innerLeftWidth}` | i32 | 0 (= use the UI default) | yes | `settings.rs:175-189` |
| `templatesMigratedV1`, `templatesV2Migrated` | bool | false | one-shot migration flags | `settings.rs:245-260` |
| `previewLoop` | bool | false | yes | `settings.rs:261-265` |
| `identityLiveRoots` | string: off / shadow / live | "live" | yes | `settings.rs:266-282` |

Bridge accessors: `bindings/src/services/settings.rs:28-154`. A grep of `bindings/src` and `app/qml` for the "no accessor found"
keys returns nothing: they are leftovers of the earlier front end, still written back on every save.

**Per-view state is not in this file.** View mode, grid size, sort and column widths are JSON blobs in the database table
`browser_folder_prefs (path, prefs, updated_at)`, keyed by the lower-cased identity form of the folder plus reserved `view:*`
keys; writers merge; pruned to the newest 2,000 rows on roughly one write in 64
(`bindings/src/services/settings.rs:126-147`, `:185-215`, `:431-470`).

**Load, save, migration.**

| Step | Behaviour | Evidence |
|---|---|---|
| Load | Main file; if unreadable or unparsable, the `.bak`; else defaults | `settings.rs:359-381` |
| Save | Write + `fsync` a `.tmp`, rotate the main file to `.bak`, rename `.tmp` into place | `settings.rs:473-523` |
| Unknown keys | Dropped on the next save (typed structs, no catch-all) | `settings.rs:191-196` |
| Migration 1 | Mesh ports: three old default values are moved to the current defaults | `settings.rs:414-449` |
| Migration 2 (macOS) | A mapping that starts with the user's home folder is rewritten to `~/…` | `settings.rs:528-551` |
| Side effect | `load()` **writes the file** when a migration fired | `settings.rs:375-379` |
| Schema version field | None | — |

`AppSettings::load()` is called from many places as a getter — every bookmark write, every folder-prefs key, the volume
registry's first touch — each a disk read and a JSON parse, often on the UI thread (`core/src/bookmarks.rs:19`,
`bindings/src/services/settings.rs:191`, `core/src/volumes.rs:148-150`). The bridge also keeps a cache and documents that other
objects bypass it, so it reloads from disk before each mutate-and-save to avoid clobbering them
(`bindings/src/services/settings.rs:161-180`).

---

## 10. Events, state and threads

### 10.1 Event bus

There is no bus. `core/src/events.rs` defines five traits — mesh, transcode, archive, mount, file operations — each
`Send + Sync + 'static`, each with a no-op implementation for tests (`core/src/events.rs:19-42`, `:67-74`, `:91-98`, `:118-173`,
`:240-256`). The bridge implements each as: update a process-global map, serialise it to JSON, queue a closure onto the UI
thread that sets a string property (`bindings/src/services/file_ops.rs:1077-1092`). There is no event for "this folder changed
on disk", "a volume appeared", or "the clipboard changed".

### 10.2 `app_state.rs`

`AppCore` bundles the database, a settings cache, the managers and the event handles (`core/src/app_state.rs:58-79`). **It is
not used**: no `AppCore::new` call exists outside its own file; the bridge builds each service as its own lazily initialised
global instead (`bindings/src/db.rs:17-36`, `bindings/src/services/settings.rs:161-165`,
`bindings/src/services/transcode.rs:134-160`, `bindings/src/services/archive.rs:126-136`). It also has no archive manager and no
archive events (`core/src/app_state.rs:38-44`). `lib.rs` still describes the crate as a port in progress
(`core/src/lib.rs:1-5`, `:25`).

### 10.3 Threads

| Work | Where it runs | Evidence |
|---|---|---|
| Runtime | One multi-thread tokio runtime for the process, started on first use | `bindings/src/runtime.rs:11-20` |
| Listing, tree listing, search, index crawl | `spawn_blocking` | `bindings/src/services/directory.rs:124`, `directory_tree.rs:214,259,317`, `search.rs:136,226` |
| Copy / move / delete | `tokio::spawn` + `spawn_blocking` | `bindings/src/services/file_ops.rs:426,444,466`; `core/src/file_ops.rs:1723,1872,2035` |
| Archive / transcode workers | Async tasks driving child processes | `core/src/archive.rs:250`, `core/src/transcode.rs:165` |
| Windows shell menu | A dedicated OS thread | `core/src/shell_context_menu.rs:57-65` |
| Reveal a deep link | A bare `std::thread` (the existence check can wait out a share time-out) | `bindings/src/services/file_ops.rs:738-742` |

### 10.4 What blocks the UI thread

Synchronous bridge calls that touch the disk, a share, or another process from the UI thread:

| Call | Why it can stall | Evidence |
|---|---|---|
| `create_directory`, `rename_path` | A file-system call on a possibly slow share | `bindings/src/services/file_ops.rs:503-539` |
| `copy_template_to` | A **recursive tree copy** | `bindings/src/services/file_ops.rs:798-816`, `core/src/file_ops.rs:1288-1336` |
| `create_date_prefixed_item` / `_note`, `create_ufb_folder`, `find_available_path` | Read a folder, create entries; the note creator runs SQLite on the target | `bindings/src/services/file_ops.rs:644-671`, `:818-852` |
| `detect_folder_layout_mode`, `find_*_subdir` | Up to 11 folder reads, each with the Windows retry sleeps | `bindings/src/services/file_ops.rs:746-780`, `core/src/file_ops.rs:1049-1085` |
| `Paths.path_exists`, `Paths.is_directory` | `stat` on a share | `bindings/src/services/paths.rs:124-144` |
| `open_file` | On macOS waits for `open -a … ` to exit; binary `.webloc` runs `plutil` | `core/src/file_ops.rs:1572-1576`, `:1404-1407` |
| `clipboard_paste_intent` (and "can paste?" on macOS) | Renders clipboard data from its owner | `core/src/file_ops.rs:553-588` |
| Archive / transcode add, cancel, remove, clear | `block_on` an async lock from the UI thread; transcode holds that lock while waiting for the child to exit | `bindings/src/services/archive.rs:233,250,261`, `bindings/src/services/transcode.rs:233,246,255`; `core/src/transcode.rs:406-412` |
| Settings and folder-prefs reads / writes, bookmarks | File and SQLite access | `bindings/src/services/settings.rs:405-470` |

---

## 11. The bridge

| Fact | Evidence |
|---|---|
| The core crate depends on nothing from Qt; it is described as "pure Rust, Qt-unaware" | `core/Cargo.toml:7`, `:13-81`; `core/src/lib.rs:1` |
| The bridge is a separate static library: cxx-qt 0.7, 16 bridge modules registered as one QML module | `bindings/Cargo.toml:14,39-44`, `bindings/build.rs:13-43` |
| Size: 9,892 lines in `bindings/src/services/` | counted with `wc -l` |
| Of which this area: `file_ops` 1,265 · `settings` 525 · `directory_tree` 371 · `archive` 301 · `transcode` 291 · `search` 288 · `paths` 231 · `bookmarks` 149 · `directory` 141 = 3,562 | same |
| No list or tree model crosses the bridge: every collection is a JSON string in a `QString` property, parsed again in QML | `bindings/src/services/directory.rs:8-12`, `directory_tree.rs:4-15` |
| Three `extern "C"` functions besides: a link smoke test and two quit hooks | `bindings/src/lib.rs:44-78` |
| `AppController.h` is small and UI-only: a banner string, deep-link delivery, Dock icon visibility | `app/AppController.h:31-70` |

**Logic that lives in the bridge and would be lost with it** (it is not in the core):

| Logic | Evidence |
|---|---|
| Same-volume test, `file://` URL → path, percent-decoding | `bindings/src/services/file_ops.rs:895-993` |
| Deep-link resolution (scheme normalising, mapping translation) | `bindings/src/services/file_ops.rs:687-728` |
| Live-operations map and the overall progress formula | `bindings/src/services/file_ops.rs:1002-1190` |
| Tool lookup (`bundled_tool`, 7-Zip resolution) | `bindings/src/services/transcode.rs:109-132`, `archive.rs:101-124` |
| Search session: generations, cancel, streaming | `bindings/src/services/search.rs:108-288` |
| Tree flattening, expand / collapse / refresh | `bindings/src/services/directory_tree.rs:107-370` |
| Per-view prefs store | `bindings/src/services/settings.rs:185-215`, `:405-470` |
| Quick Look, default cache folder | `bindings/src/services/paths.rs:158-230` |

**How much of the core is UI-agnostic today:** all of it, by construction. How much is worth keeping is section 15.

---

## 12. External tools, crates, unsafe code, platform splits

### 12.1 External programs

| Program | Purpose | Shipped? | Evidence |
|---|---|---|---|
| 7-Zip CLI | Extract / compress | Bundled | `devdocs/archive-7zip.md:17-29` |
| ffmpeg, ffprobe | Transcode, frame count | Bundled | `bindings/src/services/transcode.rs:142-143` |
| exiftool | Copy metadata onto the transcode | Bundled | `bindings/src/services/transcode.rs:144` |
| Everything CLI | Windows search | **Not bundled** — used only if the user installed it at one of two fixed paths | `core/src/search.rs:67-72` |
| `mdfind`, `plutil`, `open`, `qlmanage` | macOS search, binary link files, hand-off to the player app, Quick Look | System | `core/src/search.rs:94`, `core/src/file_ops.rs:1404,1572`, `bindings/src/services/paths.rs:179` |

### 12.2 Crates used by this area

| Crate | Purpose | Evidence |
|---|---|---|
| `fs_extra` 1.3 | Copy / move with progress; template copy | `core/src/file_ops.rs:1769-1959`, `core/src/jobs.rs:77-81` |
| `trash` 5 | OS trash | `core/src/file_ops.rs:148` |
| `arboard` 3 | Plain-text clipboard | `core/src/file_ops.rs:482,1352,1361` |
| `clipboard-win` 5 (Windows) | `CF_HDROP` | `core/src/file_ops.rs:339,497,592` |
| `objc2`, `objc2-app-kit`, `objc2-foundation` (macOS) | Pasteboard, reveal | `core/src/file_ops.rs:273,386-388,708-709` |
| `windows` 0.58 (Windows) | Shell delete, clipboard formats, shell menu, final-path query | `core/Cargo.toml:111-132` |
| `cloud-filter` (Windows) | Placeholder copy inside a sync root | `core/src/sync_aware.rs:73-74` |
| `opener` 0.7 | Open, reveal | `core/src/file_ops.rs:675,702,1564,1592` |
| `tantivy` 0.22, `ignore` 0.4, `md5`, `dunce`, `once_cell` | Search index | `core/src/search_index.rs:28-59` |
| `tokio` (full) | Runtime, child processes | `core/src/archive.rs:27-28`, `core/src/transcode.rs:5-6` |
| `rusqlite` 0.31 (bundled) | Bookmarks; the note file writer | `core/src/bookmarks.rs:42`, `core/src/file_ops.rs:931` |
| `uuid`, `chrono`, `urlencoding`, `dirs`, `serde`, `serde_json`, `log` | Ids, dates, link encoding, folders, records, logging | `core/Cargo.toml:14-45` |
| Declared, with no use found in `core/src`: `notify`, `rfd`, `glob`, `flume`, `base64`, `hostname` | — | `core/Cargo.toml:31,39,48,51,71,74` |

### 12.3 Unsafe / FFI

| Where | What | Evidence |
|---|---|---|
| Shell delete | `SHFileOperationW` with a hand-built double-NUL list | `core/src/file_ops.rs:199-213` |
| Clipboard, Windows | Sequence number; `GlobalAlloc` / `GlobalLock` / `SetClipboardData` for the drop effect (if `SetClipboardData` fails the memory is leaked); reading it back; format probe | `core/src/file_ops.rs:264-266`, `:360-374`, `:639-654`, `:577-580` |
| Pasteboard, macOS | `readObjectsForClasses_options` | `core/src/file_ops.rs:426` |
| Shell menu | COM apartment, window class, `IContextMenu`, verb passed as an integer cast to a pointer | `core/src/shell_context_menu.rs:40-46`, `:82-183` |
| Final path (packaged Windows build) | `CreateFileW` / `GetFinalPathNameByHandleW` | `core/src/utils.rs:34-48` |
| Bridge | `get_unchecked_mut` on pinned objects in the tree | `bindings/src/services/directory_tree.rs:142,222,233,267,337` |

### 12.4 Platform splits

| Concern | Windows | macOS | Evidence |
|---|---|---|---|
| Listing retry | Error-448 back-off | none | `core/src/file_ops.rs:40-67` |
| Trash fallback | System dialog | Permanent delete | `core/src/file_ops.rs:157-174` |
| Cut semantics | Clipboard flag | Internal record | `core/src/file_ops.rs:522-551` |
| Reveal | One item | Many | `core/src/file_ops.rs:693-703` |
| Open video | Default handler | Player app first | `core/src/file_ops.rs:1569-1591` |
| Shell menu | COM | Elsewhere (Objective-C++) | `core/src/shell_context_menu.rs:22,192` |
| Search | Everything CLI or walk | Spotlight, then walk | `core/src/search.rs:39-51` |
| Sync-aware copy | Placeholder | Declines | `core/src/sync_aware.rs:71-120` |
| Child processes | `CREATE_NO_WINDOW` | — | `core/src/archive.rs:30-31,540-541` |
| Other systems | `current_os_tag()` and `clipboard_copy_paths` have no branch for them: the core does not compile elsewhere | — | `core/src/utils.rs:186-195`, `core/src/file_ops.rs:308-317` |

---

## 13. Tests

| File | Tests | What they cover | Evidence |
|---|---|---|---|
| `core/src/archive.rs` | 7 unit + 1 ignored live test | Extension gate, stems, listing parse and single-root detection, progress segments, zip naming, compress grouping, exit codes; the live test round-trips zip / extract / compressed tar when an environment variable names a 7-Zip binary | `core/src/archive.rs:1172-1322` |
| `core/src/file_ops.rs` | 6 | `parent_dirs` (3); web-link parsing, naming, create-and-read (3) | `core/src/file_ops.rs:1620-1657`, `:2096-2138` |
| `core/src/utils.rs` | 27 | Project slugs, home expansion, mapping translation in every direction, `file://` URLs, identity storage round trips | `core/src/utils.rs:621-974` |
| `core/src/volumes.rs` | 6 | Volume UUIDs, classification, live overlay, database round trip | `core/src/volumes.rs:291-433` |
| `core/tests/minnotes_note.rs` | 1 | The created note is a valid document | `core/tests/minnotes_note.rs:1-12` |
| `core/tests/classify_*.rs`, `migration_v4_dryrun.rs`, `normalize_paths_dryrun.rs` | 5 (3 ignored, need a copy of a real database) | Path-identity migration — other readers' area | `core/tests/*.rs` |

**Untested in this area:** `list_directory`; copy, move and delete (not one test: conflicts, nesting, cross-volume, failure);
trash and its fallback; rename; the clipboard; search and the search index; transcode; settings load / save / migration;
bookmarks; the shell menu; everything in `bindings/`.

---

## 14. Rough spots

No `TODO`, `FIXME`, `HACK` or `XXX` marker exists in any file of this area (grep, 2026-11-15). The problems are in behaviour.

### 14.1 Data-loss risks

| # | Risk | Evidence |
|---|---|---|
| D1 | Copy and move always overwrite, with no prompt, and the destination is truncated before the first byte is read: an interrupted overwrite destroys the old file and leaves a partial new one | `core/src/file_ops.rs:1765,1787,1933,1955`; `fs_extra 1.3.0 src/file.rs:201` |
| D2 | A move is copy-then-delete with no check that the copy is complete; a folder move interrupted half-way leaves the tree split between two places | `fs_extra 1.3.0 src/file.rs:302-305`, `src/dir.rs:1376-1378` |
| D3 | On macOS a failed trash becomes a permanent recursive delete with no second question | `core/src/file_ops.rs:162-173`, `:180-190` |
| D4 | Rename onto an existing name silently replaces that file | `core/src/file_ops.rs:142-144` |
| D5 | Copying or moving a folder onto an existing folder of the same name nests it one level down instead of merging or asking | `core/src/file_ops.rs:1764-1769`; `fs_extra 1.3.0 src/dir.rs:1227-1229` |
| D6 | No guard against moving a folder into its own descendant | not found (compare `core/src/file_ops.rs:1666-1695`) |
| D7 | Transcode writes `MP4/<stem>.mp4` with `-y`: two sources with the same stem, or an existing file, are overwritten; cancel or failure then deletes that path | `core/src/transcode.rs:96-100`, `:352`, `:245-250` |
| D8 | Sync-root placeholder copy removes an existing target before creating the placeholder; a failed create leaves nothing | `core/src/sync_aware.rs:102-111` |
| D9 | Timestamps, permissions, ACLs, extended attributes, resource forks and tags are lost on every copy and every move, including moves inside one volume | section 2.2 |

### 14.2 Races and state

| # | Issue | Evidence |
|---|---|---|
| R1 | A late listing result overwrites a newer one (no request id) | `bindings/src/services/directory.rs:117-140` |
| R2 | Transcode: `remove_job` on a job that is about to start makes the worker `unwrap()` a missing job (panic in the worker task, which then never runs again) | `core/src/transcode.rs:147-150`, `:195-199` |
| R3 | Transcode: removing a running job does not kill its child process | `core/src/transcode.rs:147-150` |
| R4 | Transcode: the state lock is held across `child.wait()`; UI-thread calls that take the lock wait with it | `core/src/transcode.rs:406-412` |
| R5 | Free-name helpers check and then create (two steps): two users on a share can pick the same name; `create_dir_all` then succeeds for both | `core/src/file_ops.rs:735-748`, `:137-139` |
| R6 | Any number of copy / move jobs run at once, with no ordering between jobs that touch the same paths | `bindings/src/services/file_ops.rs:415-449` |
| R7 | Settings: several writers each load, change and save the whole file; the bridge's own comment describes the clobbering this causes | `bindings/src/services/settings.rs:167-180` |
| R8 | The "what we cut" record survives after a move completes; pasting again re-attempts the move (the sources are gone, so it fails per item) | `core/src/file_ops.rs:283-305` — no clear on paste found |
| R9 | A cancelled Windows delete dialog is reported as success | `core/src/file_ops.rs:215-218` |

### 14.3 Where it differs from what Finder / Explorer users expect

| # | Difference | Evidence |
|---|---|---|
| F1 | The view does not update when something else changes the folder | section 1.4 |
| F2 | No conflict dialog (replace / skip / keep both / apply to all) | section 2.2 |
| F3 | No cancel or pause for copy, move, delete | section 2.2 |
| F4 | Failures are not shown | `bindings/src/services/file_ops.rs:1150-1189` |
| F5 | No undo | section 3 |
| F6 | A move on one volume is not instant | section 2.2 |
| F7 | Names sort bytewise after lower-casing: `file10` before `file2`; accented names out of place | `core/src/file_ops.rs:127-131` |
| F8 | No hidden-file policy, no package detection, no alias / shortcut resolution, no link badge | section 1.2 |
| F9 | The confirmation text says "recycle bin" on every OS, including when the delete will be permanent | `app/qml/Ufb/App/FileBrowser.qml:1666-1668` |
| F10 | Search finds names only, capped at 200 / 500, and on Windows needs a third-party tool the user must install, else it walks the tree | section 6 |
| F11 | The Windows shell menu acts on one item of a selection | `core/src/shell_context_menu.rs:23` |
| F12 | No drive list, no free space, no eject | section 7.1 |
| F13 | No "duplicate", no "new file", no "compress to" other than ZIP, no archive password | sections 2.1, 4 |
| F14 | Windows search: the scope and the query are joined with a space into one argument; a scope path containing spaces is not quoted — likely wrong results; not verified against the tool | `core/src/search.rs:61-65`, `:76` |
| F15 | macOS same-volume test is always true for absolute paths, so the copy-versus-move choice on drop cannot follow the volume | `bindings/src/services/file_ops.rs:950-961` |

### 14.4 Duplicated code

| What | Evidence |
|---|---|
| `copy_files` and `move_files`: about 140 lines each, identical but for the library call and logging | `core/src/file_ops.rs:1698-1838`, `:1841-2013` |
| The three date-letter creators (folder, item, note) repeat one slot walk | `core/src/file_ops.rs:758-891` |
| Percent-encoding / decoding and `file://` conversion exist in the core and again in the bridge | `core/src/utils.rs:223-271`, `bindings/src/services/file_ops.rs:895-937` |
| Building a `FileEntry` from metadata: four copies | `core/src/file_ops.rs:96-124`, `core/src/search.rs:170-190`, `:198-218`, `core/src/search_index.rs:282-309` |
| Archive and transcode job managers share a shape (queue, worker, cancel, forwarder) with no common code | `core/src/archive.rs:100-267`, `core/src/transcode.rs:38-183` |
| The default cache folder is computed in three places that "must match" | `bindings/src/services/paths.rs:202-230`, `core/src/sync_aware.rs:122-152` |
| Three event-sink implementations for file operations, one of them dead | `bindings/src/services/file_ops.rs:1037-1069`, `:1095-1190`, `:1229-1265` |

### 14.5 Errors swallowed

| Where | Evidence |
|---|---|
| Unreadable entries vanish from a listing with no count and no message | `core/src/file_ops.rs:81-84`, `:92-95` |
| The reason a trash attempt failed | `core/src/file_ops.rs:148` |
| Every error type is flattened to `String`; nothing downstream can tell "exists" from "denied" from "offline" | throughout, e.g. `core/src/file_ops.rs:71,137,142` |
| Open, reveal, shell-menu and web-link failures are logged, not returned | `bindings/src/services/file_ops.rs:473-501`, `:637-642` |
| Rename and new-folder failures reach QML and stop at `console.warn` | `app/qml/Ufb/App/FileBrowser.qml:1645-1648`, `:1013-1014` |
| ffmpeg's stderr; exiftool failure | `core/src/transcode.rs:356`, `:274-276` |
| A malformed JSON argument makes a bridge call return silently | `bindings/src/services/file_ops.rs:376,401,452-458` |
| User-facing error sentences are English literals built with `format!`; none passes through a translation layer | e.g. `core/src/archive.rs:1093-1113` |

### 14.6 Housekeeping

- `core/src/utils.rs` test fixtures contain what look like real share and job names (`:724-750`). They must not be carried into
  a public repository as they are.
- Dead or stale: `AppCore` (section 10.2), six unused crates (section 12.2), six groups of unread settings keys (section 9),
  module comments that still describe a port in progress (`core/src/lib.rs:1-5,25`, `bindings/src/lib.rs:3-12`).

---

## 15. Reuse: what can be kept, what the platform crates already cover

### 15.1 minplatform crates against this area

| Crate | Already covers | Does not cover |
|---|---|---|
| `min-paths` | Clipboard file lists with copy / move intent (`clipboard.rs`); `file://` URLs and `text/uri-list` both ways (`url.rs`); the `ufb:///` link codec (`links.rs`); cross-OS mapping translation and `{vol, rel}` references with a boundary check (`map.rs`); same volume **by device identity**, the same-file guard **and** the into-own-descendant guard, the packaged-build final path (`volume.rs`); network / volatile source detection by file-system type (`classify.rs`); safe file names with Windows reserved names and the web-link name rule (`names.rs`); image-sequence detection (`sequence.rs`) | Listing, watching, copy / move / delete, trash, volume enumeration, free space, eject, shell menu, open-with |
| `min-settings` | One JSON file per app in the platform config folder, atomic write, typed keys with defaults and clamps, reset, a change generation, unknown keys preserved, an environment override for tests | Nested values and lists: its `Value` is bool / int / float / string only, while ufb's file holds lists of structures (path mappings, tab state). Per-folder view state (ufb keeps it in SQLite) |
| `min-ffmpeg` | Decoding: open / read / seek, frame index, hardware devices | Encoding and muxing — nothing in it writes a file. ufb's transcode shells out to the ffmpeg CLI |
| `min-i18n` | `tr` / `trc` / plurals / placeholders, live language switch, pseudo-localisation, miss recording | — (ufb's core has no translation layer; every core error string would need one) |

So `min-paths` already replaces: the clipboard block of `file_ops.rs` (`:225-655`), the URL and link code in `utils.rs`
(`:197-302`), the mapping translation (`utils.rs:304-453`, `:612-619`), `is_drop_onto_self` (`file_ops.rs:1666-1695`), the
bridge's same-volume and URL helpers (`bindings/src/services/file_ops.rs:895-993`), and `web_link_file_name`
(`file_ops.rs:1465-1494`). Three of its versions fix defects listed above (F15, D6, and the duplicated codecs).

### 15.2 Module by module

| Module | Lines | Verdict | Why |
|---|---|---|---|
| `archive.rs` | 1,442 | **Keep nearly as is** | Clean separation of pure planning and process plumbing, tested, safe placement rules, cancel with clean-up. Needs: typed errors, translated messages, a password path, a symlink policy, a place in a shared job queue |
| `shell_context_menu.rs` | 198 | Keep the Windows half, extend to multi-selection | Correct threading and ownership; one-item limit |
| Web links, open, reveal (`file_ops.rs:657-728`, `:1366-1593`) | ~330 | Keep | Small, tested, cross-OS |
| Clipboard (`file_ops.rs:225-655`) | ~430 | Already in `min-paths` | — |
| `utils.rs` path / URL / link code | ~450 | Already in `min-paths` | — |
| `search_index.rs` | 335 | Keep the schema and query; rebuild the life cycle | Needs incremental updates, a watcher, pruning, a per-index writer, a substring post-filter |
| `search.rs` | 222 | Keep the walk; reconsider the OS back ends | Walk is sound and cancellable; the third-party-tool path is fragile |
| `transcode.rs` | 470 | Rework | One preset, unsafe overwrite, races R2–R4; the queue shape duplicates the archive one |
| `list_directory` (`file_ops.rs:14-134`) | ~120 | **Replace** | Record too thin, no streaming, no cancel, no watcher, lossy names, naive sort |
| Copy / move / delete (`file_ops.rs:146-223`, `:1595-2094`) | ~580 | **Replace** | Every expectation of a file browser is missing (D1–D6, D9, F2–F6); the library underneath cannot provide them |
| `settings.rs` | 590 | Replace the mechanism, keep the atomic-save idea | A third dead; whole-file races; `load()` used as a getter with side effects |
| `bookmarks.rs` | 111 | Keep the table and the identity form (owner's "metadata" side); drop the per-call settings load | — |
| `events.rs`, `app_state.rs` | 439 | Keep the trait idea; drop `AppCore` | Unused container |
| `bindings/` (this area) | 3,562 | **Discard**, after lifting the logic listed in section 11 into the core | Qt-specific by definition |

Rough proportion for this area's core code (about 6,400 lines without tests of other areas): **about a third is reusable as it
stands** (archive, shell menu, links / open / reveal, the search walk and index schema), **about a sixth is already superseded
by `min-paths`**, and **about half must be rewritten** — and that half is the part the owner's sentence is about ("this is a
file browser and needs to work like one").

---

## 16. Open questions for the owner

1. **Conflicts.** On a name collision during copy / move, is the wanted behaviour the system one (replace / skip / keep both,
   "apply to all", merge for folders)? Today it overwrites silently and nests folders.
2. **Undo.** Is undo of move / rename / trash / new folder in scope for the rebuild, and how far back?
3. **Trash on shares.** When a location has no trash, should the app refuse, ask explicitly for a permanent delete, or keep a
   holding folder of its own on the share? Today macOS deletes permanently without saying so.
4. **Fidelity of a copy.** Which of these must survive a copy or a move across volumes: modification and creation dates,
   permissions, extended attributes / tags / resource forks, sparse layout? Should large copies be verified (size, or a
   checksum) before a move deletes its source?
5. **Job queue.** One queue for everything (file operations, archives, transcodes) with pause / cancel / retry and an error
   list, or file operations in parallel as today? Should the queue survive a restart?
6. **Live folders.** Is a file-system watcher wanted on network shares too, where notifications are unreliable — and if so, is
   a periodic re-list an acceptable fallback, at what interval?
7. **Hidden and system files, packages.** A "show hidden" switch? Treat `.app` and other bundles as single items, as Finder
   does? Resolve aliases and `.lnk` shortcuts on double-click?
8. **Sort order.** Natural numeric order (`shot2` before `shot10`) and locale-aware collation — matching each OS, or one rule
   on both?
9. **Search.** Names only, or also contents and the project metadata? Keep depending on a third-party tool on Windows, or make
   the app's own index the single back end, kept fresh by the watcher? Any query syntax (extension, date, size)?
10. **Volumes.** Should the sidebar list drives and mounted volumes with free space and eject, like the system browsers, or
    stay with bookmarks plus the app's own mounts?
11. **Transcode.** Is one fixed preset enough, or are presets (codec, size, rate, destination, naming) wanted? Encode through
    the family's FFmpeg crate, or keep calling the CLI? Is exiftool still needed?
12. **Archives.** Password entry, formats other than ZIP for creation, "extract to…" with a destination, browsing inside an
    archive?
13. **Windows shell menu.** Keep embedding the system menu (third-party extensions run inside the app's process), or offer
    only the app's own menu with an "open with" list? Multi-selection is required either way?
14. **Settings.** May the unused keys be dropped on migration? Should per-folder view state stay in the shared database or move
    to the settings service?
15. **Other systems.** The core compiles only for Windows and macOS today. Is Linux a target of the rebuild?
16. **Sync-root shortcuts.** The placeholder copy and rename inside a sync root (Windows) tie the copy engine to the mount
    agent's configuration. Keep that coupling, or make the agent answer "can you do this copy for me"?
17. **Test fixtures.** The path-translation tests carry real-looking share and job names. Replace them with invented ones
    before any of that code is vendored into the public repository?
