// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { inboxItemKey, type InboxItem } from "../lib/githubTasks";
import { isInboxEntryUnseen, markInboxItemSeen } from "../lib/inboxSeen";
import {
  loadNotificationPreferences,
  updateNotificationPreferences,
} from "../lib/notificationPreferences";
import {
  inboxNotificationProject,
  rememberNotificationProjects,
} from "../lib/notificationProjects";
import { ProjectNotificationSettings } from "../surfaces/ProjectNotificationSettings";
import { useInboxActivity, type InboxActivity } from "./useInboxUnseen";

// Only provider I/O and the audio device are replaced. Notification policy,
// polling, persisted preferences, unread tracking, and Settings are real.
const { listInboxItems, play } = vi.hoisted(() => ({
  listInboxItems: vi.fn(),
  play: vi.fn(),
}));
vi.mock("../lib/githubTasks", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/githubTasks")>()),
  listInboxItems,
}));
vi.mock("cuelume", () => ({ play, setEnabled: vi.fn(), setVolume: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockRejectedValue(new Error("No native bridge")),
}));

const oneId = "repository:github.com/acme/one";
const twoId = "repository:github.com/acme/two";
const recents = [
  { path: "/one", openedAt: 1 },
  { path: "/two", openedAt: 2 },
];
const sessions = [];
let items: InboxItem[];
let root: Root;
let container: HTMLDivElement;
let activity: InboxActivity;

function Harness() {
  activity = useInboxActivity(recents, "/one", sessions);
  return createElement(ProjectNotificationSettings, { cwd: "" });
}
async function mount() {
  await act(async () => root.render(createElement(Harness)));
}
function entry(index: number) {
  return { key: inboxItemKey(items[index]), updatedAt: items[index].updatedAt };
}
async function pollUpdated(...indices: number[]) {
  const updatedAt = new Date(Date.now() + 1000).toISOString();
  items = items.map((item, index) =>
    indices.includes(index) ? { ...item, updatedAt } : item,
  );
  await act(async () => vi.advanceTimersByTimeAsync(30_000));
}
function projectRow(name: string) {
  return container
    .querySelector(
      `button[aria-label="Notification categories for acme/${name}"]`,
    )!
    .closest("fieldset")!;
}
function click(scope: ParentNode, label: string) {
  const button = [...scope.querySelectorAll<HTMLButtonElement>("button")].find(
    (item) =>
      item.getAttribute("aria-label") === label || item.textContent === label,
  );
  expect(button, label).toBeDefined();
  act(() => button!.click());
}
function mute(scope: ParentNode) {
  click(scope, "Mute notifications");
  click(document.querySelector('[role="menu"]')!, "Until resumed");
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2030-01-15T12:00:00Z"));
  localStorage.clear();
  play.mockClear();
  items = ["one", "two"].map((name) => ({
    provider: "github",
    kind: "pr",
    repo: `acme/${name}`,
    number: 1,
    title: `Update ${name}`,
    url: `https://github.com/acme/${name}/pull/1`,
    state: "open",
    updatedAt: "2030-01-15T11:59:00Z",
    labels: [],
    assignees: [],
    draft: false,
    projectPath: `/${name}`,
  }));
  rememberNotificationProjects(items.map(inboxNotificationProject));
  listInboxItems
    .mockReset()
    .mockImplementation(async () => ({ items, errors: {} }));
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  localStorage.clear();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it("honors category choices before and after a project mute while another project can still notify", async () => {
  items.push({
    ...items[0],
    kind: "issue",
    number: 2,
    url: "https://github.com/acme/one/issues/2",
  });
  await mount();
  click(projectRow("one"), "Notification categories for acme/one");
  act(() =>
    projectRow("one")
      .querySelector<HTMLInputElement>(
        '[aria-label="Issues and Linear tasks for acme/one"]',
      )!
      .click(),
  );
  await pollUpdated(2);
  expect(play).not.toHaveBeenCalled();
  expect(activity.unseen).toBe(false);
  expect(isInboxEntryUnseen(entry(2))).toBe(true);
  await pollUpdated(0);
  expect(play.mock.calls).toEqual([["bloom"]]);
  expect(activity.unseen).toBe(true);
  play.mockClear();
  mute(projectRow("one"));
  await pollUpdated(0);
  expect(activity.unseen).toBe(false);
  expect(play).not.toHaveBeenCalled();
  expect(isInboxEntryUnseen(entry(0))).toBe(true);

  // Muted activity arrives first and must not consume the batch's sound slot.
  await pollUpdated(0, 1);
  expect(play.mock.calls).toEqual([["bloom"]]);
  expect(activity.unseen).toBe(true);
  expect(isInboxEntryUnseen(entry(0))).toBe(true);
  expect(isInboxEntryUnseen(entry(1))).toBe(true);
  act(() => markInboxItemSeen(entry(1)));
  expect(activity.unseen).toBe(false);
  expect(isInboxEntryUnseen(entry(0))).toBe(true);

  play.mockClear();
  click(projectRow("one"), "Resume notifications");
  expect(loadNotificationPreferences()[oneId].disabled).toEqual(["issues"]);
  expect(play).not.toHaveBeenCalled();
  act(() => markInboxItemSeen(entry(0)));
  expect(activity.unseen).toBe(false);
  await pollUpdated(2);
  expect(play).not.toHaveBeenCalled();
  expect(activity.unseen).toBe(false);
  await pollUpdated(0);
  expect(play.mock.calls).toEqual([["bloom"]]);
  expect(activity.unseen).toBe(true);
});

it("restores a timed mute from storage on remount and expires without replaying missed sounds", async () => {
  await mount();
  const deadline = Date.now() + 60_000;
  act(() => updateNotificationPreferences([oneId], { mutedUntil: deadline }));
  await pollUpdated(0);
  expect(activity.unseen).toBe(false);
  expect(play).not.toHaveBeenCalled();

  const saved = Array.from({ length: localStorage.length }, (_, index) => {
    const key = localStorage.key(index)!;
    return [key, localStorage.getItem(key)!] as const;
  });
  act(() => root.unmount());
  localStorage.clear();
  for (const [key, value] of saved) localStorage.setItem(key, value);
  vi.setSystemTime(deadline - 15_000);
  root = createRoot(container);
  await mount();
  expect(loadNotificationPreferences()[oneId].mutedUntil).toBe(deadline);
  expect(activity.unseen).toBe(false);
  expect(isInboxEntryUnseen(entry(0))).toBe(true);
  expect(play).not.toHaveBeenCalled();

  await act(async () => vi.advanceTimersByTimeAsync(15_000));
  expect(activity.unseen).toBe(true);
  expect(play).not.toHaveBeenCalled();
  await act(async () => vi.advanceTimersByTimeAsync(15_000));
  expect(play).not.toHaveBeenCalled();
  await pollUpdated(0);
  expect(play.mock.calls).toEqual([["bloom"]]);
});

it("bulk mutes through Settings and resumes just one project without losing unread items", async () => {
  await mount();
  click(container, "Select projects");
  act(() =>
    container
      .querySelector<HTMLInputElement>('[aria-label="Select all projects"]')!
      .click(),
  );
  mute(container.querySelector('[aria-label="Mute selected projects"]')!);
  await pollUpdated(0, 1);
  expect(activity.unseen).toBe(false);
  expect(play).not.toHaveBeenCalled();
  expect(isInboxEntryUnseen(entry(0))).toBe(true);
  expect(isInboxEntryUnseen(entry(1))).toBe(true);

  click(projectRow("one"), "Resume notifications");
  expect(loadNotificationPreferences()[oneId].mutedUntil).toBeUndefined();
  expect(loadNotificationPreferences()[twoId].mutedUntil).toBeNull();
  expect(activity.unseen).toBe(true);
  expect(play).not.toHaveBeenCalled();
  act(() => markInboxItemSeen(entry(0)));
  expect(activity.unseen).toBe(false);
  await pollUpdated(1);
  expect(activity.unseen).toBe(false);
  expect(play).not.toHaveBeenCalled();
  await pollUpdated(0, 1);
  expect(activity.unseen).toBe(true);
  expect(play.mock.calls).toEqual([["bloom"]]);
  expect(isInboxEntryUnseen(entry(1))).toBe(true);
});
