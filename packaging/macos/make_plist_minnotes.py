#!/usr/bin/env python3
"""Info.plist for minNotes' macOS bundle (canon row 347). The identity is minNotes 2.x's (`app.minnotes`, the `.mnd`
and `.mnpkg` types it declared, the `minnotes:` scheme), so 3.0 replaces it in place - and 2.x's update feed and public
key (canon row 348): an installed 2.x only ever trusts these, and 3.0 goes on reading the same feed.
Usage: make_plist_minnotes.py <repo root> <out Info.plist> [--icon-name NAME]"""
import plistlib, re, sys, pathlib

root, out = pathlib.Path(sys.argv[1]), sys.argv[2]
icon_name = sys.argv[sys.argv.index("--icon-name") + 1] if "--icon-name" in sys.argv else None
version = re.search(r'^version\s*=\s*"([^"]+)"', (root / "apps/mn/Cargo.toml").read_text(), re.M).group(1)
def exported(ext, what): return {"UTTypeIdentifier": f"app.minnotes.{ext}", "UTTypeDescription": what, "UTTypeConformsTo": ["public.data"],
                                 "UTTypeTagSpecification": {"public.filename-extension": [ext]}}
plist = {
    "CFBundleDevelopmentRegion": "en", "CFBundleExecutable": "minNotes", "CFBundleIdentifier": "app.minnotes", "CFBundleInfoDictionaryVersion": "6.0",
    "CFBundleName": "minNotes", "CFBundleDisplayName": "minNotes", "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": version, "CFBundleVersion": version,   # 2.x: both X.Y.Z (Sparkle compares these)
    "CFBundleIconFile": "minnotes.icns",
    "LSMinimumSystemVersion": "13.0", "NSHighResolutionCapable": True, "NSSupportsAutomaticGraphicsSwitching": True,
    "LSApplicationCategoryType": "public.app-category.productivity",
    "NSHumanReadableCopyright": "minNotes is free software under the GNU General Public License v3.",
    "SUFeedURL": "https://minnotes.app/appcast.xml", "SUPublicEDKey": "+Xlrad3WswxIRTyzBTp8KaTEjyfPO5Ap4llmPdZf3RQ=",
    "SUEnableAutomaticChecks": True, "SUScheduledCheckInterval": 86400, "SUEnableSystemProfiling": False,
    "CFBundleDocumentTypes": [
        {"CFBundleTypeName": "minNotes Document", "CFBundleTypeRole": "Editor", "LSHandlerRank": "Owner", "LSItemContentTypes": ["app.minnotes.mnd"]},
        {"CFBundleTypeName": "minNotes Package", "CFBundleTypeRole": "Viewer", "LSHandlerRank": "Owner", "LSItemContentTypes": ["app.minnotes.mnpkg"]}],
    "CFBundleURLTypes": [{"CFBundleURLName": "minNotes Document URI", "CFBundleURLSchemes": ["minnotes"], "CFBundleTypeRole": "Editor"}],
    "UTExportedTypeDeclarations": [exported("mnd", "minNotes Document"), exported("mnpkg", "minNotes Package")],
}
if icon_name: plist["CFBundleIconName"] = icon_name   # macOS 26's layered icon in Assets.car; older systems use the .icns
with open(out, "wb") as f: plistlib.dump(plist, f, sort_keys=False)
print(f"Info.plist: minNotes {version}")
