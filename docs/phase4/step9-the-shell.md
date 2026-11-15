# minRender, the port: the application's shell (2026-11-15)

Canon row 403. `apps/mr/src/shell.rs`, `instance.rs`, `main.rs` - the rules of spike S14 (row 402) and the rulings of
row 389, in the product.

## What is built

- **Two ways to run.** `minRender --data <folder>` is the application: a window, an icon in the tray or the menu bar,
  the node on its own threads. `--no-window` is the node alone, as before. (`--data` is still required: the port runs
  only where it is told to.)
- **The window hides.** Its close button hides it and the node goes on; with no tray icon it minimises instead. It is
  empty for now: its content is the next step.
- **The tray.** A status line (Idle / Stopped / Rendering <job> / ... - exits after this chunk), Show Window, Stop
  Node / Resume Node, Exit; the tooltip follows the status. macOS: a click opens the menu only; Windows: a left click
  shows the window. The icon is a placeholder dot (the owner: fine until the real one is in).
- **One way out, and it does not ask** (row 404, which takes back row 389's question). Exit in the tray, Quit in the
  macOS menu (Cmd+Q), the Dock's Quit, a log-out, a shutdown, a session's end: all the same. A render under way is
  killed with its whole tree at once, the leader is told, the application leaves. Stop Node kills at once too.
  (A question - Kill and Exit / Exit After This Chunk / Cancel - was built first. The owner, walking it: "adding
  complexity to the brute force death that worked in the old app". It was also broken: the system's dialog held the
  main loop still, its buttons misbehaved, and the application could not leave behind it.)
- **Leaving is always the node's shutdown**: the render killed, the leader told, the phonebook entry removed, a
  goodbye to the heartbeats - then the loop is told to end, once.
- **One instance for a data folder.** A later launch tells the first how it was started and leaves; only a launch
  that was not minimized shows the window.
- **Starting itself again**: a copy of the program that waits for this one to be gone. It answers `POST
  /api/node/restart` and the restart file on the share - on macOS too, where 0.5.x could not. No helper executable.
- **Windows, with a package's identity**: a start by the startup task is a hidden start; a Store update's restart is
  registered with `--minimized`.

## Held to

Without a hand: a hidden start seen by its leader; a second launch gone in 0.01 s with the first untouched; a restart
asked over the network - a new process was the same node 0.5 s later; a stop - gone in 0.3 s with its phonebook entry
removed. The drivers pass as before on macOS and on Windows.

By the owner's hand: an exit with a render under way (awaited).

## Not yet

- The window's content.
- The real icon.
- A start at sign-in seen on Windows (S14's open item); nothing for macOS (ruled out for this release).
- The rule that the port refuses to start beside an installed 0.5.x (row 389): it comes when the port takes the real
  data folder, since until then the two must run side by side for the tests.
