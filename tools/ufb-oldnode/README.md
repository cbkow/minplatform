# A genuine UFB 1.4.2 mesh node, headless and isolated (canon row 504)

The mixed test of slice 7: the rebuilt node (`apps/ufb/src/mesh.rs`) beside a REAL 1.4.2 node, without going near
an installed UFB or a production mesh.

**What the old node is.** 1.4.2's `core` crate (pure Rust, no Qt) started by `main.rs` here: `MeshSyncManager` with
its HTTP servers, phonebook, election, outbox and snapshot worker, exactly as shipped. The UFB repository is not
touched (row 458): its `core/` and `netfs-guard/` are COPIED to a scratch folder, and in that copy five constants of
`core/src/mesh/mod.rs` are changed so that the node belongs to a test mesh of its own:

| constant | 1.4.2 | scratch copy |
|---|---|---|
| `MESH_EPOCH` | 6 | 9006 |
| `FARM_VERSION_SUBDIR` | `v6` | `v9006` |
| `SNAPSHOT_FILENAME` | `ufb_snapshot_v6.db` | `ufb_snapshot_v9006.db` |
| `SNAPSHOT_META_FILENAME` | `ufb_snapshot_v6.meta.json` | `ufb_snapshot_v9006.meta.json` |
| `DEFAULT_UDP_PORT` | 4265 | 4465 |

No logic is changed. The HTTP ports and the farm's folder are arguments.

**The belt.** 1.4.2's mesh reaches its fixed application-data folder behind its caller's back (the snapshot worker
opens the database there, the edit queues and the settings are read there). The harness is therefore ALWAYS run with
`HOME` and `TMPDIR` pointing into the scratch folder, and refuses to start when its application-data folder is not
under the folder given as its first argument, or when the epoch is not the test one.

**Set up** (`$S` = a scratch folder outside every repository; `$UFB` = the UFB 1.4.2 checkout, read only):

    mkdir -p $S/old/harness/src && cp -R $UFB/core $S/old/core && rsync -a --exclude target $UFB/netfs-guard $S/old/
    cp $UFB/Cargo.lock $UFB/rust-toolchain.toml $S/old/
    # $S/old/Cargo.toml: [workspace] members = ["core", "harness"], exclude = ["netfs-guard"], then the
    #   [workspace.package] and [workspace.dependencies] tables of $UFB/Cargo.toml
    # $S/old/harness/Cargo.toml: a binary `oldnode` depending on ufb-core (path ../core), tokio (full), serde_json, env_logger
    # change the five constants; copy main.rs to $S/old/harness/src/
    (cd $S/old && CARGO_TARGET_DIR=$S/old-target cargo build --release -p oldnode)

**Run** (`UFB_BIN` = the rebuilt application's executable; everything happens under `$S/old-run`):

    S=$S UFB_BIN=target/release/UFB python3 -I tools/ufb-oldnode/mixed.py            # the new node leads
    S=$S UFB_BIN=target/release/UFB python3 -I tools/ufb-oldnode/mixed.py oldleads   # the old node leads
    S=$S UFB_BIN=target/release/UFB python3 -I tools/ufb-oldnode/late.py             # the old node comes late, empty

The new node runs on ports 49421 / 49422 / 4465, the old one on 49521 / 49522; a walk window on the usual test
ports is not disturbed. The harness takes commands on its standard input: `set <job>|<item>|<folder>|<0/1>|<json>`,
`get <item>`, `rows`, `cols`, `status`, `table <json>`, `snap`, `quit`.
