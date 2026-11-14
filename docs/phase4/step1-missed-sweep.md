# Phase 4, step 1 - minRender: the sweep for what step 0 missed

Read 2026-11-14 from the MinRender repository, branch `min-paths` (release 0.5.11 plus the path-library change), HEAD
`9526557`, read-only. Nothing was built or run; statements marked **(by reading)** were not reproduced. Paths are
relative to the MinRender repository root unless they start with `crates/`, `apps/` or `docs/phase4/` (this
repository). Line numbers are those of that tree.

Method: the four step-0 notes were read in full; every tracked file was walked (`git ls-files`, 506 files, of which 277
are vendored image-library headers and binaries under `external/install`); all 245 commit subjects were read and the
commits naming a behaviour absent from the notes were opened; each finding was checked against the notes.

Three areas are being read in depth by other step-1 readers and are only cross-linked here: the jobs / chunks / preview
UI; the tray, window lifecycle, single instance, start at login and the restart helper; the RNDR companion
(`tools/rndr-companion`).

Samples, comments and scripts in the source name a facility, machines, a developer account and local paths. None is
repeated here.

Short names for the step-0 notes: **PA** = `step0-process-architecture.md`, **CT** = `step0-contracts.md`,
**UI** = `step0-ui-inventory.md`, **KP** = `step0-kill-path.md`.

---

## A. Coverage table

"Well" = a porter could work from the note alone. "Thin" = named, but details a port needs are absent or wrong.
"Missed" = not in any note.

| # | Subsystem / feature / file group | Files | Verdict | Where covered | Section here |
|---|---|---|---|---|---|
| 1 | Discovery: phonebook, restart file, advertised address, liveness poll, UDP | `src/monitor/peer_manager.*`, `src/core/udp_notify.*`, `net_utils.*`, `peer_info.h` | well | CT 1 | - |
| 2 | HTTP server, bearer check, client timeouts, the 38 routes | `src/core/http_server.*` | well | CT 2 | B13 (two small additions) |
| 3 | Election, snapshot, takeover, split brain, versions | `peer_manager.cpp`, `dispatch_manager.cpp`, `monitor_app.cpp` | well | CT 3 | - |
| 4 | Manifest, job ids, chunking, chunk and job state machines, queue order, dispatch cycle | `job_types.h`, `database_manager.*`, `dispatch_manager.*` | well | CT 4.1-4.6 | - |
| 5 | Failure tracker and suspension | `node_failure_tracker.h` | well | CT 4.7 | - |
| 6 | Edits, the epoch fence, resubmission | `monitor_app.cpp` | well | CT 4.8, 4.9 | - |
| 7 | Worker side, staging, node enable / disable | `render_coordinator.*` | well | CT 4.10-4.12 | B12 (what is NOT checked) |
| 8 | SQLite schema and migrations | `database_manager.cpp` | well | CT 5.1 | - |
| 9 | The data directory and its 0.5.8 move | `src/core/platform.cpp` | well | CT 5.2 | B6 (what else lives there, where messages go) |
| 10 | `config.json` keys | `src/core/config.h` | thin | CT 5.3 | B6 (load / save hazards, live edits) |
| 11 | Share layout, `farm.json`, bundled content refresh, command drop box, cleanup | `farm_init.cpp`, `monitor_app.cpp` | well | CT 6 | B2 (the plug-in side of the refresh) |
| 12 | Node log files on the share | `src/core/monitor_log.*` | thin | CT 6.4 | B7 |
| 13 | Render output logs | `render_coordinator.cpp` | well | CT 6.5 | - |
| 14 | Template schema, baking, runtime tokens, regex dialect | `template_manager.*`, `job_types.h` | well | CT 7.1, 7.2, 7.4, 7.5 | - |
| 15 | Submit-time pattern tokens (`default_pattern`, `frame_padding`) | `template_manager.cpp:201-282` | **wrong** | CT 7.3 describes them as live | B3 |
| 16 | The six shipped templates, flag by flag | `resources/templates/**` | thin | CT 7.2 (ids and flag ids only) | B4 |
| 17 | `resources/templates/backups/` | one file | thin | CT 6.3 (one clause) | B4 |
| 18 | Path translation | `path_mapping.h`, `cmake/MinPaths.cmake` | well | CT 7.6 | - |
| 19 | DCC plug-ins: the drop-box contract | `resources/plugins/**`, `submission_watcher.*` | thin | CT 8.2 (the file format and the watcher) | B1 |
| 20 | DCC plug-ins: what each one does inside its host, its UI, versions, installation | `resources/plugins/**` | **missed** | - | B1, B2 |
| 21 | Process list, agent IPC, lifetimes, threads, port options | `mr-agent/**`, `agent_supervisor.*`, `ipc_server.*` | well | PA 1-7, CT App. A | - |
| 22 | The kill path | `mr-agent/src/kill.rs`, `executor.rs` | well | KP | - |
| 23 | `minrender-headless` | `main_headless.cpp`, `system_tray.cpp` | well | PA 1, 5 | B8 (no arguments at all) |
| 24 | Command-line arguments, all binaries | several | thin | PA (scattered) | B8 |
| 25 | Environment variables | several | **missed** (only `TMPDIR`) | PA 7.7 | B9 |
| 26 | Window, tray, menus, panels, cards, models, widget gap | `src/ui/**` | well | UI 1-3 | another reader |
| 27 | Preview decoding | `src/preview/**` | well | UI 5 | another reader |
| 28 | Tray, single instance, start at login, restart helper, session end | `src/ui/platform/*`, `src/core/single_instance.*`, `src/restart/main.cpp` | well | PA 4, UI 1.1-1.2, 4 | another reader |
| 29 | RNDR companion | `tools/rndr-companion/**` | thin | PA 1, CT 5.2 (the registry key) | another reader; C lists what it depends on |
| 30 | Fonts, theme tokens, formatters | `resources/fonts`, `Theme.qml`, `Format.qml`, `main_qt.cpp` | thin | UI 1.1 (two lines) | B14 |
| 31 | Localisation | `src/ui/**` | well | UI 4 | - |
| 32 | UI state store (Qt `Settings`) | QML `Settings {}` blocks | thin | UI 1.1, CT 5.2 | B6 |
| 33 | Security model | `SECURITY.md`, `http_server.cpp`, `platform.cpp` | thin | CT 2.2, PA 3 (quoted in passing) | B10 |
| 34 | Firewall | `installer/msix/AppxManifest.xml.in`, `platform.cpp:228-310` | thin | PA 1 (the two declared rules) | B10 |
| 35 | Updater, appcasts, release channels | `docs/appcast*.xml`, `scripts/update_appcast.sh`, `src/ui/updater/*`, `cmake/MacOSXBundleInfo.plist.in` | thin | PA 4 ("Updater") | B15 |
| 36 | Version numbers and where they must agree | several | **missed** | - | B16 |
| 37 | Build system | `CMakeLists.txt`, `cmake/*`, `scripts/build_msvc.bat` | thin | PA 1 (targets) | B17 |
| 38 | Windows packaging and the Store history | `scripts/package.bat`, `scripts/make_msix.ps1`, `installer/**` | thin | PA 1, 6 | B17, B18 |
| 39 | macOS release | `scripts/macos-release.sh`, `fetch_sparkle.sh`, `entitlements.plist` | thin | PA 1 | B17 |
| 40 | Third-party libraries and licences | `THIRD_PARTY_NOTICES.txt`, `resources/fonts/*LICENSE*`, `LICENSE` | **missed** | CT conventions (three library versions) | B19 |
| 41 | The manual as a list of promises | `docs/*.md`, `README.md` | thin | CT 11.3 f-h, UI 6 f-h | D |
| 42 | The documentation site, privacy page, Store listing text | `docs/_config.yml`, `docs/CNAME`, `docs/privacy.md`, `installer/STORE.md` | **missed** | - | B15, B18 |
| 43 | Retired pieces still in the field (Inno installer, WinSparkle, in-app RNDR supervisor, Tauri UI) | git history | thin | PA 3, CT 5.3 (old keys) | B18 |
| 44 | Tests | `mr-agent/src/kill.rs` | well | PA 6 | B20 |
| 45 | Power / sleep, disk space, licences, notifications, e-mail, webhooks, wake-on-LAN, job dependencies, pools | - | **missed** as explicit "absent" | UI ("no notifications"), CT 4.6 ("no pools") | B12 |
| 46 | Icons and brand assets | `resources/icons/*`, `installer/msix/Assets/*` | **missed** | - | B14 |
| 47 | `external/install` (vendored macOS image libraries, committed) | 277 files | thin | UI 5 | B19 |

---

## B. What was missed or covered thinly

### B1. The DCC submission plug-ins

Three single-file scripts, one per host. CT 8.2 gives the JSON they write; nothing in step 0 says what they do to get
there, what the user sees, or what they assume about the app.

**How they find the app (all three).** No network, no registry, no executable lookup, no check that the app is running.
They locate the data folder by rule, read `config.json`, take `sync_root`, and require the directory
`<sync_root>/MinRender-v2` to exist (the literal is in each file). Then they create `submissions/` if missing and write
one JSON file per job. Folder rule on Windows: `%USERPROFILE%\minRender` if it holds `config.json` **or** the old
`%LOCALAPPDATA%\MinRender` does not exist; otherwise the old one. macOS: `~/Library/Application Support/MinRender`.
Linux (the two Python ones): `$XDG_DATA_HOME/MinRender`, else `~/.local/share/MinRender`.

| | Blender add-on | Cinema 4D script | After Effects script |
|---|---|---|---|
| File | `resources/plugins/blender/MinRender.py` (395 lines) | `resources/plugins/cinema4d/MinRender.py` (402) | `resources/plugins/afterEffects/MinRender.jsx` (434) |
| Kind | registered add-on (`bl_info`, min. Blender 4.0, `:7-15`) | Script Manager script; a modal dialog per run (`:396-398`) | ScriptUI panel when in `ScriptUI Panels`, else a palette window (`:359`) |
| Where it shows | Properties > Render > "MinRender" panel, closed by default (`:286-292`) | "MinRender 0.5.11" dialog, 420 x 320 | Window > MinRender |
| Version shown | `bl_info.version` (0, 5, 11) in the add-on list | `PLUGIN_VERSION` in the dialog title (`:16`, `:139`) | `SCRIPT_VERSION` in the palette title (`:13`); not shown when docked |
| Data folder rule | `get_config_dir` `:29-45` (environment `USERPROFILE`, `LOCALAPPDATA`, `XDG_DATA_HOME`) | `:23-38`, same | `getDataDir` `:102-111` (`Folder("~")`, `Folder.userData`); no Linux |
| Reads `config.json` with | `json.load` | `json.load` | **`eval("(" + text + ")")`** (`:144`) |
| Template id sent | `blender-5.2-plugin` (`:78`) | `cinema4d-2026-plugin` (`:68`) | `after-effects-plugin` (`:309`) |
| Saves the document first | yes, `wm.save_mainfile` (`:207`); refuses an unsaved file | yes, `SaveDocument` (`:302-304`); refuses an unsaved file | yes, `app.project.save()` (`:284`); refuses an unsaved project |
| One submission = | one job per scene (current scene, or every scene with "All Scenes") | one job (the chosen Take) | one job per ticked output module of every QUEUED render-queue item |
| Job name | file stem, or `<stem> - <scene>` when several | file stem, plus ` - <take>` when not the Main take | `<project stem> - <comp>`, plus ` (OM<n>)` for the 2nd and later output modules |
| Overrides sent (template flag ids) | `scene_file`, `output_path`; `scene_name` only when several scenes or the scene is not named "Scene" (`:241-248`) | `scene_file`, `output_path`; `take_name` when not Main; `output_format` when the scene's format id is in a 9-entry table (`:96-106`, `:345-352`); never `threads` | `project_file`, `comp_name`, `output_path`; `rs_template`, `om_template` when a named template is chosen (`:298-304`) |
| Frame range | scene range, or an "Override Range" pair (min 0) | render-settings range through the document fps, or an override pair | `round(timeSpanStart * fps)` to `round((start + duration) * fps) - 1`, per render-queue item (`:194-196`) |
| Output path | `scene.render.filepath` made absolute, or an override field | render-settings path, joined to the document folder when relative, or an override field; refused when empty | the output module's file, as the host reports it |
| Chunk size | 1-10 000, default 10 | 1-10 000, default 10 | text field, default 10; **a video output (`mov mp4 avi mkv wmv m4v webm mxf`) is sent with chunk size = the whole range** (`:49-51`, `:292`) |
| Priority | 1-**100**, default 50, tooltip "**higher = rendered first**" (`:104-110`) | 1-100, default 50 | free text, default 50, no bounds |
| File name | `<13-digit ms>.<hostname>.json`; `ms + n` for the n-th job of one click | `<13-digit ms>.<hostname>.json` | `<13-digit (ms + i)>.<hostname>.json`, joined with `/` even on Windows (`:327-328`) |
| Write | `.tmp` then `os.replace` (`:268-272`) | `.tmp` then `os.replace` (`:382-386`) | **direct** `open("w")` / `write` / `close` (`:331-335`) |
| Host name from | `socket.gethostname()` | `socket.gethostname()` | `system.callSystem("hostname")`, `"unknown"` on failure (`:53-59`) |
| After a click | a 2 s cooldown, button reads "Submitted!" (`:165-168`, `:361-369`) | status line "Submitted: <name>" | list cleared, Submit disabled until the next scan |
| Messages when not connected | "MinRender config not found." / "Set sync root in MinRender Monitor." / "Farm folder not found." | "MinRender config not found." / "Sync root not set in Monitor." / "Farm not initialized." | "Config not found. Is MinRender installed?" / "Failed to parse config." / "Sync root not configured in MinRender." / "Farm not initialized." / "Save the project first." / "No queued items in render queue." |

Things a port has to know that follow from this:

1. **Three installed scripts read `config.json` directly, one of them with `eval`.** The file's location, its name, the
   key `sync_root`, valid JSON that is also a valid JavaScript expression, and the directory name `MinRender-v2` are a
   contract with scripts that sit in users' Blender, Cinema 4D and After Effects installations and are updated by hand.
   The app writes that file in place, not through a temporary file (`monitor_app.cpp:248-261`); a plug-in reading it
   during a save sees a partial file and reports "config not found / failed to parse".
2. **Nothing is validated on the plug-in path.** `SubmissionWatcher::processSubmission`
   (`submission_watcher.cpp:69-275`) does not call `validateSubmission`: required flags may be empty, the executable for
   this system may be empty, and `frame_start > frame_end` produces a job with no chunks, which the leader marks
   completed at once (CT 4.5).
3. **A mistyped field crashes the app, repeatedly.** `sub.value("frame_start", 1)` and its four siblings
   (`submission_watcher.cpp:87-93`) are outside any `try`, on the 50 ms pump, which has no handler
   (`monitor_app.cpp:77-200`, `main_qt.cpp`). A submission whose `frame_start`, `frame_end`, `chunk_size`, `priority`,
   `template_id` or `job_name` has the wrong JSON type throws before the file is moved, so the next start meets the same
   file **(by reading)**. The shipped plug-ins always send the right types; a hand-made file or a future plug-in may not.
4. **A failed move resubmits every 3 s.** Every exit path ends in `fs::rename(...)` into `submissions/processed/` with
   the error ignored. If the rename fails (a read-only legacy folder, a file held open, a different volume) the same file
   is processed again on each poll and a new job (`-2`, `-3`, ...) is submitted each time **(by reading)**.
5. **The submitter's machine must run the app** (the drop box is local) **and its user must be the one the app runs
   as** (the folder is per user).
6. **The plug-ins never read templates** (a comment in the After Effects script says the farm folder is checked "for
   template scanning validation"; nothing is scanned). The template id is a literal in each script, and carries the host
   version in its name (`blender-5.2-plugin`, `cinema4d-2026-plugin`). Retargeting a host version means a new template
   file AND reinstalled plug-ins on every workstation (release notes for 0.5.2, `docs/appcast.xml:148-158`).
7. **Paths are sent in the submitter's native form.** The app canonicalises on macOS (CT 7.6). Blender's `//` prefix is
   resolved by the add-on; Cinema 4D tokens and After Effects `[#####]` are passed as the host gives them.
8. **Priority semantics differ between the plug-ins and the app**: see D2.
9. Hosts and versions targeted today: Blender 5.2 (add-on declares 4.0 as its minimum), Cinema 4D 2026, After Effects
   2026 on Windows / 2025 on macOS in the plug-in template (D2).

### B2. How plug-ins and templates are deployed and kept current

CT 6.2 / 6.3 give the rule on the share. The whole chain, which no note states:

1. The plug-in and template sources ship **inside the app**: macOS `Contents/Resources/{plugins,templates}`, Windows
   `<package>/resources/{plugins,templates}` (`farm_init.cpp:84-99`; `CMakeLists.txt:318-342`; `scripts/package.bat:81-82`).
2. At each farm start a node whose version is **numerically greater** than `farm.json`'s `last_example_update` copies
   them to `<farm>/plugins/<host>/`, `<farm>/templates/examples/`, `<farm>/templates/plugins/`, deletes example and
   plug-in template files it does not ship, logs every copy to its node log (category `farm`: "Copied plugin: ...",
   "Copied template: ...", "Pruned stale template: ...", "Updating examples (a -> b)", "Farm examples are from newer ...;
   leaving them"), and stamps the version only when every copy succeeded (`farm_init.cpp:134-264`, `:349-379`).
3. **The app never installs a plug-in into a host.** The user copies the file from `<farm>/plugins/` by hand
   (`docs/submit.md:31-48`): Blender "Install from Disk"; Cinema 4D into the user scripts folder; After Effects into
   `Scripts/ScriptUI Panels`. There is no in-app "install plug-ins" action, no check of the installed plug-in's version,
   and no warning when it is older than the app.
4. Plug-in versions are bumped with every app release even when the plug-in text is otherwise unchanged (0.5.10 notes),
   so the version in a host tells the operator which copy is installed.
5. A release whose bundle carries stale plug-ins poisons the share for a whole version (0.5.8 on macOS did). Two guards
   were added: resource files are link inputs (`CMakeLists.txt:336-342`), and the macOS release script refuses to
   continue when the bundle's `plugins` / `templates` differ from the source tree (`scripts/macos-release.sh:90-99`).
   There is no such check in the Windows packaging scripts.
6. Nodes at 0.5.5 and older copy their own files over the share at **every** start, whatever is there (release notes
   for 0.5.9). On a farm that still has such nodes the port's files can be replaced by 0.5.5's.
7. The retired Inno installer could install the plug-in sources as optional components into the program folder only
   (history, `installer/minrender_installer.iss`); it never touched a host either.

### B3. Correction to CT 7.3: the submit-time pattern tokens are dead code

`TemplateManager::resolvePattern` (`src/monitor/template_manager.cpp:201-282`) has **no caller**. Its last caller was the
ImGui submission form, removed by the Qt port (`git log -S'resolvePattern('`: only the first commit and `72ce7d8`). The
Qt form receives each flag's `flag, value, info, help, editable, required, type, filter, id`
(`AppBridge::templateById`, `src/ui/app_bridge.cpp:1276-1335`); `default_pattern` is parsed (`job_types.h:386-387`) and
never passed on. `frame_padding` is used only inside `resolvePattern`.

Consequences in 0.5.11:

- The tokens `{frame_pad}`, `{project_dir}`, `{file_name}`, `{flag:<id>}`, `{date:...}`, `{time:...}` and the separator
  clean-up **do nothing**. The Output Path field of New Job is empty until the user types a path.
- `default_pattern` and `frame_padding` in the shipped templates, and the whole "Pattern tokens" section of
  `docs/templates.md:149-170`, describe a feature the shipped app does not have.
- `TemplateManager::buildCommandPreview` (`template_manager.cpp:536-603`, a command-line preview with the same
  optional-flag rule) is likewise unused.
- CT 11.1 row 41 ("Submit-time pattern tokens and the separator clean-up order") is therefore not a conformance item
  against 0.5.11; it is a decision (restore the feature, or drop the keys from the reference).
- Had it been live, the documented example would still be wrong: the function does not normalise `..`, and its clean-up
  turns the literal `--` in the After Effects example's pattern into `-`.

### B4. The shipped templates, flag by flag

All six say `"_version": 2`. Flags not marked editable are fixed; "opt." = editable, not required (dropped with its
preceding switch when left empty, CT 7.5).

| Template (file under `resources/templates/`) | Executable, Windows / macOS / Linux | Arguments in order | Defaults (range, chunk, retries, timeout) | Progress | Per-frame "done" pattern | Output detection | Tag |
|---|---|---|---|---|---|---|---|
| Blender 5.2 example (id ends in a site-specific word) | `C:/Program Files/Blender Foundation/Blender 5.2/blender.exe` / `/Applications/Blender.app/Contents/MacOS/Blender` / `/usr/bin/blender` | `-b <scene_file*>` `-o <output_path*>` `-F OPEN_EXR_MULTILAYER` `--python-exit-code 1` `-s {chunk_start}` `-e {chunk_end}` `-a` | 1-100, 10, 3, none | fraction `Rendering (\d+)\s*/\s*(\d+) samples`; fraction `Rendered?\s+(\d+)/(\d+)\s+Tiles` | `Saved: '?(.+)'?` | `Saved: '?(.+\.[a-zA-Z0-9]+)'?`, group 1, `exists_nonzero` | `blend` |
| `plugins/blender-5.2-plugin.json` | same | as above plus `-S <scene_name opt.>` after the scene file | same | same | same | same | `blend` |
| After Effects example (id ends in the same word) | `.../Adobe After Effects 2026/Support Files/aerender.exe` / `/Applications/Adobe After Effects 2026/aerender` / none | `-project <project*>` `-comp <comp_name*>` `-output <path*>` `-OMtemplate <opt., default "Multi-Machine Sequence">` `-RStemplate <opt.>` `-v ERRORS_AND_PROGRESS` `-s {chunk_start}` `-e {chunk_end}` `-reuse` `-continueOnMissingFootage` | 0-100, 20, 3, **1800 s** | percentage `PROGRESS:\s*.*\((\d+)\)` (the frame index used as a percentage) | `Finished composition` (once per process, so one frame per chunk is ever reported) | none, `exit_code_only` | `ae` |
| `plugins/after-effects-plugin.json` | Windows as above / **`/Applications/Adobe After Effects 2025/aerender`** / none | `-project <project_file*>` `-comp <comp_name*>` `-RStemplate <rs_template opt.>` `-OMtemplate <om_template opt.>` `-s {chunk_start}` `-e {chunk_end}` `-output <output_path*>` | 0-90, 10, 3, none | none | `PROGRESS:\s+\d+:\d+:\d+:\d+\s+\(\d+\)` | none, `exit_code_only` | `ae` |
| `cinema4d-2026-all-options.json` | `C:/Program Files/Maxon Cinema 4D 2026/Commandline.exe` / `/Applications/Maxon Cinema 4D 2026/Commandline.app/Contents/MacOS/Commandline` / `""` | `-nogui` `-render <scene_file*>` `-take <opt.>` `-oimage <output_path*>` `-omultipass <opt.>` `-oformat <opt.>` `-oresolution <width opt.> <height opt.>` `-threads <opt.>` `-redshift-gpu <opt.>` `-frame {chunk_start} {chunk_end} 1` | 0-90, 10, 3, none | percentage `Progress:\s*(\d+)%` | `Rendering Phase: Finalize` | none, `exit_code_only` | `c4d` |
| `plugins/cinema4d-2026-plugin.json` | same | `-nogui` `-render <scene_file*>` `-take <take_name opt.>` `-oimage <output_path*>` `-oformat <output_format opt.>` `-threads <threads opt.>` `-frame {chunk_start} {chunk_end} 1` | same | same | same | same | `c4d` |

Notes:

- `frame_padding` is `####` (Blender, Cinema 4D), `[####]` (After Effects example), `[#####]` (After Effects plug-in) -
  and unused (B3). The preview's file finder works from the `#` runs in the output flag's value itself (CT 9).
- Each template carries 5 to 11 `error_patterns`; none is evaluated (CT 11.3 f).
- **Every Blender job is forced to multilayer EXR**: `-F OPEN_EXR_MULTILAYER` is a fixed flag in both Blender templates,
  and the add-on sends no format. A scene set to PNG renders EXR on the farm.
- The plug-in template's `filter` for After Effects is `"aep,aepx"`; the form builds the picker filter as
  `<filter> (*.<filter>)` (UI 1.11), which is not a valid pattern for two extensions. It shows only when such a job is
  edited.
- In the all-options Cinema 4D template the width and height are two optional positionals after one switch. With only
  the width filled the command line is `-oresolution <w>` (the dropping rule looks one entry ahead).
- `resources/templates/backups/blender-5.0-all-options.json` (117 lines, 30 flags) is shipped inside the app's resources
  on both systems (the whole `resources` tree is copied) but never copied to the share: it is a reference of every
  Blender switch the author once exposed, and the only "all options" Blender template.
- The example templates are the ones the manual tells users to copy (`docs/submit.md:23`); a port that prunes or renames
  them changes what `templates/examples/` holds on every farm (CT 6.3).

### B5. "DCC gate"

The phrase belongs to the RNDR companion (another reader). For the app itself: 0.5.4 / 0.5.5 had an opt-in gate inside
the app (keys `rndr_dcc_gate`, `rndr_block_processes`, `rndr_dual_mode` in `config.json`); 0.5.6 removed it with the
in-process supervisor (`src/core/config.h:62-65`, `SECURITY.md:167-176`). What remains in the app is only the surface
the companion reads (C, rows 13-16) and the fact that the app **drops those keys** from `config.json` at its next save,
which the companion's installer works around by reading the pre-0.5.8 copy of the file
(`tools/rndr-companion/install.ps1:95-104`).

### B6. Settings: the files, the hazards, the UI state store

CT 5.3 lists the eleven keys and their defaults. Not in step 0:

| Fact | Where | Why a port cares |
|---|---|---|
| A `config.json` that fails to parse (or has one mistyped key) is **silently replaced by defaults**: the load error goes to the process's standard error only, the app runs unconfigured, and the next save - at the latest at exit - overwrites the file | `monitor_app.cpp:226-261`, `:205-221` | a node loses its sync root, tags and mappings with no trace in any log file |
| The save is in place (`ofstream` on the final path), not temporary-file + rename | `monitor_app.cpp:248-261` | plug-ins and the companion read this file (B1) |
| Settings edits change the **live** configuration as the user types; "Cancel" restores a snapshot, "Save" writes | `app_bridge.cpp:160-256`, `:869-899` | while the card is open, `/api/status` already reports the edited tags; quitting with the card open saves the unsaved edits (exit calls `saveConfig`) |
| Tags are split on commas and trimmed, case kept | `AppBridge::setTagsCsv` | election compares exact strings (CT 3.1) |
| Ports are clamped to 1-65535 in the UI; `ip_override` is free text | `app_bridge.cpp:207-256` | |
| A partial `POST /api/config` that fails on a later key leaves the earlier keys changed in memory and unsaved | `http_server.cpp:901-961` | |
| What restarts the farm on Save: sync root, HTTP port, IP override, UDP on / off, UDP port. Not tags, not staging | `app_bridge.cpp:877-899` | CT 3.1 (tags), CT 11.3 q |
| The data folder also holds things the app does not own: `companion/`, `rndr-companion.json`, `rndr-companion.log` | `tools/rndr-companion/README.md` | a port must leave unknown entries alone; the 0.5.8 migration copies them too |
| The 0.5.8 migration reports only to standard error ("Migrated data folder from ...") | `platform.cpp:45-71` | invisible in the shipped builds |
| UI state (split sizes, panel visibility, tab, column widths) is Qt `Settings` under organisation and application `MinRender`, categories `panels`, `jobDetailLayout`, `chunkTableColumns`, `nodePanel`, `jobsTableColumns` | `main_qt.cpp:87-89`, `Main.qml:73-75`, panel files | Windows: a registry key beside the node-state key (`HKCU\Software\MinRender\MinRender` by Qt's rule, virtualised in the Store package); macOS: a preferences file. Not worth migrating; must not collide |
| Dead keys still written on every save: `auto_start_agent` (never read), `show_notifications` (read by nothing but the checkbox) | `config.h:50`, `:57` | keep writing them, or an older build run on the same machine resets them (CT 5.3) |

There is one configuration file. There is no per-farm settings file other than `farm.json`, no per-job settings beyond
the manifest, and no settings for the agent.

### B7. Logs: every file, where, how long, and what never reaches a file

| Log | Where | Written by | Rotation / cap | Shown in the app as |
|---|---|---|---|---|
| Node log | `<farm>/nodes/<node_id>/monitor-YYYY-MM-DD.log` | the app, one `open-append-close` per line (`monitor_log.cpp:119-137`) | files older than 7 days are deleted **only when the date rolls over while the app is running and a line is written** (`:121-127`, `:139-187`); a node that is shut down every evening never prunes | "Monitor Log" (live, from memory) and each node's entry (from the file) |
| In-memory ring | - | `MonitorLog` | 1 000 entries in the backend (`monitor_log.h:58`); the UI model keeps 5 000 | "Monitor Log" |
| Render output | `<farm>/jobs/<job>/stdout/<node>/<range>_<ms>.log` | the app, from agent messages | removed with the job | "Task Output" |
| Agent log | Windows `%USERPROFILE%\minRender\mr-agent.log` (from the **environment variable**, while the app uses the known-folder API); other systems `~/.local/share/MinRender/mr-agent.log` | `mr-agent` (`mr-agent/src/main.rs:81-134`), RFC 3339 times, level Info | deleted at agent start when over 2 MiB - the agent starts once per chunk | nowhere; named in `docs/install.md:46` |
| Companion log | `%USERPROFILE%\minRender\rndr-companion.log` | the companion | another reader | nowhere |
| Sparkle | the framework's own | Sparkle | - | - |

What a porter should take from it:

- **There is no local log file for the app.** File logging starts with the farm (`monitor_app.cpp:288`) and stops with
  it (`:444`). With no sync root, an unreachable share, or before the farm starts, nothing the app logs is written
  anywhere; the ring is lost at exit. The failures an operator most needs explained (the share cannot be reached) are the
  ones that leave no file.
- Messages printed straight to standard output / error never reach any log in the shipped builds: config load and save
  results, the data-folder migration, system information, QML warnings, CRT invalid-parameter reports
  (`monitor_app.cpp:231-260`, `platform.cpp:65-70`, `node_identity.cpp:159-162`, `main_qt.cpp`).
- Categories in use are `farm`, `job`, `render`, `db`, `dispatch`, `peer`, `agent`, `http`, `config`, plus **`app`** and
  **`node`** (CT 6.4 lists nine).
- The log viewer's remote source is the share file, not the network (CT 6.4); the manual says otherwise (D1).
- `SECURITY.md:97-106` sets a rule the port must keep: the farm secret, flags and environment never appear in any log.

### B8. Command lines

| Binary | Arguments | Notes |
|---|---|---|
| `minRender` | `--minimized` (start hidden); `--native-frame` (keep the OS title bar, a development aid) | read with `app.arguments().contains(...)` (`main_qt.cpp:217`, `:268`). Because the object is a Qt application, Qt's own switches (`-platform`, `-style`, `-qmljsdebugger`, ...) are also accepted; nothing documents or uses them |
| `minrender-headless` | **none** | `argc` / `argv` go only to Qt (`main_headless.cpp:31-41`). No `--minimized`, no config path, no port. It reads the same `config.json` and takes the same single-instance name as the app |
| `mr-agent` | `--node-id <id>` (required) | clap (`mr-agent/src/main.rs:20-24`); also `--help`, `--version` from clap |
| `mr-restart.exe` | `--pid <n> --exe <path> [--minimized]`, or none | another reader |
| Registered restart command (Windows) | `--minimized` | `app_restart.cpp:53` |

There is no command-line submitter, no "submit this file" argument, no file association, no URL scheme, and no headless
mode of the shipped app. The only programmatic ways in are the local drop box and the HTTP API with the farm secret.

### B9. Environment variables

| Variable | Read by | For |
|---|---|---|
| `HOME` | app, non-Windows (`platform.cpp:120`, `:136`); agent (`main.rs:92`) | data folder; agent log folder |
| `XDG_DATA_HOME` | app on Linux (`platform.cpp:129`); the two Python plug-ins | data folder |
| `TMPDIR` | app (`ipc_server.cpp:394`) and agent (`ipc/unix.rs:24`) | the agent socket's folder |
| `USERPROFILE` | agent on Windows (`main.rs:84`); the Python plug-ins; the companion | agent log folder; data folder |
| `LOCALAPPDATA` | the Python plug-ins; the companion | the pre-0.5.8 data folder |
| (none) | the app on Windows | uses `SHGetKnownFolderPath` for both folders (`platform.cpp:29-37`, `:103`, `:113`) |
| `PYTHONUNBUFFERED=1` | set by the agent for the render process | CT 4.10 |
| the whole environment | inherited by the agent and by the render process | PA 7.11 |

Build and release only: `MR_MSIX_IDENTITY_NAME`, `MR_MSIX_PUBLISHER`, `MR_MSIX_PUBLISHER_DISPLAY`, `MR_BUILD_DIR`,
`VCINSTALLDIR` (Windows packaging); `QT_PREFIX`, `SIGN_ID`, `TEAM_ID`, `NOTARY_PROFILE`, `BUILD_DIR`, `RELEASE_DIR`,
`ENTITLEMENTS` (macOS release); CMake cache entries `MINPLATFORM_GIT`, `MINPLATFORM_REV`, `MINPLATFORM_DIR`,
`MINRENDER_UPDATE_PUBLIC_KEY`. The define `MINRENDER_STORE_BUILD` named in a commit subject no longer exists.

The app reads no variable to override its data folder, its ports or its sync root. Two nodes on one machine therefore
need two user profiles (CT 11, the test note).

### B10. The security model as a whole (`SECURITY.md`, 181 lines)

CT 2.2 has the bearer mechanism; PA quotes two paragraphs. The document makes further commitments that are parity items:

| Commitment | Source | Where implemented |
|---|---|---|
| Trust model: a trusted LAN plus a trusted share; the share's permissions ARE the farm's security; never exposed to the internet | `SECURITY.md:8-20` | - |
| Every `/api/` route needs the bearer secret except `GET /api/status`; auth fails closed (503 without a secret); init refuses to write a blank secret | `:22-31` | CT 2.2; `farm_init.cpp:285-293`, `:333-346` |
| Constant-time token comparison | `:33-34` | `http_server.cpp:1011-1037` |
| Request bodies capped at 16 MiB, and an unauthenticated request is refused before its body is read | `:36-40` | CT 2.1 |
| Plain HTTP by decision; TLS or body HMAC "not planned" | `:42-51` | - |
| UDP is an untrusted hint: never creates a peer, never changes an endpoint; signing heartbeats is "the natural next step" | `:53-72` | CT 1.6 |
| **Firewall stance: inbound TCP 8420 and UDP 4243, all profiles, any remote address - deliberate.** `localsubnet` and `domain,private` were each evaluated and rejected, with reasons | `:74-95` | `AppxManifest.xml.in:109-118` |
| The secret, flags and environment never reach a log; no HTTP library logger is installed | `:97-106` | B7 |
| Renders run arbitrary code by design; an executable allowlist was considered and rejected; the stated mitigation is a dedicated non-administrator account. Two execution channels: `POST /api/jobs` (secret) and the edit command file (share write access only) | `:108-124` | CT 6.6 |
| Ids that become paths are checked on every route and again in `deleteJob`; farm cleanup may delete only inside `<farm>/jobs/*` and the local staging root. Before 0.5.6 `DELETE /api/jobs/..` resolved to the farm root | `:126-137` | `platform.cpp:312-346` |
| Two read-only local surfaces for other programs: loopback `/api/status` and the registry key | `:139-155` | C |
| Agent channel: owner-only pipe; socket in the per-user temporary folder, mode 0600, "not in `/tmp`" | `:157-160` | CT App. A (the code does fall back to `/tmp`) |
| Rotation: stop every node, edit `farm.json`, restart | `:162-165` | CT 2.2 |
| Distribution: Store-signed on Windows with no self-updater; signed, notarised, Ed25519-verified updates on macOS; no hidden launches of third-party programs; the restart helper may relaunch only its sibling | `:167-176` | PA 3 |

Firewall facts not in step 0:

- **The declared rules are for the default ports only.** The ports are settings (`http_port`, `udp_port`); the Store
  manifest names 8420 and 4243 literally. A node moved to other ports has no rule and Windows prompts or blocks.
- The rules are declared for `minRender.exe` only; a port that renames the executable, or serves from a helper, loses
  them.
- `addFirewallRule` (`platform.cpp:228-310`, rules named "MinRender" and "MinRender UDP" through the firewall COM API)
  has **no caller**. Machines that once ran the pre-Store installer may still carry rules of those names, added by
  `netsh` at install time (history, B18).
- macOS: nothing is done; the system's application firewall, when on, asks at the first listen.
- A local shell is used to open folders and links on macOS and Linux (`std::system("open \"" + path + "\"")`,
  `platform.cpp:201-226`): a job's output folder name reaches a shell unescaped when "Open output" is pressed
  **(by reading)**. See D2.

### B11. Scheduling features: what exists, in one place

All covered by CT; listed so that the "absent" column is explicit.

| Feature | Present? | Where |
|---|---|---|
| Priority (lower first), drag order, move across priorities | yes | CT 4.5 |
| Retries per chunk, per-chunk node blacklist | yes | CT 4.4 |
| Per-chunk timeout | yes (`timeout_seconds`, agent timer) | CT 4.1, KP |
| Node suspension (5 failures in 300 s), 30 s cooldown, manual unsuspend only | yes, leader memory | CT 4.7 |
| Tags as the only matching rule | yes | CT 4.6 |
| Pause / resume / cancel / archive / delete, stop chunk, edit with three modes, resubmit (whole, incomplete, a range) | yes | CT 4.4-4.9 |
| Snapshot every 30 s, restore at takeover | yes | CT 3.3, 3.4 |
| **Dependencies between jobs** | **no** | no key, column or code |
| Pools / groups / per-job node limits / concurrent chunks per node | no | CT 4.6 |
| Scheduled start, working hours, idle detection | no (idle detection lives in the external companion) | - |
| Frame step, frame lists, reverse or "ends first" order | no: a range and a chunk size only; chunks go in ascending order | CT 4.3 |
| Negative frames | undefined | CT 11.3 y |

### B12. Checks the app does NOT make

| Check | Status |
|---|---|
| Output path exists / is writable before dispatch | no. The worker creates `output_dir` best effort (`render_coordinator.cpp:497`), ignoring errors; a bad path fails in the renderer |
| Rendered file exists and is non-empty | no (`exists_nonzero` is never evaluated; CT 11.3 f) |
| Disk space (share, staging, output) | no. No call to any free-space API in the tree |
| Scene / project file reachable from the worker | no |
| Executable exists on the worker | no; the spawn fails and the chunk fails |
| Renderer licence available | no. The templates' licence `error_patterns` are inert |
| Path mapping covers a path | no; an unmapped path is passed through with separators changed (CT 7.6) |
| Plug-in version against app version | no (B2) |
| Clock agreement between nodes | no (CT 10) |
| **System sleep / App Nap** | **nothing**. No power assertion, no execution-state call, no activity token anywhere in `src/` or `mr-agent/`. A render node may sleep under a render; on macOS a hidden-window app with a 50 ms timer is a candidate for timer throttling **(by reading; not measured)** |
| Wake-on-LAN, shutdown, reboot of nodes | no. "Restart App" restarts the app, not the machine, and only on Windows |
| Notifications, e-mail, webhooks, chat hooks | none. `show_notifications` is a dead setting (UI 6 a) |
| Statistics, history, reports, cost | none. Durations are computed from two timestamps per chunk |
| User accounts, roles, permissions | none. Any node's UI can do everything |

### B13. Two additions to the HTTP table (CT 2.5)

- The server exists only while the farm runs (CT 2.1), so `POST /api/config` cannot give a node its first sync root.
- `GET /api/status` on loopback is polled once a second by the companion when the registry is not visible
  (`tools/rndr-companion/rndr-companion.ps1:242`); the keys it reads are listed in C.

### B14. Fonts, theme, icons

- **Fonts** (`resources/fonts/`, loaded from disk at start, `main_qt.cpp`): Geist Regular / Bold / Italic and Geist Mono
  Regular / Bold (SIL OFL 1.1), Phosphor (MIT, the icon font; glyph names in `src/ui/qml/PhosphorIcons.js`), Material
  Symbols Sharp (Apache 2.0, loaded, unused). Default font Geist 10 pt. Fonts are loose files, not embedded; icons are
  embedded.
- **Theme** (`src/ui/qml/Theme.qml`, 154 lines, one dark theme, no light variant, no user setting): 12 surface and
  border greys from `#0d0d0d` to `#333333`, five text greys, accent `#0189f1` (hover `#1b95f1`, muted `#10395b` = the
  selection fill), success `#4cb050`, warning `#f5a623`, error `#c04040`, info `#9cc9ff`, six frame-grid colours, a
  55 % scrim; type sizes 10 / 11 / 12 / 13 / 14 / 16; radii 3 / 6 / 10; hover ramp 100 ms, popup fade 80 ms. A second,
  different dark palette is set for native widgets (`main_qt.cpp`, highlight `#7aa2f7`). UI question 2 asks whose look
  wins; these are the values if minRender's is carried.
- **Formatting** (`Format.qml`): durations as `42s`, `3m 12s`, `1h 4m`.
- **Icons** (`resources/icons/`): `minrender.ico` (Windows, also embedded and used for the tray), `minrender.icns`
  (macOS bundle), `minRenderMacOStray.png` (menu-bar template image), plus source artwork (`.ai`, `.svg`, `.png`) for a
  macOS and a Windows mark; Store tiles in `installer/msix/Assets/` (150, 44, Store logo, each at 100 % and 200 %, one
  unplated 44).
- The OS accent colour reader exists and is unused (UI 6 w).

### B15. Update channels and the web site

| Channel | Mechanism | Fixed points |
|---|---|---|
| macOS | Sparkle 2.9.2, vendored at build time by `scripts/fetch_sparkle.sh` (checksum pinned). Feed URL and public key are baked into `Info.plist` (`cmake/MacOSXBundleInfo.plist.in:59-68`): `https://minrender.com/appcast.xml`, daily check, no system profile | every installed macOS copy since 0.4.8 polls that URL and accepts whatever it lists, if signed with the family's Ed25519 key and newer by `sparkle:version` |
| Windows, Store | the Store updates the package; the app has no updater; farm nodes are told to turn Store auto-update off and upgrade with `winget` when idle (`installer/STORE.md:183-210`) | the Store product, its package identity, the application id |
| Windows, direct (retired) | WinSparkle against `https://minrender.com/appcast-win.xml`, **frozen at 0.5.5** on purpose (`docs/appcast-win.xml:2-4`) | installs at 0.5.5 and older never see an update again; they must be uninstalled and replaced by hand |

- `scripts/update_appcast.sh` adds an item after the marker line `<!-- @@APPCAST_INSERT@@ -->`: version from
  `CMakeLists.txt`, enclosure `https://github.com/<repo>/releases/download/v<version>/minRender-<version>-macOS.dmg`,
  signature and length from Sparkle's `sign_update`, minimum system 12.0, notes from an HTML file or a link.
- The appcast's notes are the **only changelog** of the product, and they carry operator instructions ("Action needed:
  reinstall the plug-ins", "start minRender once on each node after it updates").
- Versions never published on a channel: 0.5.4 (anywhere), 0.5.6 and 0.5.7 (macOS; on Windows 0.5.6 was denied by the
  Store and 0.5.7 withdrawn). In the field: macOS 0.4.8-0.5.11 without those; Windows direct installs up to 0.5.5;
  Windows Store 0.5.8-0.5.11.
- **The web site is part of the product**: `docs/` is a Jekyll site (remote theme `just-the-docs`, dark scheme,
  `docs/_config.yml`) served at the domain in `docs/CNAME`. It serves the manual, both appcasts, and
  `/privacy/` - the privacy policy the Store listing requires (`docs/privacy.md`). The Store's certification notes cite
  `/install/#uninstall` (`installer/STORE.md:138`). The app's "Docs" menu item opens the site root (`Main.qml:129`).
  Moving the manual means keeping these URLs alive.
- Privacy promises that bind the port (`docs/privacy.md:13-30`): no accounts, no telemetry, no crash reporting; the only
  outbound request is the macOS update check, "carrying no personal information".
- The manual's screenshots (`docs/images/`, 24 files) show the pre-0.5.8 interface in part and three plug-in panels;
  several file names still carry the product's first name.

### B16. Version numbers: every place one is written

| Place | File | Read by |
|---|---|---|
| the source of truth | `CMakeLists.txt:2` `project(MinRender VERSION x.y.z)` | CMake; `make_msix.ps1` (appends `.0`); both shell scripts |
| `APP_VERSION` | compile definition from CMake; a fallback literal in `src/core/config.h:110-112` | `/api/status`, the About menu, the farm-content stamp rule |
| agent | `mr-agent/Cargo.toml:3` and `Cargo.lock` | the agent's version resource only; **never compared with the app** |
| Windows version resources | `resources/version.rc.in` (three C++ binaries), `mr-agent/build.rs` | Explorer; antivirus heuristics |
| macOS bundle | `CFBundleVersion` / `CFBundleShortVersionString` from CMake | Sparkle's comparison |
| MSIX | `Identity/@Version` = `x.y.z.0` | the Store |
| Blender add-on | `bl_info["version"]` (`MinRender.py:10`) | Blender's add-on list |
| Cinema 4D script | `PLUGIN_VERSION` (`:16`) and the header comment (`:1`) | the dialog title |
| After Effects script | `SCRIPT_VERSION` (`:13`) and the header comment (`:1`) | the palette title |
| share stamp | `farm.json` `last_example_update` | CT 6.2 |
| appcast | `sparkle:version`, the enclosure URL, the tag `v<version>` | Sparkle |
| Store notes | `installer/STORE.md:124-139` ("Update the first paragraph each release") | a person |
| template `_version` (2), manifest `_version` (1), submission `_version` (1), `farm.json` `_version` (1), `protocol_version` (2) | several | nothing (CT 3.6) |

`installer/STORE.md:212-216` lists three places to bump (CMake, `config.h`, the agent); the three plug-ins and the appcast
are bumped by habit, not by a script. Nothing checks agreement. A port's version must be numerically above 0.5.11 for
Sparkle and the Store to deliver it, and that same fact makes it the owner of the share's templates and plug-ins
(CT open question 2).

### B17. Build and release: what a release consists of

**Dependencies fetched at configure time** (`CMakeLists.txt:15-56`): nlohmann/json 3.11.3, cpp-httplib 0.18.3 (no TLS,
no compression), SQLiteCpp 3.3.2 (bundled SQLite), and `min-paths-ffi` from this repository at a pinned revision, built
by cargo into a static library (`cmake/MinPaths.cmake`; `MINPLATFORM_DIR` substitutes a local checkout). Qt 6.8 or newer
is required by the build file; the scripts use 6.11.1. C++20. CMake 3.24 (the README says 3.21).

**Optional pieces, decided by what is present on the build machine**: the preview (five image libraries; macOS binaries
and headers are committed under `external/install`, Windows ones are expected in the ignored `external/install-win64`
from a vcpkg export); Sparkle (`external/Sparkle`, ignored); the agent (a prebuilt `mr-agent/target/release` binary is
copied in if it exists - CMake does not run cargo for it and only warns when it is missing).

| | Windows | macOS |
|---|---|---|
| Driver | `scripts/build_msvc.bat` (finds Visual Studio through `vswhere`, Ninja), `cargo build --release` in `mr-agent`, `scripts/package.bat [--msix]`, `scripts/make_msix.ps1` | `scripts/macos-release.sh` (nine steps), then `scripts/update_appcast.sh` |
| Artifact | `build/minRender-<x.y.z.0>-x64.msix`, uploaded to Partner Center by hand; signed by the Store | `release/minRender-<x.y.z>-macOS.dmg`, attached by hand to the release tagged `v<x.y.z>` |
| Contents | `minRender.exe`, `minrender-headless.exe` (unreachable), `mr-restart.exe`, `mr-agent.exe`, the image DLLs, `resources/` (fonts, icons, templates incl. `backups/`, plug-ins), Qt through `windeployqt` (release, compiler runtime, no translations, no software OpenGL, QML scanned), the manifest and `Assets/`; the redistributable installer is removed | `minRender.app`: `MacOS/minRender`, `MacOS/mr-agent`, `Frameworks/` (Qt through `macdeployqt`, Sparkle), `Resources/` (fonts, icons, templates, plug-ins, `THIRD_PARTY_NOTICES.txt`, `LICENSE`, the icon); the SQL, print-support and network-information Qt plug-ins are deleted |
| Signing | none locally | Developer ID, hardened runtime, inside-out (Sparkle helpers, Qt frameworks, loose libraries, plug-ins, the agent with entitlements, the bundle), one entitlement (`disable-library-validation`), notarise and staple the app, build a compressed DMG with an Applications link, sign, notarise and staple the DMG |
| Guards | capabilities must be exactly three; no virtualisation switches; the agent and restart helper must be present; manifest comments stripped (`make_msix.ps1:52-95`) | the signing identity must exist; bundle resources must equal the source tree (`macos-release.sh:60-64`, `:90-99`) |
| Floor | Windows 10 2004 (build 19041), x64 only | macOS 12, the build machine's architecture (no universal build step in the script) |
| Local test | `make_msix.ps1 -Sideload -SkipPack` + `Add-AppxPackage -Register`, in an interactive session only (`STORE.md:46-68`) | - |
| Machine-specific defaults in the script | Qt at a fixed drive path | a Qt path under one home directory, one signing identity and team, a notary profile named after a sister product |

The release is manual end to end: no CI configuration exists in the repository, no checksum file is published, and the
per-release routine is prose (`installer/STORE.md:212-222`).

### B18. Store facts and retired pieces still in the field

- **Package identity is external.** Name, publisher and publisher display name come from Partner Center through three
  environment variables (`STORE.md:11-26`); the application id is `minRender`, the startup task id `minRenderStartup`
  (`AppxManifest.xml.in:86-104`). A port delivered as an update to the same Store product must keep the identity; the
  companion launches the app through `<PackageFamilyName>!minRender` (C).
- **Certification history** (`STORE.md:147-176`): `unvirtualizedResources` was denied; the answer was to move the data
  folder, not to argue. The packaging script now fails the build if that capability or any virtualisation switch
  returns. A second, hidden `Application` was refused. Only `runFullTrust` has to be justified, and its justification
  text names "a bundled render agent" (PA 6).
- **Listing text** must open with the dependency on Blender / After Effects / Cinema 4D (policy 10.2.4) and say plainly
  what the app launches (`STORE.md:97-122`). The manifest's own description still names only two of the three
  (`AppxManifest.xml.in:91`).
- **Uninstall keeps the data folder**, by design and by promise to the Store (`docs/install.md:61-74`).
- **Under the Store package the registry is virtualised**: the node-state key is invisible to other programs and removed
  at uninstall (`AppxManifest.xml.in:27-29`).
- **The pre-Store installer** (Inno Setup, removed in `8fbbe35`) installed per machine into the program-files folder,
  required administrator rights, added a per-user Startup shortcut with `--minimized`, added two firewall rules by
  `netsh` ("MinRender", "MinRender UDP"), offered to delete the user's configuration, and supported Windows 10 1809.
  Machines migrated to the Store by hand may still have the shortcut (now dangling) and the rules.
- **Old data**: `%LOCALAPPDATA%\MinRender` stays after migration until the user deletes it, and the app keeps polling its
  `submissions\` (CT 5.2). `restart.bat` from a still older restart mechanism is deleted at start
  (`monitor_app.cpp:35-39`).
- **The Tauri UI** of March 2026 left names behind: comments ("synced from Tauri UI"), the HTTP routes with no caller
  (CT 2.5 rows 31-38, labelled "Tauri UI support endpoints" in `http_server.cpp:730`), and the word "sidecar" for the
  headless build.

### B19. Third-party software and its licences

From `THIRD_PARTY_NOTICES.txt` (950 lines) and the font licence files. "Port" = what happens in a Rust build on this
repository's platform.

| Component | Licence | Used for | Port |
|---|---|---|---|
| Qt 6 (Core, Gui, Network, Qml, Quick, QuickControls2, Widgets) | LGPLv3, dynamically linked so the user can replace it | the whole UI, tray, single instance, settings store | **gone** - and with it the LGPL obligations |
| nlohmann/json 3.11.3 | MIT | all JSON | replaced (serde). Its behaviours a peer can see are in CT "Conventions" |
| cpp-httplib 0.18.3 | MIT | HTTP server and client | replaced. Peer-visible behaviours in CT 2.1 |
| SQLiteCpp 3.3.2 + SQLite | MIT / public domain | the leader's database, the backup API for snapshots | replaced by a Rust SQLite binding; the file format and the backup call stay |
| own SHA-256 (`src/core/sha256.cpp`) | project code | the secret fingerprint | replaced by a crate; 8 hex characters must match |
| Rust crates of the agent: serde, serde_json, clap, log, env_logger (declared, unused), simplelog (used, **not listed** in the notices), regex, windows 0.58, libc, winresource | MIT or Apache-2.0 | the agent | kept or folded in; `regex` is a contract (CT 7.4) |
| min-paths | MIT or Apache-2.0 | path translation | already this repository's |
| Sparkle 2.9.2 (with bsdiff, sais-lite, SUDSAVerifier and other bundled notices) | MIT and BSD-style | macOS updates | kept (the family loads it at run time) |
| OpenEXR 3.3.5, Imath 3.2.1 | BSD-3-Clause | preview | `min-image` |
| libpng 1.6.44 | PNG licence v2 | preview | `min-image` |
| libjpeg-turbo 3.x | IJG + BSD-3-Clause + zlib | preview | `min-image` |
| libtiff 4.7 | libtiff licence | preview | `min-image` |
| zstd, libdeflate, zlib (transitive) | BSD / MIT / zlib | preview | as `min-image` needs |
| Geist, Geist Mono | SIL OFL 1.1 | type | keep or take the platform's faces |
| Phosphor | MIT | icons | the platform has its own icon set (a ruling) |
| Material Symbols Sharp | Apache 2.0 | unused | drop |

Two distribution facts: the notices file and the project licence are inside the macOS bundle
(`CMakeLists.txt:439-443`) but **are not staged into the Windows package** (`scripts/package.bat` copies executables,
DLLs and `resources/` only; the retired installer did ship them). And nothing in the app's UI shows licences or notices.
The project itself is MIT (`LICENSE`).

### B20. Tests

Two unit tests in the whole repository, both Unix-only, both in `mr-agent/src/kill.rs:139-190` (PA 6). No C++ tests, no
QML tests, no test target in CMake, no fixtures, no recorded requests. Every conformance test in CT 11.1 has to be
created; none can be lifted.

---

## C. Everything outside the app that something already installed depends on

"Dependant" = what breaks if the port changes the item. All are 0.5.11's values.

### C1. Files and folders on the local machine

| # | Item | Exact form | Dependant |
|---|---|---|---|
| 1 | Data folder, Windows | `%USERPROFILE%\minRender` | plug-ins, companion, the manual, the Store certification notes |
| 2 | Data folder, macOS | `~/Library/Application Support/MinRender` | plug-ins |
| 3 | Data folder, Linux | `$XDG_DATA_HOME/MinRender` or `~/.local/share/MinRender` | the two Python plug-ins |
| 4 | Legacy data folder, Windows | `%LOCALAPPDATA%\MinRender` - existence switches plug-ins and the companion back to it unless the new folder has `config.json` | plug-ins older than 0.5.8; the fallback rule in all current ones |
| 5 | `config.json` | JSON; `sync_root` (string) read by plug-ins; `http_port` read by the companion; legal as a JavaScript expression | plug-ins (one by `eval`), companion |
| 6 | `submissions/` | `*.json` directly inside, name free but written as `<13 digits>.<host>.json`; consumed and moved to `submissions/processed/`; also polled in the legacy folder | plug-ins |
| 7 | Submission file | CT 8.2 | plug-ins |
| 8 | `node_id.txt` | 12 characters | the node's identity across the upgrade; the farm's `nodes/<id>/` |
| 9 | `minrender.db` | CT 5.1 | a node that leads with no snapshot on the share |
| 10 | `staging/<job>/<range>/` | CT 4.11 | chunks in flight across an upgrade; the cleanup dialog |
| 11 | `mr-agent.log` | named in the manual | operators |
| 12 | `companion/`, `rndr-companion.json`, `rndr-companion.log` | owned by the companion, inside the app's data folder | companion - leave alone |

### C2. Windows registry, process and package names

| # | Item | Exact form | Dependant |
|---|---|---|---|
| 13 | Node-state key | `HKCU\Software\MinRender\Node`: `RenderState` (`REG_SZ` `idle` / `rendering`), `NodeState` (`REG_SZ` `active` / `stopped`), `Pid` (`REG_DWORD`, 0 = clean exit), `UpdatedMs` (`REG_QWORD`) | companion (loose builds; invisible under the Store package) |
| 14 | Process name | `minRender.exe` (the companion looks for a process named `minRender` in its session) | companion's keep-alive; the declared firewall rules; the startup task chain |
| 15 | Application id and package | AUMID `<PackageFamilyName>!minRender`; the package name the companion is configured with | companion's launch of the packaged app |
| 16 | Start-hidden argument | `--minimized` | companion (`app_args`), the registered restart command, the restart helper, old Startup shortcuts |
| 17 | Companion's own key | `HKCU\Software\MinRender\Companion` (`Enabled`, `KeepAlive`) | not the app's - do not reuse the parent key for anything that could collide |
| 18 | UI settings | Qt's store under `MinRender` / `MinRender` | nothing external; avoid clobbering |
| 19 | Store identity | package name, publisher, application id `minRender`, startup task id `minRenderStartup`, the three capabilities | the Store update path; the user's Startup toggle |
| 20 | Helper names | `mr-agent.exe`, `mr-restart.exe` beside the app | the Store justification text; operators' process lists; antivirus exclusions an operator may have made |

### C3. Network

| # | Item | Exact form | Dependant |
|---|---|---|---|
| 21 | HTTP | TCP, all interfaces, port `http_port` (default 8420), plain HTTP | every node; the declared firewall rule (8420 only) |
| 22 | `GET /api/status` without authentication, also on `127.0.0.1` | keys `render_state`, `node_state`, `active_job`, `ready_for_work` at least (CT 2.4 for all) | every node; the companion; `SECURITY.md` "Local integration" |
| 23 | The other 37 routes | CT 2.5 | nodes; possibly private tools (CT open question 9) |
| 24 | UDP | group `239.42.0.1`, port `udp_port` (default 4243), TTL 1 | every node; the declared firewall rule (4243 only) |
| 25 | Outbound | none except Sparkle's request to the feed URL on macOS | the privacy policy |

### C4. The share

| # | Item | Dependant |
|---|---|---|
| 26 | `<sync_root>/MinRender-v2` - the name is a literal in all three plug-ins | plug-ins, every node |
| 27 | `farm.json`, `nodes/`, `jobs/`, `state/`, `commands/`, `templates/`, `plugins/` and their contents (CT 6) | every node |
| 28 | `templates/plugins/<id>.json` with ids `blender-5.2-plugin`, `cinema4d-2026-plugin`, `after-effects-plugin` and flag ids `scene_file`, `scene_name`, `output_path`, `take_name`, `output_format`, `threads`, `project_file`, `comp_name`, `rs_template`, `om_template` | installed plug-ins; a port that prunes or renames these files breaks submission from every workstation until plug-ins are reinstalled |
| 29 | `plugins/blender/MinRender.py`, `plugins/cinema4d/MinRender.py`, `plugins/afterEffects/MinRender.jsx` - the paths the manual and the release notes send users to | users, the manual |
| 30 | `templates/examples/*.json` - what users are told to copy | users' own templates derive from them; user templates in `templates/` must keep parsing (schema CT 7.2, tolerance of `_version` 2 and `help`) |
| 31 | `<sync_root>/snapshot_tmp.db` (transient, outside the farm folder) | 0.5.x leaders |

### C5. Executables and command lines the app starts

| # | Item | Dependant |
|---|---|---|
| 32 | The renderer: `manifest.cmd[<os>]` with the argument vector of CT 4.10, no shell, inherited environment plus the manifest's plus `PYTHONUNBUFFERED=1`, working directory from the manifest | every stored manifest and user template; DCC behaviour |
| 33 | `mr-agent --node-id <id>` from the app's own folder; pipe `\\.\pipe\MinRenderAgent_<id>`, mutex `MinRenderAgent_<id>`, socket `minrender-agent-<id>.sock` | only itself (PA: private) |
| 34 | `mr-restart.exe --pid --exe --minimized`; with no arguments as the startup task | the Store manifest |
| 35 | macOS: `system_profiler SPDisplaysDataType` through a shell pipeline at start for the GPU name (`node_identity.cpp:134`); `open` for folders and links | the value of `gpu_name` peers display |
| 36 | Windows: `explore` / `open` shell verbs | - |

### C6. Web and release artefacts

| # | Item | Dependant |
|---|---|---|
| 37 | `https://minrender.com/appcast.xml`, the Ed25519 public key, the bundle identifier, `sparkle:version` ordering | every installed macOS copy |
| 38 | `https://minrender.com/appcast-win.xml`, frozen | Windows installs at 0.5.5 and older (must keep answering "no update") |
| 39 | `https://minrender.com/` (Docs menu), `/privacy/` (Store listing), `/install/#uninstall` (certification notes) | the app's menu in installed copies; the Store |
| 40 | Release asset name `minRender-<version>-macOS.dmg` under tag `v<version>` | appcast items; the manual's download link |
| 41 | The Store product (id in `docs/install.md:12`), `winget` id | the manual; farm rollout instructions |

---

## D. Promises against code, and things that look wrong today

Nothing here is to be fixed silently; each line is for the owner.

### D1. The manual promises it; the code does not do it (or does something else)

Already in step 0 and not repeated: `error_patterns`, `exists_nonzero`, `_version`, drag across priorities, the asterisk,
"reveals the action strip" (CT 11.3 f-h; UI 6 f-h).

| # | Promise | Where | Reality |
|---|---|---|---|
| 1 | Pattern tokens auto-fill the output path at submission (`{project_dir}`, `{file_name}`, `{frame_pad}`, `{flag:id}`, date and time) | `docs/templates.md:68`, `:133`, `:149-170` | dead since the Qt port (B3) |
| 2 | The pattern example resolves `{project_dir}\..renders\...` to a path with the `..` folded | `docs/templates.md:163-170` | the example lacks a separator, and no code folds `..` |
| 3 | `cmd.label` is shown and `cmd.editable` lets the user change the executable at submission | `docs/templates.md:92-98` | the form has no executable field (UI 6 u); the executable can only be changed by editing the template |
| 4 | `job_defaults`: "Users can override all of these" | `docs/templates.md:176` | `max_retries` and `timeout_seconds` are not in New Job; only in Edit Job (CT 7.5) |
| 5 | "each node keeps a SQLite snapshot of the leader's state" | `README.md:14`, `docs/index.md:22` | one snapshot on the share; no node but the leader holds state (CT 3.3) |
| 6 | `nodes/`: "each node writes its own `meta.json` and heartbeat here" | `docs/install.md:111` | the file is `endpoint.json`, written once per farm start; there is no heartbeat file |
| 7 | The sync root's contents: four folders | `docs/install.md:107-114` | also `farm.json` (with the secret), `state/`, `commands/`, `templates/` (user) and `templates/plugins/`, and `snapshot_tmp.db` beside the farm folder |
| 8 | `config.json` holds "sync root, ports, node name" | `docs/install.md:41` | there is no node name setting |
| 9 | `minrender.db`: "local job/chunk history" | `docs/install.md:43` | present only on a node that has led; overwritten at each takeover |
| 10 | A peer's log is "streamed from the agent on another node" | `docs/monitor.md:36` | the app's own log file, read from the share by the viewer |
| 11 | Logs "are also written to disk in the sync root under `jobs/<job_id>/`" | `docs/monitor.md:39` | true for render output only; node logs are under `nodes/<id>/` and expire after 7 days |
| 12 | Staging "only copies finished chunks into the sync root" | `docs/configure.md:26` | it copies into the job's output folder, wherever that is; the path given is the Windows one only |
| 13 | `noleader`: never leads "unless it's the only one alive" | `docs/configure.md:20` | it leads whenever every other live node is also `noleader` or it sorts first among them (CT 3.1) |
| 14 | Cinema 4D: copy the script into the scripts folder and run it from Extensions > User Scripts | `docs/submit.md:48` | the script's own header says Script Manager > Import; both work, the two texts differ |
| 15 | "Requires CMake >= 3.21, Qt 6.11" | `README.md:37` | the build file asks for CMake 3.24 and Qt 6.8 |
| 16 | The agent socket is "not in `/tmp`" | `SECURITY.md:157-160` | it falls back to `/tmp` when `TMPDIR` is unset or the path is too long (CT App. A) |
| 17 | aerender's `-reuse` does not survive between chunks, "prefer larger chunks" | `docs/monitor.md:49-50` | consistent with the code - but the shipped After Effects example template still passes `-reuse` |
| 18 | The Store description: "for Blender and After Effects" | `AppxManifest.xml.in:91` | Cinema 4D is supported and named everywhere else |

### D2. The code does it; the manual does not say so

None of these appears in `docs/*.md`: Edit Job and its three modes; stop chunk and the "partial" state; resubmit a chunk
or the incomplete part as a new job; the frame preview and the EXR layer picker; Farm Cleanup; node suspension and
unsuspend; the peer menu (stop, start, restart app, restart signal); path mappings and cross-OS submission in general;
the secret fingerprint and the "Farm security" section; IP override and the UDP settings; the title-row status dot;
"Show in Explorer / Finder" for logs; multi-select and its keys; "Open output"; the tray's Stop / Resume; the
`--minimized` argument; the edit queue on the share when the leader is unreachable; the `help` key of a template flag;
plug-in templates as a separate, hidden kind; macOS as a submitter with canonical Windows paths; the companion (linked
only from the uninstall paragraph). The appcast notes are the only user-facing text for most of them (B15).

### D3. Things that look broken or inconsistent today

Not repeated from step 0: CT 11.3 a-z, UI 6 a-z, PA 7.1-7.13, KP 3.

| # | What | Where | Kind |
|---|---|---|---|
| 1 | `default_pattern` / `frame_padding` / pattern tokens are parsed, documented, shipped in templates, and never applied | B3 | lost feature |
| 2 | A corrupt or mistyped `config.json` is replaced by defaults at the next save, with no log entry | `monitor_app.cpp:226-261` | data loss |
| 3 | A submission file with a mistyped field throws on the main pump before it is moved aside | `submission_watcher.cpp:87-93` | crash loop **(by reading)** |
| 4 | A submission whose move to `processed/` fails is resubmitted every 3 s | `submission_watcher.cpp:83`, `:99`, `:153`, `:190`, `:274` | duplicate jobs **(by reading)** |
| 5 | The plug-in path skips every submit-time validation | B1 | empty executable, empty required flags, reversed range accepted |
| 6 | Blender add-on: priority tooltip says "higher = rendered first" and caps at 100; the app is lower-first, 1-999 | `resources/plugins/blender/MinRender.py:104-110`; Cinema 4D `:251` | wrong help text, mismatched range |
| 7 | After Effects plug-in template: macOS executable points at the 2025 folder, Windows at 2026 | `resources/templates/plugins/after-effects-plugin.json:7-8` | inconsistency |
| 8 | After Effects script reads `config.json` with `eval` | `MinRender.jsx:144` | executes whatever is in the file |
| 9 | After Effects script writes the submission in place; a poll during the write loses the job | `MinRender.jsx:331-335` (CT 11.3 m names it) | lost submission |
| 10 | After Effects script: `submitted_at_ms` is `ts` but the file name uses `ts + i`; the hand-written JSON fallback escapes only `\`, `"`, newline and return | `MinRender.jsx:61-84`, `:306-327` | harmless today; invalid JSON for a comp name with a tab when the host lacks `JSON` |
| 11 | After Effects frame numbers are derived from the render item's time span times the frame rate, not from the comp's displayed start frame | `MinRender.jsx:194-196` | **(by reading; needs a check with a comp whose start frame is not 0)** |
| 12 | After Effects example template: per-frame pattern matches once per process; its progress pattern reads a frame index as a percentage; it passes `-reuse`, which the manual says is useless | B4 | misleading progress on that template |
| 13 | Both Blender templates force `-F OPEN_EXR_MULTILAYER` | B4 | may be intended; changes the scene's format silently |
| 14 | Blender add-on declares Blender 4.0 as minimum while its template runs 5.2's executable; a file saved by an older Blender on the workstation is rendered by 5.2 on the farm | `MinRender.py:11`, `:78` | by design or not - owner |
| 15 | Cinema 4D: the format table has no PNG entry; `threads` exists in the template and no UI sends it; a Take is passed by bare name (two Takes of one name are indistinguishable) | `cinema4d/MinRender.py:96-106`, `:329-339` | gaps |
| 16 | The declared firewall rules name the default ports; the ports are user settings | B10 | a non-default port has no rule |
| 17 | `addFirewallRule`, `buildCommandPreview`, `resolvePattern`, `openUrl`, the `auto_start_agent` key, the `env_logger` dependency | several | dead code and keys |
| 18 | "Open output" and "Docs" go through a shell on macOS / Linux with the path in double quotes only | `platform.cpp:201-226` | a folder name with a quote or `$(...)` is interpreted **(by reading)** |
| 19 | No local log file; console-only messages for config, migration and QML problems | B7 | undiagnosable unconfigured nodes |
| 20 | Node logs are pruned only on a date roll-over seen by a running app | `monitor_log.cpp:121-127` | logs of nodes switched off nightly grow without bound |
| 21 | The agent log is truncated by deletion at 2 MiB at agent start, i.e. possibly between two chunks of one job | `mr-agent/src/main.rs:117-121` | history lost at an arbitrary point |
| 22 | The agent finds its log folder from `USERPROFILE`; the app from the known-folder API | `main.rs:84` vs `platform.cpp:103` | two folders when they disagree (redirected profiles) |
| 23 | Settings edits are live before Save; exit while the card is open saves them | B6 | surprising |
| 24 | Notices and licence are not in the Windows package; `simplelog` is not in the notices | B19 | licence hygiene |
| 25 | Nothing keeps a rendering machine awake | B12 | a node can sleep under a render |
| 26 | The GPU name on macOS comes from a shell pipeline over `system_profiler` run synchronously at start | `node_identity.cpp:132-148` | slow start; empty when the tool's output changes |
| 27 | The version must be bumped by hand in at least seven places; nothing checks agreement; the agent's version is never compared with the app's | B16 | drift (0.5.8 shipped stale plug-ins) |
| 28 | `STORE.md`'s routine omits the plug-ins and the appcast from the bump list; a comment in `CMakeLists.txt:7-11` still speaks of a Windows updater key | docs drift | |
| 29 | The macOS release script holds one person's paths and identities as defaults | `scripts/macos-release.sh:33-36` | not reproducible elsewhere |
| 30 | A farm restart from Settings (new sync root or port) happens under an active render: the farm stops and starts while `RenderCoordinator` still holds the chunk | `monitor_app.cpp:167-173`, `:263-375` | outcome not traced here **(by reading)** - needs a test |

---

## E. Ranked findings and open questions

### E1. The ten most important findings for a porter

1. **The plug-ins are installed, hand-updated clients with literals in them** (B1, C). The data-folder rule, `config.json`
   as valid JSON *and* JavaScript, `sync_root`, the folder name `MinRender-v2`, the `submissions/` drop box and the three
   template ids with their flag ids cannot change without a reinstall on every workstation. Step 0 gave the file format
   only.
2. **The update channels will deliver the port to every node without asking** (B15, B16). Installed macOS copies follow
   one feed URL and one key; Store nodes follow one product. A higher version is at once the update, the new owner of the
   share's templates and plug-ins, and - on Windows - a replacement of executables whose names the firewall rules, the
   startup task and the companion depend on.
3. **CT 7.3 is wrong about a feature**: output-path patterns (`default_pattern`, `frame_padding`, six token families)
   have been dead code since the Qt port, although the manual and every shipped template rely on them (B3). The port
   must decide: restore, or remove from the reference.
4. **The plug-in path has no validation, and two ways to hurt the node**: a mistyped field throws on the main pump before
   the file is moved (a crash loop by reading), and a failed move resubmits the job every 3 s (B1, D3 3-5).
5. **`config.json` handling loses settings**: a file that fails to parse is replaced by defaults at the next save, the
   save is not atomic although three plug-ins and the companion read the file, and edits are live before Save (B6).
6. **There is no local log** and a class of messages reaches no file at all; node logs prune only under a running app;
   the agent log has its own rule and its own folder (B7). Any port will be judged first by whether a dead node can be
   diagnosed.
7. **The security document is a list of commitments, not background** (B10): fail-closed auth, the 16 MiB cap, path
   segment checks on every id, cleanup confined to two roots, nothing secret in logs, the deliberate
   all-profiles firewall stance, no hidden launches of third-party programs. The declared firewall rules cover only the
   default ports.
8. **What the product does not do is as much a parity fact as what it does** (B11, B12): no job dependencies, no pools,
   no disk / licence / output checks, no notifications, no wake or power management - and nothing prevents a rendering
   machine from sleeping.
9. **The shipped templates encode decisions** (B4): Blender always renders multilayer EXR; After Effects video outputs
   become one chunk; optional switches vanish when empty; the After Effects plug-in template names different host years
   per system; the example templates are what users copy from. A port that reships them unchanged inherits the oddities;
   one that changes them rewrites `templates/examples` and `templates/plugins` on every farm.
10. **A release is a manual, machine-bound procedure with no tests** (B16, B17, B20): seven or more places carry the
    version, two packaging chains, optional pieces decided by what lies on the build machine, a notices file that ships on
    one system only, and two unit tests in the whole product.

### E2. Open questions for the owner

1. **Plug-ins.** Does the port keep the three scripts byte-compatible and only bump their version, or may it change
   them (atomic write and no `eval` in the After Effects one; the priority tooltip; a version check against the app)?
   Any change needs a reinstall on every workstation - is an in-app "install / update plug-ins" action wanted, which
   0.5.x never had?
2. **Template ids with host versions in them** (`blender-5.2-plugin`, `cinema4d-2026-plugin`): keep the scheme, or move
   to stable ids so a host upgrade no longer needs new plug-ins?
3. **Output-path patterns** (B3): restore the feature in the port's form, or delete `default_pattern`, `frame_padding`
   and the manual's section? [proposed: restore - the templates and the manual already describe it]
4. **Blender's forced multilayer EXR** (B4): intended house rule, or should the scene's own format be respected?
5. **The After Effects items** (D3 7, 11, 12): which macOS host year is right; is the frame-number derivation correct
   for comps that do not start at frame 0; should the example template lose `-reuse`?
6. **Which nodes are still in the field?** Windows direct installs at 0.5.5 or older never update and overwrite the
   share's plug-ins at every start (B2, B15). Is there a floor the farm is brought to before the port joins?
7. **Delivery.** Is the port published as the next version on the existing feed and Store product (so every node gets
   it), or beside 0.5.x under another identity until it has run on a test farm? If the former: the same bundle
   identifier, the same package identity, the same executable name `minRender.exe` and application id?
8. **Firewall.** Keep the fixed-port manifest rules and accept that non-default ports need a manual rule, or have the
   app tell the operator (it cannot add rules itself in a Store package)?
9. **`config.json`.** May the port write it atomically and refuse to overwrite a file it could not parse (both
   compatible with every reader)? [proposed: yes to both]
10. **A local log file.** Wanted in the port (a new file in the data folder), and should the agent's log fold into it
    (PA question 9)? [proposed: yes; one rotated file]
11. **Sleep.** Should a node hold the machine awake while it renders, or while the farm runs at all? New behaviour on
    both systems.
12. **Absent features** (B11, B12): confirm that job dependencies, pools, disk and licence checks, notifications and
    power control are out of the port's scope. [proposed: out; listed for later]
13. **Notices.** Ship `THIRD_PARTY_NOTICES` in the Windows package and show it from the About menu?
14. **The web site.** Do the manual, the privacy page and the two appcasts stay in the MinRender repository's `docs/`
    (as the other two products keep theirs on their repositories' `main`), and who retakes the screenshots?
15. **Dead keys and dead code** (D3 17): keep writing `auto_start_agent` and `show_notifications` for the sake of an
    old build on the same machine, and drop the rest?
16. **Linux.** The two Python plug-ins, the data-folder rule and one template carry Linux paths; the app has never
    shipped there. Carry these as they are, with no Linux build of the port (KP: "an eventual target")?
