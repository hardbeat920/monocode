import { execFileSync } from "node:child_process";
import { build } from "esbuild";
import { copyFile } from "node:fs/promises";

await build({
  entryPoints: ["host/cli.ts"],
  outfile: "build/host/monocode-host.mjs",
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node24",
  loader: { ".ps1": "text" },
  define: { "import.meta.hot": "undefined" },
  sourcemap: true,
});
await copyFile("host/provider-guard.mjs", "build/host/provider-guard.mjs");

if (process.platform === "darwin") {
  execFileSync("cargo", ["build", "--locked", "--release", "-p", "monocode-isolation"], { stdio: "inherit" });
  await copyFile("target/release/monocode-isolation", "build/host/monocode-isolation");
}
