// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { updateNotificationPreferences } from "../lib/notificationPreferences";
import { ApprovalToasts } from "./ApprovalToasts";
import { invoke } from "@tauri-apps/api/core";
import { refreshNotificationProjects } from "../lib/notificationProjects";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (_command: string, { cwd }: { cwd: string }) => ({
    root: cwd,
    commonDir: null,
    remote: `https://github.com/user/${cwd.split("/").at(-1)}.git`,
  })),
}));

type Notice = ComponentProps<typeof ApprovalToasts>["notices"][number];
function notice(
  id: string,
  kind: Notice["kind"] = "approval",
  requestId = 1,
): Notice {
  return {
    sessionId: id,
    requestId,
    label: `Request ${id}`,
    kind,
    session: {
      id,
      cwd: `/projects/${id}`,
      title: id,
      harness: "codex",
      model: "",
      modelSettings: {},
      runtimeMode: "supervised",
      blocks: [],
    },
  };
}

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  onApproval.mockClear();
  onFocusSession.mockClear();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const onApproval = vi.fn();
const onFocusSession = vi.fn();
function render(notices: Notice[]) {
  root.render(
    createElement(ApprovalToasts, { notices, onApproval, onFocusSession }),
  );
}
function visibleRequests() {
  return [...document.querySelectorAll(".approval-toast")].map(
    (item) => item.textContent,
  );
}

it("hides muted project approval popups while another project's controls remain usable", async () => {
  updateNotificationPreferences(["repository:github.com/user/private"], {
    mutedUntil: null,
  });
  act(() => render([notice("private"), notice("work")]));
  expect(visibleRequests()).toHaveLength(0);
  await act(async () => {});
  expect(visibleRequests()).toHaveLength(1);
  expect(visibleRequests()[0]).toContain("Request work");
  const allow = [...document.querySelectorAll("button")].find(
    (item) => item.textContent === "Allow",
  )!;
  act(() => allow.click());
  expect(onApproval).toHaveBeenCalledWith("work", 1, "allow");
});

it("immediately hides an existing question when its notification category is disabled", async () => {
  await act(async () => render([notice("work", "question")]));
  expect(visibleRequests()).toHaveLength(1);
  act(() =>
    updateNotificationPreferences(["repository:github.com/user/work"], {
      disabled: ["agentInput"],
    }),
  );
  expect(visibleRequests()).toHaveLength(0);
});

it("applies the current checkout identity to a mounted approval without replaying it", async () => {
  let now = 1_800_000_000_000;
  vi.spyOn(Date, "now").mockImplementation(() => now);
  updateNotificationPreferences(["repository:github.com/user/new"], {
    mutedUntil: null,
  });
  await act(async () => render([notice("work")]));
  expect(visibleRequests()).toHaveLength(1);
  vi.mocked(invoke).mockResolvedValueOnce({
    root: "/projects/work",
    commonDir: null,
    remote: "https://github.com/user/new.git",
  });
  await act(async () => refreshNotificationProjects(["/projects/work"]));
  expect(visibleRequests()).toHaveLength(0);
  now += 1000;
  act(() =>
    updateNotificationPreferences(["repository:github.com/user/new"], {
      mutedUntil: undefined,
    }),
  );
  expect(visibleRequests()).toHaveLength(0);
  now += 1;
  await act(async () => render([notice("work", "approval", 2)]));
  expect(visibleRequests()).toHaveLength(1);
});

it.each(["resume", "expiry", "category"] as const)(
  "does not replay a suppressed request after %s, but shows a new request",
  async (reason) => {
    let now = 1_800_000_000_000;
    vi.spyOn(Date, "now").mockImplementation(() => now);
    const ids = ["repository:github.com/user/private"];
    updateNotificationPreferences(
      ids,
      reason === "category"
        ? { disabled: ["agentInput"] }
        : { mutedUntil: reason === "expiry" ? now + 60_000 : null },
    );
    await act(async () => render([notice("private", "question")]));
    expect(visibleRequests()).toHaveLength(0);
    now += 120_000;
    act(() => {
      if (reason === "resume")
        updateNotificationPreferences(ids, { mutedUntil: undefined });
      else if (reason === "category")
        updateNotificationPreferences(ids, { disabled: [] });
      else window.dispatchEvent(new Event("focus"));
    });
    expect(visibleRequests()).toHaveLength(0);
    now += 1;
    await act(async () => render([notice("private", "question", 2)]));
    expect(visibleRequests()).toHaveLength(1);
  },
);
