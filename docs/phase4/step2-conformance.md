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

## Not covered yet (part 2)

Everything that writes: joining as a peer, an election, a submission, a dispatch and its reports, the commands,
the snapshot. These run on a private farm with a scratch sync root, not on the live farm.
