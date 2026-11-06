import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createFixtureClient } from "@/features/inbox/fixtures";
import { ItemSheet } from "@/features/inbox/ItemSheet";
import type { InboxClient, InboxItem } from "@/features/inbox/protocol";
import { createReviewBook } from "@/features/inbox/review";
import { createInboxStore } from "@/features/inbox/store";

const OPEN_PR = "github:monocode/monocode#1284";
const ISSUE = "linear:MON-482";

type AlertButton = { text: string; style?: string; onPress?: () => void };

const seen = vi.hoisted(() => ({
  inbox: null as unknown,
  alert: vi.fn<(title: string, detail?: string, buttons?: unknown[]) => void>(),
  push: vi.fn<(target: unknown) => void>(),
  copy: vi.fn<(text: string) => Promise<boolean>>(),
  keepDiff: vi.fn<(file: unknown, reviewOf?: string) => string>(() => "diff"),
}));

vi.mock("react-native", () => ({
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({
    children,
    onPress,
    accessibilityLabel,
  }: {
    children?: ReactNode;
    onPress?: () => void;
    accessibilityLabel?: string;
  }) => (
    <button aria-label={accessibilityLabel} onClick={onPress}>
      {children}
    </button>
  ),
  ActivityIndicator: () => <span>busy</span>,
  Alert: { alert: seen.alert },
  Linking: { openURL: vi.fn() },
}));
vi.mock("expo-haptics", () => ({
  selectionAsync: vi.fn(),
  impactAsync: vi.fn(),
  notificationAsync: vi.fn(),
  ImpactFeedbackStyle: { Light: "light" },
  NotificationFeedbackType: { Success: "success" },
}));
vi.mock("expo-clipboard", () => ({ setStringAsync: seen.copy }));
vi.mock("expo-router", () => ({
  router: { push: seen.push, back: vi.fn() },
  Stack: { Screen: () => null },
}));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
}));
vi.mock("@/features/changes/diffTheme", () => ({ MONO: "mono" }));
vi.mock("@/features/changes/shownDiffs", () => ({ keepDiff: seen.keepDiff }));
vi.mock("@/features/files/codeScreen", () => ({ HeaderButton: () => null }));
vi.mock("@/features/sessions/Markdown", () => ({ Markdown: () => null }));
vi.mock("@/features/settings/SettingsUI", () => ({
  SettingsPage: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
}));
vi.mock("@/ui/GradualBlur", () => ({ GradualBlur: () => null }));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/LiquidView", () => ({
  LiquidView: ({
    children,
    onPress,
    accessibilityLabel,
  }: {
    children?: ReactNode;
    onPress?: () => void;
    accessibilityLabel?: string;
  }) =>
    onPress ? (
      <button aria-label={accessibilityLabel} onClick={onPress}>
        {children}
      </button>
    ) : (
      <div>{children}</div>
    ),
}));
vi.mock("@/ui/theme", () => ({
  themedStyles: () => () => ({}),
  useColors: () => ({}),
  withAlpha: () => "",
}));
vi.mock("@/ui/ChoiceMenu", () => ({
  ChoiceMenu: ({
    label,
    actions,
    children,
  }: {
    label: string;
    actions: { label: string; onPress: () => void }[];
    children?: ReactNode;
  }) => (
    <div aria-label={label}>
      {children}
      {actions.map((action) => (
        <button key={action.label} aria-label={action.label} onClick={action.onPress} />
      ))}
    </div>
  ),
}));
vi.mock("@/features/inbox/ItemMenu", () => ({
  ItemMenu: ({ entries }: { entries: { label: string; onPress: () => void }[] }) => (
    <div>
      {entries.map((entry) => (
        <button key={entry.label} aria-label={entry.label} onClick={entry.onPress} />
      ))}
    </div>
  ),
}));
vi.mock("@/features/inbox/Avatar", () => ({ Avatar: () => null }));
vi.mock("@/features/inbox/InboxMarkdown", () => ({ InboxMarkdown: () => null }));
vi.mock("@/features/inbox/InboxRow", () => ({
  ChecksGlyph: () => null,
  whyLabel: () => "",
}));
vi.mock("@/features/inbox/InboxToast", () => ({ InboxToast: () => null }));
vi.mock("@/features/inbox/ProviderMark", () => ({ SOURCE_NAMES: {} }));
vi.mock("@/features/inbox/useNow", () => ({ useNow: () => 0 }));
vi.mock("@/features/inbox/InboxProvider", () => ({ useInbox: () => seen.inbox }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function fixture() {
  return createFixtureClient({ projectIds: ["p1"], now: () => 0, controls: { latencyMs: 0 } });
}

let root: Root | null = null;
let host: HTMLElement;

/** The real sheet on the real store; only the toast's `act` is the provider's. */
async function open(key: string, client: InboxClient) {
  const store = createInboxStore({
    client,
    storage: { get: () => null, set: () => {} },
    namespace: "test",
  });
  await store.refresh({ scope: "all" });
  const notify = vi.fn();
  const review = createReviewBook(client);
  seen.inbox = {
    client,
    review,
    store,
    notify,
    mark: vi.fn(),
    act: (item: InboxItem, action: Parameters<typeof store.act>[1]) => store.act(item.key, action),
  };
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<ItemSheet itemKey={key} />);
  });
  await settle();
  return { store, notify, review };
}

async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 5));
  });
}

const button = (label: string) => host.querySelector<HTMLElement>(`button[aria-label="${label}"]`);

async function openTab(name: string) {
  const tab = [...host.querySelectorAll("button")].find((entry) => entry.textContent?.startsWith(name));
  if (!tab) throw new Error(`No tab "${name}"`);
  await act(async () => tab.click());
  await settle();
}

async function press(label: string) {
  const target = button(label);
  if (!target) throw new Error(`No button "${label}"`);
  await act(async () => target.click());
}

/** Presses a button of the alert on screen. */
async function answer(text: string) {
  const buttons = seen.alert.mock.lastCall?.[2];
  const choice = (buttons as AlertButton[] | undefined)?.find((entry) => entry.text === text);
  if (!choice?.onPress) throw new Error(`No alert button "${text}"`);
  await act(async () => choice.onPress?.());
  await settle();
}

beforeEach(() => {
  seen.alert.mockClear();
  seen.push.mockClear();
  seen.keepDiff.mockClear();
  seen.copy.mockReset();
  seen.copy.mockResolvedValue(true);
});

afterEach(() => {
  act(() => root?.unmount());
  root = null;
  host.remove();
});

describe("an open pull request", () => {
  it("offers the three merges and the rest of the desktop's actions", async () => {
    await open(OPEN_PR, fixture());
    for (const label of [
      "Create a merge commit",
      "Squash and merge",
      "Rebase and merge",
      "Add comment",
      "Copy link",
      "Copy branch name",
      "Convert to draft",
      "Close pull request",
    ]) {
      expect(button(label), label).not.toBeNull();
    }
    expect(button("Reopen pull request")).toBeNull();
    expect(button("Ready for review")).toBeNull();
  });

  it("merges nothing until the user confirms", async () => {
    const client = fixture();
    const merge = vi.spyOn(client, "act");
    await open(OPEN_PR, client);

    await press("Squash and merge");

    expect(seen.alert.mock.lastCall?.[0]).toBe("Squash and merge?");
    expect(seen.alert.mock.lastCall?.[1]).toContain("“fix/sample-branch”");
    expect(merge).not.toHaveBeenCalled();

    await answer("Squash and merge");

    expect(merge).toHaveBeenCalledExactlyOnceWith(OPEN_PR, "squash");
  });

  it("stops offering merges once merged", async () => {
    const { store } = await open(OPEN_PR, fixture());
    await press("Rebase and merge");
    await answer("Rebase and merge");

    expect(store.find(OPEN_PR)?.status).toBe("merged");
    expect(button("Rebase and merge")).toBeNull();
    expect(button("Close pull request")).toBeNull();
    expect(button("Add comment")).not.toBeNull();
  });

  it("shows what the provider said when it refuses, and changes nothing", async () => {
    const client = fixture();
    vi.spyOn(client, "act").mockRejectedValue(new Error("Pull request is not mergeable"));
    const { store } = await open(OPEN_PR, client);

    await press("Create a merge commit");
    await answer("Merge pull request");

    expect(seen.alert.mock.lastCall?.slice(0, 2)).toEqual([
      "Couldn’t merge",
      "Pull request is not mergeable",
    ]);
    expect(store.find(OPEN_PR)?.status).toBe("open");
    expect(button("Create a merge commit")).not.toBeNull();
  });

  it("marks closing as destructive", async () => {
    await open(OPEN_PR, fixture());
    await press("Close pull request");
    const buttons = seen.alert.mock.lastCall?.[2] as AlertButton[];
    expect(buttons.find((entry) => entry.text === "Close pull request")?.style).toBe("destructive");
  });

  it("copies the branch name", async () => {
    const { notify } = await open(OPEN_PR, fixture());
    await press("Copy branch name");
    expect(seen.copy).toHaveBeenCalledWith("fix/sample-branch");
    expect(notify).toHaveBeenCalledWith("Branch name copied");
  });

  it("opens the reply sheet on the entry's thread", async () => {
    await open(OPEN_PR, fixture());
    const reply = host.querySelector<HTMLElement>('button[aria-label^="Reply to"]');
    if (!reply) throw new Error("No reply button");
    await act(async () => reply.click());
    expect(seen.push).toHaveBeenCalledWith({
      pathname: "/inbox/comment",
      params: expect.objectContaining({ key: OPEN_PR, replyTo: `${OPEN_PR}#0` }),
    });
  });
});

describe("other items", () => {
  it("offers an issue comments but no pull request actions", async () => {
    await open(ISSUE, fixture());
    expect(button("Add comment")).not.toBeNull();
    expect(button("Copy link")).not.toBeNull();
    expect(button("Create a merge commit")).toBeNull();
    expect(button("Close pull request")).toBeNull();
    expect(button("Copy branch name")).toBeNull();
  });

  it("offers an older desktop's items nothing it can't answer", async () => {
    const client = fixture();
    const read = client.item;
    client.item = async (key) => {
      const { actions: _actions, ...detail } = await read(key);
      return detail;
    };
    await open(OPEN_PR, client);
    expect(button("Copy link")).not.toBeNull();
    expect(button("Add comment")).toBeNull();
    expect(button("Create a merge commit")).toBeNull();
    expect(button("Close pull request")).toBeNull();
  });
});

const LINE = { path: "src/net/retry.rs", line: 42, side: "right" } as const;

describe("the comments section", () => {
  it("heads the discussion and counts comments, not commits", async () => {
    const client = fixture();
    const detail = await client.item(OPEN_PR);
    const comments = detail.thread.filter((entry) => entry.kind !== "commit").length;
    const commit = {
      ...detail.thread[0],
      id: "c1",
      kind: "commit" as const,
      body: "Fold the retries",
    };
    await open(OPEN_PR, {
      ...client,
      item: async () => ({ ...detail, thread: [...detail.thread, commit] }),
    });
    expect(host.textContent).toContain(`Comments${comments}`);
    expect(host.textContent).toContain("committed: Fold the retries");
    expect(button(`Reply to ${commit.author.name}`)).not.toBeNull();
    expect(host.querySelectorAll('button[aria-label^="Reply to"]')).toHaveLength(comments);
  });

  it("says where a line comment sits", async () => {
    const client = fixture();
    const detail = await client.item(OPEN_PR);
    const placed = { ...detail.thread[0], path: "src/net/retry.rs", line: 42 };
    await open(OPEN_PR, { ...client, item: async () => ({ ...detail, thread: [placed] }) });
    expect(host.textContent).toContain("src/net/retry.rs:42");
  });

  it("says so when nobody has commented", async () => {
    const client = fixture();
    const detail = await client.item(OPEN_PR);
    await open(OPEN_PR, { ...client, item: async () => ({ ...detail, thread: [] }) });
    expect(host.textContent).toContain("No comments yet.");
    expect(host.textContent).toContain("Comments0");
  });
});

describe("a review", () => {
  it("opens a pull request's files to line comments", async () => {
    await open(OPEN_PR, fixture());
    await openTab("Code");
    expect(host.textContent).toContain("Tap a line in a file to comment on it.");
    const file = host.querySelector<HTMLElement>('button[aria-label$=" removed"]');
    if (!file) throw new Error("No file row");
    await act(async () => file.click());
    await settle();
    expect(seen.keepDiff.mock.lastCall?.[1]).toBe(OPEN_PR);
  });

  it("shows comments written at the desk when the Code tab opens", async () => {
    const client = fixture();
    await client.reviewAdd(OPEN_PR, LINE, "Cap this");
    await open(OPEN_PR, client);
    await openTab("Code");
    expect(host.textContent).toContain("retry.rs:42");
    expect(host.textContent).toContain("Cap this");
  });

  it("posts nothing until the user confirms, then clears and reloads", async () => {
    const client = fixture();
    await client.reviewAdd(OPEN_PR, LINE, "Cap this");
    const submit = vi.spyOn(client, "reviewSubmit");
    const { notify } = await open(OPEN_PR, client);
    await openTab("Code");
    await press("Post review");
    expect(seen.alert.mock.lastCall?.[0]).toBe("Post this review?");
    expect(seen.alert.mock.lastCall?.[1]).toContain("1 comment will go to GitHub");
    expect(submit).not.toHaveBeenCalled();
    await answer("Post review");
    expect(submit).toHaveBeenCalledTimes(1);
    expect(notify).toHaveBeenCalledWith("Review posted");
    expect(host.textContent).not.toContain("Cap this");
  });

  it("keeps the comments when GitHub refuses", async () => {
    const client = fixture();
    await client.reviewAdd(OPEN_PR, LINE, "Cap this");
    const { notify } = await open(OPEN_PR, {
      ...client,
      reviewSubmit: async () => {
        throw new Error("Line is outside the diff");
      },
    });
    await openTab("Code");
    await press("Post review");
    await answer("Post review");
    expect(seen.alert).toHaveBeenLastCalledWith(
      "Couldn’t post the review",
      "Line is outside the diff",
    );
    expect(notify).not.toHaveBeenCalled();
    expect(host.textContent).toContain("Cap this");
  });

  it("removes one comment and leaves the rest", async () => {
    const client = fixture();
    await client.reviewAdd(OPEN_PR, LINE, "Cap this");
    await client.reviewAdd(OPEN_PR, { ...LINE, line: 50 }, "Name this");
    await open(OPEN_PR, client);
    await openTab("Code");
    await press("Remove comment on retry.rs:42");
    await settle();
    expect(host.textContent).not.toContain("Cap this");
    expect(host.textContent).toContain("Name this");
  });
});
