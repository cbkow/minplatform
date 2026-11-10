#!/usr/bin/env python3
"""THIRD-PARTY-NOTICES for one platform's package (canon row 252). Generated, never hand-edited:
  - the Rust crates linked into the app: cargo-about over the lock file (about.toml = the accepted licences; a crate with
    any other FAILS), for the platform's target;
  - the native libraries and assets: components.json (name, licence, the licence TEXT from licenses/, the staged files each
    entry accounts for);
  - FFmpeg's linked-in libraries: its own --enable flags (the Mac's from the built ffmpeg, Windows' from
    ffmpeg-win64.enabled, captured on the box) through components.json's table — an unknown flag FAILS.
Checks: with --stage, every shipped binary must be named by an entry (macOS). For Windows the file patterns and the
Cargo.lock hash and the pinned vcpkg ports go into a sidecar (.check) that make-msix.ps1 verifies against what it staged — no Python on that box.

  python3 packaging/notices/make_notices.py macos   --out <file> [--stage <qcview.app>]
  python3 packaging/notices/make_notices.py windows --out <file>          (also writes <file>.check)
"""
import fnmatch, html, json, os, re, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
SOURCE_REPO = "https://github.com/cbkow/minplatform"; RELEASES = "https://github.com/cbkow/QCView-Player/releases"
TRIPLE = {"macos": "aarch64-apple-darwin", "windows": "x86_64-pc-windows-msvc"}

def die(msg): print("notices: " + msg, file=sys.stderr); sys.exit(1)
def read(*p): return open(os.path.join(*p), encoding="utf-8", errors="replace").read()
def pin(spec):
    if not spec or not spec.startswith("pin:"): return spec or ""
    _, f, key = spec.split(":"); m = re.search(rf"^{key}=(.+)$", read(ROOT, f), re.M)
    if not m: die(f"no {key} in {f}")
    return m.group(1).strip().lstrip("v")

def main():
    if len(sys.argv) < 4 or sys.argv[1] not in TRIPLE or "--out" not in sys.argv: die(__doc__)
    plat = sys.argv[1]; out = sys.argv[sys.argv.index("--out") + 1]
    stage = sys.argv[sys.argv.index("--stage") + 1] if "--stage" in sys.argv else None
    cfg = json.loads(read(HERE, "components.json"))
    version = re.search(r'^version\s*=\s*"([^"]+)"', read(ROOT, "apps/qcv/Cargo.toml"), re.M).group(1)
    comps = [c for c in cfg["components"] if plat in c["platforms"]]
    # Windows: the versions are vcpkg's (tools/deps/vcpkg-windows.pin), which the box's packaging checks against what is installed
    ports = dict(l.strip().split("=", 1) for l in read(ROOT, "tools/deps/vcpkg-windows.pin").splitlines() if "=" in l and not l.startswith(("#", "BASELINE")))
    if plat == "windows":
        claimed = {c["vcpkg"] for c in comps if c.get("vcpkg")}
        if claimed != set(ports): die("components.json and tools/deps/vcpkg-windows.pin name different vcpkg ports: " + ", ".join(sorted(claimed ^ set(ports))))

    # FFmpeg's linked-in libraries
    if plat == "macos":
        conf = subprocess.run([os.path.join(ROOT, "external/ffmpeg/macos-arm64/bin/ffmpeg"), "-hide_banner", "-buildconf"], capture_output=True, text=True).stdout
        flags = sorted(set(re.findall(r"--enable-([a-z0-9_-]+)", conf)))
        if not flags: die("could not read the built FFmpeg's configuration (tools/ffmpeg/build-macos.sh)")
    else:
        flags = read(HERE, "ffmpeg-win64.enabled").split()
    table = cfg["ffmpeg_libraries"]; unknown = [f for f in flags if f not in table]
    if unknown: die("FFmpeg was built with libraries the notices do not know: " + ", ".join(unknown) + " — add them to components.json")
    ff_libs = [table[f] for f in flags if table[f]]

    # the crates
    tmp = out + ".crates.tmp"
    r = subprocess.run(["cargo", "about", "generate", "--fail", "-c", os.path.join(HERE, "about.toml"), "--manifest-path", os.path.join(ROOT, "apps/qcv/Cargo.toml"),
                        "--target", TRIPLE[plat], os.path.join(HERE, "about.hbs"), "-o", tmp], capture_output=True, text=True)
    if r.returncode != 0 or not os.path.exists(tmp): die("cargo-about refused (a crate's licence is not in about.toml, or cargo-about is not installed: cargo install cargo-about --locked --features cli)\n" + r.stderr[-1500:])
    crates = read(tmp); os.remove(tmp)
    n_crates = len({t.strip() for grp in re.findall(r"</b> — ([^<]*)</summary>", crates) for t in grp.split(",") if t.strip()})

    # the staged-file check (macOS here; Windows by make-msix.ps1 from the sidecar)
    patterns = sorted({p for c in comps for p in c.get("files", [])} | set(cfg["own"]))
    if stage:
        shipped = []
        for sub in ("Contents/MacOS", "Contents/Helpers", "Contents/Frameworks"):
            d = os.path.join(stage, sub)
            if os.path.isdir(d): shipped += os.listdir(d)
        orphans = [f for f in shipped if not any(fnmatch.fnmatch(f, p) for p in patterns)]
        if orphans: die("shipped without a notices entry: " + ", ".join(sorted(orphans)) + " — add them to components.json")

    e = html.escape; parts = []
    for c in comps:
        v = ports[c["vcpkg"]].split("#")[0] if plat == "windows" and c.get("vcpkg") else pin(c.get("version")); texts = "".join(f"<pre>{e(read(HERE, 'licenses', t))}</pre>" for t in c.get("text", []))
        extra = ""
        if c.get("ffmpeg"):
            extra = "<p>Libraries linked into this build of FFmpeg, each under its own licence:</p><ul>" + "".join(f'<li>{e(n)} — {e(l)} — <a href="{e(u)}">{e(u)}</a></li>' for n, l, u in ff_libs) + "</ul>"
        parts.append(f'<details class="lic"><summary><b>{e(c["name"])}</b>{" " + e(v) if v else ""} — {e(c["license"])}</summary>'
                     f'<p><a href="{e(c["url"])}">{e(c["url"])}</a></p>{"<p>" + e(c["note"]) + "</p>" if c.get("note") else ""}{extra}{texts}</details>')
    # the lock file the crates were read from, as git names it (the same id on every machine, whatever the line endings)
    lock = subprocess.run(["git", "-C", ROOT, "hash-object", "Cargo.lock"], capture_output=True, text=True).stdout.strip()
    if plat == "windows" and lock != subprocess.run(["git", "-C", ROOT, "rev-parse", "HEAD:Cargo.lock"], capture_output=True, text=True).stdout.strip():
        die("Cargo.lock has uncommitted changes — commit it, then generate (the Windows box checks the notices against its committed lock)")
    os_name = {"macos": "macOS", "windows": "Windows"}[plat]
    page = f"""<!doctype html>
<!-- GENERATED by packaging/notices/make_notices.py — do not edit. QCView {version}, {os_name}; Cargo.lock {lock} -->
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>QCView {e(version)} — Third-Party Notices</title>
<style>
:root {{ color-scheme: light dark; }}
body {{ font: 15px/1.5 -apple-system, "Segoe UI", system-ui, sans-serif; max-width: 60rem; margin: 2rem auto; padding: 0 1rem; }}
h1 {{ font-size: 1.5rem; }} h2 {{ font-size: 1.15rem; margin-top: 2rem; }}
details.lic {{ border-top: 1px solid rgba(128,128,128,.35); padding: .45rem 0; }}
summary {{ cursor: pointer; }} pre {{ white-space: pre-wrap; font: 12px/1.45 ui-monospace, Consolas, monospace; background: rgba(128,128,128,.12); padding: .75rem; border-radius: 6px; overflow-x: auto; }}
</style></head><body>
<h1>QCView {e(version)} — Third-Party Notices ({os_name})</h1>
<p>QCView is free software, licensed under the <b>GNU General Public License, version 3</b>. The licence's text ships with the app as <code>LICENSE</code>. QCView comes with no warranty, to the extent permitted by law.</p>
<p><b>Source code.</b> The complete corresponding source of this release — QCView itself, its build and packaging scripts, the patches applied to FFmpeg and the versions of every library below — is the tag <code>v{e(version)}</code> of <a href="{SOURCE_REPO}">{SOURCE_REPO}</a>. A source archive of that tag is attached to each release at <a href="{RELEASES}">{RELEASES}</a>.</p>
<p>QCView includes the third-party software below. Each entry opens to its licence.</p>
<h2>Libraries and assets shipped with QCView</h2>
{''.join(parts)}
<h2>Rust crates compiled into QCView ({n_crates})</h2>
<p>Grouped by licence text; each group lists the crates it covers.</p>
{crates}
</body></html>
"""
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    open(out, "w", encoding="utf-8", newline="\n").write(page)
    if plat == "windows":
        open(out + ".check", "w", encoding="ascii", newline="\n").write("lock " + lock + "\nversion " + version + "\nffmpeg " + " ".join(flags) + "\n" + "".join(f"port {k} {v}\n" for k, v in sorted(ports.items())) + "".join("file " + p + "\n" for p in patterns))
    print(f"notices: {os_name}, {len(comps)} components, {len(ff_libs)} FFmpeg libraries, {n_crates} crates → {out} ({os.path.getsize(out) // 1024} KB)")

main()
