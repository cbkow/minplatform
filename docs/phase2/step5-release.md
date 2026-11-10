# Step 5 — the release: QCView 3.0 through QCView 2.x's channels

Read from QCView-Player at v2.7.3 (+3 commits, branch `min-paths`) on 2026-11-09: `scripts/` (RELEASE.md, sign-and-notarize.sh,
bundle_dylibs.sh, prune_bundle.sh, fetch_sparkle.sh, update_appcast.sh), `packaging/`, `installer/` (msix/AppxManifest.xml.in,
build_msix.ps1, README.md), `src/app/CMakeLists.txt`, `docs/` (the site, `appcast.xml`), `LICENSES/`. No secret is quoted here.
**Status: the spec. RULED 2026-11-09 (canon row 248) — nothing in this step is built yet.**

## What 2.x users have installed (so what 3.0 must be, to upgrade them in place)

| | macOS | Windows |
|---|---|---|
| Identity | bundle id `app.qcview.player`; Developer ID team `5Z4S9VHV56`; the bundle is `qcview.app`, executable `qcview`; arm64; macOS 13.0+ | MSIX `Name="cbkow.QCView"`, `Publisher="CN=C64A1D41-EFAA-4EED-AE81-0C074867D4E9"`, `Application Id="QCView"`, `Executable="qcview.exe"`, alias `qcview.exe`; x64; 10.0.17763+ |
| Version | `CFBundleShortVersionString` = `CFBundleVersion` = X.Y.Z (no build number) | `Identity Version` = X.Y.Z.0 |
| Updates | Sparkle 2.9.2: `SUFeedURL` = `https://qcview.app/appcast.xml`, the EdDSA public key in Info.plist, daily automatic checks | the Store; a sideloaded MSIX is replaced by hand |
| Opens | document types (QCView Project `.qcv` / `.qcvproj` as Owner; video and image types as Alternate), the `qcview://` scheme — delivered as Apple events | file type associations (project, video, image, AUDIO), protocol `qcview` — delivered as argv |

**The feed and the downloads live in the QCView-Player repo.** `https://qcview.app` is GitHub Pages from that repo's `docs/`
(`docs/CNAME`); `docs/appcast.xml` holds 44 items whose enclosures are `github.com/cbkow/QCView-Player/releases/download/v<ver>/
QCView-MacOS.dmg`; the site's download button is `…/releases/latest/download/QCView-MacOS.dmg` (so the DMG's name is fixed and
unversioned). The Store product is `9p4z15p5g805`. A 2.x Mac will only ever look at that feed, signed with that key.

## The 2.x flow, as practised (all by hand, no CI)

- **Version:** one field (`CMakeLists.txt` `project(… VERSION …)`); typed again for the appcast, the tag `vX.Y.Z` and the
  "What's new" block in `docs/installation.md`.
- **macOS:** build → stage `build/dist/qcview.app` → `macdeployqt` → `bundle_dylibs.sh` (every non-system dylib into
  `Contents/Frameworks`, `@rpath` ids, **refuses `/opt/homebrew` and `/usr/local`**) → `prune_bundle.sh` (Qt) → sign inside-out
  with `--options runtime --timestamp` (Sparkle's XPC services, Updater.app, Autoupdate, the framework; every dylib; helpers; the
  app last, with the entitlements) → zip → `notarytool submit --keychain-profile QCView --wait` → staple → DMG (`hdiutil`, UDZO,
  the app + an Applications symlink, volume "QCView X.Y.Z", `QCView-MacOS.dmg`) → sign, notarize and staple the DMG.
  Entitlements: `allow-unsigned-executable-memory`, `disable-library-validation`, `network.client`,
  `files.user-selected.read-write`; not sandboxed. Helpers: `Contents/Helpers/{ffmpeg,ffprobe}`. Icon: `qcview.icns` from
  `qcviewMac.png` (`sips` + `iconutil`), plus `Assets.car` from `qcviewMac.icon` (`actool`, macOS 26's icon; needs full Xcode).
- **Sparkle:** attach the DMG to the GitHub release FIRST, then `update_appcast.sh <version> <dmg>` (`sign_update` with the
  private key from the login keychain; one `<item>` inserted), commit, push. No deltas, no channels. In the app: About ▸ Check for
  Updates…, a button in the About modal, Settings ▸ "Automatically check for updates".
- **Windows, the Store:** `build_msix.ps1` (windeployqt → stage the exe, DLLs, FFmpeg, assets → the manifest, the tile PNGs,
  LICENSE, the privacy policy, the acknowledgments and the notices → `MakePri` → `MakeAppx pack`) → the UNSIGNED `.msix` uploaded
  to Partner Center by hand (Microsoft signs it). The app mirrors ffmpeg / ffprobe to `%LOCALAPPDATA%\QCView\bin` because other
  apps cannot spawn from `WindowsApps`.
- **Windows, sideload:** the same MSIX signed with a self-made certificate whose subject is the Store publisher CN; the user
  installs the `.cer` into Trusted People. **No sideload file is attached to any release and no doc says where to get one.**
- **Icons:** `qcviewMac.*` (macOS Dock / Finder); `qcview.png` / `.ico` (Windows taskbar, Explorer, the MSIX tiles);
  `qcviewcutout.*` (QCView's in-window title-bar icon on Windows); `qcview_export.png` (the note exports' mark).

## Gaps in 2.x that 3.0 inherits unless fixed

1. The macOS app and DMG ship NO licence or notices (the MSIX does); the app has no licences view.
2. The notices' GPL source offer names the QCView-Player repo.
3. The privacy policy says the app makes no automatic network connections; Sparkle checks daily.
4. The exe carries no version resource; the MSIX's resource index is built for en-us only though 12 languages are declared.
5. The two OSes register different file types (Windows adds audio and some containers; macOS adds `dpx`, `bmp`).
6. One-machine state: the Sparkle private key (keychain only — losing it orphans every installed Mac), the Developer ID
   identity and notary profile, the sideload certificate.

## What does not exist in the port yet

- macOS: an app bundle, Info.plist, the icon, the Sparkle updater, the Apple-event opens, and non-Homebrew builds of OCIO,
  OpenEXR, Imath, libjpeg-turbo and libtiff with a 13.0 floor (the release binary links Homebrew's today).
- Windows: an icon and version resource in the exe, the runtime window / title-bar icons, the DLL staging, the CRT choice, the MSIX.
- Both: a generated third-party notices file for the Rust crates; the ffmpeg / ffprobe helpers question.

## Rulings (2026-11-09, canon row 248)

1. **Identity unchanged** on both OSes (the table above), so 2.x upgrades in place. A real 2.7.3 → 3.0 Sparkle update is
   proven against a private test feed before the public feed gets its item.
2. **No import of 2.x settings.** 3.0 starts from its own defaults; 2.x project files still open.
3. **Sideload is the user's own test vehicle.** The signing script stays; nothing is published or documented for users.
4. **Licences: files plus an in-app view.** LICENSE and a generated third-party notices file in both packages (gap 1, gap 2).
5. **One file-type list for both OSes, only what 3.0 opens** (gap 5).
6. **Icons:** the macOS icon = Dock / Finder and the in-window title-bar icon on Windows; the regular Windows icon = taskbar,
   desktop, Store tiles.
7. **Platforms unchanged** (macOS arm64 13.0+, Windows x64); the privacy policy is corrected for the updater (gap 3).
8. **Repos:** source here; QCView-Player is the product repo (releases, site, feed, issues, notices) — a clean `main` prepared
   on a review branch, 2.x kept on a `2.x` branch cut at release, a source archive of this repo's tag on every release.

## Order of work

macOS packaging first (native libraries off Homebrew → the bundle, Info.plist and icon → Apple-event opens → Sparkle → sign /
notarize / DMG → the test update), then Windows (icon and version resource, window icons, DLL staging, the CRT, the MSIX, an
upgrade test), then the notices, the privacy policy and the 3.0 docs, then QCView-Player's clean `main`.

## Progress

- **2026-11-09, macOS:** the native libraries are built from source for 13.0 (`tools/deps/build-macos.sh`, pins in
  `tools/deps/deps.pin`; FFmpeg's dav1d from source, X11 / xcb off) — nothing links Homebrew. `packaging/macos/make-app.sh`
  stages `target/dist/qcview.app` (2.x's identity; Info.plist generated by `make_plist.py` with the document types from
  `extensions.rs`; the .icns and the macOS 26 layered icon; the dylibs and Sparkle.framework in Frameworks; ad-hoc signed).
  The Finder's documents and `qcview://` links open (`mac_open.rs`, an application delegate). Sparkle 2.9.2
  (`tools/sparkle/fetch.sh`, `sparkle.rs`: loaded at run time, About ▸ Check for Updates…, Settings ▸ Updates). Version 3.0.0.
- **2026-11-09, macOS, later:** signed and notarized (`packaging/macos/sign-macos.sh`: hardened runtime, NO entitlements; the
  app and `QCView-MacOS.dmg` accepted). The 2.7.3 → 3.0 update PROVEN on a copy of 2.7.3 against a private local feed (the
  DMG signed with the keychain's Sparkle key was accepted, the bundle replaced in place, Sparkle starts in the signed 3.0 and
  reads the real feed). QCView is the suite's ffmpeg provider again (`toolbox.rs`, `Contents/Helpers/{ffmpeg,ffprobe}`).
- **2026-11-09, Windows:** `apps/qcv/build.rs` (the exe's icon, version details, the long-path manifest); the two window icons;
  `packaging/windows/make-msix.ps1` — stages by WALKING IMPORT TABLES (the VC++ runtime ships: 2.x shipped none), generates
  the manifest (2.x's identity; file types from `extensions.rs`), packs the unsigned MSIX (101 MB) and can sign a private
  sideload copy (`-SignThumbprint`: a test certificate in the user's store, made on the box 2026-11-09 — the old PFX's
  password is lost). Proven on the box: a Windows Sandbox with NO VC++ runtime loads and starts the staged build; the
  sideloaded 2.7.3 was replaced IN PLACE by 3.0.0.0 (same package family), which runs from WindowsApps, mirrors the ffmpeg
  tools and registers its 55 file types and the `qcview.exe` alias.
- **Rows 249–251** the same day: the title bar / menu row / rails are one surface on Windows; no usable graphics device →
  a dark message and exit; dragging the picture moves the window (compact mode too).
- **Open:** a walk of 3.0 on macOS 13 / 14 (the user's other Macs); the Windows libraries pinned to the Mac's set; the generated
  third-party notices + the in-app Licences view; the privacy policy; the 3.0 docs; QCView-Player's clean `main`; the Store
  submission; the Sparkle private key's backup (the user's).
