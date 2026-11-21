const { withPodfile } = require("expo/config-plugins");

// FluidAudio runs NVIDIA Parakeet on the phone through Core ML. CocoaPods'
// trunk stops at an old 0.7 release, so the pod that modules/monocode-parakeet
// depends on is taken from its git tag instead.
const FLUIDAUDIO = {
  git: "https://github.com/FluidInference/FluidAudio.git",
  tag: "v0.18.4",
};
const MARKER = "# FluidAudio for modules/monocode-parakeet";

module.exports = function withFluidAudio(config) {
  return withPodfile(config, (config) => {
    const podfile = config.modResults.contents;
    if (podfile.includes(MARKER)) return config;
    const anchor = /^(\s*)use_expo_modules!.*$/m;
    if (!anchor.test(podfile)) {
      throw new Error("with-fluidaudio: use_expo_modules! not found in the Podfile.");
    }
    config.modResults.contents = podfile.replace(
      anchor,
      (line, indent) =>
        `${line}\n${indent}${MARKER}\n${indent}pod 'FluidAudio', :git => '${FLUIDAUDIO.git}', :tag => '${FLUIDAUDIO.tag}'`,
    );
    return config;
  });
};
