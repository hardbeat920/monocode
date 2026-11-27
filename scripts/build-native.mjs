import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const env = {
  ...process.env,
  CARGO_BUILD_JOBS: process.env.CARGO_BUILD_JOBS || "1",
};

function run(command, args, capture = false) {
  const result = spawnSync(command, args, {
    cwd: root,
    env,
    encoding: "utf8",
    stdio: capture ? ["ignore", "pipe", "inherit"] : "inherit",
  });
  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
  return result.stdout;
}

const metadata = JSON.parse(
  run("cargo", ["metadata", "--manifest-path", "crates/monocode-gpui/Cargo.toml", "--locked", "--no-deps", "--format-version", "1"], true),
);
const app = metadata.packages.find((pkg) => pkg.name === "monocode-gpui");
if (!app) throw new Error("The monocode-gpui package was not found.");

run("cargo", ["build", "--manifest-path", "crates/monocode-gpui/Cargo.toml", "--locked", "-p", app.name, "--release"]);
const target = process.env.CARGO_BUILD_TARGET
  ? join(metadata.target_directory, process.env.CARGO_BUILD_TARGET)
  : metadata.target_directory;
if (process.platform === "darwin") {
  run("bash", ["scripts/bundle-native-macos.sh", "release", target, app.version, "monocode-data"]);
} else {
  const executable = process.platform === "win32" ? "monocode-gpui.exe" : "monocode-gpui";
  console.log(`Executable: ${join(target, "release", executable)}`);
}
