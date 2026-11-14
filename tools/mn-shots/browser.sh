#!/bin/sh
# browser.sh - the manual's pictures of what minNotes EXPORTS, as a browser shows them: the application exports from a
# copy of the demo collection (its scripted hooks; nothing is sent to the desktop), headless Chrome takes the pictures.
#   sh tools/mn-shots/browser.sh        (after `run.sh build`; needs Google Chrome)
set -e
cd "$(dirname "$0")/../.."
D="${MN_DEMO_DIR:-/Users/Shared/minNotes Demo}"; W="$PWD/target/mn-shots"; OUT="${MN_SHOTS_OUT:-$W/out}"; X="$D/Exports"
BIN="${MN_BIN:-target/release/minNotes}"; CH="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
export MN_NO_AUDIO=1 MN_NO_DIALOGS=1 MN_SESSIONS_DIR="$W/sessions"
copy() { rm -rf "$D/Work" "$W/sessions"; mkdir -p "$D/Work"; cp "$D/Tidewater.mnd" "$D/Work/"; [ -d "$D/minNotes Media" ] && cp -R "$D/minNotes Media" "$D/Work/"; true; }
shot() { "$CH" --headless=new --disable-gpu --hide-scrollbars --force-dark-mode --window-size="${3:-1280,800}" --force-device-scale-factor=2 --virtual-time-budget=8000 --run-all-compositor-stages-before-draw --screenshot="$OUT/$1.png" "$2" > /dev/null 2>&1; echo "browser: $1"; }
rm -rf "$X"; mkdir -p "$X" "$OUT"
copy; MN_EXPORT="$X/Delivery specs.html" "$BIN" "$D/Work/Tidewater.mnd" --note "Delivery specs" --seconds 4 > /dev/null 2>&1
copy; MN_EXPORT="$X/Review - Reel 2.html" "$BIN" "$D/Work/Tidewater.mnd" --note "Review - Reel 2" --seconds 5 > /dev/null 2>&1
copy; MN_SITE="$X/site,all" "$BIN" "$D/Work/Tidewater.mnd" --seconds 25 > /dev/null 2>&1
shot mn3-export-html-note "file://$X/Delivery specs.html"
shot mn3-export-html-video "file://$X/Review - Reel 2.html"
shot mn3-web-folder-browser "file://$X/site/index.html"
ls "$X" "$X/site" | head -30
