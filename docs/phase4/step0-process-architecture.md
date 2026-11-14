# Phase 4, step 0 - minRender's process architecture (audit for the Rust port)

Read 2026-11-14 from the MinRender repository's working tree (branch `min-paths` = release 0.5.11 plus a
path-translation change). Read-only: nothing was built or run. All paths below are relative to the MinRender
repository root unless they start with `crates/` or `apps/` (this repository).

Scope: which processes exist, why, and what a single-language port could do instead. The network and on-disk
contracts and the UI inventory are covered by the two sibling step-0 documents; network details appear here only
where they explain the process split.

Claims marked **(by reading)** are conclusions drawn from the code that were not reproduced by running anything.

## Summary

- A running node is ONE long-lived process (`minRender`, Qt UI + all farm logic in-process) plus a SHORT-LIVED
  `mr-agent` process that exists only while a chunk renders: it is spawned on demand at the first dispatch, runs one
  renderer command, reports, and exits. There is no persistent agent, no service, no daemon.
- The app and the agent speak a private, same-machine IPC (named pipe on Windows, Unix socket elsewhere;
  4-byte little-endian length + UTF-8 JSON). It is NOT part of the farm's wire contract: no other node ever sees it.
  The port is therefore free to keep, change or remove it without affecting compatibility with 0.5.x nodes.
- No written rationale for the split was found. The agent has existed since the first commit (C++ / ImGui app + Rust
  agent). What the code shows it buys today: the renderer's process tree dies when the agent dies (Windows job
  object), and the supervisor's blocking pipe reads live outside the UI process. It does not buy privilege
  separation, sandbox escape, or rendering while the app is closed.
- A UI-less build of the node already exists (`minrender-headless`, same core sources, `main_headless.cpp`), ships
  inside the Windows package but is unreachable there (the Store refused the headless entry), and is not shipped on
  macOS at all.

## 1. Executables built and shipped

| Binary | What it is | Built by | Windows (Store MSIX) | macOS (.app in a DMG) |
|---|---|---|---|---|
| `minRender` (`minRender.exe`) | The app: Qt 6 Quick UI, tray, and the whole node (coordination, HTTP server and client, discovery, election, database, dispatch, render supervision). CMake target `minrender`, `OUTPUT_NAME minRender`. Entry `src/monitor/main_qt.cpp`. | `CMakeLists.txt` (`qt_add_executable(minrender ...)`, core list `MINRENDER_CORE_SOURCES`) | Package root; the manifest's only `Application` (`installer/msix/AppxManifest.xml.in`) | `Contents/MacOS/minRender` |
| `mr-agent` (`mr-agent.exe`) | Rust render agent: runs ONE renderer command, parses its output, kills its tree. | `cargo build --release` in `mr-agent/` (`mr-agent/Cargo.toml`, `mr-agent/build.rs` embeds icon + VERSIONINFO on Windows). CMake does not drive cargo; it copies a prebuilt binary next to the app (`CMakeLists.txt`, the two `MR_AGENT_RELEASE_BIN` blocks). | Package root, next to `minRender.exe` (`scripts/package.bat` copies it; `scripts/make_msix.ps1` refuses to pack without it) | `Contents/MacOS/mr-agent`, signed separately with the same entitlements (`scripts/macos-release.sh` step 6d) |
| `minrender-headless` | The node with no window: same `MINRENDER_CORE_SOURCES` + `src/monitor/main_headless.cpp` + the Win32 tray (`src/core/system_tray.cpp`, a no-op off Windows). Links Qt6 Core + Network only (for the single-instance lock). | `CMakeLists.txt` (`add_executable(minrender-headless ...)`, define `MINRENDER_HEADLESS`) | Copied into the package (`scripts/package.bat`) but has no `Application` entry and no alias, so nothing can launch it: "headless is a dev-build feature on Windows" (`installer/STORE.md`, manifest comment; Partner Center rejected the hidden entry without a `HeadlessAppBypass` waiver) | Not shipped: `scripts/macos-release.sh` builds `--target minrender` only, and the bundle's `Contents/MacOS` holds `minRender` and `mr-agent` only |
| `mr-restart.exe` | Windows-only helper, two jobs: (a) with `--pid --exe [--minimized]` it waits up to 15 s for the app to exit (then terminates it), sleeps 2 s and relaunches it - refusing any `--exe` outside its own directory; (b) with NO arguments it is the MSIX startup task and launches `minRender.exe --minimized` (a startup task cannot pass arguments). `src/restart/main.cpp`. | `CMakeLists.txt` (`if(WIN32) add_executable(mr-restart ...)`) | Package root; named as the `windows.startupTask` executable in the manifest | Does not exist |
| Sparkle helpers (`Autoupdate`, `Updater.app`, XPC services) | The macOS updater's own processes, inside `Sparkle.framework`. | Vendored by `scripts/fetch_sparkle.sh`; embedded by `CMakeLists.txt` when `external/Sparkle` exists | None | `Contents/Frameworks/Sparkle.framework`, re-signed inside-out in `scripts/macos-release.sh` step 6-pre |
| `tools/rndr-companion/*.ps1` | An external PowerShell script (not shipped with the app) that runs another program while the node is idle. It follows the node through a registry key or `GET /api/status` on loopback. | - | Installed by hand by the operator | - |

Other build facts:

- A Rust static library is already linked into the C++ app: `cmake/MinPaths.cmake` builds `crates/min-paths-ffi` from
  this repository at a pinned revision.
- Windows ships only through the Store; the package is signed by the Store at ingestion. Capabilities are exactly
  `internetClientServer`, `privateNetworkClientServer`, `runFullTrust` (`scripts/make_msix.ps1` fails the pack if they
  drift). The manifest declares firewall rules for `minRender.exe` ONLY (inbound TCP 8420, UDP 4243) and the startup
  task.
- macOS: Developer ID signing with the hardened runtime, one entitlement
  (`com.apple.security.cs.disable-library-validation`, `scripts/entitlements.plist`), notarized and stapled twice
  (app, then DMG). `LSUIElement` is false (Dock icon plus menu-bar status item, `cmake/MacOSXBundleInfo.plist.in`).
- Both `mr-agent` lookups are "next to my own executable": `AgentSupervisor::spawnAgent()` in
  `src/monitor/agent_supervisor.cpp` (`GetModuleFileNameW` / `_NSGetExecutablePath`, then `mr-agent[.exe]` in the same
  directory). Same for `mr-restart.exe` in `MonitorApp::launchRestartSidecar()` (`src/monitor/monitor_app.cpp`).

## 2. The app / agent split in detail

### 2.1 What the app does itself

Everything except running the renderer process. `MonitorApp` (`src/monitor/monitor_app.h/.cpp`) owns:

- node identity and config (`NodeIdentity`, `Config`);
- the HTTP server (`src/core/http_server.cpp`, cpp-httplib) and all HTTP client calls to peers and the leader;
- discovery, liveness and election (`PeerManager`, `UdpNotify`);
- the leader's database and dispatch (`DatabaseManager`, `DispatchManager`), snapshots included;
- templates and submissions (`TemplateManager`, `SubmissionWatcher`), the farm-share command dropbox
  (`pollFarmCommands`);
- render bookkeeping (`RenderCoordinator`, `src/monitor/render_coordinator.cpp`): the per-node dispatch queue, the task
  JSON, token substitution, staging directories and the copy back to the output directory, the per-chunk stdout log
  written to the farm share, completion / failure reports to the leader;
- the agent's lifecycle and the IPC server (`AgentSupervisor`, `IpcServer`);
- the UI (`src/ui/*`, QML), the tray, single-instance, the updater bridge.

One node renders ONE chunk at a time: `RenderCoordinator::m_activeRender` is a single optional, and
`POST /api/dispatch/assign` answers 409 `busy` while it is set (`src/core/http_server.cpp`).

### 2.2 What `mr-agent` does (all six source files, 1 714 lines)

- `main.rs`: parses `--node-id`, sets up logging (stderr + `mr-agent.log` in the per-user data folder, truncated above
  2 MB), installs a panic hook that logs, takes a per-node named mutex on Windows (`MinRenderAgent_{node_id}`; no
  equivalent on Unix), connects to the app, sends `status idle` with its pid, then runs a two-mode loop: idle (blocking
  read) and rendering (100 ms poll of render events and of the pipe).
- `executor.rs` (`RenderExecutor`): builds the `std::process::Command` from the task (executable, args, optional
  working directory, environment, plus `PYTHONUNBUFFERED=1`), pipes stdout and stderr, and starts three threads: a
  stderr reader, an optional timeout watchdog (`timeout_seconds`, 100 ms tick, independent of output so a silent hang is
  still killed), and the stdout worker. Stdout lines are batched (50 lines or 5 s) with stderr lines folded in as
  `[stderr] ...`. Exit handling: a recorded kill reason wins over the exit status; otherwise exit 0 = completed, non-zero
  = failed.
- `parser.rs`: three regex helpers fed from the task - `ProgressParser` (`fraction` with numerator / denominator groups,
  or `percentage` with one group; first matching pattern wins), `OutputParser` (last matched output path), and
  `CompletionParser` (a per-frame "done" line; the Nth match on stdout OR stderr is reported as frame
  `frame_start + N`, capped at `frame_end`). `ProgressSpec::error_patterns` is deserialized but never used.
- `kill.rs` (`ProcessKiller`): the process-tree kill.
  - Unix: the child is put in its own process group before exec (`Command::process_group(0)`); kill is `SIGKILL` to
    `-pgid`. A `finished` flag guards against signalling a recycled group id.
  - Windows: after spawn the child is assigned to a job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; kill is
    `TerminateJobObject`; if job setup failed the fallback is `taskkill /PID <pid> /T /F`. Because the agent holds the
    only handle, the tree also dies if the agent itself dies.
  - Kills are immediate (no graceful signal first) and idempotent. `RenderExecutor`'s `Drop` kills too, so every agent
    exit path that unwinds takes the tree with it.
- `ipc/mod.rs`, `ipc/windows.rs`, `ipc/unix.rs`: the client transport (below).
- `messages.rs`: the serde types (below).
- `watchdog.rs`: one comment line ("Monitor process watchdog", never written).

### 2.3 The IPC

**Transport.** The app is the server, the agent the client. One connection at a time.

| | Windows | macOS / Linux |
|---|---|---|
| Endpoint | named pipe `\\.\pipe\MinRenderAgent_{nodeId}` | Unix stream socket `minrender-agent-{nodeId}.sock` in `$TMPDIR` (with trailing slashes stripped), falling back to `/tmp` when `TMPDIR` is unset or the path would not fit `sun_path` (104 bytes on macOS, 108 elsewhere) |
| Server | `IpcServer::create` in `src/core/ipc_server.cpp`: byte-mode duplex overlapped pipe, max 1 instance, 64 KiB buffers | same file, `#else` half: `bind` under `umask(0077)` then `chmod 0600`, `listen(fd, 1)`; a stale socket file is unlinked first |
| Client | `mr-agent/src/ipc/windows.rs`: `WaitNamedPipeW` (5 s) + `CreateFileW`, 3 attempts 3 s apart | `mr-agent/src/ipc/unix.rs`: `UnixStream::connect`, 3 attempts 3 s apart; non-blocking peek via `recv(MSG_PEEK or MSG_DONTWAIT)` |
| Access control | DACL granting Generic All to the current user's SID only | filesystem permissions only |

The path derivation is duplicated by hand on both sides (`agentSocketPath()` in `src/core/ipc_server.cpp` and
`socket_path()` in `mr-agent/src/ipc/unix.rs`) and must match exactly.

**Framing.** `[u32 little-endian payload length][UTF-8 JSON]`, both directions (`write_message` / `read_message` in
`mr-agent/src/ipc/mod.rs`; `IpcServer::send` / `receive`). Both sides reject payloads over 16 MiB. The app's writes
are serialized by a mutex and time out after 5 s on Windows.

**Messages, app to agent** (`MonitorToAgent`, tagged by `"type"`, snake_case):

| type | fields | sent by |
|---|---|---|
| `ping` | none | nothing in the current app sends it (the agent answers `pong`) |
| `shutdown` | none | `AgentSupervisor::stop()` and `shutdownAgent()` |
| `abort` | `reason: string` | `AgentSupervisor::sendAbort()` |
| `task` | `job_id: string`, `frame_start: u32`, `frame_end: u32`, `command: { executable: string, args: [string] }`, `working_dir: string or null`, `environment: { string: string }`, `progress: null or { patterns: [{ regex, type: "fraction" or "percentage", numerator_group = 1, denominator_group = 2, group = 1, info }], completion_pattern: null or { regex, info }, error_patterns: [{ regex, info }] }`, `output_detection: null or { regex: string or null, capture_group = 1 }`, `timeout_seconds: u64 or null` | `RenderCoordinator::buildTaskJson()` / `dispatchChunk()` |

**Messages, agent to app** (`AgentToMonitor`):

| type | fields | handled by |
|---|---|---|
| `pong` | none | ignored (`AgentSupervisor::processMessages`) |
| `status` | `state: "idle" or "rendering"`, `pid: u32` | `processMessages`: stores state and pid; `idle` sets ready-for-work |
| `ack` | `job_id`, `frame_start`, `frame_end` | `RenderCoordinator::handleAgentMessage`: marks the ack and restarts the chunk's clock (the flag is never read afterwards) |
| `progress` | `job_id`, `frame_start`, `frame_end`, `progress_pct: f32`, `elapsed_ms: u64` | stores the percentage |
| `stdout` | `job_id`, `frame_start`, `frame_end`, `lines: [string]` | appended to the chunk's log file on the farm share |
| `frame_completed` | `job_id`, `frame: u32` | recorded and reported to the leader |
| `completed` | `job_id`, `frame_start`, `frame_end`, `elapsed_ms: u64`, `exit_code: i32`, `output_file: string or null` | `onChunkCompleted` (staging delivery, then the report) |
| `failed` | `job_id`, `frame_start`, `frame_end`, `exit_code: i32`, `error: string` | `onChunkFailed` |

The app's handlers do not compare a message's `job_id` / frame range with the render they believe is active; every
render message is applied to `m_activeRender` (see 7.2).

**Authentication.** None beyond the endpoint's access control. There is no handshake, token or version exchange; the
agent proves nothing, and the app does not check the peer's pid or credentials. `--node-id` only selects the endpoint
name. The agent's version is not sent.

### 2.4 Lifetime

- **Who starts whom.** `MonitorApp::init()` calls `AgentSupervisor::start(nodeId)`, which creates the pipe / socket and
  starts the IPC thread - but spawns NO agent. The agent is spawned by `RenderCoordinator::update()` the first time a
  dispatch is waiting and no agent is connected (`supervisor.spawnAgent()`: `CreateProcessW` with `CREATE_NO_WINDOW`, or
  `posix_spawn`, inheriting the app's environment). If it has not connected within 10 s the chunk is failed; if the
  spawn itself fails the chunk is failed and the node is marked `needs_attention`.
- **One agent per render.** After `completed` or `failed` the agent sends `status idle` and leaves its main loop
  (`main.rs`, the `if done { ... break; }` block), i.e. the process exits. The app's IPC thread logs "Agent disconnected
  (render cycle complete)" and sets ready-for-work (`AgentSupervisor::ipcThreadFunc`, with the comment "The agent is
  one-shot per render"). `git log -S` dates this behaviour to v0.3.7; before that the agent was long-lived, which is
  what its idle mode, `ping` / `pong`, and the reconnect logic were written for.
  - Exception: `POST /api/agent/restart` (`src/core/http_server.cpp`, called by a leader for a peer whose agent health
    is `needs_attention`, `DispatchManager::tryRemoteAgentRestart`) shuts the agent down and spawns a fresh one with no
    task. That agent sits in idle mode until the next dispatch uses it.
- **The agent does not outlive the app by design.** `MonitorApp::shutdown()` aborts the current render, then
  `AgentSupervisor::stop()` sends `shutdown`; on Windows it waits 3 s and then `TerminateProcess`es the agent. On Unix
  `stop()` neither waits for nor signals the child. Closing the app therefore ends the render.
- **When the app dies uncleanly.** An idle agent exits on the first read error. A rendering agent keeps the render
  running and tries to reconnect: `attempt_reconnect` makes 6 attempts with a 5 s sleep each, and each attempt's
  `connect` retries 3 times 3 s apart, so roughly a minute passes (the comment says "~30 seconds") before it kills the
  tree and exits. If a new app instance with the same node id appears in that window, the agent reconnects to it and
  announces `status rendering` (see 7.2).
- **When the agent dies.** The app sees the disconnect; `RenderCoordinator::update()` starts a 30 s grace period
  (`DISCONNECT_GRACE_SECONDS`) but fails the chunk at once when `isAgentRunning()` is false ("Agent process died during
  render"). The IPC thread sets ready-for-work on every disconnect. On Windows the renderer tree dies with the agent
  (job object). On Unix it dies only if the agent unwinds (`Drop`); a `SIGKILL`ed agent leaves the renderer's process
  group running.
- **Kill paths.** Cancel / stop / node-stop / edit / shutdown all end in `sendAbort(reason)`; the agent calls
  `ProcessKiller::kill`, the worker sees EOF, and reports `failed` with the kill reason. The app has already cleared its
  active render by then (`abortCurrentRender` reports "failed"; `abandonCurrentRender` reports "abandoned" with no
  failure bookkeeping), so the agent's own `failed` arrives with no active render and is ignored.
- **Progress.** Stdout is parsed inside the agent with the task's regexes (2.2); the app only stores the percentage and
  forwards frame completions.

## 3. Why the split exists

No statement of intent was found: not in code comments, `docs/`, `SECURITY.md`, `installer/STORE.md`, nor in commit
messages (searched `git log --all --grep` for agent, sandbox, job object, privilege, crash, headless, sidecar, tauri,
orphan, zombie; and the history of `mr-agent/`, 45 commits, almost all version bumps). The agent is in the first commit
together with a C++ / ImGui app. The only prose is a retired README line: closing to the tray keeps "only communication,
coordination, and a lightweight Rust agent to manage render processes" - written when the agent was long-lived.

Candidate reasons against the evidence:

| Candidate | Verdict | Evidence |
|---|---|---|
| Kill the renderer's whole tree, and guarantee it dies if its supervisor dies | **Confirmed as what the agent does today**, but not a reason for a separate process | `mr-agent/src/kill.rs` (job object with kill-on-close since v0.4.7; process group on Unix). The same calls work from inside any process; what the separate process adds is that the job handle's owner is small and has nothing else to crash on. |
| Crash isolation of the UI from the renderer | **Weak** | The renderer is already a separate process either way. What is isolated is the supervisor (pipe readers, regexes) from the app. The reverse isolation - a render surviving an app crash - is not delivered: the app's replacement cannot adopt the render (7.2), and a clean exit kills it. |
| MSIX / Store sandbox: launching external DCC programs | **Ruled out** | The agent predates Store packaging (first commit vs. the MSIX commits of 2026-08). The package is `runFullTrust`; the manifest comment says the APP "spawns DCC renderers and the mr-agent sidecar as child processes", and the agent is a plain child inside the same package identity. Nothing about the agent changes what may be launched. |
| Windows job objects / process-group kill needing Rust | **Ruled out as a language reason** | The app itself calls `CreateProcessW`, `TerminateProcess`, `posix_spawn`, `waitpid` (`src/monitor/agent_supervisor.cpp`). The job-object code is about 40 lines of Win32. |
| Rendering while the UI is closed | **Ruled out** | Closing the window hides it (`Main.qml` `onClosing`), the process keeps running and so does the node; quitting the process stops the farm node and aborts the render (`MonitorApp::shutdown`). The agent never runs without the app. There is no service or daemon mode. |
| Privilege separation | **Ruled out** | Same user, same token, same package identity, inherited environment; the pipe's DACL / socket mode admit exactly that user. The agent runs whatever executable the task names (`SECURITY.md`, "Renders run arbitrary code by design"). |
| A stable seam while the UI was rewritten three times | **Plausible, circumstantial** | The app went ImGui, then a Tauri attempt with an out-of-process UI (commits of 2026-03-29 / 03-31, removed 2026-04-24), then Qt. The Qt port plan listed the "Rust agent protocol" among things not to touch. The agent's code barely changed through all of it. |
| Keeping blocking I/O off the UI loop | **Plausible, circumstantial** | The app's design is a 50 ms single-threaded pump (5); line-by-line reads of a child's stdout and stderr and the timeout watchdog would have needed threads inside the app. Putting them in another process was the alternative chosen at the start. |

Related evidence about process boundaries the owner has drawn on purpose: `SECURITY.md` ("Binary distribution") records
that 0.5.6 REMOVED an in-process supervisor of a third-party program because hidden launches and `taskkill` were "the
strongest miner-like behaviour signal in the binary", and restricted `mr-restart.exe` to relaunching its sibling; both
`CMakeLists.txt` and `mr-agent/Cargo.toml` note that binaries without version metadata attract antivirus false
positives. Any new helper executable, re-exec scheme or hidden child process should be weighed against that history.

## 4. Background behaviour

- **Close to tray.** `Main.qml` `onClosing` refuses the close and hides the window; `main_qt.cpp` sets
  `setQuitOnLastWindowClosed(false)`. Everything keeps running (5).
- **Tray.** `src/ui/platform/tray.cpp` (`QSystemTrayIcon`): Show Window, Stop / Resume (node state), Exit; click or
  double-click restores the window; a tooltip and status text refreshed every tick. One icon, no status colours (the
  tinting is noted as deferred; `MonitorApp::trayIconName()` still computes a colour nobody shows). On macOS the icon is
  a template image and the app also has a Dock icon; a Dock click re-shows a hidden window (`applicationStateChanged`
  hook in `main_qt.cpp`).
- **Two exits that behave differently (by reading).** The tray's Exit calls `MonitorApp::requestExit()`, which waits
  for the current render to finish before the pump quits. The File menu's Exit is `Qt.quit()` (`Main.qml`, `exitAction`),
  which leaves the event loop at once; `shutdown()` then aborts the render. A comment in `main_qt.cpp` says the pump is
  "the only path out of `app.exec()`", which the menu contradicts. `isExitPending()` / `cancelExit()` exist in
  `MonitorApp` but nothing in `src/ui` uses them.
- **Start at login.** Windows: the MSIX `windows.startupTask` (enabled by default, toggled by the user in Task
  Manager) runs `mr-restart.exe` with no arguments, which launches `minRender.exe --minimized`; `--minimized` makes the
  root window start invisible (`startMinimized` in `main_qt.cpp` / `Main.qml`). macOS: nothing - no login item, launch
  agent or `SMAppService` call anywhere in the tree.
- **Restart after a Store update or a crash (Windows).** `registerAppRestart()` (`src/ui/platform/app_restart.cpp`)
  calls `RegisterApplicationRestart("--minimized")` when the process has package identity, and `SessionEndWindow` owns a
  hidden top-level window so a tray-only app still receives `WM_ENDSESSION`; its handler calls `beginForceExit()`. The
  header records a measured relaunch about 0.3 s after an update closed the app.
- **Remote / self restart.** `MonitorApp::launchRestartSidecar()` starts `mr-restart.exe --pid --exe --minimized` with
  `CREATE_BREAKAWAY_FROM_JOB`, waits 3 s, then force-exits. Triggers: `POST /api/node/restart`, a `restart` file in the
  node's folder on the farm share (`PeerManager::checkRestartSignal`, ignored when older than 60 s), and the UI
  (`AppBridge::requestRestart`). On macOS the function logs "not supported" and returns false.
- **Single instance.** `src/core/single_instance.cpp`: a `QLockFile` named `MinRenderMonitor.lock` in the temp
  directory with stale-lock stealing disabled, plus a `QLocalServer` of the same name; a second launch connects, which
  shows the first one's window, and exits. Both the app and `minrender-headless` use the same name, so they exclude each
  other. Because stealing is disabled, a lock left by a crash is documented as needing manual removal (comment in the
  constructor).
- **Updater.** macOS: Sparkle 2 (`src/ui/updater/sparkle_updater_macos.mm`), started from a zero-delay timer after the
  event loop is up, daily background checks from the Info.plist keys, plus "Check for Updates" in the About menu.
  Windows: none - the Store delivers updates, `AppUpdater::available()` is false and the menu item is removed;
  `docs/appcast-win.xml` is frozen for pre-Store installs. `minrender-headless` and `mr-agent` never link the updater
  ("so a render node can never interrupt a job with an update prompt", `CMakeLists.txt`).
- **Same-machine state for other programs.** `publishNodeState()` (`src/core/platform.cpp`) writes render state, node
  state, pid and a timestamp to a per-user registry key on Windows on every transition (pid 0 = clean exit). Under the
  Store package the key is virtualized and invisible to other programs, which fall back to `GET /api/status` on loopback
  (`SECURITY.md`, `tools/rndr-companion/README.md`).
- **What runs with no window.** All of section 5. Hiding the window stops rendering of the UI only; the 50 ms pump
  still calls `bridge.refresh()`.

## 5. Threads and long-running loops in the app

The node's logic is a polled state machine: `MonitorApp::update()` is called every 50 ms and does everything that is
not on one of the threads below. In the GUI build the caller is a `QTimer` on the Qt main thread (`main_qt.cpp`); in the
headless build it is a plain `while` loop with a 50 ms sleep that also pumps Win32 messages for the tray
(`main_headless.cpp`). `MonitorApp` and its managers use no Qt types.

| Loop | Thread | Period | Tied to the UI event loop? |
|---|---|---|---|
| `MonitorApp::update()`: UDP receive + 3 s heartbeat, agent messages, `RenderCoordinator::update` (dispatch queue, agent spawn, grace timers, staged-frame delivery), leader transition, leader's `DispatchManager::update`, 2 s job-cache refresh, 3 s suspension mirror, 3 s submissions poll, command dropbox, node-state publish, deferred farm restart, restart signal, exit | caller's thread (Qt main thread in the app) | 50 ms | **Yes in the app** (a `QTimer`), no in headless |
| `AppBridge::refresh()` and the two `AppBridge` timers (chunks, remote log) | Qt main thread | 50 ms / own intervals | Yes (UI only) |
| HTTP server | `HttpServer::m_thread` + cpp-httplib's pool (`CPPHTTPLIB_THREAD_POOL_COUNT=4`) | event-driven | No |
| HTTP worker: one-off posts to the leader, completion and frame reports | `MonitorApp::m_httpWorkerThread` | 50 ms | No |
| Peer manager: endpoint file, restart signal, phonebook discovery, peer polling, election | `PeerManager::m_thread` | about 3 s | No |
| Template scan | `TemplateManager::m_thread` | cooldown-gated | No |
| Agent IPC reader | `AgentSupervisor::m_ipcThread` | blocking, 1 s receive timeout | No (messages are queued and drained by `update()`) |
| Leader database open / restore from the snapshot | `MonitorApp::m_leaderThread` | once per leadership | No |
| Database snapshot to the share | detached thread per snapshot (`DispatchManager::doSnapshot`) | per snapshot | No |
| Assignment POST to a worker; remote agent restart | detached thread per call (`DispatchManager`) | per call | No |
| UI-initiated blocking calls (farm cleanup scan / execute, manifest fetch, chunk fetch, resubmit) | detached threads in `src/ui/app_bridge.cpp`, results marshalled back with queued connections | per call | Started by the UI |
| Sparkle | its own | daily | macOS UI only |

What this means for a "headless node as the same binary":

- The core is already UI-agnostic and already runs under two different drivers. The only UI-loop coupling is WHO calls
  `update()`.
- Because `update()` runs on the Qt main thread in the app, everything it does synchronously blocks the UI: the
  per-chunk stdout append to the farm share (`RenderCoordinator::flushStdout`), the staged-frame copy to the output
  directory (`copyStagingFiles`, whole files, retried every 30 s for up to 600 s), the submissions and command-dropbox
  directory scans, and several HTTP client calls with 500 ms connect timeouts (`getChunksForJob`,
  `refreshSuspendedNodes`, the job-cache refresh on workers). On a slow or absent share this is a frozen window. A port
  that gives the node core its own thread removes that coupling without changing any contract.
- Some HTTP handlers call into `MonitorApp` and the supervisor directly from pool threads (for example
  `/api/agent/restart` calls `shutdownAgent()` / `spawnAgent()`, `/api/node/restart` calls `launchRestartSidecar()`),
  while the same members are used from the pump thread without a lock (7.6).

## 6. Options for the port

Common ground for all four:

- The agent IPC is private to one machine and one install; no option is constrained by farm compatibility. What the
  farm sees is the HTTP API, UDP messages, the share's files and the fields the node reports about its agent
  (`agent_health`, `ready_for_work` in `/api/status` and the UDP heartbeat's `ah` / `rfw`). A 0.5.x leader skips a node
  whose `agent_health` is not `ok` or whose `ready_for_work` is false and asks it to restart its agent via
  `POST /api/agent/restart` when it reports `needs_attention` (`DispatchManager`). Every option must keep reporting
  those fields truthfully and keep answering that route, whatever "agent" means inside the new node.
- The data folder, node id file, config, log file names (`mr-agent.log` is listed in `docs/install.md`) and the
  single-instance name are per-machine contracts with plug-ins, the companion script and an upgraded install, not with
  other nodes.
- This repository has no tray / status-item component yet (nothing in `crates/min-ui` or the two apps); it has a
  single-instance module (`apps/qcv/src/single_instance.rs`) and a Sparkle bridge (`apps/mn/src/sparkle.rs`). A tray is
  needed by every option that keeps "close to tray".
- Windows packaging facts that bind every option: one `Application` entry; a second, hidden one was refused; the
  startup task cannot pass arguments; firewall rules are declared per executable name; `runFullTrust` is already
  justified to the Store as "launches locally installed render applications as child processes ... and supervises a
  bundled render agent" (`installer/STORE.md`) - a change in the process list means rewording that justification.

### How reusable `mr-agent` is

- **Size and shape.** 1 714 lines, edition 2021, a binary crate with private modules (no `lib.rs`), synchronous
  `std::thread` + `mpsc`, no async runtime.
- **Dependencies** (`mr-agent/Cargo.toml`): `serde` (derive), `serde_json`, `clap` 4 (derive), `log`, `simplelog`,
  `regex`; `env_logger` is declared but unused. Windows: `windows` 0.58 (FileSystem, Security, Pipes, IO, Threading,
  JobObjects, Foundation). Unix: `libc`. Windows build dependency: `winresource`. 77 entries in the lock file (the crate itself included).
- **Platform code.** `kill.rs` (job object / process group), `ipc/windows.rs`, `ipc/unix.rs`, the named mutex and the
  log path in `main.rs`.
- **Tests.** Two unit tests, Unix only, both in `kill.rs` (a grandchild dies with the group; kill is a no-op after the
  child was reaped). Nothing covers the parsers, the framing, the executor or the Windows job object.
- **Reusable as-is as a process** (option a): entirely, byte for byte.
- **Reusable as a module** (options b, c, d): `executor.rs`, `kill.rs`, `parser.rs` and the `RenderEvent` enum lift out
  with almost no change - `RenderExecutor::start(TaskMessage)` already returns a handle with `poll_events()` and
  `abort()` and has no dependency on the pipe. `messages.rs` needs `Serialize` on the task side (and `Deserialize` on
  the reply side) if the types are to be shared by both ends. `main.rs` (the loop, reconnect, mutex) and `ipc/*` are
  needed only if a process boundary remains. The one behaviour to carry consciously is in `main.rs`, not the modules:
  one render per process, which today also guarantees a fresh job object and no state between renders.
- **Regex dialect.** Template patterns are compiled by Rust's `regex` crate today, so a Rust port keeps the dialect
  exactly; no template needs to change under any option.

### (a) Two processes: a Rust app plus the existing `mr-agent`, unchanged, over the existing IPC

- **Preserves exactly:** the render path - spawn, kill, parse, timings, `mr-agent.log`, the agent's file name in the
  process list, the tree dying with the agent on Windows.
- **Cost:** the app's side of the IPC must be rewritten in Rust (the server halves of `src/core/ipc_server.cpp`, about
  650 lines, and `AgentSupervisor`, about 450), including the overlapped-pipe server, the DACL, the socket permissions
  and the hand-matched path rule. Two crates and two build outputs remain, or `mr-agent` moves into the workspace.
- **Risk:** inherits every fragility in section 7 unless the supervisor side is redesigned, at which point "unchanged"
  no longer holds. A mixed install (new app, old agent binary) becomes possible in principle and has no version check.
- **Farm operator:** no visible change.
- **Packaging:** unchanged in kind - still several executables in the MSIX, still a second Mach-O to sign with its own
  entitlements step.
- **`mr-agent` reuse:** 100 %.

### (b) One binary, the agent's code as a module

- **Preserves:** the task semantics, parsing, kill behaviour and timeouts (same code), and everything the farm sees.
- **Changes:** no `mr-agent` process and no IPC; the supervisor's threads live in the node process. `mr-agent.log`
  either goes away or becomes a section of the node's log (it is named in `docs/install.md`). "Agent health" becomes a
  synthetic value, and `POST /api/agent/restart` must still answer 200 and do something sensible (reset the health
  flag).
- **Cost:** the smallest amount of code overall; roughly 1 100 lines of IPC on the C++ side and about 800 on the agent
  side (`main.rs`, `ipc/*`) are not ported at all.
- **Risk:** the job object's kill-on-close handle now lives in the node: if the node dies, the renderer still dies with
  it (same as today's outcome one step removed), but a panic in parsing or supervision is a panic in the node unless
  contained (`catch_unwind` at the thread boundary). Renderer children are direct children of the app; with a GUI
  process as the parent, console-window and handle-inheritance behaviour on Windows needs checking (today the agent is
  created with `CREATE_NO_WINDOW` and the renderer inherits that console-less state). On Windows, a renderer that must
  outlive nothing and be killed with its tree needs the job assigned before it can spawn children (7.4), which is
  easier to do properly here than to preserve.
- **Farm operator:** one process in the task list while rendering instead of two; one log.
- **Packaging:** the MSIX loses `mr-agent.exe` (and `make_msix.ps1`'s check for it); macOS loses the separate signing
  step. The Store justification text mentions "a bundled render agent" and should be reworded.
- **`mr-agent` reuse:** `executor.rs`, `kill.rs`, `parser.rs`, the task types - about 850 of 1 714 lines, unchanged in
  substance.

### (c) One binary with a window or without (`--headless` / service mode), render supervision in a re-exec'd child of itself

- **Preserves:** today's process SHAPE (a supervisor process per render that owns the job object and can be killed
  alone) with one file on disk; the IPC can stay as it is or become the child's stdin / stdout, which removes the named
  endpoint, its access control and the path rule altogether.
- **Changes:** the helper is the same executable started with a mode argument. The windowless node becomes a mode of
  the shipped binary instead of a second target that the Store package cannot launch.
- **Cost:** everything in (b) plus a mode switch at the very top of `main` (before any window, GPU or tray
  initialisation) and the choice of child transport. The windowless mode needs the node core on its own thread
  regardless (5), which (b) also wants.
- **Risk:** a GUI-subsystem executable re-launching itself hidden is the pattern most likely to be read as suspicious by
  antivirus heuristics (section 3, the 0.5.6 history); the firewall rules and the startup task name an executable, so a
  single name helps there. A re-exec'd child must not run the single-instance check, the updater, or the tray. On macOS a
  second instance of the bundle's main executable is an ordinary child process, but it must not register with the Dock
  (activation policy set before any AppKit start-up). Whether the Store accepts a `--headless` argument on the one
  `Application` entry was not tested by the current product: what was refused was a second, hidden `Application`.
- **Farm operator:** render nodes could run with no window at all (today that needs a dev build on Windows and is not
  shipped on macOS); the process list shows the same name twice while rendering.
- **Packaging:** one executable to version-stamp, sign and declare; `minrender-headless` disappears as a separate
  artifact. `mr-restart`'s no-argument startup-task role remains necessary unless the app treats "launched by the
  startup task" as minimized by other means.
- **`mr-agent` reuse:** as (b), plus `main.rs`'s loop if the child keeps today's message protocol (about 1 300 of 1 714
  lines), minus `ipc/*` if the transport becomes stdio.

### (d) A UI-less node daemon plus a separate UI that talks to it

- **Preserves:** farm behaviour, and it is the only option in which closing or crashing the UI cannot disturb a render
  and the node can run before anyone opens a window.
- **Changes:** two long-lived programs with independent lifetimes, and a UI-to-node control channel that does not exist
  today. The product already tried this shape once (the 2026-03 Tauri scaffold with a push IPC, removed 2026-04-24; the
  Qt plan then fixed "Process model: in-process"). The HTTP API covers much of what a UI needs (jobs, chunks, logs,
  config, cleanup, path mappings) but local-only state is reached in-process today (`AppBridge` calls `MonitorApp`
  directly), `GET /api/config` was deliberately taken off the unauthenticated list (`src/core/http_server.cpp`,
  comment at the auth handler), and the UI would need the farm secret or a separate local channel.
- **Cost:** the largest. A local API, its authentication, version skew between UI and daemon across updates, start /
  stop ordering, two single-instance rules, and where the tray lives.
- **Risk:** Windows Store: a background executable with no window is exactly what was refused as a "headless app"
  when it had its own entry; started as a child of the visible app it is permitted today, but then its lifetime is tied
  to a logged-in session and the startup task, i.e. it is not a service. A true Windows service or a macOS launch daemon
  runs outside the user's session, where mapped drives and mounted shares - the farm's sync root - are generally not
  available, and where DCC programs that need a logged-in user or a GPU session may not run. macOS: a login item or
  launch agent means `SMAppService` registration and a helper inside the bundle with its own signature.
- **Farm operator:** the biggest change - "is the node running?" stops being "is the app in the tray?".
- **Packaging:** at least two long-lived executables in the MSIX and the bundle, each named in firewall rules and
  startup configuration.
- **`mr-agent` reuse:** as (b) or (c), inside the daemon.

Options (b) and (c) are not exclusive: the module boundary of (b) is what (c) re-executes, and (c)'s windowless mode is
what (d) would call its daemon. The decisions that separate them are in the last section.

## 7. Surprising or fragile points in the current process model

1. **The agent is one-shot, but half its code and the app's supervisor still assume a long-lived agent.** Idle mode,
   `ping` / `pong` (never sent), the reconnect loop, the 30 s grace period, `agent_health`'s documented `reconnecting`
   value (`src/core/peer_info.h`; never produced by `AgentSupervisor::agentHealth()`), and `watchdog.rs` are remnants. A
   port should decide the model first rather than carry both.
2. **A render can be attributed to the wrong chunk after an app restart (by reading).** If the app dies mid-render and
   a new instance starts within the agent's reconnect window (about a minute; Windows re-launches a packaged app after
   updates and crashes), the old agent reconnects and reports `status rendering`. The new instance has no active render,
   starts with ready-for-work true, and nothing clears it on a `rendering` status. If the leader then assigns a chunk,
   `RenderCoordinator::update()` sees a connected agent and sends the task to it; the agent logs "Received task while
   already rendering, ignoring". There is no ack timeout (`ackReceived` is written, never read). When the OLD render
   ends, its `completed` / `failed` / `frame_completed` messages are applied to the NEW active chunk, because
   `handleAgentMessage` does not compare ids.
3. **Reconnect window and grace period disagree.** The agent's comment says its reconnect budget matches the app's
   30 s; the nested retries make it about twice that. Also, `ipcThreadFunc` sets ready-for-work on ANY disconnect,
   including a dropped pipe with the agent and the renderer still alive.
4. **Windows: the job is assigned after the process starts.** `ProcessKiller::new` calls `create_job_for(child)` after
   `cmd.spawn()`; a renderer that starts a helper before `AssignProcessToJobObject` runs leaves that helper outside the
   job. The Unix path has no such gap (`process_group(0)` applies between fork and exec). If job setup fails the code
   silently falls back to `taskkill`.
5. **Unix: orphans and unreaped children (by reading).**
   - An agent killed with `SIGKILL` leaves the renderer's process group running; nothing else knows its pid.
   - `AgentSupervisor::stop()` has no Unix branch: at shutdown the agent is sent `shutdown` and never waited for.
   - After each render the IPC thread calls `waitpid(..., WNOHANG)` once, immediately after the disconnect. The agent
     closes its socket slightly before it exits, so that call can find it still running; `m_childPid` is then kept and
     simply overwritten by the next spawn, leaving the earlier child unreaped until the app exits.
     `isAgentRunning()` uses `kill(pid, 0)`, which also succeeds for a process that has exited but was not reaped.
   - The Windows twin: the process handle is closed only if the agent had already exited at that same instant;
     otherwise `spawnAgent()` overwrites `m_processHandle` (one leaked handle per render).
   - There is no per-node single-instance guard for the agent on Unix (the mutex is Windows-only); a second agent
     queues in the listen backlog and is accepted after the first disconnects.
6. **Cross-thread use of the supervisor.** `POST /api/agent/restart` runs `shutdownAgent()` (a blocking wait of up to
   5 s) and `spawnAgent()` on an HTTP pool thread, while `RenderCoordinator::update()` uses the same handles from the
   pump thread; `m_agentPid`, `m_agentState`, `m_processHandle` / `m_childPid` are plain members with no lock.
   `/api/node/restart` likewise sets `m_restartPending` from a pool thread.
7. **Install-path assumptions.** The agent and the restart helper are found only next to the running executable; there
   is no search path, no version check, and no check that the file is the expected one. A missing agent is discovered at
   the first dispatch, not at start-up (the chunk fails and the node reports `needs_attention`). The socket path depends
   on `TMPDIR`: the agent inherits the app's environment, so the two agree today, but any launcher that changes `TMPDIR`
   for one side breaks the pairing silently (the agent retries for about 6 s, the app fails the chunk at 10 s).
8. **Two exits with different semantics** (4): the tray's Exit waits for the render, the menu's Exit kills it.
9. **Single-instance lock never goes stale.** A crash leaves `MinRenderMonitor.lock` behind and the next launch
   refuses to start as "not first" until the file is removed by hand (the comment in `SingleInstance`'s constructor says
   so). On an unattended render node that is an outage. Whether Qt's lock still detects a dead owner pid with stale time
   0 was not tested here.
10. **Restart is Windows-only.** `launchRestartSidecar()` returns false on macOS, so the peer menu's remote restart,
    the share's `restart` file and the UI's restart do nothing there; and macOS has no start-at-login. A macOS render
    node that reboots or updates stays off the farm until someone opens the app.
11. **Environment and logging.** The renderer inherits the app's whole environment plus the manifest's; the agent logs
    the executable but never arguments or environment (`SECURITY.md`, "Secrets and logs") - a property a folded-in
    supervisor must keep.
12. **Main-thread file and network I/O** (5): frame delivery, stdout appends and share scans run on the UI thread.
13. **An implicit dependency (not verified in this read).** The Unix IPC server writes with plain `write()` and the
    tree installs no `SIGPIPE` handling of its own; a write to a socket whose agent just died is survivable only if
    something else in the process (the HTTP library or Qt) has set `SIGPIPE` to ignored. A Rust port gets this for free
    (the Rust runtime ignores `SIGPIPE` by default), but it is worth knowing that today's behaviour is inherited, not
    designed.

## Open questions for the owner

1. **Is a separate supervisor process wanted for its own sake?** The evidence shows no sandbox, privilege or
   "render while closed" reason. The remaining arguments are containment of supervisor faults and a small owner for the
   job handle. Is that worth a process boundary in an all-Rust node (options a / c), or is a contained thread enough
   (option b)?
2. **Should a render survive the app?** Today it never does: quitting, updating or crashing ends it (and a crash can
   mis-attribute it, 7.2). Keep "the app's exit ends the render" as the rule, or make renders adoptable by a restarted
   node (which needs render identity in the protocol and argues for c or d)?
3. **Is a windowless node a product feature?** Today it is a dev-only target on Windows and absent on macOS. If render
   nodes should be able to run with no window, is that a mode of the one binary (c) or a separate program (d)? And is
   running before login / as a service wanted at all, given that the farm share and the DCC programs live in a user
   session?
4. **macOS parity.** Start at login, restart after an update, and the remote-restart commands do not exist on macOS.
   Does the port add them (and with what mechanism), or keep the present asymmetry?
5. **`mr-restart`'s future.** Its two jobs are the argument-less startup task and the relaunch. Keep a helper
   executable, or fold both into the app (a "launched with no window" rule, and a self-relaunch)? The Store's startup
   task cannot pass arguments, and 0.5.6 deliberately narrowed what this helper may do.
6. **What should `agent_health` and `POST /api/agent/restart` mean** in a node with no separate agent? They must keep
   existing for 0.5.x leaders; the question is their meaning and whether the UI still shows an "agent" at all.
7. **Which exit is the intended one** - wait for the render (tray) or stop it (menu)? The port should have one rule.
8. **The fragilities in section 7: reproduce or fix?** In particular 7.2 (mis-attribution), 7.4 (job assigned after
   start), 7.5 (unreaped children), 7.9 (stale lock). None is visible to other nodes, so fixing them does not affect
   compatibility; carrying them would be parity for its own sake.
9. **`mr-agent.log`** is a documented file. Keep the name, or fold it into the node's log and update the install page?
10. **Antivirus and Store posture.** Is a hidden re-launch of the app's own executable (c) acceptable given the 0.5.6
    history, or is a separately named, version-stamped helper (a) the safer reading by heuristics? This may deserve a
    small packaging spike (a sideloaded MSIX and a scan) before the architecture is fixed.
11. **Where does `mr-agent`'s code live** if it is reused: moved into this repository's workspace as a crate (with
    tests added for the parsers, framing and the Windows job object, which have none), or kept in the MinRender
    repository and consumed from there?
