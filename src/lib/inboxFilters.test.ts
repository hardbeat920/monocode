import { describe, expect, it } from "vitest";
import {
  applyInboxFilters,
  DEFAULT_INBOX_FILTERS,
  filterInboxByKind,
  filterInboxByProject,
  filterInboxByProvider,
  filterInboxByStatus,
  filterInboxByTime,
  hasActiveInboxFilters,
  inboxFetchState,
  pruneInboxFilters,
} from "./inboxFilters";
import type { InboxItem } from "./githubTasks";

function item(
  overrides: Partial<InboxItem> & Pick<InboxItem, "number" | "updatedAt">,
): InboxItem {
  return {
    kind: "issue",
    title: "Item",
    url: "https://github.com/acme/web/issues/1",
    state: "open",
    labels: [],
    assignees: [],
    draft: false,
    repo: "acme/web",
    projectPath: "/tmp/web",
    provider: "github",
    ...overrides,
  };
}

describe("filterInboxByProject", () => {
  it("hides selected projects", () => {
    const rows = [
      item({ number: 1, updatedAt: "2026-08-27T10:00:00Z", projectPath: "/tmp/web" }),
      item({ number: 2, updatedAt: "2026-08-27T10:00:00Z", projectPath: "/tmp/docs" }),
    ];
    expect(
      filterInboxByProject(rows, ["/tmp/web/"]).map((row) => row.number),
    ).toEqual([2]);
  });

  it("keeps Linear issues that are not tied to a folder", () => {
    const rows = [
      item({ number: 1, updatedAt: "2026-08-27T10:00:00Z", projectPath: "/tmp/web" }),
      item({
        number: 9,
        kind: "linear",
        provider: "linear",
        projectPath: "",
        updatedAt: "2026-08-27T10:00:00Z",
      }),
    ];
    expect(
      filterInboxByProject(rows, ["/tmp/web"]).map((row) => row.number),
    ).toEqual([9]);
  });
});

describe("filterInboxByKind", () => {
  it("hides selected kinds", () => {
    const rows = [
      item({ number: 1, kind: "issue", updatedAt: "2026-08-27T10:00:00Z" }),
      item({ number: 2, kind: "pr", updatedAt: "2026-08-27T10:00:00Z" }),
      item({
        number: 9,
        kind: "linear",
        provider: "linear",
        updatedAt: "2026-08-27T10:00:00Z",
      }),
    ];
    expect(filterInboxByKind(rows, ["pr"]).map((row) => row.number)).toEqual([
      1, 9,
    ]);
    expect(filterInboxByKind(rows, ["linear"]).map((row) => row.number)).toEqual(
      [1, 2],
    );
  });
});

describe("filterInboxByProvider", () => {
  it("keeps GitHub or Linear items", () => {
    const rows = [
      item({ number: 1, updatedAt: "2026-08-27T10:00:00Z" }),
      item({
        number: 9,
        kind: "linear",
        provider: "linear",
        updatedAt: "2026-08-27T10:00:00Z",
      }),
    ];
    expect(
      filterInboxByProvider(rows, "github").map((row) => row.number),
    ).toEqual([1]);
    expect(
      filterInboxByProvider(rows, "linear").map((row) => row.number),
    ).toEqual([9]);
  });
});

describe("filterInboxByStatus", () => {
  const rows = [
    item({ number: 1, updatedAt: "2026-08-27T10:00:00Z", state: "open" }),
    item({
      number: 2,
      kind: "pr",
      updatedAt: "2026-08-27T10:00:00Z",
      state: "open",
      draft: true,
    }),
    item({ number: 3, updatedAt: "2026-08-27T10:00:00Z", state: "closed" }),
    item({
      number: 4,
      kind: "pr",
      updatedAt: "2026-08-27T10:00:00Z",
      state: "merged",
    }),
  ];

  it("keeps every item when no status is selected", () => {
    expect(
      filterInboxByStatus(rows, DEFAULT_INBOX_FILTERS.status).map(
        (row) => row.number,
      ),
    ).toEqual([1, 2, 3, 4]);
  });

  it("matches any selected status", () => {
    expect(
      filterInboxByStatus(rows, {
        open: true,
        draft: false,
        closed: true,
        merged: false,
      }).map((row) => row.number),
    ).toEqual([1, 3]);
  });
});

describe("filterInboxByTime", () => {
  const now = new Date("2026-08-27T15:00:00").getTime();

  it("keeps items updated today", () => {
    const rows = [
      item({
        number: 1,
        updatedAt: new Date("2026-08-27T10:00:00").toISOString(),
      }),
      item({
        number: 2,
        updatedAt: new Date("2026-08-20T10:00:00").toISOString(),
      }),
    ];
    expect(
      filterInboxByTime(rows, "today", now).map((row) => row.number),
    ).toEqual([1]);
  });
});

describe("applyInboxFilters", () => {
  it("combines project, kind, and search filters", () => {
    const rows = [
      item({
        number: 1,
        title: "Fix checkout",
        kind: "pr",
        projectPath: "/tmp/web",
        updatedAt: "2026-08-27T10:00:00Z",
      }),
      item({
        number: 2,
        title: "Fix checkout",
        kind: "issue",
        projectPath: "/tmp/docs",
        updatedAt: "2026-08-27T10:00:00Z",
      }),
    ];
    expect(
      applyInboxFilters(
        rows,
        { ...DEFAULT_INBOX_FILTERS, hiddenProjects: ["/tmp/docs"] },
        "checkout",
      ).map((row) => row.number),
    ).toEqual([1]);
  });

  it("scopes to a provider tab", () => {
    const rows = [
      item({
        number: 1,
        title: "Fix checkout",
        kind: "pr",
        updatedAt: "2026-08-27T10:00:00Z",
      }),
      item({
        number: 9,
        title: "Fix checkout",
        kind: "linear",
        provider: "linear",
        updatedAt: "2026-08-27T10:00:00Z",
      }),
    ];
    expect(
      applyInboxFilters(
        rows,
        DEFAULT_INBOX_FILTERS,
        "checkout",
        Date.now(),
        "linear",
      ).map((row) => row.number),
    ).toEqual([9]);
  });

  it("ignores GitHub-only status filters on the Linear tab", () => {
    const rows = [
      item({
        number: 9,
        kind: "linear",
        provider: "linear",
        updatedAt: "2026-08-27T10:00:00Z",
      }),
    ];
    expect(
      applyInboxFilters(
        rows,
        {
          ...DEFAULT_INBOX_FILTERS,
          status: { open: false, draft: true, closed: false, merged: true },
        },
        "",
        Date.now(),
        "linear",
      ).map((row) => row.number),
    ).toEqual([9]);
  });
});

describe("hasActiveInboxFilters", () => {
  it("is false for defaults", () => {
    expect(hasActiveInboxFilters(DEFAULT_INBOX_FILTERS)).toBe(false);
  });

  it("is true when a project is hidden", () => {
    expect(
      hasActiveInboxFilters({
        ...DEFAULT_INBOX_FILTERS,
        hiddenProjects: ["/tmp/web"],
      }),
    ).toBe(true);
  });

  it("ignores GitHub-only filters on the Linear tab", () => {
    expect(
      hasActiveInboxFilters(
        {
          ...DEFAULT_INBOX_FILTERS,
          hiddenProjects: ["/tmp/web"],
          hiddenKinds: ["pr"],
        },
        "linear",
      ),
    ).toBe(false);
  });
});

describe("inboxFetchState", () => {
  it("fetches everything while no status is selected", () => {
    expect(inboxFetchState(DEFAULT_INBOX_FILTERS)).toBe("all");
  });

  it("narrows to open once only open or draft is selected", () => {
    expect(
      inboxFetchState({
        ...DEFAULT_INBOX_FILTERS,
        status: { ...DEFAULT_INBOX_FILTERS.status, open: true },
      }),
    ).toBe("open");
    expect(
      inboxFetchState({
        ...DEFAULT_INBOX_FILTERS,
        status: { ...DEFAULT_INBOX_FILTERS.status, draft: true },
      }),
    ).toBe("open");
  });

  it("widens to all when closed or merged is selected", () => {
    expect(
      inboxFetchState({
        ...DEFAULT_INBOX_FILTERS,
        status: { ...DEFAULT_INBOX_FILTERS.status, closed: true },
      }),
    ).toBe("all");
    expect(
      inboxFetchState({
        ...DEFAULT_INBOX_FILTERS,
        status: { ...DEFAULT_INBOX_FILTERS.status, open: true, merged: true },
      }),
    ).toBe("all");
  });
});

describe("pruneInboxFilters", () => {
  it("drops hidden projects that are no longer in the rail", () => {
    const pruned = pruneInboxFilters(
      { ...DEFAULT_INBOX_FILTERS, hiddenProjects: ["/tmp/web", "/tmp/gone"] },
      ["/tmp/web"],
    );
    expect(pruned.hiddenProjects).toEqual(["/tmp/web"]);
  });
});
