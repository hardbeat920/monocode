import { beforeEach, expect, it, vi } from "vitest";
import {
  AcceptancePersistenceError,
  createAcceptancePersistence,
  dispatchAfterContextSave,
  persistNativeContextDelivery,
} from "./acceptancePersistence";
import { saveProviderContextSession } from "./providerContextPersistence";
import {
  newSession,
  type Session,
} from "../../features/sessions/model/session";
import {
  acceptProviderDelivery,
  beginProviderDelivery,
  confirmProviderDeliveryInspection,
  markProviderContextDelivered,
  markProviderRequestSubmitted,
  recoverSubmittedProviderDelivery,
} from "../../features/sessions/model/providerContext";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

beforeEach(() => {
  mocks.invoke.mockReset();
});

function transferSession(cwd = "/repo", inbox = false): Session {
  return beginProviderDelivery(
    {
      ...newSession("codex", cwd),
      providerSessionId: "codex-native",
      ...(inbox
        ? {
            inboxAsk: {
              key: "github:example/repo:1",
              title: "Issue",
              provider: "github" as const,
              url: "https://github.com/example/repo/issues/1",
            },
          }
        : {}),
      blocks: [
        { id: "history", role: "user", text: "Earlier request" },
        {
          id: "handoff",
          role: "handoff",
          text: "Shared history",
          handoff: {
            from: "claude",
            to: "codex",
            status: "preparing",
            pending: true,
          },
        },
        { id: "current", role: "user", text: "Continue once" },
      ],
    },
    {
      switchId: "switch",
      from: "claude",
      to: "codex",
      cwd,
      currentUserBlockId: "current",
      sourceThroughBlockId: "history",
      includedBlockIds: ["history"],
      omittedBlockIds: [],
      targetProviderSessionId: "codex-native",
    },
  );
}

function summary(session: Session) {
  return {
    id: session.id,
    cwd: session.cwd,
    harness: session.harness,
    model: session.model,
    runtimeMode: session.runtimeMode,
    title: session.title,
    createdAt: 1,
    updatedAt: 2,
  };
}

it.each([
  ["home", "~", false],
  ["inbox", "/repo", true],
] as const)(
  "continues a %s transfer with in-memory import, submission, and acceptance",
  async (_, cwd, inbox) => {
    let session = markProviderContextDelivered(
      transferSession(cwd, inbox),
      "switch",
      "native",
      "codex-native",
    );
    const persistence = createAcceptancePersistence();
    const paused = vi.fn();
    const providerInput = vi.fn(async () => {
      session = acceptProviderDelivery(session, "switch");
    });

    await persistNativeContextDelivery("native", () =>
      saveProviderContextSession(session, "Import failed"),
    );
    session = markProviderRequestSubmitted(session, "switch");
    await dispatchAfterContextSave(
      () => saveProviderContextSession(session, "Submission failed"),
      () => true,
      providerInput,
    );
    await persistence.start(
      session.id,
      "switch",
      () => saveProviderContextSession(session, "Acceptance failed"),
      paused,
    );

    expect(providerInput).toHaveBeenCalledOnce();
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(paused).not.toHaveBeenCalled();
    expect(persistence.hasPending(session.id)).toBe(false);
    expect(session.providerContext?.delivery).toMatchObject({
      status: "accepted",
      mode: "native",
      requestSubmitted: true,
    });
    expect(session.providerSessionId).toBe("codex-native");
    expect(
      session.blocks.find((block) => block.id === "current")?.draft,
    ).toBeUndefined();
  },
);

it("uses the existing persistence policy for remote and empty sessions", async () => {
  await saveProviderContextSession(
    transferSession("remote://host/project"),
    "Save failed",
  );
  await saveProviderContextSession(newSession("codex", "/repo"), "Save failed");
  expect(mocks.invoke).not.toHaveBeenCalled();
});

it("awaits a persistable submission marker before dispatching provider input", async () => {
  const session = markProviderRequestSubmitted(transferSession(), "switch");
  let complete!: (value: unknown) => void;
  mocks.invoke.mockReturnValue(
    new Promise((resolve) => {
      complete = resolve;
    }),
  );
  const providerInput = vi.fn(async () => undefined);
  const sending = dispatchAfterContextSave(
    () => saveProviderContextSession(session, "Submission failed"),
    () => true,
    providerInput,
  );
  await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledOnce());
  expect(mocks.invoke).toHaveBeenCalledWith(
    "session_upsert",
    expect.objectContaining({
      session: expect.objectContaining({
        providerContext: expect.objectContaining({
          state: expect.objectContaining({
            delivery: expect.objectContaining({ requestSubmitted: true }),
          }),
        }),
      }),
    }),
  );
  expect(providerInput).not.toHaveBeenCalled();
  complete(summary(session));
  await sending;
  expect(providerInput).toHaveBeenCalledOnce();
});

it.each(["null", "rejection"] as const)(
  "blocks dispatch after a real persistable save %s",
  async (failure) => {
    const session = markProviderRequestSubmitted(transferSession(), "switch");
    if (failure === "null") mocks.invoke.mockResolvedValue(null);
    else mocks.invoke.mockRejectedValue(new Error("Disk full"));
    const providerInput = vi.fn(async () => undefined);

    await expect(
      dispatchAfterContextSave(
        () => saveProviderContextSession(session, "Submission failed"),
        () => true,
        providerInput,
      ),
    ).rejects.toThrow(failure === "null" ? "Submission failed" : "Disk full");
    expect(mocks.invoke).toHaveBeenCalledOnce();
    expect(providerInput).not.toHaveBeenCalled();
  },
);

it("pauses a failed acceptance save and reconciles storage without resending", async () => {
  const session = acceptProviderDelivery(
    markProviderRequestSubmitted(transferSession(), "switch"),
    "switch",
  );
  const originalInput = session.blocks.find((block) => block.id === "current");
  const providerInput = vi.fn();
  const paused = vi.fn();
  const persistence = createAcceptancePersistence();
  mocks.invoke
    .mockRejectedValueOnce(new Error("Disk full"))
    .mockResolvedValueOnce(summary(session));
  providerInput();

  await expect(
    persistence.start(
      session.id,
      "switch",
      () => saveProviderContextSession(session, "Acceptance failed"),
      paused,
    ),
  ).rejects.toBeInstanceOf(AcceptancePersistenceError);
  expect(persistence.submissionMode(session.id, false)).toBe("reconcile");
  expect(paused).toHaveBeenCalledOnce();
  await persistence.reconcile(session.id, () =>
    saveProviderContextSession(session, "Acceptance failed"),
  );

  expect(persistence.hasPending(session.id)).toBe(false);
  expect(providerInput).toHaveBeenCalledOnce();
  expect(mocks.invoke).toHaveBeenCalledTimes(2);
  expect(session.providerContext?.delivery?.status).toBe("accepted");
  expect(session.providerSessionId).toBe("codex-native");
  expect(session.blocks.find((block) => block.id === "current")).toBe(
    originalInput,
  );
  expect(originalInput?.draft).toBeUndefined();
});

it("clears a temporary session's earlier false save failure without provider input", async () => {
  const session = acceptProviderDelivery(transferSession("~"), "switch");
  const persistence = createAcceptancePersistence();
  const providerInput = vi.fn();
  providerInput();
  await expect(
    persistence.start(
      session.id,
      "switch",
      async () => {
        throw new Error("The earlier save required a conversation record");
      },
      vi.fn(),
    ),
  ).rejects.toBeInstanceOf(AcceptancePersistenceError);

  await persistence.reconcile(session.id, () =>
    saveProviderContextSession(session, "Acceptance failed"),
  );

  expect(persistence.submissionMode(session.id, false)).toBe("submit");
  expect(mocks.invoke).not.toHaveBeenCalled();
  expect(providerInput).toHaveBeenCalledOnce();
});

it.each([
  ["home", "~", false],
  ["inbox", "/repo", true],
] as const)(
  "records inspection in a %s session without storage or provider input",
  async (_, cwd, inbox) => {
    const submitted = markProviderRequestSubmitted(
      transferSession(cwd, inbox),
      "switch",
    );
    const uncertain = recoverSubmittedProviderDelivery(submitted, "switch");
    const confirmed = confirmProviderDeliveryInspection(uncertain);
    const providerInput = vi.fn();

    await saveProviderContextSession(confirmed, "Inspection failed");

    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(providerInput).not.toHaveBeenCalled();
    expect(confirmed.providerContext?.delivery).toBeUndefined();
    expect(
      confirmed.blocks.find((block) => block.id === "handoff")?.handoff
        ?.transfer,
    ).toMatchObject({ inspectionConfirmed: true });
    expect(confirmed.providerSessionId).toBe("codex-native");
  },
);

it("rejects a missing session instead of treating it as intentionally temporary", async () => {
  await expect(
    saveProviderContextSession(undefined, "Missing session"),
  ).rejects.toThrow("Missing session");
  expect(mocks.invoke).not.toHaveBeenCalled();
});
