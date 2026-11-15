# Phase 5, step 0 — UFB audit: mounts, the agent and the cache layer

Audit of `ufb` at `main` @ `9e197f0` (version 1.4.2), read-only. Nothing was built or run; every statement is from the
source or the dev docs. Citations are `path:line` in the ufb repository. Real server, share, host and user names that
appear in ufb's comments, tests and dev docs are written generically here (`<server>`, `<share>`, `<team>`).

How it was read: the agent's control plane (`messages`, `state`, `config`, `main`, `orchestrator`, `mount_service`,
`ipc`), `netfs-guard`, the FinderSync extension, LucidLink, `sync_aware`, entitlements, the installer and the dev docs
were read directly. The two data paths (`nfs_server` + `macos_cache`, `winfsp_server` + `windows_cache`) and the GUI
side (`core/src/*_mounts.rs`, `mount_client.rs`, `bindings/src/local_mounts.rs`, the dialog) were read by three
sub-readers whose citations were then spot-checked against the code. Items marked **(reading)** are inferences from the
code that no test or dev doc confirms.

## 0. The short version

- **Two kinds of mount exist, and only one of them uses the agent.** A *plain* mount is an ordinary OS SMB mount made
  by the GUI process itself: a `/Volumes/<share>` volume on macOS, a persistent mapped drive letter on Windows. A
  *sync* mount ("On-demand sync" checkbox) is the same share re-exported through a user-mode filesystem with a local
  block cache, hosted by the child process `ufb-agent`. The agent skips every non-sync entry
  (`agent/src/mount_service.rs:277-280`); the GUI spawns the agent only when an enabled sync mount exists
  (`bindings/src/services/mount.rs:245-256`).
- **The only third-party driver in this layer is WinFsp, and only Windows sync mounts need it.** Plain Windows mounts
  use `WNetAddConnection2W` (`core/src/windows_mounts.rs:302-311`). macOS uses no third-party driver or system
  extension at all: the sync mount is the kernel's own NFS client pointed at a loopback NFSv3 server inside the agent
  (`agent/src/sync/nfs_server.rs:183-192`).
- **The Store build of ufb already ships without WinFsp.** The MSIX cannot install a driver; plain mounts and browsing
  work, and a sync mount shows an "Install WinFsp…" pill (`packaging/windows/msix/README.md:73-75`,
  `app/qml/Ufb/App/Sidebar.qml:196-200`). Dropping WinFsp on Windows therefore removes one feature (sync mounts), not
  mounting.
- **FileProvider is not used.** The README still names it (`README.md:5`), but the extension and its IPC surface were
  deleted (`agent/src/messages.rs:229-235`); the config comment "Windows Cloud Files / macOS FileProvider"
  (`agent/src/config.rs:69`) and `core/src/sync_aware.rs` are leftovers of earlier designs.
- **The cache is a read cache with write-through, not an offline-edit store.** Writes go synchronously to the NAS on
  both OSes; nothing is queued locally. There is no pinning. Offline, macOS serves cached listings and cached bytes;
  Windows serves them only through handles that were open before the drop.

## 1. What the user gets

### 1.1 Mount manager (one dialog, both OSes)

"Mount Management", opened from the sidebar's MOUNTS gear (`app/qml/Ufb/App/MountManagerDialog.qml:2,21-28`).

| Feature | Behaviour | Cite (`MountManagerDialog.qml` unless noted) |
|---|---|---|
| Add / edit / remove a share | Editor with path, optional display name; id derived from the share's leaf name, deduplicated, immutable | 190-195, 521-558, 746-751, 841-843 |
| Remove | Confirm dialog; says OS-stored credentials are kept | 898-926 |
| Enable / disable | Checkbox; disabled rows show a "prohibit" icon | 389-440, 820-835 |
| Is jobs folder | Marks the mount's subfolders as subscribable jobs (project side) | 820-835; `agent/src/config.rs:65-67` |
| Drive letter (Windows only) | "Automatic" or Z:→D:; letters in use are disabled | 762-791, 615-621 |
| On-demand sync | The checkbox that turns a plain mount into an agent-hosted sync mount | 820-835 |
| Bookmark only ("unmanaged") | Entry listed under Mounts but never mounted, probed or authenticated | 820-835; `agent/src/config.rs:30-36` |
| Sync cache location | Global folder for all sync caches: Reveal, Browse…, Reset | 212-214, 235-329 |
| Cache stats + Drain | "Cache: X · N file(s)", Drain… with confirm, refresh (edit mode, sync on) | 656-717, 932-960 |
| Connect / Disconnect, Restart | Per row; busy spinner while a command is pending | 364-372, 450-504 |
| Status | Dot colour: mounted / mounting / error / muted, tooltip = state name | 65-76, 389-440 |
| Header | "N mount(s) · agent not needed / connected / waiting" | 182-185 |

Not in the dialog: a cache size limit field (the schema field exists, `agent/src/config.rs:81-83`, default 50 GiB
and capped at half the cache volume, `agent/src/orchestrator.rs:21,68-92`), credential fields (removed by design,
`MountManagerDialog.qml:809-815`) and a working "test credentials" (the verb is a tombstone that answers
`unsupported`, `agent/src/mount_service.rs:583-598`).

Status detail and repair actions live in the sidebar row, not the dialog (`app/qml/Ufb/App/Sidebar.qml`):

| Pill / text | When | Cite |
|---|---|---|
| "Authentication failed" + "Sign in…" | state `auth_error`; the click restarts the mount with the OS auth dialog allowed | 728-738, 855-884 |
| " · X:" suffix | Windows, mounted: the letter | 731 |
| Warning icon + tooltip | Mount landed somewhere unexpected (`notice`) | 752-765 |
| "Fix…" / "Restart…" | `noticeFixable`: remove a stale `/Volumes` placeholder with admin rights, or restart the wedged macOS mount daemon | 812-846 |
| "Install WinFsp…" | Windows sync mount whose state detail contains `winfsp_init failed` | 196-200, 767-799 |
| Context menu | Start / Stop / Restart / open logs | 932-963 |

Auto-mount: each enabled plain mount gets a task at GUI start that mounts silently
(`bindings/src/local_mounts.rs:467-468`); each sync mount is started by its orchestrator as soon as the agent runs
(`agent/src/orchestrator.rs:441-443`). There is no login-time auto-start of the agent on either OS
(`agent/src/platform/mod.rs:40-43`); Windows' "start at login" starts the GUI with `--background`
(`installer/ufb_installer.iss:255-262`).

### 1.2 What a "mount" is

| | Plain mount | Sync mount |
|---|---|---|
| macOS | The OS's own SMB volume at `/Volumes/<share>` (NetFS picks the path; a `-N` suffix if the name is taken) (`core/src/macos_mounts.rs:450-457,695-706`) | A folder `~/ufb/mounts/<share>` that is a loopback NFSv3 mount, `nobrowse` so it is not a Finder sidebar volume (`agent/src/sync/nfs_server.rs:109-112,183`). The backing SMB volume is also mounted at `/Volumes/<share>` (`agent/src/orchestrator.rs:1007-1024`) |
| Windows | A persistent mapped network drive letter, restored by Windows at logon (`core/src/windows_mounts.rs:7-11,302-311`) | A drive letter that is a WinFsp volume; the configured letter, else the first free Z:→D: (`agent/src/orchestrator.rs:1629-1648`, `agent/src/platform/windows/fallback.rs:188-225`). The backing share is reached by UNC through a deviceless SMB session (`fallback.rs:50-90`) |

### 1.3 What the cache layer gives a sync mount, in user terms

| Benefit | macOS (NFS loopback) | Windows (WinFsp) |
|---|---|---|
| Faster folder listing | Listings and attributes served from SQLite; the folder is re-read from the NAS only when its mtime moves (`nfs_server.rs:2055-2081`) | Same idea; re-enumeration on mtime change or a 5 s timer (`windows_cache.rs:31,677-704`) |
| Faster re-reads | Bytes read once are kept in 1 MiB chunks and served locally (`cache_core.rs:45`, `nfs_server.rs:780-893`) | Same; measured warm reads at native disk speed for ≥64 KB app reads (`devdocs/windows-live-sync-audit-2026-09-11.md:52-58`) |
| Read-after-write is local | The written bytes are mirrored into the cache (`nfs_server.rs:1593-1687`) | Same (`winfsp_server.rs:926-977`) |
| Offline reading | Cached listings and cached bytes keep answering; anything uncached returns "try later" (`nfs_server.rs:328-334,820-822,2051-2055`) | Only through handles opened before the drop; a new open fails with "device not connected" (`winfsp_server.rs:565-571`) |
| Offline writing | No | No |
| No hang when the NAS drops | SMB-touching operations fail fast once the heartbeat marks the NAS down (`nfs_server.rs:328-334`) | Same (`winfsp_server.rs:276-279`) |
| Conflict protection | If the NAS copy changed since it was cached, it is copied to a `…conflict-<host>-<UTC stamp>…` sidecar before the user's truncate / first write / replace (`nfs_server.rs:591-606`, `conflict.rs:14-54`) | Same, but only on overwrite and rename-replace, not on plain in-place writes (`winfsp_server.rs:1102-1104,1295-1301`) |
| Finder badges | "Cached locally" badge on fully cached files (section 9) | None |
| Pinning ("keep this offline") | Not found | Not found |
| Prefetch / read-ahead | Not found (`nfs_server.rs:780-893`) | None (`winfsp_server.rs:1545-1546`) |
| Live sync between machines | Not part of this layer. "Sync" here means on-demand caching of one NAS share; peer changes are noticed by polling attributes, not pushed |

The MSIX store text describes the feature as "offline-capable sync mounts for remote work"
(`packaging/windows/msix/README.md:124-125`); the code supports that description on macOS more than on Windows.

## 2. Architecture

### 2.1 Processes

```
UFB GUI (Qt/QML + Rust bindings)
 ├─ owns plain mounts directly: core/src/macos_mounts.rs (NetFS) / core/src/windows_mounts.rs (WNet)
 │   one task per mount in bindings/src/local_mounts.rs (30 s heartbeat)
 ├─ writes mounts.json (atomic)                       core/src/mount_client.rs:793-846
 └─ IPC client ──────────────► ufb-agent (detached child, one per user)
                                 MountService ─ one Orchestrator per sync mount ─ FSM (state.rs)
                                   ├─ backing SMB mount / session
                                   ├─ VFS server: nfs_server.rs (macOS) | winfsp_server.rs (Windows)
                                   └─ cache: macos_cache.rs | windows_cache.rs (SQLite + blob files)
UFBFinderSync.appex (macOS, sandboxed) ─ second IPC client, badges only
```

### 2.2 The agent process

| Topic | Finding | Cite |
|---|---|---|
| Launch | Detached child spawned by the GUI when an enabled sync mount exists; `setsid` on macOS, `CREATE_NO_WINDOW` on Windows; a 15 s keep-alive loop respawns it when required but disconnected | `bindings/src/services/mount.rs:245-256,697-712,771-786,1490-1512` |
| Lifetime | Outlives the GUI; the GUI never sends `Quit` | `bindings/src/services/mount.rs:320-324` |
| Single instance | Named mutex (Windows) / `flock` on a per-user lock file (Unix); a second instance exits | `agent/src/main.rs:40-106` |
| PID file | Written at start, removed in `graceful_exit`; the GUI's heal-on-open reads it, checks the executable name, SIGTERM → 3 s → SIGKILL → spawn | `agent/src/main.rs:125-250`; `bindings/src/services/mount.rs:639-715` |
| Faceless | No tray, no window (macOS bundle is `LSUIElement`); the GUI's tray is the only surface | `agent/src/main.rs:423-425`; `agent/macos/Info.plist` |
| Startup (macOS) | Single-instance check, then force-unmount of leftover loopback NFS mounts from a crashed run, before anything stats them | `agent/src/main.rs:574-592` |
| Shutdown | SIGINT / SIGTERM → Stop to every mount, 15 s cap each, then exit; cancels any in-flight NetFS request first | `agent/src/main.rs:466-491,543-558,200-221`; `agent/src/mount_service.rs:729-760` |
| Config reload | Polls the config file's mtime every 5 s; an unreadable or unparseable file keeps the last good config | `agent/src/main.rs:438-462,526-541`; `agent/src/config.rs:385-420` |
| Windows install-time flag | `--prime-smartscreen`: sleep 3 s and exit, so the first traversal of a WinFsp mount does not hit `ERROR_UNTRUSTED_MOUNT_POINT` (448) | `agent/src/main.rs:605-618`; `installer/ufb_installer.iss:370-386` |
| Logging | `simplelog` to stderr and one log file (per-user app-data folder on Windows, the user's Logs folder on macOS); level from `RUST_LOG` / `UFB_DEBUG`; the file is deleted at start when over 2 MB; panics are appended | `agent/src/main.rs:255-377` |
| Linux | Code paths reference `platform::linux`, but the module does not exist in the tree; the Linux arms cannot compile | `agent/src/orchestrator.rs:971,987,2177`; `agent/src/platform/mod.rs:1-5` |

### 2.3 IPC

| Topic | Finding | Cite |
|---|---|---|
| Transport, Windows | One fixed-name named pipe, byte mode, duplex, default security descriptor; one client served at a time on a dedicated I/O thread (peek-poll, to avoid the synchronous-pipe deadlock) | `agent/src/ipc/server.rs:12,78-115,231-254` |
| Transport, macOS | Unix stream socket with a deliberately short file name inside the App Group container (so the sandboxed Finder extension can reach it; 104-byte `sun_path` limit); mode 0700; several clients, every outgoing message broadcast to all | `agent/src/ipc/unix_server.rs:6-45,56-141` |
| Framing | `u32` little-endian length + UTF-8 JSON, 16 MiB cap | `agent/src/ipc/mod.rs:9-36` |
| Encoding | Serde tagged enums, `type` in snake_case, fields camelCase | `agent/src/messages.rs:5-7,176-178` |
| Handshake | None. A new client gets a full snapshot; compatibility is by optional fields | `agent/src/messages.rs:9-15`; `core/src/mount_client.rs:111-117,433` |
| Back-pressure | 5 s per-client write budget, then the client is dropped; badge updates are droppable, everything else must deliver | `agent/src/ipc/unix_server.rs:47-54`; `agent/src/main.rs:505-520` |
| Client reconnect | 500 ms for the first five tries, then 3 s; commands sent while disconnected are discarded | `core/src/mount_client.rs:681-688` |
| Command tracking | UUID per command, 10 s timeout sweep in the GUI | `bindings/src/services/mount.rs:377-423,1514-1572` |

Message set (`agent/src/messages.rs`):

| Direction | Message | Fields | Line |
|---|---|---|---|
| GUI → agent | `start_mount` / `stop_mount` / `restart_mount` | `mountId`, `commandId`, `allowUi` (may show the OS auth dialog) | 179-181, 215-227 |
| | `clear_sync_cache`, `get_cache_stats` | `mountId`, `commandId` | 182-186 |
| | `reload_config`, `get_states`, `ping`, `quit` | — | 188-191 |
| | `freshness_sweep` | `domain?`, `commandId` (macOS posts a notification; no-op on Windows, `mount_service.rs:485-523`) | 196, 205-213 |
| | `test_credentials` (tombstone), `create_symlinks` (ignored) | | 187, 202; `mount_service.rs:454-459,583-598` |
| Agent → GUI | `mount_state_update` | `mountId`, `state`, `stateDetail`, `syncState?`, `syncStateDetail?`, `needsElevation?`, `mountedAt?`, `notice?`, `noticeFixable?`, `noticeFix?` | 117-159 |
| | `mount_state_snapshot` | `mounts[]` (replaces the client's whole map) | 39-43 |
| | `ack` / `error` | `commandId` (+ `message`) | 161-172 |
| | `pong` | — | 18 |
| | `conflict_detected` | `domain`, `originalPath`, `conflictPath`, `host`, `detectedAt` | 102-115 |
| | `cache_stats` | `mountId`, `hydratedBytes`, `hydratedCount`, `commandId` | 89-100 |
| | `badge_update` | `domain`, `relpath`, `badge` = hydrated / partial / uncached | 45-64 |
| | `test_credentials_result` | `mountId`, `kind`, `message`, `commandId` | 77-87 |

### 2.4 State machine per mount (`agent/src/state.rs`)

States: `Initializing`, `Mounting`, `Mounted(SyncPhase)`, `Error(ConfigInvalid | MountFailed | AuthFailed)`, `Stopped`
(`state.rs:14-31`). Sync phase inside Mounted: `NotApplicable`, `Spawning` ("registering"), `Active`, `Offline`,
`Reconnecting` (`state.rs:41-69`). Wire state names: initializing, mounting, mounted, auth_error, error, stopped
(`orchestrator.rs:2196-2206`).

| | Start | Stop | Restart | ConfigChanged | MountFailed | AuthFailed |
|---|---|---|---|---|---|---|
| Initializing | Mounting | Stopped | Mounting | Mounting | Error | Error |
| Mounting | — | Stopped | — | Mounting | Error | Error |
| Mounted | Mounting (as restart) | Stopped | Mounting | Mounting | Error | Error |
| Error | Mounting | Stopped | Mounting | Mounting | Error (an auth error is never downgraded) | Error |
| Stopped | Mounting | — | Mounting | Mounting | Error | Error |

(`state.rs:171-190`.) The transition function is pure and returns effects: `MountDrive`, `DisconnectDrive`,
`ReleaseDrive`, `SpawnSyncServer`, `TeardownSyncServer`, `UpdateTray`, `LogEvent`, `EmitStateUpdate`
(`state.rs:129-158`). The VFS server is always torn down before the backing mount (`state.rs:304-307`).
`Handoff` = Stop that leaves the backing SMB volume up, used when sync is switched off and the GUI adopts the share
as a plain mount (`state.rs:100-105,419-422`; `mount_service.rs:283-300`). Mounting → Mounted happens only after the
backing path is resolved and the VFS server reports ready within 15 s (`orchestrator.rs:1541-1570,1668-1700`).

### 2.5 Config file

One JSON file, `mounts.json`, in the app's per-user data folder (`agent/src/config.rs:364-383`); written by the GUI
with tmp → fsync → rename and a `.bak` of the last parseable version (`core/src/mount_client.rs:793-841`), read by both
processes. Shape: `{ version, mounts[], syncCacheRoot? }` (`config.rs:5-15`).

| Field (per mount) | Meaning | Line (`config.rs`) |
|---|---|---|
| `id`, `displayName`, `enabled` | identity | 21-24 |
| `nasSharePath` | UNC path of the share (or any native path for a bookmark) | 26-28 |
| `unmanaged` | bookmark only | 30-36 |
| `mountDriveLetter` | Windows letter preference | 45-47 |
| `mountPathMacos`, `smbMountPath`, `mountPathLinux` | per-OS path overrides | 49-63 |
| `isJobsFolder` | project side | 65-67 |
| `syncEnabled`, `syncRootPath`, `syncCacheLimitBytes` | sync mode, root override, cache budget (0 = default) | 69-83 |
| `credentialKey` | legacy; credentials are OS-owned now | 38-43 |
| about 20 legacy fields (rclone, junctions, VFS tuning, health thresholds) | parsed and ignored | 85-141 |

Only seven fields force a remount when changed (`config.rs:187-201`). The legacy fields record the layer's history:
rclone mounts, then junction/symlink farms, then Cloud Files / FileProvider, then WinFsp / NFS loopback.

Three copies of the config struct exist (agent, core, bindings) and are kept in step by hand
(`core/src/mount_client.rs:216-220`; `bindings/src/services/mount.rs:46-50`).

## 3. macOS path

### 3.1 The backing SMB mount (plain mounts and sync mounts alike)

| Topic | Finding | Cite |
|---|---|---|
| API | `NetFSMountURLAsync` + `NetFSMountURLCancel` by direct FFI; `mount_smbfs` is not used. Two near-identical copies: GUI and agent | `core/src/macos_mounts.rs:735-767,1283-1295`; `agent/src/platform/macos/netfs.rs:3-7,64-96,497-566`; "keeps its own copy … until F2 dedupes" `macos_mounts.rs:4-7` |
| Where | Mount path NULL → NetFS picks `/Volumes/<share>`; a custom path would raise an OS approval prompt every time | `macos_mounts.rs:450-457`; `agent/src/platform/macos/fallback.rs:320-326` |
| Auth | NULL user and password → NetFS reads the login Keychain. Silent attempts set `UIOption = 0`; only a user action (`allowUi`) lets the system sign-in dialog appear | `macos_mounts.rs:1055-1067,1191-1196`; `netfs.rs:105-110,463-470` |
| Pre-flight | DNS + TCP connect to port 445, 3 s; failure never reaches the mount daemon | `macos_mounts.rs:797,1026-1049`; `netfs.rs:266-289` |
| Timeouts | Silent 60 s, with UI 600 s, cancel grace 10 s, liveness probe 3 s | `macos_mounts.rs:472,791-800`; `netfs.rs:120-129,152` |
| Reuse / adopt | The mount table is scanned for an existing mount of the same source; a live one is adopted (and never ejected by ufb), a dead one that is ours is force-unmounted | `fallback.rs:74-83,334-383,452-463` |
| Ownership check | `mount_at_path_is_ours`: fstype smbfs and exact `host/share` match from the mount table | `fallback.rs:646-676,722-751` |
| Stale placeholder | An empty, unmounted directory squatting `/Volumes/<share>` makes the mount land at `<share>-1`; reported as a fixable notice; the fix runs `rmdir` with administrator rights via `osascript` | `core/src/macos_mounts.rs:483-502`; `bindings/src/services/mount.rs:1197-1231` |
| Unmount | `diskutil unmount`, then `umount`; forced: `umount -f`, then `diskutil unmount force` | `fallback.rs:480-538` |
| On GUI quit | Plain mounts are left mounted; in-flight NetFS requests are cancelled | `bindings/src/local_mounts.rs:75-95`; `bindings/src/services/mount.rs:325-339` |
| In-flight registry | Every issued request is recorded; Stop and process exit cancel it and wait up to 10 s, because a client dying with a request pending wedges the per-user mount daemon for every app | `netfs.rs:159-259`; `agent/src/main.rs:190-221` |

Plain-mount health (GUI): 30 s heartbeat, ownership check then a bounded (10 s) `read_dir`; a dead mount is unmounted
and silently remounted; auth errors wait for the user (`bindings/src/local_mounts.rs:436-457,553-619`).

### 3.2 NFS loopback server (`agent/src/sync/nfs_server.rs`)

| Topic | Finding | Cite |
|---|---|---|
| Listener | `127.0.0.1:<port>`, one server per sync mount; ports from a base of 12345 with a process-wide counter; no bind retry | 43, 2455-2463; `orchestrator.rs:1454-1464` |
| Mount command | `mount -t nfs -o port=P,mountport=P,nolocks,vers=3,tcp,nobrowse,intr,deadtimeout=60,actimeo=1,rsize=1048576,wsize=1048576 localhost:/<share> ~/ufb/mounts/<share>` | 183-192 |
| Why hard + `intr` + `deadtimeout` | A soft mount surfaces spurious write errors; `intr` lets a stuck Finder be force-quit; `deadtimeout=60` makes the kernel drop the mount if the agent itself is gone | 171-181 |
| Privilege | Runs as the user: the mount point is under the home folder, created by the agent | `agent/src/main.rs:637-661` |
| Teardown | `umount`, then `umount -f` | 2524-2542 |
| Stale cleanup | At agent start, every `localhost:/…` mount under the mounts folder is force-unmounted — a dead loopback mount hangs Finder, Spotlight and `ls` | 71-105; `agent/src/main.rs:581-590` |
| Operations | lookup, getattr, setattr (size only; mode / owner / times are accepted and ignored), read, write, commit, create, exclusive create (verifier remembered 120 s), mkdir, remove / rmdir, rename, readdir, readlink | 1203-2168, 1282-1286, 282 |
| Refused | symlink → read-only-filesystem error; MKNOD and LINK unavailable | 2122; `external/nfsserve/src/nfs_handlers.rs:158-164` |
| No locking | `nolocks` mount option; no lock manager | 183 |
| Attributes | Synthesized: mode 755 / 644, a hard-coded uid / gid, directory size 4096 | 1049-1077 |
| File handles | The SQLite row id (`AUTOINCREMENT`, never reused), fixed generation, so handles survive an agent restart | 2146-2167 |
| Names | NFC-normalized at the boundary (the client sends NFD, the NAS stores NFC); keys case-insensitive | `agent/Cargo.toml:20-23`; `macos_cache.rs:1803-1810` |
| Offline | SMB-touching ops answer `NFS3ERR_JUKEBOX` ("try later"); readdir serves cached children; the root's attributes are served from the last good value | 328-334, 474-522, 2051-2055 |
| Error mapping | Connection-class I/O errors → JUKEBOX; timeouts → IO | 1157-1199 |

### 3.3 The nfsserve fork (`external/nfsserve`, 4,969 lines, upstream 0.10.2)

Wired in by `[patch.crates-io]` (`agent/Cargo.toml:94-103`). Why and what (`devdocs/nfsserve-fork.md`):

| Change | Why | Line |
|---|---|---|
| `NFSPROC3_COMMIT` dispatched; WRITE echoes the client's `stable_how` | Upstream answered every WRITE as `FILE_SYNC` and rejected COMMIT, forcing an fsync over SMB per chunk; with the fork a 100 MB save is N unstable writes + one commit | 11-21, 27-31, 67-69 |
| READDIR passes the client's cookie | Upstream re-sent the first page for every page; large folders never listed fully | 44 |
| SETATTR guard mismatch returns | Upstream reported the mismatch and then truncated anyway, writing two replies | 45 |
| Exclusive CREATE passes the verifier | A retransmitted create must succeed, not fail with "exists" | 46 |

About 150 added lines in three files; the SETATTR `return` is silent if lost on an upstream bump
(`nfsserve-fork.md:48-50,94-95`). The doc calls the upstream licence Apache 2.0 (`nfsserve-fork.md:86`); the vendored
manifest says BSD-3-Clause (`external/nfsserve/Cargo.toml:33`) — to be reconciled.

### 3.4 The cache (`agent/src/sync/macos_cache.rs`, shared helpers in `cache_core.rs`)

| Topic | Finding | Cite |
|---|---|---|
| Location | `<cache root>/<share>.db` and `<cache root>/by_handle/<share>/<row id as 16 hex digits>`; default root under the user's local data folder; one global override | `macos_cache.rs:108,1462-1478`; `config.rs:292-307` |
| Store | SQLite, WAL, `synchronous=NORMAL`, 5 s busy timeout; r2d2 pool of 32 (raised from 6 because a blocked checkout waits 30 s) | `cache_core.rs:17-21,176-185,247-248` |
| Schema | `known_files(fh PK, path UNIQUE NOCASE, name, is_dir, nas_size, nas_mtime, nas_created, is_hydrated, hydrated_size, last_accessed, last_verified_at, parent_path, chunk_bitmap, cached_bytes)`; `visited_folders(path, folder_mtime)`; `metadata(key, value)` | `macos_cache.rs:129-154,215-217` |
| What is cached | Folder listings, attributes, and file content | above |
| Content model | 1 MiB chunks, one bit per chunk; sparse blob files; all-zero chunks are not written; a full bitmap flips the row to "hydrated" | `cache_core.rs:45,96-121`; `nfs_server.rs:780-909` |
| Ignored names | Dotfiles and four NAS housekeeping names | `cache_core.rs:34-40` |
| Eviction | Every 30 s; when cached bytes exceed the budget, least-recently-accessed entries go until 80 % of the budget | `nfs_server.rs:2406-2413`; `macos_cache.rs:1081-1119`; `cache_core.rs:16` |
| Budget | 0 → 50 GiB; never more than half the cache volume | `orchestrator.rs:21,68-92` |
| What protects a file in use | A per-handle read/write lock held for the duration of one RPC; the evictor skips a busy handle. NFSv3 is stateless, so "open in an app" is not known | `nfs_server.rs:1427-1432`; `macos_cache.rs:1103-1107` |
| Stats / drain | Cached bytes (partial included) and the count of fully cached files; drain skips busy handles; unclaimed blobs are swept at startup | `macos_cache.rs:444-476,1634-1686` |
| Freshness | Folder: re-enumerate when the live mtime is more than 1 s newer. File: a stat at most every 5 s on read (2 s after a failed stat). A 15 s quiet window after our own write suppresses both, because the macOS SMB attribute cache lags about 10 s | `nfs_server.rs:257-259,985-987,1369-1417,2055-2081`; `macos_cache.rs:1736-1795` |
| Drift → invalidate | A size change or mtime change on a row holding bytes drops the blob, then adopts the new metadata | `macos_cache.rs:669-690,893-907` |
| Prune guards | A partial listing, or a listing that suddenly comes back empty, prunes nothing; 60 s grace; ignored names never pruned | `macos_cache.rs:750-784,792,823-829` |

### 3.5 Write path, conflicts, offline

- **Write-through, synchronous.** Each WRITE opens the SMB file, writes at the offset, and fsyncs unless the client
  asked for unstable; COMMIT fsyncs the whole file (`nfs_server.rs:1534-1587,1739-1773`). Nothing is buffered locally;
  a NAS drop mid-write returns "try later" or an I/O error to the application (`nfs_server.rs:1174-1181,1519`).
- **Mirror.** The same bytes go into the cache blob without fsync; a mirror failure invalidates the entry
  (`nfs_server.rs:1593-1641`). Size and mtime are stamped from the local clock (`nfs_server.rs:1689-1698`).
- **Conflict check** at truncate-to-zero, at the first write of a burst and at rename-over-existing: a size change or
  an mtime difference over 2 s between the NAS and the cached row (`nfs_server.rs:591-599,717-731,997-1008,1295-1299,
  2000-2008`). The NAS version is copied to a sidecar `<stem>.conflict-<host>-<UTC stamp><ext>` in the same folder
  (`conflict.rs:14-54`), then the user's operation proceeds. If the copy fails the operation is refused
  (`nfs_server.rs:609-619`). The GUI is told by `conflict_detected`, sent with `try_send` and dropped if the channel is
  full (`nfs_server.rs:645-654`).
- **Watcher.** `macos_watcher.rs` is a 53-line remnant: it only posts a distributed notification
  (`macos_watcher.rs:17-52`). The FSEvents watcher was deleted; the `notify` dependency is still declared
  (`agent/Cargo.toml:31`).
- **NAS health.** One atomic boolean, optimistic at start, fed only by the orchestrator's heartbeat
  (`nas_health.rs:14-21,36,47-62`).

### 3.6 Performance characteristics the code states

SMB `read_dir` + per-entry stat costs "100ms+ per folder" (`nfs_server.rs:376-378`); an SMB call against a dead
session blocks about 60 s (`nfs_server.rs:325-326`); unstable writes + commit removed the per-chunk fsync that
dominated write latency (`nfs_server.rs:1508-1511`); `readdir` loads and sorts all children on every page
(`nfs_server.rs:2092-2093`). No measured macOS benchmark was found in the dev docs
(`devdocs/macos-session-primer-2026-09-10.md` covers the player, not mounts).

### 3.7 Entitlements, signing, provisioning

| Item | Finding | Cite |
|---|---|---|
| App and agent | Hardened runtime, not sandboxed; network client + server; an App Group shared by app, agent and extension | `app/UFB.entitlements:10-39`; `agent/ufb-agent.entitlements:8-25` |
| Agent packaging | The bare binary is wrapped in its own `.app` beside the main app, signed and notarized separately | `scripts/sign-mac-dev.sh:109-117`; `scripts/release-mac.sh:181-197` |
| Keychain sharing | The `keychain-access-groups` entitlement was tried and rejected at launch without a provisioning profile; both entitlement files leave it off | `app/UFB.entitlements:41-52`; `agent/ufb-agent.entitlements:27-31` |
| Provisioning profile | `devdocs/macos-provisioning-profile.md` describes per-bundle Developer ID profiles (yearly renewal; an expired profile stops the app launching, lines 82-88) to enable a shared keychain group. With credentials now read by NetFS itself (3.1), whether this is still needed is **not verified**; the doc still lists a tray bundle that was deleted (`macos-helpers/project.yml:3-4`) | |
| Admin rights | Only the optional stale-placeholder fix | `bindings/src/services/mount.rs:1197-1231` |
| Installer | The pkg preinstall stops a running agent | `packaging/macos/scripts/preinstall:4-25` |

## 4. Windows path

### 4.1 Plain mounts (GUI, no driver)

| Topic | Finding | Cite (`core/src/windows_mounts.rs`) |
|---|---|---|
| Silent mount | `WNetAddConnection2W(NULL user, NULL password, CONNECT_UPDATE_PROFILE)`, disk resource, persistent | 302-311 |
| Interactive | On auth failure with user intent: `WNetUseConnectionW` with `CONNECT_INTERACTIVE | CONNECT_PROMPT | CONNECT_UPDATE_PROFILE` (the OS credential dialog) | 354-357, 420-434 |
| Errors | 5 / 86 / 1326 → auth; 1219 (credential conflict) → explanatory message; 1202 → forget and retry once; 85 → reuse if ours | 318-362 |
| Letters | Preferred if free or already ours, else first free Z:→D:; an existing mapping at another letter is migrated; only the share root is mapped | 87-127, 218-285 |
| Shell | `SHChangeNotify` on add / remove | 377-390 |
| Unmap | `WNetCancelConnection2W`; Stop forgets the profile entry; not unmapped on quit | 461-493; `bindings/src/local_mounts.rs:75-95` |
| Limits noted | Elevated processes do not see per-user letters; one credential per server per session | 25-26, 31-32 |
| Health | 30 s heartbeat, 10 s probe (same task as macOS) | `bindings/src/local_mounts.rs:436-457` |

### 4.2 Sync mounts: session and drive letter (agent)

The agent establishes a deviceless session with `WNetAddConnection2W` on the share root with NULL credentials, treats
1219 and 85 as "already connected", and classifies 5 / 86 / 1326 as auth failure
(`agent/src/platform/windows/fallback.rs:27-126`; `orchestrator.rs:901-965`). Any other failure is logged and the
mount proceeds on whatever session Windows has cached (`orchestrator.rs:951-957`). Stop cancels the session without
force (`fallback.rs:129-157`). The letter must be genuinely free because WinFsp creates its own device; a leftover
mapping of the same share on the preferred letter is forgotten first (`fallback.rs:180-225`).

### 4.3 WinFsp server (`agent/src/sync/winfsp_server.rs`)

Crate `winfsp` 0.12, native `FileSystemContext` API; `winfsp-x64.dll` is delay-loaded and found through the WinFsp
install registry (`agent/Cargo.toml:47-74`; `agent/build.rs:4-5`). One OS thread per share: init → host → mount
`"X:"` → start, with a readiness channel (`winfsp_server.rs:1802-1923`). Without WinFsp installed, init fails with
"winfsp_init failed — is the WinFsp runtime installed?" and the mount goes to Error (`winfsp_server.rs:1816-1828`).

Volume parameters (`winfsp_server.rs:1832-1874`): case-preserving, case-insensitive, Unicode, persistent ACLs flag on,
reparse-points flag on, named streams off (deliberate), flush-and-purge on cleanup, POSIX unlink/rename, file-info and
volume-info timeouts 10 s. Volume size is reported as a fixed 1 TiB with 512 GiB free (`winfsp_server.rs:636-646`).

| Callback | What it does | Line |
|---|---|---|
| `get_security_by_name`, `get_security` | Attributes from the cache row or an SMB stat; one fixed permissive security descriptor for everything | 478-556, 157-176 |
| `open` | SMB stat, drift reconcile, one backing handle (read + write when granted) kept for the life of the open | 558-600 |
| `close`, `cleanup`, `set_delete` | Delete-on-close on the NAS, then drop the row and blob | 602, 1212-1267 |
| `get_file_info` | Cache row with a 5 s re-stat gate | 606-634, 397 |
| `get_volume_info` | Fixed sizes, label = share name | 636-646 |
| `read` | Hydrated → blob; partial → per-chunk fetch into a sparse blob; else passthrough | 648-719, 1545-1686 |
| `read_directory` | Re-enumerate if needed, fill from cache rows | 721-839 |
| `write` | Write-through to the NAS on the reused handle, mirror to the blob; no per-write fsync | 847-1002 |
| `create` | Create on the NAS, record the row | 1004-1078 |
| `overwrite`, `set_file_size` | Conflict pre-check (overwrite), truncate, drop the blob | 1080-1169 |
| `set_basic_info` | Best-effort attributes and times | 1171-1210 |
| `rename` | Collision and conflict checks, rename on the NAS, re-key the cache subtree | 1269-1308 |
| `flush` | fsync the backing handle | 1310-1331 |

Not implemented: `set_security`, reparse get / set / delete, stream info, extended attributes, `control`
(`winfsp_server.rs:475-1333`). Real NAS ACLs are never read; read-only / hidden / system attributes are not reported
(`winfsp_server.rs:428-432,459-463`). No change notifications are delivered: a peer's change is seen only when a
client re-queries, bounded by the 10 s kernel timeout and the 5 s gates (`winfsp_server.rs:1873,397`;
`windows_cache.rs:31`).

Offline (`winfsp_server.rs`): `open` always fails (565-571); a fully cached file is readable through a handle opened
earlier (663-684); partially cached files fail even for cached chunks (689-691); `read_directory` serves cached
children (754-767); every mutating callback fails fast. The struct comment claiming cached data "still serve"
(276-279) overstates this.

### 4.4 The cache (`agent/src/sync/windows_cache.rs`)

Same design as macOS, separately implemented: `<cache root>/<share>.db` + `<cache root>/by_key/<share>/<row id hex>`
(`windows_cache.rs:66,1229-1241`); the same three tables (260-282); 1 MiB chunks; sparse blobs via
`FSCTL_SET_SPARSE` (`winfsp_server.rs:74-91,1555-1568`); LRU eviction every 30 s down to 80 %
(`winfsp_server.rs:1766-1795`; `windows_cache.rs:1258-1295`); a 60 s prune grace and suspicious-empty guard
(848-872); migrations for case-insensitive keys, blob layout and path-key normalisation (97-206, 287-341); a corrupt
database is deleted and recreated (210-237). Differences that matter:

- `db()` panics on pool exhaustion (`windows_cache.rs:241-243`); macOS returns an error.
- `drain_all` and `cache_stats` cover fully cached files only; partial blobs survive "Drain"
  (`windows_cache.rs:1340-1405`).
- File-wide `#![allow(dead_code)]` hides an unused older API, `stat_and_refresh` and `rebuild`
  (`windows_cache.rs:1-6,353-595,1448-1567`).

### 4.5 The two audits (dev docs)

`devdocs/windows-mount-sync-audit.md` is a porting checklist from the macOS refactor: seven shared fixes that needed
only validation on Windows (lines 16-50) and six Windows-side audits to do — write-through cache, per-handle write
lock, stale post-write stat, orphan-prune guard, Y2038, conflict sidecar failure (lines 57-186). It also flags an
unchecked lifecycle case: toggling sync on and off may leave state inconsistent (lines 218-236).

`devdocs/windows-live-sync-audit-2026-09-11.md` measured the live path on a test machine (agent 1.2.0):

| # | Finding | Status in the doc | Lines |
|---|---|---|---|
| 1 | Files written with small buffered writes hung a fixed 240 s at close and stayed locked for 10+ minutes (a second handle held the mapped section open) | Fixed 2026-09-11: one reused handle; same case closes in 0.012 s | 65-121 |
| 2 | Cache blobs were not sparse: 104.5 GB allocated for 18.8 GB of content; the budget ignored partial blobs | Fixed; old blobs stay allocated until re-fetched or cleared | 123-161 |
| 3 | Two path-key formats in the database; every touched file had a ghost row; rename protection hit the wrong key | Fixed with a one-time migration | 165-201 |
| 4 | No per-file freshness: a peer's append or an editor's own atomic save was read back stale until a folder listing | Fixed: reconcile at open, 5 s gate on file info | 203-237 |
| — | Classic delete failed ("Incorrect function") | Fixed (`set_basic_info` implemented) | 248-260 |
| — | `cmd.exe` `dir` / `del` with an exact name and `Remove-Item -Recurse` find nothing | **Open**; "needs a WinFsp-level investigation" | 262-272, 358-359 |
| — | Small-read / small-write cliff: 4 KB reads 8.9 MB/s against 723 MB/s direct | **Open** | 54, 241-246 |
| — | Single-strike offline: one failed 30 s heartbeat fast-fails everything | **Open** | 274-278 |
| — | Log spam between GUI sessions | Fixed in the pipe server (`agent/src/ipc/server.rs:88-96,219-228`) | 280-285 |
| — | `sync_aware.rs` targets a path layout and API no longer used; freshness sweep is a no-op | **Open** | 287-293 |
| — | Pool-exhaustion panic; dead `stat_and_refresh` | Deferred | 352-357 |

Measured after-state: warm reads at native speed for ≥64 KB application reads, cold hydrate 177 MB/s, a 200 MB
buffered copy in 0.67 s (lines 50-58, 67-75). "The lifecycle layer is genuinely solid … The defects below are all in
the data path" (lines 32-37).

Checked against HEAD: the cmd.exe residual is still documented in the code (`winfsp_server.rs:1842-1851`); the
heartbeat result still sets the health flag with no threshold (`orchestrator.rs:1916-1923`), and the module comment
describing a failure threshold is stale (`nas_health.rs:3-5`); a cached read still costs two SELECTs, an UPDATE and a
blob open (`winfsp_server.rs:661-673`); the panic and the dead function are still there (4.4);
`read_directory` now logs at debug (`winfsp_server.rs:786-792`).

## 5. Credentials

| OS | Where | Form | Who reads it |
|---|---|---|---|
| macOS | Login Keychain, internet-password items written by the system sign-in dialog | OS-managed | NetFS, on ufb's behalf, because ufb passes NULL credentials (`core/src/macos_mounts.rs:1055-1060`; `agent/src/platform/macos/fallback.rs:307-327`). ufb never reads or stores the secret; the old per-app credential store was deleted (`agent/src/main.rs:594-596`) |
| Windows | Credential Manager, server-keyed domain-password entries | OS-managed, per-user | The SMB redirector, because ufb passes NULL credentials (`core/src/windows_mounts.rs:302-311`; `agent/src/orchestrator.rs:2139-2149`). ufb's only write is a one-time migration of its legacy generic entries to server-keyed ones, after which the legacy entry is deleted (`core/src/windows_mounts.rs:495-625`) |
| Linux | A prefixed entry in a platform store | — | Code references only; the module is absent (2.2) |

`mounts.json` holds no secrets (`agent/src/config.rs:38-43`). One stale comment in the agent's manifest still speaks
of a plaintext credentials file (`agent/Cargo.toml:39-41`); no such file or module exists in the tree.

## 6. Sessions, reconnect, timeouts — the numbers

| What | Value | Cite |
|---|---|---|
| Sync-mount heartbeat | every 30 s; a real `read_dir` (a stat is answered from the kernel's attribute cache even when the session is dead); 10 s timeout; a still-stuck previous probe counts as unreachable | `orchestrator.rs:447,612-670` |
| Offline trigger | one failed heartbeat → Offline → Reconnecting | `orchestrator.rs:1933-1947` |
| Reattach | Windows: re-run the session call; macOS: probe, force-unmount a dead backing mount that is ours, remount or adopt; if the share returns at a different path the NFS server is respawned | `orchestrator.rs:2008-2089` |
| Give up in place | after 3 consecutive failed heartbeats → Error → full teardown and remount | `orchestrator.rs:15,1966-1980` |
| Error auto-retry | exponential backoff 30 s, 60 s … capped at 480 s | `orchestrator.rs:559-576` |
| Sleep / wake | a heartbeat gap over 90 s resets the backoff | `orchestrator.rs:540-551` |
| VFS server ready wait | 15 s | `orchestrator.rs:1541-1545,1668-1670` |
| Mount shutdown wait | 15 s per mount | `mount_service.rs:746` |
| UI state emit while sync is active | every 2 s | `orchestrator.rs:446,522-526` |
| Config poll | 5 s | `agent/src/main.rs:447` |
| Plain-mount heartbeat (GUI) | 30 s, 10 s probe | `bindings/src/local_mounts.rs:436-457` |
| macOS NetFS | 60 s silent, 600 s with UI, 3 s TCP pre-flight, 10 s cancel grace | `netfs.rs:120-129,152` |
| IPC | 5 s client write budget; reconnect 500 ms ×5 then 3 s; 10 s command timeout | `unix_server.rs:54`; `mount_client.rs:684-688`; `bindings/src/services/mount.rs:468` |

## 7. LucidLink and "sync-aware" storage

**LucidLink (`core/src/lucidlink.rs`, 498 lines)** is independent of the mount layer and the agent. It finds
LucidLink's own command-line client, maps a path to a filespace by mount point, and offers four context-menu rows:
Pin Locally, Unpin, Copy LucidLink Link, Create Guest Link (`lucidlink.rs:1-25`;
`bindings/src/services/lucid.rs:68-117`). Everything shells out or calls the client's local HTTP port on loopback
(`lucidlink.rs:115-135,411-422`); CLI timeout 4 s, instance table cached 30 s, pin list 3 s (`lucidlink.rs:165-167`).
It changes nothing in how ufb mounts or caches. Four parser tests (`lucidlink.rs:304-323`).

**`core/src/sync_aware.rs` (152 lines)** is the hook that makes move and copy "instant" inside a sync root: move by
rename, copy by creating a Cloud Files placeholder through the `cloud-filter` crate (`sync_aware.rs:1-8,72-115`;
`core/Cargo.toml:110`). It is called from the file operations (`core/src/file_ops.rs:1751,1915`). Its notion of the
sync root is a folder path from an earlier design, so on today's drive-letter sync mounts it never matches
(`sync_aware.rs:124-141`; `devdocs/windows-live-sync-audit-2026-09-11.md:287-293`). In effect it is dead code, and a
copy inside a sync mount reads and rewrites through the mount instead of a server-side copy. No tests.

No other "sync-aware" storage detection (cloud-drive folders, other vendors) was found in the files in scope.

## 8. netfs-guard (`netfs-guard/`, 1,013 lines, 12 tests)

A small standalone crate used by both the GUI and the agent (`core/Cargo.toml:90`; `agent/Cargo.toml:28`). It exists
because of a macOS behaviour, described in its header (`netfs-guard/src/lib.rs:1-63`): every NetFS mount goes through
one per-user system daemon; two fresh SMB sessions to one server set up at the same moment can park one of its
threads forever, after which every later mount on the machine — Finder's included — queues behind it; cancel returns
success but does nothing; the only cure is restarting the daemon. NetFS gives no signal on a hang.

| Mechanism | What it does | Cite (`netfs-guard/src/lib.rs`) |
|---|---|---|
| Gate | One mount request at a time per user across both processes (advisory lock on a file); held until a verdict, at most 20 s; a waiter proceeds ungated after 30 s | 30-35, 109-114, 583 |
| Observation | A request still pending after 3 s gets the daemon sampled (the system `sample` tool); a thread seen in the account-lookup frames in two samples 3 s apart is a certain diagnosis | 36-42, 94-104, 145, 201 |
| Recovery | Cancel, restart the daemon automatically at most once per 10 minutes, wait 1.5 s for the client library's replay, re-issue; a recurrence inside the cooldown goes to the user as the "Restart…" pill | 43-52, 117-127, 825 |
| Pacing | After a host-level failure the host is held (30 s doubling to 5 min) in a small shared file, so several shares on a dead server cost one probe; user actions bypass it | 53-63, 136-141, 504 |
| Refusal | While the daemon is flagged wedged, new requests are refused up front with a dedicated error | 82-87 |

Dated 2026-11-05 / 06 in its own comments: the newest and most experimentally derived part of the layer.

## 9. The FinderSync extension (`macos-helpers/UFBFinderSync`, 380 lines of Swift)

| Topic | Finding | Cite |
|---|---|---|
| What it is | A sandboxed Finder Sync app extension embedded in the app bundle; one target, built with xcodegen + xcodebuild, deployment target macOS 14 | `macos-helpers/project.yml:1-61`; `scripts/release-mac.sh:102-128` |
| Scope | Monitors one folder: `~/ufb/mounts` (sync mounts only; plain `/Volumes` mounts get nothing) | `FinderSyncController.swift:22-38` |
| Badges | "Cached locally" (green check) and "Partially cached" (half circle), drawn from system symbols | `FinderSyncController.swift:61-75` |
| Toolbar + menu | A toolbar button and "Open in UFB"; "Drain Cache for this Share" on a share root | `FinderSyncController.swift:110-180` |
| Talks to | The agent, not the app: the same Unix socket and framing; reads `badge_update`, sends `clear_sync_cache`; reconnects every 3 s | `BadgeClient.swift:3-61,150-186` |
| Entitlements | Sandbox, the App Group, network client (a Unix socket counts as network in the sandbox) | `UFBFinderSync.entitlements:6-20` |
| Limits | No initial state: files cached before the extension started show no badge until they change. The agent never emits `partial` — only hydrated and uncached — so the "Partially cached" badge cannot appear. The user must enable the extension in System Settings; the bundle id is frozen to avoid re-approval | `BadgeClient.swift:14-18`; `macos_cache.rs:1433,1457`; `project.yml:17-19` |

## 10. Third-party dependencies of this layer

| OS | Dependency | Kind | Needed for | Cite |
|---|---|---|---|---|
| Windows | **WinFsp 2.x** | **Kernel-mode file system driver + user DLL, installed machine-wide by its own MSI (admin)** | Sync mounts only | `agent/Cargo.toml:47-55`; `installer/ufb_installer.iss:336-358` |
| Windows | `winfsp` crate 0.12, WinFsp SDK headers, LLVM `libclang` | Build-time | Building the agent | `agent/Cargo.toml:48-52`; `README.md:43-47` |
| Windows | `cloud-filter` crate 0.0.6 | Rust binding to the built-in Cloud Files API; no driver | Only the dead `sync_aware` copy | `core/Cargo.toml:110` |
| Windows | SQLite (bundled), r2d2 | Library | Cache index | `agent/Cargo.toml:66-68` |
| macOS | nfsserve fork | Vendored library, user space | NFS loopback server | `agent/Cargo.toml:94-103` |
| macOS | Kernel NFS client, NetFS, `mount` / `umount` / `diskutil` / `sample` / `osascript` | Ships with macOS | Mounting, teardown, diagnosis, the admin fix | sections 3 and 8 |
| macOS | SQLite (bundled), r2d2, `block2`, `core-foundation`, `notify` (unused) | Libraries | Cache, FFI | `agent/Cargo.toml:25-45` |

Constraints:

- **Nothing else on Windows needs a driver.** Plain mounts, the session calls, the pipe, the cache files and the
  credential handling are all built-in user-mode APIs.
- **Licence.** WinFsp is GPL-3.0 with a FLOSS exception; ufb is GPL-3.0 "to satisfy the FLOSS exception"
  (`agent/Cargo.toml:51`; `README.md:150-161`). Without WinFsp that constraint on the layer goes away.
- **Store.** The MSIX cannot carry WinFsp; the listing must disclose the prerequisite first (policy 10.2.4), and the
  package runs as full trust with `runFullTrust` + `privateNetworkClientServer`
  (`packaging/windows/msix/AppxManifest.xml.in:10-18,98-101`). The classic installer bundled the MSI, ran it silently
  as admin, and primed SmartScreen for both executables (`installer/ufb_installer.iss:77,196-201,336-386`).
- **macOS.** No system or kernel extension. Requirements are Developer ID signing and notarization of the app and the
  separate agent bundle, the App Group, and the user enabling the Finder extension (3.7, 9). The agent is not
  sandboxed; a Mac App Store build would not allow `mount`/NetFS from a sandbox — **not verified**, no evidence in the
  repository either way.

## 11. Windows without WinFsp: what maps to a built-in facility

A list, not a design.

| Today's feature | Built-in facility that covers it | Notes |
|---|---|---|
| Add / edit / remove a share, enable, bookmark | None needed (config) | Already independent of WinFsp |
| A share at a drive letter | WNet (`WNetAddConnection2W`), as plain mounts do today (4.1) | Already shipped |
| Persistent mapping restored at logon | `CONNECT_UPDATE_PROFILE` | Already shipped |
| Credential prompt and storage | `WNetUseConnectionW` interactive + Credential Manager | Already shipped |
| Auth-failed / unreachable status, 30 s health, reconnect | The GUI's existing mount task | Already shipped |
| Real NAS ACLs, attributes, alternate streams, change notifications, byte-range locks | The SMB redirector provides all of these on a plain mapping | The WinFsp mount hides or fakes them today (4.3), so a plain mount is *more* faithful here |
| Offline availability of files and folders | Windows Offline Files (client-side caching) for SMB shares; **not verified** as suitable — edition, policy and server-flag dependent, no evidence in the repository | A different model (whole-file, sync centre) |
| On-demand placeholders with hydrate-on-open, per-file state icons in Explorer, "free up space", pinning | Cloud Files API (`cfapi`), with the in-box `cldflt` filter; the repository's own history used it (`agent/src/config.rs:69`, `core/src/sync_aware.rs:72-115`) | Works on a sync-root *folder* on a local NTFS volume, not on a drive letter; the provider must populate and hydrate from the share itself; partial (ranged) hydration exists in the API. Why ufb left it for WinFsp is **not found** in the files in scope |
| Projected folder tree served by a user-mode provider | Projected File System (ProjFS) — an optional Windows feature that must be enabled; stale comments mention it (`windows_cache.rs:597,923`) | Read-mostly projection; enabling the feature needs admin |
| Server-side copy inside a share | The redirector does it for a plain mapping or UNC copy | Today a copy inside a sync mount goes through the cache instead (section 7) |

No built-in equivalent as built today:

| Today's feature | Why there is no direct equivalent |
|---|---|
| A **drive letter** whose reads are served from a local 1 MiB-chunk cache, transparently to every application | A user-mode file system at a drive letter is exactly what WinFsp supplies; Cloud Files and ProjFS both live in a folder |
| Chunk-level partial caching of very large media read by any app through a letter | Same; Cloud Files can hydrate ranges but under a sync root |
| Fast-fail of every file operation the moment the NAS is marked down (no redirector timeout) | The redirector's own timeouts apply to a plain mapping |
| Conflict sidecars on overwrite of a file that changed on the NAS | Only possible because the agent sits in the write path; an application writing to a plain mapping bypasses ufb (ufb's own file operations could still check) |
| Cached folder listings for instant browsing in Explorer and other apps | The redirector has only short-lived directory caching; ufb's own browser could keep its own listing cache, other apps could not use it |
| Cache stats, Drain, cache location and budget | Exist only because the cache exists |

What the user would lose in practice, per the audits: faster repeat reads of large media over a slow or remote link,
and the limited offline reading of already-open files. What they would stop suffering: the driver prerequisite, the
cmd.exe enumeration quirk, faked ACLs and volume size, no change notifications, the small-block read cliff, and the
240 s class of bugs.

## 12. Size and health

### 12.1 Lines and tests

| Component | Files | Lines | Tests |
|---|---|---|---|
| Agent control plane | `main` 669, `config` 708, `messages` 355, `state` 730, `mount_service` 774, `orchestrator` 2,332, `ipc/*` 543, `platform/mod` 44 | 6,155 | config 14, messages 6, state 20, orchestrator 1; `mount_service`, `main`, `ipc` 0 |
| macOS mount layer (agent) | `platform/macos/*` | 1,863 | 22 |
| macOS data path | `nfs_server` 2,555, `macos_cache` 2,471, `macos_watcher` 53 | 5,079 | 25 |
| Windows data path | `winfsp_server` 1,947, `windows_cache` 1,575, `platform/windows/*` 251 | 3,773 | **0** |
| Shared sync helpers | `cache_core` 251, `conflict` 76, `nas_health` 63, `sync/mod` 124 | 514 | 0 |
| **Agent crate total** | | **17,384** | 88 |
| nfsserve fork | `external/nfsserve/src` | 4,969 | not counted |
| netfs-guard | | 1,013 | 12 |
| GUI side | `macos_mounts` 1,621, `windows_mounts` 676, `mount_client` 1,006, `local_mounts` 637, `services/mount.rs` 2,089, dialog 961 | 6,990 | 10 + 5 + 4 + 0 + 0 |
| FinderSync | Swift | 380 | 0 |
| LucidLink, sync_aware | | 650 | 4 + 0 |

Roughly 31,000 lines for the whole layer including the vendored fork. Tests are unit tests of pure functions (FSM,
config parsing, mount-table parsing, bitmap and SQL helpers); there is no integration test of either VFS server, of
the write path or of conflict handling. No `agent/tests` directory exists.

`TODO` / `FIXME` / `HACK` / `XXX`: none in any file in scope (grep). Limitations are instead recorded as dated
comments ("audit 2026-09-11 C-6", "KNOWN RESIDUAL"), which is where the items below come from.

### 12.2 Data-loss and correctness risks in the write and conflict paths

| # | Risk | Evidence |
|---|---|---|
| 1 | **Conflict detection has blind windows by design.** A peer's edit within 15 s of our own write, or a same-size edit within the echo window, is not seen: "A peer edit landing inside these seconds of our own write is the accepted residual" | `nfs_server.rs:255-256,977-978,1386-1410` |
| 2 | **Windows checks conflicts only on overwrite and rename-replace.** An application that writes in place over a file a peer changed gets no sidecar | `winfsp_server.rs:1102-1104,1295-1301` |
| 3 | **Sidecar names have one-second granularity**; a second conflict in the same second overwrites the first sidecar | `nfs_server.rs:2323-2324`; `conflict.rs:46-54` |
| 4 | **Conflict and badge messages are best-effort** (`try_send`, dropped when the channel is full), so a conflict can be preserved on disk with the user never told | `nfs_server.rs:645-654`; `winfsp_server.rs:1418-1427` |
| 5 | A failed stat before the check lets the destructive operation proceed with no sidecar | `nfs_server.rs:577-581` |
| 6 | **Local clock used as the file's mtime after a write.** With client / NAS clock skew beyond the 1–2 s tolerances, the next open can see false drift (cache dropped) or a false conflict **(reading)** | `nfs_server.rs:1689-1698`; `winfsp_server.rs:987-994,1392` |
| 7 | Cache blob is not fsynced on the write-through path; after a crash the index can claim bytes the blob lacks. Hydrated reads demote a short or missing blob, which covers truncation but not torn content **(reading)** | `nfs_server.rs:1593-1623,1445-1483` |
| 8 | "a persist racing an `invalidate_cache` can resurrect bits over a deleted blob" | `macos_cache.rs:1179-1181` |
| 9 | macOS CREATE over an existing cached file truncates on the NAS but the upsert does not clear the row's hydration state or blob; stale bytes could be served until the new content is written through **(reading)** | `nfs_server.rs:1794-1799`; `macos_cache.rs:1238-1248` |
| 10 | Windows: readers update the chunk bitmap under a shared lock; two concurrent readers of one file can lose bits (re-fetch, not corruption) **(reading)** | `winfsp_server.rs:1549-1550,1571,1684` |
| 11 | Windows: every write I/O error is reported to the application as "disk full" | `winfsp_server.rs:49,908` |
| 12 | Windows: `cleanup` (delete-on-close) is not gated on NAS health and does not sync, although a comment says it does | `winfsp_server.rs:914-916,1227-1267` |
| 13 | A folder genuinely emptied on the NAS may never be pruned from the cache: the "suddenly empty" guard has no memory of the previous attempt **(reading)** | `macos_cache.rs:745-759` |
| 14 | A sidecar-copy failure returns "try later" on a hard mount, so the client retries indefinitely instead of surfacing an error **(reading)** | `nfs_server.rs:609-619,171-181` |

### 12.3 Races, panics and fragile areas

| Area | Evidence |
|---|---|
| macOS mount daemon wedge — the reason for a whole crate, a registry of in-flight requests and an exit path that must cancel them; "a client dying with a request in flight wedges the daemon for every process on the machine"; a hard crash still skips it | section 8; `core/src/macos_mounts.rs:728-729`; `app/main.cpp:441-442` |
| A dead loopback NFS mount hangs Finder / Spotlight; mitigated by startup cleanup, `intr`, `deadtimeout=60`. The cleanup would also rip out a live sibling agent's mounts, hence the ordering after the single-instance check | `agent/src/main.rs:574-590`; `nfs_server.rs:67-70,171-181` |
| Lock `unwrap()` on request paths: a poisoned mutex panics the handler, and a dead handler wedges the hard mount | `nfs_server.rs:312,320,477,501,1849,1870`; `macos_cache.rs:526-533` |
| Windows panics reachable from callbacks: pool exhaustion, poisoned locks, the security-descriptor builder | `windows_cache.rs:242`; `winfsp_server.rs:188,668,882,1550` |
| Single-strike offline on both OSes (one missed 30 s probe) | `orchestrator.rs:1916-1947` |
| Blob leaks on Windows: blobs skipped under a lock are "left for scan_and_index", which nothing calls | `windows_cache.rs:886-898,1448-1494` |
| Eviction protects only in-flight calls; on Windows the `open_handles` map is stored and never consulted | `windows_cache.rs:41,79,1304-1307`; `orchestrator.rs:1588-1601` |
| Duplicated code kept in step by hand: NetFS FFI twins, three config structs, the IPC types, the App Group string | `core/src/macos_mounts.rs:4-7`; `core/src/mount_client.rs:19,180,216-220,852-855` |
| Stale documentation in code: file headers describing earlier designs, comments that contradict the code | `nfs_server.rs:1-12`; `winfsp_server.rs:1-11,276-279,733,1362-1368`; `nas_health.rs:3-6`; `mount_service.rs:473-484` |
| GUI: the one-shot agent spawn "is raceable"; named-pipe I/O uses a 10 ms peek-poll loop to avoid a deadlock | `bindings/src/services/mount.rs:233-236`; `core/src/mount_client.rs:426-428,481` |
| Privacy note for a public rebuild: comments and test constants in `core/src/macos_mounts.rs`, `core/src/windows_mounts.rs` and two dev docs contain what look like real host, share and user names | not cited by line on purpose |

## 13. Separability

The browser runs with the agent absent, today:

- The agent is not spawned when no sync mount is configured, and a missing agent binary is only logged
  (`bindings/src/services/mount.rs:740-749,1458-1462`); connection failure is a debug log and a retry
  (`core/src/mount_client.rs:564-566,676-678`).
- Start / Stop / Restart go to the GUI's own mount tasks first (`bindings/src/services/mount.rs:1123-1129`); the UI
  says "agent not needed" / "agent idle" (`MountManagerDialog.qml:182-185`; `app/qml/Ufb/App/Main.qml:2293-2298`).
- The Store build is the proof: no WinFsp, sync unavailable, "the rest of the app works normally"
  (`packaging/windows/msix/README.md:135-136`).

Without the agent the user loses: sync mounts, cache stats and Drain, conflict events, the freshness sweep, Finder
badges (`bindings/src/services/mount.rs:1302-1327,1694-1715`; `core/src/mount_client.rs:149-152,527-533`).

Coupling points to the rest of the app:

| Importer | Of what | Cite |
|---|---|---|
| `bindings/src/services/mount.rs` (2,089 lines) | Everything: the `Mount` QML object, both mount owners, agent launch | — |
| `core/src/events.rs`, `core/src/app_state.rs` | `mount_client` types (the `AppState` client appears unused) | `events.rs:113`; `app_state.rs:29,69,137` |
| `core/src/file_ops.rs` | `sync_aware` (dead in practice) | `file_ops.rs:1751,1915` |
| `bindings/src/services/subscription.rs`, `columns.rs` | The list of jobs-folder mount paths | `subscription.rs:57`; `columns.rs:1916` |
| QML | `Mount.*` in `FileBrowser.qml`, `Main.qml`, `Sidebar.qml`, the dialog | — |
| Path identity | `mountedAt` feeds the volume registry so the same share resolves on every machine | `agent/src/messages.rs:132-138` |

So the layer splits cleanly into three independent pieces: (a) plain OS mounts + credentials + health, in the GUI
process, about 5,000 lines, no driver; (b) the agent with its FSM and IPC, about 6,000 lines; (c) one data path per
OS, 5,000 lines (macOS, plus the 5,000-line fork and netfs-guard) and 3,800 lines (Windows, untested, WinFsp). Piece
(c)-Windows can be removed without touching (a); if only macOS keeps sync, (b) is still needed there.

## Open questions for the owner

1. **Scope on Windows.** Is "plain mapped drive only" the intended Windows end state (what the Store build already
   gives without WinFsp), or should some on-demand behaviour return through a built-in facility (Cloud Files sync-root
   folder, Offline Files)? If so, is a *folder* instead of a *drive letter* acceptable for it?
2. **Why was Cloud Files left for WinFsp?** The history is visible (config comment, `sync_aware.rs`, the `cloud-filter`
   dependency) but the reason is not in the files in scope. It decides whether question 1's second option is worth a
   spike.
3. **What is the cache for, today?** Remote / VPN work on large media, or LAN speed-ups too? The measured numbers show
   it slower than direct SMB for cold reads and small blocks on a LAN; its value is on slow links. Who actually runs
   sync mounts, on which OS?
4. **Offline expectations on macOS.** Keep the present model (read what is already cached; no offline writes, no
   pinning), or is "keep this folder available offline" wanted? Pinning does not exist today.
5. **Conflict policy.** Keep sidecar-on-overwrite with its accepted blind windows (15 s after own write; best-effort
   notification), or tighten it? Should a sidecar ever be created when ufb is not in the write path (plain mounts)?
6. **Keep the agent as a separate process on macOS?** It exists so sync mounts survive the GUI quitting. If macOS
   keeps the cache, is that lifetime still wanted, and should it start at login (it does not today)?
7. **Finder extension.** Keep the badges and the "Drain" menu? The extension needs user approval in System Settings,
   only covers sync mounts, has no initial state and its "partial" badge is never emitted.
8. **The macOS mount point.** Sync mounts appear at `~/ufb/mounts/<share>` (not a sidebar volume) while the backing
   share is also visible at `/Volumes/<share>`. Is exposing both paths acceptable in the rebuild, and should path
   identity keep treating them as one?
9. **nfsserve.** Keep the vendored fork (and reconcile its licence statement), try to upstream the five fixes, or
   replace the NFS loopback with something else on macOS?
10. **netfs-guard.** Its behaviour was derived by experiment in November. Carry it over as is (it is the newest and
    most machine-specific part), and is the automatic restart of a system daemon acceptable policy for the rebuild?
11. **Provisioning profile.** With credentials read by the OS, is the shared-keychain profile described in the dev
    doc still needed at all?
12. **Linux.** The agent's Linux arms reference a module that is not in the tree. Is Linux out of scope?
13. **Existing users.** On Windows, should the rebuild migrate a sync mount to a plain mapping on the same letter and
    delete the cache folder, and should it offer to remove WinFsp, or leave it?
