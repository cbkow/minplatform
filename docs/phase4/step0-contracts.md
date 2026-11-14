# Phase 4, step 0 — minRender's frozen contracts

Read from the MinRender repository, branch `min-paths` (= `main` at release 0.5.11 plus the min-paths adoption),
working tree as checked out on 2026-11-14 (HEAD `9526557`). All paths below are relative to that repository's
root. Line numbers are those of that tree. Nothing here was run; every statement is from the source.

Purpose: a Rust node must join a live farm of 0.5.x nodes, lead or follow, take and report work, and read / write
the same shared folder, with old and new nodes mixed for a long time. Every wire format and every on-disk format
below is therefore frozen. Where the code and the user docs disagree, the code is the contract and the
disagreement is listed in §11.3.

Conventions used here

- "ms" timestamps are **Unix epoch milliseconds from the wall clock** (`std::chrono::system_clock`), signed 64-bit.
- JSON is produced by nlohmann/json 3.11.3: `dump()` = compact, no spaces, **object keys in ascending byte order**
  (the library's default `std::map`); `dump(2)` = 2-space indent, same key order. No reader depends on key
  order or whitespace. `dump()` throws on invalid UTF-8, so every string on the wire is valid UTF-8.
- Readers use `if (j.contains(k)) j.at(k).get_to(x)` or `j.value(k, default)`. Both **throw when the key is
  present with the wrong JSON type, including `null`**. A missing key is always tolerated; a `null` or
  mistyped one is an error for the whole document unless noted. A port must therefore *omit* a key rather
  than send `null`, except for the keys listed as nullable.
- Libraries (root `CMakeLists.txt`): cpp-httplib v0.18.3 (HTTP server and client, no TLS), nlohmann/json v3.11.3,
  SQLiteCpp 3.3.2 (bundled SQLite), Qt 6 (UI only; nothing Qt is on the wire).
- Constants: `PROTOCOL_VERSION = 2`, `APP_VERSION "0.5.11"` (`src/core/config.h:108-112`).

Contents: 1 Discovery · 2 HTTP mesh · 3 Election · 4 Jobs and scheduling · 5 Persistence · 6 Share layout ·
7 Templates and paths · 8 Submission · 9 Monitoring data · 10 Time · 11 Compatibility checklist ·
Appendix A agent IPC · Open questions.

---

## 1. Discovery

There are three mechanisms and a node does not choose between them: the **phonebook on the share is the only
way a peer is created or its address learned**; **HTTP polling** fills in a peer's details and decides whether
it is alive; **UDP multicast** is an optional fast path that can only refresh a peer already known from the
phonebook. Order of each cycle of the peer thread (`PeerManager::threadFunc`, `src/monitor/peer_manager.cpp:161`):
`ensureEndpointPresent → checkRestartSignal → discoverPeers → pollPeers → recomputeLeader`, then sleep 3 s
(30 × 100 ms). The cycle period is therefore 3 s **plus** the time the serial HTTP polls take.

### 1.1 Node identity

- `node_id`: 12 lowercase hex characters, generated once and kept in the local file `node_id.txt`
  (`src/core/node_identity.cpp:25-85`). Windows: first 12 hex digits of a new UUID (`Data1` 8 digits + `Data2`
  4 digits); other systems: 6 bytes of `/dev/urandom`. The file holds the id with no newline; on load the first
  line is taken and **accepted only if its length is exactly 12** (no character check), otherwise a new id is
  generated and the file overwritten.
- The id is used as a directory name on the share, as the election tie-break (plain byte-wise string compare), in
  every report, and in the agent's pipe / socket name.

### 1.2 The phonebook — `nodes/<node_id>/endpoint.json`

Location: `<farm>/nodes/<node_id>/endpoint.json`, where `<farm> = <sync_root>/MinRender-v2` (§6).

Format (`PeerEndpoint`, `src/core/peer_info.h:11-35`), written with `dump(2)`:

| Key | Type | Meaning |
|---|---|---|
| `node_id` | string | must equal the directory name, else the file is ignored |
| `ip` | string | IPv4 dotted quad (or whatever `ip_override` holds — it is not validated; a host name works because the HTTP client resolves it) |
| `port` | integer, 0–65535 | the node's HTTP port |
| `timestamp_ms` | integer | wall clock at the moment of the write |

Reader rules: any key may be missing (defaults `""`, `""`, 8420, 0); a present key of the wrong type makes the
read fail. The peer's endpoint string is `ip + ":" + port`.

Writer (`PeerManager::writeEndpoint`, lines 206-262):

1. `create_directories(<farm>/nodes/<node_id>)`.
2. Write `endpoint.json.tmp` (path = final path with `.tmp` appended), then `rename` over `endpoint.json`.
   If the rename fails, write `endpoint.json` directly. No lock file, no fsync.
3. **Cadence (0.5.11): once at farm start, and again only when the file is missing from the directory listing**
   (`ensureEndpointPresent`, checked every cycle but not within 15 s of the node's own last write).
   **0.5.10 and older rewrote it every cycle (~3 s).** Both behaviours are live on a mixed farm.
4. On clean stop (`PeerManager::stop`) the node **deletes** its `endpoint.json` (the directory and logs stay).

`timestamp_ms` is never compared with a clock. Its one use: a 0.5.11 reader treats "the listing changed but
`timestamp_ms` is the same as at my last read" as a stale SMB cache. **Every write must therefore carry a new
`timestamp_ms`**, or 0.5.11 readers delay accepting the change by up to 60 s (below).

Reader (`PeerManager::discoverPeers`, lines 284-425), per directory under `<farm>/nodes/` other than its own:

1. Find `endpoint.json` **by enumerating the directory** (`findEndpointEntry`), taking the entry's modification
   time (ms) and size from the directory entry. Never `exists()` or a by-path stat: on a Windows SMB client those
   can be answered from a cached handle of a file another client renamed over. No entry → skip the node.
2. If the peer is already known, a good read exists, and (mtime, size) are unchanged → do not open the file.
3. If inside a back-off window → skip.
4. Open and parse. Failure → back off 15 s (warning at most every 300 s per node).
5. `node_id` ≠ directory name → ignore.
6. Stale-cache rule: previous good read exists, the stamp changed, but `timestamp_ms` equals the previous value →
   back off 20 s, then 40 s; on the third such read accept the content.
7. Known peer → update `endpoint` only (and zero `failed_polls` if it changed). Unknown → create the peer with
   `is_alive = true`, `failed_polls = 0`, `last_seen_ms = 0`.

Pre-0.5.11 readers opened every peer's file every cycle and skipped unreadable files silently.

Removal (`pollPeers`, lines 569-587): a peer leaves the in-memory map only when it is **dead and its
`endpoint.json` is absent from the listing**. A dead peer whose file remains stays listed (shown offline) and is
polled every cycle forever. Nothing prunes `nodes/<id>/` directories automatically; the UI's cleanup removes
`nodes/<id>` recursively (`remove_peers`) and "forget peer" removes only `endpoint.json`
(`MonitorApp::forgetPeer`, `src/monitor/monitor_app.cpp:1888`). A live node whose file was removed rewrites it
within one cycle after the 15 s guard.

### 1.3 Restart signal — `nodes/<node_id>/restart`

An empty file; presence is the signal (`MonitorApp::writePeerRestartSignal`, line 1912 — written by any node
into another node's directory, only if that directory exists). The owner checks every cycle
(`PeerManager::checkRestartSignal`, lines 659-686): if the file's modification time is more than 60 s older than
the local file clock it is deleted and ignored; otherwise it is deleted and the app relaunches itself through
`mr-restart.exe --pid <pid> --exe <path> --minimized` (Windows only; on other systems the signal is consumed and
a warning logged — `MonitorApp::launchRestartSidecar`, line 2288).

### 1.4 The advertised address

`MonitorApp::advertisedIp` (`src/monitor/monitor_app.cpp:376-413`):

1. `ip_override` from the local config, if not empty, verbatim.
2. Otherwise, cached for 30 s: the host of the share the sync root lives on (`shareHostForPath`: a UNC host with
   `user@` stripped; a Windows mapped drive through `WNetGetConnectionW`; on macOS the `statfs` mount source,
   `//user@host/share` or `host:/export`), then the local IPv4 source address the OS would use to reach that
   host (`getLocalIpAddressTowards`: `getaddrinfo(host, "445")`, UDP `connect`, `getsockname`; rejects `0.0.0.0`
   and `127.0.0.1`; `src/core/net_utils.cpp:106-166`).
3. Otherwise the first non-loopback IPv4 address in adapter enumeration order (`getLocalIpAddress`), or
   `127.0.0.1`.

The address written to `endpoint.json` is computed **once at farm start**; `/api/status` and the UDP heartbeat
recompute it (30 s cache). IPv4 only everywhere.

### 1.5 HTTP liveness poll

`PeerManager::pollPeers` (lines 427-567). Each cycle, serially, for every known peer (alive or dead):

- Skip when the peer has UDP contact, its `hostname` is known, and the last successful poll was < 9 s ago.
- `GET http://<ip>:<port>/api/status`, **no Authorization header**, a new connection per call.
  Connect timeout = clamp(4 × smoothed RTT, 2000 ms, 8000 ms); read timeout = that + 1000 ms.
- 200 and the body parses as `PeerInfo` (§2.4) → the peer's record is **replaced** by the response; `is_alive =
  true`, `failed_polls = 0`, `last_seen_ms = now`; RTT EWMA = 0.7 × old + 0.3 × sample (first sample taken as is).
  With UDP contact, `ready_for_work` keeps the UDP value, and `agent_health` / `alert_reason` keep the UDP value
  when the response says `"ok"`.
- Anything else (no response, non-200, parse or type error) → `failed_polls += 1`.
- **Dead** when `failed_polls ≥ 3` and no UDP heartbeat was received in the last 10 s. A later successful poll
  or heartbeat makes the peer alive again.
- `has_udp_contact` is cleared after 15 s without a heartbeat.

Detection time for a silent death is about three cycles (≥ 9 s, longer with many slow peers). A clean stop is
seen at once through the UDP `bye`.

### 1.6 UDP multicast

`src/core/udp_notify.cpp`, `MonitorApp::sendUdpHeartbeat` / `handleUdpMessages`
(`src/monitor/monitor_app.cpp:2429-2496`).

| Item | Value |
|---|---|
| Group | `239.42.0.1` (hard-coded default argument; not configurable) |
| Port | config `udp_port`, default **4243** |
| Enable | config `udp_enabled`, default true; a failed start degrades to HTTP-only with a warning |
| Socket | IPv4 UDP, `SO_REUSEADDR`, bound to `0.0.0.0:<port>`, `IP_ADD_MEMBERSHIP` on the default interface (`INADDR_ANY`), non-blocking |
| TTL | `IP_MULTICAST_TTL = 1` |
| Loopback | left at the OS default (on); own packets are dropped by the `from` filter |
| Payload | one JSON object per datagram, UTF-8, `dump()`; **dropped by the sender if > 1400 bytes**; receive buffer 1500 bytes |
| Heartbeat interval | every 3000 ms (checked on the 50 ms main tick), plus one at farm start |
| Authentication | none |

Heartbeat (`"t":"hb"`):

| Key | Type | Sent | Receiver default | Meaning |
|---|---|---|---|---|
| `t` | string | always | `""` | `"hb"` |
| `from` | string | always | `""` | sender's node id; a packet whose `from` equals the receiver's id is dropped |
| `n` | string | always | `""` | node id (the key the receiver uses); empty or own id → ignored |
| `ip` | string | always | `""` | advertised IP — parsed, **never used** |
| `port` | integer | always | 8420 | HTTP port — parsed, **never used** |
| `st` | string | always | `"active"` | node state: `"active"` \| `"stopped"` |
| `rs` | string | always | `"idle"` | render state: `"idle"` \| `"rendering"` |
| `job` | string | only while rendering | `""` | current job id |
| `chunk` | string | only while rendering | `""` | chunk label, `"f42"` or `"f42-50"` (§4.3) |
| `pri` | integer | **not sent by 0.5.11**; older builds sent it | 100 | old election priority; stored, unused |
| `ah` | string | always | `"ok"` | agent health: `"ok"` \| `"needs_attention"` |
| `ar` | string | only when `ah` ≠ `"ok"` | `""` | alert reason: `"Agent needs_attention"` + `" (disconnected)"` when the agent is not connected |
| `rfw` | boolean | always | `true` | ready for work |

Goodbye, sent once in `MonitorApp::stopFarm` before the socket closes: `{"from":"<id>","n":"<id>","t":"bye"}`.

Receiver effect (`PeerManager::processUdpHeartbeat`, lines 94-145): for a peer **already in the map** set
`node_state, render_state, active_job, active_chunk, priority, agent_health, alert_reason, ready_for_work`,
`is_alive = true`, `failed_polls = 0`, `has_udp_contact = true`, `last_udp_contact_ms = now`. For an unknown node:
nothing (a heartbeat can never create a peer or change an endpoint — the 0.5.3 hardening). `bye`: for a known peer
`is_alive = false`, `has_udp_contact = false`.

Type strictness: `UdpNotify::poll` swallows malformed JSON and non-object JSON, but `handleUdpMessages` reads the
fields with `value()` **outside any try block on the main thread** — an object whose `t`, `n`, `port`, `st`,
`rs`, `job`, `chunk`, `pri`, `ah`, `ar` or `rfw` has the wrong JSON type throws an uncaught exception in a 0.5.x
node. A port must send exactly the types above (and should itself tolerate anything).

---

## 2. The HTTP mesh

### 2.1 Server

`src/core/http_server.cpp` (`HttpServer::start`, lines 987-1072).

- cpp-httplib `Server`, plain HTTP/1.1, **no TLS**. Bound to `0.0.0.0:<http_port>`; the port is the config value
  (default **8420**), no fallback or search: a failed bind sets a farm error and the node carries on without a
  server (it still writes its endpoint and polls others).
- The server starts with the farm (only when `sync_root` is an existing directory) and stops with it.
- Body limit 16 MiB (`set_payload_max_length`); larger → the library's 413.
- Library behaviour a port must reproduce where a peer can see it: an unmatched path or method answers **404
  with an empty body**; an exception escaping a handler answers **500**; routes are matched with full-match
  regular expressions in registration order; the path is percent-decoded before matching.
- Every JSON response is sent with `Content-Type: application/json`; a few error paths set only a status and
  no body (marked "empty" below).

### 2.2 Authentication

What shipped (0.5.3, `SECURITY.md`): **one shared bearer secret, nothing else.** There is no HMAC, no request
signing, no nonce, no timestamp, no replay protection and no clock assumption. (The "signing" in the branch name
`harden-auth-and-signing-0.5.3` is Windows binary code signing.) UDP is unauthenticated.

- Secret: 32 bytes from the system RNG (`BCryptGenRandom` / `/dev/urandom`) rendered as **64 lowercase hex
  characters** (`generateApiSecret`, `src/monitor/farm_init.cpp:31-48`). Stored in clear in `<farm>/farm.json`
  under `api_secret` (§6.2). Created by the first node that initialises the farm, or by any node that finds
  `farm.json` without a non-empty string `api_secret`.
- Distribution: every node reads `farm.json` from the share **once, at farm start**
  (`MonitorApp::startFarm`, lines 295-307). Rotation = stop all nodes, edit the file, restart. The value is used
  as an opaque string (not checked to be hex or 64 long).
- Client: header `Authorization: Bearer <secret>` on every call except the status poll; **no header at all**
  when the node has no secret (`authHeaders`, `src/core/http_server.h:41-46`).
- Server check (pre-routing handler, lines 1011-1037), in this order:
  1. path does not start with `/api/` → not protected;
  2. `GET` and path exactly `/api/status` → not protected;
  3. server has no secret → **503** `{"error":"auth_unavailable"}`;
  4. header `Authorization` missing, or its value not byte-equal to `"Bearer " + secret` (constant-time compare
     after a length check; scheme word case-sensitive, exactly one space) → **401** `{"error":"unauthorized"}`.
  The check runs before route matching and before the body is read, so an unknown `/api/` path without a valid
  token answers 401, with one answers 404 (empty).
- `secret_fingerprint` on `/api/status` = first 8 hex characters of the lowercase hex SHA-256 of the secret
  string's bytes; `""` when there is no secret (`src/core/sha256.cpp:113`). Display only.

### 2.3 Client behaviour

A new `httplib::Client(host, port)` — so a new TCP connection — per request; `Content-Type: application/json`
on every POST (an empty string body for commands without one); `DELETE` carries no body. Host and port come from
splitting the peer's `endpoint` at the **last** colon (`parseEndpoint`, `src/core/net_utils.cpp:178`).

| Caller (file:function) | Request | Connect / read timeout | On failure |
|---|---|---|---|
| `PeerManager::pollPeers` | `GET /api/status` | 2–8 s adaptive / +1 s | `failed_polls += 1` |
| `MonitorApp::refreshCachedJobs` (non-leader, every 2 s, on the main tick) | `GET /api/jobs` | 500 ms / 1 s | 5 s "leader contact cooldown" shared by the next two rows |
| `MonitorApp::getChunksForJob` (non-leader; UI, 3 s) | `GET /api/jobs/<id>` | 500 ms / 1 s | cooldown only when there is no response |
| `MonitorApp::refreshSuspendedNodes` (non-leader, every 3 s) | `GET /api/nodes/suspended` | 500 ms / 1 s | keep the previous list; cooldown only when no response |
| `MonitorApp::httpWorkerLoop` queue: every user command, `POST /api/jobs` from the UI, pushes to peers | `POST` / `DELETE` | 3 s / 10 s | **no retry**; callback gets `status == 200` or the status (0 = no response) |
| `MonitorApp::flushCompletionReports` | `POST /api/dispatch/complete` \| `/failed` | 2 s / 5 s | **kept and retried forever**, in order; after any failure the worker waits 5 s |
| `MonitorApp::flushFrameReports` (every 2 s) | `POST /api/dispatch/frame-complete` | 2 s / 5 s | the **whole batch** is re-queued (groups already delivered are sent again) |
| `DispatchManager::assignWork` (leader, detached thread) | `POST /api/dispatch/assign` | 500 ms / 2 s | chunk reverted to `pending` on the next cycle; no retry of the POST |
| `DispatchManager::tryRemoteAgentRestart` (leader) | `POST /api/agent/restart` | 2 s / 5 s | none; at most once per 60 s per node |
| `SubmissionWatcher::processSubmission` (non-leader) | `POST /api/jobs` | 3 s / 5 s | **the submission is lost** (the file is still moved to `processed/`) |
| `MonitorApp::getJobManifestJson` (non-leader) | `GET /api/jobs/<id>/manifest` | 3 s / 10 s | error text to the UI |

"Leader" for a client = the first peer in its map with `is_leader && is_alive`
(`MonitorApp::getLeaderEndpoint`, line 2187). With no such peer nothing is sent: reports stay queued, commands
fail at once. The `leader_endpoint` hint in a 503 (§2.5) is never used by the client.

A report is treated as delivered on **any 200**. If the 200 body contains the substring `"stale"` (with the
quotes) the report is dropped; for a frame batch the worker additionally abandons its current render when it is
for that job and epoch (§4.8).

### 2.4 `GET /api/status` — the peer record

Unauthenticated. Called by every node on every peer. 503 with an empty body is only possible before the app is
initialised. Response 200: one `PeerInfo` object (`src/core/peer_info.h:107-160`, filled by
`MonitorApp::buildLocalPeerInfo`, `src/monitor/monitor_app.cpp:2376-2425`).

| Key | Type | Read by peers | Value sent by 0.5.11 |
|---|---|---|---|
| `node_id` | string | yes | the node id |
| `hostname` | string | yes | machine name (`GetComputerNameW` / `gethostname`) |
| `os` | string | yes | `"windows"` \| `"macos"` \| `"linux"` — drives path translation at dispatch (§7.6) |
| `app_version` | string | yes (display only) | `"0.5.11"` |
| `gpu_name` | string | yes | first DXGI adapter / macOS "Chipset Model"; `""` on Linux |
| `cpu_cores` | integer | yes | logical cores |
| `ram_mb` | **non-negative** integer | yes | physical RAM in MiB |
| `node_state` | string | yes | `"active"` \| `"stopped"` |
| `render_state` | string | yes | `"idle"` \| `"rendering"` |
| `active_job` | string | yes | job id or `""` |
| `active_chunk` | string | yes | chunk label or `""` |
| `priority` | integer | yes | always `100` (legacy, unused) |
| `tags` | array of strings | yes | the node's tags, as typed (trimmed, **case-sensitive**) — election and job matching |
| `capabilities` | array of strings | yes | `["job_edit","edit_result","edit_epoch","edit_commands","chunk_stop"]`; absent on builds before the field existed |
| `endpoint` | string | yes — **overwrites the phonebook endpoint until the next change of `endpoint.json`** | `"<advertised ip>:<http_port>"` |
| `secret_fingerprint` | string | yes (display) | §2.2 |
| `agent_health` | string | yes | `"ok"` \| `"needs_attention"` |
| `alert_reason` | string | yes | `""` or `"Agent needs_attention"[ + " (disconnected)"]` |
| `ready_for_work` | boolean | yes | false from dispatch until the agent exits or reports idle |
| `is_local` | boolean | no | `true` |
| `is_alive` | boolean | no | `true` |
| `is_leader` | boolean | no | **always `false`** — a node never says whether it leads |
| `last_seen_ms` | integer | no | `0` |
| `has_udp_contact` | boolean | no | `false` |
| `failed_polls` | integer | no | `0` |

A response in which any "read by peers" key is present with the wrong type (for example `ram_mb: -1`,
`tags: null`) counts as a failed poll; three in a row and 0.5.x nodes declare the sender dead.

### 2.5 Endpoint table

Legend — **Auth**: B = bearer required. **Role**: L = answers only on the node that currently believes it is
leader; elsewhere it answers **503** `{"error":"not_leader"}` plus `"leader_endpoint":"<ip:port>"` when that node
knows a leader (`HttpServer::requireLeader`, line 46). **Caller**: who calls it in 0.5.11 (W = a non-leader node,
L = the leader, any = any node from its UI, — = nothing in the app; kept from an earlier UI or for tools).
Unless stated, success is **200 `{"status":"ok"}`**, a body that fails to parse or lacks a required key is
**400 `{"error":"<exception text>"}`**, and the command is applied without checking that the job exists.

| # | Method, path | Auth | Role | Caller | Request body | Responses / effect |
|---|---|---|---|---|---|---|
| 1 | `GET /api/status` | — | any | any → any | — | §2.4 |
| 2 | `GET /api/peers` | B | any | — | — | 200: array of `PeerInfo` for the peers this node knows (itself excluded), runtime keys filled |
| 3 | `POST /api/node/stop` | B | any | any → any | ignored | node becomes `stopped`, current render aborted (reported **failed**), persisted in its config |
| 4 | `POST /api/node/start` | B | any | any → any | ignored | node becomes `active` |
| 5 | `POST /api/node/restart` | B | any | any → any | ignored | relaunch through the restart helper; 500 `{"error":"restart_failed"}` when it cannot (always on non-Windows) |
| 6 | `POST /api/agent/restart` | B | any | L → W (automatic) | ignored | shut the agent down, wait 500 ms, spawn it; 500 `{"error":"agent_spawn_failed"}` |
| 7 | `POST /api/dispatch/assign` | B | any | L → W | `{"manifest":{…§4.1},"frame_start":int,"frame_end":int,"edit_epoch":int?}` | 409 `{"error":"busy"}` when a render is active; 409 `{"error":"stopped"}`; 400 `{"error":"invalid_job_id"}` (id not a plain path segment); else the chunk is queued. Missing `edit_epoch` = −1 |
| 8 | `POST /api/render/abort` | B | any | L → W | `{"job_id":string,"reason":string?}` (`reason` default `"job edited"`) | abandon the current render if it is for that job (no report), drop queued dispatches of that job |
| 9 | `POST /api/jobs` | B | L | W → L | `{"manifest":{…},"priority":int?}` (default 50) | 400 `{"error":"invalid_job_id"}`; else queued, inserted on the leader's next 2 s cycle. **A duplicate `job_id` is dropped silently** (insert fails after the 200) |
| 10 | `GET /api/jobs` | B | L | W → L (2 s) | — | 200: array of job summaries (§2.6), dispatch order, archived jobs omitted. May be an **empty body** before the leader's first refresh |
| 11 | `GET /api/jobs/<id>` | B | L | W → L | — | 200: job detail (§2.7); 404 `{"error":"not_found"}` (also while the leader's database is not ready) |
| 12 | `POST /api/jobs/<id>/pause` | B | L | W → L | ignored | job state `paused` |
| 13 | `POST /api/jobs/<id>/resume` | B | L | W → L | ignored | job state `active` |
| 14 | `POST /api/jobs/<id>/cancel` | B | L | W → L | ignored | job state `cancelled`; the leader aborts its own render of that job |
| 15 | `POST /api/jobs/<id>/archive` | B | L | W → L | ignored | job state `archived` |
| 16 | `DELETE /api/jobs/<id>` | B | L | W → L | — | 400 `{"error":"invalid_path"}`; else cancel, delete rows, remove `<farm>/jobs/<id>` recursively |
| 17 | `POST /api/jobs/<id>/move` | B | L | W → L | `{"target":string,"position":"before"\|"after"}` (default `"before"`; anything else means after) | 409 `{"error":"move_rejected"}` when either job is missing or both ids are equal (§4.5) |
| 18 | `POST /api/jobs/<id>/priority` | B | L | W → L | `{"priority":int}` | sets priority, resets the job's place to submission order |
| 19 | `POST /api/jobs/<id>/retry-failed` | B | L | W → L | ignored | failed chunks → pending; job → active if any |
| 20 | `POST /api/jobs/<id>/requeue` | B | L | W → L | ignored | every chunk reset, job → active |
| 21 | `GET /api/jobs/<id>/manifest` | B | L | W → L | — | 200 `{"manifest":{…}}`; 404 `{"error":"not_found"}`; 500 `{"error":…}` if the stored text is not JSON |
| 22 | `POST /api/jobs/<id>/edit` | B | L | W → L | `{"manifest":{…},"mode":"continue"\|"restart"\|"startover","priority":int}` (defaults `"continue"`, 50) | 200; 404 `{"error":"job_not_found"}`; 400 `{"error":"invalid_manifest","detail":string}` (unparseable, or `manifest.job_id` ≠ path id); 503 `{"error":"db_unavailable"}` (§4.8) |
| 23 | `POST /api/jobs/<id>/resubmit` | B | L | W → L | empty, or `{"incomplete_only":true}`, or `{"frame_start":int,"frame_end":int,"chunk_size":int}` | 200 `{"status":"ok","job_id":"<new id>"}`; 404 `{"error":"resubmit_failed"}` (§4.9) |
| 24 | `POST /api/chunks/reassign` | B | L | W → L | `{"chunk_id":int,"target_node":string?}` | §4.4; always 200 when the body parses |
| 25 | `POST /api/chunks/stop` | B | L | W → L | `{"chunk_id":int}` | §4.4; always 200 when the body parses |
| 26 | `POST /api/nodes/<id>/unsuspend` | B | L | W → L | ignored | clears the failure record of that node |
| 27 | `GET /api/nodes/suspended` | B | L | W → L (3 s) | — | 200: array of `{"node_id":string,"failure_count":int,"first_failure_ms":int,"last_failure_ms":int}` |
| 28 | `POST /api/dispatch/frame-complete` | B | L | W → L | `{"node_id":string?,"job_id":string,"frames":[int…],"edit_epoch":int?}` | 200 `{"status":"ok"}`, or 200 `{"status":"stale","stale":true}` when `edit_epoch ≥ 0` and differs from the job's |
| 29 | `POST /api/dispatch/complete` | B | L | W → L | `{"node_id":string?,"job_id":string,"frame_start":int,"frame_end":int,"elapsed_ms":int?,"exit_code":int?,"edit_epoch":int?}` | 200 ok, or 200 stale when no chunk matches (job, range, state `assigned`, `assigned_to` = `node_id` unless `node_id` is `""`, epoch equal unless < 0) |
| 30 | `POST /api/dispatch/failed` | B | L | W → L | `{"node_id":string?,"job_id":string,"frame_start":int,"frame_end":int,"error":string?,"edit_epoch":int?}` (`error` default `"Unknown"`) | as 29 |
| 31 | `GET /api/jobs/<id>/task-output` | B | any, farm running | — | — | 200: array of `{"node_id","range","timestamp_ms","path"}` for every `*.log` under `<farm>/jobs/<id>/stdout/*/` (§6.5), `path` in the answering node's form; `[]` when none; 400 `{"error":"invalid_path"}`; 503 empty when the farm is stopped |
| 32 | `GET /api/jobs/<id>/task-output/<node>/<file>` | B | any, farm running | — | — | 200 `text/plain` file content; 404 `{"error":"not_found"}`; 400 `invalid_path` |
| 33 | `GET /api/nodes/<id>/log?max_lines=N` | B | any, farm running | — | — | 200: array of strings — the last N (default 500) lines of that node's log files for the **answering** node's local yesterday + today (§6.4) |
| 34 | `POST /api/farm/scan-cleanup` | B | any, farm running | — | ignored | 200: `{"finished_jobs":[…],"archived_jobs":[…],"orphaned_dirs":[…],"stale_peers":[…],"stale_staging_dirs":[…],"failed_staging_copies":[…],"is_leader":bool}`, items `{"id","label","detail"}` |
| 35 | `POST /api/farm/cleanup` | B | any, farm running | — | `{"action":"archive"\|"delete_jobs"\|"delete_dirs"\|"remove_peers","ids":[string…]}` | 200 `{"status":"ok","count":int}` |
| 36 | `GET /api/config` | B | any | — | — | 200: the local config (§5.3) plus `node_id, hostname, cpu_cores, ram_mb, gpu_name, farm_running, farm_path, farm_error, is_leader` |
| 37 | `POST /api/config` | B | any | — | any subset of `sync_root, http_port, ip_override, udp_enabled, udp_port, tags, staging_enabled, show_notifications, path_mappings` | 200 `{"status":"ok","restarted":bool}`; a change to one of the first five restarts the farm |
| 38 | `POST /api/config/path-mappings` | B | any | — | array of path mappings (§5.3) | replaces the mappings |

Notes

- Reports 28–30 carry no retry counter, no sequence number and no idempotency key. Idempotence comes from the
  database guards: a second `complete` for a chunk no longer `assigned` is answered stale; frames are a set.
- `elapsed_ms` and `exit_code` are parsed and discarded; 0.5.11 always sends `0` and `0`. `error` is always sent
  as `""` by 0.5.11 (the real error text stays in the worker's log).
- Rows 31–38 have no caller in 0.5.11 (the Qt UI reads the share and its own process directly). They are
  reachable by anyone holding the secret and must exist with these shapes if external tooling is to keep working
  (open question 9).

### 2.6 Job summary (`GET /api/jobs` item)

Built in `MonitorApp::refreshCachedJobs` (`src/monitor/monitor_app.cpp:675-705`); parsed by the same function's
non-leader branch.

| Key | Type | Notes |
|---|---|---|
| `job_id` | string | |
| `template_id` | string | |
| `current_state` | string | `active` \| `paused` \| `cancelled` \| `completed` \| `failed` (never `archived` — omitted) |
| `priority` | int | |
| `submitted_at_ms` | int | |
| `submitted_by` | string | node id |
| `frame_start`, `frame_end`, `chunk_size` | int | from the manifest |
| `total_chunks` | int | **excludes** stopped chunks |
| `completed_chunks`, `failed_chunks`, `rendering_chunks`, `pending_chunks`, `stopped_chunks` | int | `rendering` = chunks in state `assigned` |
| `first_assigned_ms`, `last_completed_ms` | int | `MIN(assigned_at_ms)`, `MAX(completed_at_ms)`; 0 when none |
| `output_dir` | string | **only when the manifest has one** (never `null`) |
| `output_stem` | string | only when an output flag has a non-empty value: that value (canonical form, with padding token) |

Array order = dispatch order (§4.5). A worker **must** act on this list: a job it is rendering that is absent
from a successfully fetched list, or present with `current_state == "cancelled"`, is killed at once
(`MonitorApp::abortRenderIfJobGone`, line 810) — this is how cancel and delete reach every node. A failed fetch
changes nothing.

### 2.7 Job detail (`GET /api/jobs/<id>`)

`{"job_id","current_state","priority","submitted_at_ms","manifest":{…},"chunks":[…]}`, chunks ordered by
`frame_start`, each:

`{"id":int,"frame_start":int,"frame_end":int,"state":string,"assigned_to":string,"assigned_at_ms":int,`
`"completed_at_ms":int,"retry_count":int,"completed_frames":[int…],"failed_on":[string…]}` — `assigned_to` is
`""` and the times `0` where the database holds NULL. `id` is the leader's row id; it survives snapshots and takeovers and changes only when the chunks are rebuilt
(edit `startover`).

---

## 3. Leader election and failover

### 3.1 The algorithm

`PeerManager::recomputeLeader` (`src/monitor/peer_manager.cpp:590-657`), run by every node every cycle on its own
view. There are **no terms, epochs, votes, leases or leader heartbeats**; leadership is a pure function of the
set of nodes a node believes alive.

1. Candidates = this node (always) + every peer with `is_alive`. A `stopped` node is a full candidate.
2. Sort: has tag `leader` first; then lacks tag `noleader` first; then `node_id` ascending (byte-wise).
   Tags compare exactly (`"Leader"` is not `"leader"`). A node with both tags sorts in the `leader` group, last
   within it.
3. The first candidate is the leader. `isLeader` = it is this node. Peers get `is_leader` set accordingly.

Inputs and their staleness: a peer's tags come from its last successful `/api/status`; a peer discovered but not
yet polled has no tags; **this node's own tags are those captured at farm start** (`PeerManager::start`), while
its `/api/status` reports the live config — after a tag edit without a farm restart a node and its peers can
disagree about who leads until that node restarts its farm (tags are not among the settings that restart it).

A newly started node sees no peers in its first cycle only if `nodes/` is empty; otherwise peers are created
alive before the first election, so it normally does not lead for a moment. It can, however, lead briefly when
its peers' tags are not yet known.

### 3.2 What the leader owns

Only the leader has the SQLite database open (§5.1): jobs, chunks, their states, the queue order, edit epochs.
It also holds, **in memory only**: the per-node failure tracker (suspensions and cooldowns, §4.7), the
"recently dispatched" map (§4.6), the agent-restart cooldowns. Everything else (peer table, templates, logs) is
per node or on the share.

### 3.3 Followers and the snapshot

Followers hold no copy of the job state; they poll the leader (§2.3). The durable hand-over medium is one file,
`<farm>/state/snapshot.db`:

- Written by the leader every 30 s, measured from the dispatch manager's initialisation
  (`DispatchManager::doSnapshot`, `src/monitor/dispatch_manager.cpp:820-850`), skipped while a previous one is
  still running: SQLite online backup of the live database into **`<sync_root>/snapshot_tmp.db`** (the parent of
  the farm directory, on the share, despite the variable's name), then `copy_file` with overwrite onto
  `state/snapshot.db`, then removal of the temporary file. Not atomic: a reader can see a partial file.
- There is no snapshot on leadership loss or on shutdown. **Up to 30 s of state (including whole submissions,
  completions and edits) is lost at every failover.**

### 3.4 Takeover

`MonitorApp::update` detects the edge on the 50 ms tick; `onBecomeLeader` (`src/monitor/monitor_app.cpp:452-485`)
runs on a background thread:

1. If `state/snapshot.db` exists: copy it over the local `minrender.db` (overwrite) and open it
   (`DatabaseManager::restoreFrom`). The schema statements and migrations run on open (§5.1).
2. If there is no snapshot, or the copy fails: open the local `minrender.db` as it is — **whatever this node last
   had as leader, however old** — creating it if absent.
3. Dispatch starts when the open has succeeded (`m_leaderDbReady`). The first cycle then runs the normal steps
   (§4.6): chunks `assigned` to nodes now dead go back to `pending`; chunks assigned to nodes that are alive
   **stay assigned** and their workers' reports are accepted by the new leader (matching is by job id, frame
   range, node id and epoch — never by leader identity).
4. Not restored: the failure tracker (all suspensions lifted), the dispatched map, reports the old leader had
   accepted but not yet applied or snapshotted (workers were already told 200 and do not resend).

`onLoseLeadership`: close the database. The local file is left behind.

### 3.5 Split brain

Nothing prevents it. Two nodes that each consider the other dead both lead, both restore the same snapshot, both
dispatch (possibly the same chunks to different workers) and both overwrite `snapshot.db` every 30 s; the last
writer wins. Workers follow whichever leader their own view elects. The only mitigations in the code: a report is
accepted only from the node the answering leader assigned that chunk to, and a UDP heartbeat within 10 s vetoes
the "dead" verdict. When the partition heals the lower-ranked leader closes its database and its changes since
the last snapshot it won are gone.

### 3.6 Versions

- **There is no version negotiation and no minimum-version check anywhere.** `app_version` is displayed only.
  A node of any version that writes a valid `endpoint.json` and answers `/api/status` takes part in the election
  with full rights.
- `PROTOCOL_VERSION` (2) appears in exactly two places: the farm directory name `MinRender-v2`, and
  `farm.json`'s `protocol_version`, which is written and never read. A build with another protocol version would
  simply use another directory — a separate farm on the same share.
- Feature detection is by the `capabilities` array of the **current leader's** status
  (`MonitorApp::leaderSupports`, line 2203; absent or empty = none). Used only to enable UI: `job_edit` (edit
  dialog), `chunk_stop` (stop chunk). The others are advertised and not queried: `edit_result`, `edit_epoch`,
  `edit_commands`.
- Otherwise compatibility is by tolerance: unknown JSON keys are ignored; missing keys take defaults
  (`edit_epoch` missing = −1 = "legacy, skip the epoch check"); a missing route shows as a 404 with an empty
  body, which callers tell apart from a 404 carrying `job_not_found` / `not_found`.
- Shared files are protected against downgrade in one place only: templates and plug-ins on the share are
  refreshed only by a node whose `APP_VERSION` is numerically greater than `farm.json`'s `last_example_update`
  (§6.2). The SQLite schema has no version; migrations are additive and re-run on every open (§5.1).

---

## 4. Job model and scheduling

### 4.1 The job manifest

`JobManifest` (`src/core/job_types.h:128-153, 467-515`). It is stored as text in the database, sent whole in
`assign`, `POST /api/jobs`, `edit`, the edit command file, and returned by `manifest` and job detail. All keys
are written; on read every key is optional.

| Key | Type | Default on read | Meaning |
|---|---|---|---|
| `_version` | int | 1 | always 1 |
| `job_id` | string | `""` | the slug (§4.2); primary key; directory name under `<farm>/jobs/` |
| `template_id` | string | `""` | provenance only |
| `submitted_by` | string | `""` | node id of the submitting node |
| `submitted_os` | string | `""` | `"windows"` \| `"macos"` \| `"linux"` — the form the paths are in (§7.6); macOS submitters write `"windows"` |
| `submitted_at_ms` | int | 0 | submitter's clock; also the initial queue position |
| `cmd` | object string → string | `{}` | executable per OS, keys `"windows"`, `"linux"`, `"macos"` (only non-empty ones, plus the submitter's OS) |
| `flags` | array | `[]` | ordered: `{"flag":string,"value":string\|null,"is_output":true?}`; `is_output` is written only when true |
| `frame_start`, `frame_end` | int | 1, 250 | inclusive |
| `chunk_size` | int | 1 | frames per chunk |
| `max_retries` | int | 3 | see §4.4 |
| `timeout_seconds` | int \| **null** | none | per chunk process timeout |
| `output_dir` | string \| **null** | none | parent directory of the output flag's value |
| `progress` | object | empty | `{"patterns":[…],"error_patterns":[…],"completion_pattern":{…}\|null}` (§7.4) |
| `output_detection` | object | defaults | `{"stdout_regex":string\|null,"path_group":int,"validation":string,"info":string}` |
| `process` | object | — | `{"working_dir":string\|null}`; a `kill_method` key from old manifests is ignored |
| `environment` | object string → string | `{}` | extra environment for the render process |
| `tags_required` | array of strings | `[]` | a node needs all of them |

The priority is **not** in the manifest; it travels beside it and lives in its own column.

### 4.2 Job ids

`TemplateManager::generateSlug` (`src/monitor/template_manager.cpp:359-422`), from the job name: each byte
lower-cased with the C locale; ASCII letters, digits and `_` kept, every other byte (each byte of a multi-byte
character included) becomes `-`; runs of `-` collapsed; leading and trailing `-` trimmed; cut to 64 bytes; empty →
submission refused. Uniqueness is checked **against the existence of `<farm>/jobs/<slug>` on the share** (a
directory that only appears once a node has written render output for the job), trying `<slug>-2` … `<slug>-99`.
Two jobs of the same name submitted before the first has rendered get the same id; the second is then dropped by
the leader's primary key (§2.5 row 9).

Resubmitted jobs (§4.9): strip a trailing `-v<digits>`, then the first free `<base>-v2` … `-v999` **in the
database**.

Every id that becomes a path is checked with `isSafePathSegment` (`src/core/platform.cpp:312`): not empty, not
`.` or `..`, no `/`, `\` or `:`.

### 4.3 Chunks and frames

- Chunking (`computeChunks`, `src/core/job_types.h:206`): `chunk_size ≤ 0` or `frame_start > frame_end` → no
  chunks; else `[f, min(f + chunk_size − 1, frame_end)]` for `f = frame_start, frame_start + chunk_size, …`.
  A chunk is the unit of dispatch: one process renders one chunk; one node renders one chunk at a time.
- Range string (`ChunkRange::rangeStr`): both ends zero-padded to width 6, joined by `-`: `000001-000010`.
  Used in log file names and the staging path.
- Chunk label (`RenderCoordinator::currentChunkLabel`): `f42`, or `f42-50` — in heartbeats and status.
- There is no task or frame table. Frames exist only as the `completed_frames` list of their chunk.
- A chunk on the wire is identified by `(job_id, frame_start, frame_end)`; the row `id` is used only by the two
  chunk commands.

### 4.4 Chunk states

`pending`, `assigned`, `completed`, `failed`, `stopped` (`chunks.state`). All transitions are SQL in
`src/monitor/database_manager.cpp`.

| From | To | Trigger | Guard / side effects |
|---|---|---|---|
| — | `pending` | job inserted; edit `startover` (all rows deleted and rebuilt) | |
| `pending` | `assigned` | `assignChunk` (leader's dispatch) | sets `assigned_to`, `assigned_at_ms` (leader clock), `edit_epoch` = the job's current epoch |
| `assigned` | `completed` | `completeChunk` (report, or the leader's own render) | job, range match; `assigned_to` = reporter unless reporter is `""`; epoch equal unless < 0; sets `completed_at_ms` (leader clock at processing) |
| `assigned` | `pending` | `failChunk` with `retry_count + 1 < max_retries` | same guards (`isChunkCurrent`); reporter appended to `failed_on` (once); `retry_count += 1`; `assigned_to`, `assigned_at_ms` → NULL |
| `assigned` | `failed` | `failChunk` with `retry_count + 1 ≥ max_retries` | as above but `assigned_to` and `assigned_at_ms` are kept |
| `assigned` | `pending` | `revertChunkToPending` — the assign POST failed, or the leader could not parse the manifest for itself | **no state guard**: the row with that job and range is set to `pending` whatever its state |
| `assigned` | `pending` | `reassignDeadWorkerChunks` — the assignee is dead, or alive with `agent_health == "needs_attention"`; every 2 s cycle | across all jobs, whatever the job's state; no retry counted |
| `assigned` | `pending` | edit `restart` (`resetAssignedChunks`) | |
| `failed` | `pending` | `retryFailedChunks` | `retry_count = 0`, `completed_frames = '[]'`, **`failed_on` kept**; job → `active` |
| any | `pending` | requeue (`resetAllChunks`) | clears assignment, times, `retry_count`, `completed_frames`, `failed_on` |
| `assigned` \| `failed` \| `stopped` | `pending` | `POST /api/chunks/reassign` with no / empty `target_node` | clears assignment; a job in `completed` or `failed` → `active` |
| `assigned` \| `failed` \| `stopped` | `assigned` | `reassign` with a `target_node` | sets `assigned_to`, `assigned_at_ms`; **nothing is sent to that node and the epoch is not restamped**; no UI path uses this in 0.5.11 |
| `pending` \| `assigned` | `stopped` | `POST /api/chunks/stop` (`stopChunk`) | clears assignment; if it was assigned, the assignee gets `POST /api/render/abort` `{"job_id","reason":"chunk stopped"}` (or the leader abandons its own render) |
| any | `completed` | `markChunksCompleted` — the already-finished ranges of a "resubmit incomplete" job | sets `completed_at_ms` |

`max_retries` semantics: a chunk fails for good on its `max_retries`-th failure (`max_retries = 3` → two
re-queues, third failure is final; `max_retries ≤ 1` → the first failure is final). The value is read from the
job's stored manifest at the time of the failure (default 3 if unreadable).

`failed_on` is a per-chunk blacklist: a node listed there is never offered that chunk again (§4.6).

Per-frame completion (`addCompletedFrames` / `addCompletedFramesBatch`): the frame is added to the sorted,
duplicate-free `completed_frames` of the chunk whose range contains it, **whatever that chunk's state and without
checking who reports**; only the job's edit epoch is checked. `completed_frames` survives a failure / re-queue of
the chunk (it is cleared only by retry-failed, requeue and startover).

### 4.5 Job states, priority and queue order

`jobs.current_state`: `active`, `paused`, `cancelled`, `completed`, `failed`, `archived`.

| Trigger | Result | Notes |
|---|---|---|
| submit | `active` | |
| pause / resume / cancel / archive | `paused` / `active` / `cancelled` / `archived` | **unconditional** `UPDATE` — no check of the current state |
| every 2 s cycle, for `active` jobs only (`getJobCompletionState`) | `failed` if no chunk is outside {`completed`,`failed`,`stopped`} and at least one is `failed`; `completed` if none is failed | a job with no chunks completes at once; stopped chunks neither hold a job open nor fail it |
| retry-failed, requeue, edit `restart` / `startover`, reassign-to-pending on a finished job | `active` | |
| delete | row removed, chunks cascade | plus `<farm>/jobs/<id>` removed (twice, 250 ms apart, on error) |

Only `active` jobs are dispatched. **Pause** stops new dispatch; chunks in flight finish and are accepted.
**Cancel** stops dispatch and kills renders: the node that issues it aborts its own render of the job at once;
every other node notices on its next successful jobs refresh (≤ 2 s) and aborts (§2.6). Cancel does not touch
chunk rows: a chunk in flight stays `assigned` until its aborting node's report arrives — and that report is a
**failure** (see §4.10). Archive only hides the job from the list (and from `GET /api/jobs`).

Priority: integer, **lower renders sooner**, default 50, no range check. Changing it
(`updateJobPriority`) also sets `sort_key = submitted_at_ms`.

Queue order (the one ordering used for dispatch and for the list):
`ORDER BY priority ASC, COALESCE(sort_key, submitted_at_ms) ASC, submitted_at_ms ASC`.

Move (`DatabaseManager::moveJob`, line 282), in one transaction: both jobs must exist and differ; if priorities
differ the moved job **takes the target's priority**; all jobs of that priority are listed in current order, the
moved one is spliced before / after the target, and the whole group is renumbered `sort_key = (index + 1) × 1024`.
Consequence: once a group has been renumbered its keys are small integers, so every job submitted later
(`sort_key` = a 13-digit timestamp) sorts after them, and a job whose priority is changed back into the group
re-enters at the end.

### 4.6 The leader's dispatch cycle

`DispatchManager::update` (`src/monitor/dispatch_manager.cpp:28`), every 2000 ms on the main thread, only while
this node is leader and its database is open:

1. Insert queued submissions (`submitJob`: job row `active`, chunks from `computeChunks`).
2. Apply queued completions, then failures (with failure tracking, §4.7), then assignment reverts, then frame
   completions grouped by (job, epoch) with the epoch re-checked.
3. Dead-worker reclaim (§4.4) and remote agent restart (`POST /api/agent/restart`, once per 60 s per node) for
   peers with `agent_health == "needs_attention"`.
4. Job completion check (§4.5).
5. Assign work. Candidate nodes = peers in ascending `node_id` order, then this node last. A node is skipped if
   any of: not alive; `node_state == "stopped"`; `render_state == "rendering"`; it is in the "recently
   dispatched" map; suspended; in failure cooldown; `agent_health != "ok"`; `ready_for_work == false`.
   For an eligible node: walk `active` jobs having a pending chunk in queue order; skip a job unless the node's
   `tags` contain every entry of the manifest's `tags_required` (exact strings); take the job's first pending
   chunk by `frame_start` whose `failed_on` does not contain the node. **At most one chunk per node per cycle.**
   Mark it assigned in the database first, then deliver: to itself directly (no path translation), to a peer by
   `POST /api/dispatch/assign` on a detached thread with the manifest translated for the peer's OS (§7.6).
   The node enters the "recently dispatched" map; it leaves when its status shows `rendering`, when it is dead,
   or after 15 s.
6. Snapshot, if 30 s have passed (§3.3).

There are no pools, groups, per-job node limits or capability checks other than `tags_required ⊆ tags`.

### 4.7 Failure tracking and suspension (leader memory only)

`NodeFailureTracker` (`src/monitor/node_failure_tracker.h`), fed by every **remote** failure report that passed
the HTTP stale check (the leader's own failed chunks are not counted): a window starting at a node's first
failure; a failure more than 300 000 ms after the window start restarts the count; **5 failures in the window →
suspended** until someone unsuspends it (endpoint 26; any node's UI). A not-suspended node that failed less than
30 000 ms ago is in cooldown and gets no work. Suspension is published only through endpoint 27 and is lost at
leader change.

### 4.8 Edits and the edit epoch

`MonitorApp::applyEditLocally` (`src/monitor/monitor_app.cpp:1487`), on the leader: validate (manifest parses,
its `job_id` equals the target, the job exists); set priority (which also resets `sort_key`); replace the stored
manifest; then by mode:

- `continue`: nothing more — chunks in flight finish with the old manifest; later chunks use the new one. If the
  frame range or chunk size changed, the chunk rows are **not** rebuilt.
- `restart`: remember which nodes hold assigned chunks; `jobs.edit_epoch += 1`; assigned chunks → `pending`;
  job → `active`; `POST /api/render/abort` `{"job_id"}` to each of those nodes (not retried); abandon the
  leader's own render of the job.
- `startover`: the same, but all chunk rows are deleted and rebuilt from the new range and chunk size.

The epoch fence: the leader stamps the job's epoch on a chunk at assignment and sends it as `edit_epoch`; the
worker echoes it in all three reports (omitting the key when it received none). A report with `edit_epoch ≥ 0`
that does not match is answered `200 {"status":"stale","stale":true}`; the worker drops it and, for frame
reports of its current render, abandons that render. A report with no epoch (an old worker) skips the epoch test
and is held only by the state / assignee guards.

Fallback when the leader does not answer at all (status 0; never on an HTTP error): the editing node writes a
command file to the share (`MonitorApp::writeEditCommand`, line 1657) — §6.6. The leader applies it later.

### 4.9 Resubmission (leader)

`DispatchManager::resubmitJob` and `resubmitIncomplete`: copy the stored manifest, new id (§4.2), new
`submitted_at_ms` (leader clock), same priority; either the same range, or an overridden
`frame_start / frame_end / chunk_size`, or ("incomplete only") the same chunk ranges with the source's completed
ranges marked completed at once. Returns `""` when the source is missing, has no chunks, or has nothing
incomplete.

### 4.10 The worker side

`RenderCoordinator` (`src/monitor/render_coordinator.cpp`) and `MonitorApp`:

- An accepted assignment is queued (FIFO, unbounded); `/api/dispatch/assign` answers `busy` only while a render
  is *active*, so a second assignment arriving before the first has started, or while a finished chunk waits for
  its staged files to be delivered, is accepted and runs afterwards.
- Start: if the node is stopped the chunk is **abandoned** (no report at all — the leader's row stays `assigned`
  until an edit, a manual action or the node's death). Otherwise the agent process is spawned if not connected
  (failure to spawn, or no connection within 10 s → report `failed`; a spawn failure also sets `agent_health` to
  `needs_attention`), and the task is sent (Appendix A).
- The command: executable = `manifest.cmd[<this OS>]` (empty string if absent); arguments = for each flag in
  order, the `flag` text if not empty, then the `value` if not null — each with the runtime tokens replaced
  (§7.3); no shell.
- Outcomes and what the leader hears:

| Event on the worker | Report |
|---|---|
| process exits 0 (and staged files delivered, §4.11) | `complete` |
| process exits non-zero, cannot be spawned, times out | `failed` |
| agent process dies, or its connection is lost for 30 s | `failed` |
| **job cancelled or deleted** (seen locally or in the jobs list) | `failed` |
| **node stopped** (locally or by `POST /api/node/stop`) while rendering | `failed` |
| app shutdown while rendering | `failed` is queued but the sender thread has already stopped — nothing is sent |
| `POST /api/render/abort`, or a frame report answered stale | none ("abandoned") |

  So cancelling a job or stopping a node costs each affected chunk a retry, blacklists the node for that chunk,
  and counts toward the node's suspension (five cancels of rendering jobs within five minutes suspend a node).
  This is depended on in the sense that the leader's bookkeeping assumes it; a port that reported nothing instead
  would leave the chunk `assigned` on a 0.5.x leader until the worker died.
- Kill: abort, timeout and shutdown kill the **whole process tree** at once (Windows Job Object with
  kill-on-close; elsewhere a new process group and `SIGKILL` to the group — `mr-agent/src/kill.rs`). There is no
  graceful signal and no configurable method.
- Per-frame progress: each time the template's `completion_pattern` matches a line of the process's stdout **or
  stderr**, the next frame number is reported: `frame_start + (matches so far)`, while ≤ `frame_end`
  (`mr-agent/src/executor.rs`). The number is a count, not parsed from the output. Percent progress from
  `progress.patterns` stays on the worker (shown locally; never sent to the leader).
- Exit code is the only success test. `progress.error_patterns` and `output_detection.validation`
  (`exists_nonzero`) are carried in every manifest and **not acted on** (§11.3).
- The render process inherits the node's environment plus the manifest's `environment` plus
  `PYTHONUNBUFFERED=1`; working directory = `process.working_dir` with tokens replaced, if not empty.
- Render output lines (stdout, and stderr prefixed `[stderr] `) are appended to the chunk's log on the share
  (§6.5) as they arrive (batches of 50 lines or 5 s).

### 4.11 Local staging

Per node, config `staging_enabled` (default false; **read at farm start**). When on and the manifest has a
non-empty `output_dir` (`RenderCoordinator::dispatchChunk`, line 453):

1. Create `<data dir>/staging/<job_id>/<rangeStr>/` (§5.2). If that fails, render straight to the output.
2. In the argument list, the value of every flag with `is_output` is replaced by
   `<staging dir>/<file name of the original value>` (the last path component, which carries the padding token).
3. On success, create `output_dir` and copy **every regular file directly inside the staging directory** (not
   recursive) into it with overwrite, then delete the staging directory. Only then is `complete` reported.
4. If the output directory cannot be reached or any copy fails: keep the files, free the node for other work,
   retry the whole copy every 30 s; after 600 s report `failed` and leave the files in staging. A job deleted
   meanwhile stops the retries and reports nothing.
5. Frame-complete reports are sent as frames finish in staging (the UI shows them as "rendered, not yet copied").

Without staging, the node creates `output_dir` before the render (best effort). The leader never creates output
directories.

### 4.12 Node enable / disable

`node_state` `active` / `stopped`: toggled locally, or remotely by endpoints 3 / 4; stored in the local config
(`node_stopped`); shown in status and heartbeat. A stopped node is not given work, refuses assignments (409),
aborts its current render (reported failed), still takes part in the election and can lead.

---

## 5. Persistence

### 5.1 The SQLite database

One database, opened **only on the node that is currently leader**: `<data dir>/minrender.db`
(`MonitorApp::onBecomeLeader`). Opened read-write / create with `PRAGMA journal_mode=WAL`,
`PRAGMA foreign_keys=ON`, busy timeout 5000 ms (`DatabaseManager::open`, `src/monitor/database_manager.cpp:10`),
so `minrender.db-wal` and `minrender.db-shm` appear beside it. The shared copy is `<farm>/state/snapshot.db`
(§3.3), a plain SQLite file made by the backup API.

Truth: the leader's local file. The snapshot is at most 30 s behind. No other node has job state.
Write cadence: every change is its own statement or small transaction as it happens; there is no batching
beyond the 2 s cycle that feeds it.

Schema, exactly as created (`createSchema`, lines 47-119). There is **no `user_version`, no schema-version
table and no other table.**

```sql
CREATE TABLE IF NOT EXISTS jobs (
    job_id TEXT PRIMARY KEY,
    manifest_json TEXT NOT NULL,
    current_state TEXT NOT NULL DEFAULT 'active',
    priority INTEGER NOT NULL DEFAULT 50,
    submitted_at_ms INTEGER NOT NULL,
    edit_epoch INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL REFERENCES jobs(job_id) ON DELETE CASCADE,
    frame_start INTEGER NOT NULL,
    frame_end INTEGER NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    assigned_to TEXT,
    assigned_at_ms INTEGER,
    completed_at_ms INTEGER,
    retry_count INTEGER NOT NULL DEFAULT 0,
    completed_frames TEXT NOT NULL DEFAULT '[]',
    failed_on TEXT NOT NULL DEFAULT '[]',
    edit_epoch INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_chunks_job ON chunks(job_id);
CREATE INDEX IF NOT EXISTS idx_chunks_state ON chunks(state);
```

Migrations, run on **every** open, each in its own try / ignore-error:

```sql
ALTER TABLE chunks ADD COLUMN completed_frames TEXT NOT NULL DEFAULT '[]';
ALTER TABLE chunks ADD COLUMN failed_on TEXT NOT NULL DEFAULT '[]';
ALTER TABLE jobs ADD COLUMN sort_key INTEGER;
UPDATE jobs SET sort_key = submitted_at_ms WHERE sort_key IS NULL;   -- always runs
ALTER TABLE jobs ADD COLUMN edit_epoch INTEGER NOT NULL DEFAULT 0;
ALTER TABLE chunks ADD COLUMN edit_epoch INTEGER NOT NULL DEFAULT 0;
```

So `jobs.sort_key INTEGER` (nullable) is always an added column, never in the `CREATE`. A database written by an
older leader may lack `sort_key`, `edit_epoch`, `completed_frames` or `failed_on` until a newer leader opens it;
a database written by a newer leader has columns an older leader does not know and does not touch (its `INSERT`s
name their columns, so defaults apply; an old leader inserts jobs with `sort_key` NULL, which `COALESCE` and the
`UPDATE` above cover).

Column contents: `manifest_json` = the manifest as compact JSON text; `completed_frames` = JSON array of
integers, ascending; `failed_on` = JSON array of node-id strings, in order of failure; `assigned_to` = node id
or NULL; times in ms or NULL; `sqlite_sequence` exists because of `AUTOINCREMENT` (chunk ids are never reused).

A port that becomes leader must be able to open a snapshot written by any 0.5.x leader, apply the same additive
migrations, and write a snapshot that any 0.5.x leader can open with the statements above — which means: **no
new NOT NULL column without a default, no renamed or dropped column, no new table that the old code's `DELETE
FROM jobs` cascade would need to know about, and the same value vocabulary.**

### 5.2 The node's data directory

`getAppDataDir` (`src/core/platform.cpp:89-148`), created on first use:

| OS | Directory |
|---|---|
| Windows | `%USERPROFILE%\minRender` (since 0.5.8). On first run, if it has no `config.json` and `%LOCALAPPDATA%\MinRender\config.json` exists, everything there except `staging\` is **copied** (never moved, existing files kept) |
| macOS | `<home>/Library/Application Support/MinRender` |
| Linux | `$XDG_DATA_HOME/MinRender`, else `<home>/.local/share/MinRender` |
| fallback | `<current directory>/MinRender_data` |

| Entry | Format | Written by | Notes |
|---|---|---|---|
| `config.json` | JSON, `dump(2)` | the app, on every settings save, node start / stop, and at exit | §5.3. **Read by the DCC plug-ins** to find the farm |
| `node_id.txt` | 12 characters, no newline | once | §1.1 |
| `minrender.db`, `-wal`, `-shm` | SQLite | the app while leader | §5.1; left behind when leadership is lost; overwritten by the snapshot at the next takeover |
| `submissions/*.json` | §8.2 | DCC plug-ins | polled every 3 s |
| `submissions/processed/` | the same files | the app (rename) | never pruned |
| `staging/<job_id>/<rangeStr>/` | rendered files | the render process | §4.11; removed after delivery; leftovers listed by the cleanup scan |
| `mr-agent.log` | text | the agent | Windows: in this directory. **Other systems: `<home>/.local/share/MinRender/mr-agent.log`** (so not in the data directory on macOS). Deleted at agent start when over 2 MiB |
| `restart.bat` | — | old versions | deleted at start if present |

Windows also keeps: the pre-0.5.8 directory `%LOCALAPPDATA%\MinRender` (its `submissions\` is still polled while
it exists; processed files go to the **new** directory's `processed\`); the registry key
`HKCU\Software\MinRender\Node` with `RenderState` (`REG_SZ` `idle` \| `rendering`), `NodeState` (`REG_SZ` `active`
\| `stopped`), `Pid` (`REG_DWORD`, 0 = clean exit), `UpdatedMs` (`REG_QWORD`), written on the first tick, on every
change and at exit (`publishNodeState`, `src/core/platform.cpp:348`) — a contract with the external RNDR
companion script (`tools/rndr-companion/`), which may also poll `GET http://127.0.0.1:<http_port>/api/status`.
UI preferences (window layout, column widths) are Qt `Settings` under organisation and application name
`MinRender` — not needed by a port except to avoid clobbering them.

Local IPC names (Appendix A): Windows pipe `\\.\pipe\MinRenderAgent_<node_id>`; elsewhere the socket
`$TMPDIR/minrender-agent-<node_id>.sock` (or `/tmp/…`). Single-instance guard name: `MinRenderMonitor`.

### 5.3 `config.json`

`Config` (`src/core/config.h:34-105`). Every key is optional on read; unknown keys are ignored **and dropped at
the next save** (the file is rewritten from the struct). A port that must not lose settings has to read all of
these and, if it adds keys, expect an old build run on the same machine to drop them.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `sync_root` | string | `""` | the shared folder (native path of this machine). Empty or missing directory → the farm does not start |
| `http_port` | int (u16) | 8420 | |
| `ip_override` | string | `""` | §1.4 |
| `tags` | array of strings | `[]` | §3.1, §4.6 |
| `auto_start_agent` | bool | true | carried, unused |
| `udp_enabled` | bool | true | |
| `udp_port` | int (u16) | 4243 | |
| `show_notifications` | bool | true | |
| `staging_enabled` | bool | false | §4.11 |
| `node_stopped` | bool | false | §4.12 |
| `path_mappings` | array | `[]` | each `{"win":string,"mac":string,"lin":string,"enabled":bool,"label":string}`; missing keys default to `""`, `""`, `""`, true, `""` |

Keys from old versions that may still be present and must be ignored: `rndr_dual_mode`, `rndr_dcc_gate`,
`rndr_block_processes`. The plug-ins read only `sync_root`.

---

## 6. The shared folder

`<sync_root>` is whatever each node's config says (a different native path per machine).
`<farm> = <sync_root>/MinRender-v2`. Created by `MonitorApp::startFarm` (lines 268-284) and `FarmInit::init`.
**No lock files and no advisory locks are used anywhere on the share.** Atomicity, where it exists, is "write a
temporary file beside the target, then rename".

### 6.1 Layout

```
<sync_root>/
  snapshot_tmp.db                      leader, transient (backup in progress)            §3.3
  MinRender-v2/
    farm.json                          first node; later any node (version stamp)        §6.2
    nodes/<node_id>/
      endpoint.json                    its own node (tmp + rename)                       §1.2
      endpoint.json.tmp                transient
      restart                          any node; consumed by the owner                   §1.3
      monitor-YYYY-MM-DD.log           its own node (append)                             §6.4
    jobs/<job_id>/stdout/<node_id>/
      <rangeStr>_<ms>.log              the rendering node (append)                       §6.5
    state/snapshot.db                  the leader, every 30 s (copy over)                §3.3
    commands/<command_id>.json         a non-leader that could not reach the leader      §6.6
    commands/<command_id>.tmp          transient
    commands/processed/<same name>     the leader (rename)                               §6.6
    templates/*.json                   users (by hand)                                   §7
    templates/examples/*.json          a node newer than the stamp (copy over, prune)    §6.3
    templates/plugins/*.json           a node newer than the stamp (copy over, prune)    §6.3
    plugins/<app>/<file>               a node newer than the stamp (copy over)           §6.3
```

Rendered frames are not under `<farm>`: they go wherever each job's `output_dir` points.

### 6.2 `farm.json`

Written directly (no temporary file) with `dump(2)` (`FarmInit::init`, `src/monitor/farm_init.cpp:266-396`).

| Key | Type | Meaning |
|---|---|---|
| `_version` | int | 1 |
| `protocol_version` | int | 2; never read |
| `created_by` | string | node id |
| `created_at_ms` | int | |
| `last_example_update` | string | the `x.y.z` version of the app that last refreshed templates and plug-ins on the share; `""` if that first copy failed |
| `api_secret` | string | §2.2 |

Rules at every farm start:

- File absent → generate a secret (failure to get random bytes aborts with no file written), copy templates and
  plug-ins, write the file.
- File present → parse it. If `api_secret` is missing, not a string or empty: generate one and rewrite. Compare
  this build's version with `last_example_update` numerically part by part (missing or non-numeric parts = 0):
  **greater** → refresh the share's templates and plug-ins and, only if every copy succeeded, set the stamp to
  this version and rewrite; **less** → leave everything. A rewrite re-serialises the parsed object, so unknown
  keys survive.
- The secret is then read from the file as a separate step; a parse failure leaves the node with no secret
  (its API answers 503 to everything but status, and it sends no Authorization header).

### 6.3 Bundled content copied to the share

From the app's bundled `resources/` (`refreshFarmContent`): `resources/templates/*.json` →
`templates/examples/`; `resources/templates/plugins/*.json` → `templates/plugins/`; in both destinations every
`*.json` not in the bundle is **deleted**; `resources/plugins/<app>/*` → `plugins/<app>/` (no pruning).
`resources/templates/backups/` is not copied. `templates/*.json` at the top level belongs to users and is never
touched.

A port that advertises a version above every 0.5.x node becomes the one node that owns these three directories
and will delete example and plug-in templates it does not ship.

### 6.4 Node logs

`<farm>/nodes/<node_id>/monitor-YYYY-MM-DD.log`, date = the writing node's **local** date
(`src/core/monitor_log.cpp`). One line per event, appended, the file reopened for each line:

`HH:MM:SS.mmm LEVEL  [category] message` + `\n` — local time, `LEVEL` ∈ `INFO`, `WARN`, `ERROR`, **two spaces**
after the level. Files whose date is more than 7 days old are deleted when the date rolls over while the node
runs. Other nodes show a peer's log by reading these files directly from the share: the files named for the
**reader's** local yesterday and today, last 500 lines, refreshed every 3 s.

Categories in use: `farm`, `peer`, `http`, `db`, `dispatch`, `render`, `agent`, `job`, `config`.

### 6.5 Render output logs

`<farm>/jobs/<job_id>/stdout/<node_id>/<rangeStr>_<ms>.log` — `<rangeStr>` as in §4.3, `<ms>` the worker's
clock when the chunk was dispatched to its agent (so each attempt has its own file). Plain text, one output
line per line, `\n`, appended. Readers (the UI, endpoint 31) take every `*.log`, split the stem at the **last**
`_`, parse the right part as an integer (files that fail are skipped), and sort by range string then time.
Removed only with the job (`remove_all` of `<farm>/jobs/<id>`), by whichever node performs the delete — a
non-leader that deletes a job removes the directory itself as well as asking the leader.

A node must stop writing a job's log once it knows the job is deleted, or the delete fails on the share.

### 6.6 The command drop box

Used only for job edits that could not reach the leader. `<farm>/commands/<command_id>.json`, written as
`<command_id>.tmp` then renamed, `dump(2)`:

| Key | Type | Value |
|---|---|---|
| `type` | string | `"job_edit"` (the only type) |
| `command_id` | string | `<issued_at_ms>_edit_<job_id>_<issuer node id>` — also the file stem |
| `job_id` | string | |
| `manifest` | object | the edited manifest |
| `mode` | string | `continue` \| `restart` \| `startover` |
| `priority` | int | |
| `issued_at_ms` | int | issuer's clock |
| `issued_by` | string | issuer's node id |

Leader, every 3 s while its database is ready (`MonitorApp::pollFarmCommands`, line 1714): take the `*.json`
regular files directly in `commands/`, **sorted by path** (so by issue time, as text); for each: unparseable →
move to `processed/`; `type` ≠ `job_edit` → move; `issued_at_ms ≤ 0` or older than **15 minutes** by the leader's
clock → move unapplied; otherwise apply as §4.8 — applied or rejected → move; database not ready → leave for the
next poll. "Move" = rename into `commands/processed/` (delete if the rename fails). Then delete files in
`processed/` whose modification time is more than 7 days before the local file clock. There is no
acknowledgement to the issuer and no protection against two leaders applying the same file.

### 6.7 Cleanup

Automatic: node logs (7 days), processed commands (7 days), staging directories after delivery, `endpoint.json`
at clean stop. Everything else stays until a user acts: jobs (delete), `nodes/<id>` of dead nodes ("remove
peers" in the cleanup dialog, or "forget" = delete `endpoint.json`), orphaned `jobs/<dir>` with no job in the
database (the cleanup dialog; any node may list them from its cached job list, and deletes are confined to
`<farm>/jobs/*` and the local staging root — `isPathWithin`).

---

## 7. Job templates and paths

### 7.1 Where templates live and which wins

`TemplateManager::doScan` (`src/monitor/template_manager.cpp:80`), every 5 s on a background thread, on every
node, straight from the share: `templates/examples/*.json` (examples), `templates/plugins/*.json` (DCC plug-in
templates: hidden from the New Job picker, used for plug-in submissions and for editing), `templates/*.json`
(user; `farm.json` skipped by name). A user template with the same `template_id` removes the example / plug-in
one. A file that does not parse appears as an invalid template named after the file. Subdirectories other than
those two are not read.

Templates are a **submit-time** input only: the submitting node bakes a manifest (§7.5) and from then on the
manifest is self-contained. Workers never read templates.

### 7.2 Template schema

`JobTemplate` (`src/core/job_types.h:95-117, 413-445`). All keys optional on read; valid when `template_id` and
`name` are non-empty and at least one of `cmd.windows / linux / macos` is non-empty.

| Key | Type | Default | Notes |
|---|---|---|---|
| `_version` | int | 1 | not checked; the shipped files say `2`, the docs say "always 1" |
| `template_id` | string | | |
| `name` | string | | |
| `cmd` | object | | `{"windows":string,"linux":string,"macos":string,"label":string,"editable":bool}` (`editable` default true) |
| `flags` | array | `[]` | below |
| `frame_padding` | string | `""` | e.g. `####`, `[####]`, `[#####]` |
| `job_defaults` | object | | `frame_start` 1, `frame_end` 250, `chunk_size` 1, `priority` 50, `max_retries` 3, `timeout_seconds` null |
| `progress` | object | | §7.4 |
| `output_detection` | object | | `stdout_regex` null, `path_group` 1, `validation` `"exit_code_only"`, `info` `""` |
| `process` | object | | `working_dir` null |
| `environment` | object | `{}` | string → string |
| `tags_required` | array of strings | `[]` | |

Flag entry (`TemplateFlag`): `flag` string (`""` = positional); `value` string or **null** (null = a bare
switch; `""` = to be filled; other = fixed text or runtime token); `info`; `help` (text under the field; in the
shipped files, not in the docs); `editable` (false); `required` (false); `type` (`""` \| `"file"` \| `"output"`);
`filter` (file-picker extension); `id` (for `{flag:id}` and for plug-in overrides); `default_pattern` (string or
absent).

Shipped templates (`resources/templates/`): two example templates for After Effects and Blender
(named in that repository), `cinema4d-2026-all-options` (examples); `after-effects-plugin`, `blender-5.2-plugin`, `cinema4d-2026-plugin`
(plug-in templates, the ids the plug-ins send). Their `tags_required` are `["ae"]`, `["blend"]`, `["c4d"]`. Flag
ids the plug-ins rely on: Blender `scene_file`, `scene_name`, `output_path`; Cinema 4D `scene_file`,
`take_name`, `output_path`, `output_format`, `threads`; After Effects `project_file`, `comp_name`,
`rs_template`, `om_template`, `output_path`.

### 7.3 Substitution

Two separate passes, at two times, on two machines.

**Submit time, on the submitter** (`TemplateManager::resolvePattern`, line 201) — only for a flag's
`default_pattern`, to pre-fill the form; the result is what the user sees and may edit. In order:
`{frame_pad}` → `frame_padding`; `{project_dir}` and `{file_name}` → parent directory and stem of the value of
the **first** flag with `type == "file"` (both → `""` if that value is empty); `{flag:<id>}` → that flag's
current value; `{date:YYYYMMDD}`, `{date:YYYY}`, `{date:MM}`, `{date:DD}`, `{time:HHmm}`, `{time:HH}`,
`{time:mm}` → the submitter's **local** time; then a clean-up of separators left by empty tokens, as plain
replacements in this order: `-/`→`/`, `-\`→`\`, `-_`→`_`, `_-`→`_`, `--`→`-`.

**Dispatch time, on the worker** (`RenderCoordinator::substituteTokens`, line 511) — in every flag name, every
flag value and the working directory: `{frame}` → chunk start, `{chunk_start}` → chunk start, `{chunk_end}` →
chunk end (decimal, no padding). Nothing else is substituted; in particular the padding token in an output path
(`####`) is passed to the renderer as is.

### 7.4 Progress, completion and detection patterns

`progress.patterns[]`: `{"regex","type":"fraction"|"percentage","info"}` plus `numerator_group` /
`denominator_group` (fraction, defaults 1 and 2) or `group` (percentage, default 1). The writer emits only the
group keys of the entry's type. Evaluated by the agent with the Rust `regex` crate, unanchored, per line of
stdout, first matching pattern wins; fraction = numerator / denominator × 100 when the denominator > 0. Invalid
expressions and unknown types are skipped with a warning.

`progress.completion_pattern`: `{"regex","info"}` or null — §4.10 (a counter per match, on stdout and stderr).

`progress.error_patterns[]`: `{"regex","info"}` — carried, **never evaluated**.

`output_detection`: when `stdout_regex` is a string, the agent keeps the last captured `path_group` and returns
it with the completion; the monitor reads it and does nothing with it. `validation` is never evaluated.

Exit code: 0 = completed, anything else (including a signal, reported as −1) = failed. A kill requested by the
monitor or by the timeout is a failure whatever the exit status.

Regular expressions are therefore in **Rust `regex` syntax** today (the agent is already Rust); templates on
the share are written against that dialect.

### 7.5 Baking a manifest

`TemplateManager::bakeManifestStatic` (line 438), given the template, one string per template flag (the form's
values), the executable for the submitter's OS, the slug, range, chunk size:

- `cmd`: the template's non-empty OS entries, then `cmd[<submitter OS>]` = the (possibly edited) executable.
- Flags, in template order: a bare switch (non-empty `flag`, null `value`, not editable) directly followed by an
  editable, not-required positional entry whose value is empty → **both are dropped**; an editable, not-required
  positional entry with an empty value → dropped; otherwise emit `{"flag", "value"}` with the form value for
  editable entries and the template value for the rest, `is_output` = (`type == "output"`).
- `output_dir` = parent directory of the value of the first `type == "output"` flag, if not empty.
- `max_retries`, `timeout_seconds` from the template's `job_defaults` (the UI form does not expose them at
  submit; the edit dialog does); `progress`, `output_detection`, `process`, `environment`, `tags_required`
  copied; `submitted_by` = node id; `submitted_at_ms` = now; `submitted_os` = this OS.

Validation before submit (`validateSubmission`): executable not empty; name gives a slug; `frame_start ≤
frame_end`; `chunk_size ≥ 1`; every editable required flag non-empty.

### 7.6 Path translation between systems

The model (comments in `src/ui/app_bridge.cpp:1393-1414`, `src/monitor/submission_watcher.cpp:202-224`):
**manifests hold paths in "canonical Windows form".**

- A Windows submitter stores its paths as they are, `submitted_os = "windows"`.
- A macOS submitter translates **every non-empty flag value** and `output_dir` mac → win with its own
  `path_mappings` (`toCanonicalPath`) and then sets `submitted_os = "windows"`. This happens in all four
  submission paths (UI submit, UI resubmit, UI edit, plug-in drop box).
- A Linux submitter does neither (`submitted_os = "linux"`, native paths).
- The **leader**, when it assigns to a **remote** node (`DispatchManager::assignWork`, lines 429-466): if
  `submitted_os` and the worker's `os` are both non-empty and differ, and the leader's `path_mappings` is not
  empty, it translates every string flag value and `output_dir` from the submitted OS to the worker's OS **with
  the leader's own mappings**. `cmd`, `process.working_dir` and `environment` are never translated. Workers do
  not translate what they receive.
- When the leader assigns to **itself** there is no translation at all.
- For display and for finding rendered frames, each node translates win → own OS with its own mappings
  (`fromCanonicalPath`).

The translation function (`translatePath`, `src/core/path_mapping.h:66`), OS tags `win` / `mac` / `lin`:

On this branch (min-paths, `mp_translate_with`; the rules are in minplatform `crates/min-paths/src/map.rs`,
`translate`): the first **enabled** mapping with non-empty roots for both systems whose source root matches wins.
Matching compares with `\` as `/`; for a Windows source it is case-insensitive and — when the root has a path
after its drive letter — **ignores the drive letter on both sides**; the root must end on a **component
boundary**; a root or target that starts with the home shorthand is expanded when it belongs to the current OS.
Result = target root (trailing separators removed) + `/` + remainder, then separators made native to the target
(`\` for Windows, `/` otherwise). No match → the input with separators made native to the target.

On `main` (0.5.11 as released): same OS → separators only. Otherwise the first enabled mapping whose normalised
source root is a **plain string prefix** of the normalised path (case-insensitive for a Windows source, drive
letter included, **no boundary check** — a root `…/share` also matches `…/share-extra/…`); same result
construction; no home expansion.

Both versions apply the separator conversion to **every** translated value, path or not: on its way to a
Windows worker a value containing `/` gets `\`, on its way to a macOS or Linux worker every `\` becomes `/`
(a composition name, an expression or a pattern in a flag value is not spared).

Mappings are **per node** (`config.json`), not shared. What a worker executes therefore depends on the mappings
of whoever is leader at that moment.

---

## 8. Submission

Jobs enter the farm in exactly three ways. There is no command line submitter and no watched folder on the share.

### 8.1 The app's own form

`AppBridge::submitJob` (`src/ui/app_bridge.cpp:1336`): validate, slug, bake (§7.5), canonicalise on macOS
(§7.6), then on the leader queue it directly, elsewhere `POST /api/jobs` `{"manifest":…,"priority":N}` through
the command queue (one attempt; the form reports the leader's body, or "Leader did not respond").

### 8.2 The DCC plug-ins — a local drop box

Sources: `resources/plugins/blender/MinRender.py`, `resources/plugins/cinema4d/MinRender.py`,
`resources/plugins/afterEffects/MinRender.jsx`; distributed by being copied to `<farm>/plugins/` (§6.3) and
installed by hand into each application. They make **no network call**. Each:

1. Finds the node's data directory (§5.2; on Windows the new directory if it holds `config.json` or the old one
   does not exist, else the old one), reads `config.json`, takes `sync_root`, and requires
   `<sync_root>/MinRender-v2` to exist (the literal `MinRender-v2` is in all three plug-ins).
2. Saves the scene / project, then writes one file per job into `<data dir>/submissions/`:
   name `<13-digit ms timestamp>.<hostname>.json`; Blender and Cinema 4D write `<name>.tmp` and rename, **After
   Effects writes the final name directly**. UTF-8, indented.

| Key | Type | Used by the app | Notes |
|---|---|---|---|
| `_version` | int | no | 1 |
| `template_id` | string | **yes, required** | `blender-5.2-plugin` \| `cinema4d-2026-plugin` \| `after-effects-plugin` |
| `job_name` | string | **yes, required** | becomes the slug |
| `submitted_by_host` | string | no | |
| `submitted_at_ms` | int | no | |
| `overrides` | object string → string | yes | keyed by template flag `id`; non-string values are ignored |
| `frame_start`, `frame_end` | int | yes | defaults 1, 250 |
| `chunk_size` | int | yes | default 10 (After Effects sends the whole range for video outputs) |
| `priority` | int | yes | default 50 |

The app side (`SubmissionWatcher`, `src/monitor/submission_watcher.cpp`), every 3 s while the farm runs, for
each regular `*.json` directly in `submissions/` (and in the pre-0.5.8 folder on Windows), in directory order:
parse; find the template by id among the cached valid templates, else by scanning `templates/plugins/`; build
one value per template flag (override by `id` if a string, else the template's value, else `""`); slug; bake
with the template's `max_retries` / `timeout_seconds` and this node's id and OS; canonicalise on macOS; submit
(leader: insert directly; otherwise one `POST /api/jobs`, 3 s / 5 s). **Whatever happens — parse error, unknown
template, no leader, POST failure — the file is renamed into `submissions/processed/`**, and the only trace of a
failure is a log line. A file half-written by the After Effects plug-in when the poll passes is treated as a
parse error and lost.

### 8.3 Resubmission and editing

Covered in §4.8 and §4.9; both end in endpoints 22 / 23 or the command drop box.

---

## 9. Monitoring data between nodes

- **Hardware and state**: only what `/api/status` carries (§2.4) — static `cpu_cores`, `ram_mb`, `gpu_name`,
  and the state words. **No live CPU, GPU, memory or temperature figures are exchanged**, and no per-chunk
  percentage leaves the worker.
- **Progress**: chunk states and `completed_frames` from the leader (§2.6, §2.7), fed by the workers' frame
  reports.
- **Suspension**: endpoint 27, polled every 3 s.
- **Logs**: read from the share (§6.4, §6.5), not over HTTP, in 0.5.11.
- **Previews** (`src/preview/`, `AppBridge::previewInfoForRange` in `src/ui/app_bridge.cpp:1569`): there is no
  preview transport. Each node opens the rendered frames itself from the job's `output_dir` (translated to its
  own OS), choosing a file by a name pattern derived from `output_stem` / the output flag (each run of `#` =
  digits; `[####]` unwrapped; a stem without padding gets `<digits>.<ext>` appended), else by the trailing digits
  before the extension, else by newest modification time; image types `.exr .png .jpg .jpeg .tif .tiff .bmp
  .webp .gif .tga`. A node that cannot reach the output directory shows no preview. The only wire contract here
  is `output_dir` and `output_stem` in the job summary.

---

## 10. Time

Every place a wall clock or a file time matters between machines.

| # | Where | What is compared | Effect of skew |
|---|---|---|---|
| 1 | `nodes/<id>/restart` (§1.3) | the file's modification time (set by the file server or the writer, depending on the share) against the owner's local file clock; > 60 s = stale | a restart request is silently discarded when the two differ by more than a minute; a file dated in the future is always honoured |
| 2 | `commands/*.json` (§6.6) | `issued_at_ms` (issuer's clock) against the leader's clock; > 15 min = expired; ≤ 0 = expired | an issuer more than 15 minutes behind the leader can never queue an edit; one ahead gets a longer life |
| 3 | `commands/` ordering | file names starting with the issuer's `issued_at_ms` | commands from different issuers apply in the order of their own clocks |
| 4 | `commands/processed/` pruning | file modification time against the leader's file clock, 7 days | harmless |
| 5 | `endpoint.json` | `timestamp_ms` and the listing's modification time — **compared only for change**, never with a clock | none (but see §1.2: it must change on every write) |
| 6 | `submitted_at_ms` | submitter's clock; the default queue position inside a priority (`sort_key`) and the list's tie-break | jobs from a machine whose clock is behind jump ahead of earlier submissions from other machines |
| 7 | `assigned_at_ms`, `completed_at_ms` | leader's clock at assignment and at the *processing* of the completion | job durations mix two clocks across a leader change; completion times lag the real finish by up to the report delay + 2 s |
| 8 | failure window and cooldown (§4.7) | leader's clock only | none between machines; reset at leader change |
| 9 | log file names (§6.4) | writer's local date for writing; **reader's** local date for choosing which files to read | a reader in another time zone or with a wrong date shows an empty or one-day-off log |
| 10 | render log names (§6.5) | worker's clock in the name; shown in the viewer's local time | ordering of attempts by different nodes follows their clocks |
| 11 | `{date:…}` / `{time:…}` tokens | submitter's local time | folder names differ by machine time zone |
| 12 | `farm.json` `created_at_ms`, registry `UpdatedMs`, plug-in `submitted_at_ms` and file names | informational | none |
| 13 | liveness (§1.5, §1.6), HTTP timeouts, dispatch TTLs, staging retries | each node's own monotonic clock from the moment of receipt | none — nothing on the wire carries a time |

There is no clock synchronisation requirement stated anywhere and none enforced; rows 1, 2 and 6 are the ones
that misbehave with ordinary skew.

---

## 11. Compatibility checklist

Kinds: **wire** (bytes between nodes), **disk (shared)**, **disk (local)**, **behaviour**.
Tests name the cheapest way to prove conformance against a real 0.5.11 node. "Two-node run" = one C++ node and
the port sharing one folder; on one machine this needs separate data directories (a different home / profile per
node), different `http_port`s; the C++ app also has a single-instance guard (`MinRenderMonitor`) and binds UDP with
`SO_REUSEADDR` only, so a second C++ node in one session may be refused or run HTTP-only (not verified here);
two machines or one VM are simpler. One C++ node plus the port on one machine avoids both.

### 11.1 Must match

| # | Item | Kind | Test |
|---|---|---|---|
| 1 | Farm directory `<sync_root>/MinRender-v2` and the subdirectories of §6.1, created if missing | disk (shared) | golden tree after a first start on an empty folder, diffed against the C++ node's |
| 2 | `farm.json`: keys, the 64-hex secret, reading the secret once at start, generating one when missing or blank, preserving unknown keys on rewrite | disk (shared) | golden file; start the port on a farm made by C++ and the reverse, then compare `secret_fingerprint` on both `/api/status` |
| 3 | The version stamp rule: refresh templates / plug-ins only when strictly newer, stamp only on full success, never downgrade | disk (shared) + behaviour | two-node run with the port's version above and below 0.5.11; checksum the three directories before and after |
| 4 | `node_id.txt`: 12 characters, kept from the existing file; the same id reused by the port on a machine that ran the C++ app | disk (local) | start the port on a copied data directory; compare the id on `/api/status` |
| 5 | `nodes/<id>/endpoint.json`: four keys and types, temporary-file + rename, **new `timestamp_ms` on every write**, written at start and when missing, deleted at clean stop | disk (shared) | golden file; two-node run: the C++ log line `Discovered peer: <id> at <ip:port>`; change the port's address and expect `Peer endpoint changed` within one cycle (not after 20–60 s) |
| 6 | Reading peers' `endpoint.json` from the directory listing, tolerating both the write-once (0.5.11) and the every-3-s (≤ 0.5.10) writers, ignoring a file whose `node_id` is not its directory | behaviour | two-node run against 0.5.11 and against a 0.5.10 build; unit test with a hand-made mismatching file |
| 7 | `GET /api/status` unauthenticated, every key of §2.4 with the exact type, never `null`, `os` one of the three words, `endpoint` = advertised `ip:port` | wire | recorded response from C++ as the golden shape; two-node run: the C++ node must list the port as alive for 10 minutes with `failed_polls` 0 (visible in `GET /api/peers`) |
| 8 | Bearer check: 401 / 503 bodies, exemption of exactly `GET /api/status`, check before routing, 404 with empty body for unknown routes | wire | recorded request / response table driven against both implementations (no token, wrong token, right token, unknown path) |
| 9 | Sending `Authorization: Bearer <secret>` on every call but the status poll | wire | two-node run: no `401` lines in either log; packet capture of one of each call |
| 10 | Status poll cadence and the dead rule (3 consecutive failures, vetoed by a heartbeat < 10 s old), revival on the next success | behaviour | two-node run: kill the C++ node hard, time the port's "dead" verdict; block HTTP only and keep UDP, expect no verdict |
| 11 | UDP: group `239.42.0.1`, port from config, TTL 1, JSON heartbeat every 3 s with the keys and **types** of §1.6, `bye` at stop, ≤ 1400 bytes | wire | packet capture compared key by key with a C++ heartbeat; two-node run: `has_udp_contact` true for the port in the C++ node's `GET /api/peers` |
| 12 | UDP never creates a peer or changes an endpoint; unknown and mistyped packets are ignored without failing | behaviour | send forged packets at the port (unknown id, wrong types, non-object JSON, 1500 bytes) |
| 13 | Election: self + alive peers; `leader` first, `noleader` last, then id ascending; tags exact-case; no terms | behaviour | table-driven unit test of orderings; two- and three-node runs with tag permutations, each node's leader read from the C++ log line `Leader elected: <id>` |
| 14 | Leader-only routes answer `503 {"error":"not_leader"[,"leader_endpoint"]}` on a follower | wire | recorded call to a follower of each kind |
| 15 | The whole endpoint table §2.5: methods, paths, body keys and defaults, status codes, error bodies | wire | a replay suite of recorded C++ request / response pairs per route, run against the port; and the port's client calls run against a C++ leader |
| 16 | Job summary and job detail shapes (§2.6, §2.7), keys omitted rather than null, list in dispatch order, archived omitted | wire | golden responses from a C++ leader holding a fixed set of jobs; a C++ follower's UI showing the port-leader's jobs correctly |
| 17 | A worker kills its render when a fetched job list lacks the job or shows it `cancelled`, and only then | behaviour | two-node run both ways: cancel and delete from the other node, time the kill (≤ ~2 s); stop the leader mid-render and expect no kill |
| 18 | Manifest JSON: every key of §4.1, nullable keys as `null`, `is_output` only when true, unknown keys tolerated | wire + disk | round-trip golden manifests (one per shipped template) through both implementations and compare as JSON values |
| 19 | Slug rule (bytes, case, dashes, 64, the `jobs/<slug>` existence check) and the `-vN` resubmit rule | behaviour | table-driven unit test from C++ outputs for a list of names, including non-ASCII and over-long ones |
| 20 | Chunking and the two chunk spellings (`000001-000010`, `f1-10`) | wire + disk | unit test; log file names and heartbeat `chunk` in a two-node run |
| 21 | `POST /api/dispatch/assign`: accept `{manifest, frame_start, frame_end, edit_epoch?}`, 409 `busy` / `stopped`, queue otherwise | wire | C++ leader assigns to the port; the port as leader assigns to C++ (render a trivial command template) |
| 22 | Reports: `complete` / `failed` / `frame-complete` bodies, `node_id` = own id, epoch echoed only when received, retry until any 200, drop on a body containing `"stale"`, abandon the render on a stale frame answer | wire + behaviour | recorded bodies; two-node run with the leader stopped during a render (report must arrive after it returns); an edit `restart` mid-render |
| 23 | Leader-side guards: assignee and epoch checks, `200 stale` (never an error status) for reports that do not match, legacy reports with no epoch accepted | behaviour | replay recorded reports (matching, wrong node, wrong epoch, no epoch, duplicate) against the port as leader and compare resulting rows with C++ |
| 24 | Chunk and job state machines of §4.4 / §4.5, including `max_retries` arithmetic, `failed_on`, what each command resets, completion with stopped chunks | behaviour | drive both leaders with the same recorded command / report sequence and diff the `jobs` and `chunks` tables |
| 25 | Queue order and the move renumbering (`(index+1) × 1024`, adopting the target's priority, priority change resetting `sort_key`) | behaviour + disk | the same diff, on `sort_key` and on the order of `GET /api/jobs` |
| 26 | Dispatch eligibility (alive, active, idle, ready, healthy, not suspended / cooling, tags ⊇ required, not blacklisted), one chunk per node per cycle | behaviour | simulated peers (canned `/api/status` servers) against the port as leader; compare assignments with C++ under the same inputs |
| 27 | Dead-worker reclaim and the `needs_attention` reclaim + `POST /api/agent/restart` | behaviour | two-node run: kill a rendering worker; expect its chunk `pending` after the dead verdict |
| 28 | Failure tracker numbers (5 in 300 s, 30 s cooldown), `GET /api/nodes/suspended` shape, unsuspend | behaviour + wire | unit test with a fake clock; recorded response |
| 29 | Edit modes, epoch bump, `POST /api/render/abort` push, and the response codes the C++ UI distinguishes (404 + `job_not_found`, 400 + `invalid_manifest`, 503 `db_unavailable`, bare 404) | wire + behaviour | recorded calls; edit from a C++ follower's UI against the port as leader |
| 30 | SQLite schema of §5.1 exactly (names, types, defaults, indexes, WAL), additive migrations on every open, JSON text columns | disk (local / shared) | `.schema` diff of a database created by each; open a C++ snapshot in the port and a port snapshot in C++ (`sqlite3` + a takeover in a two-node run) |
| 31 | `state/snapshot.db` every 30 s while leader, restore-on-takeover, fall back to the local database when absent | disk (shared) + behaviour | two-node run: submit on leader A, wait > 30 s, stop A, expect the job on B; repeat with roles swapped |
| 32 | Takeover keeps chunks assigned to live nodes and accepts their reports | behaviour | two-node run: leader dies while the other node renders; the chunk must complete without re-rendering |
| 33 | `commands/` drop box: file name, keys, tmp + rename, 15-minute rule, sorted application, move to `processed/`, 7-day prune, unknown types moved aside | disk (shared) | golden file written by a C++ follower with its leader unreachable; the port as leader applies it; the reverse |
| 34 | `nodes/<id>/restart`: empty file, 60 s rule, consumed by the owner | disk (shared) | write the file by hand; from the C++ node panel |
| 35 | Node log files: path, name by local date, the exact line shape, 7-day purge | disk (shared) | golden line regex; the C++ log panel showing the port's log |
| 36 | Render log files: path, name `<range>_<ms>.log`, plain lines, `[stderr] ` prefix, stop writing after delete | disk (shared) | the C++ "Task Output" panel listing and opening the port's logs; delete a job mid-render from C++ and expect the directory gone |
| 37 | `DELETE` removes `<farm>/jobs/<id>`; path-segment rule on every id | behaviour | recorded calls with `..`, `a/b`, `a:b` |
| 38 | Template files: schema of §7.2, the three directories and the override rule, tolerance of `_version` 2 and of `help` | disk (shared) | load the six shipped files and compare the parsed model as JSON with C++'s re-serialisation |
| 39 | Baking: the optional-flag dropping rule, `output_dir`, `cmd` map, copied blocks | behaviour | golden manifests from C++ for each shipped template with fixed form values (including empty optional fields) |
| 40 | Runtime tokens `{frame}`, `{chunk_start}`, `{chunk_end}` in flags, values and working directory; argument list order; no shell | behaviour | a template whose executable echoes its arguments; compare render logs |
| 41 | Submit-time pattern tokens and the separator clean-up order | behaviour | table-driven unit test from C++ outputs |
| 42 | Success = exit code 0; kill of the whole process tree on abort / timeout; frame counting by `completion_pattern` on stdout and stderr; `PYTHONUNBUFFERED=1` | behaviour | a script that spawns a grandchild and prints the pattern; cancel and timeout cases on each OS |
| 43 | Path handling: canonical Windows form at macOS submit with `submitted_os = "windows"`; leader-side translation of flag values and `output_dir` for remote workers of another OS; nothing for itself; nothing for `cmd` | behaviour + wire | table-driven translation vectors (both the `main` and the min-paths rule sets — see open question 3); a mixed-OS two-node run in each leader / worker arrangement |
| 44 | `config.json`: every key of §5.3 read and kept; `path_mappings` shape; `node_stopped` restored at start | disk (local) | start the port on a real node's copied `config.json`, save, diff as JSON |
| 45 | Data directory locations per OS, including the Windows legacy-folder copy and continued polling of the legacy `submissions\` | disk (local) | start on a profile holding only the legacy folder |
| 46 | Plug-in drop box: file discovery, keys and defaults, template lookup incl. `templates/plugins/`, move to `processed/` | disk (local) | feed the port the files written by the three shipped plug-ins (captured once) and compare the resulting manifest with C++'s |
| 47 | Local staging: path `staging/<job>/<range>/`, output-flag rewrite to `<staging>/<file name>`, flat copy with overwrite, delete after, 30 s / 600 s retry, completion only after delivery | behaviour + disk (local) | render with staging on to a folder that is made unavailable, then restored |
| 48 | Node stop / start endpoints and persistence; a stopped node refuses work and can still lead | wire + behaviour | from the C++ node panel against the port and the reverse |
| 49 | Windows only: registry key `HKCU\Software\MinRender\Node` with the four values and their types | disk (local) | run the shipped companion script against the port |
| 50 | Capabilities array content, so C++ followers enable job editing and chunk stop when the port leads | wire | the C++ UI's edit and stop actions being enabled |

### 11.2 The ten riskiest

1. **#7 status types** — one `null` or negative number and every 0.5.x node declares the port dead.
2. **#11 UDP types** — a mistyped heartbeat field is an uncaught exception on the main thread of every 0.5.x
   receiver.
3. **#30 / #31 schema and snapshot** — the only durable job state; both directions must open each other's file.
4. **#5 `timestamp_ms` freshness and rename-over** — otherwise address changes take a minute, or never land on
   SMB.
5. **#22 / #23 report matching and the `200 stale` convention** — a non-200 makes old workers retry forever; a
   missing guard lets a late report complete someone else's chunk.
6. **#17 cancel by absence** — a job list that is wrong or empty for one poll kills live renders farm-wide.
7. **#43 path translation at the leader** — the leader's private mappings decide what every worker runs.
8. **#3 version stamp** — the port's advertised version decides who owns (and prunes) the shared templates and
   plug-ins.
9. **#13 election determinism** — any difference in ordering or in the alive set gives two leaders.
10. **#24 / #25 state machines and `sort_key`** — a leader change between implementations must not reorder the
    queue or resurrect chunks.

### 11.3 Ambiguous, undocumented, or bug-like behaviour that existing nodes depend on or exhibit

A port has to reproduce the ones marked **keep**, and decide (with the owner) on the others, knowing that a
0.5.x node in the same farm will go on doing them.

| # | Behaviour | Where | Verdict |
|---|---|---|---|
| a | Cancel, delete and node-stop during a render are reported to the leader as chunk **failures** (retry burned, node blacklisted for the chunk, counts toward suspension) | `RenderCoordinator::abortCurrentRender` → `failChunk` | keep on the worker side (a 0.5.x leader needs *some* report to free the chunk); the leader side could stop counting them — owner |
| b | A stopped node that had a chunk queued "abandons" it with no report; the leader keeps it `assigned` to a live node indefinitely | `RenderCoordinator::update` | exhibited; a port leader could reclaim by timeout — owner |
| c | Reports always carry `elapsed_ms: 0`, `exit_code: 0`, `error: ""`; the leader discards the first two | `MonitorApp::reportCompletion`, `flushCompletionReports` | keep accepting; sending real values is harmless |
| d | A failed frame-report flush re-queues the whole batch, including groups already delivered | `MonitorApp::flushFrameReports` | harmless (frames are a set); keep the leader idempotent |
| e | Frame numbers are a **count of pattern matches**, not parsed; the pattern is also applied to stderr | `mr-agent/src/executor.rs` | keep (templates are written for it) |
| f | `progress.error_patterns` and `output_detection.validation` are documented as active (`docs/templates.md`) and are never evaluated | agent executor; `RenderCoordinator::onChunkCompleted` | keep ignoring, or implement only as an opt-in — a port that started failing chunks on these patterns would change results on a mixed farm |
| g | `_version` in templates: docs say always 1, shipped files say 2, code never checks | `resources/templates/*.json` | accept anything |
| h | Docs say a drag across priority groups is refused; the code makes the job adopt the target's priority | `DatabaseManager::moveJob`, `docs/monitor.md` | keep the code's behaviour |
| i | `revertChunkToPending` has no state guard: a failed assign POST resets the row even if it has meanwhile completed | `database_manager.cpp:820` | narrow race; a port may guard on `assigned` — compatible |
| j | `reassign` with a `target_node` marks the chunk assigned and tells nobody | `DatabaseManager::reassignChunk` | no caller in 0.5.11; keep the route, behaviour open |
| k | Edit `continue` with a changed frame range or chunk size leaves the old chunk rows | `MonitorApp::applyEditLocally` | keep (the UI steers such edits to `startover`) |
| l | Slug uniqueness is tested against `<farm>/jobs/<slug>` existing, which is only true after the job has rendered; a duplicate id is acknowledged with 200 and then dropped | `generateSlug`, `submitJob` | keep the acknowledgement; a port leader could also check its database — owner |
| m | A plug-in submission is moved to `processed/` even when it could not be delivered; After Effects writes its file non-atomically | `SubmissionWatcher::processSubmission`, `MinRender.jsx` | exhibited; a port could retry instead — compatible |
| n | `GET /api/jobs` can return 200 with an empty body before the leader's first refresh; followers treat the parse error as a failed fetch | `MonitorApp::getCachedJobsJson` | a port follower must treat it as a failed fetch, **not** as an empty list (see #17) |
| o | `/api/status` always says `is_leader: false`, `priority: 100` | `buildLocalPeerInfo` | keep the keys; values unused |
| p | A node's own election tags are those at farm start; its published tags are live | `PeerManager::start`, `recomputeLeader` | exhibited; a port that used live tags would agree with its peers sooner — compatible |
| q | `staging_enabled` and the secret are read only at farm start | `MonitorApp::startFarm` | either is compatible |
| r | The leader does not translate paths for chunks it renders itself, so a macOS (or Linux) leader runs canonical Windows paths | `DispatchManager::assignWork` self branch | looks like a bug; **owner must rule** — fixing it in the port changes what a port leader renders, not the wire |
| s | Translation rewrites separators in every flag value, path or not | `translatePath` via `assignWork` | keep for parity unless the owner rules otherwise; it is leader-side only |
| t | The snapshot's temporary file is written into the sync root itself, and `snapshot.db` is replaced by a non-atomic copy; a new leader may read a partial file, fail to open it, and then fail again on the same local file | `DispatchManager::doSnapshot`, `MonitorApp::onBecomeLeader` | a port may write tmp + rename inside `state/` (compatible for readers) and should validate before use |
| u | Restore copies the snapshot over `minrender.db` without removing a leftover `-wal` / `-shm` from a crashed earlier leadership | `DatabaseManager::restoreFrom` | hazard; a port should remove them first — compatible |
| v | With no snapshot on the share, a new leader opens whatever local database it last had | `onBecomeLeader` | exhibited; owner |
| w | Two nodes initialising an empty farm at once each write a `farm.json` with their own secret; the loser keeps the wrong one in memory until restart | `FarmInit::init` | exhibited; a port can re-read on 401 — compatible |
| x | The node's `mr-agent.log` is outside the data directory on macOS | `mr-agent/src/main.rs` `log_file_path` | either |
| y | Frame numbers are signed in the manifest and database but unsigned in the agent protocol; a negative range cannot be dispatched (the agent rejects the task and the monitor waits) and its range string is malformed | `mr-agent/src/messages.rs`, `ChunkRange::rangeStr` | undefined today; owner |
| z | The dispatcher comment says heartbeats are "~2s"; they are 3 s; the "recently dispatched" TTL (15 s) is sized on that | `dispatch_manager.h` | keep the numbers, not the comment |

---

## Appendix A. Monitor ↔ agent IPC (local only)

Not a farm contract — it never leaves the machine — but it is frozen if a ported monitor is to drive the shipped
`mr-agent` (already Rust, `mr-agent/src/`), or a ported agent is to be driven by the C++ monitor.

- Transport: Windows named pipe `\\.\pipe\MinRenderAgent_<node_id>` (DACL: the current user only); elsewhere a
  Unix stream socket `$TMPDIR/minrender-agent-<node_id>.sock` (trailing slashes trimmed; `/tmp/…` when `TMPDIR`
  is unset or the path would not fit), mode 0600. The monitor is the server; one connection at a time.
- Framing: 4-byte **little-endian** length, then that many bytes of UTF-8 JSON; maximum 16 MiB.
- The agent is started by the monitor as `mr-agent --node-id <id>` from the monitor's own directory, **on demand
  for each render**, and exits after one render (or when the pipe closes). Windows: a named mutex
  `MinRenderAgent_<node_id>` prevents a second agent.
- Monitor → agent (`"type"`): `ping`; `shutdown`; `abort` `{"reason":string}`; `task`:
  `{"type":"task","job_id":string,"frame_start":uint,"frame_end":uint,"command":{"executable":string,"args":[string…]},`
  `"working_dir":string|null,"environment":{…},"progress":{…§7.4}|null,"output_detection":{"regex":string,"capture_group":int}|null,"timeout_seconds":uint|null}`.
- Agent → monitor: `pong`; `status` `{"state":"idle"|"rendering","pid":uint}`; `ack` `{job_id,frame_start,frame_end}`;
  `progress` `{…,"progress_pct":float,"elapsed_ms":uint}`; `stdout` `{…,"lines":[string…]}`;
  `frame_completed` `{"job_id","frame":uint}`; `completed` `{…,"elapsed_ms":uint,"exit_code":int,"output_file":string|null}`;
  `failed` `{…,"exit_code":int,"error":string}`.
- Timings: the monitor fails a chunk when the agent has not connected 10 s after being spawned, or when its
  connection is lost for 30 s during a render (the agent retries the connection 6 × 5 s and then kills the
  render). `ready_for_work` returns to true when the agent disconnects or reports `idle`.

---

## Open questions for the owner

1. **Which versions must the port coexist with?** Only 0.5.11, or every 0.5.x still installed? Older builds
   differ in ways named above (endpoint.json rewritten every 3 s; `pri` in heartbeats; no `capabilities`; no
   `edit_epoch`; before 0.5.3 the bearer check had exemptions and failed open, and a UDP heartbeat could create a
peer). Is there a floor below which a node may be
   told to upgrade — and since nothing in the protocol can refuse a node, should the port add a version floor at
   all (it could only be advisory)?
2. **What `app_version` does the port advertise, and does it take ownership of the share's templates and
   plug-ins?** Any version above 0.5.11 makes the first port node overwrite `templates/examples`,
   `templates/plugins` (pruning files it does not ship) and `plugins/`. Should the port ship those files
   byte-identical, ship new ones, or never refresh the share while 0.5.x nodes remain?
3. **Which path-translation rule is the contract** — `main`'s plain-prefix rule (what the released 0.5.11 runs)
   or this branch's min-paths rule (component boundary, drive letter ignored, home expansion)? They give
   different results for some paths, and the leader's rule decides what workers run. Will `min-paths` be merged
   and released to the farm before the port joins?
4. **The self-dispatch gap** (§11.3 r): should a macOS or Linux port leader translate paths for its own renders?
   And should the port translate on the *worker* (with its own mappings) when it receives a manifest whose paths
   are not in its form — which would make results independent of who leads?
5. **Cancel and stop reported as failures** (§11.3 a, b): keep exactly, or should a port leader recognise them
   (for instance by the job's state at the time of the report) and not burn a retry or count toward suspension?
6. **Failover loss.** Up to 30 s of job state, all suspensions, and any report acknowledged but not yet
   snapshotted are lost at a leader change; with no snapshot an arbitrarily old local database is revived. Is
   reproducing that acceptable, or may the port snapshot more often / on demotion / atomically (all compatible
   for 0.5.x readers)?
7. **Split brain** has no protection. Is the port allowed to add a guard that 0.5.x nodes will ignore (for
   example a leader lease file in `state/`), given that it can only bind port nodes?
8. **May the port add things at all?** New keys in `/api/status`, new capability tokens, new endpoints, new
   columns (nullable or defaulted), new files on the share are all tolerated by 0.5.x. Is the rule "additive
   only, never required" acceptable as the port's own compatibility policy, and who records each addition?
9. **The endpoints with no caller** (§2.5 rows 2, 31–38, including remote config rewrite and farm cleanup over
   HTTP): keep them all for tools, or may the port drop the ones the app itself never uses? Does anything
   outside the repository (scripts, the RNDR companion beyond `/api/status`) call them?
10. **`error_patterns` and output validation** (§11.3 f): documented, never implemented. Implement in the port
    (changing which chunks fail), keep ignoring, or fix the docs?
11. **Negative frame numbers and very large ranges** (§11.3 y): supported, refused at submit, or left undefined?
12. **Linux.** The code has Linux branches throughout (paths, `os: "linux"`, no canonicalisation at submit, no
    GPU name, no restart helper). Is Linux a target for the port, and if so what should a Linux submitter put in
    `submitted_os` and in its paths?
13. **The agent.** Keep the two-process design and the existing `mr-agent` binary (Appendix A frozen), or fold
    the agent into the port — in which case only the behaviours of §4.10 remain as contract?
14. **Local settings ownership.** Unknown keys in `config.json` are dropped by the C++ app on save. Will users
    run both apps on one machine during the transition (so the port must not rely on keys of its own in that
    file), or is it one or the other per machine?
15. **The secret.** Stay with the single clear-text bearer secret for the port's lifetime on mixed farms (the
    only option 0.5.x understands), and is signing heartbeats (named as the next step in `SECURITY.md`) in or
    out of scope?
16. **Test bed.** Is there a farm (or two spare machines / VMs, one Windows and one macOS) on which recorded
    request / response pairs, golden files and two-node runs may be made with synthetic jobs only, and may a
    0.5.10 or older build be installed there for the mixed-version cases?
