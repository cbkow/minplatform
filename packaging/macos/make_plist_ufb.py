#!/usr/bin/env python3
"""Info.plist for UFB's macOS bundle (canon rows 457, 519, 521).

`test`    - the TEST package: its own bundle id and name ("UFB Test"), a link scheme of its own (`ufbtest:` -
            an installed 1.4.2 keeps `ufb:` and `union:`), no updater. It installs beside the real application and shares nothing with it.
`release` - 1.4.2's identity: the bundle id, the feed and the update key, the two link schemes. NOT made yet: the
            script refuses it until the release's pieces (the updater, the notices, the transition package) are there.

Usage: make_plist_ufb.py <repo root> <out Info.plist> <test|release>"""
import plistlib, re, sys, pathlib

root, out, mode = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
if mode != "test": sys.exit("make_plist_ufb.py: only the test package is made so far")
version = re.search(r'^version\s*=\s*"([^"]+)"', (root / "apps/ufb/Cargo.toml").read_text(), re.M).group(1).split("-")[0]
plist = {
    "CFBundleDevelopmentRegion": "en", "CFBundleExecutable": "UFB", "CFBundleIdentifier": "dev.ufb.app.test", "CFBundleInfoDictionaryVersion": "6.0",
    "CFBundleName": "UFB Test", "CFBundleDisplayName": "UFB Test", "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": version, "CFBundleVersion": version, "CFBundleIconFile": "ufb.icns",
    "LSMinimumSystemVersion": "13.0", "NSHighResolutionCapable": True, "NSSupportsAutomaticGraphicsSwitching": True, "LSUIElement": False,
    "LSApplicationCategoryType": "public.app-category.utilities",
    "NSHumanReadableCopyright": "UFB is free software under the GNU General Public License v3.",
    # what the system asks about before it lets the application at these places
    "NSNetworkVolumesUsageDescription": "UFB shows the files on your network shares.",
    "NSRemovableVolumesUsageDescription": "UFB shows the files on your external drives.",
    "NSDesktopFolderUsageDescription": "UFB shows the files on your Desktop.", "NSDocumentsFolderUsageDescription": "UFB shows the files in your Documents folder.", "NSDownloadsFolderUsageDescription": "UFB shows the files in your Downloads folder.",
    "NSLocalNetworkUsageDescription": "UFB finds the other machines it syncs metadata with on the local network.",
    # the test package's own link scheme (the application's are `ufb:` and `union:`, which an installed 1.4.2 keeps):
    # `ufbtest:///a/folder` is read as `ufb:///a/folder` - the way a link reaches the application can be tried
    "CFBundleURLTypes": [{"CFBundleURLName": "UFB Test link", "CFBundleTypeRole": "Viewer", "CFBundleURLSchemes": ["ufbtest"]}],
}
with open(out, "wb") as f: plistlib.dump(plist, f, sort_keys=False)
print(f"Info.plist: UFB Test {version}")
