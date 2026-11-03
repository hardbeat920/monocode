#!/usr/bin/env bash
# Package an already-built native executable; do not build or launch the app.
# Development bundles use separate data; release bundles use installed app data.
set -euo pipefail
cd "$(dirname "$0")/.."
if [ "$(uname -s)" != Darwin ]; then
  echo 'macOS app bundles must be packaged on macOS.' >&2
  exit 1
fi

native_profile="${1:-release}"
native_target="${2:-${CARGO_TARGET_DIR:-crates/monocode-gpui/target}}/$native_profile"
native_version="${3:-$(sed -n 's/^version = "\([^"]*\)".*/\1/p' crates/monocode-gpui/Cargo.toml)}"
if [ "$native_profile" = release ]; then
  native_data_mode="${4:-monocode-data}"
else
  native_data_mode="${4:-dev-data}"
fi
case "$native_data_mode" in
  dev-data|monocode-data) ;;
  *) echo "Unknown data mode: $native_data_mode" >&2; exit 1 ;;
esac
native_bundle="$native_target/Monocode Native.app"
native_binary="$native_target/monocode-gpui"
if [ ! -f "$native_binary" ]; then
  echo "Missing native executable: $native_binary" >&2
  echo 'Build it with cargo first, or use npm run native:build.' >&2
  exit 1
fi

mkdir -p "$native_bundle/Contents/MacOS" "$native_bundle/Contents/Resources"
# Replace the inode atomically so macOS does not reuse an older signature cache.
cp "$native_binary" "$native_bundle/Contents/MacOS/monocode-gpui.next"
mv -f "$native_bundle/Contents/MacOS/monocode-gpui.next" "$native_bundle/Contents/MacOS/monocode-gpui"
cp src-tauri/icons/icon.icns "$native_bundle/Contents/Resources/monocode.icns"
# macOS 26 draws the Liquid Glass icon from Assets.car; the .icns covers older releases.
cp src-tauri/macos/Assets.car "$native_bundle/Contents/Resources/Assets.car"
# Ship the app's attribution and the license texts for GPUI and embedded assets.
# Add these before signing so the signature covers the notices too.
native_licenses="$native_bundle/Contents/Resources/licenses"
mkdir -p "$native_licenses"
cp LICENSE NOTICE docs/native/LICENSE-GPUI-APACHE docs/native/LICENSE-MERMAID-MIT "$native_licenses/"
cp crates/monocode-gpui/assets/hugeicons.LICENSE \
  crates/monocode-gpui/assets/dark-plus.LICENSE \
  crates/monocode-gpui/assets/native-editor.LICENSE.txt "$native_licenses/"
cp crates/monocode-gpui/assets/material/LICENSE "$native_licenses/material-icons.LICENSE"
rm -f "$native_licenses/Geist-OFL.txt"
# Launch the binary itself: an exec'ing launcher changes the pid version, and macOS
# then refuses the menu bar item. The app reads MonocodeDataMode from Info.plist.
native_executable="monocode-gpui"
native_short_version="${native_version%%-*}"
native_build_version="$native_short_version"
if [[ "$native_version" == *-preview.* ]]; then
  native_build_version="${native_version##*-preview.}"
fi
rm -f "$native_bundle/Contents/MacOS/monocode-launcher"
cat > "$native_bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>$native_executable</string>
<key>MonocodeDataMode</key><string>$native_data_mode</string>
<key>CFBundleIdentifier</key><string>dev.usemono.gpui</string>
<key>CFBundleName</key><string>Monocode Native</string>
<key>CFBundleDisplayName</key><string>Monocode Native</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$native_short_version</string>
<key>CFBundleVersion</key><string>$native_build_version</string>
<key>MonocodeNativeVersion</key><string>$native_version</string>
<key>CFBundleIconFile</key><string>monocode.icns</string>
<key>CFBundleIconName</key><string>AppIcon</string>
<key>NSPrincipalClass</key><string>NSApplication</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
native_identity="${MONOCODE_NATIVE_SIGNING_IDENTITY:--}"
if [ "$native_identity" = - ]; then
  codesign --force --sign - "$native_bundle"
else
  codesign --force --sign "$native_identity" --options runtime --timestamp \
    --entitlements src-tauri/Entitlements.plist "$native_bundle"
fi
printf 'App: %s\n' "$native_bundle"
