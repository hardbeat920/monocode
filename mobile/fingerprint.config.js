/** @type {import('@expo/fingerprint').Config} */
module.exports = {
  // Native folders and compiled libraries are regenerated on EAS. Hash their
  // source inputs so local OTA publishing matches a clean cloud build.
  ignorePaths: [
    "ios",
    "ios/**/*",
    "android",
    "android/**/*",
    "modules/monocode-link/ios/*.xcframework",
    "modules/monocode-link/ios/*.xcframework/**/*",
    "modules/monocode-link/android/src/main/jniLibs",
    "modules/monocode-link/android/src/main/jniLibs/**/*",
  ],
  extraSources: [
    { type: "file", filePath: "../Cargo.toml", reasons: ["monocodeRust"] },
    { type: "file", filePath: "../Cargo.lock", reasons: ["monocodeRust"] },
    {
      type: "dir",
      filePath: "../crates/mobile-link",
      reasons: ["monocodeRust"],
    },
    {
      type: "file",
      filePath: "scripts/build-native-link.mjs",
      reasons: ["monocodeRust"],
    },
    {
      type: "file",
      filePath: "scripts/eas-build-pre-install.mjs",
      reasons: ["monocodeRust"],
    },
  ],
};
