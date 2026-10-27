// Package a prebuilt native executable. Never invokes the Tauri app build or
// writes production release/update paths.
import { spawnSync } from "node:child_process";
import {
  cp,
  mkdir,
  readFile,
  readdir,
  writeFile,
  chmod,
  lstat,
} from "node:fs/promises";
import { join, resolve, dirname, basename } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
export function previewVersion(version) {
  const match =
    /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)-preview\.(0|[1-9]\d*)$/.exec(
      version,
    );
  if (!match || match.slice(1).some((n) => Number(n) > 65535))
    throw new Error(
      "Expected a native preview version such as 0.16.0-preview.1",
    );
  return {
    version,
    base: match.slice(1, 4).join("."),
    sequence: match[4],
    windows: match.slice(1).join("."),
  };
}
function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
    ...options,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    const error = new Error(`${command} failed (${result.status})`);
    error.status = result.status;
    throw error;
  }
  return result;
}

// stapler's EX_NOHOST (68) can indicate a ticket-service/network failure.
// Retry it boundedly; every other status still stops packaging immediately.
export async function runStapler(
  action,
  file,
  { wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms)) } = {},
) {
  for (let attempt = 1; attempt <= 5; attempt++) {
    try {
      run("xcrun", ["stapler", action, file]);
      return;
    } catch (error) {
      if (error.status !== 68 || attempt === 5) throw error;
      const delay = attempt * 10_000;
      console.warn(
        `Apple ticket service unavailable during ${action}; retrying in ${delay / 1000}s (${attempt}/5)`,
      );
      await wait(delay);
    }
  }
}

// Count apparent bytes, including sparse files, without following the
// Applications shortcut or any other symlinks outside the staging tree.
async function logicalBytes(path) {
  const info = await lstat(path);
  if (!info.isDirectory()) return info.size;
  const children = await Promise.all(
    (await readdir(path)).map((name) => logicalBytes(join(path, name))),
  );
  return info.size + children.reduce((sum, size) => sum + size, 0);
}
async function notices(directory) {
  await mkdir(directory, { recursive: true });
  for (const file of [
    "LICENSE",
    "NOTICE",
    "docs/native/LICENSE-GPUI-APACHE",
    "crates/monocode-gpui/assets/hugeicons.LICENSE",
    "crates/monocode-gpui/assets/dark-plus.LICENSE",
    "crates/monocode-gpui/assets/native-editor.LICENSE.txt",
  ]) {
    await cp(join(root, file), join(directory, file.split("/").at(-1)));
  }
  await cp(
    join(root, "crates/monocode-gpui/assets/material/LICENSE"),
    join(directory, "material-icons.LICENSE"),
  );
}
async function linuxTree(directory, binary, kind) {
  const app = join(directory, "usr/lib/monocode-native");
  await mkdir(app, { recursive: true });
  await cp(binary, join(app, "monocode-gpui"));
  await chmod(join(app, "monocode-gpui"), 0o755);
  await writeFile(
    join(app, "native-install.json"),
    JSON.stringify({ app_id: "dev.usemono.gpui", kind }),
  );
  await notices(join(app, "licenses"));
  await mkdir(join(directory, "usr/bin"), { recursive: true });
  await writeFile(
    join(directory, "usr/bin/monocode-native"),
    '#!/bin/sh\nexec /usr/lib/monocode-native/monocode-gpui "$@"\n',
    { mode: 0o755 },
  );
  await mkdir(join(directory, "usr/share/applications"), { recursive: true });
  const desktop =
    "[Desktop Entry]\nType=Application\nName=Monocode Native\nExec=monocode-native %F\nIcon=monocode-native\nCategories=Development;\nTerminal=false\nStartupWMClass=dev.usemono.gpui\n";
  await writeFile(
    join(directory, "usr/share/applications/monocode-native.desktop"),
    desktop,
  );
  const icons = join(directory, "usr/share/icons/hicolor/256x256/apps");
  await mkdir(icons, { recursive: true });
  await cp(
    join(root, "public/monocode.png"),
    join(icons, "monocode-native.png"),
  );
  return { app, desktop };
}

export async function packageNative({
  kind,
  binary,
  out,
  version,
  arch = "x86_64",
}) {
  const parsed = previewVersion(version);
  binary = resolve(binary);
  out = resolve(out);
  const embedded = run(binary, ["--version"], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  }).stdout.trim();
  if (embedded !== `Monocode Native ${version}`)
    throw new Error(`Native executable version does not match ${version}`);
  await mkdir(out, { recursive: true });
  const work = join(out, "work");
  await mkdir(work, { recursive: true });
  const artifacts = [];
  const add = (filename, target = null) => {
    artifacts.push({ filename, target });
    return join(out, filename);
  };
  if (kind === "macos") {
    const expectedArch = arch === "aarch64" ? "arm64" : "x86_64";
    const actualArch = run("lipo", ["-archs", binary], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "inherit"],
    }).stdout.trim();
    if (actualArch !== expectedArch)
      throw new Error(`Expected ${expectedArch}, found ${actualArch}`);
    run(
      "bash",
      [
        "scripts/bundle-native-macos.sh",
        "release",
        dirname(dirname(binary)),
        version,
        "monocode-data",
      ],
      {
        env: {
          ...process.env,
          MONOCODE_NATIVE_SIGNING_IDENTITY:
            process.env.APPLE_SIGNING_IDENTITY || "",
        },
      },
    );
    const app = join(dirname(binary), "Monocode Native.app");
    if (
      !process.env.APPLE_SIGNING_IDENTITY ||
      process.env.APPLE_SIGNING_IDENTITY === "-"
    )
      throw new Error(
        "Public previews require a Developer ID signing identity",
      );
    const submit = async (file, staple) => {
      const result = run(
        "xcrun",
        [
          "notarytool",
          "submit",
          file,
          "--key",
          process.env.APPLE_API_KEY_PATH,
          "--key-id",
          process.env.APPLE_API_KEY,
          "--issuer",
          process.env.APPLE_API_ISSUER,
          "--wait",
          "--output-format",
          "json",
        ],
        { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
      );
      if (JSON.parse(result.stdout).status !== "Accepted")
        throw new Error(
          `Apple did not accept ${basename(file)} for notarization`,
        );
      if (staple) {
        await runStapler("staple", staple);
        await runStapler("validate", staple);
      }
    };
    const zip = join(work, "notarize.zip");
    run("ditto", ["-c", "-k", "--keepParent", app, zip]);
    await submit(zip, app);
    run("codesign", ["--verify", "--deep", "--strict", app]);
    run("spctl", ["--assess", "--type", "execute", app]);
    const name = arch === "aarch64" ? "aarch64" : "x64";
    const archive = add(
      `MonocodeNative_${version}_${name}.app.tar.gz`,
      `darwin-${arch}-app`,
    );
    run("tar", ["-czf", archive, "-C", dirname(app), "Monocode Native.app"], {
      env: { ...process.env, COPYFILE_DISABLE: "1" },
    });
    const imageRoot = join(work, "dmg");
    await mkdir(imageRoot);
    run("ditto", [app, join(imageRoot, "Monocode Native.app")]);
    run("ln", ["-s", "/Applications", join(imageRoot, "Applications")]);
    const dmg = add(`MonocodeNative_${version}_${name}.dmg`);
    const imageSizeMiB =
      Math.ceil((await logicalBytes(imageRoot)) / 1048576) + 64;
    run("hdiutil", [
      "create",
      "-volname",
      "Monocode Native",
      "-srcfolder",
      imageRoot,
      "-ov",
      "-format",
      "UDZO",
      "-fs",
      "HFS+",
      "-size",
      `${imageSizeMiB}m`,
      dmg,
    ]);
    run("codesign", [
      "--sign",
      process.env.APPLE_SIGNING_IDENTITY,
      "--timestamp",
      dmg,
    ]);
    await submit(dmg, dmg);
  } else if (kind === "linux") {
    const debRoot = join(work, "deb");
    await linuxTree(debRoot, binary, "deb");
    await mkdir(join(debRoot, "DEBIAN"));
    await writeFile(
      join(debRoot, "DEBIAN/control"),
      `Package: monocode-native\nVersion: ${version.replace("-preview.", "~preview.")}\nArchitecture: amd64\nMaintainer: Monocode maintainers\nSection: devel\nPriority: optional\nDepends: libasound2, libfontconfig1, libfreetype6, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libx11-xcb1, libxcb1, libxcb-shape0, libxcb-xfixes0, libxcb-randr0, libegl1, libvulkan1, libdbus-1-3, pkexec | policykit-1\nDescription: Monocode Native experimental preview\n`,
    );
    run("dpkg-deb", [
      "--build",
      "--root-owner-group",
      debRoot,
      add(`monocode-native_${version}_amd64.deb`, "linux-x86_64-deb"),
    ]);
    const appDir = join(work, "MonocodeNative.AppDir");
    const { desktop } = await linuxTree(appDir, binary, "appimage");
    await writeFile(
      join(appDir, "monocode-native.desktop"),
      desktop.replace("Exec=monocode-native %F", "Exec=monocode-gpui %F"),
    );
    await cp(
      join(root, "public/monocode.png"),
      join(appDir, "monocode-native.png"),
    );
    await writeFile(
      join(appDir, "AppRun"),
      '#!/bin/sh\nexec "$APPDIR/usr/lib/monocode-native/monocode-gpui" "$@"\n',
      { mode: 0o755 },
    );
    run(
      process.env.APPIMAGETOOL || "appimagetool",
      [
        appDir,
        add(
          `MonocodeNative_${version}_amd64.AppImage`,
          "linux-x86_64-appimage",
        ),
      ],
      {
        env: { ...process.env, ARCH: "x86_64", APPIMAGE_EXTRACT_AND_RUN: "1" },
      },
    );
  } else if (kind === "rpm") {
    const top = join(work, "rpmbuild");
    for (const name of ["BUILD", "RPMS", "SOURCES", "SPECS", "SRPMS"])
      await mkdir(join(top, name), { recursive: true });
    const tree = join(top, "SOURCES/tree");
    await linuxTree(tree, binary, "rpm");
    const spec = join(top, "SPECS/monocode-native.spec");
    await writeFile(
      spec,
      `Name: monocode-native\nVersion: ${parsed.base}\nRelease: 0.preview.${parsed.sequence}%{?dist}\nSummary: Monocode Native experimental preview\nLicense: MIT AND Apache-2.0\nBuildArch: x86_64\nRequires: polkit\n%description\nMonocode Native experimental preview.\n%install\nmkdir -p %{buildroot}\ncp -a %{_sourcedir}/tree/. %{buildroot}/\n%files\n/usr/bin/monocode-native\n/usr/lib/monocode-native\n/usr/share/applications/monocode-native.desktop\n/usr/share/icons/hicolor/256x256/apps/monocode-native.png\n`,
    );
    run("rpmbuild", [
      "--define",
      `_topdir ${top}`,
      "--define",
      "_build_id_links none",
      "-bb",
      spec,
    ]);
    const rpms = (await readdir(join(top, "RPMS/x86_64"))).filter(
      (n) =>
        n.endsWith(".rpm") &&
        !n.includes("debuginfo") &&
        !n.includes("debugsource"),
    );
    if (rpms.length !== 1) throw new Error("Expected exactly one native rpm");
    await cp(
      join(top, "RPMS/x86_64", rpms[0]),
      add(rpms[0], "linux-x86_64-rpm"),
    );
  } else if (kind === "windows") {
    const stage = join(work, "windows");
    await mkdir(stage);
    await cp(binary, join(stage, "monocode-gpui.exe"));
    await writeFile(
      join(stage, "native-install.json"),
      JSON.stringify({ app_id: "dev.usemono.gpui", kind: "nsis" }),
    );
    await notices(join(stage, "licenses"));
    const installer = add(
      `MonocodeNative_${version}_x64-setup.exe`,
      "windows-x86_64-nsis",
    );
    const quote = (value) =>
      value.replaceAll("$", "$$").replaceAll('"', '$\\"');
    const template = await readFile(
      join(root, "scripts/native-installer.nsi"),
      "utf8",
    );
    const script = template
      .replaceAll("@STAGE@", quote(stage))
      .replaceAll("@OUTFILE@", quote(installer))
      .replaceAll("@ICON@", quote(join(root, "src-tauri/icons/icon.ico")))
      .replaceAll("@VERSION@", version)
      .replaceAll("@WINDOWS_VERSION@", parsed.windows);
    const nsi = join(work, "installer.nsi");
    await writeFile(nsi, script);
    run("makensis", [nsi]);
  } else throw new Error(`Unknown native package kind: ${kind}`);
  await writeFile(
    join(out, `artifacts-${kind}-${arch}.json`),
    JSON.stringify(artifacts, null, 2),
  );
  return artifacts;
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(resolve(process.argv[1])).href
) {
  const [kind, binary, out, arch] = process.argv.slice(2);
  await packageNative({
    kind,
    binary,
    out,
    arch,
    version: process.env.MONOCODE_NATIVE_VERSION,
  });
}
