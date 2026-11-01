import { describe, expect, it } from "vitest";
import {
  normalizeSelection,
  selectedWorkspace,
} from "@/features/connection/selection";
import { parseInvitation } from "@/features/connection/invitation";
const catalog = {
  projects: [{ id: "a" }, { id: "b" }, { id: "c" }],
  monos: [
    { id: "m", projectIds: ["b"] },
    { id: "both", projectIds: ["a", "b"] },
    { id: "loose", projectIds: [] },
    { id: "far", projectIds: ["c"] },
  ],
  sessions: [
    { id: "s1", projectId: "a" },
    { id: "s2", projectId: "b" },
    { id: "s3", projectId: "b", monoId: "m" },
    { id: "s4", projectId: "c", monoId: "m" },
  ],
};
describe("mobile workspace choices", () => {
  it("shows a Mono whenever any of its projects is picked, and one with no project always", () => {
    const selected = selectedWorkspace(catalog, { projectIds: ["a"], monoIds: [] });
    expect(selected.projects).toEqual([{ id: "a" }]);
    expect(selected.monos.map((mono) => mono.id)).toEqual(["both", "loose"]);
    expect(
      selectedWorkspace(catalog, { projectIds: ["b"], monoIds: [] }).monos.map((mono) => mono.id),
    ).toEqual(["m", "both", "loose"]);
    expect(selectedWorkspace(catalog, null).monos.map((mono) => mono.id)).toEqual(["loose"]);
  });
  it("never shows a Mono-started session for a project that is not picked", () => {
    const selected = selectedWorkspace(catalog, { projectIds: ["b"], monoIds: [] });
    // s4 is a's Mono's, started in c, which is not picked.
    expect(selected.sessions.map((session) => session.id)).toEqual(["s2", "s3"]);
  });
  it("ignores the monoIds an older phone saved", () => {
    const selected = selectedWorkspace(catalog, { projectIds: ["a"], monoIds: ["far", "m"] });
    expect(selected.monos.map((mono) => mono.id)).toEqual(["both", "loose"]);
    expect(selected.sessions.map((session) => session.id)).toEqual(["s1"]);
  });
  it("preserves an explicit empty choice on restore", () => {
    const restored = normalizeSelection(
      JSON.parse(JSON.stringify({ projectIds: [], monoIds: [] })),
    );
    expect(restored).toEqual({ projectIds: [], monoIds: [] });
    expect(selectedWorkspace(catalog, restored).projects).toEqual([]);
    expect(normalizeSelection({ projectIds: "all", monoIds: [] })).toBeNull();
  });
  it("retains unavailable choices so they can reappear when a desktop project returns", () => {
    const chosen = normalizeSelection({
      projectIds: ["a", "a", 5, "later"],
      monoIds: ["m"],
    });
    expect(chosen?.projectIds).toEqual(["a", "later"]);
    expect(chosen?.monoIds).toEqual(["m"]);
    expect(
      selectedWorkspace(
        { ...catalog, projects: [...catalog.projects, { id: "later" }] },
        chosen,
      ).projects,
    ).toEqual([{ id: "a" }, { id: "later" }]);
  });
});
const invitation = {
  version: 1,
  name: "Desktop 🖥",
  fingerprint: "a".repeat(64),
  addresses: [{ host: "desktop.example", port: 38416 }],
  identity: { certificate: "certificate", privateKey: "temporary-key" },
  expires: Date.now() / 1000 + 900,
};
function link(value: unknown) {
  return (
    "monocode://pair?i=" +
    Buffer.from(JSON.stringify(value)).toString("base64url")
  );
}
describe("QR invitation", () => {
  it("accepts the one-use desktop scheme and UTF-8 names", () =>
    expect(parseInvitation(link(invitation))).toEqual(invitation));
  it("rejects expired codes, foreign links and invalid endpoints before pairing", () => {
    expect(() => parseInvitation(link({ ...invitation, expires: 1 }))).toThrow(
      "expired",
    );
    expect(() => parseInvitation("https://example.com/?i=abc")).toThrow();
    expect(() =>
      parseInvitation(
        link({ ...invitation, addresses: [{ host: "host", port: 0 }] }),
      ),
    ).toThrow();
    expect(() =>
      parseInvitation(link({ ...invitation, fingerprint: "bad" })),
    ).toThrow();
  });
});
