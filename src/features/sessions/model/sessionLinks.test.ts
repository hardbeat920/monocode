import { describe, expect, it } from "vitest";
import {
  areLinked,
  linkedMessagePrompt,
  peersOf,
  type SessionLink,
} from "./sessionLinks";

const links: SessionLink[] = [
  { a: "a", b: "b", aTitle: "Alpha", bTitle: "Beta", agentMessages: 0, createdAt: 1 },
  { a: "b", b: "c", aTitle: "Beta", bTitle: "Gamma", agentMessages: 2, createdAt: 2 },
];

describe("session links", () => {
  it("lists the peers on either side of a link", () => {
    expect(peersOf(links, "b")).toEqual([
      { id: "a", title: "Alpha" },
      { id: "c", title: "Gamma" },
    ]);
    expect(peersOf(links, "z")).toEqual([]);
    expect(areLinked(links, "c", "b")).toBe(true);
    expect(areLinked(links, "a", "c")).toBe(false);
  });

  it("tells the receiving agent who sent the message and how to answer", () => {
    const prompt = linkedMessagePrompt({ id: "a", title: 'Say "hi"' }, "  Auth is done. ");
    expect(prompt).toContain('from_session_id="a"');
    expect(prompt).toContain("from_title=\"Say 'hi'\"");
    expect(prompt).toContain("not from the user");
    expect(prompt).toContain("links.send");
    expect(prompt).toContain("\nAuth is done.\n");
  });
});
