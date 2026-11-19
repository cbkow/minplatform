# Phase 4, step 10 - minRender: the Microsoft Store release flow of 0.5.x, and what 1.0.0 must keep (2026-11-15)

The owner, 2026-11-15: "lets inspect the old windows store release flow for the old minRender so we keep that going and
can upgrade from there. Lets bump to 1.0.0 for this release." Read from the minRender repository (branch `min-paths` @
9526557 = 0.5.11): `installer/STORE.md`, `installer/msix/AppxManifest.xml.in`, `installer/msix/Assets`,
`scripts/make_msix.ps1`, `scripts/package.bat`. Nothing there was changed. On the Windows machine the installed package
was read (name and version only); nothing was installed, removed or started.

## 1. The flow as it is

| Step | 0.5.x |
|---|---|
| Channel | **The Microsoft Store only** since 0.5.6. The Store signs the package at ingestion and delivers updates; the Windows build has no updater. The old installer and its feed are retired (the feed frozen at 0.5.5). |
| Identity | Three values from Partner Center's "Product identity" page, given to the pack script as environment variables (`MR_MSIX_IDENTITY_NAME`, `MR_MSIX_PUBLISHER`, `MR_MSIX_PUBLISHER_DISPLAY`) - never in the repository. A mismatch is refused at ingestion. Store ID `9PN4VXDZ3THW`. |
| Version | Four parts, the application's three plus `.0`. A package that was denied is resubmitted under a HIGHER version. |
| Pack | `scripts\package.bat --msix`: stage the build's deploy folder, copy `Assets`, fill the manifest template, strip its comments, CHECK the capabilities are exactly the three below and that nothing disables virtualization, `makeappx pack`. Output unsigned: the Store signs. |
| Local test | Developer Mode, `make_msix.ps1 -Sideload -SkipPack`, then `Add-AppxPackage -Register <stage>\AppxManifest.xml` - in the interactive desktop session only (over SSH it fails with `0x80070005`). |
| Submission | A new submission in Partner Center, the `.msix` uploaded, the `runFullTrust` justification and the notes for certification pasted. **"Publish this submission manually"**, so a passed certification never lands on nodes in the middle of a job. Typically 1-3 working days. |
| Rollout | Store auto-update is turned OFF on farm nodes. With the farm idle, every node together: `winget upgrade --id 9PN4VXDZ3THW --source msstore --accept-package-agreements`. A workstation may keep auto-update on. |

## 2. The manifest, line by line

| Item | 0.5.x | Why |
|---|---|---|
| `Identity` | name / publisher from Partner Center, `ProcessorArchitecture="x64"` | the Store's key for "this is an update of that" |
| `DisplayName`, logos | "minRender"; `StoreLogo`, `Square150x150Logo`, `Square44x44Logo` (+ `scale-200`, `targetsize-44_altform-unplated`) | |
| `TargetDeviceFamily` | `Windows.Desktop`, `MinVersion 10.0.19041.0`, `MaxVersionTested 10.0.26100.0` | Windows 10 2004 and later |
| Capabilities | `internetClientServer`, `privateNetworkClientServer`, `runFullTrust` - exactly these | the 0.5.6.0 denial was for `unvirtualizedResources`; since 0.5.8 nothing but `runFullTrust` needs a justification |
| Application | `Id="minRender"`, `Executable="minRender.exe"`, `EntryPoint="Windows.FullTrustApplication"`, `BackgroundColor="transparent"` | |
| Startup task | `windows.startupTask`, `TaskId="minRenderStartup"`, `Enabled="true"`, executable **`mr-restart.exe`** | a startup task cannot pass arguments; `mr-restart.exe` with none starts the application minimized |
| Firewall rules | `windows.firewallRules` for `minRender.exe`: inbound TCP **8420** and UDP **4243**, all profiles | no firewall prompt at the first farm start |
| Headless | NO application entry and no alias for `minrender-headless.exe` | hiding it from Start needs a waiver Partner Center refused |

Around it: the data folder is `%USERPROFILE%\minRender` - outside AppData ON PURPOSE, so the package never redirects it
and the plug-ins (which run inside Blender, without the package's identity) read the same `config.json` and write the
same `submissions\`. Registry writes stay redirected (the RNDR companion falls back to its HTTP poll). Uninstalling
keeps the data folder.

## 2a. The ground truth: the package the Store has installed (read 2026-11-15)

The owner, the same day: "specifically exactly the capabilities the last version declares as that is the only one on
the windows store right now". So the measure is not the repository's template but the manifest INSIDE the installed
package (`cbkow.minRender` 0.5.11.0, signature kind Store, x64), read on the Windows machine without changing anything.
It agrees with the template in every line; the Store added only its own `mp:PhoneIdentity` element at ingestion.

**The capabilities of 0.5.11.0, verbatim - and of 1.0.0.0, with none added and none dropped:**

```xml
<Capabilities>
  <Capability Name="internetClientServer" />
  <Capability Name="privateNetworkClientServer" />
  <rescap:Capability Name="runFullTrust" />
</Capabilities>
```

Also as installed, and to be the same in 1.0.0: the namespaces `uap`, `uap3`, `desktop`, `desktop2`, `rescap` (all
ignorable); `TargetDeviceFamily Windows.Desktop`, `MinVersion 10.0.19041.0`, `MaxVersionTested 10.0.26100.0`; one
application, `Id="minRender"`, `Executable="minRender.exe"`, `EntryPoint="Windows.FullTrustApplication"`; one startup
task, `TaskId="minRenderStartup"`, `Enabled="true"`, `DisplayName="minRender"`; the firewall rules for `minRender.exe`,
inbound TCP 8420 and UDP 4243, `Profile="all"`; `PublisherDisplayName` "cbkow"; the seven logo files. The publisher is
the one minNotes' and QCView's tracked manifests already carry.

**The only lines that differ in 1.0.0**: `Version`; the startup task's `Executable` (`minRender.exe` instead of
`mr-restart.exe`, K4); `Description` (Blender only, row 390). The package's root loses `mr-agent.exe`,
`mr-restart.exe` and `minrender-headless.exe`.

**The pack script's guard** (as 0.5.x's `make_msix.ps1` had): it refuses to pack unless the sorted capability list is
exactly `internetClientServer, privateNetworkClientServer, runFullTrust`, and it compares the generated manifest
against the installed one (kept as `fixtures/minrender/store-0.5.11-AppxManifest.xml`, not tracked) and prints every
difference, so that nothing beyond the three lines above changes unseen.

## 3. What 1.0.0 must keep for the Store to treat it as an update, and for a node to come back as itself

| # | Keep | State in `apps/mr` today |
|---|---|---|
| K1 | The identity's name and publisher, x64. The installed package on the test machine is `cbkow.minRender` 0.5.11.0, signature kind Store - the same publisher as minNotes' package, whose pack script already carries it. | to build (the pack script) |
| K2 | A version above 0.5.11.0: **1.0.0.0**. | bumped in this step |
| K3 | The application's `Id` ("minRender") and executable name (`minRender.exe`). | the binary already has that name |
| K4 | The startup task's `TaskId` ("minRenderStartup") - the same id keeps each user's on / off choice in Task Manager. Its executable becomes `minRender.exe` itself: spike S14 showed the application can tell that the startup task started it and then starts minimized, so `mr-restart.exe` is not carried. **S14's open item stands: that start has been installed and has never fired at a sign-in.** | the detection is in S14, not yet in `apps/mr`'s package |
| K5 | The firewall rules: TCP 8420 and UDP 4243 for `minRender.exe`. The leash is the same executable, so one rule set covers it. | the node's defaults are 8420 / 4243 |
| K6 | **Exactly the capabilities of the installed 0.5.11.0** (section 2a) - none added, none dropped; the pack script's guard against drift. | to build |
| K7 | **The data folder: `%USERPROFILE%\minRender`** (macOS: `~/Library/Application Support/MinRender`), read when the application is started with no `--data`. That is what makes an upgraded node the SAME node: its `node_id.txt`, its `config.json` (sync root, tags, port, path mappings), its `submissions\` for the add-on. | **NOT there: `--data` is required today, on purpose, so that no test could touch a real node's folder.** The default arrives with the package and nowhere before it. |
| K8 | `config.json` written whole and never losing a key it does not know (row 389). | done |
| K9 | No headless alias, no second application entry. | `--no-window` and `--leash` are arguments of the one executable |
| K10 | Minimum Windows 10 2004, `Windows.Desktop`. | to check against wgpu's needs on Windows 10 |
| K11 | The logos: the seven files of `installer/msix/Assets`. | to vendor with the pack script |

What is deliberately NOT kept: `mr-agent.exe`, `mr-restart.exe`, `minrender-headless.exe`, the Qt runtime; the copy of
a pre-0.5.8 `%LOCALAPPDATA%\MinRender` folder (every node is on the latest: the owner, 2026-11-14).

## 4. What changes in the submission's words

- The description opens with the dependency (policy 10.2.4): now **"Requires Blender installed on each render node."**
  After Effects and Cinema 4D are gone from this release (row 390); the manifest's `Description` says Blender only.
- The `runFullTrust` justification: "launches the locally installed Blender as a child process on the user's own
  machines to render frame ranges" - there is no bundled agent any more; the application supervises the renderer through
  a second copy of ITSELF (the leash). No code is downloaded or run beyond the installed renderer.
- Notes for certification: Settings is a button in the title row (0.5.x: a menu at the top left); the rest holds - an
  empty local folder as the sync root makes a one-node farm with a LEADER badge, closing the window hides it to the
  tray, tray > Exit quits, the two firewall rules, the startup task, the data folder kept at uninstall.

## 5. Risks to settle before the submission

1. **The startup task has never been seen to fire** with the application as its own executable (S14, open). One
   sign-out and sign-in on the test machine settles it; until then K4 is unproven.
2. **Defender and the leash.** S13 passed for a relaunch of our own signed binary; the real package is a new binary
   and is scanned again. Defender-scan the package on the build machine, as 0.5.x's routine does.
3. **A node upgraded in the middle of a mixed farm.** The node keeps its id and its place; 0.5.10 / 0.5.11 nodes beside
   it are the case the whole port was built against (rows 391-400). The share's content stamp rises to 1.0.0, so the
   upgraded node writes the 5.3 templates and the 1.0.0 add-on to the share and removes nothing (row 417).
4. **An update of a RUNNING node.** The Store replaces the package's files; what it does to a running full-trust
   process that is rendering is not established here. 0.5.x's answer was procedural (auto-update off on nodes, `winget
   upgrade` with the farm idle) and stays the recommendation.
5. **Windows 10.** The presenter's needs (D3D12 through wgpu) against the 10.0.19041 floor have not been tried; the
   farm's own machines decide whether the floor can stay.

## 6. The routine for 1.0.0, as proposed

1. Version in `apps/mr/Cargo.toml` (the add-on's `bl_info` follows it).
2. On the Windows machine: build; a pack script on the lines of minNotes' (`packaging/windows/make-msix-minnotes.ps1`):
   the import walk for DLLs, the manifest of section 2 with K1-K11, the capability guard, `makeappx`; an unsigned
   package for the Store and a test-signed copy for sideloading.
3. Sideload on the test machine in the desktop session - **over the installed Store 0.5.11 only with the owner's word**
   (it IS the production node: its data folder is the real one). A first proof can run under a test identity beside
   it, which is what the separate test identity of row 389 is for.
4. The owner submits, with manual publishing; nodes are upgraded together by `winget upgrade`, farm idle.
5. macOS goes its own way, as before: a notarized DMG and the Sparkle feed at minrender.com (0.5.x:
   `scripts/macos-release.sh`, `scripts/update_appcast.sh`; team and notary profile as the other two applications).

## 1.0.2 (2026-11-19, rows 593-594): what the release taught

- **A kill test on Windows is run from INSIDE a package.** A program started by a packaged executable, when it is not
  of the package, is started outside the starter's job; the leash's job did not hold the renderer from the Store
  package while it did from the same executable unpackaged, which is where every earlier kill test had run. The leash
  now puts the renderer in its job by hand after starting it. To test: install the test-identity package from the
  desktop session (an install over ssh is refused), run the check with `Invoke-CommandInDesktopPackage`, remove the
  package again.
- **The notices are generated after the version's `Cargo.lock` is committed** (the check file records HEAD's), and
  `make_notices.py windows --app mr` needs `--out packaging/notices/generated/minrender-THIRD-PARTY-NOTICES-windows.html`.
- A wait for a script's "done" file starts by deleting the old one, or it returns at once.
- Still open: a real Blender render stopped on a Store-installed 1.0.2 node (a stand-in tree was what was killed).

