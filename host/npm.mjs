// Assembles the `monocode-host` npm package from build/host, so a machine
// with Node.js runs `npx monocode-host connect`. Run `npm run host:npm`; the
// tarball lands in build/. Publishing is a separate, manual step:
// `npm publish build/host-npm`.
import { execFileSync } from "node:child_process";
import { copyFile, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";

export const HOST_PACKAGE = "monocode-host";
const output = "build/host-npm";
const { version } = JSON.parse(await readFile("package.json", "utf8"));

await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
for (const file of ["monocode-host.mjs", "monocode-host.mjs.map", "provider-guard.mjs"])
  await copyFile(join("build/host", file), join(output, file));
await copyFile("LICENSE", join(output, "LICENSE"));
await writeFile(
  join(output, "package.json"),
  `${JSON.stringify(
    {
      name: HOST_PACKAGE,
      version,
      description:
        "Run Codex and Claude Code sessions on this machine and pair it with the MonoCode desktop app.",
      type: "module",
      bin: { [HOST_PACKAGE]: "monocode-host.mjs" },
      files: [
        "monocode-host.mjs",
        "monocode-host.mjs.map",
        "provider-guard.mjs",
        "README.md",
        "LICENSE",
      ],
      engines: { node: ">=22.13" },
      os: ["darwin", "linux", "win32"],
      license: "MIT",
      repository: {
        type: "git",
        url: "git+https://github.com/zaesho/monocode.git",
        directory: "host",
      },
      keywords: ["monocode", "codex", "claude-code", "remote", "agents"],
    },
    null,
    2,
  )}\n`,
);
await writeFile(
  join(output, "README.md"),
  `# ${HOST_PACKAGE}

Runs Codex and Claude Code sessions on this machine for the MonoCode desktop app.
Your laptop can close; the agents keep working here.

\`\`\`sh
npx ${HOST_PACKAGE}@${version} connect
\`\`\`

\`connect\` copies the host into \`~/.monocode-host\`, installs it as a login service
(systemd user service on Linux, a launch agent on macOS, Task Scheduler on Windows),
listens on port 3774 over TLS, and prints a one-time pairing link. In MonoCode, open
**Settings → Connections → Pair machine** and paste the link.

Install and sign in to Codex or Claude Code on this machine first, as the same user.
Requires Node.js 22.13 or newer.

| Command | What it does |
| --- | --- |
| \`connect\` | Install or update the host, turn on network access, print a pairing link |
| \`connect pair\` | Print a new one-time pairing link |
| \`connect status\` | Show the host version, addresses, and paired desktops |
| \`connect disable\` | Listen on loopback only; SSH pairing keeps working |
| \`devices\`, \`revoke <id>\` | List or revoke paired desktops |
| \`service uninstall\` | Stop the host and remove the login service; data is kept |

Run \`npx ${HOST_PACKAGE} --help\` for every option.
`,
);

const actual = execFileSync(process.execPath, [join(output, "monocode-host.mjs"), "--version"], {
  encoding: "utf8",
}).trim();
if (actual !== version) throw new Error("The packaged host failed its smoke test");
const packed = execFileSync("npm", ["pack", "--pack-destination", "..", "--silent"], {
  cwd: output,
  encoding: "utf8",
  shell: process.platform === "win32",
})
  .trim()
  .split("\n")
  .at(-1);
console.log(`build/${packed}`);
