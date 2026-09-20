import { describe, expect, it } from "vitest";
import { leaf, newTab, newTerminalFile } from "../../workspace/model/layout";
import {
  addTerminalToDock,
  applyDockGridStyle,
  clampDockSize,
  closeTerminalInDock,
  createProjectTerminal,
  dockGridStyle,
  findProjectTerminal,
  mapProjectTerminal,
  nextDockTerminalTitle,
  patchProjectTerminals,
  projectTerminalFileIds,
  selectDockTerminal,
  splitProjectTerminalsForMove,
  withDockOpen,
  withDockSide,
} from "./projectTerminal";
import type { Session } from "../../sessions/model/session";

function chat(id: string, cwd: string): Session {
  return {
    id,
    cwd,
    harness: "cursor",
    title: "",
    blocks: [],
    busy: false,
    model: "",
    modelSettings: {},
    runtimeMode: "default",
  };
}

describe("createProjectTerminal", () => {
  it("opens a bottom dock with the first terminal focused", () => {
    const file = newTerminalFile("/tmp/a");
    const dock = createProjectTerminal("/tmp/a/", file);
    expect(dock.projectPath).toBe("/tmp/a");
    expect(dock.side).toBe("bottom");
    expect(dock.open).toBe(true);
    expect(dock.pane.files).toEqual([file]);
    expect(dock.pane.activeFileId).toBe(file.id);
  });
});

describe("addTerminalToDock", () => {
  it("appends a tab, focuses it, and reveals a hidden dock", () => {
    const first = newTerminalFile("/tmp/a", "a");
    const second = newTerminalFile("/tmp/a", "a 2");
    const dock = withDockOpen(createProjectTerminal("/tmp/a", first), false);
    const next = addTerminalToDock(dock, second);
    expect(next.open).toBe(true);
    expect(next.pane.files.map((file) => file.id)).toEqual([
      first.id,
      second.id,
    ]);
    expect(next.pane.activeFileId).toBe(second.id);
    expect(nextDockTerminalTitle(next, "/tmp/a")).toBe("a 3");
  });
});

describe("closeTerminalInDock", () => {
  it("drops the dock when the last terminal closes", () => {
    const file = newTerminalFile("/tmp/a");
    const dock = createProjectTerminal("/tmp/a", file);
    expect(closeTerminalInDock(dock, file.id)).toBeNull();
  });

  it("focuses a neighbor after closing one of several terminals", () => {
    const first = newTerminalFile("/tmp/a", "one");
    const second = newTerminalFile("/tmp/a", "two");
    const dock = addTerminalToDock(
      createProjectTerminal("/tmp/a", first),
      second,
    );
    const next = closeTerminalInDock(dock, second.id);
    expect(next?.pane.files.map((file) => file.id)).toEqual([first.id]);
    expect(next?.pane.activeFileId).toBe(first.id);
  });
});

describe("mapProjectTerminal", () => {
  it("updates only the matching project and can remove it", () => {
    const alpha = createProjectTerminal("/tmp/a", newTerminalFile("/tmp/a"));
    const beta = createProjectTerminal("/tmp/b", newTerminalFile("/tmp/b"));
    const docks = [alpha, beta];
    expect(findProjectTerminal(docks, "/tmp/a/")?.pane.id).toBe(alpha.pane.id);

    const hidden = mapProjectTerminal(docks, "/tmp/a", (dock) =>
      withDockOpen(dock, false),
    );
    expect(hidden[0]?.open).toBe(false);
    expect(hidden[1]?.open).toBe(true);

    expect(
      mapProjectTerminal(docks, "/tmp/a", () => null).map(
        (dock) => dock.projectPath,
      ),
    ).toEqual(["/tmp/b"]);
  });
});

describe("patchProjectTerminals", () => {
  it("renames a terminal by id without touching other docks", () => {
    const file = newTerminalFile("/tmp/a", "zsh");
    const other = newTerminalFile("/tmp/b", "other");
    const docks = [
      createProjectTerminal("/tmp/a", file),
      createProjectTerminal("/tmp/b", other),
    ];
    const next = patchProjectTerminals(docks, file.id, {
      title: "npm",
      cwd: "/tmp/a/app",
    });
    expect(next[0]?.pane.files[0]).toMatchObject({
      path: "npm",
      cwd: "/tmp/a/app",
    });
    expect(next[1]?.pane.files[0]?.path).toBe("other");
    expect(selectDockTerminal(next[0]!, other.id)).toBe(next[0]);
  });

  it("records a foreground process without renaming other docks", () => {
    const file = newTerminalFile("/tmp/a", "zsh");
    const docks = [createProjectTerminal("/tmp/a", file)];
    const next = patchProjectTerminals(docks, file.id, {
      title: "vite",
      foreground: "vite",
    });
    expect(next[0]?.pane.files[0]).toMatchObject({
      path: "vite",
      foreground: "vite",
    });
    expect(
      patchProjectTerminals(next, file.id, { foreground: null })[0]?.pane
        .files[0]?.foreground,
    ).toBeUndefined();
  });
});

describe("clampDockSize", () => {
  it("clamps to the axis min and 70% of the viewport", () => {
    expect(clampDockSize("bottom", 10, { width: 1000, height: 400 })).toBe(88);
    expect(clampDockSize("left", 900, { width: 1000, height: 400 })).toBe(700);
  });
});

describe("withDockSide", () => {
  it("keeps size when the axis stays the same", () => {
    const dock = { ...createProjectTerminal("/tmp/a", newTerminalFile("/tmp/a")), size: 200 };
    expect(withDockSide(dock, "top").size).toBe(200);
    expect(withDockSide(dock, "top").side).toBe("top");
  });
});

describe("dockGridStyle", () => {
  it("collapses to a single main area when hidden", () => {
    expect(dockGridStyle(null, 220).gridTemplateAreas).toBe('"main"');
  });

  it("places the dock on the requested edge", () => {
    expect(dockGridStyle("bottom", 220).gridTemplateAreas).toBe('"main" "dock"');
    expect(dockGridStyle("top", 220).gridTemplateAreas).toBe('"dock" "main"');
    expect(dockGridStyle("left", 360).gridTemplateAreas).toBe('"dock main"');
    expect(dockGridStyle("right", 360).gridTemplateAreas).toBe('"main dock"');
  });

  it("writes the same template onto an element", () => {
    const el = { style: {} } as unknown as HTMLElement;
    applyDockGridStyle(el, "left", 300);
    expect(el.style.gridTemplateAreas).toBe('"dock main"');
    expect(el.style.gridTemplateColumns).toBe("300px minmax(0, 1fr)");
  });
});

describe("projectTerminalFileIds", () => {
  it("lists every terminal in every dock", () => {
    const a = newTerminalFile("/tmp/a");
    const b = newTerminalFile("/tmp/b");
    expect(
      projectTerminalFileIds([
        createProjectTerminal("/tmp/a", a),
        createProjectTerminal("/tmp/b", b),
      ]),
    ).toEqual([a.id, b.id]);
  });
});

describe("splitProjectTerminalsForMove", () => {
  it("moves a dock only when every tab of that project is leaving", () => {
    const a1 = newTab("s1");
    const a2 = newTab("s2");
    const b1 = { ...newTab("s3"), layout: leaf("s3") };
    const sessions = [
      chat("s1", "/tmp/a"),
      chat("s2", "/tmp/a"),
      chat("s3", "/tmp/b"),
    ];
    const docks = [
      createProjectTerminal("/tmp/a", newTerminalFile("/tmp/a")),
      createProjectTerminal("/tmp/b", newTerminalFile("/tmp/b")),
    ];

    const partial = splitProjectTerminalsForMove(
      docks,
      [a1],
      [a2, b1],
      sessions,
    );
    expect(partial.moving).toEqual([]);
    expect(partial.remaining.map((dock) => dock.projectPath)).toEqual([
      "/tmp/a",
      "/tmp/b",
    ]);

    const allA = splitProjectTerminalsForMove(docks, [a1, a2], [b1], sessions);
    expect(allA.moving.map((dock) => dock.projectPath)).toEqual(["/tmp/a"]);
    expect(allA.remaining.map((dock) => dock.projectPath)).toEqual(["/tmp/b"]);
  });
});
