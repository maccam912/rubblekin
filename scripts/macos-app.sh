#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --locked -p rubblekin_client
app="artifacts/Rubblekin.app"
mkdir -p "$app/Contents/MacOS"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Rubblekin</string>
<key>CFBundleIdentifier</key><string>local.rubblekin.prototype</string>
<key>CFBundleName</key><string>Rubblekin</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>0.1.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
cat > "$app/Contents/MacOS/Rubblekin" <<'LAUNCHER'
#!/bin/sh
cd "$(dirname "$0")/../../../.."
exec ./target/debug/rubblekin "$@"
LAUNCHER
chmod +x "$app/Contents/MacOS/Rubblekin"
printf 'Created %s/%s\n' "$PWD" "$app"
