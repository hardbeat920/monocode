import { expect, it, vi } from "vitest";
import {
  AcceptancePersistenceError,
  createAcceptancePersistence,
  dispatchAfterContextSave,
  persistNativeContextDelivery,
  withAcceptancePersistenceError,
} from "./acceptancePersistence";
import { newSession } from "../../features/sessions/model/session";
import { buildPortableContext } from "../../features/sessions/model/portableContext";
import { prepareContextTransferInput } from "../../features/sessions/model/contextTransfer";
import type { HarnessEvent } from "../../integrations/harness/core/types";

it("keeps a failed accepted save blocked and retries storage without provider input", async () => {
  const persistence = createAcceptancePersistence();
  let durableStatus = "imported";
  const providerInput = vi.fn();
  const repair = vi.fn();
  const paused = vi.fn();
  const save = vi.fn().mockRejectedValueOnce(new Error("disk full"));
  providerInput();
  const accepted = persistence.start("session", "switch", save, paused);
  const continuation = accepted.then(repair);
  await expect(continuation).rejects.toBeInstanceOf(AcceptancePersistenceError);
  expect(durableStatus).toBe("imported");
  expect(persistence.hasPending("session")).toBe(true);
  expect(paused).toHaveBeenCalledOnce();
  expect(repair).not.toHaveBeenCalled();
  await expect(persistence.wait("session", "switch")).rejects.toBeInstanceOf(
    AcceptancePersistenceError,
  );
  await persistence.reconcile("session", async () => {
    durableStatus = "accepted";
  });
  expect(durableStatus).toBe("accepted");
  expect(persistence.hasPending("session")).toBe(false);
  expect(providerInput).toHaveBeenCalledOnce();
  expect(repair).not.toHaveBeenCalled();
});

it("waits for an accepted save before allowing a follow-up", async () => {
  const persistence = createAcceptancePersistence();
  let complete!: () => void;
  const save = new Promise<void>((resolve) => {
    complete = resolve;
  });
  const repair = vi.fn();
  persistence.start("session", "switch", () => save, vi.fn());
  const continuation = persistence.wait("session", "switch").then(repair);
  await Promise.resolve();
  expect(repair).not.toHaveBeenCalled();
  expect(persistence.hasPending("session")).toBe(true);
  complete();
  await continuation;
  expect(repair).toHaveBeenCalledOnce();
  expect(persistence.hasPending("session")).toBe(false);
});

it("does not dispatch provider input before its submission marker is saved", async () => {
  let finish!: () => void;
  const save = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const dispatch = vi.fn(async () => undefined);
  const turn = dispatchAfterContextSave(
    () => save,
    () => true,
    dispatch,
  );
  expect(dispatch).not.toHaveBeenCalled();
  finish();
  await turn;
  expect(dispatch).toHaveBeenCalledOnce();
  dispatch.mockClear();
  await expect(
    dispatchAfterContextSave(
      async () => {
        throw new Error("disk full");
      },
      () => true,
      dispatch,
    ),
  ).rejects.toThrow("disk full");
  expect(dispatch).not.toHaveBeenCalled();
  await dispatchAfterContextSave(
    async () => undefined,
    () => false,
    dispatch,
  );
  expect(dispatch).not.toHaveBeenCalled();
});

it("pauses follow-ups without changing accepted user input or active provider work", () => {
  const session = {
    ...newSession("codex", "/repo"),
    busy: true,
    providerSessionId: "accepted-native",
    blocks: [{ id: "current", role: "user" as const, text: "Build" }],
  };
  const failed = withAcceptancePersistenceError(
    session,
    new AcceptancePersistenceError(new Error("disk full")),
  );
  expect(failed.busy).toBe(true);
  expect(failed.providerSessionId).toBe("accepted-native");
  expect(failed.blocks[0]).toBe(session.blocks[0]);
  expect(failed.blocks[0].draft).toBeUndefined();
  expect(failed.queueStatus).toBe("paused");
  expect(failed.blocks.at(-1)).toMatchObject({
    role: "system",
    notice: "error",
  });
});

it("saves inline delivery with acceptance without a failing redundant import write", async () => {
  const persistence = createAcceptancePersistence();
  const events: HarnessEvent[] = [];
  const importedSave = vi.fn(async () => {
    throw new Error("disk full during imported save");
  });
  const acceptedSave = vi.fn(async () => undefined);
  let deliveryMode: string | undefined;
  const input = prepareContextTransferInput(
    {
      sessionId: "session",
      cwd: "/repo",
      model: "claude:sonnet",
      runtimeMode: "supervised",
      text: "Build approved plan",
      contextTransfer: {
        context: buildPortableContext(newSession("codex", "/repo")),
        onDelivered: ({ mode }) => {
          deliveryMode = mode;
          return persistNativeContextDelivery(mode, importedSave);
        },
      },
      onAccepted: () => {
        persistence.start(
          "session",
          "switch",
          async () => {
            expect(deliveryMode).toBe("inline");
            await acceptedSave();
          },
          vi.fn(),
        );
      },
      onEvent: (event) => events.push(event),
    },
    { nativeMessages: false, resumedAppend: true, explicitAcceptance: true },
  );
  input.onAccepted?.();
  await persistence.wait("session", "switch");
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(importedSave).not.toHaveBeenCalled();
  expect(acceptedSave).toHaveBeenCalledOnce();
  expect(events.some((event) => event.type === "session.error")).toBe(false);
});

it("awaits native import persistence before the current request can start", async () => {
  const providerInput = vi.fn();
  await expect(
    (async () => {
      await persistNativeContextDelivery("native", async () => {
        throw new Error("disk full");
      });
      providerInput();
    })(),
  ).rejects.toThrow("disk full");
  expect(providerInput).not.toHaveBeenCalled();
});
