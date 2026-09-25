import { createHash } from "node:crypto";
import {
  mkdir,
  readFile,
  writeFile,
  chmod,
  copyFile,
  rm,
} from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { resolve, join } from "node:path";

// Official Node release checksums, pinned so release builds are reproducible.
// https://nodejs.org/dist/v24.21.0/SHASUMS256.txt
const nodeVersion = "24.21.0";
const runtimes = {
  "darwin-arm64":
    "bed7eea5325e1108f32ce5228ddd6a5f0f08a499ee42aa7442aea583702f6057",
  "darwin-x64":
    "1462cb3b3046b815cf8ea436d3da450ec1a9f11dac7e5a46b0ada5305d7e8097",
  "linux-arm64":
    "724282c3b43aec998aa9527380465b45d229e021b58035f5f4f63095eabfe5d5",
  "linux-x64":
    "6e1db87ef58b8819e5d5402eff1536491b18edd8eb7bee5ef7897876e88dc5ff",
  "win32-x64":
    "158f7685b44de51f6c0df1d153526cbcd3e1bc739a8dfc607721cef75de9e541",
  "win32-arm64":
    "8779b1bde1d39f8d420e3b57aa657b39891af434d3de44a919044cec06785921",
};
const psQuote = (value) => `'${value.replaceAll("'", "''")}'`;
const ps = (script) =>
  execFileSync("powershell.exe", [
    "-NoProfile",
    "-NonInteractive",
    "-EncodedCommand",
    Buffer.from(
      `$ErrorActionPreference = 'Stop'; ${script}`,
      "utf16le",
    ).toString("base64"),
  ]);
const args = process.argv.slice(2);
const target = args.includes("--target")
  ? args[args.indexOf("--target") + 1]
  : `${process.platform}-${process.arch}`;
const targets = args.includes("--all") ? Object.keys(runtimes) : [target];
const output = resolve("build/host-packages");
const cache = resolve("build/host-runtime-cache");
await mkdir(output, { recursive: true });
await mkdir(cache, { recursive: true });
const { version } = JSON.parse(await readFile("package.json", "utf8"));
for (const target of targets) {
  if (!(target in runtimes))
    throw new Error(`Unsupported host target: ${target}`);
  const windows = target.startsWith("win32-");
  const extension = windows ? "zip" : "tar.gz";
  const stem = `node-v${nodeVersion}-${target.replace("win32-", "win-")}`;
  const archive = join(cache, `${stem}.${extension}`);
  let bytes;
  try {
    bytes = await readFile(archive);
  } catch {
    /* first download */
  }
  if (
    !bytes ||
    createHash("sha256").update(bytes).digest("hex") !== runtimes[target]
  ) {
    const response = await fetch(
      `https://nodejs.org/dist/v${nodeVersion}/${stem}.${extension}`,
      { signal: AbortSignal.timeout(120_000) },
    );
    if (!response.ok)
      throw new Error(`Runtime download failed: HTTP ${response.status}`);
    bytes = Buffer.from(await response.arrayBuffer());
    if (createHash("sha256").update(bytes).digest("hex") !== runtimes[target])
      throw new Error("Node runtime checksum mismatch");
    await writeFile(archive, bytes);
  }
  const folder = join(output, target);
  await rm(folder, { recursive: true, force: true });
  await mkdir(folder, { recursive: true });
  if (windows) {
    if (process.platform === "win32") {
      const unpacked = join(folder, "runtime");
      ps(
        `Expand-Archive -LiteralPath ${psQuote(archive)} -DestinationPath ${psQuote(unpacked)}`,
      );
      await copyFile(
        join(unpacked, stem, "node.exe"),
        join(folder, "node.exe"),
      );
      await copyFile(join(unpacked, stem, "LICENSE"), join(folder, "LICENSE"));
      await rm(unpacked, { recursive: true, force: true });
    } else {
      execFileSync("unzip", [
        "-q",
        "-j",
        archive,
        `${stem}/node.exe`,
        `${stem}/LICENSE`,
        "-d",
        folder,
      ]);
    }
  } else
    execFileSync("tar", [
      "-xzf",
      archive,
      "-C",
      folder,
      "--strip-components=1",
      `${stem}/bin/node`,
      `${stem}/LICENSE`,
    ]);
  await copyFile("build/host/monocode-host.mjs", join(folder, "host.mjs"));
  await copyFile("LICENSE", join(folder, "MONOCODE-LICENSE"));
  await writeFile(
    join(folder, "version.json"),
    JSON.stringify({ version, nodeVersion, target }),
  );
  if (windows) {
    await writeFile(
      join(folder, "monocode-host.cmd"),
      '@echo off\r\nsetlocal DisableDelayedExpansion\r\n"%~dp0node.exe" "%~dp0host.mjs" %*\r\nexit /b %errorlevel%\r\n',
    );
  } else {
    await writeFile(
      join(folder, "monocode-host"),
      '#!/bin/sh\nset -eu\nDIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)\nexec "$DIR/bin/node" "$DIR/host.mjs" "$@"\n',
    );
    await chmod(join(folder, "monocode-host"), 0o755);
  }
  if (target === `${process.platform}-${process.arch}`) {
    const executable = windows
      ? join(folder, "node.exe")
      : join(folder, "bin/node");
    const actual = execFileSync(
      executable,
      [join(folder, "host.mjs"), "--version"],
      {
        encoding: "utf8",
      },
    ).trim();
    if (actual !== version)
      throw new Error("Packaged host failed its executable smoke test");
  }
  const filename = `monocode-host-${target}.${extension}`;
  await rm(join(output, filename), { force: true });
  if (windows) {
    if (process.platform === "win32")
      ps(
        `Compress-Archive -Path ${psQuote(join(folder, "*"))} -DestinationPath ${psQuote(join(output, filename))}`,
      );
    else
      execFileSync("zip", ["-q", "-r", join(output, filename), "."], {
        cwd: folder,
      });
  } else
    execFileSync("tar", ["-czf", join(output, filename), "-C", folder, "."]);
  const hash = createHash("sha256")
    .update(await readFile(join(output, filename)))
    .digest("hex");
  await writeFile(join(output, `${filename}.sha256`), `${hash}  ${filename}\n`);
  console.log(`${filename} (${version}, Node ${nodeVersion})`);
}
