import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { InboxItem } from "../lib/githubTasks";
import type { SessionSummary } from "../lib/sessionStore";
import { InboxDetail, inboxShowsFullFileDiff } from "./InboxView";

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

function renderDetail(
  inboxItem: InboxItem,
  relatedSessions: SessionSummary[] = [],
) {
  return renderToStaticMarkup(
    createElement(InboxDetail, {
      item: inboxItem,
      cwd: "/tmp/web",
      projects: [],
      revision: 0,
      relatedSessions,
      onDiscuss: () => {},
      onStart: () => {},
    }),
  );
}

describe("InboxDetail layout", () => {
  it("keeps issue identity and actions outside the body scroller", () => {
    const markup = renderDetail(item({ projectPath: "/tmp/local-project" }));
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
    expect(header).toContain("Unassigned");
    expect(header).toContain("whitespace-nowrap");
    expect(header).not.toContain("local-project");
    expect(header).not.toContain("overflow-y-auto");
    expect(header).not.toContain("bg-background-base");
    expect(body).toContain("overflow-y-auto");
    expect(body).not.toContain("Unassigned");
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

  it("offers full-file diffs only for GitHub pull requests", () => {
    expect(inboxShowsFullFileDiff(item({ kind: "pr" }))).toBe(true);
    expect(
      inboxShowsFullFileDiff(
        item({
          kind: "pr",
          provider: "gitlab",
          repo: "acme/platform",
          url: "https://gitlab.example.com/acme/platform/-/merge_requests/12",
        }),
      ),
    ).toBe(false);
    expect(inboxShowsFullFileDiff(item({ kind: "issue" }))).toBe(false);
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

  it("shows why a remote GitLab item needs attention and asks for a workspace", () => {
    const markup = renderDetail(
      item({
        provider: "gitlab",
        repo: "acme/platform",
        projectPath: "",
        url: "https://gitlab.example.com/acme/platform/-/issues/157",
        attentionReason: "mentioned",
      }),
    );
    const headerIndex = markup.indexOf("data-inbox-detail-header");
    const scrollIndex = markup.indexOf("data-inbox-detail-scroll");
    const header = markup.slice(headerIndex, scrollIndex);

    expect(header).toContain("Mentioned you");
    expect(header).toContain("Choose project");
    expect(header).toContain("Open on GitLab");
  });

  it("keeps related threads in the pinned header", () => {
    const markup = renderDetail(item(), [
      {
        id: "session-1",
        cwd: "/tmp/web",
        harness: "codex",
        model: "gpt-5",
        runtimeMode: "supervised",
        title: "Review MonoCode Pull Request",
        createdAt: 1,
        updatedAt: 1,
      },
    ]);
    const headerIndex = markup.indexOf("data-inbox-detail-header");
    const scrollIndex = markup.indexOf("data-inbox-detail-scroll");
    const header = markup.slice(headerIndex, scrollIndex);
    const body = markup.slice(scrollIndex);

    expect(header).toContain("Related thread");
    expect(header).toContain("Review MonoCode Pull Request");
    expect(body).not.toContain("Review MonoCode Pull Request");
  });
});
