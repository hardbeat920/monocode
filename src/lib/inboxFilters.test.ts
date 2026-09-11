import { beforeEach, describe, expect, it } from "vitest";
import {
  applyInboxFilters,
  DEFAULT_INBOX_FILTERS,
  filterInboxByKind,
  filterInboxByLinearProject,
  filterInboxByProject,
  filterInboxByProvider,
  filterInboxByStatus,
  filterInboxByTime,
  hasActiveInboxFilters,
  inboxFetchState,
  LINEAR_NO_PROJECT,
  linearProjectOptions,
  pruneInboxFilters,
  connectableInboxSources,
  loadInboxConnections,
  resolveInboxSource,
  saveInboxConnections,
  visibleInboxSources,
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
      item({
        number: 1,
        updatedAt: "2026-08-27T10:00:00Z",
        projectPath: "/tmp/web",
      }),
      item({
        number: 2,
        updatedAt: "2026-08-27T10:00:00Z",
        projectPath: "/tmp/docs",
      }),
    ];
    expect(
      filterInboxByProject(rows, ["/tmp/web/"]).map((row) => row.number),
    ).toEqual([2]);
  });

  it("keeps Linear issues that are not tied to a folder", () => {
    const rows = [
      item({
        number: 1,
        updatedAt: "2026-08-27T10:00:00Z",
        projectPath: "/tmp/web",
      }),
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

describe("linearProjectOptions", () => {
  function linearItem(number: number, project?: { id: string; name: string }) {
    return item({
      number,
      kind: "linear",
      provider: "linear",
      projectPath: "",
      projectId: project?.id ?? "",
      projectName: project?.name ?? "",
      updatedAt: "2026-08-27T10:00:00Z",
    });
  }

  it("collects distinct projects sorted by name", () => {
    const rows = [
      linearItem(1, { id: "p2", name: "Onboarding" }),
      linearItem(2, { id: "p1", name: "Billing" }),
      linearItem(3, { id: "p1", name: "Billing" }),
    ];
    expect(linearProjectOptions(rows)).toEqual([
      { id: "p1", name: "Billing" },
      { id: "p2", name: "Onboarding" },
    ]);
  });

  it("appends a No project row when an issue sits outside every project", () => {
    const rows = [linearItem(1, { id: "p1", name: "Billing" }), linearItem(2)];
    expect(linearProjectOptions(rows)).toEqual([
      { id: "p1", name: "Billing" },
      { id: LINEAR_NO_PROJECT, name: "No project" },
    ]);
  });

  it("ignores GitHub items", () => {
    const rows = [item({ number: 1, updatedAt: "2026-08-27T10:00:00Z" })];
    expect(linearProjectOptions(rows)).toEqual([]);
  });

  it("falls back to the id when a project has no name", () => {
    expect(
      linearProjectOptions([linearItem(1, { id: "p1", name: "" })]),
    ).toEqual([{ id: "p1", name: "p1" }]);
  });
});

describe("filterInboxByLinearProject", () => {
  const rows = [
    item({
      number: 1,
      kind: "linear",
      provider: "linear",
      projectPath: "",
      projectId: "p1",
      projectName: "Billing",
      updatedAt: "2026-08-27T10:00:00Z",
    }),
    item({
      number: 2,
      kind: "linear",
      provider: "linear",
      projectPath: "",
      projectId: "",
      projectName: "",
      updatedAt: "2026-08-27T10:00:00Z",
    }),
    item({ number: 3, updatedAt: "2026-08-27T10:00:00Z" }),
  ];

  it("keeps everything when nothing is hidden", () => {
    expect(
      filterInboxByLinearProject(rows, []).map((row) => row.number),
    ).toEqual([1, 2, 3]);
  });

  it("hides the selected project", () => {
    expect(
      filterInboxByLinearProject(rows, ["p1"]).map((row) => row.number),
    ).toEqual([2, 3]);
  });

  it("hides project-less issues via the No project sentinel", () => {
    expect(
      filterInboxByLinearProject(rows, [LINEAR_NO_PROJECT]).map(
        (row) => row.number,
      ),
    ).toEqual([1, 3]);
  });

  it("never hides GitHub items", () => {
    expect(
      filterInboxByLinearProject(rows, ["p1", LINEAR_NO_PROJECT]).map(
        (row) => row.number,
      ),
    ).toEqual([3]);
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
    expect(
      filterInboxByKind(rows, ["linear"]).map((row) => row.number),
    ).toEqual([1, 2]);
  });
});

describe("filterInboxByProvider", () => {
  it("keeps GitHub, GitLab, or Linear items", () => {
    const rows = [
      item({ number: 1, updatedAt: "2026-08-27T10:00:00Z" }),
      item({
        number: 2,
        provider: "gitlab",
        updatedAt: "2026-08-27T10:00:00Z",
      }),
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
    expect(
      filterInboxByProvider(rows, "gitlab").map((row) => row.number),
    ).toEqual([2]);
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

  it("is true when a Linear project is hidden on the Linear tab", () => {
    expect(
      hasActiveInboxFilters(
        { ...DEFAULT_INBOX_FILTERS, hiddenLinearProjects: ["p1"] },
        "linear",
      ),
    ).toBe(true);
  });

  it("ignores hidden Linear projects on the GitHub tab", () => {
    expect(
      hasActiveInboxFilters(
        { ...DEFAULT_INBOX_FILTERS, hiddenLinearProjects: ["p1"] },
        "github",
      ),
    ).toBe(false);
  });

  it("is true when a Linear team is hidden on the Linear tab", () => {
    expect(hasActiveInboxFilters(DEFAULT_INBOX_FILTERS, "linear", ["t1"])).toBe(
      true,
    );
  });

  it("ignores hidden Linear teams on the GitHub tab", () => {
    expect(hasActiveInboxFilters(DEFAULT_INBOX_FILTERS, "github", ["t1"])).toBe(
      false,
    );
  });

  it("is false on the Linear tab when no team is hidden", () => {
    expect(hasActiveInboxFilters(DEFAULT_INBOX_FILTERS, "linear", [])).toBe(
      false,
    );
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

describe("visibleInboxSources", () => {
  it("drops sources that are known to be disconnected", () => {
    expect(
      visibleInboxSources({ github: false, linear: false, gitlab: false }),
    ).toEqual([]);
    expect(
      visibleInboxSources({ github: true, linear: false, gitlab: false }),
    ).toEqual(["github"]);
    expect(
      visibleInboxSources({ github: false, linear: true, gitlab: false }),
    ).toEqual(["linear"]);
    expect(
      visibleInboxSources({ github: true, linear: true, gitlab: true }),
    ).toEqual(["github", "linear", "gitlab"]);
  });

  it("keeps unresolved sources visible so tabs do not flash away", () => {
    expect(
      visibleInboxSources({ github: null, linear: null, gitlab: null }),
    ).toEqual(["github", "linear", "gitlab"]);
  });
});

describe("connectableInboxSources", () => {
  it("offers only the sources confirmed to be disconnected", () => {
    expect(
      connectableInboxSources({ github: true, linear: false, gitlab: true }),
    ).toEqual(["linear"]);
    expect(
      connectableInboxSources({
        github: false,
        linear: false,
        gitlab: false,
      }),
    ).toEqual(["github", "linear", "gitlab"]);
  });

  it("offers nothing while the checks are unresolved", () => {
    expect(
      connectableInboxSources({ github: null, linear: null, gitlab: null }),
    ).toEqual([]);
  });
});

describe("resolveInboxSource", () => {
  it("falls back to the first visible source when the selection disconnects", () => {
    expect(
      resolveInboxSource("linear", {
        github: true,
        linear: false,
        gitlab: true,
      }),
    ).toBe("github");
    expect(
      resolveInboxSource("github", {
        github: false,
        linear: false,
        gitlab: true,
      }),
    ).toBe("gitlab");
  });

  it("keeps GitHub as an internal fallback when every source is disconnected", () => {
    expect(
      resolveInboxSource("linear", {
        github: false,
        linear: false,
        gitlab: false,
      }),
    ).toBe("github");
  });

  it("leaves a still-visible selection alone", () => {
    expect(
      resolveInboxSource("linear", {
        github: false,
        linear: true,
        gitlab: false,
      }),
    ).toBe("linear");
    expect(
      resolveInboxSource("github", {
        github: true,
        linear: false,
        gitlab: false,
      }),
    ).toBe("github");
  });
});

function mockLocalStorage() {
  const data = new Map<string, string>();
  const storage = {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value);
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => {
      data.clear();
    },
    key: (index: number) => [...data.keys()][index] ?? null,
    get length() {
      return data.size;
    },
  };
  Object.defineProperty(globalThis, "localStorage", {
    value: storage,
    configurable: true,
  });
}

describe("inbox connection cache", () => {
  const KEY = "monocode.inboxConnections";
  beforeEach(mockLocalStorage);

  it("round-trips the last known connect state", () => {
    saveInboxConnections({ github: true, linear: true, gitlab: false });
    expect(loadInboxConnections()).toEqual({
      github: true,
      linear: true,
      gitlab: false,
    });
  });

  it("reads unknown when nothing is stored", () => {
    expect(loadInboxConnections()).toEqual({
      github: null,
      linear: null,
      gitlab: null,
    });
  });

  it("reads unknown rather than trusting a malformed value", () => {
    localStorage.setItem(KEY, "not json");
    expect(loadInboxConnections()).toEqual({
      github: null,
      linear: null,
      gitlab: null,
    });
    localStorage.setItem(KEY, '{"linear":"yes"}');
    expect(loadInboxConnections()).toEqual({
      github: null,
      linear: null,
      gitlab: null,
    });
  });
});
