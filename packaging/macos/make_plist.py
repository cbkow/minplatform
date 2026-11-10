#!/usr/bin/env python3
"""Info.plist for the macOS bundle (canon row 248). The identity is QCView 2.x's, so 3.0 upgrades it in place; the document
types are generated from the app's own lists (crates/qcv-project/src/extensions.rs) — one list for both OSes, only what the
app opens. Usage: make_plist.py <repo root> <out Info.plist> [--icon-name NAME]"""
import plistlib, re, sys, pathlib

root, out = pathlib.Path(sys.argv[1]), sys.argv[2]
icon_name = sys.argv[sys.argv.index("--icon-name") + 1] if "--icon-name" in sys.argv else None
version = re.search(r'^version\s*=\s*"([^"]+)"', (root / "apps/qcv/Cargo.toml").read_text(), re.M).group(1)
src = (root / "crates/qcv-project/src/extensions.rs").read_text()
def exts(name): return re.findall(r'"([a-z0-9]+)"', re.search(rf"pub const {name}: &\[&str\] = &\[(.*?)\];", src).group(1))

# Extensions the system has a type for; the rest are claimed by extension (a dictionary with content types ignores extensions).
UTI = {
    "mov": "com.apple.quicktime-movie", "mp4": "public.mpeg-4", "m4v": "com.apple.m4v-video", "avi": "public.avi",
    "webm": "org.webmproject.webm", "mxf": "org.smpte.mxf", "mpg": "public.mpeg", "mpeg": "public.mpeg", "mpe": "public.mpeg",
    "ts": "public.mpeg-2-transport-stream", "mts": "public.avchd-mpeg-2-transport-stream", "m2ts": "public.avchd-mpeg-2-transport-stream",
    "3gp": "public.3gpp", "3g2": "public.3gpp2", "gif": "com.compuserve.gif", "webp": "org.webmproject.webp", "dv": "public.dv-movie",
    "wav": "com.microsoft.waveform-audio", "aif": "public.aiff-audio", "aiff": "public.aiff-audio", "aifc": "public.aifc-audio",
    "mp3": "public.mp3", "m4a": "com.apple.m4a-audio", "caf": "com.apple.coreaudio-format", "flac": "org.xiph.flac",
    "png": "public.png", "jpg": "public.jpeg", "jpeg": "public.jpeg", "tif": "public.tiff", "tiff": "public.tiff",
    "exr": "com.ilm.openexr-image", "bmp": "com.microsoft.bmp", "tga": "com.truevision.tga-image",
}
def doc_types(name, kind):
    known = sorted({UTI[e] for e in exts(kind) if e in UTI}); rest = [e for e in exts(kind) if e not in UTI]
    base = {"CFBundleTypeRole": "Viewer", "LSHandlerRank": "Alternate"}
    out = [dict(base, CFBundleTypeName=name, LSItemContentTypes=known)]
    if rest: out.append(dict(base, CFBundleTypeName=name, CFBundleTypeExtensions=rest))
    return out
def exported(ext): return {"UTTypeIdentifier": f"app.qcview.{ext}", "UTTypeDescription": "QCView Project", "UTTypeConformsTo": ["public.data"],
                           "UTTypeTagSpecification": {"public.filename-extension": [ext]}}

plist = {
    "CFBundleDevelopmentRegion": "en",
    "CFBundleLocalizations": ["en", "zh-Hans", "ja", "ko", "de", "fr", "es", "pt-BR", "it", "pl", "ru", "uk"],
    "CFBundleExecutable": "qcview", "CFBundleIdentifier": "app.qcview.player", "CFBundleInfoDictionaryVersion": "6.0",
    "CFBundleName": "QCView", "CFBundleDisplayName": "QCView", "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": version, "CFBundleVersion": version,   # 2.x: both X.Y.Z, no build number (Sparkle compares these)
    "CFBundleIconFile": "qcview.icns",
    "LSMinimumSystemVersion": "13.0", "NSHighResolutionCapable": True, "NSSupportsAutomaticGraphicsSwitching": True,
    "LSApplicationCategoryType": "public.app-category.video",
    "NSHumanReadableCopyright": "QCView is free software under the GNU General Public License v3.",
    # Sparkle (2.x's feed and key: an installed 2.x only ever trusts these)
    "SUFeedURL": "https://qcview.app/appcast.xml", "SUPublicEDKey": "+Xlrad3WswxIRTyzBTp8KaTEjyfPO5Ap4llmPdZf3RQ=",
    "SUEnableAutomaticChecks": True, "SUScheduledCheckInterval": 86400, "SUEnableSystemProfiling": False,
    "CFBundleDocumentTypes": [{"CFBundleTypeName": "QCView Project", "CFBundleTypeRole": "Editor", "LSHandlerRank": "Owner",
                               "LSItemContentTypes": ["app.qcview.qcv", "app.qcview.qcvproj"]}]
        + doc_types("Video File", "VIDEO") + doc_types("Audio File", "AUDIO") + doc_types("Image File", "IMAGE"),
    "CFBundleURLTypes": [{"CFBundleURLName": "QCView Project URI", "CFBundleURLSchemes": ["qcview"], "CFBundleTypeRole": "Editor"}],
    "UTExportedTypeDeclarations": [exported("qcv"), exported("qcvproj")],
}
if icon_name: plist["CFBundleIconName"] = icon_name   # macOS 26's layered icon in Assets.car; older systems use the .icns
with open(out, "wb") as f: plistlib.dump(plist, f, sort_keys=False)
print(f"Info.plist: QCView {version}")
