# Phase 4, step 1 - the RNDR companion and its contract with minRender

Read-only audit of the minRender repository at branch `min-paths` (release 0.5.11 plus the path-library change),
2026-11-14. Nothing was built or changed. Paths are relative to the minRender repository unless they start with
`docs/phase4/`. "The script" is `tools/rndr-companion/rndr-companion.ps1`; a bare `:N` after it is a line number in
that file.

Identifiers are left out on purpose (public repository): the Store package's Identity Name is referred to as "the
value of `app_package`" (the script's default, `:85`); no machine, account or share is named.

## 0. Summary

- The companion is a per-user PowerShell script, installed by hand on Windows nodes, never shipped with the app. It
  runs a third-party GPU client (the RNDR client: launcher `rndrclient.exe`, worker processes named `TCPSVCS`) while
  minRender is idle, kills it when a render starts, and relaunches it after a cooldown. Optional extras: a "DCC gate"
  (stand down while an artist's Blender / After Effects is open) and a keepalive that relaunches minRender itself.
- It reads from minRender exactly four things: a registry key, one unauthenticated HTTP route, one field of
  `config.json`, and the process table (`minRender` by name). It writes nothing minRender reads. It passes one
  argument to minRender (`--minimized`) and only when the keepalive is on.
- It never looks for `mr-agent.exe`. A port without that process does not confuse it. A port that starts **extra
  processes named `minRender.exe`** can (section 6).

## 1. What it is for, and how it came to be

Plain words: some farm nodes earn their keep between jobs by running another network's render client on the same
GPU. Two programs cannot share the GPU, so something has to stop the other client the instant a farm render starts
and bring it back afterwards. Until 0.5.5 minRender did that itself ("RNDR dual mode"). That code - a hidden launch of
an executable from the user's AppData and a forced kill of a process with a system-sounding name - was "the strongest
miner-like behaviour signal in the binary" (`SECURITY.md:172-175`), so 0.5.6 moved it out into a script the operator
installs. The app now only publishes two facts (rendering or idle; active or stopped) and "knows nothing about" the
consumer (`SECURITY.md:139-155`).

| Date | Commit | Release | What happened |
|---|---|---|---|
| 2026-02-28 | `83bf767` | 0.3.3 | First in-app supervisor (`src/monitor/rndr_supervisor.*`); comments say it mirrors an earlier PowerShell script. |
| 2026-04-23 / 24 | `16a08a4`, `f068be1` | - | Removed from the UI, then from the backend ("no longer used in production"). |
| 2026-06-18 | `81ef8c2` | 0.4.9 | Restored, Windows only. Timings fixed here: watchdog 5 s, warm-up 15 s, hang grace 15 s, cooldown 120 s, start-up delay 120 s. |
| 2026-09-20 | `247054e` | 0.5.4 | The creative-app gate added (opt-in). Program Files is "the whole discriminator"; occupancy is sampled, never inferred from open / close events; presence is the kill trigger, the settle only gates relaunch. The message records four rejected designs (input-idle detection, audio-capture detection, a path denylist, a self-disabling circuit breaker) "so they are not re-proposed". |
| 2026-10-17 | `57d1555` | 0.5.6 | Security audit: the supervisor, its three config keys, the `/api/config` field, the bridge properties and the Settings toggle removed. The app starts publishing `HKCU\Software\MinRender\Node`. |
| 2026-10-17 | `257dcf1` | 0.5.6 | The companion added: same state machine and timings; registry notification with an HTTP fallback; "unreachable app = hold steady"; logon task through `conhost --headless`. |
| 2026-10-22 | `0f94bb8` | 0.5.7 | Store denial: the package keeps its registry virtualized, so on Store installs the companion cannot see the key and runs on the HTTP poll. |
| 2026-10-23 | `1298a0d` | 0.5.8 | Data folder moves from `%LOCALAPPDATA%\MinRender` to `%USERPROFILE%\minRender`; the companion resolves the new folder with the old one as fallback. |
| 2026-10-23 | `1bc83ab` | - | Incident: the installer had written an absolute `rndr_path` into the old folder; deleting that folder (as the install page suggests) left the companion pointing at nothing. Fresh configs now omit `rndr_path`; old ones are remapped at load. |
| 2026-10-24 | `f42906b` | - | Incident: the "unreachable" branch ran before the gate, so an artist who quit minRender and opened a creative app left the client on the GPU. Gate now applies there. Keepalive added (`app_keepalive`, `-KeepAlive`, `-NoRndr`) because a Store auto-update closes the app and does not restart it. |
| 2026-10-24 | `2242a0c` | 0.5.9 / 0.5.10 | The app's own answer to the same problem: `RegisterApplicationRestart("--minimized")` and a hidden window that answers the session-end message (`src/ui/platform/app_restart.*`). |
| 2026-10-24 | `f755c8f` | - | Incident: the installer wrote `dcc_gate = false` and never read the app's old `rndr_dcc_gate`, so nodes that had the gate on lost it silently. One-time carry-over added. |
| 2026-10-28 | `96099a6` | - | Two incidents: (1) a client left running across a companion restart stayed on the GPU next to an artist, because the Inactive state only skipped launching - it now kills; (2) the kill returned before the processes were gone, so the hung-client path logged a kill and then "already running, adopting" - the kill now waits up to 10 s. |

The companion is not versioned and not tagged; "which companion a node runs" is whatever copy of the script was last
installed there.

## 2. The files; install, start, update, removal, privileges

| File | Lines | What it does |
|---|---|---|
| `tools/rndr-companion/rndr-companion.ps1` | 629 | The watchdog. One long-running Windows PowerShell process. `-Once` runs one pass and exits (`:40-45`, `:606`). |
| `tools/rndr-companion/install.ps1` | 173 | Copies the script into the data folder, writes or repairs the config, carries the old in-app gate over once, registers and starts a scheduled task. Switches: `-TaskName`, `-NoStart`, `-KeepAlive`, `-NoRndr` (`install.ps1:26-36`). |
| `tools/rndr-companion/uninstall.ps1` | 41 | Disables, waits, removes the task, kills strays, deletes the installed script and the companion's registry key. |
| `tools/rndr-companion/README.md` | 142 | Operator documentation. |

**Install** (`install.ps1`): run as the user who runs minRender, no elevation (`install.ps1:23`).

1. Data folder = `%USERPROFILE%\minRender` if it holds `config.json` or if `%LOCALAPPDATA%\MinRender` does not exist;
   otherwise the old folder (`install.ps1:43-48`; the same function is in the script, `:54-59`).
2. Copy the script to `<data>\companion\rndr-companion.ps1` (`install.ps1:55-56`).
3. If `<data>\rndr-companion.json` is missing, write five defaults, ASCII (`install.ps1:61-69`); otherwise keep it and
   repoint a `rndr_path` that still names the old folder (`install.ps1:76-88`).
4. Once per user (`Companion\DccGateMigrated`), read `rndr_dcc_gate` / `rndr_block_processes` from the current and the
   old `config.json` and, only if the gate was on, turn `dcc_gate` on with that list (`install.ps1:100-122`).
5. `-KeepAlive` sets `app_keepalive = true`; `-NoRndr` sets `rndr_enabled = false` (`install.ps1:124-129`).
6. Set `HKCU\Software\MinRender\Companion\Enabled = 1` (`install.ps1:131`).
7. Register the scheduled task (default name "minRender RNDR companion", `install.ps1:133-158`), then start it.

**How it is kept running**: a per-user scheduled task, not a service and not a Startup entry.

| Task property | Value | Reference |
|---|---|---|
| Action, Windows 11 (build >= 22000) | `conhost.exe --headless powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "<script>"` | `install.ps1:135-137` |
| Action, older builds | `powershell.exe -WindowStyle Hidden ...` (a window flashes at logon) | `install.ps1:139` |
| Trigger | at logon of the installing account, delayed 30 s | `install.ps1:146-147` |
| Principal | that account, interactive logon, limited run level | `install.ps1:149` |
| Settings | runs on battery, start when available, ignore a second instance, no time limit, restart every minute up to 999 times on failure | `install.ps1:151-156` |

The task lives in the interactive session because the client needs the desktop and the GPU; it "runs only when the
user is logged on and ends with the session" (`README.md:142`). Inside the script a session-local mutex
`Local\MinRenderRndrCompanion` enforces one instance (`:394-395`). Task Scheduler stops the task by killing `conhost`,
not PowerShell, so the script records its parent's pid and exits when the parent is gone, leaving the client as it is
(`:427-436`, `:453`).

**Update**: there is no updater. The operator re-runs `install.ps1`, which overwrites the copy and re-registers the
task. The config is read once at start (`:397`), so a config edit needs a task restart.

**Removal** (`uninstall.ps1`): set `Enabled = 0` and sleep 7 s so the companion kills the client itself (stopping the
task would leave it running, `uninstall.ps1:6-9`, `15-20`); stop and unregister the task; kill any `powershell.exe`
whose command line contains `rndr-companion.ps1` (`uninstall.ps1:30-32`); delete `<data>\companion` in both data folders; delete
the whole `Companion` registry key (`uninstall.ps1:40`). The config and the log stay. `docs/install.md:73-74` tells users to run it
before deleting the data folder.

**Privileges**: a standard user. It needs: the right to register a task for itself; `powershell.exe` 5.1 with
`-ExecutionPolicy Bypass` on the command line; read / write of its own `HKCU`; read / write in the data folder; the
right to terminate its own user's processes; `Get-AppxPackage` and COM activation for the keepalive. No administrator
rights, no service, no driver, no firewall rule, no network access beyond loopback.

## 3. The watchdog loop

### 3.1 Start-up (`:393-449`)

1. Delete the log if it is over 2 MB (`:150-156`).
2. Take the mutex or exit 0.
3. Read the config (section 3.6) and log one `starting:` line.
4. Initial state `Inactive`. `lastOccupied = now`, so with the gate on a full settle is served before the first
   launch ("unknown history", `:416`).
5. Record the parent pid; create an event; open the Node key for notification, or log that it is absent (`:438-440`).

### 3.2 One pass (`:452-623`)

| Step | What | Reference |
|---|---|---|
| 1 | Exit if the parent process is gone. | `:453` |
| 2 | Read `Companion\Enabled` (missing = enabled). | `:455`, `:210-215` |
| 3 | Read the node state (section 4.1 / 4.2): `Reachable`, `Rendering`, `Stopped`, `Source` (`registry`, `http`, `none`). | `:456`, `:219-253` |
| 4 | Reachability edge: unreachable -> reachable stamps `reachableSince` and logs the start-up settle; the reverse logs "holding steady". | `:459-465` |
| 5 | Keepalive, if `app_keepalive` (section 3.5). | `:474-495` |
| 6 | DCC scan, if `dcc_gate` and `dcc_poll_seconds` (5) have passed (section 3.4). | `:498-511` |
| 7 | State machine (section 3.3). | `:514-597` |
| 8 | Write `Companion\LastSeen` (every pass), `Rndr`, `State`, `App`. | `:599-604` |
| 9 | Wait on the event: `watchdog_seconds` (5 s) when the Node key is open and the last source was not `http`; otherwise `fallback_poll_seconds` (1 s). A registry change wakes it at once; the notification is re-armed first because the app writes four values per transition. A failed re-arm closes the key; it is reopened on a later pass. | `:609-622` |

So the cadence is: event-driven plus a 5 s tick on an unpackaged install; a 1 s HTTP poll on a Store install (the key
is never visible there, so `$key` stays null and every pass also retries opening it).

### 3.3 The state machine

Outer guards, in this order (`:514-529`):

| Condition | Effect |
|---|---|
| `rndr_enabled = false` | Nothing. The client is never touched (keepalive-only install). |
| `Enabled = 0` | If not already Inactive: kill the client ("disabled by operator"), go Inactive. |
| minRender unreachable | Hold steady: no launch, no watchdog, a running client is left alone. Exception: if a creative app is present **now**, kill the client and go Inactive. |
| otherwise | The table below. |

| State | Condition, first match wins | Action | Next |
|---|---|---|---|
| Inactive | rendering | none (the client is **not** killed here, see section 8) | RenderActive |
| | creative app present | kill a running client ("unmanaged client") | Inactive |
| | less than `startup_delay_seconds` (120) since minRender became reachable | wait | Inactive |
| | gate blocked (present or within the settle) | wait | Inactive |
| | a client is already running | adopt | Running |
| | launch succeeds | - | Running |
| Running | rendering | kill ("render started") | RenderActive |
| | creative app present | kill | Inactive |
| | every `watchdog_seconds` (5), after `warmup_seconds` (15) since launch: no client process | relaunch | Running |
| | ... client has a window that is not responding | first sight starts a grace; after `hang_grace_seconds` (15) kill, relaunch | Running |
| RenderActive | not rendering | stamp the cooldown | Cooldown |
| Cooldown | rendering | - | RenderActive |
| | gate blocked | - | Inactive |
| | `cooldown_seconds` (120) elapsed | adopt a running client, else launch | Running |

"Unmanaged client" means: `TCPSVCS` processes exist while the companion's state is Inactive - a client the companion
did not launch in this run (left over from a previous companion run, which exits "client left as is", or started by
hand). Since `96099a6` the gate kills it; otherwise the Inactive state adopts it once the settles have passed.

### 3.4 The DCC gate (`:306-321`, `:498-511`)

- Off by default. A process counts when its image name plus `.exe`, lower-cased, is in `block_processes` (default
  `blender.exe`, `afterfx.exe`; exact match, never a prefix) **and** its full path starts with `%ProgramFiles%\` or
  `%ProgramFiles(x86)%\`. A process whose path cannot be read (another user's) does not count.
- Reason for the path rule (`247054e`): the third-party client is a per-user install and cannot write to Program
  Files, so its own child processes can never be mistaken for an artist's session.
- Sampled every `dcc_poll_seconds` (5). Each sighting stamps `lastOccupied`. "Present" = seen on the last scan (kill
  trigger). "Blocked" = within `dcc_settle_seconds` (600) of the last sighting (launch gate).
- While minRender is unreachable the gate still kills (section 3.3), since `f42906b`.

### 3.5 The keepalive (`:324-373`, `:474-495`)

- Off by default. "Running" = at least one process named `minRender` in the companion's own session (`:334-338`).
  Deliberately by process, not by reachability: "a second launch is not harmless (the app's single-instance guard asks
  the running copy to show its window)".
- Launch when all hold: `Companion\KeepAlive` is not 0; at least `app_startup_grace_seconds` (90) since the companion
  started (so the package's own start-up task wins at logon); the app has been missing `app_relaunch_delay_seconds`
  (30) (covers the package being serviced mid-update); the back-off time has passed.
- Back-off: after each attempt the next is allowed in `appBackoff` seconds, then `appBackoff` doubles up to
  `app_backoff_max_seconds` (600); it resets to 30 only after the app has stayed up 300 s (`:482`, `:490-492`).
- How (section 4.6): `app_path` if set (a loose build), otherwise the Store package through the activation manager.
- `Enabled` does not pause the keepalive; `KeepAlive = 0` does.

### 3.6 Processes started and killed

| Process | Started how | Killed how |
|---|---|---|
| RNDR launcher (`rndr_path`, default `<data>\rn\rndrclient.exe`) | `Start-Process`, working directory = its folder, hidden window; not started if a `TCPSVCS` process already exists or the file is missing (`:271-287`). The launcher spawns the workers and exits. | never (it exits by itself) |
| RNDR workers (`process_name`, default `TCPSVCS`) | by the launcher | `Stop-Process -Id <pid> -Force` for **every** process of that name the user can see, errors ignored; then poll every 250 ms for up to 10 s until none is left; log how many were stopped and how many remain (`:289-301`). This is a hard kill (TerminateProcess), by pid, with no tree kill. The in-app ancestor used `taskkill /F /IM TCPSVCS.EXE`, fire and forget. |
| minRender | section 4.6 | never |

"Kill waits for exit" is `96099a6`: a relaunch straight after a kill must not adopt processes that are still
unwinding. The wait blocks the whole loop for up to 10 s.

### 3.7 Configuration and timeouts

`<data>\rndr-companion.json`, read once at start; unknown keys are logged and ignored; a parse error means all
defaults (`:159-199`).

| Key | Default | Meaning |
|---|---|---|
| `rndr_path` | `<data>\rn\rndrclient.exe` | launcher; a path under the old data folder is remapped when the old file is gone and the new exists |
| `process_name` | `TCPSVCS` | worker process name |
| `startup_delay_seconds` | 120 | after minRender becomes reachable, "let dispatch claim the GPU first" |
| `warmup_seconds` / `watchdog_seconds` / `hang_grace_seconds` | 15 / 5 / 15 | client health |
| `cooldown_seconds` | 120 | after a render |
| `dcc_gate` / `dcc_poll_seconds` / `dcc_settle_seconds` / `block_processes` | false / 5 / 600 / `blender.exe`, `AfterFX.exe` | section 3.4 |
| `http_port` | 0 = read from minRender's `config.json`, else 8420 | fallback polling |
| `fallback_poll_seconds` | 1 | fallback polling interval |
| `rndr_enabled` | true | false = keepalive only |
| `app_keepalive` | false | section 3.5 |
| `app_package` | the Store package's Identity Name | |
| `app_path` | empty | loose build; overrides the package |
| `app_args` | `--minimized` | |
| `app_startup_grace_seconds` / `app_relaunch_delay_seconds` / `app_backoff_max_seconds` | 90 / 30 / 600 | section 3.5 |

Other fixed numbers: HTTP timeout 1 s (`:243`); kill wait 10 s (`:297`); keepalive "stayed up" 300 s (`:482`); log
cap 2 MB (`:152`).

### 3.8 Logging

`<data>\rndr-companion.log`, UTF-8, appended, one line per event: `yyyy-MM-dd HH:mm:ss <message>` in local time, no
level, no source (`:143-148`). Message families: `starting:`, `state: A -> B (why)`, `launch:`, `kill:`, `watchdog:`,
`dcc:`, `keepalive:`, `config:`, `Enabled ->`, `KeepAlive ->`, `minRender reachable via ...`, `minRender unreachable`,
`exiting`. Rotation: none while running; the file is **deleted** at the next start if it is over 2 MB. Write failures
are swallowed. The log is also echoed to the host with `-Once`.

Observability outside the log: `HKCU\Software\MinRender\Companion` values `State` (string), `Rndr` (`running` /
`absent`), `App` (`running` / `absent`, keepalive only), `LastSeen` (QWORD, ms since epoch, every pass).

## 4. The contract with minRender

Everything the companion depends on that minRender owns. This is the part a port must not break.

### 4.1 Registry (unpackaged installs only)

Key `HKCU\Software\MinRender\Node`, written by `publishNodeState` (`src/core/platform.cpp:348-382`), read at
`:219-240`, watched with `RegNotifyChangeKeyValue` (last-set filter, no subtree, `:385-390`).

| Value | Type | Written | Read by the companion |
|---|---|---|---|
| `RenderState` | `REG_SZ` | `idle` or `rendering` | rendering iff the string equals `rendering` (PowerShell `-eq`: case-insensitive); anything else, or missing, = idle |
| `NodeState` | `REG_SZ` | `active` or `stopped` | parsed into `Stopped`, which **no decision uses** |
| `Pid` | `REG_DWORD` | the app's process id; 0 on clean exit | must exist as a value for the registry path to be used at all; non-zero and `Get-Process -Id` finds **any** process with that id = alive and reachable; 0 or dead = go to the HTTP fallback |
| `UpdatedMs` | `REG_QWORD` | ms since epoch | never read |

When the app writes (`src/monitor/monitor_app.cpp:148-164`, `211-217`): on the first update tick after launch (always,
to overwrite what a crashed instance left), then only when rendering or stopped changes, then once at shutdown with
`idle` and `Pid = 0`. The tick is the 50 ms pump (`src/monitor/main_qt.cpp:353-369`). "Rendering" is
`RenderCoordinator::isRendering()` = an active render exists (`src/monitor/render_coordinator.h:71`), which becomes
true in the same call that hands the chunk to the agent (`render_coordinator.cpp:157-161`) and false when the chunk
completes, fails or is abandoned. The app never reads the key (`src/core/platform.h:59-66`). Missing-field handling: a
key without `Pid` is treated as "no key" and the HTTP fallback is used.

**Store installs**: the package's registry writes are virtualized (`installer/msix/AppxManifest.xml.in:27-29`,
`installer/STORE.md:87-89`, `:159`), so the key is invisible outside the package. The companion then runs on HTTP.
Windows ships only through the Store today (`SECURITY.md:169`), so on the live farm **the HTTP poll is the path in
use**; the registry path serves 0.5.6-era unpackaged installs and development builds.

### 4.2 HTTP

Exactly one route. The answer to the open question in `docs/phase4/step0-contracts.md` (question 8, "do scripts or the
companion call anything beyond `/api/status`"): no.

| Item | Value |
|---|---|
| Request | `GET http://127.0.0.1:<port>/api/status`, no body, no headers of its own (`Invoke-RestMethod`, `:242-243`) |
| Auth | none. The route is the single exemption from the Bearer check (`src/core/http_server.cpp:1017-1018`). The companion never reads `farm.json` and holds no secret. |
| Port | `http_port` in `rndr-companion.json` if > 0; else `http_port` in `<data>\config.json` if present and non-zero; else 8420 (`:173-182`). Read once at companion start. |
| Timeout | 1 s total |
| Fields read | `render_state` (rendering iff equal to `rendering`), `node_state` (parsed, unused). Nothing else. |
| Missing fields | a body that parses but lacks `render_state` reads as idle and reachable |
| Errors | any failure - refused, timeout, non-2xx (the handler's 503 when the app object is absent, `http_server.cpp:72-76`), unparseable body - means unreachable: hold steady |
| When it is called | only when the registry path did not prove the app alive: every pass on a Store install (1 s interval), every pass while the app is down |

The route is served by `HttpServer::setupRoutes` (`http_server.cpp:70-81`) from `MonitorApp::buildLocalPeerInfo`
(`src/monitor/monitor_app.cpp:2376-2420`; serialised in `src/core/peer_info.h:107-137`). The server binds `0.0.0.0`
and is started inside the farm start (`monitor_app.cpp:334`), so **with no farm running (no sync root, or the farm
failed to start) a Store-installed minRender is "unreachable"** to the companion even though it runs.

### 4.3 Files in the data folder

Data folder: `%USERPROFILE%\minRender` (`src/core/platform.cpp:89-121`); before 0.5.8 `%LOCALAPPDATA%\MinRender`. The
first 0.5.8 launch copies the old folder across (everything except `staging\`), which is how the companion's own files
arrived in the new folder.

| Path | Owner | Read / written by the companion | Format |
|---|---|---|---|
| `config.json` | minRender | Read. (a) Its **existence** in the new folder selects the new folder (`:54-59`). (b) `http_port` (number). (c) Installer only, once: `rndr_dcc_gate` (bool) and `rndr_block_processes` (string array), keys removed from the app in 0.5.6 and dropped from the file at the app's next save (`src/core/config.h:62-65`). | JSON |
| `rndr-companion.json` | companion | Read at start; written by the installer | JSON, ASCII, section 3.7 |
| `rndr-companion.log` | companion | Appended | text, section 3.8 |
| `companion\rndr-companion.ps1` | companion | The scheduled task runs this copy by absolute path | - |
| `rn\rndrclient.exe` (and the client's own files) | the third-party client | Launched | - |

The companion reads or writes nothing else there, and nothing on the farm share. `docs/phase4/step0-contracts.md`
section 6 lists the data folder's files but not these four, and not that `config.json`'s existence is a signal.

### 4.4 Registry values the companion owns

`HKCU\Software\MinRender\Companion`: `Enabled` (DWORD, operator), `KeepAlive` (DWORD, operator), `DccGateMigrated`
(DWORD, installer), `State`, `Rndr`, `App` (strings), `LastSeen` (QWORD). minRender neither reads nor writes this key.
It shares the parent `HKCU\Software\MinRender` with the app's key; a port (or its uninstaller) must not delete the
parent.

### 4.5 Process and executable names

| Name | Where | Use |
|---|---|---|
| `minRender` | `:336` | keepalive only: "is the app running" = any process of this name in the companion's session. Case-insensitive, exact name. `minrender-headless` does not match. |
| process id in `Pid` | `:224-228` | liveness; the name of the process holding that id is not checked |
| `TCPSVCS` (configurable) | `:256` | the third-party client's workers |
| `rndrclient.exe` (configurable path) | `:70`, `:280` | the third-party launcher |
| `blender.exe`, `AfterFX.exe` (configurable) | `:80` | the gate |
| `powershell.exe` with `rndr-companion.ps1` in its command line | `uninstall.ps1:30-32` | stray companions |
| `conhost.exe` | `install.ps1:137` | hidden host |

`mr-agent.exe`, `mr-restart.exe` and `minrender-headless.exe` appear nowhere in the companion. A port with **no
`mr-agent.exe` process does not confuse it**.

### 4.6 Install path, package identity, command line (keepalive only)

| Mode | What the companion does | What it assumes |
|---|---|---|
| Packaged (default) | `Get-AppxPackage -Name <app_package>`; requires `Status = Ok`; builds the application user model id `<PackageFamilyName>!minRender`; calls `IApplicationActivationManager::ActivateApplication(aumid, "--minimized", AO_NOERRORUI)` (`:110-134`, `:356-372`) | the package's Identity Name is unchanged; the manifest's `Application Id` is `minRender` (`installer/msix/AppxManifest.xml.in`, `<Application Id="minRender" Executable="minRender.exe">`); the application accepts arguments through activation |
| Unpackaged | `Start-Process <app_path> --minimized`, working directory = the executable's folder (`:343-355`) | `app_path` is an absolute path the operator typed; nothing is discovered |

The companion never starts the packaged executable by path ("it would run without package identity", `:110-112`) and
has no knowledge of the install directory. The only argument it ever passes is `app_args` (default `--minimized`).
The app honours `--minimized` by starting with the window hidden (`src/monitor/main_qt.cpp:264-270`); the same
argument is used by the package's start-up task through `mr-restart.exe` (`src/restart/main.cpp:41-67`) and by
`RegisterApplicationRestart` (`src/ui/platform/app_restart.cpp:41-55`).

A second launch while one runs is caught by the single-instance guard (`src/monitor/main_qt.cpp:96-101`,
`src/core/single_instance.cpp`), which makes the first instance **show its window** and exits 0 - the reason the
keepalive tests the process table first.

## 5. The app's side today

Everything in `src/` that exists for the companion. There is no UI, no setting and no route of its own.

| Piece | Where | Default / behaviour |
|---|---|---|
| `publishNodeState(renderState, nodeState, alive)` | `src/core/platform.h:59-66`, `src/core/platform.cpp:348-382` | Windows only; no-op elsewhere. Creates the key if needed; errors ignored. |
| The call on change | `src/monitor/monitor_app.cpp:148-164`; members `m_nodeStatePublished`, `m_lastPublishedRendering`, `m_lastPublishedStopped` (`monitor_app.h:324-328`) | first tick always, then on change only |
| The clean-exit marker | `monitor_app.cpp:211-217` (`shutdown()`) | `idle`, `Pid = 0` |
| `GET /api/status` unauthenticated | `src/core/http_server.cpp:70-81`, exemption `:1017-1018` | exists for peer discovery; the companion is a second consumer (`SECURITY.md:144-146`) |
| `http_port` | `src/core/config.h:41` | 8420 |
| `--minimized` | `src/monitor/main_qt.cpp:264-270` | window hidden at start |
| Restart after servicing | `src/ui/platform/app_restart.h:8-27`, `.cpp:41-55`; used at `main_qt.cpp:320-325` | packaged only; overlaps the keepalive |
| Tolerating the old keys | `src/core/config.h:62-65` | `rndr_dual_mode`, `rndr_dcc_gate`, `rndr_block_processes` are ignored on read and dropped on save |
| Data-folder migration that carries the companion's files | `src/core/platform.cpp:105-118` | copy, skip existing, never move |

Removed in 0.5.6 and not to be rebuilt in the app: the supervisor, `/api/config`'s `rndr_dual_mode`, the bridge
properties `rndrDualMode` / `rndrAvailable` / `rndrStatus`, the Settings checkbox and status line (`57d1555`).

Documentation that states the contract: `SECURITY.md:139-155` ("Local integration"), `installer/STORE.md:87-89`
(the Store verification checklist expects the key to be invisible and the companion on HTTP), `docs/install.md:73-74`.
`mr-agent/` contains nothing for the companion.

## 6. What a single-executable port changes

Premise: (a) one executable, the agent folded in and relaunched hidden per chunk; (b) same HTTP routes and data
folder; (c) nothing else changes.

### 6.1 Assumptions that would break

| # | Companion assumption | How a single executable breaks it | Consequence |
|---|---|---|---|
| 1 | Every process named `minRender` in the session is "the app is running" (`:334-338`). | The per-chunk child has the same name. If it outlives the main process (crash of the parent with the child not on a kill-on-close leash, or a slow teardown), the keepalive sees "running" and never relaunches. | A node stays off the farm until the child exits. Only with `app_keepalive` on. |
| 2 | `Pid` in the registry is the long-lived app (`:224-228`). | If the child runs the same start-up code it would overwrite `Pid` with its own, and write `idle` / `Pid = 0` when it exits mid-render or at chunk end. | Unpackaged installs: the companion reads "idle" during a render and launches the client onto a busy GPU, or flips to the HTTP path at each chunk end. |
| 3 | One writer of `RenderState`. | Same cause as 2. | Same. |
| 4 | A launch of `minRender.exe` while one runs is the user asking for the window (`single_instance.cpp`). | The child is a second launch. If it goes through the guard it exits at once and pops the window; if the guard is bypassed carelessly, a keepalive launch could start a second full app. | Internal to the port, but the keepalive's "test by process" rule was written around this guard. |
| 5 | `/api/status` answers within 1 s on loopback. | Not a single-executable issue as such, but a child that ever binds the port, or a main process that blocks the listener while relaunching itself, turns "rendering" into "unreachable" = hold steady = the client is **not** killed. | The client keeps the GPU during a render. |
| 6 | `render_state` says `rendering` from before the renderer starts until the chunk is over. | If the port reports rendering only once the child has connected or the renderer has produced output, the kill comes late. | GPU contention at chunk start. |

Not broken: the absence of `mr-agent.exe` (never looked for); the gate (it matches only the configured names; the
port's hidden child is `minRender.exe` and is not on the list); the launcher and worker handling (the port is not
involved).

### 6.2 What the port must keep so an installed companion works without reinstalling

| # | Keep | Why |
|---|---|---|
| K1 | `GET /api/status` on `127.0.0.1:<http_port>`, no auth, JSON with string `render_state` = `rendering` / `idle` and string `node_state`, answered well inside 1 s, by the long-lived process only. | The only path on Store installs. |
| K2 | `render_state` = `rendering` no later than the moment the hidden child is started for a chunk, and until the chunk is finished or abandoned. | Kill timing; there is no handshake (section 8). |
| K3 | Data folder `%USERPROFILE%\minRender`, with `config.json` present and carrying `http_port` as a number. | Folder selection and the port number; the scheduled task's absolute path to the script. |
| K4 | Never delete, rewrite or "clean up" `rndr-companion.json`, `rndr-companion.log`, `companion\`, `rn\` in the data folder; never delete `HKCU\Software\MinRender` or its `Companion` subkey. | The companion's own state. |
| K5 | Main executable named `minRender.exe`. | Keepalive process test. |
| K6 | Either: the per-chunk child cannot outlive the main process (kill-on-close leash), or it carries a different image name. | Assumption 1. |
| K7 | Package Identity Name unchanged; `Application Id="minRender"`; `--minimized` accepted through activation and from the command line, meaning "start hidden"; unknown arguments ignored rather than fatal. | Keepalive launch. |
| K8 | A second launch of the app while one runs exits quietly with code 0 (and may show the window); the per-chunk child bypasses that guard by an explicit internal argument that no outside launcher passes. | Keepalive back-off counts a launch that dies as a failure; a second full instance must never start. |
| K9 | If the port can run unpackaged on Windows: write `HKCU\Software\MinRender\Node` with the same four values and types, from the main process only, first tick always, on change, `Pid = 0` at clean exit. If the port never writes the key, a stale key left by a 0.5.x unpackaged build must not be left with a non-zero `Pid` (delete it or write `Pid = 0` once). | Assumptions 2-3 and the stale-pid hazard in section 8. |
| K10 | The HTTP listener up whenever the app runs a farm, on the configured port, bound so that loopback reaches it. | Reachability. |
| K11 | Do not restore `rndr_dcc_gate` / `rndr_block_processes` to `config.json`, and do not strip them from an old folder's copy. | The installer's one-time carry-over reads them. |

`docs/phase4/step0-contracts.md` verification row 49 ("run the shipped companion script against the port") should be
widened from the registry key to K1-K10, on both a packaged and an unpackaged install, with the keepalive on.

## 7. macOS

Windows only. The script is PowerShell with Win32 calls; `publishNodeState` is a no-op off Windows
(`src/core/platform.cpp:377-381`); the in-app ancestor was wholly inside `#ifdef _WIN32` (`81ef8c2`). There is no
launch agent, login item or equivalent watchdog for macOS anywhere in the tree. `GET /api/status` is the documented
"cross-platform view of the node" (`SECURITY.md:144-146`) should one ever be wanted. The comment "macOS Swift
companion via IPC" at `src/monitor/monitor_app.h:224-226` refers to a menu-bar tray helper added in `5af849e` and
since removed - a different thing, and a stale comment.

## 8. Fragile or inconsistent today (for the owner to rule on)

1. **No handshake at render start.** minRender starts the chunk and publishes "rendering" in the same tick; it never
   waits for the client to be gone. On the HTTP path the companion notices up to about 1 s later, then the kill can
   take up to 10 s. The renderer and the client overlap on the GPU for that time.
2. **Inactive + rendering does not kill** (`:533-534`). A client the companion did not launch (after a task restart
   the previous run exits "client left as is") stays up through a render that starts before it is adopted, for example
   inside the 120 s start-up settle. `96099a6` fixed this for the gate only.
3. **"The start-up settle applies exactly as on a fresh start" is true only in Inactive** (`:27-31` versus `:553-579`,
   `:586-594`). If minRender goes away while the state is Running or Cooldown and comes back, the watchdog or the
   cooldown relaunches the client at once.
4. **Stale `Pid`, reused.** Liveness is "some process has this id". After a crash of an unpackaged build the key keeps
   a non-zero `Pid`; if Windows reuses the id, the companion trusts the stale `RenderState` indefinitely and never
   tries HTTP. Also a hazard on a machine that moved from an unpackaged 0.5.6 to the Store build after a crash.
5. **The farm's own renders trip the gate.** With `dcc_gate` on, a farm render that runs `blender.exe` from Program
   Files stamps `lastOccupied`, so after each render the relaunch waits the 600 s settle, not the 120 s cooldown.
   Whether After Effects renders (`aerender.exe`) also bring up `AfterFX.exe` was not verified here.
6. **Store install with no farm running = unreachable** (section 4.2). The companion holds steady forever; nothing in
   its log says why.
7. **`NodeState` / `node_state` is read and never used.** A node the operator stopped still runs the client when idle
   (probably intended), but the field is dead weight in the contract. `UpdatedMs` is never read either.
8. **`-Once` is not a dry run** (`README.md:140-141`). It takes the mutex (so it exits immediately while the task
   runs) and, with the gate on and a creative app open, it kills the client.
9. **The log is deleted, not rotated**, and only at start; a companion that runs for months grows it without bound,
   and the history is lost at the 2 MB boundary.
10. **Re-running the installer may not replace the running instance.** The task is registered "ignore new" and the
    script holds a mutex; whether `Register-ScheduledTask -Force` stops the old process was not verified. If not, an
    "update" keeps running the old script until the next logon.
11. **Uninstall deletes `DccGateMigrated`** with the rest of the key, so a reinstall repeats the carry-over and can
    turn the gate back on from an old `config.json` after the operator turned it off.
12. **Two restarters.** The keepalive and `RegisterApplicationRestart` both relaunch after a Store update. The 30 s
    "missing" delay normally lets Windows win; if both fire, the loser hits the single-instance guard and the window
    pops up on an unattended node.
13. **Kill by name, for every match.** Any process called `TCPSVCS` that the user may terminate is killed, wherever
    its image lives. There is no path check on the kill side, only on the gate side.
14. **`uninstall.ps1` removes the new folder's `companion\` twice** (`uninstall.ps1:34-39`), and its closing message names only the
    new folder. Cosmetic.
15. **The companion has no version.** Nothing on a node says which revision of the script is installed; the incidents
    of 2026-10-23, -24 and -28 each needed a reinstall on every node.

## 9. Open questions for the owner

1. Is the companion in scope for Phase 4 at all (kept as the PowerShell script, untouched), or is a Rust replacement
   wanted later? This document assumes "kept, untouched, must go on working".
2. How many nodes run it, with which options (`dcc_gate`, `app_keepalive`, `-NoRndr`), and are any still on an
   unpackaged build (registry path) or all on the Store build (HTTP path)?
3. Will the port ship on Windows only through the Store, as today? If yes, K9 reduces to "handle a stale key"; if an
   unpackaged build is planned, the registry writer must be ported.
4. Does the port's per-chunk child keep the image name `minRender.exe`? If so, K6 depends entirely on the leash
   (spike S13); rule whether a distinct child name is preferred for the keepalive's sake.
5. Should the port add a handshake (hold the renderer until the client is gone, or a short fixed delay) to close item
   1, or is the overlap accepted as it has been since 0.5.6?
6. Items 2, 3, 4 and 8 are companion bugs found on the way: fix in the script (and reinstall on the nodes), or leave?
7. Should `/api/status` be answered when no farm is running (item 6), so the companion can tell "app up, no farm" from
   "app gone"?
8. Once the port has its own restart-after-update, is the keepalive still wanted, or should it be retired to avoid
   two restarters (item 12)?
9. May the port add fields for the companion (for example a boolean "about to render") or must the surface stay
   exactly as in section 4?
