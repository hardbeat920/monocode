import { beforeEach, describe, expect, it, vi } from "vitest";
import { newSession, type Session } from "../model/session";
import { getSession, persistFingerprint, sanitizeSessionForPersist, upsertSession } from "./sessionStore";
import { buildPortableContext } from "../model/portableContext";
import { canDispatchQueuedHead } from "../model/messageQueue";
import {
  failProviderDelivery,
  recordProviderBound,
  settleProviderBinding,
} from "../model/providerContext";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

function switchingSession(): Session {
  return {
    ...newSession("codex", "/repo"),
    blocks: [{ id: "user-1", role: "user", text: "Preserve my first instruction" }],
    pendingSwitch: {
      from: "claude", fromModel: "claude-opus", fromSettings: { effort: "high" },
      fromProviderSessionId: "claude-1", fromProviderAccountId: "work-account",
    },
    providerContext: {
      version: 1,
      bindings: [{ harness: "claude", providerSessionId: "claude-1", providerAccountId: "work-account", cwd: "/repo", deliveredThroughBlockId: "user-1" }],
      delivery: {
        switchId: "switch-1", status: "imported", mode: "native", from: "claude", to: "codex",
        cwd: "/repo", currentUserBlockId: "user-2", sourceThroughBlockId: "user-1",
        includedBlockIds: ["user-1"], omittedBlockIds: [], targetProviderSessionId: "codex-1",
      },
    },
  };
}

describe("durable provider switching", () => {
  beforeEach(() => invoke.mockReset());

  it("round trips picker intent and native bindings while recovering a partially delivered transfer", async () => {
    const original = switchingSession();
    const payload = sanitizeSessionForPersist(original);
    invoke.mockResolvedValue({ ...payload, createdAt: 1, updatedAt: 2 });
    const restored = await getSession(original.id);
    expect(restored?.pendingSwitch).toEqual(original.pendingSwitch);
    expect(restored?.providerContext).toEqual({
      ...original.providerContext,
      delivery: { ...original.providerContext!.delivery!, status: "uncertain" },
    });
    expect(restored?.blocks[0].text).toBe(original.blocks[0].text);
    expect(restored?.busy).toBe(false);
  });

  it("includes picker-only and delivery receipt changes in the persistence fingerprint", () => {
    const original = switchingSession();
    const settled = { ...original, pendingSwitch: undefined };
    expect(persistFingerprint(settled)).not.toBe(persistFingerprint(original));
    const accepted = {
      ...original,
      providerContext: { ...original.providerContext!, delivery: { ...original.providerContext!.delivery!, status: "accepted" as const } },
    };
    expect(persistFingerprint(accepted)).not.toBe(persistFingerprint(original));
  });

  it.each(["preparing", "imported"] as const)("recovers an interrupted %s transfer as one draft with a fresh target", async (status) => {
    const original = switchingSession();
    original.providerSessionId = "codex-1";
    original.providerContext!.delivery!.status = status;
    original.providerContext!.bindings.push({ harness: "codex", providerSessionId: "codex-1", cwd: "/repo" });
    original.blocks.push({ id: "user-2", role: "user", text: "Submit this request once" });
    const payload = sanitizeSessionForPersist(original);
    invoke.mockResolvedValue({ ...payload, createdAt: 1, updatedAt: 2 });
    const restored = await getSession(original.id);
    expect(restored?.providerSessionId).toBeUndefined();
    expect(restored?.providerContext?.delivery?.status).toBe("uncertain");
    expect(restored?.providerContext?.bindings.map((binding) => binding.harness)).toEqual(["claude"]);
    expect(restored?.pendingSwitch?.fromProviderSessionId).toBe("claude-1");
    expect(restored?.blocks.filter((block) => block.draft).map((block) => block.id)).toEqual(["user-2"]);
    expect(buildPortableContext(restored!).items.some((item) => item.sourceBlockId === "user-2")).toBe(false);
  });

  it("preserves a later provider binding when loading an already uncertain transfer", async () => {
    const interrupted = switchingSession();
    interrupted.providerSessionId = "codex-1";
    interrupted.blocks.push({ id: "user-2", role: "user", text: "Restore this interrupted request" });
    let recovered = failProviderDelivery(interrupted, "switch-1");
    recovered = recordProviderBound(recovered, "codex", "/repo", "codex-2");
    recovered = {
      ...recovered,
      blocks: [...recovered.blocks, { id: "user-3", role: "user", text: "A later accepted command" }],
    };
    recovered = settleProviderBinding(recovered, "codex", "/repo");
    const payload = sanitizeSessionForPersist(recovered);
    invoke.mockResolvedValue({ ...payload, createdAt: 1, updatedAt: 2 });

    const restored = await getSession(recovered.id);

    expect(restored?.providerContext).toEqual(recovered.providerContext);
    expect(restored?.providerSessionId).toBe("codex-2");
    expect(restored?.blocks.filter((block) => block.draft).map((block) => block.id)).toEqual(["user-2"]);
    expect(buildPortableContext(restored!).items.some((item) => item.sourceBlockId === "user-3")).toBe(true);
  });

  it.each(["preparing", "imported"] as const)("requires inspection when a submitted %s request was acknowledged but its acceptance save failed", async (status) => {
    const original = switchingSession();
    original.providerSessionId = "codex-1";
    original.providerContext!.delivery = {
      ...original.providerContext!.delivery!, status, requestSubmitted: true,
    };
    original.providerContext!.bindings.push({ harness: "codex", providerSessionId: "codex-1", cwd: "/repo" });
    original.blocks.push({ id: "user-2", role: "user", text: "Apply the external action once" });
    original.queuedMessages = [{ id: "queued-1", text: "Continue automatically", attachments: [] }];
    original.queueStatus = "active";
    const savedBeforeAcceptance = sanitizeSessionForPersist(original);
    const accepted = {
      ...original,
      pendingSwitch: undefined,
      providerContext: {
        ...original.providerContext!,
        delivery: { ...original.providerContext!.delivery!, status: "accepted" as const },
      },
    };
    invoke.mockRejectedValueOnce(new Error("Disk full"));
    await expect(upsertSession(accepted)).rejects.toThrow("Disk full");
    invoke.mockResolvedValue({ ...savedBeforeAcceptance, createdAt: 1, updatedAt: 2 });

    const restored = await getSession(original.id);

    expect(restored?.providerContext?.delivery).toMatchObject({
      status: "uncertain", requestSubmitted: true, needsInspection: true,
    });
    expect(restored?.blocks.find((block) => block.id === "user-2")?.draft).toBeFalsy();
    expect(restored?.providerSessionId).toBe("codex-1");
    expect(restored?.providerContext?.bindings).toEqual(original.providerContext!.bindings);
    expect(canDispatchQueuedHead({
      ...restored!, queuedMessages: original.queuedMessages, queueStatus: "active",
    })).toBe(false);
  });

  it("recovers a crash during snapshot preparation before a delivery receipt exists", async () => {
    const original = switchingSession();
    original.providerContext!.delivery = undefined;
    original.blocks.push(
      { id: "preflight", role: "handoff", text: "Preparing shared history", handoff: { from: "claude", to: "codex", status: "preparing" } },
      { id: "user-2", role: "user", text: "Do not send this twice" },
    );
    const payload = sanitizeSessionForPersist(original);
    expect(payload.blocks.find((block) => block.id === "preflight")?.handoff?.status).toBe("preparing");
    invoke.mockResolvedValue({ ...payload, createdAt: 1, updatedAt: 2 });
    const restored = await getSession(original.id);
    expect(restored?.blocks.filter((block) => block.draft).map((block) => block.id)).toEqual(["user-2"]);
    expect(restored?.blocks.find((block) => block.id === "preflight")?.handoff?.status).toBe("ready");
    expect(restored?.pendingSwitch?.fromProviderSessionId).toBe("claude-1");
  });

  it("does not turn an inspection-required request into a draft on a later reload", async () => {
    const original = switchingSession();
    original.providerSessionId = "codex-1";
    original.providerContext!.delivery = {
      ...original.providerContext!.delivery!, status: "uncertain", requestSubmitted: true, needsInspection: true,
    };
    original.blocks.push(
      { id: "handoff", role: "handoff", text: "Shared history", handoff: {
        from: "claude", to: "codex", status: "preparing", pending: true,
        transfer: { switchId: "switch-1", status: "uncertain", mode: "native", included: 1, omitted: 0, historicalAttachments: 0, requestSubmitted: true, needsInspection: true },
      } },
      { id: "user-2", role: "user", text: "May already have executed" },
    );
    const saved = sanitizeSessionForPersist(original);
    invoke.mockResolvedValue({ ...saved, createdAt: 1, updatedAt: 2 });
    const restored = await getSession(original.id);
    expect(restored?.blocks.find((block) => block.id === "user-2")?.draft).toBeFalsy();
    expect(restored?.providerContext).toEqual(original.providerContext);
    expect(restored?.providerSessionId).toBe("codex-1");
  });

  it("loads old records without adding a synthetic transcript turn", async () => {
    const payload = sanitizeSessionForPersist({ ...switchingSession(), pendingSwitch: undefined, providerContext: undefined });
    invoke.mockResolvedValue({ ...payload, createdAt: 1, updatedAt: 2 });
    const restored = await getSession(payload.id);
    expect(restored?.providerContext).toBeUndefined();
    expect(restored?.pendingSwitch).toBeUndefined();
    expect(restored?.blocks).toHaveLength(1);
  });

  it("discards malformed saved switch state while retaining the conversation", async () => {
    const payload = sanitizeSessionForPersist(switchingSession());
    invoke.mockResolvedValue({ ...payload, providerContext: { version: 1, state: { version: 99, bindings: [] }, pendingSwitch: { from: "unknown", fromModel: "anything", fromSettings: {} } } });
    const restored = await getSession(payload.id);
    expect(restored?.pendingSwitch).toBeUndefined();
    expect(restored?.providerContext).toBeUndefined();
    expect(restored?.blocks).toHaveLength(1);
  });

  it("retains transfer status and omissions in persisted handoff rows", () => {
    const session = switchingSession();
    const transfer = { switchId: "switch-1", status: "uncertain" as const, mode: "native" as const, included: 12, omitted: 4, historicalAttachments: 2, retrievalPath: "/data/history/switch-1.md", requestSubmitted: true as const, needsInspection: true as const };
    session.blocks.push({ id: "handoff", role: "handoff", text: "Shared history", handoff: { from: "claude", to: "codex", status: "ready", pending: true, transfer } });
    expect(sanitizeSessionForPersist(session).blocks[1].handoff?.transfer).toEqual(transfer);
  });
});
