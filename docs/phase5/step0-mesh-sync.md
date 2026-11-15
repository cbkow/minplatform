# Phase 5, step 0 — UFB mesh sync: audit and wire contract

Audit of the mesh sync in UFB 1.4.2 (repository `ufb`, branch `main` @ `9e197f0`), read on 2026-11-15.
Read-only: nothing was built or run, so every statement below is derived from the source. Where a
behaviour depends on a library or an operating system and was not confirmed, it says "not verified".

Other step-0 readers cover the metadata model and schema, the browser, file operations, mounts and
media. This document only names the touch points with them.

**Citation legend.** `M/` = `core/src/mesh/`, `C/` = `core/src/`, `B/` = `bindings/src/services/`,
`Q/` = `app/qml/Ufb/App/`. Example: `M/coordinator.rs:346` is `core/src/mesh/coordinator.rs`, line 346.

**Terms.** "Farm path" = the shared folder the user types into the mesh settings. "Phonebook" = the
per-node endpoint files under it. "Epoch" = the protocol version number (`MESH_EPOCH`).

---

## 0. Summary

- A node is one running UFB with mesh sync enabled. All nodes that share a farm path and an epoch form
  one mesh. There is no server.
- One node is **leader**, chosen by a pure sort over each node's own view of who is alive. There are no
  votes, terms or leases, so two nodes can both believe they lead.
- A change travels as a **whole-row push over plain HTTP**: follower → leader → every other node. The
  leader also writes a **SQLite snapshot file** to the farm path about every 30 s when something
  changed; followers merge it. The snapshot is both the repair path and the bootstrap.
- Conflict rule: **last writer wins on wall-clock milliseconds**, per row. No logical clocks, no
  sequence numbers.
- No authentication, no TLS, no shared secret.
- The only version field is the epoch (currently **6**). A mismatch means "ignore that node entirely".

---

## 1. Topology and roles

| Item | Finding | Evidence |
|---|---|---|
| Roles | Every node is a peer; exactly one per view is leader, the rest followers | `M/peer_manager.rs:306-380` |
| Election input | Self (always) + every known peer that is `is_alive` **or** whose phonebook timestamp is younger than 15 s | `M/peer_manager.rs:309`, `:320-341` |
| Election rule | Sort: has tag `leader` first; has tag `noleader` last; then `node_id` ascending (Rust `String` order, byte-wise). First entry wins | `M/peer_manager.rs:343-351` |
| Cadence | Every pass of the peer loop (3 s nominal), and at once on a UDP goodbye | `M/coordinator.rs:520-524`, `M/peer_manager.rs:520-529`, `M/mod.rs:43` |
| Terms / votes / leases | Not found. Leadership is recomputed locally from scratch each pass | `M/peer_manager.rs:306-380` |
| Leader's extra duties | Relays pushes to all peers; writes the shared snapshot; ignores snapshot-notify | `M/http_server.rs:270-285`, `:443-451`, `:473-482`; `M/coordinator.rs:1083-1109`, `:1211-1230` |
| Follower's duties | Posts its own edits to the leader; queues on failure; merges the snapshot | `M/coordinator.rs:1015-1068`, `:1240-1292` |
| On becoming leader | `snapshot_needed` is armed so a snapshot is written within one cycle | `M/coordinator.rs:525-542` |
| Leader loss | Peer is marked dead after 3 failed status polls or a goodbye; the next pass elects the next node in sort order. No hand-over message | `M/peer_manager.rs:437-452`, `:520-529`, `M/mod.rs:46` |
| Peers reappear after isolation | Leader queues a snapshot; follower queues a restore | `M/coordinator.rs:577-586` |

### Split brain

Nothing prevents it; the design tolerates it instead.

- Two partitions each elect their own leader, because self is always a candidate
  (`M/peer_manager.rs:336-341`). A node that sees nobody is leader of one.
- Both leaders write the **same snapshot path**. The only fence is a sidecar file: a leader skips its
  write if the sidecar names another node and is younger than 60 s
  (`M/snapshot_worker.rs:34`, `:270-282`). The check and the write are not atomic (section 12, R3).
- An established leader never merges the shared snapshot (`M/coordinator.rs:1110-1114`); only a leader
  that has not yet done its startup restore does (`:1116-1118`). So after a partition heals, the
  surviving leader keeps its own state and overwrites the snapshot. Rows that only the other side's
  leader accepted stay on the nodes that received them and are not pulled back (section 12, R2).

---

## 2. Discovery and liveness

Three channels feed one in-memory peer map (`M/peer_manager.rs:65`).

| Channel | What it does | Evidence |
|---|---|---|
| Phonebook (shared folder) | Each node rewrites `<farm>/v6/nodes/<node_id>/endpoint.json` every loop pass; every node lists `<farm>/v6/nodes/*/endpoint.json` every pass | `M/peer_manager.rs:186-226`, `:238-300`; `M/coordinator.rs:480-519` |
| UDP multicast | Heartbeat every 3 s; goodbye on mesh shutdown; received packets drained every 200 ms | `M/coordinator.rs:605-668`, `M/udp_notify.rs:99-127` |
| HTTP status poll | `GET /api/status` on each known peer every pass, 3 s timeout, **one peer after another** | `M/peer_manager.rs:383-454` |

Configured peer addresses: not found. A node is only ever found through the phonebook or multicast.

### Rules

| Rule | Value | Evidence |
|---|---|---|
| Peer loop interval | 3 000 ms sleep after each pass (a pass is longer when polls time out) | `M/mod.rs:43`, `M/coordinator.rs:596-599` |
| Phonebook entry is "fresh" | writer's `timestamp_ms` less than 15 000 ms behind the **reader's** clock | `M/peer_manager.rs:249`, `:270` |
| New peer from a fresh entry | starts alive | `M/peer_manager.rs:272-291` |
| New peer from a stale entry | starts not-alive until a poll succeeds | `M/peer_manager.rs:273-287` |
| Heartbeat received | peer becomes alive, its ip / ports / tags are overwritten from the packet | `M/peer_manager.rs:469-517` |
| Status poll succeeds | alive, failure counter reset, tags replaced from the response | `M/peer_manager.rs:410-435` |
| Status poll fails | counter +1 (only while alive); dead at 3 | `M/peer_manager.rs:437-452`, `M/mod.rs:46` |
| UDP silence 15 s | only clears the `has_udp_contact` flag; no effect on liveness | `M/peer_manager.rs:456-465`, `M/mod.rs:45` |
| Goodbye received | peer marked dead, election re-run at once | `M/peer_manager.rs:520-529` |
| Entry skipped | own `node_id`; `data_port == 0`; `mesh_epoch != 6` | `M/peer_manager.rs:258-269` |
| Peer forgotten | dead and its endpoint file is missing; or dead and its timestamp is over 1 h old (then **this node deletes the other node's file and folder**) | `M/peer_manager.rs:532-558` |
| Local address published | the local IP of the route towards the farm share's host (Windows: host part of the UNC path; macOS: parsed from `mount` output); fallback is the route towards a public address | `M/peer_manager.rs:561-645` |
| Address change | detected each pass; triggers an extra heartbeat | `M/coordinator.rs:406-444` |
| Two or more peers failed delivery within 30 s | extra heartbeat, failure log cleared | `M/coordinator.rs:446-476`, `M/peer_manager.rs:105-132` |

---

## 3. Transport

### 3.1 Ports and configuration

| Thing | Default | Configurable | Evidence |
|---|---|---|---|
| HTTP control port (`/api/status`) | 49221 | `meshSync.httpPort` in `settings.json` only; not in the dialog | `M/mod.rs:36`, `C/settings.rs:90-92`, `Q/MeshSettingsDialog.qml:89-131` |
| HTTP data port (everything else) | 49222 | `meshSync.dataPort`, same | `M/mod.rs:37`, `C/settings.rs:93-95` |
| UDP port | 4265 | No — constant passed at construction | `M/mod.rs:39`, `M/coordinator.rs:103-107` |
| Multicast group | 239.42.6.3, TTL 1 | No | `M/mod.rs:40-41`, `M/udp_notify.rs:58-62` |
| Bind address | `0.0.0.0` for both HTTP listeners (IPv4 only, `SO_REUSEADDR`, backlog 1024) and for UDP | `M/http_server.rs:87-95`, `M/udp_notify.rs:42-56` |
| TLS | None. Every URL is built as `http://` | `M/peer_manager.rs:403`, `M/peer_outbox.rs:249`, `M/coordinator.rs:1018-1021` |
| Secret / token / auth header | Not found | `M/http_server.rs:59-78` (no middleware), `M/peer_outbox.rs:250` |
| Proxy | Outbound client is built with `no_proxy()` | `M/commands.rs:22-33` |
| Old defaults moved on load | 49200 → 49221; 49201 → 49221; 49202 → 49222; data port 0 → 49222 | `C/settings.rs:413-449` |

The comments in the UDP module still quote older ports and groups; the constants in `M/mod.rs` are what
runs (`M/udp_notify.rs:9-16` versus `M/mod.rs:36-40`).

### 3.2 HTTP routes

Server is axum 0.7.9 (`core/Cargo.toml:54`, `Cargo.lock:320-321`). Bodies are JSON. Field names are
`snake_case` on the wire. Standard axum behaviour for the JSON extractor (wrong content type 415,
malformed JSON 400, missing required field 422, wrong method 405, unknown path 404) is assumed, not
verified.

**Control port**

| Method, path | Auth | Request | Response 200 | Evidence |
|---|---|---|---|---|
| `GET /api/status` | none, **not** epoch-gated | none | `{ "node_id": string, "is_leader": bool, "peer_count": number, "last_snapshot_time": number (ms, 0 = never), "enabled": bool, "tags": [string], "mesh_epoch": number }` | `M/http_server.rs:59-62`, `:150-174` |

The poller reads `mesh_epoch` and `tags` only; it drops the peer when the epoch differs. `node_id`,
`is_leader` and `enabled` in the response are parsed and unused (`M/peer_manager.rs:43-55`, `:411-434`).

**Data port**

| Method, path | Request body | Behaviour | Responses | Evidence |
|---|---|---|---|---|
| `POST /api/metadata/update` | `{ "job_path": string, "item_path": string, "metadata": string (a JSON document carried as a string), "folder_name": string, "is_tracked": bool (default false), "modified_time": i64 ms (default 0), "mesh_epoch": u32 (default 0) }` | Paths are converted to identity form. Upsert of `item_metadata` keyed by `item_path`; on conflict the row is replaced only when incoming `modified_time >=` local. `modified_time == 0` is replaced by the receiver's clock. `deleted_at` is set to NULL. UI event fired. **If this node is leader, the same payload is queued to every peer** (the sender included) | 200 `{"status":"ok"}`; 409 on epoch mismatch; 500 on DB error | `M/http_server.rs:185-288` |
| `POST /api/table/update` | A change object (section 4.2) with `"mesh_epoch": u32` added at top level | Applied through `apply_table_change`; column caches invalidated for `col_*`; relayed to every peer if leader; UI event fired | 200 `{"status":"ok"}`; 409 epoch; 500 when the change fails to apply or the action is unknown | `M/http_server.rs:392-468`, `M/commands.rs:279-551` |
| `POST /api/snapshot/notify` | `{}` (body is not read) | Follower queues a snapshot restore; leader ignores. **Not epoch-gated** | 200 `{"status":"ok"}` | `M/http_server.rs:470-485`, `M/peer_outbox.rs:246` |
| `POST /api/metadata/batch` | `{ "job_path": string, "_since": i64 (optional, ignored), "mesh_epoch": u32 }` | Returns every live `item_metadata` row of that job | 200 array of `{ "item_path", "job_path", "folder_name", "metadata": string, "is_tracked": bool, "modified_time": i64 or null }`; 409 epoch | `M/http_server.rs:339-390` |
| `GET /api/metadata/{job_path}?mesh_epoch=N` | none | Meant to return the same array | see note | `M/http_server.rs:72`, `:290-337` |

Notes on the two read routes:

- No caller of either read route was found anywhere in `core/`, `bindings/` or `agent/`. They are
  served but unused by 1.4.2.
- The `GET` route is registered with `{job_path}` syntax (`M/http_server.rs:72`). The router in use is
  matchit 0.7.3 (`Cargo.lock:2004-2005`), whose capture syntax is `:name`. So the route probably
  matches only the literal text `{job_path}` and cannot work. Not verified by running.

**Outbound client behaviour**

| Call | Timeout | Retries | Evidence |
|---|---|---|---|
| Status poll | 3 s | none (counted towards "dead") | `M/peer_manager.rs:404-409` |
| Follower → leader push | 5 s | queued; see section 4.4 | `M/coordinator.rs:1032-1066`, `:1463` |
| Leader → peer push | 5 s | 3 attempts, 500 ms then 1 000 ms apart, then dropped | `M/peer_outbox.rs:19-21`, `:176-207` |

Success is any 2xx status; the response body is never read (`M/peer_outbox.rs:256`,
`M/coordinator.rs:1034`).

### 3.3 UDP datagrams

One JSON object per datagram, UTF-8, at most 1 400 bytes on send, 2 048-byte receive buffer
(`M/udp_notify.rs:133-135`, `:159`).

| Type | Fields | Evidence |
|---|---|---|
| Heartbeat | `"t":"hb"`, `"from": node id`, `"n": node id`, `"ip": string`, `"port": control port`, `"data_port": data port`, `"epoch": u32`, `"tags": [string]` | `M/udp_notify.rs:99-117` |
| Goodbye | `"t":"bye"`, `"from": node id`, `"n": node id` | `M/udp_notify.rs:120-127` |

- The receiver takes the node id from `n`, falling back to `from` (`M/coordinator.rs:639-640`, `:654-655`).
- A packet whose `from` equals the receiver's own node id is discarded (`M/udp_notify.rs:166`).
- A heartbeat with `data_port == 0` or a different `epoch` is ignored. Note the key is `epoch` here but
  `mesh_epoch` in HTTP and in the phonebook (`M/peer_manager.rs:478-485`, `M/coordinator.rs:644`).
- A goodbye carries no epoch and is not gated (`M/coordinator.rs:653-659`).
- Unknown `t` values are ignored (`M/coordinator.rs:660`).

---

## 4. What is replicated

### 4.1 Unit of change: item metadata

- One push = **one whole `item_metadata` row**: the complete metadata JSON document of one item, with
  its tracked flag and the sender's edit time (`M/peer_outbox.rs:216-234`).
- The local write happens first and returns the timestamp that is then sent, so the time and the
  document cannot be mismatched (`C/subscription.rs:230-261`, `B/subscription.rs:652`, `:705`).
- The document is keyed by column UUID, not by column name (touch point with the metadata reader;
  `C/columns.rs:53-59`).
- Granularity of conflict is therefore the **item**, not the field: two people editing different
  fields of one item on two machines each send their full document, and the later one replaces the
  other (`M/http_server.rs:242-249`).
- **Deleting an item's metadata is not on the wire.** A push always clears `deleted_at`
  (`M/http_server.rs:248`). `delete_item_metadata` writes a tombstone locally but no caller of it was
  found (`C/subscription.rs:344-356`). A tombstone can only travel inside a leader's snapshot
  (`M/snapshot_worker.rs:872-884`).

### 4.2 Unit of change: table changes (columns)

A table change is a JSON object with an `action` and top-level `modified_time` (ms).

| `action` | Other fields | Receiver behaviour | Evidence |
|---|---|---|---|
| `col_add` | `def`: a `ColumnDefinition` serialised **as a string**, camelCase inside | Find the row by `column_uuid`, else by (job path, folder, column name). If found and incoming time `<=` local: ignore. Otherwise update or insert, replace all its options, then try to load its template | `M/commands.rs:306-391`, `:250-277` |
| `col_update` | `def` as above | Same lookup. No local row: ignored. Stale: only re-read options from the template. Otherwise update, replace options, load template | `M/commands.rs:392-476` |
| `col_delete` | `job_path`, `folder_name`, `column_name`, `column_uuid`, `deleted_at` | Terminal: applied regardless of `modified_time`, unless already deleted at an equal or later time. Options are tombstoned too | `M/commands.rs:477-534`, `B/columns.rs:1184-1198` |
| `sub_add`, `sub_remove` | — | Accepted and ignored (200). A leader still relays the raw payload | `M/commands.rs:293-305` |
| `preset_save`, `preset_delete` | — | Accepted and ignored | `M/commands.rs:535-548` |
| anything else | — | Error → HTTP 500 | `M/commands.rs:549` |

`ColumnDefinition` on the wire (inside `def`), camelCase: `id`, `jobPath`, `folderName`, `columnName`,
`columnType`, `columnOrder`, `columnWidth`, `isVisible`, `defaultValue`, `options`, `modifiedTime`,
`deletedAt`, `templateHash` (omitted when null), `trashedAt` (omitted when null), `columnUuid`
(`C/columns.rs:23-60`). Trash and restore travel as an ordinary `col_update` carrying `trashedAt`
(`B/columns.rs:1028-1040`).

The effective timestamp is `def.modifiedTime`, else the top-level `modified_time`, else the receiver's
clock (`M/commands.rs:315-321`).

Receiving a column change can **read the shared folder**: the template named by `templateHash` is
loaded from the local cache, else from the farm path. If it cannot be loaded the column's hash is set
to NULL locally, which hides the column (`M/commands.rs:186-233`, `C/templates.rs:844-878`).

### 4.3 Ordering, versioning, conflict resolution, idempotency

| Aspect | Finding | Evidence |
|---|---|---|
| Clock | Wall clock, UTC milliseconds, taken on the editing machine | `C/utils.rs:100-102`, `C/subscription.rs:244` |
| Sequence numbers, vector clocks, per-node counters | Not found | — |
| Push, item metadata | incoming `>=` local wins | `M/http_server.rs:249` |
| Push, columns | incoming `>` local wins | `M/commands.rs:329`, `:420` |
| Snapshot merge | incoming `>` local wins; local NULL always loses | `M/snapshot_worker.rs:808-811`, `:881-884` |
| Column trash / delete in snapshot | terminal: applied even if the local row is newer | `M/snapshot_worker.rs:813-833` |
| Idempotency | A replay of the same row with the same time is a no-op in effect (same data rewritten for metadata; ignored for columns). No message ids, no de-duplication | `M/http_server.rs:242-249`, `M/commands.rs:329-335` |
| Echo | The leader relays to all peers including the origin; the origin re-applies its own edit and fires a UI event | `M/http_server.rs:270-285`, `M/peer_outbox.rs:119-131` |
| Legacy NULL times | Stamped with "now" at every mesh start so the merge predicates work | `M/coordinator.rs:264-283`, `M/snapshot_worker.rs:50-95` |
| Path form | Wire and snapshot carry identity strings `vol:<uuid>/<rel>`; receivers also accept legacy native paths and classify them | `M/http_server.rs:221-223`, `M/commands.rs:235-242`, `C/identity.rs:32-45` |

### 4.4 Queues and retries

| Queue | Holds | Bound | Persistence | Drop rule | Evidence |
|---|---|---|---|---|---|
| Command channel | every local edit and internal command | **unbounded** | none | — | `M/coordinator.rs:109`, `:167` |
| Per-peer outbox (leader → peer) | pushes and snapshot notices | 64 messages | none | dropped when full (log only); dropped after 3 attempts | `M/peer_outbox.rs:19-21`, `:119-131`, `:184-199` |
| Edit queue (follower → leader) | failed metadata pushes | unbounded `Vec` | `edit_queue.json`, written **only in mesh shutdown** | retried every 5 s; dropped at 10 retries with a UI warning | `M/coordinator.rs:59`, `:701`, `:865-883`, `:1240-1246`, `:1355-1434`, `M/mod.rs:54`, `:58` |
| Table-change queue (follower → leader) | failed column pushes | unbounded `Vec` | `table_change_queue.json`, same | dropped at 10 retries, **log only** | `M/coordinator.rs:60`, `:702`, `:907-925`, `:1537-1580` |
| Snapshot requests | take / restore | 4 | none | "take" skipped when full | `M/snapshot_worker.rs:35`, `:119`, `M/coordinator.rs:1218-1227` |

A follower with no known leader queues the edit and the retry counter still advances every 5 s
(`M/coordinator.rs:1060-1067`, `:1423-1430`). So an edit that cannot reach a leader for about 50 s is
dropped from the queue. It stays in the local database.

### 4.5 Snapshots

| Aspect | Finding | Evidence |
|---|---|---|
| Format | A SQLite database file with four tables: `subscriptions` (always empty, kept so old builds skip cleanly), `column_definitions`, `column_options`, `item_metadata` | `M/snapshot_worker.rs:343-399`, `:407-420` |
| Content filter | Only rows whose `job_path` (and `item_path`) start with `vol:` | `M/snapshot_worker.rs:422-447` |
| Tombstones | Included (`deleted_at`, `trashed_at` columns are copied) | `M/snapshot_worker.rs:426-443` |
| Who writes | The node that believes it is leader, after its startup restore finished or failed | `M/coordinator.rs:1083-1091`, `:1161` |
| Cadence | When `snapshot_needed` is set and 30 s passed since the last one; the flag is re-armed every 10 min; also on demand and when peers reappear | `M/coordinator.rs:1189-1230`, `M/mod.rs:47`, `:53` |
| Skipped when | `item_metadata` and `column_definitions` are both empty locally; or the sidecar names another node and is under 60 s old | `M/snapshot_worker.rs:248-282` |
| How written | Built in the OS temp folder, copied to `<name>.tmp` on the share, renamed over the final name; then the sidecar the same way | `M/snapshot_worker.rs:301-507` |
| Notice | `POST /api/snapshot/notify` is queued to all peers **when the write is requested, before it has happened** | `M/coordinator.rs:1094-1100`, `:1218-1223` |
| How followers notice | The notice; plus a poll of the file's modification time every 10 s, restoring when it is greater than the last one restored | `M/coordinator.rs:1248-1292`, `M/mod.rs:56` |
| How merged | Copied to the OS temp folder, attached, missing columns added, paths normalised, then per table: insert rows that do not exist locally, update rows where the snapshot is newer. **Nothing is ever deleted locally** | `M/snapshot_worker.rs:513-946` |
| Column match key | `column_uuid`; name-based fallback only for rows without one | `M/snapshot_worker.rs:774-794` |
| Option match key | (column, option name); the column is mapped through `column_uuid`, never through the row id | `M/snapshot_worker.rs:837-863` |
| Schema version inside the file | Not found. The version is the file name and the folder name | `M/mod.rs:81-91` |
| Integrity check (size, hash) | Not found. The sidecar holds only `node_id` and `written_at` | `M/snapshot_worker.rs:27-32` |

### 4.6 Bootstrap of a new or long-offline node

1. On enable, if the snapshot file exists a restore is queued; otherwise the node is marked "restored"
   at once (`M/coordinator.rs:305-320`).
2. The restore merges the whole snapshot into the local database (section 4.5).
3. From then on the node receives pushes (if the leader knows it) and re-merges on every new snapshot.

There is no "send me everything since T" exchange. The `batch` route that could serve one has no
caller (section 3.2). A long-offline node's **own** offline edits are not sent anywhere on return
unless they are still in its queue (section 12, R1).

---

## 5. Files

### 5.1 On the shared folder

`<farm>` is the farm path; `v6` is `FARM_VERSION_SUBDIR` (`M/mod.rs:91-96`).

| Path | Format | Writer | How often | Can two machines write it? | Atomicity | Cleanup | Evidence |
|---|---|---|---|---|---|---|---|
| `<farm>/v6/nodes/<node_id>/endpoint.json` | Pretty JSON: `node_id`, `ip`, `port`, `data_port`, `mesh_epoch`, `timestamp_ms`, `tags` | The node of that id | Every loop pass (about 3 s) and once at start | **Yes** if two machines share a node id. Also **any** node may delete it (and its folder) when the peer is dead and the timestamp is over 1 h old | Written to `endpoint.json.tmp` (fixed name, same folder), then renamed | Owner deletes the file (not the folder) on mesh shutdown | `M/peer_manager.rs:6-26`, `:191-235`, `:532-558`; `M/coordinator.rs:182-191`, `:480-511`, `:704-709` |
| `<farm>/v6/nodes/<node_id>/endpoint.json.tmp` | same | same | same | same | — | Removed only if the rename fails | `M/peer_manager.rs:217-224` |
| `<farm>/v6/snapshots/ufb_snapshot_v6.db` | SQLite (section 4.5) | Any node that believes it leads | At most every 30 s, at least every 10 min while something exists to write | **Yes**, under split brain; fenced only by the sidecar check | Copy to `.tmp`, rename | Never removed | `M/snapshot_worker.rs:262-307`, `:469-488` |
| `<farm>/v6/snapshots/ufb_snapshot_v6.db.tmp` | SQLite, in flight | same | same | **Yes**; and each writer first **deletes any existing `.tmp`**, which may be another leader's upload in progress | — | Removed on failure and before each write | `M/snapshot_worker.rs:305-312`, `:473-487` |
| `<farm>/v6/snapshots/ufb_snapshot_v6.meta.json` | Compact JSON `{"node_id": string, "written_at": i64 ms}` | same, **after** the database landed | same | Yes | `.meta.json.tmp`, rename; failure is non-fatal | Never removed | `M/snapshot_worker.rs:27-32`, `:490-507` |
| `<farm>/v6/templates/<hash>.json`, `<farm>/v6/templates/_index.json` | JSON | Column templates code (another reader's area). Read here when a column change arrives | — | The manifest is one path written by many machines (compare-and-swap in that module) | temp + replace, with a set-aside dance on Windows | — | `C/templates.rs:563-579`, `:939-1012`, `M/commands.rs:155-233` |
| `<farm>/v5/templates/…` | JSON | Older builds; read-only fallback here | — | — | — | Never deleted | `C/templates.rs:575-578`, `:858-866` |

Notes:

- Resolving the templates folder from the receive path creates folders and may copy the previous
  epoch's templates forward. So **an incoming HTTP message can cause writes to the shared folder**
  (`M/commands.rs:163-167`, `C/templates.rs:662-672`).
- Folder names under `nodes/` are the raw node id typed by the user; no sanitising was found
  (`M/peer_manager.rs:192`).
- The dialog text describes the layout as `<farm path>/nodes/<node id>/`, without the `v6` level
  (`Q/MeshSettingsDialog.qml:80`).
- The `STALE_FILE_CLEANUP_MS` constant (7 days) has no user (`M/mod.rs:57`; no reference found).

### 5.2 Local files

App data folder = the platform's local configuration folder + `ufb` (`C/utils.rs:6-11`).

| Path | Format | Use | Atomicity | Evidence |
|---|---|---|---|---|
| `<app data>/ufb_v5.db` | SQLite | The metadata store. The snapshot worker opens its own connection to it | SQLite | `C/utils.rs:90-92`, `M/snapshot_worker.rs:121-141`, `M/coordinator.rs:286-290` |
| `<app data>/settings.json` (block `meshSync`) | JSON, camelCase | `nodeId`, `farmPath`, `httpPort`, `dataPort`, `tags`, `enabled` | temp + fsync + rolling `.bak` + rename | `C/settings.rs:83-103`, `:451-470` |
| `<app data>/edit_queue.json` | Pretty JSON array, camelCase: `jobPath`, `itemPath`, `metadataJson`, `folderName`, `isTracked`, `modifiedTime`, `queuedTime`, `retryCount` | Follower's undelivered edits | Plain overwrite; deleted when empty | `M/commands.rs:77-91`, `M/coordinator.rs:865-903` |
| `<app data>/table_change_queue.json` | Pretty JSON array: `changeJson`, `queuedTime`, `retryCount` | Follower's undelivered column changes; legacy `sub_*` entries are dropped on load | Plain overwrite; deleted when empty | `M/commands.rs:96-102`, `M/coordinator.rs:907-964` |
| `<OS temp>/ufb_snapshot_v4_take_<ms>.db` | SQLite | Snapshot staging | Removed after upload | `M/snapshot_worker.rs:301-304`, `:488` |
| `<OS temp>/ufb_snapshot_restore_<ms>.db` | SQLite | Local copy of the snapshot for merging | Removed after merge | `M/snapshot_worker.rs:530-541`, `:942` |
| `<app data>/templates/` | JSON | Local template cache | — | `C/templates.rs:795-799` |
| `<app data>/device_id.txt` | A UUID v4 as text | **Not used by the mesh** (backups and app state use it) | Plain write | `C/utils.rs:68-79`, `C/app_state.rs:97` |

---

## 6. Identity

| Identity | How created | Where stored | Evidence |
|---|---|---|---|
| Node id | **Typed by the user** in the mesh dialog. No generator and no default were found | `settings.json`, `meshSync.nodeId` | `Q/MeshSettingsDialog.qml:110-117`, `B/settings.rs:237-250`, `C/settings.rs:86-87` |
| User identity | Not part of the mesh. No user field in any message, file or row written by the mesh | — | `M/peer_outbox.rs:215-247`, `M/peer_manager.rs:6-26` |
| Device id | UUID v4 on first use; unrelated to the mesh | `device_id.txt` | `C/utils.rs:68-79` |
| Volume UUID (inside `vol:` paths) | UUID v5 over the lower-cased, slash-normalised pair "windows hint \| mac hint" of a path mapping, fixed namespace. Each machine derives it alone | derived | `C/volumes.rs:12-30` |
| Column UUID | UUID v5 over job path, folder, lower-cased name and type, joined by `0x1f`, fixed namespace; assigned once at creation | `column_definitions.column_uuid` | `C/columns.rs:102-137` |
| `item_metadata.device_id` | Column exists and is copied by snapshots; no writer was found in the sync path | — | `M/snapshot_worker.rs:385-396`, `C/subscription.rs:248-254` |

**Cloned machine (same node id on two machines).** Each ignores the other on every channel: phonebook
entries with its own id are skipped (`M/peer_manager.rs:258-260`) and multicast packets from its own
id are discarded (`M/udp_notify.rs:166`). Both rewrite the same `endpoint.json` every 3 s
(`M/peer_manager.rs:192`, `:217-221`). Other nodes see one peer whose address flips. No detection or
warning was found.

**Data folder lost.** Settings are gone, so the mesh is unconfigured until the user re-enters farm
path and node id (`B/mesh.rs:168-174`). After that the empty database is filled by the startup
restore (section 4.6). If that node ranks as leader and holds at least one row before a successful
restore, it may publish a near-empty snapshot (section 12, R6).

**Volume identity depends on configuration.** Two machines agree on a `vol:` path only if their path
mappings give the same hint pair. A share registered by a bare drive letter never becomes a volume and
is never shared; the path below the volume root is case-sensitive, so two spellings of one folder are
two identities (`C/identity.rs:463-511`). Touch point with the path / mounts reader.

---

## 7. Scoping

| Question | Answer | Evidence |
|---|---|---|
| Per project? Per subscription? | No. Every node receives every shared row | `M/peer_outbox.rs:119-131`, `M/snapshot_worker.rs:422-447` |
| What is shared | Rows whose path identity starts with `vol:` | `B/mesh.rs:432-459`, `:495-504`; `M/snapshot_worker.rs:407-447` |
| What is never shared | `native:` paths (local disks, desktop); subscriptions (per user since 1.1.1); presets (files, not mesh); backups | `B/mesh.rs:501-504`, `M/commands.rs:293-305`, `:535-548`, `Q/BackupManagerDialog.qml:74` |
| Inbound filter | None. A received push with any path string is classified and stored | `M/http_server.rs:221-258` |
| Mesh boundary | Same farm path (phonebook) or same multicast group, **and** same epoch | `M/peer_manager.rs:264-269`, `:481-485` |
| Tags | Free text, comma separated. Only `leader` and `noleader` have meaning | `B/mesh.rs:177-183`, `M/peer_manager.rs:329-330` |

The schema comment says the `volumes` table is mesh-synced, but neither the push path nor the
snapshot carries it (`C/db.rs:864-868` versus `M/snapshot_worker.rs:343-399`).

---

## 8. Failure behaviour

| Case | What happens | Evidence |
|---|---|---|
| Peer offline (leader's view) | Pushes are retried 3 times and dropped. The peer catches up from the next snapshot | `M/peer_outbox.rs:176-207`, `M/coordinator.rs:1248-1292` |
| Leader offline (follower's view) | Until re-election the edit is queued; 10 retries at 5 s, then dropped with a warning | `M/coordinator.rs:1037-1066`, `:1376-1389` |
| Partial delivery | No acknowledgement tracking beyond the HTTP status. Repair is the snapshot only, and only for rows the leader has | `M/peer_outbox.rs:256-284` |
| Shared folder unreachable | Phonebook write fails (warning after 5 consecutive failures, then every 30); snapshot write and restore fail and are retried on the next trigger; UDP and HTTP keep working with last known addresses | `M/coordinator.rs:478-511`, `M/snapshot_worker.rs:262-265`, `:534-541` |
| Startup restore fails | The node is marked "restored" anyway so a leader is not silenced | `M/coordinator.rs:1150-1168` |
| Clock skew | A node whose clock is ahead wins every conflict until others pass it. A phonebook entry from a node more than 15 s behind looks stale; one from a node ahead looks fresh for longer | `M/http_server.rs:249`, `M/peer_manager.rs:270`, `:323` |
| Older epoch (protocol) | Ignored on multicast and phonebook, dropped after a status poll, refused with 409 on the data routes. It uses other folder, file names, ports and group, so it never meets this mesh | `M/mod.rs:30-91`, `M/peer_manager.rs:264-269`, `:412-424`, `:481-485`, `M/http_server.rs:207-214` |
| Older schema, same epoch | Snapshot merge adds missing columns to the attached copy before merging; unknown table-change actions return 500 and make the sender retry; unknown JSON fields are ignored by the typed bodies (serde default, not verified) | `M/snapshot_worker.rs:554-581`, `M/commands.rs:549` |
| Version field | `mesh_epoch` only (`epoch` in multicast). No application version, no schema version, no capability list is exchanged | `M/http_server.rs:150-162`, `M/udp_notify.rs:106-115` |
| HTTP port taken | Mesh still starts; the status message shows the bind error | `M/coordinator.rs:227-258` |
| Multicast blocked | Discovery falls back to the phonebook and status polls | `M/coordinator.rs:194-196`, `:512-520` |

---

## 9. Security model

There is none beyond "be on the network and know the ports".

| Capability | Who has it | Evidence |
|---|---|---|
| Write any item's metadata on any node | Anyone who can reach the data port. A far-future `modified_time` makes the value win until that time passes | `M/http_server.rs:203-261` |
| Have the write spread to the whole mesh | Send it to the leader, which relays it | `M/http_server.rs:270-285` |
| Delete a column everywhere | One `col_delete`; it is terminal and ignores timestamps | `M/commands.rs:491-531` |
| Read all metadata of a job | `POST /api/metadata/batch` with the epoch number | `M/http_server.rs:348-390` |
| Make followers re-merge the snapshot repeatedly | `POST /api/snapshot/notify`, no gate at all | `M/http_server.rs:470-485` |
| Become leader | Multicast a heartbeat with tag `leader`, or write a phonebook entry. Followers then post their edits to the address in it | `M/peer_manager.rs:469-517`, `:343-358`, `M/coordinator.rs:1017-1021` |
| Take a node out of the mesh | Multicast a goodbye with its id (not gated) | `M/coordinator.rs:653-659`, `M/peer_manager.rs:520-529` |
| Inject rows into every node | Anyone with write access to the farm folder can replace the snapshot file; it is merged without any check | `M/snapshot_worker.rs:528-552` |
| Redirect traffic | Edit another node's `endpoint.json` | `M/peer_manager.rs:255-294` |

Limits: multicast TTL is 1 (`M/udp_notify.rs:61`); listeners are IPv4 only (`M/http_server.rs:92`).
The data is attached as a database, not executed, and all SQL text is fixed
(`M/snapshot_worker.rs:756-890`).

---

## 10. Settings exposed to the user

Dialog "Mesh Sync", opened from the sidebar footer (`Q/MeshSettingsDialog.qml:1-18`).

| Control | Stored as | Default | Evidence |
|---|---|---|---|
| Enabled — "Run mesh sync at app launch" | `meshSync.enabled` | off | `Q/MeshSettingsDialog.qml:95-99`, `C/settings.rs:98-102` |
| Farm path | `meshSync.farmPath` | empty | `Q/MeshSettingsDialog.qml:101-108`, `C/settings.rs:88-89` |
| Node ID ("Unique per machine") | `meshSync.nodeId` | empty | `Q/MeshSettingsDialog.qml:110-117` |
| Tags (comma separated) | `meshSync.tags` | empty | `Q/MeshSettingsDialog.qml:119-130` |
| Trigger snapshot (leader only) | — | — | `Q/MeshSettingsDialog.qml:206-218` |
| Refresh | — | — | `Q/MeshSettingsDialog.qml:219-224` |
| Read-only: status, leader id, peer count, last snapshot time, last warning, peer list (alive / leader / tags) | — | — | `Q/MeshSettingsDialog.qml:144-304` |

Not in the dialog: the two HTTP ports (file only), the UDP port and group (constants), every timing
value. The meaning of the tags `leader` and `noleader` is not explained in the dialog; its placeholder
suggests role labels (`Q/MeshSettingsDialog.qml:123-127`).

Save = write settings, rebuild the manager (which stops the old one), enable if ticked
(`Q/MeshSettingsDialog.qml:307-331`, `B/mesh.rs:330-348`). The mesh is "configured" when farm path and
node id are both non-empty (`M/coordinator.rs:93`).

"Last snapshot" shows the time a write was **requested**, not completed; a skipped or failed write
still updates it (`M/coordinator.rs:1097-1099`, `:1219-1221`).

---

## 11. Threading and runtime model

- One shared multi-threaded tokio runtime for the whole app (`bindings/src/runtime.rs:11-20`).
- The Qt thread calls `block_on(set_enabled(...))`, so the user interface waits for the start
  sequence, including the first phonebook write to the share (`B/mesh.rs:314-318`,
  `M/coordinator.rs:182-191`).
- Blocking file and database work is moved to the blocking pool (`M/coordinator.rs:478-519`,
  `M/http_server.rs:131-146`). The snapshot worker is one long-lived blocking task with its own
  database connection (`M/snapshot_worker.rs:113-192`).
- Events reach QML as queued property bumps on the Qt thread (`B/mesh.rs:357-410`).

### Tasks while enabled

| Task | Loop | Evidence |
|---|---|---|
| Sync worker | command channel + 500 ms tick: snapshot gate, queue flush (5 s), file-time poll (10 s) | `M/coordinator.rs:972-1298` |
| Peer loop | address check, phonebook write, phonebook read, status polls, election, outbox upkeep, stale cleanup, sleep 3 s | `M/coordinator.rs:371-603` |
| UDP task | heartbeat every 3 s; drain every 200 ms | `M/coordinator.rs:605-668` |
| HTTP control and data servers | axum, graceful shutdown | `M/http_server.rs:82-128` |
| Snapshot worker | serial take / restore | `M/snapshot_worker.rs:149-183` |
| One task per peer | serial delivery from its outbox | `M/peer_outbox.rs:162-208` |

### Start order (`set_enabled(true)`)

0. Phonebook write · 1. UDP socket · 2. First heartbeat · 3. HTTP control, HTTP data · 4. Load both
queues; stamp NULL times; start snapshot worker; reset outboxes · 5. Queue a restore if a snapshot
exists · 6. Sync worker · 7. Peer loop · 8. UDP task (`M/coordinator.rs:158-670`).

### Stop order (`shutdown`)

Send shutdown command → clear enabled flag → goodbye + close UDP → stop both HTTP servers → stop
snapshot worker → stop outboxes → **save both queues** → delete own phonebook file → abort remaining
tasks (`M/coordinator.rs:677-717`).

`shutdown` is reached only from "disable" and from "save settings" (`M/coordinator.rs:671-673`,
`B/mesh.rs:330-337`). **No call on application quit was found**: the quit hook handles mounts only
(`bindings/src/lib.rs:58-70`, `B/mount.rs:312-339`). So on a normal quit no goodbye is sent, the
phonebook file stays, and the queues are not saved.

---

## 12. Rough spots

No `TODO`, `FIXME`, `HACK` or `XXX` markers exist in `core/src/mesh/`. The items below come from
reading the logic. "Sync storage" marks the ones that get worse when the farm path is on a
Dropbox-like folder (lag, conflict copies, half-arrived files).

### Data loss and divergence

| # | Problem | Evidence | Sync storage |
|---|---|---|---|
| R1 | **A follower's undelivered edits vanish from the mesh.** The queue is dropped after 10 retries (about 50 s), and it is only saved to disk in `shutdown`, which a normal quit never calls. Nothing later re-sends rows that exist only in a follower's database: followers never write snapshots and the leader never pulls. The edit stays local until someone edits that item again | `M/coordinator.rs:701-702`, `:1376-1389`, `:1427-1430`; `bindings/src/lib.rs:58-70` | — |
| R2 | **Split brain does not heal fully.** An established leader never merges the snapshot, so rows accepted by a second leader do not reach it | `M/coordinator.rs:1110-1114` | Worse: lag makes two leaders likely |
| R3 | **Two leaders, one snapshot path.** The 60 s sidecar fence is read, then the write happens later, then the sidecar is updated last; two leaders can both pass. Each also deletes the other's `.tmp` upload first | `M/snapshot_worker.rs:270-282`, `:311-312`, `:473-507` | **Yes** — the sidecar arrives late, so the fence is blind; conflict copies of the database appear |
| R4 | **Item-level last-writer-wins.** Concurrent edits to different fields of one item lose one side | `M/http_server.rs:242-249`, `B/subscription.rs:652-659` | — |
| R5 | **No delete on the wire** for item metadata, and every push clears `deleted_at` | `M/http_server.rs:248`, `C/subscription.rs:344-356` | — |
| R6 | **A nearly empty leader can publish a nearly empty snapshot.** The guard only skips when both tables are completely empty, and a failed startup restore arms the writer anyway. Existing nodes keep their rows (merge never deletes), but new nodes bootstrap from it | `M/snapshot_worker.rs:248-260`, `M/coordinator.rs:1150-1168` | Worse: "snapshot not arrived yet" looks like "no snapshot" |
| R7 | **Leader → peer pushes are best effort.** Outbox of 64, dropped when full with a log line only; the `outbox_full` warning kind is documented but only `edit_dropped` is ever raised | `M/peer_outbox.rs:119-131`, `C/events.rs:33-41`, `M/coordinator.rs:1381` | — |
| R8 | **Follower posts to a node that is not really leader.** That node applies the row, answers 200 and does not relay; the sender considers it delivered | `M/http_server.rs:270-285`, `M/coordinator.rs:1034-1036` | Worse: stale phonebook views disagree more often |
| R9 | **Dropped table changes are silent** (log only), unlike dropped edits | `M/coordinator.rs:1562-1570` | — |

### Shared-folder behaviour

| # | Problem | Evidence | Sync storage |
|---|---|---|---|
| R10 | **Constant churn.** Every node rewrites its endpoint file every 3 s through a `.tmp` and a rename | `M/peer_manager.rs:217-224`, `M/coordinator.rs:480-511` | **Yes** — endless uploads; `.tmp` files get synced; renames arrive as delete + create |
| R11 | **Freshness compares the writer's clock with the reader's clock, with a 15 s window.** Sync lag or clock skew above 15 s makes every entry stale. Stale entries are still polled, so reachable peers recover, but a peer that is "fresh" yet unreachable over HTTP stays an election candidate and can be elected | `M/peer_manager.rs:249`, `:270`, `:323-326` | **Yes** |
| R12 | **Any node deletes another node's phonebook file and folder** after 1 h of that node's timestamp | `M/peer_manager.rs:545-553` | **Yes** — a delete propagates and races the owner's next write; skew can trigger it early |
| R13 | **Cloned node id = two writers on one path**, undetected | `M/peer_manager.rs:192`, `:258-260`; `M/udp_notify.rs:166` | **Yes** — conflict copies |
| R14 | **Snapshot is read without any integrity check.** A half-arrived file is copied and attached; per-table failures are logged and skipped, the restore still reports success and records the file time | `M/snapshot_worker.rs:534-552`, `:919-944`; `M/coordinator.rs:1133-1147` | **Yes** |
| R15 | **Notice before write.** Peers are told about a snapshot when it is requested. They restore the old file. After that restore the file time is read **afterwards**; if the new file landed in between, its time is recorded although its content was not merged, and it is skipped until the next snapshot | `M/coordinator.rs:1094-1100`, `:1133-1147` | Worse with lag |
| R16 | **Change detection by file modification time, "greater than last".** The file is produced by a copy from the leader's temp folder; if the copy keeps the source's time (not verified per platform), the time is the leader's clock, and a new leader with a slower clock produces snapshots that followers ignore | `M/snapshot_worker.rs:473`, `M/coordinator.rs:1253-1265` | **Yes** — sync tools set file times their own way |
| R17 | **Receiving a message can write to the share** (template folder creation and migration) | `M/commands.rs:163-167`, `:195-199`; `C/templates.rs:662-672` | Yes |
| R18 | **Local queue files are overwritten in place**, not via temp + rename | `M/coordinator.rs:875-879`, `:917-921` | — |

### Networking and runtime

| # | Problem | Evidence | Home networks |
|---|---|---|---|
| R19 | **A peer's outbox keeps the address it was created with.** The removal code compares the peer list with itself and never removes anything, and `add` returns early for a known id. After a peer changes address, the leader keeps pushing to the old one until the mesh is restarted | `M/coordinator.rs:545-568`, `M/peer_outbox.rs:85-111`, `:162-180` | **Yes** — laptops, DHCP, VPN |
| R20 | **Status polls are sequential with a 3 s timeout.** Each unreachable peer adds 3 s to the loop, delaying this node's own phonebook write, election and address detection | `M/peer_manager.rs:402-409` | Yes |
| R21 | **Election is purely local.** Different views give different leaders; nothing reconciles them (see R2, R8) | `M/peer_manager.rs:306-380` | Yes |
| R22 | **Blocking calls on async tasks.** The UDP task resolves the local address every 3 s inline; on macOS that runs the `mount` command and touches the share. Queue load and save use blocking file calls inline | `M/coordinator.rs:628`, `M/peer_manager.rs:595-629`, `M/coordinator.rs:869-903` | — |
| R23 | **The user interface blocks during enable**, including a write to the share | `B/mesh.rs:314-318`, `M/coordinator.rs:182-191` | Yes, on a slow share |
| R24 | **The local address is "the route towards the share's host".** With a sync-storage farm path there is no share host; the fallback is the route towards a public address, which may be a VPN or the wrong interface | `M/peer_manager.rs:561-580` | **Yes** |
| R25 | **Multicast with TTL 1** does not cross routers, VPNs or most guest Wi-Fi; discovery then depends entirely on the phonebook | `M/udp_notify.rs:58-62` | Yes |
| R26 | **Unbounded command channel and unbounded follower queues** | `M/coordinator.rs:109`, `:59-60` | — |
| R27 | **No goodbye and a stale phonebook file after a normal quit** (section 11). Peers need 3 failed polls to notice | `bindings/src/lib.rs:58-70`, `M/peer_manager.rs:437-452` | — |
| R28 | **Status poll does not check who answered.** The `node_id` in the reply is unused, so a reused address marks the wrong peer alive | `M/peer_manager.rs:411-434` | Yes (DHCP reuse) |
| R29 | **The `GET /api/metadata/{job_path}` route is probably unreachable** (section 3.2) | `M/http_server.rs:72`, `Cargo.lock:2004-2005` | — |
| R30 | **Dead fields and stale comments**: `has_ever_seen_peer`, the leadership callback, `STALE_FILE_CLEANUP_MS`, `send_to`; port numbers in comments; the dialog's folder layout text; the `plans/*.md` documents named in the module comments are not in the repository | `M/coordinator.rs:45-48`, `M/peer_manager.rs:181-183`, `M/mod.rs:3-12`, `:57`, `M/udp_notify.rs:9-16`, `Q/MeshSettingsDialog.qml:80` | — |

### Tests

- `core/tests/` has no mesh test (five files, none mention the mesh).
- Inside the mesh there is one unit test, for the subscription filter (`M/coordinator.rs:1582-1609`).
- Path identity has unit tests, including two that document known hazards (`C/identity.rs:239-512`).
- Not found: tests for election, last-writer-wins, snapshot write or merge, queues, or any two-node run.

---

## 13. Compatibility list

What a rebuilt node must do to live in one mesh with 1.4.2 nodes. JSON is parsed by every receiver, so
"exact" means names, types and meaning, not byte order or whitespace. File and folder names, ports,
the group address and the two UUID namespaces are exact.

### Must match exactly

| # | Contract | Value | Evidence |
|---|---|---|---|
| C1 | Epoch | `6` in every message and file below; ignore or refuse anything else the same way | `M/mod.rs:77` |
| C2 | Shared layout | `<farm>/v6/nodes/<node_id>/endpoint.json`; `<farm>/v6/snapshots/ufb_snapshot_v6.db`; `<farm>/v6/snapshots/ufb_snapshot_v6.meta.json` | `M/mod.rs:81-103`, `M/peer_manager.rs:186-192` |
| C3 | `endpoint.json` | Object with `node_id` string, `ip` string (IPv4 text), `port` number, `data_port` number (non-zero), `mesh_epoch` number, `timestamp_ms` number (UTC ms, refreshed at least every 15 s, 1.4.2 does it every 3 s), `tags` string array | `M/peer_manager.rs:6-26`, `:249-270` |
| C4 | Multicast | Group 239.42.6.3, port 4265, TTL 1; heartbeat and goodbye objects as in section 3.3 (note key `epoch`); send a heartbeat at least every few seconds | `M/mod.rs:39-44`, `M/udp_notify.rs:99-127` |
| C5 | Control port | Default 49221; `GET /api/status` → 200 JSON with at least `node_id`, `mesh_epoch`, `tags` (send all seven fields) | `M/http_server.rs:150-174`, `M/peer_manager.rs:43-55` |
| C6 | Data port | Default 49222; `POST /api/metadata/update`, `POST /api/table/update`, `POST /api/snapshot/notify` with the bodies of sections 3.2 and 4.2; 2xx = accepted; 409 on epoch mismatch | `M/http_server.rs:66-78`, `:203-214` |
| C7 | Plain HTTP over IPv4, no auth header required or sent | — | `M/http_server.rs:87-95`, `M/peer_outbox.rs:249-250` |
| C8 | Election | Same candidates and the same sort (tag `leader`, tag `noleader`, byte-wise node id). A different rule means two leaders | `M/peer_manager.rs:320-351` |
| C9 | Leader behaviour | Relay every accepted push to all peers; write the snapshot; answer `snapshot/notify` without restoring | `M/http_server.rs:270-285`, `:443-451`, `:473-482` |
| C10 | Follower behaviour | Post own changes to the leader's `ip:data_port`; merge the snapshot on notice and when the file changes | `M/coordinator.rs:1015-1036`, `:1248-1292` |
| C11 | Timestamps | UTC milliseconds since the Unix epoch, signed 64-bit | `C/utils.rs:100-102` |
| C12 | Conflict rules | Item push `>=`; column push `>`; snapshot `>`; column trash and delete terminal; `col_delete` ignores `modified_time` | `M/http_server.rs:249`, `M/commands.rs:329`, `:420`, `:491-521`; `M/snapshot_worker.rs:808-833`, `:881-884` |
| C13 | Path identity | `vol:<uuid>/<rel>` with forward slashes, `vol:<uuid>` for a root; volume UUID = UUID v5, namespace `9e1d4f60-8b2a-4c17-a3e5-6f0c2d7b41a8`, name = `lower(win hint, slashes normalised, no trailing slash) + "|" + lower(mac hint, same)`; never send `native:` | `C/identity.rs:34-45`, `C/volumes.rs:12-30`, `B/mesh.rs:495-504` |
| C14 | Column identity | UUID v5, namespace `4b2c9a17-6f3d-4e88-b1a0-5c7e9d2438f1`, name = job path, folder, lower-cased column name, lower-cased type, joined by byte `0x1f`; assigned once, then carried | `C/columns.rs:102-137` |
| C15 | Metadata document | The `metadata` string is the full per-item JSON document, keyed by column UUID (shape: metadata reader's spec) | `C/columns.rs:53-59`, `B/subscription.rs:652-659` |
| C16 | `ColumnDefinition` in `def` | A JSON **string** containing the camelCase object of section 4.2, with `options` | `C/columns.rs:23-60`, `B/columns.rs:2360-2371` |
| C17 | Snapshot file | SQLite; tables and columns exactly as created in `M/snapshot_worker.rs:343-399`, including the empty `subscriptions` table; only `vol:` rows; `column_options.column_id` refers to `column_definitions.id` **inside the same file**; every column row carries `column_uuid` | `M/snapshot_worker.rs:343-447`, `:837-846` |
| C18 | Snapshot sidecar | `{"node_id": string, "written_at": ms}`, written after the database; honour the 60 s yield | `M/snapshot_worker.rs:27-34`, `:270-282`, `:490-507` |
| C19 | Snapshot merge | Insert missing, update when newer, never delete; tolerate a snapshot with missing newer columns | `M/snapshot_worker.rs:554-581`, `:756-890` |
| C20 | Legacy actions | Accept `sub_add`, `sub_remove`, `preset_save`, `preset_delete` with 200 and do nothing; never emit them | `M/commands.rs:293-305`, `:535-548`; `M/coordinator.rs:1482-1511` |
| C21 | Templates | A column push with `templateHash` expects `<farm>/v6/templates/<hash>.json` to exist; a 1.4.2 receiver that cannot read it hides the column | `M/commands.rs:186-233`, `C/templates.rs:563-579` |

### Free to change without breaking 1.4.2 peers

- All internals: queues, persistence, retry counts, task model, how local edits are stored.
- Saving the queues continuously, sending a goodbye on quit, refreshing a peer's outbox address,
  polling peers in parallel, checking `node_id` in status replies.
- Verifying a snapshot before merging it; staging names for temp files (as long as the final names and
  the rename stay).
- Not serving the two read routes (no 1.4.2 node calls them).
- Detecting a duplicate node id and refusing to start.

### Cannot be fixed while 1.4.2 nodes remain

- Authentication or TLS (1.4.2 sends none and accepts none).
- The election rule, the single shared snapshot path and its sidecar fence.
- 1.4.2 nodes deleting other nodes' stale phonebook files (R12).
- Item-level conflict granularity and wall-clock ordering, unless the new scheme degrades to exactly
  these messages for old peers.
- Any of these needs an epoch bump, which splits the mesh in two (`M/mod.rs:60-77`).

---

## Open questions for the owner

1. **Is the farm path ever on sync storage today**, or always a network share? Rows R3, R10–R16 and
   R24 decide how much of the shared-folder design can be kept.
2. **How long will 1.4.2 nodes stay?** Almost every real fix (auth, election, snapshot ownership,
   field-level merge) needs a new epoch. Is a mixed period with two separate meshes acceptable, or
   must the rebuilt node speak epoch 6 first and switch later?
3. **Follower edits that never reach the leader (R1)** — is this known in practice (people reporting
   "my change did not show up elsewhere")? It decides whether the rebuilt epoch-6 node should add a
   re-send on reconnect, which old peers would accept as ordinary pushes.
4. **Node id**: keep it user-typed, or generate it (and what should happen to existing ids and to a
   cloned machine)?
5. **Tags `leader` / `noleader`**: are they used on the live installation, and on which kind of
   machine? The dialog does not explain them.
6. **Item metadata deletion**: is "cannot be deleted across the mesh" intended, or a missing feature?
7. **The `volumes` table** is described as mesh-synced in the schema but is not carried by the mesh.
   Intended?
8. **Scoping**: every node receives every job's metadata. Is that wanted for the rebuild, or should
   freelancers / remote machines get only some projects?
9. **Security**: is "anyone on the network can read and write metadata" acceptable for the rebuild,
   given home and VPN use?
10. **Ports**: must the defaults 49221 / 49222 / 4265 stay (firewall rules already rolled out), and
    should the ports appear in the dialog?
11. **The planning documents** named in the source (`plans/11-mesh-sync.md`,
    `plans/path-identity-migration.md`) are not in the repository. Do they exist elsewhere, and may
    they be read for the rationale behind epochs 4–6?
12. **The read routes** (`batch`, `GET metadata`) have no caller. Drop them, or were they meant for a
    bootstrap that was never finished?
