import { execFileSync, spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const platform = process.env.EAS_BUILD_PLATFORM;
if (platform !== "ios" && platform !== "android") {
  console.log(
    "Skipping native transport build outside an EAS iOS/Android build.",
  );
  process.exit(0);
}

const mobile = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const cargoBin = join(
  process.env.CARGO_HOME ?? join(homedir(), ".cargo"),
  "bin",
);
const env = {
  ...process.env,
  PATH: `${cargoBin}:${process.env.PATH ?? ""}`,
  // Match the toolchain used to validate the native library locally.
  RUSTUP_TOOLCHAIN: "1.97.1",
};
const run = (command, args, options = {}) =>
  execFileSync(command, args, {
    cwd: mobile,
    env,
    stdio: "inherit",
    ...options,
  });

// This hook runs before npm install and CocoaPods, so use only Node built-ins.
const rustup = spawnSync("rustup", ["--version"], { env, stdio: "ignore" });
if (rustup.error?.code === "ENOENT") {
  const installer = run(
    "curl",
    [
      "--proto",
      "=https",
      "--tlsv1.2",
      "--fail",
      "--silent",
      "--show-error",
      "--location",
      "https://sh.rustup.rs",
    ],
    { stdio: ["ignore", "pipe", "inherit"] },
  );
  run(
    "sh",
    [
      "-s",
      "--",
      "-y",
      "--profile",
      "minimal",
      "--default-toolchain",
      env.RUSTUP_TOOLCHAIN,
      "--no-modify-path",
    ],
    {
      input: installer,
      stdio: ["pipe", "inherit", "inherit"],
    },
  );
} else if (rustup.error || rustup.status !== 0) {
  throw rustup.error ?? new Error(`rustup exited with status ${rustup.status}`);
} else {
  run("rustup", [
    "toolchain",
    "install",
    env.RUSTUP_TOOLCHAIN,
    "--profile",
    "minimal",
    "--no-self-update",
  ]);
}

const args = [join(mobile, "scripts/build-native-link.mjs"), platform];
// Store builds need only the device slice. Local development still builds both.
if (platform === "ios" && process.env.EAS_BUILD_PROFILE === "production") {
  args.push("--device-only");
}
run(process.execPath, args);
