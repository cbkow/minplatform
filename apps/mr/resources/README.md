What minRender puts on a farm's share (canon row 390: Blender only for this release).

**From 1.0.0 these files are the source** (canon row 417, 2026-11-15): the Blender add-on and its two templates are
kept here and changed here. They began as copies of the minRender repository's files (branch `min-paths`, b0cfda1,
release 0.5.11); that repository's copies are frozen with 0.5.x. The After Effects and Cinema 4D templates and
plug-ins stay there as a starting point; this build neither ships them nor removes copies a farm already has.

- `plugins/blender/MinRender.py` - the add-on, installed by hand on workstations (version 1.0.2). It names its
  template by id: `blender-5.3-plugin`.
- `templates/plugins/blender-5.3-plugin.json` - the template the add-on's submissions are baked from.
- `templates/blender-5.3-studio.json` - the template the New Job form offers ("Studio": the owner, 2026-11-14).

Against the 5.2 files only three things changed: the ids, the names, and the Windows program path
(`…/Blender Foundation/Blender 5.3/blender.exe`). Nothing on a share is ever removed: a farm keeps its
`blender-5.2-…` files, and an add-on 0.5.11 still installed on a workstation keeps submitting through
`blender-5.2-plugin` (to the Blender 5.2 path) until that workstation gets the new add-on.

A template's id carries the Blender version because the Windows program path does. Every Blender release therefore
means: new template files here, the add-on's `TEMPLATE_ID` and version, and the add-on installed again by hand.
