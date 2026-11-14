# Phase 4, step 1 - minRender: the tray icon and the application's life, per OS (2026-11-14)

Read from the MinRender repository, branch `min-paths` @ 9526557 (release 0.5.11 plus the path-library change).
Read-only: nothing was built, run or edited. Paths are relative to the MinRender repository root; `file:N` is a line
number at that commit.

Where the answer depends on Qt rather than on minRender's code, Qt's own source was opened (6.11.1, the version both
release scripts pin: `scripts/macos-release.sh:33`, `scripts/package.bat:22`) and the file is named. Such claims are
marked **(Qt, by reading)**. Nothing here was confirmed by a walk; section 9 lists what a walk should settle.

Corrections to the step-0 notes are collected in section 9.3.

## Summary

- There are two tray implementations. The shipped application uses Qt's (`src/ui/platform/tray.cpp`) on both systems.
  A hand-written Win32 tray (`src/core/system_tray.cpp`) is compiled only into `minrender-headless`, which nobody can
  launch in the Store build and which is not shipped on macOS.
- One icon per OS, no states, no notifications. The only live status is the tooltip. A status line and five status
  colours are computed every 50 ms and shown nowhere.
- Close hides the window. There are exactly two exit rules: **the tray's Exit waits** for a running render (silently,
  with no way to cancel the wait); **everything else kills it at once** (the menu's Exit, Cmd+Q, the Dock's Quit,
  log-out, shutdown, a Store update, a Sparkle update, a remote restart).
- The waiting rule is a remnant: before the Qt port the same call opened a dialog, "Kill && Exit" / "Cancel". The
  dialog was not ported; the wait it was built around stayed.
- Start at login exists on Windows only (the Store package's startup task, through `mr-restart.exe`). Restart after
  an update exists on Windows only. Remote restart exists on Windows only. macOS has none of the three.
- No code anywhere handles sleep, wake, or power; nothing keeps a rendering machine awake.

---

## 1. The tray / menu-bar icon

### 1.1 Which code runs where

| Implementation | File | Compiled into | Windows | macOS |
|---|---|---|---|---|
| Qt tray (`MR::Tray`, a `QSystemTrayIcon`) | `src/ui/platform/tray.cpp`, `tray.h` | `minRender` (the application) | **the shipped tray** | **the shipped menu-bar item** |
| Win32 tray (`MR::SystemTray`, `Shell_NotifyIconW`) | `src/core/system_tray.cpp`, `system_tray.h` | `minrender-headless` only (`CMakeLists.txt:500-504`) | dev builds only | a stub: every method is empty (`system_tray.cpp:213-225`) |

Both expose the same three callbacks (`onShowWindow`, `onStopResume`, `onExit`) and three setters (`setTooltip`,
`setStatusText`, `setNodeActive`). They are wired in `src/monitor/main_qt.cpp:302-315` and
`src/monitor/main_headless.cpp:62-79`.

`QApplication` (QtWidgets) is used instead of `QGuiApplication` only because `QSystemTrayIcon` needs it
(`main_qt.cpp:85-86`). This choice also decides how quitting works (section 2.4).

### 1.2 The shipped tray (Qt), Windows

| Aspect | Today | Reference |
|---|---|---|
| Icon | `:/icons/minrender.ico`, embedded in the executable (7 sizes up to 256 px, colour). One icon, never changed at run time. | `tray.cpp:28-31`; embedded by `CMakeLists.txt:295-301` |
| Initial tooltip | `minRender` | `tray.cpp:67` |
| Tooltip, live | `minRender - Rendering <job id>` / `minRender - Stopped` / `minRender - No Farm` / `minRender - Idle`, tested in that order. Pushed every 50 ms; Qt's Windows back end drops an unchanged string **(Qt, by reading:** `qwindowssystemtrayicon.cpp` `updateToolTip`**)**. | `monitor_app.cpp:2241-2253`; `main_qt.cpp:362` |
| Left click | Show, raise and activate the window (`Trigger`) | `tray.cpp:71-78`, `main_qt.cpp:288-298` |
| Double click | The same (`DoubleClick`; the first click of the pair has already shown the window) | the same |
| Right click | The menu below | `tray.cpp:66` |
| Middle click | Nothing | not handled |
| Notifications / balloons | **None.** No `showMessage`, no toast, anywhere in `src/` or `mr-agent/` | grep |
| Explorer restarts (the taskbar is recreated) | Handled by Qt, not by minRender's code | - |

### 1.3 The shipped tray (Qt), macOS

| Aspect | Today | Reference |
|---|---|---|
| Icon | `:/icons/minRenderMacOStray.png` (512 x 512, black on transparent), marked as a mask so AppKit treats it as a template image and tints it for a light or dark menu bar and for the pressed state. One image, never changed. | `tray.cpp:18-26` |
| Tooltip | As on Windows (shown when the pointer rests on the item) | as above |
| Any click (left or right) | **The menu opens.** Because a menu is attached, AppKit opens it on mouse-down; Qt reports the click when the menu starts tracking. **(Qt, by reading:** `qcocoasystemtrayicon.mm` `updateMenu`, lines 181-190, and `statusItemMenuBeganTracking`**)** | - |
| Left click, in addition | Qt reports `Trigger`, so minRender's handler also **shows and activates the window while the menu is open**. A right click reports `Context`, which is not handled: the menu only. | `tray.cpp:71-78` |
| Double click | Reported as `DoubleClick`: shows the window (the menu is already open from the first click) | the same |
| Notifications | None | - |

The comment at `tray.cpp:69-70` says single and double click both restore the window "matching the Win32 tray's
behavior". The Win32 tray restores on a double click only (1.5).

### 1.4 The menu (Qt tray, both systems)

Built once in `Tray::init()` (`tray.cpp:52-56`). Every string goes through `QObject::tr`, but no translation is loaded
(English only).

| # | Item (verbatim) | Enabled | Checked | Does |
|---|---|---|---|---|
| 1 | `Show Window` | always | never | `showWindow`: `setVisible(true)`, `raise()`, `requestActivate()` on the root window (`main_qt.cpp:288-298`) |
| 2 | `Stop` when this node is Active, `Resume` when it is Stopped (`tray.cpp:126-134`; relabelled when the node state changes) | always | never | Toggles the node between Active and Stopped (`main_qt.cpp:307-312`). Stopping **aborts the current render at once** (`MonitorApp::setNodeState`, `monitor_app.cpp:2165-2183`) and writes `node_stopped` to the configuration file. No confirmation. |
| - | separator | | | |
| 3 | `Exit` | always | never | `MonitorApp::requestExit()` (section 2.4): quits when idle, waits silently when rendering |

No title line, no status line, no version, no "Check for Updates", no Settings. Nothing is ever disabled or checked.

`setStatusText()` receives "Rendering <chunk label>" / "Stopped" / "No Farm" / "Idle" every 50 ms
(`monitor_app.cpp:2255-2264`, `main_qt.cpp:363`), stores it and calls `updateMenuLabels()`, which only sets the
Stop / Resume label (`tray.cpp:106-112`). The text is displayed nowhere.

### 1.5 The Win32 tray (headless dev builds only)

Recorded because it is the older design the Qt tray was meant to mirror, and it differs.

| Aspect | Today | Reference |
|---|---|---|
| Creation | A message-only window, class `MinRenderTray`, title `MinRender Tray`; `Shell_NotifyIconW(NIM_ADD)` with icon id 1 | `system_tray.cpp:18-53` |
| Icon | `resources\icons\minrender.ico` loaded at 16 x 16 from beside the executable; fallback: the executable's resource 1 | `system_tray.cpp:10`, `101-115` |
| Initial tooltip | `MinRender` (capital M, unlike the Qt tray) | `system_tray.cpp:47` |
| Tooltip, live | The same four strings; `NIM_MODIFY` **every 50 ms with no change check** | `system_tray.cpp:78-89`, `main_headless.cpp:86` |
| Left click | Nothing | `system_tray.cpp:164-177` |
| Double click | `onShowWindow`, which in the headless build only prints `[Headless] Show window requested (no GUI available)` | `main_headless.cpp:65-68` |
| Right click | The menu, rebuilt each time | `system_tray.cpp:117-156` |
| Menu | `MinRender` (disabled) / `Status: <status text>` (disabled; initially `Status: Initializing`) / separator / `Show Window` / separator / `Stop Node` or `Resume Node` / separator / `Exit` | the same |
| Taskbar recreated | Not handled: the icon is lost if Explorer restarts | - |
| Second-launch message | `WM_SHOW_WINDOW` (`WM_APP + 2`) is still handled, but nothing has posted it since the single-instance rewrite (commit ef50f4e) | `system_tray.cpp:180-184` |

Differences from the shipped tray: a title and a **status line in the menu**, "Stop Node" / "Resume Node" instead of
"Stop" / "Resume", double click only.

### 1.6 Icon states

There are none. What exists:

- `MonitorApp::trayIconName()` returns `gray` (no farm, or Stopped), `red` (agent not connected), `green` (rendering),
  `blue` (idle) - `monitor_app.cpp:2224-2239`. Its header comment also lists `yellow`, which no branch returns
  (`monitor_app.h:224-227`). **Nothing calls it.** It was written for a Swift menu-bar companion (`minrender-tray/`,
  added in 5af849e, removed in 4b9bae9).
- With a one-shot agent, "agent not connected" is the normal idle state, so `red` would be the usual colour if this
  function were ever used as written.
- `tray.cpp:120-123`: "Status-coloured tray icon is deferred ... do them at runtime by painting Theme.success / warn /
  error onto a QPixmap copy".
- There is no leader state, no error state and no "needs attention" state on the icon or in the tooltip.

Icon files in `resources/icons/`: `minrender.ico` (Windows tray, window and executable), `minrender.icns` (macOS
bundle / Dock), `minRenderMacOStray.png` + `.svg` (menu bar), `minRenderMacOS.*` and `minRenderWindows.*` (artwork
sources). Only the `.ico` and the tray `.png` are embedded for the tray.

### 1.7 When the tray is unavailable

`Tray::init()` returns false when `QSystemTrayIcon::isSystemTrayAvailable()` is false; `main_qt.cpp:303-304` logs
`System tray unavailable; continuing without tray` and carries on. `tray.h:25-26` says the caller "should degrade to
hide-on-close off". **It does not**: the close button still hides the window. Without a tray the ways back are a
second launch (section 4), the Dock on macOS, and nothing else; the only exit is the menu's Exit before the window is
closed. In practice this is a Linux case; Windows and macOS always have a tray. (On macOS the user can remove or hide
a menu-bar item; the application is not told.)

---

## 2. The window's life

### 2.1 Start

| Fact | Today | Reference |
|---|---|---|
| Arguments understood | `--minimized` (start with the window hidden) and `--native-frame` (keep the OS title bar; a development aid). Nothing else. Both are looked up anywhere in the argument list. | `main_qt.cpp:267-270`, `216-217` |
| Normal start | Window visible, 1440 x 900. Position, size and maximised state are **not** persisted (only splitter sizes and panel visibility are). | `Main.qml:10-11`, `26`, `73-84` |
| `--minimized` | `visible: !startMinimized`: the window is created but not shown. "Minimized" means hidden to the tray, not minimised to the taskbar. | `Main.qml:24-26` |
| Who passes `--minimized` | The Store startup task (through `mr-restart.exe`), the restart helper's relaunch, and Windows' restart after an update (`RegisterApplicationRestart`). Nobody on macOS. | sections 3-5 |
| Order | Single-instance check, then the whole node (`MonitorApp::init()`: configuration, HTTP server, farm start), then the UI, tray, session-end window, restart registration, the 50 ms pump, then Sparkle on a zero-delay timer | `main_qt.cpp:96-110`, `272`, `302-325`, `356-376` |
| Node state at start | `node_stopped` from the configuration file is restored, so a node stopped before a restart comes back Stopped | `monitor_app.cpp:41-43` |
| History | `--minimized` was passed by the installer's startup shortcut and by `mr-restart` from the Qt port (2026-04) on, but was **not parsed** until b3469dc (2026-08-04): for those months a login start always opened the window. | commit b3469dc |

### 2.2 Close, minimise

| Gesture | Windows | macOS |
|---|---|---|
| Close button / Alt+F4 / the red light | `onClosing` refuses the close and hides the window; the process and the node keep running. No message tells the user where the application went. | the same |
| Minimise | Ordinary: to the taskbar. No code touches it. | Ordinary: to the Dock |
| Last window closed | Does not quit: `setQuitOnLastWindowClosed(false)` | the same; Qt's delegate also answers "no" to AppKit's own question |

References: `Main.qml:59-66`, `main_qt.cpp:351`.

The Store listing text states the behaviour ("closing the window hides the app to the tray icon (quit via tray >
Exit)", `installer/STORE.md:136`), as does the manual (`docs/configure.md:32-40`: "To actually quit, use File → Exit or
the tray menu's Exit item").

### 2.3 The Dock and activation on macOS

- `LSUIElement` is `false` (`cmake/MacOSXBundleInfo.plist.in:41-48`): a regular application with a Dock icon, a menu
  bar and the status item, at all times. Hiding the window does not remove the Dock icon. No code changes the
  activation policy.
- The Dock icon is the bundle's `minrender.icns`; `setWindowIcon` is deliberately not called on macOS
  (`main_qt.cpp:113-123`).
- Reopen: AppKit does not reshow a hidden window on a Dock click, so `main_qt.cpp:327-349` listens for
  `applicationStateChanged` and, on every change to Active, shows the window if it is not visible (commit 7d32fa5).
  This fires on a Dock click, on Cmd+Tab to the application, and on **any other activation**, including the first
  activation at launch (section 9.1, item 6).
- The application menu: Qt moves menu items into the application menu by their text **(Qt, by reading:**
  `qquicknativemenuitem.cpp:192` gives every item the text-heuristic role; `qcocoamenuitem.mm` `detectMenuRole`**)**.
  `E&xit` starts with "Exit" and becomes **"Quit minRender", Cmd+Q**; `Settings…` starts with "Setting" and becomes
  the application menu's **"Settings…", Cmd+,**. So on macOS the File menu is left with "Farm Cleanup…" only. (The
  step-0 UI inventory lists both under File.)
- No Window menu is defined; Cmd+W and Cmd+M are not bound by minRender's code.

### 2.4 Quitting: the two rules

The backend has two entry points (`monitor_app.cpp:2268-2284`):

- `requestExit()`: sets `m_exitRequested`; sets `m_shouldExit` only if nothing is rendering. Otherwise every later
  `update()` re-tests and sets `m_shouldExit` once the render has ended (`monitor_app.cpp:194-201`).
- `beginForceExit()`: `abortCurrentRender("shutdown")`, then `m_shouldExit = true`.

The 50 ms pump calls `QCoreApplication::quit()` when `shouldExit()` is true (`main_qt.cpp:366-367`). After
`app.exec()` returns: `tray.shutdown()`, then `monitor.shutdown()` (`main_qt.cpp:378-384`), which stops the HTTP
worker, stops the farm, **aborts the current render** ("shutdown"), stops the agent (Windows: waits 3 s, then
terminates it), publishes the clean-exit marker and saves the configuration (`monitor_app.cpp:205-220`,
`agent_supervisor.cpp:45-80`).

So every path that leaves the event loop kills a render; the only question is whether a path waits first.

| Path | OS | Code path | While rendering | Dialog |
|---|---|---|---|---|
| Tray > `Exit` | both | `tray.onExit` -> `requestExit()` (`main_qt.cpp:313-315`) | **Waits** until the current chunk ends, then quits. Nothing is shown; the tray item stays enabled; the node is not stopped, but one chunk is the unit: the exit happens at the first tick with no active render. `cancelExit()` exists and nothing calls it, so the wait cannot be cancelled except by one of the paths below. | none |
| Menu > `Exit` (the Windows hamburger list) | Windows | `Qt.quit()` (`Main.qml:90`) -> the engine's quit signal -> `QCoreApplication::quit()` | **Kills at once** (below) | none |
| "Quit minRender" / Cmd+Q | macOS | the same action, moved to the application menu by Qt (2.3) | **Kills at once** | none |
| Dock > Quit; log-out; restart; shutdown; Sparkle's "Install and Relaunch" | macOS | AppKit `terminate:` -> Qt's delegate sends the same Quit event **(Qt, by reading:** `qcocoaapplicationdelegate.mm:82-117`**)** | **Kills at once** | none |
| Alt+F4, the close button | both | `onClosing` | Nothing happens to the render: the window hides | none |
| Sign-out, shutdown, a Store update | Windows | section 2.5 | **Kills at once** | none |
| Remote / self restart | Windows | `launchRestartSidecar()`, then `beginForceExit()` after 3 s (section 4.3) | **Kills at once** | none |

Why the menu's Exit is immediate **(Qt, by reading)**: `QCoreApplication::quit()` sends a Quit event
(`qcoreapplication.cpp:2126-2147`). `QGuiApplication` would first try to close every window and give up if one
refused (`qguiapplication.cpp:2114-2126`) - and minRender's window always refuses (`onClosing`). But the application
object is a `QApplication`, whose handler closes **widget** windows only and then calls `QCoreApplication::event`
directly, "so that we don't let QGuiApplication close any windows we skipped" (`qapplication.cpp:1682-1700`). The QML
window is never asked, and the loop exits. In other words: **the menu's Exit works only because the tray forced the
choice of `QApplication`**; under `QGuiApplication` it would hide the window and quit nothing.

The comment at `main_qt.cpp:353-355` ("The only path out of `app.exec()` is `monitor.shouldExit()`") is therefore
wrong, and so is `Main.qml:59-60` ("tray → Exit (which calls `QCoreApplication::quit` via main_qt.cpp)") in what it
implies: the two exits are not the same call.

A detail of the immediate paths: `shutdown()` stops the HTTP worker and the farm **before** it aborts the render
(`monitor_app.cpp:207-211`), so the chunk's "failed" report is produced after the thread that would deliver it has
stopped. By reading, the leader learns of the loss from the node's disappearance, not from a report. (On the
`beginForceExit()` paths the abort comes first, but the quit follows within one tick.)

**History of the waiting rule.** Before the Qt port (commit 72ce7d8^, the ImGui application) both the menu's Exit and
the tray's Exit called `requestExit()`. While a render was running the main loop re-showed a hidden window
(`src/monitor/main.cpp:288-293` at that commit) and the dashboard opened a modal:

> title `Confirm Exit` - text `Node is currently rendering <chunk> of <job>. Kill render and exit?` - buttons
> `Kill && Exit` (-> `beginForceExit()`) and `Cancel` (-> `cancelExit()`)

(`src/monitor/ui/dashboard.cpp:86-114` at that commit.) The Qt port kept `requestExit`, `isExitPending`,
`beginForceExit`, `cancelExit` in the backend and did not port the dialog; the menu's Exit was written as `Qt.quit()`.
The silent wait is what `requestExit()` does when nobody answers the question. The comment at `main_qt.cpp:317-319`
still speaks of "the render-in-progress confirmation".

### 2.5 Windows: sign-out, shutdown, a Store update

`src/ui/platform/app_restart.cpp`, wired at `main_qt.cpp:317-325` (commit 2242a0c, 2026-10-24, shipped in 0.5.10).

- `SessionEndWindow` creates a hidden **top-level** window (class `MinRenderSessionEnd`, `WS_POPUP`,
  `WS_EX_TOOLWINDOW`, never shown; not message-only, because message-only windows do not get the broadcast). It
  answers `WM_QUERYENDSESSION` with TRUE (never vetoes, registers no shutdown-block reason) and on `WM_ENDSESSION`
  with a true `wParam` calls the handler (`app_restart.cpp:19-37`, `65-86`).
- The handler logs `Windows is closing minRender (update or sign-out)` and calls `beginForceExit()`: the render is
  aborted inside the message handler, and the pump quits within 50 ms if the process is still alive.
- Reason given in the header: servicing asks top-level windows only, and "a tray-only minRender may have none (the QML
  window's HWND goes away on close-to-tray)" (`app_restart.h:21-25`).
- Qt has its own handler as well: when any of Qt's own top-level windows receives `WM_ENDSESSION`, it emits
  `aboutToQuit` and calls `quit()` **(Qt, by reading:** `qwindowscontext.cpp:1159-1177`**)**. Either route ends the
  render.
- There is no distinction between a sign-out, a shutdown and an update, and no attempt to finish or hand back the
  chunk first.

### 2.6 Sleep, wake, lock, fast user switching

Nothing. Searched `src/`, `mr-agent/src/`, the build files and scripts for power broadcasts, workspace sleep / wake
notifications, power assertions, `SetThreadExecutionState`, App Nap controls, activity tokens, sudden / automatic
termination keys, and signal handlers: no hit.

Consequences, by reading:

- Nothing prevents a rendering node from sleeping. A node that sleeps mid-chunk simply stops; the farm sees it go
  silent.
- Nothing reacts to wake (no immediate re-announce, no re-check of the share); the ordinary timers resume.
- macOS: no App Nap opt-out. A hidden window plus a 50 ms timer is the profile App Nap throttles. Whether it does so
  to a process with open listening sockets and a child renderer was not tested.
- No handling of `SIGTERM` / `SIGINT`: on macOS `kill <pid>` ends the process without `shutdown()` (no abort message,
  no clean-exit marker; the agent then notices the broken connection by itself).

---

## 3. Start at login

### 3.1 Windows

| Mechanism | State | Detail |
|---|---|---|
| Store package startup task | **Current** | `installer/msix/AppxManifest.xml.in:96-103`: `windows.startupTask`, `Executable="mr-restart.exe"`, `TaskId="minRenderStartup"`, `Enabled="true"`, `DisplayName="minRender"`. A startup task cannot carry arguments, so it names the helper; the helper, started with no arguments, starts `minRender.exe --minimized` from its own folder (`src/restart/main.cpp:40-66`, `70-71`). |
| Startup-folder shortcut | Retired with the installer (commit 8fbbe35, 2026-10-17) | The Inno Setup script had an optional component "Launch at Windows startup (minimized to tray)", on in the Full install type: a shortcut in the user's Startup folder to `minRender.exe --minimized`, removed on uninstall. A machine migrated from that install has no leftover if the old version was uninstalled as the migration steps say (`installer/STORE.md:186-200`). |
| Registry Run key | Never used | No trace in the history |

- **On / off:** only in Windows (Task Manager > Startup, or Settings > Apps > Startup). The application has no
  setting for it, does not read the task's state (no `StartupTask` API call anywhere) and cannot tell the user whether
  it is on. `docs/install.md:31` documents the toggle.
- **Default:** enabled in the manifest. For a packaged desktop application Windows honours that without asking, so a
  fresh install starts at the next sign-in. `installer/STORE.md:90-91` records the check: "after sign-out/in the app is
  in the tray, window hidden".
- **State at start:** window hidden, tray icon present, node running; Active or Stopped as it was left.
- **Needs a session:** this is a per-user task at sign-in. Nothing runs before a user signs in; an unattended node
  needs automatic sign-in configured outside the application.

### 3.2 macOS

**None.** No login item, no LaunchAgent, no `SMAppService`, no shared-file-list call, no helper bundle
(`Contents/Library/LoginItems` is not built), no plist installed by the release script. The user can add minRender
under System Settings > General > Login Items by hand; that starts it **without** `--minimized`, with the window
shown (Login Items' own "hide" option aside).

---

## 4. Single instance and the restart helper

### 4.1 Mechanism (`src/core/single_instance.cpp`)

Identical on both systems (the header comment, `single_instance.h:12-13`, says Windows uses "a named pipe under the
hood"; that describes the activation channel only).

| Part | What | Reference |
|---|---|---|
| Exclusion | A `QLockFile` at `<temp>/MinRenderMonitor.lock`, `tryLock(0)`, stale time 0 | `single_instance.cpp:14-18`, `27-36` |
| Activation channel | A `QLocalServer` named `MinRenderMonitor` (Windows: the pipe `\\.\pipe\MinRenderMonitor`; macOS: a socket file in the temp folder). A leftover endpoint is removed first. | `single_instance.cpp:41-57` |
| Second launch | Constructs the guard, fails the lock, connects (500 ms), writes the 4 bytes `show`, disconnects, returns 0 | `single_instance.cpp:79-93`, `main_qt.cpp:96-101` |
| First instance | On **any connection** runs the activation callback, `showWindow`. The bytes are never read. | `single_instance.cpp:48-56`, `main_qt.cpp:300` |
| Name | `MinRenderMonitor` for both the application and `minrender-headless`: they exclude each other | `main_qt.cpp:96`, `main_headless.cpp:46` |

What a second launch does and does not do:

- It brings the first instance's window up (show, raise, request activation), whether that window was hidden to the
  tray or merely behind others.
- It passes **no arguments**. `--minimized` on a second launch is ignored; worse, the launch still pops the first
  instance's window (section 9.1, item 7).
- The check runs before the node starts, so a second launch costs nothing and touches no farm state.
- Between the first instance's start and `setActivationCallback` (`main_qt.cpp:300`, after the node and the UI are
  up) a connection is accepted and ignored.
- Windows: the activation request comes from the first process, which does not own the foreground at that moment,
  and the second process does not hand the right over. Whether the window comes to the front or only flashes in the
  taskbar was not tested.

**A crashed instance's lock.** The constructor's comment (`single_instance.cpp:31-34`) says a lock left by a crash
must be removed by hand, and the step-0 note repeats it as an outage risk. Qt's code says otherwise **(Qt, by
reading:** `qlockfile.cpp:425-451`**)**: on a failed lock Qt checks whether the lock looks stale, and a lock whose
recorded process is not running (same machine, same executable name), or which was written before the last boot, is
stale **whatever the stale time**; stale time 0 only turns off the age test. So a crash or a power cut should not
block the next start. The remaining hole is a recycled process id that now belongs to another process with the same
executable name. A walk (kill the process, start it again) settles this in a minute.

**Scope.** The lock is per user (the temp folder). The pipe name on Windows is machine-wide, and the node's ports
are machine-wide: two users signed in on one machine cannot both run a node. Under the Store package, new files in
the user's AppData are redirected to the package's private store, so a packaged and an unpackaged instance (a
development build beside the Store build) probably do not see each other's lock file, while the pipe name is shared.
Not tested.

**The headless build.** It runs no Qt event loop (`main_headless.cpp:38-41`, `81-101`) and sets no callback, so a
second launch against a running headless node connects (or times out after 500 ms) and exits with no effect.

### 4.2 `mr-restart.exe`: every mode (`src/restart/main.cpp`, all 129 lines)

A Windows-only GUI-subsystem program (no console window; `CMakeLists.txt:556-563`), version resource description
"MinRender Update Restart Helper". It has exactly two modes, chosen by whether any argument is present.

| Mode | Invocation | What it does | Exit codes | Who uses it | Why it exists |
|---|---|---|---|---|---|
| Startup task | no arguments (`argc == 1`) | Finds its own folder, starts `<folder>\minRender.exe --minimized` with default flags, closes the handles, returns (`main.cpp:40-66`) | 0 started; 1 own path unavailable or the start failed | Windows, at sign-in, through the manifest's startup task | A startup task cannot pass arguments (commit 746b1a0, 2026-08-04; before it the Startup-folder shortcut carried `--minimized` itself) |
| Relaunch | `--pid <PID> --exe <path> [--minimized]` | 1. Refuses unless `--exe` is in the helper's own folder (`sameDirectoryAsSelf`, case-insensitive compare of the two folders, `main.cpp:22-38`). 2. Opens the process `<PID>`, waits up to 15 s for it to exit, **terminates it** on timeout. If the process cannot be opened (already gone) it carries on. 3. Sleeps 2 s ("let file locks release"). 4. Starts `<path>` (+ ` --minimized` if given). The result of that start is not checked. (`main.cpp:73-128`) | 0 (also when the final start failed); 1 `--pid` or `--exe` missing; 2 `--exe` outside its folder | The application itself, `MonitorApp::launchRestartSidecar()` | A process cannot restart itself: something must outlive it. First version in v0.3.9 (2026-03-06), replacing a generated `restart.bat` (v0.3.4), whose leftovers the application still deletes at every start (`monitor_app.cpp:35-39`, `2305-2310`). |

Other facts:

- The folder restriction came with the 2026-10 security pass (commit 57d1555): "refusing anything else keeps it from
  being a general-purpose 'kill this, run that' tool once the binaries are signed". It restricts `--exe`, **not**
  `--pid`: the helper will still wait for, and after 15 s terminate, any process the user may terminate. The file name
  inside the folder is not checked either (any executable beside the helper qualifies).
- `--exe` is widened byte by byte (`main.cpp:91`), so a path with non-ASCII characters is mangled. The Store's install
  path is ASCII; a development build under a non-ASCII folder would fail the folder test (exit 2).
- An argument that is not recognised is ignored. One unrecognised argument alone (so `argc > 1`, no `--pid`) returns 1
  rather than acting as the startup task.
- The name and the version description say "Update Restart Helper": the helper was first used by the application's
  own updater flow on Windows (WinSparkle, removed in 8fbbe35). No update path uses it today.

### 4.3 The relaunch as the application drives it

`MonitorApp::launchRestartSidecar()` (`monitor_app.cpp:2288-2348`):

1. Windows only. On macOS it logs `Restart sidecar not supported on this platform` and returns false.
2. Requires `mr-restart.exe` beside the executable; otherwise logs and returns false.
3. Starts `mr-restart.exe --pid <own pid> --exe "<own path>" --minimized` with
   `CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP`.
4. Sets `m_restartPending`; 3 s later `update()` calls `beginForceExit()` (`monitor_app.cpp:182-192`): the render is
   aborted and the application exits.
5. The helper sees the exit (or kills the application after 15 s), sleeps 2 s and starts it again, hidden.

So a restart costs the current chunk and about 5 s, and the node always comes back **hidden**, whatever state the
window was in.

Triggers:

| Trigger | From | Reference |
|---|---|---|
| `POST /api/node/restart` (farm secret required) | another node's "Restart App" in the peer menu | `http_server.cpp:117-129`; `NodePanel.qml:399-401` |
| A file named `restart` in this node's folder on the farm share, not older than 60 s (older ones are deleted and ignored) | another node's "Write restart signal (filesystem)" for a peer that looks dead | `peer_manager.cpp:659-686`, `monitor_app.cpp:175-180`; `NodePanel.qml:435-437` |
| `AppBridge::requestRestart()` | **nothing**: no QML calls it | `app_bridge.cpp:2293-2297` |

There is no confirmation on the receiving node, and no check whether it is rendering.

### 4.4 Interplay

| Situation | What happens |
|---|---|
| Window hidden to the tray, the user starts minRender again (Start menu, Dock, a double click) | The second process signals and exits; the first shows its window. This is the documented way back besides the tray. |
| Store update while running | Windows sends the session-end messages; the application aborts the render and exits (2.5). Because it registered, Windows starts the new version with `--minimized` (section 5.2). `mr-restart` plays no part. |
| Sign-in after a reboot that Windows itself restarts applications for | Both the startup task and the registered restart may fire. The loser of the race is a second launch: it makes the winner **show its window**, so a start meant to be hidden ends visible. By reading; not tested. |
| Relaunch helper and the lock | The old process normally exits cleanly and releases the lock within the helper's 15 s + 2 s. If the helper had to terminate it, the lock file remains and the new start relies on Qt's stale-lock test (4.1). |
| An agent still rendering when a new instance starts | See step 0 (`step0-process-architecture.md`, 7.2): the old agent can reconnect to the new instance. All the restart paths here abort the render first, so this needs a crash or a terminated application. |

---

## 5. Updates as they touch the application's life

### 5.1 macOS: Sparkle 2

| Fact | Today | Reference |
|---|---|---|
| Start | `startSparkleUpdater()` on a zero-delay timer once the event loop runs; a standard updater controller with **no delegates** | `main_qt.cpp:371-376`; `sparkle_updater_macos.mm:18-31` |
| Policy (Info.plist) | `SUEnableAutomaticChecks` true (so no first-run permission question), `SUScheduledCheckInterval` 86400 s, `SUEnableSystemProfiling` false, the feed URL and the public key. `SUAutomaticallyUpdate` is not set: Sparkle asks before installing. | `cmake/MacOSXBundleInfo.plist.in:59-68` |
| Manual check | About > `Check for &Updates…` -> `checkForUpdatesNow()`. The item is removed when no updater is compiled in. | `Main.qml:128`, `166-171`; `app_updater.h` |
| Built in only if | `external/Sparkle/Sparkle.framework` exists at configure time (fetched by `scripts/fetch_sparkle.sh`); otherwise the calls are empty | `CMakeLists.txt:363-394`; `sparkle_updater.h:19-43` |
| A running render when an update installs | **Killed.** Sparkle terminates the application through AppKit; that is the immediate-quit path of 2.4. There is no delegate to postpone the relaunch, so nothing checks for a render and nothing warns. | - |
| After the update | Sparkle relaunches the application normally: **window shown**, node state as it was left | - |
| An unattended node | The daily check opens Sparkle's window and waits for an answer. With nobody at the machine the node stays on the old version indefinitely. | by reading |
| Headless and the agent | Never link the updater: "a render node must not interrupt a job with an update prompt, and a farm should roll forward deliberately, not per-node" | `sparkle_updater.h:12-14` |

### 5.2 Windows: the Microsoft Store

- No updater in the application; `AppUpdater::available()` is false and the menu item is absent
  (`sparkle_updater.h:36-43`; commits ea4546d, a06f6f7, 8fbbe35). `docs/appcast-win.xml` is frozen so that pre-Store
  installs polling it see "no update".
- **The incident** (commit 2242a0c): "A Store auto-update closes the running app and does not start it again, leaving
  the node off the farm." Two halves fix it, both in `src/ui/platform/app_restart.cpp`:
  - `registerAppRestart()`: only when the process has package identity (`GetCurrentPackageFullName`), calls
    `RegisterApplicationRestart(L"--minimized", 0)` - flags 0, so restart after updates and also after a crash or hang
    once the process has run 60 s (`app_restart.cpp:41-63`). A development build does not register ("should not
    respawn itself under a debugger", `app_restart.h:15-19`). On success the application logs
    `Registered for restart after updates: <package full name>`.
  - `SessionEndWindow` (2.5), so that an application with no visible window still hears the request to close.
  - Measured by the owner with a throwaway package on 2026-10-24: a registered application was started again from the
    new version, with its arguments, about 0.3 s after the update closed it; an unregistered one stayed closed
    (`app_restart.h:8-13`).
- A render running when the update arrives is killed (2.5). The operational answer is in `installer/STORE.md:200-221`:
  turn Store auto-update **off on farm nodes**, upgrade all nodes together with `winget upgrade` while the farm is
  idle, and publish manually "so certification passing never lands on nodes mid-job". A workstation may leave
  auto-update on.

### 5.3 Version skew

The tolerance for a farm that is briefly on two versions does **not** live in the tray or lifecycle code. It is in the
farm protocol: capability tokens advertised in the node's status (`job_edit`, `edit_result`, `edit_epoch`,
`edit_commands`, `chunk_stop`; `monitor_app.cpp:2389-2393`), `leaderSupports()` (`monitor_app.cpp:2203-2220`; a leader
that sends no tokens is treated as the oldest feature set), and the edit-epoch fence on completion reports
(`monitor_app.cpp:502-567`, `2700-2724`, `2780-2824`). `PROTOCOL_VERSION` is 2 (`config.h:108`).
`installer/STORE.md:209-210`: "The epoch-fence/capabilities machinery tolerates brief version skew either way."
These belong to `step0-contracts.md`; the lifecycle only decides when a node changes version.

---

## 6. `minrender-headless`

| Question | Answer | Reference |
|---|---|---|
| What it is | The same node with no window: the core sources plus `main_headless.cpp` plus the Win32 tray. It links Qt Core and Network only, for the single-instance guard. Define `MINRENDER_HEADLESS`. | `CMakeLists.txt:497-551` |
| Main loop | Its own: `update()`, tray labels, a Win32 message pump, a 50 ms sleep, until `shouldExit()` | `main_headless.cpp:81-101` |
| Tray | Windows: section 1.5. Elsewhere: none (a stub). | - |
| How it exits | Windows: the tray's Exit (`requestExit()`: waits for a render). Elsewhere there is no tray and no signal handler: only a kill, which skips `shutdown()`. | `main_headless.cpp:77-79` |
| Arguments | None read. `--minimized` means nothing. | - |
| Origin | 5af849e (2026-03-29): the backend for an attempted UI in a separate process ("headless sidecar"); the UI attempt was removed in 2026-04, the target stayed | commit messages |
| Where it is used | Nowhere in production. The manifest comment says "used for same-machine sandbox farms"; but it shares the application's single-instance name, data folder and node identity, so it cannot run beside the application as a second node. | `AppxManifest.xml.in:35-36` |
| Store build | Built and **copied into the package** (`scripts/package.bat:62`), but with no `Application` entry, no alias and no firewall rule: nothing can launch it. The hidden entry that gave it an alias was refused by Partner Center on 2026-10-17 ("The package specifies a headless app. You don't have permission to create a headless app"); removed in 5d36671. `make_msix.ps1` does not require it. | `AppxManifest.xml.in:41-46`; `installer/STORE.md:59-62` |
| macOS | Not built by the release script, not in the bundle | `scripts/macos-release.sh` |
| Updater, session end, restart registration | None of them: those are wired in `main_qt.cpp` only | - |

---

## 7. Settings that change tray or lifecycle behaviour

The configuration file is `config.json` in the data folder (`%USERPROFILE%\minRender` on Windows,
`~/Library/Application Support/MinRender` on macOS; `platform.cpp:89-123`). Keys and defaults: `src/core/config.h:34-72`.

| Key | Default | In Settings as | Effect on tray / lifecycle |
|---|---|---|---|
| `node_stopped` | `false` | not shown; written by Stop / Resume (tray, title row, remote stop / start) | The node starts Stopped or Active; the tray's Stop / Resume label and the tooltip follow it |
| `show_notifications` | `true` | "User interface" > `Show notifications` (`SettingsPanel.qml:224-228`) | **None.** Stored, saved, accepted by `POST /api/config` (`http_server.cpp:938-939`), read by nothing. No notification exists to show. |
| `auto_start_agent` | `true` | not shown | **None.** Read and written by the JSON code only; the agent is started on demand whatever it says. |
| `sync_root` | empty | "Sync root" | Indirect: the farm starts at launch only if it names an existing folder (`monitor_app.cpp:67-69`); otherwise the tooltip reads "No Farm" |

There is **no** setting for: start at login, start hidden, close-to-tray versus quit, what Exit does while rendering,
the tray icon, update checks, or keeping the machine awake. UI state (splitter sizes, panel visibility, tab, column
widths) is stored separately by Qt's settings store (the registry on Windows - virtualised under the Store package -
and a preferences file on macOS; `Main.qml:68-84`); none of it affects the lifecycle.

Command-line switches: `--minimized`, `--native-frame` (2.1). Environment variables: none read for lifecycle.

---

## 8. For the port

### 8.1 Behaviour -> today -> what a winit + Rust application needs

Risk: **H** hard to reproduce or easy to get subtly wrong; **M** needs care or a walk; **L** routine.

| # | Behaviour | Today (Qt) | winit + Rust | Risk |
|---|---|---|---|---|
| 1 | Tray icon, Windows | `QSystemTrayIcon` over `Shell_NotifyIconW`; Qt re-adds the icon when the taskbar is recreated | `tray-icon` (same family as `muda`, which the workspace already uses at 0.21; it takes a `muda` menu) or raw `Shell_NotifyIconW` + a hidden window through the `windows` crate. Either way: re-add on the `TaskbarCreated` message; a version-4 icon gives proper click and keyboard messages. | M |
| 2 | Menu-bar item, macOS | `NSStatusItem` with a template image and an attached `NSMenu` | `tray-icon` (`with_icon_as_template(true)`), or `NSStatusItem` through `objc2-app-kit` (already a dependency). Must be created on the main thread after the application has finished launching - with winit, in or after `resumed` / `can_create_surfaces`, not before the loop runs. | M |
| 3 | Tray events beside winit's loop | Qt's own loop | `tray-icon` and `muda` deliver on global channels; forward them into the winit loop with an `EventLoopProxy` so a hidden window (which gets no redraws) still reacts. The 50 ms pump must be a timer on the loop (`ControlFlow::WaitUntil`), not a redraw. | M |
| 4 | Left click shows the window (Windows); menu on right click | `activated(Trigger / DoubleClick)` | `TrayIconEvent::Click` / `DoubleClick`, with `menu_on_left_click(false)` | L |
| 5 | Click on macOS | The menu opens on any click, and a left click also shows the window (1.3) | A ruling first (9.2). The native convention is "click opens the menu"; `tray-icon` can do either. | L once ruled |
| 6 | Tooltip following state | set every 50 ms | set on change only (`set_tooltip`). Windows limits the text to 127 characters; the job id is unbounded today. | L |
| 7 | Menu labels following state (Stop / Resume) | `QAction::setText` | `muda::MenuItem::set_text`; on Windows the popup is built from the same items | L |
| 8 | Close hides the window; the node lives on | `onClosing` refuses, `setVisible(false)`; `setQuitOnLastWindowClosed(false)` | Handle `CloseRequested` with `window.set_visible(false)`; never exit the loop on it. Both existing applications quit on close, so this is new shell behaviour. GPU surface: stop presenting while hidden, and rebuild or reconfigure on show. | M |
| 9 | Show, raise, activate from the tray / a second launch | `setVisible` + `raise` + `requestActivate` | `set_visible(true)`, `set_minimized(false)`, `focus_window()`. Windows refuses to give the foreground to a process that does not own it: the second instance must call `AllowSetForegroundWindow(first pid)` before signalling (or the first only flashes). macOS: `NSApplication activate` as well. | **H** (Windows foreground rules) |
| 10 | Start hidden (`--minimized`) | `visible: !startMinimized` | Create the window with `with_visible(false)`; on macOS also avoid activating at launch, or the reopen hook will show it (9.1, item 6) | M |
| 11 | Dock click reopens a hidden window | `applicationStateChanged` -> show if hidden | The precise hook is `applicationShouldHandleReopen:hasVisibleWindows:`. winit owns the application delegate; supply the method the way minNotes supplies `application:openURLs:` (`apps/mn/src/mac_open.rs`), or watch `NSApplicationDidBecomeActiveNotification` to reproduce today's broader behaviour. | M |
| 12 | Dock icon always present | `LSUIElement` false | The same key in the bundle's Info.plist; nothing at run time | L |
| 13 | Application menu on macOS: Quit (Cmd+Q), Settings (Cmd+,) | Qt moves "Exit" and "Settings…" there by their text | `muda` predefined items (`PredefinedMenuItem::quit`, a Settings item) - but the predefined Quit calls `terminate:` directly; to apply the ruled exit rule use a custom item, or implement `applicationShouldTerminate:` | M |
| 14 | Quit from the Dock, log-out, shutdown, Sparkle (macOS) | AppKit `terminate:` -> Qt's delegate -> quit event -> `shutdown()` runs | winit does not implement `applicationShouldTerminate:`; by default AppKit then exits the process **without returning from the event loop**, so `shutdown()` (abort the render, clean marker, save) would not run. Add `applicationShouldTerminate:` to the delegate: run the exit rule, return "later" or "cancel", then exit the loop. Sparkle depends on this too. | **H** |
| 15 | Exit rules (wait versus kill) | Two rules by accident (2.4) | One state machine in the node core, UI-independent, with the ruled behaviour (9.2). If a wait survives, it needs a visible state and a way out. | L once ruled |
| 16 | Session end on Windows (sign-out, shutdown, Store update) | A hidden top-level window answering `WM_QUERYENDSESSION` / `WM_ENDSESSION`; Qt's own handler as a second route | winit surfaces neither message. Keep a dedicated hidden top-level window with its own window procedure (the `windows` crate), created on the thread that pumps messages (winit's loop thread does). It must exist while the main window is hidden - and with winit a hidden window keeps its HWND, unlike the comment's claim for Qt, but the dedicated window is still the safe form. Do the abort **inside** the handler: the process may be ended as soon as it returns. | **H** |
| 17 | Restart after a Store update | `RegisterApplicationRestart(L"--minimized", 0)` when packaged | The same two calls through the `windows` crate (`GetCurrentPackageFullName`, `RegisterApplicationRestart`). Carry the measured fact (restart about 0.3 s after close) as the walk's pass bar. | M (needs a packaged walk) |
| 18 | Start at login, Windows | Manifest `windows.startupTask` -> `mr-restart.exe` (no arguments) -> `minRender.exe --minimized` | Two forms. (a) Keep a tiny helper executable as today. (b) Name the application itself in the startup task and detect the activation kind (`AppInstance::GetActivatedEventArgs` -> `StartupTask`) to start hidden; this removes the helper but changes what Task Manager launches and must be proven in a package. In-app on/off, if wanted: `Windows.ApplicationModel.StartupTask` (`GetAsync`, `RequestEnableAsync`, `Disable`) - once the user disables the task in Task Manager, only the user can re-enable it. | M |
| 19 | Start at login, macOS | none | If ruled in: `SMAppService.mainApp` (macOS 13+; today's floor is 12.0) through `objc2-service-management`, or the `auto-launch` / `smappservice-rs` crates. There is no argument channel: "started at login" must be inferred (`NSApp` launched as a login item, or a stored flag) to start hidden. | M |
| 20 | Single instance | `QLockFile` + `QLocalServer` | Lift QCView's `apps/qcv/src/single_instance.rs` (`interprocess` local socket: a per-user socket file on macOS, a named pipe on Windows; the endpoint itself is the lock, and a dead owner's endpoint is detected by a failed connect). Decide whether the name stays `MinRenderMonitor`: keeping it makes the new and the old application exclude each other on a machine that has both - but the old one also takes a lock FILE the new one would have to honour for that to be symmetric. | M |
| 21 | Self / remote restart, Windows | `mr-restart.exe --pid --exe --minimized`, started with a job breakaway flag | (a) keep a helper; (b) the application starts a second copy of itself with a "wait for pid, then run" switch, which then becomes the node (no extra executable, but a hidden relaunch of its own binary - the posture question already raised for the leash, canon rows 387-388). In a package, check that the relaunched process keeps package identity and that the breakaway flag is accepted (9.1, item 12). | **H** (packaged behaviour unverified today) |
| 22 | Self / remote restart, macOS | not supported | If ruled in: start a detached copy (`open -n -a` or `posix_spawn` with a new session) that waits for the pid; no helper needed | M |
| 23 | Sparkle (macOS) | linked framework, standard controller, no delegate | Exists twice in this workspace (`apps/qcv/src/sparkle.rs`, `apps/mn/src/sparkle.rs`, loaded at run time). New here: Sparkle's relaunch must go through item 14, and a delegate could postpone an install while rendering (`updater:shouldPostponeRelaunchForUpdate:untilInvokingBlock:`) if the owner wants that. | M |
| 24 | Notifications | none | nothing, unless ruled in (then `UNUserNotificationCenter` on macOS, toast notifications on Windows - which need package identity or a registered AppUserModelID) | - |
| 25 | Sleep / wake | none | nothing for parity. If ruled in: `SetThreadExecutionState` / `PowerCreateRequest` on Windows, an `IOPMAssertion` or `NSProcessInfo beginActivity` on macOS, held while a chunk runs | L |
| 26 | Hidden-window cost | QML stops rendering; the pump continues | Do not render or present while hidden; keep the node's tick alive. On macOS consider an activity token so App Nap does not slow the tick of a hidden application. | M |
| 27 | Dark title bar, window icon, taskbar identity | Qt | exists in both applications (`with_theme`, the icon resource). The tray icon and the window icon come from the same `.ico`; `tray-icon` wants RGBA or a resource. | L |
| 28 | The headless program | a separate target | A mode of the one binary or nothing (owner's question 3 in step 0). The Store will not accept an entry hidden from Start without a waiver. | - |

### 8.2 Packaged (MSIX) versus unpackaged on Windows

| Matter | Packaged (Store) | Unpackaged (development build) |
|---|---|---|
| Restart registration | `RegisterApplicationRestart` is called | Deliberately skipped (`registerAppRestart()` returns empty) |
| Start at login | The manifest's startup task; on by default; the user toggles it in Windows | Nothing. No Run key, no shortcut. |
| `mr-restart` with no arguments | Run by Windows at sign-in | Never run |
| Session end from servicing | Sent before an update; the application exits and is restarted | Only at sign-out / shutdown |
| Updater | The Store | none |
| Firewall | Manifest rules for `minRender.exe` (TCP 8420, UDP 4243) | A prompt, or rules made by hand |
| Registry | Per-user keys are virtualised: the node-state key and Qt's UI settings are invisible to other programs and removed on uninstall | Real registry |
| Files under AppData (the single-instance lock in the temp folder) | New files are redirected to the package's private store (by reading; not tested here) | The real temp folder |
| Data folder | `%USERPROFILE%\minRender` in both: outside AppData on purpose, so never redirected | the same |
| Job objects | The process tree may already be inside a job; a breakaway flag can be refused. Spike S13 found the application's own jobs nest inside the package's. | Under a debugger or a scheduled task the same can apply |
| Identity of children | A child started from the package folder keeps package identity (the helper's relaunch relies on it; unverified for the remote-restart path) | none |
| Headless | In the package, unreachable | Runnable |
| Install / remove of a test package | Must be done in an interactive desktop session, not over a remote shell (`installer/STORE.md:64-68`) | - |
| Toast notifications (if ever wanted) | Identity comes with the package | Needs an AppUserModelID and a shortcut |

### 8.3 What is risky, in one list

1. **macOS termination (item 14).** Today every quit runs `shutdown()` because Qt turns AppKit's `terminate:` into an
   ordinary return from the event loop. winit does not. Without a delegate method, Cmd+Q, the Dock's Quit, log-out
   and Sparkle would end the process with the render still owned by nobody (on macOS nothing kills a renderer whose
   supervisor vanished - the kill-path note, G5).
2. **Windows session end (item 16).** It is the only thing between a Store update and a node that stays offline, it
   was fixed by an incident, and it cannot be tested without a package and an update.
3. **Foreground on a second launch (item 9).** Unverified even today.
4. **The relaunch under a package (item 21).** Not in the Store checklist; may not work today.
5. **A tray beside winit on macOS (items 2, 3).** Creation order and event delivery to a hidden window. Worth a small
   spike with a pass bar: icon present, menu works, click delivered within one tick, with the window hidden for an
   hour.
6. **Starting hidden on macOS (item 10)** interacts with whatever reopens the window on activation.

---

## 9. For the owner

### 9.1 Things that look broken, accidental or inconsistent today

None of these is fixed or reproduced silently; each needs a ruling.

1. **Two exit rules, one of them an orphan.** Tray Exit waits; everything else kills. The wait is what remained when
   the "Kill && Exit / Cancel" dialog was dropped in the Qt port (2.4). While it waits there is no sign of it, the
   node keeps its current chunk, and nothing can cancel it. The manual presents the two exits as equivalent.
2. **The menu's Exit works by a side effect** of using `QApplication` for the tray (2.4). Two source comments describe
   a single exit path that does not exist.
3. **`Show notifications` is a visible setting that does nothing** (default on); `auto_start_agent` is a stored key
   that does nothing.
4. **Status that is computed and never shown:** the tray status text (every 50 ms), and `trayIconName()` with a
   documented colour it never returns and a `red` that would be the normal idle state.
5. **macOS: a left click on the menu-bar item opens the menu and shows the window at once** (1.3). By reading Qt; to
   be confirmed by a walk. The source comment claims parity with a Win32 tray that only reacts to a double click.
6. **macOS: `--minimized` is probably defeated** by the Dock-reopen hook, which shows a hidden window on any
   activation - including the activation at launch. Harmless today because nothing on macOS passes the flag; it
   matters the day macOS gets a login start or a restart.
7. **A second launch with `--minimized` pops the running instance's window**, because the activation channel carries
   no arguments (4.1, 4.4). Reachable today when the startup task and Windows' own restart both fire at sign-in.
8. **No tray, still hide-on-close** (1.7), against the header's own note.
9. **macOS has no start at login, no restart after an update, and the remote restart commands do nothing there**,
   while the peer menu offers "Restart App" for a macOS peer all the same (the peer answers with an error, which
   the sender writes to its log only: `monitor_app.cpp:1459-1485`). Already raised in step 0; repeated here because it is the lifecycle's largest
   asymmetry.
10. **The single-instance comments are out of date** in two places: the lock is a file on both systems, and a crashed
    instance's lock is (by Qt's code) detected and removed, not left for "the user (or installer)" (4.1).
11. **`mr-restart` restricts the program it starts but not the process it kills** (`--pid` is unrestricted; 15 s, then
    terminate), reports success when its final start failed, mangles non-ASCII paths, and is still described as an
    "Update Restart Helper".
12. **The relaunch is started with a job-breakaway flag.** If the Store package runs the application in a job that
    does not allow breakaway, that start fails and "Restart App" / the restart file do nothing on Store nodes. The
    Store checklist (`installer/STORE.md:70-95`) does not cover remote restart. Unknown, not asserted.
13. **`minrender-headless` ships in the Store package and cannot be launched;** the manifest's stated purpose for it
    (a second node on one machine) is impossible with the shared instance name, data folder and identity. Off
    Windows it has no way to exit cleanly.
14. **Immediate exits abort the render after the reporting thread has stopped** (2.4), so the leader is not told.
15. **Nothing keeps a rendering node awake, and nothing notices sleep or wake** (2.6).
16. **Stop in the tray kills the current render with no confirmation,** one row above Exit, which waits. The two
    neighbouring items have opposite manners.
17. **The window's size and position are not remembered;** every start is 1440 x 900 at the system's default place.
18. **Naming drifts:** tooltip `minRender` (Qt) versus `MinRender` (Win32 tray), window title "minRender Monitor",
    menu "Stop" versus "Stop Node", the lock `MinRenderMonitor`, the helper "MinRender Update Restart Helper".
19. **macOS: the File menu holds one item** ("Farm Cleanup…") because Qt moves the other two to the application menu
    (2.3). Probably unintended; harmless.
20. **A Sparkle prompt on an unattended macOS node waits forever;** an attended one loses its render on "Install and
    Relaunch" with no warning (5.1).

### 9.2 Open questions

1. **The exit rule.** One rule for every exit, or two on purpose? Candidates: (a) always ask when rendering - the old
   dialog, "Kill and exit" / "Cancel", perhaps with a third "Exit after this chunk"; (b) always kill; (c) always wait,
   visibly, with a way to cancel. And which of these applies to Cmd+Q and the Dock's Quit?
2. **Exits nobody can answer** (log-out, shutdown, a Store update, a Sparkle install): kill as today? Should the node
   first tell the leader, so the chunk is requeued at once instead of after the node is declared dead?
3. **Tray click on macOS:** menu only (the convention), or the window as well (today)?
4. **Tray click on Windows:** single click shows the window (today's Qt tray) or double click only (the older tray)?
5. **Should the tray show state?** A status line in the menu (the older tray had one), and / or icon states - and
   which: rendering, stopped, no farm, error / needs attention, leader? On macOS a template image cannot carry colour;
   states would be shapes or a badge.
6. **Notifications:** remove the dead setting, or give it a meaning (render failed, node suspended, job finished)?
7. **macOS start at login and restart after an update:** add them (and accept macOS 13 as the floor for
   `SMAppService`, or keep 12 with an older mechanism), or keep macOS as a workstation-only citizen?
8. **An in-application switch for "start at login" on Windows,** or leave it to Task Manager as today?
9. **`mr-restart`:** keep a helper executable, or fold both of its jobs into the application (step 0, question 5)?
   The answer interacts with the leash ruling (rows 387-388).
10. **Does "Restart App" work on Store nodes today?** A two-minute check on a Store-installed node would turn item 12
    into a fact.
11. **Sleep:** should a node hold the machine awake while a chunk renders? (New behaviour.)
12. **Sparkle while rendering:** postpone the install until the chunk ends, warn, or leave as it is?
13. **The headless program:** drop it from the package and the port, or make "no window" a mode of the one binary?
14. **Remember the window's size and position?** (New behaviour; QCView's port does.)
15. **Should the first close tell the user** the application is still running in the tray (once)? Today it says
    nothing; the Store listing text carries the explanation instead.
16. **The instance name:** should the port exclude a still-installed 0.5.x application on the same machine (two nodes
    with one identity and one port would otherwise fight), and if so by the old lock, by the port, or both?

### 9.3 Corrections to the step-0 notes

| Step-0 statement | Correction |
|---|---|
| `step0-process-architecture.md` 7.9: a crash leaves the lock and the next launch refuses to start until the file is removed by hand | Qt removes a lock whose owner is dead or which predates the last boot, whatever the stale time (4.1). The risk is small (a recycled process id with the same executable name), not an outage by default. To be confirmed by a walk. |
| `step0-ui-inventory.md` 1.2: "Click or double-click on the icon: show, raise and activate the window" | True on Windows. On macOS any click opens the menu; a left click also shows the window (1.3). |
| `step0-ui-inventory.md` 1.2: tooltip "refreshed every 50 ms" | Set every 50 ms; on Windows Qt drops unchanged text, so the shell is not called 20 times a second. (The headless Win32 tray does call it every time.) |
| `step0-ui-inventory.md` 1.3: File menu on macOS holds Settings…, Farm Cleanup…, Exit | Qt moves Settings… and Exit into the application menu (as "Settings…", Cmd+, and "Quit minRender", Cmd+Q); File keeps Farm Cleanup… only (2.3). So Cmd+Q exists and is the immediate exit. |
| Both notes: the two exits "by reading" | Confirmed against Qt's source, with the reason (2.4) and the history (the lost dialog). |
| `step0-process-architecture.md` 4: the tray has "Show Window, Stop / Resume, Exit" | Correct for the shipped tray. The headless Win32 tray differs (title, status line, "Stop Node" / "Resume Node"; 1.5). |

### 9.4 What a walk should settle (nothing here was run)

| Check | Where | Settles |
|---|---|---|
| Kill the process, start it again | both | the stale lock (4.1) |
| Left click and right click on the menu-bar item | macOS | 1.3, item 5 |
| `open -a minRender --args --minimized` | macOS | item 6 |
| Menu Exit, Cmd+Q, Dock Quit while a synthetic chunk runs | both | 2.4 |
| Tray Exit while a synthetic chunk runs; then try to cancel | both | 2.4 |
| Start the application a second time while the first is hidden, from Explorer | Windows | foreground behaviour (4.1) |
| "Restart App" sent to a Store-installed node | Windows | item 12 |
| Sign out and in with both the startup task and a pending restart | Windows | item 7 |

On the Windows machine these use a synthetic renderer only, never while a real render is running there.
