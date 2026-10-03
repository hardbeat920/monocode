import { afterEach, describe, expect, it, vi } from "vitest";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { SendTurnInput } from "../src/integrations/harness/core/types";
import type { HostProvider } from "./providers";
import { hostProviders } from "./providers";
import { HostEngine, parseCommand } from "./engine";
import { HostStore } from "./store";
import { readAttachmentChunk, writeAttachmentChunk } from "./attachments";
import { markProviderRequestSubmitted } from "../src/features/sessions/model/providerContext";

const cleanups: Array<() => Promise<void> | void> = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
});

function setup(harness: "codex" | "claude" = "codex") {
  const directory = mkdtempSync(join(tmpdir(), "monocode-engine-test-"));
  const store = new HostStore(join(directory, "host.db"));
  const project = store.addProject(directory, "Test");
  const turns: Array<{ input: SendTurnInput; finish: () => void }> = [];
  const provider: HostProvider = {
    send: vi.fn(
      (input) =>
        new Promise<void>((resolve) => {
          turns.push({ input, finish: resolve });
        }),
    ),
    cancel: vi.fn(async () => {
      turns.at(-1)?.finish();
    }),
    stop: vi.fn(async () => {
      turns.at(-1)?.finish();
    }),
    bind: vi.fn(),
    approve: vi.fn(),
    answer: vi.fn(),
  };
  const engine = new HostEngine(store, { codex: provider, claude: provider });
  const created = engine.command({
    type: "create",
    commandId: "create",
    projectId: project.id,
    harness,
    model: `${harness}:test`,
    runtimeMode: "supervised",
  });
  cleanups.push(async () => {
    await engine.close();
    store.close();
    rmSync(directory, { recursive: true, force: true });
  });
  return {
    directory,
    store,
    provider,
    project,
    engine,
    turns,
    id: created.sessionId,
  };
}

function switchCommand(s: ReturnType<typeof setup>, harness: "codex" | "claude", commandId: string) {
  return {
    type: "switchProvider" as const, commandId, sessionId: s.id,
    expectedRevision: s.store.session(s.id).revision,
    harness, model: `${harness}:test`, modelSettings: {}, runtimeMode: "supervised" as const,
  };
}

async function finishTurn(s: ReturnType<typeof setup>, index: number, providerId: string, reply: string) {
  const turn = s.turns[index];
  turn.input.onEvent({ type: "session.providerBound", providerSessionId: providerId });
  turn.input.onEvent({ type: "message.delta", text: reply });
  turn.finish();
  await vi.waitFor(() => expect(s.store.session(s.id).status).toBe("idle"));
}

describe("host-owned provider switching", () => {
  it("transfers exact visible history once and restores the source with only its missing interval", async () => {
    const s = setup();
    const original = `Keep this requirement ${"long visible text ".repeat(180)} end`;
    s.engine.command({ type: "send", commandId: "source-turn", sessionId: s.id, text: original });
    await vi.waitFor(() => expect(s.turns).toHaveLength(1));
    await finishTurn(s, 0, "source-native", "Original answer");
    const selected = switchCommand(s, "claude", "choose-claude");
    const receipt = s.engine.command(selected);
    expect(s.engine.command(selected)).toEqual(receipt);
    expect(s.store.session(s.id).session.pendingSwitch?.fromProviderSessionId).toBe("source-native");
    expect(s.turns).toHaveLength(1);
    s.engine.command({ type: "send", commandId: "target-turn", sessionId: s.id, text: "Continue here" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(2));
    expect(s.turns[1].input.text).toContain(original);
    expect(s.turns[1].input.text).toContain("Original answer");
    expect(s.turns[1].input.text.match(/Continue here/g)).toHaveLength(1);
    expect(s.store.session(s.id).session.blocks.filter((block) => block.role === "user").map((block) => block.text))
      .toEqual([original, "Continue here"]);
    await finishTurn(s, 1, "target-native", "Target answer");
    expect(s.store.session(s.id).session.providerContext?.delivery).toMatchObject({ status: "accepted", mode: "inline" });
    expect(s.store.session(s.id).session.pendingSwitch).toBeUndefined();
    expect(s.store.session(s.id).session.blocks.filter((block) => block.role === "user").map((block) => block.turnModel?.harness))
      .toEqual(["codex", "claude"]);

    s.provider.contextTransferCapabilities = { nativeMessages: true, resumedAppend: true };
    s.engine.command(switchCommand(s, "codex", "return-codex"));
    s.engine.command({ type: "send", commandId: "return-turn", sessionId: s.id, text: "Now compare" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(3));
    const returning = s.turns[2].input;
    expect(returning.text).toBe("Now compare");
    expect(returning.contextTransfer?.context.items.map((item) => item.text)).toEqual(["Continue here", "Target answer"]);
    expect(returning.contextTransfer?.fallbackContext?.items.some((item) => item.text === original)).toBe(true);
    expect(s.provider.bind).toHaveBeenCalledWith(s.id, "source-native", s.directory);
    await returning.contextTransfer?.onDelivered?.({ mode: "native", providerSessionId: "source-native" });
    returning.onAccepted?.();
    await finishTurn(s, 2, "source-native", "Comparison");
    expect(s.store.session(s.id).session.providerContext?.delivery).toMatchObject({ status: "accepted", mode: "native" });
    s.turns[0].input.onEvent({ type: "session.providerBound", providerSessionId: "late-source" });
    expect(s.store.session(s.id).session.providerSessionId).toBe("source-native");
  });

  it("rejects stale selection and running-turn switches before changing durable state", async () => {
    const s = setup();
    const stale = switchCommand(s, "claude", "stale");
    s.engine.command({ type: "configure", commandId: "config", sessionId: s.id, model: "codex:other", modelSettings: {}, runtimeMode: "supervised" });
    expect(() => s.engine.command(stale)).toThrow("Session changed on the host");
    expect(s.store.session(s.id).session.harness).toBe("codex");
    s.engine.command({ type: "send", commandId: "running", sessionId: s.id, text: "Work" });
    expect(() => s.engine.command(switchCommand(s, "claude", "running-switch"))).toThrow("Wait for the current turn");
    await vi.waitFor(() => expect(s.turns).toHaveLength(1));
    s.turns[0].finish();
  });

  it("starts fresh with surviving history when the saved target boundary disappears", async () => {
    const s = setup();
    s.engine.command({ type: "send", commandId: "source", sessionId: s.id, text: "Original" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(1));
    await finishTurn(s, 0, "source-native", "Source answer");
    s.provider.contextTransferCapabilities = { nativeMessages: true, resumedAppend: true };
    s.provider.forget = vi.fn(async () => {});
    s.engine.command(switchCommand(s, "claude", "choose-other"));
    s.engine.command({ type: "send", commandId: "other", sessionId: s.id, text: "Other provider" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(2));
    await s.turns[1].input.contextTransfer?.onDelivered?.({ mode: "native", providerSessionId: "target-native" });
    s.turns[1].input.onAccepted?.();
    await finishTurn(s, 1, "target-native", "Target answer");
    const before = s.store.session(s.id);
    s.store.transaction(() => s.store.save({
      ...before,
      revision: before.revision + 1,
      session: { ...before.session, providerContext: {
        ...before.session.providerContext!,
        bindings: before.session.providerContext!.bindings.map((binding) => binding.harness === "codex"
          ? { ...binding, deliveredThroughBlockId: "removed-boundary" }
          : binding),
      } },
    }, { type: "fixture" }));
    s.engine.command(switchCommand(s, "codex", "return-source"));
    vi.mocked(s.provider.bind).mockClear();
    vi.mocked(s.provider.forget).mockClear();
    s.engine.command({ type: "send", commandId: "fresh-return", sessionId: s.id, text: "Continue safely" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(3));
    expect(s.provider.forget).toHaveBeenCalledWith(s.id);
    expect(s.provider.bind).not.toHaveBeenCalled();
    expect(s.turns[2].input.contextTransfer?.context.items.map((item) => item.text))
      .toEqual(["Original", "Source answer", "Other provider", "Target answer"]);
    await s.turns[2].input.contextTransfer?.onDelivered?.({ mode: "native", providerSessionId: "fresh-native" });
    s.turns[2].input.onAccepted?.();
    await finishTurn(s, 2, "fresh-native", "Recovered");
  });

  it("retains the source after failed startup and requires inspection before returning", async () => {
    const s = setup();
    s.engine.command({ type: "send", commandId: "source", sessionId: s.id, text: "Original" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(1));
    await finishTurn(s, 0, "source-native", "Source answer");
    s.engine.command(switchCommand(s, "claude", "choose-failing"));
    vi.mocked(s.provider.send).mockRejectedValueOnce(new Error("Login required"));
    s.engine.command({ type: "send", commandId: "failed", sessionId: s.id, text: "Next request" });
    await vi.waitFor(() => expect(s.store.session(s.id).status).toBe("idle"));
    expect(s.store.session(s.id).session.providerContext?.delivery?.status).toBe("uncertain");
    expect(s.store.session(s.id).session.pendingSwitch?.fromProviderSessionId).toBe("source-native");
    s.engine.command({ type: "confirmProviderInspection", commandId: "inspect-failure", sessionId: s.id, expectedRevision: s.store.session(s.id).revision });
    s.provider.contextTransferCapabilities = { nativeMessages: true, resumedAppend: true };
    s.engine.command(switchCommand(s, "codex", "return-after-failure"));
    expect(s.store.session(s.id).session.providerSessionId).toBe("source-native");
    expect(s.store.session(s.id).session.pendingSwitch?.from).toBe("claude");
    s.engine.command({ type: "send", commandId: "continue-source", sessionId: s.id, text: "Continue after inspection" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(2));
    expect(s.turns[1].input.text).toBe("Continue after inspection");
    expect(s.turns[1].input.contextTransfer?.context.items.filter((item) => item.text === "Next request")).toHaveLength(1);
    await s.turns[1].input.contextTransfer?.onDelivered?.({ mode: "native", providerSessionId: "source-native" });
    s.turns[1].input.onAccepted?.();
    await finishTurn(s, 1, "source-native", "Continued");
  });

  it("does not accept a resumed Claude request from a startup plan before initialization fails", async () => {
    const s = setup();
    s.engine.command({ type: "send", commandId: "source", sessionId: s.id, text: "Original" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(1));
    await finishTurn(s, 0, "source-native", "Source answer");

    s.provider.contextTransferCapabilities = hostProviders.claude.contextTransferCapabilities;
    s.engine.command(switchCommand(s, "claude", "choose-claude"));
    s.engine.command({ type: "send", commandId: "claude-turn", sessionId: s.id, text: "First Claude request" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(2));
    s.turns[1].input.onEvent({ type: "session.providerBound", providerSessionId: "target-native" });
    s.turns[1].input.onAccepted?.();
    await finishTurn(s, 1, "target-native", "Claude answer");

    s.provider.contextTransferCapabilities = hostProviders.codex.contextTransferCapabilities;
    s.engine.command(switchCommand(s, "codex", "return-source"));
    s.engine.command({ type: "send", commandId: "returned-source", sessionId: s.id, text: "Back on source" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(3));
    await s.turns[2].input.contextTransfer?.onDelivered?.({ mode: "native", providerSessionId: "source-native" });
    s.turns[2].input.onAccepted?.();
    await finishTurn(s, 2, "source-native", "Returned source answer");

    s.provider.contextTransferCapabilities = hostProviders.claude.contextTransferCapabilities;
    s.provider.forget = vi.fn(async () => {});
    s.engine.command(switchCommand(s, "claude", "resume-claude"));
    vi.mocked(s.provider.bind).mockClear();
    const beforeFailure = s.store.session(s.id).revision;
    vi.mocked(s.provider.send).mockImplementationOnce(async (input) => {
      input.onEvent({ type: "plan", text: "# Startup plan" });
      throw new Error("Claude resumed conversation differs from the requested session");
    });
    s.engine.command({ type: "send", commandId: "failed-resume", sessionId: s.id, text: "Submit this exactly once" });
    await vi.waitFor(() => expect(s.store.session(s.id).status).toBe("idle"));

    const failed = s.store.session(s.id).session;
    expect(s.provider.bind).toHaveBeenCalledWith(s.id, "target-native", s.directory);
    expect(failed.providerContext?.delivery?.status).toBe("uncertain");
    expect(failed.providerContext?.delivery?.needsInspection).toBe(true);
    expect(failed.providerContext?.bindings.map((binding) => binding.providerSessionId)).toEqual(["target-native", "source-native"]);
    expect(failed.pendingSwitch?.fromProviderSessionId).toBe("source-native");
    expect(failed.providerSessionId).toBe("target-native");
    const requests = failed.blocks.filter((block) => block.role === "user" && block.text === "Submit this exactly once");
    expect(requests).toHaveLength(1);
    expect(requests[0].draft).not.toBe(true);
    const failedEvents = s.store.events(s.id, beforeFailure).events;
    expect(failedEvents).toBeDefined();
    expect(failedEvents).not.toEqual(expect.arrayContaining([
      expect.objectContaining({ event: expect.objectContaining({ type: "providerContext.accepted" }) }),
    ]));
    expect(failedEvents).not.toEqual(expect.arrayContaining([
      expect.objectContaining({ event: expect.objectContaining({ type: "providerContext.delivered" }) }),
    ]));
    expect(s.provider.forget).not.toHaveBeenCalled();
  });

  it("preserves an imported target for inspection when the current request was not acknowledged", async () => {
    const s = setup();
    s.provider.contextTransferCapabilities = { nativeMessages: true, resumedAppend: true };
    s.provider.forget = vi.fn(async () => {});
    s.engine.command({ type: "send", commandId: "source", sessionId: s.id, text: "Original" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(1));
    await finishTurn(s, 0, "source-native", "Source answer");
    s.engine.command(switchCommand(s, "claude", "choose-import"));
    vi.mocked(s.provider.send).mockImplementationOnce(async (input) => {
      input.onEvent({ type: "session.providerBound", providerSessionId: "ambiguous-target" });
      await input.contextTransfer?.onDelivered?.({ mode: "native", providerSessionId: "ambiguous-target" });
      throw new Error("Acknowledgement lost");
    });
    s.engine.command({ type: "send", commandId: "uncertain", sessionId: s.id, text: "Uncertain request" });
    await vi.waitFor(() => expect(s.store.session(s.id).status).toBe("idle"));
    const failed = s.store.session(s.id).session;
    expect(failed.providerSessionId).toBe("ambiguous-target");
    expect(failed.providerContext?.delivery).toMatchObject({ status: "uncertain", needsInspection: true });
    expect(failed.providerContext?.bindings.map((binding) => binding.providerSessionId)).toEqual(["source-native", "ambiguous-target"]);
    s.engine.command({ type: "confirmProviderInspection", commandId: "inspect-import", sessionId: s.id, expectedRevision: s.store.session(s.id).revision });
    const inspected = s.store.session(s.id).session;
    expect(inspected.pendingSwitch?.from).toBe("codex");
    expect(inspected.providerContext?.bindings.find((binding) => binding.providerSessionId === "ambiguous-target")?.deliveredThroughBlockId).toBeUndefined();
    vi.mocked(s.provider.bind).mockClear();
    s.engine.command({ type: "send", commandId: "retry", sessionId: s.id, text: "Inspect and continue" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(2));
    expect(s.turns[1].input.text).toBe("Inspect and continue");
    expect(s.turns[1].input.contextTransfer?.context.items.map((item) => item.text)).toEqual(expect.arrayContaining(["Original", "Source answer", "Uncertain request"]));
    expect(s.turns[1].input.contextTransfer?.context.items.filter((item) => item.text === "Uncertain request")).toHaveLength(1);
    expect(s.provider.bind).not.toHaveBeenCalled();
    expect(s.provider.forget).toHaveBeenCalledTimes(2);
    s.turns[1].input.onEvent({ type: "session.providerBound", providerSessionId: "fresh-target" });
    await s.turns[1].input.contextTransfer?.onDelivered?.({ mode: "native", providerSessionId: "fresh-target" });
    s.turns[1].input.onAccepted?.();
    await finishTurn(s, 1, "fresh-target", "Recovered");
  });

  it("preserves an acknowledged request when saving its acceptance receipt fails", async () => {
    const s = setup();
    s.engine.command({ type: "send", commandId: "source", sessionId: s.id, text: "Original" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(1));
    await finishTurn(s, 0, "source-native", "Source answer");
    s.provider.contextTransferCapabilities = { nativeMessages: true, resumedAppend: true };
    s.provider.forget = vi.fn(async () => {});
    s.engine.command(switchCommand(s, "claude", "choose-target"));
    s.engine.command({ type: "send", commandId: "acknowledged-request", sessionId: s.id, text: "Execute exactly once" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(2));
    expect(s.store.session(s.id).session.providerContext?.delivery?.requestSubmitted).toBe(true);
    s.turns[1].input.onEvent({ type: "session.providerBound", providerSessionId: "target-native" });
    await s.turns[1].input.contextTransfer?.onDelivered?.({ mode: "native", providerSessionId: "target-native" });
    vi.mocked(s.provider.forget).mockClear();
    const save = s.store.save.bind(s.store);
    let failed = false;
    let settlementFailed = false;
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(s.store, "save").mockImplementation((value, event) => {
      if (!failed && (event as { type?: string }).type === "providerContext.accepted") {
        failed = true;
        throw new Error("Acceptance receipt storage failed");
      }
      if (!settlementFailed && (event as { type?: string }).type === "settled") {
        settlementFailed = true;
        throw new Error("Settlement storage failed");
      }
      return save(value, event);
    });
    expect(() => s.turns[1].input.onAccepted?.()).not.toThrow();
    await vi.waitFor(() => expect(settlementFailed).toBe(true));
    expect(() => s.engine.command({ type: "send", commandId: "blocked-during-storage", sessionId: s.id, text: "Follow up" })).toThrow("already running");
    await vi.waitFor(() => expect(s.store.session(s.id).status).toBe("interrupted"), { timeout: 3_000 });
    const recovered = s.store.session(s.id).session;
    expect(failed).toBe(true);
    expect(recovered.providerContext?.delivery?.status).toBe("accepted");
    expect(recovered.providerSessionId).toBe("target-native");
    expect(recovered.providerContext?.bindings.map((binding) => binding.providerSessionId)).toEqual(["source-native", "target-native"]);
    expect(recovered.blocks.filter((block) => block.id === "acknowledged-request")).toHaveLength(1);
    expect(recovered.blocks.find((block) => block.id === "acknowledged-request")?.draft).not.toBe(true);
    expect(s.provider.forget).not.toHaveBeenCalled();
    expect(s.turns).toHaveLength(2);
    log.mockRestore();
  });

  it("requires explicit inspection after restarting a submitted target request", async () => {
    const s = setup();
    s.engine.command({ type: "send", commandId: "source", sessionId: s.id, text: "Original" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(1));
    await finishTurn(s, 0, "source-native", "Source answer");
    s.provider.contextTransferCapabilities = { nativeMessages: true, resumedAppend: true };
    s.engine.command(switchCommand(s, "claude", "choose-target"));
    s.engine.command({ type: "send", commandId: "submitted-request", sessionId: s.id, text: "Inspect before continuing" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(2));
    s.turns[1].input.onEvent({ type: "session.providerBound", providerSessionId: "target-native" });
    await s.turns[1].input.contextTransfer?.onDelivered?.({ mode: "native", providerSessionId: "target-native" });
    const before = s.store.session(s.id);
    s.store.transaction(() => s.store.save({
      ...before, revision: before.revision + 1,
      session: markProviderRequestSubmitted(before.session, "submitted-request"),
    }, { type: "fixture" }));
    const restarted = new HostEngine(s.store, { codex: s.provider, claude: s.provider });
    cleanups.push(() => restarted.close());
    const recovered = s.store.session(s.id);
    expect(recovered.status).toBe("interrupted");
    expect(recovered.session.providerContext?.delivery).toMatchObject({ status: "uncertain", requestSubmitted: true, needsInspection: true });
    expect(recovered.session.providerSessionId).toBe("target-native");
    expect(recovered.session.blocks.find((block) => block.id === "submitted-request")?.draft).not.toBe(true);
    expect(() => restarted.command({ type: "send", commandId: "blocked", sessionId: s.id, text: "Continue" })).toThrow("Inspect");
    expect(() => restarted.command({ type: "compact", commandId: "blocked-compact", sessionId: s.id })).toThrow("Inspect");
    expect(() => restarted.command(switchCommand(s, "codex", "blocked-switch"))).toThrow("Inspect");
    expect(() => restarted.command({ type: "confirmProviderInspection", commandId: "stale-confirmation", sessionId: s.id, expectedRevision: recovered.revision - 1 })).toThrow("Session changed");
    const confirmation = { type: "confirmProviderInspection", commandId: "confirm-inspection", sessionId: s.id, expectedRevision: recovered.revision };
    const receipt = restarted.command(confirmation);
    expect(restarted.command(confirmation)).toEqual(receipt);
    const inspected = s.store.session(s.id).session;
    expect(inspected.providerContext?.delivery).toBeUndefined();
    expect(inspected.pendingSwitch?.from).toBe("codex");
    expect(inspected.providerSessionId).toBe("target-native");
    expect(inspected.providerContext?.bindings.find((binding) => binding.providerSessionId === "target-native")?.deliveredThroughBlockId).toBeUndefined();
    expect(inspected.blocks.find((block) => block.id === "submitted-request")?.draft).not.toBe(true);
    expect(s.turns).toHaveLength(2);
    restarted.command({ type: "send", commandId: "after-inspection", sessionId: s.id, text: "A new request" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(3));
    expect(s.turns[2].input.text).toBe("A new request");
    expect(s.turns[2].input.contextTransfer?.context.items.map((item) => item.text)).toEqual(expect.arrayContaining(["Original", "Source answer", "Inspect before continuing"]));
    expect(s.turns[2].input.contextTransfer?.context.items.filter((item) => item.text === "Inspect before continuing")).toHaveLength(1);
    s.turns[2].input.onEvent({ type: "session.providerBound", providerSessionId: "fresh-target" });
    await s.turns[2].input.contextTransfer?.onDelivered?.({ mode: "native", providerSessionId: "fresh-target" });
    s.turns[2].input.onAccepted?.();
    await finishTurn(s, 2, "fresh-target", "Continued");
  });

  it("stores omitted visible history on the owning host and excludes private reasoning", async () => {
    const s = setup();
    const original = "Oversized complete message ".repeat(2_000);
    s.engine.command({ type: "send", commandId: "source", sessionId: s.id, text: original });
    await vi.waitFor(() => expect(s.turns).toHaveLength(1));
    s.turns[0].input.onEvent({ type: "reasoning.delta", text: "Private reasoning must stay excluded" });
    await finishTurn(s, 0, "source-native", "Source answer");
    s.engine.command(switchCommand(s, "claude", "choose-budget"));
    s.engine.command({ type: "send", commandId: "budget", sessionId: s.id, text: "Continue" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(2));
    const rendered = s.turns[1].input.text;
    expect(rendered).not.toContain(original);
    const manifest = JSON.parse(rendered.split("\n\n")[1]) as { retrievalPath: string };
    expect(manifest.retrievalPath).toContain(join(s.directory, "context-history", s.id));
    const { readFileSync } = await import("node:fs");
    const snapshot = readFileSync(manifest.retrievalPath, "utf8");
    expect(snapshot).toContain(original);
    expect(snapshot).not.toContain("Private reasoning must stay excluded");
    expect(s.store.session(s.id).session.blocks.find((block) => block.id === "source")?.text).toBe(original);
    await finishTurn(s, 1, "target-native", "Done");
  });

  it("keeps picker intent and command receipts after host restart without resending", async () => {
    const s = setup();
    s.engine.command({ type: "send", commandId: "source", sessionId: s.id, text: "Original" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(1));
    await finishTurn(s, 0, "source-native", "Source answer");
    const selection = switchCommand(s, "claude", "durable-selection");
    const receipt = s.engine.command(selection);
    const restarted = new HostEngine(s.store, { codex: s.provider, claude: s.provider });
    cleanups.push(() => restarted.close());
    expect(restarted.command(selection)).toEqual(receipt);
    expect(s.store.session(s.id).session).toMatchObject({ harness: "claude", pendingSwitch: { from: "codex", fromProviderSessionId: "source-native" } });
    expect(s.turns).toHaveLength(1);
    restarted.command({ type: "send", commandId: "after-restart", sessionId: s.id, text: "Continue" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(2));
    expect(s.turns[1].input.text).toContain("Original");
    await finishTurn(s, 1, "target-native", "Continued");
  });

  it("recovers an imported receipt as uncertain and retains a retryable request", async () => {
    const s = setup();
    const before = s.store.session(s.id);
    s.store.transaction(() => s.store.save({
      ...before, revision: before.revision + 1, status: "running", runId: "lost-run",
      session: {
        ...before.session, harness: "claude", model: "claude:test", providerSessionId: "uncertain-native", busy: true,
        pendingSwitch: { from: "codex", fromModel: "codex:test", fromSettings: {}, fromProviderSessionId: "retained-source" },
        blocks: [
          { id: "source", role: "user", text: "Original" },
          { id: "lost-context", role: "handoff", text: "Imported history", handoff: { from: "codex", to: "claude", status: "preparing", pending: true, transfer: { switchId: "lost", status: "imported", mode: "native", included: 1, omitted: 0, historicalAttachments: 0 } } },
          { id: "lost-user", role: "user", text: "Unacknowledged request" },
        ],
        providerContext: {
          version: 1,
          bindings: [{ harness: "codex", cwd: s.directory, providerSessionId: "retained-source", deliveredThroughBlockId: "source" }, { harness: "claude", cwd: s.directory, providerSessionId: "uncertain-native" }],
          delivery: { switchId: "lost", status: "imported", mode: "native", from: "codex", to: "claude", cwd: s.directory, currentUserBlockId: "lost-user", sourceThroughBlockId: "source", includedBlockIds: ["source"], omittedBlockIds: [], targetProviderSessionId: "uncertain-native" },
        },
      },
    }, { type: "fixture" }));
    const restarted = new HostEngine(s.store, { codex: s.provider, claude: s.provider });
    cleanups.push(() => restarted.close());
    const recovered = s.store.session(s.id);
    expect(recovered.status).toBe("interrupted");
    expect(recovered.session.providerSessionId).toBeUndefined();
    expect(recovered.session.providerContext?.delivery?.status).toBe("uncertain");
    expect(recovered.session.blocks.find((block) => block.id === "lost-user")).toMatchObject({ draft: true, text: "Unacknowledged request" });
    expect(recovered.session.blocks.find((block) => block.id === "lost-context")?.handoff?.transfer?.status).toBe("uncertain");
    expect(s.provider.send).not.toHaveBeenCalled();
    expect(s.provider.bind).not.toHaveBeenCalledWith(s.id, "uncertain-native", s.directory);
    restarted.command(switchCommand(s, "codex", "return-source"));
    expect(s.store.session(s.id).session.providerSessionId).toBe("retained-source");
  });

  it("delivers historical attachments through immutable host references", async () => {
    const s = setup();
    const fileId = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    writeAttachmentChunk(s.store, { id: fileId, offset: 0, size: 5, data: Buffer.from("notes").toString("base64") });
    s.engine.command({ type: "send", commandId: "source-file", sessionId: s.id, text: "Read the attached notes", attachments: [{ id: fileId, name: "notes.txt", mimeType: "text/plain", kind: "file", size: 5 }] });
    await vi.waitFor(() => expect(s.turns).toHaveLength(1));
    await finishTurn(s, 0, "source-native", "Read notes");
    const originalPath = s.store.session(s.id).session.blocks[0].attachments?.[0].path;
    s.engine.command(switchCommand(s, "claude", "choose-files"));
    s.engine.command({ type: "send", commandId: "target-files", sessionId: s.id, text: "Continue" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(2));
    const history = JSON.parse(s.turns[1].input.text.split("\n\n")[3]) as Array<{ attachments?: Array<{ path?: string; sha256?: string; delivery: string }> }>;
    const reference = history[0].attachments![0];
    expect(reference.path).toContain(join(s.directory, "context-history", s.id, "assets"));
    expect(reference.sha256).toHaveLength(64);
    expect(reference.delivery).toBe("reference-only");
    expect(s.turns[1].input.attachments).toEqual([]);
    expect(s.store.session(s.id).session.blocks[0].attachments?.[0].path).toBe(originalPath);
    expect(s.store.session(s.id).session.blocks.find((block) => block.id === "target-files-context")?.handoff?.transfer?.historicalAttachments).toBe(1);
    await finishTurn(s, 1, "target-native", "Continued");
  });

  it.each(["image", "file references", "edited draft", "approved plan"] as const)("reserves the actual %s request capacity before accepting a target turn", async (request) => {
    const s = setup();
    s.provider.contextTransferCapabilities = { nativeMessages: true, resumedAppend: true };
    s.engine.command({ type: "send", commandId: "source", sessionId: s.id, text: "Original" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(1));
    s.turns[0].input.onEvent({ type: "session.providerBound", providerSessionId: "source-native" });
    s.turns[0].input.onEvent({ type: "context", used: 36_000, window: 40_000 });
    await finishTurn(s, 0, "source-native", "Source answer");
    s.engine.command(switchCommand(s, "claude", "choose-other"));
    s.engine.command({ type: "send", commandId: "other", sessionId: s.id, text: "Other provider" });
    await vi.waitFor(() => expect(s.turns).toHaveLength(2));
    await s.turns[1].input.contextTransfer?.onDelivered?.({ mode: "native", providerSessionId: "target-native" });
    s.turns[1].input.onAccepted?.();
    await finishTurn(s, 1, "target-native", "Target answer");
    s.engine.command(switchCommand(s, "codex", "return-full"));
    const fileId = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
    let text = "Continue";
    let draftBlockId: string | undefined;
    let planBlockId: string | undefined;
    let attachments: Array<{ id: string; name: string; mimeType: string; kind: "image" | "file"; size: number }> = [];
    if (request === "image") {
      writeAttachmentChunk(s.store, { id: fileId, offset: 0, size: 5, data: Buffer.from("image").toString("base64") });
      attachments = [{ id: fileId, name: "image.png", mimeType: "image/png", kind: "image", size: 5 }];
    } else if (request === "file references") {
      attachments = Array.from({ length: 20 }, (_, index) => {
        const id = `${index.toString(16).padStart(8, "0")}-dddd-4ddd-8ddd-dddddddddddd`;
        writeAttachmentChunk(s.store, { id, offset: 0, size: 0, data: "" });
        return { id, name: `${"資料".repeat(80)}-${index}.txt`, mimeType: "text/plain", kind: "file" as const, size: 0 };
      });
    } else if (request === "edited draft") {
      s.engine.command({ type: "draft", commandId: "short-draft", sessionId: s.id, text: "Short draft" });
      draftBlockId = "short-draft";
      text = "Edited request ".repeat(400);
    } else {
      const before = s.store.session(s.id);
      planBlockId = "reviewed-plan";
      const plan = "Approved step ".repeat(400);
      s.store.transaction(() => s.store.save({ ...before, revision: before.revision + 1, session: { ...before.session, blocks: [...before.session.blocks, { id: planBlockId!, role: "plan", text: plan, plan: { status: "ready" } }] } }, { type: "fixture" }));
      text = `Build the approved plan:\n\n${plan}`;
    }
    const stops = vi.mocked(s.provider.stop).mock.calls.length;
    expect(() => s.engine.command({ type: "send", commandId: "capacity", sessionId: s.id, text, attachments, ...(draftBlockId ? { draftBlockId } : {}), ...(planBlockId ? { planBlockId, intent: "build" as const } : {}) }))
      .toThrow("enough remaining context");
    expect(s.store.session(s.id).status).toBe("idle");
    expect(s.store.session(s.id).session.blocks.some((block) => block.id === "capacity")).toBe(false);
    expect(s.turns).toHaveLength(2);
    expect(vi.mocked(s.provider.stop).mock.calls).toHaveLength(stops);
  });
});

describe("headless session ownership", () => {
  it.each(["send", "compact"] as const)("clears the old draft when a normal %s starts", async (type) => {
    const { engine, store, turns, provider, id } = setup();
    provider.compact = (input) => provider.send({ ...input, text: "/compact" });
    engine.command({ type: "draft", commandId: "draft", sessionId: id, text: "Later" });
    engine.command({ type, commandId: "next", sessionId: id, text: "New work" });
    expect(store.session(id).session.blocks.some((block) => block.draft)).toBe(false);
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    turns[0].finish();
  });

  it("contains a persistence failure while requesting approval", async () => {
    const { engine, store, turns, provider, id } = setup();
    engine.command({ type: "send", commandId: "approval-failure", sessionId: id, text: "Work" });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(store, "save").mockImplementationOnce(() => { throw new Error("disk full"); });
    try {
      expect(() => turns[0].input.onEvent({ type: "approval.requested", requestId: 1, title: "Run?" })).not.toThrow();
      await vi.waitFor(() => expect(provider.stop).toHaveBeenCalled());
      await vi.waitFor(() => expect(store.session(id).status).toBe("interrupted"));
    } finally { log.mockRestore(); }
  });

  it("stores a remote draft with an uploaded file, then sends it in plan mode", async () => {
    const { engine, store, turns, id } = setup();
    const fileId = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    expect(
      writeAttachmentChunk(store, {
        id: fileId,
        offset: 0,
        size: 5,
        data: Buffer.from("hello").toString("base64"),
      }),
    ).toEqual({ offset: 5 });
    const attachment = {
      id: fileId,
      name: "notes.txt",
      mimeType: "text/plain",
      kind: "file" as const,
      size: 5,
    };
    engine.command({
      type: "draft",
      commandId: "draft-1",
      sessionId: id,
      text: "Plan this",
      attachments: [attachment],
    });
    expect(store.session(id).session.blocks[0]).toMatchObject({
      draft: true,
      attachments: [{ name: "notes.txt" }],
    });
    expect(store.summaries(store.session(id).projectId)[0].draft).toBe(true);
    engine.command({
      type: "send",
      commandId: "send-draft",
      sessionId: id,
      text: "Plan this",
      intent: "plan",
      draftBlockId: "draft-1",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    expect(turns[0].input).toMatchObject({
      intent: "plan",
      attachments: [{ name: "notes.txt", size: 5 }],
    });
    expect(turns[0].input.attachments?.[0].path).toContain(fileId);
    expect(store.summaries(store.session(id).projectId)[0].draft).toBe(false);
    turns[0].finish();
  });

  it("passes an uploaded image to the host provider on an attachment-only turn", async () => {
    const { engine, store, turns, id } = setup("claude");
    const fileId = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
    const image = Buffer.from("image-bytes");
    writeAttachmentChunk(store, {
      id: fileId,
      offset: 0,
      size: image.length,
      data: image.toString("base64"),
    });
    engine.command({
      type: "send",
      commandId: "image-turn",
      sessionId: id,
      text: "",
      attachments: [
        {
          id: fileId,
          name: "shot.png",
          mimeType: "image/png",
          kind: "image",
          size: image.length,
        },
      ],
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    expect(turns[0].input.attachments?.[0]).toMatchObject({
      name: "shot.png",
      data: image.toString("base64"),
    });
    expect(readAttachmentChunk(store, { sessionId: id, id: fileId, offset: 0 })).toEqual({
      offset: image.length, size: image.length, data: image.toString("base64"),
    });
    const other = engine.command({ type: "create", commandId: "other-session", projectId: store.session(id).projectId,
      harness: "claude", model: "claude:test", runtimeMode: "supervised" });
    expect(() => readAttachmentChunk(store, { sessionId: other.sessionId, id: fileId, offset: 0 })).toThrow();
    turns[0].finish();
  });

  it("removes a remote draft without starting the provider", () => {
    const { engine, store, provider, id } = setup();
    engine.command({
      type: "draft",
      commandId: "draft-2",
      sessionId: id,
      text: "Later",
    });
    engine.command({
      type: "removeDraft",
      commandId: "remove-2",
      sessionId: id,
      draftBlockId: "draft-2",
    });
    expect(store.session(id).session.blocks).toEqual([]);
    expect(provider.send).not.toHaveBeenCalled();
  });

  it("marks a reviewed host plan as built after its build turn", async () => {
    const { engine, store, turns, id } = setup();
    engine.command({
      type: "send",
      commandId: "plan-turn",
      sessionId: id,
      text: "Plan this",
      intent: "plan",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    turns[0].input.onEvent({ type: "plan", text: "# Steps\n\n1. Change code" });
    turns[0].finish();
    await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));
    const plan = store
      .session(id)
      .session.blocks.find((block) => block.role === "plan")!;
    engine.command({
      type: "send",
      commandId: "build-turn",
      sessionId: id,
      text: `Build the approved plan:\n\n${plan.text}`,
      intent: "build",
      planBlockId: plan.id,
    });
    expect(
      store.session(id).session.blocks.find((block) => block.id === plan.id)
        ?.plan?.status,
    ).toBe("building");
    await vi.waitFor(() => expect(turns).toHaveLength(2));
    turns[1].finish();
    await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));
    expect(
      store.session(id).session.blocks.find((block) => block.id === plan.id)
        ?.plan?.status,
    ).toBe("built");
  });

  it("keeps a manually renamed title when first-turn generation finishes later", async () => {
    const { engine, store, provider, turns, id } = setup();
    let finishTitle: (title: {
      title: string;
      workItem: null;
    }) => void = () => {};
    provider.generateTitle = vi.fn(
      () =>
        new Promise((resolve) => {
          finishTitle = resolve;
        }),
    );
    engine.command({
      type: "send",
      commandId: "name-first-turn",
      sessionId: id,
      text: "Fix remote project titles",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    engine.updateSession(id, { title: "codex · My own title" });
    finishTitle({ title: "Generated title", workItem: null });
    await vi.waitFor(() =>
      expect(provider.generateTitle).toHaveBeenCalledTimes(1),
    );
    turns[0].finish();
    await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));
    expect(store.session(id).session.title).toBe("codex · My own title");
  });

  it("uses one creation timestamp and advances only updatedAt on later commands", () => {
    let now = 1_700_000_000_000;
    const clock = vi.spyOn(Date, "now").mockImplementation(() => now++);
    try {
      const { engine, store, project, id } = setup();
      const initial = store.session(id);
      const timestamps = {
        createdAt: initial.createdAt,
        updatedAt: initial.createdAt,
        revision: 1,
      };
      expect(initial).toMatchObject(timestamps);
      expect(store.sessions(project.id)[0]).toMatchObject(timestamps);
      expect(store.summaries(project.id)[0]).toMatchObject(timestamps);

      now = initial.updatedAt + 1_000;
      engine.command({
        type: "configure",
        commandId: "configure-timestamps",
        sessionId: id,
        model: "codex:updated",
        modelSettings: {},
        runtimeMode: "supervised",
      });
      const updatedTimestamps = {
        createdAt: initial.createdAt,
        updatedAt: initial.updatedAt + 1_000,
        revision: 2,
      };
      expect(store.session(id)).toMatchObject(updatedTimestamps);
      expect(store.sessions(project.id)[0]).toMatchObject(updatedTimestamps);
      expect(store.summaries(project.id)[0]).toMatchObject(updatedTimestamps);
    } finally {
      clock.mockRestore();
    }
  });

  it("keeps remote card changes in host history and removes deleted sessions", () => {
    const { store, project, id } = setup();
    const initial = store.summaries(project.id)[0];
    expect(initial.model).toBe("codex:test");
    expect(initial.createdAt).toBe(initial.updatedAt);

    store.save(
      {
        ...store.session(id),
        revision: initial.revision + 1,
        updatedAt: initial.updatedAt + 1_000,
      },
      { type: "session.test" },
    );
    expect(store.summaries(project.id)[0]).toMatchObject({
      createdAt: initial.createdAt,
      updatedAt: initial.updatedAt + 1_000,
    });

    const updated = store.updateSession(id, {
      title: "Codex · Renamed",
      pinned: true,
      archived: true,
      linkedWorkItem: {
        kind: "issue",
        repo: "example/repo",
        number: 42,
        url: "https://github.com/example/repo/issues/42",
      },
    });
    expect(updated).toMatchObject({
      title: "Codex · Renamed",
      pinned: true,
      archived: true,
      model: "codex:test",
      linkedWorkItem: { number: 42 },
    });
    expect(store.summaries(project.id)[0]).toMatchObject({
      title: updated.title,
      pinned: true,
      archived: true,
      revision: updated.revision,
    });
    expect(store.sync(id, initial.revision)).toMatchObject({ kind: "delta" });
    store.updateSession(id, { linkedWorkItem: null });
    expect(store.summaries(project.id)[0].linkedWorkItem).toBeUndefined();

    store.deleteSession(id);
    expect(store.summaries(project.id)).toEqual([]);
    expect(() => store.session(id)).toThrow("Session not found");
  });

  it("includes the harness ID in remote summaries, including older cached rows", () => {
    const { store, project, id } = setup();
    const current = store.session(id);
    store.save(
      {
        ...current,
        revision: current.revision + 1,
        session: { ...current.session, providerSessionId: "harness-session" },
      },
      { type: "session.test" },
    );
    expect(store.summaries(project.id)[0].providerSessionId).toBe(
      "harness-session",
    );

    const legacySummary = { ...store.summaries(project.id)[0] };
    delete legacySummary.providerSessionId;
    store.db.prepare("UPDATE sessions SET summary=? WHERE id=?").run(
      JSON.stringify(legacySummary),
      id,
    );
    expect(store.summaries(project.id)[0].providerSessionId).toBe(
      "harness-session",
    );
    const repaired = store.db
      .prepare("SELECT summary FROM sessions WHERE id=?")
      .get(id)!;
    expect(JSON.parse(String(repaired.summary)).providerSessionId).toBe(
      "harness-session",
    );
  });

  it("keeps a legacy session's last known timestamp when adding creation time", () => {
    const { directory, store, project, id } = setup();
    const legacy = { ...store.session(id) };
    delete legacy.createdAt;
    store.db.prepare("UPDATE sessions SET snapshot=? WHERE id=?").run(
      JSON.stringify(legacy),
      id,
    );

    const reopened = new HostStore(join(directory, "host.db"));
    cleanups.push(() => reopened.close());
    const original = reopened.session(id);
    reopened.save(
      {
        ...original,
        revision: original.revision + 1,
        updatedAt: original.updatedAt + 1_000,
      },
      { type: "session.test" },
    );
    expect(reopened.summaries(project.id)[0]).toMatchObject({
      createdAt: original.updatedAt,
      updatedAt: original.updatedAt + 1_000,
    });
  });

  it("preserves the transaction error and invalidates cached state if rollback fails", () => {
    const { store, id } = setup();
    const cached = store.session(id);
    store.db.prepare("UPDATE sessions SET snapshot=? WHERE id=?").run(
      JSON.stringify({
        ...cached,
        session: { ...cached.session, title: "Updated" },
      }),
      id,
    );
    const exec = store.db.exec.bind(store.db);
    const rollback = vi.spyOn(store.db, "exec").mockImplementation((sql) => {
      if (sql === "ROLLBACK") throw new Error("rollback failed");
      return exec(sql);
    });
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    const original = new Error("transaction failed");
    try {
      expect(() =>
        store.transaction(() => {
          throw original;
        }),
      ).toThrow(original);
      expect(store.session(id).session.title).toBe("Updated");
    } finally {
      rollback.mockRestore();
      log.mockRestore();
      store.db.exec("ROLLBACK");
    }
  });
  it("keeps the checkout idle while a branch switch is in progress", async () => {
    const { engine, project, id, turns } = setup();
    let finishSwitch = () => {};
    const switching = engine.withIdleProject(
      project.id,
      () =>
        new Promise<void>((resolve) => {
          finishSwitch = resolve;
        }),
    );
    expect(() =>
      engine.command({
        type: "send",
        commandId: "during-switch",
        sessionId: id,
        text: "Work",
      }),
    ).toThrow("branch switch");
    expect(() =>
      engine.command({
        type: "create",
        commandId: "new-during-switch",
        projectId: project.id,
        harness: "codex",
        model: "codex:test",
        runtimeMode: "supervised",
      }),
    ).toThrow("branch switch");
    finishSwitch();
    await switching;
    engine.command({
      type: "send",
      commandId: "after-switch",
      sessionId: id,
      text: "Work",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    turns[0].finish();
  });

  it("runs provider context compaction once and persists its transcript marker", async () => {
    const { engine, store, provider, id } = setup();
    let finishCompact = () => {};
    provider.compact = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finishCompact = resolve;
        }),
    );
    const command = { type: "compact", commandId: "compact", sessionId: id };
    const receipt = engine.command(command);
    expect(engine.command(command)).toEqual(receipt);
    await vi.waitFor(() => expect(provider.compact).toHaveBeenCalledTimes(1));
    expect(store.session(id).session.blocks).toContainEqual(
      expect.objectContaining({ text: "/compact" }),
    );
    finishCompact();
    await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));
  });

  it("persists model and permission changes for the next turn and rejects changes mid-turn", async () => {
    const { engine, store, turns, id } = setup();
    const change = {
      type: "configure",
      commandId: "settings",
      sessionId: id,
      model: "codex:new",
      modelSettings: { reasoningEffort: "high" },
      runtimeMode: "full-access",
    };
    const receipt = engine.command(change);
    expect(engine.command(change)).toEqual(receipt);
    expect(store.session(id).session).toMatchObject({
      model: "codex:new",
      modelSettings: { reasoningEffort: "high" },
      runtimeMode: "full-access",
    });
    engine.command({
      type: "send",
      commandId: "turn",
      sessionId: id,
      text: "Continue",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    expect(turns[0].input).toMatchObject({
      model: "codex:new",
      modelSettings: { reasoningEffort: "high" },
      runtimeMode: "full-access",
    });
    expect(() => engine.command({ ...change, commandId: "later" })).toThrow(
      "current turn",
    );
    turns[0].finish();
  });
  it("keeps working with no client, persists output, and deduplicates a lost acknowledgement", async () => {
    const { engine, store, turns, provider, id } = setup();
    const command = {
      type: "send",
      commandId: "send-once",
      sessionId: id,
      text: "Do the work",
    };
    const receipt = engine.command(command);
    expect(engine.command(command)).toEqual(receipt);
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    expect(provider.send).toHaveBeenCalledTimes(1);
    const before = store.session(id).revision;
    turns[0].input.onEvent({
      type: "session.providerBound",
      providerSessionId: "provider-thread",
    });
    turns[0].input.onEvent({
      type: "message.delta",
      text: "still working while disconnected",
    });
    turns[0].finish();
    await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));
    expect(
      store
        .session(id)
        .session.blocks.some((block) => block.text.includes("still working")),
    ).toBe(true);
    expect(store.events(id, before).events?.length).toBeGreaterThan(1);
    expect(engine.command(command)).toEqual(receipt);
    expect(provider.send).toHaveBeenCalledTimes(1);
    expect(provider.bind).toHaveBeenCalledWith(
      id,
      "provider-thread",
      expect.any(String),
    );
    expect(() =>
      engine.command({ ...command, text: "Changed payload" }),
    ).toThrow("different payload");
  });

  it.each(["codex", "claude"] as const)(
    "keeps %s turn timing and model provenance after settlement and reconnect",
    async (harness) => {
      const { engine, store, turns, id } = setup(harness);
      engine.command({
        type: "send",
        commandId: "first-turn",
        sessionId: id,
        text: "Inspect the project",
      });
      await vi.waitFor(() => expect(turns).toHaveLength(1));
      const running = store.session(id);
      expect(running.session.blocks[0]).toMatchObject({
        id: "first-turn",
        startedAt: expect.any(Number),
        turnModel: { harness, id: `${harness}:test` },
      });
      turns[0].input.onEvent({ type: "message.delta", text: "Found it" });
      turns[0].finish();
      await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));

      const reconnected = store.sync(id);
      expect(reconnected.kind).toBe("snapshot");
      if (reconnected.kind !== "snapshot") return;
      expect(reconnected.value.session.blocks[0]).toMatchObject({
        id: "first-turn",
        startedAt: expect.any(Number),
        durationMs: expect.any(Number),
        turnModel: { harness, id: `${harness}:test` },
      });
      expect(
        reconnected.value.session.blocks[0].durationMs,
      ).toBeGreaterThanOrEqual(0);
      expect(reconnected.value.session.blocks[1].text).toBe("Found it");

      const delta = store.sync(id, running.revision);
      expect(delta.kind).toBe("delta");
      if (delta.kind === "delta")
        expect(
          delta.blocks.some(
            (block) => block.id === "first-turn" && block.durationMs != null,
          ),
        ).toBe(true);
    },
  );

  it("serializes concurrent sends and accepts only one approval decision for a run", async () => {
    const { engine, store, turns, provider, id } = setup();
    engine.command({
      type: "send",
      commandId: "send",
      sessionId: id,
      text: "Work",
    });
    expect(() =>
      engine.command({
        type: "send",
        commandId: "other-send",
        sessionId: id,
        text: "More work",
      }),
    ).toThrow("already running");
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    turns[0].input.onEvent({
      type: "approval.requested",
      requestId: 7,
      title: "Run a command?",
    });
    const runId = store.session(id).runId!;
    const approval = {
      type: "approve",
      commandId: "approval-1",
      sessionId: id,
      runId,
      requestId: 7,
      decision: "allow",
    };
    expect(() => engine.command({ ...approval, runId: "stale" })).toThrow(
      "finished or replaced",
    );
    engine.command(approval);
    engine.command(approval);
    expect(() =>
      engine.command({
        ...approval,
        commandId: "approval-2",
        decision: "deny",
      }),
    ).toThrow("already resolved");
    expect(provider.approve).toHaveBeenCalledTimes(1);
    expect(provider.approve).toHaveBeenCalledWith(id, 7, "allow");
  });

  it("stores pending questions and rejects a second device's stale answer", async () => {
    const { engine, store, turns, provider, id } = setup();
    engine.command({
      type: "send",
      commandId: "send",
      sessionId: id,
      text: "Work",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    turns[0].input.onEvent({
      type: "question.asked",
      requestId: 3,
      questions: [
        {
          id: "q1",
          prompt: "Choose",
          multiSelect: false,
          allowCustom: false,
          options: [{ id: "yes", label: "Yes" }],
        },
      ],
    });
    const reply = {
      type: "answer",
      commandId: "answer",
      sessionId: id,
      runId: store.session(id).runId,
      requestId: 3,
      reply: { kind: "answered", answers: { q1: ["yes"] } },
    };
    engine.command(reply);
    expect(store.session(id).session.pendingQuestion).toBeUndefined();
    expect(() =>
      engine.command({ ...reply, commandId: "other-answer" }),
    ).toThrow("already resolved");
    expect(provider.answer).toHaveBeenCalledTimes(1);
  });

  it("recovers interrupted durable state without replaying an uncertain provider send", async () => {
    const { store, provider, id } = setup();
    const value = store.session(id);
    store.transaction(() =>
      store.save(
        {
          ...value,
          revision: value.revision + 1,
          status: "running",
          runId: "old-run",
          session: {
            ...value.session,
            busy: true,
            providerSessionId: "retained",
            blocks: [
              {
                id: "interrupted-turn",
                role: "user",
                text: "Work",
                startedAt: value.updatedAt - 2_000,
              },
            ],
          },
        },
        { type: "accepted" },
      ),
    );
    const recovered = new HostEngine(store, { codex: provider });
    expect(store.session(id).status).toBe("interrupted");
    expect(store.session(id).session.busy).toBe(false);
    expect(store.session(id).session.blocks[0].durationMs).toBe(2_000);
    expect(provider.send).not.toHaveBeenCalled();
    expect(provider.bind).toHaveBeenCalledWith(
      id,
      "retained",
      value.session.cwd,
    );
    await recovered.close();
  });

  it("retries a failed event write and settles the stopped turn", async () => {
    const { engine, store, turns, id } = setup();
    engine.command({
      type: "send",
      commandId: "send",
      sessionId: id,
      text: "Work",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    const original = store.save.bind(store);
    let failed = false;
    vi.spyOn(store, "save").mockImplementation((value, event) => {
      if (!failed && (event as { type?: string }).type === "events") {
        failed = true;
        throw new Error("temporary storage error");
      }
      return original(value, event);
    });
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      turns[0].input.onEvent({
        type: "message.delta",
        text: "Retained output",
      });
      turns[0].finish();
      await vi.waitFor(
        () => expect(store.session(id).status).toBe("interrupted"),
        {
          timeout: 4_000,
        },
      );
      expect(
        store
          .session(id)
          .session.blocks.some((block) => block.text === "Retained output"),
      ).toBe(true);
      expect(store.session(id).session.busy).toBe(false);
    } finally {
      log.mockRestore();
    }
  });

  it("retries a failed final settlement", async () => {
    const { engine, store, turns, id } = setup();
    engine.command({
      type: "send",
      commandId: "send",
      sessionId: id,
      text: "Work",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    const original = store.save.bind(store);
    let failed = false;
    vi.spyOn(store, "save").mockImplementation((value, event) => {
      if (!failed && (event as { type?: string }).type === "settled") {
        failed = true;
        throw new Error("temporary storage error");
      }
      return original(value, event);
    });
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      turns[0].finish();
      await vi.waitFor(
        () => expect(store.session(id).status).toBe("interrupted"),
        {
          timeout: 4_000,
        },
      );
      expect(store.session(id).session.busy).toBe(false);
    } finally {
      log.mockRestore();
    }
  });

  it("batches streamed output and syncs only changed blocks", async () => {
    const { engine, store, turns, id, project } = setup();
    engine.command({
      type: "send",
      commandId: "send",
      sessionId: id,
      text: "Work",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    const started = store.session(id).revision;
    for (let index = 0; index < 50; index++)
      turns[0].input.onEvent({
        type: "message.delta",
        text: `chunk ${index} `,
      });
    expect(store.session(id).revision).toBe(started);
    await vi.waitFor(() =>
      expect(store.session(id).revision).toBe(started + 1),
    );
    const sync = store.sync(id, started);
    expect(sync.kind).toBe("delta");
    if (sync.kind !== "delta") return;
    expect(sync.blocks.map((block) => block.role)).toEqual(["assistant"]);
    expect(sync.blockIds).toHaveLength(2);

    turns[0].input.onEvent({
      type: "approval.requested",
      requestId: 1,
      title: "Run a command?",
    });
    expect(store.session(id).revision).toBe(started + 2);

    const streamed = store.session(id).revision;
    turns[0].finish();
    await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));
    const settled = store.sync(id, streamed);
    if (settled.kind !== "delta") throw new Error("Expected a delta");
    expect(
      settled.blocks.some(
        (block) => block.role === "user" && block.durationMs != null,
      ),
    ).toBe(true);
    expect(store.sync(id, store.session(id).revision).kind).toBe("unchanged");
    expect(store.sync(id).kind).toBe("snapshot");
    expect(store.summaries(project.id)[0]).toMatchObject({
      id,
      status: "idle",
      title: "codex · Work",
    });
  });

  it("requires snapshot recovery when the client's event cursor is invalid", () => {
    const { store, id } = setup();
    expect(store.events(id, 100_000).snapshot?.session.id).toBe(id);
  });

  it("validates untrusted commands before execution", () => {
    for (const expectedRevision of [-1, 1.5, "1", undefined]) {
      expect(() => parseCommand({
        type: "switchProvider", commandId: "switch", sessionId: "session",
        expectedRevision, harness: "claude", model: "claude:test",
        modelSettings: {}, runtimeMode: "supervised",
      })).toThrow("Invalid expected session revision");
      expect(() => parseCommand({ type: "confirmProviderInspection", commandId: "inspect", sessionId: "session", expectedRevision }))
        .toThrow("Invalid expected session revision");
    }
    expect(() =>
      parseCommand({ type: "send", commandId: "x", sessionId: "y", text: "" }),
    ).toThrow();
    expect(() =>
      parseCommand({
        type: "create",
        commandId: "x",
        projectId: "y",
        harness: "shell",
        model: "x",
        runtimeMode: "auto",
      }),
    ).toThrow();
    expect(() =>
      parseCommand({
        type: "answer",
        commandId: "x",
        sessionId: "y",
        runId: "z",
        requestId: 1,
        reply: { kind: "answered", answers: { a: [42] } },
      }),
    ).toThrow();
  });
});
