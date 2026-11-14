# minRender: the conformance harness, and what the live farm answered (2026-11-14)

`tools/mr-conform` (canon rows 387, 389, 391). Part 1 listens and checks; nothing in it can change a farm.

## What it does

- `mr-conform record --config <the node's config.json> --out <dir>` - run ON a node, in its desktop session (the share
  is not reachable from a remote shell there). It sends **GET requests only**, to that node and - for what only a
  leader answers - to the leader, and to every other node for its own record. It reads four kinds of small file on the
  share (`farm.json`, each `endpoint.json`, and the names in the farm's folder). The farm's secret is read, used and
  never written: `farm.json` is saved with the secret replaced by its length and character set.
- `mr-conform check <dir>` - checks a recording against `step0-contracts.md` (sections 1.2, 2.2, 2.4-2.7, 6.2): status
  codes, content types, error bodies, and for every object each key's presence and type. A key the contract does not
  know is reported as a note. Its output names keys and types only, never a value, so it can be quoted.
- A recording holds job and machine names. It lives under `fixtures/minrender/` (ignored) and is never committed.

The same checks are what the Rust node will be held to: `record` against the port, `check` on the result.

## The first recording (one Windows node of the live farm, a follower; 50 requests)

**Nothing contradicts the contract.** 56 answers and files checked, 0 contradictions, 8 notes.

| Probed | Answer | As the contract says |
|---|---|---|
| `GET /api/status`, with no token and with a wrong one | 200, the peer record, 25 keys, every type right | yes |
| `GET /api/peers` with the token | 200, an array of peer records | yes |
| The same with no token, a wrong token, `bearer` in lower case, two spaces, `Basic` | 401 `{"error":"unauthorized"}` each time | yes |
| An unknown `/api/` path: with the token / without | 404 with an empty body / 401 | yes |
| An unknown path outside `/api/`, and `/` | 404 with an empty body | yes (the contract did not say for `/`) |
| `GET /api/config`, `GET /api/nodes/<id>/log` | 200, the shapes described | yes |
| A task-output path that climbs (`..`) | 400 `{"error":"invalid_path"}` | yes |
| Leader-only routes asked of a follower (jobs, suspended nodes, a job's detail) | 503 `{"error":"not_leader"}` with `leader_endpoint` | yes |
| The same asked of the leader | 200: job summaries, job details with chunks in frame order, manifests, the suspended list | yes |
| A job that does not exist, and its manifest | 404 `{"error":"not_found"}` | yes |
| A job's task output, and one of its files | 200 an array; 200 `text/plain` | yes |
| The share: `farm.json`, five `endpoint.json` | the keys and types described; the secret is 64 lowercase hex characters | yes |

Response headers on every answer: `content-type`, `content-length`, `connection` - nothing else for a peer to lean on.

## What the recording also showed

- **The farm is mixed: the node recorded and one peer answer as 0.5.11, three peers as 0.5.10.** The owner's
  understanding was that every node is on the latest. For the port this is not a problem (the contract notes where
  0.5.10 differs: it rewrites its `endpoint.json` every cycle), but the conformance target is "0.5.10 and 0.5.11", not
  0.5.11 alone, until those three are updated.
- Every job on the farm at the time was complete and every chunk `completed`; all came from the Blender plug-in's
  template. So the recording proves the shapes, not the other states (`active`, `paused`, `failed`, `cancelled`;
  chunks `pending`, `assigned`, `failed`, `stopped`, `partial`). Those need a farm that is doing something - the
  private two-node farm of part 2, never the live one.

## Part 2: a private farm, driven (the same day)

`tools/mr-conform/farm.sh start 2` runs two nodes of a 0.5.11 headless build on one Mac: each with its own home,
temporary folder, node id and port, one scratch share under `target/mr-farm`, loopback addresses, UDP off. It cannot
reach a real farm, and the real installation's data folder is not used. (Starting that build WITHOUT a scratch home
uses the real data folder - it has no `--help`; it just starts. Always go through `farm.sh`.)

`mr-conform drive --config <a private node's config.json> --out <dir>` refuses anything but such a farm. It submits
three synthetic jobs whose "renderer" is a shell script printing one `Saved:` line per frame, sends every job command,
and files the job's detail each time it changes. 81 requests; **0 contradictions**; and `record` against the private
farm (a macOS leader) also shows none.

What 0.5.11 was SEEN to do - each line is now a fact, not a reading:

| Seen | Detail |
|---|---|
| Two nodes, one chunk each at a time | the leader assigns to itself and to the follower; a chunk goes `pending` -> `assigned` -> `completed` |
| A submission is answered `{"status":"ok"}` before it exists | the job appears at the leader's next cycle; the SAME id submitted twice is answered ok twice and makes one job |
| Refusals | no token 401; an id that climbs 400 `invalid_job_id`; a body that is not JSON, or lacks `manifest`, 400 with the JSON library's own text (nothing parses it) |
| **Pause does not stop a running chunk** | assigned chunks render to their end and are counted; only new dispatch stops |
| **A chunk can be `completed` with fewer `completed_frames` than it has frames** | frame reports travel in 2-second batches apart from the completion; a detail read just after shows 1 of 3, 4 of 5 |
| Stop a chunk | `assigned` -> `stopped`, the assignment cleared; "reassign" then makes it `pending` and it is dispatched again |
| Stopping a chunk that does not exist | 200 ok |
| **A cancel is charged to the node as a failure** | the chunk it was rendering comes back with `retry_count` 1 and that node in `failed_on` |
| Requeue | every chunk `pending`, counts and `failed_on` cleared, the job `active` again - from `cancelled` too |
| Move onto itself | 409 `move_rejected` |
| **The job list lags the commands by up to two seconds** | after archive the list still showed the job as `active`; after delete it still listed the job. The detail is immediate |
| A job's detail can say `archived` | the list never does (the job is left out) |
| A command for a job that is gone | 200 ok (pause, after delete) |
| A renderer that fails every time, 2 tries, 2 nodes | each failure adds the node to the chunk's `failed_on` and `retry_count`; the chunk is `failed` at its second failure; the job `failed` when every chunk is |
| No suspension after those failures | the suspended list stayed empty |
| **"Retry failed" keeps `failed_on`** | `retry_count` returns to 0 but both nodes stay barred from both chunks: on a farm where every node has failed a chunk, the retried chunk is `pending` and can never be dispatched |

The bold lines are behaviours a port would not guess and must reproduce (rows 387, 389: what another node can see is
carried). The last one is a trap for the owner to rule on; it is carried until then.

## Not covered yet (part 3)

The wire between nodes as a node sends it: the leader's `assign` body, a worker's three reports and their answers,
`render/abort`, the edit commands and epochs, resubmission, the snapshot and a takeover, the phonebook's timing. For
those the harness has to BE a node - join the private farm as a worker, then as its leader - which is also the first
shape of the Rust node itself.
