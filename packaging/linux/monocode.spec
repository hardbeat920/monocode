Name: mono-code
Version: @@VERSION@@
Release: 1
Summary: Native MonoCode desktop and remote host
License: MIT
Requires: alsa-lib, vulkan-loader, wayland, libX11, libxkbcommon, libxkbcommon-x11, fontconfig, freetype, gstreamer1, gstreamer1-plugins-base, gstreamer1-plugins-good, gstreamer1-plugin-libav

%description
MonoCode runs coding agents in a native desktop workspace.

%install
mkdir -p %{buildroot}/usr
cp -a "@@ROOT@@/usr/." %{buildroot}/usr/

%files
/usr/bin/monocode
/usr/bin/monocode-host
/usr/share/applications/MonoCode.desktop
/usr/share/icons/hicolor/512x512/apps/monocode.png
