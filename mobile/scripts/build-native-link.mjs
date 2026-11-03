import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  copyFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve, join } from "node:path";

const mobile = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = resolve(mobile, "..");
const platform = process.argv[2] ?? "ios";
const run = (cmd, args, env = {}) =>
  execFileSync(cmd, args, {
    cwd: root,
    stdio: "inherit",
    env: { ...process.env, ...env },
  });
const installed = execFileSync("rustup", ["target", "list", "--installed"], {
  encoding: "utf8",
});
const build = (target, env = {}) => {
  if (!installed.split(/\s+/).includes(target))
    run("rustup", ["target", "add", target]);
  run(
    "cargo",
    [
      "build",
      "--locked",
      "--release",
      "-p",
      "monocode-mobile-link",
      "--target",
      target,
    ],
    env,
  );
};
if (platform === "ios") {
  if (process.platform !== "darwin")
    throw new Error(
      "Build the iOS native library on macOS with Xcode installed.",
    );
  const targets = [
    "aarch64-apple-ios",
    ...(process.argv.includes("--device-only")
      ? []
      : ["aarch64-apple-ios-sim", "x86_64-apple-ios"]),
  ];
  for (const target of targets)
    build(target, { IPHONEOS_DEPLOYMENT_TARGET: "16.4" });
  const artifacts = join(root, "target/mobile-link-frameworks");
  const framework = (variant, simulator) => {
    const directory = join(artifacts, variant, "MonocodeLinkCore.framework");
    rmSync(directory, { recursive: true, force: true });
    mkdirSync(join(directory, "Headers"), { recursive: true });
    mkdirSync(join(directory, "Modules"), { recursive: true });
    copyFileSync(
      join(mobile, "modules/monocode-link/ios/MonocodeLink.h"),
      join(directory, "Headers/MonocodeLink.h"),
    );
    writeFileSync(
      join(directory, "Modules/module.modulemap"),
      'framework module MonocodeLinkCore { umbrella header "MonocodeLink.h" export * }\n',
    );
    writeFileSync(
      join(directory, "Info.plist"),
      `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.monocode.link.core</string>
<key>CFBundleName</key><string>MonocodeLinkCore</string>
<key>CFBundleExecutable</key><string>MonocodeLinkCore</string>
<key>CFBundlePackageType</key><string>FMWK</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>MinimumOSVersion</key><string>16.4</string>
<key>CFBundleSupportedPlatforms</key><array><string>${simulator ? "iPhoneSimulator" : "iPhoneOS"}</string></array>
</dict></plist>`,
    );
    return directory;
  };
  const device = framework("device", false);
  copyFileSync(
    join(root, "target", targets[0], "release/libmonocode_mobile_link.a"),
    join(device, "MonocodeLinkCore"),
  );
  const frameworks = ["-framework", device];
  if (targets.length > 1) {
    const simulator = framework("simulator", true);
    run("xcrun", [
      "lipo",
      "-create",
      ...targets
        .slice(1)
        .map((target) =>
          join(root, "target", target, "release/libmonocode_mobile_link.a"),
        ),
      "-output",
      join(simulator, "MonocodeLinkCore"),
    ]);
    frameworks.push("-framework", simulator);
  }
  const output = join(
    mobile,
    "modules/monocode-link/ios/MonocodeLinkCore.xcframework",
  );
  rmSync(output, { recursive: true, force: true });
  run("xcodebuild", ["-create-xcframework", ...frameworks, "-output", output]);
} else if (platform === "android") {
  const sdk = process.env.ANDROID_HOME ?? process.env.ANDROID_SDK_ROOT;
  const ndkRoot =
    process.env.ANDROID_NDK_HOME ??
    (sdk && join(sdk, "ndk", readdirSync(join(sdk, "ndk")).sort().at(-1)));
  if (!ndkRoot || !existsSync(ndkRoot))
    throw new Error("Set ANDROID_NDK_HOME to an installed Android NDK.");
  const host = process.platform === "darwin" ? "darwin-x86_64" : "linux-x86_64";
  const bin = join(ndkRoot, "toolchains/llvm/prebuilt", host, "bin");
  for (const [target, abi, clang] of [
    ["aarch64-linux-android", "arm64-v8a", "aarch64-linux-android"],
    ["armv7-linux-androideabi", "armeabi-v7a", "armv7a-linux-androideabi"],
    ["x86_64-linux-android", "x86_64", "x86_64-linux-android"],
    ["i686-linux-android", "x86", "i686-linux-android"],
  ]) {
    const linker = join(bin, `${clang}24-clang`);
    build(target, {
      [`CARGO_TARGET_${target.toUpperCase().replaceAll("-", "_")}_LINKER`]:
        linker,
      CC: linker,
      AR: join(bin, "llvm-ar"),
    });
    const destination = join(
      mobile,
      "modules/monocode-link/android/src/main/jniLibs",
      abi,
    );
    mkdirSync(destination, { recursive: true });
    copyFileSync(
      join(root, "target", target, "release/libmonocode_mobile_link.so"),
      join(destination, "libmonocode_mobile_link.so"),
    );
  }
} else throw new Error("Choose ios or android.");
