#!/bin/sh
# S9 step 1: build the reference window as a small app bundle (a writing tool keys on the bundle id; an unbundled binary may
# be ignored) into target/spikes/s9/, signed ad hoc. Run from the repo root:  sh tools/s9/build.sh
#   open it:   S9_LOG="$PWD/docs/spikes/s9/out/run.log" target/spikes/s9/S9Text.app/Contents/MacOS/S9Text
set -e
cd "$(dirname "$0")/../.."
APP=target/spikes/s9/S9Text.app; C="$APP/Contents"
rm -rf "$APP"; mkdir -p "$C/MacOS" docs/spikes/s9/out
swiftc -O -swift-version 5 -o "$C/MacOS/S9Text" tools/s9/main.swift -framework Cocoa
cat > "$C/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>app.minplatform.spike.s9text</string>
  <key>CFBundleName</key><string>S9Text</string>
  <key>CFBundleExecutable</key><string>S9Text</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
EOF
codesign --force --sign - "$APP" 2>/dev/null
echo "built $APP"
