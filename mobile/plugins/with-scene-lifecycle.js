const { withAppDelegate, withInfoPlist } = require("expo/config-plugins");

// iOS 27 traps at launch unless the app adopts the UIScene life cycle. Expo
// ships the scene delegate, but the prebuild template still starts React
// Native from the app delegate, so point UIKit at ExpoAppSceneDelegate and
// let it create the window instead.
module.exports = function withSceneLifecycle(config) {
  config = withInfoPlist(config, (config) => {
    config.modResults.UIApplicationSceneManifest = {
      UIApplicationSupportsMultipleScenes: false,
      UISceneConfigurations: {
        UIWindowSceneSessionRoleApplication: [
          {
            UISceneConfigurationName: "Default Configuration",
            UISceneDelegateClassName: "EXExpoAppSceneDelegate",
          },
        ],
      },
    };
    return config;
  });

  return withAppDelegate(config, (config) => {
    if (config.modResults.language !== "swift")
      throw new Error("with-scene-lifecycle expects a Swift AppDelegate");
    let source = config.modResults.contents;
    const replace = (pattern, value) => {
      if (!pattern.test(source))
        throw new Error(
          `with-scene-lifecycle couldn't find ${pattern} in AppDelegate.swift`,
        );
      source = source.replace(pattern, value);
    };
    if (!source.includes("ExpoReactNativeFactoryProvider"))
      replace(
        /class AppDelegate: ExpoAppDelegate \{/,
        "class AppDelegate: ExpoAppDelegate, ExpoReactNativeFactoryProvider {",
      );
    // The scene delegate creates the window and starts React Native into it.
    if (source.includes("factory.startReactNative("))
      replace(
        /\n#if os\(iOS\) \|\| os\(tvOS\)\n\s*window = UIWindow\(frame: UIScreen\.main\.bounds\)\n\s*factory\.startReactNative\([\s\S]*?\)\n#endif\n/,
        "",
      );
    config.modResults.contents = source;
    return config;
  });
};
