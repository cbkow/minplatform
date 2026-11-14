# minRender, the port: the node leads (2026-11-14)

`apps/mr/src/lead.rs` (canon row 395). With this the windowless node of row 387's order does all three things: it
joins, it follows and renders, it leads.

## What is built

- **The database** (contract 5.1): 0.5.x's two tables and two indexes, statement for statement, with its six additive
  steps tried on every open; WAL, foreign keys, the 5 s busy wait. Opened only while the node leads.
- **Taking over and giving way** (3.4): when the election puts this node first, the share's `state/snapshot.db` is
  copied over the local file and opened (with no snapshot: the local file as it is); when another node comes first,
  the database is closed.
- **The cycle** (4.6), every two seconds: queued submissions inserted (an id already there is dropped without a
  word), completions, failures (the retry arithmetic, the barred list, the failure window: 5 in 5 minutes suspend a
  node, 30 s of cooldown after each), reverted assignments, frames; chunks of dead nodes returned; finished jobs
  marked `completed` or `failed`; each free node given at most one chunk - peers in the order of their ids, this node
  last, by the queue's order, the job's tags, the chunk's barred list; the list for `GET /api/jobs` made anew (so it
  trails a command by up to two seconds, as 0.5.11's does); every 30 s the snapshot.
- **The routes** (2.5, rows 9-30): submit, the list, a job's detail and manifest, pause, resume, cancel, archive,
  delete, move (the group renumbered by 1024), priority, retry failed, requeue, edit in its three modes with the
  epoch and the aborts, resubmit in its three forms, chunk stop and reassign, the suspended list and unsuspend, the
  three reports with their "stale" answers.
- The node's own renders, when it leads, are handed to the cycle directly, not sent over HTTP; a cancel or a delete
  ends its own render as the command is taken.

## What it was held to

A private farm of the port as LEADER and a 0.5.11 node as its worker (`MR_TAGS='"noleader"' farm.sh start 1`, then
`MR_PORT_TAGS='' farm.sh port`), driven by `mr-conform drive` - the 73 requests a 0.5.11 leader was put through:

- **0 contradictions but one**: a job's task output, a route this build does not have yet.
- The 0.5.11 worker took chunks from the Rust leader, and its frames, completions and failures were counted. The
  sequences of states are those the 0.5.11 leader produced: pause lets running chunks finish; a stopped chunk is
  `stopped` and returns to `pending` on "reassign"; a cancel is charged to the rendering node; requeue clears
  everything; a job whose chunks all failed is `failed`.

**Takeover, both ways**, with a job of 120 frames under way:

| What was done | What happened |
|---|---|
| The Rust leader killed outright (its snapshot 17 s old) | the 0.5.11 node answered as leader 8 s later, **holding the job from the Rust node's snapshot**, and went on dispatching. The Rust node's own render died with it (no renderer left) |
| The Rust node started again (it sorts first) | it took over from **the 0.5.11 leader's snapshot**; the 0.5.11 node answered `not_leader` again; the job was there and was finished under the Rust leader |

So a 0.5.x node can follow a Rust leader, be led by it, replace it and be replaced by it.

## Seen on the way, for the owner

- **A chunk lost to a failover hangs for ever** (contract 3.3 said state is lost; this is what it looks like). The
  snapshot held chunk 31-40 as assigned to the 0.5.11 node. That node finished it and told the OLD leader, which died
  before its next snapshot. The new leader's copy still says "assigned" to a node that is alive and idle, the node
  never reports it again, and nothing returns it: the job stayed `active` at 11 of 12 chunks until the chunk was
  returned to the queue by hand. This is 0.5.x's behaviour when it leads.
  PROPOSED for when the port leads: a chunk assigned to a node that is alive and has shown itself idle (or rendering
  another chunk) at three polls in a row goes back to `pending`, with no retry counted.
- **"Retry failed" now lets the barred nodes in again** when the port leads (ruled 2026-11-14: the owner's "nice" to
  the suggestion of row 394).

## Not yet

- Paths in a manifest translated for a peer on another system (7.6): a manifest is sent as it is.
- The command drop box on the share (6.6), submissions from the plug-ins' drop box, templates and the baking of a
  manifest, staging, routes 31-38, UDP, the stale-listing rule, the share's bundled content and `farm.json`'s making.
- The leader side on Windows (the worker side has run there).
- Scale: the list is rebuilt with several statements a job each cycle. Row 389's bar (10,000 jobs, 100,000 chunks)
  has not been tried.
