import { beforeEach, describe, expect, it } from "vitest";
import { projectKey } from "../../../shared/lib/paths";
import {
  loadSessionFolders,
  saveSessionFolders,
} from "../../sessions/model/sessionFolders";
import {
  loadTabGroupLabels,
  saveTabGroupLabel,
} from "../../workspace/model/tabGroups";
import {
  loadProjectChatBackgroundSettings,
  saveProjectChatBackgroundSettings,
} from "./projectChatBackground";
import { rebaseProjectData } from "./projectData";
import {
  loadProjectGroupAssignments,
  saveProjectGroups,
  setProjectGroupAssignment,
} from "./projectGroups";

function mockBrowserStorage() {
  const data = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => data.get(key) ?? null,
      setItem: (key: string, value: string) => data.set(key, value),
      removeItem: (key: string) => data.delete(key),
      clear: () => data.clear(),
      key: (index: number) => [...data.keys()][index] ?? null,
      get length() {
        return data.size;
      },
    },
  });
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: new EventTarget(),
  });
}

beforeEach(mockBrowserStorage);

describe("rebaseProjectData", () => {
  it("moves path-keyed project settings to the renamed folder", () => {
    const from = "/work/monocode";
    const to = "/work/monocode-personal";
    const oldKey = projectKey(from);
    const newKey = projectKey(to);

    saveProjectGroups([{ id: "personal", name: "Personal", collapsed: false }]);
    setProjectGroupAssignment(from, "personal");
    saveTabGroupLabel(oldKey, "My MonoCode");
    saveProjectChatBackgroundSettings(oldKey, {
      path: "/images/background.png",
      emptyOpacity: 0.2,
      sessionOpacity: 0.1,
      scope: "all",
      effect: "dither",
    });
    saveSessionFolders(from, [
      {
        id: "folder-1",
        name: "Active",
        sessionIds: ["session-1"],
        collapsed: false,
      },
    ]);

    rebaseProjectData(from, to);

    expect(loadTabGroupLabels()).toEqual({ [newKey]: "My MonoCode" });
    expect(loadProjectGroupAssignments()).toEqual({ [to]: "personal" });
    expect(loadProjectChatBackgroundSettings(oldKey)).toBeNull();
    expect(loadProjectChatBackgroundSettings(newKey)?.path).toBe(
      "/images/background.png",
    );
    expect(loadProjectChatBackgroundSettings(newKey)?.effect).toBe("dither");
    expect(loadSessionFolders(from)).toEqual([]);
    expect(loadSessionFolders(to)[0]?.name).toBe("Active");
  });
});
