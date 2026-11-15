# minRender, the port: the smaller pieces of the windowless node, and a mixed farm on real machines (2026-11-14)

Canon row 400.

## Built

| Piece | Contract | What it does | Held to |
|---|---|---|---|
| Heartbeats | 1.6 | one at the start and every 3 s to the farm's multicast group on the settings' port, with exactly 0.5.x's keys and types (a 0.5.x node reads them without a guard); `bye` on a clean stop; a heartbeat heard refreshes a peer the phonebook already gave and never makes one or changes an address; a peer heard lately is polled less, and is not held dead while it is heard | 0.5.11 on the Mac and the port on the Windows machine, on a private port: each shows the other with heartbeat contact, and 0.5.11 went on running |
| The cached listing | 1.2 | an entry whose listing changed while the file still carries the time of the last read is left for 20 s, then 40 s, then believed; an entry that cannot be read is left for 15 s | by reading; it needs a Windows share that caches, and time |
| The address given out | 1.4 | the address this machine uses to reach the share's host (`crates/min-paths`), else the one it leaves the machine by | both real machines gave their LAN address |
| The restart signal | 1.3 | taken from the node's folder; one more than 60 s old ignored; logged - this build cannot start itself again yet | by reading |
| The output folder | 4.11 | made by the rendering node before the render | the real renders |
| Local staging | 4.11 | the render writes to `staging/<job>/<range>/` on this machine; its files go to the output folder before the chunk is called complete; a delivery that fails is tried every 30 s for ten minutes with the node free, then reported failed, the files kept; a job gone meanwhile ends the tries | a farm of two Rust nodes with staging on: four staging folders seen while the job ran, eight files in the output folder, no staging folder left |
| The command drop box | 6.6 | the leader applies job edits left on the share in the order of their names, sets aside what is not an edit or is more than 15 minutes old, keeps `processed/` seven days | an edit of a priority left on the share: applied; one fifteen minutes too old: set aside |

Two heartbeat nodes cannot be tried on one Mac: 0.5.x binds the port in a way that lets no second program have
it there. A `bye` between the two versions was not tried (neither test node could be stopped cleanly on the machine
it was on).

## A mixed farm on real machines

0.5.11 on the Mac (its headless build, in a scratch home) and the port on the Windows machine, on the private sync
root of `step7`, heartbeats on, Blender 5.2 on both. A submission in the add-on's form was dropped into the **0.5.11**
node's box.

- The port led. 0.5.11 baked and submitted the job to it; three chunks; `completed` in 74 s; six EXR files.
- 0.5.11 rendered two chunks under the Rust leader and the port one.

## A ghost in the phonebook, found by it

The Windows test node's data folder had been emptied between tests, so it came back under a NEW id at the same
address, and its old entry was still on the share. 0.5.x - and the port, until now - takes the newcomer's answer to a
poll for the old node's: the old id looks alive and free, the leader gives it a chunk, the machine renders it, and
every report is refused because it comes from the new id. Under a 0.5.x leader that chunk stays `assigned` for ever;
here the rule of row 396 returned it and it was rendered a second time.

The port now counts **an answer from another node as no answer**: the old id fails its polls and is held dead after
three. This differs from 0.5.x only in what the port itself believes; it is the owner's to veto. On a real farm the
same thing follows a reinstall that empties the data folder.

## Not yet

- The cleanup routes and `POST /api/config` (2.5 rows 34, 35, 37, 38): nothing in 0.5.11 calls them; they come with
  the window, which needs their work anyway.
- The issuer's side of the command drop box, the form, output-path patterns: with the window.
- Starting itself again (the restart signal, "Restart App", the Store's restart): with the tray, where `mr-restart`
  is to be folded in (row 389).
- A real farm's node: the port has still never been started beside the live farm.
