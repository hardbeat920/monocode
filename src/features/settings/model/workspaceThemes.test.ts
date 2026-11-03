// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => path,
  invoke: vi.fn().mockResolvedValue(undefined),
}));
import {
  applyWorkspaceTheme,
  loadWorkspaceTheme,
  saveWorkspaceTheme,
} from "./appearance";
import {
  WORKSPACE_THEMES,
  WORKSPACE_THEME_DEFAULT,
  WORKSPACE_THEME_VARIABLE_NAMES,
  workspaceThemeVariables,
} from "./workspaceThemes";

const root = document.documentElement;

afterEach(() => {
  localStorage.clear();
  applyWorkspaceTheme(WORKSPACE_THEME_DEFAULT);
});

describe("workspace themes", () => {
  it("gives every theme a unique id and complete hex palettes", () => {
    const ids = WORKSPACE_THEMES.map((theme) => theme.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const theme of WORKSPACE_THEMES) {
      for (const palette of [theme.dark, theme.light]) {
        const { syntax, ...shell } = palette;
        for (const color of [
          ...Object.values(shell),
          ...Object.values(syntax),
        ]) {
          expect(color).toMatch(/^#[0-9a-f]{6}$/);
        }
      }
    }
  });

  it("sets no variables for the default theme", () => {
    expect(workspaceThemeVariables(WORKSPACE_THEME_DEFAULT)).toEqual({});
  });

  it("round-trips the stored theme and falls back on unknown ids", () => {
    expect(loadWorkspaceTheme()).toBe(WORKSPACE_THEME_DEFAULT);
    saveWorkspaceTheme("moss");
    expect(loadWorkspaceTheme()).toBe("moss");
    localStorage.setItem("monocode.workspaceTheme", "not-a-theme");
    expect(loadWorkspaceTheme()).toBe(WORKSPACE_THEME_DEFAULT);
  });

  it("applies one theme's variables and clears them on switching back", () => {
    applyWorkspaceTheme("ember");
    expect(root.classList.contains("has-workspace-theme")).toBe(true);
    expect(root.style.getPropertyValue("--wt-dark-surface")).toBe("#120e0c");
    expect(root.style.getPropertyValue("--wt-light-panel-solid")).toBe(
      "#ffffff",
    );
    expect(root.style.getPropertyValue("--wt-dark-syntax-keyword")).toBe(
      "#fe8019",
    );

    applyWorkspaceTheme(WORKSPACE_THEME_DEFAULT);
    expect(root.classList.contains("has-workspace-theme")).toBe(false);
    for (const name of WORKSPACE_THEME_VARIABLE_NAMES) {
      expect(root.style.getPropertyValue(name)).toBe("");
    }
  });
});
