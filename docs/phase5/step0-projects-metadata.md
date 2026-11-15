# Phase 5, step 0 - UFB audit: project management and metadata

Audited: the `ufb` repository, branch `main` @ `9e197f0`, version 1.4.2. Read only; nothing was built or run.
Every citation is `path:line` inside that repository. "Not found" / "not verified" mean exactly that.
This document is a CONTRACT for the rebuild: sections 2, 4, 9 and 11 are the data a rebuild must read and write
unchanged. Client, job, host and share names in the source are deliberately not reproduced here.

Layers, for orientation:

| Layer | Files | Role |
|---|---|---|
| core (Rust) | `core/src/{db,columns,subscription,templates,backup,jobs,identity,volumes,metadata,project_config}.rs` | storage, rules |
| bindings (Rust, cxx-qt) | `bindings/src/services/{columns,subscription,backup}.rs` | QML singletons `Columns`, `Subscription`, `Backup`; path translation; mesh broadcast; template promotion |
| QML | `app/qml/Ufb/App/{JobView,ItemListPanel,KanbanBoard,TrackerView,CellEditor,ColumnEditor,ColumnManagerDialog,ColumnTrashDialog,NewJobDialog,BackupManagerDialog}.qml` | views |

A large part of the behaviour (what is shared, template promotion, uuid key translation) lives in the bindings layer,
not in core. A rebuild that ports only `core/` loses it.

---

## 1. Concepts

| Concept (code) | User-facing word | Definition as the code has it | Evidence |
|---|---|---|---|
| Jobs folder | "Jobs folder" | A directory whose children are jobs. Flagged two ways: a bookmark with `is_project_folder = 1`, or a mount config with `isJobsFolder` (default true). | `core/src/db.rs:848-854`, `bindings/src/services/mount.rs:58-59`, `app/qml/Ufb/App/FileBrowser.qml:85-109` |
| Job | "Job" (tab title, sidebar "subscriptions"; the code also says "project") | A folder. There is no job table: a job exists as (a) the `job_path` string on metadata and column rows, and (b) optionally a `subscriptions` row on one machine. "Project" and "job" are the same thing; `is_project_folder` means "jobs folder". | `core/src/subscription.rs:40-53`, `core/src/db.rs:783-793` |
| Subscription | "Sync as Job...", sidebar entry, "Unsubscribe" | A per-user, per-machine row saying "show this job in my sidebar and in All Tracked". Never synced since 1.1.1. | `bindings/src/services/subscription.rs:6-7,369,392`, `core/src/mesh/commands.rs:293-305` |
| Folder (folder type) | inner tab of a job | A top-level, non-dot subdirectory of the job (`3d`, `ae`, ...). Stored only as `folder_name` = the basename, a plain string. | `app/qml/Ufb/App/JobView.qml:61-68`, `app/qml/Ufb/App/ItemListPanel.qml:294` |
| Item ("shot" for some folders) | "Items (n)", "Add Shot" / "Add Item" | A top-level, non-dot subdirectory of a folder, excluding names starting `_t_` or `000000`. Files are never items. | `app/qml/Ufb/App/ItemListPanel.qml:415-421,476-478` |
| Tracked item | star, "All Tracked", "Tracker" | An `item_metadata` row with `is_tracked = 1`. | `core/src/subscription.rs:309-326` |
| Task | - | Not found. No task entity exists; the nearest thing is a tracked item with a status/date column. (`TaskQueue.qml` is the file-operation queue, another reader's area.) | - |
| Column | "Manage Metadata", "Column" | A `column_definitions` row scoped to (job, folder): name, type, options, default. | `core/src/columns.rs:25-60` |
| Column option | "Options" | Name + optional colour hex for `dropdown` / `priority` columns. | `core/src/columns.rs:8-21` |
| Template (v2) | "setup", "shared template" | One JSON file per column schema on the shared farm folder, keyed by `template_hash`; the source of truth for a column's options. A column with a `template_hash` is "promoted". | `core/src/templates.rs:585-617` |
| Preset (v1) | "PRESETS", "Save Column as Template" (the UI calls these "templates" too) | A named, hand-saved single-column definition on the farm folder, reusable across jobs. | `core/src/templates.rs:52-62`, `app/qml/Ufb/App/ColumnManagerDialog.qml:213-218,629` |
| Project template | "New Job from Template", "Add Shot" | A directory tree bundled with the app, copied to make a job or a shot. | `core/src/jobs.rs:46-89`, `core/src/file_ops.rs:1288-1307` |
| Tracker | "Tracker" (per job), "All Tracked" (all subscribed jobs) | Table of tracked items with metadata columns. | `app/qml/Ufb/App/TrackerView.qml:23-28` |
| Board | "Board" | Kanban view of one folder's items, lanes from one column. | `app/qml/Ufb/App/KanbanBoard.qml:1-17` |
| Volume / identity | - (settings: path mappings) | `vol:{uuid}/{rel}` for paths under a mapped share, `native:{os}/{path}` otherwise. The storage form of every path. | `core/src/identity.rs:5-10,34-45` |
| Farm path | "shared folder" (Mesh Sync settings) | `settings.mesh_sync.farm_path`: where templates, presets and the mesh snapshot live. | `bindings/src/services/columns.rs:1789-1802` |

Relations: jobs folder 1-n job; job 1-n folder (by name); folder 1-n item (by path); (job, folder) 1-n column;
column 1-n option; column n-1 template (by `template_hash`); item 1-1 `item_metadata` row holding a JSON object of
column_uuid -> value.

---

## 2. Storage

### 2.1 Where things live

| Thing | Location (generic) | Scope |
|---|---|---|
| Main database `ufb_v5.db` | the OS local config dir + `/ufb/` | per machine; some tables replicated (2.4) |
| `settings.json`, `device_id.txt` | same dir | per machine |
| v2 template cache | same dir + `/templates/` (mirror of the farm dir, incl. `_index.json`) | per machine |
| `ufb_v5.db.pre-identity-<ms>` | next to the DB; written once by the v4 migration | per machine |
| v2 templates | `<farm>/v6/templates/<template_hash>.json` + `_index.json` | shared, by file |
| v2 templates, previous location | `<farm>/v5/templates/` - read-only fallback, never deleted | shared |
| v1 presets | `<farm>/ufb/presets/<slug>.json` | shared, by file |
| Backups | `<job>/.ufb/backups/backup_<date>.json` + `backup_metadata.json` | inside the job folder on the share |
| Legacy project config | `<job>/.ufb/projectConfig.json` or `.ufb/config.json` | read path only, no live caller (2.6) |

Evidence: `core/src/utils.rs:6-11,68-79,90-96`; `core/src/templates.rs:49,572-579,795-799`; `core/src/backup.rs:26-28`;
`core/src/db.rs:1078-1084`; `core/src/project_config.rs:174-187`.

`ufb_v5.db` is a deliberate clean break from `ufb_v4.db` / `ufb_v3.db`: no migration, old files left in place
(`core/src/utils.rs:83-92`). The farm subfolder `v6` is derived from the mesh epoch constant
(`core/src/mesh/mod.rs:77-95`, `core/src/templates.rs:572-574`). Several comments in the bindings still say
`<farm>/ufb/templates/` for v2 (`bindings/src/services/columns.rs:1785`, `core/src/columns.rs:40-41`); the code path is
`<farm>/v6/templates`.

### 2.2 Connection and pragmas

`journal_mode=WAL`, `foreign_keys=ON`, `busy_timeout=5000` on the opening connection; each pooled connection sets
`foreign_keys=ON`, `busy_timeout=5000`, `synchronous=NORMAL`; r2d2 pool of 6 (`core/src/db.rs:32,69-88`). The file is
pre-created with `std::fs` before SQLite opens it, to survive Windows 8.3 short-name profile paths
(`core/src/db.rs:45-64`). One shared handle for all services (`bindings/src/db.rs:15-36`).

### 2.3 Schema (as created by `run_migrations`, `core/src/db.rs:766-978`)

`subscriptions` (`db.rs:770-781`, added columns `:931-941`)

| Column | Type | Notes |
|---|---|---|
| id | INTEGER PK AUTOINCREMENT | |
| job_path | TEXT NOT NULL UNIQUE | identity form |
| job_name | TEXT NOT NULL | display name; defaults to the folder basename |
| is_active | INTEGER NOT NULL DEFAULT 1 | never written after insert (no caller found) |
| subscribed_time | INTEGER NOT NULL | epoch ms |
| last_sync_time | INTEGER | writer exists, no caller found |
| sync_status | TEXT NOT NULL DEFAULT 'Pending' | enum Pending/Syncing/Synced/Stale/Error (`subscription.rs:6-36`); writer has no caller |
| shot_count | INTEGER NOT NULL DEFAULT 0 | writer has no caller |
| modified_time | INTEGER NOT NULL DEFAULT 0 | |
| deleted_at | INTEGER | tombstone = unsubscribed |

Index `idx_subs_active(is_active)`.

`item_metadata` (`db.rs:783-799`, added `:906,937`)

| Column | Type | Notes |
|---|---|---|
| item_path | TEXT NOT NULL, PRIMARY KEY | identity form of the item folder path |
| job_path | TEXT NOT NULL | identity form |
| folder_name | TEXT NOT NULL | basename of the folder tab |
| metadata_json | TEXT NOT NULL DEFAULT '{}' | JSON object, keys = column_uuid (section 4.3) |
| is_tracked | INTEGER NOT NULL DEFAULT 0 | |
| created_time | INTEGER | never written by any current write path |
| modified_time | INTEGER | epoch ms, the LWW clock |
| device_id | TEXT | never written by any current write path |
| deleted_at | INTEGER | tombstone; cleared by any upsert |

Indexes `idx_item_metadata_job(job_path)`, `idx_item_metadata_tracked(is_tracked)`,
`idx_item_metadata_job_tracked(job_path, is_tracked)`.

`column_definitions` (`db.rs:801-817`, added `:919-978`)

| Column | Type | Notes |
|---|---|---|
| id | INTEGER PK AUTOINCREMENT | local only; differs per machine |
| job_path, folder_name, column_name | TEXT NOT NULL | natural key; NO unique constraint |
| column_type | TEXT NOT NULL DEFAULT 'text' | section 4.2 |
| column_order | INTEGER NOT NULL DEFAULT 0 | team default; personal overlay wins |
| column_width | REAL NOT NULL DEFAULT 120.0 | same |
| is_visible | INTEGER NOT NULL DEFAULT 1 | same |
| default_value | TEXT | stored and carried; never applied to items (not found in QML) |
| column_uuid | TEXT | stable identity (4.3) |
| modified_time | INTEGER NOT NULL DEFAULT 0 | LWW clock |
| deleted_at | INTEGER | permanently deleted |
| trashed_at | INTEGER | in the column trash |
| template_hash | TEXT | NULL = unpromoted, hidden from grids |
| name_conflict_marker | TEXT | rejected name of a failed untrash; written, never read by QML (not found) |

Index `idx_column_defs_job_folder(job_path, folder_name)`.

`column_options` (`db.rs:819-825`, added `:925-939`): `id` PK, `column_id` INTEGER NOT NULL REFERENCES
`column_definitions(id)` ON DELETE CASCADE, `option_name` TEXT NOT NULL, `option_color` TEXT, `modified_time` INTEGER NOT
NULL DEFAULT 0, `deleted_at` INTEGER. No index, no uniqueness, no order column (order = rowid insertion order).

`column_layout_personal` (`db.rs:834-846`): `id` PK, `job_path`, `folder_name`, `column_name` TEXT NOT NULL,
`column_order` INTEGER, `column_width` REAL, `is_visible` INTEGER (each NULL = "no override"), `modified_time` INTEGER NOT
NULL DEFAULT 0, UNIQUE(job_path, folder_name, column_name); index on (job_path, folder_name). Keyed by NAME, not uuid.

`bookmarks` (`db.rs:848-854`): `id`, `path` TEXT UNIQUE, `display_name`, `created_time`, `is_project_folder` INTEGER
DEFAULT 0. (Browser reader's area; listed because `is_project_folder` defines a jobs folder.)

`column_presets` (`db.rs:856-862`, `:940`): `id`, `preset_name` UNIQUE, `columns_json`, `created_time`,
`modified_time`, `deleted_at`. Legacy: read only by the one-time export to preset files
(`core/src/templates.rs:447-539`); the CRUD in `core/src/columns.rs:1038-1150` has no caller in the bindings.

`volumes` (`db.rs:869-876`): `uuid` PK, `display_name`, `kind` DEFAULT 'smb', `connection_hints` JSON DEFAULT '{}',
`modified_time`, `deleted_at`. `volume_bindings` (`db.rs:877-882`): `uuid` PK, `local_mount_root`, `bound_time`,
`auto_discovered`. `migration_orphans` (`db.rs:888-894`): `id`, `source_table`, `original_path`, `detail` JSON,
`quarantined_at`.

`browser_folder_prefs` (created lazily by the settings binding, `bindings/src/services/settings.rs:196-210`): `path` TEXT
PK, `prefs` TEXT (JSON), `updated_at`. Holds per-folder view state including this area's keys: `activeSubtab`,
`ilpNameColWidth`, `ilpViewMode`, `ilpBoardColumn`, tracker `trk*` widths, and the reserved key `view:tracker`
(`JobView.qml:96-111,145-163`, `ItemListPanel.qml:108-128`, `TrackerView.qml:102-125`).

The thumbnail cache is a separate database owned by C++ (`db.rs:897-902`); not this area.

### 2.4 Per-machine vs replicated

| Table | Replicated? | How | Evidence |
|---|---|---|---|
| `item_metadata` | yes, rows whose `item_path` AND `job_path` start `vol:` | live HTTP edit + periodic snapshot | `bindings/src/services/mesh.rs:483-511`, `core/src/mesh/snapshot_worker.rs:438-443` |
| `column_definitions`, `column_options` | yes, `vol:` rows | live `col_add` / `col_update` / `col_delete` (only when the job is a "shared folder", 4.6) + snapshot (all `vol:` rows) | `bindings/src/services/columns.rs:1542-1550`, `snapshot_worker.rs:422-436` |
| `subscriptions` | no (since 1.1.1) | legacy `sub_add` / `sub_remove` are accepted and ignored; the snapshot keeps an EMPTY table for old readers | `core/src/mesh/commands.rs:293-305`, `snapshot_worker.rs:407-420` |
| `column_layout_personal` | no | "never mesh-broadcast" | `db.rs:827-833`, `columns.rs:660-664` |
| `column_presets` | no (removed from sync) | | `commands.rs:535-548` |
| `bookmarks`, `volume_bindings`, `migration_orphans`, `browser_folder_prefs` | no | not in the snapshot copy list | `snapshot_worker.rs:422-445` |
| `volumes` | not verified | a comment calls it mesh-synced (`db.rs:864-868`); it is not in the snapshot copy list | |

The live broadcast and the snapshot use different gates: live column changes require `is_shared_folder`; the snapshot
ships every `vol:` row. A column made in a `vol:` folder that is not a subscribed job / jobs folder reaches peers late,
through the snapshot only.

### 2.5 Migrations and versioning

Versioning is `PRAGMA user_version` plus idempotent every-boot passes. Order inside `run_migrations`:

| Step | What | Evidence |
|---|---|---|
| CREATE IF NOT EXISTS | all tables above | `db.rs:768-896` |
| ADD COLUMN, error ignored | `device_id`, `modified_time`, `deleted_at`, `template_hash`, `trashed_at`, `column_uuid`, `name_conflict_marker` | `db.rs:757-763,906-978` |
| user_version 1 | columns: old `deleted_at` reclassified as `trashed_at` | `db.rs:990-1001` |
| user_version 2, 3 | path columns normalised to the local OS form; collisions keep the newer `modified_time` (bookmarks: lower id) | `db.rs:145-385,1019-1046` |
| every boot | seed `volumes` + `volume_bindings` from path mappings | `db.rs:1053-1059` |
| user_version 4 | `VACUUM INTO` backup, then every path column rewritten to identity form inside one transaction; a synced-table row not under a volume moves to `migration_orphans`; then `VACUUM`. No backup = no migration this boot. | `db.rs:406-754,1074-1139` |
| every boot | dedup ACTIVE column rows on (job, folder, name): survivor = has template_hash, then newest, then highest id; survivor inherits a sibling's template_hash; trashed/deleted rows untouched | `db.rs:1141-1239` |
| every boot | backfill NULL/empty `column_uuid` deterministically | `db.rs:1241-1286` |
| first use of the column manager | seed `column_layout_personal` from definitions (`INSERT OR IGNORE`), on a background thread | `columns.rs:714-733`, `bindings/.../columns.rs:399-435` |
| settings flags | `templates_migrated_v1` (SQLite presets -> preset files), `templates_v2_migrated` (give every shared-folder column a `template_hash`) | `core/src/settings.rs:251-260`, `bindings/.../columns.rs:1977-2152` |

On a fresh `ufb_v5.db` steps 1-4 are no-ops on empty tables but still run and still set `user_version` to 4. There is
no schema version number beyond that; "v5" is in the file name and "epoch 6" in the mesh.

Tests: `core/tests/classify_migration.rs:23-108` (synthetic: convert / dedup / quarantine counts);
`migration_v4_dryrun.rs`, `normalize_paths_dryrun.rs`, `classify_shadow_dryrun.rs` are `#[ignore]` dry-runs against a
copied real database (not run, not read beyond their assertions). Unit tests: `columns.rs:1284-1357` (uuid derivation,
key remap), `templates.rs:1532-2276` (slug, CAS, manifest, pull, epoch copy, restore), `identity.rs:239-512`.

### 2.6 Files written into shares

1. `<job>/.ufb/backups/` - section 7. The only thing this area writes INTO a job folder.
2. `<farm>/v6/templates/` and `<farm>/ufb/presets/` - section 4.5.
3. `<job>/.ufb/projectConfig.json` (fallback `.ufb/config.json`, then `projectTemplate.json` in the app data dir):
   a loader exists (`core/src/project_config.rs:156-187`) with a schema of `version`, `folderTypes{}` (isShot, isAsset,
   isPosting, isDoc, addAction, statusOptions, categoryOptions, defaultMetadata, displayMetadata, sortState), `users[]`,
   `priorityOptions[]` (`project_config.rs:5-96`). No caller in bindings or QML was found; nothing writes it. It is the
   previous generation's per-job config. Existing jobs may still contain these files.
4. No sidecar files next to items; no dot-file marks a folder as a job.

---

## 3. From folder to job

### 3.1 Detection - there is none on disk

A folder is a job because of where it sits or because someone subscribed to it:

- The browser shows job affordances (a "synced" column, "New Job") when the current directory is a jobs folder
  (bookmark flag or mount flag) (`FileBrowser.qml:78-109,3753-3761`).
- "Sync as Job..." on any folder calls `Subscription.subscribe_to_job(path, name)` (`FileBrowser.qml:1069-1073,1480-1513`).
- A `ufb://` deep link into an unsubscribed job auto-subscribes: the job root is the first path component under the
  LONGEST matching jobs-folder root, compared case-insensitively in identity space; it refuses when an active
  subscription already matches (so a custom name is not reset) and when the target is not a directory
  (`bindings/src/services/subscription.rs:44-140`, `Main.qml:1151-1165`).
- Opening a subscription opens a `JobView` tab; its inner tabs are the job's top-level non-dot directories, in the
  listing's order, plus "Tracker" (`JobView.qml:44-71,317-413`).

### 3.2 Naming rules

`{number}_{name}`; each part has the characters `\ / : * ? " < > |` removed, is trimmed, and whitespace becomes `_`;
an empty part drops the underscore (`core/src/jobs.rs:14-35`). The QML preview reimplements this with `\s+` -> one `_`
where Rust replaces each whitespace character (`NewJobDialog.qml:40-55`): the preview and the result differ for runs
of spaces.

### 3.3 "New Job" step by step

1. Dialog asks number + name, shows the folder-name preview; visible only in a jobs folder (`NewJobDialog.qml:1-64`).
2. `Subscription.create_job_from_template(parent, number, name)` (`NewJobDialog.qml:66-78`).
3. Template dir: `<exe_dir>/templates/projectTemplate`, on macOS also `<exe_dir>/../Resources/templates/projectTemplate`;
   first existing wins (`bindings/src/services/subscription.rs:406-445`).
4. Errors if the parent is not a directory, the template is missing, both fields are empty, or the target exists
   (`jobs.rs:52-72`).
5. `create_dir_all(target)` then a recursive copy of the template CONTENTS, unfiltered (`jobs.rs:77-81`).
6. Subscribe locally with display name = folder basename; refresh; return the native path, "" on failure
   (`subscription.rs:463-490`). The dialog shows one generic error line (`NewJobDialog.qml:72-74`).

Nothing is written to the database except the subscription; no columns, no metadata, no `.ufb`. A failed copy leaves a
partial folder (no cleanup).

### 3.4 The template "format"

There is no template file format and there are no variables: a template is a directory tree, and the rules are naming
conventions read at run time (`core/src/file_ops.rs:1150-1166`).

Bundled tree (`app/templates/projectTemplate/`):

| Folder | Contents | Add mode |
|---|---|---|
| `3d` | `_t_project_name/{renders,cache,playblasts,project/template.blend}`, `assets/` | shot |
| `ae` | `_t_project_name/{renders,proxies,project/_template.aep}` | shot |
| `premiere` | `_t_project_name/{renders,project/_template.prproj}` | shot |
| `photoshop` | `_t_project_name/{projects,outputs}` | shot |
| `illustrator` | `_t_project_name/{project,outputs}` | shot |
| `audition` | `projects/`, `outputs/` | none (hard-coded) |
| `postings` | `000000a_shorthand_description/` | date_prefixed |
| `assets` | `000000_asset_nametype/` | date_prefixed |
| `docs` | `decks/` | folder |

Rules:

- Add mode of a folder = look up its name (case-insensitive) in the bundled tree: a `_t_*` child -> `shot`; a
  `000000*` child -> `date_prefixed`; otherwise `folder`; `audition` -> `none` (`file_ops.rs:1168-1206`).
- Add Shot: copy the `_t_*` tree to `<folder>/<name>`; files named `template.<ext>` or `_template.<ext>` become
  `<name>_v001.<ext>`; `.gitkeep` skipped; fails if the destination exists (`file_ops.rs:1252-1346`).
- Date-prefixed: `<YYMMDD><letter>_<name>`, local date, the letter is the first of a-z not used by any directory
  starting with today's prefix (shared across base names) (`file_ops.rs:795-829`). The dialog previews letter `a` only
  (`ItemListPanel.qml:1499-1511`).
- Folder: plain `create_directory`.
- New Job copies the tree unfiltered, so the placeholder folders `_t_project_name`, `000000*` and the `.gitkeep`
  files land in every job; the item list hides names starting `_t_` or `000000` (`ItemListPanel.qml:415-421`), and
  the template application files stay inside the hidden `_t_project_name`.
- The bundled-template search list for add modes has eight candidate locations (`file_ops.rs:1213-1235`); New Job uses
  two (`subscription.rs:420-435`).

Because add modes are looked up by folder NAME in the bundled tree, a job folder renamed away from these names
silently becomes mode `folder`.

---

## 4. Metadata

### 4.1 What can carry metadata

Only items (directories directly inside a job's folder) get rows from the UI: every writer is called from
`ItemListPanel`, `KanbanBoard` (through the panel) or `TrackerView` with an item path (`ItemListPanel.qml:1029,1072,1183-1206`,
`TrackerView.qml:1273`). Files have no metadata. The table itself would accept any path. Each row holds the tracked
flag plus one JSON object of column values. There are no fixed fields: status, priority, due date, artist, notes,
links are ordinary user-made columns. The struct with fixed fields (`status, category, priority, dueDate, artist,
note, links, isTracked`, `core/src/metadata.rs:7-29`) is legacy: its manager is constructed in `app_state.rs:100` only,
and the `Metadata` QML singleton is a stub (`bindings/src/services/metadata.rs:1-29`).

People: no person type, no user list in live code (the legacy config had `users[]`). Assignee is a text or dropdown
column.

### 4.2 Column types and value encoding

Type list offered by the editor: `text, dropdown, date, priority, number, checkbox, links, notes`
(`ColumnEditor.qml:262-263`). The type is a free string in the DB; comparisons lower-case it; `note` is accepted as
`notes`; unknown types fall back to text (`CellEditor.qml:49-51,96-115`).

| Type | Stored JSON value | Editor | Evidence |
|---|---|---|---|
| text | string | click -> text field; Enter or focus loss commits, Esc cancels | `CellEditor.qml:119-164` |
| number | number, or `null` when unparseable | same, with a double validator | `CellEditor.qml:167-212` |
| checkbox | `true` / `false`; reads also accept `"true"` and `1` | always-on checkbox | `CellEditor.qml:215-226` |
| dropdown, priority | the option NAME as a string (not an id) | coloured pill -> combo box; falls back to text when the column has no options | `CellEditor.qml:75-78,229-334` |
| date | number: epoch ms of LOCAL midnight; strings are parsed on read; clear = `null` | month popup, Today, Clear | `CellEditor.qml:342-526` |
| notes | string with newlines | popup text area; closing the popup saves if changed, Ctrl+Enter saves, Esc reverts | `CellEditor.qml:532-635` |
| links | ONE string; split on newline or comma for display; click opens externally | popup; closing saves, Esc reverts | `CellEditor.qml:642-799` |

`priority` is a dropdown under another name: no numeric rank, no ordering semantics. Sorting on it is alphabetical.
"Clear" writes `null` and keeps the key (`TrackerView.qml:1537-1545`, `KanbanBoard.qml:121-123`). Formulas: not found.
`default_value`: stored, synced, copied into templates and presets; never applied to a new item (no reader in QML),
and the edit dialog blanks it on every save ("not exposed yet", `ColumnEditor.qml:62`, `bindings/.../columns.rs:984`).

Options: `{name, color}`, colour = `#hex` or empty; palette cycling in the editor (`ColumnEditor.qml:353-360`).
A value that matches no option still displays (grey pill) and gets an "Other: x" lane on the board
(`KanbanBoard.qml:124-139`). Renaming an option is impossible on a promoted column (locked) - removing one orphans
existing values, as the tooltip says (`ColumnEditor.qml:386-388`).

### 4.3 Keys: how values attach to things

Three identities, all strings:

1. **Path identity** (`core/src/identity.rs`). Storage form `vol:{volume_uuid}/{rel}` (rel forward-slash, no leading
   slash, empty rel = `vol:{uuid}`) or `native:{win|mac}/{native path}` (`identity.rs:34-45`). Classification matches
   volume prefixes separator-, drive-letter- and case-insensitively on a component boundary, longest prefix first
   (`identity.rs:128-157,215-237`); the `rel` keeps the case it was given. `volume_uuid` = UUIDv5(namespace
   `9e1d4f60-8b2a-4c17-a3e5-6f0c2d7b41a8`, `"<win>|<mac>"`), each hint trimmed, backslash -> slash, trailing slash
   removed, lower-cased (`core/src/volumes.rs:14-30`); volumes come from the user's enabled path mappings that have
   both a Windows and a macOS side (`volumes.rs:39-58`). Every manager converts on entry (`columns.rs:94-97`,
   `subscription.rs:78-81`); the bindings convert back to native for QML (`bindings/.../subscription.rs:18-35,525-526`).
2. **Column identity** `column_uuid` = UUIDv5(namespace `4b2c9a17-6f3d-4e88-b1a0-5c7e9d2438f1`,
   `job_path \x1f folder_name \x1f lower(column_name) \x1f lower(column_type)`), with `job_path` in identity form
   (`core/src/columns.rs:102-137`). Assigned at insert, stored, never re-derived on rename (`columns.rs:374-375,411-427`).
3. **Template identity** `template_hash` = UUIDv5(namespace `7c1e3b95-42a8-5d6f-9e04-8a172c5b6d39`,
   `project_slug \x1f common_name_slug \x1f lower(column_type)`), or a random UUIDv4 for a deliberate fork
   (`core/src/templates.rs:1197-1220,1272-1276`).

**Value keys.** `metadata_json` is `{ "<column_uuid>": value, ... }`. QML only ever sees display names; the bindings
translate at the boundary:

- read: uuid -> name per (job, folder) (`bindings/.../subscription.rs:504-524,551-564`);
- single-cell write: read the row's current blob from the DB, set `blob[uuid_for(name)]`, upsert, broadcast the whole
  blob (`subscription.rs:585-663`); if the name has no column in that (job, folder) the RAW NAME is used as the key
  (`:625-627`);
- unknown keys pass through both ways (`core/src/columns.rs:339-358`).

**What happens on rename / move / copy.** Metadata is keyed by the item's PATH and nothing follows the folder:

| Event | Effect | Evidence |
|---|---|---|
| Rename an item in the app | `std::fs::rename` only; the old row stays under the old path, the renamed item shows empty cells and loses its star | `core/src/file_ops.rs:142-144`, `bindings/.../file_ops.rs:520-539`, `ItemListPanel.qml:1403-1418` |
| Rename / move outside the app | same: orphaned row; a tracked orphan stays in the Tracker pointing at a path that no longer exists | `core/src/subscription.rs:309-326` (no existence check) |
| Copy an item | the copy has no metadata | no code found that copies rows |
| Delete an item | row untouched (`delete_item_metadata` has no caller) | `core/src/subscription.rs:344-356` |
| Rename a job folder or a folder tab | every row and every column of it is orphaned (`job_path` / `folder_name` are part of the keys); the subscription points at a dead path | schema 2.3 |
| Same folder referenced with different letter case | two identities, metadata splits (documented hazard) | `core/src/identity.rs:485-511` |
| Share mapped by bare drive letter (`Z:\`) | classifies as `native:`, never shared, silently | `identity.rs:463-483` |
| Rename a column | values survive (uuid key); only possible while unpromoted, promoted columns have name and type locked | `ColumnEditor.qml:250-268`, `core/src/columns.rs:446-473` |

### 4.4 Column scope, ordering, lifecycle

Scope: a column belongs to one (job, folder). There are no global columns. A wildcard `folder_name = '*'` is honoured
by reads (`core/src/columns.rs:184,981`) but no writer for it was found. Sharing across folders and jobs happens
through templates (same job, same name+type -> same options) and presets (hand-applied anywhere).

Ordering, width, visibility: the definition row carries team defaults; the user's drag-reorder, width drag and
visibility checkbox write ONLY `column_layout_personal`, never broadcast (`bindings/.../columns.rs:1237-1362`); reads
overlay then re-sort by order (`core/src/columns.rs:234-275`). Reorder writes position index for each changed row
(`bindings/.../columns.rs:1332-1360`). New columns get `max(order)+1` (`ColumnManagerDialog.qml:179-186`). The "Visible"
box in the add dialog writes the SHARED default (`ColumnEditor.qml:473-483`).

Lifecycle states of a column row:

| State | Flags | Shown in | Transitions | Evidence |
|---|---|---|---|---|
| Promoted | `template_hash` set | grids, board, manager | normal | `ItemListPanel.qml:359-363`, `TrackerView.qml:451-454` |
| Unpromoted | `template_hash` NULL | manager only, with a warning icon | "Promote" (lookup-or-mint), happens automatically on add when the share is reachable; "Unpromote" clears it and keeps options | `ColumnManagerDialog.qml:465-475,531-580`, `core/src/columns.rs:898-925` |
| Trashed | `trashed_at` set | "View Trash" | "Delete column" = trash, broadcast as `col_update`; Restore fails with `RenameRequired:` if an active column has the name, then accepts a new name | `core/src/columns.rs:793-891`, `bindings/.../columns.rs:1009-1084` |
| Permanently deleted | `deleted_at` set | nowhere | "Delete Forever": tombstones the row + its options, removes the column's key from every item blob of that (job, folder) with `json_remove`, broadcasts `col_delete` | `core/src/columns.rs:932-962`, `bindings/.../columns.rs:1134-1201` |

The trash is per (job, folder) (`core/src/columns.rs:968-1010`); there is no "empty trash" for all, only per-row
(`ColumnTrashDialog.qml`). Trashing keeps values so a restore brings them back. No janitor removes tombstoned rows
(mentioned as future, `core/src/columns.rs:927-931`).

Add column flow (`ColumnEditor.qml:410-523`, `bindings/.../columns.rs:1502-1616`):

1. On add, `lookup_template(name, type, job)` searches the manifest for (project slug, name slug, type). A hit opens
   "Setup already exists": **Use it** (take the team's options) or **Create new** (fork with a random hash).
2. Insert or update the row by natural key (`core/src/columns.rs:379-429`).
3. If the job is a shared folder: lookup-or-mint the template, store `template_hash`, copy the template's options when
   reused, mirror to the local cache, broadcast `col_add`.
4. For dropdown/priority, `set_column_options` then replaces options, pushes them to the template (revision CAS) and
   broadcasts `col_update` (`bindings/.../columns.rs:1364-1421`).

"Duplicate as..." is the sanctioned rename of a promoted column: new column with a forked template, optional copy of
values from the source uuid key to the new uuid key in SQL, optional trashing of the source
(`bindings/.../columns.rs:532-783`).

### 4.5 Templates and presets on the share

**v2 template file** `<farm>/v6/templates/<template_hash>.json`, pretty-printed JSON, camelCase
(`core/src/templates.rs:585-617`):

```
{ "schemaVersion": 2, "templateHash": "<uuid>", "displayName": "...", "commonNameSlug": "...",
  "projectSlug": "...", "columnType": "...", "defaultValue": null | "...",
  "options": [ { "id": null, "name": "...", "color": null | "#hex", "modifiedTime": 0, "deletedAt": null } ],
  "createdBy": "<os user>", "createdAt": <ms>, "updatedAt": <ms>, "revision": <n >= 1> }
```

**Manifest** `_index.json` (`templates.rs:622-651`): `{ manifestRevision, manifestUpdatedAt, templates: [ { uuid,
displayName, commonNameSlug, projectSlug, columnType, revision, updatedAt, lastUsedAt } ] }`.

- Slug: ASCII alphanumerics lower-cased, everything else `_`, runs collapsed, trimmed, max 64, empty -> `untitled`
  (`templates.rs:91-126`). Project slug: the job path relative to the farm path, slugged; `_root` if equal; else the
  last two path components (`core/src/utils.rs:120-146`).
- Writes: temp file with a per-write uuid in its name (`<hash>.json.tmp.<marker>.<uuid>`), fsync, rename; on Windows the
  destination is set aside as `.json.bak.<uuid>` and restored on failure, 5 tries (`templates.rs:939-1023`). Every
  listing skips non-`.json` and `.json.tmp` names.
- Concurrency: optimistic. `cas_save_template` re-reads and requires the expected `revision`; `expected 0` means
  create-only (`templates.rs:1038-1072`). The manifest is CAS'd on `manifestRevision` with up to 5 retries
  (`templates.rs:1082-1103,1371-1392`). A template mismatch on update is only logged; the local row keeps the edit and
  the share keeps the other writer's (`bindings/.../columns.rs:1739-1746`).
- Reads: local cache first, then the farm dir, then the previous epoch's dir; a remote hit warms the cache
  (`templates.rs:844-878`). `pull_to_local_cache` copies files whose manifest revision is newer and mirrors the manifest;
  a missing remote manifest leaves the cache alone (`templates.rs:1135-1188`).
- Reconcile: on every folder open (async, coalesced per (job, folder)) each promoted column's options are REPLACED from
  its template (`core/src/columns.rs:545-616`, `bindings/.../columns.rs:1430-1469`). A fetch failure leaves the column
  as it is.
- Self-heal: a referenced template missing from the share is re-created from the cache copy or from the column row,
  create-only, revision seeded from the manifest (`templates.rs:1415-1438`, `bindings/.../columns.rs:1660-1719`).
- Epoch move: on first touch, files from `<farm>/v5/templates` are COPIED (never moved) into the current epoch dir if
  absent (`templates.rs:696-752`); v2 files misplaced in the presets dir are moved back (`templates.rs:246-304`).
- On receiving a column from a peer, the receiver fetches the template and takes its options, or clears the hash
  (column hidden) when it cannot (`core/src/mesh/commands.rs:362-366`, comment at `templates.rs:833-843`).

**v1 preset file** `<farm>/ufb/presets/<slug>.json` (`templates.rs:52-62`):
`{ schemaVersion: 1, uniqueName: "<slug>", displayName, createdBy, createdAt, updatedAt, data: { columnName, columnType,
columnOrder, columnWidth, isVisible, defaultValue, options[] } }`. Atomic write by `<slug>.json.tmp` + rename; on Windows
remove-then-rename (`templates.rs:395-419`). Save refuses a display-name collision (case-insensitive) with a different
slug (`bindings/.../columns.rs:2211-2222`); same slug overwrites, keeping `createdAt` / `createdBy` (`:2251-2254`).
Apply inserts the column at the end and broadcasts `col_add` WITHOUT the shared-folder gate and without promotion
(`bindings/.../columns.rs:2273-2305`), so an applied preset column is unpromoted (hidden) until promoted. Delete removes
the file for everyone, no confirmation in the binding (`:2307-2324`). Legacy v1 files in `<farm>/ufb/templates/` are
moved to `ufb/presets/` by content (`schemaVersion: 1`) (`templates.rs:159-236`).

### 4.6 "Shared folder" gate

`is_shared_folder(job_path)`: the path equals or is under an active subscription, a bookmark flagged
`is_project_folder`, or a mount flagged jobs folder; lower-cased prefix compare (`bindings/.../columns.rs:1846-1934`).
Gated by it: template promotion and the live `col_*` broadcast for add / update / options / trash / untrash /
unpromote / promote / permanent delete. NOT gated: metadata edits (those are gated only by `vol:` identity,
`bindings/src/services/mesh.rs:495-504`) and preset apply. Because subscriptions are per-user, the same column edit is
"shared" on one machine and "local-only" on another unless the job sits under a flagged jobs folder.

---

## 5. Views

### 5.1 JobView (`JobView.qml`)

Header: job name, path, "Backups", Refresh (re-lists tabs, refreshes the folder tab, `refresh_tracked`)
(`JobView.qml:274-302`). Inner tab bar: one tab per top-level non-dot directory, then "Tracker" (`:317-413`). The active
subtab is remembered per job as `activeSubtab` (a folder name, or `::tracker`), restored once, saved with a 400 ms
debounce (`:96-111,145-169`). A tab refresh keeps the active tab by PATH (`:44-90`). `goToFolderItem(folder, item)`
switches tab case-insensitively and selects the item; buffered until tabs load (`:171-211`). One shared `FolderTabView`
is re-pointed per tab (item list on the left, browsers on the right) (`:438-443`).

### 5.2 Item list panel (`ItemListPanel.qml`)

- Rows: star, Name, one cell per visible promoted column, Modified. "Items (n)" (`:585`, `:724-946`).
- Columns shown: defined, visible (personal overlay), promoted, sorted by order (`:345-386`).
- Data: one `folder_item_metadata(job, folderName)` call per refresh, parsed into a path -> blob map and the tracked
  set (`:286-322`). Refreshes on `tracked_items_json`, `metadata_rev`, `columns_rev`, `options_rev` (`:167-194`), and on
  file-operation `dirs_changed` for this folder (`:537-548`).
- Sort: Name, Modified, or any column; click cycles asc -> desc -> off; numbers numeric, everything else
  case-insensitive string; empty values always last; not persisted (`:423-466`).
- Filtering / grouping: none in the list (grouping is the board).
- Inline edit: every cell, tracked or not, via `set_item_metadata_field(job, item, folderName, columnName, json, isTracked)`
  (`:1061-1078`). The star toggles `set_item_tracked` (`:1027-1034`).
- Context menu: show as board grouped by (eligible column), Mark as Tracked / Untrack, Open in Left / Right Browser,
  Reveal, Cut, Copy, Rename, Delete (to the OS trash, with confirm), Copy Path, Copy `ufb://` link, Copy `union://` link
  (`:1277-1369`).
- Bottom row: Add (label and behaviour by add mode, hidden for `none`), "Manage Metadata" (`:1224-1243`).
- Per-folder prefs: name column width, list/board, lane column; read-merge-write into the shared prefs row, 500 ms
  debounce, flushed to the OLD folder on change (`:105-144,508-519`).
- Keyboard: none found in the panel itself (no `Keys` / `Shortcut`); editing keys are the cell editors' (4.2).
- Double-click a row: `itemActivated` (host opens it) (`:1129`).

### 5.3 Board (`KanbanBoard.qml`, driven by the panel)

- Lane column: any visible column of type checkbox, or dropdown/priority WITH options, plus a synthetic "Tracked"
  column (`ItemListPanel.qml:57-77`). If the lane column disappears (deleted, hidden, renamed, locally or by a peer) the
  panel falls back to the list and forgets the choice (`:88-98`).
- Lanes: one per option in option order, then "Unset"; checkbox: Checked / Unchecked; Tracked: Tracked / Untracked;
  plus one non-droppable "Other: x" lane per value matching no option (`KanbanBoard.qml:91-150`).
- Cards: star, name, the lane value pill, modified date; order = the panel's sort (the board never sorts) (`:29-31`).
- Selection: click, Ctrl-click toggle, Shift-click range within one lane (`:164-196`). Right-click: the panel's menu.
  Double-click: activate.
- Drag: internal, not an OS drag; the drag set is the selection if the pressed card is in it, else that card; cards
  already in the target lane are skipped; Escape or window deactivation cancels; lane rebuilds are deferred during a
  drag (`:217-277`). A drop calls `bulk_set_item_metadata_field` with each item's current tracked state stated
  explicitly, or `set_item_tracked` per item for the Tracked column (`ItemListPanel.qml:1186-1210`).

### 5.4 Tracker (`TrackerView.qml`)

- Two instances: per job (`jobPath` set) and aggregated "All Tracked" (`jobPath` empty) (`:23-25`, `Main.qml:1986`).
- Source: `Subscription.tracked_items_json` = ALL tracked rows of ALL jobs, filtered in QML: per job by exact string
  equality of the native job path; aggregated by membership in the subscribed set (separator- and case-insensitive)
  (`:285-314`). The SQL deliberately returns tracked rows of unsubscribed jobs too (`core/src/subscription.rs:309-326`).
- Fixed columns: Project (aggregated only), Name, [metadata columns], Folder, Modified (= the row's `modified_time`,
  i.e. last metadata edit, not the folder's mtime) (`:97-100`, `:320`).
- Metadata columns: union over every (job, folder) present, merged by `name|type`; the first definition seen supplies
  order, width, options and id (`:397-476`).
- Filter: "Due:" chips All / Overdue / Today / This Week / This Month, shown when any visible column is a date; a row
  passes if ANY date column falls in the bucket; week = next 7 days, month = same day next month (`:34-76,700-745`).
- Sort: Project, Name, Folder, Modified or any column; same rules as the item list (`:220-236,358-383`).
- Selection: click, Ctrl-click, Shift-click range (`:150-180,1331-1345`). Double-click: go to the item in its job tab.
- Inline edit: same cell editors; always writes `is_tracked = true` (`:1273-1279`).
- Row menu: Open in Project, Reveal, Open in Left / Right Browser, Copy Path, Copy link (single selection); "Set
  <column>" submenus for dropdown/priority options, Checked / Unchecked, Pick date / Set to today, Type value (text,
  number), Clear; Untrack / Untrack n (`:1380-1565`). Bulk edits call `bulk_set_item_metadata_field` once with the list.
- "Manage Metadata" (per-job tracker only) opens the column manager for the folder of the FIRST row (`:505-516,592-609`).
- Column widths: fixed columns in view prefs (`trk*`, key = job path or `view:tracker`), metadata columns in the
  personal layout (`:102-125,259-270`).
- Keyboard: none found.

### 5.5 Column manager, editor, trash

Covered in 4.4. Notable UI rules: drag handle reorders; visibility checkbox; warning icon on unpromoted rows; per-row
Promote / Save as preset / Edit / Duplicate / Unpromote / Trash; "Refresh from share" pulls templates; presets list
with Apply / Delete (`ColumnManagerDialog.qml:189-610`). Opening the dialog pulls templates synchronously
(`:169-176`). Every change calls `Columns.invalidate`, which drops ALL column caches and bumps `columns_rev`
(`:95-101`, `bindings/.../columns.rs:463-482`).

---

## 6. Subscriptions

Subscribing = upsert of a `subscriptions` row (identity path, display name, `subscribed_time`), clearing a tombstone;
re-subscribing overwrites the display name (`core/src/subscription.rs:92-142`). An empty name becomes the native
folder basename (`bindings/.../subscription.rs:358-368`). Unsubscribing sets `deleted_at` and nothing else; metadata is
deliberately untouched (`core/src/subscription.rs:144-160`). Neither is broadcast.

What it changes locally: the sidebar list (ordered by name, `subscription.rs:162-171`); the aggregated tracker's
scope; `is_shared_folder` for that job (so whether column edits promote and broadcast); the browser's "synced"
marker. It does NOT gate metadata visibility: stars and cells show for any browsed job (`ItemListPanel.qml:158-165`),
and metadata for unsubscribed jobs still arrives from peers.

`is_active`, `sync_status`, `last_sync_time`, `shot_count` are dead columns in this version (no caller of their
writers was found). History: until 1.1.0 unsubscribe tombstoned the job's metadata mesh-wide; removed
(`subscription.rs:110-118,149-152`).

---

## 7. Backups

| Aspect | Behaviour | Evidence |
|---|---|---|
| Trigger | manual only: "Backups" in the job header -> "Backup Now". A once-a-day check and a retention eviction exist in core with no caller. | `JobView.qml:274-284`, `BackupManagerDialog.qml:91-107`, `core/src/backup.rs:99-137` |
| Content | one JSON: `{ version: 1, createdAt, jobPath (native, as given), jobName, columns: [ColumnDefinition...], items: [{itemPath, folderName, metadataJson, isTracked}] }`. Columns = active, non-trashed columns of every folder seen in either table; items = all non-deleted rows of the job, tracked or not. Paths inside are identity form; `metadataJson` is uuid-keyed. | `bindings/src/services/backup.rs:5-15,111-163,253-302` |
| File | `<job>/.ufb/backups/backup_<YYYY-MM-DD_HH-MM-SS>.json`, UTC, plain `fs::write` (not atomic); second-resolution name, a second backup in the same second overwrites | `core/src/backup.rs:36-47` |
| Index | `backup_metadata.json`: array of `{timestamp, filename, createdBy (device id), shotCount (always 0), checksum, uncompressedSize, date}`; read-modify-write, not atomic, unparseable -> treated as empty | `backup.rs:49-73` |
| Checksum | hex of Rust's `DefaultHasher` over the bytes; never verified on restore; not stable across Rust versions | `backup.rs:141-147` |
| List | newest first, shows date, filename, size, creator | `BackupManagerDialog.qml:41-53,156-175` |
| Restore | after a confirm: each column through `add_column` (upsert by natural key), each item through `upsert_item_metadata` with a NEW `modified_time`. A merge: rows absent from the backup are kept. No broadcast call. | `bindings/.../backup.rs:165-245` |

The dialog says backups are "not mesh-synced - each device keeps its own history"; in fact they sit in the job folder
on the share, so every machine sees the same list. Not backed up: trashed columns, the personal layout, subscriptions,
templates, presets. The pre-migration `VACUUM INTO` copy (2.5) is the only whole-database backup.

---

## 8. Identity and authorship

There is no user account and no per-edit author.

| Where | What is recorded | Evidence |
|---|---|---|
| `item_metadata.device_id`, `created_time` | columns exist, replicated, never populated by current writers | `core/src/subscription.rs:247-258`, `core/src/mesh/http_server.rs:240-249` |
| Templates, presets | `createdBy` = `$USER`, else `$USERNAME`, else `unknown` (the OS login name is written to the share) | `core/src/templates.rs:431-437` |
| Backups | `createdBy` = the machine's device id (UUIDv4 in `device_id.txt`) | `core/src/utils.rs:68-79`, `bindings/.../backup.rs:75-79` |
| Metadata edits, column edits | nothing | - |

No ownership, permissions or locking of metadata: any machine in the mesh edits anything.

---

## 9. Conflict rules at the data level

All clocks are wall-clock epoch ms of the editing machine (`core/src/utils.rs:100-102`). No vector clocks, no
per-field merge.

| Data | Rule | Evidence |
|---|---|---|
| Item metadata, live | whole row, last writer wins; the receiver applies when `incoming >= local`; the sender's timestamp is kept; a sender without one is stamped at receive time | `core/src/mesh/http_server.rs:230-249` |
| Item metadata, snapshot | insert missing; update all fields when `snap > local` (strict) | `core/src/mesh/snapshot_worker.rs:866-884` |
| Item metadata, local write | read-modify-write of the whole blob in the binding, then upsert; any upsert clears `deleted_at` | `bindings/.../subscription.rs:603-634`, `core/src/subscription.rs:247-258` |
| Consequence | two people editing DIFFERENT cells of the same item within the propagation window: the later blob replaces the earlier, one edit is lost | - |
| Item tombstone | `deleted_at` exists and replicates; no current code sets it for items | `core/src/subscription.rs:344-356` |
| Column, live `col_add` / `col_update` | matched by `column_uuid`, else natural key; applied when `incoming > local` (strict); full replace of fields and options; then options re-read from the template. A stale `col_update` still triggers the template re-read. | `core/src/mesh/commands.rs:250-272,306-476` |
| Column, live `col_delete` | terminal: applies regardless of `modified_time`; idempotent on `deleted_at` | `commands.rs:477-534` |
| Column, snapshot | insert when no row with that uuid (natural key if the snapshot row has no uuid); LWW update when `snap > local`; trashed / deleted flags are copied even when the local row is newer | `snapshot_worker.rs:774-833` |
| Options, snapshot | merged per (column, option name): insert missing, LWW colour; never removes | `snapshot_worker.rs:835-863` |
| Options, authority | the template file; replaced wholesale on folder open and on receive | `core/src/columns.rs:545-616` |
| Template file | revision compare-and-swap; loser's change is dropped with a log line | `core/src/templates.rs:1038-1072`, `bindings/.../columns.rs:1739-1746` |
| Preset file | last rename wins | `templates.rs:31-37` |
| Trash vs edit | trash, untrash, promote, unpromote travel as ordinary `col_update` (LWW) | `bindings/.../columns.rs:1027-1040` |
| Subscriptions, personal layout | local only; no conflict | section 2.4 |

Only `vol:` rows travel; `native:` rows are skipped at broadcast and at snapshot (`bindings/src/services/mesh.rs:495-504`,
`snapshot_worker.rs:407-443`). The mesh rejects peers of another epoch (`http_server.rs:207-214`).

---

## 10. Invariants defended, and rough spots

### 10.1 Invariants the code defends

- Paths are stored only in identity form; every entry point converts, idempotently (`core/src/columns.rs:89-97`).
- A column's uuid never changes after creation; incoming updates `COALESCE` it (`core/src/mesh/commands.rs:346,456`).
- Active duplicate columns on one natural key are collapsed every boot, preferring the promoted one (`db.rs:1141-1239`).
- Untrash never creates two active columns with one name (`columns.rs:846-874`).
- Duplicate refuses the source's own name, case-insensitively (`bindings/.../columns.rs:565-576`).
- A template write never leaves the destination missing; a restore never overwrites a live template
  (`templates.rs:965-1023,1409-1424`).
- A missing remote manifest never blanks the local cache (`templates.rs:1137-1145`).
- The destructive path migration never runs without a backup and is transactional (`db.rs:1061-1139`).
- Unsubscribe and subscribe never touch shared metadata (`subscription.rs:110-118,149-152`).
- Deep links never subscribe over an existing subscription or to a file (`bindings/.../subscription.rs:112-138`).
- The board passes each item's tracked state explicitly because the bulk call defaults a missing `isTracked` to TRUE
  (`ItemListPanel.qml:1199-1203`, `bindings/.../subscription.rs:742`).
- Template pulls and option reconciles never run on the UI thread and are coalesced (`bindings/.../columns.rs:1423-1469`).

### 10.2 Rough spots

No `TODO` / `FIXME` / `HACK` markers exist in the audited files (searched). The problems are structural:

Data loss and orphans

1. Metadata is keyed by path; rename, move, copy and delete - even the app's own Rename - orphan or strand it (4.3).
   Tracked orphans stay in the Tracker forever; there is no cleanup and no item tombstone writer.
2. Whole-blob last-writer-wins loses concurrent edits to different cells of one item (section 9).
3. Editing a column wipes `default_value` (the dialog blanks the field, the binding maps "" to NULL)
   (`ColumnEditor.qml:62`, `bindings/.../columns.rs:984`); and it resets the shared `is_visible` to true
   (`ColumnEditor.qml:63,473`).
4. Re-adding a column whose name matches a TRASHED row updates that row and leaves `trashed_at` set: the "new" column
   is invisible (`core/src/columns.rs:379-409` selects without a trash filter and clears only `deleted_at`).
5. A template CAS conflict is logged and dropped; the next folder-open reconcile then overwrites the local options
   with the template's - the user's option edit disappears silently (`bindings/.../columns.rs:1739-1746`,
   `core/src/columns.rs:592-608`).
6. `duplicate_column`'s value copy and `permanently_delete_column`'s key scrub are direct SQL updates with a new
   `modified_time` and no per-item broadcast (`bindings/.../columns.rs:715-734,1158-1175`); peers get them only through
   the snapshot, and the bumped timestamp can overwrite a peer's newer edit to another cell.
7. Backup restore: columns come back unpromoted (hidden) because `add_column` does not write `template_hash`; a column
   renamed after creation gets a different derived uuid, so its restored values no longer match; every restored item
   gets "now" as its timestamp and so beats newer peer edits (`bindings/.../backup.rs:199-238`,
   `core/src/columns.rs:374-375,388-427`).
8. A cell edit for a column name that does not exist in that item's (job, folder) writes a NAME-keyed value
   (`bindings/.../subscription.rs:625-627`). The aggregated tracker merges columns by name+type across folders
   (`TrackerView.qml:455-457`), so its bulk edit hits exactly this when a selected item's folder lacks the column.
9. `project_slug_for` receives three different spellings of the same job: the native path on add
   (`bindings/.../columns.rs:1516,1543,1569`), the identity string on promote, duplicate and the v2 backfill
   (`:542,590,1114-1123,2064`). Unless the job sits under the farm path, the slug falls back to "last two path
   components" (`core/src/utils.rs:139-144`), which differs between those spellings and between machines with
   different mount layouts - the same logical column can mint different template hashes. Follows from the code as
   read; not verified at run time.

Schema warts

10. No UNIQUE on the column natural key or on `column_uuid`; uniqueness is repaired at boot (`db.rs:801-817,1141`).
    `find_col_row` by uuid alone would match a row in any state (`commands.rs:258-268`).
11. `column_layout_personal` is keyed by column NAME: renaming a column drops the user's order/width/visibility
    (`db.rs:834-844`).
12. `column_options` has no order column and is delete-and-reinsert on every change (`columns.rs:405-436`); option ids
    are meaningless; the snapshot merge can re-add an option that was removed (section 9).
13. Dead schema: `subscriptions.is_active / sync_status / last_sync_time / shot_count`, `item_metadata.created_time /
    device_id`, `column_presets`, `name_conflict_marker`, backup `shotCount`. Dead code: `MetadataManager`,
    `ProjectConfig`, backup daily policy and eviction, `delete_item_metadata`, `get_tracked_items`,
    `reset_local_columns_and_presets`, the whole-blob `set_item_metadata` invokable (no QML caller found).
14. `folder_name` is a bare basename: two folders with one name in different places of a job would share columns.
15. Comments contradict code: v2 template path (2.1); `classify_all_path_columns` lists `column_layout_personal` and
    `subscriptions` as synced tables (`db.rs:394-398`); the delete-column confirm still says values "re-appear as a
    discovered column" although delete now trashes and the discovered pass is gone (`ColumnEditor.qml:685`,
    `ColumnManagerDialog.qml:150-160`); the backup dialog's "not mesh-synced" line (section 7).

Duplicated logic between Rust and QML

16. Job folder-name sanitising (`jobs.rs:14-35` vs `NewJobDialog.qml:40-55`, differing on whitespace runs).
17. Placeholder filtering `_t_` / `000000` in QML (`ItemListPanel.qml:415-421`) vs add-mode detection in Rust
    (`file_ops.rs:1168-1206`).
18. Path normalisation for comparing job paths: three variants (`TrackerView.qml:285-287`, `FileBrowser.qml:120-131`
    lower-case only, the per-job tracker's exact `!==` at `TrackerView.qml:312`) plus the Rust one.
19. Column refresh, sorting, width override and `_cellValue` are copied between `ItemListPanel.qml` and
    `TrackerView.qml`; `_isDarkColor` between `CellEditor.qml` and `KanbanBoard.qml` (noted in `KanbanBoard.qml:204-205`).
20. The "discovered column" branches remain in the manager's delegate although nothing produces such entries.

Performance on many items

21. Every metadata write re-reads ALL tracked items of ALL jobs, remaps each blob and serialises one JSON string
    (`bindings/.../subscription.rs:493-535,660`); the Tracker re-parses it, and its `metadata_rev` handler calls
    `refresh_tracked` a second time (`TrackerView.qml:527-530`).
22. The Tracker's sort comparator `JSON.parse`s both rows' blobs per comparison (`TrackerView.qml:364-368`); each row
    delegate parses its blob again (`:1221-1224`).
23. The item list refetches the whole folder's metadata on every `metadata_rev`, from any panel (`ItemListPanel.qml:183`).
24. Bulk edit = per item: read, modify, upsert, spawn a broadcast; no transaction (`subscription.rs:737-785`).
25. `get_column_defs` runs one options query per column (`core/src/columns.rs:213-232`); `Columns.invalidate` clears the
    cache for every folder.
26. Each path conversion calls `AppSettings::load()` (`core/src/columns.rs:94-97`, `subscription.rs:78-81`); whether
    that hits the disk each time was not verified.
27. `lookup_template` reads one template file from the share per manifest match, and opening the column manager pulls
    templates synchronously, on the UI thread (`bindings/.../columns.rs:920-945`, `ColumnManagerDialog.qml:169-176`).
28. `is_shared_folder` queries subscriptions and bookmarks and reloads settings on every column write (`:1846-1924`).

---

## 11. Compatibility surface - what a rebuild must keep

To keep existing users' data and to interoperate with machines still on 1.4.2. Items marked (wire) are the mesh
reader's protocol; they are listed because this area defines their payloads.

Local database

1. File `ufb_v5.db` in the app data dir; WAL; tables and columns of section 2.3 exactly (names, types, defaults), or a
   one-way import from them. `PRAGMA user_version` = 4 on current databases.
2. Semantics of `deleted_at` / `trashed_at` / `template_hash` (4.4), `is_tracked`, ms timestamps.
3. `browser_folder_prefs` keys `activeSubtab` (`::tracker`), `ilpNameColWidth`, `ilpViewMode`, `ilpBoardColumn`, `trk*`,
   and the row key `view:tracker`, if view memory is to carry over.

Identities (byte-exact, or peers split their data)

4. Path storage strings: `vol:{uuid}/{rel}`, `vol:{uuid}`, `native:{win|mac}/{path}`; rel with forward slashes, case
   preserved; prefix matching rules of `identity.rs:215-237`.
5. `volume_uuid`: UUIDv5, namespace `9e1d4f60-8b2a-4c17-a3e5-6f0c2d7b41a8`, name `"<win>|<mac>"` normalised as in
   `volumes.rs:21-30`; derived from the path mappings in `settings.json` (`win`, `mac`, `enabled`).
6. `column_uuid`: UUIDv5, namespace `4b2c9a17-6f3d-4e88-b1a0-5c7e9d2438f1`, name = identity job path, folder name,
   lower-cased column name, lower-cased type, joined by `0x1F`.
7. `template_hash`: UUIDv5, namespace `7c1e3b95-42a8-5d6f-9e04-8a172c5b6d39`, name = project slug, name slug,
   lower-cased type, joined by `0x1F`; random v4 for forks. Slug functions `templates.rs:91-126` and
   `utils.rs:120-146` with its `slug_path_segment` helper just below (the 64-character cut included, per its doc
   comment at `utils.rs:148-150`; the helper's tail was not read line by line).
8. `folder_name` = the folder's basename, case as on disk.

Value encoding

9. `metadata_json`: a JSON object keyed by `column_uuid` (unknown keys preserved verbatim); value types per 4.2 -
   option NAME strings, epoch-ms local-midnight dates, booleans, numbers, `null` for cleared, one newline/comma
   separated string for links.
10. Column type strings: `text, dropdown, date, priority, number, checkbox, links, notes` (accept `note`, any case).

Files on shares

11. `<farm>/v6/templates/<hash>.json` and `_index.json`: field names, camelCase, `schemaVersion: 2`, the `revision` and
    `manifestRevision` CAS discipline, temp-file naming that other versions' listings ignore (`*.json.tmp*`,
    `*.json.bak.*` do not end in `.json`), read fallback to `<farm>/v5/templates`.
12. `<farm>/ufb/presets/<slug>.json`: `schemaVersion: 1` shape of 4.5.
13. `<job>/.ufb/backups/backup_*.json` (`version: 1`) and `backup_metadata.json`, if old backups must stay restorable.
14. The job tree produced by New Job and Add Shot (folder names, `_t_*` / `000000*` conventions, `<name>_v001.<ext>`,
    `<YYMMDD><letter>_<name>`), since existing jobs and other tools rely on the layout; and hiding `_t_*` / `000000*`.

Replication payloads (wire)

15. Metadata edit: `{job_path, item_path, metadata, folder_name, is_tracked, modified_time, mesh_epoch}` with identity
    paths, the full uuid-keyed blob as a string, apply rule `>=` (`http_server.rs:186-201,239-249`).
16. Table changes: `{action: "col_add"|"col_update", def: "<ColumnDefinition as a JSON string>", modified_time}` and
    `{action: "col_delete", job_path, folder_name, column_name, column_uuid, modified_time, deleted_at}`;
    `ColumnDefinition` camelCase fields of `columns.rs:23-60` (`templateHash` / `trashedAt` omitted when null); legacy
    `sub_add`, `sub_remove`, `preset_save`, `preset_delete` must be accepted and ignored (`commands.rs:279-551`).
17. Snapshot database tables `column_definitions`, `column_options`, `item_metadata` with the column lists of
    `snapshot_worker.rs:424-443`, an empty `subscriptions` table, `vol:` rows only; merge rules of section 9. Epoch 6,
    folder `<farm>/v6`.

Behaviour other machines depend on

18. A column without a reachable template is hidden, not shown with stale options; options come from the template.
19. Subscriptions, order, width, visibility stay local. Unsubscribe never deletes shared data.
20. Permanent column delete removes the key from item blobs; trash does not.

---

## Open questions for the owner

1. "Stay similar": must the rebuild interoperate LIVE with 1.4.2 machines (same epoch, same snapshot, same template
   files), or is a one-time import of `ufb_v5.db` plus a new epoch acceptable? The app has already made two clean
   breaks (`ufb_v5.db`, farm `v6`).
2. Path-keyed metadata: keep it (byte compatibility), or give items a durable id so rename / move keep their metadata?
   If kept, should the app's own Rename, Move and Delete at least carry or tombstone the row?
3. Is whole-row last-writer-wins acceptable, or is per-cell merge wanted? Per-cell changes the wire payload.
4. Promotion: is "unpromoted columns are hidden" a rule users rely on, or an artefact? Should Promote / Unpromote
   remain user-visible actions?
5. Templates vs presets: two shared stores, both called "templates" in the UI. Keep both?
6. `default_value`: implement it (apply to new or untracked items), or drop the field?
7. `priority`: should it gain an order (sorting, lanes), or stay a renamed dropdown?
8. Dead fields (`shot_count`, `sync_status`, `device_id`, `created_time`): keep the columns for compatibility and
   start populating `device_id` / author, or leave them empty?
9. Authorship: is recording who changed a value wanted? Today only templates carry an OS login name - is writing
   login names to the share acceptable?
10. Backups: manual-only today; the daily policy and retention exist unused. Wanted? Should restore keep original
    timestamps and promotion state, and should the dialog's "per device" wording be fixed?
11. Legacy `.ufb/projectConfig.json` in old jobs: ignore, or import its status options and users?
12. The project template tree: keep it bundled and fixed, or make it a user/studio-editable location? Should New Job
    stop copying the placeholder folders and `.gitkeep` files into jobs?
13. Jobs with the same folder referenced in different letter case, and shares mapped by bare drive letter, split or
    skip sync today (documented hazards). Fix in the rebuild (changes identity strings) or preserve?
14. Is the per-user nature of "shared folder" (a column edit broadcasts only if THIS user subscribed, or the job is
    under a flagged jobs folder) intended?
15. Tracker scope: per-job trackers show tracked items even when not subscribed; the aggregated one only subscribed
    jobs; orphaned tracked rows never leave. Intended?
16. Keyboard: the tracker, item list and board have no keyboard navigation today. Parity, or add it?
