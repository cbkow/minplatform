# minRender, the port: a farm of its own, Windows, and the scale bar (2026-11-14)

Canon row 397.

## A farm the port makes

- **`farm.json`** (contract 6.2): made when it is not there, with a secret of 32 bytes from the system's randomness
  as 64 lowercase hex characters; read when it is there, a secret made only if it has none; a file that is there and
  is not JSON is never replaced. Unknown keys survive a rewrite.
- **What the port brings to a share** (row 390: Blender only): the Blender plug-in template and the Blender add-on,
  copied UNCHANGED from the minRender repository (`apps/mr/resources`, with a note of where they are from). They are
  written when this build is newer than the build named in `last_example_update`, and the stamp is then set to this
  build. **Nothing is removed** - 0.5.x deleted every example and plug-in template it did not ship; the After Effects
  and Cinema 4D files a farm already has stay as they are. The Blender EXAMPLE template (the one for the New Job
  form) is not brought yet: the file is named after a studio in that repository and needs a neutral name first.
- **The node's log on the share** (6.4), which other nodes read: `nodes/<id>/monitor-YYYY-MM-DD.log`, one line an
  event as `HH:MM:SS.mmm LEVEL  [category] message` in local time; files more than seven days old are removed at
  start as well as when the date turns (row 389).
- **The read routes** any node answers (2.5 rows 31-33, 36): a job's chunk logs and one of them, a node's log, the
  node's settings.

A farm of two Rust nodes and nothing else: the first makes the farm, both elect, `mr-conform record` finds **every
route in line with the contract** (no gap left among those it asks), and `mr-conform drive` - whose stand-in renderer
is now the harness itself, with no shell - gives 0 contradictions and leaves no process.

## Windows

`tools/mr-conform/ports.ps1` does the same on a Windows machine in a local scratch folder. Two Rust nodes: one leads,
both render, the driver's 65 requests - 0 contradictions, nothing left running. So leading, the database, the
snapshot and the leash's job object have all run on Windows. (Found on the way: the node id and the secret now come
from Windows' own random source.)

## The scale bar (row 389: 10,000 jobs; one job of 1,000,000 frames in 100,000 chunks)

`mr-conform load` submits exactly that to a private farm's leader, the jobs asking for a tag no node has so that
nothing is given out.

| | First run | After the rework |
|---|---|---|
| 10,001 submissions | 188 s | **0.7 s** |
| The leader's two-second cycle | 6 to 10 s and growing; requests timed out behind it | never over half a second |
| `GET /api/jobs` (3.7 MB) | - | 2 to 16 ms |
| The million-frame job's detail (18 MB) | - | about 180 ms |
| Every job command, a job rendered, paused, cancelled, failed - WITH those 10,001 jobs in the queue | - | the driver passes: 0 contradictions |

What was reworked: the cycle is one transaction; a job's chunks are inserted with one prepared statement; every
job's counts come from one pass over the chunks; finished jobs are found by one grouped query; the queue is read once
a cycle and only when a node is free, a job's tags read without the rest of its manifest; the first chunk a node may
take is asked for, not the whole list. The server now waits for a connection without a pause (each request had
waited up to 15 ms).

What the bar does NOT yet cover: the interface. An 18 MB detail every three seconds is what a 0.5.x follower's window
asks of its leader; the port's own window must not (row 389: nothing fetched on the interface's thread, fetches
incremental). And a real farm's disk and share are slower than this machine's.

## Not yet

Staging; UDP heartbeats; the stale-listing rule; the address towards the share's host; the cleanup routes and
`POST /api/config` (34, 35, 37, 38); the command drop box; the restart signal; a real Blender render; the tray and
the window.
