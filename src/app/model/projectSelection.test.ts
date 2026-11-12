// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createMono, saveMonoSessionId } from "../../features/monos/model/mono";
import { selectProjectWithMonoLanding } from "./projectSelection";

beforeEach(() => {
  const stored = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
  });
});
afterEach(() => vi.unstubAllGlobals());

it("hands the pre-close remembered Mono to project landing for legacy rosters", () => {
  const first = createMono(["/project"]);
  const second = createMono(["/project"]);
  saveMonoSessionId(first.id, "first-session");
  saveMonoSessionId(second.id, "second-session");
  let openMonoId: string | undefined = second.id;
  let restored: (() => void) | undefined;
  const openMono = vi.fn();

  selectProjectWithMonoLanding("/project", {
    loadOpenMono: () => openMonoId,
    closeMonoView: () => {
      openMonoId = undefined;
    },
    cancelNavigation: vi.fn(),
    openProject: vi.fn(),
    selectProject: (_path, onRestored) => {
      restored = onRestored;
    },
    monosEnabled: () => true,
    landingEnabled: () => true,
    openMono,
  });

  expect(openMonoId).toBeUndefined();
  expect(openMono).not.toHaveBeenCalled();
  restored?.();
  expect(openMono).toHaveBeenCalledExactlyOnceWith(second.id);
});
