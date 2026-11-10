# Step 5 — the release: QCView 3.0 through QCView 2.x's channels

Read from QCView-Player at v2.7.3 (+3 commits, branch `min-paths`) on 2026-11-09: `scripts/` (RELEASE.md, sign-and-notarize.sh,
bundle_dylibs.sh, prune_bundle.sh, fetch_sparkle.sh, update_appcast.sh), `packaging/`, `installer/` (msix/AppxManifest.xml.in,
build_msix.ps1, README.md), `src/app/CMakeLists.txt`, `docs/` (the site, `appcast.xml`), `LICENSES/`. No secret is quoted here.
**Status: the spec. The rulings are OPEN — nothing in this step is built.**

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

## Rulings needed

See the proposal of 2026-11-09 (canon row to follow once ruled).
