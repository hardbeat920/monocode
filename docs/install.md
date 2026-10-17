# Linux installation

Download the x86_64 `.deb`, `.rpm`, or AppImage from [GitHub Releases](https://github.com/hardbeat920/monocode/releases/latest). Install and sign in to at least one [supported provider](providers.md) before starting a session.

## Ubuntu / Debian packages

Install the downloaded `.deb` with apt so its runtime dependencies are installed too:

```bash
sudo apt install ./MonoCode_*.deb
```

## Fedora / Enterprise Linux packages

On Fedora, or on an Enterprise Linux 10 system (registered RHEL, Rocky, Alma, CentOS Stream, Oracle), install the release `.rpm`. The `.rpm` declares its own runtime dependencies, so `dnf` pulls the WebKitGTK stack for you.

Fedora needs no extra repository step:

```bash
sudo dnf install ./MonoCode-*.rpm
```

Enterprise Linux 10 needs EPEL first, because `webkit2gtk4.1` is an EPEL package there. CRB is not needed to run MonoCode.

On Rocky, Alma, or CentOS Stream 10:

```bash
sudo dnf install -y epel-release
sudo dnf install ./MonoCode-*.rpm
```

On registered RHEL 10:

```bash
sudo dnf install -y https://dl.fedoraproject.org/pub/epel/epel-release-latest-10.noarch.rpm
sudo dnf install ./MonoCode-*.rpm
```

On Oracle Linux 10, `epel-release` does not enable `ol10_developer_EPEL`, which is the repository that provides WebKitGTK. Enable it before installing the rpm:

```bash
sudo dnf install -y oracle-epel-release-el10 dnf-plugins-core
sudo dnf config-manager --set-enabled ol10_developer_EPEL
sudo dnf install ./MonoCode-*.rpm
```

GitHub Releases builds the `.rpm` on Enterprise Linux 10 so it loads on Fedora and EL 10. EL 9 and older are unsupported (`webkit2gtk4.1-devel` only exists in EPEL 10).

For native build dependencies and packaging commands, see [building from source](building.md#fedora--enterprise-linux-packages). Building on EL 10 also needs CRB; the repository’s setup helper enables it and EPEL automatically.

## AppImage

The AppImage needs WebKitGTK 4.1 on the host, the same as the `.deb` and `.rpm`:

| Distribution | Host package |
| --- | --- |
| Debian / Ubuntu | `libwebkit2gtk-4.1-0` |
| Fedora / Enterprise Linux 10 | `webkit2gtk4.1` |
| Arch | `webkit2gtk-4.1` |

On Enterprise Linux 10, enable EPEL as described above before installing the host package. On Fedora, install WebKit if the launcher asks for it:

```bash
sudo dnf install webkit2gtk4.1
```

Keep the AppImage somewhere you can write to, such as `~/Applications`, so updates can replace it. In that directory, make it executable and run it:

```bash
chmod +x MonoCode_*.AppImage
./MonoCode_*.AppImage
```

## Updates

The AppImage updates itself from **Settings → General**. The `.deb` and `.rpm` update through apt or dnf: download the newer package from [GitHub Releases](https://github.com/hardbeat920/monocode/releases/latest) and install it with the same command used above.

For beta release feeds and testing AppImage updates, see [release and updater details](building.md#beta-releases-and-updates).

## Troubleshooting on Fedora / Wayland

The AppImage uses the host WebKitGTK 4.1 stack and native Wayland, like the `.deb` and `.rpm`. Set `GDK_BACKEND=x11` to keep the previous X11-forced behavior:

```bash
GDK_BACKEND=x11 ./MonoCode_*.AppImage
```

Older AppImages that bundled Ubuntu-built libraries aborted with `Could not create default EGL display: EGL_BAD_PARAMETER`; current builds do not.

## Troubleshooting on NVIDIA / Wayland

On an NVIDIA GPU under a Wayland session, MonoCode can fail in one of two ways:

- No window appears, and the terminal prints `Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.`
- A window appears but stays almost entirely white, with only a strip of the UI drawn.

Both appear to come from WebKitGTK's DMA-BUF renderer allocating its buffers through NVIDIA's GBM backend, which rejects them. Keep the renderer but make it use shared-memory buffers instead:

```bash
WEBKIT_DMABUF_RENDERER_FORCE_SHM=1 ./MonoCode_*.AppImage
```

The `.deb` and `.rpm` use the same host WebKitGTK, so the same variable should apply to them.

Two commonly suggested workarounds do not fix this. `WEBKIT_DISABLE_DMABUF_RENDERER=1` gets past the crash but produces the mostly white window. `GDK_BACKEND=x11` gives a fully white window and logs `Failed to create GBM buffer of size ...: Invalid argument`.

To make the fix permanent for every WebKitGTK app, add it to your session environment and log back in:

```bash
mkdir -p ~/.config/environment.d
echo 'WEBKIT_DMABUF_RENDERER_FORCE_SHM=1' > ~/.config/environment.d/webkit.conf
```

Or prepend `env WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` to the `Exec=` line of the MonoCode `.desktop` entry. Tools like AppImageLauncher regenerate that entry when the AppImage updates itself, so the session-wide setting is more durable.

Verified on Arch Linux with KDE Plasma 6.7 on Wayland, NVIDIA driver 615.78, WebKitGTK 2.54, and a GeForce RTX 4070 SUPER, running MonoCode 0.13.0.

Return to the [README](../README.md) for macOS and Windows downloads.
