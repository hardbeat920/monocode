#!/usr/bin/env bash
set -euo pipefail

app_id="com.monocode.desktop"
current_user="${USER:-$(id -un)}"
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source_binary="${MONOCODE_BINARY:-$repo_root/target/release/monocode}"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
bin_dir="${MONOCODE_BIN_DIR:-$HOME/.local/bin}"
installed_binary="$bin_dir/monocode"
applications_dir="$data_home/applications"
desktop_file="$applications_dir/$app_id.desktop"
icons_dir="$data_home/icons/hicolor"

# Print the supported command and environment overrides.
usage() {
  cat <<'EOF'
Usage: scripts/install-linux-desktop.sh [--uninstall]

Installs a release build and desktop launcher for the current user.

Environment overrides:
  MONOCODE_BINARY  Release binary to install (default: target/release/monocode)
  MONOCODE_BIN_DIR Directory for the installed binary (default: ~/.local/bin)
  XDG_DATA_HOME    Base directory for desktop data (default: ~/.local/share)
EOF
}

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "This installer only supports Linux." >&2
  exit 1
fi

# Ask installed desktop tools to notice launcher and icon changes immediately.
refresh_desktop_caches() {
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$applications_dir" >/dev/null 2>&1 || true
  fi
  if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t "$icons_dir" >/dev/null 2>&1 || true
  fi
}

# Remove only the files managed by this user-scoped installer.
uninstall() {
  rm -f -- \
    "$installed_binary" \
    "$desktop_file" \
    "$icons_dir/32x32/apps/$app_id.png" \
    "$icons_dir/128x128/apps/$app_id.png" \
    "$icons_dir/256x256/apps/$app_id.png"
  refresh_desktop_caches
  printf 'Removed the MonoCode desktop installation for %s.\n' "$current_user"
}

case "${1:-}" in
  "") ;;
  --uninstall)
    uninstall
    exit 0
    ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac

if [[ ! -x "$source_binary" ]]; then
  cat >&2 <<EOF
MonoCode release binary not found at:
  $source_binary

Build it first with:
  npm run build
  cargo build --release
EOF
  exit 1
fi

# Exec treats %, ", backticks, dollar signs, and backslashes specially inside
# a quoted argument. Escape them so paths such as a home directory with spaces
# remain a single executable name in freedesktop launchers.
desktop_exec="$(printf '%s' "$installed_binary" | sed \
  -e 's/\\/\\\\\\\\/g' \
  -e 's/"/\\"/g' \
  -e 's/`/\\`/g' \
  -e 's/\$/\\\\$/g' \
  -e 's/%/%%/g')"
desktop_try_exec="$(printf '%s' "$installed_binary" | sed -e 's/\\/\\\\/g')"

desktop_tmp="$(mktemp --suffix=.desktop)"
binary_tmp="$bin_dir/.monocode.install.$$"
trap 'rm -f -- "$desktop_tmp" "$binary_tmp"' EXIT
cat >"$desktop_tmp" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=MonoCode
GenericName=AI coding workspace
Comment=Run coding agents from one desktop workspace
Exec="$desktop_exec"
TryExec=$desktop_try_exec
Icon=$app_id
Terminal=false
Categories=Development;IDE;
Keywords=AI;Agent;Coding;Development;IDE;
StartupNotify=true
StartupWMClass=monocode
EOF

if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate "$desktop_tmp"
fi

mkdir -p -- "$bin_dir" "$applications_dir"
install -m 755 -- "$source_binary" "$binary_tmp"
mv -f -- "$binary_tmp" "$installed_binary"

for size in 32 128 256; do
  icon_dir="$icons_dir/${size}x${size}/apps"
  mkdir -p -- "$icon_dir"
  case "$size" in
    32) icon_source="$repo_root/src-tauri/icons/32x32.png" ;;
    128) icon_source="$repo_root/src-tauri/icons/128x128.png" ;;
    256) icon_source="$repo_root/src-tauri/icons/128x128@2x.png" ;;
  esac
  install -m 644 -- "$icon_source" "$icon_dir/$app_id.png"
done

install -m 644 -- "$desktop_tmp" "$desktop_file"
refresh_desktop_caches

cat <<EOF
Installed MonoCode for $current_user:
  Application: $installed_binary
  Launcher:    $desktop_file

If your desktop menu was already open, close and reopen it before searching
for MonoCode.
EOF
