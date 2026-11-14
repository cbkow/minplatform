# minRender, the port: the node's first slice - it joins, follows and renders (2026-11-14)

`apps/mr` (binary `minRender`; canon row 393). The ruled order (row 387): the conformance harness, then a windowless
node that joins, follows and renders, then leading, the interface last. This is the second step's first slice.

## What is built

- **One executable** (rows 387-388). `minRender --data <folder> [--no-window]` is the node; the same file started with
  `--leash -- <program> <arguments>` is the leash of one render (`src/leash.rs`, 60 lines, no farm logic): on Windows
  it enters a kill-on-close job before it starts anything and refuses to start a renderer if it cannot; elsewhere the
  renderer leads a new process group whose id the leash prints first. It kills the tree when its input closes.
- **Joining** (contract 1.1, 1.2, 6.2): the node id file, the farm's secret read from the share, the phonebook entry
  written by temporary file and rename with a new time on every write, rewritten when it is gone from the listing,
  removed on a clean stop.
- **Answering** (2.1-2.5): the token asked for before a route is looked for, compared without an early exit; the
  empty 404; `GET /api/status` (the 25 keys), `GET /api/peers`, node stop / start (kept in `config.json`, written
  atomically with unknown keys preserved), `agent/restart` (answered ok: there is no agent), `node/restart` (500, as a
  non-Windows 0.5.x), `dispatch/assign` with its three refusals, `render/abort`; every leader-only route answers
  `503 not_leader` with the leader's address.
- **Peers and the leader** (1.2, 1.5, 3.1): other nodes found by listing, their entries re-read only when the listing
  changes, polled without a token, dead at three failed polls, forgotten when dead and gone from the listing; the
  election's three-key sort.
- **Rendering** (4.10, 6.5, 7.3): the command from the manifest with the three run-time tokens and no shell; the
  chunk's log on the share, one file an attempt, stderr marked; a frame counted at each match of the completion
  pattern (Rust's `regex`, the engine the 0.5.x agent used); frames sent in batches every two seconds, a "stale"
  answer abandoning the render; `complete` or `failed` kept and re-sent until the leader takes it; the time limit; the
  jobs list watched while rendering, a job cancelled or gone killed at once.

## What it was held to

On the private farm (`tools/mr-conform/farm.sh start 2`, then `farm.sh port`): two 0.5.11 nodes and this one.

- `mr-conform record` against the port: every answer it gives matches the contract. Three routes it does not have
  yet are reported as such by the harness (`/api/config`, a node's log, task output).
- `mr-conform drive`: the 0.5.11 leader **accepted it as a peer, gave it chunks, counted its frames and its
  completions**, and charged its failures exactly as it charges a 0.5.11 node's (a retry, the node barred from that
  chunk). 77 requests, 0 contradictions on the leader's side.
- The kill path, with a renderer that starts a helper process:

| While the port renders | The render and its helper | What the leader records |
|---|---|---|
| the job is cancelled | dead 1.8 s later (the jobs list is asked every 2 s) | a failure: retry 1, the node barred - as 0.5.x |
| `render/abort` | dead at once | nothing: the chunk stays `assigned` - as 0.5.x |
| the node is stopped | dead at once | a failure - as 0.5.x |

  No process was left after any of them.

## The worker's wire, 0.5.11 beside the port (the same day)

`mr-conform lead` makes the harness the LEADER of a private farm of one worker: it enters the phonebook with the tag
`leader`, answers what a worker asks, files everything it is sent, and puts the worker through nine scenarios with a
renderer that has no shell in it (`mr-conform fake-render`, the same on every system, which starts a helper process).
Run against a 0.5.11 node on macOS, against the port on macOS, and against the port on Windows.

| Scenario | 0.5.11 sends its leader | The port sends | The same? |
|---|---|---|---|
| a chunk renders | frame batches, then `complete` {node, job, frames, `elapsed_ms` 0, `exit_code` 0, the epoch} | the same | yes |
| assigned with no epoch | the same bodies without `edit_epoch` | the same | yes |
| the renderer fails | `failed` with `error` `""` | the same | yes |
| `render/abort` | nothing after the abort | nothing | yes |
| the job turns `cancelled` in the list | `failed` | `failed` | yes |
| the job leaves the list | `failed` | `failed` | yes |
| a frame report answered "stale" | nothing more | nothing more | yes |
| a second assignment while rendering; a bad id; a body that is not JSON | 409 `busy` to all three | 409 `busy` to all three (after a correction: the port read the body first) | yes |
| the node is stopped | `failed` | `failed` | yes |

Differences that remain, none visible to a leader's bookkeeping:
- 0.5.11 takes 2.1 s from an assignment to "rendering" (it starts its agent); the port 0.1 s.
- 0.5.11 sends frame batches of one or two and may send the last AFTER `complete`; the port sends what it has before
  `complete`.
- 0.5.11 also asks its leader for the suspended nodes every 3 s (its interface shows them); the port has no interface
  yet.
- 0.5.11's requests carry `User-Agent: cpp-httplib/0.18.3` and `Accept-Encoding: gzip, deflate`; the port's do not.

**Helpers left behind.** On macOS 0.5.11 kills a render's process group only when it kills the render. A renderer
that ends by itself leaves whatever it started running: after the nine scenarios four helper processes were still
alive. (On Windows the job object ends them as the agent exits.) The port's leash now ends the group when the renderer
ends, on every system: nine scenarios, no process left, on macOS and on Windows. This is a change from 0.5.x on macOS,
of the kind row 389 allows (only that machine sees it); it is the owner's to veto.

**Windows.** `tools/mr-conform/port-alone.ps1` runs the port on a Windows machine with a scratch farm in a local
folder and the stand-in leader: the nine scenarios give the same reports as on macOS, no process is left, and the
leash's job object is what kills. (Found on the way: the node id was made from `/dev/urandom`; fixed.)

## Not in this slice

- Leading: no database, no dispatch, no snapshot. The node refuses to start without the tag `noleader`; if it finds
  itself first in the election it says so in its log and still answers as a follower.
- UDP heartbeats (1.6); the stale-listing rule for a share that caches (1.2, steps 3, 4 and 6); the address towards
  the share's host (1.4, step 2); the restart signal (1.3).
- Local staging (4.11); the GPU's name in the record (sent empty); routes 31-38; submissions from the drop box.
- Windows: the worker's side has run there under the stand-in leader (above); a farm on a real share, with paths translated between systems, has not.
- The real data folder: this build runs only where `--data` points. It has never been started against a real farm.
