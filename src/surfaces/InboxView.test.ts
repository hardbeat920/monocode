import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { InboxItem } from "../lib/githubTasks";
import { InboxDetail } from "./InboxView";

function item(overrides: Partial<InboxItem> = {}): InboxItem {
  return {
    kind: "issue",
    title: "A long inbox issue",
    url: "https://github.com/acme/web/issues/157",
    state: "open",
    updatedAt: "2026-09-11T08:00:00Z",
    labels: [],
    assignees: [],
    draft: false,
    repo: "acme/web",
    number: 157,
    projectPath: "/tmp/web",
    provider: "github",
    ...overrides,
  };
}

function renderDetail(inboxItem: InboxItem) {
  return renderToStaticMarkup(
    createElement(InboxDetail, {
      item: inboxItem,
      cwd: "/tmp/web",
      projects: [],
      revision: 0,
      relatedSessions: [],
      onDiscuss: () => {},
      onStart: () => {},
    }),
  );
}

describe("InboxDetail layout", () => {
  it("keeps issue identity and actions outside the body scroller", () => {
    const markup = renderDetail(item());
    const headerIndex = markup.indexOf("data-inbox-detail-header");
    const scrollIndex = markup.indexOf("data-inbox-detail-scroll");
    const header = markup.slice(headerIndex, scrollIndex);
    const body = markup.slice(scrollIndex);

    expect(headerIndex).toBeGreaterThan(-1);
    expect(scrollIndex).toBeGreaterThan(headerIndex);
    expect(header).toContain("line-clamp-2");
    expect(header).toContain('title="A long inbox issue"');
    expect(header).toContain("Send to agent");
    expect(header).toContain("Ask");
    expect(header).toContain("Open on GitHub");
    expect(header).not.toContain("overflow-y-auto");
    expect(body).toContain("overflow-y-auto");
    expect(body).toContain("Unassigned");
  });

  it("keeps pull request tabs in the pinned header", () => {
    const markup = renderDetail(item({ kind: "pr" }));
    const headerIndex = markup.indexOf("data-inbox-detail-header");
    const scrollIndex = markup.indexOf("data-inbox-detail-scroll");
    const header = markup.slice(headerIndex, scrollIndex);

    expect(header).toContain('aria-label="Pull request sections"');
    expect(header).toContain("Summary");
    expect(header).toContain("Code");
  });

  it("keeps the Linear project picker beside the pinned send action", () => {
    const markup = renderDetail(
      item({
        provider: "linear",
        kind: "linear",
        id: "linear-157",
        identifier: "ENG-157",
        teamName: "Engineering",
      }),
    );
    const headerIndex = markup.indexOf("data-inbox-detail-header");
    const scrollIndex = markup.indexOf("data-inbox-detail-scroll");
    const header = markup.slice(headerIndex, scrollIndex);

    expect(header).toContain("Send to agent");
    expect(header).toContain("Choose project");
    expect(header).not.toContain("overflow-y-auto");
  });
});
