import { invoke } from "@tauri-apps/api/core";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearInboxCache,
  githubWorkItemDetails,
  type InboxItem,
} from "../model/githubTasks";
import type { LinkedWorkItem } from "../../sessions/model/session";
import type { SessionSummary } from "../../sessions/data/sessionStore";
import {
  InboxDetail,
  inboxShowsFullFileDiff,
  LinkedWorkItemPanel,
} from "./InboxView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

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
  it("renders a linked item as a standalone, closable side panel", () => {
    const target: LinkedWorkItem = {
      kind: "issue",
      repo: "acme/web",
      number: 157,
      url: "https://github.com/acme/web/issues/157",
    };
    const markup = renderToStaticMarkup(
      createElement(LinkedWorkItemPanel, {
        target,
        cwd: "/tmp/web",
        recents: [],
        onClose: () => {},
      }),
    );

    expect(markup).toContain("data-linked-work-item-panel");
    expect(markup).toContain('aria-label="Linked issue #157"');
    expect(markup).toContain('aria-label="Resize linked issue panel"');
    expect(markup).toContain('title="Close issue panel"');
    expect(markup).not.toContain("data-app-inbox");
    expect(markup).not.toContain("bg-background-base");
    expect(markup).not.toContain("backdrop-blur");
  });

  it("shows when a PR was created alongside its last update", () => {
    const markup = renderDetail({
      ...item({ kind: "pr" }),
      createdAt: "2026-09-01T08:00:00Z",
    });
    expect(markup).toContain("Created ");
    expect(markup).toContain('dateTime="2026-09-01T08:00:00Z"');
    expect(markup).toContain("Updated ");
    expect(renderDetail(item({ kind: "pr" }))).not.toContain("Created ");
  });

  it("keeps issue identity and actions outside the body scroller", () => {
    const markup = renderDetail(item({ projectPath: "/tmp/local-project" }));
    const headerIndex = markup.indexOf("data-inbox-detail-header");
    const scrollIndex = markup.indexOf("data-inbox-detail-scroll");
    const header = markup.slice(headerIndex, scrollIndex);
    const body = markup.slice(scrollIndex);
    const identityIndex = header.indexOf("data-inbox-detail-identity");
    const identityTag = header.slice(
      identityIndex,
      header.indexOf(">", identityIndex),
    );

    expect(headerIndex).toBeGreaterThan(-1);
    expect(scrollIndex).toBeGreaterThan(headerIndex);
    expect(header).toContain("line-clamp-2");
    expect(header).toContain('title="A long inbox issue"');
    expect(header).toContain("Send to agent");
    expect(header).toContain("Ask");
    expect(header).toContain("Open on GitHub");
    expect(header).toContain("Unassigned");
    expect(header).toContain("whitespace-nowrap");
    expect(header).not.toContain("data-inbox-detail-fixed-header");
    expect(identityTag).not.toContain("border-b");
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

  it("adds a Checks tab to GitHub pull requests while Summary stays initial", () => {
    const markup = renderDetail(item({ kind: "pr", state: "open" }));
    const headerIndex = markup.indexOf("data-inbox-detail-header");
    const scrollIndex = markup.indexOf("data-inbox-detail-scroll");
    const header = markup.slice(headerIndex, scrollIndex);

    expect(header).toContain('aria-label="Checks: Loading checks"');
    expect(header).toContain('title="Checks: Loading checks"');
    expect(header).toContain(">Checks<");
    // Summary is the first selected tab; loading never steals the selection.
    const summarySelected = markup.indexOf('aria-selected="true"');
    expect(summarySelected).toBeGreaterThan(-1);
    expect(summarySelected).toBeLessThan(
      markup.indexOf('aria-label="Checks: Loading checks"'),
    );
  });

  it("keeps the Checks tab off issues and GitLab merge requests", () => {
    expect(renderDetail(item())).not.toContain('aria-label="Checks:');
    expect(
      renderDetail(
        item({
          kind: "pr",
          provider: "gitlab",
          repo: "acme/platform",
          url: "https://gitlab.example.com/acme/platform/-/merge_requests/12",
        }),
      ),
    ).not.toContain('aria-label="Checks:');
  });

  it("shows the Checks tab for linked pull requests in panel mode", () => {
    const markup = renderToStaticMarkup(
      createElement(InboxDetail, {
        item: item({ kind: "pr", state: "open" }),
        cwd: "/tmp/web",
        projects: [],
        revision: 0,
        mode: "panel",
        relatedSessions: [],
      }),
    );
    expect(markup).toContain('aria-label="Checks: Loading checks"');
  });

  it("offers GitHub-style actions for an open pull request", () => {
    const markup = renderDetail(item({ kind: "pr", state: "open" }));

    expect(markup).toContain("Merge pull request");
    expect(markup).toContain('aria-label="Merge options"');
    expect(markup).toContain("Convert to draft");
    expect(markup).toContain("Close pull request");
  });

  it("adapts pull request actions to draft and closed states", () => {
    const draft = renderDetail(
      item({ kind: "pr", state: "open", draft: true }),
    );
    expect(draft).toContain("Ready for review");
    expect(draft).toContain("Close pull request");
    expect(draft).not.toContain('aria-label="Merge options"');

    const closed = renderDetail(item({ kind: "pr", state: "closed" }));
    expect(closed).toContain("Reopen pull request");
    expect(closed).not.toContain("Convert to draft");

    const merged = renderDetail(item({ kind: "pr", state: "merged" }));
    expect(merged).not.toContain("Reopen pull request");
    expect(merged).not.toContain('aria-label="Merge options"');
  });

  it("does not show GitHub lifecycle actions for GitLab merge requests", () => {
    const markup = renderDetail(
      item({
        kind: "pr",
        provider: "gitlab",
        repo: "acme/platform",
        url: "https://gitlab.example.com/acme/platform/-/merge_requests/12",
      }),
    );

    expect(markup).not.toContain('aria-label="Merge options"');
    expect(markup).not.toContain("Convert to draft");
    expect(markup).not.toContain("Close pull request");
  });

  it("pins the linked-item identity above the panel scroller", () => {
    const markup = renderToStaticMarkup(
      createElement(InboxDetail, {
        item: item({ kind: "pr" }),
        cwd: "/tmp/web",
        projects: [],
        revision: 0,
        mode: "panel",
        relatedSessions: [
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
        ],
      }),
    );
    const fixedIndex = markup.indexOf("data-inbox-detail-fixed-header");
    const scrollIndex = markup.indexOf("data-inbox-detail-scroll");
    const headerIndex = markup.indexOf("data-inbox-detail-header");
    const fixedHeader = markup.slice(fixedIndex, scrollIndex);
    const reviewIndex = markup.lastIndexOf("Review on GitHub");
    const reviewButton = markup.slice(
      markup.lastIndexOf("<button", reviewIndex),
      reviewIndex,
    );

    expect(fixedIndex).toBeGreaterThan(-1);
    expect(scrollIndex).toBeGreaterThan(fixedIndex);
    expect(scrollIndex).toBeGreaterThan(-1);
    expect(headerIndex).toBeGreaterThan(scrollIndex);
    expect(markup.match(/data-inbox-detail-scroll/g)).toHaveLength(1);
    expect(fixedHeader).toContain("Pull request");
    expect(fixedHeader).toContain("#157");
    expect(fixedHeader).toContain("Review on GitHub");
    expect(reviewIndex).toBeLessThan(scrollIndex);
    expect(markup.match(/aria-label="Review on GitHub"/g)).toHaveLength(1);
    expect(markup.slice(scrollIndex)).not.toContain("Review on GitHub");
    expect(fixedHeader).toContain("h-9");
    expect(fixedHeader).toContain("px-4");
    expect(fixedHeader).not.toContain("h-10");
    expect(fixedHeader).not.toContain("px-5");
    expect(fixedHeader).toContain("border-b");
    expect(fixedHeader).not.toContain("A long inbox issue");
    expect(markup).toContain("text-[18px]");
    expect(markup).not.toContain("Related thread");
    expect(markup).not.toContain("Review MonoCode Pull Request");
    expect(reviewButton).toContain("h-6.5");
    expect(reviewButton).toContain("hover:bg-content/10");
    expect(reviewButton).not.toContain("h-6.5 bg-content/10");
    expect(fixedHeader).toContain("pr-[34px]");
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

describe("InboxDetail PR branch row", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    clearInboxCache();
  });

  it("offers a copy action for the head branch name", async () => {
    vi.mocked(invoke).mockResolvedValue({
      body: "",
      author: "octocat",
      baseRefName: "main",
      headRefName: "feature/inbox-branch-copy",
    } as never);
    await githubWorkItemDetails("/tmp/web", "acme/web", "pr", 157);

    const markup = renderDetail(
      item({ kind: "pr", repo: "acme/web", number: 157 }),
    );

    expect(markup).toContain("main ← feature/inbox-branch-copy");
    expect(markup).toContain("Copy branch name");
  });

  it("has nothing to copy when the PR carries no branch info", () => {
    const markup = renderDetail(
      item({ kind: "pr", repo: "acme/web", number: 999 }),
    );

    expect(markup).not.toContain("Copy branch name");
  });
});
