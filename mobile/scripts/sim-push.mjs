#!/usr/bin/env node
// Sends the desktop's pushes to a booted iOS Simulator, which takes them
// without an APNs key: `npm run push:sim -- approval`. The payload has the
// shape Expo's push service delivers (custom data under `body`).
// working and clear arrive as banners here; the desktop sends them silently.
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const BUNDLE =
  process.env.MONOCODE_IOS_BUNDLE_ID?.trim() ||
  JSON.parse(readFileSync(new URL("../app.json", import.meta.url), "utf8")).expo.ios.bundleIdentifier;
const state = process.argv[2] ?? "working";
const PROJECTS = {
  monocode: { projectPath: "/code/monocode", color: "#4fd1c5", mascot: "invader" },
  relay: { projectPath: "/code/relay", color: "#c48cff", mascot: "ghost" },
};
const session = (id, title, status, project = "monocode") => ({
  id, projectId: project, project, title, model: "Opus 5.5", status, ...PROJECTS[project],
});
const scenes = {
  working: { activity: [session("a", "Fix mobile notifications", "working")] },
  approval: {
    activity: [
      { ...session("a", "Fix mobile notifications", "approval"),
        request: { id: "demo", label: "Run cargo test -p monocode-core mobile::", count: 1 } },
      session("b", "Relay service", "working", "relay"),
    ],
    alert: { title: "Fix mobile notifications", subtitle: "monocode", body: "Needs your approval" },
  },
  question: {
    activity: [
      { ...session("b", "Relay service", "question", "relay"),
        request: { id: "demo", label: "Deploy the relay to Workers or Supabase?", count: 1 } },
    ],
    alert: { title: "Relay service", subtitle: "monocode", body: "Has a question for you" },
  },
  done: {
    activity: [session("a", "Fix mobile notifications", "done")],
    alert: { title: "Fix mobile notifications", subtitle: "monocode", body: "Finished" },
  },
  clear: { activity: [] },
};
const scene = scenes[state];
if (!scene) {
  console.error(`Unknown state "${state}". Use: ${Object.keys(scenes).join(", ")}`);
  process.exit(1);
}
// The Simulator refuses a push nobody would see, so the silent ones the
// desktop sends get a banner here; the app reads the activity the same way.
if (!scene.alert)
  console.log("Simulator: sending as a banner, since it rejects silent pushes.");
const payload = {
  "Simulator Target Bundle": BUNDLE,
  aps: {
    "content-available": 1,
    alert: scene.alert ?? { title: "Live Activity", body: `Simulated ${state} update` },
    ...(scene.alert ? { sound: "default" } : {}),
  },
  body: { activity: scene.activity, projectId: "demo", sessionId: scene.activity[0]?.id ?? "" },
};
const file = join(mkdtempSync(join(tmpdir(), "monocode-push-")), "push.apns");
writeFileSync(file, JSON.stringify(payload));
try {
  execFileSync("xcrun", ["simctl", "push", "booted", BUNDLE, file], { stdio: "pipe" });
  console.log(`Sent "${state}".`);
} catch (error) {
  const output = String(error.stderr ?? error.message);
  console.error(
    output.includes("not authorized")
      ? "The app may not show notifications on this simulator. Pair it, allow the prompt (or Settings > Notifications > Monocode), then try again."
      : output,
  );
  process.exit(1);
}
