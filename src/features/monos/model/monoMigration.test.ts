// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

function stubStorage(entries: [string, string][]) {
  const stored = new Map(entries);
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
    removeItem: (key: string) => stored.delete(key),
  });
  return stored;
}

it("turns each project's Mono into one of its own that works on that project", async () => {
  const stored = stubStorage([
    ["monocode:project-agents", JSON.stringify({ "/code/app": "chat-1" })],
    [
      "monocode:project-agent-profiles",
      JSON.stringify({
        "/code/app": { name: "Skull", claim: "claimed" },
        "/code/site": { claim: "claimed" },
        "/code/off": { claim: "declined" },
      }),
    ],
    ["monocode:tab-group:mascots", JSON.stringify({ "/code/app": "skull" })],
  ]);
  const { isMonoSession, listMonos, monoForSession } = await import("./mono");

  const monos = listMonos();
  expect(monos.map((mono) => mono.projects)).toEqual([
    ["/code/app"],
    ["/code/site"],
  ]);
  expect(monoForSession("chat-1")).toMatchObject({
    name: "Skull",
    mascot: "skull",
    projects: ["/code/app"],
    legacyProject: "/code/app",
  });
  expect(isMonoSession("chat-1")).toBe(true);
  expect(monos[1].sessionId).toBeUndefined();
  expect(
    [...stored.keys()].filter((key) => /agent|monos|profiles/.test(key)),
  ).toEqual([]);
});

it("migrates only once, so a deleted Mono stays deleted", async () => {
  stubStorage([["monocode:monos", JSON.stringify({ "/code/app": "chat-1" })]]);
  const mono = await import("./mono");
  const [only] = mono.listMonos();
  mono.removeMono(only.id);
  vi.resetModules();
  const again = await import("./mono");
  expect(again.listMonos()).toEqual([]);
});
