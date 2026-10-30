// Adds the Live Activity widget extension to the generated Xcode project. Its
// Swift stays in targets/live-activity and compiles from there, so editing it
// needs no prebuild. targets/_shared builds into both the app and the widget,
// as the widget's Approve button runs an intent the app performs.
const { withXcodeProject } = require("expo/config-plugins");
const path = require("node:path");

const PBXFile = require(
  require.resolve("xcode/lib/pbxFile", {
    paths: [require.resolve("expo/config-plugins")],
  }),
);

const TARGET = "LiveActivity";
// Relative to ios/, where the project lives; _shared sits beside it.
const WIDGET = "../targets/live-activity";
const WIDGET_SOURCES = ["index.swift", "SessionActivity.swift", "MonocodeActivityAttributes.swift"];
const SHARED_SOURCES = ["SessionDecisionIntent.swift"];

const quote = (value) => `"${value}"`;

/** A file in `group`, compiled by each of `phases`. */
function addSource(project, group, file, phases) {
  const ref = new PBXFile(file);
  ref.fileRef = project.generateUuid();
  project.addToPbxFileReferenceSection(ref);
  project.getPBXGroupByKey(group).children.push({ value: ref.fileRef, comment: ref.basename });
  for (const phase of phases) {
    const build = new PBXFile(file);
    build.fileRef = ref.fileRef;
    build.uuid = project.generateUuid();
    build.group = "Sources";
    project.addToPbxBuildFileSection(build);
    phase.files.push({ value: build.uuid, comment: `${build.basename} in Sources` });
  }
}

module.exports = function withLiveActivity(config) {
  return withXcodeProject(config, (config) => {
    const project = config.modResults;
    if (project.pbxTargetByName(TARGET)) return config;

    const app = project.getFirstTarget();
    const bundleId = `${config.ios?.bundleIdentifier}.live-activity`;
    // Also embeds the widget in the app and makes the app depend on it.
    const target = project.addTarget(TARGET, "app_extension", TARGET, bundleId);

    const group = project.pbxCreateGroup(TARGET, quote(WIDGET));
    const mainGroup = project.getFirstProject().firstProject.mainGroup;
    project.getPBXGroupByKey(mainGroup).children.push({ value: group, comment: TARGET });

    const widgetSources = project.addBuildPhase([], "PBXSourcesBuildPhase", "Sources", target.uuid).buildPhase;
    project.addBuildPhase([], "PBXResourcesBuildPhase", "Resources", target.uuid);
    project.addBuildPhase([], "PBXFrameworksBuildPhase", "Frameworks", target.uuid);
    const appSources = project.pbxSourcesBuildPhaseObj(app.uuid);
    for (const file of WIDGET_SOURCES) addSource(project, group, file, [widgetSources]);
    for (const file of SHARED_SOURCES) {
      addSource(project, group, path.posix.join("..", "_shared", file), [widgetSources, appSources]);
    }
    project.addToPbxGroup(new PBXFile("Info.plist"), group);

    const settings = {
      CODE_SIGN_STYLE: "Automatic",
      CURRENT_PROJECT_VERSION: quote(config.ios?.buildNumber ?? "1"),
      GENERATE_INFOPLIST_FILE: "YES",
      INFOPLIST_FILE: quote(`${WIDGET}/Info.plist`),
      INFOPLIST_KEY_CFBundleDisplayName: quote(config.name),
      IPHONEOS_DEPLOYMENT_TARGET: "17.0",
      LD_RUNPATH_SEARCH_PATHS: quote("$(inherited) @executable_path/Frameworks @executable_path/../../Frameworks"),
      MARKETING_VERSION: quote(config.version ?? "1.0.0"),
      PRODUCT_BUNDLE_IDENTIFIER: quote(bundleId),
      PRODUCT_NAME: quote("$(TARGET_NAME)"),
      SKIP_INSTALL: "YES",
      SWIFT_VERSION: "5.0",
      TARGETED_DEVICE_FAMILY: quote("1,2"),
      ...(config.ios?.appleTeamId ? { DEVELOPMENT_TEAM: config.ios.appleTeamId } : {}),
    };
    const lists = project.pbxXCConfigurationList();
    const configurations = project.pbxXCBuildConfigurationSection();
    for (const { value } of lists[target.pbxNativeTarget.buildConfigurationList].buildConfigurations) {
      const configuration = configurations[value];
      configuration.buildSettings = {
        ...settings,
        ...(configuration.name === "Debug"
          ? { SWIFT_ACTIVE_COMPILATION_CONDITIONS: quote("DEBUG $(inherited)") }
          : {}),
      };
    }
    return config;
  });
};
