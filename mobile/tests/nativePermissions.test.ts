// @vitest-environment node

import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { beforeAll, expect, it } from "vitest";

let ios: {
  infoPlist: {
    NSMicrophoneUsageDescription?: string;
    NSCameraUsageDescription?: string;
    UIBackgroundModes?: string[];
  };
  entitlements: Record<string, unknown>;
};
let android: {
  manifest: {
    manifest: {
      "uses-permission": { $: { "android:name": string; "tools:node"?: string } }[];
      application: { service?: { $: { "android:name": string } }[] }[];
    };
  };
};

beforeAll(() => {
  const output = execFileSync(
    process.execPath,
    [
      fileURLToPath(new URL("../node_modules/expo/bin/cli", import.meta.url)),
      "config",
      "--type",
      "introspect",
      "--json",
    ],
    {
      cwd: fileURLToPath(new URL("..", import.meta.url)),
      encoding: "utf8",
      env: { ...process.env, EXPO_NO_DOTENV: "1" },
      timeout: 25_000,
    },
  );
  const mods = JSON.parse(output)._internal.modResults;
  ios = mods.ios;
  android = mods.android;
}, 30_000);

it("keeps a microphone purpose string after all Expo plugins run", () => {
  // Plugins share the same Info.plist: disabling microphone access in a
  // camera or image-picker plugin can erase the audio plugin's description.
  const description = ios.infoPlist.NSMicrophoneUsageDescription;
  // iOS terminates the app if it requests microphone access without this.
  expect(description).toEqual(expect.any(String));
  expect(description?.trim().length).toBeGreaterThan(0);
});

it("keeps local session notifications compatible with signing profiles without APNs", () => {
  expect(ios.entitlements).not.toHaveProperty("aps-environment");
});

it("keeps camera access for pairing after the image-picker plugin runs", () => {
  expect(ios.infoPlist.NSCameraUsageDescription).toEqual(expect.any(String));
  expect(android.manifest.manifest["uses-permission"]).toContainEqual({
    $: { "android:name": "android.permission.CAMERA" },
  });
});

it("removes unused Android microphone, storage and audio foreground-service permissions", () => {
  const permissions = android.manifest.manifest["uses-permission"];
  for (const name of [
    "RECORD_AUDIO",
    "READ_EXTERNAL_STORAGE",
    "WRITE_EXTERNAL_STORAGE",
    "FOREGROUND_SERVICE",
    "FOREGROUND_SERVICE_MEDIA_PLAYBACK",
  ]) {
    expect(permissions).toContainEqual({
      $: { "android:name": `android.permission.${name}`, "tools:node": "remove" },
    });
  }
  const services = android.manifest.manifest.application[0].service ?? [];
  expect(services.map((service) => service.$["android:name"])).not.toContain(
    "expo.modules.audio.service.AudioControlsService",
  );
  expect(ios.infoPlist.UIBackgroundModes ?? []).not.toContain("audio");
});
