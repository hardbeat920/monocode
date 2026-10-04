//! Packages prebuilt Rust executables. No Node, Tauri, or web build step.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use flate2::{Compression, write::GzEncoder};
use object::Object;
use sha2::{Digest, Sha256};

fn main() {
    if let Err(error) = run(std::env::args().skip(1).collect()) {
        eprintln!("monocode-package: {error:#}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<()> {
    let Some((command, args)) = args.split_first() else {
        bail!("usage: monocode-package <bundle|checksums|manifest> [options]");
    };
    let options = Options::parse(args)?;
    match command.as_str() {
        "bundle" => bundle(&options),
        "checksums" => checksums(&options),
        "manifest" => manifest(&options),
        _ => bail!("unknown command {command}"),
    }
}

struct Options(BTreeMap<String, String>);
impl Options {
    fn parse(args: &[String]) -> Result<Self> {
        let mut out = BTreeMap::new();
        for pair in args.chunks(2) {
            if pair.len() != 2 || !pair[0].starts_with("--") {
                bail!("options use --name value");
            }
            if out
                .insert(pair[0][2..].to_owned(), pair[1].clone())
                .is_some()
            {
                bail!("duplicate option {}", pair[0]);
            }
        }
        Ok(Self(out))
    }
    fn get<'a>(&'a self, name: &str, default: &'a str) -> &'a str {
        self.0.get(name).map(String::as_str).unwrap_or(default)
    }
    fn required(&self, name: &str) -> Result<&str> {
        self.0
            .get(name)
            .map(String::as_str)
            .with_context(|| format!("--{name} is required"))
    }
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn version(options: &Options) -> Result<String> {
    let version = options.get("version", env!("CARGO_PKG_VERSION"));
    if version.is_empty()
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
    {
        bail!("version contains invalid characters");
    }
    Ok(version.to_owned())
}

fn command(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("starting {:?}", command.get_program()))?;
    if !status.success() {
        bail!("{:?} exited with {status}", command.get_program());
    }
    Ok(())
}

fn copy(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> Result<()> {
    let destination = destination.as_ref();
    fs::create_dir_all(destination.parent().context("file has no parent")?)?;
    fs::copy(source.as_ref(), destination)
        .with_context(|| format!("copying {}", source.as_ref().display()))?;
    Ok(())
}

fn archive(directory: &Path, name: &str, output: &Path) -> Result<()> {
    let gzip = GzEncoder::new(fs::File::create(output)?, Compression::default());
    let mut tar = tar::Builder::new(gzip);
    if directory.is_dir() {
        tar.append_dir_all(name, directory)?;
    } else {
        tar.append_path_with_name(directory, name)?;
    }
    tar.into_inner()?.finish()?;
    Ok(())
}

fn check_windows_runtime(path: &Path) -> Result<()> {
    check_windows_runtime_bytes(&fs::read(path)?)
        .with_context(|| format!("checking portable Windows executable {}", path.display()))
}

fn check_package_target(path: &Path, target: &str) -> Result<()> {
    check_package_target_bytes(&fs::read(path)?, target)
        .with_context(|| format!("checking package target for {}", path.display()))
}

fn check_package_target_bytes(bytes: &[u8], target: &str) -> Result<()> {
    use object::{Architecture, BinaryFormat};

    let expected = match target {
        "aarch64-apple-darwin" => (BinaryFormat::MachO, Architecture::Aarch64),
        "x86_64-apple-darwin" => (BinaryFormat::MachO, Architecture::X86_64),
        "aarch64-unknown-linux-gnu" => (BinaryFormat::Elf, Architecture::Aarch64),
        "x86_64-unknown-linux-gnu" => (BinaryFormat::Elf, Architecture::X86_64),
        "aarch64-pc-windows-msvc" => (BinaryFormat::Pe, Architecture::Aarch64),
        "x86_64-pc-windows-msvc" => (BinaryFormat::Pe, Architecture::X86_64),
        _ => bail!("unsupported package target {target}"),
    };
    let actual = match object::FileKind::parse(bytes)? {
        // Read only the Mach-O header. object's full parser rejects some load
        // commands that the macOS 26 linker writes, and the CPU type is all
        // this check needs.
        object::FileKind::MachO64 => {
            use object::read::macho::MachHeader;
            let header = object::macho::MachHeader64::<object::Endianness>::parse(bytes, 0)?;
            let architecture = match header.cputype(header.endian()?) {
                object::macho::CPU_TYPE_ARM64 => Architecture::Aarch64,
                object::macho::CPU_TYPE_X86_64 => Architecture::X86_64,
                _ => Architecture::Unknown,
            };
            (BinaryFormat::MachO, architecture)
        }
        _ => {
            let executable = object::File::parse(bytes)?;
            (executable.format(), executable.architecture())
        }
    };
    if actual != expected {
        bail!(
            "the executable is {:?}/{:?}, but package target {target} requires {:?}/{:?}",
            actual.0,
            actual.1,
            expected.0,
            expected.1,
        );
    }
    Ok(())
}

fn check_windows_runtime_bytes(bytes: &[u8]) -> Result<()> {
    let executable = object::File::parse(bytes)?;
    if executable.format() != object::BinaryFormat::Pe {
        bail!("the Windows executable is not a PE image");
    }
    for import in executable.imports()? {
        let library = String::from_utf8_lossy(import.library()).to_ascii_lowercase();
        if library.starts_with("vcruntime")
            || (library.starts_with("msvcp") && library != "msvcp_win.dll")
            || library.starts_with("concrt")
            || (library.starts_with("msvcr") && library != "msvcrt.dll")
        {
            bail!(
                "the executable imports redistributable runtime {library}. Build for the matching Windows target with '-C target-feature=+crt-static'"
            );
        }
    }
    Ok(())
}

fn bundle(options: &Options) -> Result<()> {
    let target = options.required("target")?;
    let version = version(options)?;
    let profile = options.get("profile", "release");
    let output = PathBuf::from(options.get("output", "build/native"));
    let default_binaries = workspace().join("target").join(target).join(profile);
    let binaries = options
        .0
        .get("binaries")
        .map(PathBuf::from)
        .unwrap_or(default_binaries);
    let app = binaries.join(if target.contains("windows") {
        "monocode-app.exe"
    } else {
        "monocode-app"
    });
    let host = binaries.join(if target.contains("windows") {
        "monocode-host.exe"
    } else {
        "monocode-host"
    });
    if !host.is_file() {
        bail!(
            "missing {}. Build monocode-host for {target} first",
            host.display()
        );
    }
    check_package_target(&host, target)?;
    if target.contains("windows") {
        check_windows_runtime(&host)?;
    }
    if target.contains("linux") {
        let dependencies = Command::new("readelf").args(["-d"]).arg(&host).output()?;
        if !dependencies.status.success() {
            bail!("readelf could not inspect the portable Linux host");
        }
        if String::from_utf8_lossy(&dependencies.stdout).contains("libicu") {
            bail!(
                "the portable host links shared ICU. Build with static ICU libraries, as on the Ubuntu release runner"
            );
        }
    }
    let reported = Command::new(&host)
        .arg("--version")
        .output()
        .with_context(|| format!("checking {} on the packaging runner", host.display()))?;
    if !reported.status.success() || String::from_utf8_lossy(&reported.stdout).trim() != version {
        bail!("the prebuilt host does not report package version {version}");
    }
    let host_only = options.get("formats", "all") == "host";
    if !host_only {
        if !app.is_file() {
            bail!(
                "missing {}. Build monocode-app for {target} first",
                app.display()
            );
        }
        check_package_target(&app, target)?;
        if target.contains("windows") {
            check_windows_runtime(&app)?;
        }
    }
    fs::create_dir_all(&output)?;
    let stage = tempfile::tempdir()?;
    let host_name = host
        .file_name()
        .context("host has no file name")?
        .to_str()
        .context("host name is not UTF-8")?;
    archive(
        &host,
        host_name,
        &output.join(format!("monocode-host_{version}_{target}.tar.gz")),
    )?;
    if host_only {
        return Ok(());
    }
    if target.contains("apple-darwin") {
        macos(
            &app,
            &host,
            &version,
            target,
            stage.path(),
            &output,
            options,
        )?;
    } else if target.contains("linux") {
        linux(
            &app,
            &host,
            &version,
            target,
            stage.path(),
            &output,
            options,
        )?;
    } else if target.contains("windows") {
        windows(&app, &host, &version, target, stage.path(), &output)?;
    } else {
        bail!("unsupported target {target}");
    }
    Ok(())
}

fn macos(
    app: &Path,
    host: &Path,
    version: &str,
    target: &str,
    stage: &Path,
    output: &Path,
    options: &Options,
) -> Result<()> {
    let bundle = stage.join("MonoCode.app");
    copy(app, bundle.join("Contents/MacOS/monocode-app"))?;
    copy(host, bundle.join("Contents/MacOS/monocode-host"))?;
    for (source, destination) in [
        ("assets/icon.icns", "icon.icns"),
        ("macos/Assets.car", "Assets.car"),
    ] {
        copy(
            workspace().join("packaging").join(source),
            bundle.join("Contents/Resources").join(destination),
        )?;
    }
    let plist = include_str!("../../../packaging/macos/Info.plist").replace("@@VERSION@@", version);
    fs::write(bundle.join("Contents/Info.plist"), plist)?;
    let identity = options.get("signing-identity", "-");
    command(
        Command::new("codesign")
            .args(["--force", "--options", "runtime", "--timestamp"])
            .arg("--sign")
            .arg(identity)
            .arg(bundle.join("Contents/MacOS/monocode-host")),
    )?;
    let mut sign = Command::new("codesign");
    sign.args([
        "--force",
        "--options",
        "runtime",
        "--timestamp",
        "--entitlements",
    ])
    .arg(workspace().join("packaging/macos/Entitlements.plist"))
    .arg("--sign")
    .arg(identity)
    .arg(&bundle);
    command(&mut sign)?;
    if let Some(profile) = options.0.get("notary-profile") {
        let notary_zip = stage.join("notarize.zip");
        command(
            Command::new("ditto")
                .args(["-c", "-k", "--keepParent"])
                .arg(&bundle)
                .arg(&notary_zip),
        )?;
        command(
            Command::new("xcrun")
                .args(["notarytool", "submit"])
                .arg(notary_zip)
                .args(["--keychain-profile", profile, "--wait"]),
        )?;
        command(
            Command::new("xcrun")
                .args(["stapler", "staple"])
                .arg(&bundle),
        )?;
    }
    command(
        Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&bundle),
    )?;
    archive(
        &bundle,
        "MonoCode.app",
        &output.join(format!("MonoCode_{version}_{target}.app.tar.gz")),
    )?;
    if options.get("formats", "all") != "archive" {
        let image_root = stage.join("dmg");
        fs::create_dir(&image_root)?;
        command(
            Command::new("ditto")
                .arg(&bundle)
                .arg(image_root.join("MonoCode.app")),
        )?;
        #[cfg(unix)]
        std::os::unix::fs::symlink("/Applications", image_root.join("Applications"))?;
        command(
            Command::new("hdiutil")
                .args([
                    "create",
                    "-volname",
                    "MonoCode",
                    "-format",
                    "UDZO",
                    "-srcfolder",
                ])
                .arg(image_root)
                .arg(output.join(format!("MonoCode_{version}_{target}.dmg"))),
        )?;
    }
    // Keep an inspectable app beside the distributable packages.
    command(
        Command::new("ditto")
            .arg(bundle)
            .arg(output.join("MonoCode.app")),
    )
}

fn linux(
    app: &Path,
    host: &Path,
    version: &str,
    target: &str,
    stage: &Path,
    output: &Path,
    options: &Options,
) -> Result<()> {
    let root = stage.join("root");
    copy(app, root.join("usr/bin/monocode"))?;
    copy(host, root.join("usr/bin/monocode-host"))?;
    copy(
        workspace().join("packaging/linux/MonoCode.desktop"),
        root.join("usr/share/applications/MonoCode.desktop"),
    )?;
    copy(
        workspace().join("packaging/assets/icon.png"),
        root.join("usr/share/icons/hicolor/512x512/apps/monocode.png"),
    )?;
    let formats = options.get("formats", "all");
    let deb_arch = if target.starts_with("aarch64") {
        "arm64"
    } else {
        "amd64"
    };
    if formats == "all" || formats.split(',').any(|format| format == "deb") {
        fs::create_dir(root.join("DEBIAN"))?;
        fs::write(
            root.join("DEBIAN/control"),
            format!(
                "Package: mono-code\nVersion: {version}\nArchitecture: {deb_arch}\nMaintainer: MonoCode contributors\nSection: devel\nPriority: optional\nDepends: libasound2 | libasound2t64, libvulkan1, libwayland-client0, libx11-6, libxkbcommon0, libxkbcommon-x11-0, libfontconfig1, libfreetype6, libgstreamer1.0-0, libgstreamer-plugins-base1.0-0, gstreamer1.0-plugins-base, gstreamer1.0-plugins-good, gstreamer1.0-libav\nDescription: Native MonoCode desktop and remote host\n"
            ),
        )?;
        command(
            Command::new("dpkg-deb")
                .args(["--root-owner-group", "--build"])
                .arg(&root)
                .arg(output.join(format!("MonoCode_{version}_{target}.deb"))),
        )?;
    }
    if formats == "all" || formats.split(',').any(|format| format == "rpm") {
        let rpm_root = stage.join("rpm");
        for dir in ["BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"] {
            fs::create_dir_all(rpm_root.join(dir))?;
        }
        let spec = include_str!("../../../packaging/linux/monocode.spec")
            .replace("@@VERSION@@", version)
            .replace("@@ROOT@@", &root.to_string_lossy());
        let path = rpm_root.join("SPECS/monocode.spec");
        fs::write(&path, spec)?;
        command(
            Command::new("rpmbuild")
                .arg("-bb")
                .arg("--define")
                .arg(format!("_topdir {}", rpm_root.display()))
                .arg(path),
        )?;
        for directory in fs::read_dir(rpm_root.join("RPMS"))? {
            for entry in fs::read_dir(directory?.path())? {
                let file = entry?.path();
                copy(
                    &file,
                    output.join(format!("MonoCode_{version}_{target}.rpm")),
                )?;
            }
        }
    }
    if formats == "all" || formats.split(',').any(|format| format == "appimage") {
        let appdir = stage.join("MonoCode.AppDir");
        copy(app, appdir.join("usr/bin/monocode"))?;
        copy(host, appdir.join("usr/bin/monocode-host"))?;
        copy(
            workspace().join("packaging/linux/MonoCode.desktop"),
            appdir.join("MonoCode.desktop"),
        )?;
        copy(
            workspace().join("packaging/assets/icon.png"),
            appdir.join("monocode.png"),
        )?;
        copy(
            workspace().join("packaging/linux/AppRun"),
            appdir.join("AppRun"),
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(appdir.join("AppRun"), fs::Permissions::from_mode(0o755))?;
        }
        let tool = options.get("appimage-tool", "linuxdeploy");
        let package = output.join(format!("MonoCode_{version}_{target}.AppImage"));
        let libraries = bundle_gstreamer(&appdir)?;
        let mut deploy = Command::new(tool);
        deploy
            .arg("--appdir")
            .arg(&appdir)
            .arg("--executable")
            .arg(appdir.join("usr/bin/monocode"))
            .arg("--executable")
            .arg(appdir.join("usr/bin/monocode-host"));
        // These libraries load through dlopen, so binary dependency scans cannot find them.
        // linuxdeploy also copies each plugin's linked codec and audio dependencies.
        for library in libraries {
            deploy.arg("--library").arg(library);
        }
        deploy
            .arg("--executable")
            .arg(appdir.join("usr/libexec/gstreamer-1.0/gst-plugin-scanner"))
            .args(["--output", "appimage"])
            .env("LDAI_OUTPUT", std::path::absolute(&package)?)
            .env("APPIMAGE_EXTRACT_AND_RUN", "1");
        command(&mut deploy)?;
    }
    Ok(())
}

fn gstreamer_path(variable: &str) -> Result<PathBuf> {
    let result = Command::new("pkg-config")
        .arg(format!("--variable={variable}"))
        .arg("gstreamer-1.0")
        .output()
        .context("finding GStreamer for the AppImage")?;
    let path = String::from_utf8(result.stdout)?;
    if !result.status.success() || path.trim().is_empty() {
        bail!("GStreamer {variable} is unavailable. Run scripts/install-native-linux-deps.sh");
    }
    Ok(PathBuf::from(path.trim()))
}

fn bundle_gstreamer(appdir: &Path) -> Result<Vec<PathBuf>> {
    let libraries = gstreamer_path("libdir")?;
    let plugins = gstreamer_path("pluginsdir")?;
    let scanner = gstreamer_path("pluginscannerdir")?.join("gst-plugin-scanner");
    let files = gstreamer_files(&libraries, &plugins)?;
    for file in &files {
        let name = file.file_name().context("GStreamer file name")?;
        let directory = if file.parent() == Some(plugins.as_path()) {
            "usr/lib/gstreamer-1.0"
        } else {
            "usr/lib"
        };
        copy(file, appdir.join(directory).join(name))?;
    }
    copy(
        scanner,
        appdir.join("usr/libexec/gstreamer-1.0/gst-plugin-scanner"),
    )?;
    Ok(files)
}

fn gstreamer_files(libraries: &Path, plugins: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for name in [
        "libgstreamer-1.0.so.0",
        "libgstapp-1.0.so.0",
        "libgstvideo-1.0.so.0",
    ] {
        let path = libraries.join(name);
        if !path.is_file() {
            bail!("missing GStreamer runtime {}", path.display());
        }
        files.push(path);
    }
    for name in [
        "libgstcoreelements.so",
        "libgstapp.so",
        "libgstplayback.so",
        "libgsttypefindfunctions.so",
        "libgstisomp4.so",
        "libgstmatroska.so",
        "libgstlibav.so",
        "libgstvpx.so",
    ] {
        if !plugins.join(name).is_file() {
            bail!(
                "missing GStreamer plugin {name}. Install its base, good, and libav plugin packages"
            );
        }
    }
    if !plugins.join("libgstvideoconvertscale.so").is_file()
        && !(plugins.join("libgstvideoconvert.so").is_file()
            && plugins.join("libgstvideoscale.so").is_file())
    {
        bail!("missing GStreamer's video conversion plugin");
    }
    for entry in fs::read_dir(plugins)? {
        let path = entry?.path();
        if path.is_file() && path.extension().is_some_and(|extension| extension == "so") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn windows(
    app: &Path,
    host: &Path,
    version: &str,
    target: &str,
    stage: &Path,
    output: &Path,
) -> Result<()> {
    copy(app, stage.join("monocode-app.exe"))?;
    copy(host, stage.join("monocode-host.exe"))?;
    let installer =
        std::path::absolute(output.join(format!("MonoCode_{version}_{target}-setup.exe")))?;
    command(
        Command::new("makensis")
            .arg("/WX")
            .arg(format!("/DVERSION={version}"))
            .arg(format!("/DBIN_DIR={}", stage.display()))
            .arg(format!("/DOUTPUT={}", installer.display()))
            .arg(format!(
                "/DICON={}",
                workspace().join("packaging/assets/icon.ico").display()
            ))
            .arg(workspace().join("packaging/windows/installer.nsi")),
    )
}

fn package_files(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut files = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    files.retain(|path| {
        path.is_file()
            && path
                .file_name()
                .is_some_and(|name| name != "SHA256SUMS" && name != "latest.json")
    });
    files.sort();
    Ok(files)
}

fn checksums(options: &Options) -> Result<()> {
    let directory = Path::new(options.get("directory", "build/native"));
    let mut content = String::new();
    for path in package_files(directory)? {
        let digest = Sha256::digest(fs::read(&path)?);
        content.push_str(&format!(
            "{digest:x}  {}\n",
            path.file_name().context("file name")?.to_string_lossy()
        ));
    }
    if content.is_empty() {
        bail!("no artifacts in {}", directory.display());
    }
    fs::write(directory.join("SHA256SUMS"), content)?;
    Ok(())
}

/// Selects only installer-compatible desktop artifacts. Host archives and
/// disk images cannot enter the self-update feed.
fn feed_target(name: &str, version: &str) -> Option<String> {
    let rest = name.strip_prefix(&format!("MonoCode_{version}_"))?;
    if let Some(target) = rest.strip_suffix(".app.tar.gz") {
        return target
            .strip_suffix("-apple-darwin")
            .map(|arch| format!("darwin-{arch}"));
    }
    if let Some(target) = rest.strip_suffix("-setup.exe") {
        return target
            .strip_suffix("-pc-windows-msvc")
            .map(|arch| format!("windows-{arch}"));
    }
    for (extension, installer) in [(".deb", "deb"), (".rpm", "rpm"), (".AppImage", "appimage")] {
        if let Some(target) = rest.strip_suffix(extension) {
            return target
                .strip_suffix("-unknown-linux-gnu")
                .map(|arch| format!("linux-{arch}-{installer}"));
        }
    }
    None
}

fn manifest(options: &Options) -> Result<()> {
    let version = version(options)?;
    let directory = Path::new(options.get("directory", "build/native"));
    let base_url = options.required("base-url")?.trim_end_matches('/');
    if !base_url.starts_with("https://") {
        bail!("release base URL must use HTTPS");
    }
    let mut platforms = BTreeMap::new();
    for path in package_files(directory)? {
        let name = path.file_name().context("artifact name")?.to_string_lossy();
        let Some(target) = feed_target(&name, &version) else {
            continue;
        };
        let signature = fs::read_to_string(path.with_file_name(format!("{name}.sig")))
            .with_context(|| format!("missing signature for {name}"))?;
        if signature.trim().is_empty() {
            bail!("empty signature for {name}");
        }
        if platforms.insert(target.clone(), serde_json::json!({"signature": signature.trim(), "url": format!("{base_url}/{name}")})).is_some() { bail!("duplicate updater target {target}"); }
    }
    for required in options
        .get("require-targets", "")
        .split(',')
        .filter(|target| !target.is_empty())
    {
        if !platforms.contains_key(required) {
            bail!("missing updater target {required}");
        }
    }
    if platforms.is_empty() {
        bail!("no signed update packages");
    }
    let date =
        time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?;
    let value = serde_json::json!({"version":version, "notes": options.get("notes", &format!("MonoCode {version}")), "pub_date":date, "platforms":platforms});
    fs::write(
        options.get("output", "build/native/latest.json"),
        format!("{}\n", serde_json::to_string_pretty(&value)?),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows_import_fixture(library: &str) -> Vec<u8> {
        let mut bytes = vec![0; 0x400];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x84..0x86].copy_from_slice(&0x8664_u16.to_le_bytes());
        bytes[0x86..0x88].copy_from_slice(&1_u16.to_le_bytes());
        bytes[0x94..0x96].copy_from_slice(&240_u16.to_le_bytes());
        bytes[0x96..0x98].copy_from_slice(&0x22_u16.to_le_bytes());
        bytes[0x98..0x9a].copy_from_slice(&0x20b_u16.to_le_bytes());
        bytes[0xb8..0xbc].copy_from_slice(&0x1000_u32.to_le_bytes());
        bytes[0xbc..0xc0].copy_from_slice(&0x200_u32.to_le_bytes());
        bytes[0xd0..0xd4].copy_from_slice(&0x2000_u32.to_le_bytes());
        bytes[0xd4..0xd8].copy_from_slice(&0x200_u32.to_le_bytes());
        bytes[0x104..0x108].copy_from_slice(&16_u32.to_le_bytes());
        bytes[0x110..0x114].copy_from_slice(&0x1000_u32.to_le_bytes());
        bytes[0x114..0x118].copy_from_slice(&40_u32.to_le_bytes());
        bytes[0x188..0x190].copy_from_slice(b".idata\0\0");
        bytes[0x190..0x194].copy_from_slice(&0x200_u32.to_le_bytes());
        bytes[0x194..0x198].copy_from_slice(&0x1000_u32.to_le_bytes());
        bytes[0x198..0x19c].copy_from_slice(&0x200_u32.to_le_bytes());
        bytes[0x19c..0x1a0].copy_from_slice(&0x200_u32.to_le_bytes());
        bytes[0x200..0x204].copy_from_slice(&0x1040_u32.to_le_bytes());
        bytes[0x20c..0x210].copy_from_slice(&0x1080_u32.to_le_bytes());
        bytes[0x210..0x214].copy_from_slice(&0x1040_u32.to_le_bytes());
        bytes[0x240..0x248].copy_from_slice(&0x10c0_u64.to_le_bytes());
        bytes[0x280..0x280 + library.len()].copy_from_slice(library.as_bytes());
        bytes[0x2c2..0x2d1].copy_from_slice(b"fixture_symbol\0");
        bytes
    }

    fn target_fixture(target: &str) -> Vec<u8> {
        let arm = target.starts_with("aarch64-");
        if target.contains("windows") {
            let mut bytes = windows_import_fixture("KERNEL32.dll");
            let machine: u16 = if arm { 0xaa64 } else { 0x8664 };
            bytes[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
            bytes
        } else if target.contains("linux") {
            let mut bytes = vec![0; 64];
            bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
            bytes[16..18].copy_from_slice(&3_u16.to_le_bytes());
            let machine: u16 = if arm { 183 } else { 62 };
            bytes[18..20].copy_from_slice(&machine.to_le_bytes());
            bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
            bytes[52..54].copy_from_slice(&64_u16.to_le_bytes());
            bytes
        } else {
            let mut bytes = vec![0; 32];
            bytes[..4].copy_from_slice(&0xfeedfacf_u32.to_le_bytes());
            let cpu: u32 = if arm { 0x0100000c } else { 0x01000007 };
            bytes[4..8].copy_from_slice(&cpu.to_le_bytes());
            bytes[12..16].copy_from_slice(&2_u32.to_le_bytes());
            bytes
        }
    }

    #[test]
    fn package_target_rejects_mislabeled_cpu_and_operating_system() {
        let targets = [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
            "aarch64-pc-windows-msvc",
            "x86_64-pc-windows-msvc",
        ];
        for actual in targets {
            let bytes = target_fixture(actual);
            for requested in targets {
                let result = check_package_target_bytes(&bytes, requested);
                assert_eq!(result.is_ok(), actual == requested, "{actual}/{requested}");
            }
        }
        assert!(check_package_target_bytes(b"not an executable", targets[0]).is_err());
        assert!(check_package_target_bytes(&target_fixture(targets[0]), "unknown").is_err());
    }

    #[test]
    fn package_target_reads_only_the_mach_o_header() {
        // One LC_SEGMENT_64 whose section count overflows its command size,
        // which object's full parser rejects as an invalid number of sections.
        let mut bytes = target_fixture("aarch64-apple-darwin");
        bytes[16..20].copy_from_slice(&1_u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&72_u32.to_le_bytes());
        let mut segment = vec![0; 72];
        segment[..4].copy_from_slice(&0x19_u32.to_le_bytes());
        segment[4..8].copy_from_slice(&72_u32.to_le_bytes());
        segment[64..68].copy_from_slice(&1_u32.to_le_bytes());
        bytes.extend(segment);
        assert!(object::File::parse(&*bytes).is_err());
        check_package_target_bytes(&bytes, "aarch64-apple-darwin").unwrap();
        assert!(check_package_target_bytes(&bytes, "x86_64-apple-darwin").is_err());
    }

    #[test]
    fn wrong_host_architecture_fails_before_execution_or_archive_creation() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("output");
        fs::write(
            root.path().join("monocode-host.exe"),
            target_fixture("x86_64-pc-windows-msvc"),
        )
        .unwrap();
        let options = Options(BTreeMap::from([
            ("target".into(), "aarch64-pc-windows-msvc".into()),
            ("formats".into(), "host".into()),
            (
                "binaries".into(),
                root.path().to_string_lossy().into_owned(),
            ),
            ("output".into(), output.to_string_lossy().into_owned()),
        ]));
        let error = bundle(&options).unwrap_err();
        assert!(format!("{error:#}").contains("package target aarch64-pc-windows-msvc"));
        assert!(!output.exists());
    }

    #[test]
    fn portable_windows_executable_rejects_redistributable_runtime_imports() {
        for library in [
            "VCRUNTIME140.dll",
            "vcruntime140_1.dll",
            "MSVCP140.dll",
            "concrt140.dll",
            "msvcr120.dll",
        ] {
            let error = check_windows_runtime_bytes(&windows_import_fixture(library)).unwrap_err();
            assert!(error.to_string().contains("redistributable runtime"));
        }
        for library in [
            "KERNEL32.dll",
            "msvcrt.dll",
            "msvcp_win.dll",
            "ucrtbase.dll",
            "api-ms-win-crt-runtime-l1-1-0.dll",
        ] {
            check_windows_runtime_bytes(&windows_import_fixture(library)).unwrap();
        }
        assert!(check_windows_runtime_bytes(b"not a PE executable").is_err());
    }

    #[test]
    fn appimage_requires_decoders_and_collects_the_dynamically_loaded_plugins() {
        let directory = tempfile::tempdir().unwrap();
        let libraries = directory.path().join("lib");
        let plugins = libraries.join("gstreamer-1.0");
        fs::create_dir_all(&plugins).unwrap();
        for name in [
            "libgstreamer-1.0.so.0",
            "libgstapp-1.0.so.0",
            "libgstvideo-1.0.so.0",
        ] {
            fs::write(libraries.join(name), b"fixture").unwrap();
        }
        for name in [
            "libgstcoreelements.so",
            "libgstapp.so",
            "libgstplayback.so",
            "libgsttypefindfunctions.so",
            "libgstisomp4.so",
            "libgstmatroska.so",
            "libgstlibav.so",
            "libgstvpx.so",
            "libgstvideoconvertscale.so",
        ] {
            fs::write(plugins.join(name), b"fixture").unwrap();
        }
        fs::write(plugins.join("README"), b"not a plugin").unwrap();
        let files = gstreamer_files(&libraries, &plugins).unwrap();
        assert_eq!(files.len(), 12);
        assert!(files.contains(&plugins.join("libgstlibav.so")));
        assert!(files.contains(&libraries.join("libgstapp-1.0.so.0")));
        fs::remove_file(plugins.join("libgstlibav.so")).unwrap();
        assert!(
            gstreamer_files(&libraries, &plugins)
                .unwrap_err()
                .to_string()
                .contains("libgstlibav.so")
        );
    }

    #[test]
    fn feed_artifacts_match_the_installer_and_host_archives_are_excluded() {
        let cases = [
            ("aarch64-apple-darwin.app.tar.gz", "darwin-aarch64"),
            ("x86_64-apple-darwin.app.tar.gz", "darwin-x86_64"),
            ("x86_64-pc-windows-msvc-setup.exe", "windows-x86_64"),
            ("x86_64-unknown-linux-gnu.deb", "linux-x86_64-deb"),
            ("x86_64-unknown-linux-gnu.rpm", "linux-x86_64-rpm"),
            ("x86_64-unknown-linux-gnu.AppImage", "linux-x86_64-appimage"),
        ];
        for (name, target) in cases {
            assert_eq!(
                feed_target(&format!("MonoCode_0.6.0_{name}"), "0.6.0").as_deref(),
                Some(target)
            );
        }
        assert_eq!(
            feed_target("MonoCode_0.6.0_aarch64-apple-darwin.dmg", "0.6.0"),
            None
        );
        assert_eq!(
            feed_target("monocode-host_0.6.0_aarch64-apple-darwin.tar.gz", "0.6.0"),
            None
        );
    }

    #[test]
    fn unsigned_package_cannot_enter_manifest() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory
                .path()
                .join("MonoCode_0.6.0_x86_64-unknown-linux-gnu.deb"),
            "package",
        )
        .unwrap();
        let options = Options(BTreeMap::from([
            (
                "directory".into(),
                directory.path().to_string_lossy().into_owned(),
            ),
            ("version".into(), "0.6.0".into()),
            (
                "base-url".into(),
                "https://example.test/releases/0.6.0".into(),
            ),
            (
                "output".into(),
                directory
                    .path()
                    .join("latest.json")
                    .to_string_lossy()
                    .into_owned(),
            ),
        ]));
        assert!(manifest(&options).is_err());
        assert!(!directory.path().join("latest.json").exists());
    }
}
