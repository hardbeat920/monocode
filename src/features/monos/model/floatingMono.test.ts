import { describe, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import {
  deliverFloatingMonoRequest,
  floatingMonoSession,
  type FloatingMonoHost,
} from "./floatingMono";

const session = newSession("codex", "/tmp", "default", "auto");
function host(): FloatingMonoHost {
  return {
    open: vi.fn().mockResolvedValue(session),
    submit: vi.fn().mockReturnValue(true),
    stop: vi.fn(),
    approval: vi.fn(),
    question: vi.fn(),
    questionInteraction: vi.fn(),
    reveal: vi.fn().mockResolvedValue(undefined),
    openFile: vi.fn(),
    resume: vi.fn(),
  };
}

describe("floating Mono delivery", () => {
  it("uses the existing session for chat and approvals", async () => {
    const runtime = host();
    const accept = vi.fn().mockResolvedValue(true);
    await deliverFloatingMonoRequest(
      {
        id: 1,
        monoId: "mono",
        action: { kind: "submit", text: "hello", attachments: [] },
      },
      runtime,
      accept,
    );
    expect(runtime.open).toHaveBeenCalledWith("mono");
    expect(runtime.submit).toHaveBeenCalledExactlyOnceWith(
      session.id,
      "hello",
      [],
    );
    await deliverFloatingMonoRequest(
      {
        id: 2,
        monoId: "mono",
        action: { kind: "approval", requestId: 7, decision: "deny" },
      },
      runtime,
      accept,
    );
    expect(runtime.approval).toHaveBeenCalledWith(session.id, 7, "deny");
  });

  it("does not deliver a request that expired while its Mono was loading", async () => {
    const runtime = host();
    await deliverFloatingMonoRequest(
      {
        id: 1,
        monoId: "mono",
        action: { kind: "submit", text: "hello", attachments: [] },
      },
      runtime,
      async () => false,
    );
    expect(runtime.submit).not.toHaveBeenCalled();
  });

  it("reports rejection so the floating composer can retain the draft", async () => {
    const runtime = host();
    vi.mocked(runtime.submit).mockReturnValue(false);
    await expect(
      deliverFloatingMonoRequest(
        {
          id: 1,
          monoId: "mono",
          action: { kind: "submit", text: "hello", attachments: [] },
        },
        runtime,
        async () => true,
      ),
    ).rejects.toThrow("could not be sent");
  });

  it("transfers attachment bytes instead of another webview's blob URL", () => {
    const file = {
      id: "image",
      name: "pasted.png",
      kind: "image" as const,
      mimeType: "image/png",
      size: 3,
      data: "YWJj",
      previewUrl: "blob:main-only",
    };
    const original = {
      ...session,
      blocks: [
        {
          id: "message",
          role: "user" as const,
          text: "look",
          attachments: [file],
        },
      ],
    };
    const transferred = floatingMonoSession(original);
    expect(transferred.blocks[0].attachments?.[0]).toMatchObject({
      data: "YWJj",
      name: "pasted.png",
    });
    expect(transferred.blocks[0].attachments?.[0].previewUrl).toBeUndefined();
    expect(original.blocks[0].attachments[0].previewUrl).toBe("blob:main-only");
  });
});
