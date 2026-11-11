# Network-share lessons from UFB (read 2026-11-11)

Source: the sibling repo `ufb` at its `main` (1.4.2), read only. Paths below are relative to that repo. Its public
history starts at 1.0.7 (one squashed commit; "prior development history lives in a private archive"), and the plan
documents its comments cite (`macOSplans/11-mount-and-sync-refactor.md`, `plans/17-…`) are not in the repo — so the
evidence is code comments, three devdocs and about fifteen commit messages. "Says" = their notes say it happened.
"Inference" = mine.

Applies column: **yes** = applies to a one-file document copied locally and published by temp + rename; **partly** =
the mechanism differs but the cause is shared; **no** = only matters with a driver / virtual file system.

## 1. What the cache layer is

| Part | Path | Problem it solved | State |
|---|---|---|---|
| Sync mount ("on-demand sync") | `agent/src/sync/mod.rs` | Present a NAS share as a local volume whose metadata and content are served from a local cache, passing through to SMB when cold | Ships (macOS pkg; Windows needs a user-installed driver). Audited twice in 2026-09; the macOS fix commit says "not yet exercised against a live dead NAS / quit-mid-mount" |
| macOS backend: NFS3 loopback server | `agent/src/sync/nfs_server.rs`, `macos_cache.rs`, `external/nfsserve` (fork, `devdocs/nfsserve-fork.md`) | Replaced a FileProvider extension ("per-op framework overhead"); the kernel NFS client mounts `localhost` and every RPC is answered from SQLite + sparse blobs or SMB | Ships. 30 audit findings fixed in one commit (927d0e3) |
| Windows backend: user-mode file system | `agent/src/sync/winfsp_server.rs`, `windows_cache.rs` | Same, on a drive letter. Earlier Cloud Files / ProjFS attempts left dead code (`core/src/sync_aware.rs`) | Ships; one open defect (exact-name enumeration from `cmd.exe`) |
| Cache index | `cache_core.rs` + the two `*_cache.rs` | Per-share SQLite (WAL, pool of 32): known files, visited folders, 1 MiB chunk bitmap, LRU eviction (50 GiB default, capped at half the volume) | Ships |
| Health + lifecycle | `agent/src/sync/nas_health.rs`, `agent/src/orchestrator.rs`, `agent/src/state.rs` | One 30 s heartbeat per mount decides online/offline; handlers fail fast when offline instead of parking on the SMB timeout; FSM with back-off, sleep/wake reset, reconnect | Ships |
| Conflict sidecars | `agent/src/sync/conflict.rs` | Before a truncate / first write / rename-over, if the share copy drifted, copy it to `name.conflict-host-time.ext` | Ships |
| Mount guard (macOS) | `netfs-guard/src/lib.rs`, `core/src/macos_mounts.rs`, `agent/src/platform/macos/netfs.rs`, `fallback.rs` | The system mount daemon parks forever on concurrent fresh sessions; the guard serialises, observes, restarts | Ships (2026-11-06) |
| Plain mounts | `core/src/macos_mounts.rs`, `core/src/windows_mounts.rs`, `bindings/src/local_mounts.rs` | GUI-owned SMB mounts with OS-stored credentials | Ships |
| **Closest relatives of the minNotes design** (not the cache layer) | `core/src/mesh/snapshot_worker.rs`, `core/src/mesh/coordinator.rs`, `core/src/templates.rs`, `core/src/mesh/peer_manager.rs` | A SQLite snapshot staged locally, copied to the share as a temp, renamed, and picked up by other machines polling mtime every 10 s; JSON files on the share replaced by temp + rename with a revision check | Ships; carries two recorded data-loss incidents |
| Abandoned | `agent/src/sync/macos_watcher.rs` (stub), comments in `mod.rs` | FileProvider extension, FSEvents recursive watcher, 5 s poll fallback, dirty-folder set, echo suppressor, a second probe loop, a private credential store, a login item | Deleted |

## 2. The lessons

### 2.1 Hung and slow mounts; protecting the UI and the threads

| # | Lesson | Evidence | Applies | What to do |
|---|---|---|---|---|
| H1 | A blocked share call cannot be cancelled. A timeout only stops the *waiting*; the thread stays parked ("stale connections can block 60s+", and on a dead mount "indefinitely"). | `agent/src/orchestrator.rs:623-630`; `agent/src/platform/macos/fallback.rs:447-451` ("the worker is abandoned (detached) and we report dead") | yes | Run every share call on a thread you are prepared to lose. The time limit ends the wait, not the operation. |
| H2 | Abandoned threads pile up unless latched. Each 30 s tick detached one more thread stuck in `read_dir` on a dead server. Fix: a per-mount busy flag cleared by the thread itself when it *really* returns; while set, no new call is made and the tick reports "unreachable". | `agent/src/orchestrator.rs:314-319, 610-634`; `bindings/src/local_mounts.rs:430-435` (audit "P2 thread leak") | yes | "One share operation at a time" must mean: no new operation until the previous one has returned, not until it timed out. Per share (per volume), not global — one dead share must not stall a document on another. |
| H3 | A result that arrives after the caller moved on must be recognisable as stale. Their mount attempts carry a generation tag; a late success is discarded and, if it created something, cleaned up. | `agent/src/orchestrator.rs:94-98, 1105-1118, 1156-1177` | yes | A publish that timed out is **outcome unknown**, not failed: the rename can land minutes later. Tag each operation; when a late one returns, re-read the share and reconcile; never start a second publish on top of an unreturned one. |
| H4 | `stat` on a dead SMB mount lies: "the kernel serves the mount root's attributes from its attribute cache even when the SMB session is dead, so a stat probe reports zombie mounts as healthy forever. A directory read forces real wire traffic." | `agent/src/orchestrator.rs:653-663`; `fallback.rs:253-259`; `bindings/src/local_mounts.rs:424-428` | yes | A successful size+mtime poll proves neither reachability nor freshness. Make the poll (or every Nth) a real read of bytes from the file. |
| H5 | Checking a path before acting on it is itself the hang: "`path.exists()` is deliberately NOT checked first — stat on a dead mount is exactly the thing that hangs." They read the `mount` table instead (no I/O on the mount). | `fallback.rs:409-410, 513-517` | yes | No `exists()` / `metadata()` / `canonicalize()` on the document path outside the worker — including recent-files validation, title-bar proxy icons, "reveal", file dialogs' initial folder. |
| H6 | UI freezes came from incidental calls, not the main I/O path: a reveal-in-file-manager that did `exists()` ("SMB timeouts froze the UI"), a CLI helper run synchronously (now a 4 s deadline), a shell thumbnail handler on "a stalled network drive" (now a throwaway thread, 10 s, abandoned on timeout). | commit 28e443e; commit 98d7dae + `core/src/lucidlink.rs:166`; `app/thumbnails/ShellThumbnail_win.cpp:95-103`; `core/src/file_ops.rs:668` | yes | Grep the UI layer for any path API applied to a document path. The rule "the UI never waits on the share" is broken by helpers, not by the worker. |
| H7 | A bounded thread pool is starved by one hung share call (4 thumbnail workers; tokio workers pinned by inline SMB I/O "stalled every concurrent request"; a connection pool of 6 exhausted, 30 s checkout timeout, then a panic that never answered). | `ShellThumbnail_win.cpp:98-100`; `nfs_server.rs:375-378, 758-761`; `cache_core.rs:16-20`; `macos_cache.rs:526-533` | yes | The share worker must not borrow from a shared pool (async runtime, SQLite pool, image loaders). It gets its own thread and its own DB connection. |
| H8 | Fail fast while known-offline: one health flag fed by one probe; every share-touching path checks it first "instead of waiting for a 60-second SMB timeout on every op". The earlier design ran a probe loop per server instance and leaked them. | `agent/src/sync/nas_health.rs:14-22`; `nfs_server.rs:323-326, 2051-2054` | yes | One reachability state per share, one prober. While offline: no polls, no publish attempts, only the probe on its back-off. |
| H9 | Single-strike offline is too twitchy: "a busy NAS during a render push reads as an outage"; they recommend 2-of-N and tripping from real operation errors too. Listed as open. | `devdocs/windows-live-sync-audit-2026-09-11.md` "Single-strike offline" | yes | Go offline on 2 consecutive failures or one hard error; come back on one success. |
| H10 | Errors must be classified. "Timed out while reachable" is a stall, not an outage (returning "retry" made the client re-issue "the same doomed op forever"); a DB-layer failure is not "file missing"; auth failure is never retried into a prompt; host-level failures hold the whole host, share-level ones do not. | `nfs_server.rs:1165-1173, 357-360`; `netfs-guard/src/lib.rs:430-436`; commit 927d0e3 ("AuthFailed is never downgraded") | yes | Four classes at least: not found / auth / unreachable / stalled. Back-off and user message differ per class. |
| H11 | A cheap, cancellable reachability test exists without any driver: TCP connect to the server's port 445 with a 3 s budget, before handing anything to the OS. Added after mounts "parked forever" when the VPN was not up. | commit e758b58; `agent/src/platform/macos/netfs.rs:125-126, 262-266` | partly (needs the host; inference: obtainable from the mount table's smbfs source or a UNC path; not for sync folders) | After a failure, gate retries on a socket connect rather than on another file call that may park a thread. |
| H12 | Sleep/wake: a tick gap far beyond the cadence (>90 s on a 30 s tick) means the machine slept; "the SMB session is almost certainly dead"; they reset the back-off so a wake into a healthy network retries at once. | `agent/src/orchestrator.rs:534-551` | yes | Detect wake by timer gap; treat the share as unknown, reset back-off, probe before the first poll. |
| H13 | Quit must not depend on the share. Their agent exited with a system request in flight and wedged a system daemon machine-wide; teardown that touched a dead mount hung. | commit 927d0e3 (agent lifecycle); `orchestrator.rs:1292-1294` | partly (the daemon wedge is mount-specific) | Quit never joins a share thread and never does a last publish synchronously; unpublished state stays in the local copy and is published next launch. |
| H14 | "Is this path on a network volume?" is not answerable reliably: "symlinks, firmlinks, SMB loopback gateways, File Provider mounts" — so their read-ahead runs for every file and is kept cheap instead of branching. | `app/player/read_ahead.h:16-22` | yes | Do not branch on local vs network. Treat every document path as a slow share. (Where they *do* need to know what is mounted where, they parse `mount` output on macOS and ask the network provider per drive letter on Windows — `fallback.rs:597-611`, `core/src/windows_mounts.rs:142-171`.) |

### 2.2 Change detection

| # | Lesson | Evidence | Applies | What to do |
|---|---|---|---|---|
| D1 | The SMB client caches attributes. macOS: "caches positive attribute lookups for ~10s; the post-write stat could return PRE-write metadata". Windows: the redirector's directory cache (10 s default lifetime) "hides peer changes from `fs::metadata(folder).modified()`". | `nfs_server.rs:249-257, 1689-1693`; `devdocs/windows-mount-sync-audit.md` §3; `agent/src/sync/windows_cache.rs:25-31` | yes | A poll can be up to ~10 s stale in either direction. Never stat the share after your own publish to learn the new baseline — record what you wrote. |
| D2 | Stale stats after own writes produced false "someone else changed it": spurious conflict sidecars "on every write→truncate", and "a half-copied file's bitmap nuked". Fix: a 15 s quiet window after an own write, keyed on an in-memory marker of *our* write. | `nfs_server.rs:238-257, 547-555` | yes | After a publish, for ~15 s a size+mtime mismatch against what you published is not evidence of a peer. Decide by content identity, not by stat. |
| D3 | The quiet window was first keyed on the file's mtime. That lost data: the mtime is also adopted from peer edits, so a peer save inside the window was clobbered with no sidecar; and "a NAS clock running ahead made `now - mtime` negative, i.e. 'recent' forever, so the file was never re-verified". | `nfs_server.rs:238-245, 1355-1362, 2263-2268` (the regression test) | yes | Never compute "recent" from a share mtime against the local clock. Own-write knowledge lives in local state only. |
| D4 | mtime granularity on SMB is ~1 s; they use a 1.0 s slop for folders, 2 s for conflict decisions, 30 s to tolerate their own write echoed back "with the NAS's clock" — and record the cost: "a wider window here only costs a missed same-size peer edit until the next size change". | `nfs_server.rs:966-987`; `macos_cache.rs:1620-1621, 1780-1782` | yes | Same-size replacement within the slop is invisible to size+mtime. A SQLite file changes size only in whole pages, so same-size is the normal case (inference). Size+mtime is a hint to fetch, never proof of "unchanged". |
| D5 | Their snapshot follower compares `mtime > last_restored` (strictly newer). | `core/src/mesh/coordinator.rs:1248-1287` | yes | Inference: with two publishers and clock skew, or a copy that preserves the source mtime, a newer file can carry an older mtime and is never fetched. Compare for *difference*, never order by mtime. |
| D6 | Notifications are not the mechanism anywhere in what ships. The follower learns of a new snapshot by a peer message *and* by a 10 s mtime poll that exists "to detect leader writes we missed". The FSEvents watcher on the share was deleted together with its 5 s poll fallback and echo suppressor. | `coordinator.rs:52-54`; `core/src/mesh/mod.rs` (`SNAPSHOT_MTIME_POLL_INTERVAL_MS = 10_000`); `agent/src/sync/macos_watcher.rs:1-7` | yes (confirms "no watchers") | The recorded reason for the deletion is that the watcher had no consumer after the backend change — the repo does not record that it failed. The poll fallback having existed beside it is the only hint. |
| D7 | Folder mtime only moves when a child is added or removed; a file edited in place is invisible to a folder check. They needed a per-file check with a 5 s TTL, and a 2 s negative TTL after a failed stat so a flaky share is not re-paid on every read. | commit a121f39; `nfs_server.rs:1348-1352`; `macos_cache.rs:1732-1736` | partly | Poll the file, not its folder. After a failed poll, do not poll again immediately. |
| D8 | Adopt-before-act ordering bug: the new size/mtime was stored before the stale content was dropped; when the drop was skipped, "every later stat said Fresh and the stale hydrated bytes were served until eviction". | `macos_cache.rs:679-687, 957-961`; commit 927d0e3 C-1; commit ef9e6c8 | yes | Update the remembered baseline (size, mtime, identity) only after fetch + check + merge have all succeeded, in one local transaction. A failed fetch must leave the old baseline so the change is detected again. |
| D9 | Their own replace-on-Windows makes the destination briefly absent (set aside, then renamed in). | `core/src/templates.rs:964-1016` | yes | A poll that returns NotFound once is not a deletion. Confirm on a later poll before telling the user or doing anything. |

### 2.3 Reading

| # | Lesson | Evidence | Applies | What to do |
|---|---|---|---|---|
| R1 | "`pread` over SMB can legitimately return fewer bytes than requested without being at EOF — loop until the buffer fills or a zero read signals true EOF." | `nfs_server.rs:839-843` | yes | Read in a loop; compare bytes read with the size you stat'ed *on the same handle*; a mismatch is "changed during read", retry later. |
| R2 | Absence is not emptiness. An SMB listing that came back empty mid-blip pruned a whole folder's cache; a partial listing "is indistinguishable from a deleted one". A missing remote manifest overwrote the local cache "with the empty default". | `macos_cache.rs:742-758`; `nfs_server.rs:398-403`; commit 979f174 | yes | Missing / empty / unreadable results never overwrite or delete local state. Act on absence only when confirmed twice and never automatically. |
| R3 | `Path::exists()` "reports false on ANY metadata error (stale SMB handle, EACCES, VPN blip)". Treating that as "lost" made a self-heal overwrite a peer's newer file with a stale local copy. Fix: only `ErrorKind::NotFound` means missing; "never writes blind over a file it couldn't inspect". | commit e508b48; `core/src/templates.rs:811-818, 1062-1065` | yes | No `exists()`. Match the error kind. An unreadable destination blocks the publish. |
| R4 | SQLite is never opened on the share: "SQLite ATTACH against UNC paths is unreliable: byte-range locking semantics differ across SMB versions, file create can fail with ambiguous 'unable to open database' errors when the calling user has read-but-not-write ACLs". They copy to local temp, then attach. Also so a slow SMB read does not hold the local writer lock. | `core/src/mesh/snapshot_worker.rs:284-294, 528-529` | yes (confirms the design) | Keep it absolute: no SQLite handle on the share path, including for a "quick check" of the fetched file — check the local copy. |
| R5 | UFB itself breaks R4 for minNotes documents: its "Add Note" creates the `.mnd` by opening SQLite directly on the share path (rollback journal, one transaction, deletes the file and its `-journal` on failure). | `core/src/file_ops.rs:844-848, 867-877, 936-1001` | yes | A document on the share can be mid-creation with a `-journal` beside it. Before fetching, if `<name>-journal` or `<name>-wal` exists beside the file, treat the file as being written and wait. Publish only a single-file database with no sidecars. |
| R6 | "Complete" is decided by protocol, not by inspection: readers only ever see a renamed-in file ("followers polling mtime see either the old snapshot or the new one, never a half-written file"); plain `write` "truncates-then-streams, which leaves a partial/corrupt file on SMB over high-latency or flaky links". The restored snapshot is not checksummed or integrity-checked. | `snapshot_worker.rs:469-472`; `core/src/mesh/peer_manager.rs:212-217`; `snapshot_worker.rs:528-552` | yes | Your rule (nothing trusted until checked) is stricter than theirs and needed: other writers (file managers, sync tools, UFB) do not follow the rename protocol. |
| R7 | A flag in an index is a claim about a separate file: "trust-but-verify… if the blob is missing or short… demote" rather than serve zeros or an error. | `nfs_server.rs:1439-1444` | partly | The local staged copy and the record describing it (baseline, identity) can disagree after a crash. Verify the staged copy on open before trusting the record. |

### 2.4 Writing

| # | Lesson | Evidence | Applies | What to do |
|---|---|---|---|---|
| W1 | **A temp name derived from the destination lost files.** Every machine replacing the same file used one temp path on the share; interleaved create/remove/rename plus the Windows remove-before-rename "let an interleaved writer delete the destination with nothing left to rename over it". Two shared files vanished and stayed lost. Fix: a uuid in every temp name. | commit 979f174; `core/src/templates.rs:927-938` | yes | Temp name = destination stem + machine id + random per write. Never delete a temp you did not create in this attempt. |
| W2 | Remove-then-rename on Windows was written as "acceptable given the LWW design" and then caused W1. Replacement: set the old destination aside under a unique name, rename the new one in, restore the old one if that fails, 5 attempts. "The destination is never deleted while the replacement could still be lost." | `templates.rs:405-416` (old), `964-1016` (new) | yes | Their comment says Windows rename "refuses to overwrite"; which error they actually saw is not recorded. Have a defined path for "rename-over refused" that is not delete: set-aside + rename + roll back. Expect the brief absence it causes on other machines (D9). |
| W3 | Replace-by-rename is not a compare-and-swap. Their revision check re-reads the file, compares the revision, then writes — the window stays open. Whole-file replacement silently lost edits: "the second rename to land wins, the first writer's edits are lost silently"; and for the snapshot path, "follower saves silently lose to leader-written snapshots". | `templates.rs:20-37, 1036-1069` | yes | Between your last fetch and your rename a peer can publish; your rename destroys it. Re-read the destination's identity immediately before the rename (a read, not a cached stat), and after every publish keep the version you replaced. Assume the window still loses a race sometimes: merge must tolerate "the share file lacks changes I already published". |
| W4 | Before a rename-over or truncate, if the destination drifted from what was last seen, they copy it aside first — and "bail loudly when the sidecar copy fails. Pre-refactor the function logged + returned, allowing the truncate to proceed and silently lose the conflicting version's data." Rename-over and first-write were initially not covered at all. | `nfs_server.rs:524-555, 1994-1999`; `devdocs/windows-mount-sync-audit.md` §6 | yes | The fetch-before-publish is your sidecar. If the fetch of the drifted destination fails, the publish does not happen. |
| W5 | Writes can be short and report success: "`pwrite` over SMB can return short, and ignoring the count silently drops the tail". They `fsync` the temp before the rename; an NFS path that skipped per-write sync needs an explicit commit. | `nfs_server.rs:1581-1583`; `templates.rs:952-955`; `devdocs/nfsserve-fork.md` | yes | `write_all`, then `fsync`, then read back, then rename. Inference: a read-back straight after writing may be answered from the client's cache; the `fsync` first, and a second verification of the *final* name on a later poll, are what make it meaningful. |
| W6 | The user can have read access to the file and no right to create files in its folder ("could read the existing snapshot but not create new files"). | `snapshot_worker.rs:286-290` | yes | Temp-beside-original is impossible there. Find out at open (try creating and removing a temp) and open the document read-only with a stated reason; never fall back to writing in place. |
| W7 | Never publish emptiness over content: the snapshot writer skips when its tables are empty, and "a node with subs but no metadata must NOT publish an empty-metadata snapshot over a good one". | `snapshot_worker.rs:245-262` | yes | A local copy that is new, empty or failed its own check never replaces an existing share file. |
| W8 | A gate "publish only after a successful fetch" silently stopped publishing: one failed restore at startup left the writer disarmed — "a 10-day snapshot gap with the leader still heartbeating". Separately, a fire-and-forget worker dropped its errors, "which is exactly how a leader can spend hours 'writing' snapshots that never land". | `coordinator.rs:1151-1161`; `snapshot_worker.rs:153-160` | yes | A permanently failing fetch (corrupt file on the share, wrong format) must become a visible document state with a user decision, not an endless silent back-off. Log every publish outcome at the place it happens. |
| W9 | Names: SMB is case-insensitive and case-preserving; macOS clients may send or receive decomposed (NFD) names. Raw byte comparison produced ghost rows and "NOENT on files plainly visible in listings"; a case-only rename matched its own source and deleted it. | `macos_cache.rs:312-323, 1271-1277`; `nfs_server.rs:413-414, 2170-2176` | partly | Compare names NFC-normalised and case-folded when looking for your temp, sidecars or the document in a listing; treat a case-only difference as the same file. |
| W10 | Hidden and appliance names appear beside your file: `.DS_Store`, `._*` AppleDouble sidecars, `@eaDir`, `@Recycle`, `#recycle`, `#snapshot`. A broad `@`/`#` filter hid real user content; they match exact names. An `rmdir` fails on a folder holding only such junk. | `cache_core.rs:22-39`; `nfs_server.rs:1111-1127` | partly | Ignore unknown siblings; never delete by pattern. Inference: a dot-prefixed temp is hidden from these listings and from most tools, but copying with extended attributes onto a share is what creates `._` files — write plain bytes, set no attributes. |
| W11 | Leftover temps are expected: their listing skips `*.json.tmp`, leftovers from a crashed run are removed at the next write; a 7-day stale-file constant exists (what it sweeps was not checked). | `templates.rs:348-349`; `snapshot_worker.rs:308-312`; `mesh/mod.rs:57` (`STALE_FILE_CLEANUP_MS`) | yes | Clean up only your own machine's temps, by name pattern + age, never during another publish. |
| W12 | Rollback wrote the opposite config's teardown against the old state ("runs the NEW config's branch against on-disk state from the OLD config"). | `devdocs/windows-mount-sync-audit.md` "Lifecycle gotcha" | partly | If the document's path changes while a publish is queued, the queued operation carries a snapshot of its own target (their `InflightMount` does this, `orchestrator.rs:110-115`). |

Not found in their notes: sharing violations by error code, oplocks / leases, antivirus, ACLs or ownership lost through
rename, cross-directory rename failures. See §5.

### 2.5 Reconnects, paths, prompts

| # | Lesson | Evidence | Applies | What to do |
|---|---|---|---|---|
| P1 | On macOS a share can come back at a different path: `/Volumes/<share>-1` when the name is occupied. Their server kept serving "a vanished tree" until they compared the path on every reconnect. | `orchestrator.rs:329-331, 2049-2053`; `core/src/volumes.rs:101-110` | yes | The stored document path is a hint. Identity = server + share + path inside the share (from the mount table) and re-resolve it after any outage. |
| P2 | An unmounted share leaves a real, empty local directory at the mount point (created before mounting, mode 0111, root-owned parent, "no user-space cleanup ever runs"). An "emptied ex-mountpoint directory answers read_dir happily and must not be adopted as a live mount". A foreign volume (disk image, USB) with the same name can take the path. | `fallback.rs:353-355, 417-441`; `orchestrator.rs:637-642` | yes | "Folder exists, file missing" most often means "not mounted". Never create the document, a temp or a folder at that path; verify the path is on the expected volume before any write. |
| P3 | Their volume identity is a deterministic UUID from the server/share strings, with a per-machine table of where it is mounted now. Canonicalising paths is avoided because it "resolves symlinks to their target, breaking path prefix matching". | `core/src/volumes.rs:12-20`; `core/src/file_ops.rs:97-99` | partly | Keep the path as the user gave it plus the resolved identity; do not replace one with the other. |
| P4 | Windows: a drive letter restored at logon can be "remembered but disconnected"; it still reports a target, and "a directory probe distinguishes live from dead". Elevated processes do not see per-user letters; the UNC spelling always works. One credential per server per session (error 1219). | `core/src/windows_mounts.rs:24-32, 201-202, 265-268` | yes | Record the UNC form of a drive-letter path at open and fall back to it. |
| P5 | Prompts: background attempts are never allowed to raise the OS sign-in dialog; only an explicit user action may. A permit that leaked made a background retry pop the dialog. Mounting to a non-standard folder raises a macOS approval dialog "every single time". macOS also gates network volumes behind a one-time privacy consent (they declare `NSNetworkVolumesUsageDescription`). | `orchestrator.rs:215-221, 761-768`; `fallback.rs:320-325`; `app/Info.plist.in:73-75` | partly | The worker never mounts, never opens a share URL, never triggers a reconnect. Expect the first touch of a network volume from the worker thread to raise the macOS consent — do the first touch as a result of a user action, and declare the usage string. Whether a silent background touch of a disconnected Windows letter prompts is not recorded. |
| P6 | Long paths on Windows need the verbatim prefix (`\\?\UNC\…`), and the verbatim form changes how prefix-stripping behaves (a stray leading separator gave every file two keys). | `winfsp_server.rs:372-375, 1755-1758`; commit 704ecc2 | partly | If you normalise share paths for comparison, normalise once, in one function, and test the UNC and verbatim forms. |
| P7 | Local staging on Windows: SQLite's create path fails "unable to open database" when the path resolves through an 8.3 short name; creating an empty file first and letting SQLite open it works. A staging folder under the local app-data folder failed on one profile; the OS temp folder "works everywhere". | `snapshot_worker.rs:296-326`; `cache_core.rs:219-226` | yes | Pre-create the private copy with plain file calls before SQLite opens it. Test with a profile whose name is long enough to get a short name. |

### 2.6 Measured values and settled constants

| What | Value | Where |
|---|---|---|
| SMB call on a stale connection | "can block 60s+"; dead mount: indefinitely | `orchestrator.rs:623`; `fallback.rs:448` |
| Healthy share stat | "normally answers in ms"; cold folder listing 100 ms+ | `nfs_server.rs:277-280, 375-378` |
| Liveness probe budget (read_dir + first entry) | 3 s (reuse/pre-flight), 10 s (heartbeat) | `fallback.rs:25`; `orchestrator.rs:625` |
| Heartbeat | 30 s; wake detected at a gap >90 s | `orchestrator.rs:447, 542` |
| Back-off after errors | 30 s doubling, cap 480 s; reset on wake and on success | `orchestrator.rs:565-566` |
| Give up on in-place reconnect | after 3 failed ticks → full teardown and retry | `orchestrator.rs:10-15` |
| TCP pre-flight to port 445 | 3 s | `netfs.rs:126` |
| macOS SMB attribute cache | ~10 s | `nfs_server.rs:249-257` |
| Windows directory cache lifetime | 10 s default; they force a re-list at 5 s | `windows_cache.rs:25-31` |
| Quiet window after own write | 15 s | `nfs_server.rs:257` |
| mtime slop: folder / conflict / own-write echo | 1 s / 2 s / 30 s | `macos_cache.rs:1620`; `nfs_server.rs:986-987` |
| Per-file freshness TTL; negative TTL after a failed stat | 5 s; 2 s | `nfs_server.rs:985`; `macos_cache.rs:1736` |
| Grace before believing a file was deleted | 60 s, plus "confirmed on the second listing" | `macos_cache.rs:786-792, 742-749` |
| Snapshot poll / write interval / yield to another writer | 10 s / 30 s / 60 s | `mesh/mod.rs`; `snapshot_worker.rs:34` |
| Replace retries (Windows) | 5 | `templates.rs:981` |
| Mount deadline silent / with dialog | 60 s / 600 s; healthy mount ≈1 s LAN, "a few seconds over VPN" | `netfs.rs:120-124`; `netfs-guard/src/lib.rs:91-94` |
| Per-host hold after host-level failure | 30 s doubling to 5 min, decays after 10 min | `netfs-guard/src/lib.rs:133-141` |
| Throughput, one site, direct SMB | 200 MB written in 0.31 s; 718 MB read in 0.07 s (cached) | `devdocs/windows-live-sync-audit-2026-09-11.md` table |
| Clock agreement, one site | "NAS clock matched local time exactly, so the… drift concern is theoretical at this site" | same doc, "Working well" |
| Elapsed-time hang detection | 70 s before the user learned anything; observation gave a verdict in ~6 s | commit 3d4acc7 |

### 2.7 What they tried and backed out of

| Tried | Why it went | Evidence |
|---|---|---|
| FileProvider extension as the macOS surface | "per-op framework overhead"; replaced by the NFS loopback | `nfs_server.rs:1-2`; `macos_watcher.rs:1-7` |
| FSEvents watcher on the share + 5 s poll + echo suppressor | Deleted with the FileProvider design; no consumer left | `macos_watcher.rs:1-7` |
| `stat` as the liveness probe | Reported dead mounts healthy | `orchestrator.rs:653-659` |
| Stat the share after a write to refresh metadata | Returned pre-write values; replaced by computing locally | `nfs_server.rs:1689-1693` |
| "Recently written" derived from mtime | Lost a peer's save; never re-verified under clock skew | `nfs_server.rs:238-245, 1355-1362` |
| Inferring a hang from elapsed time alone | 70 s to a verdict, and recovery re-created the fault | commit 3d4acc7 |
| A probe loop per server instance | Leaked a loop on every respawn; one heartbeat owns health now | `nas_health.rs:14-22` |
| Invalidate the cache on every write | Doubled traffic; replaced by write-through, which then needed "a failed mirror always invalidates" | `nfs_server.rs:1553-1572` |
| Temp name derived from the destination; remove-then-rename | Lost files on the share | commit 979f174 |
| Self-heal that re-created a "missing" file | Overwrote a newer file after a transient error; now create-only | commit e508b48 |
| SQLite attached directly on the share | Unreliable locking, ambiguous open errors | `snapshot_worker.rs:284-294` |
| Syncing a preset by whole-snapshot replace | "follower saves silently lose to leader-written snapshots"; moved to one file per item with a revision | `templates.rs:20-23` |
| Mounting under a private folder | OS approval dialog every time; the OS picks the location now | `fallback.rs:320-325` |
| A private credential store | Replaced by the OS keychain / credential manager; the app never holds the secret | `windows_mounts.rs:495-500` |
| A mount lock held across the wait | One share on a 600 s sign-in blocked every other mount; a panic poisoned it for the process | `fallback.rs:9-14` |
| Reporting "mounted" before the thing was verified | Green state over an empty directory; now waits for readiness (15 s cap) | `nfs_server.rs:2384-2389` |

## 3. Where the minNotes rules are wrong or insufficient

1. **"Polling size + mtime" is not sufficient as the change test.** Their record: attribute caches up to ~10 s stale
   (D1), stat answered from cache on a dead mount (H4), 1 s mtime granularity with same-size edits missed (D4), and
   two data-loss bugs from reasoning about mtime (D3). Use size+mtime only as the cheap trigger. The decision "this
   is the file I last saw / the file I published" needs content: read the first bytes of the file on a slower cadence
   and whenever you are about to publish. The SQLite header's change counter plus a version id you write into the
   document on every publish gives that in one small read (inference; the header read also serves as the real
   liveness probe of H4).
2. **"Each with a time limit" does not bound anything on its own.** The call stays parked (H1), threads accumulate
   (H2), and a timed-out publish can still complete (H3). The rule needs three additions: a busy latch per share
   cleared only by the returning thread; a generation tag on every operation; and "timed-out write = unknown outcome,
   reconcile by reading before any further write".
3. **"Write beside the original and rename over" loses a concurrent publish.** It is last-writer-wins with a window
   (W3), and their own revision check has the same window. Your fetch-and-merge narrows it; nothing in the rules
   closes it or detects it afterwards. Add: re-read identity immediately before the rename; after the rename, the
   next poll must be able to tell "the share now holds something that is neither what I published nor something I
   merged" and merge again from the local copy.
4. **The temp name is unspecified.** Theirs was shared between machines and lost files (W1). Unique per write, per
   machine.
5. **"Never delete-then-write" needs its Windows counterpart spelled out.** They hit a refused rename-over and chose
   delete (W2). Decide now what happens when the rename is refused: set-aside + rename + roll back, bounded retries,
   then give up with the temp left in place and the document marked "not published".
6. **"Read back before the rename" verifies the wrong moment.** The failure they recorded was at and after the rename
   (W1, W2), and reads straight after a write are the case their notes say the client cache answers (D1 — inference
   for file content). `fsync` before the read-back, and verify the final name again on the next poll.
7. **Nothing in the rules covers "missing".** Their two worst incidents came from treating an error or an absence as
   "gone" (R2, R3, P2). Add: only NotFound is missing; missing is confirmed twice; missing never causes a write, a
   re-create, or a local delete; a mount-point directory that exists without the volume is "not mounted".
8. **Nothing covers "cannot create files there"** (W6) or **"the check keeps failing"** (W8). Both need a document
   state the user can see, decided at open, not a back-off loop.
9. **"The file is copied on open"** is itself a share wait. The first open cannot be instant; it needs progress, a
   cancel that abandons the thread (H1), and — on macOS — it is where the network-volume consent appears (P5).
10. **"Back-off after failures"** needs the reset conditions they found necessary: wake from sleep, any success, an
    explicit user action (H12, `netfs-guard` "explicit user actions bypass the hold"). Without them a laptop that
    wakes on a good network waits out a long back-off.
11. **Other writers do not follow your protocol.** UFB's own "Add Note" runs SQLite directly on the share (R5); file
    managers and sync tools copy in place. The fetch check must reject a file with a journal/WAL sidecar and a file
    whose size changed during the read (R1), and wait.

Not contradicted, and confirmed by their experience: no SQLite on the share (R4); no watchers (D6); keeping previous
versions (they keep a `.bak` of the last parseable config, `core/src/mount_client.rs:731-835`, and a sidecar of any
version about to be overwritten, W4).

## 4. What can be ignored (driver / virtual file system only)

- Chunk bitmaps, sparse blobs, hydration flags, eviction budgets, blob namespacing, the per-handle read/write locks
  (`macos_cache.rs`, `windows_cache.rs`, commits 7b8eb4b, ef9e6c8).
- NFS specifics: persistent file handles, READDIR cookies, EXCLUSIVE-create verifiers, UNSTABLE/COMMIT, hard vs soft
  mount options, the "retry later" status, the nfsserve fork (`devdocs/nfsserve-fork.md`).
- The Windows user-mode file system: the 240 s close hang from a second backing handle (commit 0739279), attribute
  setting before delete (12e169c), exact-name enumeration from `cmd.exe`, error 448 on first traversal of a reparse
  point (`core/src/file_ops.rs:26-68`), the security descriptor, drive-letter broadcast.
- Mounting itself: the system mount daemon wedge and the whole of `netfs-guard`, mount serialisation, placeholder
  removal with admin rights, adopting/ejecting volumes, drive-letter selection, credential migration. minNotes never
  mounts. What carries over from that code is only H1–H3, H10–H12 and P1–P5.
- Stale loopback mounts after a crash hanging every process that stats them (`nfs_server.rs:51-70`).
- The mesh (peer discovery, leader election, HTTP data plane, UDP) and last-writer-wins row merging.
- LIKE-pattern escaping of path prefixes and path-key formats — unless minNotes keeps share paths as SQL keys.

## 5. Not determined

- **Why rename-over failed on Windows shares.** The comments say rename "refuses to overwrite"; no error code, no
  mention of the destination being open elsewhere, sharing modes, oplocks/leases or antivirus anywhere in the repo.
- **Rename against a file another client has open** (the minNotes fetch holding the document open while a peer
  publishes, or the reverse). Not tested or noted.
- **ACLs, ownership, extended attributes and resource forks across a rename-over.** Not mentioned. The only
  permissions note is W6.
- **NFS and AFP shares as the backing store.** Every backing share in their notes is SMB; NFS appears only as their
  own loopback. Nothing on NFS attribute caching (`actimeo` is set to 1 on their own mount only), silly-renames or
  stale handles on a real NFS server; nothing on AFP.
- **Sync folders** (Dropbox, iCloud, OneDrive, Syncthing): how they react to a temp beside the document, to rename-
  over, or to conflicts. UFB removed a Google Drive integration without recording why (commit 7c0982a). One mention
  of File Provider mounts as indistinguishable from local (H14).
- **NAS recycle bins and snapshots**: the sentinel names are filtered; whether a rename-over or delete lands the old
  file in a recycle folder is not recorded.
- **Whether notifications from a share fail to arrive.** The watcher was removed for architectural reasons; no
  observation of missed events is written down.
- **Clock skew in practice.** One site, measured as none. The 30 s self-echo tolerance is called a "pre-audit value".
- **Latency over VPN / WAN.** Only "a few seconds" for a mount; no per-operation numbers. All throughput numbers are
  one LAN site.
- **Whether touching a disconnected drive letter or a share path silently triggers reconnection or a credential
  prompt** from a background thread on Windows. Their code avoids the question by probing with a directory read and
  mounting only through explicit calls.
- **Integrity of fetched files.** Their snapshot restore has no checksum and no integrity check; no incident of a
  torn fetch is recorded, which shows nothing either way.
- **Whether the snapshot path's fixed temp name (`<file>.tmp`, `snapshot_worker.rs:305-306`) has lost a snapshot.**
  It has the shape of W1; it is protected only by leader election and the 60 s yield window. No incident recorded.
- **The private history before 1.0.7** and the plan documents the comments cite: the reasoning for abandoning the
  Cloud Files / ProjFS attempts on Windows is not in the repo.
