#!/usr/bin/env python3
"""Info.plist for UFB's macOS bundle (canon rows 457, 519, 521).

`test`    - the TEST package: its own bundle id and name ("UFB Test"), a link scheme of its own (`ufbtest:` -
            an installed 1.4.2 keeps `ufb:` and `union:`), no updater. It installs beside the real application and shares nothing with it.
`release` - 1.4.2's identity: the bundle id, the feed and the update key, the two link schemes.

Usage: make_plist_ufb.py <repo root> <out Info.plist> <test|release>"""
import plistlib, re, sys, pathlib

root, out, mode = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
if mode not in ("test", "release"): sys.exit("make_plist_ufb.py: test or release")
version = re.search(r'^version\s*=\s*"([^"]+)"', (root / "apps/ufb/Cargo.toml").read_text(), re.M).group(1).split("-")[0]
test = mode == "test"; name = "UFB Test" if test else "UFB"
plist = {
    "CFBundleDevelopmentRegion": "en", "CFBundleExecutable": "UFB", "CFBundleIdentifier": "dev.ufb.app.test" if test else "dev.ufb.app", "CFBundleInfoDictionaryVersion": "6.0",
    "CFBundleName": name, "CFBundleDisplayName": name, "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": version, "CFBundleVersion": version, "CFBundleIconFile": "ufb.icns",
    "LSMinimumSystemVersion": "13.0", "NSHighResolutionCapable": True, "NSSupportsAutomaticGraphicsSwitching": True, "LSUIElement": False,
    "LSApplicationCategoryType": "public.app-category.utilities",
    "NSHumanReadableCopyright": "UFB is free software under the GNU General Public License v3.",
    # what the system asks about before it lets the application at these places
    "NSNetworkVolumesUsageDescription": "UFB shows the files on your network shares.",
    "NSRemovableVolumesUsageDescription": "UFB shows the files on your external drives.",
    "NSDesktopFolderUsageDescription": "UFB shows the files on your Desktop.", "NSDocumentsFolderUsageDescription": "UFB shows the files in your Documents folder.", "NSDownloadsFolderUsageDescription": "UFB shows the files in your Downloads folder.",
    "NSLocalNetworkUsageDescription": "UFB finds the other machines it syncs metadata with on the local network.",
    # the links: the release's are 1.4.x's two; the test package has one of its own (`ufbtest:///...` is read as
    # `ufb:///...`), so that an installed 1.4.2 keeps `ufb:` and `union:` and the way a link arrives can still be tried
    "CFBundleURLTypes": [{"CFBundleURLName": "UFB Test link", "CFBundleTypeRole": "Viewer", "CFBundleURLSchemes": ["ufbtest"]}] if test else
                        [{"CFBundleURLName": "UFB link", "CFBundleTypeRole": "Viewer", "CFBundleURLSchemes": ["ufb", "union"]}],
}
# the release keeps 1.4.x's update feed and key (canon rows 457, 536): an installed 1.4.x only ever trusts those
if not test: plist.update({"SUFeedURL": "https://ufbrowser.com/appcast.xml", "SUPublicEDKey": "+Xlrad3WswxIRTyzBTp8KaTEjyfPO5Ap4llmPdZf3RQ=", "SUEnableAutomaticChecks": True, "SUScheduledCheckInterval": 86400, "SUEnableSystemProfiling": False})
with open(out, "wb") as f: plistlib.dump(plist, f, sort_keys=False)
print(f"Info.plist: {name} {version}")
