import { describe, expect, it } from "vitest";
import {
  canReplaceSessionTitle,
  formatSessionTitle,
  HARNESS_LABEL,
  newSession,
  removeSessionDraft,
  sessionDisplayTitle,
  titleFromPrompt,
  withHarnessChoice,
  type HarnessId,
} from "./session";

describe("removeSessionDraft", () => {
  it("removes a follow-up draft without changing earlier conversation history", () => {
    const session = newSession("codex", "/repo");
    session.title = "codex · Existing thread";
    session.blocks = [
      { id: "sent", role: "user", text: "Start here" },
      { id: "reply", role: "assistant", text: "Done" },
      { id: "draft", role: "user", text: "Maybe later", draft: true },
    ];

    const updated = removeSessionDraft(session, "draft");

    expect(updated?.blocks).toEqual(session.blocks.slice(0, 2));
    expect(updated?.title).toBe("codex · Existing thread");
  });

  it("restores a draft-only session to a blank untitled state", () => {
    const session = newSession("codex", "/repo");
    session.title = "codex · Maybe later";
    session.blocks = [
      { id: "draft", role: "user", text: "Maybe later", draft: true },
    ];

    expect(removeSessionDraft(session, "draft")).toMatchObject({
      title: "codex",
      blocks: [],
    });
  });

  it("keeps a custom title when removing the only draft", () => {
    const session = newSession("codex", "/repo");
    session.title = "codex · Keep this name";
    session.blocks = [
      { id: "draft", role: "user", text: "Maybe later", draft: true },
    ];

    expect(removeSessionDraft(session, "draft")?.title).toBe(
      "codex · Keep this name",
    );
  });

  it("ignores sent messages and unknown blocks", () => {
    const session = newSession("codex", "/repo");
    session.blocks = [{ id: "sent", role: "user", text: "Keep this" }];

    expect(removeSessionDraft(session, "sent")).toBeUndefined();
    expect(removeSessionDraft(session, "missing")).toBeUndefined();
  });
});

it("preserves an explicit prompt seed when removing a draft, including subsequent naming", () => {
  const session = newSession("codex", "/repo");
  session.title = titleFromPrompt("Maybe later", session.harness);
  session.titleIsExplicit = true;
  session.blocks = [
    { id: "draft", role: "user", text: "Maybe later", draft: true },
  ];
  const updated = removeSessionDraft(session, "draft")!;
  expect(updated.title).toBe(session.title);
  expect(canReplaceSessionTitle(updated, session.title)).toBe(false);
  expect(updated.blocks).toEqual([]);
});

const modelChoices: [string, HarnessId, boolean][] = [
  ["same model and provider", "codex", false],
  ["different model, same provider", "codex", true],
  ["different provider", "claude", true],
];

it.each(modelChoices)(
  "keeps an explicit title after draft removal and %s selection",
  (_, harness, changeModel) => {
    const session = newSession("codex", "/repo");
    const displayTitle = "#646 — codex · Operator session titles";
    session.title = formatSessionTitle(session.harness, displayTitle);
    session.titleIsExplicit = true;
    session.blocks = [
      { id: "draft", role: "user", text: "Fix titles", draft: true },
    ];
    const empty = removeSessionDraft(session, "draft")!;
    const model = changeModel ? "another-model" : session.model;
    const changed = withHarnessChoice(empty, harness, model, {
      effort: "high",
    });
    expect(changed).toMatchObject({
      harness,
      model,
      modelSettings: { effort: "high" },
      blocks: [],
      titleIsExplicit: true,
      title: formatSessionTitle(harness, displayTitle),
    });
    expect(sessionDisplayTitle(changed.title, harness)).toBe(displayTitle);
    expect(canReplaceSessionTitle(changed, changed.title)).toBe(false);
    expect(
      withHarnessChoice(changed, session.harness, session.model, {}).title,
    ).toBe(session.title);
  },
);

it.each(modelChoices)(
  "keeps automatic naming after draft removal and %s selection",
  (_, harness, changeModel) => {
    const session = newSession("codex", "/repo");
    session.title = titleFromPrompt("Fix titles", session.harness);
    session.blocks = [
      { id: "draft", role: "user", text: "Fix titles", draft: true },
    ];
    const empty = removeSessionDraft(session, "draft")!;
    const changed = withHarnessChoice(
      empty,
      harness,
      changeModel ? "another-model" : session.model,
      {},
    );
    expect(changed.title).toBe(HARNESS_LABEL[harness]);
    expect(changed.titleIsExplicit).toBeUndefined();
    expect(canReplaceSessionTitle(changed, HARNESS_LABEL[harness])).toBe(true);
  },
);
