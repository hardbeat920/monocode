#!/usr/bin/env node
// Installs this checkout's MonoCode Host on an SSH machine. The desktop's SSH
// setup runs `npx monocode-host@<version> connect`, which needs the version on
// npm. This packs the package locally, uploads it, and runs the desktop's own
// connect script with the uploaded tarball as the package. It restarts a
// running host, interrupting its turns, so the new build takes effect.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const target = process.argv[2];
if (!target || target.startsWith("-")) {
  console.error("usage: npm run host:install -- <ssh-target>");
  process.exit(1);
}

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

const ssh = (args, input, stderr = "inherit") =>
  execFileSync("ssh", [target, ...args], {
    input,
    encoding: "utf8",
    stdio: [input === undefined ? "inherit" : "pipe", "pipe", stderr],
  });

const powershellEncoded = (script) =>
  Buffer.from(script, "utf16le").toString("base64");

// Matches powershell_reader() in src-tauri/src/remote_ssh.rs.
const powershellReader = powershellEncoded(
  "$env:PSModulePath = $PSHOME + '\\Modules;' + $env:PSModulePath; $ErrorActionPreference = 'Stop'; [Console]::InputEncoding = [Text.UTF8Encoding]::new($false); [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); try { & ([ScriptBlock]::Create([Console]::In.ReadToEnd())) } catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }",
);

// Windows PowerShell wraps progress on a redirected stderr in CLIXML; keep the
// text only, as powershellErrorText() in host/windows.ts does.
function runPowershell(script) {
  try {
    return ssh(
      [
        "powershell.exe",
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        powershellReader,
      ],
      script,
      "pipe",
    );
  } catch (error) {
    const text = String(error.stderr ?? "")
      .replace(/#< CLIXML\r?\n?/g, "")
      .replace(/<Objs [\s\S]*?<\/Objs>/g, "")
      .trim();
    throw new Error(text || error.message);
  }
}

function isWindows() {
  try {
    ssh(["uname -s"], undefined, "ignore");
    return false;
  } catch {
    // Windows shells have no uname.
    return true;
  }
}

function replaceOnce(text, search, replacement) {
  if (!text.includes(search))
    throw new Error(`The connect script no longer has ${search}`);
  return text.replace(search, () => replacement);
}

// The desktop's script, with the package pointing at the upload in the
// remote home directory. `--restart` replaces a running build of the same
// version; `--yes` allows interrupting its turns.
function connectScript(windows, file) {
  if (windows) {
    const script = readFileSync(
      join(root, "src-tauri/src/remote_connect.ps1"),
      "utf8",
    );
    return replaceOnce(
      replaceOnce(
        script,
        "$package = @@PACKAGE@@",
        `$package = Join-Path ([Environment]::GetFolderPath('UserProfile')) '${file}'`,
      ),
      "@@FLAGS@@",
      " --restart --yes",
    );
  }
  const script = readFileSync(
    join(root, "src-tauri/src/remote_connect.sh"),
    "utf8",
  );
  return replaceOnce(
    replaceOnce(script, "PACKAGE=@@PACKAGE@@", `PACKAGE="$HOME/${file}"`),
    "@@FLAGS@@",
    " --restart --yes",
  );
}

try {
  install();
} catch (error) {
  console.error(error.message);
  process.exit(1);
}

function install() {
  const windows = isWindows();
  console.log(`Packing MonoCode Host for ${target}`);
  const packed = execFileSync("npm", ["run", "--silent", "host:npm"], {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
    shell: process.platform === "win32",
  })
    .trim()
    .split("\n")
    .at(-1);
  const file = basename(packed);
  execFileSync("scp", ["-q", join(root, packed), `${target}:`], {
    stdio: "inherit",
  });
  try {
    const script = connectScript(windows, file);
    const output = windows
      ? runPowershell(script)
      : ssh(["sh", "-l", "-s"], script);
    const line = output
      .trim()
      .split("\n")
      .reverse()
      .find((entry) => entry.trim().startsWith("{"));
    if (!line) throw new Error(`connect printed no result:\n${output}`);
    const result = JSON.parse(line);
    console.log(
      [
        `Host ${result.version} is running on ${target} (${result.service}, port ${result.port}).`,
        ...(result.endpoints.length
          ? ["Addresses:", ...result.endpoints.map((entry) => `  ${entry}`)]
          : ["No network address; pair it with SSH setup in MonoCode."]),
        "",
        "Pair a desktop with this link (Settings → Connections → Add machine):",
        `  ${result.link}`,
      ].join("\n"),
    );
  } finally {
    if (windows) {
      runPowershell(
        `Remove-Item -Force -ErrorAction SilentlyContinue -LiteralPath (Join-Path ([Environment]::GetFolderPath('UserProfile')) '${file}')`,
      );
    } else {
      ssh([`rm -f "$HOME/${file}"`]);
    }
  }
}
