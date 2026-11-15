# S14 - the life of a tray application on winit (opened 2026-11-15)

**Status: OPEN.**

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
