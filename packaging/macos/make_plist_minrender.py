#!/usr/bin/env python3
"""Info.plist for minRender's macOS bundle (canon row 427). The identity is minRender 0.5.x's - the bundle id, the
executable's name, the update feed and its public key (the family's) - so 1.0 replaces 0.5.x in place: an installed
0.5.11 only ever trusts that feed and key, and 1.0 goes on reading the same feed. The application shows in the Dock
and has a menu bar beside its menu-bar item, as 0.5.x (LSUIElement false).
Usage: make_plist_minrender.py <repo root> <out Info.plist>"""
import plistlib, re, sys, pathlib

root, out = pathlib.Path(sys.argv[1]), sys.argv[2]
version = re.search(r'^version\s*=\s*"([^"]+)"', (root / "apps/mr/Cargo.toml").read_text(), re.M).group(1)
plist = {
    "CFBundleDevelopmentRegion": "en", "CFBundleExecutable": "minRender", "CFBundleIdentifier": "studio.bialkow.minrender", "CFBundleInfoDictionaryVersion": "6.0",
    "CFBundleName": "minRender", "CFBundleDisplayName": "minRender", "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": version, "CFBundleVersion": version,   # 0.5.x: both X.Y.Z (Sparkle compares these)
    "CFBundleIconFile": "minrender.icns",
    "LSMinimumSystemVersion": "13.0", "NSHighResolutionCapable": True, "NSSupportsAutomaticGraphicsSwitching": True, "LSUIElement": False,
    "LSApplicationCategoryType": "public.app-category.developer-tools",
    "NSHumanReadableCopyright": "minRender is free software under the GNU General Public License v3.",
    "SUFeedURL": "https://minrender.com/appcast.xml", "SUPublicEDKey": "+Xlrad3WswxIRTyzBTp8KaTEjyfPO5Ap4llmPdZf3RQ=",
    "SUEnableAutomaticChecks": True, "SUScheduledCheckInterval": 86400, "SUEnableSystemProfiling": False,
}
with open(out, "wb") as f: plistlib.dump(plist, f, sort_keys=False)
print(f"Info.plist: minRender {version}")
