// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { activityTimeline, InboxComments } from "./InboxComments";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

const comment = (id: string, createdAt: string) => ({
  id,
  kind: "comment",
  author: "maya",
  body: id,
  createdAt,
  url: "",
  state: "",
  path: "",
  line: null,
  resolved: false,
  replies: [],
});
const commit = (oid: string, author: string, committedDate: string) => ({
  oid,
  messageHeadline: oid,
  author,
  committedDate,
  url: "",
});

describe("activityTimeline", () => {
  it("interleaves comments and commits oldest first", () => {
    const entries = activityTimeline({
      truncated: false,
      comments: [
        comment("review", "2026-10-03T12:00:00Z"),
        comment("opened", "2026-10-03T09:00:00Z"),
      ],
      commits: [
        commit("c1", "maya", "2026-10-03T10:00:00Z"),
        commit("c2", "maya", "2026-10-03T10:05:00Z"),
        commit("c3", "jonas", "2026-10-03T10:10:00Z"),
        commit("c4", "maya", "2026-10-03T13:00:00Z"),
      ],
    });
    expect(
      entries.map((entry) =>
        entry.kind === "comment" ? entry.comment.id : entry.commit.oid,
      ),
    ).toEqual(["opened", "c1", "c2", "c3", "review", "c4"]);
  });
});

describe("InboxComments timeline", () => {
  it("puts a push and a bare approval on the rail between comments", () => {
    (
      globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
    ).IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    const root = createRoot(container);
    act(() =>
      root.render(
        createElement(InboxComments, {
          thread: {
            truncated: false,
            comments: [
              comment(
                "Can we handle concurrent retries?",
                "2026-10-03T09:00:00Z",
              ),
              {
                ...comment("", "2026-10-03T12:00:00Z"),
                id: "approval",
                kind: "review",
                author: "priya",
                state: "APPROVED",
              },
            ],
            commits: [
              commit("abc1234def", "maya", "2026-10-03T10:00:00Z"),
              commit("def5678abc", "maya", "2026-10-03T10:05:00Z"),
            ],
          },
          loading: false,
          error: null,
          cwd: "/repo",
          provider: "github",
        }),
      ),
    );
    const text = container.textContent ?? "";
    expect(text).toContain("Activity");
    expect(text).toContain("1 comment · 2 commits");
    expect(text.indexOf("concurrent retries")).toBeLessThan(
      text.indexOf("added 2 commits"),
    );
    expect(text.indexOf("abc1234")).toBeLessThan(text.indexOf("priya"));
    expect(text).toContain("priyaapproved");
    act(() => root.unmount());
  });
});
