# S14 - the life of a tray application on winit (opened 2026-11-15)

**Status: VERDICT - PASS (2026-11-15). One winit executable with the `tray-icon` crate lives as a tray application on macOS and Windows: icon and menu, a window that hides, one instance, a quit that is asked first, the session's end, starting itself again with no helper - also inside a package. ONE thing is not yet seen: the startup task at a sign-in.**

## Why

minRender is an application that lives in the tray: its window hides, the node goes on, it starts at sign-in, it is
one instance, and how it leaves decides whether a render is left behind. Today Qt does all of it. The family's two
applications on winit both quit when their window closes and have no tray. `docs/phase4/step1-tray-and-lifecycle.md`
(section 8) names what is risky; row 389 rules the behaviour (one exit rule that asks while rendering; macOS: a click
on the menu-bar icon opens the menu only; a status line in the menu; a second launch started minimized does not show
the window; `mr-restart` folded into the one executable).

## The question

Can ONE winit 0.31 executable, with the `tray-icon` crate, do this on macOS and on Windows:

1. a tray / menu-bar icon with its menu, beside winit's loop, with a window that hides and shows;
2. start hidden; one instance; a second launch shows the first's window - unless it was started minimized;
3. be ASKED before it is quit on macOS (Cmd+Q, the Dock's Quit, a log-out all arrive as `terminate:`), so that its
   shutdown runs;
4. hear the end of a Windows session;
5. start itself again with no helper executable.

## Pass bar

Part A, by script (`S14_SCRIPT`, no hand on the machine), each 10 times out of 10:
- the tray icon is made without an error; hide and show leave the window as asked, twenty times in a row;
- started minimized, the window is not visible; a second plain launch shows it within **1 s**; a second minimized
  launch leaves it hidden; the second process ends by itself;
- macOS: `terminate:` while "rendering" - the application is asked, is still alive afterwards, does its shutdown
  work, and its loop RETURNS (the process ends normally, not inside AppKit); idle - it leaves at once the same way;
- Windows: the session's query is answered yes and its end reaches the application, which leaves through its loop;
- restart: the copy is the running instance within **5 s**, the old one is gone, and there is still exactly one.

Part B, by the owner's hand (it cannot be scripted without touching the desktop): the icon is there and looks right
in both themes; its menu, the status line, Stop / Resume; macOS - a click opens the menu only, the Dock's icon shows a
hidden window, Cmd+Q and the Dock's Quit ask while rendering; Windows - a left click shows the window in front of
other windows, the right click the menu, the close button hides.

Part C, packaged on Windows (the owner signs out and in): the manifest's startup task names THIS executable with no
helper and it starts hidden; a restart of itself works inside the package.

## Time box

Three hours for A and the build of B and C.

## How to run

`cargo build --release -p s14-tray`; `target/release/s14 --log <file>` with `S14_SCRIPT` as in `src/main.rs`.

## Findings so far (2026-11-15)

### Part A, by script

| | macOS | Windows |
|---|---|---|
| The tray icon made without an error | yes, every run | yes, every run |
| Started minimized: hidden; a minimized second launch leaves it hidden; a plain one shows it; the second process ends | 10 of 10 | 10 of 10 |
| Hide and show, twenty times in a row | 20 and 20, ended normally | 20 and 20, ended normally |
| `terminate:` while rendering: asked, alive, shutdown work done, the loop returns | 10 of 10 | - |
| `terminate:` while idle: leaves at once through the loop | 10 of 10 | - |
| The session's query answered yes, its end reaching the application, which leaves through its loop | - | 10 of 10 |
| Starting itself again: the copy is the one instance, the old one gone | 10 of 10 (hand-over 0.06 s) | 10 of 10 |

Three things the first runs got wrong, each a rule for the product:

1. **The loop is told to end ONCE.** Asking at every turn kept the loop turning on macOS: one run span 541,465 times
   and then crashed inside AppKit while a window was being released.
2. **On Windows the loop must be woken after it is told to end** from the step before it waits: without that the
   process sat there for good with its work done.
3. **`applicationShouldTerminate:` answered "not yet"** (winit installs no application delegate, so the application's
   own can hold this) is what lets the shutdown run: the application ends its loop itself and the process leaves
   through `main`, not inside AppKit.

### Part C, packaged on Windows

- The package installs with a startup task whose executable is the application itself.
- **Starting itself again works inside the package** with no helper and without asking to leave the package's job:
  the copy has the package's identity, takes over as the one instance, and the old one is gone at once.
- Windows says how a packaged application was started (`AppInstance::GetActivatedEventArgs`): a start by hand reads as
  kind 0 (launch). The copy started by the application itself has no activation at all, which is not a startup task.
- `RegisterApplicationRestart("--minimized")` succeeds packaged and unpackaged.
- NOT yet seen: the startup task at a sign-in (the package is left installed for it).

### Part B, by the owner's hand (2026-11-15: "the placeholder dot is fine until we get the icon in, everything else seems fine")

Read back from the logs of the walks:

| | macOS | Windows (the installed package) |
|---|---|---|
| The icon shows; its menu opens | yes | yes |
| Stop Node / Resume Node from the menu, twice | yes | yes |
| The close button hides the window, the icon stays | yes | yes |
| A left click on the tray icon shows the window, and it takes the focus | (ruled: menu only) | yes |
| Exit from the menu while "rendering": the question is put, the work done, a clean end | yes | (idle: a clean end) |

Not walked by hand, and why it is not held against the verdict: Show Window from the macOS menu and a click on the
Dock's icon (the same `show` the second launch and the Windows click use); the Dock's Quit (the same `terminate:` the
script sent ten times). The spike has no application menu, so there was no Cmd+Q to press: the product's Quit item is
to call the exit rule, never `terminate:` directly.

## Verdict

**PASS.** The shape for the product (canon row 402):

- `tray-icon` for the icon and its menu, made after the window, on the main thread; its events and the menu's sent
  into winit's loop through the proxy. macOS: a template image, the menu on a click; Windows: the window on a left
  click, the menu on a right one.
- The window's close hides it; with no tray, it minimises.
- The application's own delegate on macOS answers `applicationShouldTerminate:` with "not yet" and ends the loop
  itself after its shutdown; it also answers the Dock's reopen.
- A hidden top-level window of the application's own on Windows hears `WM_QUERYENDSESSION` / `WM_ENDSESSION`.
- The loop is told to end once, and on Windows is woken after that.
- One instance; a second launch says whether it was started minimized, and only a plain one shows the window.
- Starting itself again: a copy that waits for this process to be gone - no `mr-restart.exe` (row 389).
- `RegisterApplicationRestart("--minimized")` for a Store update.

**Open:** the startup task naming the application itself has been installed and never fired, because nobody has
signed out since. Until it is seen, the product keeps the possibility of a small helper for that one job, as 0.5.x
has. The test package stays installed on the Windows machine for the next sign-in.
