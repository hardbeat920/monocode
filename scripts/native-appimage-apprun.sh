#!/bin/sh
set -eu

if [ "${1-}" != "--install-desktop" ]; then
    exec "$APPDIR/usr/lib/monocode-native/monocode-gpui" "$@"
fi

if [ "$#" -ne 1 ]; then
    printf '%s\n' 'Usage: AppImage --install-desktop' >&2
    exit 1
fi
case "${APPIMAGE-}" in
    /*) ;;
    *) printf '%s\n' 'Desktop registration requires an AppImage launched from its permanent location.' >&2; exit 1 ;;
esac
if [ ! -f "$APPIMAGE" ] || [ ! -x "$APPIMAGE" ]; then
    printf '%s\n' 'The AppImage must exist and be executable.' >&2
    exit 1
fi
if [ "$(printf '%s' "$APPIMAGE" | LC_ALL=C tr -d '[:cntrl:]=')" != "$APPIMAGE" ]; then
    printf '%s\n' 'Move the AppImage to a path without control characters or an equals sign before registering it.' >&2
    exit 1
fi

data_home=${XDG_DATA_HOME:-"${HOME:?HOME must be set}/.local/share"}
case "$data_home" in
    /*) ;;
    *) printf '%s\n' 'The desktop data directory must be an absolute path.' >&2; exit 1 ;;
esac
applications="$data_home/applications"
icons="$data_home/icons/hicolor/256x256/apps"
# Desktop values are unescaped before Exec quoting and percent field expansion.
exec_path=$(printf '%s' "$APPIMAGE" | sed 's/\\/\\\\\\\\/g; s/["`$]/\\\\&/g; s/%/%%/g')

mkdir -p "$applications" "$icons"
install -m 644 "$APPDIR/monocode-native.png" "$icons/monocode-native.png"
desktop=$(mktemp "$applications/.dev.usemono.gpui.desktop.XXXXXX")
trap 'rm -f "$desktop"' 0
sed '/^Exec=/d' "$APPDIR/dev.usemono.gpui.desktop" > "$desktop"
# GLib validates the executable before expanding %% in AppImage paths.
printf 'Exec=env "%s" %%F\n' "$exec_path" >> "$desktop"
chmod 644 "$desktop"
mv -f "$desktop" "$applications/dev.usemono.gpui.desktop"
# Icon caches only notice new icons when the theme directory changes.
touch "$data_home/icons/hicolor"
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -t "$data_home/icons/hicolor" || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$applications" || true
fi
printf 'Desktop entry installed: %s\n' "$applications/dev.usemono.gpui.desktop"
