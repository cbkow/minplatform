What minRender puts on a farm's share (canon row 390: Blender only for this release).

The plug-in template and the add-on are copied UNCHANGED from the minRender repository (branch `min-paths`, files last changed in b0cfda1,
release 0.5.11): the add-on is installed by hand on workstations and names this template by its id, so neither may
drift from what is in the field. The After Effects and Cinema 4D templates and plug-ins stay in that repository as a
starting point; this build neither ships them nor removes copies a farm already has.

`templates/blender-5.2-studio.json` is that repository's Blender example template (the one the New Job form offers),
with its id and name changed to "Studio" (the owner, 2026-11-14: "Just call it Studio") and nothing else. A farm that
already has the example under its old name keeps that file too: nothing on a share is removed.
