import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  access,
  chmod,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { setTimeout as delay } from "node:timers/promises";
import { delimiter, join } from "node:path";
import { test } from "node:test";
import { linuxTree, packageNative, runStapler } from "./package-native.mjs";

test("Linux desktop identity resolves the bundled application icon on Wayland", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "native-linux-icon-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const binary = join(directory, "monocode-gpui");
  await writeFile(binary, "native executable fixture");
  const tree = join(directory, "tree");
  const { app } = await linuxTree(tree, binary, "deb");
  const { app_id } = JSON.parse(await readFile(join(app, "native-install.json"), "utf8"));
  const desktop = await readFile(join(tree, "usr/share/applications", `${app_id}.desktop`), "utf8");
  assert.ok(desktop.includes(`StartupWMClass=${app_id}\n`));
  const icon = /^Icon=(.+)$/m.exec(desktop)?.[1];
  assert.ok(icon);
  const bundled = await readFile(join(tree, "usr/share/icons/hicolor/256x256/apps", `${icon}.png`));
  assert.deepEqual(bundled, await readFile(new URL("../public/monocode.png", import.meta.url)));
  await access(join(tree, "usr/bin/monocode-native"));
});

test("AppImage desktop registration is opt-in and launches the permanent image", {
  skip: process.platform === "win32",
}, async (t) => {
  const directory = await mkdtemp(join(tmpdir(), 'native AppImage " $ ` %F \\-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const tools = join(directory, "tools");
  await mkdir(tools);
  for (const name of ["dpkg-deb", "appimagetool"]) {
    const tool = join(tools, name);
    await writeFile(tool, '#!/bin/sh\nfor output do :; done\n: > "$output"\nchmod +x "$output"\n');
    await chmod(tool, 0o755);
  }
  const binary = join(directory, "monocode-gpui");
  await writeFile(binary, '#!/bin/sh\nif [ "${1-}" = "--version" ]; then\nprintf "Monocode Native 0.16.0-preview.2\\n"\nelse\nprintf "<%s>\\n" "$@"\nfi\n');
  await chmod(binary, 0o755);
  const out = join(directory, "packages");
  const originalPath = process.env.PATH;
  const originalTool = process.env.APPIMAGETOOL;
  try {
    process.env.PATH = `${tools}${delimiter}${originalPath}`;
    process.env.APPIMAGETOOL = join(tools, "appimagetool");
    await packageNative({ kind: "linux", binary, out, version: "0.16.0-preview.2" });
  } finally {
    process.env.PATH = originalPath;
    if (originalTool === undefined) delete process.env.APPIMAGETOOL;
    else process.env.APPIMAGETOOL = originalTool;
  }
  const appDir = join(out, "work/MonocodeNative.AppDir");
  const appImage = join(directory, 'MonoCode " $ ` %F \\.AppImage');
  await writeFile(appImage, '#!/bin/sh\nprintf "%s\\n" "$0" "$@" > "$NATIVE_TEST_LAUNCH"\n');
  await chmod(appImage, 0o755);
  const data = join(directory, "desktop data");
  const env = { ...process.env, APPDIR: appDir, APPIMAGE: appImage, XDG_DATA_HOME: data };
  const launch = (args, overrides = {}) => spawnSync(join(appDir, "AppRun"), args, {
    env: { ...env, ...overrides }, encoding: "utf8",
  });

  const normal = launch(["--dev-data", "--cwd", "a project with spaces"]);
  assert.equal(normal.status, 0, normal.stderr);
  assert.equal(normal.stdout, "<--dev-data>\n<--cwd>\n<a project with spaces>\n");
  await assert.rejects(access(data), { code: "ENOENT" });
  for (const overrides of [{ APPIMAGE: "" }, { APPIMAGE: "relative.AppImage" }, { APPIMAGE: join(directory, "missing.AppImage") }, { XDG_DATA_HOME: "relative" }]) {
    const invalid = launch(["--install-desktop"], overrides);
    assert.notEqual(invalid.status, 0);
    await assert.rejects(access(data), { code: "ENOENT" });
  }
  for (const name of ["invalid=name.AppImage", "invalid\nname.AppImage"]) {
    const invalidImage = join(directory, name);
    await writeFile(invalidImage, "#!/bin/sh\n");
    await chmod(invalidImage, 0o755);
    assert.notEqual(launch(["--install-desktop"], { APPIMAGE: invalidImage }).status, 0);
    await assert.rejects(access(data), { code: "ENOENT" });
  }
  assert.notEqual(launch(["--install-desktop", "extra"]).status, 0);

  const installed = launch(["--install-desktop"]);
  assert.equal(installed.status, 0, installed.stderr);
  const desktop = join(data, "applications/dev.usemono.gpui.desktop");
  const entry = await readFile(desktop, "utf8");
  assert.ok(entry.includes("StartupWMClass=dev.usemono.gpui\n"));
  assert.ok(!entry.includes(appDir));
  assert.deepEqual(await readFile(join(data, "icons/hicolor/256x256/apps/monocode-native.png")),
    await readFile(new URL("../public/monocode.png", import.meta.url)));
  const repeated = launch(["--install-desktop"]);
  assert.equal(repeated.status, 0, repeated.stderr);
  assert.equal(await readFile(desktop, "utf8"), entry);

  const fallbackHome = join(directory, "home");
  const fallback = launch(["--install-desktop"], { XDG_DATA_HOME: "", HOME: fallbackHome });
  assert.equal(fallback.status, 0, fallback.stderr);
  await access(join(fallbackHome, ".local/share/applications/dev.usemono.gpui.desktop"));

  const validate = spawnSync("desktop-file-validate", [desktop], { encoding: "utf8" });
  if (validate.error?.code !== "ENOENT") assert.equal(validate.status, 0, validate.stderr);
  if (process.platform !== "linux") {
    t.diagnostic("Desktop launcher round-trip is Linux-only");
    return;
  }
  const launched = join(directory, "launched");
  const gio = spawnSync("gio", ["launch", desktop], {
    env: { ...env, NATIVE_TEST_LAUNCH: launched }, encoding: "utf8",
  });
  if (gio.error?.code !== "ENOENT") {
    assert.equal(gio.status, 0, gio.stderr);
    // gio may return before the launched AppImage writes its arguments.
    for (let attempt = 0; attempt < 50; attempt += 1) {
      const output = await readFile(launched, "utf8").catch(() => "");
      if (output.endsWith("\n")) break;
      await delay(100);
    }
    assert.equal((await readFile(launched, "utf8")).trimEnd(), appImage);
  } else {
    t.diagnostic("gio unavailable; desktop launcher round-trip was not exercised");
  }
});

test(
  "Mac preview packaging checks notarization, preserves preview identity and emits both download and updater packages",
  { skip: process.platform === "win32" },
  async (t) => {
    const directory = await mkdtemp(
      join(tmpdir(), "native packaging with spaces-"),
    );
    t.after(() => rm(directory, { recursive: true, force: true }));
    const bin = join(directory, "tools");
    const release = join(directory, "target/release");
    await mkdir(bin);
    await mkdir(release, { recursive: true });
    const binary = join(release, "monocode-gpui");
    await writeFile(
      binary,
      "#!/bin/sh\nprintf 'Monocode Native 0.16.0-preview.2\\n'\n",
    );
    await chmod(binary, 0o755);
    const fakeTool = `#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const name = path.basename(process.argv[1]);
const args = process.argv.slice(2);
if (name === 'uname') console.log('Darwin');
if (name === 'lipo') console.log('arm64');
if (name === 'ditto') {
  if (args[0] === '-c') fs.writeFileSync(args.at(-1), 'zip');
  else {
    fs.cpSync(args[0], args[1], { recursive: true });
    if (process.env.NATIVE_TEST_SPARSE_DMG_BYTES) {
      fs.truncateSync(path.join(args[1], 'Contents/MacOS/monocode-gpui'), Number(process.env.NATIVE_TEST_SPARSE_DMG_BYTES));
    }
  }
}
if (name === 'tar') fs.writeFileSync(args[1], 'archive');
if (name === 'hdiutil') {
  fs.writeFileSync(process.env.NATIVE_TEST_HDIUTIL_ARGS, JSON.stringify(args));
  fs.writeFileSync(args.at(-1), 'dmg');
}
if (name === 'xcrun' && args[0] === 'notarytool') console.log(JSON.stringify({ status: process.env.NATIVE_TEST_NOTARY_STATUS || 'Accepted' }));
if (name === 'xcrun' && args[0] === 'stapler' && process.env.NATIVE_TEST_STAPLER_MODE) {
  const state = process.env.NATIVE_TEST_STAPLER_STATE;
  const calls = Number(fs.readFileSync(state, 'utf8')) + 1;
  fs.writeFileSync(state, String(calls));
  const mode = process.env.NATIVE_TEST_STAPLER_MODE;
  if (mode === 'invalid') process.exit(65);
  if (mode === 'offline' || calls < 3) process.exit(68);
}
`;
    for (const name of [
      "uname",
      "lipo",
      "codesign",
      "spctl",
      "ditto",
      "tar",
      "hdiutil",
      "xcrun",
    ]) {
      await writeFile(join(bin, name), fakeTool);
      await chmod(join(bin, name), 0o755);
    }
    const overrides = {
      PATH: `${bin}${delimiter}${process.env.PATH}`,
      APPLE_SIGNING_IDENTITY: "Developer ID Application: Fixture (TESTTEAM)",
      APPLE_API_KEY_PATH: join(directory, "fixture.p8"),
      APPLE_API_KEY: "FIXTURE",
      APPLE_API_ISSUER: "fixture-issuer",
      NATIVE_TEST_STAPLER_MODE: "",
      NATIVE_TEST_STAPLER_STATE: join(directory, "stapler-state"),
      // A sparse executable exposes image estimates based on allocated blocks.
      NATIVE_TEST_SPARSE_DMG_BYTES: String(128 * 1024 * 1024),
      NATIVE_TEST_HDIUTIL_ARGS: join(directory, "hdiutil-args.json"),
    };
    const original = Object.fromEntries(
      Object.keys(overrides).map((key) => [key, process.env[key]]),
    );
    t.after(() => {
      for (const [key, value] of Object.entries(original)) {
        if (value === undefined) delete process.env[key];
        else process.env[key] = value;
      }
      delete process.env.NATIVE_TEST_NOTARY_STATUS;
    });
    Object.assign(process.env, overrides);
    const out = join(directory, "output");
    const artifacts = await packageNative({
      kind: "macos",
      binary,
      out,
      version: "0.16.0-preview.2",
      arch: "aarch64",
    });
    assert.equal(artifacts.length, 2);
    assert.ok(
      artifacts.some(
        (a) =>
          a.target === "darwin-aarch64-app" &&
          a.filename.endsWith(".app.tar.gz"),
      ),
    );
    assert.ok(artifacts.some((a) => !a.target && a.filename.endsWith(".dmg")));
    for (const artifact of artifacts)
      await access(join(out, artifact.filename));
    const imageArgs = JSON.parse(
      await readFile(process.env.NATIVE_TEST_HDIUTIL_ARGS, "utf8"),
    );
    assert.equal(imageArgs[imageArgs.indexOf("-fs") + 1], "HFS+");
    const imageSize = imageArgs[imageArgs.indexOf("-size") + 1];
    assert.match(imageSize, /^\d+m$/);
    assert.ok(
      parseInt(imageSize, 10) >= 192,
      "capacity covers logical bytes and filesystem headroom",
    );
    assert.ok(
      parseInt(imageSize, 10) < 256,
      "Applications symlink is not traversed",
    );
    const plist = await readFile(
      join(release, "Monocode Native.app/Contents/Info.plist"),
      "utf8",
    );
    assert.ok(
      plist.includes(
        "<key>CFBundleShortVersionString</key><string>0.16.0</string>",
      ),
    );
    assert.ok(plist.includes("<key>CFBundleVersion</key><string>2</string>"));
    assert.ok(
      plist.includes(
        "<key>MonocodeNativeVersion</key><string>0.16.0-preview.2</string>",
      ),
    );
    assert.ok(plist.includes("<string>dev.usemono.gpui</string>"));
    for (const action of ["staple", "validate"]) {
      await t.test(
        `${action} recovers from ticket-service failures`,
        async () => {
          process.env.NATIVE_TEST_STAPLER_MODE = "recover";
          await writeFile(process.env.NATIVE_TEST_STAPLER_STATE, "0");
          const delays = [];
          await runStapler(action, release, {
            wait: async (ms) => delays.push(ms),
          });
          assert.deepEqual(delays, [10_000, 20_000]);
          assert.equal(
            await readFile(process.env.NATIVE_TEST_STAPLER_STATE, "utf8"),
            "3",
          );
        },
      );
    }
    await t.test(
      "permanent ticket validation failures stop immediately",
      async () => {
        process.env.NATIVE_TEST_STAPLER_MODE = "invalid";
        await writeFile(process.env.NATIVE_TEST_STAPLER_STATE, "0");
        const delays = [];
        await assert.rejects(
          () =>
            runStapler("validate", release, {
              wait: async (ms) => delays.push(ms),
            }),
          /xcrun failed \(65\)/,
        );
        assert.deepEqual(delays, []);
        assert.equal(
          await readFile(process.env.NATIVE_TEST_STAPLER_STATE, "utf8"),
          "1",
        );
      },
    );
    await t.test(
      "persistent ticket-service failures stop after five attempts",
      async () => {
        process.env.NATIVE_TEST_STAPLER_MODE = "offline";
        await writeFile(process.env.NATIVE_TEST_STAPLER_STATE, "0");
        const delays = [];
        await assert.rejects(
          () =>
            runStapler("staple", release, {
              wait: async (ms) => delays.push(ms),
            }),
          /xcrun failed \(68\)/,
        );
        assert.deepEqual(delays, [10_000, 20_000, 30_000, 40_000]);
        assert.equal(
          await readFile(process.env.NATIVE_TEST_STAPLER_STATE, "utf8"),
          "5",
        );
      },
    );
    process.env.NATIVE_TEST_STAPLER_MODE = "";
    process.env.NATIVE_TEST_NOTARY_STATUS = "Invalid";
    await assert.rejects(
      () =>
        packageNative({
          kind: "macos",
          binary,
          out: join(directory, "mismatched"),
          version: "0.16.0-preview.3",
          arch: "aarch64",
        }),
      /executable version does not match/,
    );
    await writeFile(
      binary,
      "#!/bin/sh\nprintf 'Monocode Native 0.16.0-preview.3\\n'\n",
    );
    await assert.rejects(
      () =>
        packageNative({
          kind: "macos",
          binary,
          out: join(directory, "rejected"),
          version: "0.16.0-preview.3",
          arch: "aarch64",
        }),
      /Apple did not accept/,
    );
  },
);
