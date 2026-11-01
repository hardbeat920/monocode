import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const app = vi.hoisted(() => ({
  replace: vi.fn(),
  archive: vi.fn(async (_id: string) => {}),
  unarchive: vi.fn(async (_id: string) => {}),
  canUnarchive: false,
  alert: vi.fn(),
  newMono: vi.fn(),
  store: new Map<string, string>(),
  workspace: {} as Record<string, unknown>,
}));

vi.mock("expo-sqlite/kv-store", () => ({
  default: {
    getItemSync: (key: string) => app.store.get(key) ?? null,
    setItem: async (key: string, value: string) => void app.store.set(key, value),
  },
}));
vi.mock("expo-router", () => ({ router: { replace: app.replace } }));
vi.mock("expo-haptics", () => ({
  impactAsync: vi.fn(),
  selectionAsync: vi.fn(),
  notificationAsync: vi.fn(),
  ImpactFeedbackStyle: { Medium: "medium" },
  NotificationFeedbackType: { Success: "success" },
}));
vi.mock("react-native", () => ({
  ActivityIndicator: () => null,
  ActionSheetIOS: { showActionSheetWithOptions: vi.fn() },
  Alert: { alert: app.alert, prompt: vi.fn() },
  StyleSheet: { create: (styles: unknown) => styles, hairlineWidth: 1 },
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({ children, onPress, accessibilityLabel }: { children?: ReactNode; onPress?: () => void; accessibilityLabel?: string }) => (
    <button aria-label={accessibilityLabel} onClick={onPress}>{children}</button>
  ),
}));
vi.mock("react-native-reanimated", () => ({
  default: {
    FlatList: ({ data, renderItem, ListHeaderComponent, ListEmptyComponent }: {
      data: unknown[];
      renderItem: (info: { item: unknown }) => ReactNode;
      ListHeaderComponent?: ReactNode;
      ListEmptyComponent?: ReactNode;
    }) => (
      <section>
        {ListHeaderComponent}
        {data.length ? data.map((item, index) => <div key={index}>{renderItem({ item })}</div>) : ListEmptyComponent}
      </section>
    ),
    View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  },
  FadeIn: { duration: () => ({ delay: () => ({}) }) },
  FadeOut: { duration: () => ({}) },
  FadeInDown: { duration: () => ({ reduceMotion: () => ({}) }) },
  FadeOutDown: { duration: () => ({ reduceMotion: () => ({}) }) },
  ReduceMotion: { System: "system" },
  LinearTransition: { duration: () => ({}) },
}));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({ pairing: { desktop: { fingerprint: "desk" } } }),
}));
vi.mock("@/features/files/FileSearchBar", () => ({
  SEARCH_BAR: 48,
  FileSearchBar: ({ accessory }: { accessory?: ReactNode }) => <nav>{accessory}</nav>,
}));
vi.mock("@/features/workspace/monoSeen", () => ({
  useUnreadMonos: (_desktop: string, monos: { id: string }[]) =>
    new Set(monos.filter((mono) => mono.id === "unread").map((mono) => mono.id)),
}));
vi.mock("@/features/workspace/useCreateMono", () => ({ useCreateMono: () => app.newMono }));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({ useWorkspace: () => app.workspace }));
vi.mock("@/features/mascots/PixelMascot", () => ({
  PixelMascot: ({ name }: { name: string }) => <i data-mascot={name} />,
}));
vi.mock("@/ui/Icon", () => ({ Icon: ({ name }: { name: string }) => <i data-icon={name} /> }));
vi.mock("@/ui/AnimationsPaused", () => ({ AnimationsPaused: ({ children }: { children: ReactNode }) => <>{children}</> }));
vi.mock("@/ui/GradualBlur", () => ({ GradualBlur: () => null }));
vi.mock("@/ui/theme", () => ({
  themedStyles: (create: (colors: object) => object) => () => create({}),
  useColors: () => ({ muted: "gray" }),
  useScheme: () => "light",
  withAlpha: () => "black",
  radii: { row: 12 },
}));
vi.mock("@/features/sessions/SessionScreen", () => ({
  useSessionLayout: () => ({ pageTop: 0, dockBottom: 0 }),
}));
vi.mock("@/features/sessions/useSessionPages", () => ({
  useSessionPages: () => null,
  visibleSessions: (_pages: unknown, recent: unknown[]) => recent,
}));
vi.mock("@/features/sessions/useArrivals", () => ({
  ARRIVE_MS: 280,
  useArrivals: (sessions: unknown[]) => ({ sessions, arriving: new Set(), moving: false }),
}));
vi.mock("@/features/sessions/SessionFilterMenu", () => ({
  NO_FILTER: { working: false, harness: null },
  filterSessions: (sessions: unknown[]) => sessions,
  SessionFilterMenu: ({ monos }: { monos?: { shown: boolean; onChange: (shown: boolean) => void } }) =>
    monos ? (
      <button aria-label="Monos filter" data-shown={String(monos.shown)} onClick={() => monos.onChange(!monos.shown)} />
    ) : (
      <span data-no-monos-filter />
    ),
}));
vi.mock("@/features/sessions/SessionCard", () => ({
  SessionCard: ({ session, origin, onOpenOrigin, onArchive }: {
    session: { id: string; title: string };
    origin?: { kind: string; mono?: { name: string }; openable?: boolean } | null;
    onOpenOrigin?: () => void;
    onArchive?: () => void;
  }) => (
    <article data-session={session.id}>
      {session.title}
      {origin ? (
        <button aria-label={`chip ${session.id}`} data-tappable={String(!!onOpenOrigin)} onClick={onOpenOrigin}>
          {origin.kind === "removed" ? "Mono removed" : `via ${origin.mono?.name}`}
        </button>
      ) : null}
      <button aria-label={`archive ${session.id}`} onClick={onArchive} />
    </article>
  ),
}));

import { resetMonoGroupPrefs } from "@/features/sessions/monoGroupPrefs";
import { SessionsPage } from "@/features/sessions/SessionsPage";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const mono = (id: string, projectIds: string[], extra = {}) => ({
  id, name: id, mascot: `mascot-${id}`, color: "red", projectIds, unread: 0, status: "idle",
  lastMessage: `${id} last line`, lastActiveAt: 0, ...extra,
});
const row = (id: string, extra = {}) => ({
  id, projectId: "p1", title: id, harness: "claude", model: "m", branch: "main", updatedAt: 1, ...extra,
});

let root: ReturnType<typeof createRoot>;
let page: HTMLElement;
function render(projectId = "p1") {
  page = document.createElement("div");
  root = createRoot(page);
  act(() => root.render(<SessionsPage projectId={projectId} active onOpen={vi.fn()} />));
}
const byLabel = (label: string) => page.querySelector(`[aria-label='${label}']`) as HTMLElement;
const click = (element: HTMLElement) => act(() => element.click());

beforeEach(() => {
  vi.clearAllMocks();
  app.store.clear();
  resetMonoGroupPrefs();
  app.canUnarchive = false;
  const taskGiver = mono("TaskGiver", ["p1"]);
  const away = mono("Away", ["p9"]);
  app.workspace = {
    projects: [{ id: "p1" }, { id: "p2" }],
    monos: [taskGiver, mono("Both", ["p1", "p2"]), mono("Loose", []), mono("unread", ["p1"])],
    allMonos: [taskGiver, away, mono("Both", ["p1", "p2"]), mono("Loose", []), mono("unread", ["p1"])],
    sessions: [
      row("by-tg", { monoId: "TaskGiver" }),
      row("by-away", { monoId: "Away" }),
      row("by-gone", { monoId: "Deleted" }),
      row("by-hand"),
      row("other-project", { projectId: "p2" }),
    ],
    edited: (session: unknown) => session,
    rememberSession: vi.fn(),
    archiveSession: app.archive,
    unarchiveSession: app.unarchive,
    get canUnarchive() { return app.canUnarchive; },
    renameSession: vi.fn(),
    deleteSession: vi.fn(async () => {}),
  };
});
afterEach(() => act(() => root.unmount()));

describe("a project's Sessions page", () => {
  it("pins its Monos above the rows, with the unassigned ones on the first project's page", () => {
    render("p1");
    expect(page.textContent).toContain("Monos");
    const names = Array.from(page.querySelectorAll("[data-mascot]")).map((el) => (el as HTMLElement).dataset.mascot);
    expect(names).toEqual(["mascot-TaskGiver", "mascot-Both", "mascot-unread", "mascot-Loose"].sort((a, b) => names.indexOf(a) - names.indexOf(b)));
    expect(page.textContent).toContain("Unassigned");
    expect(page.textContent).toContain("Loose");
  });

  it("lists a Mono assigned to several projects on each, and leaves out ones with no project there", () => {
    render("p2");
    expect(page.textContent).toContain("Both");
    expect(page.textContent).not.toContain("TaskGiver");
    expect(page.textContent).not.toContain("Unassigned");
  });

  it("opens a Mono's chat from its row", () => {
    render("p1");
    click(byLabel("Unread, unread, unread last line"));
    expect(app.replace).toHaveBeenCalledWith({ pathname: "/mono/[monoId]", params: { monoId: "unread" } });
  });

  it("marks unread Monos on their rows", () => {
    render("p1");
    expect(byLabel("Unread, unread, unread last line")).not.toBeNull();
    expect(byLabel("TaskGiver, TaskGiver last line")).not.toBeNull();
  });

  it("folds the group from its header and remembers it", () => {
    render("p1");
    click(byLabel("Monos, 4"));
    expect(page.textContent).not.toContain("TaskGiver last line");
    act(() => root.unmount());
    resetMonoGroupPrefs();
    render("p1");
    expect(page.textContent).not.toContain("TaskGiver last line");
    click(byLabel("Monos, 4"));
    expect(page.textContent).toContain("TaskGiver last line");
  });

  it("hides and shows the group from the filter, and remembers it per desktop", () => {
    render("p1");
    expect(byLabel("Monos filter").dataset.shown).toBe("true");
    click(byLabel("Monos filter"));
    expect(page.textContent).not.toContain("TaskGiver last line");
    // Still offered, so it can come back.
    expect(byLabel("Monos filter").dataset.shown).toBe("false");
    expect(app.store.get("monocode.monoGroup.desk")).toContain('"shown":false');
    click(byLabel("Monos filter"));
    expect(page.textContent).toContain("TaskGiver last line");
  });

  it("offers a plus for a new Mono in the group's header", () => {
    render("p1");
    click(byLabel("New Mono"));
    expect(app.newMono).toHaveBeenCalledOnce();
  });

  it("says who started each session, and nothing for one started by hand", () => {
    render("p1");
    expect(byLabel("chip by-tg").textContent).toBe("via TaskGiver");
    expect(byLabel("chip by-tg").dataset.tappable).toBe("true");
    // Its Mono isn't on this phone, so it is named but not opened.
    expect(byLabel("chip by-away").textContent).toBe("via Away");
    expect(byLabel("chip by-away").dataset.tappable).toBe("false");
    expect(byLabel("chip by-gone").textContent).toBe("Mono removed");
    expect(byLabel("chip by-hand")).toBeNull();
    // Another project's session isn't on this page.
    expect(page.querySelector("[data-session='other-project']")).toBeNull();
  });

  it("opens the Mono's chat from a session's chip", () => {
    render("p1");
    click(byLabel("chip by-tg"));
    expect(app.replace).toHaveBeenCalledWith({ pathname: "/mono/[monoId]", params: { monoId: "TaskGiver" } });
  });

  it("archives from the swipe with an Undo toast where the desktop can unarchive", async () => {
    app.canUnarchive = true;
    render("p1");
    await act(async () => byLabel("archive by-hand").click());
    expect(app.archive).toHaveBeenCalledWith("by-hand");
    expect(app.alert).not.toHaveBeenCalled();
    expect(page.textContent).toContain("Archived “by-hand”");
    await act(async () => {
      byLabel("Undo").click();
      await new Promise((done) => setTimeout(done, 0));
    });
    expect(app.unarchive).toHaveBeenCalledWith("by-hand");
  });

  it("asks first and shows no Undo where it can't be taken back", async () => {
    render("p1");
    act(() => byLabel("archive by-hand").click());
    expect(app.archive).not.toHaveBeenCalled();
    expect(app.alert.mock.calls[0][0]).toBe("Archive “by-hand”?");
    await act(async () => app.alert.mock.calls[0][2][1].onPress());
    expect(app.archive).toHaveBeenCalledWith("by-hand");
    expect(page.textContent).toContain("Archived “by-hand”");
    expect(byLabel("Undo")).toBeNull();
  });
});
