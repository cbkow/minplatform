# minRender, the port: submissions, paths between systems, a chunk nobody renders (2026-11-14)

Canon row 396. Three pieces on top of the leading node (`step4-the-node-leads.md`).

## A chunk held by a node that is not rendering it

Ruled 2026-11-14 (the owner, to the proposal of row 395: "sure. that sounds good"). When the port leads, a chunk
that is `assigned` to a node which is alive and - for twelve seconds, four of its polls - free or rendering something
else, and which was given out at least fifteen seconds ago, returns to `pending`. No retry is counted.

The takeover of `step4` was repeated with the leader killed when its snapshot was 22 s old. Under the 0.5.11 leader
that followed, the job hung at a chunk "assigned" to a node that had finished it. When the Rust node led again it
returned that chunk (and one more) by itself and the job completed, with no hand in it.

## Paths between systems (contract 7.6)

`crates/min-paths`' `translate` - the rule of minRender's `min-paths` branch (row 387, ruling 5): the first enabled
mapping whose source root matches on a folder boundary; a Windows root matched whatever its case; separators made the
target's in every value.

- **A leader giving a chunk to a node on another system than the submitter's** translates every flag value that is
  text, and the output folder, with ITS mappings; the program, the working folder and the environment are never
  touched (three tests).
- **Its own chunks too.** 0.5.x translates nothing when the leader gives itself a chunk, so a leader on macOS runs a
  job in Windows form with Windows paths. The port translates its own chunks like any node's. This differs from 0.5.x
  only on the leading machine; it is the owner's to veto.
- **A submission made on macOS** is put in the form manifests travel in: every flag value that is not empty and the
  output folder from macOS to Windows, and `submitted_os` says `windows`.

## Submissions (7.1, 7.2, 7.5, 4.2, 8.2)

- The templates on the share: plug-in ones, examples, users' own (which win over a shipped one of the same id).
- A job's id from its name, the first free of `id`, `id-2` ... `id-99` by the folders under `jobs/`.
- The manifest baked from a template and one value a flag: an optional value left empty goes, and takes the bare
  switch before it along; the output folder from the first output flag.
- The plug-ins' drop box, every three seconds: each `*.json` in `submissions/` becomes a job and is moved to
  `processed/`.

**Held to 0.5.11:** the same Blender add-on submission dropped into a 0.5.11 node's box and into the port's, on one
farm, with and without a scene name, with and without path mappings: the manifests the leader holds are IDENTICAL
(but for the id, the submitting node and the time), and so are the priorities.

Unlike 0.5.x (row 389's rulings, submitting): a submission is checked as the form checks one - a file with a
required value missing is logged ("Scene File (.blend)" is required) and set aside, not submitted; a file whose
leader does not answer stays in the box and is tried again, where 0.5.x moved it away and lost it; a file that could
not be moved is remembered and not submitted twice; a file still being written is given ten seconds.

## Not yet

- The share's content: this build reads templates that are there and makes none. A farm of ports alone has no Blender
  template until the bundled one is put on the share (row 390: Blender only; what is there is neither updated nor
  removed).
- A real render: everything so far ran the harness's stand-in renderer. Blender itself, on a share both systems
  reach, has not been run.
- The pre-0.5.8 drop box on Windows; the form (it comes with the interface); output-path patterns (row 389: restored
  as a step of its own).
