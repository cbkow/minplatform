# minRender 1.0 — Microsoft Store listing and notes (draft)

Drafted 2026-11-15 for the owner to edit and paste (canon rows 421, 423, 431). English only. Store ID 9PN4VXDZ3THW;
the package is `minRender-<version>-win64.msix` from `packaging/windows/make-msix-minrender.ps1` (identity
`cbkow.minRender`, the same as 0.5.11, so the Store installs it as an update). 0.5.x's listing is the base; what
changed is marked.

## Description

Policy 10.2.4: the dependency on other software comes FIRST.

> **Requires Blender installed on each render node.** minRender is a lightweight render-farm manager for small
> studios. It coordinates render jobs across the Windows and macOS machines on your own network, launching the
> Blender already installed on each node to render frame ranges in parallel, and shows progress, logs and frame
> previews in one place. Nodes find each other over the LAN and a shared folder; nothing leaves your network.
> Submit from the included Blender add-on or from the app.

(Changed: Blender only — After Effects and Cinema 4D are not part of 1.0.)

## What's new in this version

minRender 1.0 is the same farm, rebuilt from the ground up.

- Works in one farm with minRender 0.5.10 and 0.5.11 nodes: upgrade one machine at a time. A node keeps its
  settings and its identity.
- Edit a running job; submit part of a job as a separate job; Farm Cleanup.
- EXR layers in the frame preview.
- A node that starts before its shared folder is reachable waits for it and joins by itself.
- Safer on synced folders (Dropbox, Synology Drive and the like) as the farm's shared folder.
- Blender 5.3 templates and add-on. Install the add-on again from the shared folder's `plugins/blender`.
- After Effects and Cinema 4D templates and plug-ins are no longer included.

## Restricted capability: runFullTrust

> Win32 render-farm manager. Launches the locally installed Blender as a child process on the user's own machines to
> render frame ranges, and supervises it through a second instance of its own executable. No code is downloaded or
> executed beyond the installed render application.

(Changed: there is no bundled render agent any more; the supervisor is the application itself.)

The package declares exactly the capabilities of 0.5.11.0: `internetClientServer`, `privateNetworkClientServer`,
`runFullTrust`.

## Notes for certification (~200 words)

```
This version replaces 0.5.11.0 under the same identity and declares the same three capabilities. App data lives in a normal per-user folder, %USERPROFILE%\minRender, as before.

minRender is a render-farm manager for small studios. It coordinates render jobs across machines on the user's own LAN and launches the Blender already installed there. No account or internet service is needed.

To test without a farm: the app starts and shows "NO FARM" in its title row. Click Settings (top right), set "Sync root" to any empty local folder and press Save; the app restarts itself. The machine becomes a one-node farm, shown in the title row with a LEADER badge. New Job opens the submission form; actual rendering needs Blender installed.

Expected behavior: closing the window hides the app to the tray icon (quit via tray > Exit). Declared firewall rules (TCP 8420, UDP 4243) allow LAN peer discovery without a prompt. A startup task launches it hidden at sign-in.

Uninstall removes the app, startup task and firewall rules. %USERPROFILE%\minRender is kept so a reinstall keeps farm settings; removal is documented at https://minrender.com/install/#uninstall.
```

## Submission routine (0.5.x's, kept)

1. Regenerate the notices on the Mac if `Cargo.lock`, the version or `components.json` changed, and commit.
2. Build and pack on the Windows machine; the pack script's two guards must pass.
3. New submission in Partner Center, upload the unsigned `.msix`, paste the justification and the notes.
4. **Publish this submission manually**, so a passed certification never lands on nodes in the middle of a job.
5. Farm nodes: Store auto-update off; with the farm idle,
   `winget upgrade --id 9PN4VXDZ3THW --source msstore --accept-package-agreements`.
